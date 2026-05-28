# 13 — 文件系统与 VM 深度设计

> 对应 `target.md` §3.5 + §3.7 + §4.6
>
> 姊妹篇：[07-data-and-filesystem.md](../../core/07-data-and-filesystem.md)（顶层设计）, [03-pager-and-memory.md](../../core/03-pager-and-memory.md)（Pager 细节）

## 讨论范围

本文是 [07-data-and-filesystem.md](../../core/07-data-and-filesystem.md) 与 [03-pager-and-memory.md](../../core/03-pager-and-memory.md) 的深挖材料，展开目录树、索引和 FS/VM 边界的论证。三部分：现代 FS 实践调研 → 目录树问题分析与 Ousia OS 方案 → VM 整合。顶层契约由 07/03 的主设计章节定义。

---

# 第一部分：现代文件系统实践调研

## 1.1 各文件系统的核心选择与启示

| FS           | 核心选择                                                                                      | 对 Ousia OS 的关键启示                                                         |
| ------------ | --------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| **bcachefs** | CoW + 压缩 + 加密 + 分层存储全整合。每个 extent 自描述（checksum + compression + encryption） | 一套代码库整合这些是工程可行的。Ousia OS 的 Object 也应该自描述                |
| **btrfs**    | 最成熟的 CoW FS，但代码 100K+ 行，RAID write hole 长期未解决                                  | 代码复杂度本身就是安全隐患。用户态 FS 的优势：bug 修复不需要内核更新           |
| **fxfs**     | **对象存储 + 文件系统分离**。u64 key 标识对象，目录树是 key→path 映射。journal-based 事务     | **最重要参考**。和 Ousia OS 方向一致。但 fxfs 仍保留 POSIX 层，Ousia OS 更激进 |
| **ZFS**      | 池化存储、ARC 缓存、send/recv、块级去重（代价太高）                                           | 池化 + ARC 值得吸收。去重应在对象级而非块级                                    |
| **APFS**     | space sharing、flash-first（不做碎片整理）、clone（CoW 副本）                                 | flash-first 设计正确。HDD 只作为冷数据备选                                     |

## 1.2 七个共同趋势

1. **CoW 是主流** — 快照、崩溃安全、reflink。Ousia OS Object Store 必须 CoW
2. **元数据和数据自描述** — b-tree 元数据，extent 携带 checksum。不需要 fsck
3. **压缩加密是存储层能力** — per-object 或 per-extent 透明压缩/加密
4. **快照版本内建** — 不是用户手动 `cp -r backup`。object 级版本管理
5. **路径和身份应解耦** — fxfs 最明确。tree view 是一等命名入口，但路径不应是唯一身份
6. **Flash-first** — 关注写放大而非碎片整理。CoW 天然 flash-friendly
7. **存储池化** — 多盘合并为一个 pool。用户不关心数据在哪块盘上

## 1.3 可借鉴的前沿工作

| 技术        | 来源      | 启示                                                   |
| ----------- | --------- | ------------------------------------------------------ |
| **SplitFS** | SOSP 2019 | 用户态 FS + 内核页缓存协同。Pager + 存储服务分工的参考 |
| **ZoFS**    | OSDI 2020 | 用户态 FS 可做到和内核态同等的崩溃一致性               |
| **Assise**  | OSDI 2021 | PM 字节寻址。Ousia OS 预留 DAX 模式参考                |
| **XRP**     | OSDI 2022 | eBPF 存储加速。策略注入思路参考                        |

---

# 第二部分：目录树——什么该留，什么该改

## 2.1 目录树做对了什么（不可丢弃）

1. **层级命名 = 人类心智模型** — 任何替换方案必须提供至少同样直观的导航
2. **作用域是天然操作边界** — `rm -r /scope/*` 清晰。纯 tag 系统无法简洁表达
3. **相对路径简洁** — `../lib/utils.js` 远比 OID 可读
4. **权限继承自然** — "这个目录下的所有文件属于 alice" 是自然的权限边界
5. **工具链兼容** — 地球上几乎所有软件理解路径

## 2.2 目录树的根本局限

1. **单层级 = 单视角（最核心）** — 文件只能在一个位置。照片管理软件绕过了文件系统自建数据库，这正是"文件系统抽象不足"的证据
2. **路径 = 身份** — rename 破坏引用。符号链接/硬链接都是补丁，不是解决
3. **没有内容身份** — 两个内容相同的文件占两份存储，系统不知情
4. **发现 = 遍历** — `find / -name "*.pdf"` 是 O(n)，没有索引。Spotlight/locate 只能在 FS 之外建索引
5. **原子性边界太小** — `rename()` 只能单文件原子。无法原子更新多个文件
6. **权限绑定路径，不绑定数据** — `cp secret.txt /tmp/` 权限变了。数据可以被复制到不受保护的路径
7. **层级是人为的且经常是错的** — "这个文件应该放哪个文件夹？" Gmail/Spotify 不需要用户做这个决定

