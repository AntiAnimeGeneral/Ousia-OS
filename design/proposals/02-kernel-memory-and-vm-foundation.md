# 02 — Kernel Memory And VM Foundation 提案

> Proposal packet。本文用于交接给 implementation agent 执行。通过 review 和实施后，稳定结论应回写到 [00-ousia-kernel-architecture.md](../implementation/00-ousia-kernel-architecture.md)、[03-pager-and-memory.md](../core/03-pager-and-memory.md) 或代码 rustdoc；本文本身不作为长期规范源。

## 用户目标

用户追问 Zircon 是否有自己的内核内存分配器，并指出实现时不能给动态分配失败留口子。目标是判断 Ousia 是否应该先实现 kernel memory allocator 和 VM 组件，再继续扩展 handle/object/channel/VFS。用户倾向是：所有内核都需要这套 foundation，而且 allocator/VM 应尽早抽象和解耦。

本提案给出更具体的实施建议：在继续扩大 object/handle/channel/namespace 之前，先建立 Ousia kernel memory foundation：physical frame metadata、page allocator、kernel heap/slab/fixed pool、reservation token、minimal MemoryObject/AddressSpace/VM mapping owner 和 page-table boundary。它不是完整 VM，也不是用户态 VFS/pager；它是后续所有动态 kernel state 的失败边界和资源真相源。

## Mode And Target

- Mode：新模块。
- Target：代码。
- Scope：`ostd/src/mm/**`、`kernel/src/**` 中的 memory/resource/vm 相关模块、`kernel/tests/**`，必要时同步 `design/implementation/**` 和 `design/core/03-pager-and-memory.md`。
- Reference：Zircon/Fuchsia 的 PMM/kernel heap/VMO/VMAR/address-space/object create 分配失败模式是主参考；Asterinas/ostd 可参考 boot memory 和 page-table 工程边界；seL4 只提供 Untyped/frame capability discipline 参考，不定义 Ousia memory API。

## 背景与现状

当前代码已经有三块早期基础：

- `ostd/src/mm/frame.rs`：boot memory map normalization、reserved range subtraction、early frame allocator 和 `FrameAllocError`。
- `ostd/src/mm/heap.rs`：固定 1 MiB early heap 和 `linked_list_allocator::LockedHeap` global allocator；`alloc_error_handler` 当前会打印并 `wait_forever()`。
- `kernel/src/object/mod.rs` 与 `kernel/src/syscall/mod.rs`：Ousia-native `ObjectManager`、`HandleTable`、`Process`、`Syscall`、fixed-capacity `ChannelEndpointObject`、`MemoryObject`、`AddressSpaceObject` 和 mapping metadata skeleton。

这些代码证明了方向，但还不是完整 foundation：

- early frame allocator 是 boot-time facility，不是长期 PMM。
- early heap 是 smoke/bring-up 设施，不是 kernel object allocator。
- kernel object/handle/process table 仍主要依赖 fixed capacity `Vec` 初始化和局部 preflight helper，没有统一 allocation context / reservation token。
- `MemoryObject` 已有 page-aligned size、mapping policy、eager contiguous runtime frame backing、active mapping count 和 last-reference frame reclaim；尚未连接 page cache、pager-backed state 或真实 page table commit。
- `AddressSpaceObject` 只有固定 mapping slots，尚未建立 VMA/VMAR owner、page-table owner、TLB invalidation boundary 或 fault routing。
- 当前 VM tests 已覆盖 mapping overlap、bounds、rights 和 no partial metadata mutation，但还不能证明 page allocation、metadata allocation 或 page table mutation 失败无副作用。

Zircon 参考说明这条 foundation 是必须的：Zircon 有 PMM、kernel heap、VM object/VMAR/address-space、page-list/page-table metadata，并在 object/VM create path 用 `AllocChecker`、`ZX_ERR_NO_MEMORY` 和局部回滚显式处理分配失败。Ousia 不能在更高级的 native API 下弱化这条纪律。

Asterinas 研究线补充了另一类参考：不是用户可见对象 API，而是 Rust kernel 内存和验证如何避免变成安全口号。

