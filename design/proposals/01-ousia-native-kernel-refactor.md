# 01 — Ousia Native Kernel 推倒重来提案

> Proposal packet。本文用于交接给 implementation agent 执行。通过 review 和实施后，稳定结论应回写到 [00-ousia-kernel-architecture.md](../implementation/00-ousia-kernel-architecture.md)、相关 `core/**` owning 文档或代码 rustdoc；本文本身不作为长期规范源。

> Implementation status：本文的 greenfield 方向已经开始落地。当前 `kernel/src` 实现树只保留 Ousia-native `error`、`handle`、`object`、`process` 和 `syscall` 主线；旧 CSpace/Untyped/invocation/Endpoint/Reply/TCB/Scheduler prototype 源码已移出实现树。后续以 owning implementation 文档和当前源码为准，不再把旧文件布局视为迁移约束。

## 用户目标

用户明确指出严格 seL4 baseline 路线走偏：Ousia 需要高级、易用、可组合的内核设计，内核内部需要能自然承载 VM、VFS/Object Namespace、allocator、Object Store metadata 和 driver/resource handles。当前任务要求“提案激进一点，不要妥协，就给一个最激进的推倒重来都可以的方案”。

本提案目标是给出一次 greenfield replacement handoff：允许整体替换 `kernel/src/**` 和 `kernel/tests/**` 的主线结构，旧 seL4 prototype 只作为只读证据库，不作为迁移底座。不保留旧 API、旧测试 helper、旧 `Invocation` variant、旧 `CapabilitySpace` facade、旧 CNode/Untyped 调用面或旧文件布局。实现应先建立 Ousia-native skeleton，再按新行为契约补测试；不是在旧内核上慢慢套 facade。

## Mode And Target

- Mode：重构。
- Target：代码。
- Scope：`kernel/src/**`、`kernel/tests/**`，必要时同步 `design/implementation/**` 和 `core/**` 的 owning 结论。
- Reference：Zircon/Fuchsia 是主参考，覆盖 handle/object/rights、VMO/VMAR、channel/call、typed wrapper 和 driver framework；seL4 只作为高保证能力系统的安全审查参考，不再定义 Ousia 内核结构。

## 背景与约束

重构前的 `kernel` 曾经拆成 `cap`、`object`、`invocation`、`ipc`、`notification`、`reply`、`thread`、`scheduler` 和 `state` 等模块。它证明了若干重要语义：rights 只能收缩、slot/object generation 可以拒绝 stale descriptor、retype/cnode 操作能做 preflight 后 commit、Endpoint/Notification/Reply/TCB/Scheduler 有可测试状态机，失败路径能保护部分 owner state 不被提交。

这些删除前的结构以 seL4-style prototype 为中心：

- `kernel/src/cap/space.rs` 暴露 `CapabilitySpace`、`SlotId`、`CNodePath`、`UntypedCap`、`RetypeTarget`、MDB-like lineage 和 CNode window 语义。
- `kernel/src/invocation/mod.rs` 的 `Invocation` 仍以 Endpoint/CNode/Untyped/TCB/Notification/Reply 操作为主要 syscall 形态。
- `kernel/src/object/table.rs` 的 `ObjectTable` 是独立 runtime namespace，和 capability owner、process owner、VM owner 尚未统一。
- `kernel/src/state/kernel.rs` 作为跨 owner 编排者，先从 `CapabilitySpace` 授权，再分散修改 `ObjectTable`、`ThreadTable`、`Scheduler`、Endpoint/Notification/Reply runtime state。
- `kernel/tests/**` 曾大量围绕 CNode path、Untyped retype、seL4-like endpoint/reply semantics 写 host integration。

这些形态不再是稳定约束，也不是必须迁移的中间层。它们属于历史 prototype evidence：可以帮助识别哪些不变量值得重新实现，但不能继续定义 Ousia public API、Phase 1 模块边界、测试结构或代码组织。当前实现树已经删除这些旧模块，接手实现时不要按旧路径寻找兼容入口。

动态分配失败必须作为内核设计的一等错误边界处理，不能用“推倒重来”降低正确性要求。Zircon 在这一点上不是假设内核分配永远成功：对象创建和 VM 路径大量使用 `fbl::AllocChecker`、`ZX_ERR_NO_MEMORY` 和 `zx::error(ZX_ERR_NO_MEMORY)` 显式返回失败。Ousia 可以比 Zircon 更激进地重建结构，但不能比 Zircon 更随意地处理分配失败。

## 目标

