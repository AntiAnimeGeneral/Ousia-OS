# Ousia OS 总纲

本文档是 Ousia OS 设计文档的总纲。它只回答五件事：现有系统哪里错了，Ousia OS 想建立什么秩序，第一阶段必须满足哪些硬需求，需求如何推出核心抽象，以及这些目标和抽象分别由哪些主线章节承接。完整文档地图见 [outline.md](./outline.md)，完整需求库和抽象推导见 [requirements.md](./requirements.md)。

项目自造术语和重新定义过的设计术语见 [glossary.md](./glossary.md)。除非特别说明，Portal、Operation、Continuation、Communication Fabric 等词都是 Ousia OS 的设计术语，不指代某个现有系统的专有技术。

本文作为总纲入口，约束各专题文档的愿景边界、需求边界和抽象边界。随需求增长而扩展的内容应进入 [requirements.md](./requirements.md)，文档归属和查漏补缺规则由 [outline.md](./outline.md) 维护。

## 阅读顺序

Ousia OS 的组织逻辑应从问题开始，而不是从抽象开始：

1. 先读 [pain-points.md](./pain-points.md)，理解现代软件栈的核心痛点。
2. 再读本文的目标摘要和判断标准，确认系统为什么要做、第一阶段必须满足什么。
3. 需要追踪需求和抽象推导时，读 [requirements.md](./requirements.md)。
4. 按 [outline.md](./outline.md) 的阅读路径进入主线设计，再用 [06-roadmap.md](./topics/06-roadmap.md) 查看第一阶段落地顺序。

## 1. 痛点

Ousia OS 的设计不是为了“重写一个 Unix”，而是因为现有系统在现代软件栈下暴露出一组结构性问题。

### 1.1 依赖、安装与分发失控

系统包管理器、语言包管理器、容器镜像和 shell 环境彼此割裂。安装、升级、卸载、回滚、多版本并存和钻石依赖处理缺少统一系统模型，最终由用户、脚本和约定承担复杂度。

主线章节：[08-package-cell.md](./core/08-package-cell.md)，边界专题：[04-environment-and-config.md](./topics/04-environment-and-config.md)。

### 1.2 默认权限模型过宽

传统进程默认继承用户身份、全局文件系统视图、网络能力和大量环境状态。沙盒通常是额外补丁，而不是系统的默认运行方式。

主线章节：[01-capsule-and-capability.md](./core/01-capsule-and-capability.md)。

### 1.3 同步阻塞与粗糙调度不适合现代交互系统

现代工作负载高度并发，CPU、GPU、NPU、IO 队列和内存带宽都会成为竞争资源。现有系统很难统一表达取消、超时、背压、前台保活、实时约束和跨设备资源预算。

主线章节：[05-compute-and-scheduling.md](./core/05-compute-and-scheduling.md)，通信基础见 [02-communication-fabric.md](./core/02-communication-fabric.md)。

### 1.4 文件系统抽象落后于现代数据使用方式

目录树是必须保留的一等命名和导航抽象，但“路径字符串 + 字节流文件”不应独占原生数据模型。现代应用需要稳定对象 ID、tree view、元数据、索引、版本、事务、流和配置语义共同表达数据。

主线章节：[07-data-and-filesystem.md](./core/07-data-and-filesystem.md)，VM 细节见 [03-pager-and-memory.md](./core/03-pager-and-memory.md)。

### 1.5 兼容性经常污染原生设计

如果把 POSIX、`/dev`、fork/exec/pipe、路径权限和全局环境直接压进原生 API，系统会从一开始被历史抽象锁死。但完全不兼容又无法承接现有生态。

边界专题：[01-compatibility.md](./topics/01-compatibility.md)。

### 1.6 异构硬件已经是常态

现代硬件不再是“均质 CPU + 外设”。GPU、NPU、DSP、SmartNIC、大小核、电源状态、设备内存和 DMA 隔离都需要进入统一资源模型。

