---
applyTo: "**"
description: "开发规范索引：按任务读取入口、架构抽象、实现质量、测试演进和设计任务模块。"
---

# 开发规范索引

这些规则写给在本仓库工作的 AI agent。它们是直接约束，不是教程。除非任务只是机械改名、纯文案修改或格式修正，否则实现任务默认也包含设计决策，必须同时遵守架构、抽象、边界和演进规则。

本文件是开发规范总入口和兼容索引。具体规范拆在同目录模块中；实现者、架构师、黑队 reviewer 和 proposal reviewer 都应按任务读取相关模块，而不是只依赖本索引。

## 规范模块

- `.github/instructions/development-entry.instructions.md`：需求识别、依赖复用、相邻模块阅读、现有模式判断。
- `.github/instructions/architecture-abstraction.instructions.md`：架构边界、依赖方向、抽象取舍、命名和职责。
- `.github/instructions/implementation-quality.instructions.md`：实现质量、错误边界、失败前置检查、内部 invariant 和 Rust 状态机。
- `.github/instructions/testing-evolution.instructions.md`：测试语义、失败无副作用、黑队输入、可测试性和演进。
- `.github/instructions/design-task.instructions.md`：设计任务、候选方案、边界、迁移、验证和实施计划。
- `.github/instructions/prompt-architecture.instructions.md`：prompt/skill/reference/workflow 的边界性、正交可组合性、简约性、闭环和自我迭代。

## 读取规则

- 实现者至少读取 `development-entry`、`architecture-abstraction` 和 `implementation-quality`；涉及测试或行为变化时读取 `testing-evolution`。
- 架构师读取全部开发规范模块，并按范围追加 Ousia/kernel、documentation 或 workflow instructions。
- 黑队 reviewer 按 review focus 读取对应模块：实现偏移看 entry/architecture/implementation，测试质量看 testing，proposal 审查看 design-task。
- Proposal reviewer 必须读取 `design-task`，并按 proposal 内容追加 architecture、implementation、testing 和领域 instruction。
- 如果任务涉及 kernel、OSTD、tooling、Cargo target 或 implementation design，还必须读取 `.github/instructions/ousia-kernel-boundaries.instructions.md`。
- 如果任务涉及 Markdown、design docs、README、skills 或 instructions，还必须读取 `.github/instructions/documentation-standards.instructions.md`。
- 如果任务涉及 instructions、skills、shared assets、reference corpus、workflow 或 prompt 体系演进，还必须读取 `.github/instructions/prompt-architecture.instructions.md`。
- 如果任务涉及验证、subagent review、handoff 或最终报告，还必须读取 `.github/instructions/ousia-workflow.instructions.md`。

## 组合式工作流

Skills 只负责少量外部维度和入口输出协议：`architecture-planner` 使用 `mode` 与 `target`，`black-team-review` 使用 `subject` 与 `mode`。组合方式由 `.github/instructions/ousia-workflow.instructions.md`、`.github/instructions/prompt-architecture.instructions.md` 和入口 skill 的“组合资产”段约束。

不要在 skill 中复制整份规范。具体产品层设计、代码实现、kernel 边界、文档归属和测试质量规则由 instructions 提供；shared assets 只描述任务形状。