1. 建立 Ousia-native `Handle` / `KernelObject` / `Process` / `Syscall` 主线，替换旧 CSpace/Untyped/retype invocation 主线。
2. 让 `Process` 或等价 Capsule runtime owner 拥有 handle table、address space、thread set 和 resource budget。
3. 让 kernel object manager 成为对象生命周期、kind、generation、dispatcher state 和 finalization 的单一权威位置。
4. 建立 channel/call + bytes + handle transfer 的 IPC baseline，替换 Endpoint/Reply cap 作为主线 API。
5. 建立 VM/MemoryObject/address-space/resource allocator 的第一版边界，为 VFS/Object Namespace、page cache 和 pager fault path 留出内核拥有状态的位置。
6. 重新实现正确性纪律：不可伪造 handle、rights 单调性、stale handle 明确失败、失败前置检查、commit 无可恢复失败、显式 enum state machine、always-multicore native HMP scheduler 假设。
7. 从零重写测试为 Ousia 行为契约，不迁移旧 CNode/Untyped helper，不让旧 expected error ordering 继续约束新语义。

## 非目标

- 不保留旧 `CapabilitySpace`、`CNodePath`、`RetypeTarget`、`UntypedCap` 或 seL4-style `Invocation` 作为兼容 API。
- 不把旧 CNode/Untyped/retype 测试迁移成新 facade 的表面兼容测试。
- 不要求旧 `kernel/src` 在重构过程中保持可用；每个 slice 只要求新 Ousia-native skeleton 自洽、可测、可 review。
- 不为了减少 diff 保留旧模块名、旧 enum variant、旧 error shape、旧测试夹具或旧 public helper。
- 不冻结最终 syscall ABI；本轮只冻结 Phase 1 可执行语义和模块 owner。
- 不复制 Zircon class hierarchy、Fuchsia component framework、ABI 或 product policy。
- 不实现完整 Object Store、完整 driver framework、完整 pager-backed filesystem 或 Linux compatibility domain。
- 不在热路径引入没有 budget、preflight、reclaim 或退出条件的动态分配。
- 不使用 panic、`unwrap`、unchecked `Vec::push`、隐式扩容或“分配基本不会失败”作为 kernel 可恢复路径的处理方式。

## Zircon 动态分配失败参考

Zircon 的主线对象模型虽然高级，但并不回避内核内存分配失败：

- `third_party/fuchsia/zircon/kernel/object/channel_dispatcher.cc` 的 `ChannelDispatcher::Create` 为 peer holder 和两个 dispatcher 分别用 `fbl::AllocChecker` 检查分配，任一步失败都返回 `ZX_ERR_NO_MEMORY`，只有全部成功后才初始化 peer 并交出 handles。
- `third_party/fuchsia/zircon/kernel/object/vm_object_dispatcher.cc` 的 `VmObjectDispatcher::CreateWithSsm` 分配 dispatcher 失败时返回 `ZX_ERR_NO_MEMORY`；`Create` 调用 `StreamSizeManager::Create` 时也传播 `zx::error(ZX_ERR_NO_MEMORY)`。
- `third_party/fuchsia/zircon/kernel/object/process_dispatcher.cc` 的 `ProcessDispatcher::Create` 在 `ShareableProcessState`、dispatcher、address space 和 root VMAR 创建失败时都返回错误，并且在完全初始化前不把 process 注册到 parent job，避免外部观察到半初始化对象。
- `third_party/fuchsia/zircon/kernel/vm/vm_address_region.cc` 的 mapping 路径分配 `VmMapping` 失败会返回 `ZX_ERR_NO_MEMORY`；部分 VM 激活路径承认失败可能发生并向上传播。
- `third_party/fuchsia/zircon/kernel/vm/vm_page_list.cc` 的 `PopulateSlotsInInterval` 在第二个 slot 分配失败时会归还第一个 slot，再返回 `ZX_ERR_NO_MEMORY`，体现失败回滚。
- `third_party/fuchsia/zircon/system/ulib/zx/include/lib/zx/result.h` 的示例把分配封装成 `zx::result`，失败返回 `zx::error(ZX_ERR_NO_MEMORY)`，成功后才把 node 加入 tree。

Ousia 采用的不是 Zircon 具体 C++ 类型，而是这条纪律：所有可能由动态分配、容量不足、page/cache reservation 或 quota 触发的失败必须显式进入 preflight/result 模型；对象、handle、queue、mapping、namespace 和 scheduler owner state 只能在失败不可恢复的 commit 阶段被修改。

## Ousia 分配失败硬约束

