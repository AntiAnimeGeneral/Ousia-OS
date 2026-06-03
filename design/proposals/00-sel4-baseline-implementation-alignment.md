# 00 — seL4 Baseline 实现语义对齐提案

> Proposal packet。本文用于交接给 implementation agent 执行。通过 review 和实施后，稳定结论应回写到 `design/implementation/00-sel4-baseline-rust-replica.md`、`.github/instructions/ousia-kernel-boundaries.instructions.md` 或对应代码 rustdoc；本文本身不作为长期规范源。

## 用户目标

一口气把现有 `kernel` 实现的核心语义对齐到 seL4 baseline，而不是继续在通用 `HashMap`、`VecDeque` 和模型化对象表上局部修补。交接目标是让后续 agent 可以按本文直接实施，并在完成后进入 `代码实现 + diff` review。

## Mode And Target

- Mode：重构。
- Target：代码。
- Scope：`kernel/src/cap/`、`kernel/src/object.rs`、`kernel/src/ipc.rs`、`kernel/src/notification.rs`、`kernel/src/reply.rs`、`kernel/src/tcb.rs`、`kernel/src/thread_action.rs`、`kernel/src/scheduler.rs`、`kernel/src/state.rs`、`kernel/src/invocation.rs` 和对应 `kernel/tests/**`。

## 背景与约束

Phase 1 的目标是在 Rust 中复刻 seL4 baseline。Rust 可以改善 API、类型和错误表达，但不能改变 capability authority、CSpace/CNode、MDB、Untyped/retype、Endpoint、Notification、Reply、TCB、IPC 和 scheduler 的对象关系与状态所有权。

当前实现已经建立了有价值的可执行语义模型：typed `Capability`、Endpoint/Notification/Reply 状态机、`KernelState::execute_invocation` 的 decode/perform 边界、retype 预检与提交分离、失败无副作用测试，以及 TCB/scheduler 的基础协作。这些应继承为测试和迁移保护。

但当前内部结构仍明显偏离 seL4：`CapabilitySpace` 使用 `HashMap`/`HashSet` 表达 slot、对象和派生关系；`ObjectTable` 用分散 map 表达运行时对象；`ThreadTable` 用 map 表达 TCB 集合；scheduler 用 `HashMap<CpuId, PerCpuRunQueue>` 和 `VecDeque`；Endpoint/Notification 等待队列用 `VecDeque` 保存消息或 waiter。seL4 core 不用通用 map 表达这些核心关系，而是使用 CNode 连续 CTE slot、slot 内嵌 MDB link、TCB 内嵌队列 link、固定 ready queue 数组和 bitmap。

另一个约束是 SIMD/FPU：`kernel` core 不能依赖会在当前 target cfg 下生成 SIMD/FP 指令的容器实现。即使短期继续存在 `hashbrown`，实施完成后的核心路径也不应以 generic map 作为最终结构；需要 SIMD/full-speed 的能力必须由 OSTD guard/token 边界拥有。

## 目标

- 用 seL4-style CSpace/CNode/CTE 替换当前 `CapabilitySpace` 的通用 map 关系。
- 将 MDB predecessor/successor/parent-child 关系嵌入 CTE/slot 元数据，而不是单独用 `HashMap`/`HashSet` 维护派生事实。
- 将 Untyped/retype 改成 seL4-style 边界：decode 阶段完成源 Untyped、目标 CNode slot window、对象大小、alignment、watermark、remaining space 和目标 slot 空闲检查；perform 阶段只消费已验证 memory/slot。
- 将 runtime object ownership 收敛到 typed object memory：对象存在性、对象类型、TCB binding、Endpoint/Notification/Reply state 与 capability slot 不再通过一个全局 `ObjectTable<HashMap>` 拼接。
- 将 Endpoint/Notification waiting queues 改成 TCB embedded queue links，由 endpoint/notification 只持有 head/tail 或等价轻量指针。
- 将 scheduler 改成固定 per-CPU ready queue 数组加 priority bitmap，避免运行时 CPU map 插入和动态扩容成为主路径。
- 保留并升级现有语义测试，使测试约束 seL4 行为、失败无副作用和状态所有权，而不是复述旧容器形态。

## 非目标

