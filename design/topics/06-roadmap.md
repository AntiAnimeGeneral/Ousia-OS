# 12 — 路线图与非目标

> 对应 `target.md` §2、`requirements.md`、`target.md` §5 + §6 + §7 + §8

## 非目标（第一阶段绝对不做）

- **不兼容 POSIX 作为原生接口**——需要 POSIX 的应用通过 Linux 兼容域运行
- **不支持老旧硬件**——BIOS、32 位 CPU、无 IOMMU 的系统
- **不重复造轮子**——不开发浏览器引擎、完整桌面环境、编程语言、完整数据库
- **不开放内核扩展**——没有 LKM，所有驱动在用户态
- **暂不实现完整的能力级联撤销**——第一阶段仅直接撤销
- **暂不实现完整去中心化账户**——预留身份句柄类型

## 系统分层

```
第 0 层: 微内核
  调度+执行等级、地址空间+页表、Communication Fabric+能力句柄、
  中断+时钟、IOMMU/DMA 仲裁、MemoryObject、启动句柄注入；
  纯内核态 FS 方案还包含 Object Store 核心

第 1 层: 基础系统服务
  名字服务(←内核启动句柄注入)、Object Namespace、Capsule 管理器、
  网络服务、设备管理与 Driver Manager/Index/Host、日志与观测；
  纯用户态 FS 方案还包含对象存储服务与 Pager 监督服务

第 2 层: 平台服务
  Package Cell 管理器、图形与窗口系统、策略引擎、兼容域网关

第 3 层: 应用与兼容环境
  原生应用、Linux 兼容域、开发者工具链
```

**Bootstrap**：内核注入初始句柄 → 启动名字服务(第一个用户态进程) → 启动 Capsule 管理器 → 依次启动所有第 1 层服务 → 第 2 层服务 → 用户应用。

## 需求驱动原则

第一阶段路线图以 [requirements.md](../requirements.md) 的硬需求为验收入口。每个阶段都必须能说明它验证了哪条需求；如果一个阶段只验证抽象名称，而不能验证类 FUSE 接入、目录挂载、mmap、zero-copy、同步/异步一等、能力撤销、兼容域边界或 Package Cell 生命周期中的至少一项，它就不应进入第一阶段主线。

需求编号来自 [requirements.md](../requirements.md)：R1 类 FUSE 接入，R2 目录挂载，R3 mmap，R4 zero-copy / low-copy，R5 同步/异步一等，R6 用户态服务热路径，R7 能力权限，R8 兼容域边界，R9 远程资源，R10 Package Cell 生命周期。

## 第一阶段落地顺序

| Phase                 | 需求               | 目标                                                                  | 核心验证                                                    |
| --------------------- | ------------------ | --------------------------------------------------------------------- | ----------------------------------------------------------- |
| 1a: 微内核原语        | R5, R7             | QEMU 中启动内核，任务+Portal/Operation+能力句柄+抢占调度+启动句柄注入 | 两个任务通过 Portal fast call 传递能力句柄                  |
| 1a.5: 异步通信原语    | R5, R6             | Continuation + EventPort/WaitSet + timeout/cancel/late reply          | 一个任务提交异步 Operation，另一个任务延迟完成并唤醒 Future |
| 1b: 名字服务+Capsule  | R7, R10            | Service Graph bootstrap + Capsule 生命周期                            | Capsule 通过名字服务发现并调用另一个 Capsule                |
| 1c: Object Namespace  | R1, R2, R7, R8, R9 | 路径解析 + ProviderRoot + MountBinding + ObjectHandle 缓存与撤销      | native 目录挂载 remote provider；应用拿到统一 ObjectHandle  |
| 1d: MemoryObject      | R3, R4, R9         | 缺页处理 + 纯用户态 Pager / 纯内核 Object Store 两条供页路径          | mmap 缺页正常供页；故障按所选 FS 放置方案处理               |
| 1e: 最小对象存储      | R1, R2, R3         | 对象 CRUD + 元数据 + 标签 + 目录树兼容投影；裁决用户态或内核态落地    | "路径不是唯一真相"                                          |
| 1f: Package Cell 原型 | R7, R10            | 声明式安装/激活/回滚/卸载 + 多版本并存                                | 安装两个依赖不同版本库的 Cell                               |
| 1g: 驱动框架原型      | R4, R6, R7         | 设备能力句柄 + IOMMU 授权 + IOQueue/IOBuffer + 用户态 MMIO            | 用户态 NVMe 队列提交/完成；驱动崩溃→撤销 DMA→复位→恢复      |
| 1h: 兼容层            | R1, R2, R7, R8     | Linux 兼容域（类 WSL2 VM）+ 兼容域网关                                | 兼容域内运行 bash+gcc+编译 C 程序                           |

## 设计判断标准

1. 消除隐式全局状态？ 2. 权限显式、可审计、可回收？ 3. 支持异步、取消、背压？ 4. 前台交互被保护？ 5. 消除对 PATH/bashrc/profile 依赖？ 6. 强化 Package Cell 和 Capsule？ 7. 兼容性限制在边界上？ 8. 对异构硬件友好？ 9. 遵循 let-it-crash？ 10. 故障可测试、可观测、可诊断？

## 文档索引与归属

### 主线设计

