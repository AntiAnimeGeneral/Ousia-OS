# 01 测试架构与执行边界

本文定义 Ousia OS 当前测试架构。它描述每一类测试证明什么、不证明什么、由哪个执行环境承载，以及什么时候才需要拆出新的 crate。

## 目标

- 让每个测试落在明确层级中，并能说明它保护的语义。
- 按执行环境和状态 owner 编排测试，而不是按工具名组织测试。
- 保持 host-side 语义测试轻量可运行，同时为 bare-metal、QEMU smoke、model/property、BDD、fuzzing 和 benchmark 留出边界。
- 避免把生产启动路径、runner 工具和测试 ABI 混在一起。

## 非目标

- 不立即创建完整测试框架 crate。
- 不为尚未稳定的 property、benchmark 或 guest-side reporter 引入依赖。
- 不为尚未出现协作式 acceptance 场景的 BDD 或尚未暴露不可信字节输入的 fuzzing 引入依赖。
- 不把 QEMU smoke 当作 capability、IPC、scheduler 或事务语义测试。
- 不因为测试工具不同就拆 crate；crate 边界必须来自执行环境、依赖方向或语义 owner。

## 测试层级

### Host Unit Tests

Host unit tests 验证单一 owner 内部语义，例如权限判断、状态 enum、endpoint queue、capability lineage 或对象表 lookup。它们可以直接调用模块 API，但不应把 private helper 的机械返回值当作产品语义。

位置：`kernel/src/**`、`ostd/src/**` 的 `#[cfg(test)]` 模块。

标准 runner：`cargo nextest run`。

适用工具：`rstest` 可用于参数化 case 和 fixture 复用，但只有当 case label、fixture 复用或失败定位比手写 `Case` 表更清楚时才使用。

### Host Integration Tests

Host integration tests 验证宿主 Rust test harness 下的跨 owner 协作，例如 `KernelState::execute_invocation`、CSpace/ObjectTable/ThreadTable/Scheduler 事务、失败无副作用和边界错误映射。

位置：`kernel/tests/**`。

标准 runner：`cargo nextest run`。

这些测试不证明 no_std/no_main 环境、QEMU platform 链路或真实硬件行为。

### Model And Property Tests

Model/property tests 验证已经稳定的不变量，例如 capability derivation、IPC、mapping、revoke、scheduler queue 或并发状态转换。它们必须有明确 property、输入生成边界、oracle 和 shrinking 诊断价值。

当前边界：先在 owning module 或 `kernel/tests/**` 中承载；当 generators、model oracle 或 test support 变重，并开始影响普通 host integration tests 时，再拆出独立 model test crate。

准入条件：没有稳定不变量和 oracle 时，不引入随机化测试工具。

### BDD Acceptance Tests

Ousia 当前先采用轻量 BDD：非平凡 Rust 测试通过测试名、case label、`Goal`、`Scope` 和 `Semantics` 描述行为契约。这个形态适合 kernel/OSTD host unit tests 和 host integration tests，因为它保留 Rust 类型、owner 状态和失败无副作用断言，同时让 reviewer 能按行为而不是实现步骤审查测试。

完整 BDD acceptance tests 用可读场景描述跨角色认可的产品行为，例如 future service graph、package cell、shell/tool workflow、用户可见 IPC/service contract 或 host tooling workflow。它们面向 PM、QA 和开发协作阅读，不是 kernel core 状态机测试的默认表达方式。

候选工具：Rust `cucumber`，使用 `.feature` 文件描述 `Given` / `When` / `Then`，Rust step definitions 只承载执行 glue。

当前边界：轻量 BDD contract 已进入 Rust 测试规范；`.feature`/`cucumber` 形态不进入 kernel core 默认测试栈。只有当某个用户可见 workflow 需要非 Rust 读者共同维护验收标准，并且场景语言能稳定映射到 public boundary 时，才建立 BDD test crate 或 host tooling acceptance suite。

准入条件：每个 `.feature` 场景必须绑定稳定产品边界和领域词汇；不得用 BDD 复述 Rust 单元测试、内部 helper 或 seL4 baseline 状态表。

### Fuzz Tests

Fuzz tests 用覆盖率引导的随机输入搜索 panic、OOM、死循环、parser 状态爆炸和未定义边界。它们服务不可信字节输入、协议解析、镜像/manifest/trace 解码、capability descriptor/syscall decode、IPC message decode、loader 或 host tooling parser。

候选工具：`cargo-fuzz` / libFuzzer。fuzz target 应只进入具备不可信输入边界的模块；输入生成和 crash triage 归 fuzz harness 所有。

当前边界：不把 fuzzing 当作普通 semantic test 或 property test 的替代。kernel/OSTD 只有出现稳定字节级 decode boundary 时才引入 fuzz target；host tooling parser 可以更早引入，但必须独立于 bare-metal workspace 主路径。

