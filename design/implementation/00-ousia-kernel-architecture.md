# 00 — Ousia Kernel Architecture Baseline

> 临时实现草案。本文指导近期代码推进，不是冻结后的 syscall ABI 或最终架构规范。稳定结论应回写到 `core/` 和 `topics/` 的 owning 文档。

## 背景

Ousia 的平台目标需要高级、易用、可组合的内核原语：Handle、Object、Channel、MemoryObject、Object Namespace、VFS/Object Store、Process/Thread、Driver boundary 和 Package Cell bootstrap。严格复刻 seL4 的 CSpace/Untyped/retype baseline 会把资源管理复杂度推给用户库，并阻碍内核态 VFS、VM 和对象管理器自然落地。

第一阶段内核路线因此改为 Ousia 原生高级 capability kernel：内核提供 handle/object、VM、IPC、VFS 边界和资源预算等一等机制；用户态看到的是稳定 handle API，而不是裸 CNode slot、Untyped retype 或 seL4-style CPtr plumbing。

Zircon/Fuchsia 是近期主要结构参考：handle rights、kernel object/dispatcher、VMO/VMAR、channel/call、driver manager 和用户态 `zx`/`fdio` 库。seL4 保留为能力安全参考：不可伪造 authority、rights 单调性、硬撤销、失败前置检查、最小权限和状态提交纪律。

## 阶段性目标

当前阶段目标是实现一个可运行、可测试的 Ousia kernel baseline。它不追求形式化验证，但必须通过清晰的不变量、类型边界、单元测试、集成测试、QEMU smoke 和 review discipline 保证足够的工程正确性。

这个 baseline 不复刻任何参考内核。它吸收 Zircon 的工程化对象模型和 seL4 的能力纪律，服务 Ousia 自己的长期抽象：Capsule、Capability、Communication Fabric、MemoryObject、Object Namespace、Service Graph、Package Cell 和 Driver Host。

## 目标

- 实现不可伪造的 `Handle` / `Capability` facade，绑定 kernel object、rights、generation 和 lifetime。
- 建立 kernel object manager，统一管理 process、thread、channel、event/notification、memory object、address space、file/object namespace node 和 device/resource object。
- 建立 VM/page allocator/kernel heap 或 slab/fixed-pool 边界，允许内核拥有 VFS、Object Store、page cache 和 object metadata 所需动态状态。
- 实现 channel/call 和等待原语，支持 bytes + handles、同步 call wrapper、异步 completion、cancel、timeout 和 late reply。
- 让 Object Namespace、VFS/Object Store 和 MemoryObject 在 Phase 1 进入主线裁决，而不是等 seL4 baseline 闭环后再映射。
- 从一开始按多核、失败无部分提交和热路径分配约束设计。

## 非目标

- 不复刻 seL4 C API、CSpace/CNode addressing、Untyped/retype 或 MDB 结构作为 Ousia public API。
- 不把 Fuchsia/Zircon 作为可直接复制的产品架构；参考事实必须经过 Ousia 需求过滤。
- 不开放不受控内核扩展接口。
- 不把 POSIX、Linux syscall 或传统全局文件系统语义作为原生接口。
- 不在热路径引入无边界、不可归属、不可回收的隐式内核分配。
- 不承诺第一阶段冻结最终 syscall ABI。

## 参考采用策略

Zircon/Fuchsia 参考重点：

- `third_party/fuchsia/zircon/kernel/object`：kernel object、dispatcher、handle rights 和对象生命周期。
- `third_party/fuchsia/zircon/kernel/vm`：VMO、VMAR、page fault、mapping 和 VM subsystem 边界。
- `third_party/fuchsia/zircon/system/ulib/zx`：用户态 `zx::*` handle wrapper 和 syscall API 人体工程学。
- `third_party/fuchsia/zircon/system/public`：syscall-facing 类型和 rights/handle ABI。
- `third_party/fuchsia/src/devices/bin/driver_manager`、`src/lib/driver`、`zircon/system/ulib/ddk`：driver manager、driver runtime 和 DDK 边界。

seL4 参考重点：

- capability authority 不可伪造。
- rights 只能收缩，不能凭空扩权。
- revoke/delete/finalization 需要严肃处理派生关系和对象生命周期。
- syscall 可恢复错误必须在 owner state mutation 前完成。
- 内核热路径应避免隐式分配、无界扫描和临时通用容器扩容。

