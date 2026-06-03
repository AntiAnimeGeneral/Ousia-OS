# 提案

本目录保存进入实施前的 proposal packet。它介于 `implementation/` 的短期路线草案和具体代码任务之间：每份提案应说明目标、非目标、候选方案、推荐方案、模块边界、状态所有权、迁移步骤、验证命令、review focus 和剩余风险。

提案通过 review 后，实施结论应回写到对应 owning 文档或代码 rustdoc；过期提案不应继续作为规范源。

## 提案列表

1. [00-sel4-baseline-implementation-alignment.md](./00-sel4-baseline-implementation-alignment.md)
   将现有 `kernel` Rust model 的核心对象关系、容器形态、错误边界和测试语义一次性收敛到 seL4 baseline。