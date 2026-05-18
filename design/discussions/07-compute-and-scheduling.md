# 07 — 计算域、调度与异构硬件

> 对应 `target.md` §3.6 + §4.4 + §4.12

## 讨论范围

xos 的调度模型必须同时解决三个现代系统中最棘手的问题：前台交互不卡顿、异构资源（CPU/GPU/NPU）统一调度、电源管理作为一等维度。本文讨论 Compute Domain 模型和执行等级的设计。

---

## Compute Domain 模型

### 为什么需要 Compute Domain

传统 OS 把硬件资源当作独立的东西管理：

- `taskset` 绑 CPU 核
- `cgroups` 限制 CPU/内存
- GPU 调度完全由厂商驱动私有
- NPU/DSP 通常没有 OS 级调度

xos 用 Compute Domain 统一描述异构计算资源：

```
ComputeDomain {
    type: CPU | GPU | NPU | DSP,
    topology: [Core | SM | Tile],   // 计算单元层级
    capabilities: {
        simd: bool,                 // SIMD 支持
        fp64: bool,                 // 双精度浮点
        tensor: bool,               // 矩阵加速（NPU/ Tensor Core）
        realtime: bool,             // 实时保证
    },
    memory: {
        local: usize,               // 本地内存 (如 GPU VRAM)
        bandwidth: Bandwidth,       // 带宽 (GB/s)
        coherence: CoherenceModel,  // 缓存一致性模型
    },
    power: {
        base_freq: MHz,
        boost_freq: MHz,
        power_zones: [PowerZone],   // 功耗域
    },
    scheduling: {
        preemptive: bool,           // 是否支持抢占
        granularity: Duration,      // 最小调度粒度
    }
}
```

### 任务路由

应用声明任务需求，系统自动路由：

```
任务声明:
  compute_type: "tensor-inference"
  latency_target: "10ms"
  priority: "interactive"

系统决策:
  1. 查询所有 Compute Domain 的能力
  2. 匹配：NPU 支持 tensor，CPU 也支持但慢 10x
  3. 检查 NPU 当前负载和功耗预算
  4. 如 NPU 可用 → 路由到 NPU
  5. 如 NPU 繁忙 → 根据优先级决定等待（interactive）还是降级到 CPU（batch）
```

---

## 执行等级体系

### 五级模型

```
优先级（高 → 低）

[0] Real-time (RT)
    - 硬实时保证（最大延迟有界）
    - 资源预分配，不可被抢占
    - 功耗上限豁免
    - 例：安全关键系统、工业控制

[1] Interactive (INT)
    - 软实时（延迟感知，但允许偶尔超限）
    - 保底 CPU/GPU 预算
    - 允许 boost 频率
    - 例：GUI 合成器、输入处理、音频 pipeline

[2] Foreground Service (FG)
    - 当前用户可见的应用
    - 公平份额 CPU，优先于后台
    - 例：浏览器、编辑器、视频播放

[3] Background Batch (BG)
    - 非交互非实时
    - 使用剩余资源，不保证延迟
    - 限制功耗，优先能效核
    - 可被随时暂停/降级
    - 例：编译、视频导出、备份

[4] Maintenance (MAINT)
    - 系统后台维护
    - 最低优先级，仅在完全空闲时运行
    - 例：垃圾回收、索引构建、更新预下载
```

### 执行等级不是 nice 值

Linux 的 nice 值 (-20 到 +19) 只是一个调度权重，无法表达：

- "我需要最低 10% GPU"
- "我不关心延迟但需要吞吐"
- "我可以被暂停但不能被杀死"
- "请把我在大核上运行"

xos 的执行等级是**带语义的声明**，调度器理解每个等级的含义，做出不同的调度决策。

---

## 交互保活机制

### 问题重述

编译 Chromium（`make -j$(nproc)`）不应该让鼠标卡顿。在 Linux 上，这需要手动设置 `nice` 或 `cgroups`。在 xos 上，这是默认保证。

### 工作机制

1. **预算预留**：Interactive 等级的总 CPU 预算（如 30%）不参与公平竞争。即使有 100 个 BG 任务，INT 任务总有一个保底比例。

