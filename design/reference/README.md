# 参考索引：高性能驱动与内核旁路

本目录按主题组织 Ousia 关于高性能驱动、kernel bypass、SDK 和子系统路径划分的参考材料，便于独立阅读和 review。

## 阅读顺序

1. [00-bypass-first-class.md](./00-bypass-first-class.md)
   内核旁路为什么应成为第一公民，以及它和 Portal / Operation / syscall 的边界。

2. [01-modern-driver-patterns.md](./01-modern-driver-patterns.md)
   当前先进实现的架构模式：WDDM、Linux DRM、Fuchsia DFv2、DriverKit、io_uring、AF_XDP、SPDK、Asterinas。

3. [02-driver-sdk-draft.md](./02-driver-sdk-draft.md)
   Ousia Driver SDK 轮廓：对象模型、分层、运行时、恢复模型、工具链。

4. [03-subsystem-path-matrix.md](./03-subsystem-path-matrix.md)
   FS / GPU / NIC / NVMe 的统一路径矩阵：哪些走 Portal / Operation，哪些走 syscall，哪些走 bypass。

5. [04-epoll-and-kqueue.md](./04-epoll-and-kqueue.md)
   Linux `epoll` 与 BSD/macOS `kqueue` 的事件等待模型，以及它们对 Ousia EventPort / WaitSet 的启示。

这些 reference 文档提供背景、比较和草图。通信原语由 [core/02-communication-fabric.md](../core/02-communication-fabric.md) 定义；驱动和旁路设计由 [core/04-driver-and-kernel.md](../core/04-driver-and-kernel.md) 定义。

## 当前最重要的三条判断

1. **内核旁路是第一公民的数据面模式，不是特权逃逸路径。**
2. **Ousia 的高性能驱动框架必须围绕 queue / buffer / event / fence / doorbell / recovery 构建。**
3. **SDK、调试、回放、观测和恢复工具是驱动架构的一部分，不是后补。**