| #   | 文件                                                                 | 归属                                     |
| --- | -------------------------------------------------------------------- | ---------------------------------------- |
| 00  | [00-philosophy.md](../core/00-philosophy.md)                         | 设计立场与顶层原则                       |
| 00  | [00-philosophy.md](../core/00-philosophy.md)                         | 设计哲学                                 |
| 01  | [01-capsule-and-capability.md](../core/01-capsule-and-capability.md) | 运行隔离与能力权限                       |
| 02  | [02-communication-fabric.md](../core/02-communication-fabric.md)     | 统一通信基座                             |
| 03  | [03-pager-and-memory.md](../core/03-pager-and-memory.md)             | MemoryObject 与 Pager 边界               |
| 04  | [04-driver-and-kernel.md](../core/04-driver-and-kernel.md)           | 内核/驱动边界与 IO 原语                  |
| 05  | [05-compute-and-scheduling.md](../core/05-compute-and-scheduling.md) | 调度、计算域、异构资源                   |
| 06  | [06-service-graph.md](../core/06-service-graph.md)                   | 服务发现、版本协商、启动                 |
| 07  | [07-data-and-filesystem.md](../core/07-data-and-filesystem.md)       | Object Namespace / Store / Stream 主设计 |
| 08  | [08-package-cell.md](../core/08-package-cell.md)                     | 软件单元、依赖、生命周期                 |

### 边界专题

| #   | 文件                                                           | 归属                 |
| --- | -------------------------------------------------------------- | -------------------- |
| 00  | [00-async-and-mmap.md](./00-async-and-mmap.md)                 | 异步语义与 mmap 张力 |
| 01  | [01-compatibility.md](./01-compatibility.md)                   | Linux 兼容域         |
| 02  | [02-engineering.md](./02-engineering.md)                       | 工程化、构建、测试   |
| 03  | [03-shell-and-tools.md](./03-shell-and-tools.md)               | Shell 与交互工具     |
| 04  | [04-environment-and-config.md](./04-environment-and-config.md) | 环境与配置管理       |
| 05  | [05-identity-and-accounts.md](./05-identity-and-accounts.md)   | 身份与信任模型       |

### 深挖与参考

| #   | 文件                                             | 归属                       |
| --- | ------------------------------------------------ | -------------------------- |
| req | [../requirements.md](../requirements.md)         | 需求库与抽象推导索引       |
| 06  | [06-roadmap.md](./06-roadmap.md)                 | 路线图与非目标             |
| 00  | [00-fs-vm.md](../deep-dives/00-fs-vm.md)         | FS/VM 深挖材料，不是主规范 |
| ref | [../reference/README.md](../reference/README.md) | 驱动、旁路、子系统路径参考 |

全局术语表见 [../glossary.md](../glossary.md)。

## 文档层级

Ousia 文档按“问题 → 目标 → 需求/推导 → 主设计 → 深挖/参考”组织：

| 层级     | 文档                                | 职责                                                                 |
| -------- | ----------------------------------- | -------------------------------------------------------------------- |
| 问题层   | [pain-points.md](../pain-points.md) | 解释为什么现有系统不够好，提供案例和动机。                           |
| 总纲层   | [target.md](../target.md)           | 定义愿景目标、需求摘要、推导摘要、设计约束、非目标和落地顺序。 |
| 需求层   | [requirements.md](../requirements.md) | 保存可增长的硬需求库、抽象推导索引和结论落点。 |
| 主设计层 | [core/](../core/)                   | 定义可长期演进的系统抽象和主线契约。每个主设计应说明承接了哪些需求。 |
| 专题层   | [topics/](./)                       | 处理跨主线的边界问题、工程路线、兼容性和路线图。                     |
| 深挖层   | [deep-dives/](../deep-dives/)       | 保存论证、候选方案、裁决标准和开放问题，不作为唯一主规范。           |
| 参考层   | [reference/](../reference/)         | 保存路径矩阵、SDK 草案、调研材料和可替换的工程背景。                 |

泛目标和硬需求都必须保留，但用途不同：泛目标决定系统方向，硬需求决定第一阶段验收，抽象推导决定哪些设计是被需求迫出来的，主线章节再把这些抽象写成稳定契约。`target.md` 保持摘要入口；需求和推导增长时更新 [requirements.md](../requirements.md)。

## 文档归属原则

为避免同一设计在多个章节各自演化，后续新增内容按下面规则归属：

- **通信、异步请求、事件等待、服务间旁路队列**：归属 [02-communication-fabric.md](../core/02-communication-fabric.md)。其他章节只说明如何使用这些原语。
- **Package Cell、依赖解析、多版本并存、生命周期**：归属 [08-package-cell.md](../core/08-package-cell.md)。环境章节只消费解析结果。
- **硬需求、需求编号、抽象推导索引和结论落点**：归属 [requirements.md](../requirements.md)。`target.md` 只保留摘要。
- **运行环境、用户/系统配置、配置服务**：归属 [04-environment-and-config.md](./04-environment-and-config.md)。Shell 章节只描述交互命令。
- **Object Namespace / Object Store / Stream 的主设计**：归属 [07-data-and-filesystem.md](../core/07-data-and-filesystem.md)。[00-fs-vm.md](../deep-dives/00-fs-vm.md) 承载调研、论证和细化方案。
- **MemoryObject 与 Pager 边界**：归属 [03-pager-and-memory.md](../core/03-pager-and-memory.md)。FS 深挖只讨论两种 FS 放置方案下如何使用它。
- **内核/驱动边界、IOQueue/IOBuffer/Doorbell/Fence**：归属 [04-driver-and-kernel.md](../core/04-driver-and-kernel.md)。`reference/` 下文档承载背景材料、路径矩阵和 SDK 轮廓。
- **实现语言、构建、测试、更新**：归属 [02-engineering.md](./02-engineering.md)。不要在工程章节重复具体子系统设计。
