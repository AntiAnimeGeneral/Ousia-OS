# 01 — Linux 兼容

> 承接 [target.md](../target.md) 的兼容性目标、非目标和第一阶段落地顺序。

## 为什么兼容层必须是隔离的，不是嵌入的

历史上"兼容 Unix 的新 OS"走嵌入路线——在原生 API 中直接提供 POSIX 兼容接口（`open()`, `read()`, `fork()` 等）。结果是原生设计被 POSIX 语义拖累，原生抽象被架空，系统退化为"又一个 Unix-like"。

Ousia OS 的第一阶段解决方案：**兼容域是类似 WSL2 的轻量 VM，不是 syscall 翻译层，也不是嵌入到原生 API 的 POSIX 适配层。**

## 架构

```
Ousia OS 原生空间
  ├── 原生 App
  ├── 兼容域网关 (Proxy/Gateway)
  │     ├── 文件:   Linux 路径 ↔ tree view / ObjectHandle
  │     ├── 窗口:   X11/Wayland ↔ 原生窗口协议
  │     ├── 网络:   socket fd ↔ 网络能力句柄
  │     ├── 剪贴板: X11 selection ↔ 原生剪贴板服务
  │     └── 设备:   /dev/dri/renderD128 ↔ 设备能力句柄
  │
Linux 兼容域 (轻量 VM)
  ├── Linux 用户态 (glibc, 动态链接器, ld.so)
  ├── Linux kernel
  └── virtio / paravirt 设备与 Ousia 兼容域网关通信
```

网关不尝试"完美映射"——它提供足够好的转换让大多数 Linux 应用可运行。兼容域运行在独立 VM 中，受资源配额约束，不能绕过能力模型访问原生资源，崩溃不影响原生系统。未来可以研究 syscall 翻译路线，但那是另一条高维护成本路径，不作为第一阶段目标。

## 原则

兼容层向旧生态让步，原生层不向旧抽象让步。Linux 兼容域内部保持传统模型（`ld.so`、`LD_LIBRARY_PATH`、POSIX 路径、uid/gid），但这一切被隔离在 VM 边界内，不污染 Ousia OS 的原生 API、Service Graph、Object Store 和能力模型。

## 开放问题

1. 虚拟化级别的性能损失对图形密集型应用（游戏）是否可行？
2. Wayland/X11 映射到原生窗口系统的复杂度？

## 相关章节

- [00-philosophy.md](../core/00-philosophy.md) — 兼容层不污染原生层
- [06-service-graph.md](../core/06-service-graph.md) — 网关作为特殊服务
- [04-environment-and-config.md](./04-environment-and-config.md) — 兼容域内的传统库环境
