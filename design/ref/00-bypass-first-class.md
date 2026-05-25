# 00 — 内核旁路作为第一公民

## 讨论范围

本文只回答一个问题：Ousia 是否应该把 kernel bypass 作为第一公民支持。

结论是：**应该，但只能作为受治理的数据面模式。**

这意味着它既不是“以后再做的性能优化”，也不是“允许用户态随意绕开内核”的特权通道。

## 1. 三条路径的边界

Ousia 应明确区分三条路径：

- **Portal / Operation**：控制面、策略面、生命周期管理
- **syscall**：机制面、授权面、仲裁面
- **kernel bypass**：共享队列、注册内存、doorbell、completion、poll/interrupt hybrid 构成的数据面

它们不是互斥关系，而是三种不同层级的系统通路。

### Portal / Operation 的角色

- 设备发现
- 驱动绑定
- 上下文创建
- 资源授权
- 版本协商
- 恢复编排

Portal / Operation 适合易变语义、服务协议和异步控制请求，不适合高频数据面热路径。

### syscall 的角色

- 映射与撤销
- 能力检查
- 等待对象
- IOMMU 管理
- 中断绑定
- reset / isolate / revoke

syscall 适合由内核独占的机制，不适合承载大规模设备策略。

### kernel bypass 的角色

- submission/completion
- buffer/frame 所有权交接
- DMA 可达内存
- doorbell
- poll / interrupt 混合等待
- fence / timeline 同步

旁路的目标是把内核移出**每次数据操作的同步路径**，不是把内核移出系统。

## 2. 为什么不能把旁路当成附加优化

如果 Ousia 只把旁路当成“以后也许会加”，会出现三个后果：

- 高性能驱动各自发明共享环、doorbell、descriptor、frame pool，框架无法统一。
- 用户态驱动虽然获得隔离，但数据面仍然被逐请求 Portal/Operation 往返或 syscall 风暴拖垮。
- FS、网络、存储、GPU 分别长出不同快路径，最后回到历史系统那种 fd/ioctl/mmap/eventfd/dma-buf 杂交局面。

因此，Ousia 更合理的顶层判断是：

- 默认控制面组合模型是 Portal / Operation
- 默认硬仲裁模型是 syscall
- 默认高频数据面模型是 kernel bypass substrate

## 3. 第一公民支持意味着什么

“第一公民支持”不是一句性能口号，而意味着内核和 SDK 都要把旁路对象做成标准能力。

### 内核必须原生提供

- queue substrate
- registered memory substrate
- completion / event substrate
- fence / timeline substrate
- direct / proxy doorbell
- reset / revoke / isolate / device lost
- 队列和映射的 tracing / metrics / timeout 观测点

### SDK 必须原生提供

- ring helper
- descriptor builder
- registered memory allocator / frame pool
- polling / interrupt hybrid runtime
- direct / proxy doorbell 封装
- recovery callback 和 device lost 传播
- 回放、仿真、压测、profiling 工具

如果做不到这些，旁路就不是真正的系统能力，只是个别团队手里的私有技巧。

## 4. 旁路的硬边界

Ousia 必须同时写清楚旁路**不是什么**：

- 不是允许任意应用直接 mmap 全部 BAR
- 不是允许用户态自由绕过 IOMMU、资源预算和调度器
- 不是默认 busy polling 抢占所有 CPU
- 不是把 vendor blob 直接塞进内核之外就算架构现代化

旁路应始终满足以下前提：

- 受 Capability 授权
- 受 IOMMU/SMMU 约束
- 受 queue ownership 和 budget 管理
- 受 power budget 和 execution class 管理
- 有明确的 device lost / revoke / reset 语义

## 5. 对 Ousia 的直接结论

### 正确表达

- Ousia 支持受治理的 kernel bypass substrate。
- 高性能数据面默认优先使用 queue / buffer / event / fence / doorbell。
- Portal / Operation 与 syscall 仍然保留，但分别退回控制面和机制面。

### 错误表达

- Ousia 倾向让用户态尽量直驱硬件。
- Ousia 会提供一批 mmap 和 ioctl，让驱动自己决定如何高性能。
- Ousia 的性能依赖“把东西搬到用户态”。

前两种表达都会让系统重新滑向 ABI 爆炸和快路径失控。

## 6. 接下来要看的文档

- [01-modern-driver-patterns.md](./01-modern-driver-patterns.md)
- [02-driver-sdk-draft.md](./02-driver-sdk-draft.md)
- [03-subsystem-path-matrix.md](./03-subsystem-path-matrix.md)
