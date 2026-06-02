# Kernel Baseline Reference

Kernel baseline reference 用于把 Ousia kernel 的 Phase 1 实现压回 seL4 baseline：先在 Rust 中复刻 seL4 的 capability、object、invocation、IPC 和 scheduler baseline，再评估 Ousia-specific 扩展。

## Scope

使用本正文处理：

- Capability、CSpace、CNode、slot、rights、guard 和 lookup。
- Untyped、retype、object creation、derivation 和 revoke/delete 方向。
- Endpoint、Notification、TCB、reply object、syscall/invocation。
- seL4 baseline 与 Ousia-specific interface 的差异判断。
- Rust 类型化表达和 seL4 baseline 语义之间的 drift 判断。

## Planning Prompts

- 当前设计是否先映射到 seL4 baseline；任何偏离是否被明确标为 post-baseline Ousia extension。
- “更 Rust”的类型、enum、error 或 helper 是否只表达不变量，没有改变 seL4 对象关系、调用语义、权限语义或状态机。
- Capability lookup 的真相源是 CSpace/CNode/slot，还是被其他结构重复维护。
- Rights、object type、badge/guard、slot occupancy 的检查边界在哪里；检查完成后内部是否能信任 invariant。
- Untyped/retype 是否先建立 typed frame/object metadata 和 derivation 关系，再讨论 allocator 细节。
- Endpoint/Notification/TCB 的状态机是否能用 enum 和显式 transition 表达。
- Syscall/invocation 的外部错误类别是否少而稳定，内部诊断是否没有泄漏成长期 public ABI。

## Review Attacks

- Diff 是否绕过 capability lookup，直接拿内部 object reference 做权限或对象类型判断。
- Diff 是否引入 Portal、Operation、Continuation、EventPort、Service Graph、Package Cell、Device Service 或浏览器授权语义，并把它们当成 Phase 1 kernel baseline。
- Diff 是否只说 `seL4-like`，却没有列出本地 `third_party/sel4` 或 `third_party/rust-sel4` 中对应源码、符号或语义映射。
- Rust 类型是否改变了 seL4 的 Reply cap 生命周期、Endpoint queue、Notification badge accumulation、CNode lookup、Untyped retype、copy/mint/move/delete/revoke 语义。
- slot/object generation 是否从 stale detection 或诊断辅助漂移成授权语义、freshness policy、lease 或 service capability 替代品。
- Rights 或 object type 检查是否散落在多个层，导致某条 invocation path 可漏检或重复检。
- Retype/object creation 是否在全部外部输入检查前就修改 slot、derivation graph 或 object table。
- Endpoint queue、reply object、TCB blocked/runnable 状态是否在失败路径留下部分 mutation。
- Wildcard match 是否吞掉未来 object type、thread state 或 invocation label。
- 错误类型是否暴露 expected/actual、slot、rights 等细节，但调用方、测试或 trace 没有消费。
- Rust 类型是否只是把 seL4 名称换皮，没表达所有权、不变量或状态转换。

## Evidence To Seek

- 本地 seL4 reference 中的对应 object、capability、CSpace、endpoint、notification、TCB 或 invocation 路径。
- 本地 rust-sel4 reference 中的 Rust 表达方式是否只是 wrapper/binding，而不是语义替代。
- 目标代码中的状态 enum、transition、slot mutation、queue mutation 和 error mapping。
- 通过真实 invocation/syscall 路径触发的测试，而不是直接构造内部 error variant。
- 失败路径前后 slot/object/queue/thread state 不变性的证据。
- Proposal 中的 baseline comparison、Ousia constraint 和 adoption decision。

## Residual Risk Triggers

- 没有读取本地 seL4/rust-sel4 reference，却声称对齐 baseline。
- Baseline 语义和 Ousia-specific 改动混在同一个设计段落、测试目标或阶段验收项里。
- Capability/object/TCB 状态机没有清晰 owner。
- 测试只断言 error variant，不证明失败无副作用。
- Public error 结构携带未被消费的内部诊断字段。
