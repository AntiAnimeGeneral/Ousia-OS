# Diff Mode

用于审查已经落地的设计提案、文档 diff、代码实现 diff、测试变更或 workflow 改动。

## Required Inputs

- 用户目标。
- Review subject：`设计提案` 或 `代码实现`。
- 真实 diff 或改动文件。
- 实现/提案摘要。
- 已运行检查和结果。
- 已知 residual risks。

## Stop Conditions

- 没有真实 diff 或落地文件改动时，不执行 mode：`diff`。
- 输入只是计划、想法或未落地 proposal 时，改用全局启发扫描或要求补充 diff。
