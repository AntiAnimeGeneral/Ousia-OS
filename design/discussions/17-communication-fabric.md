# 17 — Communication Fabric

> 对应 `target.md` §4.3 + §4.5
>
> 相关备忘录：[../memorandum/00-ipc-sel4-fuchsia.md](../memorandum/00-ipc-sel4-fuchsia.md)

## 讨论范围

本文定义 Ousia OS 的统一通信基座。这里的目标不是在“同步 IPC”和“异步 IPC”之间二选一，而是提供一组正交、统一、强大的系统原语，让控制面 RPC、异步请求、流式通信、内核旁路数据面和设备队列都落在同一套能力、等待、取消、背压、调度和观测语义上。

Ousia 不追求为了“小内核”而把关键语义推给每个用户态 runtime 自行实现。相反，系统应把通信生命周期中会影响性能、安全、调度、公平性和生态一致性的部分做成统一 OS 原语；协议内容、IDL、服务框架和语言绑定留给用户态。

---

## 1. 设计目标

Ousia 的通信系统必须同时满足五个目标：

1. **性能不妥协**：小 RPC 不能被队列化拖慢；高吞吐数据面不能每条消息 syscall；大数据不能塞进 IPC 消息拷贝。
2. **语义统一**：同步、异步、流、队列、设备完成事件都使用统一的等待、取消、超时、背压和观测模型。
3. **能力安全**：服务入口、回复权、共享队列、内存对象和设备队列都必须是可授权、可转交、可撤销或可失效的 Capability 对象。
4. **生态一致**：异步请求的 completion routing、late reply、取消、超时、pending quota、priority/deadline propagation 不应由每个框架各自发明。
5. **旁路一等公民**：高频数据面使用共享队列、registered memory、doorbell、event、fence，而不是退化为“每次请求一次 IPC”。

---

## 2. 核心原语族

Ousia 的通信基座暂称 **Communication Fabric**。这是 Ousia 文档中的设计术语，不指代现有某个具体技术或产品。

### 2.1 Portal

Portal 是服务入口能力。持有 Portal Capability 的 Capsule 才能向对应服务提交请求。

Portal 负责：

- 服务调用入口
- 能力检查
- 小消息 fast path
- 调用方执行等级、deadline、budget 的传播入口
- 服务崩溃时的 `SERVICE_LOST` 传播

Portal 不等于 Fuchsia Channel，也不等于 seL4 Endpoint。它是 Ousia 的服务入口抽象：可以支持同步 fast call，也可以支持异步 Operation submit。

### 2.2 Operation

Operation 表示一次请求的系统级生命周期对象。

它绑定：

- 目标 Portal
- 调用方 Capsule
- inline message
- transferred capabilities
- deadline / timeout
- cancel token
- priority / execution context
- completion target
- audit tag / trace id

Operation 是 Ousia 对“异步请求”与“同步调用”的统一抽象。同步调用是一个在 fast path 上立即完成的 Operation；异步请求是一个提交后稍后完成的 Operation。

### 2.3 Continuation

Continuation 是一次受限回复权。它回答一个问题：谁有权完成这个 Operation？

Continuation 必须支持：

- 一次性回复（reply-once）
- deadline 后自动失效
- cancel 后失效
- late reply 返回明确错误
- pending quota 计数
- 审计和观测
- 可选的 priority / deadline donation

Continuation 不是用户态约定的 reply endpoint，也不是业务协议里的 txid。txid 可以由用户态 runtime 使用；Continuation 是内核可见的回复权和生命周期控制对象。

### 2.4 EventPort / WaitSet

EventPort / WaitSet 是统一等待聚合器。

它应能等待：

- Operation completion
- Portal readable / service lost
- Timer expired
- cancel signal
- MemoryObject lost
- SharedQueue readable / writable
- IOQueue completion
- Device lost / queue revoked
- Fence / Timeline point reached

这不是通用内核消息队列，而是事件聚合与唤醒机制。它让用户态 runtime 能用一个统一入口驱动 async/await、服务框架、设备事件和旁路队列。

