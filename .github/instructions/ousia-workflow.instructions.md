---
applyTo: "**"
description: "Ousia OS project workflow: choose validation commands based on actual files changed; use doc checks only when design Markdown changed."
---

# Ousia OS Workflow

Use this instruction for all work in this repository.

## Completion Checks

Choose checks according to the files actually changed in the current task. Do not run unrelated checks just because they exist.

- If `design/**/*.md` changed, run the documentation hygiene check: `ruby scripts/check-docs.rb`.
- If Rust source or Cargo metadata changed, run Rust checks appropriate to the change. Prefer `cargo fmt --check` and `cargo check`; run targeted tests when tests exist or behavior changed.
- If only answering questions, reviewing text without edits, or discussing designs, do not run validation commands unless explicitly asked.
- If both documentation and code changed, run the relevant checks for both surfaces.
- If a check cannot be run, report why and what risk remains.

## Design Documentation Hygiene

When editing `design/**/*.md`, keep the documentation structure consistent:

- Markdown links must resolve.
- `design/core/` mainline chapters must remain continuously numbered.
- A core file's filename number must match its H1 title number.
- Do not leave stale references to removed or renumbered core files.
- If document structure or ownership changes, update `design/target.md` and `design/topics/06-roadmap.md` when they are affected.
- Deep design review is not required for routine edits. Only perform broader architectural review when requested.

## Reporting

In the final response, summarize the changed files and list the checks that were run with their result.