主线章节：[05-compute-and-scheduling.md](./core/05-compute-and-scheduling.md)，驱动与硬件边界见 [04-driver-and-kernel.md](./core/04-driver-and-kernel.md)。

### 1.7 抽象边界不能牺牲关键路径性能

用户态服务、微内核、能力模型和异步接口都不能成为性能借口。高频控制面、异步请求、大数据路径和设备 fast path 必须各自走最低成本路径。

主线章节：[02-communication-fabric.md](./core/02-communication-fabric.md)，旁路和驱动路径见 [04-driver-and-kernel.md](./core/04-driver-and-kernel.md)。

完整展开见 [pain-points.md](./pain-points.md)。

## 2. 目标、需求与抽象推导

本节按四层组织：**愿景目标**说明系统想建立什么秩序，**硬需求摘要**说明第一阶段必须被验证的能力类别，**抽象推导摘要**说明哪些抽象是被需求强制推出的，**设计约束**说明做取舍时不能越过的边界。完整需求和推导索引见 [requirements.md](./requirements.md)。

### 2.1 愿景目标

Ousia OS 的目标是建立一套新的默认秩序：软件以声明式单元交付，运行默认受能力约束，系统以服务图组织，数据拥有语义，身份去中心化且不等同于权限，通信和调度原生支持同步调用、异步 Operation、等待、取消、背压和优先级传播，兼容性被限制在边界上。

| 目标域     | 顶层目标                                                                                        | 主线承接                                                |
| ---------- | ----------------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| 软件交付   | 软件交付、依赖解析、运行环境、服务生命周期和回滚由系统统一管理                                  | Package Cell、Service Graph                             |
| 权限与隔离 | Capsule 默认无权限，所有资源访问都通过 Capability 显式授予、传递和回收                          | Capsule、Capability                                     |
| 身份与信任 | Identity 证明主体，Device Owner / Policy Authority 替代传统 root，PIN 只解锁本地 Key Agent      | Identity、Key Agent                                     |
| 系统组织   | Service Graph 替代全局命名空间成为原生系统组织方式                                              | Service Graph                                           |
| 通信与等待 | Communication Fabric 统一同步调用、异步请求、事件等待和高吞吐数据面                             | Portal、Operation、Continuation、EventPort、SharedQueue |
| 数据与 VM  | Object Namespace、Object Store、Stream 和 MemoryObject 构成原生命名、数据与 VM 模型             | Object Namespace、MemoryObject                          |
| 计算与调度 | Compute Domain 和 Execution Class 统一描述异构计算、实时性、交互性、吞吐和功耗预算              | Compute Domain、Execution Class                         |
| 驱动与硬件 | 驱动主逻辑默认在用户态运行，内核只提供隔离、仲裁、复位、IOMMU/DMA、MMIO 授权和 fast-path assist | Hardware Core、Driver Host                              |
| 兼容性     | Linux/POSIX 兼容通过 Compatibility Domain 承接，不污染原生 API                                  | Compatibility Domain                                    |

### 2.2 第一阶段硬需求摘要

硬需求是第一阶段验收条件，不是实现细节偏好。完整需求编号、验收条件和主线承接见 [requirements.md](./requirements.md)。总纲只保留需求类别：

- 存储接入：类 FUSE provider、目录挂载、远程 FS、兼容投影。
- VM 与数据路径：`mmap`、Remote-backed MemoryObject、zero-copy / low-copy、共享队列和 DMA 授权。
- 通信与热路径：同步/异步一等、Portal fast call、batch、bypass session、late reply。
- 权限与兼容：Capability 授权、审计、撤销，以及 POSIX 兼容域边界。
- 身份与密钥：去中心化 Identity、设备所有权、Key Agent、加密 FS key policy 的元数据预留。
- 软件生命周期：Package Cell 安装、升级、回滚、多版本并存和环境声明化。

### 2.3 抽象推导摘要