## 2.3 四种解决方案

| 方案                                 | 代表                 | 为何不选                                                         |
| ------------------------------------ | -------------------- | ---------------------------------------------------------------- |
| A: 纯标签                            | Gmail 标签, TMSU     | 无层级=无作用域=无权限继承=工具链全废                            |
| B: **稳定对象身份 + 一等目录树视图** | Nix, Git, IPFS, fxfs | ✅ **Ousia OS 选这个**                                           |
| C: reflink 打补丁                    | bcachefs/btrfs       | 只解决存储层去重，不解决命名模型                                 |
| D: 数据库文件系统                    | WinFS, BeFS          | WinFS 过度设计已失败。数据库开销 vs 路径遍历的简单性矛盾不可调和 |

## 2.4 Ousia OS 方案 B 的具体设计

### 核心模型

```
命名索引层:
  路径索引: /photos/2024/IMG_001 → OID       [显式]
  标签索引: tag://vacation → [OID, ...]       [显式]
  时间索引: date://2024-07 → [OID, ...]      [自动]
  类型索引: type://image/png → [OID, ...]    [自动]
  大小索引: size:>10MB → [OID, ...]          [自动]

Object Store:
  OID → { data, metadata, versions, extents, checksums, compression, encryption }
```

**路径 = tree view 中的命名引用，OID = 稳定对象身份。** 类比 Git：路径接近 ref，OID 接近 blob sha。移动文件 = 更新 NameBinding。删除文件 = 删除 tree view 引用（数据在 object store 待到 GC）。

### 设计要点

**① 对象身份是 OID，tree view 是一等命名入口。** 普通文件应能出现在某个 tree view 中；路径变化不改变对象身份，OID 也不取代人类可导航的路径层级。

**② 索引分自动和显式两类。** 类型（MIME）、时间、大小、内容哈希自动维护——任何对象创建时自动更新索引。路径和标签是用户/应用显式管理。这就是图片管理器不再需要自建库的原因：

```
传统: find /photos -name "*.png" -size +10M  → O(n) 遍历
Ousia OS:  query({type: "image/png", size: (10MB, ∞)})  → 倒排索引交集, O(result)
```

不是 WinFS——只做最通用、最基础的自动索引（类型、时间、大小、哈希），不引入 SQL，不需要 schema。

**③ 查询 API 是 predicate → set intersection**，AND 语义，O(最小结果集)。不需要 SQL 解析器。`list_prefix()` 兼容传统路径遍历。

**④ 标签是 tree view 的平行维。** tree view 有层级（擅长表达归属、挂载和作用域），标签扁平（擅长表达跨维度属性）。两者互补。同一个 OID 同时有路径和标签。

**⑤ 权限以对象能力为准，tree view 可作为批量授权入口。** `alice 有 Capability{oid-1234, READ}` → 不管通过路径还是标签访问，权限一致。目录上的策略用于作用域授权、默认继承和挂载边界，但最终访问仍落到 ObjectHandle / Capability。

**⑥ 版本在 Object 层，目录树指向最新。** `/photos/IMG@v2` 或 `/photos/IMG@{2024-07-15}` 访问历史版本。

**⑦ 消除软/硬链接区分 — 统一的 NameBinding。**

```
NameBinding { source: Name, target: Object(OID) | Name(Path) }

直接引用: /photos/IMG → oid-1234           (类似硬链接)
间接引用: /etc/nginx.conf → /cells/nginx.conf   (类似软链接)
解析器自动跟踪链, visited set 防循环。
```

目录是 tree view 的命名结构，不是对象身份本身。没有"dangling symlink"——解析失败返回 ENOENT，和文件不存在一样。

**⑧ 目录大小计算 — 去重和递归是唯一特殊处理。**

```
du /scope/:
  1. 遍历路径 → 解析(name→name 链跟踪, visited set 防循环) → 收集{OID}
  2. 去重 → sum(每个唯一 OID 的 size)
```

子目录之和可能 ≠ 父目录（跨子目录去重导致），这不是 bug。默认输出去重物理大小，`--no-dedup` 兼容传统。

**⑨ 结构化类型的边界。**

原则：只内置"缺乏 FS 支持就无法高效实现"的类型。

