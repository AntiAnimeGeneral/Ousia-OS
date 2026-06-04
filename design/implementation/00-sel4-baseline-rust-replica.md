# 00 — Phase 1 seL4 Baseline Rust 复刻草案

> 临时实现草案。本文指导近期代码推进，不是冻结后的 ABI 或最终架构规范。稳定结论应回写到 `core/` 和 `topics/` 的 owning 文档。

## 背景

Ousia 的平台目标包括浏览器权限、服务授权、lease、session、Package Cell、Device Service 和用户态驱动治理。这些语义不适合直接进入内核。更稳的路线是先做一个 Rust 表达的 seL4 baseline 微内核底座，再把 Ousia 的现代 OS 语义建立在用户态系统服务之上。

这意味着第一阶段内核实现应在 Rust 中复刻 seL4 baseline：算法、抽象、对象关系、权限语义和状态机先对齐 seL4；Rust 只用于更清楚地表达类型、不变量、错误边界和测试。Ousia 专有内核抽象和平台语义后置到 baseline 闭环之后。

## 阶段性目标

当前阶段目标是实现一个 Rust 表达的 seL4 kernel baseline。它不追求 seL4 级别的形式化验证，但必须通过清晰的不变量、类型边界、单元测试、集成测试、内核态测试和 review checklist 来保证足够的工程正确性。

这不是 Ousia 平台语义的最终形态，而是 Ousia 的可信底座：内核提供极窄、硬、可审计的机制；浏览器权限、服务授权、lease、session、Package Cell 和 Device Service 通过用户态系统服务在这个底座上封装。

## 目标

- 先实现一个 Rust 表达的 seL4 baseline。
- 能力核心、CSpace/CNode、Untyped/retype、delete/revoke、Endpoint、Notification、Reply、TCB、IPC、syscall/invocation 和 scheduler 先对齐 seL4 baseline。
- 高层授权语义留在用户态系统服务。
- slot/object generation 只作为 fast-path descriptor stale 检测、测试和诊断辅助，不改变 seL4 authority、revoke 或 capability freshness 语义。

## 非目标

- 不把浏览器 origin、窗口、Package Cell 策略、用户授权 UI 放进内核。
- 不在 Phase 0.5 冻结最终 syscall ABI。
- 不承诺 Rust 重写自动继承 seL4 的形式化证明。
- 不直接复制大型外部代码，除非完成 license、边界和维护成本审查。
- 不把 Portal、Operation、Continuation、Service Graph、Package Cell、lease、session、Device Service 或浏览器/用户授权策略提前并入 Phase 1 kernel baseline。

## 正确性策略

不做形式化验证不等于放松正确性。近期实现必须用工程手段替代一部分证明资产：

- 能力不变量写进类型和构造函数，不依赖调用者自觉。
- 删除、撤销、retype、IPC transfer 等状态变化必须有单元测试覆盖主路径和失败路径。
- 对 capability derivation tree、slot generation、object generation、free list 和并发 revoke 建立专门测试。
- 公共入口返回显式错误，不在权限路径上使用隐式 panic。
- 每个阶段保留 `cargo fmt --check`、`cargo check`、`cargo nextest run`，进入裸机后增加 kernel-mode test。
- 从一开始按多核不变量建模 scheduler、IPC、revoke 和 CSpace 生命周期；早期测试可以先覆盖较小实现面，但不能把单核执行当作设计前提。跨核 revoke、IPC、TLB shootdown 后续需要 stress / model-like 测试。

## Asterinas OSTD / OSDK 调研结论

Asterinas 的可复用价值主要在工程底座，而不是直接提供 Phase 1 seL4 baseline capability 语义。它的关键分层是：

- `ostd/`：Operating System Standard Library，把内存管理、任务、用户空间、interrupt、timer、driver support、boot 和 synchronization 等低层 unsafe/架构相关能力封成较安全的 Rust API。
- `osdk/`：`cargo-osdk` 工作流工具，提供 new/build/run/test/debug/doc，使用 `OSDK.toml` 描述构建和运行方案。
- `ostd-test`：kernel-mode testing framework，让 `#![no_std]` bare metal crate 获得接近 `cargo test` 的测试体验。
- Asterinas kernel 自身把 unsafe 限制在 OSTD，kernel 上层尽量保持 safe Rust。这种 framekernel 边界值得 Ousia 借鉴。

