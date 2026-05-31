---
name: code-refactor-architect
description: "Use when: planning or implementing code refactoring, architecture cleanup, module boundary repair, Rust kernel/OSTD/tooling redesign, industrial engineering review, dependency reuse decisions, or applying development-standards to concrete code changes."
argument-hint: "target files, subsystem, refactor goal, constraints, or validation expectations"
---

# 代码重构架构师

这个 skill 用于把通用开发规范落到具体代码重构。目标不是把代码写得更复杂，而是让代码的职责、边界、状态所有权和演进路径更清楚。

使用这个 skill 时，先读取 `.github/instructions/development-standards.instructions.md`。如果改动涉及 Ousia kernel、OSTD、QEMU runner、Cargo target 或 implementation design，还必须读取 `.github/instructions/ousia-kernel-boundaries.instructions.md`。凡涉及实施、验证、最终报告或按改动文件选择完成检查时，都必须读取 `.github/instructions/ousia-workflow.instructions.md`。

## 调用时机

在以下场景使用：

- 用户要求代码重构、架构清理、模块边界调整、工程化改造或工业界最佳实践落地。
- 现有实现职责混杂，状态所有权、数据流、错误边界或副作用边界不清楚。
- 需要决定逻辑应该放在 handler/controller、domain、kernel object、OSTD、tooling 或测试支持层中的哪一层。
- 重构触及 capability、IPC、scheduler、memory、boot、tooling、doc checker 或跨模块 API。
- 用户要求“优雅”“框架化”“边界感”“工程哲学”时，用这个 skill 把抽象冲动转化为可验证的工程边界。

纯格式化、机械改名、单行 bugfix 或只需解释代码时，不需要使用。

## 输入信息

开始前尽量收集：

- 用户目标和不希望改变的行为。
- 目标文件、相关模块、直接依赖和被依赖方。
- 当前测试、验证命令和失败信息。
- 现有设计文档或 instruction 对该区域的约束。
- 是否允许同步修改测试、文档或 public API。

如果这些信息不足，先探索代码，再给出受限假设；不要凭感觉大拆。

## 工作流程

1. 读取相关 instruction、目标文件、相邻模块、测试和调用方。
2. 用一两句话说清当前主流程：输入从哪来，输出到哪去，谁拥有状态，失败由谁处理。
3. 判断现有模式是稳定约束还是历史偶然：看它是否被多个模块一致采用、是否有测试依赖、是否代表外部契约。
4. 找出真正的变化轴：经常变化的策略、稳定的不变量、外部副作用、传输模型、领域模型和持久化模型。
5. 至少比较两个方案：保守局部演进、边界修正、抽象提取、成熟库/现有模块复用，或暂不改动。
6. 推荐最小可验证方案，说明为什么它改善边界而不是只增加层数。
7. 实施时保持改动范围贴近目标行为；不要顺手重排无关代码。
8. 完成后运行与改动匹配的验证，并按 workflow 触发 review。

## 重构原则

优先追求这些结果：

- 状态所有权唯一且可命名。
- 高层策略不反向依赖底层细节。
- 校验、归一化、默认值和错误映射有单一权威位置。
- 副作用集中在边界层，核心决策可测试。
- 类型、enum 和显式 match 表达状态机，不用 wildcard fallback 吞掉未来状态。
- 公共抽象保存真实语义，而不是只包装调用。
- 模块名和类型名暴露职责，避免模糊的 manager、handler、data、info。
- 测试覆盖新语义、失败路径和边界状态，不只覆盖 happy path。

避免这些问题：

- 为了“工程化”增加透传 helper、薄 service、空泛 adapter 或私有小框架。
- 把多个变化频率不同的东西硬塞进一个结构。
- 为了沿用旧模式继续复制旧问题。
- 在内部层层重复防御同一个已经由边界建立的不变量。
- 在 `kernel` 中引入架构 cfg、MMIO、boot stack、QEMU machine 或 host tooling 细节。

## 输出格式

重构提案或实施计划应包含：

- 背景与约束。
- 当前结构中应继承、演进或停止模仿的部分。
- 候选方案与取舍。
- 推荐方案和依赖方向。
- 状态所有权、数据流、副作用边界、校验/归一化所在层。
- 实施步骤和每步验证方式。
- 兼容性、迁移成本、回滚方式和剩余风险。
- 需要交给 review 阶段重点检查的问题。

如果已经实施，最终报告还应列出改动文件、验证结果和未覆盖风险。

## 提案 Review 闭环

架构师提案不能自证正确。非平凡重构、边界调整、行为变化或跨文件设计进入实施前，应优先调用 `.github/skills/red-team-review/SKILL.md` 做只读复查；如果风险只在实现后才暴露，完成后也要复查。

review 必须继承 red-team-review 的约束：优先交给独立只读 subagent，显式指定同型号且带 provider 后缀的模型；不能显式指定时不要 fallback 到默认模型，并把未运行同型号 review 记录为剩余风险。

交给 review 时至少提供：

- 用户目标。
- 重构提案或实现摘要。
- 改动文件或计划改动文件。
- 已运行或计划运行的验证。
- 特别关注的边界：状态所有权、薄抽象、错误映射、测试缺口、seL4 baseline、kernel/OSTD/tooling 污染。

review 必须优先找真实 bug、边界错位、语义漂移和缺失测试。没有阻塞问题时，也要列出 residual risks 和 recommended follow-ups。
