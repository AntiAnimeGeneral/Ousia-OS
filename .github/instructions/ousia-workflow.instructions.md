---
applyTo: "**"
description: "Ousia OS workflow：按改动文件选择完成检查，保持格式化自动化安全，并报告验证结果。"
---

# Ousia OS Workflow

本仓库所有工作都使用这些 workflow 规则。

## 完成检查

按当前任务实际改动的文件选择检查。不要因为某个检查存在就运行无关检查。可重复的文档校验流程使用通用 [doc-validation skill](../skills/doc-validation/SKILL.md)，并使用项目自有配置 `design/check-docs.config.json`。

- 如果 `design/**/*.md` 改动，运行文档 hygiene 检查：`deno task --cwd .github/skills/doc-validation check:docs --config ../../../design/check-docs.config.json`。
- 如果 `design/check-docs.config.json` 改动，运行 `deno task --cwd .github/skills/doc-validation fmt:docs-checker --check` 和 `deno task --cwd .github/skills/doc-validation check:docs --config ../../../design/check-docs.config.json`。
- 如果 `.github/skills/doc-validation/scripts/**/*.ts`、`.github/skills/doc-validation/deno.json` 或 `.github/skills/doc-validation/tsconfig.json` 改动，运行 `deno task --cwd .github/skills/doc-validation fmt:docs-checker --check`、`deno task --cwd .github/skills/doc-validation check:types`、`deno task --cwd .github/skills/doc-validation lint:docs-checker`、`deno task --cwd .github/skills/doc-validation test:docs` 和 `deno task --cwd .github/skills/doc-validation check:docs --config ../../../design/check-docs.config.json`。
- 如果 `.github/instructions/**/*.instructions.md` 或 `.github/skills/**/SKILL.md` 改动，检查 YAML frontmatter 和 description。只有这些编辑影响文档链接、文档结构或验证命令时，才运行文档检查。
- 如果 `.github/skills/_shared/reference/**/*.md` 改动，运行 `deno task --cwd .github/skills/doc-validation check:docs --config ../../../design/check-docs.config.json`，并检查 reference index 是否仍能路由到正文。
- 如果 Rust source 或 Cargo metadata 改动，运行与改动匹配的 Rust 检查。优先 `cargo fmt --check` 和 `cargo check`；有测试或行为变化时运行 targeted tests。
- 如果只是回答问题、review 文本但不编辑、或讨论设计，除非用户明确要求，否则不运行验证命令。
- 如果文档和代码都改动，分别运行对应检查。
- 如果某个检查无法运行，说明原因和剩余风险。

## 组合式规范和 Skill 使用规则

- 开发规范放在 `.github/instructions/*.instructions.md` 中。`development-standards.instructions.md` 是索引，具体规范拆在 `development-entry`、`architecture-abstraction`、`implementation-quality`、`testing-evolution` 和 `design-task` 模块。
- 项目元架构规范放在 `.github/instructions/prompt-architecture.instructions.md` 中；修改代码边界、文档归属、skills、reference 或 workflow 前，必须按该规范检查边界性、正交可组合性、简约性和闭环可执行性。
- 实现者、架构师、黑队 reviewer 和 proposal reviewer 都必须按任务读取对应规范模块。不要把规范正文复制到 skill 中。
- `.github/skills/_shared/**` 是组合资产，不是规范源本身。它们只负责少量任务维度：architecture planner 的 `mode/target`，black-team review 的 `subject/mode`。输出协议归入口 skill 自己声明；subagent 和 handoff 协议归本 workflow 声明。
- `.github/skills/_shared/reference/**` 是快速变动的项目经验库和 checklist corpus，不是硬规范源。入口 skill 读取 reference index 后按 scope 选择正文；被动 reference 正文不应包含外部调用协议、trigger table、mode/target/subject 定义或 subagent prompt contract。
- 入口 skill 负责发现和路由：声明适用场景、外部维度、必须读取的 shared assets 和 focus。入口 skill 不应承载整份开发规范、完整 checklist 或通用输出协议。
- 如果发现某条规则是所有角色都应遵守的规范，把它写入 `.github/instructions/**`；如果只是某个 skill 如何组合规范和输出，把它写入 `.github/skills/_shared/**` 或入口 skill。

## 外部 Skill 接口

- 外部调用优先使用 facade 入口，而不是手动拼接 `_shared` 组合资产。
- 黑队 review 的默认 facade 是 [black-team-review skill](../skills/black-team-review/SKILL.md)。调用方提供 `subject`、`mode`、`scope`、`user goal`、`inputs` 和可选 `focus`；入口 skill 内部按 `_shared/index.md` 选择 review mode。
- 不再暴露 implementation/test/proposal 的专项 review skill。专项性由 `black-team-review` 的 `subject`、`mode`、`scope` 和 instructions 展开。
- Shared assets 不是外部入口，不应被当作 subagent skill 直接调用。

## Review Subagent 启动协议

