---
name: code-refactor-architect
description: "Use when: producing code refactor architecture proposals, implementation plans, architecture cleanup plans, module boundary repair proposals, Rust kernel/OSTD/tooling redesign plans, industrial engineering review proposals, dependency reuse decisions, or applying development-standards before concrete code changes."
argument-hint: "target files, subsystem, refactor goal, constraints, or validation expectations"
---

# 代码重构架构师

这个 skill 用于把通用开发规范落到代码重构提案和实施计划。目标不是把代码写得更复杂，而是让代码的职责、边界、状态所有权和演进路径更清楚。

它不直接实施代码改动，也不审查已经实施的 diff。提案通过 `architecture-proposal-review` 后，才进入普通实现流程；实现完成后再由 `implementation-diff-review` 审查真实改动。

使用这个 skill 时，先读取 `.github/instructions/development-standards.instructions.md`。如果改动涉及 Ousia kernel、OSTD、QEMU runner、Cargo target 或 implementation design，还必须读取 `.github/instructions/ousia-kernel-boundaries.instructions.md`。凡涉及实施、验证、最终报告或按改动文件选择完成检查时，都必须读取 `.github/instructions/ousia-workflow.instructions.md`。

## 调用时机

在以下场景使用：

- 用户要求代码重构、架构清理、模块边界调整、工程化改造或工业界最佳实践落地。
- 现有实现职责混杂，状态所有权、数据流、错误边界或副作用边界不清楚。
- 需要决定逻辑应该放在 handler/controller、domain、kernel object、OSTD、tooling 或测试支持层中的哪一层。
- 重构触及 capability、IPC、scheduler、memory、boot、tooling、doc checker 或跨模块 API。
- 用户要求“优雅”“框架化”“边界感”“工程哲学”时，用这个 skill 把抽象冲动转化为可验证的工程边界。

纯格式化、机械改名、单行 bugfix 或只需解释代码时，不需要使用。

## 输入信息

开始前尽量收集：

- 用户目标和不希望改变的行为。
- 目标文件、相关模块、直接依赖和被依赖方。
- 本地 reference 证据，尤其是 `third_party/sel4`、`third_party/asterinas`、`third_party/rust-sel4` 中与目标子系统对应的实现；涉及 Linux 语义时也应说明 Linux 参考来源。
- 当前测试、验证命令和失败信息。
- 现有设计文档或 instruction 对该区域的约束。
- 是否允许同步修改测试、文档或 public API。

如果这些信息不足，先探索代码和本地 reference，再给出受限假设；不要凭感觉大拆。若发现信息缺口来自 instruction/skill 没有要求读取关键 reference，应把这个教训写回对应 instruction 或 skill。

## 工作流程

1. 读取相关 instruction、目标文件、相邻模块、测试和调用方。
2. 涉及 kernel、OSTD、scheduler、IPC、memory、driver 或 boot 时，先读取本地 reference 中对应实现，并在提案中列出读到的具体文件或符号。
3. 用一两句话说清当前主流程：输入从哪来，输出到哪去，谁拥有状态，失败由谁处理。
4. 判断现有模式是稳定约束还是历史偶然：看它是否被多个模块一致采用、是否有测试依赖、是否代表外部契约。
5. 找出真正的变化轴：经常变化的策略、稳定的不变量、外部副作用、传输模型、领域模型和持久化模型。
6. 单独分析错误边界：哪些错误来自外部输入，哪个层建立不变量，哪些内部函数之后可以信任不变量，失败是否会留下副作用。
7. 至少比较两个方案：保守局部演进、边界修正、抽象提取、成熟库/现有模块复用，或暂不改动。
8. 推荐最小可验证方案，说明为什么它改善边界而不是只增加层数。
9. 形成 proposal packet，交给 `architecture-proposal-review` 审查。
10. review 通过后，把实施步骤、验证命令和 implementation diff review focus 交给后续实现流程。

## 重构原则

优先追求这些结果：

- 状态所有权唯一且可命名。
- 高层策略不反向依赖底层细节。
- 校验、归一化、默认值和错误映射有单一权威位置。
- 所有可能因外部输入失败的检查先完成，再做状态修改、对象创建、slot/graph mutation、队列写入或外部副作用。
- 内部不变量由边界建立后，内部实现应信任它们；不要把内部 graph/object 损坏伪装成 public recoverable error。
- 错误库选择必须服务边界语义、`no_std` 约束、ABI 稳定性或调用方行为，不要只为了省样板引入框架。
- 副作用集中在边界层，核心决策可测试。
- 类型、enum 和显式 match 表达状态机，不用 wildcard fallback 吞掉未来状态。
- 公共抽象保存真实语义，而不是只包装调用。
- 模块名和类型名暴露职责，避免模糊的 manager、handler、data、info。
- 测试覆盖新语义、失败路径、失败后的状态不变性和边界状态，不只覆盖 happy path。

避免这些问题：

- 为了“工程化”增加透传 helper、薄 service、空泛 adapter 或私有小框架。
- 把多个变化频率不同的东西硬塞进一个结构。
- 为了沿用旧模式继续复制旧问题。
- 在内部层层重复防御同一个已经由边界建立的不变量。
- 在 `kernel` 中引入架构 cfg、MMIO、boot stack、QEMU machine 或 host tooling 细节。

## 输出格式

重构提案或实施计划应包含：

- 背景与约束。
- 当前结构中应继承、演进或停止模仿的部分。
- 已读取的本地 reference 文件/符号，以及从 seL4、Asterinas、rust-sel4、Linux 或其他成熟实现中继承、调整或拒绝的设计点。
- 候选方案与取舍。
- 推荐方案和依赖方向。
- 状态所有权、数据流、副作用边界、校验/归一化所在层。
- 错误所有权、错误映射层、内部 invariant 表达方式、失败前副作用控制，以及是否需要错误库。
- 实施步骤和每步验证方式。
- 兼容性、迁移成本、回滚方式和剩余风险。
- 需要交给 review 阶段重点检查的问题。

如果调用者提供的是已经实施的 diff，本 skill 不应继续审查；应使用 `.github/skills/implementation-diff-review/SKILL.md`。

## Review 闭环

架构师提案不能自证正确。非平凡重构、边界调整、行为变化或跨文件设计进入实施前，应优先调用 `.github/skills/architecture-proposal-review/SKILL.md` 做只读提案审查。提案通过或修正后，才能进入实现。

实现完成后，如果产生真实代码、文档、配置或 workflow diff，再调用 `.github/skills/implementation-diff-review/SKILL.md` 审查实现结果。不要用 implementation-diff-review 代替提案审查，也不要让架构师自己给自己的提案盖章。

proposal review 和 implementation diff review 都必须遵守 `.github/instructions/ousia-workflow.instructions.md` 中的 Review Subagent 启动协议。

交给 proposal review 时至少提供：

- 用户目标。
- 重构提案和候选方案。
- 计划改动文件、模块或 API。
- 状态所有权、依赖方向、数据流和副作用边界。
- 计划运行的验证。
- 特别关注的边界：状态所有权、薄抽象、错误映射、失败无副作用、内部 invariant、测试缺口、seL4 baseline、kernel/OSTD/tooling 污染。

交给 implementation diff review 时至少提供真实改动文件、实现摘要、关键 diff、已运行检查和剩余风险。