硬需求的作用是压缩设计空间。Ousia 的核心抽象应从需求组合中推出，而不是从现有系统名词复制而来。完整推导表和结论落点见 [requirements.md](./requirements.md)。当前已经接受的主线结论是：

- `类 FUSE 接入 + 目录挂载 + mmap` 推出 Object Namespace、FS Provider、ObjectHandle 和 MemoryObject。
- `类 FUSE 接入 + 目录挂载 + POSIX 隔离` 推出 Object/Provider/Capability VFS-like 层，而不是 POSIX VFS。
- `mmap + zero-copy + 远程资源` 推出 Remote-backed MemoryObject；CPU fault 不能直接等价为任意远程 RPC。
- `zero-copy + 用户态热路径 + Capability` 推出受授权的 MemoryDescriptor、IOBuffer、SharedQueue 和 IOMMU 映射。
- `同步/异步一等 + 用户态热路径` 推出 Portal fast call、Operation、Continuation、EventPort 和 bypass session。
- `Capability + 身份密钥分层` 推出 Identity、Device Owner / Policy Authority、Key Agent、FSKeyPolicy 和 WrappedKey。

### 2.4 设计约束

- 正确抽象优先于复刻 Unix。
- 显式声明优先于隐式约定。
- 同步和异步都是一等抽象，按语义、延迟、取消边界和数据路径成本选择。
- 前台交互、实时任务和关键系统服务必须有保活语义。
- 性能是一级约束，抽象边界不能让关键路径退化。
- 机制与策略分离，但不能为了形式上的分离破坏热路径。
- 可复现、可回滚、可审计是系统级能力。
- 故障模型遵循“能恢复则恢复，不能恢复或代价太大则快速失败并由上层监督者重启”。

设计立场和原则的展开见 [00-philosophy.md](./core/00-philosophy.md)。

## 3. OS 基建原语与服务实现

Ousia OS 的主线设计先定义内核与系统服务共同依赖的低层原语，再说明这些原语如何组合成平台服务。

### 3.1 OS 基建原语

| 抽象                        | 作用                                                                        | 主线章节                                                            |
| --------------------------- | --------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| Capsule                     | 运行隔离域，包含地址空间、线程、能力集合和资源预算                          | [01-capsule-and-capability.md](./core/01-capsule-and-capability.md) |
| Capability                  | 不可伪造的权限句柄，绑定对象和操作                                          | [01-capsule-and-capability.md](./core/01-capsule-and-capability.md) |
| Communication Fabric        | Portal、Operation、Continuation、EventPort、SharedQueue、Fence 等通信原语族 | [02-communication-fabric.md](./core/02-communication-fabric.md)     |
| Object Namespace / Store    | 统一路径解析、Provider 挂载、持久对象、元数据、索引、版本和流式 IO          | [07-data-and-filesystem.md](./core/07-data-and-filesystem.md)       |
| MemoryObject / Pager        | 可分页内存对象；支持用户态 Pager 或内核 Object Store 供页                   | [03-pager-and-memory.md](./core/03-pager-and-memory.md)             |
| Hardware Core / Driver Host | 最小硬件仲裁层与用户态驱动宿主                                              | [04-driver-and-kernel.md](./core/04-driver-and-kernel.md)           |
| Compute Domain              | 异构计算资源、执行等级和功耗预算模型                                        | [05-compute-and-scheduling.md](./core/05-compute-and-scheduling.md) |

### 3.2 服务实现

| 抽象          | 作用                                   | 主线章节                                                      |
| ------------- | -------------------------------------- | ------------------------------------------------------------- |
| Service Graph | 服务发现、版本协商、启动和系统组织模型 | [06-service-graph.md](./core/06-service-graph.md)             |
| FS Provider   | 用户态/远程/加密/同步存储接入协议      | [07-data-and-filesystem.md](./core/07-data-and-filesystem.md) |
| Package Cell  | 软件交付、依赖、环境和生命周期单元     | [08-package-cell.md](./core/08-package-cell.md)               |

