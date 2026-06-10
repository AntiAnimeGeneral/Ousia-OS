---
applyTo: "kernel/**,ostd/**,tools/qemu-runner/**,Cargo.toml,.cargo/**,design/implementation/**"
description: "Ousia OS 内核边界：kernel/OSTD/tooling 职责归属、Ousia capability kernel 路线、原生 HMP 假设和平台参考规则。"
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
- `ostd::mm::heap` 只是 early heap，用于早期 `alloc` 和 smoke tests。不要把 `linked_list_allocator` 演进成最终 kernel heap。真正的内存路径应围绕 boot memory map、typed frame metadata、page table ownership、kernel page allocator、slab/zone 或 fixed-pool allocator、resource budget 和 reclaim/preflight 边界建立；seL4 Untyped/retype 只能作为显式资源来源和硬撤销参考，不能限制 Ousia 内核拥有 VM、VFS cache 或 object metadata。
- `kernel` core 不使用 FP。SIMD 可作为 OSTD/arch-owned 的加速能力，用于 copy、checksum、crypto、compression 等明确热点；入口应由 OSTD 管理 FPU/SIMD ownership、preemption/interrupt 约束和寄存器保存恢复。`kernel` 只通过架构无关 OSTD API 请求这些能力，不直接持有或污染 FP/SIMD 状态。
- 内核空间里出现 `hashbrown` 之类容器不自动意味着可以接受 SIMD：是否触碰 FP/SIMD 取决于该依赖在当前 target cfg 下编译出的实现。普通 kernel 路径只能使用明确证明为 generic/no-SIMD 的依赖或数据结构；若需要 SIMD/full-speed 版本，必须放在 OSTD-owned 的 guard/token 边界后面，并且不要把同一个 package identity 当作运行时切换开关。

## Reference-First 内核工作

- 实现 OS、kernel、boot、QEMU、driver、MMIO、IPC、scheduler、FPU/SIMD 或 loader 能力前，先查看项目已有 reference、成熟 crate、工业级实现和硬件手册。
- 本仓库存在本地 reference 时必须优先读取本地源码，例如 `third_party/fuchsia`、`third_party/sel4`、`third_party/asterinas`、`third_party/rust-sel4`。不要在未检查本地 reference 的情况下只凭记忆、网络搜索或概括性知识做内核设计判断。
- Ousia Phase 1 的主结构参考优先读取 Fuchsia/Zircon：handle/object/rights 看 `third_party/fuchsia/zircon/kernel/object` 和 `zircon/system/public`；VMO/VMAR/address space 看 `third_party/fuchsia/zircon/kernel/vm`；用户态 handle wrapper 看 `third_party/fuchsia/zircon/system/ulib/zx`；driver framework 看 `third_party/fuchsia/src/devices/bin/driver_manager`、`src/lib/driver` 和 `zircon/system/ulib/ddk`。
- seL4、rust-sel4、Microkit 和 sDDF 仍是 capability discipline、硬撤销、失败无副作用、用户态驱动隔离和高保证系统构建的重要参考；它们不再定义 Ousia Phase 1 的 public API、对象模型或资源分配模型。
- Asterinas OSTD/OSDK、Linux/rust-osdev 生态和硬件手册仍用于 boot、allocator、page table、interrupt、driver、QEMU runner 和工具链参考。只有检查过边界、license、维护成本和语义适配后，才写自定义实现或复制代码。
- 读取参考后先抽取结构表，再编码：对象 owner、handle/rights 检查点、source/target/destination lookup、提交前检查、状态副作用点、失败后必须保持不变的 owner state、VM/allocator 分配上下文、reclaim/quota 边界和热路径访问次数。Rust API 可以更清晰，但必须能说明采用、调整或拒绝参考的理由。
- 如果实现过程中发现自己对项目 reference、边界或现有代码了解不足，先补读本地源码和 owning docs；若这个不足来自 instruction/skill 没有约束到位，应同步更新对应 instruction 或 skill，避免下次复发。
- 遇到 QEMU、boot、serial、exception level、CPU feature、loader 和 device tree 问题时，不要把偶然跑通的路径当成最佳实践。先对比 seL4、Asterinas 和 rust-sel4 的 machine 参数、boot 约束、exception-level 假设、device model 和测试方式，再选择 Ousia 的最小路径。

## Memory 与平台方向

