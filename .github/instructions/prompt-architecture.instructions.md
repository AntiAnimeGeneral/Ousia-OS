---
applyTo: ".github/instructions/**/*.instructions.md,.github/skills/**/SKILL.md,.github/skills/_shared/**/*.md"
description: "Prompt 架构规范：边界性、正交可组合性、简约优雅性、流程化闭环和自我迭代。"
---

# Prompt 架构规范

这些规则用于设计、review 和演进本仓库的 instructions、skills、shared assets、reference corpus 和 workflow 文档。

## 核心原则

- 边界优先：每个 prompt 文件应有清晰职责。入口、规范、模式、reference、workflow、validation 不应互相偷职责。
- 正交可组合：外部维度应少而稳定；新差异优先投影到 scope、focus、instructions 或 reference corpus，而不是新增入口或新维度。
- 简约优雅：能用一个稳定 facade 加少量索引解决的问题，不拆成多套角色、透镜、contracts 或中转层。
- 流程化闭环：非平凡设计和实现应能进入 proposal -> review -> implementation -> review 的闭环；每一环有输入、输出、停止条件和下一步。
- 自我迭代：当用户指出语义偏移、边界错位、过度抽象或 prompt 失效时，应把可复用教训固化到合适层，而不是只修当前文本。

## 职责分层

- `.github/instructions/**` 承载硬规范、边界约束、workflow 协议和跨角色必须遵守的规则。
- `.github/skills/**/SKILL.md` 承载可发现入口、外部接口、少量稳定维度、工作流程和输出要求。
- `.github/skills/_shared/modes/**` 承载任务形状、输入/输出重点和 stop conditions。
- `.github/skills/_shared/reference/**` 承载快速变动的项目经验、reference 读取入口、planning prompts 和 review attacks。
- Reference corpus 必须从 `index.md` 进入；正文不写主动触发、外部调用协议、subagent prompt contract、完整输出协议或硬规范。
- Workflow instruction 承载 subagent 启动协议、handoff packet、验证选择和闭环编排；不要把领域 checklist 塞进 workflow。

## 设计检查

修改 prompt 系统前，先问：

- 这是硬规范、入口界面、任务模式、领域经验、验证规则，还是一次性说明？
- 这条规则的 owner 是否唯一；以后改它应该去哪一个文件？
- 新增文件是否真的降低复杂度，还是只是把一个概念拆成更多名字。
- 新维度是否会和已有 `mode`、`target`、`subject`、`scope`、`focus` 重叠。
- Reference 正文是否具体到证据和攻击问题，而不是泛泛复述规范。
- Review 是否能发现本次 prompt 设计的边界错位、过度抽象、语义漂移和验证盲区。

## Prompt Review Attacks

- 被动 reference 是否写了 `When To Read`、trigger table、外部调用接口或 subagent contract。
- Entry skill 是否承载了整份规范、完整 checklist 或大量 Ousia-specific 正文。
- Shared asset 是否只是薄中转层，不能保存独立语义。
- Workflow 是否混入领域 checklist，导致 always-on instruction 过重。
- 同一个输出协议、handoff packet、验证规则或 reference 读取规则是否出现在多个权威位置。
- 新增入口是否只是旧入口的 subject/mode/focus 组合，应该收回 facade。
- Reference checklist 是否没有 Evidence To Seek 或 Residual Risk Triggers，导致只能机械打勾。

## 自我迭代规则

- 用户指出 prompt 体系问题时，先定位失效层：instructions、entry skill、mode、reference、workflow、validation 或一次性任务说明。
- 能通过调整现有层解决时，不新增层。
- 如果问题会反复出现，优先写入 instruction；如果只是 Ousia-specific 经验，写入 reference corpus；如果只是某个入口的输出协议，留在该 skill。
- 每次修改 reference corpus 后，运行文档校验流程；每次修改 entry skill 或 workflow 后，检查 frontmatter、链接和 stale 旧路径。
- 非平凡 prompt 架构修改完成后，使用 `black-team-review` 审查真实 diff，重点攻击边界性、正交性、简约性和闭环可执行性。
