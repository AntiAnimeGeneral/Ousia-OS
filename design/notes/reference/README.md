# 参考笔记索引

本目录只保留外部参考材料，帮助理解现有系统、外部机制和已有技术模式。

## 阅读顺序

1. [00-ipc-sel4-fuchsia.md](./00-ipc-sel4-fuchsia.md)
   seL4 / Fuchsia IPC 背景、机制和比较材料。

2. [01-epoll-and-kqueue.md](./01-epoll-and-kqueue.md)
   Linux `epoll` 与 BSD/macOS `kqueue` 的事件等待模型，以及它们对 Ousia EventPort / WaitSet 的启示。

3. [02-sel4-kernel-objects-and-ipc.md](./02-sel4-kernel-objects-and-ipc.md)
   seL4 内核对象、capability、Endpoint、Notification、Reply 与 IPC 快路径参考。

4. [03-sel4-mcs-microkit-roadmap.md](./03-sel4-mcs-microkit-roadmap.md)
   seL4 MCS 调度、Microkit 静态系统构建、系统 roadmap 与 Ousia OSDK/Service Graph 启示。

5. [04-sel4-sddf-driver-framework.md](./04-sel4-sddf-driver-framework.md)
   seL4 Device Driver Framework 的用户态驱动、SPSC transport、虚拟化组件和 legacy driver reuse 参考。

6. [05-rust-os-and-sel4-ecosystem.md](./05-rust-os-and-sel4-ecosystem.md)
   Rust OS/no_std/seL4 相关生态中可直接复用、可拆参考和暂不采用的库与工程组件。

本目录只保存 external reference；基于现有技术的 Ousia 设计讨论、SDK 草案、旁路判断和子系统路径矩阵放在 [notes/analysis/](../analysis/README.md)。

## 语义区分

- `notes/reference/`：外部世界怎么做。
- `notes/analysis/`：Ousia 为什么这样选。

## 当前最重要的判断

1. **`epoll` / `kqueue` 证明等待集合需要成为一等内核对象，而不是每次阻塞都重建监听列表。**
2. **Ousia 的原生等待模型应避免把 fd readiness 作为中心抽象。**
3. **事件源应类型化，native 事件应覆盖 Operation completion、FenceReached、MemoryObjectLost、DeviceInterrupt 等。**
4. **seL4 社区的主线不只是 kernel，而是 kernel + Microkit + sDDF + roadmap 共同演进。**
5. **驱动框架应优先采用用户态隔离、共享内存数据面、Notification 同步和显式 virtualiser，而不是复制宏内核驱动模型。**
6. **Rust OS 生态中小型架构 crate 可以直接复用，大型 seL4 userspace/runtime 仓库应先作为本地参考和拆件来源。**