- CortenMM/Asterinas/Zircon 的 memory-management 启发应先落实为边界。避免让 VMA tree、VMO/MemoryObject metadata 和 page table 成为多套互相竞争的真相源。
- 后续 address space 应以 VM object/address-space owner、page-table structure、typed frame metadata 和 range/cursor guard 作为权威边界。multi-level page-table locking、HMP 并发、reclaim 和 verification structure 应等 VM object、page table 与 frame-metadata 语义稳定后再接入。
- Ousia 是 always-multicore native HMP kernel 项目。单核不是支持目标、实现捷径或 correctness/performance 论据；不要把“single-core first, SMP later”设计成主路径，也不要把同构 SMP 当成最终硬件模型。SMP 只是 HMP 的退化情况；scheduler、per-CPU/per-compute-domain state、IRQ/timer routing、TLB shootdown、FPU/SIMD/accelerator ownership、locks、allocator 和 power/thermal boundary 从一开始就必须按异构多处理器语义建模，即使第一版实现很小。
- 所有 kernel/OSTD 主路径设计都必须以并发、并行和跨资源竞争下的性能为出发点。不能用“第一版只有一个 CPU 跑得通”证明锁、队列、allocator、page-table mutation、IPC wait/wake 或 driver queue 设计成立；临时 smoke path 可以单 CPU 运行，但 owning design 必须说明并行 owner、同步边界、可扩展瓶颈和 HMP 退出条件。
- HMP 语义覆盖 CPU 大小核、cluster/topology、GPU/NPU/DSP/SmartNIC 等异构执行后端、共享内存带宽、设备本地内存、电源/热设计功耗域和硬件队列。kernel 和 OSTD 需要把这些硬件事实归一化为 Compute Domain、Execution Class、resource budget 和 device/resource handle；不要把 NPU/GPU 视为普通外设旁路，也不要让厂商 runtime 私有调度绕过系统级预算和隔离。

## Ousia Kernel Architecture Direction

- Phase 1 的内核目标是 Ousia 原生高级 capability kernel，而不是 seL4 baseline 复刻。核心 public API 应围绕 handle/object、channel/call、MemoryObject/VM、process/thread、VFS/Object Namespace 和 driver/resource handle 设计。
- Zircon/Fuchsia 是 handle/object、VMO/VMAR、channel/call、driver framework 和用户态 wrapper 的主要结构参考。参考事实必须经过 Ousia 需求过滤，不能因为 Zircon 这样做就直接复制 class hierarchy、component policy 或 ABI。
- seL4 是 capability discipline 参考：不可伪造 authority、rights 单调性、硬撤销、最小权限、失败无部分提交和热路径约束。不要把 seL4 CSpace/CNode/Untyped/retype 暴露为 Ousia 的普通用户态资源申请 API。
- Capability/Handle 的 public 语义必须高级且易用：用户态持有 typed handle，内核在 object boundary 检查类型、rights、generation 和 lifetime；裸 slot、CNode path、Untyped allocator metadata 和内部 derivation graph 只允许作为实现细节或安全算法参考。
- Portal、Operation、Continuation、EventPort、MemoryObject、Object Namespace、Package Cell、Service Graph、Device Service 和 Driver Host 是 Ousia 主线抽象，不再被要求等 seL4 baseline 闭环后才能进入 Phase 1 裁决。
- 内核可以拥有 VM subsystem、page allocator、kernel heap/slab/fixed pool、VFS/Object Store metadata、page cache、handle table 和 object manager。每类动态状态必须有清晰 owner、资源预算或 quota、reclaim/销毁路径、失败前置检查和热路径分配说明。
- slot/object generation 或 handle generation 可以作为 stale handle 检测、测试和诊断辅助；它不能替代 rights 检查、object lifetime、revoke 或授权语义。
- `kernel`/`ostd` 中的动态容器必须显式标注边界语义。owner storage、return/result collection、message buffer、diagnostic list、preflight/commit plan、cache/reclaim storage 和 initialization-only backing storage 必须通过模块、类型名或紧邻注释区分；不能让同一个 `Vec` 同时承担事实存储和返回集合职责。
- 核心 owner storage 若使用 `Vec`、`Box<[Option<_>]>`、map/set、slab 或 cache，必须说明它是否初始化期固定容量、是否在 commit 前完成容量预检、是否可能在热路径扩容、分配失败如何回滚或报告，以及退出到更低成本结构的条件。
- 边界返回值可以使用动态集合，但只能携带已经提交或已验证事实的快照；返回集合的分配失败不得发生在 owner state 已部分修改之后。需要在修改 owner state 后生成大集合时，应先预收集/预检、改成 iterator/cursor，或把该路径标为诊断辅助而非长期 kernel 主路径。
- 在 `kernel`/`ostd` 主路径里，`Vec::push`、`resize_with`、iterator `collect` 和类似隐式扩容 API 必须被视为潜在失败点。只有容量已在 decode/preflight 边界通过 `try_reserve`、budget/quota、slot/window、cache reservation 或固定数组容量证明过，且调用点注释或封装名称说明不会扩容时，才允许使用；否则该路径必须返回可恢复错误或改成固定容量/游标式结构。
- 每个非平凡 kernel 语义实现或重构都应能指出本地 Zircon、seL4、Asterinas 或硬件 reference 的对应路径，并说明采用、调整或拒绝理由；没有读取或无法映射 reference 时，应把 reference gap 标为 residual risk，而不是凭概括性记忆放行。