## 4. 系统分层

Ousia OS 的第一阶段分层如下，详细路线见 [06-roadmap.md](./topics/06-roadmap.md)。

| 层级                    | 内容                                                                                                                                           |
| ----------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| 第 0 层：微内核         | 调度、地址空间、能力句柄、Communication Fabric、IOMMU/DMA、MemoryObject、启动句柄注入；纯内核态 FS 方案还包含 Object Store 核心                |
| 第 1 层：基础系统服务   | 名字服务、Object Namespace、Capsule 管理器、网络、设备管理、Driver Manager/Index/Host、日志与观测；纯用户态 FS 方案还包含对象存储与 Pager 监督 |
| 第 2 层：平台服务       | Package Cell 管理器、图形与窗口系统、策略引擎、兼容域网关、身份与同步服务                                                                      |
| 第 3 层：应用与兼容环境 | 原生应用、Linux 兼容域、开发者工具链                                                                                                           |

## 5. 非目标

第一阶段明确不追求：

- 不把 POSIX 作为原生接口。
- 不支持 BIOS、32 位 CPU、无 IOMMU/SMMU 的老旧硬件。
- 不自研浏览器引擎、完整桌面环境、编程语言或完整数据库。
- 不开放不受控内核扩展接口。
- 不承诺完整能力级联撤销和任意跨 Capsule 转发追踪。
- 不实现完整去中心化账户体系，只预留身份句柄和平台服务位置。

## 6. 落地顺序

第一阶段应优先验证系统最核心的闭环：

1. 微内核原语：任务、地址空间、能力句柄、Portal/Operation、抢占调度、启动句柄注入。
2. Communication Fabric 最小闭环：Portal fast call、Continuation、EventPort/WaitSet、timeout、cancel、late reply。
3. Service Graph bootstrap 与 Capsule 生命周期。
4. Object Namespace：路径解析、ProviderRoot、MountBinding、ObjectHandle 解析缓存和撤销。
5. MemoryObject、缺页处理，以及纯用户态 Pager / 纯内核 Object Store 两条供页路径。
6. 最小 Object Store 与 tier-1 tree view；裁决其作为用户态服务还是内核原语落地。
7. Package Cell 安装、激活、回滚、卸载和多版本并存。
8. 用户态驱动框架：设备能力句柄、IOMMU 授权、IOQueue/IOBuffer、用户态 MMIO。
9. Linux Compatibility Domain 与兼容域网关。

## 7. 设计判断标准

后续每个重要设计都应通过这些问题过滤：

1. 是否消除了隐式全局状态？
2. 权限是否显式、可审计、可回收？
3. 是否为同步、异步、等待、取消、超时和背压选择了正确边界？
4. 前台交互和关键任务是否被保护？
5. 是否消除了对 PATH、bashrc、profile 等全局环境拼装的依赖？
6. 是否强化 Package Cell、Capsule、Capability 和 Service Graph，而不是绕开它们？
7. 兼容性是否被限制在边界上？
8. 对异构硬件、设备隔离、功耗预算和用户态驱动是否友好？
9. 是否遵循可恢复则恢复、不可恢复则快速失败的故障模型？
10. 故障模式是否可测试、可观测、可诊断？

如果多数答案是否定的，这个设计大概率仍在重复旧系统的问题。

## 8. 参考项目

Ousia OS 受以下项目启发，但不复刻其中任何一个：

- Fuchsia：微内核、组件化、能力模型、用户态驱动框架。
- seL4：能力系统和形式化验证经验。
- Asterinas：Rust 内核框架与 safe/unsafe 边界组织。
- Windows WDDM：用户态厂商主驱动与内核调度/内存管理层分工。
- Apple DriverKit：受签名、受授权、可升级的用户态驱动扩展。
- SPDK / io_uring：高性能用户态 IO 与内核旁路路径的工程经验。