1. 所有会分配或可能扩容的操作必须返回 `Result` 或等价 commit plan，不得在可恢复路径 panic。
2. `Vec`、map、queue、slab、page table node、VM range node、namespace cache entry、channel message buffer、handle table entry 和 object table entry 的容量必须在 preflight 阶段检查或保留。
3. Commit 阶段不得调用可能失败的普通分配 API。若实现确实调用 `push`、`insert`、`extend` 或页表填充函数，必须通过 reservation token、fixed-capacity storage 或紧邻 invariant 证明它不会失败。
4. 部分资源预留失败必须回滚已经预留但未提交的资源；回滚失败是 internal invariant violation，不是普通 public error。
5. Public error 至少需要区分 `NO_MEMORY`、`NO_CAPACITY` 和 `QUOTA_EXCEEDED`，即使 syscall ABI 尚未冻结；测试可以先断言语义类别。
6. 每个 slice 必须列出新增动态状态的 owner、容量来源、reservation/preflight API、commit 消费点和失败后不变的 owner state。

## 现有结构处理原则

旧代码的地位是 reference corpus，不是 migration substrate。Implementation agent 可以直接新建 Ousia-native 模块，把旧模块整块删除或移出主线；只有当某段代码的语义能被新 owner 清楚接住，并且不会带入旧 CSpace/Untyped plumbing 时，才允许摘取。

### 只继承语义，不继承形状

- `Rights` 的单调收缩语义，但应重新定义在 Ousia handle/object boundary 上，而不是迁移 seL4-local cap model。
- generation/stale descriptor 的测试思路，但 generation 只用于 stale detection 和 lifetime guard，不替代授权策略、lease 或 service capability。
- `KernelState::execute_invocation` 中“decode/authorize 与 perform/commit 分离”的纪律，但旧 executor 不迁移；新入口是 `Syscall` / object operation boundary。
- Endpoint/Notification/Reply/TCB/Scheduler 的状态机经验可以参考，但新 IPC 主线是 Channel/Portal/Operation/EventPort，不保留旧 public naming。
- `ThreadAction` 可作为“把 thread mutation 和 scheduler mutation组织成可测试 action”的设计证据，但不要求保留类型或调用面。
- Scheduler 的 native HMP 假设、per-CPU/per-domain run queue、ready lane 和重复 enqueue 防护必须重新实现；同构 SMP 只是退化情况。

### 直接废弃

### 应停止模仿

- seL4 public CSpace/CNode addressing、CNode path lookup、Untyped/retype、MDB 和 Reply cap 作为产品 API。
- `ObjectTable` 与 `CapabilitySpace` 分别持有同一 object existence 事实。
- 通过 `KernelState` 中大量 match 把所有 object-specific operation 串在一个编排函数里。
- 测试直接构造旧 `Capability` variant、旧 helper、旧 expected error ordering 后声称覆盖 Ousia 语义。
- 旧 `executor_cnode.rs`、`executor_retype.rs` 作为测试文件本身；其中只保留可转写为新 Ousia behavior 的 case 意图。

## 候选方案

### 方案 A：在现有 seL4 prototype 上加 Ousia facade

做法：保留 `CapabilitySpace`、CNode/Untyped/retype、Endpoint/Reply 等内部结构，在外层新增 `Handle`、`Channel`、`MemoryObject` facade，把 Ousia API 翻译到旧 invocation。

优点：短期 diff 小，现有测试可大量保留。

不选择原因：这会把旧 CSpace/Untyped/resource model 继续藏在主路径，VM/VFS/Object Namespace 会被迫围绕旧 retype 和 slot 语义弯折；错误边界和状态 owner 仍分散在 `CapabilitySpace`、`ObjectTable`、`KernelState` 和 object runtime state 之间。用户已经明确要求不留历史兼容性，本方案违背目标。

### 方案 B：先只替换 capability 层，IPC/VM/VFS 后置

做法：先把 `CapabilitySpace` 改为 process-local handle table 和 object manager，但保持 Endpoint/Notification/Reply、FrameMap unsupported、无 MemoryObject/VFS 主线。

优点：范围比全量重构小，可以较快得到 handle/object skeleton。

不选择原因：Ousia 路线偏移的核心不只是 capability 表，而是 Phase 1 必须同时容纳 IPC、VM、VFS/Object Namespace 和资源预算。只替换 handle table 会导致新 object manager 很快被后续 VM/VFS 需求推翻，重构成本重复。

### 方案 C：推倒重来为 Ousia-native kernel skeleton

