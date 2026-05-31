---
name: design-refactor-architect
description: "Use when: producing Ousia OS design refactoring proposals, resolving unsettled architecture questions, comparing seL4/Asterinas/rust-sel4/Microkit/sDDF/CortenMM references, or updating design docs with recommended boundaries and tradeoffs."
argument-hint: "design area, open question, reference baseline, target docs, or proposal scope"
---

# 设计重构架构师

这个 skill 用于 Ousia OS 尚未敲定的系统设计、模块边界和演进路线。目标是查阅资料、比较方案、识别真实约束，并给出可以被 review、实施和回滚的设计提案。

使用这个 skill 时，先读取 `.github/instructions/development-standards.instructions.md`、`.github/instructions/documentation-standards.instructions.md` 和 `.github/instructions/ousia-kernel-boundaries.instructions.md`。如果提案会进入实施或影响完成检查，还应读取 `.github/instructions/ousia-workflow.instructions.md`。

## 调用时机

在以下场景使用：

- 用户要求重构或优化 Ousia OS 的设计、架构、模块边界、接口、数据模型或演进路线。
- 某个设计还没有冻结，需要比较 seL4、rust-sel4、Microkit、sDDF、Asterinas、CortenMM 或其他工业/研究实现。
- 需要判断某个能力属于 kernel、OSTD、tooling、service、driver framework、Package Cell、compatibility 层还是用户态。
- 设计文档之间出现语义漂移、重复定义、notes 结论未回写 owning docs，或当前文档难以指导实现。
- 用户要求“查阅资料”“借鉴先进理念”“得出最优解”“设计重构提案”。

只做代码局部重构、格式修正文档或实现单个已明确方案时，不需要使用；这些更适合代码重构架构师或普通实现流程。

## 资料读取顺序

1. 读取相关 instruction，确认全局开发、文档和 Ousia kernel 边界。
2. 找 owning docs：优先 `design/core/**`、`design/topics/**`、`design/implementation/**`、`design/target.md` 和相关 README。
3. 读取 `design/notes/reference/**` 和 `design/notes/analysis/**`，但只把它们当参考材料，不当稳定规范。
4. 查找当前代码中已经形成的 executable contract，例如 `kernel/src/**`、`ostd/src/**` 和 `tools/qemu-runner/**`。
5. 必要时查询外部资料或成熟实现。外部参考必须标明适用边界、不能直接照搬的原因、license/维护风险和 Ousia 的采用策略。

如果资料不足，先输出探索结论和待确认问题，不要把猜测写成稳定设计。

## 设计原则

设计提案必须遵守：

- seL4-like baseline first：Capability、CSpace/CNode、Untyped/retype、Endpoint、Notification、TCB、IPC、syscall/invocation 和 scheduling 语义先对齐 seL4，再评估 Ousia-specific 修改。
- Rust 风味只用于更清楚地表达类型、不变量和错误，不为了语言风格改变 baseline 语义。
- `kernel` 只表达架构无关内核语义；OSTD 拥有 boot、架构差异、MMIO、exception、early serial、heap/frame/page-table 等底层能力。
- Ousia 是 multi-core-only kernel。scheduler、per-CPU state、IRQ/timer routing、TLB shootdown、FPU/SIMD ownership、锁和 allocator 边界不能以单核为长期不变量。
- memory/address-space 设计避免建立两套互相竞争的真相源；page-table structure、typed frame metadata 和 range/cursor guard 应成为权威边界。
- 稳定结论应回写 owning docs，不要长期漂浮在 notes 中。

## 提案结构

每个非平凡设计提案都应包含：

- 背景与约束：当前文档、代码和参考实现分别提供了什么证据。
- 目标与非目标：明确本轮要解决什么，不解决什么。
- 现状判断：哪些结构应继承、哪些应演进、哪些不应继续模仿。
- 候选方案：至少两个方案；必要时包括现有模式、成熟实现参考、自定义实现和暂缓决策。
- 推荐方案：说明取舍理由、失败模式和为什么它最适合 Ousia 当前阶段。
- 模块边界与依赖方向：谁依赖谁，哪些 API 是稳定边界，哪些只是内部细节。
- 状态所有权与数据流：谁拥有状态，输入从哪来，输出到哪去，副作用在哪一层发生。
- 校验与归一化：哪个边界建立不变量，错误如何保留上下文。
- 迁移路径：如何从当前设计过渡，如何保持兼容，如何回滚。
- 验证策略：文档检查、代码测试、QEMU smoke、模型测试、review 或外部 reference 对照。
- Review focus：明确希望黑队 review 重点攻击的假设和薄弱点。

## 外部参考规则

引用外部设计时，必须区分三层：

- Reference fact：外部项目或论文实际怎么做。
- Ousia constraint：Ousia 的目标、现有代码、边界和阶段性限制。
- Adoption decision：采用、改造、延后或拒绝的理由。

不要因为 seL4、Asterinas 或 CortenMM 先进就直接照搬。也不要为了 Ousia-specific 叙事过早偏离 seL4 baseline。外部资料的价值在于澄清约束和失败模式，而不是替代本项目的边界判断。

## 文档归属

设计提案应明确最终落点：

- 稳定核心概念：`design/core/**`。
- 工程路线、兼容性、shell/tooling、环境配置等专题：`design/topics/**`。
- 当前可执行实现 baseline：`design/implementation/**`。
- 外部资料摘要：`design/notes/reference/**`。
- 临时分析、候选矩阵、探索记录：`design/notes/analysis/**`。

如果提案改变文档结构、编号、链接或 `target.md` 引用，必须按 documentation standards 和 doc-validation workflow 做对应检查。

## 提案 Review 闭环

设计架构师提案不能直接当最终结论。以下情况必须调用 `.github/skills/red-team-review/SKILL.md` 做只读复查：

- 提案会修改 owning design docs。
- 提案会影响 kernel/OSTD/tooling 边界。
- 提案会改变 seL4 baseline 语义、multi-core 假设、memory model、driver model 或 compatibility 策略。
- 提案准备进入实现。
- 用户要求没有差错、没有语义误差或找盲点。

交给 review 时至少提供：

- 用户目标和设计问题。
- 读取过的 owning docs 和外部参考。
- 候选方案与推荐方案摘要。
- 预期修改的文档或代码区域。
- 已知 assumptions、open questions 和 residual risks。
- 特别关注：参考误读、候选方案不足、目标/非目标不清、owning doc 归属错误、迁移路径缺失、Ousia-specific 语义过早发明。

review 通过后，才能把稳定结论写入 owning docs 或交给实现流程。review 发现问题时，先修提案，不要靠措辞掩盖设计缺口。

## 输出要求

输出应高信号、可执行、可 review。避免只复述现有结构，避免只有单一路径，避免用抽象名词包装架构感却说不清职责和演进路径。

最终回答应说明：

- 推荐方案是什么。
- 为什么不是其他方案。
- 哪些边界和不变量因此更清楚。
- 下一步应改哪些文档或代码。
- 需要哪些验证和 review。
