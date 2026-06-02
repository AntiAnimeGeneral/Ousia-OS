# Kernel OSTD Tooling Reference

Kernel/OSTD/tooling reference 用于防止实现边界被平台、QEMU、host tooling 或 LSP 便利性污染。

## Scope

使用本正文处理：

- `kernel` 与 OSTD 的职责归属。
- QEMU runner、host tooling、Cargo target、rust-analyzer target、workspace metadata。
- `cfg(target_arch)`、`cfg(target_os = "none")`、MMIO、boot stack、exception vector、CPU register 相关边界。
- OSTD API 是否足以向 kernel 暴露架构无关能力。

## Planning Prompts

- 目标能力属于架构无关 kernel 语义，还是 OSTD/platform/tooling 的实现细节。
- 如果 kernel 需要平台能力，是否可以通过架构无关 OSTD API 表达，而不是在 kernel 内写 cfg 或 MMIO。
- Host tooling 是否应该独立为脚本/工具项目，而不是改变 bare-metal workspace target 或核心 crate 形状。
- Rust analyzer、Cargo test/doctest/bench 或 target 配置问题是否能通过 workspace/tooling 配置解决。
- OSTD 中的 arch-owned API 是否把 exception、serial、CPU halt、heap、page table、frame allocator、MMIO 等边界收束清楚。

## Review Attacks

- `kernel` 是否出现 `target_arch`、MMIO address、boot stack、exception level、device tree、QEMU machine 或 CPU register 细节。
- `#[cfg(target_os = "none")]` 是否隐藏了 bare-metal core crate 的主路径模块、入口点或核心实现。
- 为了 host tools 或 LSP 便利，是否改变了 kernel/OSTD 的 public boundary 或 Cargo target 语义。
- OSTD 是否把 early heap、serial、boot memory-map normalization 或 page table capability 暴露成过宽 API。
- Tooling 是否反向依赖 kernel 内部结构，而不是通过稳定配置、runner contract 或测试接口协作。
- QEMU runner 是否把偶然可跑参数固化成 kernel 语义。

## Evidence To Seek

- `kernel/**`、`ostd/**`、`tools/qemu-runner/**`、Cargo metadata 和 `.cargo/**` 的 diff。
- OSTD API 边界和调用方。
- QEMU command、machine 参数、target triple、linker script、runner config。
- Asterinas/rust-sel4/seL4 的 boot、machine、exception 或 tooling 参考入口。
- 验证结果是否覆盖 bare-metal target，而不只是 host build。

## Residual Risk Triggers

- Kernel 文件引入 platform/cfg/MMIO/boot 细节。
- Tooling 修改要求核心 crate 改形状才能跑通。
- OSTD API 只是透传具体架构细节，没有保存架构无关语义。
- QEMU 参数或 boot 假设没有 reference 对比。
- 验证只覆盖 host convenience path。