准入条件：fuzz target 必须说明输入边界、允许 panic 的 internal invariant、不可接受的 crash/OOM/timeout、seed corpus 所在位置、最小复现和 CI 运行策略。没有 crash triage 流程时，不把 fuzzing 写入默认完成检查。

### QEMU Smoke Tests

QEMU smoke tests 验证 boot/platform 链路没有断裂，例如 kernel entry、early heap、serial marker、exception marker、runner 参数和目标平台配置。

位置：host runner 继续放在 `tools/qemu-runner/**`；guest-side marker 可以由 `kernel-bin` 或未来 test image 产生。

这些测试不负责证明 capability、IPC、scheduler、CSpace 或 ObjectTable 深层语义。

### Bare-Metal Integration Tests

Bare-metal integration tests 验证 no_std/no_main 环境下 kernel、OSTD、基础服务和硬件模拟协作。它们需要专门测试镜像或测试 ABI 承载，不应继续把测试语义堆进 `kernel-bin/src/entry.rs` 的生产启动路径。

拆分触发条件：当 guest-side reporter、panic handling、serial protocol、QEMU exit marker 或平台矩阵开始稳定时，创建独立 test image crate 或 test ABI crate。

### Benchmarks

Benchmarks 只验证已经有性能契约的算法或数据结构，例如 scheduler queue、capability lookup 或 revoke traversal。性能契约稳定前，不引入 benchmark crate 或 benchmark 依赖。

## Crate 边界

当前保持轻拆：

- `kernel` 保留 host unit tests 和 host integration tests。
- `kernel/tests/support` 只承载 host integration fixtures，不承载产品逻辑。
- `ostd` 保留本 crate 的 host unit tests。
- `tools/qemu-runner` 继续作为 host 工具，不进入 bare-metal root workspace。
- `kernel-bin` 保持生产启动镜像，不作为长期 guest-side test framework。

未来只在边界压力出现时拆 crate：

- `kernel-model-tests`：property generators、model oracle 或语义 fixtures 开始明显独立时创建。
- `kernel-acceptance-tests` 或 host acceptance crate：BDD 场景需要由 PM/QA/开发共同维护，且绑定到稳定 public workflow 时创建。
- `kernel-fuzz`、`ostd-fuzz` 或 tooling-specific fuzz crate：不可信字节输入和 fuzz triage 流程稳定时创建。
- `kernel-test-abi` 或 `kernel-ktest`：bare-metal reporter、panic/test protocol 和 guest-side test image 稳定时创建。
- `qemu-runner` 扩展 crate：runner 需要平台矩阵、镜像类型和 test protocol 编排时创建或扩展。
- benchmark crate：性能契约稳定并需要独立报告时创建。

## 工具栈准入

- `cargo nextest run` 是 host-side 标准测试 runner。
- `rstest` 用于 host-side 参数化和 fixture 复用。
- Property testing 工具只在 property、输入生成边界、oracle 和 shrinking 价值已经写明后引入。
- BDD 工具只在场景由 PM、QA 和开发共同维护，并且映射到稳定产品 workflow 时引入；不要用 `.feature` 文件包装内部单元测试。
- Fuzzing 工具只在模块处理不可信字节输入或复杂 parser/decoder 时引入；fuzz target 必须有 crash triage、seed corpus 和运行策略。
- Snapshot 工具只用于稳定文本、trace、AST、JSON 或协议格式，不用于替代内核状态语义断言。
- Mock 工具只用于已经存在且语义稳定的 HAL trait 或外部依赖 trait。
- HTTP mock 不属于 kernel core 默认测试工具。
- Benchmark 工具只在性能契约稳定后引入。

## 验证命令矩阵

- 修改 Rust source：先运行 `cargo fmt`，再运行 `cargo fmt --check` 和对应 `cargo check`。
- 修改 host unit tests：运行对应 `cargo nextest run` target。
- 修改 host integration tests：运行对应 integration test target。
- 修改 model/property tests：运行对应 property test target。
- 修改 BDD tests：运行对应 acceptance test target，并确认 `.feature` 场景仍映射到稳定 public workflow。
- 修改 fuzz targets：运行对应 fuzz smoke 或 corpus regression target；长时间 fuzz campaign 不属于每次完成检查。
- 修改 `kernel-bin`、`ostd` boot/platform、linker、QEMU runner、boot marker 或 bare-metal test image：运行 QEMU smoke 或 bare-metal integration 检查。
- 修改本文档或其他 `design/**/*.md`：运行文档 hygiene 检查。

## 演进原则

测试层级可以演进，但不能让测试语义漂移。新增测试前先说明 Goal、Scope 和 Semantics；新增工具前先说明它所在层级、准入条件和不能覆盖的风险；新增 crate 前先说明它隔离的是执行环境、依赖方向还是稳定 test ABI。
