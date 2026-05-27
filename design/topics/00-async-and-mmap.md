# 00 — 同步、异步与 mmap 的边界

> 补充 [target.md](../target.md) 中同步、异步、等待与 `mmap` 缺页语义之间的边界。

## 本章定位

统一通信基座、Portal / Operation / Continuation / EventPort / SharedQueue 等原语归属 [02-communication-fabric.md](../core/02-communication-fabric.md)。本文聚焦一个边界问题：同步调用、异步 Operation、事件等待和 `mmap` 这种硬件层面的同步缺页模型如何作为一等抽象共存。

## 同步与异步都是一等抽象

Ousia OS 不把所有 API 强行做成 Future，也不把所有请求压回阻塞调用。正确边界是：短控制面 RPC 可以是同步 fast call；可能等待外部事件的工作应能表达为异步 Operation；高吞吐数据面走 SharedQueue / IOQueue；内存映射和缺页保留硬件同步语义。

系统需要统一治理的是等待、取消、超时、背压、优先级传播和观测，而不是统一成某一种编程范式。长耗时操作应尽量提供可取消入口（>1ms 的路径原则上都应说明取消点或不可取消理由）、等待不阻塞调度器（等待线程暂停但调度器可切换）、组合操作有显式语义（all/any/race/timeout）、背压是系统原语而非用户态框架的附加逻辑。

内核提供统一的 EventPort / WaitSet：`wait(events, timeout)` / `signal` / `cancel`。Event 来源包括 Operation completion、TimerExpired、DeviceInterrupt、StreamReadable、MemoryObjectLost、QueueReadable、FenceReached、ProcessTerminated 等。完整事件来源和通信路径见 [02-communication-fabric.md](../core/02-communication-fabric.md)。

## mmap 的同步本质

`*ptr = 42` 是 CPU 指令，不经过系统调用，也不能返回 Future。缺页时线程被硬件阻塞——这是 CPU 的硬性行为，软件无法改变。mmap 在访问未映射页时与同步阻塞 IO 没有本质区别：线程完全 stall，直到对应供页路径完成。纯用户态 FS 方案下，这条路径等待用户态 Pager；纯内核态 FS 方案下，这条路径进入内核 Object Store / page cache。

## 路径选择策略

| 场景           | 推荐模型                         | 原因                                             |
| -------------- | -------------------------------- | ------------------------------------------------ |
| 顺序读大文件   | 显式异步 IO                      | 可流水线，不阻塞线程                             |
| 随机读写数据库 | mmap                             | 零拷贝 + 页缓存共享                              |
| 配置/小数据    | 显式异步 IO                      | 简单直接                                         |
| 图形管线       | mmap                             | GPU 直接访问 mmap 区域                           |
| 网络 IO        | 显式异步 IO                      | 延迟大，异步收益高                               |
| IPC            | Portal / Operation / SharedQueue | 小 RPC、异步请求和高吞吐数据面走不同最低成本路径 |

使用 mmap 的 Capsule 需声明执行等级。调度器在缺页 stall 期间保护其优先级预算。纯用户态 FS 方案下，慢 Pager 被监控（内核记录 Pager 平均响应时间，持续 >10ms 标记为 degraded）并按故障模型终止；纯内核态 FS 方案下，等价监控对象是 Object Store / page cache 的缺页延迟和 writeback 队列。应用可通过 `prefetch` 原语填充页面避免热路径缺页。

**关键保证**：mmap 只阻塞使用它的 Capsule 的线程，不阻塞整个系统。BG 任务因缺页 stall 时，调度器立即切换到其他可运行线程。BG 的缺页请求按执行等级进入用户态 Pager 或内核 Object Store 的低优先级队列。

## 等待治理：取消、超时、背压

**取消**是协作式的——内核发送取消信号，操作在下一个安全点检查并响应。取消的语义由操作类型决定：

- 读操作取消 → 未读取的数据丢失（可接受）
- 写操作取消 → 部分写入可能已持久化也可能未持久化（应用需处理不确定性）
- mmap 缺页 → 不可取消（线程已 stall 在硬件层）

强制取消仅用于 Capsule 被杀死时的资源清理。

**超时**：每个可等待操作可附加 timeout。超时后操作被取消、资源释放、Stream 仍处于可用状态。同步 fast call 如果可能长时间等待，应显式降级为可等待 Operation 或声明不可取消边界。

**背压**：Stream 通过缓冲区水位线传导——消费者慢 → 生产者 `write()` 限速。跨进程传导：A 的 Stream 连到 B，B 的消费速度影响 A 的生产速度。

## 同步 API 的位置

同步 API 不是低等包装。短 RPC、初始化阶段、脚本式工具和明确可接受阻塞的后台任务都可以使用同步接口。限制不在于“同步是否被允许”，而在于它是否掩盖了长时间等待、不可取消副作用或交互线程上的调度风险。理想情况下，构建工具链可静态检测：

- Interactive 等级的 Capsule 中使用了同步 IO → 编译警告
- 事件循环线程中调用 `block_on()` → 编译错误

## 开放问题

1. 取消的传播范围：已产生的副作用（部分写入）是否需要回滚？由谁负责？
2. mmap dirty page 与 Operation cancellation 的边界：当异步事务取消时，已产生的脏页在纯用户态 FS 方案下由 Pager 丢弃、保留还是转为显式回滚？在纯内核态 FS 方案下由 Object Store 事务层如何表达？

## 相关章节

- [03-pager-and-memory.md](../core/03-pager-and-memory.md) — Pager 超时和崩溃模型
- [05-compute-and-scheduling.md](../core/05-compute-and-scheduling.md) — 缺页 stall 时的调度行为
- [02-communication-fabric.md](../core/02-communication-fabric.md) — 统一通信基座
