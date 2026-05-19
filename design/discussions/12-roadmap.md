# 12 — 路线图与非目标

> 对应 `target.md` §5 + §6 + §7 + §8

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
  调度+执行等级、地址空间+页表、IPC+能力句柄、中断+时钟、
  IOMMU/DMA 仲裁、Pager-backed Memory Object、启动句柄注入

第 1 层: 基础系统服务
  名字服务(←内核启动句柄注入)、Capsule 管理器、对象存储服务、
  网络服务、Driver Manager/Index/Host、日志与观测、Pager 监督服务

第 2 层: 平台服务
  Package Cell 管理器、图形与窗口系统、策略引擎、兼容域网关

第 3 层: 应用与兼容环境
  原生应用、Linux 兼容域、开发者工具链
```

**Bootstrap**：内核注入初始句柄 → 启动名字服务(第一个用户态进程) → 启动 Capsule 管理器 → 依次启动所有第 1 层服务 → 第 2 层服务 → 用户应用。

## 第一阶段落地顺序

| Phase                   | 目标                                                     | 核心验证                                                        |
| ----------------------- | -------------------------------------------------------- | --------------------------------------------------------------- |
| 1a: 微内核原语          | QEMU 中启动内核，任务+IPC+能力句柄+抢占调度+启动句柄注入 | 两个任务通过 IPC 传递能力句柄                                   |
| 1b: 名字服务+Capsule    | Service Graph bootstrap + Capsule 生命周期               | Capsule 通过名字服务发现并调用另一个 Capsule                    |
| 1c: Pager+Memory Object | 缺页处理 + Pager 崩溃模型                                | mmap 缺页正常供页；Pager 崩溃 → Capsule 收到 MEMORY_OBJECT_LOST |
| 1d: 最小对象存储        | 对象 CRUD + 元数据 + 标签 + 目录树兼容投影               | "路径不是唯一真相"                                              |
| 1e: Package Cell 原型   | 声明式安装/激活/回滚/卸载 + 多版本并存                   | 安装两个依赖不同版本库的 Cell                                   |
| 1f: 驱动框架原型        | 设备能力句柄 + IOMMU 授权 + 用户态 MMIO                  | 用户态 NVMe 驱动读写；驱动崩溃→复位→恢复                        |
| 1g: 兼容层              | Linux 兼容域（类 WSL2 VM）+ 兼容域网关                   | 兼容域内运行 bash+gcc+编译 C 程序                               |

## 设计判断标准

1. 消除隐式全局状态？ 2. 权限显式、可审计、可回收？ 3. 支持异步、取消、背压？ 4. 前台交互被保护？ 5. 消除对 PATH/bashrc/profile 依赖？ 6. 强化 Package Cell 和 Capsule？ 7. 兼容性限制在边界上？ 8. 对异构硬件友好？ 9. 遵循 let-it-crash？ 10. 故障可测试、可观测、可诊断？

## 文档索引

| #   | 文件                                                           | 主题                       |
| --- | -------------------------------------------------------------- | -------------------------- |
| 00  | [00-philosophy.md](./00-philosophy.md)                         | 反 Unix 立场与设计总纲     |
| 01  | [01-pain-points.md](./01-pain-points.md)                       | 现代软件栈核心痛点         |
| 02  | [02-package-cell.md](./02-package-cell.md)                     | 软件单元与依赖管理         |
| 03  | [03-capsule-and-capability.md](./03-capsule-and-capability.md) | 沙盒与能力模型             |
| 04  | [04-service-graph.md](./04-service-graph.md)                   | 服务图与 Bootstrap         |
| 05  | [05-data-and-filesystem.md](./05-data-and-filesystem.md)       | 数据抽象与文件系统         |
| 06  | [06-pager-and-memory.md](./06-pager-and-memory.md)             | Pager-backed Memory Object |
| 07  | [07-compute-and-scheduling.md](./07-compute-and-scheduling.md) | 计算域、调度、异构硬件     |
| 08  | [08-driver-and-kernel.md](./08-driver-and-kernel.md)           | 内核原语与驱动框架         |
| 09  | [09-async-model.md](./09-async-model.md)                       | 异步模型与 mmap 张力       |
| 10  | [10-compatibility.md](./10-compatibility.md)                   | Linux 兼容                 |
| 11  | [11-engineering.md](./11-engineering.md)                       | 工程化基础设施             |
| 12  | [12-roadmap.md](./12-roadmap.md)                               | 路线图与非目标             |
| 13  | [13-fs-vm-deep-dive.md](./13-fs-vm-deep-dive.md)               | 文件系统与 VM 深度设计     |
| 14  | [14-shell-and-tools.md](./14-shell-and-tools.md)               | Shell 与交互环境           |
| 15  | [15-environment-and-deps.md](./15-environment-and-deps.md)     | 环境管理与依赖解析         |
| 16  | [16-identity-and-accounts.md](./16-identity-and-accounts.md)   | 身份与信任模型             |
