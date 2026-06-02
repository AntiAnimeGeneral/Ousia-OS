---
name: black-team-review
description: "Use when: running a unified black-team review subagent for implementation diffs, global scans, architecture proposals, test strategies, semantic drift, boundary violations, missing tests, workflow risks, or proposal assumptions."
argument-hint: "subject, mode, scope, user goal, inputs, validation results, and optional review focus"
---

# 黑队 Review Facade

这个 skill 是统一 black-team reviewer 外部入口。调用方只需要说明 subject、mode、scope、user goal、inputs 和可选 focus；本 skill 负责按 `_shared/index.md` 选择少量 review 组件。

它只读审查，不修改文件，不生成完整替代方案。结构性问题通过 handoff packet 交给对应 architect；proposal 通过审查后才进入 implementation。

## 外部接口

调用时提供：

- `subject`：`设计提案` 或 `代码实现`。
- `mode`：`diff` 或 `全局启发扫描`。
- `scope`：真实 diff、文件列表、子系统、测试树、proposal packet、文档区域或 workflow 区域。
- `user goal`：用户原始目标和不希望偏移的语义。
- `inputs`：实现摘要、验证结果、测试结果、proposal packet、已知 assumptions、open questions 或 residual risks。
- `focus`：可选。未提供时，根据 subject、mode 和 scope 使用默认规范。

调用方不需要手动列出更多 review 类型；设计/代码规范由 instructions 提供。

## 组合资产

执行时先读取 `.github/skills/_shared/index.md`，再根据 `subject`、`mode`、`scope` 和 `focus` 读取唯一匹配的 mode 组件。

不要一次性加载 `_shared/modes/**`。只有 `_shared/index.md` 选中的 mode 才进入本次 review 上下文。

规范来源由 instructions 提供。根据 subject 和 scope 读取目标文件、相邻模块、owning docs、测试、reference notes 或验证结果。涉及 Ousia OS 语义防偏移时，先读取 `.github/skills/_shared/reference/index.md` 索引，再按索引选择 1 到 3 个 reference 正文。

## Mode 映射

- `diff`
- `全局启发扫描`

具体 mode component 和 stop conditions 由 `_shared/index.md` 决定。如果 subject、mode 和 scope 不匹配，先把输入不匹配作为 finding 或要求切换 mode；不要替调用方隐式改写任务。

## 证据要求

Review 前尽量收集：

- 用户目标和不希望偏移的语义。
- 真实 diff、proposal packet、扫描范围或目标文件列表。
- 已运行检查、测试结果、失败信息和已知 residual risks。
- 目标区域的 owning docs、相邻模块、调用方和测试。
- 项目专用语义或外部 baseline 的 reference 证据；Ousia OS 场景按 `.github/skills/_shared/reference/index.md` 索引选择正文后收集。

证据不足时，把无法证明的部分列为 residual risk 或输入不匹配 finding；不要补假设后放行。

## Subject 攻击焦点

`subject: 设计提案` 重点攻击：

- 用户目标是否被 proposal 偷换，目标与非目标是否清楚。
- 是否至少比较了两个真实候选方案，而不是只包装单一路径。
- 模块边界、状态所有权、依赖方向、数据流和副作用边界是否闭合。
- 产品层落点、代码落点或 owning docs 是否明确。
- 迁移、兼容性、回滚和验证策略是否可执行。
- Assumptions、open questions 和 residual risks 是否足以阻止误实施。
- Ousia OS 专用漂移风险按 `.github/skills/_shared/reference/index.md` 索引选择正文后追加攻击。

`subject: 代码实现` 重点攻击：

- 真实 diff 是否偏离用户目标、architecture plan 或 owning docs。
- 校验、归一化、默认值和错误映射是否出现多个权威位置。
- 失败路径是否先完成外部输入检查，再做状态修改或外部副作用。
- 内部 invariant 是否被边界建立后仍层层重复防御，或被包装成 public recoverable error。
- 抽象是否只是透传 helper、薄 service、空泛 adapter 或私有小框架。
- 测试是否约束使用语义、失败无副作用和边界状态，而不是复述实现或只覆盖 happy path。
- Ousia OS 专用边界、reference 和实现偏好按 `.github/skills/_shared/reference/index.md` 索引选择正文后追加攻击。

`mode: 全局启发扫描` 只能报告风险和代表性证据；不能把扫描 finding 当成已验证修复方案。结构性问题应 handoff 给 architecture planner。

## 输出要求

Review 输出必须以 `findings` 开头。按严重程度排序，每条 finding 包含：

- 严重级别：`critical` / `high` / `medium` / `low`。
- 位置：文件、测试名、提案章节、代码区域或文档区域；能给行号时给行号。
- 问题：实际会坏在哪里，或哪条语义无法被证明。
- 证据：来自代码、测试、文档、proposal、diff、验证结果或 reference。
- 建议：最小修正方向，或是否需要 handoff 给 architecture planner。

无阻塞问题时使用对应固定句式：

- `设计提案`：`未发现需要阻塞提案进入实施的问题。`
- `代码实现`：`未发现需要阻塞合入的问题。`

随后列出：

- `Open questions`：需要用户、实现者或提案作者确认的问题。
- `Residual risks`：本次 review 无法覆盖或无法证明的风险。
- `Recommended follow-ups`：后续建议，不要混入当前必须修的 finding。

根据 subject 和 mode 追加要求：

- `设计提案` 必须明确是否阻塞 proposal 进入 implementation。
- `代码实现` 必须明确验证结果是否覆盖实际改动。
- 涉及 Ousia OS reference corpus 时，必须列出已读取的 reference 正文；未读取相关正文的部分标为 residual risk。
- `全局启发扫描` 必须明确哪些 finding 只是启发式风险，哪些需要 handoff 给 architecture planner。

需要后续架构处理时，按 workflow instruction 输出 handoff packet。保持高信号；不要为了显得严格而制造低价值噪音。
