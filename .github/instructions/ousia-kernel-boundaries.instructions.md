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
- 内核空间里出现 `hashbrown` 之类容器不自动意味着可以接受 SIMD：是否触碰 FP/SIMD 取决于该依赖在当前 target cfg 下编译出的实现。普通 kernel 路径只能使用明确证明为 generic/no-SIMD 的依赖或数据结构；若需要 SIMD/full-speed 版本，必须放在 OSTD-owned 的 guard/token 边界后面，并且不要把同一个 package identity 当作运行时切换开关。

## Reference-First 内核工作

- 实现 OS、kernel、boot、QEMU、driver、MMIO、IPC、scheduler、FPU/SIMD 或 loader 能力前，先查看项目已有 reference、成熟 crate、工业级实现和硬件手册。
- 本仓库存在本地 reference 时必须优先读取本地源码，例如 `third_party/sel4`、`third_party/asterinas`、`third_party/rust-sel4`。不要在未检查本地 reference 的情况下只凭记忆、网络搜索或概括性知识做内核设计判断。
- 优先参考 seL4、rust-sel4、Microkit、sDDF、Asterinas OSTD/OSDK、Linux/rust-osdev 生态和相关硬件手册。只有检查过边界、license、维护成本和语义适配后，才写自定义实现。
- Phase 1 kernel 语义实现必须先读取对应本地 seL4 baseline 源码，再设计 Rust 表达。常见入口包括 `third_party/sel4/src/object/cnode.c`、`untyped.c`、`endpoint.c`、`notification.c`、`tcb.c` 以及对应 `include/object/**` 结构定义；不要先按 Ousia 当前代码补洞，再事后用 seL4 找局部依据。
- 读取 seL4 后先抽取语义表，再编码：decode 顺序、authority root、source/target/destination lookup、guard/depth/error ordering、提交前检查、状态副作用点、失败后必须保持不变的 owner state、CTE/MDB/object/scheduler 改动顺序。Rust API 可以更清晰，但实现和测试必须能映射回这张表。
- 如果实现过程中发现自己对项目 reference、边界或现有代码了解不足，先补读本地源码和 owning docs；若这个不足来自 instruction/skill 没有约束到位，应同步更新对应 instruction 或 skill，避免下次复发。
- 遇到 QEMU、boot、serial、exception level、CPU feature、loader 和 device tree 问题时，不要把偶然跑通的路径当成最佳实践。先对比 seL4、Asterinas 和 rust-sel4 的 machine 参数、boot 约束、exception-level 假设、device model 和测试方式，再选择 Ousia 的最小路径。

## Memory 与平台方向

- CortenMM/Asterinas 的 memory-management 启发应先落实为边界。避免让 VMA tree 和 page table 成为两套互相竞争的真相源。
- 后续 address space 应以 page-table structure、typed frame metadata 和 range/cursor guard 作为权威边界。multi-level page-table locking、SMP 并发和 verification structure 应等 page-table 与 frame-metadata 语义稳定后再接入。
- Ousia 是 multi-core-only kernel 项目。不要把“single-core first, SMP later”设计成主路径。scheduler、per-CPU state、IRQ/timer routing、TLB shootdown、FPU/SIMD ownership、locks 和 allocator 边界从一开始就必须按多核语义建模，即使第一版实现很小。

## seL4 Baseline

