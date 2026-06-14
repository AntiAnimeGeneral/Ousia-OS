# 03 — Agent Harness Library 提案

> Proposal packet。本文用于 review 一个通用 agent harness library 方向。通过 review 后，稳定结论应回写到未来的 harness owning docs、`.github/instructions/prompt-architecture.instructions.md` 或项目 adapter 文档；本文本身不作为长期规范源。

## 用户目标

用户希望把当前 Ousia OS 仓库中的 agent harness 通用化，作为可发布的库或框架，而不是只为本 OS 项目服务。这个 harness 应支持项目特化扩展，同时保持通用 core 的边界清晰。用户还提出一条更广的工程哲学：开发应用也应像开发库一样，先建立清晰的领域原语、边界和组合接口，再由应用层调用这些原语，而不是让 helper、流程和状态交织成面条代码。

本提案把这个想法收敛为一个可 review 的方向：提取项目无关的 agent harness core，并把 Ousia-specific rules、reference corpus、validation matrix 和 review attacks 作为 project adapter。当前 `.github/skills/_shared/reference/**` 与 `design/**` 的语义不正交，是本提案必须重新设计的问题之一。

## Mode And Target

- Mode：新模块。
- Target：产品层。
- Scope：当前 `.github/instructions/**`、`.github/skills/**`、未来 harness package、project adapter 目录和 design owning docs。
- Non-implementation status：本文只定义架构方向和第一个纵向切片，不直接重写现有 skills、instructions 或工具实现。

## 背景与约束

当前 prompt 体系已经有一些接近通用 harness 的结构：

- `.github/instructions/**` 承载硬规范和 workflow 规则。
- `.github/skills/*/SKILL.md` 承载 facade 入口，例如 architecture planning、black-team review 和 documentation validation。
- `.github/skills/_shared/modes/**` 承载 mode-specific 任务形状。
- `.github/skills/_shared/reference/**` 承载 Ousia-specific reference、review attacks 和 planning prompts。
- `design/**` 承载 Ousia OS 的产品、核心设计、implementation direction 和 proposal packets。

这些结构已经说明 harness 的核心问题不是某个具体 skill，而是如何管理以下事实：

- 哪些规则是所有 agent 都必须遵守的硬规范。
- 哪些入口是 task facade。
- 哪些内容只是 mode shape。
- 哪些内容是项目领域经验或 evidence corpus。
- 什么时候进入 plan、review、implementation、validation 或 handoff。
- 什么时候直接执行，什么时候可以把任务交给只读 subagent。

当前结构的主要问题是边界还没有完全正交：

- `_shared/reference/**` 既像 review/planning 的 evidence corpus，也夹带了某些接近项目设计判断的内容。
- `design/**` 是 Ousia OS owning design，但有时会被 prompt workflow 当作 reference evidence 使用。
- `workflow`、`review skill` 和 `planner skill` 曾经共享 handoff/prompt 细节，容易形成多个权威位置。
- Harness 本身缺少项目无关的 package boundary；Ousia OS 是第一个实例，但还没有被建模为 adapter。

## 目标

1. 定义项目无关的 agent harness core，使其可以作为库或框架发布。
2. 定义 project adapter 机制，让 Ousia OS 这类项目提供自己的 instructions、domain docs、reference evidence、validation policy 和 review attacks。
3. 重新设计 `_shared/reference` 与 `design/**` 的语义边界，让它们正交：一个服务 agent evidence 和 attack prompts，一个服务项目 owning design。
4. 保持现有 `.github` prompt 资产可逐步迁移，不要求一次性重写。
5. 让 harness 本身体现“应用像库一样开发”的原则：先建原语和组合协议，再由具体项目调用。
6. 给出第一个可实施纵向切片，用真实 Ousia prompt 资产验证 core/adapter 分离是否成立。

## 非目标

- 不立即发布 package。
- 不立即把 `.github/skills/**` 全量迁出仓库。
- 不绑定某个模型供应商、编辑器、CLI 或 MCP server。
- 不把 Ousia OS 的 kernel、capability、HMP、VM 或文档规范内置到通用 core。
- 不让 reference corpus 成为隐藏规范源。
- 不为所有未来项目设计完整插件系统；第一版只定义 Ousia adapter 能跑通的最小接口。

## 现有结构判断

### 应继承

- Facade skill 模式：`architecture-planner`、`black-team-review` 和 `doc-validation` 作为任务入口是清晰的。
- Mode shape 与入口 skill 分离：`_shared/modes/**` 只描述任务形状，不承载项目规范。
- Workflow instruction 保留闭环触发、验证选择和 subagent 使用边界。
- Prompt architecture instruction 明确 instructions、skills、shared assets 和 reference corpus 的 owner 边界。