做法：以新的 `handle`、`object`、`process`、`syscall`、`ipc`、`vm`、`namespace`、`scheduler` 模块边界重建 kernel。旧 seL4 prototype 只作为只读证据，不作为迁移素材的默认来源；不保留旧 API 兼容。先实现薄但真实的 end-to-end vertical slice：启动 process 拥有 handle table，创建 channel/event/memory object，channel send/call 传递 handles，失败路径证明 handle/object/thread/queue/VM state 不变。

优点：一次性把 owner、数据流和错误边界摆正，后续 VFS/Object Namespace、driver/resource handle 和 Package Cell bootstrap 不再被旧 CNode/Untyped 形态牵制。

代价：旧测试会大面积删除，新 skeleton 初期功能少；中途不能依赖旧 executor tests 证明新语义。

推荐：采用方案 C。

## 推荐架构

### 模块边界

建议重建为以下模块。实施时优先新建这些 owner，再删除旧主线模块；不要为了复用旧文件名牺牲边界。

| 模块 | 职责 | 不应拥有 |
| ---- | ---- | -------- |
| `handle` | `HandleValue`、handle table entry、rights、generation、handle install/dup/transfer/delete/revoke preflight | object runtime state、scheduler queue、VM mapping truth |
| `object` | kernel object id、kind、generation、object store、dispatcher enum、finalization plan、reverse references | process-local handle slots、syscall decoding policy |
| `process` | process/capsule runtime owner：handle table、address space binding、thread set、resource budget | object global lifetime truth、per-CPU scheduling policy |
| `syscall` | syscall descriptor、decode、boundary validation、stable error mapping、commit plan dispatch | object-specific state machines 的内部 mutation |
| `ipc` | channel endpoints、message bytes、handle transfer plan、call transaction、event/wait primitive | process handle table owner、thread scheduler owner |
| `vm` | MemoryObject、address space、VMAR/VMA、mapping metadata、fault routing、allocator reservation | VFS naming policy、driver protocol policy |
| `namespace` | Object Namespace skeleton、ProviderRoot、MountBinding、ObjectHandle cache/revoke hooks | complete filesystem implementation、POSIX compatibility policy |
| `scheduler` | per-CPU run queue、current thread、wake/block/yield、cross-core wake boundary | IPC queue truth、process handle table |
| `thread` | TCB/register context、thread state、wait membership link、affinity/priority fields | global object lifetime、handle rights |
| `resource` | kernel allocation context、quota/budget, reservation tokens, reclaim hooks | object-specific semantic policy |

### 依赖方向

- `syscall` 依赖 `handle`、`object`、`process` 和具体 subsystem 的 public boundary，不反向被 subsystem 依赖。
- `process` 拥有 handle table 和 thread membership；`object` 拥有 object lifetime；二者通过 object id 和 validated references 协作。
- `ipc`、`vm`、`namespace`、`thread`、`scheduler` 只通过明确 operation/preflight/commit API 修改自己的 owner state。
- `resource` 提供 reservation token；commit 阶段消费 token，不再调用可能失败的普通 allocator API。
- OSTD 继续拥有 boot memory map、early page table、架构差异和 MMIO primitives；kernel 拥有架构无关 VM object、MemoryObject、address-space policy 和 page/cache metadata。

## 核心数据模型

### Handle

`HandleValue` 是用户态可携带的不可伪造引用编码，内部解析到 process-local handle table entry。handle table entry 至少包含：

- object id
- object generation snapshot
- handle generation 或 entry generation
- rights
- handle flags，例如 transferable、duplicable、close-on-exec 等后续可选语义
- derivation parent 或 revoke lineage metadata

约束：

- rights 派生只能收缩。
- lookup 必须同时检查 entry generation、object generation、object kind 和 rights。
- stale handle、wrong type、missing rights、dead object 映射到少量 stable kernel error。
- generation 不能承载授权策略；授权仍由 rights、policy 和 object boundary 建立。

### Kernel object

`KernelObject` 建议先用显式 enum，而不是 trait object 或复制 Zircon dispatcher hierarchy：

- `ChannelEndpoint`
- `Event` 或 `EventPort`
- `Process`
- `Thread`
- `MemoryObject`
- `AddressSpace`
- `NamespaceNode` 或 `ProviderRoot`
- `Resource` / `DeviceResource`

对象表 entry 至少包含 object id、kind、generation、state、reverse handle count、finalization state 和 optional owner process。对象销毁由 object manager 生成 finalization plan，再交给 subsystem commit；不要让 handle table 和 object table分别决定 object 是否存在。

### Process and resource budget

Process/Capsule runtime owner 持有：