- CortenMM 把 memory management 作为 clean-slate transactional system 处理，同时追求性能和同步正确性。它对 Ousia 的直接启发是：VM 操作不能只是若干 owner 各自加锁后逐步修改；`map`、`unmap`、`protect`、fault commit、future CoW 和 demand paging 都应走统一事务接口，先形成 reservation/commit plan，再一次性发布 owner state。
- Converos 说明 Rust kernel 的关键并发模块可以用多层、多粒度规格和模型检查验证。Ousia 不需要从第一天验证全系统，但 frame lifecycle、reservation token、channel call、handle revoke 和 VM fault commit 这类小状态机应设计成能被 TLA+/PlusCal 或 Verus 描述。
- RusyFuzz 说明 Rust kernel 的 panic-prone paths 是独立 bug class。Ousia 的 syscall/object/VM/IPC 边界应把 `unwrap`、`expect`、unchecked index、overflow assertion 和 panic in recoverable path 当成 fuzz 目标。
- MlsDisk 属于后续可信存储和 TEE 路线。它的 layered secure logging 值得未来的 secure block device、Package Cell sealed storage 或 rollback-protected object store 参考，但不进入当前 allocator/VM foundation scope。

## 目标

1. 建立长期 physical frame metadata 和 page allocator owner，替代 early frame allocator 作为 runtime truth source。
2. 建立 kernel allocation context：heap/slab/fixed pool 或 zone 的组合，并为 object、handle、channel queue、VM range、page table metadata 和 namespace cache 提供 reservation token。
3. 明确 `NO_MEMORY`、`NO_CAPACITY`、`QUOTA_EXCEEDED` 的边界和测试入口。
4. 建立最小 VM foundation：`MemoryObject`、`AddressSpace`、VMA/VMAR mapping metadata、page-table boundary、fault routing skeleton。
5. 让 existing handle/object/process/syscall skeleton 消费 reservation，而不是直接使用临时 `Vec`/fixed table 作为最终资源模型。
6. 从第一版就按 multi-core-only 设计 allocator locks、per-CPU cache placeholder、TLB invalidation boundary 和 page-table mutation serialization。
7. 让 VM 主路径具备 CortenMM 式事务形态：descriptor decode、rights/lifetime validation、resource reservation、commit plan construction 和 state publication 必须分阶段，且 commit 阶段不再发现可恢复分配失败。

## 非目标

- 不实现完整 pager-backed filesystem、Object Store、remote provider 或 full mmap semantics。
- 不实现完整 buddy allocator、NUMA、huge page、page compression、swap 或 production-grade reclaim。
- 不把 early heap 演进成最终 kernel heap。
- 不把 VM 写成与 handle/object/process/scheduler 无关的孤立库。
- 不复制 Zircon VM class hierarchy 或 Fuchsia policy；只采用它的 object/VM allocation discipline 和边界经验。
- 不在 commit 阶段调用可能失败的 allocator、page table node allocation、metadata map insert 或 queue growth。

## 现有模式判断

### 应继承

- `ostd::mm::frame` 的 boot memory map normalization、reserved range subtraction 和 explicit `FrameAllocError`。
- `kernel` 当前 fixed-capacity object/channel/address-space tests 中“失败后 owner state 不变”的行为契约。
- `KernelError::NoMemory` / `NoCapacity` / quota 类错误分离方向。
- Ousia-native handle/object/process/syscall boundary。

### 应重建

- runtime PMM、frame metadata、kernel heap/slab/fixed pool、reservation token 和 VM range owner 都应新建为长期模块。
- `AddressSpaceObject` 的 fixed mapping slots 应重建为可 reservation 的 mapping metadata owner；第一版可以 fixed-capacity，但 API 必须表达 future VMA/VMAR owner。
- `MemoryObject` 应从 metadata-only object 重建为 memory owner：当前保留 size、rights-compatible mapping policy 和 runtime frame owner evidence；backing taxonomy、page/cache metadata hook 和 pager state 只在对应 owner boundary 落地时引入。

### 应停止模仿

- 把 early heap 当成所有内核动态状态的长期 backing。
- 用 object table capacity 代替 process budget 或 allocator reservation。
- 让 VMA metadata、page table entry 和 page cache 三方各自成为 mapping truth。
- 在 syscall commit 中临时发现 page allocation、metadata allocation 或 handle slot allocation 失败。

## 候选方案

### 方案 A：继续先做 handle/object/channel，allocator/VM 后置

