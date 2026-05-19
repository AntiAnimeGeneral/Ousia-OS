# 11 — 工程化基础设施

> 对应 `target.md` §2.3 + §4.10 + §4.11 + §4.13 + §4.14

## 实现语言

**内核**：Rust。零成本抽象 + borrow checker 在编译期消除内存安全 bug。`unsafe` 块显式标注、可审计、有形式化理由。参考 Asterinas 将 unsafe 封装在验证过的核心原语中。

**用户态基础服务**：Rust。策略注入和受限扩展用 WASM 沙盒（过滤规则、观测 hook），不允许直接系统调用。

**驱动**：SDK 提供 Rust 安全绑定。闭源 C 驱动由 Driver Host 隔离边界保护，不进入内核地址空间。

**为什么不是 Zig/Nim/Go/C**：Zig 无 borrow checker，Nim 有 GC，Go 有大 runtime 不适合微内核，C 无内存安全保证。

**形式化目标**：第一阶段对能力传递、缺页处理、IOMMU 映射做机器可检查规约，不追求全系统验证。

## 构建系统

LLVM 工具链（rustc + clang + lld）。用 Bazel/Buck2 做多语言内容寻址构建。交叉编译目标：`aarch64-unknown-xos`、`x86_64-unknown-xos`。QEMU 作为第一阶段运行平台。

## 测试策略（四级）

1. **内核单元测试**：宿主系统上直接运行内核逻辑测试（不依赖硬件）
2. **用户态集成测试**：QEMU 中运行完整内核 + 基础服务栈
3. **驱动模拟测试**：录制设备 PCI/MMIO/中断交互 → 回放验证，用于厂商驱动回归测试
4. **形式化模型检查**：能力传递不越权、IOMMU 映射不重叠、缺页处理不泄漏。工具候选：TLA+, Verus

## 内核更新：A/B 启动分区

内核镜像 + 第 1 层基础服务打包为 System Image。写入非活跃分区 → 验证签名 → 重启切换。新版本启动后健康检查超时未通过 → bootloader 自动回退。与 Package Cell 共享签名验证链。

## 组件框架

统一组件模型（参考 Fuchsia Component Framework）：声明式生命周期、能力声明、资源预算、热更新、依赖图启动顺序。WASM 用于策略注入/过滤/受限扩展，不用于高性能 IO 或直接硬件访问。

## 硬件支持边界

**第一阶段支持**：AArch64 (ARMv8.1+, UEFI, GICv3+, SMMUv3+) 和 x86-64 (v3 Haswell+, UEFI, xAPIC/X2APIC, VT-d/AMD-Vi)。

**明确不支持**：BIOS/Legacy、32 位 CPU、无 IOMMU、传统 PCI 不带 ACS、MBR。

## 开放问题

1. 自举编译器：xos 何时能在自己上编译内核？
2. 调试体验：Capsule 崩溃后需要什么级别的调试信息？
3. Bootloader 签名信任链：UEFI Secure Boot?

## 相关章节

- [08-driver-and-kernel.md](./08-driver-and-kernel.md) — 驱动 SDK 和 ABI
- [12-roadmap.md](./12-roadmap.md) — 落地顺序