- handle table
- address space id
- thread set
- resource budget/quota
- initial bootstrap handles

所有会创建 object、安装 handle、扩大 channel queue、建立 mapping、写入 namespace cache 的 syscall 都必须先检查 process budget 或 resource reservation。预算失败是可恢复错误，必须发生在任何 owner mutation 之前。

预算和内存分配是两个不同错误来源：quota 足够但内核 heap/slab/page metadata 分配失败时，应返回 `NO_MEMORY`；quota 不足时返回 `QUOTA_EXCEEDED`；固定表或队列无空位时返回 `NO_CAPACITY`。三者不能用一个模糊错误吞掉。

### Syscall transaction

每个 syscall 分三段：

1. Decode：解析用户输入和 handle value，不修改 owner state。
2. Preflight：检查 rights、kind、generation、budget、capacity、queue state、VM range、namespace state，执行所有可失败的动态分配或 reservation，生成 commit plan 和 reservation token。
3. Commit：只消费已经验证的 entry、object reference、queue slot、page/cache reservation 或 scheduler action。commit 中出现的失败只能是 internal invariant violation。

这个结构重新实现旧 retype preflight/commit 曾验证过的正确性纪律，但不继承 Untyped/retype API 或旧 executor shape。

## 子系统重构设计

### Handle/Object baseline

第一刀应建立全新的 `ProcessTable`、`ObjectManager` 和 `HandleTable` 最小闭环：

- 创建 bootstrap process。
- 给 bootstrap process 安装初始 handles。
- 支持 duplicate/transfer/delete/revoke/destroy。
- 支持 wrong type、missing rights、stale handle、dead object 的统一错误映射。
- 删除旧 CNode path 和 Untyped object creation public path。

Implementation note：旧 `Rights`、generation、delete/revoke 测试只能按行为意图重写。旧 `SlotId` 不作为类型迁移；新 handle table 若需要 index，应重新命名并重新定义 generation 语义。

### IPC and waiting

Channel 是 Phase 1 IPC 主线：

- Channel object 拥有两个 endpoint states 或一个 object 内的 peer state。
- Message 包含 bounded bytes 和 bounded handle list。
- Send preflight 必须验证 receiver/peer state、message size、handle transfer rights、destination process handle capacity 和 queue capacity。
- Commit 同时从 sender handle table 移动或复制 handles，写入 channel queue，必要时唤醒 waiting thread。
- Call 是 send + wait + response correlation 的 syscall/用户库 wrapper；不要求 seL4 reply cap 模型。
- Event/EventPort/WaitSet 是等待主线；旧 Notification 可作为 state machine 素材，但不保留 public naming。

### VM and MemoryObject

VM baseline 应在同一重构中落位 skeleton，即使第一版功能很小：

- `MemoryObject` 表示匿名/共享/pager-backed 的内存对象。
- `AddressSpace` 拥有 VMAR/VMA tree 或等价 range map。
- `MapMemory` syscall preflight 检查 handle rights、range alignment、overlap、page/cache reservation 和 address-space owner。
- Commit 只安装已验证 mapping metadata；实际 page table update 的架构细节通过 OSTD boundary 完成。
- Page fault path 先记录为 skeleton：fault input、lookup、pager/provider handoff、cancel/error boundary。

第一版可以只支持匿名 MemoryObject + fixed mapping，但模块 owner 和错误边界必须按最终方向设计。

### Object Namespace and VFS boundary

Object Namespace 不需要第一刀实现完整 FS，但需要建立 owner 决策：

- 内核至少拥有 `NamespaceNode` / `ProviderRoot` / `MountBinding` object kind 的 skeleton。
- 路径解析是否完全内核态、服务态或 hybrid，应在 implementation 前保留为明确 TODO，但 handle/object/revoke hooks 必须预留。
- `ObjectHandle` 是 native VFS 权限边界，不投影成 POSIX fd 语义。
- Page cache、provider fault、remote provider bridge 进入 VM/VFS joint design，不应落在 IPC 或 process helper 中。

### Scheduler and thread

保留当前 scheduler 的 native HMP shape，但修正存储和 lookup：

- Thread object 与 TCB state 应由 `thread` subsystem owner 管理，object manager 只持有 object entry 和 finalization hook。
- Thread id lookup 不应长期线性扫描 fixed array；可以使用 generational index 或 bounded slab。
- Wait membership link 继续放在 TCB，queue owner 只持有 head/tail 或 bounded queue metadata。
- Cross-core wake、timer routing、TLB shootdown 先用 explicit boundary placeholder，不能假设 single-core；单核不是支持目标或性能论据。

