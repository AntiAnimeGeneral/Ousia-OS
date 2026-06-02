# Kernel Baseline Reference

Kernel baseline reference 用于把 Ousia kernel 的早期实现压回 seL4-like 基线：先形成 capability、object、invocation 和 scheduler 的闭环，再评估 Ousia-specific 改动。

## Scope

使用本正文处理：

- Capability、CSpace、CNode、slot、rights、guard 和 lookup。
- Untyped、retype、object creation、derivation 和 revoke/delete 方向。
- Endpoint、Notification、TCB、reply object、syscall/invocation。
- seL4 baseline 与 Ousia-specific interface 的差异判断。

## Planning Prompts

- 当前设计是否先表达 seL4 baseline，再说明 Ousia 为什么偏离。
- Capability lookup 的真相源是 CSpace/CNode/slot，还是被其他结构重复维护。
- Rights、object type、badge/guard、slot occupancy 的检查边界在哪里；检查完成后内部是否能信任 invariant。
- Untyped/retype 是否先建立 typed frame/object metadata 和 derivation 关系，再讨论 allocator 细节。
- Endpoint/Notification/TCB 的状态机是否能用 enum 和显式 transition 表达。
- Syscall/invocation 的外部错误类别是否少而稳定，内部诊断是否没有泄漏成长期 public ABI。

## Review Attacks

- Diff 是否绕过 capability lookup，直接拿内部 object reference 做权限或对象类型判断。
- Rights 或 object type 检查是否散落在多个层，导致某条 invocation path 可漏检或重复检。
- Retype/object creation 是否在全部外部输入检查前就修改 slot、derivation graph 或 object table。
- Endpoint queue、reply object、TCB blocked/runnable 状态是否在失败路径留下部分 mutation。
- Wildcard match 是否吞掉未来 object type、thread state 或 invocation label。
- 错误类型是否暴露 expected/actual、slot、rights 等细节，但调用方、测试或 trace 没有消费。
- Rust 类型是否只是把 seL4 名称换皮，没表达所有权、不变量或状态转换。

## Evidence To Seek

- 本地 seL4 reference 中的对应 object、capability、CSpace、endpoint、notification、TCB 或 invocation 路径。
- 目标代码中的状态 enum、transition、slot mutation、queue mutation 和 error mapping。
- 通过真实 invocation/syscall 路径触发的测试，而不是直接构造内部 error variant。
- 失败路径前后 slot/object/queue/thread state 不变性的证据。
- Proposal 中的 baseline comparison、Ousia constraint 和 adoption decision。

## Residual Risk Triggers

- 没有读取本地 seL4/rust-sel4 reference，却声称对齐 baseline。
- Baseline 语义和 Ousia-specific 改动混在同一个设计段落里。
- Capability/object/TCB 状态机没有清晰 owner。
- 测试只断言 error variant，不证明失败无副作用。
- Public error 结构携带未被消费的内部诊断字段。