## 核心模块方向

### 1. Handle and object manager

`kernel::cap` 当前 CSpace-like prototype 应演进为内部 handle/object rights substrate：

- 外部 facade 是 `Handle` / `CapabilityDescriptor` 这类不可伪造引用，携带 generation 以拒绝 stale handle。
- 内部 owner 是 kernel object manager 和 per-process handle table，而不是 seL4 public CSpace。
- `SlotId`、CNode window、Untyped retype 和 MDB-like lineage 可以保留为实验事实或撤销算法参考，但不能定义 Ousia public API。
- handle transfer 通过 channel/call、Portal 或显式 broker 边界完成；权限检查集中在 syscall/object boundary。

### 2. VM, allocator, and memory objects

内核需要一等 VM subsystem：

- OSTD 继续拥有 boot memory map normalization、early frame allocator、page tables、MMIO 和架构差异。
- Kernel 拥有架构无关的 VM object、address-space owner、mapping policy、fault routing、MemoryObject 和 page cache / pager boundary。
- 内核 allocator 可以采用 page allocator + slab/zone/fixed pool 的组合，但每类动态状态必须有 owner、quota/budget、reclaim 或退出条件。
- Commit 阶段不得临时发现可恢复分配失败；容量、slot、page、cache entry 和 metadata 应在 syscall decode/preflight 边界保留。

### 3. IPC and waiting

Ousia IPC 以 Zircon-style channel/call 和 Ousia Communication Fabric 为主线：

- Channel 支持 message bytes + handle transfer。
- 同步 call 是 write + wait + read 的内核/用户库封装，不要求 seL4 rendezvous/reply cap 结构。
- Portal fast call、Operation、Continuation、EventPort/WaitSet、Fence 和 SharedQueue 是 Ousia 长期通信族。
- seL4 Endpoint/Notification/Reply prototype 可作为同步 IPC 状态机参考，但不再约束最终 API。

### 4. Process, thread, and scheduler

线程和调度继续保持多核主路径：

- Process/Capsule 拥有 address space、handle table、thread set 和 resource budget。
- Thread 拥有 register context、affinity、priority、blocking state 和 wait membership。
- Scheduler 拥有 per-CPU run queues、current thread、priority/domain lanes 和 cross-core wakeup/TLB shootdown 边界。
- 早期 QEMU smoke 可以覆盖小实现面，但设计不能假设 single-core。

### 5. VFS, Object Namespace, and Object Store

Ousia 不把 VFS 视为用户态纯策略后置项。Phase 1 必须裁决并验证：

- Object Namespace 的路径解析、mount binding、provider root 和 handle cache 哪些属于内核。
- Object Store、metadata index、page cache 和 remote provider bridge 哪些属于内核或系统服务。
- `mmap` / MemoryObject / pager fault path 如何在内核 VM 与 provider 之间建立失败和取消边界。
- POSIX VFS 只属于兼容域投影，不能污染 native Object Namespace。

内核态 VFS/Object Store 若落地，必须显式声明资源预算、缓存回收、失败前置检查、锁/并发策略和 revoke 语义。

### 6. Driver boundary

默认方向仍是用户态驱动主逻辑，但内核要有高级设备对象与资源仲裁：

- Device/resource handles 控制 MMIO、IRQ、DMA、IOMMU mapping 和 reset authority。
- Driver Manager/Index/Host 参考 Fuchsia 的分层，但 Ousia 的 Device Service、Service Graph 和 Package Cell 负责最终策略。
- IOQueue、IOBuffer、Doorbell、Fence 和 shared memory data path 必须能表达 fast path，不把所有 IO 退化为同步 syscall。

## 当前代码定位

当前 `kernel/src` 的公开主线已经切到 Ousia-native skeleton：`handle`、`object`、`process` 和 `syscall`。旧 CSpace-like capability、Endpoint、Notification、Reply、TCB、ObjectTable、Scheduler 和 KernelState 源码仍可作为本地 prototype evidence 阅读，但不再从 `kernel/src/lib.rs` 导出，也不再约束 host integration tests。

当前 Slice 1 代码事实：

