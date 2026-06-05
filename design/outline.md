# Ousia OS 文档大纲

本文是 Ousia OS 设计文档的全局地图，用于两件事：给人类提供阅读路径，给 AI 提供查漏补缺和一致性检查的索引。

如果某个概念已经有 owning 文档，其他文档只应链接和消费它，不应重新定义。需要改变语义时，先改 owning 文档，再同步引用方。

本文档树是持续演进的草案，不是冻结后的实现说明。现有实现只能作为证据来源，不能反过来锁死更好的抽象、边界或实现路径；如果更好的方案出现，应优先重构 owning 文档和对应实现。

## 1. 推荐阅读路径

### 1.1 快速理解

1. [target.md](./target.md)：理解愿景、目标摘要、设计约束和阶段方向。
2. [pain-points.md](./pain-points.md)：理解 Ousia OS 要解决的问题来源。
3. [requirements.md](./requirements.md)：查看硬需求、抽象推导和结论落点。
4. [glossary.md](./glossary.md)：确认项目自造术语和重新定义术语。
5. [topics/06-roadmap.md](./topics/06-roadmap.md)：查看第一阶段落地顺序和验收重点。

### 1.2 主线设计阅读

1. [core/00-philosophy.md](./core/00-philosophy.md)：设计立场与顶层原则。
2. [core/01-capsule-and-capability.md](./core/01-capsule-and-capability.md)：运行隔离与能力权限。
3. [core/02-communication-fabric.md](./core/02-communication-fabric.md)：同步调用、异步请求、等待、队列和旁路通信。
4. [core/03-pager-and-memory.md](./core/03-pager-and-memory.md)：MemoryObject、Pager、缺页和映射边界。
5. [core/04-driver-and-kernel.md](./core/04-driver-and-kernel.md)：内核/驱动边界、硬件授权和 IO 原语。
6. [core/05-compute-and-scheduling.md](./core/05-compute-and-scheduling.md)：异构计算、执行等级、调度和功耗预算。
7. [core/06-service-graph.md](./core/06-service-graph.md)：服务发现、版本协商、启动和系统组织。
8. [core/07-data-and-filesystem.md](./core/07-data-and-filesystem.md)：Object Namespace、Object Store、Stream、FS Provider。
9. [core/08-package-cell.md](./core/08-package-cell.md)：软件单元、依赖、环境和生命周期。

### 1.3 专题与边界阅读

| 主题                  | 文档                                                                         | 何时阅读                                                     |
| --------------------- | ---------------------------------------------------------------------------- | ------------------------------------------------------------ |
| 同步、异步、mmap 边界 | [topics/00-async-and-mmap.md](./topics/00-async-and-mmap.md)                 | 当设计涉及等待、缺页、取消或 sync/async API 时。             |
| Linux 兼容            | [topics/01-compatibility.md](./topics/01-compatibility.md)                   | 当设计需要旧生态、POSIX 语义或兼容域网关时。                 |
| 工程化                | [topics/02-engineering.md](./topics/02-engineering.md)                       | 当设计涉及实现语言、构建、测试、更新和硬件支持边界时。       |
| Shell 与工具          | [topics/03-shell-and-tools.md](./topics/03-shell-and-tools.md)               | 当设计涉及交互命令、管道、调试体验和开发工具时。             |
| 环境与配置            | [topics/04-environment-and-config.md](./topics/04-environment-and-config.md) | 当设计涉及运行环境、配置服务和兼容域库视图时。               |
| 身份与账户            | [topics/05-identity-and-accounts.md](./topics/05-identity-and-accounts.md)   | 当设计涉及用户身份、设备身份、信任和 Package Cell 发布者时。 |
| 路线图                | [topics/06-roadmap.md](./topics/06-roadmap.md)                               | 当需要确定第一阶段顺序、非目标和验收闭环时。                 |

### 1.4 参考与分析笔记

