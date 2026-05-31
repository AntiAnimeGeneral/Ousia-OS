---
name: red-team-review
description: "Use when: performing black-team/red-team review after implementation, before final reporting, or when asking a subagent to find bugs, semantic drift, missing tests, boundary violations, and hidden risks with fresh context."
argument-hint: "changed files, implementation summary, validation results, or review focus"
---

# 黑队 Review

这个 skill 用于实现完成后的只读黑队审查。目标不是复述实现，而是从新视角寻找 bug、语义偏移、遗漏测试、边界破坏和未来容易踩坑的风险。

优先把这个 review 交给 subagent 执行，让主实现上下文和审查上下文分离。subagent 应只做读取、搜索、分析和报告，不修改文件。

## 调用时机

在以下场景使用：

- 完成非平凡实现、重构、架构边界调整或行为变更后。
- 改动触及 capability、IPC、scheduler、memory、boot、tooling、doc checker 或 workflow 边界时。
- 用户要求 review、黑队、red-team、找盲点、检查偏移或确认没有语义误差时。
- 最终回复前，如果改动风险高、跨多个文件，或刚刚修过一个隐藏 bug。

纯文案小改、机械改名、格式修正或已经由用户明确跳过 review 时，可以不运行。

## Subagent 提示词模板

把下面信息交给 subagent，并要求它读取本 skill 后执行 review：

- 用户原始目标。
- 本轮实现摘要。
- 改动文件列表。
- 已运行的验证命令和结果。
- 需要特别关注的语义边界或参考基线。

调用 subagent 时必须显式指定与当前主上下文同型号、且带 provider 后缀的完整模型名，例如 `gpt-5.5::fxh (oaicopilot)`。不要使用裸型号名、`Auto`、Copilot 默认模型或任何隐式 fallback。若指定失败或环境无法显式指定模型，停止本次 subagent review，并在最终报告中说明没有运行黑队 subagent；不要自动改用默认模型。

建议提示词：

```text
你是本仓库的只读黑队 reviewer。请读取 .github/skills/red-team-review/SKILL.md，并按其中流程 review 当前改动。

上下文：
- 用户目标：<填入用户目标>
- 实现摘要：<填入实现摘要>
- 改动文件：<填入 git diff --name-only 结果>
- 已运行检查：<填入命令和结果>
- 特别关注：<填入 seL4/OSTD/doc/workflow 等边界>

要求：只读，不修改文件。优先找真实风险和可复现问题；没有问题时明确说明剩余风险和未覆盖测试。
最终只返回 findings、open questions、residual risks 和 recommended follow-ups。
```

## 审查流程

1. 读取相关 instruction、design 文档和被改文件，不要只看实现摘要。
2. 查看 `git diff --name-only` 和关键 diff，确认审查范围。
3. 找与改动直接相邻的依赖方和被依赖方，判断状态所有权、数据流、错误边界和测试边界是否一致。
4. 对照项目基线：
   - 通用工程标准见 `.github/instructions/development-standards.instructions.md`。
   - Ousia kernel/OSTD 边界见 `.github/instructions/ousia-kernel-boundaries.instructions.md`。
   - 文档规则见 `.github/instructions/documentation-standards.instructions.md`。
   - 完成检查见 `.github/instructions/ousia-workflow.instructions.md`。
5. 如果改动涉及 seL4 baseline，重点检查是否偏离 seL4 的 capability、CSpace/CNode、Endpoint、Notification、Reply、TCB、scheduler 或 syscall/invocation 语义。
6. 如果改动涉及 OSTD 或 tooling，重点检查架构细节是否泄漏到 `kernel`，host tooling 是否污染 bare-metal workspace。
7. 如果改动涉及文档或技能，重点检查归属、触发 description、frontmatter、链接、命令和是否把项目数据写进通用工具。
8. 检查测试是否覆盖新语义、失败路径和边界状态；不要只看 happy path。
9. 检查验证命令是否与实际改动匹配；不要要求无关检查。

## 黑队检查清单

优先找这些问题：

- 行为 bug、权限扩大、状态机漏状态、对象生命周期错误、stale descriptor / generation / revoke / move 语义错误。
- seL4 baseline 语义偏移，尤其是把 copy/mint/move、reply cap、badge、grant/grant-reply、blocking/nonblocking、bound notification 混为一谈。
- 状态所有权放错层，例如 invocation 持有调度事实、Notification 读取 TCB 状态、kernel 写架构 cfg、doc checker 写项目路径。
- 错误处理不保留上下文，或用默认值、静默容错掩盖边界问题。
- 测试只覆盖主路径，没有覆盖拒绝扩权、错误 capability、stale descriptor、空队列、重复绑定、跨 CPU 或并发相关风险。
- 文档把实现过程写成稳定规范，或把稳定结论留在 notes 而没有回写 owning 文档。
- 新 abstraction 只是薄包装、透传 helper 或为了风格拆分，不能稳定语义边界。

## 输出格式

最终报告必须以 findings 开头。按严重程度排序，每条 finding 包含：

- 严重级别：`critical` / `high` / `medium` / `low`。
- 文件和位置：尽量给出路径和行号。
- 问题：实际会坏在哪里。
- 证据：来自代码、文档、测试或参考语义。
- 建议：最小修复方向。

如果没有发现问题，明确写：`未发现需要阻塞合入的问题。`

随后列出：

- `Open questions`：需要用户或实现者确认的语义问题。
- `Residual risks`：这次 review 未覆盖或无法证明的风险。
- `Recommended follow-ups`：后续建议，不要混入当前必须修的 finding。

保持高信号，不要为了显得严格而制造低价值噪音。

## 模型选择说明

黑队 review subagent 必须使用与当前主上下文同型号的模型。主 agent 调用 subagent 时，如果工具提供 `model` 参数，必须传入带 provider 后缀的完整模型名，例如 `gpt-5.5::fxh (oaicopilot)`。

如果当前环境不支持显式指定模型，或指定的同型号模型不可用，不能 fallback 到 `Auto`、Copilot 默认模型或其他模型。主 agent 应跳过 subagent review，在最终报告中说明原因，并把“未运行同型号黑队 subagent review”标记为剩余风险。
