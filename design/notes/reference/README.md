# 参考笔记索引

本目录只保留外部参考材料，帮助理解现有系统、外部机制和已有技术模式。

## 阅读顺序

1. [00-ipc-sel4-fuchsia.md](./00-ipc-sel4-fuchsia.md)
   seL4 / Fuchsia IPC 背景、机制和比较材料。

2. [01-epoll-and-kqueue.md](./01-epoll-and-kqueue.md)
   Linux `epoll` 与 BSD/macOS `kqueue` 的事件等待模型，以及它们对 Ousia EventPort / WaitSet 的启示。

本目录只保存 external reference；基于现有技术的 Ousia 设计讨论、SDK 草案、旁路判断和子系统路径矩阵放在 [notes/analysis/](../analysis/README.md)。

## 语义区分

- `notes/reference/`：外部世界怎么做。
- `notes/analysis/`：Ousia 为什么这样选。

## 当前最重要的判断

1. **`epoll` / `kqueue` 证明等待集合需要成为一等内核对象，而不是每次阻塞都重建监听列表。**
2. **Ousia 的原生等待模型应避免把 fd readiness 作为中心抽象。**
3. **事件源应类型化，native 事件应覆盖 Operation completion、FenceReached、MemoryObjectLost、DeviceInterrupt 等。**