- Phase 1 的内核目标是在 Rust 中复刻 seL4 baseline，而不是只做宽松的 seL4-like 启发实现。
- Capability、CSpace/CNode、Untyped/retype、delete/revoke、Endpoint、Notification、Reply、TCB、IPC、syscall/invocation 和 scheduling 的算法、抽象、对象关系、权限语义和状态机必须先对齐 seL4 baseline。
- 对齐范围包括算法、语义和抽象本身；不能只保留外观 API 或测试可见行为，却在内部状态所有权、数据结构关系、错误顺序、revocation/finalisation 规则或 capability derivation 语义上偏离 seL4 原版。
- Rust 语言特性只用于更清楚地表达类型、不变量、错误边界、状态机和测试；不得用“更 Rust”作为改变 seL4 baseline 语义的理由。
- Rust 风格是实现表达层的要求，不是语义自由度：API 应符合 Rust 人体工程学、类型安全和误用抵抗，优先用 enum、newtype、Result、借用、所有权和清晰 module boundary 表达 seL4 语义，而不是机械复刻 C API 的指针式、参数堆叠式或易误用形状。
- 当 seL4 C API 难用或怪异时，先抽取其真实算法和抽象，再设计 Rust API；Rust API 可以更优雅、更安全、更符合调用者直觉，但必须能明确映射回本地 seL4 reference 的对应算法、状态和错误边界。
- 测试应围绕 seL4 baseline 使用语义和失败不变量编写，而不是围绕 Ousia 当前 helper 或过渡 descriptor 形状补 fixture。旧测试与 baseline 冲突时，优先修正 authority shape、owner 边界和测试语义，不要添加兼容 facade 让旧测试继续通过。
- Ousia-specific interface、Portal/Operation/Continuation、Package Cell、Service Graph、lease、session、Device Service 和浏览器/用户授权语义都属于 baseline 闭环后的扩展层，不得提前混入 Phase 1 kernel baseline。
- slot/object generation 可以作为 Rust model 中的 stale descriptor 检测、测试和诊断辅助；不得替代 seL4 authority、revoke、capability freshness 或授权语义。
- seL4 core 不使用通用 map 表达核心对象关系，也没有运行时 map 插入后动态扩容的主路径。CSpace/CNode 是连续 CTE slot 数组，MDB 是 slot 内嵌链，scheduler 是固定 ready queue 数组加 bitmap，endpoint/notification 等待队列是 TCB 内嵌链。Ousia 早期模型中出现 `HashMap`、`BTreeMap`、`VecDeque` 或类似通用容器时，只能视为过渡脚手架；长期必须收敛到对应 seL4 领域容器。
- `kernel`/`ostd` 中的动态容器必须显式标注边界语义。owner storage、return/result collection、message buffer、diagnostic list、preflight/commit plan 和 initialization-only backing storage 必须通过模块、类型名或紧邻注释区分；不能让同一个 `Vec` 同时承担事实存储和返回集合职责。
- 核心 owner storage 若暂时仍用 `Vec`/`Box<[Option<_>]>` 等 Rust backing storage 表达，必须说明它是否初始化期固定容量、是否在 commit 前完成容量预检、是否可能在热路径扩容，以及退出到 seL4-style CTE array、typed object memory、TCB link 或 ready queue array 的条件。
- 边界返回值可以使用动态集合，但只能携带已经提交或已验证事实的快照；返回集合的分配失败不得发生在 owner state 已部分修改之后。需要在修改 owner state 后生成大集合时，应先预收集/预检、改成 iterator/cursor，或把该路径标为模型/诊断辅助而非长期 kernel 主路径。
- 在 `kernel`/`ostd` 主路径里，`Vec::push`、`resize_with`、iterator `collect` 和类似隐式扩容 API 必须被视为潜在失败点。只有容量已在 decode/preflight 边界通过 `try_reserve`、slot/window 检查或固定数组容量证明过，且调用点注释或封装名称说明不会扩容时，才允许使用；否则该路径必须返回可恢复错误或改成固定容量/游标式结构。
- 每个非平凡 kernel 语义实现或重构都应能指出本地 seL4 或 rust-sel4 reference 的对应路径；没有读取或无法映射 reference 时，应把 baseline drift 标为 residual risk，而不是凭概括性记忆放行。

## Kernel 错误模型

- 通用错误边界以 `.github/instructions/implementation-quality.instructions.md` 为权威。本节只规定这些规则在 Ousia `kernel` 中的领域投影。
- `kernel` 的外部可恢复错误只应来自 descriptor/syscall/invocation/capability rights/retype request 等边界。边界检查完成后，内部 object graph、slot linkage、TCB/reply/notification 状态转换应信任已经建立的不变量。
- 在 `kernel` 中，可恢复错误返回前不得产生部分副作用。capability 派生、retype、IPC enqueue/dequeue、reply handoff、scheduler mutation 和内存对象创建都必须先完成全部可失败检查，再提交状态修改。
- 内存分配、扩容、页表填充或资源保留等隐式失败点也属于 seL4 baseline 对齐范围。核心对象创建应优先像 seL4 `Untyped_Retype` 一样，把资源来源、目标 slot/window、对象大小和剩余空间检查收在 retype/invocation 边界；提交阶段只消费已验证的内存和 slot，不在内部普通路径临时发现可恢复的“分配失败”。
- `kernel` 的内部不变量破坏应使用带语义说明的 `expect`、assertion 或 panic 路径暴露为实现错误；不要把它伪装成用户可恢复的 `CapError`、`InvocationError` 或 syscall error。
- 参考 Asterinas 时，注意它的 kernel 主要使用自定义 errno-style `Error` 和局部 subsystem error，OSTD 使用小型 enum error；没有把 `thiserror`、`anyhow`、`eyre`、`snafu` 作为 kernel/OSTD 错误模型核心。
- Ousia `kernel` 默认不引入 derive-heavy 或 std-oriented 错误框架。只有当库能在 `no_std`、边界语义、代码尺寸和长期 ABI 上给出明确收益时，才考虑引入。host tooling 可以按普通 Rust 工具项目另行评估 `thiserror` 或 `anyhow`。
- Capability 和 invocation 的局部 typed error 可以保留为模型开发和测试工具，但长期 public/syscall-facing 错误应收敛到少量稳定语义类别。slot、object、expected/actual、rights 等细节只有在调用方行为、测试、trace 或诊断确实消费时才保留在公开结构中。