做法：保持现在的 fixed-capacity object manager、handle table、channel queue 和 address-space mapping slots，继续向 IPC、namespace、resource 扩展；等功能变多后再统一 allocator/VM。

优点：短期功能推进快，测试写起来简单。

不选择原因：这会把每个 subsystem 的容量和失败边界各自做一套。等 VFS/Object Namespace、page cache、driver resource 和 channel queue 扩大后，统一 reservation 会变成跨模块拆墙。更严重的是，dynamic allocation failure 会长期被 fixed table 或 test-only capacity 掩盖。

### 方案 B：先做完整 VM，再回到 handle/object

做法：暂停 object/handle/channel，先实现完整 PMM、kernel heap、VMO/VMAR、page fault、pager-backed memory 和 page table commit。

优点：VM foundation 扎实。

不选择原因：完整 VM 会牵涉 scheduler、fault handling、pager IPC、Object Namespace 和 driver DMA。当前还没有足够对象/handle/process 边界承载这些协作。一次做完整 VM 会把内存模块变成孤立大工程，反而难 review。

### 方案 C：先做 allocator + minimal VM foundation，再接回 object/handle

做法：先建立 runtime PMM、frame metadata、kernel allocation context、reservation token 和最小 MemoryObject/AddressSpace/VMA owner。第一版不追求完整 pager/VFS mmap，只要求 object creation、handle install、channel message、VM mapping 和 namespace/resource skeleton 都能通过统一 reservation/preflight 表达失败边界。

优点：解决最早会污染全局的资源 owner 和失败模型，同时保持 scope 可 review。后续 handle/object/channel/VFS 都能消费同一套 allocation context。

推荐：采用方案 C。

## 推荐架构

### 模块边界

| 模块                                   | 职责                                                                                        | 不应拥有                                          |
| -------------------------------------- | ------------------------------------------------------------------------------------------- | ------------------------------------------------- |
| `ostd::mm::boot` 或现有 `frame` 子模块 | boot memory map normalization、reserved range subtraction、early frame allocation           | runtime frame ownership、process quota、VM policy |
| `kernel::memory::frame`                | runtime frame metadata、free lists、frame allocation/free、pin/mapping count hooks          | page-table policy、VFS page cache policy          |
| `kernel::memory::heap`                 | kernel heap/slab/fixed pool/zone owner、fallible allocation API                             | object lifecycle、syscall error mapping           |
| `kernel::memory::reservation`          | reservation token、rollback, commit consumption, allocation context                         | subsystem-specific state transition               |
| `kernel::vm::memory_object`            | MemoryObject size、mapping policy、runtime frame owner evidence；future page-cache boundary | handle rights table、path namespace policy        |
| `kernel::vm::address_space`            | AddressSpace、VMAR/VMA range owner、mapping metadata、page-table boundary                   | process handle table、scheduler queue             |
| `kernel::vm::fault`                    | fault descriptor、future pager/provider handoff、cancel/error skeleton                      | filesystem provider implementation                |
| `kernel::resource`                     | process/capsule budget, quota accounting, resource limits                                   | physical allocator internal free list             |

### 依赖方向

- `ostd` owns boot/platform primitives；kernel consumes normalized boot memory facts and arch page-table operations through explicit boundary APIs.
- `kernel::memory` owns runtime frame/heap/slab allocation and reservation.
- `kernel::vm` owns MemoryObject/AddressSpace policy and mapping metadata, and consumes `kernel::memory` reservation tokens.
- `object` and `process` own object lifetime and process budgets, but they do not allocate directly; they request reservation from `kernel::memory` / `kernel::resource`.
- `syscall` decodes and maps public errors; it does not own allocator internals.

## 核心数据模型

### Frame Metadata

Frame metadata must be able to grow toward:

- physical address or frame index
- state: free, reserved, kernel, user, device, page-table, cache, pinned
- owner: kernel, process/capsule, MemoryObject, device resource, page table
- refcount or mapping count
- pin count
- reclaim eligibility
- generation or poison marker for debug

第一版可以用 fixed-capacity array 或 bitmap，但 type shape must expose ownership and future reclaim hooks.

### Allocation Context

Every syscall or kernel operation that may allocate receives or constructs an allocation context:

- process/capsule budget snapshot
- requested object count / handle slots / queue entries / frame count / metadata nodes
- preflight result
- reservation token list
- rollback path

