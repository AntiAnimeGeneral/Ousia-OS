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
- 如果 Rust source 或 Cargo metadata 改动，运行与改动匹配的 Rust 检查。优先 `cargo fmt --check` 和 `cargo check`；有测试或行为变化时运行 targeted tests。
- 如果只是回答问题、review 文本但不编辑、或讨论设计，除非用户明确要求，否则不运行验证命令。
- 如果文档和代码都改动，分别运行对应检查。
- 如果某个检查无法运行，说明原因和剩余风险。

## 格式化边界

commit-time automation 可以在 commit 创建前写入格式化结果。如果 formatter 从 hook 运行，应使用 pre-commit hook，只格式化相关 staged files 或项目范围，并在 Git 创建 commit 前重新 stage 这些 formatter edits。

不要使用会在 commit 已存在后修改 worktree 的 post-commit formatter。commit path 之外的手动验证优先使用 check-only 命令，例如 `deno fmt --check` 和 `cargo fmt --check`；commit hooks 中应先格式化、重新 stage，再继续检查。

## 报告

最终回复中总结改动文件，并列出已运行的检查及其结果。