### 2.5 SharedQueue

SharedQueue 是受 Capability 授权的共享内存队列，用于高吞吐消息或数据面描述符传递。

它应支持：

- bounded ring
- producer / consumer 所有权
- backpressure
- batch submit / batch complete
- doorbell 或 event 通知
- queue revoke / poison
- metrics / tracing
- 与 IOBuffer / MemoryObject / Fence 组合

SharedQueue 是普通服务之间的 kernel bypass 通信 substrate，也是驱动 IOQueue 的上层统一形式。区别在于：驱动 IOQueue 的消费者可能是设备，普通 SharedQueue 的消费者是另一个 Capsule。

### 2.6 MemoryObject / IOBuffer

大数据不应内联进 Operation 消息体。Operation 只传递 MemoryObject、IOBuffer 或切片 Capability。

- MemoryObject 面向 VM 映射、缺页、共享、CoW 和回写。
- IOBuffer 面向 registered memory、pin 生命周期、DMA 可达性和设备授权。

两者可以共享页框和映射元数据，但语义不能混同。

### 2.7 Fence / Timeline

Fence / Timeline 是跨队列、跨设备、跨 Capsule 的同步对象。

它们用于表达：

- queue A 的 batch 完成后，queue B 才能消费
- GPU / NIC / NVMe / 用户态服务之间的依赖
- async runtime 等待某个完成点
- device lost 或 queue poison 的传播

Fence / Timeline 不应是 GPU 私有机制，而应属于 Communication Fabric 的同步对象家族。

---

## 3. 三条通信路径

Communication Fabric 的核心是让不同负载走最低成本路径。

### 3.1 小控制消息：Portal Fast Call

适用场景：

- 服务发现后的短 RPC
- 纯内存查询
- 权限检查
- 小状态读取
- 同步控制面操作

目标：

- 不进内核消息队列
- 不分配内核缓冲区
- 不经过 EventPort
- 尽量同核直接交接
- 小消息和少量 Capability 走 fast path

示意：

```text
client -> portal.call(op)
server ready -> direct handoff
server reply -> client resumes
```

这一路径吸收 seL4 同步 Endpoint 的关键经验，但不把整个系统限制在同步编程模型里。

### 3.2 异步请求：Operation + Continuation + EventPort

适用场景：

- 文件 IO
- 网络请求
- 设备控制操作
- 可能等待外部事件的服务调用
- 调用方不能阻塞线程的场景

示意：

```text
client runtime:
  op = create Operation(portal, msg, caps, deadline, completion_port)
  submit(op)
  return Future(op.id)

server:
  recv(op)
  if fast:
      complete(op.continuation, result)
  else:
      save op.continuation
      start async work
      later complete(op.continuation, result)

client runtime:
  EventPort wakes
  completion = read_completion()
  wake Future(completion.operation_id)
```

内核知道 Operation / Continuation 的生命周期，但不理解业务方法、IDL schema 或用户态 Future 表。这样既避免每个 runtime 自行发明 reply endpoint/txid/cancel 规则，又不把协议格式锁进内核 ABI。

### 3.3 高频数据面：SharedQueue / IOQueue Bypass

适用场景：

- 网络包收发
- NVMe 提交/完成队列
- GPU command queue
- 日志批量传输
- 高吞吐服务间管道
- 兼容域网关的数据面

目标：

- 不逐消息 syscall
- 不逐消息 IPC
- 不把大数据放进内核队列
- 通过 bounded ring 表达背压
- 通过 Event / Doorbell / Fence 表达通知与完成
- 通过 IOBuffer / MemoryObject 表达数据所有权和可达性

示意：

```text
setup:
  kernel grants SharedQueue + memory + event capabilities

data path:
  producer writes descriptors into ring
  producer signals doorbell/event
  consumer drains descriptors
  consumer posts completions / fence points
```

这是 Ousia “内核旁路是第一公民”的通信版本：旁路不是特权逃逸，而是受 Capability、预算、观测和撤销治理的数据面模式。

