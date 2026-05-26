# 06 — 路线图与非目标

> 汇总 [target.md](../target.md) 中的非目标、系统分层、落地顺序和文档索引。

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
  中断+时钟、IOMMU/DMA 仲裁、Pager-backed Memory Object、启动句柄注入

第 1 层: 基础系统服务
  名字服务(←内核启动句柄注入)、Capsule 管理器、对象存储服务、
  网络服务、设备管理与 Driver Manager/Index/Host、日志与观测、Pager 监督服务

第 2 层: 平台服务
  Package Cell 管理器、图形与窗口系统、策略引擎、兼容域网关

第 3 层: 应用与兼容环境
  原生应用、Linux 兼容域、开发者工具链
```

**Bootstrap**：内核注入初始句柄 → 启动名字服务(第一个用户态进程) → 启动 Capsule 管理器 → 依次启动所有第 1 层服务 → 第 2 层服务 → 用户应用。

## 第一阶段落地顺序

| Phase                           | 目标                                                                            | 核心验证                                                              |
| ------------------------------- | ------------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| 1a: 微内核原语                  | QEMU 中启动内核，任务+Portal/Operation+能力句柄+抢占调度+启动句柄注入           | 两个任务通过 Portal fast call 传递能力句柄                            |
| 1a.5: Communication Fabric 闭环 | Portal fast call + Continuation + EventPort/WaitSet + timeout/cancel/late reply | 一个任务完成同步 fast call；另一个任务提交异步 Operation 并被延迟唤醒 |
| 1b: 名字服务+Capsule            | Service Graph bootstrap + Capsule 生命周期                                      | Capsule 通过名字服务发现并调用另一个 Capsule                          |
| 1c: Pager+Memory Object         | 缺页处理 + Pager 崩溃模型                                                       | mmap 缺页正常供页；Pager 崩溃 → Capsule 收到 MEMORY_OBJECT_LOST       |
| 1d: 最小对象存储                | 对象 CRUD + 元数据 + 标签 + 目录树兼容投影                                      | "路径不是唯一真相"                                                    |
| 1e: Package Cell 原型           | 声明式安装/激活/回滚/卸载 + 多版本并存                                          | 安装两个依赖不同版本库的 Cell                                         |
| 1f: 驱动框架原型                | 设备能力句柄 + IOMMU 授权 + IOQueue/IOBuffer + 用户态 MMIO                      | 用户态 NVMe 队列提交/完成；驱动崩溃→撤销 DMA→复位→恢复                |
| 1g: 兼容层                      | Linux 兼容域（类 WSL2 VM）+ 兼容域网关                                          | 兼容域内运行 bash+gcc+编译 C 程序                                     |

## 设计判断标准

1. 消除隐式全局状态？ 2. 权限显式、可审计、可回收？ 3. 同步、异步、取消、背压的边界正确？ 4. 前台交互被保护？ 5. 消除对 PATH/bashrc/profile 依赖？ 6. 强化 Package Cell 和 Capsule？ 7. 兼容性限制在边界上？ 8. 对异构硬件友好？ 9. 遵循 let-it-crash？ 10. 故障可测试、可观测、可诊断？

## 文档索引与归属

### 顶层入口

| 文件                                | 归属                         |
| ----------------------------------- | ---------------------------- |
| [pain-points.md](../pain-points.md) | 问题定义与痛点枚举           |
| [target.md](../target.md)           | 目标、约束、判断标准与阅读线 |
| [glossary.md](../glossary.md)       | 项目自造术语和设计术语       |

### 主线设计

| #   | 文件                                                                 | 归属                         |
| --- | -------------------------------------------------------------------- | ---------------------------- |
| 00  | [00-philosophy.md](../core/00-philosophy.md)                         | 设计立场与顶层原则           |
| 01  | [01-capsule-and-capability.md](../core/01-capsule-and-capability.md) | 运行隔离与能力权限           |
| 02  | [02-communication-fabric.md](../core/02-communication-fabric.md)     | 统一通信基座                 |
| 03  | [03-pager-and-memory.md](../core/03-pager-and-memory.md)             | Pager-backed Memory Object   |
| 04  | [04-driver-and-kernel.md](../core/04-driver-and-kernel.md)           | 内核/驱动边界与 IO 原语      |
| 05  | [05-compute-and-scheduling.md](../core/05-compute-and-scheduling.md) | 调度、计算域、异构资源       |
| 06  | [06-service-graph.md](../core/06-service-graph.md)                   | 服务发现、版本协商、启动     |
| 07  | [07-data-and-filesystem.md](../core/07-data-and-filesystem.md)       | Object Store / Stream 主设计 |
| 08  | [08-package-cell.md](../core/08-package-cell.md)                     | 软件单元、依赖、生命周期     |

### 边界专题

| #   | 文件                                                           | 归属                   |
| --- | -------------------------------------------------------------- | ---------------------- |
| 00  | [00-async-and-mmap.md](./00-async-and-mmap.md)                 | 同步、异步与 mmap 边界 |
| 01  | [01-compatibility.md](./01-compatibility.md)                   | Linux 兼容域           |
| 02  | [02-engineering.md](./02-engineering.md)                       | 工程化、构建、测试     |
| 03  | [03-shell-and-tools.md](./03-shell-and-tools.md)               | Shell 与交互工具       |
| 04  | [04-environment-and-config.md](./04-environment-and-config.md) | 环境与配置管理         |
| 05  | [05-identity-and-accounts.md](./05-identity-and-accounts.md)   | 身份与信任模型         |

### 深挖与参考

| #         | 文件                                          | 归属                       |
| --------- | --------------------------------------------- | -------------------------- |
| 06        | [06-roadmap.md](./06-roadmap.md)              | 路线图与非目标             |
| 00        | [00-fs-vm.md](../deep-dives/00-fs-vm.md)      | FS/VM 深挖材料，不是主规范 |
| reference | [reference/README.md](../reference/README.md) | 驱动、旁路、子系统路径参考 |

全局术语表见 [../glossary.md](../glossary.md)。

## 文档归属原则

为避免同一设计在多个章节各自演化，后续新增内容按下面规则归属：

- **通信、异步请求、事件等待、服务间旁路队列**：归属 [02-communication-fabric.md](../core/02-communication-fabric.md)。其他章节只说明如何使用这些原语。
- **Package Cell、依赖解析、多版本并存、生命周期**：归属 [08-package-cell.md](../core/08-package-cell.md)。环境章节只消费解析结果。
- **运行环境、用户/系统配置、配置服务**：归属 [04-environment-and-config.md](./04-environment-and-config.md)。Shell 章节只描述交互命令。
- **Object Store / Stream 的主设计**：归属 [07-data-and-filesystem.md](../core/07-data-and-filesystem.md)。[00-fs-vm.md](../deep-dives/00-fs-vm.md) 只放调研、论证和细化方案。
- **Pager-backed Memory Object 契约**：归属 [03-pager-and-memory.md](../core/03-pager-and-memory.md)。FS 深挖只讨论它如何被存储服务使用。
- **内核/驱动边界、IOQueue/IOBuffer/Doorbell/Fence**：归属 [04-driver-and-kernel.md](../core/04-driver-and-kernel.md)。`reference/` 下文档只保留参考、矩阵和草案。
- **实现语言、构建、测试、更新**：归属 [02-engineering.md](./02-engineering.md)。不要在工程章节重复具体子系统设计。
