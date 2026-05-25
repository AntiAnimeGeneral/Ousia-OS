# 05 — 数据抽象与文件系统

> 对应 `target.md` §3.5 + §4.6
> 姊妹篇：[13-fs-vm-deep-dive.md](./13-fs-vm-deep-dive.md)（目录树分析与 FS+VM 整合）

本文是数据抽象与文件系统的主设计。`13-fs-vm-deep-dive.md` 是深挖材料，用于展开目录树、索引和 FS/VM 边界的论证；若两者冲突，以本文和 `06-pager-and-memory.md` 的契约为准。

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

## 三层存储架构

**内核基座**：页框池、缺页入口、IOMMU/DMA、块设备队列。不做 inode/dentry、不做文件系统格式、不做回写策略。

**用户态存储核心服务**：对象命名/索引/元数据、事务协议（WAL/CoW）、页缓存策略、崩溃恢复、GC、快照。通过 Pager 与内核 VM 协作。

**直通路径**：数据库等场景通过设备能力句柄直接操作块设备，绕过通用存储栈。IOMMU 约束访问范围。

## 性能边界

高频元数据操作的挑战：跨服务往返延迟。对策：元数据推送 + Capsule 本地缓存、批量操作（一次 Operation 返回整个目录）、内核 Object ID / 基础元数据缓存。

共享页缓存：内核管理页框池，Pager 的 `share_page` 原语让不同 Memory Object 映射到同一物理页框。

## 开放问题

1. 大对象的流式部分更新：修改 10GB 对象的一个字节需要重写整个对象？需要增量补丁模型？
2. 跨设备同步冲突：离线修改同一 Object 后的合并策略？

## 相关章节

- [13-fs-vm-deep-dive.md](./13-fs-vm-deep-dive.md) — 目录树分析、索引设计、结构化类型边界、FS+VM 整合
- [06-pager-and-memory.md](./06-pager-and-memory.md) — Pager 细节