- 不引入 Portal、Operation、Continuation、Package Cell、lease、session、Device Service 或浏览器授权语义。
- 不在本轮冻结 syscall ABI、message register ABI 或完整用户态 root server ABI。
- 不直接复制 seL4 C 代码；以本地 seL4 reference 映射算法和不变量，再用 Rust 类型表达。
- 不把 generation 机制升级为 authority 语义。slot/object generation 只能作为 stale descriptor 检测、测试和诊断辅助。
- 不把 `hashbrown` 的 target-cfg 问题当作本轮唯一目标；容器替换的主要目标是 seL4 baseline 结构对齐，SIMD 风险只是额外动机。

## Evidence To Read

实施 agent 开始前必须读取下列证据，并在实现记录中说明映射关系：

- Ousia 规范：`.github/instructions/development-entry.instructions.md`、`.github/instructions/architecture-abstraction.instructions.md`、`.github/instructions/implementation-quality.instructions.md`、`.github/instructions/testing-evolution.instructions.md`、`.github/instructions/ousia-kernel-boundaries.instructions.md`。
- Ousia owning doc：`design/implementation/00-sel4-baseline-rust-replica.md`。
- 当前实现：`kernel/src/cap/mod.rs`、`kernel/src/object.rs`、`kernel/src/ipc.rs`、`kernel/src/notification.rs`、`kernel/src/reply.rs`、`kernel/src/tcb.rs`、`kernel/src/thread_action.rs`、`kernel/src/scheduler.rs`、`kernel/src/state.rs`、`kernel/src/invocation.rs`。
- 当前测试：`kernel/src/**` 内单元测试和 `kernel/tests/**` host integration tests。
- seL4 reference：`third_party/sel4/src/kernel/cspace.c`、`third_party/sel4/include/kernel/cspace.h`、`third_party/sel4/src/object/cnode.c`、`third_party/sel4/src/object/untyped.c`、`third_party/sel4/src/object/endpoint.c`、`third_party/sel4/src/object/notification.c`、`third_party/sel4/src/object/reply.c`、`third_party/sel4/src/object/tcb.c`、`third_party/sel4/src/kernel/thread.c`、`third_party/sel4/src/model/statedata.c`、`third_party/sel4/include/model/statedata.h`、`third_party/sel4/manual/parts/objects.tex`、`third_party/sel4/manual/parts/notifications.tex`。

## 当前结构取舍

应继承的部分：

- typed capability enum、Endpoint/Notification/Reply/TCB 的显式状态机表达。
- `KernelState::execute_invocation` 中 decode/authorize 与 perform/commit 分层。
- retype 预检后提交的事务意识。
- 测试中对权限、失败无副作用、reply cap 一次性语义、TCB resume/configure、notification active/waiting 语义的断言。
- `KernelErrorCode` 折叠 syscall-facing 错误类别的方向。

应演进的部分：

- `CapabilityDescriptor` 可以继续作为测试和模型边界输入，但 descriptor lookup 必须通过 CNode guard/radix path 和 CTE slot，而不是全局 slot id map。
- `ObjectId` 可以继续作为 debug/test id，但不能再作为对象关系的主事实源。对象 backing memory 和 CTE capability 应成为权威。
- `ThreadId` 可以继续作为 scheduler/debug 标识，但 TCB object 和 thread state 应统一，不再由 `ObjectTable` 与 `ThreadTable` 双写事实。

应停止模仿的部分：

- 用 `HashMap`/`HashSet` 保存 slot、child set、object table、thread table、CPU run queue。
- Endpoint/Notification 用 `VecDeque` 保存 waiter/message，而不是 TCB 内嵌 queue links。
- CNode 只作为 `KernelObjectKind::CNode` runtime presence，而没有 CTE array、guard/radix lookup 和 slot window。
- Untyped retype 只检查模型 size table 或 source size，而没有以 target CNode slots 与 memory watermark 为单一事务边界。

## 候选方案

### 方案 A：继续局部演进当前模型

保留 `HashMap`/`VecDeque`，只补 CNode lookup、Untyped accounting 和更多测试。

优点是改动小，现有测试迁移成本低。缺点是核心结构仍然不是 seL4 baseline，动态扩容和分散事实会继续污染错误边界；hashbrown/SIMD 风险也仍然留在普通 kernel 路径。该方案不能满足“一口气全部对齐”的目标。