## Kernel 错误模型

- 通用错误边界以 `.github/instructions/implementation-quality.instructions.md` 为权威。本节只规定这些规则在 Ousia `kernel` 中的领域投影。
- `kernel` 的外部可恢复错误只应来自 descriptor/syscall/invocation/capability rights/retype request 等边界。边界检查完成后，内部 object graph、slot linkage、TCB/reply/notification 状态转换应信任已经建立的不变量。
- 在 `kernel` 中，可恢复错误返回前不得产生部分副作用。capability 派生、retype、IPC enqueue/dequeue、reply handoff、scheduler mutation 和内存对象创建都必须先完成全部可失败检查，再提交状态修改。
- 内存分配、扩容、页表填充或资源保留等隐式失败点属于 Ousia kernel 错误边界的一部分。Zircon 的 object/VM create path 会用 `AllocChecker`、`ZX_ERR_NO_MEMORY` 或 `zx::error(ZX_ERR_NO_MEMORY)` 显式处理分配失败；Ousia 不得比它更随意。核心对象创建、handle 安装、VM mapping 和 VFS cache mutation 必须把资源来源、目标持有点、对象大小、预算/quota 和剩余空间检查收在 syscall/invocation preflight 边界；提交阶段只消费已验证的内存、handle slot、page 或 cache entry，不在内部普通路径临时发现可恢复的“分配失败”。
- `NO_MEMORY`、`NO_CAPACITY` 和 `QUOTA_EXCEEDED` 必须作为不同语义处理：heap/slab/page metadata 分配失败是内存不足，固定表/队列/slot 无空位是容量不足，process/capsule budget 不足是 quota 超限。测试可以先断言语义类别；实现不得用一个模糊错误吞掉三者。
- 在 kernel 可恢复路径中，不得用 panic、`unwrap`、unchecked `Vec::push`、隐式扩容或“分配基本不会失败”处理动态分配失败。若 commit 中调用可能分配或扩容的 API，必须有 reservation token、固定容量证明或紧邻 invariant 说明为什么不会失败。
- `kernel` 的内部不变量破坏应使用带语义说明的 `expect`、assertion 或 panic 路径暴露为实现错误；不要把它伪装成用户可恢复的 `CapError`、`InvocationError` 或 syscall error。
- 参考 Asterinas 时，注意它的 kernel 主要使用自定义 errno-style `Error` 和局部 subsystem error，OSTD 使用小型 enum error；没有把 `thiserror`、`anyhow`、`eyre`、`snafu` 作为 kernel/OSTD 错误模型核心。
- Ousia `kernel` 默认不引入 derive-heavy 或 std-oriented 错误框架。只有当库能在 `no_std`、边界语义、代码尺寸和长期 ABI 上给出明确收益时，才考虑引入。host tooling 可以按普通 Rust 工具项目另行评估 `thiserror` 或 `anyhow`。
- Capability 和 invocation 的局部 typed error 可以保留为模型开发和测试工具，但长期 public/syscall-facing 错误应收敛到少量稳定语义类别。slot、object、expected/actual、rights 等细节只有在调用方行为、测试、trace 或诊断确实消费时才保留在公开结构中。