- Review 类 subagent 必须只读，只做读取、搜索、分析和报告，不修改文件。
- 调用 review subagent 时，必须显式指定与当前主上下文同型号、且带 provider 后缀的完整模型名，例如 `gpt-5.5::fast (oaicopilot)`。不要使用裸型号名、`Auto`、Copilot 默认模型或任何隐式 fallback。
- 如果指定失败或当前工具无法显式指定同型号模型，跳过该 subagent review，并在最终报告中说明原因，把未运行同型号 review 标记为剩余风险。
- subagent prompt 必须要求 subagent 读取对应入口 skill，并说明当前 subject、mode、用户目标、输入材料、范围、已运行或计划运行的检查、review focus 和输出要求。
- 入口 skill 如果声明组合资产，subagent 必须按该 skill 的“组合资产”段读取 `_shared/index.md` 选中的 mode；不要把 shared asset 当作独立 skill 调用。
- `doc-validation` 是 standalone validation skill，不属于 review/architect 组合资产模型。

Review 类 subagent prompt 必须包含：

- Review subject：`设计提案` 或 `代码实现`。
- Review mode：`diff` 或 `全局启发扫描`。
- 用户原始目标：保留用户的关键原话和不希望偏移的语义。
- Review scope：真实 diff、文件列表、子系统、proposal packet、测试树或文档区域。
- Inputs：实现摘要、proposal packet、验证结果、测试结果、已知 assumptions、open questions、residual risks。
- Invariants：必须保持的边界、状态所有权、错误模型、测试语义、文档归属或 workflow 约束。
- Evidence to read：入口 skill、`_shared/index.md`、index 路由到的 mode、相关 instructions、目标文件、相邻模块、owning docs 或 reference。
- Checks：已运行或计划运行的验证命令，以及它们覆盖或未覆盖的风险。
- Review focus：调用者希望重点攻击的问题。
- Output requirements：使用对应入口 skill 的输出要求，必要时追加本 workflow 的 handoff packet。

Prompt 必须要求 subagent 只读，不修改文件；不得生成完整替代方案；结构性问题通过 handoff packet 交给 architecture planner；无法证明的部分标为 residual risk，不要补假设后放行。

## Review/Architect 闭环

- 实现闭环：完成非平凡实现、重构、架构边界调整或行为变更后，使用 [black-team-review skill](../skills/black-team-review/SKILL.md) 审查真实 diff、验证结果和行为风险。prompt 必须声明 subject：`代码实现`，mode：`diff`；如果没有真实 diff，只能在用户明确要求时切换为 mode：`全局启发扫描`。
- 测试专项闭环：测试新增、测试重构、用户质疑测试质量、需要全局扫描某个测试/子系统，架构师提案需要审查测试策略，或 implementation review 发现测试可能只是复述实现时，使用 [black-team-review skill](../skills/black-team-review/SKILL.md)。prompt 必须声明 subject：`代码实现` 或 `设计提案`，mode：`diff` 或 `全局启发扫描`。
- 架构规划闭环：当新实现、重构、子系统或文档区域需要先明确边界、状态所有权、错误模型、测试策略或设计结论落点时，使用 [architecture-planner skill](../skills/architecture-planner/SKILL.md) 生成 architecture plan / proposal packet；architect 不直接实施，也不自证正确。
- 提案审查闭环：架构提案进入实施或 owning docs 落地前，使用 [black-team-review skill](../skills/black-team-review/SKILL.md) 并声明 subject：`设计提案`，mode：`diff`。提案通过或修正后才能实现；实现后再回到 `代码实现 + diff` review。
- Handoff：review 发现结构性问题时，按本 workflow 的 handoff packet 输出，交给对应 architect；`设计提案 + diff` review 通过后，handoff 给 implementation，并明确验证命令和 `代码实现 + diff` review focus。
- 主 agent 根据 review findings 决定是否继续修复、调整提案并重新验证。
- 纯文案小改、机械改名、格式修正或用户明确跳过 review 时，可以不运行 review subagent。

从 review 到 architecture planner 的 handoff packet 包含：

- 目标产品区域、代码模块或文档区域。
- 触发 handoff 的 findings。
- 疑似边界、状态所有权、错误模型、测试质量或文档归属问题。
- 必须保留的外部语义。
- 建议 planner 比较的候选方向。
- 需要 `设计提案 + diff` review 重点攻击的问题。

从 `设计提案 + diff` review 到 implementation 的 handoff packet 包含：

- 已通过或已修正的 architecture plan 摘要。
- 允许修改范围。
- 必须保持的不变量和边界。
- 实施步骤。
- 验证命令。
- `代码实现 + diff` review focus。

## 格式化边界

commit-time automation 可以在 commit 创建前写入格式化结果。如果 formatter 从 hook 运行，应使用 pre-commit hook，只格式化相关 staged files 或项目范围，并在 Git 创建 commit 前重新 stage 这些 formatter edits。

不要使用会在 commit 已存在后修改 worktree 的 post-commit formatter。commit path 之外的手动验证优先使用 check-only 命令，例如 `deno fmt --check` 和 `cargo fmt --check`；commit hooks 中应先格式化、重新 stage，再继续检查。

## 报告

最终回复中总结改动文件，并列出已运行的检查及其结果。
