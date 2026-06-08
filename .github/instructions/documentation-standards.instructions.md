---
applyTo: "design/**/*.md,**/README.md,**/*.md"
description: "文档标准：写作风格、文档归属、设计文档和文档结构 hygiene 规则。"
---

# 文档标准

写作、review、重组或验证 Markdown 文档时使用这些规则。

## 写作标准

- 面向第一次接触项目的人写文档。优先呈现当前架构、设计理由、边界和下一步演进，而不是实现流水账。
- 历史过程应放在迁移指南、变更记录、事故复盘或 review 记录中。设计文档、参考文档和 README 应呈现稳定结论、当前约束和可执行命令。
- 进度、当前状态和 owning design 文档应默认写“现在是什么样、谁拥有状态、下一步从哪里接手”。不要写“刚从哪里迁来、之前怎么组织、这次 agent 做了什么”，除非这些历史事实解释了仍存在的兼容入口、删除条件、风险或迁移步骤。
- Review 文档改动时，若叙述不能帮助下一位接手者理解当前结构、当前约束、剩余风险或可执行下一步，应作为文档噪音提出 finding；不要把它降级成可选 follow-up。
- 描述外部库或参考实现时，重点写职责、适用边界、不适用原因、license/维护风险和本项目采用策略。不要把“用了某个 crate”写成项目成就。
- 只有当“当前”能帮助读者决策时才使用它，例如当前支持的目标架构、runner 覆盖范围、尚未冻结的 ABI。不要用它记录 agent 刚完成的编辑。
- 候选方案笔记可以比较选项和取舍，但稳定结论应回写 owning 文档。不要让新结论长期漂浮在 notes 中，也不要在多个文档重复定义同一个概念。
- 如果用户指出语义偏移、噪音、边界混乱或容易复发的实现方式问题，应把可复用教训记录到 instruction 文件或 owning design 文档中。记录要短、可执行、可验证。
- 设计文档描述临时实现、stub、placeholder、diagnostic scaffolding、固定容量脚手架或 fake/no-op backend 时，必须明确它不是稳定结论，并写出不可依赖语义、最终 owner/状态、退出条件和验证要求。不要把临时实现写成当前架构事实，除非它解释了仍存在的风险和删除条件。

## 设计文档 Hygiene

编辑 `design/**/*.md` 时保持文档结构一致：

- Markdown 链接必须可解析。
- 编号 Markdown 文件在各自目录内必须保持连续编号。
- 编号 Markdown 文件的文件名前缀数字必须与 H1 标题数字一致。
- 不要留下指向已删除或已重编号 Markdown 文件的陈旧引用。
- `target.md §x.y` 引用必须指向 `design/target.md` 中仍然存在的章节。
- 如果文档树结构变化，且现有通用 checker 规则能表达新结构，应更新 `design/check-docs.config.json`。
- 如果文档结构或归属变化，并影响 `design/target.md` 或 `design/topics/06-roadmap.md`，应同步更新它们。
- 常规编辑不需要做深度设计 review。只有用户要求时才做更广的架构 review。

## 校验边界

- doc checker 实现是通用能力。Ousia 专属的文档拓扑和正则数据应放在 `design/check-docs.config.json`，不要写进 `.github/skills/doc-validation/scripts/**/*.ts`。
- 只有新增一类校验逻辑时才修改 doc-checker TypeScript；不要为了编码本仓库当前目录名而修改脚本。
