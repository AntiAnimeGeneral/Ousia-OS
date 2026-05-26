# 05 — 计算域、调度与异构硬件

> 承接 [target.md](../target.md) 中的 Compute Domain、执行等级、交互保活与异构资源目标。

## Compute Domain：统一异构资源描述

传统 OS 把 CPU (`taskset`)、GPU（厂商私有调度）、NPU/DSP（无 OS 级调度）当独立资源管理，各有各的 API、各有各的调度器，彼此不可见。结果：一个 NPU 推理任务和 GPU 渲染任务无法在系统层做联合调度——即使它们共享同一块 SoC 的内存带宽和功耗预算。

Ousia OS 用 Compute Domain 统一描述所有计算后端：类型、拓扑、能力（SIMD/FP64/Tensor）、本地内存大小与带宽、缓存一致性模型、功耗参数（base/boost 频率 + 功耗域）、抢占粒度。

应用声明任务需求（`compute_type: "tensor-inference", latency_target: "10ms", power_budget: "low"`），系统按 Compute Domain 和执行等级自动路由。路由决策不考虑原始算力——数据移动成本也是输入：如果图片已经在 GPU 显存中，CPU 推理需要先搬数据，总延迟可能比 GPU 推理更高。调度器将此纳入 cost model。

## 执行等级：五级语义不是 nice 值

Linux nice 值 (-20~+19) 只是一个调度权重。它不表达"我需要最低 10% GPU"，不表达"我可以被暂停但不能被杀死"，不表达"请优先大核"。Ousia OS 的执行等级是带语义的声明：

| 等级  | 延迟保证   | 资源策略                           | 功耗   | 示例                      |
| ----- | ---------- | ---------------------------------- | ------ | ------------------------- |
| RT    | 硬实时有界 | 预分配，不可抢占                   | 无上限 | 工业控制                  |
| INT   | 软实时     | 保底 CPU/GPU 预算，允许 boost      | 高     | GUI 合成器、音频 pipeline |
| FG    | 感知延迟   | 公平份额，优先于后台               | 中     | 浏览器、编辑器            |
| BG    | 不保证     | 剩余资源，可暂停/降级，优先 E-core | 低     | 编译、备份                |
| MAINT | 最宽松     | 仅空闲时运行                       | 最低   | GC、索引构建              |

## 交互保活：机制而非希望

这是 Ousia OS 调度设计中最核心的工程保证。Linux 上编译 Chromium 导致鼠标卡顿不是"CPU 不够快"——是调度器把 100 个 `cc1` 进程和 GUI 合成器放在同一优先级竞争。

Ousia OS 的机制：

1. **预算预留**：INT 等级的保底份额（如总 CPU 的 30%）不参与公平竞争。无论有多少 BG 任务，INT 总能拿到这部分。这不是"高优先级先跑"——是先确保 INT 的预算不会被 BG 吞掉。

2. **立即抢占**：INT 可以抢占 BG。调度目标不应只写成一个全局数字，而应拆成可测指标：CPU runnable latency、IRQ-to-thread latency、关键路径 frame deadline miss rate、IO tail latency。长期目标可以向亚毫秒甚至百微秒级靠近，但第一阶段应先保证指标可观测、可回归，并确保用户态 Pager 的缺页处理不会无限阻塞 INT 线程。

3. **关键路径识别**：输入事件 → 窗口合成 → 显示这条链上的任务标记为 CRITICAL。调度器确保它们在每次帧周期（如 16.6ms for 60Hz）内获得足够的执行窗口，不因 BG 任务的 CPU/GPU/IO 竞争而丢帧。

4. **背压传导**：BG 任务大量 IO 时不填满 IO 队列——IO 调度器将背压传导至任务本身，使其降速。这阻止了"BG 备份任务打满磁盘 IO → GUI 应用读配置文件被卡住"的连锁反应。

## 异构资源调度

GPU 调度不是黑盒。Ousia OS 理解 GPU 内部多引擎（Graphics/Compute/Copy/Video Decode/Encode），支持引擎级并发——一个 Capsule 用 Compute 引擎做推理，另一个用 Graphics 引擎做渲染，它们应真正并行而非时分复用。

抢占粒度取决于硬件能力：支持 thread-level preemption 的 GPU 上 INT 可直接打断 BG；不支持时降级为 draw-call 边界抢占。VRAM 满时按执行等级决定换出顺序（BG 任务先被换出）。

**数据位置感知**：一个推理任务如果数据已在 GPU 显存中，优先 GPU 执行；如果需要 CPU→GPU 搬运，搬运成本计入总延迟后与其他后端比较。这是调度器和内存管理器的联合决策。

## 电源管理：调度的一等维度

执行等级携带功耗语义：INT 允许 boost，BG 限功耗上限 + 优先 E-core。系统电源状态（CPU idle、GPU 降频、设备休眠）与调度器共用同一份资源模型——不是独立的外挂 governor。Compute Domain 的功耗目标声明同时影响调度路由和设备电源状态选择。

## 开放问题

1. GPU 硬件不支持 fine-grained preemption 时，INT 被长时间 BG 任务阻塞的降级策略？
2. E-core 上有实时任务需求：等待 P-core 还是立即在 E-core 上运行（延迟 vs 吞吐）？
3. 功耗预算跨 CPU/GPU/NPU 的动态分配策略？

## 相关章节

- [04-driver-and-kernel.md](./04-driver-and-kernel.md) — GPU 驱动与调度器接口
- [00-async-and-mmap.md](../topics/00-async-and-mmap.md) — 取消和背压的异步语义
