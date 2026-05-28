# 参考索引：高性能事件等待模型

本目录只保留外部参考材料，帮助理解现有系统的事件等待语义、模式和边界。

## 阅读顺序

1. [04-epoll-and-kqueue.md](./04-epoll-and-kqueue.md)
   Linux `epoll` 与 BSD/macOS `kqueue` 的事件等待模型，以及它们对 Ousia EventPort / WaitSet 的启示。

本目录只保存 external reference；基于现有技术的 Ousia 设计讨论、SDK 草案、旁路判断和子系统路径矩阵已移到 [analysis/](../analysis/README.md)。

## 语义区分

- `reference/`：外部系统的机制与模式参考。
- `research/`：对已有系统的深入调研与背景分析。
- `analysis/`：基于调研和参考得出的 Ousia 设计判断与方案草案。

## 当前最重要的判断

1. **`epoll` / `kqueue` 证明等待集合需要成为一等内核对象，而不是每次阻塞都重建监听列表。**
2. **Ousia 的原生等待模型应避免把 fd readiness 作为中心抽象。**
3. **事件源应类型化，native 事件应覆盖 Operation completion、FenceReached、MemoryObjectLost、DeviceInterrupt 等。**