- `object::ObjectManager` 是 object id、kind、generation、lifetime 和 handle count 的单一 owner。
- `object::ObjectPayload` 已让 `Event`、`ChannelEndpoint`、`MemoryObject`、`AddressSpace` 和 `Thread` 拥有最小 runtime state；`ObjectKind` 由 payload 推导，而不是独立事实源。
- `handle::HandleTable` 是 process-local authority table；lookup 校验 handle entry generation、object generation snapshot、object kind 和 rights。
- `handle::HandleTable` 已有 process-local derivation lineage；`RevokeDescendants` 删除同一 process handle table 内从 root 派生出的 live descendants，跨进程 reverse index 尚未进入本 slice。
- `process::Process` 拥有 handle table、resource budget stub、address-space placeholder 和 thread-set placeholder。
- `syscall::Kernel` 提供最小 decode/preflight/commit 边界，先覆盖 bootstrap process、object handle creation、MemoryObject creation、channel pair creation、bounded channel send/recv、handle duplicate、close 和 revoke descendants。
- `ChannelEndpointObject` 已有 peer endpoint、fixed-capacity message slots、peer-closed state 和 bytes + handle transfer 的 host-side vertical slice。采用 Zircon/Fuchsia `ChannelDispatcher` 的 peer endpoint、peer queue、peer closed 和 bounded pending-message discipline；本 slice 暂不复制 txid/call waiter、signal observer、owner koid 或锁分层。
- `AddressSpaceObject` 已有固定容量 mapping slots，`MapMemoryObject` / `UnmapAddressRange` 通过 handle rights、object kind、generation、MemoryObject size bounds、range overlap 和 mapping-table capacity 检查后提交 metadata。采用 Zircon/Fuchsia VMAR/VMO 的“range owner + mapping metadata + create/map 失败前置”纪律；本 slice 暂不复制 VMAR tree、page table commit、pager fault path、cache policy 或 multi-level VM locking。
- 旧 CNode path、Untyped retype、seL4-specific invocation variants 和旧 executor tests 已从主线测试约束中移除。

旧 prototype 只继承语义，不继承形状：

- 保留：rights 单调性、generation/stale handle、失败无部分提交、typed object check、thread/scheduler transaction discipline。
- 演进：`CapabilitySpace` 已转向 per-process handle table / object manager；Endpoint/Reply 后续转向 channel/call；Frame/MemoryObject 后续转向 VM subsystem。
- 停止模仿：裸 CNode/CSpace public API、Untyped/retype 作为普通用户库资源申请模型、seL4-specific error ordering、MDB 作为产品 API。

## 近期代码步骤

1. 将 process-local revoke lineage 演进为可跨 process handle transfer 追踪的 reverse index 或 generation domain，并比较 revoke 成本、内存占用和 hot-path lookup 成本。
2. 设计 resource reservation token，使 object creation、handle installation、queue entry、VM mapping 和 namespace cache 的失败都发生在 commit 前，并替换当前局部 preflight helper。
3. 在 channel vertical slice 上补 call txid / response correlation 和 wait/wake boundary，但不要在没有 thread scheduler owner 接入前复制 Zircon call waiter 形状。
4. 将 `AddressSpaceObject` 从固定 mapping slots 演进到可保留 metadata/page-table resources 的 VM commit plan，明确 page table owner、TLB shootdown、多核 locking 和 pager fault 边界。
5. 设计 Object Namespace/VFS/Object Store 的内核/服务放置 proposal，比较 kernel-resident、user-service 和 hybrid 三种方案。
6. 保持 QEMU AArch64 smoke 与 host tests 双线验证；涉及 Rust source 时按 workflow 运行 `cargo fmt`、`cargo fmt --check`、`cargo check` 和 targeted tests。

## Review 问题

- 文档或代码是否仍把 seL4 CSpace/Untyped/retype 写成 Ousia Phase 1 governing baseline？
- Fuchsia/Zircon 参考是否被直接当成 Ousia 规范，而没有说明采用、调整或拒绝理由？
- Kernel VM/allocator/VFS 动态状态是否有 owner、quota/budget、reclaim、preflight 和 hot-path 约束？
- Handle/object rights 是否仍满足不可伪造、不可扩权、可撤销和 stale handle 明确失败？
- Native API 是否保持高级、易用、Ousia-owned，而不是暴露参考内核的底层 plumbing？
