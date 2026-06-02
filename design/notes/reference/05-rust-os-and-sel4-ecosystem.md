# 05 — Rust OS 与 seL4 相关生态参考

> 状态：参考材料。本文用于梳理 Rust 社区中可用于 Ousia OS 的 OS/kernel/no_std/seL4 相关库，并区分可直接依赖、可拆参考和暂不采用的组件。规范性设计见 [02-engineering.md](../../topics/02-engineering.md)、[04-driver-and-kernel.md](../../core/04-driver-and-kernel.md) 与 [03-sel4-mcs-microkit-roadmap.md](./03-sel4-mcs-microkit-roadmap.md)。

## 1. 结论先行

当前最适合 Ousia 直接复用的是小而稳定的底层 crate：

- `aarch64-cpu`：AArch64 指令和系统寄存器封装。
- `x86_64`：x86_64 指令、寄存器、地址和页表结构。
- `tock-registers`：MMIO register map 和 bitfield 类型系统。
- `bitflags`、`spin`：已在项目中使用。

更大的项目如 `seL4/rust-sel4`、Asterinas OSTD、rust-osdev bootloader 等，不应直接整包依赖到 Ousia kernel。它们更适合：

- 拉到 `third_party/` 做本地参考。
- 研究 runtime、loader、capDL、driver adapter、shared ring buffer 的边界。
- 复制或重写小组件时保留许可证和来源说明。

## 2. seL4/rust-sel4

本地参考目录：`third_party/rust-sel4/`

`seL4/rust-sel4` 是 seL4 Foundation 当前维护的 Rust userspace 生态，不是 crates.io 上多年未更新的旧 `sel4` crate。

它包含：

- `sel4`：纯 Rust seL4 API binding。
- `sel4-sys`：从 libsel4 header 和接口定义生成的 raw binding。
- `sel4-microkit`：Microkit protection domain runtime。
- `sel4-root-task`：root task runtime。
- `sel4-capdl-initializer`：Rust 版 capDL initializer。
- `sel4-kernel-loader`：seL4 kernel loader。
- `sel4-shared-memory`：共享内存抽象。
- `sel4-sync`：基于 seL4 IPC/notification 的同步结构。
- `experimental/sel4-shared-ring-buffer`：共享 ring buffer。
- `experimental/sddf`：sDDF 相关 Rust 实验。
- `drivers/pl011`、`drivers/virtio/*`：可研究的驱动组件。

### 为什么暂不直接依赖

`rust-sel4` 面向的是“在 seL4 userspace 中写 Rust 程序”，而 Ousia 当前是在实现自己的 Phase 1 seL4 baseline Rust model 和 OSTD。它的很多 crate 需要 seL4 构建产物、libsel4 headers、Microkit SDK 或 capDL 环境变量。

因此直接依赖会带来错误耦合：

- 把 seL4 userspace ABI 前提带进 Ousia kernel。
- 把外部构建系统和环境变量绑到我们的最小 boot path。
- 在对象语义还没稳定前导入过多 runtime 假设。

### 最值得拆看的部分

- `crates/sel4-microkit`：PD runtime、入口函数、notification/ppcall 抽象。
- `crates/sel4-capdl-initializer`：capDL 初始化器的对象创建和 capability 分发。
- `crates/sel4-kernel-loader`：kernel loader、payload、平台信息处理。
- `crates/sel4-shared-memory`：共享内存安全封装。
- `crates/experimental/sel4-shared-ring-buffer`：共享 ring buffer 设计。
- `crates/drivers/pl011`：PL011 UART driver。
- `crates/drivers/virtio`：virtio block/net 和 HAL adapter。

## 3. aarch64-cpu

`aarch64-cpu` 来自 Rust Embedded Arm team，提供 AArch64 指令和系统寄存器封装。它适合直接依赖。

适合用途：

- `wfe`、`wfi`、barrier 等指令封装。
- EL 切换、SPSR/HCR/CNTHCTL 等系统寄存器。
- 未来 timer、exception entry、MMU 初始化。

AArch64 指令和系统寄存器访问应优先通过该 crate 或同等成熟封装表达；只有在 crate 不覆盖目标寄存器或指令时，才在 `ostd` 的架构层写最小 unsafe 汇编。

## 4. x86_64

`rust-osdev/x86_64` 是 Rust OSDev 社区成熟度较高的 x86_64 crate，提供：

- `hlt`、interrupt enable/disable、TLB flush 等指令。
- control registers、model-specific registers。
- GDT/IDT 等 descriptor table。
- physical/virtual address 类型。
- page table 数据结构。

amd64 指令、寄存器和页表结构应优先通过该 crate 或同等成熟封装表达；只有在 crate 不覆盖目标能力时，才在 `ostd` 的架构层写最小 unsafe 汇编。

