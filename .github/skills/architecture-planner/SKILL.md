---
name: architecture-planner
description: "Use when: producing architecture plans or proposal packets for new implementation, refactoring, module boundary repair, Ousia OS design updates, Rust kernel/OSTD/tooling design, reference comparisons, dependency decisions, global architecture scans, implementation planning, or owning docs updates."
argument-hint: "mode, target, scope, user goal, inputs, validation expectations, and optional focus"
---

# 架构规划 Facade

这个 skill 是统一 architecture planning 入口。调用方提供 mode、target、scope、user goal、inputs 和可选 focus；本 skill 按 `_shared/index.md` 选择少量 planning 组件。

它生成可 review 的 architecture plan / proposal packet，不直接实施改动，不自证正确，也不审查已经实施的 diff。非平凡方案必须经过 `black-team-review` 的 proposal review；实施完成后再由 `black-team-review` 审查真实 implementation diff。

## 外部接口

调用时提供：

- `mode`：`重构` 或 `新模块`。
- `target`：`产品层` 或 `代码`。
- `scope`：目标文件、文档区域、子系统、测试树、workflow 区域、reference baseline 或扫描范围。
- `user goal`：用户原始目标和不希望偏移的语义。
- `inputs`：当前结构、已知痛点、约束、允许修改范围、验证期待、assumptions、open questions 或 residual risks。
- `focus`：可选。未提供时，根据 mode、target 和 scope 使用默认规范。

调用方不需要选择更多 type。新实现和重构之外的差异由 `target`、`scope` 和 instructions 处理。

## 组合资产

执行时先读取 `.github/skills/_shared/index.md`：

- `mode: 重构` 读取 `.github/skills/_shared/modes/planning/refactor.md`。
- `mode: 新模块` 读取 `.github/skills/_shared/modes/planning/new-module.md`。
- `target: 产品层` 或 `target: 代码` 不单独读取 shared 组件；由相关 instructions 和 scope 投影。
- 输出协议由本 skill 自己声明。

不要一次性加载 `_shared/modes/**`。只有 `_shared/index.md` 选中的 mode 才进入本次 planning 上下文。

规范来源由 instructions 提供。根据 target 和 scope 读取 owning docs、目标代码、相邻模块、测试、reference notes 或本地 third-party/reference source。涉及 Ousia OS 语义防偏移时，先读取 `.github/skills/_shared/reference/index.md` 索引，再按索引选择 1 到 3 个 reference 正文。

## 输入信息

开始前尽量收集：

- 用户目标和不希望改变的行为或设计语义。
- 目标文件、文档区域、相关模块、直接依赖和被依赖方。
- 当前测试、验证命令、失败信息和已知 residual risks。
- 现有设计文档或 instruction 对该区域的约束。
- 是否允许同步修改测试、文档、public API 或 workflow。

涉及项目专用语义、kernel/OSTD/tooling 边界或成熟实现参考时，按 `.github/skills/_shared/reference/index.md` 选择正文并读取证据。资料不足时先输出受限假设和待确认问题，不凭感觉大拆。

## 调用时机

在以下场景使用：

- 用户要求新功能/新实现前的架构方案、代码重构、设计重构、架构清理、模块边界调整、工程化改造或实现计划。
- 项目、子系统、测试树、文档区域或 workflow 出现长期偏移。
- 状态所有权、数据流、错误边界、副作用边界、文档归属或测试切入点不清楚。
- 需要判断能力应属于 kernel、OSTD、tooling、service、driver framework、Package Cell、compatibility 层、用户态或文档 owning area。
- 需要比较 seL4、Asterinas、rust-sel4、Microkit、sDDF、CortenMM、Linux 或其他工业/研究实现。

纯格式化、机械改名、单行 bugfix、只需解释代码或已有明确实施方案时，不需要使用。

## 工作流程

