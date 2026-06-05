# 06 — 路线图与非目标

> 承接 [target.md](../target.md) 的非目标、落地顺序和设计判断标准，以及 [requirements.md](../requirements.md) 的需求编号。全文档地图见 [outline.md](../outline.md)。

本章当前用于指导第一阶段实现，建议从 `0.5: handle/object 能力合同` 开始切入，再推进 `1a: Ousia capability kernel baseline` 与 `1a.5: IPC/等待原语`。这些 phase 的拆分、先后顺序和边界都仍是草案，后续应根据验证结果继续重构，不要被现有实现反向约束。

第一阶段实现遵循 [工程化复用策略](./02-engineering.md)：积极复用成熟库和现有内核 SDK 经验来降低工程风险，但所有复用都必须服从 Ousia 自己的 capability、通信、pager、驱动和 Package Cell 语义边界。

近期阶段性目标是先做一个 [Ousia kernel architecture baseline](../implementation/00-ousia-kernel-architecture.md)。它不追求形式化验证，但必须用类型边界、不变量、测试和 review 纪律保证足够的工程正确性。Zircon/Fuchsia 是 handle/object、VMO/VMAR、channel/call、driver framework 和用户库人体工程学的主要结构参考；seL4 降级为 capability discipline、硬撤销和失败无副作用的安全参考，不再决定 Phase 1 API 或对象模型。

## 非目标（第一阶段绝对不做）

- **不兼容 POSIX 作为原生接口**——需要 POSIX 的应用通过 Linux 兼容域运行
- **不支持老旧硬件**——BIOS、32 位 CPU、无 IOMMU 的系统
- **不重复造轮子**——不开发浏览器引擎、完整桌面环境、编程语言、完整数据库
- **不开放内核扩展**——没有 LKM，所有驱动在用户态
- **不承诺任意用户态语义委托的全局回滚**——但第 0 层内核可见能力必须支持派生链硬撤销
- **暂不实现完整去中心化账户**——预留身份句柄类型

## 系统分层

```
第 0 层: Ousia capability kernel
  Phase 1 先验证 Ousia 原生高级内核底座：handle table、kernel object manager、
  VM/page allocator、IPC channel/call、process/thread/scheduler、启动句柄注入、
  VFS/Object Namespace 内核边界和资源预算；Communication Fabric、MemoryObject、
  IOMMU/DMA 仲裁和 Object Store 作为内核/服务共同依赖的主线能力同步裁决

第 1 层: 基础系统服务
  名字服务(←内核启动句柄注入)、Capsule 管理器、网络服务、
  设备管理与 Driver Manager/Index/Host、日志与观测；Object Namespace、
  Object Store 和 Pager 可以按边界裁决落在内核或系统服务中

第 2 层: 平台服务
  Package Cell 管理器、图形与窗口系统、策略引擎、兼容域网关

第 3 层: 应用与兼容环境
  原生应用、Linux 兼容域、开发者工具链
```

**Bootstrap**：内核注入初始句柄 → 启动名字服务(第一个用户态进程) → 启动 Capsule 管理器 → 依次启动所有第 1 层服务 → 第 2 层服务 → 用户应用。

## 需求驱动原则

第一阶段路线图以 [requirements.md](../requirements.md) 的硬需求为验收入口。每个阶段都必须能说明它验证了哪条需求；如果一个阶段只验证抽象名称，而不能验证类 FUSE 接入、目录挂载、mmap、zero-copy、同步/异步一等、能力撤销、兼容域边界或 Package Cell 生命周期中的至少一项，它就不应进入第一阶段主线。

需求编号来自 [requirements.md](../requirements.md)：R1 类 FUSE 接入，R2 目录挂载，R3 mmap，R4 zero-copy / low-copy，R5 同步/异步一等，R6 用户态服务热路径，R7 能力权限与分层撤销语义，R8 兼容域边界，R9 远程资源，R10 Package Cell 生命周期，R11 身份与密钥分层。

## 第一阶段落地顺序

| Phase                    | 需求               | 目标                                                                                                          | 核心验证                                                      |
| ------------------------ | ------------------ | ------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| 0.5: handle/object 能力合同       | R7                 | Handle、kernel object、rights 单调性、generation/stale handle、delete/revoke/destroy 和资源预算语义             | 派生只降权；撤销父句柄后代；stale handle 明确失败             |
| 1a: Ousia capability kernel baseline | R5, R7             | QEMU 中启动内核，handle table、object manager、process/thread、scheduler、启动句柄注入和最小 channel/call       | 两个任务通过 channel/call 传递 handle 并验证撤销语义           |
| 1a.5: IPC/等待原语                 | R5, R6             | Portal/Operation/Continuation/EventPort/WaitSet、timeout、cancel、late reply 和 handle transfer                | 一个任务提交 Operation，另一个任务延迟完成并唤醒 Future       |
| 1b: VM/MemoryObject baseline       | R3, R4, R9         | page allocator、kernel heap/slab 边界、VMO/MemoryObject、VMAR/address-space owner、mmap fault path             | mmap 缺页正常供页；分配失败无部分提交                         |
| 1c: Object Namespace + VFS/Object Store | R1, R2, R7, R8, R9 | 路径解析、ProviderRoot、MountBinding、ObjectHandle 缓存与撤销；裁决内核态和服务态边界                          | native 目录挂载 remote provider；应用拿到统一 ObjectHandle    |
| 1d: 名字服务+Capsule               | R7, R10            | Service Graph bootstrap + Capsule 生命周期                                                                    | Capsule 通过名字服务发现并调用另一个 Capsule                  |
| 1e: 身份与密钥预留                 | R7, R11            | IdentityHandle + Device Owner / Policy Authority + Key Agent 元数据                                           | PIN 不等于私钥或 root；加密 FS 可表达 key policy              |
| 1f: Package Cell 原型              | R7, R10            | 声明式安装/激活/回滚/卸载 + 多版本并存                                                                        | 安装两个依赖不同版本库的 Cell                                 |
| 1g: 驱动框架原型                   | R4, R6, R7         | 设备能力句柄 + IOMMU 授权 + IOQueue/IOBuffer + 用户态 MMIO + Driver Manager/Index/Host                       | 用户态 NVMe 队列提交/完成；驱动崩溃→撤销 DMA→复位→恢复        |
| 1h: 兼容层                         | R1, R2, R7, R8     | Linux Compatibility Domain（类 WSL2 VM）+ 兼容域网关                                                         | 兼容域内运行 bash+gcc+编译 C 程序                             |

## 设计判断标准

1. 消除隐式全局状态？ 2. 内核可见权限显式、可审计、可硬撤销，服务语义授权可失效？ 3. 支持异步、取消、背压？ 4. 前台交互被保护？ 5. 消除对 PATH/bashrc/profile 依赖？ 6. 强化 Package Cell 和 Capsule？ 7. 兼容性限制在边界上？ 8. 对异构硬件友好？ 9. 遵循 let-it-crash？ 10. 故障可测试、可观测、可诊断？

## 文档地图

完整阅读顺序、文档层级、语义归属和 AI 查漏补缺清单见 [outline.md](../outline.md)。本章只维护第一阶段路线、非目标和阶段验收。
