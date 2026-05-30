# 04 — seL4 sDDF 驱动框架参考

> 状态：参考材料。本文用于理解 seL4 Device Driver Framework 的用户态驱动、共享内存 transport、设备虚拟化和 legacy driver reuse 策略，并提炼 Ousia Driver / Device Service 的参考约束。规范性设计见 [04-driver-and-kernel.md](../../core/04-driver-and-kernel.md)、[07-data-and-filesystem.md](../../core/07-data-and-filesystem.md) 与 [03-driver-sdk-draft.md](../analysis/03-driver-sdk-draft.md)。

## 1. sDDF 的问题意识

sDDF 针对的是微内核系统中最难绕开的矛盾：驱动应在用户态隔离运行，但性能不能明显输给宏内核。

Trustworthy Systems 对 sDDF 的描述很明确：

- 驱动是用户态普通组件，位于自己的地址空间。
- 驱动只做硬件相关接口到设备类接口的翻译。
- 共享设备、地址转换、cache 管理、mux/demux 等职责由独立组件承担。
- 数据面走共享内存和有界队列，通知只负责同步。
- 目标是在高鲁棒性下接近或超过传统内核驱动性能。

这和 Ousia 的“Device Service / Driver SDK”方向高度相关。我们不应该把驱动框架设计成“Linux driver model 的用户态翻版”。

## 2. 单一职责：driver 不负责共享策略

sDDF 的核心判断之一是：driver 的唯一职责是把硬件接口翻译成设备类接口。

不属于 driver 的职责包括：

- 多客户端共享。
- 网络 mux/demux。
- 客户端地址到 DMA 地址的转换。
- cache 管理策略。
- 数据复制策略。
- QoS 和吞吐限制。

这些职责由 explicit virtualiser / copier / mux 组件处理。

这对 Ousia 很重要。Device Service 不应变成一个“全能驱动管理器”。更好的分层是：

- hardware driver：硬件寄存器、DMA ring、中断 ack。
- device-class transport：块设备、网络、串口、音频等通用协议。
- virtualiser/mux：多租户共享和策略。
- copier/translator：跨隔离域数据复制、地址转换、cache 维护。
- policy service：权限、限速、审计、恢复。

## 3. Transport layer：SPSC 队列 + 共享内存 + notification

sDDF 的 transport layer 使用 shared memory ring buffer。以 serial queue 为例，队列显式假设：

- 单生产者。
- 单消费者。
- 生产者只修改 `tail`。
- 消费者只修改 `head`。
- 双方可读取两个索引。
- acquire/release 操作保证跨组件可见性。

这种模型比“通用消息队列”窄，但非常适合驱动数据面。

优势：

- 队列有界，内存占用可静态分析。
- 单生产者/单消费者避免复杂锁。
- 数据走共享内存，通知只做唤醒。
- 背压由队列容量自然表达。
- 组件可跨核布置，仍保持接口稳定。

Ousia 的驱动数据面应该优先采用这种窄协议，而不是过早引入复杂的多生产者队列。

## 4. 三类内存区域

sDDF 文档和演示反复强调 driver model 使用不同内存区域，例如控制区、server/driver 可见区域、data region。

可以抽象成：

- control region：队列元数据、head/tail、信号状态。
- data region：实际 payload buffer。
- device / DMA region：设备可访问或硬件相关映射。

这些区域是否对 driver、virtualiser、client 可见，是设计的关键。sDDF 的重要点是：driver 不一定需要访问所有 data。某些路径上 driver 只处理 descriptor 或 buffer token。

Ousia 后续设计 DMA 和用户态驱动时，必须把内存区域建模成 capability，而不是让 driver 获得“整个设备相关内存”。

## 5. Serial virtualiser 的启示

sDDF serial 的 `virt_rx.c` 展示了一个非常具体的模式：

- 从 driver-facing RX queue 消费字符。
- 根据当前客户端状态选择目标 client queue。
- 对 client queue 做本地 tail 批量更新。
- 必要时通知 client。
- 如果 driver queue 释放了空间，再通知 driver。

这体现出几个细节：

- 虚拟化逻辑可以完全在 driver 之外。
- 通知不是每个字节都发，而是根据状态批量发。
- 队列可见性更新和 notify 是分开的动作。
- backpressure 可以从 client queue 传导到 driver queue。

Ousia 的事件和队列 API 需要支持这种“批量移动 + 条件通知”模式。

## 6. Network path 的启示

sDDF 网络路径通常拆成 TX/RX、MUX、copy、client、driver 等组件。每个组件单线程、隔离、通过 SPSC 队列传递 buffer。

这说明高性能不一定来自“大而全的 driver”。相反，性能可以来自：

- 简单组件。
- 明确队列。
- 零拷贝 buffer ownership 转移。
- 减少锁和共享状态。
- 在多核上把组件分布调度。

Ousia 不应把 network stack、NIC driver、packet classifier、capability policy 和 copy path 粘成一个服务。它们变化频率不同，安全边界不同，测试方式也不同。

## 7. Legacy driver reuse

sDDF 也承认现实：所有驱动从零重写不现实。因此它有 legacy Linux driver reuse 路线：

- 把原 Linux driver 放进最小 Linux VM。
- 用 UIO driver 控制设备。
- VM 对外表现为一个 sDDF driver。
- seL4 notifications 映射到 VM 中的 interrupt injection。
- 共享内存区域仍按 sDDF 协议暴露。

对 Ousia 来说，这是很重要的工程路线。早期工业可用性不能全靠从零写 driver。更现实的组合是：

- 核心设备类优先写 native driver。
- 复杂或硬件资料不足的设备先走 driver VM。
- 对外统一成 Ousia driver protocol。
- 明确标记 trusted / untrusted / legacy driver 边界。

## 8. sDDF 当前设备类

当前 sDDF README 和 Trustworthy Systems 页面提到的重点设备类包括：

- network。
- block。
- serial。
- I2C。
- audio。

TS 页面还提到 mature/native 或 VM-supported 的更多类，如 timer、clock、Pinmux、SPI、Ethernet、SDHC storage、NFC、GPU 2D、storage、sound 等。

这说明 driver framework 不应只围绕网络写死。网络可以先成熟，但抽象层要能承载 block、serial、I2C、audio 等不同 flow control 和 buffer 语义。

## 9. 对 Ousia 的近期建议

1. 在 `ostd` 之外定义 `driver-sdk` 或 `driver-protocol` 模块，先写 transport 协议，不急着写具体驱动。
2. 把 SPSC queue、shared region、notification、buffer ownership 做成一组独立可测试原语。
3. 明确 driver、virtualiser、copier、policy service 的职责边界。
4. AArch64 QEMU virt 的串口和 virtio 可以作为第一批协议验证对象。
5. 为 legacy driver VM 预留统一接口，不要等到需要 Linux 驱动时再重构。
6. 每个 driver 移植都要求记录参考文档、硬件手册版本、源代码来源和许可证。

## 10. 读源码时最值得看的位置

本地参考目录：`third_party/sddf/`

优先看这些文件：

- `README.md`
- `docs/developing.md`
- `include/sddf/serial/queue.h`
- `serial/components/virt_rx.c`
- `serial/components/virt_tx.c`
- `network/`
- `blk/components/`
- `examples/serial/`
- `examples/net/` 或当前网络相关 example 目录

同时参考：

- https://trustworthy.systems/projects/drivers/
- https://trustworthy.systems/projects/drivers/sddf-design-latest.pdf