后续 amd64 bring-up 时可以进一步用它承载：

- GDT/IDT 初始化。
- 页表结构。
- CR3/CR0/EFER 等寄存器访问。
- interrupt gate 和 exception handler 表达。

## 5. tock-registers

`tock-registers` 适合规范化 MMIO register map。它的价值不在“少写几行 volatile”，而在：

- register struct offset 可静态校验。
- bitfield 和 register 类型绑定，减少把字段写到错误寄存器的风险。
- `ReadOnly`、`WriteOnly`、`ReadWrite` 明确访问方向。
- 可用于 PL011、GIC、virtio MMIO、timer 等设备。

PL011 early console 使用 `tock-registers` 建模 register block 和 bitfield。实现参考 `rust-sel4/crates/drivers/pl011` 的寄存器表达方式，保留 QEMU virt 所需的最小初始化寄存器，并在写 `DR` 前等待 `FR.TXFF` 清空。

后续 GIC、timer、virtio MMIO 和平台设备都应优先采用同类 typed register map，而不是恢复到手写 offset + volatile 的风格。

## 6. arm-gic-driver

`arm-gic-driver` 是 rcore-os/tgoskits 中拆出的 GICv1/v2/v3 driver。它支持 no_std，API 包括 GIC init、CPU interface、interrupt ack/eoi、priority 和 enable。

适合参考或后续试用：

- AArch64 QEMU virt 的 GICv2/GICv3 初始化。
- 中断控制器抽象。
- `IntId` 类型化。
- FDT interrupt specifier 解析。

需要注意：

- 它是较完整的 driver，不是纯寄存器 crate。
- 需要和我们自己的 interrupt capability / IRQ object 语义对齐后再接入。
- 许可证和依赖边界需要单独确认。

## 7. rust-osdev bootloader

`bootloader` 是 x86_64 BIOS/UEFI bootloader crate。它成熟、文档多，但当前不是 Ousia 直接路径。

原因：

- Ousia 当前阶段优先测试 AArch64 QEMU direct boot。
- amd64 是第一支持目标，但不是当前 runner 的主测试面。
- 之前项目曾尝试 x86 bootloader 路线，和 AArch64-first bring-up 发生过方向冲突。

建议：

- 暂不接入。
- 等 amd64 QEMU smoke 成为目标时，再评估 `bootloader` 或 Microkit/x86 multiboot 结构。

## 8. Asterinas OSTD / OSDK

Asterinas 的 OSTD/OSDK 仍然是 Rust OS 工业化工程参考中最值得看的对象之一。

适合参考：

- OSDK build/run/test 开发体验。
- OSTD 对 arch、boot、console、memory、task 的分层。
- host-mode / kernel-mode 双环境开发。
- 可复用小组件，如已经拆入的 NS16550A UART。

不建议整包依赖：

- Asterinas 是 framekernel/Linux ABI 路线。
- 其 OSTD 的对象假设和 Ousia 的 Phase 1 seL4 baseline microkernel 不完全一致。
- 整包引入会把架构边界变成外部项目边界。

## 9. 复用分级

| 组件             | 当前策略             | 原因                                          |
| ---------------- | -------------------- | --------------------------------------------- |
| `aarch64-cpu`    | 直接依赖             | 小、稳定、no_std、架构指令/寄存器边界清晰     |
| `x86_64`         | 直接依赖             | 小、成熟、适合 amd64 bring-up                 |
| `tock-registers` | 直接依赖             | 已用于 PL011，适合继续承载 MMIO register map  |
| `arm-gic-driver` | 先参考，后评估       | 和 IRQ/capability 语义耦合较强                |
| `seL4/rust-sel4` | 本地参考，拆件学习   | 面向 seL4 userspace，不是 Ousia kernel 内部库 |
| Asterinas OSTD   | 本地参考，复制小组件 | 架构路线不同，但工程组件很有价值              |
| `bootloader`     | 暂不采用             | 当前 runner 不是 amd64 bootloader 路线        |

## 10. 下一步建议

1. 研究 `rust-sel4` 的 `sel4-kernel-loader`，明确 Ousia direct boot 与未来 loader / DTB / payload 协议的分界。
2. 对照 seL4 AArch64 FPU 初始化和 lazy ownership，把当前 boot 阶段的 FP/SIMD 允许策略演进成线程级 FPU 状态管理。
3. 研究 `rust-sel4` 的 `sel4-capdl-initializer`，给 Ousia 的 system graph initializer 设计一个最小版本。
4. 研究 `experimental/sel4-shared-ring-buffer` 与 sDDF queue 的关系，形成 Ousia driver transport 原型。
5. amd64 路线继续用 `x86_64` crate 承载 GDT/IDT/page table，不再手写基础结构。