Reservation token is consumed by commit. Dropping an uncommitted token returns resources.

### VM Transaction Interface

VM operations must be expressed as transactions rather than scattered side effects. The first version does not need CortenMM-level sophistication, but the API shape must preserve the same correctness affordance:

1. Decode user/kernel descriptor and normalize ranges.
2. Validate object type, rights, lifetime, mapping policy and alignment.
3. Reserve every dynamic resource: VMA node, page-table metadata, frame/page materialization, TLB invalidation intent and quota.
4. Build an exclusive VM reservation token such as `VmMapReservation` or `VmUnmapReservation` containing the publication intent.
5. Consume the reservation in one publication path.
6. Drop uncommitted reservations without mutating AddressSpace, MemoryObject, frame metadata or page-table placeholder.

This interface is the local Ousia lesson from CortenMM: correctness comes from making the synchronization boundary explicit, not from hoping each subsystem's local lock order remains compatible.

### MemoryObject

`MemoryObject` is the kernel-visible memory object, not a frame list exposed to userspace:

- page-aligned non-zero size
- rights-compatible mapping policy
- eager contiguous runtime frame backing evidence for the first anonymous memory slice
- active mapping count used to delay frame reclaim until no AddressSpace mapping references the object
- future page-cache metadata only when a real pager or page-cache owner exists
- future zero-fill, CoW and pager fault endpoint only when their state owner exists

Do not add future backing taxonomy before the backing owner exists. A single variant backing enum, an unused backing field or an `anonymous` constructor is not a harmless placeholder; it hides the fact that the final owner has not been designed. Until pager/page-cache state exists, MemoryObject exposes only the current facts above. Eager contiguous backing is an implementation slice for physical anonymous memory, not a compatibility layer for future SSD、pager、CoW 或 page-cache semantics.

MemoryObject creation must go through an explicit size descriptor. Generic `CreateObject(MemoryObject)` must not synthesize a zero-sized placeholder object; without page-aligned non-zero size, the runtime frame owner cannot be attached without changing semantics. Creation preflights process quota, handle slot, object entry and contiguous frame reservation before publishing the object or handle; frame exhaustion must leave all public owner state unchanged.

MemoryObject frame reclaim is driven by object lifetime plus mapping references: closing the last handle destroys and frees an unmapped MemoryObject, while a mapped MemoryObject keeps its frames until the final unmap removes the AddressSpace reference. Generic object destruction is not a MemoryObject reclaim path because it cannot free frame ownership; MemoryObject entries are removed only by unpublished creation rollback or after the reclaim path has freed their frames. Frame ownership records the MemoryObject slot and generation so reclaim cannot release frames with stale object-generation evidence. Process-local descendant revoke removes derived MemoryObject handles through the same close/reclaim path while preserving the root handle as authority; mapped frames remain reserved until the final unmap. Current tests cover the single-process handle/map/unmap/revoke path; cross-process shared mappings and hard revoke of root authority remain later lifecycle slices.

### AddressSpace and Mapping

`AddressSpace` owns range metadata:

- VMAR/VMA range tree or fixed-capacity range set in first version
- mapping rights
- MemoryObject reference + generation snapshot
- offset and size
- OSTD page-table update intent when map has runtime frame owner evidence, or unmap has hardware-state removal work to describe
- OSTD TLB invalidation intent and pending-work storage for page-table removals that need later coherence work

VMA is the policy/source-of-truth for virtual ranges; page table is committed hardware state. They must not compete as two mapping truth sources.

Map reservations may carry an OSTD page-table map intent only after MemoryObject can name owned runtime frames. Unmap reservations may carry an OSTD page-table unmap intent and TLB invalidation intent, because those are hardware-state work descriptions for removing a mapping once page-table ownership exists.

Committed unmaps publish pending TLB invalidation work under the AddressSpace owner, and a later flush boundary can consume that work so fixed pending storage is not a permanent unmap limit. Consumption only transfers the invalidation intent out of AddressSpace metadata; it is not proof that an architecture TLB shootdown, ordering barrier or completion has happened.

When map intents become valid, their physical input must come from frame-owner evidence such as OSTD `FrameRange`; page-table code should not introduce a second physical-range type that repeats frame allocator alignment and ownership invariants.

