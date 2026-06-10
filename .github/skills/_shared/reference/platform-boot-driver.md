# Platform Boot Driver Reference

Platform/boot/driver reference 用于防止 bring-up 期间把 QEMU、machine、MMIO、device model 或 driver SDK 的偶然路径固化成 kernel 语义。

## Scope

使用本正文处理：

- QEMU machine、boot protocol、linker/loader、exception level、early serial。
- Device tree、MMIO mapping、interrupt/timer routing、CPU feature setup。
- Driver framework、Microkit/sDDF/Asterinas/rust-sel4 driver reference。
- Platform-specific code placement in OSTD/tooling versus kernel。

## Planning Prompts

- Bring-up 目标是 smoke-test、platform abstraction、driver model，还是长期 boot contract。
- Machine 参数、boot entry、exception level 和 memory layout 是否来自 reference 对比，而不是一次跑通的命令。
- Device tree/MMIO/interrupt/timer 信息由谁归一化，kernel 是否只看到架构无关能力。
- Driver framework 是 kernel object 语义、用户态 service、OSTD platform support，还是 tooling/runtime contract。
- Microkit/sDDF 的 driver isolation 和 communication pattern 哪些可借鉴，哪些不适合 Ousia 当前阶段。
- Early serial/logging 是否只是 diagnostics，不反向塑造 kernel/platform API。

## Review Attacks

- Kernel 是否直接持有 QEMU machine、MMIO base、device tree node、exception level 或 UART register。
- Boot code 是否把 loader-specific layout 当成长期 ABI，却没有 owning doc 或 reference comparison。
- Driver SDK proposal 是否混淆 kernel driver、user-space service、OSTD arch support 和 host tooling。
- Interrupt/timer routing 是否按 single-core 或同构 SMP bring-up 写死，无法支持 always-multicore native HMP 方向。
- Device tree parsing 是否散落在多个模块，或缺少 normalization owner。
- QEMU runner 是否绕过 project boundary，靠修改 kernel cfg 或 Cargo target 解决 host/tooling 问题。
- sDDF/Microkit/Asterinas 参考是否只列名字，没有分析 isolation、IPC、capability 和 scheduling 适配成本。

## Evidence To Seek

- QEMU command line、machine、CPU、serial、device tree、bootloader 和 linker assumptions。
- OSTD arch modules、early serial、exception vector、MMIO mapping、timer/IRQ setup。
- Driver design docs、service graph docs、communication fabric docs。
- seL4/Microkit/sDDF/Asterinas/rust-sel4 reference 中对应 boot、driver、device model 或 machine setup。
- Bring-up tests 是否区分 smoke-test success 和 stable platform contract。

## Residual Risk Triggers

- Boot/QEMU details appear in kernel core。
- Platform assumption has no owning doc。
- Driver proposal cannot say whether driver runs in kernel, OSTD, service graph, or host tooling。
- Interrupt/timer plan assumes one CPU or homogeneous CPU topology。
- Reference comparison lists projects but not adoption/rejection reasoning。
