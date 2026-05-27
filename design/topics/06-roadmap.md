# 06 — 路线图与非目标

> 对应 [target.md](../target.md) §5 + §6 + §7，以及 [requirements.md](../requirements.md) 的需求编号。全文档地图见 [outline.md](../outline.md)。

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

需求编号来自 [requirements.md](../requirements.md)：R1 类 FUSE 接入，R2 目录挂载，R3 mmap，R4 zero-copy / low-copy，R5 同步/异步一等，R6 用户态服务热路径，R7 能力权限，R8 兼容域边界，R9 远程资源，R10 Package Cell 生命周期，R11 身份与密钥分层。

## 第一阶段落地顺序

| Phase                 | 需求               | 目标                                                                  | 核心验证                                                    |
| --------------------- | ------------------ | --------------------------------------------------------------------- | ----------------------------------------------------------- |
| 1a: 微内核原语        | R5, R7             | QEMU 中启动内核，任务+Portal/Operation+能力句柄+抢占调度+启动句柄注入 | 两个任务通过 Portal fast call 传递能力句柄                  |
| 1a.5: 异步通信原语    | R5, R6             | Continuation + EventPort/WaitSet + timeout/cancel/late reply          | 一个任务提交异步 Operation，另一个任务延迟完成并唤醒 Future |
| 1b: 名字服务+Capsule  | R7, R10            | Service Graph bootstrap + Capsule 生命周期                            | Capsule 通过名字服务发现并调用另一个 Capsule                |
| 1c: Object Namespace  | R1, R2, R7, R8, R9 | 路径解析 + ProviderRoot + MountBinding + ObjectHandle 缓存与撤销      | native 目录挂载 remote provider；应用拿到统一 ObjectHandle  |
| 1d: MemoryObject      | R3, R4, R9         | 缺页处理 + 纯用户态 Pager / 纯内核 Object Store 两条供页路径          | mmap 缺页正常供页；故障按所选 FS 放置方案处理               |
| 1e: 最小对象存储      | R1, R2, R3         | 对象 CRUD + 元数据 + 标签 + tier-1 tree view；裁决用户态或内核态落地  | OID 与 tree view 正交：身份稳定，命名可导航                 |
| 1e.5: 身份与密钥预留  | R7, R11            | IdentityHandle + Device Owner / Policy Authority + Key Agent 元数据   | PIN 不等于私钥或 root；加密 FS 可表达 key policy            |
| 1f: Package Cell 原型 | R7, R10            | 声明式安装/激活/回滚/卸载 + 多版本并存                                | 安装两个依赖不同版本库的 Cell                               |
| 1g: 驱动框架原型      | R4, R6, R7         | 设备能力句柄 + IOMMU 授权 + IOQueue/IOBuffer + 用户态 MMIO            | 用户态 NVMe 队列提交/完成；驱动崩溃→撤销 DMA→复位→恢复      |
| 1h: 兼容层            | R1, R2, R7, R8     | Linux 兼容域（类 WSL2 VM）+ 兼容域网关                                | 兼容域内运行 bash+gcc+编译 C 程序                           |

## 设计判断标准

1. 消除隐式全局状态？ 2. 权限显式、可审计、可回收？ 3. 支持异步、取消、背压？ 4. 前台交互被保护？ 5. 消除对 PATH/bashrc/profile 依赖？ 6. 强化 Package Cell 和 Capsule？ 7. 兼容性限制在边界上？ 8. 对异构硬件友好？ 9. 遵循 let-it-crash？ 10. 故障可测试、可观测、可诊断？

## 文档地图

完整阅读顺序、文档层级、语义归属和 AI 查漏补缺清单见 [outline.md](../outline.md)。本章只维护第一阶段路线、非目标和阶段验收。