| 类别         | 文档                                                                               | 职责                                                       |
| ------------ | ---------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| 外部参考     | [notes/reference/README.md](./notes/reference/README.md)                           | 保存外部系统、外部机制和现有技术模式。                     |
| IPC 参考     | [notes/reference/00-ipc-sel4-fuchsia.md](./notes/reference/00-ipc-sel4-fuchsia.md) | 保存 seL4 / Fuchsia IPC 背景、机制和比较材料。             |
| 事件等待参考 | [notes/reference/01-epoll-and-kqueue.md](./notes/reference/01-epoll-and-kqueue.md) | 比较 epoll / kqueue 的 wait set、ready notification 语义。 |
| Zircon 内核参考 | [notes/reference/06-fuchsia-zircon-kernel.md](./notes/reference/06-fuchsia-zircon-kernel.md) | 保存 Fuchsia/Zircon handle/object、VMO/VMAR、channel/call、driver framework 和用户库参考。 |
| 设计分析     | [notes/analysis/README.md](./notes/analysis/README.md)                             | 保存 Ousia 设计判断、候选方案、SDK 草案和子系统路径矩阵。  |
| 实现草案     | [implementation/README.md](./implementation/README.md)                             | 保存临时实现路线、代码演进草案和短期 reviewer 入口。       |
| FS/VM 分析   | [notes/analysis/00-fs-vm.md](./notes/analysis/00-fs-vm.md)                         | 保存 FS/VM 候选方案、调研、裁决标准和开放问题。            |

## 2. 文档层级

| 层级     | 文档                                   | 职责                                                           |
| -------- | -------------------------------------- | -------------------------------------------------------------- |
| 大纲层   | 本文                                   | 提供全文档地图、阅读路径、归属表和查漏补缺清单。               |
| 问题层   | [pain-points.md](./pain-points.md)     | 解释为什么现有系统不够好，提供案例和动机。                     |
| 总纲层   | [target.md](./target.md)               | 定义愿景目标、需求摘要、推导摘要、设计约束、非目标和落地顺序。 |
| 需求层   | [requirements.md](./requirements.md)   | 保存可增长的硬需求库、抽象推导索引和结论落点。                 |
| 术语层   | [glossary.md](./glossary.md)           | 定义项目术语，避免同一概念在不同文档漂移。                     |
| 主设计层 | [core/](./core/)                       | 定义可长期演进的系统抽象和主线契约。                           |
| 专题层   | [topics/](./topics/)                   | 处理跨主线的边界问题、工程路线、兼容性和路线图。               |
| 笔记层   | [notes/reference/](./notes/reference/) | 保存外部系统、外部机制和现有技术模式，不作为 Ousia 规范。      |
| 分析层   | [notes/analysis/](./notes/analysis/)   | 保存 Ousia 设计推导、候选方案、草案和深挖，不作为最终规范。    |
| 提案层   | [proposals/](./proposals/)             | 保存进入实施前的 proposal packet，不作为长期规范源。            |
| 实现层   | [implementation/](./implementation/)   | 保存短期实现草案、代码演进说明和临时路线，不作为最终规范。     |

## 3. 语义归属表

