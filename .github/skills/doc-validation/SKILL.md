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
   - `.github/skills/_shared/reference/**/*.md`: reference corpus entries.
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
| `.github/skills/_shared/reference/**/*.md`                                         | `deno task --cwd .github/skills/doc-validation check:docs --config ../../../design/check-docs.config.json`, plus confirm the reference index routes to existing entries                                                                                                                                                                                                         |
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

## Implementation Boundaries

- [scripts/check-docs.ts](./scripts/check-docs.ts) is only the CLI boundary: parse arguments, load config, print diagnostics, and choose the exit code.
- [scripts/check-docs-lib.ts](./scripts/check-docs-lib.ts) is the public library boundary: normalize config, read the document tree, run configured rules, and return a result.
- [scripts/config.ts](./scripts/config.ts) owns config types, defaults, loading, and normalization.
- [scripts/document-tree.ts](./scripts/document-tree.ts) owns filesystem traversal and path normalization.
- [scripts/rules.ts](./scripts/rules.ts) owns validation rules. Rules consume the scanned document tree and normalized config; they should not perform broad filesystem traversal.
- [scripts/diagnostics.ts](./scripts/diagnostics.ts) owns diagnostic collection and output formatting.

Keep project data out of the TypeScript implementation. Repository-specific paths, filename patterns, directory filters, and section-reference patterns belong in the documentation project's config file.
