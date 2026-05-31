# 00 — seL4-like Rust Baseline 草案

> 临时实现草案。本文指导近期代码推进，不是冻结后的 ABI 或最终架构规范。稳定结论应回写到 `core/` 和 `topics/` 的 owning 文档。

## 背景

Ousia 的平台目标包括浏览器权限、服务授权、lease、session、Package Cell、Device Service 和用户态驱动治理。这些语义不适合直接进入内核。更稳的路线是先做一个 seL4-like 的 Rust 微内核底座，再把 Ousia 的现代 OS 语义建立在用户态系统服务之上。

这意味着第一阶段内核实现应尽量复用 seL4 的能力纪律和微内核边界，而不是提前发明过多 Ousia 专有内核抽象。

## 阶段性目标

当前阶段目标是实现一个 seL4-like Rust kernel baseline。它不追求 seL4 级别的形式化验证，但必须通过清晰的不变量、类型边界、单元测试、集成测试、内核态测试和 review checklist 来保证足够的工程正确性。

这不是 Ousia 平台语义的最终形态，而是 Ousia 的可信底座：内核提供极窄、硬、可审计的机制；浏览器权限、服务授权、lease、session、Package Cell 和 Device Service 通过用户态系统服务在这个底座上封装。

## 目标

- 先实现一个 seL4-like Rust baseline。
- 能力核心不弱于 seL4 的派生、单调权限、删除和撤销语义。
- 高层授权语义留在用户态系统服务。
- 在不破坏 seL4-like 基线的前提下，保留 Ousia 所需的 fast-path descriptor stale 检测和 generation 增强。

## 非目标

- 不把浏览器 origin、窗口、Package Cell 策略、用户授权 UI 放进内核。
- 不在 Phase 0.5 冻结最终 syscall ABI。
- 不承诺 Rust 重写自动继承 seL4 的形式化证明。
- 不直接复制大型外部代码，除非完成 license、边界和维护成本审查。

## 正确性策略

不做形式化验证不等于放松正确性。近期实现必须用工程手段替代一部分证明资产：

- 能力不变量写进类型和构造函数，不依赖调用者自觉。
- 删除、撤销、retype、IPC transfer 等状态变化必须有单元测试覆盖主路径和失败路径。
- 对 capability derivation tree、slot generation、object generation、free list 和并发 revoke 建立专门测试。
- 公共入口返回显式错误，不在权限路径上使用隐式 panic。
- 每个阶段保留 `cargo fmt --check`、`cargo check`、`cargo test`，进入裸机后增加 kernel-mode test。
- SMP 前先冻结单核不变量；SMP 后对跨核 revoke、IPC、TLB shootdown 增加 stress / model-like 测试。

## Asterinas OSTD / OSDK 调研结论

Asterinas 的可复用价值主要在工程底座，而不是直接提供 seL4-like capability 语义。它的关键分层是：

- `ostd/`：Operating System Standard Library，把内存管理、任务、用户空间、interrupt、timer、driver support、boot 和 synchronization 等低层 unsafe/架构相关能力封成较安全的 Rust API。
- `osdk/`：`cargo-osdk` 工作流工具，提供 new/build/run/test/debug/doc，使用 `OSDK.toml` 描述构建和运行方案。
- `ostd-test`：kernel-mode testing framework，让 `#![no_std]` bare metal crate 获得接近 `cargo test` 的测试体验。
- Asterinas kernel 自身把 unsafe 限制在 OSTD，kernel 上层尽量保持 safe Rust。这种 framekernel 边界值得 Ousia 借鉴。

Asterinas Book 对 framekernel 的定义也值得吸收：unsafe 低层能力集中在 OS Framework，OS Services 用 safe Rust 实现；framework 需要同时满足 soundness、expressiveness、minimalism、efficiency。它也明确承认 soundness 没有立即走完整形式化验证路线，而是通过设计分析、社区审查和实现约束来逼近。这和 Ousia 当前“不做形式化验证，但必须有足够工程正确性”的阶段目标一致。

