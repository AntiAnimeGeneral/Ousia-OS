# Shared Index

这个文件只做入口 skill 的路由表。规范内容由 `.github/instructions/**` 提供；本 index 不复述产品设计规范、代码规范、kernel 约束或文档规范。

Ousia-specific 经验、reference 读取入口和 checklist 放在 `.github/skills/_shared/reference/**`。入口 skill 先读取 `.github/skills/_shared/reference/ousia-os.md` 索引，再按 scope 选择少量正文；不要直接全量加载 reference corpus。

## Architecture Planner

入口：`.github/skills/architecture-planner/SKILL.md`

调用维度：

- `mode`：`重构` 或 `新模块`。
- `target`：`产品层` 或 `代码`。

路由：

- Mode：读取 `.github/skills/_shared/modes/planning/refactor.md` 或 `.github/skills/_shared/modes/planning/new-module.md`。
- Target：由入口 skill 根据 `target` 和相关 instructions 投影；不单独拆组件。
- 输出协议由 `architecture-planner` skill 自己声明。

说明：

- `产品层` 指设计文档、理念、目标/非目标、能力归属、owning docs 和采用理由。
- `代码` 指具体实现结构、模块/API、依赖方向、状态所有权、错误边界、测试切入点和最佳实践。
- 当 `mode` 或 `target` 不明确时，先按用户目标推断；推断不可靠时只问这一处，不展开更多选项。

## Black-Team Review

入口：`.github/skills/black-team-review/SKILL.md`

调用维度：

- `subject`：`设计提案` 或 `代码实现`。
- `mode`：`diff` 或 `全局启发扫描`。

路由：

- Mode：读取 `.github/skills/_shared/modes/review/diff.md` 或 `.github/skills/_shared/modes/review/heuristic-scan.md`。
- Subject：由入口 skill 根据 `subject` 和相关 instructions 投影；不单独拆组件。
- 输出协议由 `black-team-review` skill 自己声明。
- Subagent prompt 和 handoff 由 `.github/instructions/ousia-workflow.instructions.md` 约束。

说明：

- `设计提案 + diff`：审刚写出的 proposal/doc diff 是否能进入实施。
- `设计提案 + 全局启发扫描`：扫描设计文档、概念区域或 proposal set 的漂移、空洞、冲突和归属问题。
- `代码实现 + diff`：审真实实现 diff、测试和验证结果。
- `代码实现 + 全局启发扫描`：扫描子系统、测试树或代码区域的长期边界问题。

## Shared Rules

- Shared 组件只描述任务形状，不写具体产品/代码规范或输出协议。
- `.github/skills/_shared/reference/ousia-os.md` 是项目专用 reference corpus 索引，不是入口界面、规范源或 trigger table。
- `.github/skills/_shared/reference/*.md` 正文用于快速变动的工程经验和 checklist；硬规范仍应进入 `.github/instructions/**`。
- 不要新增过多 mode、target 或 subject；优先把差异交给 instructions 和当前任务 scope。
- Shared assets 不是外部入口，不应被当作 subagent skill 直接调用。
