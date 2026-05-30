# 02 — 工程化基础设施

> 补充 [target.md](../target.md) 中实现语言、组件框架、硬件边界、更新和测试目标。

## 实现语言

**内核**：Rust。零成本抽象 + borrow checker 在编译期消除内存安全 bug。`unsafe` 块显式标注、可审计、有形式化理由。参考 Asterinas 将 unsafe 封装在验证过的核心原语中。

**用户态基础服务**：Rust。策略注入和受限扩展用 WASM 沙盒（过滤规则、观测 hook），不允许直接系统调用。

**驱动**：SDK 提供 Rust 安全绑定。闭源 C 驱动由 Driver Host 隔离边界保护，不进入内核地址空间。第一阶段优先稳定设备资源句柄、IOQueue/IOBuffer、事件和 reset/revoke 这些对象语义，不急于冻结巨大的设备 API 面。驱动对象模型和 ABI 边界归属 [04-driver-and-kernel.md](../core/04-driver-and-kernel.md)，本文只讨论工程化要求。

从工程化角度看，SDK 不应只是一层 syscall/FIDL 绑定，而应把**内核旁路作为第一公民的数据面能力**显式支持进去：

- queue/ring 映射和 descriptor helper
- registered memory / frame pool / DMA 生命周期管理
- direct/proxy doorbell 封装
- completion/fence/timeline 等待帮助器
- poll/interrupt hybrid runtime
- tracing、录制回放、仿真设备和性能分析工具

如果这些能力不进入 SDK，驱动作者最终会各自重写一套私有 runtime，旁路就不再是系统设计，而只是个别团队的技巧。

**为什么不是 Zig/Nim/Go/C**：Zig 无 borrow checker，Nim 有 GC，Go 有大 runtime 不适合微内核，C 无内存安全保证。

**形式化目标**：第一阶段对能力传递、缺页处理、IOMMU 映射做机器可检查规约，不追求全系统验证。

## 复用与自有 SDK 策略

早期实现应主动复用成熟库、现有内核 SDK 经验和上游项目设计，以加速进度并降低隐蔽工程坑。优先复用的对象包括 `no_std` 基础设施、bitflags/位集、allocator、页表、同步原语、任务队列、测试 harness、驱动队列 helper 和设备模拟工具。复用可以直接引入库、参考接口、复制改造小型组件，或把成熟项目的边界设计转写成 Ousia 的本地抽象。

复用不得反向冻结 Ousia 的核心语义。凡是会决定 Capability、Communication Fabric、Service Graph、Pager、Driver SDK 或 Package Cell 生命周期语义的库，都只能经过 Ousia owning 文档定义的边界进入实现；如果库的抽象与 Ousia 目标冲突，应优先保留 Ousia 语义，选择适配、局部复制、fork 或替换，而不是让外部 API 成为系统架构。

第一阶段可把 Asterinas、seL4、Fuchsia、Redox、Theseus、Tock 等项目作为工程素材库：工程底座型组件可以大胆吸收，架构语义型组件只在明确边界后局部采用。长期方向是在核心抽象稳定后沉淀 Ousia 自有 kernel SDK，把早期复用层中已经验证的 allocator、capability helper、IPC helper、driver queue/runtime、测试与仿真工具收敛成项目维护的 SDK。

## 构建系统

LLVM 工具链（rustc + clang + lld）。用 Bazel/Buck2 做多语言内容寻址构建。交叉编译目标：`aarch64-unknown-Ousia OS`、`x86_64-unknown-Ousia OS`。QEMU 作为第一阶段运行平台。

## 测试策略（四级）

1. **内核单元测试**：宿主系统上直接运行内核逻辑测试（不依赖硬件）
2. **用户态集成测试**：QEMU 中运行完整内核 + 基础服务栈
3. **驱动模拟测试**：录制设备 PCI/MMIO/doorbell/中断交互 → 回放验证，用于厂商驱动和通用队列原语的回归测试
4. **形式化模型检查**：能力传递不越权、IOMMU 映射不重叠、缺页处理不泄漏。工具候选：TLA+, Verus

## 内核更新：A/B 启动分区

内核镜像 + 第 1 层基础服务打包为 System Image。写入非活跃分区 → 验证签名 → 重启切换。新版本启动后健康检查超时未通过 → bootloader 自动回退。与 Package Cell 共享签名验证链。

## 组件框架

统一组件模型（参考 Fuchsia Component Framework）：声明式生命周期、能力声明、资源预算、热更新、依赖图启动顺序。WASM 用于策略注入/过滤/受限扩展，不用于高性能 IO 或直接硬件访问。

## 硬件支持边界

**第一阶段支持**：AArch64 (ARMv8.1+, UEFI, GICv3+, SMMUv3+) 和 x86-64 (v3 Haswell+, UEFI, xAPIC/X2APIC, VT-d/AMD-Vi)。

**明确不支持**：BIOS/Legacy、32 位 CPU、无 IOMMU、传统 PCI 不带 ACS、MBR。

## 开放问题

1. 自举编译器：Ousia OS 何时能在自己上编译内核？
2. 调试体验：Capsule 崩溃后需要什么级别的调试信息？
3. Bootloader 签名信任链：UEFI Secure Boot?

## 相关章节

- [04-driver-and-kernel.md](../core/04-driver-and-kernel.md) — 驱动 SDK 和 ABI
- [06-roadmap.md](./06-roadmap.md) — 落地顺序