VM map/unmap descriptors must establish hardware page-granularity before metadata publication: virtual range base/size and MemoryObject offset are page-aligned, non-empty and overflow-checked at the VM boundary. Kernel VM may consume OSTD `VirtualRange` as the normalized virtual range fact without reimplementing separate page-range rules.

The current fixed mapping slots, OSTD page-table update intent and fixed pending TLB invalidation storage are incomplete final-boundary scaffolding, not stable abstractions. They must stay marked with adjacent TODOs that name the missing final owner, the semantics callers cannot rely on and the tests required to exit the scaffold. Do not make them look more complete by adding single-variant operation enums, future-only fields or compatibility facades.

## Error Boundary

Public memory errors must preserve distinct behavior:

- `NO_MEMORY`: allocator/heap/slab/page metadata cannot allocate.
- `NO_CAPACITY`: fixed table, queue, range set, handle table or reservation pool is full.
- `QUOTA_EXCEEDED`: process/capsule budget cannot cover the request.
- `INVALID_ARGUMENT`: malformed range, zero size, overflow, unaligned input.
- `MISSING_RIGHTS`: handle rights do not permit mapping/protection.
- `WRONG_OBJECT_TYPE`: handle does not name MemoryObject/AddressSpace/resource.

Failure ordering: validate cheap descriptor shape first, then handle/type/rights/lifetime, then capacity/quota/allocation reservation, then commit. Tests should assert both error category and unchanged owner state.

## Implementation Slices

### Slice 0：现状审计和入口封锁

- Mark `ostd::mm::heap` as early heap only; do not let new kernel code treat it as final allocator.
- Audit all `Vec`, fixed arrays, message buffers, mapping slots and object/handle table growth paths in current `kernel/src`.
- Add comments or TODOs only where they identify owner, reservation plan and exit condition; avoid noise comments.
- No behavior change required, but output a short implementation note listing dynamic state owners.

### Slice 1：Runtime frame metadata and page allocator

- Introduce kernel runtime frame metadata owner fed by `ostd` normalized memory map.
- Preserve reserved ranges for kernel image, boot data, device tree, MMIO and early heap.
- Provide fallible frame allocation/free with `NO_MEMORY`.
- Add tests for reserved range exclusion, alignment, exhaustion and double-free or wrong-owner rejection.

### Slice 2：Kernel allocation context and reservation tokens

- Introduce reservation token types for object entry, handle slot, queue entry, VM mapping node, page-table metadata and frame allocation.
- Integrate process/capsule budget with reservation.
- Ensure dropping uncommitted token rolls resources back.
- Replace direct object/handle/channel capacity checks with reservation API before later slices depend on those paths. A direct check may survive only as a local implementation detail behind the reservation owner, not as a second public preflight path.
- Produce a dynamic-state reservation matrix before moving to Slice 3. The matrix must list each state class, its owner, capacity source, public error category, reservation token, commit consumer and rollback test.

Required initial matrix rows:

| Dynamic state                          | Owner                                                | Error category                                 | Reservation evidence                                                           |
| -------------------------------------- | ---------------------------------------------------- | ---------------------------------------------- | ------------------------------------------------------------------------------ |
| object entry                           | `ObjectManager` through memory reservation           | `NO_MEMORY` / `NO_CAPACITY` / `QUOTA_EXCEEDED` | object creation failure leaves no object entry and no handle                   |
| handle slot                            | `Process` / `HandleTable` through memory reservation | `NO_CAPACITY` / `QUOTA_EXCEEDED`               | install/transfer failure leaves source and destination handle tables unchanged |
| channel queue entry and message buffer | `ipc` / channel object through memory reservation    | `NO_MEMORY` / `NO_CAPACITY` / `QUOTA_EXCEEDED` | send failure leaves sender handles, receiver queue and peer state unchanged    |
| VM mapping node                        | `vm::AddressSpace` through memory reservation        | `NO_MEMORY` / `NO_CAPACITY` / `QUOTA_EXCEEDED` | map failure leaves VMA metadata and page-table placeholder unchanged           |
| frame allocation                       | `memory::frame` through frame reservation            | `NO_MEMORY` / `QUOTA_EXCEEDED`                 | failed frame reservation leaves frame metadata unchanged                       |
| page-table metadata                    | `vm::AddressSpace` + OSTD page-table boundary        | `NO_MEMORY` / `NO_CAPACITY`                    | failed map leaves page-table placeholder and TLB state unchanged               |

