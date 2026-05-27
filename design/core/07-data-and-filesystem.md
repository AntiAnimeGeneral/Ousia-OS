# 07 — 数据抽象与文件系统

> 承接 [target.md](../target.md) 中的数据语义、Object Store、Stream 与文件系统目标，以及 R1/R2/R3/R4/R8/R9 硬需求。
> 深挖材料：[00-fs-vm.md](../deep-dives/00-fs-vm.md)（目录树分析与 FS+VM 整合）

本文是数据抽象与文件系统的主设计。`deep-dives/00-fs-vm.md` 是深挖材料，用于展开目录树、索引和 FS/VM 边界的论证。文件系统放置目前只保留两个互斥候选：纯用户态 FS 与纯内核态 FS；混合态元数据缓存/fast-path assist 不作为主线方案。

## 为什么不只用"目录树 + 字节流"

字节流文件没有结构、索引、版本、关系、事务——现代应用反复在文件系统之上重建这些基础设施（SQLite 是最广泛部署的数据库）。Ousia OS 的 Object Store 位于"字节流文件"和"应用数据库"之间：提供基础结构、索引、版本，但不做完整 SQL 引擎。

## Object Store

对象由 OID 标识，不依赖路径；tree view 由 Object Namespace 维护，是同样一等的命名、导航、挂载和作用域抽象。每个持久对象都应能出现在某个 tree view 中，但对象身份、版本和权限不被某个路径字符串独占。对象自带元数据（类型、大小、时间）、自动索引、版本历史。操作包括 `create`, `read`, `write`（原子替换）, `delete`, `query`, `list_versions`, `revert`, `watch`, `relate`。

目录树不是降级的兼容投影；它是 Object Namespace 的 tier-1 tree view。POSIX 兼容域看到的是这个 tree view 的兼容翻译，原生应用可以同时使用路径、OID、标签和对象能力句柄。

## Object Namespace / VFS-like 层

Ousia 不把 POSIX 文件系统语义内置进内核，但必须内置一个 OS 级 **Object Namespace**。它是 VFS-like 层，负责统一路径解析、跨 FS Provider 挂载、NameBinding、ProviderRoot、watch、revoke、generation invalidation，以及 path/name 到 ObjectHandle / MemoryObject 的入口。

这个层的中心不是 inode、dentry、fd 和 `file_operations`，而是：

- `NameBinding`：名称到 Object、名称到名称、名称到 ProviderRoot 的绑定
- `ProviderRoot`：一个 FS Provider 根对象的 capability，可挂载到另一个 provider 的命名空间中
- `MountBinding`：把 ProviderRoot 绑定到父目录 NameBinding 下的系统对象
- `ObjectHandle`：解析后的对象能力，携带 provider、rights、version、lease 和 fast-path descriptor
- `NamespaceView`：Capsule 看到的命名空间视图，用于兼容域、沙箱和工作区

例如 native FS 中的 `/home/alice/remote` 可以是一个指向 remote FS ProviderRoot 的 MountBinding。应用解析 `/home/alice/remote/a.txt` 时，Namespace 先在 native provider 中解析到挂载点，再切换到 remote provider 继续解析，最后返回统一的 ObjectHandle。应用仍然使用同一套 Object API；native 和 remote 的差异只表现为延迟、failure event、consistency mode 和 durability fence。

因此，Ousia 的立场是：不内置 POSIX VFS，但内置 Object/Provider/Capability VFS。没有这个层，统一路径解析、远程挂载、权限撤销、watch 和 `mmap(path)` 都会退化为各 FS Provider 的私有约定。

## Stream 抽象

Stream 是纯数据流动抽象——Object 负责"数据是什么"，Stream 负责"字节怎么传"。

Stream 原生支持：背压、取消、批量、优先级、多播。

### 批判：Stream 是否在开 Unix "一切皆文件"的倒车？

**不是。** Unix fd 的问题不在 `read/write`——这本身是好的 IO 抽象。问题在 fd 被强行塞进了不该是 IO 的东西：设备控制（`ioctl`）、服务发现（socket 路径约定）、内核状态（`/proc`）、配置身份（路径=文件身份）。

Ousia OS 把这六个角色拆成了四个不同的一等抽象：

| 职责     | Unix                | Ousia OS                                 |
| -------- | ------------------- | ---------------------------------------- |
| 持久存储 | fd (open + path)    | Object (OID + tree view + 元数据 + 版本) |
| 数据流动 | fd (read/write)     | Stream                                   |
| 设备控制 | fd + ioctl          | 设备能力句柄 + 队列对象                  |
| 服务发现 | 路径约定 + 环境变量 | Service Graph                            |