### 应演进

- `_shared/reference/**` 应从“项目设计事实 + review attack 混合区”演进为 project adapter 的 `evidence corpus` 或 `agent playbook`。
- `design/**` 应被明确为 project owning docs。它可以被 harness 读取作为证据，但不能被 `_shared/reference` 复制、改写或替代。
- Review/planning handoff 应只由对应 facade skill 声明，workflow 只保留触发和组合规则。
- Harness core 应有自己的 primitives 和接口文档，而不是依赖 Ousia 文件布局天然存在。

### 应停止模仿

- 把 Ousia-specific prompt 结构直接当成通用框架结构。
- 把 reference corpus 当成轻量 design docs。
- 把 design docs 当成 review attack checklist。
- 为了“通用化”增加空泛 adapter、薄 wrapper 或一层只转发文件路径的私有框架。

## 候选方案

### 方案 A：保持 Ousia-only prompt harness

做法：继续在 `.github/instructions` 和 `.github/skills` 内演进，只在本仓库使用。

优点：成本最低，不需要新 package boundary。

不选择原因：这会让通用 workflow 原语继续和 Ousia-specific 语义纠缠。随着 review、planning、validation、reference corpus 继续增长，未来更难判断哪些规则能复用，哪些只是 Ousia 项目经验。

### 方案 B：直接抽成完整外部框架

做法：一次性创建独立 package，把现有 instructions、skills、shared assets 和 doc checker 都迁出去，再让 Ousia OS 作为配置项目接入。

优点：边界看起来最干净，发布目标明确。

不选择原因：当前还没有稳定的 core API，也没有证明 `_shared/reference` 与 `design/**` 的边界能被 adapter 正确表达。一次性迁移会把历史偶然固化成框架 API。

### 方案 C：先定义 core/adapter proposal，再做 Ousia adapter 纵向切片

做法：先把 harness 原语和 project adapter 边界写成 proposal；第一个实施切片只抽出 review/planning/reference/document owner 的接口模型，用 Ousia OS 当前 prompt 资产验证映射，不迁移全部文件。

优点：可以用真实项目压力测试抽象，同时避免提前发布大框架。Ousia adapter 成为第一个 proving ground。

推荐：采用方案 C。

## 推荐架构

### Harness Core Primitives

| Primitive          | Core 职责                                  | 不应拥有                                      |
| ------------------ | ------------------------------------------ | --------------------------------------------- |
| `InstructionSet`   | 声明硬规范、scope、apply rules 和读取策略  | 项目领域结论、review attack checklist         |
| `SkillFacade`      | 声明任务入口、输入维度、输出协议           | 具体项目 reference facts                      |
| `Mode`             | 声明任务形状、required inputs、stop rules  | 完整规范、项目设计正文                        |
| `WorkflowPolicy`   | 声明 plan/review/validation/handoff 触发点 | 某个 skill 的完整 prompt 字段                 |
| `EvidenceCorpus`   | 声明 agent 可读取的证据索引和 attack hints | owning design 结论、长期项目规范              |
| `ProjectDocs`      | 声明项目 owning docs 的索引和语义归属      | prompt-only checklist 或 agent 操作协议       |
| `ValidationPolicy` | 按 changed surfaces 选择检查               | 具体 checker 的业务规则实现                   |
| `ExecutionCarrier` | 直接执行或 subagent 执行的边界             | review/planning 的语义 owner                  |
| `HandoffPacket`    | 跨阶段传递结构化语义                       | 隐式改变源 task 的目标或扩大 scope            |

Core 的稳定职责是组合机制。它不知道 Ousia kernel、Zircon、seL4、HMP 或 design numbering。

### Project Adapter

Ousia adapter 提供：

- `instructions`：Ousia 的硬规范和 workflow rules。
- `skills`：Ousia 选择启用的 facade、mode 和 output conventions。
- `evidence corpus`：Ousia-specific reference routes、review attacks、local third-party source pointers。
- `project docs index`：`design/core/**`、`design/implementation/**`、`design/proposals/**`、`README.md` 等 owning docs 的路由。
- `validation policy`：Cargo、Deno doc checker、QEMU smoke 和 prompt hygiene 的选择矩阵。
- `adapter vocabulary`：Ousia-specific terms 的归属，例如 capability kernel、OSTD、Package Cell、Service Graph、HMP。

Adapter 不应修改 core primitives 的意义；它只填充项目事实和项目选择。

## `_shared/reference` 与 `design/**` 的正交重设计

这是本 proposal 的关键点。

### 当前语义冲突

`design/**` 是 project owning docs：它说明 Ousia OS 的产品目标、核心设计、implementation direction、proposal packet 和路线。它应回答“项目现在相信什么、为什么、下一步从哪里接手”。