### Driver/resource handles

本轮只做 skeleton：

- `Resource` / `DeviceResource` object kind。
- Rights 区分 MMIO、IRQ、DMA/IOMMU、reset、power 等后续维度。
- Device resource install 和 revoke 必须和 IOMMU/DMA failure boundary 对齐。
- Driver Manager/Host 不进入 kernel crate 第一刀，但 kernel object model 不能阻塞它。

## 错误模型

Public error 类别保持少而稳定：

- invalid handle
- wrong object type
- missing rights
- stale handle
- dead object
- invalid argument
- would block
- no capacity / no memory / quota exceeded
- peer closed / canceled / timed out
- unsupported

内部错误可以携带 object id、expected/actual、slot index、queue state 等 debug context，但不应长期进入 syscall-facing ABI 或测试外部契约。测试应断言语义类别和 owner state，不复制内部诊断字段。

## 测试策略

重写测试时按行为契约分层：

1. Handle/object unit tests：rights 收缩、stale handle、wrong type、delete/revoke/destroy、handle transfer capacity preflight。
2. Syscall host integration：通过真实 syscall/object boundary 创建 channel、传递 handle、关闭 peer、撤销父 handle、验证失败后 sender/receiver handle table、object manager、channel queue、thread state 不变。
3. VM unit/integration：MemoryObject creation、map overlap、quota failure、mapping failure no partial state。
4. Scheduler/thread tests：blocked/runnable transitions、wake after channel receive、cancel wait、cross-CPU wake placeholder。
5. Namespace skeleton tests：ProviderRoot/MountBinding handle rights、revoke invalidates ObjectHandle cache metadata。
6. QEMU smoke：仅覆盖 boot/platform marker 和最小 bootstrap handle 注入，不声称证明 kernel semantic 完整。

旧 `executor_cnode.rs`、`executor_retype.rs` 应删除。只允许把其中的语义意图写成新的 Ousia behavior tests，例如 rights cannot expand、failure before commit、stale handle fails；不得保留旧 helper、旧 descriptor、旧 CNode path 或旧 error ordering。

## 实施步骤

### Slice 0：切断旧主线

- 在代码 review checklist 中明确旧 `CapabilitySpace`、CNode/Untyped/retype tests 不再是稳定约束。
- 新增或更新测试目录说明，声明旧 executor tests 会被删除并由 Ousia-native behavior contracts 重写。
- 不新增 compatibility facade。
- 可以直接删除旧 seL4 prototype tests 或把它们移出主线；不要求保持 `cargo test` 在旧测试集上通过。
- Slice 0 是进入 Slice 1 的强制 gate。未完成前，不得把旧 `CapabilitySpace`、CNode/Untyped/retype tests 或旧 `Invocation` variants 当作隐性稳定约束继续迁就。

### Slice 1：Handle/Object/Process skeleton

- 新增 `handle`、`process`、`object` owner types。
- 定义 `HandleValue`、重新定义的 `HandleRights` / `Rights`、`HandleTableEntry`、`ObjectEntry`、`ObjectKind`、`Process`。
- 支持 bootstrap process + initial handles。
- 建立 `SyscallContext` 和 minimal `Syscall` enum。
- 为 object table、handle table 和 process table 定义固定容量或 reservation API；创建失败必须返回 `NO_MEMORY` / `NO_CAPACITY` / `QUOTA_EXCEEDED`，不能 panic。
- 写 handle/object unit tests。

### Slice 2：Object creation and resource preflight

- 引入 resource reservation token 和 process budget。
- 实现 create object：channel、event、memory object、address space、thread skeleton。
- 证明 object creation 的任一分配、容量或 quota 失败都不会安装 handle 或留下 object entry。
- 每个 create path 必须像 Zircon `Create` 路径一样：先完成所有可失败分配和初始化，最后才发布 object/handle。

### Slice 3：Channel/call vertical slice

- 读取本地 Fuchsia/Zircon channel/call、handle transfer 和 `zx` wrapper 相关源码，记录采用、调整和拒绝点。
- 实现 channel create、send、recv、close、peer closed。
- 支持 bounded bytes + bounded handle transfer。
- 实现 call wrapper 的内核 transaction id 或用户库预留字段。
- 接入 thread wait/wake 和 scheduler action。
- Channel message buffer、handle transfer destination slots 和 queue entry 必须在 preflight 保留；任何一步失败都不能从 sender handle table 移除 handle，也不能向 receiver queue 写入部分消息。

### Slice 4：VM/MemoryObject skeleton

