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
- 如果 Rust source 或 Cargo metadata 改动，运行与改动匹配的 Rust 检查。主动编辑 Rust 文件后先运行 `cargo fmt` 接受标准格式化，再用 `cargo fmt --check` 和 `cargo check` 验证；有测试或行为变化时运行 targeted tests。
- Host-side targeted tests 使用 `cargo nextest run`。
- Kernel/OSTD 语义改动按测试层级选择检查：单一 owner 的权限、类型、状态转换、内存 map 归一化或错误映射改动运行对应 host unit tests；跨 CSpace/ObjectTable/ThreadTable/Scheduler 的 invocation 或事务行为改动运行 host integration tests；`kernel-bin`、`ostd` boot/platform、linker、QEMU runner 或 boot marker 改动才要求 QEMU smoke。
- 如果只是回答问题、review 文本但不编辑、或讨论设计，除非用户明确要求，否则不运行验证命令。
- 如果文档和代码都改动，分别运行对应检查。
- 如果某个检查无法运行，说明原因和剩余风险。

## 组合式规范和 Skill 使用规则

- 开发规范放在 `.github/instructions/*.instructions.md` 中。`development-standards.instructions.md` 是索引，具体规范拆在 `development-entry`、`architecture-abstraction`、`implementation-quality`、`testing-evolution` 和 `design-task` 模块。
- 项目元架构规范放在 `.github/instructions/prompt-architecture.instructions.md` 中；修改代码边界、文档归属、skills、reference 或 workflow 前，必须按该规范检查边界性、正交可组合性、简约性和闭环可执行性。
- 实现者、架构师、黑队 reviewer 和 proposal reviewer 都必须按任务读取对应规范模块。不要把规范正文复制到 skill 中。
- `.github/skills/_shared/**` 是组合资产，不是规范源本身。它们只负责少量任务维度：architecture planner 的 `mode/target`，black-team review 的 `subject/mode`。输出协议和领域化 handoff 细节归入口 skill 自己声明；workflow 只规定何时进入闭环、何时需要交接和验证。
- `.github/skills/_shared/reference/**` 是快速变动的项目经验库和 checklist corpus，不是硬规范源。入口 skill 读取 reference index 后按 scope 选择正文；被动 reference 正文不应包含外部调用协议、trigger table、mode/target/subject 定义或 subagent prompt contract。
- 入口 skill 负责发现和路由：声明适用场景、外部维度、必须读取的 shared assets 和 focus。入口 skill 不应承载整份开发规范、完整 checklist 或通用输出协议。
- 如果发现某条规则是所有角色都应遵守的规范，把它写入 `.github/instructions/**`；如果只是某个 skill 如何组合规范和输出，把它写入 `.github/skills/_shared/**` 或入口 skill。

## 外部 Skill 接口

- 外部调用优先使用 facade 入口，而不是手动拼接 `_shared` 组合资产。
- 黑队 review 的默认 facade 是 [black-team-review skill](../skills/black-team-review/SKILL.md)。调用方提供 `subject`、`mode`、`scope`、`user goal`、`inputs` 和可选 `focus`；入口 skill 内部按 `_shared/index.md` 选择 review mode。
- 不再暴露 implementation/test/proposal 的专项 review skill。专项性由 `black-team-review` 的 `subject`、`mode`、`scope` 和 instructions 展开。
- Shared assets 不是外部入口，不应被当作 subagent skill 直接调用。

## Subagent 使用边界

- Subagent 只是可选的执行载体，不是独立规范层、独立 skill 或 review/architecture workflow 的 owner。模型可以按任务复杂度自主决定直接执行 skill，或把 skill 上下文交给只读 subagent 执行。
- 调用 subagent 时，必须显式传入当前主上下文使用的同一个完整模型身份字符串。不要使用裸型号名、`Auto`、空字符串、默认模型、不同档位模型或任何隐式 fallback。
- 如果当前工具上下文没有暴露完整模型身份，或同名模型指定失败，不要用空 `model`、默认模型或降级模型重试。改为不启动该 subagent，并在当前输出或最终报告中说明“未能用同名模型启动 subagent”，把对应 exploration、review 或 planning coverage 标记为 residual risk。
- Subagent prompt 只需要描述任务、scope、必须读取的入口 skill 或证据、只读/禁止修改约束，以及期望返回的报告。review、planning 和输出要求由对应入口 skill 承载，不在 subagent 层重新定义。

## Review/Architect 闭环

- 实现闭环：完成非平凡实现、重构、架构边界调整或行为变更后，使用 [black-team-review skill](../skills/black-team-review/SKILL.md) 审查真实 diff、验证结果和行为风险。review 的 subject、mode、prompt 内容和输出要求由该 skill 声明。
- 测试专项闭环：测试新增、测试重构、用户质疑测试质量、需要全局扫描某个测试/子系统，架构师提案需要审查测试策略，或 implementation review 发现测试可能只是复述实现时，使用 [black-team-review skill](../skills/black-team-review/SKILL.md)。
- 架构规划闭环：当新实现、重构、子系统或文档区域需要先明确边界、状态所有权、错误模型、测试策略或设计结论落点时，使用 [architecture-planner skill](../skills/architecture-planner/SKILL.md) 生成 architecture plan / proposal packet；architect 不直接实施，也不自证正确。
- 提案审查闭环：架构提案进入实施或 owning docs 落地前，使用 [black-team-review skill](../skills/black-team-review/SKILL.md) 审查 proposal diff。提案通过或修正后才能实现；实现后再回到 implementation review。
- Handoff：review 发现结构性问题时，按 review skill 的 handoff 要求交给 architecture planner；提案 review 通过后，按 architecture-planner skill 的 implementation handoff 要求进入实施。
- 主 agent 根据 review findings 决定是否继续修复、调整提案并重新验证。
- 纯文案小改、机械改名、格式修正或用户明确跳过 review 时，可以不运行 review。

## 格式化边界

commit-time automation 可以在 commit 创建前写入格式化结果。如果 formatter 从 hook 运行，应使用 pre-commit hook，只格式化相关 staged files 或项目范围，并在 Git 创建 commit 前重新 stage 这些 formatter edits。

不要使用会在 commit 已存在后修改 worktree 的 post-commit formatter。commit path 之外的手动验证优先使用 check-only 命令，例如 `deno fmt --check` 和 `cargo fmt --check`；如果当前任务正在主动编辑 Rust source，应先运行 `cargo fmt` 落地标准格式化，再用 check-only 命令确认。commit hooks 中应先格式化、重新 stage，再继续检查。

## 报告

最终回复中总结改动文件，并列出已运行的检查及其结果。
