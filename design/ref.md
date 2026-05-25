# 参考索引：高性能驱动与内核旁路

本文档改为目录分治。目的不是把所有判断塞进一篇长文，而是把 Ousia 当前关于高性能驱动、kernel bypass、SDK 和子系统路径划分的探索拆成可独立 review 的几块。

当前所有内容都应视为**待定稿**。

## 阅读顺序

1. [00-bypass-first-class.md](./ref/00-bypass-first-class.md)
   内核旁路为什么应成为第一公民，以及它和 Portal / Operation / syscall 的边界。

2. [01-modern-driver-patterns.md](./ref/01-modern-driver-patterns.md)
   当前先进实现的架构模式：WDDM、Linux DRM、Fuchsia DFv2、DriverKit、io_uring、AF_XDP、SPDK、Asterinas。

3. [02-driver-sdk-draft.md](./ref/02-driver-sdk-draft.md)
   Ousia Driver SDK 草案：对象模型、分层、运行时、恢复模型、工具链。

4. [03-subsystem-path-matrix.md](./ref/03-subsystem-path-matrix.md)
   FS / GPU / NIC / NVMe 的统一路径矩阵：哪些走 Portal / Operation，哪些走 syscall，哪些走 bypass。

这些 ref 文档是参考材料，不是最终规范。通信原语以 [discussions/17-communication-fabric.md](./discussions/17-communication-fabric.md) 为准；驱动和旁路设计以 [discussions/08-driver-and-kernel.md](./discussions/08-driver-and-kernel.md) 为准。

## 当前最重要的三条判断

1. **内核旁路是第一公民的数据面模式，不是特权逃逸路径。**
2. **Ousia 的高性能驱动框架必须围绕 queue / buffer / event / fence / doorbell / recovery 构建。**
3. **SDK、调试、回放、观测和恢复工具是驱动架构的一部分，不是后补。**