| 类型       | 决定          | 理由                                                                                                                                                                                                                                                      |
| ---------- | ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Blob**   | ✅ 保留       | 默认。支持随机读写/mmap/CoW/版本/hole punch                                                                                                                                                                                                               |
| **Stream** | ✅ 内置       | 只追加语义、不可变历史、时间索引。GC 按时间窗口，不需要 CoW                                                                                                                                                                                               |
| **Array**  | ❌ 不支持     | `mmap` + `struct` 已是 O(1) 随机访问。类型爆炸和 schema 演化代价远大于收益                                                                                                                                                                                |
| **KV**     | ❌ 问错了问题 | **配置文件是错误的抽象**。配置管理归属 [04-environment-and-config.md](../../topics/04-environment-and-config.md) 的类型化配置服务，而不是在 Object Store 里加 KV 类型存配置文件。Object Store 不应因为兼容 `.yaml` / `.toml` / `.env` 而膨胀成通用配置数据库 |

第一阶段：Blob + Stream。两个类型。不用更多。

## 2.5 性能评估：`ls` 和递归搜索会比传统 FS 慢吗？

这个设计的额外抽象层——路径索引是独立的全局 B-tree，而非嵌入目录的 dentry 链——是否会拖慢频繁操作？

### 操作逐项对比

| 操作                                 | 传统 FS（bcachefs）        | Ousia OS                | 结论                                              |
| ------------------------------------ | -------------------------- | ----------------------- | ------------------------------------------------- |
| `ls /dir/`（仅名字）                 | 读目录 B-tree              | 路径索引前缀扫描        | 同为 B-tree 范围扫描。**持平**                    |
| `ls -l /dir/`                        | dirent 内联 inode 关键字段 | path→OID 后查 metadata  | 多一次查找。可内联 size/type/mtime 消除。**持平** |
| `find /dir -name "*.rs"`（按名递归） | 递归遍历全树 + 过滤        | 路径索引前缀扫描 + 过滤 | 都是 O(n)。**持平**                               |
| `find /dir -name "*.png" -size +10M` | 递归遍历 + stat            | 类型索引 ∩ 大小索引     | **Ousia OS 快几个数量级**                         |
| `ls tag://vacation/`                 | 不存在                     | 标签倒排索引            | **传统 FS 无法做到**                              |

### 按文件名递归搜索的诚实回答

`find /dir -name "*.rs"` 在传统 FS 中是递归遍历子目录 + 逐个比对文件名。在 Ousia OS 中，路径索引是一个全局 B-tree，`/dir/` 前缀范围扫描也需要逐个比对。两者都是 O(n)，Ousia OS 没有内置"文件名后缀倒排索引"（这需要额外的 trigram 或后缀数组，不是第一阶段目标）。

**但在实践中，这个场景的重要性被传统 FS 放大了。** 用户按文件名搜索，绝大多数情况真正想要的是：

| 用户想找的         | 传统 FS 的做法                                               | Ousia OS 的做法                                                      |
| ------------------ | ------------------------------------------------------------ | -------------------------------------------------------------------- |
| "所有照片"         | `find . -name "*.jpg" -o -name "*.png" -o -name "*.heic"...` | `query({type: "image/*"})` → 类型索引                                |
| "所有 Rust 源码"   | `find . -name "*.rs"`                                        | `query({type: "text/x-rust"})` → 类型索引                            |
| "上个月的 PDF"     | `find . -name "*.pdf" -newer ...`                            | `query({type: "application/pdf", date: "last-month"})` → 类型 ∩ 时间 |
| "标记为重要的东西" | 不存在（靠目录命名约定）                                     | `query({tag: "important"})` → 标签索引                               |

**传统 FS 用户被迫用文件名后缀搜索，是因为那是传统 FS 唯一的"类型信息"。** Ousia OS 有真正的类型索引，用户不需要再靠文件名后缀来区分文件种类。当用户确实需要"文件名包含某个子串"时，Ousia OS 的路径索引前缀扫描与传统 FS 性能持平——没有退化。但这种情况在 Ousia OS 中会少得多，因为有更好的工具。

### `ls -l` 的额外开销消除

路径索引的 B-tree value 可以将 Object 的 size、type、mtime 内联存储（类似 bcachefs 将 inode 关键字段内联在 dirent 中）。每个路径记录多 ~32 字节。百万文件额外 ~32MB——可接受。

### 结论

**对相同操作，没有系统性性能退化。** 对绝大多数真实搜索意图（按类型/时间/标签），Ousia OS 有数量级优势。**最常被担心的按名搜索，传统 FS 用户之所以依赖它，恰恰是因为传统 FS 缺少类型索引——Ousia OS 从根本上减少了这种需求。**

