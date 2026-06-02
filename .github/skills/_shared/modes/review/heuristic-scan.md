# 全局启发扫描 Review Mode

用于没有单次 diff 时，扫描设计区域、proposal set、代码子系统、测试树或 workflow 区域中的长期漂移和系统性坏味道。

## Required Inputs

- 用户目标。
- Review subject：`设计提案` 或 `代码实现`。
- 扫描范围。
- 用户关心的偏移类型或已知痛点。

## Stop Conditions

- 范围太大且没有 focus 时，先收窄到一个产品区域、代码子系统、测试树或文档区。
- 扫描 finding 不是已验证修复方案；需要方案时 handoff 给 architecture planner。
