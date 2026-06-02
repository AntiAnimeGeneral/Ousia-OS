---
applyTo: "kernel/**,ostd/**,tools/qemu-runner/**,Cargo.toml,.cargo/**,design/implementation/**"
description: "Ousia OS 内核边界：kernel/OSTD/tooling 职责归属、seL4 baseline、多核假设和平台参考规则。"
---

# Ousia 内核边界

处理 Ousia OS kernel、OSTD、QEMU runner、Cargo target 和 implementation design 时使用这些规则。

## Workspace 与 Tooling

- 根 Rust workspace 应按 bare-metal kernel workspace 理解，核心成员是 `kernel` 和 `ostd`。
- host 控制工具应保持为独立项目或脚本。不要为了支持 host tools 改变 bare-metal workspace target、rust-analyzer target、Cargo target 或核心模块形状。
- `#[cfg(target_os = "none")]` 不得隐藏 bare-metal core crate 的主路径模块、入口点或核心实现。LSP 和 host tooling 问题优先通过 `.vscode/settings.json`、rust-analyzer target 设置、Cargo `test = false` / `doctest = false` / `bench = false` 或独立 host tooling 项目解决。
- 确实需要 cfg 时，只放在语义边界模块上，例如 OSTD 架构模块或 bare-metal-only heap/boot assembly。不要把 cfg 分散到单个函数或 impl item 上。

## Kernel 与 OSTD 归属

- `kernel` 只表达架构无关的内核语义：capability、IPC、scheduler 策略和少量 boot 展示字符串。除 `boot_message` 这类展示文本外，`kernel` 不应包含实现相关的 `target_arch = "aarch64"`、`target_arch = "x86_64"`、MMIO、boot stack、exception vector、CPU register 或 QEMU machine 细节。
- OSTD 拥有架构差异、bare-metal entry、exception vectors、early serial、CPU halt/wait、FPU/SIMD 初始化、page tables、frame allocator、MMIO 和 boot memory-map normalization。
- 当 `kernel` 需要某个平台能力时，应通过架构无关的 OSTD API 请求，例如“如果当前平台支持则触发诊断异常”；不要在 `kernel` 里写 architecture cfg。
- `ostd::mm::heap` 只是 early heap，用于早期 `alloc` 和 smoke tests。不要把 `linked_list_allocator` 演进成最终 kernel heap。真正的内存路径应先围绕 boot memory map、typed frame metadata、page table ownership 和 seL4-style Untyped/retype 建立，再考虑 slab 或 per-CPU cache。
- `kernel` core 不使用 FP。SIMD 可作为 OSTD/arch-owned 的加速能力，用于 copy、checksum、crypto、compression 等明确热点；入口应由 OSTD 管理 FPU/SIMD ownership、preemption/interrupt 约束和寄存器保存恢复。`kernel` 只通过架构无关 OSTD API 请求这些能力，不直接持有或污染 FP/SIMD 状态。

## Reference-First 内核工作

- 实现 OS、kernel、boot、QEMU、driver、MMIO、IPC、scheduler、FPU/SIMD 或 loader 能力前，先查看项目已有 reference、成熟 crate、工业级实现和硬件手册。
- 本仓库存在本地 reference 时必须优先读取本地源码，例如 `third_party/sel4`、`third_party/asterinas`、`third_party/rust-sel4`。不要在未检查本地 reference 的情况下只凭记忆、网络搜索或概括性知识做内核设计判断。
- 优先参考 seL4、rust-sel4、Microkit、sDDF、Asterinas OSTD/OSDK、Linux/rust-osdev 生态和相关硬件手册。只有检查过边界、license、维护成本和语义适配后，才写自定义实现。
- 如果实现过程中发现自己对项目 reference、边界或现有代码了解不足，先补读本地源码和 owning docs；若这个不足来自 instruction/skill 没有约束到位，应同步更新对应 instruction 或 skill，避免下次复发。
- 遇到 QEMU、boot、serial、exception level、CPU feature、loader 和 device tree 问题时，不要把偶然跑通的路径当成最佳实践。先对比 seL4、Asterinas 和 rust-sel4 的 machine 参数、boot 约束、exception-level 假设、device model 和测试方式，再选择 Ousia 的最小路径。

## Memory 与平台方向

- CortenMM/Asterinas 的 memory-management 启发应先落实为边界。避免让 VMA tree 和 page table 成为两套互相竞争的真相源。
- 后续 address space 应以 page-table structure、typed frame metadata 和 range/cursor guard 作为权威边界。multi-level page-table locking、SMP 并发和 verification structure 应等 page-table 与 frame-metadata 语义稳定后再接入。
- Ousia 是 multi-core-only kernel 项目。不要把“single-core first, SMP later”设计成主路径。scheduler、per-CPU state、IRQ/timer routing、TLB shootdown、FPU/SIMD ownership、locks 和 allocator 边界从一开始就必须按多核语义建模，即使第一版实现很小。

## seL4 Baseline

- 先把内核基础组件做成 Rust 风味的 seL4 baseline。
- Capability、CSpace/CNode、Untyped/retype、Endpoint、Notification、TCB、IPC、syscall/invocation 和 scheduling 语义应先对齐 seL4，再发明 Ousia-specific interface。
- Rust 语言特性只用于更清楚地表达类型、不变量和错误；不要只为了风格改变 baseline 语义。
- 只有等 baseline 组件形成闭环后，才集中评估 Ousia-specific 语义改动。

## Kernel 错误模型

- 通用错误边界以 `.github/instructions/implementation-quality.instructions.md` 为权威。本节只规定这些规则在 Ousia `kernel` 中的领域投影。
- `kernel` 的外部可恢复错误只应来自 descriptor/syscall/invocation/capability rights/retype request 等边界。边界检查完成后，内部 object graph、slot linkage、TCB/reply/notification 状态转换应信任已经建立的不变量。
- 在 `kernel` 中，可恢复错误返回前不得产生部分副作用。capability 派生、retype、IPC enqueue/dequeue、reply handoff、scheduler mutation 和内存对象创建都必须先完成全部可失败检查，再提交状态修改。
- `kernel` 的内部不变量破坏应使用带语义说明的 `expect`、assertion 或 panic 路径暴露为实现错误；不要把它伪装成用户可恢复的 `CapError`、`InvocationError` 或 syscall error。
- 参考 Asterinas 时，注意它的 kernel 主要使用自定义 errno-style `Error` 和局部 subsystem error，OSTD 使用小型 enum error；没有把 `thiserror`、`anyhow`、`eyre`、`snafu` 作为 kernel/OSTD 错误模型核心。
- Ousia `kernel` 默认不引入 derive-heavy 或 std-oriented 错误框架。只有当库能在 `no_std`、边界语义、代码尺寸和长期 ABI 上给出明确收益时，才考虑引入。host tooling 可以按普通 Rust 工具项目另行评估 `thiserror` 或 `anyhow`。
- Capability 和 invocation 的局部 typed error 可以保留为模型开发和测试工具，但长期 public/syscall-facing 错误应收敛到少量稳定语义类别。slot、object、expected/actual、rights 等细节只有在调用方行为、测试、trace 或诊断确实消费时才保留在公开结构中。