`.github/skills/_shared/reference/**` 更像 agent evidence corpus：它帮助 review/planning 选择要读哪些 reference、用哪些 attack questions、如何避免 semantic drift。它应回答“agent 做这类任务时应该查哪些证据、攻击哪些风险”。

两者现在容易交叉：reference corpus 会写项目设计判断，design docs 也会被当成 prompt checklist。这样会导致同一个结论有两个 owner。

### 推荐边界

- `design/**` owns stable project conclusions。
- `EvidenceCorpus` owns agent evidence routing、review attacks、planning prompts 和 local reference pointers。
- `EvidenceCorpus` 可以引用 `design/**`，但不能复制 design conclusion 作为新规范。
- `design/**` 可以引用外部实现和项目经验，但不写 agent trigger table、subagent prompt contract 或 checklist-only 内容。
- 当 EvidenceCorpus 中的经验变成所有项目协作者必须遵守的硬规则，上移到 instruction。
- 当 EvidenceCorpus 中的经验变成 Ousia 产品或 implementation 稳定结论，回写到 owning design doc。

### 权威数据流

Harness 不应把所有 Markdown 都当成同一类 context。第一版应采用单向权威数据流：

1. `ProjectDocs` 保存项目稳定结论和 owning design。
2. `EvidenceCorpus` 只保存 agent 查证路线、review attacks、planning prompts 和本地 reference pointers。
3. `SkillFacade` 根据任务读取 `ProjectDocs` 与 `EvidenceCorpus`，但不复制其中的长期结论。
4. Review 或 planning 发现新的稳定项目结论时，回写 `ProjectDocs`；发现新的通用硬规则时，上移 `InstructionSet`；发现新的 attack 或 evidence route 时，留在 `EvidenceCorpus`。

这个方向让 `EvidenceCorpus` 成为可变的 agent playbook，而不是第二套项目设计文档。它可以帮助 agent 发现 `design/**` 中的 owning doc，但不能替 owning doc 做决定。

### 建议目录模型

第一版可以保留现有路径，但语义重命名：

```text
.github/skills/_shared/reference/        # adapter evidence corpus, not owning docs
.github/skills/_shared/modes/            # task shapes only
design/                                  # Ousia owning docs
```

未来外部化后可变为：

```text
harness-core/
  primitives/
  workflow/
  facade-contracts/

adapters/ousia/
  instructions/
  skills/
  evidence/
  project-docs-index.md
  validation-policy.md
```

不要在第一版强制改目录名。先用文档和 validation 检查保证职责边界。

## Workflow Model

Harness core 的最小闭环：

1. 用户请求进入 `WorkflowPolicy`。
2. `WorkflowPolicy` 判断是否需要 planning、review、validation 或直接 implementation。
3. `SkillFacade` 读取自己的 mode 和 output protocol。
4. `ProjectAdapter` 提供相关 instructions、project docs、evidence corpus 和 validation policy。
5. 主 agent 直接执行，或把同一上下文交给只读 `ExecutionCarrier`。
6. 产出 proposal、implementation diff、review findings、handoff packet 或 validation report。
7. 稳定结论回写 owning docs；经验或 attacks 留在 evidence corpus；硬规范进入 instructions。

## 第一个可实施纵向切片

### 目标语义

证明 harness core 与 Ousia adapter 可以分开表达，而不破坏当前 review/planning/doc-validation 工作流。

### 跨越 owner

- Core owner：primitive definitions、facade contract、workflow policy vocabulary。
- Adapter owner：Ousia instructions、skills、evidence corpus、project docs index、validation matrix。
- Project docs owner：`design/**` 中的 stable Ousia design conclusions。

### 实施文件

第一切片建议只新增文档和轻量索引，不迁移运行时代码：

- 新增 future harness owning doc 或 package proposal。
- 增加 Ousia adapter mapping 表。
- 为 `_shared/reference/**` 增加 evidence corpus 边界说明。
- 为 `design/proposals/README.md` 或后续 owning doc 说明 proposal 通过后如何回写稳定结论。

### 初始映射模板

第一切片应先产出一张映射表，而不是先移动文件。初始分类可以从下表开始，review 后再修正：

