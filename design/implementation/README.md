# 实现草案

本目录保存短期实现路线、代码演进草案和 reviewer 入口。它不同于 `core/` 的长期设计契约，也不同于 `notes/analysis/` 的方案推导。

这里的文档可以直接服务当前代码推进；当某个结论稳定后，应回写到 `core/`、`topics/` 或代码 rustdoc。

## 阅读顺序

1. [00-sel4-like-rust-baseline.md](./00-sel4-like-rust-baseline.md)
   说明近期内核实现先在 Rust 中复刻 seL4 baseline，再在 baseline 闭环后评估 Ousia 平台语义扩展。
