# Kernel Baseline Reference

Kernel baseline reference 用于把 Ousia kernel 的 Phase 1 实现压回 Ousia 原生高级 capability kernel 路线：handle/object、VM、channel/call、scheduler、VFS/Object Namespace 和 driver/resource 边界是一等设计目标；Zircon/Fuchsia 提供结构参考，seL4 提供 capability discipline 和失败无副作用参考。

## Scope

使用本正文处理：

- Handle、kernel object、rights、generation、lifetime、lookup 和 revoke/delete 方向。
- Kernel object manager、process-local handle table、dispatcher/object enum、syscall/object boundary。
- Channel/call、message bytes + handle transfer、wait set、event/notification 和 async completion。
- VMO/MemoryObject、VMAR/address space、kernel allocator、VFS/Object Namespace 和 driver/resource handle。
- Zircon/Fuchsia structural reference、seL4 capability discipline reference 与 Ousia adoption decision 的差异判断。

## Planning Prompts

- 当前设计是否服务 Ousia-native handle/object kernel，而不是复刻参考内核 plumbing。
- Zircon/Fuchsia 的 handle/object/VM/channel 参考是否被转化为 Ousia 约束、采用理由和拒绝边界。
- seL4 的 capability discipline 是否保留下来：不可伪造、不可扩权、可撤销、失败无部分提交。
- Handle lookup 的真相源是 process-local handle table / object manager，还是被多个结构重复维护。
- Rights、object type、generation、lifetime、budget/quota 的检查边界在哪里；检查完成后内部是否能信任 invariant。
- VM allocator、VFS cache、object metadata 或 channel queue 的动态状态是否有 owner、preflight、reclaim 和 hot-path 说明。
- Channel/call、event/wait、process/thread/scheduler 的状态机是否能用 enum 和显式 transition 表达。
- Syscall/invocation 的外部错误类别是否少而稳定，内部诊断是否没有泄漏成长期 public ABI。

## Review Attacks

- Diff 是否绕过 handle/object boundary，直接拿内部 object reference 做权限或对象类型判断。
- Diff 是否继续把 CSpace/CNode/Untyped/retype、Endpoint/Reply cap 等 seL4 plumbing 当成 Ousia public API 或 Phase 1 governing baseline。
- Diff 是否只说 `Zircon-like`，却没有列出本地 `third_party/fuchsia` 中对应源码、符号或语义映射。
- Diff 是否把 Fuchsia component policy、ABI 或 class hierarchy 直接当成 Ousia 规则。
- Rust 类型是否只是包装裸整数 handle，没有表达所有权、不变量、rights、generation 或状态转换。
- handle/object generation 是否从 stale detection 或诊断辅助漂移成授权语义、freshness policy、lease 或 service capability 替代品。
- Rights 或 object type 检查是否散落在多个层，导致某条 syscall path 可漏检或重复检。
- Object creation、VM mapping、channel send/call、VFS lookup 或 driver resource install 是否在全部外部输入检查前就修改 owner state。
- Channel queue、reply/completion、thread blocked/runnable 状态是否在失败路径留下部分 mutation。
- Wildcard match 是否吞掉未来 object type、thread state 或 invocation label。
- 错误类型是否暴露 expected/actual、slot、rights 等细节，但调用方、测试或 trace 没有消费。

## Evidence To Seek

- 本地 Fuchsia/Zircon reference 中的 handle/object、dispatcher、VMO/VMAR、channel/call、driver framework 或 `zx` wrapper 路径。
- 本地 seL4 reference 中的 capability discipline、revoke、rights、失败无副作用或高保证边界参考。
- 目标代码中的状态 enum、transition、slot mutation、queue mutation 和 error mapping。
- 通过真实 syscall/object boundary 触发的测试，而不是直接构造内部 error variant。
- 失败路径前后 handle/object/queue/thread/VM state 不变性的证据。
- Proposal 中的 reference comparison、Ousia constraint 和 adoption decision。

## Residual Risk Triggers

- 没有读取本地 Fuchsia/Zircon reference，却声称采用 handle/object/VM/channel 路线。
- 没有读取 seL4 reference，却声称保留硬撤销或 capability discipline。
- Reference fact 和 Ousia adoption decision 混在同一个设计段落、测试目标或阶段验收项里。
- Handle/object/thread/VM 状态机没有清晰 owner。
- 测试只断言 error variant，不证明失败无副作用。
- Public error 结构携带未被消费的内部诊断字段。
