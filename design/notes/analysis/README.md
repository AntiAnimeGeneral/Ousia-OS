# 设计分析：基于现有技术的 Ousia 方案

本目录保存基于现有技术调研和参考的 Ousia 设计分析文档。它与 `notes/reference/` 不同：`notes/reference/` 描述外部世界怎么做，`notes/analysis/` 解释 Ousia 为什么这样选。

## 阅读顺序

1. [00-fs-vm.md](./00-fs-vm.md)
   保存 FS/VM 候选方案、调研、裁决标准和开放问题。

2. [00-bypass-first-class.md](./00-bypass-first-class.md)
   解释 Ousia 为什么要把 kernel bypass 作为受治理的第一公民数据面模式。

3. [01-modern-driver-patterns.md](./01-modern-driver-patterns.md)
   比较现代驱动架构模式，提炼对 Ousia 的启示和不应照搬点。

4. [02-driver-sdk-draft.md](./02-driver-sdk-draft.md)
   保存 Ousia Driver SDK 的对象模型、分层、运行时和恢复模型草案。

5. [03-subsystem-path-matrix.md](./03-subsystem-path-matrix.md)
   比较 FS/GPU/NIC/NVMe 的 control path、data path 和 bypass 边界，以及默认拥有者和恢复契约。

## 当前最重要的判断

1. **Ousia 的高频数据面应优先由受治理的 bypass substrate 提供，而不是把 bypass 当成后补优化。**
2. **Driver SDK 应该把 queue/buffer/event/fence/doorbell/恢复工具作为核心对象，而不是松散的 syscall 或 ioctl 接口。**
3. **这些分析与草案属于 Ousia 设计讨论，结论稳定后应回写到 `core/` 或 `topics/` 对应 owning 文档。**