| 当前资产                               | 第一归属              | 说明                                                        |
| -------------------------------------- | --------------------- | ----------------------------------------------------------- |
| `.github/instructions/*.instructions.md` | Project adapter rules | Ousia 项目硬规范和 workflow policy；可抽象的部分再进入 core |
| `.github/skills/*/SKILL.md`            | Skill facade          | task entry、输入维度、输出协议和 handoff                    |
| `.github/skills/_shared/modes/**`      | Mode                  | task shape、required inputs 和 stop conditions              |
| `.github/skills/_shared/reference/**`  | Evidence corpus       | evidence route、review attacks、planning prompts            |
| `design/core/**`                       | Project docs          | Ousia 产品和核心设计稳定结论                                |
| `design/implementation/**`             | Project docs          | Ousia implementation direction 和架构落点                   |
| `design/proposals/**`                  | Proposal docs         | review 前的候选方案和 implementation handoff                |
| `.github/skills/doc-validation/**`     | Validation adapter    | 当前作为 Ousia 文档校验工具；是否进入 core 留待后续判断      |

如果某个文件同时落入两类，第一切片不应创建新抽象掩盖冲突，而应拆出 owner：稳定结论进入 project docs，agent 操作经验进入 evidence corpus，通用接口进入 core/facade。

### 完成条件

- 能用表格明确映射：当前每个 `.github/instructions`、`.github/skills`、`_shared/modes`、`_shared/reference`、`design/**` 文件属于 core、adapter、evidence corpus 还是 project docs。
- 能说明 `_shared/reference` 中某条内容什么时候应上移 instruction，什么时候应回写 design doc。
- `black-team-review` 和 `architecture-planner` 不需要知道 Ousia-specific details 就能描述自己的 subject/mode/outputs。
- Ousia adapter 能提供 Ousia-specific evidence without changing core。

### 排除范围

- 不创建外部 package。
- 不迁移所有 prompt 文件。
- 不把 doc checker 变成 harness core。
- 不修改 kernel、OSTD 或 design core 内容。

## 测试与验证策略

第一阶段主要是文档和 prompt 资产验证：

- 文档 hygiene：`deno task --cwd .github/skills/doc-validation check:docs --config ../../../design/check-docs.config.json`。
- Prompt review：使用 `black-team-review` 审查 proposal diff，重点攻击 core/adapter 边界、reference/design 正交性和是否提前抽象。
- 手工 mapping review：列出当前 prompt 资产归属，检查是否有同一规则两个 owner。

后续如果创建 package，再增加：

- schema tests：adapter manifest、facade contract、mode contract 和 validation policy 可解析。
- golden tests：给定 changed files 和 user goal，能选择预期 skill/mode/evidence docs。
- integration tests：Ousia adapter 作为 fixture 跑通 plan -> review -> validation 的最小闭环。

## 迁移路径

1. Review 本 proposal，先确认 core/adapter/evidence/project-docs 的概念边界。
2. 新增一个 harness owning doc 或 package skeleton，只包含 primitives 和 adapter contract，不迁移现有文件。
3. 为 Ousia adapter 写 mapping 表，把现有 `.github` 与 `design` 资产分类。
4. 修正 `_shared/reference/**` 中混入 design conclusion 的内容：稳定项目结论回写 `design/**`，agent-only attacks 留在 evidence corpus。
5. 把 validation matrix 建模为 adapter policy，但 doc checker implementation 仍留在现有 skill，直到外部 package boundary 成熟。
6. 当两个以上项目或一个独立 fixture 能复用 core primitives 后，再考虑发布。

## 回滚方式

本阶段只新增 proposal 文档。若 review 判定方向不成立，删除本文或标记为 rejected，不影响现有 prompt harness。

后续若进入 implementation，任何目录重组都应保持 Ousia adapter 可直接回退到当前 `.github` 布局；core package 不应成为运行当前仓库 workflow 的单点依赖，直到验证通过。

## Review Focus

- Core primitives 是否真的项目无关，还是把 Ousia prompt 偶然结构包装成框架 API。
- Project adapter 是否有清晰 owner，能否承载 Ousia-specific rules without leaking into core。
- `_shared/reference` 与 `design/**` 的职责是否正交。
- EvidenceCorpus 是否会变成隐藏规范源。
- 第一个纵向切片是否可验证，还是只是在做横向概念拆分。
- 是否过早引入 package、schema、插件系统或私有框架。

## Open Questions

- Harness library 的首个发布形态应是 Markdown convention、JSON/YAML schema、Rust/TypeScript package，还是 editor-agnostic directory spec。
- `doc-validation` 属于 harness core 的 validation runner，还是 Ousia adapter 的项目工具。
- Adapter manifest 是否需要机器可读，还是先用 owning docs 和 tests 约束。
- EvidenceCorpus 是否需要更名，避免继续被误读为 project reference docs。

## Residual Risks

- 过早抽象会把当前 Ousia workflow 的偶然结构固化为外部 API。
- 如果没有第二个项目或 fixture，通用化边界可能缺少反例压力。
- 如果 `_shared/reference` 不改名，只靠文档约束，仍可能继续被写成 design docs。
- 如果 core/adapter 拆分过细，agent 使用成本会增加，反而降低执行可靠性。
