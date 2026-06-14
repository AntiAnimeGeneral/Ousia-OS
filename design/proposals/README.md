# 提案

本目录保存进入实施前的 proposal packet。它介于 `implementation/` 的短期路线草案和具体代码任务之间：每份提案应说明目标、非目标、候选方案、推荐方案、模块边界、状态所有权、迁移步骤、验证命令、review focus 和剩余风险。

提案通过 review 后，实施结论应回写到对应 owning 文档或代码 rustdoc；过期提案不应继续作为规范源。

## 提案列表

1. [00-sel4-baseline-implementation-alignment.md](./00-sel4-baseline-implementation-alignment.md)
   历史提案。它曾建议将现有 `kernel` Rust model 收敛到 seL4 baseline；当前路线已由 [implementation/00-ousia-kernel-architecture.md](../implementation/00-ousia-kernel-architecture.md) 取代，本文只作为历史设计记录和被拒绝方向参考。
2. [01-ousia-native-kernel-refactor.md](./01-ousia-native-kernel-refactor.md)
   当前推倒重来提案。它面向 `kernel/src/**` 和 `kernel/tests/**` 的 Ousia-native kernel skeleton greenfield replacement，不保留旧 seL4 prototype API、测试或文件布局兼容。
3. [02-kernel-memory-and-vm-foundation.md](./02-kernel-memory-and-vm-foundation.md)
   当前内存专项提案。它要求先建立 runtime frame metadata、kernel allocator/reservation 和 minimal VM/MemoryObject/AddressSpace foundation，再继续扩大 object/handle/channel/VFS 主线。
4. [03-agent-harness-library.md](./03-agent-harness-library.md)
   当前 agent harness 通用化提案。它建议把 prompt workflow 提炼为项目无关的 harness core，并把 Ousia-specific instructions、skills、evidence corpus、project docs 和 validation policy 建模为 project adapter。
