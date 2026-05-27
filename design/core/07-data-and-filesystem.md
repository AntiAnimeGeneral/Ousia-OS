# 07 — 数据抽象与文件系统

> 承接 [target.md](../target.md) 中的数据语义、Object Store、Stream 与文件系统目标。
> 深挖材料：[00-fs-vm.md](../deep-dives/00-fs-vm.md)（目录树分析与 FS+VM 整合）

本文是数据抽象与文件系统的主设计。`deep-dives/00-fs-vm.md` 是深挖材料，用于展开目录树、索引和 FS/VM 边界的论证。文件系统放置目前只保留两个互斥候选：纯用户态 FS 与纯内核态 FS；混合态元数据缓存/fast-path assist 不作为主线方案。

## 为什么不用"目录树 + 字节流"

字节流文件没有结构、索引、版本、关系、事务——现代应用反复在文件系统之上重建这些基础设施（SQLite 是最广泛部署的数据库）。Ousia OS 的 Object Store 位于"字节流文件"和"应用数据库"之间：提供基础结构、索引、版本，但不做完整 SQL 引擎。

## Object Store

对象由 OID 标识，不依赖路径。每个对象自带元数据（类型、大小、时间）、自动索引、版本历史。操作包括 `create`, `read`, `write`（原子替换）, `delete`, `query`, `list_versions`, `revert`, `watch`, `relate`。

目录树是兼容投影——对 POSIX 兼容域提供路径视图，原生应用直接用 OID 和标签。

## Stream 抽象

Stream 是纯数据流动抽象——Object 负责"数据是什么"，Stream 负责"字节怎么传"。

Stream 原生支持：背压、取消、批量、优先级、多播。

### 批判：Stream 是否在开 Unix "一切皆文件"的倒车？

**不是。** Unix fd 的问题不在 `read/write`——这本身是好的 IO 抽象。问题在 fd 被强行塞进了不该是 IO 的东西：设备控制（`ioctl`）、服务发现（socket 路径约定）、内核状态（`/proc`）、配置身份（路径=文件身份）。

Ousia OS 把这六个角色拆成了四个不同的一等抽象：

| 职责     | Unix                | Ousia OS                     |
| -------- | ------------------- | ---------------------------- |
| 持久存储 | fd (open + path)    | Object (OID + 元数据 + 版本) |
| 数据流动 | fd (read/write)     | Stream                       |
| 设备控制 | fd + ioctl          | 设备能力句柄 + 队列对象      |
| 服务发现 | 路径约定 + 环境变量 | Service Graph                |

**Stream 只传输不解释。** 语义是上层的事。硬约束：Stream 不替代设备控制、不替代对象元数据查询、不替代服务发现。如果 Stream 子类型（NetworkStream/FileStream）的 API 严重分叉，宁可拆开，不要强行统一。

## FS 放置候选

Ousia 的文件系统问题不是“是否兼容 POSIX”，而是谁拥有 Object Store 的权威语义。两个候选都不把 POSIX 作为原生 API；POSIX 兼容层只是在原生对象接口之上的投影。

### 方案 A：纯用户态 FS

内核只提供 Capability、Portal/Operation、MemoryObject、Pager 通道、IOQueue/IOBuffer、SharedQueue、EventPort、IOMMU/DMA 和调度机制。Object Store 的命名、索引、元数据、事务、版本、压缩、加密、GC、权限和缓存策略全部由用户态 FS 服务拥有。

这一方案的优势是语义边界最干净，FS 可以作为系统服务演进，多种存储服务和兼容投影可以共存。代价是 mmap 缺页、持久化确认和 metadata-heavy workload 都依赖高质量 IPC、批量接口、客户端缓存、bypass session 和 Pager 协议。

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