OSDK 的价值在工作流：`cargo osdk new/build/run/test/debug/doc` 把裸机内核的创建、构建、QEMU 运行、GDB 调试、kernel-mode test 和文档生成收拢成 Cargo 风格体验。它当前文档强调主要支持 x86_64 Ubuntu + QEMU 工具链，这意味着 Ousia 可以优先借鉴 manifest、kernel-mode test、base crate 生成和命令设计，而不是立刻依赖它作为跨平台唯一工具链。

本仓库可以在 `third_party/asterinas/` 保留一份被 `.gitignore` 忽略的本地 reference checkout。它只用于源码阅读、接口调研、license 审查和 spike，不加入 Ousia Cargo workspace，也不作为 `kernel` 的直接依赖。只有当某个小型组件边界清晰、license 和维护成本可接受，并且不反向约束 Ousia 的 seL4-like capability 语义时，才考虑复制改造或引入依赖。

对 Ousia 的影响：

- 能力模型和微内核语义继续走 seL4-like，不从 Asterinas 复制 Linux-compatible kernel 策略。
- boot、allocator、page table、interrupt、task、driver DMA/MMIO、kernel-mode test 和 cargo 工作流，应优先研究是否复用、适配或仿照 Asterinas OSTD/OSDK。
- Ousia 长期可以形成自己的 kernel SDK：底层像 OSTD 一样封装 unsafe 和架构差异，上层服务继续承载 Ousia 自己的 capability / IPC / Device Service 语义。
- 任何直接复制代码前必须检查 MPL-2.0、边界耦合和维护成本；更常见路径应是吸收接口设计和测试工作流。

## 实现路线

### 1. Capability core

当前 `kernel/src/cap/mod.rs` 应从通用 `ObjectKind + Rights` 模型逐步演进成 seL4-like CSpace：

- `CSpace` / `CNode` / `Slot`
- typed `Capability` enum
- `EndpointCap`
- `FrameCap`
- `CNodeCap`
- `UntypedCap`
- `TcbCap`
- `copy` / `mint` / `move` / `delete` / `revoke`

rights 的解释应跟随 capability 类型，而不是长期共享一套全局 `READ | WRITE | EXEC | MANAGE` 语义。Endpoint cap 当前用 `READ` 表达 receive、`WRITE` 表达 send，并显式保留 `GRANT` 和 `GRANT_REPLY`；调用边界必须把它们转换为 `can_send`、`can_receive`、`can_grant`、`can_grant_reply` 语义，不允许把 Endpoint 当普通文件式读写对象处理。Frame map 当前按 frame cap 裁剪请求的 VM rights，而不是要求 frame cap 同时具备 read/write。

### 2. Memory and retype

引入 seL4-style Untyped memory：

- boot 阶段注入 untyped caps
- 用户态或 root server retype 出 kernel objects
- revoke untyped 时能够回收派生对象

这一步比直接实现 Ousia MemoryObject 更底层，后续 MemoryObject 应构建在 frame / address space / pager 原语之上。

### 3. IPC baseline

在 capability core 稳定后实现：

- Endpoint
- Notification / Event
- Reply cap
- IPC fast path
- capability transfer

Ousia 的 Portal / Operation 可以先作为 seL4-like IPC 之上的用户态协议或薄内核扩展草案，不在第一步硬塞进 capability core。

### 4. SMP baseline

多核版本先追求清晰正确：

- per-core scheduler state
- cross-core wakeup
- TLB shootdown path
- capability table locking / epoch discipline
- revoke 与并发 IPC 的一致性

SMP 不应在 capability invariants 未稳定前展开过大。

## Ousia 扩展点

Ousia 可以在 seL4-like baseline 上增加：

- slot generation，用于防止 fast descriptor ABA
- object generation snapshot，用于让缓存映射、queue descriptor、ObjectHandle 明确失效
- service-level lease、Broker、session、watcher
- Device Service 和 Driver SDK 的 queue/buffer/event/fence 抽象