Asterinas Book 对 framekernel 的定义也值得吸收：unsafe 低层能力集中在 OS Framework，OS Services 用 safe Rust 实现；framework 需要同时满足 soundness、expressiveness、minimalism、efficiency。它也明确承认 soundness 没有立即走完整形式化验证路线，而是通过设计分析、社区审查和实现约束来逼近。这和 Ousia 当前“不做形式化验证，但必须有足够工程正确性”的阶段目标一致。

OSDK 的价值在工作流：`cargo osdk new/build/run/test/debug/doc` 把裸机内核的创建、构建、QEMU 运行、GDB 调试、kernel-mode test 和文档生成收拢成 Cargo 风格体验。它当前文档强调主要支持 x86_64 Ubuntu + QEMU 工具链，这意味着 Ousia 可以优先借鉴 manifest、kernel-mode test、base crate 生成和命令设计，而不是立刻依赖它作为跨平台唯一工具链。

本仓库可以在 `third_party/asterinas/` 保留一份被 `.gitignore` 忽略的本地 reference checkout。它只用于源码阅读、接口调研、license 审查和 spike，不加入 Ousia Cargo workspace，也不作为 `kernel` 的直接依赖。只有当某个小型组件边界清晰、license 和维护成本可接受，并且不反向约束 Ousia 的 Phase 1 seL4 baseline capability 语义时，才考虑复制改造或引入依赖。

对 Ousia 的影响：

- 能力模型和微内核语义继续先对齐 seL4 baseline，不从 Asterinas 复制 Linux-compatible kernel 策略。
- boot、allocator、page table、interrupt、task、driver DMA/MMIO、kernel-mode test 和 cargo 工作流，应优先研究是否复用、适配或仿照 Asterinas OSTD/OSDK。
- Ousia 长期可以形成自己的 kernel SDK：底层像 OSTD 一样封装 unsafe 和架构差异，上层服务继续承载 Ousia 自己的 capability / IPC / Device Service 语义。
- 任何直接复制代码前必须检查 MPL-2.0、边界耦合和维护成本；更常见路径应是吸收接口设计和测试工作流。

## 实现路线

### 1. Capability core

`kernel/src/cap/` 当前承载 seL4 baseline CSpace 基础层。`cap/mod.rs` 是 public facade，核心实现仍集中在 `cap/space.rs`，后续再按 CNode、CTE/MDB、Untyped/retype 和 capability payload 继续拆分：

- `CapabilitySpace`、slot descriptor、slot generation、object generation snapshot 和派生 lineage 已落地。
- typed `Capability` enum 已覆盖 `EndpointCap`、`FrameCap`、`CNodeCap`、`UntypedCap`、`TcbCap`、`NotificationCap` 和一次性 `ReplyCap`。
- `copy`、`mint`、`move_capability`、`delete`、`revoke_descendants`、`retype_untyped` 和 Reply cap consume/install 已分成独立语义入口。
- CSpace 当前能表达 CNode-like slot 操作和 Untyped/retype 的最小对象创建约束，但还不是完整 seL4 CNode addressing 或 Untyped allocator。

rights 的解释已经跟随 capability 类型，而不是长期共享一套全局 `READ | WRITE | EXEC | MANAGE` 语义。Endpoint cap 当前用 `READ` 表达 receive、`WRITE` 表达 send，并显式保留 `GRANT` 和 `GRANT_REPLY`；调用边界把它们转换为 `can_send`、`can_receive`、`can_grant`、`can_grant_reply` 语义，不把 Endpoint 当普通文件式读写对象处理。Call 必须由 caller endpoint cap 的 `GRANT` 或 `GRANT_REPLY` 授权建立 reply authority；Reply cap 的 grant 位来自 receiver 执行 receive 时使用的 endpoint cap `GRANT` 位，而不是 caller 的 `GRANT_REPLY`。Frame map 当前按 frame cap 裁剪请求的 VM rights，而不是要求 frame cap 同时具备 read/write。

