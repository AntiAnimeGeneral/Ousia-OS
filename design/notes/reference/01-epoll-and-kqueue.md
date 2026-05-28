# 04 — epoll 与 kqueue 事件等待参考

> 状态：参考材料。本文用于理解 Linux `epoll` 与 BSD/macOS `kqueue` 的事件等待模型，并提炼 Ousia EventPort / WaitSet 的设计约束。规范性设计见 [02-communication-fabric.md](../../core/02-communication-fabric.md)，同步/异步边界见 [00-async-and-mmap.md](../../topics/00-async-and-mmap.md)。

## 讨论范围

`epoll` 和 `kqueue` 都不是完整异步 IO 框架。它们解决的是一个更窄但极关键的问题：一个线程如何高效等待很多对象的状态变化，而不是为每个 fd 或对象分配一个阻塞线程。

本文关注四件事：

- wait set 如何注册、修改和删除等待对象
- ready notification 如何被投递和消费
- level-triggered、edge-triggered、one-shot 的语义边界
- 这些机制对 Ousia EventPort / WaitSet 有哪些启示

## 1. epoll

参考：

- https://man7.org/linux/man-pages/man7/epoll.7.html

### 基本模型

`epoll` 把等待集合做成一个内核对象：

1. `epoll_create1()` 创建 epoll instance。
2. `epoll_ctl()` 把 fd 加入、修改或移出 interest list。
3. `epoll_wait()` 从 ready list 中取出已经就绪的事件。

这比传统 `select()` / `poll()` 的关键进步是：等待集合保存在内核中，不需要每次等待都把完整 fd 列表从用户态拷贝进内核。大规模连接场景下，应用只在注册变化时付出成本，等待路径只处理就绪项。

### 触发模式

| 模式                       | 语义                                 | 风险                                        |
| -------------------------- | ------------------------------------ | ------------------------------------------- |
| Level-triggered            | 只要条件仍为真，每次等待都可再次返回 | 简单可靠，但可能重复唤醒                    |
| Edge-triggered (`EPOLLET`) | 状态从不可用变为可用时通知一次       | 必须 drain 到 `EAGAIN`，否则可能丢进度      |
| One-shot (`EPOLLONESHOT`)  | 返回一次后自动禁用，需要重新 arm     | 可避免多线程并发消费同一 fd，但需要显式重装 |

edge-triggered 的核心约束是：通知表示“状态变化发生过”，不表示“每次读取都有新通知”。使用者必须把 fd 设为 nonblocking，并在收到事件后持续读写直到 `EAGAIN`。否则用户态可能留下可读数据，但由于没有新的边沿变化而不再收到通知。

### 值得吸收的点

- 等待集合本身是一等内核对象，而不是每次调用传入一组对象。
- ready list 与 interest list 分离，注册成本和等待成本分离。
- 事件返回携带用户态 token，允许上层快速定位状态机。
- one-shot/rearm 是多 worker 事件循环的基本工具。
- timeout 与等待天然结合，是事件循环的基础能力。

### 不应照搬的点

- fd 是全局 Unix 对象模型的产物，Ousia 不应把 fd 作为原生等待单位。
- 可读/可写位太偏字节流和 POSIX 文件语义，不能覆盖 Operation completion、TimerExpired、FenceReached、MemoryObjectLost、DeviceInterrupt 等事件来源。
- edge-triggered 语义容易被误用。Ousia 若提供类似模式，应把 drain/rearm 规则写进 SDK 类型和 lint 约束，而不是只靠文档。
- `epoll` 对普通文件、磁盘 IO 和网络 IO 的语义并不统一，不能当作统一异步模型。

## 2. kqueue

参考：

- https://man.freebsd.org/cgi/man.cgi?query=kqueue&sektion=2

### 基本模型

`kqueue` 同样把等待集合做成内核对象，但它的抽象比 `epoll` 更通用。用户通过 `kevent()` 同时提交 changelist 并获取 eventlist。每个 `kevent` 由 `(ident, filter, flags, fflags, data, udata)` 描述：

| 字段              | 作用                                                                      |
| ----------------- | ------------------------------------------------------------------------- |
| `ident`           | 被观察对象，如 fd、进程 ID、信号或 timer 标识                             |
| `filter`          | 事件类型，如 `EVFILT_READ`、`EVFILT_WRITE`、`EVFILT_TIMER`、`EVFILT_PROC` |
| `flags`           | 注册、删除、启用、禁用、one-shot、clear 等控制位                          |
| `fflags` / `data` | filter 特定状态，例如可读字节数或进程退出原因                             |
| `udata`           | 用户态 token                                                              |

与 `epoll` 相比，`kqueue` 的优势在于 filter 模型：等待对象不只限于 fd readiness，也可以表达 timer、signal、process、VFS vnode 变化等事件。

### 触发与状态

`kqueue` 默认更接近 level-triggered：只要条件成立，事件会继续出现。`EV_CLEAR` 提供接近 edge-triggered 的清除语义，返回事件后内核清除当前状态，等待下一次状态变化。`EV_ONESHOT` 则在投递一次后删除事件。

这使 `kqueue` 更像一个通用事件 mux：同一个等待集合可以混合 IO、timer、process lifecycle 和文件系统通知。它仍然不是完整异步执行模型，但比 `epoll` 更接近 Ousia EventPort 的方向。

