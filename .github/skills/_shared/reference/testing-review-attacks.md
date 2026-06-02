# Testing Review Attacks Reference

Testing reference 用于把 Ousia-specific review 从“有无测试”推进到“测试是否通过真实边界证明语义和失败无副作用”。

## Scope

使用本正文处理：

- Capability、IPC、scheduler、memory、boot、driver、tooling 的测试策略。
- 失败路径、状态不变性、真实 invocation path、黑队输入。
- Proposal 中的验证计划、implementation diff 中的测试覆盖。
- 全局启发扫描中的长期测试质量风险。

## Planning Prompts

- 谁调用这个能力，前置状态是什么，允许结果是什么，失败后哪些状态必须不变。
- 测试能否通过 public/boundary path 触发语义，而不是直接构造内部 helper 或 error variant。
- 哪些状态需要在失败前后对比：slot/object graph、endpoint queue、TCB state、reply object、frame metadata、page table、scheduler queue、file/runner output。
- 是否需要模型测试、table test、property-like input、fake OSTD boundary 或 integration smoke test。
- 测试夹具是否表达领域语义，还是复制实现内部结构导致重构时一起漂移。
- 验证命令是否覆盖实际改动目标，例如 bare-metal target、doc checker、runner smoke 或 targeted Rust tests。

## Review Attacks

- 测试是否只断言 error variant，不检查失败后的状态不变性。
- 测试是否绕过真实 invocation/syscall/API path，直接调用内部 function 后声称覆盖外部语义。
- 测试是否复制 match table、rights mapping 或 default logic，导致和实现同错。
- Happy path 是否只证明“跑通”，没有覆盖错误权限、错误对象类型、stale descriptor、重复提交、乱序调用或跨 owner 输入。
- 测试夹具是否维护自己的事实源，和 production state owner 不一致。
- Mock/fake 是否过宽，掩盖 OSTD/tooling/kernel 边界问题。
- Proposal 的验证策略是否只列命令，没有说明每个命令覆盖什么风险。

## Black-Team Inputs

- Capability：错误 rights、错误 object type、空 slot、occupied slot、stale cap、revoked cap、跨 CSpace lookup。
- IPC：send to wrong object、recv on empty endpoint、reply without caller、duplicate reply、notification badge accumulation、blocked TCB cancellation。
- Scheduler：重复 enqueue、dequeue missing thread、cross-CPU wakeup、priority/domain placeholder mismatch、timer interrupt during blocked transition。
- Memory：unaligned range、overlapping map、reserved frame、double map、unmap missing mapping、mapping failure after frame allocation。
- Boot/platform：missing device tree node、wrong MMIO range、unexpected exception level、QEMU machine mismatch、serial unavailable。
- Tooling：host-only success、wrong target triple、runner command drift、generated output stale。

## Evidence To Seek

- Test names and comments that state protected semantics。
- Assertions comparing state before and after failure。
- Boundary-path tests or integration smoke tests。
- Negative tests for black-team inputs relevant to the diff。
- Verification output and why it covers the changed files。
- Residual risk notes when hardware/reference/runner coverage is not available。

## Residual Risk Triggers

- No failure-path state comparison。
- Tests directly construct internal errors or private state transitions。
- Tests mirror implementation tables.
- Verification commands are not tied to changed behavior。
- Proposal includes risky semantics but no concrete black-team inputs。