剩余工作集中在更完整的 seL4 CSpace/Untyped 细节：CNode object-owned CTE array、Untyped 可用区间、watermark、alignment、容量 accounting、完整 object size table，以及 revoke 与并发 IPC/多核调度之间的一致性。

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

Ousia 的 Portal / Operation 可以先作为 seL4 baseline IPC 之上的用户态协议或扩展草案，不在第一步硬塞进 capability core。

### 4. SMP baseline

多核版本先追求清晰正确：

- per-core scheduler state
- cross-core wakeup
- TLB shootdown path
- capability table locking / epoch discipline
- revoke 与并发 IPC 的一致性

SMP 不应在 capability invariants 未稳定前展开过大。

## Ousia 扩展点

Ousia 可以在 seL4 baseline 闭环后评估增加：

- slot generation，用于防止 fast descriptor ABA
- object generation snapshot，用于让缓存映射、queue descriptor、ObjectHandle 明确失效
- service-level lease、Broker、session、watcher
- Device Service 和 Driver SDK 的 queue/buffer/event/fence 抽象

这些扩展不得破坏底层 seL4 capability 的不可扩权、派生树和硬撤销模型，也不得反向改写 Phase 1 baseline 语义。

## 近期代码步骤

1. 保留当前 capability、IPC、Reply、TCB 和 scheduler 测试覆盖，避免 seL4 baseline 语义倒退。
2. 将当前 CNode slot window-backed lookup 继续收敛为 object-owned CTE array，使 CNode cap 指向真实 CNode object slot storage，而不只是携带 window 起点。
3. 继续收紧 Untyped 可用区间、watermark、alignment 和容量 accounting；当前代码已有模型 object size table 和 CTE slot window 预检，后续需替换为真实 seL4 object layout 与 CNode size policy。
4. 将 Endpoint/Notification 的等待关系从当前显式 FIFO wrapper 继续迁到 TCB embedded queue link；Endpoint/Notification 最终只保留 head/tail 或等价轻量指针。
5. 在 frame metadata / typed object storage 接入后，把 Frame、Untyped 和 runtime object lifecycle 收敛到 backing object memory，而不是保留独立 object namespace。
6. `cap/` 已建立 facade + implementation file 的目录层次；继续拆分前保持 `CapabilitySpace` 的 CTE、MDB、lineage、slot generation 和 object generation 不变量集中可见，避免把同一 slot 事实拆成多套 owner。
7. 用 AArch64 QEMU `virt` direct boot + `tools/qemu-runner` 建立最小 QEMU 闭环：早期启动路径应具备 PL011 串口、异常向量和可自动验证的 smoke test，再逐步接入 device tree、frame allocator、页表、GIC 和 timer。amd64 同样是一等支持目标，但当前先通过裸机编译检查覆盖，QEMU runner 暂时只跑 AArch64。

## 当前运行路径

当前仓库参考 Asterinas 的分层方式，但不直接复制其 x86/RISC-V 启动实现：

