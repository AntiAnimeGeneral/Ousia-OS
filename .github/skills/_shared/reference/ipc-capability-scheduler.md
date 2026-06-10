# IPC Capability Scheduler Reference

IPC/capability/scheduler reference 用于攻击 invocation 路径中的状态机、权限检查、队列 mutation 和失败无副作用风险。

## Scope

使用本正文处理：

- Channel/call、Portal/Operation、Endpoint prototype、reply/completion handoff、Notification/Event signal/wait。
- TCB blocked/runnable state、scheduler enqueue/dequeue、priority/domain/fairness placeholder。
- Handle/capability rights、object type、badge/txid、handle lookup、syscall/invocation label。
- Cross-CPU queue ownership、wakeups、preemption and timer routing 的早期边界。

## Planning Prompts

- IPC/Channel 主路径的输入、输出、状态 owner 和失败处理者是否能一句话说明。
- Rights、object type、endpoint state、TCB state 和 scheduler state 的可失败检查是否先于任何 queue/thread mutation。
- Channel/Endpoint prototype、Notification/Event、TCB、reply/completion object 和 run queue 是否各有清晰 owner；编排者是否只协调 transition。
- Reply/completion handoff 的状态转换是否显式：caller blocked 或 pending call、callee running、response ownership、scheduler queue。
- Notification signal/wait 是否与 endpoint IPC 分开表达，避免共享一个模糊 queue abstraction。
- Scheduler 第一版是否已经按 always-multicore native HMP 建模，即使实现很小；单核不是支持目标，同构 SMP 不能被当成最终硬件模型。
- Portal、Operation、Continuation 或 EventPort 是否作为 Ousia Phase 1 主线通信能力进入裁决，而不是被旧 seL4 Endpoint/Reply baseline 后置。

## Review Attacks

- Send/recv/reply 路径是否在 lookup、rights、object type、state compatibility 检查前修改 endpoint queue 或 TCB state。
- Reply handoff 失败时，reply object、caller/callee TCB、scheduler queue 是否能证明未变化。
- Notification wait/signal 是否错误复用 endpoint state，导致 badge accumulation 或 wake semantics 不清。
- Scheduler enqueue/dequeue 是否被 IPC 内部直接乱改，缺少单一 mutation owner。
- Cross-CPU wakeup、timer routing 或 run queue ownership 是否被 single-core happy path 偷偷假设；是否用单核结果替代并发并行性能论证。
- Invocation label match 是否有 `_` fallback，吞掉未来 IPC/capability operation。
- Diff 是否继续把 seL4 Endpoint/Reply/Notification baseline 当成唯一 Phase 1 目标，导致 Ousia Channel/Portal/Operation 语义被后置。
- Rust-side IPC helper 是否缺少对 blocking、badge/txid、handle transfer、reply/completion object 或 TCB blocked state 语义的明确 owner。
- Tests 是否只检查返回错误，没有检查 endpoint queue、TCB state、reply object 或 run queue 不变。

## Evidence To Seek

- Endpoint/Notification/TCB/reply object state enum 和 transition sites。
- Capability lookup、rights check、object type check 和 invocation dispatch 顺序。
- Scheduler queue mutation API 及其调用方。
- Zircon reference 中 channel/call、handle transfer、port/event 相关路径；seL4 reference 中 IPC、reply、notification、thread state、scheduler 的失败纪律参考。
- 测试中的真实 invocation path、错误权限、错误对象类型、stale reply、重复 send/recv、乱序 reply。

## Residual Risk Triggers

- IPC path 中 state mutation 早于全部可失败检查。
- Queue ownership 无法说清。
- Reply handoff 没有状态机或只有布尔标记。
- Scheduler queue 被多个模块直接维护。
- Tests 没有断言失败后的 endpoint/TCB/scheduler 状态。
