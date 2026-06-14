# 02 — Agent Harness Architecture

本文是 agent harness 通用化的 implementation owning doc。它落实 [03-agent-harness-library.md](../proposals/03-agent-harness-library.md) 的第一切片：先把 core primitives、Ousia adapter 和 evidence/project-docs 边界写成可执行映射，不移动现有 prompt 文件，也不创建外部 package。

## 当前目标

第一阶段只证明当前 `.github` prompt 资产可以被解释为一套通用 harness core 加 Ousia project adapter，而不是把 Ousia-specific 结构直接发布成框架 API。

完成条件：

- 当前 prompt 资产能映射到唯一 owner。
- `_shared/reference/**` 与 `design/**` 的职责不再互相偷取。
- `architecture-planner` 和 `black-team-review` 可以只拥有 facade/mode/output 协议，不内置 Ousia 领域事实。
- Ousia-specific facts 由 adapter 提供，稳定项目结论仍回写 owning design docs。

## Harness Core Primitives

| Primitive          | 稳定职责                                  | 不拥有                                      |
| ------------------ | ----------------------------------------- | ------------------------------------------- |
| `InstructionSet`   | 硬规范、scope、apply rules 和读取策略     | Ousia 领域事实、review attack checklist     |
| `SkillFacade`      | 任务入口、输入维度、输出协议和 handoff    | 本地 reference facts、项目设计结论          |
| `Mode`             | 任务形状、required inputs、stop conditions | 完整规范或 owning design                    |
| `WorkflowPolicy`   | plan/review/validation/handoff 触发       | 某个 skill 的完整 prompt 字段               |
| `EvidenceCorpus`   | evidence route、review attacks、planning prompts | 稳定项目结论、长期硬规范               |
| `ProjectDocs`      | 项目 owning docs 的索引和语义归属         | prompt-only checklist、agent 操作协议       |
| `ValidationPolicy` | 按 changed surfaces 选择检查              | checker 内部业务规则实现                    |
| `ExecutionCarrier` | 直接执行或只读 subagent 执行边界          | review/planning 的语义 owner                |

这些 primitive 是未来 harness package 的候选 API，但当前只是文档化边界。只有当 Ousia adapter 和至少一个 fixture 都能消费它们时，才考虑固化成 schema 或 package。

## Ousia Adapter Mapping

| 当前资产                                 | 当前 owner             | Core primitive 投影      | 说明                                                        |
| ---------------------------------------- | ---------------------- | ------------------------ | ----------------------------------------------------------- |
| `.github/instructions/*.instructions.md` | Ousia adapter rules    | `InstructionSet`         | Ousia 项目硬规范、workflow policy 和 prompt meta rules      |
| `.github/skills/*/SKILL.md`              | Facade skill           | `SkillFacade`            | task entry、输入维度、输出协议、handoff                     |
| `.github/skills/_shared/modes/**`        | Shared task shape      | `Mode`                   | required inputs、output focus、stop conditions              |
| `.github/skills/_shared/reference/**`    | Ousia evidence corpus  | `EvidenceCorpus`         | local reference routes、review attacks、planning prompts    |
| `design/core/**`                         | Ousia project docs     | `ProjectDocs`            | 产品和核心设计稳定结论                                      |
| `design/implementation/**`               | Ousia implementation docs | `ProjectDocs`         | 当前实现方向、架构落点、review 入口                         |
| `design/proposals/**`                    | Proposal docs          | `ProjectDocs`            | review 前候选方案、implementation handoff                   |
| `.github/skills/doc-validation/**`       | Validation adapter     | `ValidationPolicy`       | 当前文档校验工具；是否进入 core 仍是 open question          |

如果一个文件无法落入唯一 owner，先拆语义，不新增 wrapper：

- 稳定项目结论进入 `ProjectDocs`。
- 所有 agent 都必须遵守的硬规则进入 `InstructionSet`。
- 查证路线、review attacks 和 planning prompts 进入 `EvidenceCorpus`。
- facade 输入/输出协议进入 `SkillFacade`。
- mode-specific required inputs 和 stop conditions 进入 `Mode`。

## EvidenceCorpus 与 ProjectDocs 边界

`EvidenceCorpus` 是 agent playbook，不是项目设计文档。它可以告诉 agent：

- 做某类 review 或 plan 时该读哪些本地 reference。
- 哪些 drift、边界错位、测试坏味道需要攻击。
- 哪些 residual risk 需要在输出里标出来。

它不能定义 Ousia OS 的稳定产品语义、kernel architecture、implementation route 或长期 workflow rule。

`ProjectDocs` 是项目 owning docs。它可以告诉读者和 agent：

- Ousia OS 当前设计相信什么。
- 为什么选择这条架构方向。
- 下一步实施从哪里接手。
- 哪些结论已经稳定，哪些仍是 proposal。

它不写 agent trigger table、subagent prompt contract、checklist-only attack prompts 或 skill 外部接口。

## 单向回写规则

当 planning 或 review 产生新知识时，按以下规则回写：

1. 如果它是所有任务都应遵守的硬约束，进入 `.github/instructions/**`。
2. 如果它是 Ousia 产品、kernel、implementation 或文档结构的稳定结论，进入 `design/**` owning doc。
3. 如果它只是 agent 查证路线、review attack 或 Ousia-specific 经验，进入 `.github/skills/_shared/reference/**`。
4. 如果它只是某个 facade 的输入、输出或 handoff 协议，进入对应 `SKILL.md`。
5. 如果它只是某个 mode 的 required inputs 或 stop conditions，进入 `_shared/modes/**`。

这条规则是第一切片的核心 invariant：同一语义只能有一个权威位置。

## 第一个实施切片

本切片只做文档级实现：

- 保留现有 `.github` 布局。
- 新增本文作为 implementation owning doc。
- 用 mapping 表明确当前资产 owner。
- 后续再按 mapping review 结果决定是否重命名 `_shared/reference` 或增加 adapter manifest。

本切片不做：

- 外部 package。
- schema 或 parser。
- prompt 文件迁移。
- doc checker 重构。
- kernel/OSTD 改动。

## 后续实现入口

1. Review 本文和 proposal，确认 primitive 名称、owner 和回写规则。
2. 审查 `.github/skills/_shared/reference/**`，找出混入 design conclusion 的内容。
3. 为 Ousia adapter 增加更具体的 project docs index，只列 owning docs，不复制正文。
4. 设计一个最小 adapter manifest 草案，先不要求工具消费。
5. 用 `black-team-review` 审查 harness prompt diff，攻击边界性、正交性、简约性和闭环可执行性。

## Open Questions

- `EvidenceCorpus` 是否应在目录名上从 `reference` 改名，避免继续被误读为 project reference docs。
- `doc-validation` 是否应成为 core validation runner，还是保持为 Ousia adapter tool。
- Adapter manifest 应先用 Markdown 表格，还是直接定义机器可读 schema。
- 第二个 fixture 应该是本仓库内的 synthetic project，还是另一个真实项目。
