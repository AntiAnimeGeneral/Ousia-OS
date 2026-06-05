# 实现草案

本目录保存短期实现路线、代码演进草案和 reviewer 入口。它不同于 `core/` 的长期设计契约，也不同于 `notes/analysis/` 的方案推导。

这里的文档可以直接服务当前代码推进；当某个结论稳定后，应回写到 `core/`、`topics/` 或代码 rustdoc。

## 阅读顺序

1. [00-ousia-kernel-architecture.md](./00-ousia-kernel-architecture.md)
   说明近期内核实现以 Ousia 原生高级 capability kernel 为主线，参考 Zircon/Fuchsia 的 handle/object/VM/channel/driver framework，并保留 seL4 的能力纪律作为安全约束。
2. [01-test-architecture.md](./01-test-architecture.md)
   定义 host、model/property、QEMU smoke、QEMU serial scenario、bare-metal integration、fuzzing 和 benchmark 的测试层级、工具栈与 crate 边界。
