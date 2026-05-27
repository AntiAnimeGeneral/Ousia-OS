# 现代软件栈核心痛点

> 展开 [target.md](./target.md) 中的痛点枚举。

## 讨论范围

逐条展开 target.md 中识别的七大痛点，补充具体案例、量化影响，并讨论这些痛点如何直接塑造 Ousia OS 的设计选择。

---

## 1.1 依赖、安装与分发失控

### 现状问题

现代软件交付链条是三套系统拼凑的结果：

```
系统包管理器 (apt/rpm)    语言包管理器 (pip/npm/cargo)    容器 (docker/podman)
       |                           |                            |
   OS 级别的共享库          项目级别的依赖                全量环境快照
       |                           |                            |
       +------ 用户承担三者之间的协调成本 ------+
```

每个层各自维护依赖图，互不可见。结果：

- `apt install python3-numpy` 装的版本和 `pip install numpy` 可能冲突
- Docker 镜像里 `apt-get update && apt-get install` 是常态——这不是声明式，这是脚本式
- 一个上游小版本变更可能通过多层依赖传播，最终打破完全不相关的应用

### 钻石依赖

这是依赖管理中最经典的难题：

```
     App
    /   \
  LibA  LibB
    \   /
     LibC (v1 vs v2)
```

现有系统的四种解法，各有缺陷：

| 方案         | 代表               | 缺陷                                         |
| ------------ | ------------------ | -------------------------------------------- |
| 全局单版本   | pip (旧版), apt    | 不兼容版本无法共存                           |
| 嵌套复制     | npm (node_modules) | 重复安装，运行时可能有两个 LibC 实例冲突     |
| 全量容器化   | Docker             | 回避问题，不解决                             |
| 复杂约束求解 | opam 等            | 约束复杂时求解成本高，可能无解或需要人工仲裁 |

Ousia OS 的答案：多版本并存 + 每个 Package Cell 看到自己的声明版本 + Service Graph 版本协商处理跨 Capsule 通信。

### 安装污染

手工修改 `.bashrc`, `.profile`, `PATH`, `LD_LIBRARY_PATH` 是常态。这些文件的累积效应是：

- 加载的共享库取决于 shell 初始化顺序
- 两个终端窗口可能有不同的环境
- 卸载软件后残留配置影响其他程序
- 难以审计"当前环境是从哪里来的"

Ousia OS 禁止 Package Cell 依赖这些隐式状态。运行环境由声明生成，每次启动都从声明重新构建。

---

## 1.2 Unix 默认权限模型过于宽松

### 量化问题

一个典型的 Linux 桌面进程，默认状态下可以：

- 读取 ~10 万个文件（整个 `/usr`, `/etc`, `/proc`, `/sys`）
- 发起任意 TCP/UDP 连接
- 读取其他进程的 `/proc/[pid]/` 信息
- 枚举所有用户和组
- 写入 `/tmp` 和 `~/`

没有任何现代应用需要所有这些权限。

### 为什么沙盒不能只是附加层

Linux 的解决方案是 namespaces + seccomp + capabilities(7) + SELinux/AppArmor——四套机制叠加。结果：

- Flatpak/Snap 的沙盒配置是手写 JSON/YAML，经常过宽
- Docker 默认以 root 运行（需要 rootless 模式额外配置）
- 沙盒是 opt-in，默认是不安全的

Ousia OS 反转这个默认：默认无权限，显式授予。

---

## 1.3 同步阻塞与调度粗糙

### 前台失活的工程表现

当你编译大型 C++ 项目时（`make -j$(nproc)`），以下可能同时发生：

- 鼠标光标卡顿
- 音频出现爆音（buffer underrun）
- 视频播放掉帧
- 终端输入延迟

根本原因不是 CPU 不够快，而是调度器把所有 `-j$(nproc)` 个编译进程和 GUI 合成器、音频服务、输入处理放在同一个优先级队列中"公平竞争"。

### 为什么异步只是用户态框架包装不够

Linux 的 `epoll` / `io_uring` 是用户态异步的基础，但：

