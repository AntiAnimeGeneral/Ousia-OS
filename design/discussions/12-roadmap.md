# 12 — 路线图与非目标

> 对应 `target.md` §5 + §6 + §7 + §8

## 讨论范围

本文是设计系列的最后一份，整合了系统分层、第一阶段落地顺序、非目标声明和设计判断标准。它也可以作为未来实现时的参考索引。

---

## 非目标：哪些事第一阶段绝对不做

这些是**明确排除**的，目的是防止范围蔓延：

### 不兼容 POSIX 作为原生接口

POSIX 语义（路径、fork、信号、文件描述符）不会出现在原生 API 中。需要 POSIX 的应用通过 Linux 兼容域运行。

**原因**：POSIX 的语义假设（路径即身份、fork/exec 模型、信号 = 异步中断）与 xos 的核心抽象直接冲突。在原生层提供 POSIX 兼容会迫使原生抽象向 POSIX 妥协。

### 不支持老旧硬件

BIOS、32 位 CPU、无 IOMMU 的系统是第一阶段的硬排除。这确保能力模型不需要为缺少硬件隔离支持而妥协。

**潜在例外**：如果在开发社区中有足够的兴趣，未来可以为特定嵌入式场景做裁剪版（如 Raspberry Pi 5，它有 SMMU），但这不在第一阶段。

### 不重复造轮子

第一阶段不开发自己的：

- 浏览器引擎（使用现有的，通过兼容域）
- 完整的桌面环境（先做窗口系统 + 基础 Shell）
- 编程语言和编译器（使用现有的 LLVM/Rust 工具链）
- 完整数据库（Object Store 不是 SQL 引擎）

### 不开放不受控的内核扩展

不提供类似 Linux 的 loadable kernel module (LKM) 机制。所有驱动必须在用户态。内核不可动态扩展。

**原因**：LKM 是内核安全漏洞的主要入口。xos 的内核应该是"小而不可变"的。

### 暂不实现完整的能力级联撤销

第一阶段的能力撤销仅支持直接授予的沿链失效。完整的跨 Capsule 级联追踪和 Capability Broker 在第二阶段。

### 暂不实现完整去中心化账户

账户体系在平台服务阶段（第二阶段）实现。第一阶段预留身份句柄类型。

---

## 系统分层

### 总览

```
第 3 层: 应用与兼容环境
  ├── 原生应用
  ├── 系统组件
  ├── Linux 兼容域
  └── 开发者工具链

第 2 层: 平台服务
  ├── Package Cell 管理器
  ├── 图形与窗口系统
  ├── 数据同步服务
  ├── 策略引擎
  ├── 兼容域网关
  └── 账户与身份同步 (后期)

第 1 层: 基础系统服务
  ├── 名字服务 (Bootstrap 入口)
  ├── Capsule 管理器
  ├── 资源治理与 QoS
  ├── 对象存储服务
  ├── 网络服务
  ├── 设备管理服务
  ├── Driver Manager / Index / Host
  ├── 日志与观测服务
  ├── 身份与授权服务
  └── Pager 监督服务

第 0 层: 微内核
  ├── 调度与执行等级
  ├── 地址空间管理
  ├── IPC 与能力句柄
  ├── 中断与时钟
  ├── IOMMU / DMA 仲裁
  ├── Pager-backed Memory Object
  └── 启动句柄注入
```

### Bootstrap 顺序

```
0. 固件 (UEFI) → 加载内核
1. 内核初始化基本数据结构 → 创建启动句柄集合
2. 内核启动第一个用户态进程 (init/bootstrap)
   → 通过启动句柄注入:
     - handle_naming_service
     - handle_initial_memory
     - handle_kernel_channel
3. init 注册自身为 "naming" 服务
4. init 启动 Capsule 管理器
5. Capsule 管理器依次启动所有第 1 层服务
6. 所有第 1 层服务就绪 → 系统进入 ready 状态
7. 第 2 层服务启动
8. 用户应用启动
```

---

## 第一阶段落地顺序

### 总原则

- 每一步都产出可运行、可测试的工件
- 不追求一步到位——每个组件先做 MVP，再迭代
- 关键路径（Pager、能力、IPC）必须最先验证

### 详细顺序

#### Phase 1a: 微内核最小原语

**目标**：在 QEMU 中启动内核，能创建任务、传递消息、管理能力句柄。

**内容**：

- 启动代码（AArch64 / x86-64）
- 物理内存管理（页框分配器）
- 虚拟内存管理（页表、地址空间）
- 基础调度器（抢占式、多优先级）
- IPC 通道（同步消息传递）
- 能力句柄（创建、派生、传递、直接撤销）
- 中断处理（定时器中断 = 调度时钟）
- 启动句柄注入

**验证**：两个任务能通过 IPC 传递能力句柄。

#### Phase 1b: 名字服务和 Capsule 管理器

**目标**：服务图 bootstrap 可用，Capsule 可以被创建和销毁。

**内容**：

- 名字服务（注册、解析、版本协商）
- Capsule 管理器（创建地址空间、授予初始能力）
- Capsule 生命周期（create → start → stop → destroy）

**验证**：启动一个 Capsule，它能通过名字服务发现并调用另一个 Capsule。

#### Phase 1c: Pager-backed Memory Object

**目标**：验证缺页处理和 Pager 崩溃模型。

**内容**：

- Memory Object 创建/映射/销毁
- 缺页处理（内核捕获 → Pager 请求 → 填充页表）
- Pager 超时检测和故障处理
- 脏页跟踪和回写通知
- 共享映射和写时复制

