---
applyTo: "**"
description: "Ousia OS project workflow: choose validation commands by changed files; use the generic doc-validation skill with the design project config."
---

# Ousia OS Workflow

Use this instruction for all work in this repository.

## Completion Checks

Choose checks according to the files actually changed in the current task. Do not run unrelated checks just because they exist. For repeatable documentation validation workflow details, use the generic [doc-validation skill](../skills/doc-validation/SKILL.md) with the project-owned config at `design/check-docs.config.json`.

- If `design/**/*.md` changed, run the documentation hygiene check: `deno task --cwd .github/skills/doc-validation check:docs --config ../../../design/check-docs.config.json`.
- If `design/check-docs.config.json` changed, run `deno task --cwd .github/skills/doc-validation fmt:docs-checker --check` and `deno task --cwd .github/skills/doc-validation check:docs --config ../../../design/check-docs.config.json`.
- If `.github/skills/doc-validation/scripts/**/*.ts`, `.github/skills/doc-validation/deno.json`, or `.github/skills/doc-validation/tsconfig.json` changed, run `deno task --cwd .github/skills/doc-validation fmt:docs-checker --check`, `deno task --cwd .github/skills/doc-validation check:types`, `deno task --cwd .github/skills/doc-validation lint:docs-checker`, `deno task --cwd .github/skills/doc-validation test:docs`, and `deno task --cwd .github/skills/doc-validation check:docs --config ../../../design/check-docs.config.json`.
- If `.github/instructions/**/*.instructions.md` or `.github/skills/**/SKILL.md` changed, check YAML frontmatter and descriptions. Run documentation checks only when those edits affect documentation links, documentation structure, or validation commands.
- If Rust source or Cargo metadata changed, run Rust checks appropriate to the change. Prefer `cargo fmt --check` and `cargo check`; run targeted tests when tests exist or behavior changed.
- If only answering questions, reviewing text without edits, or discussing designs, do not run validation commands unless explicitly asked.
- If both documentation and code changed, run the relevant checks for both surfaces.
- If a check cannot be run, report why and what risk remains.

## Design Documentation Hygiene

When editing `design/**/*.md`, keep the documentation structure consistent:

- Markdown links must resolve.
- Numbered Markdown files must remain continuously numbered within their own directory.
- Numbered Markdown files must have filename numbers matching their H1 title numbers.
- Do not leave stale references to removed or renumbered numbered Markdown files.
- `target.md §x.y` references must point to sections that still exist in `design/target.md`.
- If the documentation tree changes shape, update `design/check-docs.config.json` when the existing generic checker rules can express the new structure.
- If document structure or ownership changes, update `design/target.md` and `design/topics/06-roadmap.md` when they are affected.
- Deep design review is not required for routine edits. Only perform broader architectural review when requested.

## Validation Boundaries

The doc checker implementation is generic. Keep Ousia-specific document topology and regex data in `design/check-docs.config.json`, not in `.github/skills/doc-validation/scripts/**/*.ts`. Change TypeScript only for a new class of validation logic, not to encode this repository's current directory names.

## Formatting Boundaries

Commit-time automation may write-format before the commit is created. If a formatter runs from a hook, make it a pre-commit hook that formats only the relevant staged files or project scope, then re-stages those formatter edits before Git creates the commit.

Do not use a post-commit formatter that mutates the worktree after the commit exists. For manual validation outside the commit path, prefer check-only commands such as `deno fmt --check` and `cargo fmt --check`; for commit hooks, format first, re-stage, then continue to checks.

## Reporting

In the final response, summarize the changed files and list the checks that were run with their result.
