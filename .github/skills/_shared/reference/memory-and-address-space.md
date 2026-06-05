# Memory And Address Space Reference

Memory reference 用于防止 Ousia 的内存路径过早落入 allocator 细节，或形成 VMA 与 page table 两套互相竞争的真相源。

## Scope

使用本正文处理：

- Boot memory map normalization 和 reserved/available range ownership。
- Typed frame metadata、physical frame ownership、page allocator、kernel heap/slab/fixed pool。
- VMO/MemoryObject、VMAR/address space、page-table structure、VMA tree、range/cursor guard。
- Zircon、CortenMM、Asterinas、seL4 或 rust-osdev memory-management reference 的采用判断。

## Planning Prompts

- 当前方案的内存真相源是什么：boot memory map、typed frame metadata、VMO/MemoryObject、address-space owner、page table，还是 VMA tree。
- VMA tree 和 page table 的关系是否明确：谁承载 policy，谁承载 committed hardware mapping。
- Frame metadata 是否能表达 ownership、type、pinning、mapping count、budget/quota、reclaim 或 future revoke 需要的信息。
- Ousia MemoryObject/VMO、Pager、Object Store 和 page cache 是否是一等设计对象，而不是被 seL4 Untyped/retype 模型后置或压扁。
- Page-table ownership 是否有明确 owner；跨 address space 或跨 CPU mutation 如何被串行化或延期设计。
- Early heap 是否只是早期 alloc/smoke-test 设施，没有被误认为最终 kernel heap。
- Multi-core-only 假设是否影响 allocator、TLB shootdown、page table locking 和 per-CPU cache 的第一版边界。

## Review Attacks

- Diff 是否把 linked-list early heap 演进成最终 kernel heap，而没有 typed frame metadata、allocator owner、budget/quota、reclaim 或 page-table ownership。
- Boot memory map 是否被多个模块各自解析或默认补齐，导致 reserved range 语义不一致。
- VMA 和 page table 是否都试图成为 mapping truth source。
- MemoryObject、Frame allocator、Pager、Object Store 或 page cache 是否缺少 owner 和失败前置检查；或者反过来，是否仍被旧 seL4 Untyped/retype baseline 阻止进入 Phase 1 裁决。
- Page table mutation 是否在权限、range、alignment、frame availability 全部检查前提交。
- Mapping failure 后，frame metadata、VMA tree、page table entry、refcount 或 TLB state 是否可能部分改变。
- Single-core assumption 是否隐含在 allocator lock、TLB invalidation、per-CPU state 或 frame ownership 里。
- Zircon/CortenMM/Asterinas/seL4 参考是否只借了术语，没有说明 Ousia 采用、调整或拒绝的边界。

## Evidence To Seek

- Boot memory map parsing/normalization owner。
- Frame metadata、page allocator、kernel heap/slab、VMO/MemoryObject、page-table object 和 address-space owner 的代码或 design docs。
- 本地 Zircon reference 中 VMO/VMAR/address-space 相关路径；本地 seL4 reference 中 Untyped/retype/frame object 的 capability discipline 参考；以及 Ousia 采用、调整或拒绝的理由。
- Mapping/unmapping 测试是否覆盖失败后状态不变性。
- Zircon/Asterinas/CortenMM reference 中对应 memory object、VM area、page table 或 frame allocator 路径。
- Multi-core implications：TLB shootdown、locking、per-CPU allocator/cache 的 deferred decision 或边界说明。

## Residual Risk Triggers

- 找不到 single source of truth for mapping state。
- Early heap API 被 public path 依赖。
- Page-table mutation 和 frame metadata update 没有事务边界。
- Tests 不覆盖 mapping failure after partial validation。
- Memory 方案没有说明 multi-core-only 影响。
