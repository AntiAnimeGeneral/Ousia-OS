---
name: doc-validation
description: "Use when: validating documentation trees, Markdown links, numbered document conventions, Deno doc checker changes, workflow instructions, skills, or before final reporting after documentation edits. Runs documentation checks based on changed files."
argument-hint: "changed files or validation goal"
---

# Documentation Validation

Use this skill to choose and run validation for Markdown documentation projects. The goal is to run checks that match the files actually changed, fix deterministic failures, and report the result clearly.

Run Deno tasks from the skill directory so the checker uses the bundled scripts with a project-owned documentation config:

```sh
deno task --cwd .github/skills/doc-validation <task>
```

The checker is configuration-driven. Each documentation project owns its structure config; in this repository, that file is [design/check-docs.config.json](../../../design/check-docs.config.json). Keep document roots, numbered-file patterns, directory sequence rules, target documents, and section-reference patterns there; change TypeScript only when the checker needs a new class of rule.

```sh
deno task --cwd .github/skills/doc-validation check:docs --config ../../../design/check-docs.config.json
```

## Procedure

1. Inspect the changed files with `git diff --name-only` and, when needed, `git diff --cached --name-only`.
2. Classify the changes:
   - `design/**/*.md`: design documentation.
   - `design/check-docs.config.json`: documentation project validation config.
   - `.github/skills/doc-validation/scripts/**/*.ts`: documentation checker implementation or tests.
   - `.github/instructions/**/*.instructions.md` or `.github/skills/**/SKILL.md`: agent customization workflow files.
   - `kernel/**/*.rs`, `**/Cargo.toml`, or `Cargo.lock`: Rust code or Cargo metadata.
3. Run only the checks relevant to those changed files.
4. If a deterministic check fails, fix the cause and rerun the affected check.
5. In the final response, list the changed surfaces and every check that was run with its result.

## Checks

| Changed files                                                                      | Required checks                                                                                                                                                                                                                                                                                                                                                                 |
| ---------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `design/**/*.md`                                                                   | `deno task --cwd .github/skills/doc-validation check:docs --config ../../../design/check-docs.config.json`                                                                                                                                                                                                                                                                      |
| `.github/skills/doc-validation/scripts/**/*.ts` or `design/check-docs.config.json` | `deno task --cwd .github/skills/doc-validation fmt:docs-checker --check`, `deno task --cwd .github/skills/doc-validation check:types`, `deno task --cwd .github/skills/doc-validation lint:docs-checker`, `deno task --cwd .github/skills/doc-validation test:docs`, `deno task --cwd .github/skills/doc-validation check:docs --config ../../../design/check-docs.config.json` |
| `.github/instructions/**/*.instructions.md`, `.github/skills/**/SKILL.md`          | Check YAML frontmatter, ensure `description` is meaningful, then run `deno task --cwd .github/skills/doc-validation check:docs --config ../../../design/check-docs.config.json` if design links or docs changed                                                                                                                                                                 |
| Rust source or Cargo metadata                                                      | `cargo fmt --check`, `cargo check`, and targeted tests when behavior changed or tests exist                                                                                                                                                                                                                                                                                     |

## Documentation Hygiene

The Deno checker lives in [scripts/check-docs.ts](./scripts/check-docs.ts) and uses Deno standard library modules for path handling, argument parsing, and filesystem walking. It validates the document tree configured by [design/check-docs.config.json](../../../design/check-docs.config.json). It checks:

- Markdown links resolve.
- Link text that looks like a Markdown filename matches the actual target filename.
- Numbered Markdown files have H1 numbers matching their filename prefix.
- Numbered Markdown files are continuous within each directory that contains numbered Markdown files.
- Bare `NN-*.md` references point to real current Markdown files.
- `target.md §x.y` references point to sections that actually exist in `design/target.md`.

Do not replace checker failures with one-off allowlists unless the allowlist encodes a real documented exception.

Prefer configuration changes over checker code changes when document roots, numbered patterns, directory filters, target documents, or section-reference patterns move. Change TypeScript only when the checker needs a new class of rule.