### 方案 B：一次性按 seL4 领域容器重构

以 CNode/CTE、MDB embedded links、typed object memory、TCB embedded queues、fixed scheduler queues 为目标结构，分子系统提交但在同一重构计划内完成语义闭环。

优点是能真正消除当前 baseline drift，让状态所有权和失败边界回到 seL4 形状。缺点是改动面大，需要迁移测试和临时兼容层，review 成本高。本文推荐该方案。

### 方案 C：引入第三方 no-SIMD map 替换 hashbrown

寻找裸机 no-SIMD hashmap 或 BTree 实现，先替换当前 `hashbrown`。

优点是可以降低 SIMD 风险。缺点是它只解决容器实现问题，不解决 seL4 不使用通用 map 表达核心对象关系的问题。该方案只能作为过渡工具，不能作为 baseline 对齐方案。

## 推荐方案

采用方案 B：一次性按 seL4 领域容器重构，但实施上使用可 review 的阶段切片。每个切片都必须保持 `cargo check` 和相关测试可运行；切片之间可以有临时 adapter，但 adapter 只能服务迁移，不能成为新的长期抽象层。

推荐顺序不是为了慢慢拖延，而是为了保证每一步都能验证失败无副作用和状态所有权。先建立 CSpace/CTE 权威，再把 object、IPC、scheduler 迁到这个权威上；不要先重写 scheduler 或 endpoint queue 后再回头改变 capability backing。

本轮默认对齐 classic/non-MCS seL4 baseline。MCS 的 scheduling context、MCS-specific reply object 语义和 budget/period accounting 不进入本轮；如果 implementation agent 发现当前本地 reference 或现有 `Reply` 模型只能按 MCS 解释，必须先停下并提交一个 baseline variant 修正提案，不能在同一实现中混合 classic 与 MCS 语义。

## 实施前 Gate

接手 agent 进入代码修改前必须先完成 Gate 0，并把结果写入实施计划或 PR 描述：

- Baseline variant：采用 classic/non-MCS seL4 baseline；MCS/schedcontext out of scope。每个 touched invocation 必须标明对应 reference 函数、关键检查顺序、状态 mutation 点、Rust decode/preflight/perform 对应位置和测试覆盖。
- Typed object storage：第一版使用由 Untyped/retype commit plan 创建的 bounded typed storage/arena。它可以暂由 Rust backing storage 表达，但对象创建、类型、大小和 slot install 必须受 Untyped/CNode preflight 约束；不得继续保留“任意 `ObjectId` 到 enum”的全局 map 作为权威。
- CNode addressing：第一版实现完整 lookup API shape，包含 root CNode、guard/radix、depth/remaining bits、slot window validation 和 lookup fault；允许初始测试只覆盖单层 CNode，但 API 和错误模型不能锁死为单层。
- Scheduler shape：第一版实现 fixed CPU topology、bounded per-CPU ready queue array、priority/domain bitmap shape 和至少一个 real priority/domain lane。可以先只启用一个 priority/domain 组合，但 public scheduler API 必须按 priority/domain 选择路径，不得暴露 FIFO-only 长期接口。

Gate 0 未完成时，不得开始大范围代码迁移。若 Gate 0 的任一选择需要偏离本文默认值，应先更新本文或提交新的 proposal review。

## 目标模块边界

### CSpace And CNode

所有 capability slot 事实由 CNode/CTE 拥有。建议在 `kernel/src/cap/` 内建立明确子模块，但不要为了形式拆分过细：

- `cnode`：CNode object、radix、guard、slot array、slot lookup path、slot window validation。
- `cte`：CTE slot、capability payload、slot state、MDB predecessor/successor links、slot generation debug metadata。
- `mdb`：MDB traversal helpers，只操作 CTE embedded links，不拥有独立 graph store。
- `untyped`：Untyped watermark、alignment、object size policy、retype preflight result。
- `rights` 或现有 capability 类型：保存 typed cap semantics，不保存容器事实。

依赖方向：`invocation` 只 decode cap and request；`state`/executor 调用 CSpace preflight，perform 阶段提交 CTE/Object mutations；object-local code 不绕过 CSpace 修改 authority。

