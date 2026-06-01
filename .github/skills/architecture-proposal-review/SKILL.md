---
name: architecture-proposal-review
description: "Use when: reviewing code refactor architect proposals, design refactor architect proposals, architecture plans before implementation, boundary decisions, tradeoff analysis, migration plans, or proposal assumptions before they become code or owning docs."
argument-hint: "proposal summary, source architect skill, affected boundaries, assumptions, open questions, or target docs/files"
---

# 架构提案 Review

这个 skill 用于审查实施前、且尚未作为稳定结论写入 owning docs 的架构提案。它不是实现后 diff review，也不是重新生成一个更漂亮的方案；它的职责是从反方视角攻击提案本身，判断问题定义、候选方案、边界、迁移和验证是否足够可靠。

优先把这个 review 交给独立 subagent 执行，让提案生成上下文和审查上下文分离。subagent 应只做读取、搜索、分析和报告，不修改文件。

## 与其他 Skill 的关系

- `code-refactor-architect` 负责提出代码重构或实施架构方案。
- `design-refactor-architect` 负责提出 Ousia OS 设计重构方案。
- `architecture-proposal-review` 负责在实施前审查这些提案。
- `implementation-diff-review` 负责实现后审查真实 diff、测试结果和行为风险。

推荐流程：

1. Architect 生成 proposal packet。
2. 本 skill 审查 proposal packet。
3. 根据 findings 修正提案。
4. 提案通过后再实施代码或更新 owning docs。
5. 实施完成后由 `implementation-diff-review` 审查真实改动。

## 调用时机

在以下场景使用：

- `code-refactor-architect` 或 `design-refactor-architect` 产出非平凡提案后。
- 提案会改变模块边界、依赖方向、状态所有权、错误边界、测试策略或 public API。
- 提案会修改 owning design docs，或把 notes / reference 中的结论提升为稳定设计。
- 提案会影响 kernel/OSTD/tooling 边界、seL4 baseline、多核假设、memory model、driver model、compatibility 策略或 workflow。
- 用户要求在实施前 review、找盲点、确认没有语义误差或比较方案是否充分。

如果已经完成代码或文档改动，并且需要审查真实 diff，应使用 `implementation-diff-review`。

## 输入要求

交给 reviewer 的 proposal packet 至少包含：

- 用户目标和原始问题。
- 由哪个 architect skill 产出提案。
- 背景与约束。
- 目标与非目标。
- 当前结构中准备继承、演进或停止模仿的部分。
- 候选方案和推荐方案。
- 模块边界、依赖方向、状态所有权、数据流和副作用边界。
- 迁移路径、兼容性、回滚方式和验证策略。
- 已知 assumptions、open questions、residual risks 和 review focus。
- 计划修改的文档、代码区域或 API。

输入缺失时，不要替提案补完并直接通过；应把缺失项作为 finding 或 open question。

## Review 关注点

优先检查这些问题：

- 问题定义是否准确，是否把症状误当根因。
- 目标与非目标是否清楚，是否存在范围膨胀。
- 是否至少比较两个真实方案，而不是只包装一个既定答案。
- 推荐方案是否解释了为什么不选其他方案。
- 状态所有权、依赖方向、数据流和副作用边界是否闭合。
- 校验、归一化、默认值、权限检查和错误映射是否有单一权威位置。
- 错误模型是否区分外部可恢复错误、内部不变量破坏和诊断/测试上下文；是否证明可恢复错误返回前不会产生部分副作用。
- 错误类型或错误库选择是否服务调用方行为、边界映射、`no_std`/ABI 约束或测试诊断，而不是只为了风格或样板。
- 是否为了“工程化”引入薄抽象、透传 helper、空泛框架或不可实施的层次。
- 是否误读 seL4、rust-sel4、Microkit、sDDF、Asterinas、CortenMM 或硬件/论文资料。
- 是否过早偏离 seL4 baseline，或过早发明 Ousia-specific 语义。
- 是否把 `design/notes/**` 或外部参考当成 owning docs。
- 是否遗漏 multi-core-only 约束、kernel/OSTD/tooling 边界或 host tooling 隔离。
- 迁移路径、兼容路径、回滚方式和验证策略是否可执行。
- 提案是否能被后续实现和 implementation diff review 验证，而不是只在文字上成立。

## 审查流程

1. 读取本 skill 和相关 architect skill。
2. 读取相关 instructions：
   - `.github/instructions/development-standards.instructions.md`
   - `.github/instructions/documentation-standards.instructions.md`，如果涉及 Markdown 或 design docs
   - `.github/instructions/ousia-kernel-boundaries.instructions.md`，如果涉及 Ousia kernel、OSTD、tooling 或 implementation design
   - `.github/instructions/ousia-workflow.instructions.md`，如果提案涉及实施、验证或 review 流程
3. 读取 proposal packet 指向的 owning docs、代码区域和 reference notes。
4. 判断 proposal 是否完整，先列出阻塞性缺口。
5. 对照候选方案和推荐方案，寻找语义偏移、边界错位、不可验证假设和迁移风险。
6. 输出 findings，不修改文件。

## 输出格式

最终报告必须以 findings 开头。按严重程度排序，每条 finding 包含：

- 严重级别：`critical` / `high` / `medium` / `low`。
- 位置：提案章节、目标文档、代码区域或相关 reference。
- 问题：提案会在哪里失真、不可实施或引入长期风险。
- 证据：来自 proposal、代码、文档、instruction 或外部参考。
- 建议：最小修正方向。

如果没有发现阻塞问题，明确写：`未发现需要阻塞提案进入实施的问题。`

随后列出：

- `Open questions`：需要用户或提案作者确认的问题。
- `Residual risks`：本次 review 无法证明或尚未覆盖的风险。
- `Recommended follow-ups`：后续建议，不要混入当前必须修的 finding。

保持高信号；不要为了显得严格而制造低价值噪音。

## Subagent 协议

调用 review subagent 时，必须遵守 `.github/instructions/ousia-workflow.instructions.md` 中的 Review Subagent 启动协议。