2. **抢占保护**：INT 任务可以立即抢占 BG 任务的 CPU。抢占延迟目标 <100µs（远低于传统 OS 的 ms 级）。

3. **关键路径保护**：系统识别出关键交互路径（输入事件 → 窗口合成 → 显示），这些路径上的任务被标记为 CRITICAL，调度器确保它们在每次帧周期内获得足够的执行时间。

4. **背压传导**：如果一个 BG 任务生成了大量 IO（如备份大量文件），IO 调度器不会让它填满 IO 队列——背压传导到任务本身，使其降速。

---

## 异构资源调度

### GPU 调度

xos 的 GPU 调度不是"把 GPU 当黑盒"。它需要理解 GPU 的内部结构：

```
GPU ComputeDomain {
    engines: [Graphics, Compute, Copy, VideoDecode, VideoEncode],
    queues_per_engine: N,
    preemption_granularity: "thread" | "instruction",
    memory: VRAM + shared_system_memory,
}
```

- **多引擎并发**：一个 Capsule 用 Compute 引擎做推理，另一个用 Graphics 引擎做渲染——它们应该真正并行，不是时分复用。
- **抢占粒度**：如果 GPU 支持 thread-level preemption（如 NVIDIA 的 CMP），INT 任务可以抢占 BG 任务，不需要等待 BG 任务完成。
- **显存调度**：VRAM 满时，谁的 allocation 被换出？由执行等级决定：BG 任务先被换出。

### 数据移动成本

把数据从 CPU 内存搬到 GPU 显存的成本可能比计算本身更高。xos 的调度器需要考虑数据位置：

```
任务: "推理这张图片"
图片位置: CPU 内存
可用计算后端: CPU(0ms 数据移动, 100ms 计算), GPU(5ms 数据移动, 10ms 计算)
总延迟: CPU = 100ms, GPU = 15ms
→ 选择 GPU

但如果图片已经在 GPU 显存中:
总延迟: CPU = 100ms (需要先搬过来), GPU = 10ms
→ GPU 的优势更大
```

---

## 电源管理

### 与调度的统一

xos 不把电源管理当作独立子系统。执行等级携带功耗语义：

| 执行等级 | 频率策略   | 核选择           | 功耗上限 |
| -------- | ---------- | ---------------- | -------- |
| RT       | 锁最高频   | P-core 独占      | 无上限   |
| INT      | 允许 boost | P-core 优先      | 高       |
| FG       | 默认频率   | P-core（如可用） | 中       |
| BG       | 限制 boost | E-core 优先      | 低       |
| MAINT    | 最低频     | E-core only      | 最低     |

### 设备级电源

当没有 Capsule 持有某个设备（如 GPU）的能力句柄时，设备可以进入低功耗状态。当有新请求时，内核协调设备唤醒。这在用户态驱动模型下尤其重要——设备驱动在用户态，但电源状态管理在内核仲裁层。

---

## 开放问题

1. **GPU 抢占的现实可行性**：不是所有 GPU 都支持 fine-grained preemption。如果硬件不支持，INT 任务被 BG 任务阻塞时怎么办？
2. **异构任务的 QoS 保证**：一个 NPU 推理任务的延迟目标如何被调度器理解？需要一个统一的 QoS 描述语言吗？
3. **E-core 上的实时任务**：如果只有 E-core 空闲而实时任务需要立即执行，是在 E-core 上运行（延迟可能不达标）还是等待 P-core（延迟可能更长）？
4. **功耗预算的跨设备分配**：如果系统总功耗上限是 15W，CPU 和 GPU 同时有任务。如何在两者之间分配功耗预算？

---

## 相关章节

- [00-philosophy.md](./00-philosophy.md) — 交互保活是基本正确性要求
- [01-pain-points.md](./01-pain-points.md) — §1.3 同步阻塞与调度粗糙
- [08-driver-and-kernel.md](./08-driver-and-kernel.md) — GPU 驱动与调度器接口
- [09-async-model.md](./09-async-model.md) — 异步与取消语义