### 值得吸收的点

- filter 把“等待什么类型的事件”显式建模，比单纯 fd readiness 更通用。
- changelist + eventlist 合并在一次调用里，适合批量注册和批量消费。
- `data` 字段允许事件携带轻量状态，减少后续查询。
- timer、process、signal 等非 IO 事件进入同一等待面，有利于构建统一事件循环。

### 不应照搬的点

- `ident` 仍然绑定传统 Unix 对象，如 fd、pid、signal。
- filter 语义由内核和历史 API 约定扩展，类型边界不够强。
- `udata` 是无类型指针风格 token，适合 C 事件循环，但不适合作为 Ousia SDK 的安全接口。
- VFS vnode filter 属于兼容文件系统世界，不应污染 Ousia 原生 Object Namespace 语义。

## 3. 共同结构

`epoll` 和 `kqueue` 共同证明了一件事：高并发系统需要把“等待多个事件源”做成内核级对象，而不是让每个等待都变成阻塞线程或全量扫描。

它们共同的结构是：

1. 创建一个等待集合对象。
2. 将事件源注册进集合，并附带用户态 token。
3. 线程在集合上等待，内核批量返回 ready/completion 事件。
4. 用户态根据 token 驱动自己的状态机。
5. 通过 clear/edge/one-shot/rearm 控制重复通知和并发消费。

这正对应 Ousia 的 EventPort / WaitSet，但 Ousia 的事件源不应退化为 fd readiness。事件源应覆盖 Operation completion、TimerExpired、DeviceInterrupt、StreamReadable、MemoryObjectLost、QueueReadable、FenceReached、ProcessTerminated 等系统原生对象。

## 4. 对 Ousia 的设计启示

### EventPort / WaitSet 应吸收的能力

- **等待集合对象化**：WaitSet 是可持有、可传递、可审计的 Capability 对象。
- **事件源类型化**：事件源不是 fd，而是 Operation、Stream、Queue、Timer、Fence、DeviceInterrupt、MemoryObject 等能力对象。
- **批量注册与批量返回**：注册变更和事件消费都应支持 batch，避免高频系统调用。
- **用户态 token**：每个注册项可携带小型用户态 token，便于事件循环 O(1) 找回状态机。
- **one-shot/rearm**：多 worker 消费同一事件源时，需要明确 rearm 语义。
- **轻量状态载荷**：事件可携带 source id、sequence、byte count、error code、lost/poisoned 标记等小数据。
- **超时一等化**：等待本身支持 timeout，TimerExpired 也应是普通事件源。

### EventPort / WaitSet 应避免的问题

- 不以 fd 作为原生抽象中心；兼容域可投影 fd，但原生接口使用 Capability。
- 不把 readiness 当作 completion。StreamReadable 表示可尝试读，OperationCompleted 表示请求已经完成，两者不能混淆。
- 不让 edge-triggered 变成默认。默认语义应可靠、容易验证；高性能模式可以显式选择 clear/edge/rearm。
- 不把所有 IO 都伪装成可读/可写。设备队列、fence、pager fault、driver reset 都需要独立事件类型。
- 不依赖无类型指针 token 暴露安全接口。底层 ABI 可保存机器字 token，SDK 应提供类型安全封装。

## 5. 与 Ousia 原语的映射

| 现有机制                          | Ousia 对应物                         | 说明                             |
| --------------------------------- | ------------------------------------ | -------------------------------- |
| epoll instance / kqueue           | WaitSet / EventPort                  | 等待集合对象，受 Capability 管理 |
| fd readiness                      | StreamReadable / StreamWritable      | 仅是事件源之一，不是统一模型     |
| `epoll_ctl` / changelist          | register / modify / unregister batch | 修改等待集合                     |
| `epoll_wait` / `kevent` eventlist | `wait(events, timeout)`              | 批量取回事件                     |
| `EPOLLONESHOT` / `EV_ONESHOT`     | one-shot + explicit rearm            | 防止多 worker 重复消费           |
| `EPOLLET` / `EV_CLEAR`            | clear/edge mode                      | 高性能可选模式，需 SDK 约束      |
| `udata`                           | typed user token                     | ABI 可为机器字，SDK 应类型化     |
| timer filter                      | TimerExpired                         | timer 是普通事件源               |

## 6. 开放问题

1. WaitSet 是否允许跨 Capsule 共享，还是只能通过 EventPort 转发事件？共享可降低开销，但会扩大审计和撤销边界。
2. 默认触发模式应是 level-triggered，还是按事件源选择最安全默认？例如 QueueReadable 可 level，FenceReached 更像 one-shot completion。
3. 用户态 token 是否只允许整数，还是允许注册 SDK 管理的 typed handle？底层 ABI 与语言绑定之间需要明确边界。
4. 事件丢失如何表达？对队列溢出、设备 reset、MemoryObjectLost，应使用独立 poison/lost 事件，而不是静默丢通知。

## 相关文件

- [02-communication-fabric.md](../../core/02-communication-fabric.md)
- [00-async-and-mmap.md](../../topics/00-async-and-mmap.md)
- [00-bypass-first-class.md](../analysis/00-bypass-first-class.md)
- [03-subsystem-path-matrix.md](../analysis/03-subsystem-path-matrix.md)