这些扩展不得破坏底层 capability 的不可扩权、派生树和硬撤销模型。

## 近期代码步骤

1. 保留当前测试覆盖，避免语义倒退。
2. 引入 typed `Capability` enum，替代长期依赖 `ObjectKind + Rights` 的泛化模型。
3. 把当前 `derive` 拆成更接近 `mint` / `copy` 的操作语义。
4. 引入 `CNodeCap` 和 slot guard 的雏形。
5. 为 Endpoint / Frame / Untyped 增加类型化 rights 校验。
6. 再考虑是否拆分 `cap/` 子模块。
7. 用 AArch64 QEMU `virt` direct boot + `tools/qemu-runner` 建立最小 QEMU 闭环：早期启动路径应具备 PL011 串口、异常向量和可自动验证的 smoke test，再逐步接入 device tree、frame allocator、页表、GIC 和 timer。amd64 同样是一等支持目标，但当前先通过裸机编译检查覆盖，QEMU runner 暂时只跑 AArch64。

## 当前运行路径

当前仓库参考 Asterinas 的分层方式，但不直接复制其 x86/RISC-V 启动实现：

- `ostd/` 是 Ousia 的 framekernel / kernel SDK 雏形，先承载架构相关 unsafe、boot `_start`、boot stack、early CPU state、early console、CPU halt、后续 boot memory、页表、异常和中断封装。它对应 Asterinas 的 OSTD 角色：把低层 unsafe 和架构差异收束在框架层。边界足够宽且相对稳定的底层能力可以拆成 `ostd/crates/*` 小 crate；单个早期模块不应为了形式上的并行编译过早拆散。
- `kernel/` 保持为核心内核 crate，承担架构无关的 `kernel_main`、panic 策略和 seL4-like capability / IPC / scheduler 等内核语义。它不直接散落 MMIO 寄存器、boot stack 或架构启动汇编。
- Ousia 按多核 only 内核设计，不提供单核长期主路径。基础组件先按广度建立最小正确骨架，但 scheduler、per-CPU state、IRQ/timer routing、TLB shootdown、FPU/SIMD ownership、锁和 allocator 边界都必须能自然扩展到多核语义；早期实现不能把“只有一个 CPU 会运行内核”当作核心不变量。
- 当前内核基础组件按 Rust 风味 seL4 baseline 推进：先让 capability、CSpace/CNode、Untyped/retype、Endpoint、Notification、TCB、IPC、syscall/invocation 和调度语义对齐 seL4，再基于可运行 baseline 评估 Ousia 是否需要修改语义或接口。Rust 风味只用于类型化状态、错误和权限表达，不改变 seL4 baseline 的对象关系和调用含义。
- `kernel::invocation` 是 capability 调用边界的最小骨架：它先把 Endpoint、Frame、Untyped 和 TCB 的 invocation 做成类型化请求和授权结果，负责对象类型检查、权限检查和 retype 大小检查；Endpoint send/recv 的授权结果显式带出 blocking、call、badge、grant 和 grant-reply 信息。Notification signal 只授权 badge 和对象，不能携带“bound TCB 当前是否正在 receive”这类调度事实。真正的 endpoint queue、address-space mapping、Untyped 派生和 scheduler 副作用由后续对象子系统接入，不能绕过 invocation 边界直接操作 capability internals。
- `kernel::cap` 的 CSpace-like model 已把 seL4 CNode 的基础 slot 操作拆开：`copy` 只降权并继承已有 badge；`mint` 在不扩权的前提下允许 Endpoint/Notification 设置新 badge；`move_capability` 转移 slot 内容并维护派生树关系，不把 move 误建模成新的派生。旧 `derive` 只是兼容入口，语义收敛到 `copy`。Reply cap 仍不可派生，避免复制一次性 reply 权限。
- `kernel::ipc` 承载 Endpoint 的最小状态机：Endpoint 显式使用 seL4-like `Idle / Send / Recv` 三态，send/recv 使用对应方向的 FIFO queue，并显式传入 `ThreadId` 和 `CpuId`。blocking send/receive 会入队并返回 blocked action；nonblocking send 在没有 receiver 时不入队，nonblocking receive 在没有 sender 时失败返回。IPC action 保留 sender badge、grant、grant-reply、call 和 receiver grant 信息；call 交付时显式返回 reply setup 需求，但不拥有 scheduler 或 reply cap slot 操作。后续 scheduler/CSpace 接入时根据 action 阻塞、唤醒、跨 CPU 投递线程，并创建或链接 reply cap。
- `kernel::notification` 承载 seL4-like Notification 的最小状态机：Notification 显式使用 `Idle / Waiting / Active` 三态。signal 在 waiting 时交付给最早等待线程；idle 且绑定的 TCB 正在等待 receive 时返回 bound receive completion；否则按 OR 语义累积成 active badge。wait 在 active 时消费 badge，在 idle/waiting 时入队；poll 在没有 active badge 时失败返回且不阻塞。Notification 不拥有 scheduler，也不直接读取 TCB 状态；TCB/调度层判断 bound TCB 是否能接收后，把条件传入 notification 边界。
- `kernel::reply` 承载 seL4-like Reply 的最小状态模型：Reply 保存至多一个 pending caller，reply 成功后消费这个 pending state 并返回需要唤醒的 caller 信息。Reply cap 当前携带 caller object、target object 和 grant 语义；普通 Reply cap 不可派生，避免复制一次性 reply 权限。CSpace 提供 `consume_reply_cap`，只允许 Reply cap 通过并删除对应 slot；Reply 对象消费 pending caller，CSpace 消费 reply cap slot，这两个动作由后续 syscall/scheduler glue 编排。Reply 不拥有 scheduler，也不直接复制消息寄存器；后续 Endpoint call 路径和 scheduler 接入时再把 caller TCB、reply cap 生命周期和消息传输闭合起来。
- `kernel::tcb` 承载 seL4-like thread identity 和 thread state baseline：`Inactive`、`Running`、`Restart`、`BlockedOnReceive`、`BlockedOnSend`、`BlockedOnReply`、`BlockedOnNotification` 和 `IdleThreadState` 显式建模，并提供 blocked/stopped 判定。`BlockedOnSend` 保存 endpoint、badge、grant、grant-reply 和 call 信息；`BlockedOnReceive` 保存 endpoint 和 receive grant 信息。TCB 持有可选 bound notification 关系，并提供 `waits_on_bound_notification_receive` 这类由 TCB 状态推导出的查询；Notification 对象只消费查询结果，不直接拥有或读取 TCB 状态。`CpuId`/`ThreadId` 属于 TCB/调度边界，不属于 IPC 私有类型；TCB affinity 从一开始显式存在，后续 scheduler 按多核语义使用它。
- `tools/qemu-runner/` 是根 workspace 外的宿主控制项目，对应 Asterinas OSDK/tooling 的方向。它负责在仓库根目录显式调用 `cargo build -p kernel --target aarch64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem`，再用 `qemu-system-aarch64 -machine virt -cpu cortex-a53 -kernel ...` 启动。手动运行时串口接 `stdio`；smoke 模式使用显式 `-chardev file,... -serial chardev:...`，避免依赖 QEMU `-nographic` 的隐式串口重定向。runner 支持普通 boot smoke 和 feature-gated exception smoke，分别验证串口启动路径和 AArch64 exception vector 诊断路径。
- `.cargo/config.toml` 只保留 bare-metal targets 的 `panic=abort` rustflag，不全局启用 `build-std`。`build-std` 只属于裸机 kernel 构建；如果泄漏到 host tools，会让普通 `std` 依赖和重建的 `core/alloc` 发生 duplicate lang item 冲突。
- 裸机 `alloc` 由 `ostd::mm::heap` 提供 early heap：底层使用 `linked_list_allocator`，内存来自 OSTD 私有静态区域，初始化发生在 `kernel_main` 的最早阶段。它只支撑早期 Rust 数据结构和 capability smoke，不承担最终物理页框管理、boot memory map 解析或 seL4-style Untyped retype；后续 frame allocator 应在 OSTD 的 boot memory / page-frame 边界内演进。
- `ostd::mm::frame` 先承载物理页框的基本不变量：页大小、物理地址类型、boot memory region、页对齐区间、memory map 归一化、boot-reserved 区间扣除、单区间和多区间 early frame allocator，以及一次性初始化的 early frame allocator state/API。它参考 Asterinas early frame allocator 的方向，先把 boot-time frame ownership 从 heap 中分离出来；后续再把 bootloader/device-tree 提供的真实 memory map 和内核镜像、boot modules、early heap 等已占用物理区间接入归一化入口，并演进到 frame metadata 和 seL4-style Untyped 派生。这个 API 只服务早期 boot 阶段，不是最终全局 frame metadata allocator。
- CortenMM 对 Ousia 的近期启发是避免在设计初期建立两套互相竞争的地址空间真相源。后续 address space 应以页表结构、typed frame metadata 和 range guard 为权威边界，再向上暴露安全 cursor/mapper；不要先做一套复杂 VMA tree，再让页表成为滞后的副本。CortenMM 的多级页表锁协议和验证结构要等 page table、frame metadata 和 SMP 约束明确后再引入。
- AArch64 和 amd64 都是一等支持目标。当前本地 runner 先测试 AArch64；amd64 先保持 OSTD-owned boot stack、early COM1 console、halt loop 和裸机编译检查可用。
- AArch64 direct boot 在进入 Rust 前建立 FP/SIMD 访问不变量。seL4 的 AArch64 FPU 代码明确把 FP/SIMD 作为内核需要初始化、关闭、保存和恢复的 CPU 状态；当前 Ousia 还没有完整线程/FPU ownership，因此先由 `ostd` boot code 在当前 exception level 允许内核早期 Rust 代码使用 FP/SIMD，避免 debug 构建中 Rust 生成的 FP/SIMD 指令直接异常。后续进入线程调度和用户态后，应演进为 seL4-style lazy FPU ownership，而不是长期全局开放。
- AArch64 PL011 early console 不再手写裸 offset，而是参考 `rust-sel4/crates/drivers/pl011` 的方式使用 `tock-registers` 建模 register block 和 bitfield，并在写 `DR` 前等待 `FR.TXFF` 清空。
- AArch64 exception vector 由 `ostd/src/arch/aarch64/exception.rs` 承载：它提供 vector table、VBAR 安装、异常寄存器快照和 early diagnostic policy。当前策略只用于早期诊断：同步异常、IRQ、FIQ 和 SError 统一打印 vector、ELR、ESR、FAR、SPSR 后停机。真正的 syscall、IRQ dispatch、timer tick 和用户态 fault 处理应在后续内核对象和调度边界明确后再接入。
- 本地运行需要 `qemu-system-aarch64` 在 `PATH` 中。没有 QEMU 时，runner 仍可完成 AArch64 kernel 构建，但最后启动会失败并报告缺少命令。

这个路径的目标只是先让 AArch64 内核跑起来，不是冻结最终启动协议。等内核能稳定进入 QEMU，再决定是否引入 UEFI/ELF loader、设备树解析、initramfs、签名验证、amd64 runner 和更完整的 Ousia boot 流程。

## Review 问题

- 当前实现是否仍然能被 reviewer 映射到 seL4 的 CSpace / CNode / Slot 语义？
- Ousia 的 generation 增强是否保持为 stale detection，而不是变成授权语义本身？
- 是否有任何浏览器、Package Cell、Device Service 策略漏进了内核核心？
- 当前 typed capability 是否足够支撑下一步 IPC 和 retype？
