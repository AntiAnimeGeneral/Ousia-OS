# 02 — seL4 内核对象与 IPC 参考

> 状态：参考材料。本文用于理解 seL4 的内核对象、capability 模型、IPC 快慢路径和通知机制，并提炼 Ousia Capability Core / Communication Fabric 的参考约束。规范性设计见 [01-capsule-and-capability.md](../../core/01-capsule-and-capability.md) 与 [02-communication-fabric.md](../../core/02-communication-fabric.md)。

## 为什么要看 seL4

seL4 的价值不只是“一个小内核”。它把下面几件事做成了一个高度统一的机制集合：

- 所有可访问对象都通过 capability 进入系统边界。
- IPC、通知、回复、调度相关状态都围绕线程对象和端点对象组织。
- 快路径把最常见的小消息调用压到极低开销。
- 内核对象语义尽量窄，避免把文件、fd、socket 之类 Unix 历史包袱混进核心。

对 Ousia 来说，这正好对应两条底线：

1. capability 必须是系统真实的权威边界，而不是装饰性句柄。
2. 通信 fabric 必须区分消息、通知、共享内存和完成事件，不能把它们都伪装成同一种 IO。

## 1. capability 先于命名

seL4 的对象通常不是通过全局名字访问，而是通过 capability 访问。capability 不是“权限标记”这么简单，而是一个可传递、可复制、可削权、可撤销的权能句柄。

这一点的设计后果很直接：

- 访问控制不靠中心化 ACL 表，而靠 capability 分发。
- 谁能调用谁、谁能重映射谁、谁能重删谁，都由 capability 关系决定。
- 权限传播是显式的，不是隐式继承。

这和 Ousia 的 Capsule/Capability 方向是同构的。需要保留的不是“名字系统”，而是“谁持有什么能力，能力能向下退化，但不能升级”。

## 2. Endpoint：同步 IPC 的中心对象

seL4 的同步 IPC 不是“向端口发消息”，而是“通过 capability 调用一个 endpoint 对象”。

关键语义：

- `Call` 是同步的，发送方会阻塞直到收到回复。
- `ReplyRecv` 把“回复上一个调用者”和“等待下一个请求”合并成一个操作。
- endpoint 本身是对象，不是全局地址。
- 消息载荷很小，常见路径优先走寄存器/小拷贝快路径。

这说明两件事：

1. 微内核里应该有一个一等请求-响应原语，而不是只剩“字节流”。
2. 回复语义不能总是做成单独 syscall，否则会把最常见路径切碎。

## 3. 快路径与慢路径分离

seL4 的高性能 IPC 并不是“平均更快”，而是“把最常见场景极度优化”。

典型快路径约束包括：

- 消息很小。
- 调用/回复语义明确。
- 队列状态简单。
- 没有复杂 capability 传输。
- 没有更高优先级线程抢占。

这给 Ousia 的启发不是“复制某个阈值”，而是：

- 先定义常见路径，再为它单独做短路径。
- 把复杂语义留给慢路径，不要污染 fast path。
- 让消息大小、权限传输、调度状态这些因素在边界上显式分流。

## 4. Notification：异步信号对象

seL4 的 notification 不是 endpoint 的附属品，而是独立对象。它更像一个带能力保护的异步信号原语。

它适合表达：

- “有事情发生了，但我不需要马上带数据回复。”
- “我只需要唤醒某个等待者。”
- “我想把多个发送者的信号折叠成一个可消费状态。”

badge 机制允许多个发送方在同一个 notification 上复用不同位，接收方可以据此判断来源或类别。

对 Ousia 来说，notification 很适合映射：

- 设备中断。
- 队列可读/可写边沿。
- 完成信号。
- 外部事件唤醒。

但它不应该被滥用成通用消息通道。需要数据的交互，还是应该走 IPC 或共享内存。

## 5. Reply 对象和调用生命周期

seL4 的 reply 语义很重要，因为它让“请求-响应”可以维持一个清晰的生命周期，而不是把上下文散落在多个对象里。

这意味着：

- 服务器可以把调用者挂起，然后在适当时机回复。
- 回复权能不是永久存在的，生命周期可被限制。
- 同步调用可以在不额外暴露全局状态的情况下完成。

这对 Ousia 很有启发：

- 需要一个能把请求上下文显式带入处理链的模型。
- 需要区分“尚未回复”的调用和“已经完成”的任务。
- 需要避免把所有 pending 状态都塞进一个模糊的会话对象里。

## 6. 共享内存不是 IPC

seL4 的 Frame / mapped memory 不是消息系统的一部分，而是独立的共享数据面。

这条分离很关键：

- IPC 负责控制面。
- 共享内存负责数据面。
- capability 负责谁能映射、谁能读写。

Ousia 也应该坚持这条边界。比如：

- 控制消息用 endpoint/operation 完成。
- 大块数据通过 frame 或 object-backed mapping 传递。
- 完成事件通过 notification 或 wait-set 机制投递。

不要让一个通道同时扮演“消息、缓冲、通知、权限转发”四种角色。

## 7. 适合 Ousia 的映射

| seL4 概念 | Ousia 候选物 | 说明 |
| --- | --- | --- |
| Capability | Capability / Capsule 权能 | 权限边界的唯一权威来源 |
| Endpoint | 请求-响应对象 | 同步控制面 IPC |
| Notification | 异步事件对象 | 唤醒、完成、interrupt |
| Frame | Shared memory / mapped region | 数据面，不承担协议语义 |
| Reply | 调用上下文 | 生命周期受控的返回权 |

## 8. 不应照搬的地方

- 不要把 seL4 的对象布局直接当成 Ousia 的对象模型。
- 不要把 endpoint 语义简化成“另一个 message queue”。
- 不要把 notification 当作轻量版 channel。
- 不要让 capability 退化成“拿到句柄就能随便干活”的弱约束。

## 9. 读源码时最值得看的位置

本地参考目录：`third_party/sel4/`

优先看这些文件：

- `src/object/endpoint.c`
- `src/object/notification.c`
- `src/object/reply.c`
- `src/object/tcb.c`
- `src/fastpath/fastpath.c`
- `src/kernel/cspace.c`
- `src/kernel/boot.c`

它们分别对应 IPC、通知、回复、线程状态、快路径、capability 空间和启动初始化。
