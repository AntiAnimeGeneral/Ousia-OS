---
applyTo: "**"
description: "测试与演进规范：测试语义、失败无副作用、黑队输入、可测试性信号、兼容迁移与观测。"
---

# 测试与演进规范

这些规则用于实现者、测试 reviewer、黑队 reviewer 和架构师判断测试是否约束真实语义，以及实现是否可演进。

## 可测试性

- 把业务决策与副作用分开，让单元测试和集成测试有清晰切入点。
- 易测试的代码形状本身是边界清晰和解耦的证据。若重要行为只能靠构造内部细节、窥探私有实现或复制 match 表才能测试，应优先怀疑模块边界，而不是先写更复杂的测试。
- 外部系统、时间、随机数、配置和权限等环境依赖影响逻辑时，应可替换、可注入、可模拟。

## 测试语义

- 写测试时先以测试工程师视角说明使用语义：谁调用、前置状态是什么、允许的结果是什么、失败后哪些状态必须不变。
- 测试应约束这些语义不偏移，而不是对着某个函数实现细节射箭后再画靶子。
- 测试不得只是复述实现，例如直接构造内部 error variant、逐项复制映射表、或断言 helper 的机械返回值。
- 除非测试的是 ABI 编号、协议常量或稳定格式这类外部契约，否则应通过真实调用路径触发行为。
- 每个非平凡测试应有简短注释或足够语义化的测试名，说明它保护的约束。
- 优先写端到端边界、状态转移、不变量、权限/能力语义、失败无副作用和跨模块协作测试。
- 测试用于防止语义意外漂移，不是禁止语义演进。确实需要改变语义时，应先说明新语义、目标/非目标、迁移和回滚风险，再同步更新 owning doc、test contract 和测试断言；不要把失败测试当作实现细节噪音直接改到通过。

## 测试层级

- 单元测试验证单一 owner 内部的本地语义，例如权限判断、输入校验、状态 enum 转换、对象表 lookup 或 capability lineage。单元测试可以直接调用模块 API，但不应把 private helper 的机械返回值当成产品语义。
- Host integration 测试验证宿主 Rust test harness 下的跨 owner 行为，例如 `KernelState::execute_invocation`、CSpace/ObjectTable/ThreadTable/Scheduler 协作、失败无副作用和边界错误映射。它们仍不是 QEMU 或 bare-metal 测试。
- QEMU smoke 测试只验证 boot/platform 链路没有断裂，例如 kernel entry、early heap、serial marker、exception marker 和 runner 参数。Smoke 不负责证明 capability、IPC 或 scheduler 深层语义。
- Platform/bare-metal integration 测试用于未来验证 no_std 环境下 kernel、OSTD、基础服务和硬件模拟协作；在该 harness 未形成前，不要把 host integration 结果说成 bare-metal 语义已经验证。
- 模型/形式化测试用于未来冻结 capability derivation、IPC、mapping、revoke 和并发不变量；当前如果没有工具链，不要在实现 review 中假装已有形式化覆盖。

## Test contract

- 每个非平凡测试应能从测试名或短注释中读出 `Goal`、`Scope` 和 `Semantics`：目标语义是什么，测试层级/调用边界是什么，成功或失败后哪些状态必须成立或保持不变。
- Table-driven 测试可以在测试组上写统一 contract，但每个 case 必须有语义化 label，不能只复制实现 match table。
- 失败路径测试必须说明失败发生在哪个边界，以及哪些 owner 的状态不应改变。
- 冒烟测试必须说明 marker 覆盖的 boot/platform 风险，不要把串口输出等同于内核语义测试。

## 黑队输入

- 失败路径测试应覆盖错误返回后的状态不变性。只断言错误 variant 不足以证明边界正确。
- 测试设计应包含黑队视角：尝试重复提交、乱序调用、错误权限、错误对象类型、跨 CPU/跨 slot/跨 owner 输入、部分失败和 stale descriptor，确认实现不会靠偶然路径保持正确。

## 演进

- 破坏性变更必须考虑兼容路径、迁移成本、回滚策略和观测手段。
- 错误应保留上下文；日志和指标应服务于排障和定位，而不是掩盖设计问题。