**验证**：

1. Capsule 使用 mmap 访问一个 Memory Object，Pager 正常供页 → 无缺页感知
2. 强制 Pager 崩溃 → Capsule 收到 MEMORY_OBJECT_LOST → 终止

#### Phase 1d: 最小对象存储服务

**目标**：验证"数据不以路径为唯一标识"。

**内容**：

- Object Store 服务（基于 Pager-backed Memory Object）
- 对象创建/读取/写入/删除
- 基础元数据和标签
- 原子对象替换
- 目录树兼容投影（纯用户态实现）

**验证**：

1. 创建对象 "my-doc" → 获得 ObjectID
2. 通过 ObjectID 读回内容 → 正确
3. 通过兼容投影路径 `/objects/my-doc` 也能访问 → 投影层工作

#### Phase 1e: Package Cell 原型

**目标**：跑通声明式软件交付的基本流程。

**内容**：

- Cell 格式定义和解析
- 内容寻址存储
- 依赖解析（确定性算法）
- 多版本并存
- 安装/激活/回滚/卸载
- 发布者签名验证（预留）

**验证**：

1. 安装 Cell "hello-app"（依赖 "libc v1.0"）
2. 安装 Cell "another-app"（依赖 "libc v2.0"）→ 两个版本并存
3. 升级 "hello-app" → 原子切换 → 回滚 → 卸载干净

#### Phase 1f: 驱动框架最小原型

**目标**：验证用户态驱动模型的基本通路。

**内容**：

- 设备能力句柄
- Driver Manager / Driver Index / Driver Host
- IOMMU 授权（在 QEMU 中模拟 IOMMU）
- MMIO BAR 映射（用户态直接访问）
- 中断路由到用户态事件对象
- 驱动崩溃恢复

**验证**：

1. 加载一个最小 NVMe 驱动（用户态）→ 读写块设备 → 正确
2. 强制驱动崩溃 → 设备被复位 → 驱动重启 → 恢复

#### Phase 1g: 兼容层

**目标**：能在 xos 上运行简单的 Linux 程序。

**内容**：

- Linux 兼容域（轻量 VM）
- 兼容域网关（文件/窗口/网络/剪贴板转换）
- 常见 Linux 发行版的 rootfs 支持

**验证**：在兼容域中运行 `bash` + `gcc` + 编译一个简单的 C 程序。

---

## 设计判断标准

每个设计决策都应该用这些问题过滤。回答"否"多于 3 个的设计需要重新考虑。

| #   | 问题                                      | 对应原则        |
| --- | ----------------------------------------- | --------------- |
| 1   | 是否消除了隐式全局状态？                  | 显式声明优先    |
| 2   | 权限是否显式、可审计、可回收？            | Capability 模型 |
| 3   | 是否默认支持异步、取消和背压？            | 异步优先        |
| 4   | 前台交互与关键任务是否被保护？            | 执行等级        |
| 5   | 是否消除了对 PATH/bashrc/profile 的依赖？ | 运行环境封装    |
| 6   | 是否强化了 Package Cell 和 Capsule？      | 核心抽象        |
| 7   | 兼容性是否限制在边界上？                  | 兼容层隔离      |
| 8   | 是否对异构硬件（含电源管理）友好？        | 异构一等公民    |
| 9   | 是否遵循 let-it-crash 故障模型？          | 原则 13         |
| 10  | 故障模式是否可测试、可观测、可诊断？      | 工程化          |

---

## 文档索引

本系列 13 份讨论文档的索引：

| 文件                                                           | 对应 target.md                       | 主题                       |
| -------------------------------------------------------------- | ------------------------------------ | -------------------------- |
| [00-philosophy.md](./00-philosophy.md)                         | §0 + §2                              | 反 Unix 立场与设计总纲     |
| [01-pain-points.md](./01-pain-points.md)                       | §1                                   | 现代软件栈核心痛点         |
| [02-package-cell.md](./02-package-cell.md)                     | §3.1 + §4.1                          | 软件单元与依赖管理         |
| [03-capsule-and-capability.md](./03-capsule-and-capability.md) | §3.2 + §3.3 + §4.2                   | 沙盒与能力模型             |
| [04-service-graph.md](./04-service-graph.md)                   | §3.4                                 | 服务图与 Bootstrap         |
| [05-data-and-filesystem.md](./05-data-and-filesystem.md)       | §3.5 + §4.6                          | 数据抽象与文件系统         |
| [06-pager-and-memory.md](./06-pager-and-memory.md)             | §3.7                                 | Pager-backed Memory Object |
| [07-compute-and-scheduling.md](./07-compute-and-scheduling.md) | §3.6 + §4.4 + §4.12                  | 计算域、调度、异构硬件     |
| [08-driver-and-kernel.md](./08-driver-and-kernel.md)           | §4.3 + §4.9                          | 内核原语与驱动框架         |
| [09-async-model.md](./09-async-model.md)                       | §4.5                                 | 异步模型与 mmap 张力       |
| [10-compatibility.md](./10-compatibility.md)                   | §4.7 + §4.8                          | Linux 兼容与账户           |
| [11-engineering.md](./11-engineering.md)                       | §2.3 + §4.10 + §4.11 + §4.13 + §4.14 | 工程化基础设施             |
| [12-roadmap.md](./12-roadmap.md)                               | §5 + §6 + §7 + §8                    | 路线图与非目标             |
| [13-fs-vm-deep-dive.md](./13-fs-vm-deep-dive.md)               | §3.5 + §3.7 + §4.6                   | 文件系统与 VM 深度设计     |