- `open()`, `stat()`, `mkdir()` 等元数据操作本质上是同步的
- 用户态异步框架（如 tokio）能调度的只是自己的任务，无法影响内核的 IO 调度决策
- 一个 `fsync()` 调用可能阻塞整个文件系统数秒，期间所有其他 IO 排队

Ousia OS 的目标：等待、取消、超时、背压和优先级传播是内核与服务的统一治理语义，不是用户态异步框架的补丁；同步和异步接口都应能接入这些语义。

---

## 1.4 文件系统抽象落后

### 路径引用为什么不是好的标识

```
/home/alice/projects/my-app/data/2024/customers.db
```

这个字符串的问题：

- 移动 `my-app/` 到 `~/archive/` → 所有硬编码路径失效
- 两个进程各自 `cd` 到不同目录 → `customers.db` 解析到不同文件
- 没有版本语义：今天是这个文件，明天被覆盖后还是这个路径
- 权限绑定路径，不绑定对象

Ousia OS 的 Object Store：对象有稳定 ID，tree view 是一等命名和导航入口。路径不再独占对象身份，但所有普通文件都应能出现在某个 tree view 中。

---

## 1.5 兼容性与原生设计的互相拖累

### 经典案例：Linux 的 `O_DIRECT` vs 页缓存

`O_DIRECT` 绕过了页缓存，但它是一个补丁式的 flag——它和 `O_SYNC`、`posix_fadvise`、`madvise` 之间的关系复杂且文档不清晰。根本原因：页缓存是为传统文件 IO 设计的，但在高性能场景下变成了瓶颈，而 API 无法干净地表达"我要直接 IO"。

Ousia OS 的方案：直通路径作为一等设计，不靠 flag hack。

---

## 1.6 异构硬件的调度空白

### CPU 大小核如何破坏交互体验

在 Intel P-core/E-core 混合架构上，一个后台编译任务可能被调度到 P-core（因为调度器只看到"有空闲 CPU"），而前台交互被挤到 E-core。Linux 的解决方案（Intel Thread Director + 调度器集成）是事后修补。

Ousia OS 的方案：Compute Domain 声明 + 执行等级语义，调度器原生理解算力类型。

### 电源管理的缺失

Linux 的 cpufreq/governor 和调度器是两个独立子系统。调度器不知道 power state，power governor 不知道任务优先级。结果是：交互任务可能在低频率下运行，后台任务可能在最高频率下运行。

Ousia OS：功耗预算作为执行等级的一等属性。

---

## 1.7 抽象边界的性能代价

### 关键问题：什么时候"用户态更好"是幻觉

Ousia OS 坚持文件系统和驱动在用户态，但这有一个真实的性能风险：

- 高频小块元数据操作（如 `stat` 一个目录下的 10000 个文件）在传统内核中只需要一次系统调用遍历 dcache
- 在用户态文件系统中，每个 `stat` 都可能是一次 IPC 往返

这不是说用户态方案注定更慢，而是说 **Ousia OS 必须提供比传统内核更强的共享缓存和批量操作原语**，否则就会在通用桌面上显得更慢。

Ousia OS 的应对策略在 [07-data-and-filesystem.md](./core/07-data-and-filesystem.md)、[03-pager-and-memory.md](./core/03-pager-and-memory.md) 与 [02-communication-fabric.md](./core/02-communication-fabric.md) 中展开。

---

## 开放问题

1. **钻石依赖的确定性解析算法的具体设计？** 最近版本优先可能导致意外的降级——是否需要 override 机制？
2. **"默认无权限"的粒度如何把握？** 如果每次打开文件都需要用户交互授权，体验会非常差。如何设计合理的默认和批量授权？
3. **用户态文件系统的 IPC 延迟能否在通用桌面上做到 <10µs？** 这需要共享内存 IPC 和内核旁路，复杂度不小。

---

## 相关章节

- [00-philosophy.md](./core/00-philosophy.md) — 设计总纲
- [08-package-cell.md](./core/08-package-cell.md) — 软件单元与依赖管理
- [07-data-and-filesystem.md](./core/07-data-and-filesystem.md) — 数据抽象与文件系统
- [05-compute-and-scheduling.md](./core/05-compute-and-scheduling.md) — 调度与异构硬件