## 2.6 FS 放置与 Object Namespace

Ousia 的 FS 边界只保留两个互斥候选：**纯用户态 FS** 与 **纯内核态 FS**。之前的“用户态 FS 权威 + 内核元数据缓存 / fast-path assist”折中方案不作为主线，因为它迫使内核理解一部分 FS 语义，同时又保留跨边界一致性复杂度。

但“不内置 POSIX 文件系统语义”不等于“不需要 VFS”。为了统一路径解析、跨 provider 挂载、watch、revoke、`mmap(path)` 和兼容域路径投影，Ousia 必须有 OS 级 **Object Namespace / VFS-like** 层。

这个层负责名字到对象能力的解析，而不负责完整 FS 实现：

```
Path / Name / Selector
  → Object Namespace
  → NameBinding / MountBinding / ProviderRoot
  → ObjectHandle
  → MemoryObject / Stream / Operation
```

它和 Linux VFS 的差异是：中心对象不是 inode、dentry、fd 和 `file_operations`，而是 ObjectHandle、NameBinding、ProviderRoot、Capability、Version、Lease 和 fast-path descriptor。它允许 native FS、remote FS、加密 FS、同步层和兼容投影成为同一命名空间中的 Provider，而应用只看到统一的 Object API。

### 方案 A：纯用户态 FS

内核提供通用 substrate：Capability、Portal / Operation、MemoryObject、Pager 通道、IOQueue / IOBuffer、SharedQueue、EventPort、IOMMU / DMA 和调度。FS Provider 拥有 Object Store 的全部权威语义：OID、路径/标签/类型/时间索引、元数据、事务、版本、压缩、加密、GC、快照、权限和缓存策略。

Object Namespace 可以把一个 ProviderRoot 挂载到另一个 Provider 的目录下。例如 native provider 的 `/home/alice/remote` 可以绑定到 remote provider 的根对象。解析 `/home/alice/remote/a.txt` 时，Namespace 先在 native provider 中解析到 MountBinding，再切换 provider context，最后返回 `ObjectHandle{provider=remote, oid=...}`。应用使用同一套 API，差异只表现为延迟、failure event、consistency mode 和 durability fence。

这一方案必须把以下能力做成第一等工程对象：

- Portal fast call 与批量 metadata API
- FS Provider 接口：Object / Version / Lease / MemoryObject / Pager fault，而不是 POSIX FUSE path callback
- ProviderRoot / MountBinding / NamespaceView
- SDK / 兼容域缓存与 generation invalidation
- SharedQueue + TransferArena bypass session
- 专用 Pager fault queue、prefetch 和批量供页
- `fsync` / `msync` / journal commit 的跨边界协议

远程 FS 的 `mmap` 由这个接口反推出来：远端对象先成为本地 Remote-backed MemoryObject，缺页命中本地 cache 或触发 Provider fetch，完成后由 Pager 向内核交付页面。CPU fault 不直接等价于任意远程 RPC。

### 方案 B：纯内核态 FS

Object Store 核心成为内核 ABI，但不等于 POSIX VFS。内核提供 Ousia 原生 FS 原语：ObjectHandle、ObjectInfo、extent IO、版本/快照、事务/持久化、MemoryObject/page cache、async zero-copy IO queue 和 capability-based 权限检查。POSIX 兼容层仍在用户态，把 `open/stat/read/write` 翻译到这些原生对象原语。

即使选择纯内核态 FS，Object Namespace 仍然必要：它负责 NamespaceView、ProviderRoot、跨 provider 挂载、兼容域路径投影和用户态 FS Provider 接入。区别只是 native Object Store 的 provider 实现在内核内。

### 裁决标准

两个 FS 放置候选的裁决不看“纯粹性”，而看哪些语义值得成为 OS 永久底座：

| 维度                      | 纯用户态 FS                                                        | 纯内核态 FS           |
| ------------------------- | ------------------------------------------------------------------ | --------------------- |
| Object Store API 演进     | 更容易                                                             | 更受 ABI 约束         |
| mmap / page fault         | 依赖 Pager fast path                                               | 内核内闭环            |
| metadata-heavy workload   | 依赖 Provider fast call、batch、SDK cache、generation invalidation | 内核对象缓存天然存在  |
| fsync / msync / writeback | 跨边界协议复杂                                                     | 内核统一协调          |
| 安全与 TCB                | FS bug 隔离于用户态                                                | FS bug 是内核 bug     |
| 用户态/远程 FS            | Tier-1，自然                                                       | 仍需 FS Provider 接口 |
| POSIX 兼容                | 用户态 VFS 投影                                                    | 用户态 VFS 投影       |
| 多存储实现                | 自然                                                               | 需要内核 ABI 预留     |