- `ostd/` 是 Ousia 的 framekernel / kernel SDK 雏形，先承载架构相关 unsafe、boot `_start`、boot stack、early CPU state、early console、CPU halt、后续 boot memory、页表、异常和中断封装。它对应 Asterinas 的 OSTD 角色：把低层 unsafe 和架构差异收束在框架层。边界足够宽且相对稳定的底层能力可以拆成 `ostd/crates/*` 小 crate；单个早期模块不应为了形式上的并行编译过早拆散。
- `kernel/` 保持为架构无关核心内核库 crate，承担 Phase 1 seL4 baseline 的 capability / IPC / scheduler / object / invocation / executor 等内核语义。它不拥有 `kernel_main`、panic 策略、OSTD boot wiring、MMIO 寄存器、boot stack 或架构启动汇编。
- `kernel-bin/` 是 bare-metal kernel binary crate，承担 `kernel_main`、panic 策略、linker script 归属和 OSTD boot wiring。它依赖 `kernel` 和 `ostd`，把可运行 QEMU image 的入口细节从内核语义库中拆出。
- Ousia 按多核 only 内核设计，不提供单核长期主路径。基础组件先按广度建立最小正确骨架，但 scheduler、per-CPU state、IRQ/timer routing、TLB shootdown、FPU/SIMD ownership、锁和 allocator 边界都必须能自然扩展到多核语义；早期实现不能把“只有一个 CPU 会运行内核”当作核心不变量。
- 当前内核基础组件按 Rust 表达的 seL4 baseline 推进：先让 capability、CSpace/CNode、Untyped/retype、Endpoint、Notification、Reply、TCB、IPC、syscall/invocation 和调度语义对齐 seL4，再基于可运行 baseline 评估 Ousia 是否需要修改语义或接口。Rust 风格只用于类型化状态、错误和权限表达，不改变 seL4 baseline 的对象关系和调用含义。
- `kernel/src` 使用目录化 facade 结构表达当前 owner 边界：`cap/mod.rs`、`ipc/mod.rs`、`notification/mod.rs`、`reply/mod.rs`、`scheduler/mod.rs`、`object/mod.rs`、`state/mod.rs` 和 `thread/mod.rs` 负责公开模块边界；主要实现分别落在 `cap/space.rs`、`ipc/endpoint.rs`、`ipc/message.rs`、`notification/state.rs`、`reply/state.rs`、`scheduler/core.rs`、`object/table.rs`、`state/kernel.rs`、`thread/tcb.rs` 和 `thread/action.rs`。IPC message primitive 只通过 `kernel::ipc::message` / `kernel::ipc` 暴露；TCB identity/state 和线程事务只通过 `kernel::thread::tcb` / `kernel::thread::action` 暴露。
- `kernel::invocation` 是 capability 调用边界的最小骨架：它先把 Endpoint、Frame、Untyped 和 TCB 的 invocation 做成类型化请求和授权结果，负责对象类型检查、权限检查和 retype 大小检查；Endpoint send/recv 的授权结果显式带出 blocking、call、badge、grant 和 grant-reply 信息，且 call 需要 caller cap 具备 `GRANT` 或 `GRANT_REPLY`。Notification signal 只授权 badge 和对象，不能携带“bound TCB 当前是否正在 receive”这类调度事实。真正的 endpoint queue、address-space mapping、Untyped 派生和 scheduler 副作用由后续对象子系统接入，不能绕过 invocation 边界直接操作 capability internals。
- `kernel::cap` 的 CSpace-like model 已把 seL4 CNode 的基础 slot 操作拆开，并把 slot 事实收敛到 indexed CTE storage：`copy` 只降权并继承已有 badge；`mint` 在不扩权的前提下允许 Endpoint/Notification 设置新 badge；`move_capability` 转移 CTE slot 内容并维护 MDB/派生关系，不把 move 误建模成新的派生。旧 `derive` 只是兼容入口，语义收敛到 `copy`。CNode path lookup 已具备 root CNode、guard、radix、depth、remaining bits 和 lookup fault shape，目标 slot 通过 `CNodeCap` 的 slot window 起点加 radix offset 解析；`KernelState` 的 CNode copy/mint/move/delete/revoke invocation 只接受被调用 CNode root 下的 path target，CSpace raw slot commit API 只服务 executor 已解析路径后的内部提交边界。当前 CNode window 仍是过渡表达，尚未把 CNode object 自身建成 owner-owned CTE array。`retype_untyped` 只允许 Untyped cap 创建新的 child object，并检查目标对象最小大小不超过源 Untyped；当前覆盖 Endpoint、Frame、CNode、Untyped、TCB 和 Notification。Reply cap 由 call/reply 路径创建，`insert_reply_capability` 只为已存在的 Reply object 安装一次性 cap slot，不分配新的 backing Reply object，避免 CSpace metadata 和 ObjectTable Reply state 分叉。当前 retype 已建模 MDB lineage、slot window preflight、模型 object size table、alignment、watermark、容量 accounting 和提交计划；这些 size 常量用于防止零大小对象和验证事务语义，不代表冻结后的 seL4 object size ABI，后续应由真实 typed object layout、CNode radix/guard policy 和 frame metadata 替换。Reply cap 仍不可派生，避免复制一次性 reply 权限。CSpace 的 retype plan 让 executor 在提交需要 ObjectTable runtime entry 的 retype 前预检真实对象表绑定，避免 CSpace 和 ObjectTable 之间出现半事务；nested Untyped 当前只提交 CSpace lineage，不写 ObjectTable，但会消耗 parent Untyped capacity 并获得独立 allocation state。
- `kernel::ipc` 承载 seL4 baseline Endpoint 的最小状态机：`ipc/mod.rs` 是 facade，`ipc/endpoint.rs` 拥有 Endpoint `Idle / Send / Recv` 状态、send/recv 队列和 IPC action，`ipc/message.rs` 拥有 `IpcPayload`、`IpcError` 和 message word 上限。Endpoint send/recv 使用对应方向的 FIFO wrapper，并显式传入 `ThreadId` 和 `CpuId`。blocking send/receive 会入队并返回 blocked action；nonblocking send 在没有 receiver 时不入队，nonblocking receive 在没有 sender 时失败返回。IPC action 保留 sender badge、grant、grant-reply、call 和 receiver grant 信息；caller 的 grant/grant-reply 只决定 call 是否可建立 reply authority，reply setup 最终 grant 位由 receiver grant 决定。call 交付时显式返回 reply setup 需求，但不拥有 scheduler 或 reply cap slot 操作。reply cap destination 属于 receiver-side receive context；Endpoint queue 不直接拥有 CSpace slot。当前 FIFO wrapper 已移除通用 `VecDeque` 主路径；Endpoint sender queue 和 receiver queue 都只保存 thread/cpu queue entry，blocked-send badge、grant、call、payload 以及 blocked-receive grant/reply destination 已归入 TCB blocked state，后续还需继续把本地 FIFO entry 收敛为真正的 TCB embedded queue links。
- `kernel::notification` 承载 seL4 baseline Notification 的最小状态机：Notification 显式使用 `Idle / Waiting / Active` 三态。signal 在 waiting 时交付给最早等待线程；idle 且绑定的 TCB 正在等待 receive 时返回 bound receive completion；否则按 OR 语义累积成 active badge。wait 在 active 时消费 badge，在 idle/waiting 时入队；poll 在没有 active badge 时失败返回且不阻塞。Notification 不拥有 scheduler，也不直接读取 TCB 状态；TCB/调度层判断 bound TCB 是否能接收后，把条件传入 notification 边界。当前等待 FIFO 已使用本地 wrapper，队列条目只保存 waiter thread identity；`ThreadState::BlockedOnNotification` 拥有 notification object 和 receiver CPU metadata，signal/finalise 在消费队列前从 TCB state 预检并重建唤醒 CPU。后续仍需把本地 FIFO wrapper 改为真正的 TCB embedded queue membership。
- `kernel::reply` 承载 seL4 baseline Reply 的最小状态模型：Reply 保存至多一个 pending caller，reply 成功后消费这个 pending state 并返回需要唤醒的 caller 信息。Reply cap 当前携带 caller object、target object 和 receiver-derived grant 语义；普通 Reply cap 不可派生，避免复制一次性 reply 权限。CSpace 提供 `consume_reply_cap`，只允许 Reply cap 通过并删除对应 slot；KernelState 的 Endpoint call executor 在 immediate delivery 和 queued receive 两条路径上创建一次性 Reply cap，并返回 `ThreadWithReplyCap` outcome。KernelState 的 Reply executor 先校验 Reply cap metadata 与 Reply pending state 的 caller、target 和 grant 语义一致，再执行 `reply_to_caller`，并在成功后消费 reply cap slot。Reply 不拥有 scheduler，也不直接复制消息寄存器；消息寄存器和完整 cap transfer 仍留给 syscall ABI/CNode 阶段。
- `kernel::thread` 是线程相关 owner 的目录层次：`thread/tcb.rs` 承载 seL4 baseline thread identity 和 thread state baseline；`thread/action.rs` 承载跨 TCB、Endpoint、Notification、Reply 和 Scheduler 的线程事务。`Inactive`、`Running`、`Restart`、`BlockedOnReceive`、`BlockedOnSend`、`BlockedOnReply`、`BlockedOnNotification` 和 `IdleThreadState` 显式建模，并提供 blocked/stopped 判定。`BlockedOnSend` 保存 endpoint、badge、grant、grant-reply 和 call 信息；`BlockedOnReceive` 保存 endpoint、receive grant 和可选 receiver-side Reply object destination。TCB configure 的首版事务只把未绑定 TCB object 绑定到新的 inactive `ThreadId` 和 affinity，不自动入队；TCB resume 只允许 `Inactive -> Restart` 并按 affinity 入队，不直接把 blocked IPC/notification/reply 状态改成 runnable；后者需要先实现取消/出队语义，避免 endpoint、notification 或 reply pending state 留下悬挂 waiter。TCB 持有可选 bound notification 关系，并提供 `waits_on_bound_notification_receive` 这类由 TCB 状态推导出的查询；Notification 对象只消费查询结果，不直接拥有或读取 TCB 状态。`CpuId`/`ThreadId` 属于 TCB/调度边界，不属于 IPC 私有类型；TCB affinity 从一开始显式存在，后续 scheduler 按多核语义使用它。
- `kernel::object` 是 Phase 1 的 runtime object storage 边界：CapabilitySpace 保存 authority、CTE/MDB lineage、object id 和 generation；ObjectTable 保存 Endpoint、Frame、Notification、Reply 实体，保存带 radix/slot-count/window-start metadata 的 CNode runtime object，并保存 TCB object id 到可选 ThreadId 的绑定。ObjectTable 当前使用 Vec-backed object storage 和绑定事实扫描，不再维护独立 hash index；后续 typed backing storage 仍需给出明确容量和分配失败边界。FrameObject 当前只保存最小 `size_bits` metadata，不保存 cap rights、mapping state、page-table ownership 或 physical frame allocator 事实；rights 仍由 Frame cap/CapabilitySpace 拥有。CNodeObject 当前记录 retype radix、逻辑 slot 数和 planned window start，尚未成为 object-owned CTE array。未绑定 TCB object 是 retype 后、配置/绑定前的合法状态；TCB object id 和 ThreadId 不允许互相假设相等。Endpoint call 记录 Reply pending caller 时必须经 ObjectTable 反查真实 caller TCB object。TCB 实体仍由 `thread::action::ThreadTable` 单一拥有，避免 ObjectTable 和 ThreadTable 双写线程状态；ThreadTable 当前也使用 Vec-backed TCB storage，不维护第二套 thread map。ObjectTable 统一维护 object id 绑定和对象类型检查；这让 executor 可以在不使用 unsafe 的前提下同时取得 Endpoint 和 Reply 的 mutable reference，用于 call/reply 事务提交。
- `kernel::scheduler` 使用 fixed CPU topology 的 per-CPU run queue vector；每个 CPU queue 拥有 current thread、ready lane array 和 non-empty bitmap。第一版只启用一个 priority/domain lane，但 enqueue、schedule、yield 和 remove 都通过 selector/bitmap 形状进入 ready queue，不暴露长期 FIFO-only scheduler API。
- `kernel::state` 是 Phase 1 的 invocation executor 骨架：KernelState 统一持有 CapabilitySpace、ObjectTable、ThreadTable 和 Scheduler。`InvocationContext` 显式携带当前线程、当前 CPU、IPC payload 和可选 receiver-side Reply object destination，避免 executor 从薄参数里猜测消息寄存器或 reply 来源。`execute_invocation` 先调用 `kernel::invocation` 做 capability type/right 授权，再按授权结果查找真实对象并分派到 `kernel::thread::action` 事务入口。当前 executor 已覆盖 Endpoint send/recv、Notification signal/wait、Reply、TCB configure/resume、path-based CNode copy/mint/move/delete/revoke，以及 Untyped retype 到 Endpoint/Frame/CNode/Notification 的最小真实对象创建、nested Untyped 的 CSpace-only 创建和 TCB 的未绑定 object 创建；Endpoint call 成功创建 Reply cap 时通过 `ThreadWithReplyCap` 返回新 descriptor，Endpoint/Frame/CNode/Notification/nested Untyped/TCB retype 成功时通过 `Retyped` 返回新 descriptor。TCB retype 只创建 ObjectTable 中的未绑定 TCB object，不创建线程、不设置 affinity，也不触碰 scheduler；TCB configure 在检查 object 未绑定、ThreadId 未存在且 affinity CPU 已知后，绑定 ObjectTable 并插入 inactive TCB，但仍不自动入队。CNode retype 创建带 radix 和 planned window metadata 的 runtime object，并让 retyped CNode 可以通过 path invocation 使用 reserved CTE window；CNode backing storage 还不是 object-owned CTE array。Frame retype 当前只建立 runtime FrameObject metadata；Frame map 仍返回明确 unsupported outcome，直到 page-table ownership、address-space owner 和 mapping failure 事务边界接入。
- KernelState 的执行边界延续 seL4 decode/perform 分层：capability 授权、对象 lookup/type check、current-thread 验证、reply object distinctness、Reply cap install precheck、Reply cap metadata consistency 和 scheduler placement 等可恢复错误必须在 Endpoint queue、Reply pending state、TCB state、run queue 或 CSpace slot mutation 前完成。commit 阶段只消费已验证不变量；如果后续 syscall glue 绕过 executor 直接组合 object-local mutator，会重新引入半事务风险。
- `kernel::error` 承载 syscall-facing 的稳定错误码折叠：Capability、Invocation、ObjectTable、ThreadAction、Scheduler 和 KernelExecution 的 richer typed errors 会映射到少量 seL4 baseline `KernelErrorCode`。slot/object/thread 的诊断字段仍保留给模型测试和 debug context，但外部边界只消费 FailedLookup、InvalidCapability、IllegalOperation、InvalidArgument、RangeError 等稳定类别。
- `tools/qemu-runner/` 是根 workspace 外的宿主控制项目，对应 Asterinas OSDK/tooling 的方向。它负责在仓库根目录显式调用 `cargo build -p kernel-bin --target aarch64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem`，再用 `qemu-system-aarch64 -machine virt -cpu cortex-a53 -smp 2 -kernel ...` 启动。手动运行时串口接 `stdio`；smoke 模式使用显式 `-chardev file,... -serial chardev:...`，避免依赖 QEMU `-nographic` 的隐式串口重定向。runner 支持普通 boot smoke 和 `kernel-bin` feature-gated exception smoke，分别验证串口启动路径和 AArch64 exception vector 诊断路径。
- 根 workspace 的 default members 只包含能在 host target 下检查的 `kernel` 和 `ostd`。`kernel-bin` 仍是 workspace member，但它是 bare-metal image crate，按 Asterinas default-members/OSDK 和 Redox target-aware cookbook 的边界，用显式 `*-unknown-none` target 构建验证，不为 host `cargo check` 增加 OSTD boot/heap fallback。
- `.cargo/config.toml` 只保留 bare-metal targets 的 `panic=abort` rustflag，不全局启用 `build-std`。`build-std` 只属于裸机 kernel 构建；如果泄漏到 host tools，会让普通 `std` 依赖和重建的 `core/alloc` 发生 duplicate lang item 冲突。
- 裸机 `alloc` 由 `ostd::mm::heap` 提供 early heap：底层使用 `linked_list_allocator`，内存来自 OSTD 私有静态区域，初始化发生在 `kernel_main` 的最早阶段。它只支撑早期 Rust 数据结构和 capability smoke，不承担最终物理页框管理、boot memory map 解析或 seL4-style Untyped retype；后续 frame allocator 应在 OSTD 的 boot memory / page-frame 边界内演进。
- `ostd::mm::frame` 先承载物理页框的基本不变量：页大小、物理地址类型、boot memory region、页对齐区间、memory map 归一化、boot-reserved 区间扣除、单区间和多区间 early frame allocator，以及一次性初始化的 early frame allocator state/API。它参考 Asterinas early frame allocator 的方向，先把 boot-time frame ownership 从 heap 中分离出来；后续再把 bootloader/device-tree 提供的真实 memory map 和内核镜像、boot modules、early heap 等已占用物理区间接入归一化入口，并演进到 frame metadata 和 seL4-style Untyped 派生。这个 API 只服务早期 boot 阶段，不是最终全局 frame metadata allocator。
- CortenMM 对 Ousia 的近期启发是避免在设计初期建立两套互相竞争的地址空间真相源。后续 address space 应以页表结构、typed frame metadata 和 range guard 为权威边界，再向上暴露安全 cursor/mapper；不要先做一套复杂 VMA tree，再让页表成为滞后的副本。CortenMM 的多级页表锁协议和验证结构要等 page table、frame metadata 和 SMP 约束明确后再引入。
- AArch64 和 amd64 都是一等支持目标。当前本地 runner 先测试 AArch64；amd64 先保持 OSTD-owned boot stack、early COM1 console、halt loop 和裸机编译检查可用。
- AArch64 QEMU boot smoke 已按 multi-core-only baseline 使用双核拓扑。当前 OSTD AArch64 boot 通过完整 MPIDR affinity 判断 primary core，只让 primary 进入 `kernel_main`；secondary cores 在设置 shared boot stack 前进入 `wfe` hold loop，避免重复初始化 heap、exception vector 或串口。这个 hold loop 是 SMP bring-up 前的安全占位，不代表完整 secondary release、per-core stack、GIC/timer、IPI 或 TLB shootdown 已完成。`kernel_main` 目前会创建双 CPU `KernelState`，插入并绑定一个 CPU1 affinity 的 inactive TCB object，并验证它不会被自动入队，用于证明裸机 boot path 已消费多核内核状态。
- AArch64 direct boot 在进入 Rust 前建立 FP/SIMD 访问不变量，但 `kernel` core 不使用 FP，也不把 FP/SIMD 当作普通内核执行资源。seL4 的 AArch64 FPU 代码明确把 FP/SIMD 作为内核需要初始化、关闭、保存和恢复的 CPU 状态；Ousia 采用相同边界：FP 属于用户线程上下文和 OSTD arch-owned 状态管理，不进入 capability、IPC、scheduler、object 或 state 语义。SIMD 只允许用于明确的加速 leaf routine，例如 copy、checksum、crypto 或 compression，并且必须位于 OSTD/arch 边界，经显式 guard 处理 FPU/SIMD ownership、preemption/interrupt 约束和寄存器保存恢复；kernel core 只能通过架构无关 API 请求这些能力。早期 boot 可以由 `ostd` 临时建立访问不变量，避免 debug 构建中 Rust 生成的 FP/SIMD 指令直接异常；进入真正线程调度和用户态后，应演进为 seL4-style lazy FPU ownership，而不是长期全局开放。
- AArch64 PL011 early console 不再手写裸 offset，而是参考 `rust-sel4/crates/drivers/pl011` 的方式使用 `tock-registers` 建模 register block 和 bitfield，并在写 `DR` 前等待 `FR.TXFF` 清空。
- AArch64 exception vector 由 `ostd/src/arch/aarch64/exception.rs` 承载：它提供 vector table、VBAR 安装、异常寄存器快照和 early diagnostic policy。当前策略只用于早期诊断：同步异常、IRQ、FIQ 和 SError 统一打印 vector、ELR、ESR、FAR、SPSR 后停机。真正的 syscall、IRQ dispatch、timer tick 和用户态 fault 处理应在后续内核对象和调度边界明确后再接入。
- 本地运行需要 `qemu-system-aarch64` 在 `PATH` 中。没有 QEMU 时，runner 仍可完成 AArch64 kernel 构建，但最后启动会失败并报告缺少命令。

这个路径的目标只是先让 AArch64 内核跑起来，不是冻结最终启动协议。等内核能稳定进入 QEMU，再决定是否引入 UEFI/ELF loader、设备树解析、initramfs、签名验证、amd64 runner 和更完整的 Ousia boot 流程。

## Review 问题

- 当前实现是否仍然能被 reviewer 映射到 seL4 的 CSpace / CNode / Slot / Untyped / Endpoint / Notification / Reply / TCB 语义？
- Rust 类型、helper、error 和 generation 是否只表达 baseline 不变量，而不是改变 seL4 语义？
- 是否有任何 Portal、Operation、Continuation、浏览器、Package Cell、Service Graph、lease、session 或 Device Service 策略漏进了 Phase 1 kernel baseline？
- 当前 typed capability 是否足够支撑下一步 IPC 和 retype？