### Slice 3：Kernel heap/slab/fixed-pool boundary

- Design fallible kernel allocation API for object metadata and subsystem-owned pools.
- Keep early heap as bootstrap backing only; long-term allocations go through memory owner API.
- Use fixed pool or slab for hot-path object/table entries where possible.
- Add allocation failure injection tests for object creation, handle install and channel message preflight.

### Slice 4：Minimal VM/MemoryObject foundation

- Move `MemoryObject` and fixed `AddressSpaceObject` mapping set into `kernel::vm` owner modules.
- Define MemoryObject size、mapping policy and eager contiguous runtime frame backing; do not add a backing taxonomy before a real pager/page-cache owner exists.
- Define AddressSpace range owner and page-table boundary placeholder.
- Make `MapMemoryObject` reserve mapping metadata and build OSTD page-table map intent from frame owner evidence before commit.
- Track active mappings so last-handle close only reclaims unmapped MemoryObjects, and final unmap can reclaim a handle-less MemoryObject.
- Introduce an exclusive VM reservation token even if it only covers metadata in the first slice; do not let syscall code mutate AddressSpace, MemoryObject and page-table placeholder directly.
- Expand existing `kernel/tests/vm.rs` to cover allocation/reservation failure no partial state.

Current exclusion: cross-process shared MemoryObject mappings, hard revoke of root authority and real page-table teardown remain separate lifecycle/page-table slices.

### Slice 5：Page fault and pager skeleton

- Add fault descriptor type: address, access kind, address space, current thread/process, MemoryObject mapping snapshot.
- Add pager/provider handoff placeholder without implementing full provider.
- Define cancel/error path and state owner when fault cannot be resolved.
- Keep demand paging and future CoW compatible with the transaction interface: fault resolution builds a commit plan before publishing frame metadata, MemoryObject page state or page-table state.
- Do not add full filesystem mmap yet; only make the VM boundary ready.

### Slice 6：Wire object/handle/channel to memory foundation

- Object creation consumes object-entry reservation.
- Handle install/transfer consumes handle-slot reservation.
- Channel send consumes queue-entry/message-buffer/destination-handle reservations.
- VM map/unmap consumes VM reservation and updates AddressSpace owner.
- Tests verify every failure path leaves all involved owners unchanged.

## Testing Strategy

- Unit tests for frame allocator and reservation token rollback.
- Host integration tests through syscall boundary for object creation, handle install, channel transfer and VM mapping failure.
- Allocation failure injection tests where each reservation step can fail independently.
- Mapping tests asserting AddressSpace mapping metadata, frame metadata and page-table placeholder remain unchanged after failure.
- Transaction tests for VM reservations: validation, reservation failure or dropped uncommitted tokens must leave every owner unchanged; commit success must have one visible publication point.
- Multi-core boundary tests can start as model assertions: page-table mutation records pending TLB invalidation work rather than silently assuming single-core.
- Converos-style model candidates: frame metadata lifecycle, reservation token lifecycle, channel call wait/wake, handle revoke lineage and VM fault commit. These should stay small enough for TLA+/PlusCal or Verus-style specifications before implementation grows concurrent shortcuts.
- RusyFuzz-style fuzz targets: syscall descriptors, handle values, VM ranges, object ids, IPC message lengths and allocation failure injection should actively search for panic-prone paths such as unchecked indexing, failed `unwrap`/`expect`, arithmetic overflow and impossible-state assertions reachable from external input.
- QEMU smoke only when boot/OSTD/platform path changes.

## Validation Commands

For docs-only edits:

- `deno task --cwd .github/skills/doc-validation check:docs --config ../../../design/check-docs.config.json`

For implementation slices touching Rust source:

- `cargo fmt`
- `cargo fmt --check`
- `cargo check`
- `cargo nextest run -p kernel`

If `ostd` boot/platform, linker or `kernel-bin` entry changes, add the matching QEMU smoke used by the repository at that time.

## Rollback

Rollback is slice-based. If a memory slice fails review or tests, revert that slice to the previous passing Ousia-native state. Do not recover by letting subsystems allocate directly or by hiding failure behind panic/early heap fallback.

## Document Ownership

