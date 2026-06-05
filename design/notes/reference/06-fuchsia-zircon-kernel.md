# 06 — Fuchsia / Zircon Kernel Reference

> 外部参考笔记。本文只记录 Fuchsia / Zircon 的机制和源码入口，不定义 Ousia 规范。采用、调整或拒绝这些机制的结论应回写到 owning design 文档。

## 参考范围

本地 sparse checkout 位于 `third_party/fuchsia/`，当前主要用于阅读 Zircon kernel、用户态 `zx`/`fdio` 库和驱动框架：

- `third_party/fuchsia/zircon/kernel/object/`：kernel object、dispatcher、handle、rights 和生命周期。
- `third_party/fuchsia/zircon/kernel/vm/`：VMO、VMAR、address space、fault 和 mapping 相关实现。
- `third_party/fuchsia/zircon/system/public/`：syscall-facing 类型、rights、handle 和 ABI 头文件。
- `third_party/fuchsia/zircon/system/ulib/zx/`：C++ `zx::*` handle wrapper，例如 channel、vmo、process、thread、job、event、socket 和 pager。
- `third_party/fuchsia/sdk/lib/fdio/` 与 `third_party/fuchsia/zircon/system/ulib/zxio/`：用户态 fd/io 抽象和 POSIX-like 投影。
- `third_party/fuchsia/src/devices/bin/driver_manager/`、`third_party/fuchsia/src/lib/driver/`、`third_party/fuchsia/zircon/system/ulib/ddk/`：driver manager、driver runtime、DDK 和用户态驱动组织。

## Handle 和 kernel object

Zircon 的用户态 API 以 handle 为中心。用户态持有 handle，内核对象真实状态留在 kernel object / dispatcher 中。Handle 携带 rights；syscall 在对象边界检查 handle 类型和 rights，而不是要求用户态直接管理 CSpace slot 或 Untyped object。

这个模型对 Ousia 的参考价值是 API 形状：普通用户库可以暴露 `channel`、`vmo`、`process`、`thread`、`event`、`socket` 等 typed wrapper，而不是暴露底层 slot plumbing。需要注意的是，Zircon 的 handle table、dispatcher class hierarchy 和 syscall ABI 是 Fuchsia 自己的工程结论，不能直接当成 Ousia 规范。

## Channel、call 和 handle transfer

Zircon Channel 是双端消息队列，消息由 bytes 和 handles 组成。普通 `write` / `read` 是异步队列操作；同步 request/reply 可由 `zx_channel_call` 封装，通常依靠 transaction id 匹配 response。

这和 seL4 endpoint rendezvous + reply authority 不同。Zircon 的模式更适合产品级 RPC、FIDL、driver protocol 和 service protocol；seL4 的模式更适合极小同步 IPC baseline。Ousia Communication Fabric 可以参考 Channel 的 message + handle transfer 和 call wrapper，但仍需保留自己的 Portal、Operation、Continuation、EventPort、SharedQueue 和 late reply 语义。

## VMO、VMAR 和 address space

Zircon 把内存对象抽象为 VMO，并通过 VMAR 管理地址空间区域。VMO 适合表达匿名内存、共享内存、文件映射、pager-backed memory 和 copy-on-write 等高级语义。用户态通过 handle 操作 VMO/VMAR，内核负责映射、fault、rights 和生命周期。

这对 Ousia 的 MemoryObject/Pager 设计有直接参考价值：原生 VM API 可以是高级内存对象，而不是只暴露 frame/page-table 操作。Ousia 仍需自己裁决 Object Store、page cache、remote provider fault、reclaim 和 failure preflight 的 owner。

## Driver framework

Fuchsia 的 driver stack 包含 driver manager、driver index、driver host/runtime、DDK 和 component integration。驱动主逻辑可以运行在用户态 host 中，通过 framework 管理绑定、启动、能力路由、设备节点和协议。

Ousia 可参考这种分层，但不能复制 Fuchsia component framework 的产品假设。Ousia 的 Driver Host、Device Service、IOMMU/DMA 授权、IOQueue/IOBuffer、Doorbell 和 Fence 应服从本项目的 Service Graph、Package Cell 和 capability policy。

## 用户态库人体工程学

`zircon/system/ulib/zx/` 展示了 typed C++ wrapper 如何把 raw handle 包装成 `zx::channel`、`zx::vmo`、`zx::process`、`zx::thread` 等对象。这对 Ousia 用户库有两点启发：

- 原生 API 应让调用者处理 typed handle，而不是裸整数 slot。
- syscall status 和业务返回值应分层：内核返回稳定状态码，业务协议通过消息、result payload 或 typed wrapper 表达。

## 对 Ousia 的使用边界

- 可以参考 Zircon 的高级 handle/object/VM/channel/driver framework 形状。
- 不应直接复制 Fuchsia 的 component framework、ABI、class hierarchy 或 product policy。
- seL4 仍提供 capability safety 参考：不可伪造、不可扩权、硬撤销、失败无部分提交。
- Ousia 的稳定结论必须落到 `target.md`、`topics/06-roadmap.md`、`core/**` 或 `implementation/**`；本文只保存外部事实。
