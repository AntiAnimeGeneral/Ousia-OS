# Ousia OS Reference Index

这个文件是 Ousia reference corpus 的索引。入口 skill 或 workflow 决定何时读取本索引；本索引只帮助 agent 在 Ousia-specific scope 中选择少量正文。

Reference corpus 是快速变动的工程经验、reference 读取入口和 planning/review checklist。全局硬规范仍归 `.github/instructions/**`，不要把 instruction 正文复制到 reference 中。

## Use Rules

- 先读本索引，再按 `target`、`subject`、`scope` 和 `focus` 选择 1 到 3 个正文。
- 不要为了保险全量加载所有正文。正文选择不确定时，优先读最接近 scope 的主题，再把遗漏风险列为 residual risk。
- 正文中的 checklist 是领域投影和经验，不是新的规范源。遇到所有角色都应遵守的硬规则，应上移到 `.github/instructions/**`。
- Plan 或 review 中引用 reference 时，应说明读了哪些正文，以及采用、调整、拒绝或无法证明的理由。

## Topic Routes

| Scope / Focus                                                                                              | Read                          |
| ---------------------------------------------------------------------------------------------------------- | ----------------------------- |
| 产品层、设计文档、owning docs、目标/非目标、稳定结论落点、reference 采用理由                               | `product-and-docs.md`         |
| Phase 1 seL4 baseline、Rust-expression-only、baseline-vs-extension、capability、CSpace、CNode、Untyped、retype、Endpoint、Notification、TCB、syscall/invocation | `kernel-baseline.md`          |
| `kernel`、OSTD、tooling、QEMU runner、host tooling、Cargo target、cfg 边界                                 | `kernel-ostd-tooling.md`      |
| boot memory map、typed frame metadata、page table ownership、address space、VMA/page-table truth source    | `memory-and-address-space.md` |
| IPC、reply handoff、notification、scheduler mutation、capability rights、object type 检查                  | `ipc-capability-scheduler.md` |
| platform bring-up、boot、QEMU machine、device tree、MMIO、exception level、driver framework                | `platform-boot-driver.md`     |
| 测试策略、失败无副作用、黑队输入、真实调用路径、状态不变性                                                 | `testing-review-attacks.md`   |

## Planner Selection

`architecture-planner` 读取本索引后：

- `target: 产品层` 默认优先 `product-and-docs.md`，再按 scope 追加一个领域正文。
- `target: 代码` 默认优先对应代码领域正文；涉及行为风险或测试策略时追加 `testing-review-attacks.md`。
- 如果 plan 比较外部 baseline，应选择包含 reference reading 的正文，并在 plan 中列出本地 reference 文件或符号。

## Review Selection

`black-team-review` 读取本索引后：

- `subject: 设计提案` 默认优先 `product-and-docs.md`，再按 proposal 的领域追加正文。
- `subject: 代码实现` 默认优先真实 diff 涉及的代码领域正文；涉及测试覆盖或失败路径时追加 `testing-review-attacks.md`。
- Blocking finding 必须落到真实 diff、代码、测试、owning docs、workflow 或 instruction 证据上；reference 只能提供攻击方向和领域证据需求。

## Corpus Boundary

Reference corpus 可以快速修改这些内容：

- 领域 review attacks 和 planning prompts。
- 本地 reference 的优先读取入口。
- Ousia-specific 工程经验、坏味道和采用判断。
- 容易复发的 semantic drift 样例。

Reference corpus 不承载这些内容：

- 外部 skill 接口、mode/target/subject 定义或 subagent prompt contract。
- 所有角色都必须遵守的硬规范。
- 完整输出协议、handoff packet 或验证规则。
- 与 Ousia OS 无关的通用工程规范。