- 读取本地 Fuchsia/Zircon VMO/VMAR/address-space 相关源码，记录采用、调整和拒绝点。
- 实现 MemoryObject create/map/unmap 的最小语义。
- 建立 address-space owner 和 mapping metadata。
- 接入 OSTD page-table boundary placeholder。
- 覆盖 overlap、rights、quota、reservation failure。
- VM range node、page table metadata、MemoryObject backing metadata 和 cache entry 必须使用 reservation token；mapping commit 失败只能表示内部 invariant 破坏。

### Slice 5：Namespace/resource skeleton

- 读取本地 Fuchsia/Zircon driver manager/DDK/resource 相关源码，记录采用、调整和拒绝点。
- 增加 NamespaceNode/ProviderRoot/MountBinding object kind。
- 定义 namespace skeleton 的最小 owner state：ProviderRoot owner、MountBinding parent/name/provider relation、ObjectHandle cache entry generation 和 revoke invalidation hook。
- 定义 namespace skeleton 的最小操作：create provider root、mount binding、lookup one segment、delete/revoke binding。失败边界必须覆盖 wrong type、missing rights、occupied name、stale provider 和 cache invalidation 后的 stale ObjectHandle。
- 增加 DeviceResource object kind 和 rights skeleton，并定义最小 owner state：resource kind、physical range 或 IRQ/DMA token id、owner process、revocation state。
- 定义 DeviceResource 最小操作：install resource handle、derive restricted resource、revoke resource。失败边界必须覆盖 range conflict、wrong rights、stale handle 和 revoke 后 DMA/IRQ token invalidation placeholder。
- 写撤销、wrong type、rights failure、occupied binding/range failure 后 owner state 不变的 tests。
- Namespace name entry、ObjectHandle cache entry、resource range entry、IRQ/DMA token metadata 必须在 preflight 保留；失败后 parent namespace、resource registry 和 process handle table 不变。

### Slice 6：清空旧 seL4 prototype 残留

- 移除 `CapabilitySpace`、`CNodePath`、`UntypedCap`、`RetypeTarget`、CNode/Untyped invocation variants；除非某个内部实现已被重命名、重定义并证明不暴露旧语义，否则不得“内部化”保留。
- 删除旧 executor tests，保留新写的 Ousia behavior tests。
- 更新 `design/implementation/00-ousia-kernel-architecture.md` 的“当前代码定位”和“近期代码步骤”。

## 验证命令

涉及 Rust source 后，每个 implementation slice 至少运行：

- `cargo fmt`
- `cargo fmt --check`
- `cargo check`
- `cargo nextest run -p kernel`

若 slice 修改 `kernel-bin`、OSTD boot/platform、linker 或 QEMU runner，再追加 QEMU smoke。若只改 docs/proposal，运行：

- `deno task --cwd .github/skills/doc-validation check:docs --config ../../../design/check-docs.config.json`

## 回滚方式

本提案不提供旧 API 兼容回滚。回滚只按 git slice 回滚：某个 slice 未通过 review 或验证时，撤回该 slice 的代码和测试，回到上一个通过的 Ousia-native slice。不得通过重新暴露 CNode/Untyped facade、恢复旧 executor tests 或恢复旧 `Invocation` variants 来“兼容旧测试”。

## 文档归属

- 稳定路线和阶段验收：`design/target.md`、`design/topics/06-roadmap.md`。
- 近期 implementation handoff：`design/implementation/00-ousia-kernel-architecture.md` 和本文。
- Capability、Communication、Memory、VFS、Driver 的长期概念：对应 `design/core/**`。
- Fuchsia/Zircon 和 seL4 的外部事实：`design/notes/reference/**`。
- AI agent 必须遵守的硬边界：`.github/instructions/ousia-kernel-boundaries.instructions.md` 和开发规范。

## 已读取证据

