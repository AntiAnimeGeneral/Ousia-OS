# Product And Docs Reference

产品层 reference 用于把 Ousia OS 的稳定设计语义落到 owning docs，而不是让结论停留在 notes、review 记录或一次性 proposal 中。

## Scope

使用本正文处理：

- 产品层目标、非目标、能力归属和设计边界。
- `design/core/**`、`design/topics/**`、`design/target.md`、`design/implementation/**` 的 stable conclusion placement。
- reference fact、Ousia constraint 和 adoption decision 的分离。
- proposal 是否足以指导后续 implementation。

## Planning Prompts

- 用户目标是否能落成一个稳定设计问题，而不是只描述“要重构/要实现/要补文档”。
- 目标和非目标是否约束了后续实现者不能偷换的语义。
- 这个结论的 owning doc 是哪里；为什么不是 notes、reference 或 workflow instruction。
- 外部参考事实、Ousia 约束和采用理由是否分别写清，没有混成“参考实现这么做所以我们也这么做”。
- 当前文档中哪些模式是稳定约束，哪些只是历史偶然或早期草案。
- proposal 是否给出至少两个真实候选方向，包括保守演进和不改动的理由。

## Review Attacks

- Proposal 是否把用户的产品语义换成了实现便利性，例如把 capability 语义问题写成 crate/API 选择问题。
- 文档落点是否过窄：稳定结论留在 notes/reference，导致后续实现者找不到权威位置。
- 文档落点是否过宽：同一概念在多个 design 文件重复定义，长期会产生互相竞争的真相源。
- 参考事实是否被直接当成 Ousia 规范，缺少本项目约束和拒绝/调整理由。
- 设计是否只有理念和口号，没有能力归属、状态所有权、失败语义或验证路径。
- Roadmap、target、core docs 和 implementation docs 是否出现同一能力的不同语义。

## Evidence To Seek

- 用户原始目标和不希望偏移的语义。
- 目标区域的 owning docs、相邻 design docs、roadmap 或 target 引用。
- 与 proposal 结论相关的 reference notes 或 external baseline 摘要。
- 设计结论会驱动的代码模块、测试树或 workflow 区域。
- 已知 assumptions、open questions 和 residual risks。

## Residual Risk Triggers

- 找不到 stable owning doc。
- 结论只存在于 proposal 或 notes 中。
- Reference fact、Ousia constraint 和 adoption decision 没有分开。
- Proposal 只有单一路径，没有比较替代方案或不改动方案。
- 设计无法指导实现者判断边界、状态所有权或验证策略。