### Object Memory

对象存在性应从 `ObjectTable<HashMap>` 收敛到 typed object memory 或等价 owner。早期可以用一个 arena/typed storage 承载 backing memory，但它必须按 seL4 对象种类和 Untyped retype 输出组织，不再是“任意 object id 到 enum”的全局 map。

第一版采用 bounded typed storage/arena，并由 Untyped/retype commit plan 创建对象。这个 arena 是 backing memory 的 Rust 表达层，不是新的全局 object namespace；lookup 必须从 CTE capability 指向 backing object，不允许 executor 用裸 `ObjectId` 穿透 authority 边界。

对象 state owner：

- Endpoint object 拥有 endpoint state 与队列 head/tail。
- Notification object 拥有 notification state、badge accumulator 与队列 head/tail。
- Reply object 拥有 pending caller state。
- TCB object 拥有 thread state、IPC/notification/reply blocking metadata、scheduler queue links、endpoint/notification queue links。
- CNode object 拥有 CTE slot array。
- Untyped object/cap metadata 拥有 watermark 和 free/retype accounting。

### IPC And Notification Queues

Endpoint/Notification 不再保存 `VecDeque<IpcMessage>` 或 `VecDeque<EndpointWaiter>`。等待关系应嵌入 TCB：TCB 保存 blocked reason、badge/grant/call/payload metadata、queue prev/next 或 intrusive node。Endpoint/Notification 保存 head/tail，并通过 TCB owner 修改队列 link。

副作用边界：入队前必须完成 cap rights、endpoint/notification object lookup、current thread state、reply destination、payload length 和 scheduler placement 检查。提交后才能修改 endpoint state、TCB blocked state、reply pending 和 scheduler state。

### Scheduler

Scheduler 不再用 `HashMap<CpuId, PerCpuRunQueue>`。CPU topology 在 boot/KernelState 初始化边界固定，scheduler 保存固定数组或 bounded storage；per-CPU ready queues 按 priority/domain 组织，并用 bitmap 标记非空队列。

早期只启用一个 priority/domain lane 时，仍必须保留 bitmap shape：enqueue、choose-next 和 dequeue 都经 priority/domain selector 进入 ready queue，不通过 FIFO-only API。验收标准是代码中能指出 ready queue array、non-empty bitmap、priority/domain selector 和对应测试；缺任何一项都不能算 scheduler 对齐完成。

### Invocation Executor

`KernelState::execute_invocation` 继续作为 syscall-like 执行收口，但其内部应更接近 seL4 decode/perform：

1. Decode：resolve CSpace path、检查 cap type/right、解析 request、收集目标 CTE window/object refs。
2. Preflight：检查所有可失败条件，产出不可变的 commit plan。
3. Perform：按 commit plan 更新 CTE、object state、TCB queue links 和 scheduler。
4. Error map：把内部 rich error 折叠到稳定 `KernelErrorCode`。

commit plan 是为了表达事务边界，不应演变成绕过类型系统的大型动态命令对象。

## 数据流与状态所有权

输入来自 syscall/invocation descriptor、current TCB context、message payload 和 CSpace root。输出是 capability mutation、object state mutation、thread action、scheduler action 或 stable error。

状态 owner：CSpace/CNode 拥有 authority 和 slot linkage；Untyped 拥有 object creation resource accounting；typed object memory 拥有对象实体；TCB 拥有线程状态和 intrusive queue membership；scheduler 只拥有 runnable placement；executor 只负责编排，不偷持有底层事实。

失败处理：所有外部可恢复失败必须在 decode/preflight 边界返回；perform 阶段只消费已验证引用和 slot，不临时发现可恢复 allocation failure、slot occupied、wrong object type 或 queue membership conflict。内部 invariant 破坏用 `expect`/assertion/panic 暴露为实现错误。

## 实施步骤