1. 读取本 skill、`_shared/index.md`、index 选中的组件、相关 instructions、目标文件或 owning docs、相邻模块、测试和调用方。
2. 用一两句话说清当前主流程：输入从哪来，输出到哪去，谁拥有状态，失败由谁处理。
3. 判断现有模式是稳定约束还是历史偶然。
4. 找出真正变化轴：经常变化的策略、稳定不变量、外部副作用、传输模型、领域模型、持久化模型和文档归属。
5. 涉及项目专用语义或外部 baseline 时，读取 reference 索引和对应正文，并在 plan 中列出正文、具体文件或符号。
6. 至少比较两个方案：保守局部演进、边界修正、抽象提取、成熟库/现有模块复用、文档归属调整，或暂不改动。
7. 输出符合本 skill 输出要求的 architecture plan / proposal packet。
8. 明确第一个可实施的纵向切片；边界整理、模块拆分或命名修正必须说明如何服务该切片。
9. 给出 proposal review focus；如需进入实施，输出本 skill 声明的 implementation handoff packet。

## Target 特化

`target: 产品层` 重点判断：

- 产品概念、目标/非目标、能力归属和稳定落点是否清楚。
- 设计是否足以指导实现，而不是只给理念、口号或历史过程。
- 项目专用归属、reference/adoption 规则按 `.github/skills/_shared/reference/index.md` 索引选择正文后判断。

`target: 代码` 重点判断：

- 逻辑是否归属到正确边界。
- 状态所有权、数据流、错误边界和副作用顺序是否能用一句话说明。
- 校验、归一化、默认值和错误映射是否有单一权威位置。
- 失败前检查、副作用顺序、状态机表达和项目专用边界按 `.github/skills/_shared/reference/index.md` 索引选择正文后判断。

## 规划原则

优先追求这些结果：

- 状态所有权唯一且可命名。
- 高层策略不反向依赖底层细节。
- 副作用集中在边界层，核心决策可测试。
- 公共抽象保存真实语义，而不是只包装调用。
- 测试覆盖新语义、失败路径、失败后的状态不变性和边界状态，不只覆盖 happy path。

避免这些问题：

- 为了“工程化”增加透传 helper、薄 service、空泛 adapter 或私有小框架。
- 把多个变化频率不同的东西硬塞进一个结构。
- 为了沿用旧模式继续复制旧问题。
- 在内部层层重复防御同一个已经由边界建立的不变量。

项目专用偏好、经验和 checklist 集中维护在 `.github/skills/_shared/reference/**`；从 `.github/skills/_shared/reference/index.md` 索引进入。

## Plan 必须说明

- 用户目标和原始问题。
- Mode：`重构` 或 `新模块`。
- Target：`产品层` 或 `代码`。
- 背景与约束。
- 目标与非目标。
- 当前结构中应继承、演进或停止模仿的部分。
- 至少两个候选方案，以及不选择它们的原因。
- 推荐方案和取舍理由。
- 推荐方案如何改善边界，而不是只增加层数。
- 第一个可实施的纵向切片：目标语义、跨越 owner、边界 API、实现文件、owning docs、测试层级、完成条件、排除范围，以及哪些边界调整是必要前置。
- 模块边界、依赖方向、状态所有权、数据流和副作用边界。
- 状态所有权、数据流、副作用边界、错误映射层和内部 invariant。
- 校验、归一化、权限检查、错误映射和内部 invariant 所在层。
- 文档归属：稳定结论、reference 事实、项目约束和采用理由分别落在哪里；项目专用命名按 reference 表达。
- 已读取的 reference 正文、本地 reference 文件/符号，以及采用、调整或拒绝的理由。
- 测试策略如何覆盖新语义、失败路径、失败后状态不变性和边界状态。
- 兼容性、迁移成本、回滚方式、验证命令和剩余风险。
- 已知 assumptions、open questions、residual risks 和 review focus。

如果计划只能说明边界会更清楚，却不能说明首个可验证纵向切片，必须先收窄 scope 或返回 architecture handoff；不要输出只会导致连续边界整理的实施计划。

如果调用者提供的是已经实施的 diff，本 skill 不应继续审查；应交给 `black-team-review` 按其 mode 规则处理。

## Implementation Handoff

Proposal review 通过或修正后，进入 implementation 的 handoff packet 包含：

- 已通过或已修正的 architecture plan 摘要。
- 第一个可实施纵向切片：目标语义、跨越 owner、边界 API、实现文件、owning docs、测试层级、完成条件和排除范围。
- 允许修改范围。
- 必须保持的不变量和边界。
- 实施步骤。
- 验证命令。
- Implementation review focus。