---

## 4. OS 与用户态的边界

### OS 必须提供

- Portal / Operation / Continuation 的生命周期和权限语义
- EventPort / WaitSet 的等待聚合
- deadline / timeout / cancel 的基础机制
- pending Operation quota
- late reply / duplicate reply 的硬错误
- priority / deadline / budget propagation 的调度入口
- SharedQueue / IOQueue / IOBuffer / MemoryObject 的授权和撤销
- Fence / Timeline 的内核可见等待语义
- tracing、metrics、audit hooks

### 用户态负责

- IDL schema
- method id 和协议版本
- txid / Future 表
- serialization / deserialization
- 服务框架路由
- async/await 语言绑定
- 业务级重试、幂等和补偿逻辑

边界原则：

> 内核提供通信生命周期和资源治理；用户态定义协议内容和编程模型。

---

## 5. 性能原则

Ousia 不用单一 IPC 模型覆盖所有通信。性能不妥协意味着每类负载都走自己的最短路径：

| 负载       | 路径                       | 性能原则                             |
| ---------- | -------------------------- | ------------------------------------ |
| 小 RPC     | Portal fast call           | 不排队、不分配、不进 EventPort       |
| 慢 RPC     | Operation + Continuation   | 调用方不占阻塞线程，服务端可延迟完成 |
| 流式消息   | SharedQueue + Event        | bounded ring + 批量 + 背压           |
| 大数据     | MemoryObject / IOBuffer    | 传 Capability，不拷贝数据            |
| 设备数据面 | IOQueue + Doorbell + Fence | 每批提交/完成，而不是每请求 syscall  |
| 跨队列依赖 | Fence / Timeline           | 同步对象化，不靠忙等或私有协议       |

如果某条路径在目标负载上比传统 OS 慢，就说明路径选择或原语设计错了，而不是让应用接受性能妥协。

---

## 6. 与 seL4 / Fuchsia 的关系

Ousia 吸收 seL4 的：

- 小消息直接交接
- Capability 引导调用
- reply right / continuation 思想
- IPC、通知、共享内存分离

Ousia 吸收 Fuchsia 的：

- 结构化协议工具链
- 异步请求/回复的 completion 模型
- wait-many / object event 的开发体验
- handle 传递与对象化资源

但 Ousia 不照搬任何一边：

- 不把同步 Endpoint 作为唯一通信模型。
- 不把通用 buffered Channel 作为唯一 IPC 基座。
- 不把 kernel bypass 只留给驱动特例。

Ousia 的答案是统一 Communication Fabric：Portal fast path、Operation lifecycle、EventPort waiting、SharedQueue bypass、MemoryObject/IOBuffer data transfer、Fence/Timeline synchronization。

---

## 7. 第一阶段落地顺序

建议按以下顺序验证：

1. Portal fast call：两个 Capsule 通过 Portal 做小消息调用和 Capability 传递。
2. Operation + Continuation：实现异步请求、reply-once、timeout、cancel、late reply 错误。
3. EventPort / WaitSet：同一线程等待 Operation completion、timer、cancel、MemoryObject lost。
4. SharedQueue：两个 Capsule 通过 bounded ring 批量传递 descriptors，并通过 Event 唤醒。
5. MemoryObject payload：Operation 中传递 MemoryObject Capability，实现大数据零拷贝。
6. Fence / Timeline：让 SharedQueue 和 IOQueue completion 进入同一等待模型。
7. Driver IOQueue 接入：验证普通 SharedQueue 与设备 IOQueue 共享同一套事件、撤销和观测语义。

---

## 8. 名词说明

本文中的 Portal、Operation、Continuation、EventPort、SharedQueue、Communication Fabric 是 Ousia OS 的设计术语，用来描述本项目希望收敛出的统一通信原语族。它们不是某个现有系统的专有技术名，也不表示已经存在的 ABI。完整术语解释见 [../glossary.md](../glossary.md)。