1. 完成 Gate 0。建立 seL4 reference mapping 表：列出当前每个 `kernel` 模块对应的 seL4 文件、函数、对象关系、关键检查顺序、状态 mutation 点、Rust decode/preflight/perform 对应位置和测试覆盖。此表可先放在 proposal implementation notes 或代码 PR 描述中，不必新增长期文档。
2. 引入 CTE/CNode 目标结构和 CSpace lookup path。让现有 descriptor 测试通过新 lookup，保留临时 descriptor facade 只服务测试和迁移。
3. 把 capability insert/copy/mint/move/delete/revoke 改成操作 CTE array 和 MDB embedded links。删除独立 child set / slot map / object map 派生事实。
4. 重写 Untyped/retype preflight：输入 source Untyped cap、target CNode、slot window、object kind/count/size；输出 commit plan；perform 阶段创建 CTE caps 和 backing objects。
5. 替换 `ObjectTable`：把 Endpoint/Notification/Reply/TCB/CNode/Frame backing state 迁到 typed object storage，并确保 cap object ref 与 backing object 一致。
6. 统一 TCB object 与 ThreadTable：TCB object 成为 thread state owner；删除双写 thread binding 的长期路径。
7. 将 Endpoint queue 改成 TCB embedded queue links，保留现有 send/receive/call/reply 语义测试并新增取消/出队失败无副作用测试。
8. 将 Notification queue 改成 TCB embedded queue links，覆盖 active badge OR、bound TCB receive、wait/poll 和取消语义。
9. 将 Reply pending state 接入 TCB/Reply object owner，确保一次性 reply cap 消费、pending caller 和 scheduler wakeup 在同一 commit plan 中完成。
10. 将 scheduler 改成固定 per-CPU ready queues + bitmap。第一版至少包含一个 priority/domain lane、non-empty bitmap 和 selector API；先覆盖现有 runnable/enqueue/schedule/yield/block 语义，再扩展更多 priority/domain。
11. 更新 `KernelState::execute_invocation` 为 decode/preflight/perform 结构，删除旧临时 adapter。
12. 删除普通 kernel 主路径中的 `hashbrown` 依赖；如果测试或 host-only helper 仍需 map，必须放在 `#[cfg(test)]` 或非 core 路径并说明边界。
13. 更新 `design/implementation/00-sel4-baseline-rust-replica.md` 的实现路线和当前运行路径，记录新的 owner 和验证命令。

## 测试策略

测试应从旧容器断言迁移到 seL4 语义断言：

- CSpace/CNode：guard/radix lookup、slot empty/occupied、copy/mint/move/delete/revoke、MDB traversal、stale descriptor debug rejection。
- Untyped/retype：slot window 预检、alignment/watermark、object count、目标 slot 全部空闲、失败后 source Untyped 和目标 CNode 不变。
- Endpoint IPC：blocking/nonblocking send/recv、call reply setup、grant/grant-reply、queued sender/receiver FIFO、取消/出队、失败后 endpoint/TCB/scheduler 不变。
- Notification：active badge OR、wait/poll、bound TCB receive、waiting queue、取消、失败后 badge 与 TCB state 不变。
- Reply：一次性 reply cap、pending caller consistency、reply object distinctness、cap consume 与 wakeup 事务。
- TCB/Scheduler：TCB configure/resume、blocked/runnable transitions、fixed CPU topology、priority/domain ready queue bitmap、重复入队拒绝、unknown CPU 边界。
- Host integration：通过 `KernelState::execute_invocation` 覆盖跨 CSpace/Object/TCB/Scheduler 的成功与失败无副作用路径。

测试不得直接复制实现表。除 ABI 常量和稳定编号外，优先通过 public boundary 或 executor 路径触发行为。

## 验证命令

实施 agent 完成代码改动后至少运行：

- `cargo fmt --check`
- `cargo check`
- `cargo test -p kernel`

如果改动 `kernel-bin`、`ostd`、linker 或 QEMU runner，再追加对应裸机构建或 QEMU smoke。若只改 `kernel` host model，不要求 QEMU smoke。

如果更新 `design/**/*.md`，运行：

- `deno task --cwd .github/skills/doc-validation check:docs --config ../../../design/check-docs.config.json`

## 兼容性与迁移

允许破坏内部 Rust API 和测试 helper，因为当前 Phase 1 还未冻结 ABI。需要保留的是 seL4 baseline 语义、现有测试覆盖的权限/状态/失败无副作用意图，以及 `KernelErrorCode` 这类外部错误类别方向。