---

# 第三部分：与 VM 系统的整合

> Pager 基本设计见 [03-pager-and-memory.md](../../core/03-pager-and-memory.md)。此处只讨论两种 FS 放置方案下的 VM 边界。

## 3.1 VM 边界随 FS 放置变化

纯用户态 FS 下，Pager-backed Memory Object 是关键支点：内核负责页表、页框、缺页入口、TLB、IOMMU 和全局回收；用户态 FS/Pager 负责页面内容、事务边界、回写策略、预读和崩溃恢复。缺页路径必须是 VM fast path，而不是普通业务 RPC。

纯内核态 FS 下，MemoryObject 与 page cache 可以直接绑定内核 Object Store。内核在缺页时直接查询对象页、触发 IO 或建立 CoW 映射；事务、dirty page、writeback 和 `fsync` / `msync` 可以在一个内核闭环内完成。

两种方案都必须保留的底线：

1. 全局内存压力时内核有最终回收权。
2. 同 Object 跨 Capsule 共享物理页框时必须保持 CoW 隔离。
3. `msync` / `fsync` 或等价持久化屏障返回成功后，系统必须保证崩溃恢复语义。
4. IOBuffer / DMA pin 生命周期不能与普通 MemoryObject 映射混同。

## 3.2 mmap + CoW 交互

纯用户态 FS 路径：

1. Capsule 写 mmap → CPU 写保护缺页
2. 内核通知 Pager："offset X 被写，当前页共享"
3. Pager 请求或选择新页框、复制数据、更新 extent 元数据
4. Pager 通知内核切换映射

比传统内核 CoW 多一次 Pager IPC。优化：批量 CoW 通知、预分裂（eager copy）。

纯内核态 FS 路径：

1. Capsule 写 mmap → CPU 写保护缺页
2. 内核 Object Store / VM 直接分配或选择新页框
3. 内核更新对象 extent / dirty state / CoW 元数据
4. 内核切换映射并维护 writeback 依赖

这一方案减少边界成本，但把 Object Store 的 CoW 和持久化逻辑放入内核。

## 3.3 直通路径 + PM 预留

数据库等场景仍应保留受控直通路径：`应用 → 设备能力句柄 → 块设备队列`，绕过通用 Object Store。IOMMU 约束访问范围。预留 DAX 模式支持 CXL/PM。纯内核态 FS 也不应取消这条路径，因为数据库需要自己控制 buffer、事务和 write ordering。

---

## 开放问题

1. **索引一致性**：路径索引更新成功但标签索引失败 → 不一致。方案：所有索引更新在同一 journal 事务中？还是路径索引为主、其他异步重建？
2. **GC 时机**：OID 无任何引用时立即 GC（影响性能）vs 延迟 GC（浪费空间）vs 引用计数（复杂度高）？
3. **名字服务故障**：名字服务崩溃后路径解析和标签查询均中断。高可用方案？

---

## 总结

1. **底层**：Object Store = CoW + 自描述 extent + per-object 压缩加密 + 池化存储
2. **中间层**：多索引（路径+标签+自动），路径是主索引不是唯一索引
3. **上层**：tree view 是一等命名、导航和作用域抽象，和 OID 稳定身份正交
4. **Object Namespace**：OS 内置 VFS-like 层，用 ProviderRoot / MountBinding 统一 native 与 remote FS
5. **FS 放置**：只保留纯用户态 FS 与纯内核态 FS 两个候选，混合态元数据缓存方案不再作为主线
6. **VM**：纯用户态 FS 依赖 Pager-backed Memory Object；纯内核态 FS 让 Object Store 与 page cache 在内核内闭环

**目录树没有被抛弃，也不只是兼容投影。** Ousia 保留 tier-1 tree view，用它承载人类导航、作用域、挂载和兼容生态；同时用 OID / ObjectHandle 解决"路径即身份"带来的引用漂移。

---

## 相关章节

- [07-data-and-filesystem.md](../../core/07-data-and-filesystem.md) — Object Store + Stream 顶层设计
- [03-pager-and-memory.md](../../core/03-pager-and-memory.md) — Pager 细节（崩溃模型、缺页协议）
- [00-philosophy.md](../../core/00-philosophy.md) — 反"一切皆文件"和"路径即身份"的哲学基础
- [06-roadmap.md](../../topics/06-roadmap.md) — 实现顺序