**Stream 只传输不解释。** 语义是上层的事。硬约束：Stream 不替代设备控制、不替代对象元数据查询、不替代服务发现。如果 Stream 子类型（NetworkStream/FileStream）的 API 严重分叉，宁可拆开，不要强行统一。

## FS 放置候选

Ousia 的文件系统问题不是“是否兼容 POSIX”，而是谁拥有 Object Store 的权威语义。两个候选都不把 POSIX 作为原生 API；POSIX 兼容层只是在原生对象接口之上的投影。

### 方案 A：纯用户态 FS

内核只提供 Capability、Portal/Operation、MemoryObject、Pager 通道、IOQueue/IOBuffer、SharedQueue、EventPort、IOMMU/DMA 和调度机制。Object Store 的命名、索引、元数据、事务、版本、压缩、加密、GC、权限和缓存策略全部由用户态 FS 服务拥有。

这一方案的优势是语义边界最干净，FS 可以作为系统服务演进，多种存储服务和兼容投影可以共存。代价是 mmap 缺页、持久化确认和 metadata-heavy workload 都依赖高质量 IPC、批量接口、客户端缓存、bypass session 和 Pager 协议。

纯用户态 FS 方案需要一套类似 FUSE 的接入接口，但它不应复刻 POSIX 的 `lookup/read/write/readdir` 回调宇宙。Ousia 的接口应是 **FS Provider**：面向 Object、NameBinding、Version、Lease、MemoryObject 和 Pager fault 的 provider 协议。远程 FS、加密 FS、同步盘、对象网关和兼容投影都通过它挂入系统。

FS Provider 至少需要表达：

- `resolve(name, version_policy) -> ObjectHandle`
- `bind_provider_root(parent, name, ProviderRoot, mount_policy)`
- `query(object, fields) -> ObjectInfo`
- `read_extent` / `write_extent` / `commit_transaction`
- `create_memory_object(object, cache_policy, fault_policy)`
- `page_in` / `page_out` / `invalidate` / `prefetch`
- `lease_acquire` / `lease_break` / `watch`
- `durability_fence(local | remote | quorum)`

这样远程 FS 可以把远端对象 materialize 成本地 Remote-backed MemoryObject，再通过 Pager 协议支持 `mmap`。tree view 负责命名、挂载和导航；真正的映射身份是 ObjectHandle + version/lease + MemoryObject Capability。

### 方案 B：纯内核态 FS

Object Store 核心语义成为内核 ABI：ObjectHandle、对象元数据、extent IO、事务/持久化、版本/快照、MemoryObject/page cache、async zero-copy IO 和权限检查都由内核直接提供。POSIX 仍不进入内核原生 API；兼容层在用户态把 `open/stat/read/write` 翻译到 Ousia 对象原语。

这一方案的优势是热路径和持久化闭环最强，mmap、page cache、writeback、fsync/msync 和 zero-copy async IO 可以在内核内统一实现。代价是内核 TCB 扩大，Object Store API 必须长期稳定，FS bug 成为内核 bug，多存储实现的自由度降低。

## 性能边界

两种方案的性能边界不同：

- 纯用户态 FS：靠 Portal fast call、批量 Operation、客户端/兼容域缓存、SharedQueue bypass、IOQueue/IOBuffer 和 Pager-backed Memory Object 达成性能目标。
- 纯内核态 FS：靠内核 Object Store、page cache、async IO queue、MemoryObject 和统一 writeback 达成性能目标。

不采用“用户态 FS 权威 + 内核元数据缓存”的折中方案，因为它会迫使内核理解一部分 FS 语义，同时又保留跨边界一致性复杂度。

## 开放问题

1. 大对象的流式部分更新：修改 10GB 对象的一个字节需要重写整个对象？需要增量补丁模型？
2. 跨设备同步冲突：离线修改同一 Object 后的合并策略？
3. FS 放置裁决：纯用户态 FS 与纯内核态 FS 哪个更符合 Ousia 的长期内核边界和性能目标？

## 相关章节

- [00-fs-vm.md](../deep-dives/00-fs-vm.md) — 目录树分析、索引设计、结构化类型边界、FS+VM 整合
- [03-pager-and-memory.md](./03-pager-and-memory.md) — Pager 细节