- [00-ousia-kernel-architecture.md](../implementation/00-ousia-kernel-architecture.md)：当前 Ousia-native kernel baseline。
- [06-roadmap.md](../topics/06-roadmap.md)：Phase 0.5 到 1h 的新路线验收。
- [06-fuchsia-zircon-kernel.md](../notes/reference/06-fuchsia-zircon-kernel.md)：Zircon handle/object/channel/VM/driver 参考边界。
- `.github/skills/_shared/reference/kernel-baseline.md`：Ousia capability kernel planning/review attacks。
- `kernel/src/cap/space.rs`：旧 CSpace/Untyped/retype prototype 和可重新实现的 rights/generation 语义。
- `kernel/src/object/table.rs`：当前 ObjectTable 与 object runtime state，作为“不要继续分散 object truth”的反例证据。
- `kernel/src/state/kernel.rs`：当前跨 owner invocation executor 和 preflight/commit 证据，作为新 syscall transaction 的纪律参考，不作为迁移底座。
- `kernel/src/ipc/endpoint.rs`、`kernel/src/thread/action.rs`、`kernel/src/scheduler/core.rs`：可参考的 IPC/thread/scheduler 状态机经验。
- `kernel/tests/executor_cnode.rs`：旧 host integration 测试如何约束 rights、path resolution 和 failure behavior。
- `third_party/fuchsia/zircon/kernel/object/channel_dispatcher.cc`：Zircon channel create 中 `AllocChecker` 和 `ZX_ERR_NO_MEMORY` 的对象发布前失败处理。
- `third_party/fuchsia/zircon/kernel/object/process_dispatcher.cc`：Zircon process create 在完全初始化前不注册到 parent job。
- `third_party/fuchsia/zircon/kernel/object/vm_object_dispatcher.cc`、`third_party/fuchsia/zircon/kernel/vm/vm_address_region.cc`、`third_party/fuchsia/zircon/kernel/vm/vm_page_list.cc`：Zircon VMO/VMAR/page-list 分配失败传播和局部回滚证据。

## Open Questions

1. `Portal` 在第一刀中是独立 kernel object，还是先由 `ChannelEndpoint + service registry handle` 表达？实现前必须在 Slice 3 proposal note 中裁决。
2. `Process` 与 `Capsule` 是否在 kernel crate 中同义，还是 Capsule 是用户态/Package Cell 层概念？第一刀建议使用 `Process`，保留 Capsule 作为产品层术语。
3. MemoryObject 第一版是否允许 pager-backed fault，还是只建立 fault routing skeleton？建议只做 skeleton，但接口不要阻塞后续 pager。
4. Object Namespace 第一版是 kernel-resident skeleton 还是 system service handle？本提案要求 object kind 和 revoke hooks 进入 kernel，但路径策略仍需单独 proposal。
5. Handle revoke lineage 采用 parent pointer、generation domain、reverse index 还是 compact derivation tree？实现前需比较 O(1) lookup、revoke cost、内存占用和 hot-path影响。

## Residual Risks

- 本提案未读取 `third_party/fuchsia` 源码具体符号，只依赖已记录的 Fuchsia/Zircon reference note。实施前若声称采用 Zircon 机制，必须读取本地源码路径并在 implementation notes 中列证据。
- 本提案未读取本地 seL4 源码；只保留抽象纪律。若实现 revoke lineage 或 IPC 失败纪律时引用 seL4 行为，必须读取本地 reference。
- 全量破坏性重构 diff 会很大，必须按 slice review，不能一次性把所有模块推到不可验证状态。
- VM/VFS/driver skeleton 容易变成空名字；每个 skeleton 必须至少有 owner、state、operation、failure boundary 和测试，否则应延后。

## 设计提案 Review Focus

- 是否仍保留旧 seL4 CSpace/Untyped/retype compatibility path，违背用户“不留历史兼容性”。
- 是否把 Zircon/Fuchsia 参考直接写成 Ousia 规范，而没有采用/拒绝边界。
- 模块 owner 是否唯一，特别是 handle table、object lifetime、process budget、VM mapping、channel queue 和 scheduler queue。
- 是否说明了失败前置检查和 commit 阶段不可恢复失败的边界。
- 是否把动态分配失败当成显式 public error / preflight reservation，而不是靠 panic、隐式扩容或“commit 中失败也返回错误”混过去。
- 测试策略是否约束 Ousia behavior，而不是复述旧 helper 或外部 reference 状态表。

## Implementation Handoff 条件

本提案通过 `设计提案 + diff` review 后，implementation agent 必须先完成 Slice 0，并把“不保留旧 seL4 prototype API 兼容”的代码/测试入口清理成显式事实；Slice 0 通过 review 后才能进入 Slice 1。实施时每个 slice 必须提交：

- touched owner 和状态流说明。
- 删除的旧 API、旧测试和旧模块列表。
- 新增测试和覆盖的失败无副作用风险。
- 新增动态状态的容量来源、reservation/preflight API、commit 消费点和失败回滚证据。
- 若 slice 采用 Zircon/Fuchsia 或 seL4 reference 形状，列出本地源码路径、采用点、调整点和拒绝点。
- 已运行验证命令和未覆盖 residual risk。
- 下一 slice 的接口假设。