| 语义                                                                                              | Owning 文档                                                                                          | 引用方应如何处理                                                                     |
| ------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| 文档地图、阅读顺序、归属规则                                                                      | 本文                                                                                                 | 只链接本文，不复制整张索引。                                                         |
| 系统愿景、顶层目标、设计约束                                                                      | [target.md](./target.md)                                                                             | 摘要引用，不重写目标表。                                                             |
| 问题陈述和痛点案例                                                                                | [pain-points.md](./pain-points.md)                                                                   | 使用痛点编号或链接，不重新展开案例。                                                 |
| 硬需求、需求编号、抽象推导索引                                                                    | [requirements.md](./requirements.md)                                                                 | 只引用 `R#` / `D#`，不要复制完整表。                                                 |
| 术语定义                                                                                          | [glossary.md](./glossary.md)                                                                         | 使用同一术语，不在章节内重新定义。                                                   |
| Capsule、Capability、能力转移与撤销                                                               | [core/01-capsule-and-capability.md](./core/01-capsule-and-capability.md)                             | 其他章节说明所需能力，不定义能力模型。                                               |
| Portal、Operation、Continuation、EventPort、SharedQueue、bypass session                           | [core/02-communication-fabric.md](./core/02-communication-fabric.md)                                 | 其他章节说明调用形态，不定义通信原语。                                               |
| Phase 1 Ousia capability kernel baseline、短期 kernel 实现路线和 reviewer 入口                     | [implementation/00-ousia-kernel-architecture.md](./implementation/00-ousia-kernel-architecture.md)     | core 文档说明长期 Ousia 抽象；implementation 文档只承接近期实现路线，不反向重定义 core 语义。 |
| 进入实施前的 proposal packet、重构交接和 review focus                                             | [proposals/README.md](./proposals/README.md)                                                         | 提案通过 review 和实施后，稳定结论应回写 owning 文档或代码 rustdoc。                 |
| MemoryObject、Pager、缺页、映射和回写边界                                                         | [core/03-pager-and-memory.md](./core/03-pager-and-memory.md)                                         | FS/VM 深挖只讨论方案和取舍。                                                         |
| Hardware Core、Driver Host、IOQueue、IOBuffer、Doorbell、Fence、IOMMU 授权                        | [core/04-driver-and-kernel.md](./core/04-driver-and-kernel.md)                                       | reference 文档只保留背景和草案。                                                     |
| Compute Domain、Execution Class、调度和功耗预算                                                   | [core/05-compute-and-scheduling.md](./core/05-compute-and-scheduling.md)                             | 其他章节只声明资源需求和优先级传播。                                                 |
| Service Graph、服务发现、版本协商和 bootstrap                                                     | [core/06-service-graph.md](./core/06-service-graph.md)                                               | Package Cell 和 Capsule 章节只消费服务组织结果。                                     |
| Object Namespace、tier-1 tree view、Object Store、Stream、FS Provider、ProviderRoot、MountBinding | [core/07-data-and-filesystem.md](./core/07-data-and-filesystem.md)                                   | 其他章节只说明如何使用对象、命名和存储接口。                                         |
| Package Cell、依赖解析、多版本并存、生命周期                                                      | [core/08-package-cell.md](./core/08-package-cell.md)                                                 | 环境章节只消费解析和激活结果。                                                       |
| 同步/异步与 mmap 的张力                                                                           | [topics/00-async-and-mmap.md](./topics/00-async-and-mmap.md)                                         | 主设计只保留最终契约。                                                               |
| Linux 兼容域和兼容域网关                                                                          | [topics/01-compatibility.md](./topics/01-compatibility.md)                                           | core 文档只定义原生接口。                                                            |
| 实现语言、构建、测试、更新                                                                        | [topics/02-engineering.md](./topics/02-engineering.md)                                               | 不重复具体子系统设计。                                                               |
| Shell、交互工具和命令体验                                                                         | [topics/03-shell-and-tools.md](./topics/03-shell-and-tools.md)                                       | 不定义 Package Cell 或配置服务。                                                     |
| 运行环境、配置服务、兼容域库视图                                                                  | [topics/04-environment-and-config.md](./topics/04-environment-and-config.md)                         | 不重新定义依赖解析。                                                                 |
| 身份、账户、Device Owner、Key Agent、FSKeyPolicy、信任和发布者                                    | [topics/05-identity-and-accounts.md](./topics/05-identity-and-accounts.md)                           | 第一阶段只引用预留身份句柄和密钥策略元数据。                                         |
| 第一阶段路线、非目标、阶段验收                                                                    | [topics/06-roadmap.md](./topics/06-roadmap.md)                                                       | 只引用需求编号和 owning 文档，不复制文档地图。                                       |

## 4. AI 查漏补缺清单

AI review 全文档时，应按下列顺序检查：

1. 每个新增抽象是否能回溯到 [requirements.md](./requirements.md) 中的 `R#` 或 `D#`。
2. 每个 `D#` 推导结论是否已经落到语义归属表中的 owning core 文档。
3. `target.md` 是否仍保持摘要入口，没有吸收完整需求库或详细论证。
4. `topics/06-roadmap.md` 的 phase 是否引用了正确需求编号。
5. 同一术语是否只在 [glossary.md](./glossary.md) 定义一次。
6. notes 中的新结论是否已经同步回 owning core/topics 文档，或明确标记为候选方案。
7. POSIX、Unix、FUSE、VFS、path、file 等兼容概念是否停留在兼容层或投影层，没有污染原生 API。
8. 同步和异步是否都保持 first-class，没有把其中一种写成唯一正确路径。
9. 用户态服务、微内核、能力模型和旁路路径是否仍满足热路径性能约束。
10. 新增文档是否更新本文、[requirements.md](./requirements.md) 或 [topics/06-roadmap.md](./topics/06-roadmap.md) 中对应索引。

## 5. 去重规则

- `target.md` 可以摘要问题、目标、需求和推导，但不保存完整需求表和完整推导表。
- [requirements.md](./requirements.md) 保存需求和推导，不展开主设计契约。
- [glossary.md](./glossary.md) 定义术语，不保存设计论证。
- `core/` 文档保存稳定契约，不保留长篇外部系统调研。
- `topics/` 文档处理跨主线边界，不拥有主抽象定义。
- `notes/reference/` 只保存外部材料；`notes/analysis/` 可以展开论证、草案和深挖，但结论稳定后必须回写 owning 文档。
- 当同一语义在多个文档出现时，保留 owning 文档中的定义，其他位置改成链接、编号或一句话摘要。