迁移时可以暂留 compatibility facade，但每个 facade 必须有删除条件。例如 `CapabilityDescriptor` facade 的删除条件是 CNode path descriptor 完整接管测试和 executor；`ObjectTable` adapter 的删除条件是 typed object storage 覆盖所有 backing object lookup。

回滚方式是保持每个切片可编译、可测试、可 review。如果某一切片暴露结构性问题，回滚该切片并保留前一个通过状态；不要把半迁移结构合并成长期状态。

## Review Focus

实施前的 `设计提案 + diff` review 应重点攻击：

- proposal 是否仍有通用 map/queue 作为长期核心事实源。
- CSpace、Object、TCB、Scheduler 的状态 owner 是否唯一。
- Untyped/retype 是否真正把所有可失败条件收在边界，而不是 perform 阶段临时失败。
- Endpoint/Notification/Reply 是否通过 TCB embedded membership 表达等待关系。
- 测试是否约束 seL4 语义，而不是复述新的 helper 或容器实现。

实施后的 `代码实现 + diff` review 应重点攻击：

- 真实 diff 是否删除旧 `HashMap`/`HashSet`/`VecDeque` core facts，而不是只包了一层新名字。
- commit plan 是否保存事务语义，还是变成新的黑箱 dispatcher。
- failure path 是否能证明 CTE、MDB、Untyped watermark、Endpoint queue、TCB state 和 scheduler placement 不变。
- 是否有 `expect` 处理外部输入失败。
- 是否有 Ousia-specific 高层语义漏进 Phase 1 kernel baseline。

## Assumptions And Open Questions

Assumptions：

- 本轮可以大范围修改 `kernel` 内部 API 和测试 helper。
- `kernel` Phase 1 仍以 host integration test 为主要语义验证，QEMU smoke 只覆盖 boot/platform 链路。
- 本地 `third_party/sel4/` checkout 可作为 reference source 使用，但不直接复制代码。
- 本轮默认采用 classic/non-MCS seL4 baseline；MCS/schedcontext 进入后续单独 proposal。

Open questions：

- classic/non-MCS baseline 下当前显式 `Reply` 模型应如何精确映射到本地 seL4 reference；若无法映射，必须在 Reply 切片中改为 reference 对应语义。
- 第一版 bounded typed storage 需要的容量上限和测试 fixture 构造方式。
- CNode lookup fault 的 Rust error 粒度如何折叠到 `KernelErrorCode`，既保留测试诊断，又不暴露内部 ABI。

## Residual Risks

- 一口气重构可能导致测试同时迁移过多，短期难以判断失败来自语义变化还是测试旧假设。
- 如果 typed object storage 设计过早绑定未来 allocator，可能把 Phase 1 seL4 baseline 和 OSTD memory manager 绑死。
- 如果为了降低改动量保留过多 adapter，旧 map 模型可能以新名字残留。
- seL4 MCS 与 non-MCS 路径存在差异；本文选择 classic/non-MCS 作为本轮 baseline，但 Reply 相关现有模型仍需要 implementation 前做函数级映射确认。

## Handoff 条件

接手 agent 可以开始实施的条件：

- Gate 0 已完成，并明确写出 baseline variant、typed object storage、CNode addressing、scheduler shape 四项初始决策。
- 已读取本文 `Evidence To Read` 列出的 Ousia 规范、owning doc、当前实现和 seL4 reference。
- 已在实施计划中列出每个切片会修改的文件、允许临时 adapter、必须删除的旧事实源和对应测试。
- 已确认不会引入 Ousia-specific 高层语义。
- 已确认每个切片都能运行 `cargo fmt --check`、`cargo check` 和对应 targeted tests。

完成条件：

- 普通 `kernel` core 主路径不再依赖通用 map/queue 保存 seL4 核心对象关系。
- CSpace/CNode/CTE/MDB、Untyped/retype、Endpoint/Notification/Reply、TCB/Scheduler 的状态 owner 能用一句话说明。
- 所有外部可恢复失败发生在 decode/preflight 边界，perform 阶段不留下半状态。
- 相关 host unit/integration tests 通过，并新增失败无副作用覆盖。
- 实施后运行 `black-team-review`，subject：`代码实现`，mode：`diff`。