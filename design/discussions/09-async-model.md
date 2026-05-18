# 09 — 异步模型与 mmap 的张力

> 对应 `target.md` §4.5

## 讨论范围

"异步优先"是 xos 的顶层原则，但 `mmap` 本质上是同步的。本文讨论这两种模型的共存策略、内核中的异步原语设计，以及同步包装层的定位。

---

## 异步优先的具体含义

### 不是"所有调用都返回 Future"

xos 的异步优先不是语法层面的——不是每个 API 都返回 `Future<T>`。它的含义是：

1. **长耗时操作必须可取消**：一个操作如果可能耗时 >1ms，它必须有取消入口
2. **等待不能阻塞调度器**：一个任务等待 IO 时，调度器可以切换到其他任务
3. **组合操作有显式语义**：多个异步操作可以组合（all, any, race, timeout）
4. **背压是系统原语**：不是用户态框架的附加逻辑

### 内核中的等待对象

xos 的内核提供一个统一的等待原语（Wait Object），所有异步操作都基于它：

```
WaitObject {
    wait(conditions: Set<Condition>, timeout: Option<Duration>) → Outcome
    signal(condition: Condition)
    cancel(wait_id: WaitID)
}

Condition:
  - PageReady(mo, offset)
  - MessageArrived(channel)
  - TimerExpired(timer)
  - DeviceInterrupt(irq)
  - StreamReadable(stream)
  - ProcessTerminated(capsule_id)
```

内核中的等待是真正的异步——等待线程被阻塞，但调度器可以选择运行其他线程。

---

## mmap 的同步本质

### mmap 为什么不可能是"异步的"

`mmap` 的核心语义是：**内存访问是透明的**。`*ptr = 42` 是一句 CPU 指令，它不经过任何系统调用层，也不能返回 `Future`。

当这句指令触发缺页时：

1. CPU 产生 page fault exception
2. 内核捕获异常
3. 内核需要填充页表
4. CPU 重新执行 `*ptr = 42`

在步骤 3 完成之前，**访问该页的线程被硬件阻塞**——这是 CPU 的硬性行为，软件无法改变。

### 这意味着什么

`mmap` 在访问未映射页时，其行为与同步阻塞 IO 没有本质区别——线程 stall：

```
时间线:
  T0: 应用访问 mmap 地址 → 缺页
  T1: 内核发送 PageRequest 给 Pager
  T2-T5: Pager 处理（读磁盘/网络/计算）
  T6: Pager 响应
  T7: 内核填充页表
  T8: 应用继续执行

T0 到 T8 之间，应用的线程完全阻塞
```

---

## 共存策略

### 两种 IO 模型的明确分工

```
场景                            推荐模型        原因
─────────────────────────────────────────────────────────
顺序读取大文件                  显式异步 IO     可以流水线化，不阻塞线程
随机读写数据库                  mmap           零拷贝 + 页缓存共享
配置文件、小数据                显式异步 IO     简单直接
图形管线（纹理、缓冲区）         mmap           GPU 可以直接访问 mmap 区域
网络 IO                        显式异步 IO     网络延迟大，异步收益高
IPC（进程间通信）               消息/流        天然异步
日志                           流 (Stream)     异步 + 背压
```

### 使用 mmap 的前提条件

xos 不禁止 mmap，但使用它的 Capsule 必须：

1. **声明执行等级**：使用 mmap 的 Capsule 应该标记为 Interactive 或 Foreground Service。如果一个 BG 任务大量使用 mmap 导致频繁缺页 stall，调度器会降低它的预算。

2. **监控 Pager 响应时间**：内核记录每个 Memory Object 的 Pager 平均响应时间。如果一个 Pager 持续慢响应（>10ms 平均延迟），它被标记为 degraded，持有其映射的 Capsule 收到警告。

3. **使用预取**：对于可预测的访问模式（如顺序扫描），应用应使用 `prefetch` 原语告诉 Pager 提前准备页面。

### 调度器如何保护交互性

即使 BG 任务因为 mmap 缺页而 stall，调度器也不会被其阻塞：

- BG 任务 stall 在缺页上 → 该线程被标记为 `BLOCKED_ON_PAGE`
- 调度器不等待该线程 → 立即切换到其他可运行线程
- BG 任务的缺页请求被 Pager 以低优先级处理（低于 INT/FG 任务的缺页请求）
- 如果 BG 任务的大量缺页导致内存压力，其页面被优先回收

关键：**mmap 只阻塞使用它的 Capsule 的线程，不阻塞整个系统**。

---

## 内核异步原语设计

### 取消模型

取消是异步系统中最难的问题之一。xos 的立场：

1. **取消是协作式的**：内核发送取消信号，被取消的操作需要在下一个安全点检查并响应
2. **强制取消仅用于特定场景**：如 Capsule 被杀死时，所有其未完成的 IO 被强制取消
3. **取消的语义由操作类型决定**：
   - 读操作取消 → 未读取的数据丢失（应用不期望）
   - 写操作取消 → 部分写入的数据可能已持久化也可能未持久化（应用需要处理）
   - mmap 缺页 → 不可取消（因为线程已经 stall 在硬件层）

### 超时

每个异步操作都可以附加超时：

```
let result = stream.read(buffer)
    .timeout(Duration::from_millis(100))
    .await;

match result {
    Ok(data) => { /* 成功 */ }
    Err(Timeout) => { /* 超时，stream 仍处于可用状态 */ }
    Err(Cancelled) => { /* 被取消 */ }
}
```

超时后，操作被取消，资源被释放。

### 背压

xos 的 Stream 原生支持背压：

- 如果消费者读取慢，生产者的 `write()` 操作会被限速
- 背压通过 Stream 的缓冲区水位线传导（high watermark → 降速，low watermark → 恢复）
- 背压可以跨进程传导：如果 A 的 Stream 连接到 B，B 的消费速度影响 A 的生产速度

---

## 同步包装层的定位

目标：提供同步 API 方便使用，但不允许它掩盖异步本质。

### 语法糖，不是实现

```rust
// 同步包装（内部是异步 + 阻塞等待）
let data = fs::read_sync("object://user-data/config")?;

// 等价于
let data = fs::read("object://user-data/config").block_on()?;
```

同步包装在以下场景适用：

- 简单脚本和工具
- 初始化阶段（在 Capsule 启动时，阻塞等待是可接受的）
- 配置读取

在以下场景不应使用：

- 事件循环中的 IO
- GUI 线程（会卡住 UI）
- 高并发服务
- 长耗时操作

### 编译器/工具辅助

理想情况下，xos 的构建工具链可以检测到：

- 在 Interactive 等级的 Capsule 中使用了同步 API → 警告
- 在事件循环线程中调用了 `block_on()` → 错误

---

## 开放问题

1. **mmap 的缺页延迟目标**：对于 Interactive 等级的 Capsule，Pager 应在多少时间内完成缺页响应？（10µs? 100µs? 1ms?）
2. **预取 API 的设计**：是应用告诉系统"我接下来要读这个范围"，还是系统自动检测访问模式？
3. **取消的传播范围**：如果一个操作被取消，它已经产生的副作用（如部分写入）是否需要回滚？由谁负责回滚？
4. **异步 IPC 的零拷贝**：异步消息传递如何做到零拷贝？（发送方和接收方的缓冲区生命周期管理？）

---

## 相关章节

- [06-pager-and-memory.md](./06-pager-and-memory.md) — Pager 超时和崩溃模型
- [07-compute-and-scheduling.md](./07-compute-and-scheduling.md) — 缺页 stall 期间的调度行为
- [00-philosophy.md](./00-philosophy.md) — 异步优先是原则 5