- Stable memory/VM conclusions: [03-pager-and-memory.md](../core/03-pager-and-memory.md) and [00-ousia-kernel-architecture.md](../implementation/00-ousia-kernel-architecture.md).
- Proposal handoff: this file.
- External reference facts: `design/notes/reference/**`.
- Hard implementation rules: `.github/instructions/ousia-kernel-boundaries.instructions.md` and `.github/instructions/implementation-quality.instructions.md`.

## Evidence Read

- [00-ousia-kernel-architecture.md](../implementation/00-ousia-kernel-architecture.md): current Ousia-native route and existing handle/object/syscall skeleton facts.
- [06-roadmap.md](../topics/06-roadmap.md): Phase 1b already names page allocator、kernel heap/slab、VMO/MemoryObject、VMAR/address-space as baseline.
- `.github/skills/_shared/reference/memory-and-address-space.md`: memory planning prompts and review attacks.
- `ostd/src/mm/frame.rs`: current boot memory normalization and early frame allocator.
- `ostd/src/mm/heap.rs`: current fixed early heap and panic-like allocation error path.
- `kernel/src/object/mod.rs`, `kernel/src/syscall/mod.rs`, `kernel/tests/vm.rs`: current MemoryObject/AddressSpace metadata skeleton and host VM behavior tests.
- Zircon evidence already captured in [01-ousia-native-kernel-refactor.md](./01-ousia-native-kernel-refactor.md): `AllocChecker` / `ZX_ERR_NO_MEMORY` create paths and VM page-list rollback.
- Asterinas research line:
  - CortenMM: transactional memory management with strong synchronization correctness guarantees; use as pressure for Ousia VM reservation tokens, demand paging and future CoW boundaries.
  - Converos: practical model checking for Rust OS kernel concurrency; use as validation reference for small critical state machines.
  - RusyFuzz: unhandled-exception guided fuzzing for Rust OS kernels; use as fuzzing reference for recoverable kernel boundaries that must not panic.
  - MlsDisk: layered secure logging for trusted block storage in TEEs; record for future secure storage proposals, not this VM foundation scope.

## Open Questions

1. Runtime frame metadata lives in `kernel::memory` or `ostd::mm`? Recommendation: `ostd` normalizes boot/platform facts; kernel owns runtime frame metadata and policy.
2. First allocator shape: fixed pool, slab, buddy, bitmap, or hybrid? Recommendation: bitmap/frame allocator + fixed pool/slab for object metadata first; defer buddy sophistication.
3. Should `KernelError` expose `NoMemory`, `NoCapacity`, `QuotaExceeded` now? Recommendation: yes, before more subsystems depend on a vague error.
4. How much page-table work belongs in Slice 4? Recommendation: metadata and boundary placeholder first; hardware page table commit only after reservation model is tested.
5. Does page cache belong to VM or VFS? Recommendation: VM owns page/cache metadata mechanics; Object Namespace/VFS owns naming and provider policy.
6. How much CortenMM should Ousia absorb immediately? Recommendation: absorb the transaction interface and verification pressure now; defer its full performance design until AddressSpace, MemoryObject and page fault owner states exist.

## Residual Risks

- Without actual page-table commit, Slice 4 still cannot prove hardware mapping correctness.
- Without scheduler/thread fault handling, page fault skeleton cannot prove wake/cancel semantics.
- A too-generic allocator abstraction could hide hot-path costs; review must require capacity source and allocation complexity evidence.
- A too-local allocator implementation could force later VFS/driver/IPC rewrites; review must require shared reservation API before subsystem-specific growth.
- Copying CortenMM terms without the transaction boundary would create false confidence. Review must ask where the reservation token is built, which owner publishes state, and which tests prove failed or dropped reservations leave no partial state.

## Review Focus

- Does the proposal prevent early heap from becoming final allocator by accident?
- Are PMM、kernel heap/slab、reservation、MemoryObject、AddressSpace and page-table boundary owned by distinct modules with clear dependencies?
- Are `NO_MEMORY`、`NO_CAPACITY` and `QUOTA_EXCEEDED` kept separate?
- Does every dynamic state list owner, capacity source, preflight/reservation and failure rollback?
- Does VM remain a core subsystem connected to handle/object/process/scheduler, rather than an isolated library?
- Does every VM mutation route through a transaction/commit-plan boundary instead of scattered owner side effects?
- Are Converos/RusyFuzz references translated into concrete model/fuzz targets rather than cited as prestige references?
