# 02 — Communication Fabric

> 承接 [target.md](../target.md) 中的统一通信、异步请求、事件等待与旁路数据面目标。
>
> 参考材料：[00-ipc-sel4-fuchsia.md](../notes/reference/00-ipc-sel4-fuchsia.md)
>
> 本文是通信语义的主设计。驱动队列的设备侧细节归属 [04-driver-and-kernel.md](./04-driver-and-kernel.md)，异步与 `mmap` 的张力归属 [00-async-and-mmap.md](../topics/00-async-and-mmap.md)。

## 讨论范围

本文定义 Ousia OS 的统一通信基座。这里的目标不是在“同步 IPC”和“异步 IPC”之间二选一，而是提供一组正交、统一、强大的系统原语，让权威控制面、异步请求、内核治理队列、用户态旁路数据面和设备队列都落在同一套能力、等待、取消、背压、调度和观测语义上。

Ousia 不追求为了“小内核”而把关键语义推给每个用户态 runtime 自行实现。相反，系统应把通信生命周期中会影响权限、安全、调度、公平性和生态一致性的部分做成统一 OS 原语；协议内容、IDL、服务框架、用户态队列布局和语言绑定留给用户态。

---

## 1. 设计目标

Ousia 的通信系统必须同时满足五个目标：

1. **性能不妥协**：小 RPC 不能被队列化拖慢；高吞吐数据面不能每条消息 syscall；大数据不能塞进 IPC 消息拷贝。
2. **语义统一**：同步、异步、流、队列、设备完成事件都使用统一的等待、取消、超时、背压和观测模型。
3. **能力安全**：服务入口、回复权、共享队列、内存对象和设备队列都必须是可授权、按策略转交、内核硬撤销或语义失效的 Capability 对象。
4. **生态一致**：异步请求的 completion routing、late reply、取消、超时、pending quota、priority/deadline propagation 不应由每个框架各自发明。
5. **旁路一等公民**：高频数据面使用共享队列、预授权共享内存、doorbell、event、fence，而不是退化为“每次请求一次 IPC”。旁路只表达预授权数据面的协议生命周期，不表达 Capability 所有权转移。

---

## 2. 核心原语族

Ousia 的通信基座称为 **Communication Fabric**。这是 Ousia 文档中的设计术语，不指代现有某个具体技术或产品。

### 2.1 Portal

Portal 是服务入口能力。持有 Portal Capability 的 Capsule 才能向对应服务提交请求。

Portal 负责：

- 服务调用入口
- 能力检查
- 小消息 fast path
- Capability / MemoryDescriptor 转移的权威入口
- 共享内存、队列、事件对象的建立和撤销入口
- 调用方执行等级、deadline、budget 的传播入口
- 服务崩溃时的 `SERVICE_LOST` 传播

Portal 不等于 Fuchsia Channel，也不等于 seL4 Endpoint。它是 Ousia 的服务入口抽象：可以支持同步 fast call，也可以支持异步 Operation submit。本文把通过 Portal、系统调用或受信服务完成的授权、映射、撤销、seal、对象创建和对象销毁统称为 **control path**。control path 是慢但有权威的路径；它不应承载高频 payload 数据面。

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

Ousia 采用 save 模式表达同步调用的延迟回复语义：同步 Portal 调用到达服务端时，内核为当前 caller 维护一个隐式 Continuation。服务端可以立即回复，也可以把它 `save` 成显式 `SaveHandle`，再把 `SaveHandle` 移动给 worker 线程、异步任务或服务框架。这个模型受 seL4 `SaveCaller` / reply cap 启发，但在 Ousia 中作为显式、能力化的 continuation 原语暴露。

Continuation 必须支持：

- 一次性回复（reply-once）
- deadline 后自动失效
- cancel 后失效
- late reply 返回明确错误
- pending quota 计数
- 审计和观测
- save 成 move-only `SaveHandle` 后跨线程转移
- 可选的 priority / deadline donation

`SaveHandle` 不是可复制的 future，也不是长期 channel。它是一次性、move-only、reply-once 的 Capability。保存后，原接收线程不再持有隐式回复权；持有 `SaveHandle` 的执行流负责 `reply`、`reply_yield`、drop/cancel 或把它继续移动给另一个执行流。

Ousia 对回复动作区分两个基础语义：

- `reply(handle, result)`：完成 caller continuation，把调用方标记为 runnable，必要时向调用方所在核心发 reschedule IPI；当前 worker / executor 继续执行其他任务。
- `reply_yield(handle, result)`：完成 caller continuation，并在当前核心让出执行权，优先让调度器运行刚被唤醒的调用方或其他更合适的任务。它适合入口线程的 inline fast path。

实现可以进一步提供 `reply_and_recv(handle, result, portal)` 作为 `reply_yield + recv` 的原子快路径，用于同一入口线程处理短请求后立即等待下一个请求。这对应 seL4 `ReplyRecv` 的性能形态；跨线程 handoff 则使用普通 `reply`。

Continuation 不是用户态约定的 reply endpoint，也不是业务协议里的 txid。txid 可以由用户态 runtime 使用；Continuation / SaveHandle 是内核可见的回复权和生命周期控制对象。

### 2.4 EventPort / WaitSet

EventPort / WaitSet 是统一等待聚合器。

它应能等待：

- Operation completion
- Portal readable / service lost
- Timer expired
- cancel signal
- MemoryObject lost
- SharedQueue readable / writable
- KernelChannel readable / writable
- IOQueue completion
- Shared memory object revoked / peer lost
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
- 与用户态 TransferArena 协议、IOBuffer / MemoryObject / Fence 组合

SharedQueue 是普通服务之间的 kernel bypass 通信 substrate，也是驱动 IOQueue 的上层统一形式。区别在于：驱动 IOQueue 的消费者可能是设备，普通 SharedQueue 的消费者是另一个 Capsule。

SharedQueue 的热路径不传递 Capability，也不表达 MemoryDescriptor 所有权转移。它只发布已经预授权的数据面描述符，例如 SDK 局部的 `pool_id + offset + len + generation + flags`。真正的 Capability 转移、MemoryDescriptor 授权、共享内存映射、撤销和 seal 必须走 control path。

### 2.6 KernelChannel

KernelChannel 是内核治理的队列式 IPC。它不是最高性能路径，也不与 bypass queue 竞争吞吐；它提供的是 policy-enforced queued IPC。

KernelChannel 适用于：

- 跨不信任域的普通队列通信
- 需要内核强制背压、公平性、优先级和预算的通信
- 需要统一等待、审计、trace、取消和 timeout 的队列
- shell 管道、工具组合、系统服务默认入口
- 不适合暴露用户态共享队列协议的兼容场景

KernelChannel 可以支持 batch push / batch pop，以减少 syscall 成本。但它的价值不在于击败 bypass queue，而在于让系统拥有一个安全、可观测、可治理的默认队列路径。

### 2.7 MemoryObject / IOBuffer / Shared Memory

大数据不应内联进 Operation 消息体。Operation 只传递 MemoryObject、IOBuffer 或切片 Capability。

- MemoryObject 面向 VM 映射、缺页、共享、CoW 和回写。
- IOBuffer 面向 registered memory、pin 生命周期、DMA 可达性和设备授权。
- Shared memory object 面向跨 Capsule 共享映射和旁路数据面。OS 只负责对象创建、授权、映射权限、引用生命周期、撤销和失效通知；不规定用户态池分配协议。

MemoryObject、IOBuffer 和 shared memory object 可以共享页框和映射元数据，但语义不能混同。

MemoryObject / IOBuffer / MemoryDescriptor / shared memory object 属于权威对象；它们的创建、授权、转移、撤销和销毁必须由内核或受信服务介入。共享内存本体由内核对象引用、映射引用、pin 引用和队列绑定引用共同决定生命周期；只要仍有有效引用或 in-flight pin，内核不能释放 backing memory。

TransferArena 不是内核原语，而是用户态 SDK 在预授权共享内存上定义的 arena 布局和协议。它把已经授权好的共享内存暴露给旁路队列使用，例如定义 slot、freelist、producer metadata、consumer retire metadata 和 generation。OS 可以给这些 metadata 页设置不对称映射权限，但不理解每个 slot 的业务含义。

TransferArena 内部的 slot 生命周期可以由用户态 SDK 管理，例如：

```text
Free -> ReservedByProducer -> Published -> AcquiredByConsumer -> RetiredByConsumer -> Free
```

这个状态机表达的是协议生命周期，不是 Capability 所有权转移。纯 bypass 不保证 payload 不变，也不能强制 sender 在 publish 后物理上停止写入 payload；它只保证 arena 对象生命周期、映射权限、边界、撤销和故障隔离。跨不信任域中需要成为权威事实的控制字段、审计数据、持久化数据或权限声明，必须走 Portal / KernelChannel，或在进入权威状态前经过 copy、seal、hash 校验、只读重映射等步骤。

不对称 TransferArena 应在 SDK 建立协议时固定角色和布局，并请求 OS 对底层共享内存页设置对应映射权限：

| 区域                              | Producer 视图 | Consumer 视图 |
| --------------------------------- | ------------- | ------------- |
| payload pages                     | writable      | readable      |
| producer metadata                 | writable      | read-only     |
| consumer retire / credit metadata | read-only     | writable      |
| kernel metadata                   | unmapped      | unmapped      |

这样 producer 可以绕过 SDK 写 payload 或提交坏 descriptor，但不能伪造只映射给 consumer 写入的 retire metadata、不能释放共享内存本体、不能扩大映射边界、不能获得额外 Capability，也不能破坏内核对象生命周期。协议违规的后果应限制在该 channel 内：drop、poison、revoke 或 peer-lost。

### 2.8 Fence / Timeline

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

### 3.1 权威控制面：Portal / Sync Call

适用场景：

- 服务发现后的短 RPC
- 纯内存查询
- 权限检查
- 小状态读取
- 同步控制面操作
- Capability / MemoryDescriptor 转移
- shared memory object / SharedQueue / EventPort 的创建、绑定、撤销和销毁

目标：

- 不进内核消息队列
- 不分配内核缓冲区
- 不经过 EventPort
- 尽量同核直接交接
- 小消息和少量 Capability 走 fast path
- 大对象只传权威 handle / Capability，不把 payload 塞进消息体

示意：

```text
client -> portal.call(op)
server entry -> recv request + implicit continuation
inline fast path -> reply_yield / reply_and_recv -> caller resumes or server waits next request
handoff path -> save continuation as SaveHandle -> worker handles -> reply -> caller becomes runnable
```

这一路径吸收 seL4 同步 Endpoint 的关键经验，但不把整个系统限制在同步编程模型里。Portal / Sync Call 是 Ousia 的 authority path：硬权限转移、对象创建、映射变更、撤销、seal 和 peer death 处理必须在这里完成，不能伪装成旁路队列里的普通消息。

服务端框架应把短请求和可能阻塞的请求分开：短请求可以在入口线程 inline 处理并使用 `reply_yield` 或 `reply_and_recv` 保持最低延迟；可能阻塞、等待 IO、进入异步 runtime 或需要线程池的请求应先 `save` 成 `SaveHandle`，再 handoff 给 worker。worker 完成后调用 `reply` 唤醒调用方，但自身继续执行 executor 中的其他任务。

`reply_yield` 的语义不是“当前线程永久绑定到调用方”，而是完成回复后把当前核心交还给调度器。调度器通常会优先运行刚被唤醒的调用方；如果有更高优先级、更早 deadline 或更合适亲和性的任务，也可以选择它。`reply` 则只完成 continuation 和唤醒，不要求当前执行流让出。

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

### 3.3 内核治理队列：KernelChannel

适用场景：

- 普通 app 到系统服务的队列通信
- shell 管道和工具组合
- 跨不信任域的消息流
- 需要内核强制背压、配额、公平性和优先级的服务入口
- 需要统一 audit / trace / timeout / cancel 的队列
- bypass queue 不适合暴露给调用方的兼容路径

目标：

- 提供安全、可观测、可治理的队列默认路径
- 支持 batch push / batch pop 降低 syscall 成本
- 由内核强制队列深度、字节预算、wake/sleep 和 peer lost
- 不承诺成为最高吞吐数据面

KernelChannel 的存在理由不是击败 bypass queue，而是填补 Portal fast call 与完全用户态 bypass 之间的治理空档。它让系统可以给不可信或普通调用方提供队列语义，而不要求每个服务都暴露自己的共享内存协议。

### 3.4 高频数据面：SharedQueue / SDK TransferArena / IOQueue Bypass

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
- 通过预授权共享内存和 SDK TransferArena 协议表达 payload 可达性
- 不在热路径表达 Capability 所有权转移

示意：

```text
setup:
  Portal/control path creates SharedQueue + shared memory object + EventPort
  kernel maps producer/consumer views with requested permissions
  kernel binds queue, event, quota, revoke policy
  SDK initializes TransferArena layout inside shared memory

data path:
  producer reserves slot and writes payload
  producer writes descriptor: pool_id + offset + len + generation + flags
  producer signals doorbell/event
  consumer drains descriptor and reads payload
  consumer retires slot / returns credit
  consumer posts completions / fence points if needed
```

这是 Ousia “内核旁路是第一公民”的通信版本：旁路不是特权逃逸，而是预授权、可撤销、可观测的数据面模式。旁路队列只保证共享对象生命周期、映射权限、边界、背压和故障隔离；payload 字节默认是非权威数据。Capability、对象身份、权限声明、所有权转移和跨信任域的控制字段必须走 Portal / Operation / KernelChannel 或 copy/seal 路径。

### 3.5 Bypass Session 与端点权限

Bypass 不表示“没有端点”，而是把端点分成 control plane 和 data plane：

- control plane endpoint 是 Capability 对象，例如服务 Portal、对象 Handle 或数据传输 Session。
- data plane transport 是该 endpoint 授权后的 `SharedQueue + shared memory + SDK TransferArena`。

以纯用户态 FS 方案下的读取为例：

```text
open:
  client --Portal fast call--> FsPortal.open(path, READ)
  fs checks path/OID/ACL/snapshot/quota
  fs returns FileHandle capability

session setup:
  client --Portal fast call--> FileHandle.create_read_session(options)
  fs creates ReadSession capability
  control path grants SharedQueue + shared memory object + EventPort
  SDK initializes SQ/CQ rings and TransferArena layout

hot path:
  client writes SQE: READ_AT + file_offset + len + pool_id + dst_offset + generation
  fs drains SQE, validates it against ReadSession policy, fills shared payload
  fs writes CQE: user_data + status + bytes_read + pool_id + dst_offset + generation
  client drains CQE, reads payload, retires slot
```

权限分工：

| 层级              | 负责者             | 语义                                                           |
| ----------------- | ------------------ | -------------------------------------------------------------- |
| 服务入口          | OS Capability + FS | 谁有权调用 FS 服务                                             |
| 文件对象          | FS                 | path/OID/ACL/snapshot/transaction/quota                        |
| 会话对象          | FS + OS Capability | `ReadSession` 绑定对象、操作、范围、预算和生命周期             |
| 传输对象          | OS                 | SharedQueue、shared memory、EventPort 的授权、映射、引用和撤销 |
| 热路径 descriptor | FS SDK             | opcode、offset、len、pool bounds、generation、session active   |

因此，bypass 之后每条 `READ_AT` 不再复用 OS 的通用权限检查；FS 必须把 SQE 当作不可信输入，并做 session-local validation：

```text
session is active
opcode is allowed by session rights
offset + len is within authorized range
len <= session.max_io_size
pool_id / dst_offset / len is within shared arena bounds
generation is valid
quota / inflight budget is available
```

这些检查不应重新执行完整 path lookup 或 ACL traversal。完整权限检查发生在 `open` 和 `create_read_session`；热路径只验证请求是否仍落在该 session 固化的权限快照内。

如果客户端绕过 SDK，OS 不保证 descriptor 语义正确，也不保证 payload 不变。OS 保证的是：未授权 Capsule 拿不到 queue/shared memory/event handle；共享内存不会在仍被引用时释放；映射权限不能被客户端扩大；peer lost、revoke 和 poison 可被观测。协议违规应被限制在该 session 内，FS 可以 drop、返回 CQE error、poison session 或 revoke session。

纯内核态 FS 方案下，高频文件 IO 更自然地落在 IOQueue / IOBuffer / CompletionQueue 上：调用方提交 ObjectHandle + offset + len + IOBuffer 的 descriptor，内核 Object Store 消费请求并完成 CQE。SharedQueue 仍用于用户态服务之间的 bypass 协议，不是内核 Object Store 的必要热路径。

### 3.6 IPC SDK 与服务框架

Communication Fabric 不能只停留在内核原语层。Ousia 必须提供 first-class 用户态 IPC SDK 和服务端框架，把 Portal、Operation、Continuation、SaveHandle、EventPort、SharedQueue、TransferArena、Fence、timeout、cancel、quota 和 revoke 组合成可用的开发模型。

核心原则是：sync / async 是 API 形态；direct sync IPC、Operation + SaveHandle、KernelChannel 和 bypass queue 是传输策略。应用和服务实现不应把“同步 API”硬绑定到某一种 IPC 路径，也不应把“异步 API”硬绑定到某一种队列协议。

服务端框架应抽象为：

```text
Service = Portal + ReceiverSet + Dispatcher + Executor + BypassSessions
```

它负责：

| 职责           | 说明                                                                                         |
| -------------- | -------------------------------------------------------------------------------------------- |
| receiver 注册  | 通常注册接近核心数的 entry runner，由内核/运行时按 same-core、NUMA、shard 和 quota 路由      |
| sync fast path | 短请求在入口 activation inline 处理，使用 `reply_yield` 或 `reply_and_recv`                  |
| save / handoff | 可能阻塞、等待 IO 或进入 async runtime 的请求先 save 成 `SaveHandle`，再交给 worker/executor |
| reply 策略     | inline 路径让出核心给调用方或下一个调度目标；worker 路径只 `reply` 并继续执行其他任务        |
| async executor | 等待 EventPort、WaitSet、Timer、Fence、IO completion 后再完成 `SaveHandle`                   |
| bypass session | 建立 SharedQueue、shared memory、EventPort 和 SDK TransferArena，并生成 SQ/CQ dispatcher     |
| validation     | 检查 capability、session、opcode、range、arena bounds、generation、quota 和 revoke 状态      |
| observability  | 统一 tracing、metrics、audit、late reply、cancel 和 service lost                             |

调用端 SDK 应同时生成或提供 sync stub、async stub 和低层 bypass API。例如：

```text
file.stat()              -> sync facade
file.stat_async().await  -> async facade
file.read_at(buf)        -> sync facade over direct IPC or bypass SQ/CQ
file.read_at_async(buf)  -> async facade over EventPort / completion / CQE
file.read_batch(batch)   -> explicit bypass / batch API
```

路径选择由 SDK 和服务框架根据负载、权限、session、可用性和策略决定：

| API 形态      | 可能传输路径                            | 说明                                                      |
| ------------- | --------------------------------------- | --------------------------------------------------------- |
| sync stub     | direct Portal sync call                 | 小请求、metadata、权限检查、低延迟控制面                  |
| sync stub     | bypass SQ/CQ + 等待 CQE                 | 大 payload 或高频 request/reply，但调用方仍要阻塞等待结果 |
| async stub    | Operation + EventPort                   | 调用方不能阻塞线程，完成后唤醒 Future                     |
| async stub    | bypass SQ/CQ + EventPort                | 高频数据面，完成事件由 CQE / EventPort 驱动               |
| low-level API | raw SharedQueue / TransferArena / Fence | 驱动、FS、数据库、runtime、批处理和 pipeline              |

IDL / 服务定义应成为这个框架的入口。开发者声明接口和会话语义，工具生成 client sync stub、client async stub、server dispatcher、SaveHandle handoff glue、bypass session protocol、validation 代码和 tracing hooks。示意：

```text
service File {
  stat(path) -> Stat                  [sync_fast]
  open(path, rights) -> FileHandle    [sync_fast]
}

session FileReadSession {
  read_at(offset, len, dst) -> bytes_read [bypass_request_reply]
}
```

这样普通应用看到的是同步和异步都自然的一等 API；高性能服务仍能显式使用 batch、stream、fence 和 raw queue。框架的职责是隐藏路径选择和生命周期 glue，而不是抹平底层语义差异。跨信任域的 Capability 转移、MemoryDescriptor 授权、对象创建和撤销仍必须走 control path；bypass SDK 只管理预授权数据面的协议生命周期。

---

## 4. OS 与用户态的边界

### OS 必须提供

- Portal / Operation / Continuation 的生命周期和权限语义
- EventPort / WaitSet 的等待聚合
- deadline / timeout / cancel 的基础机制
- pending Operation quota
- late reply / duplicate reply 的硬错误
- priority / deadline / budget propagation 的调度入口
- KernelChannel 的队列深度、阻塞/唤醒、配额、背压和 peer lost 语义
- SharedQueue / IOQueue / IOBuffer / MemoryObject / shared memory object 的授权和撤销
- shared memory object 的对象引用、映射引用、pin 引用、queue binding 和 revoke 生命周期
- shared memory object 创建时的映射权限和 metadata 页分权支持
- Fence / Timeline 的内核可见等待语义
- tracing、metrics、audit hooks

### 用户态负责

- IDL schema
- method id 和协议版本
- txid / Future 表
- serialization / deserialization
- 服务框架路由
- async/await 语言绑定
- bypass queue 的 ring layout、alloc/retire、generation、bounds check 和 batch 策略
- SDK TransferArena 的 arena layout、slot 状态机和协议生命周期
- payload 语义校验；不可把可变 bypass payload 当作权威控制字段
- 业务级重试、幂等和补偿逻辑

边界原则：

> 内核提供 authority、对象生命周期、映射权限、等待、撤销和资源治理；用户态定义协议内容、队列布局、buffer slot 生命周期和编程模型。纯 bypass 不承诺 payload 不变，也不承诺 slot 级硬所有权转移。

---

## 5. 性能原则

Ousia 不用单一 IPC 模型覆盖所有通信。选择路径时先看负载形状，而不是先问“同步还是异步”：

| 负载形状                                                     | 首选路径                                   | 为什么                                                                    | 不要拿它做什么                                             |
| ------------------------------------------------------------ | ------------------------------------------ | ------------------------------------------------------------------------- | ---------------------------------------------------------- |
| 几十到几百字节的控制请求，需要立即回答                       | Portal fast call                           | 直接交接，小消息和少量 Capability 走 fast path                            | 不要排进通用队列，不要为它建立共享内存池                   |
| 可能等待磁盘、网络、设备或远端服务的请求                     | Operation + Continuation + EventPort       | 调用方不占阻塞线程，内核治理 timeout、cancel、late reply 和 pending quota | 不要用裸 txid/reply queue 重新发明生命周期规则             |
| 普通消息流，需要内核强制背压、公平性、审计或不信任域隔离     | KernelChannel                              | 内核能看见队列深度、预算、wake/sleep 和 peer lost                         | 不要把它当最高吞吐数据面；高频 payload 应迁到 bypass       |
| 高频 payload，双方已通过 control path 建立信任边界和共享内存 | SharedQueue + SDK TransferArena            | 热路径只写 ring descriptor 和共享内存，alloc/retire 在用户态完成          | 不要在这里传 Capability，不要把可变 payload 当权威控制字段 |
| 大对象需要强授权、转交、撤销或长期映射                       | MemoryObject / IOBuffer / MemoryDescriptor | 传权威句柄和权限，不把大数据塞进消息体                                    | 不要伪装成 bypass queue 里的 `pool_id` 或普通指针          |
| 设备提交/完成队列或 DMA 数据面                               | IOQueue + Doorbell + Fence                 | 每批提交/完成，设备、驱动和 runtime 共享完成语义                          | 不要每个请求 syscall，也不要让设备队列脱离撤销和观测       |
| 多个队列、设备或服务之间有完成顺序依赖                       | Fence / Timeline                           | 把依赖对象化，统一进入 EventPort/WaitSet                                  | 不要靠忙等、sleep 或私有状态位拼同步                       |

如果某条路径在目标负载上比传统 OS 慢，就说明路径选择或原语设计错了，而不是让应用接受性能妥协。

直观规则：权威变化走 Portal / Operation；安全默认队列走 KernelChannel；高频 payload 走 SharedQueue + SDK TransferArena；硬件数据面走 IOQueue；跨路径依赖用 Fence / Timeline 收束。

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
- 不让旁路队列假装能表达 Capability 所有权转移。

Ousia 的答案是统一 Communication Fabric：Portal fast path、Operation lifecycle、EventPort waiting、KernelChannel governed queue、SharedQueue / SDK TransferArena bypass、MemoryObject/IOBuffer authority transfer、Fence/Timeline synchronization。

---

## 7. 第一阶段落地顺序

建议按以下顺序验证：

1. Portal fast call：两个 Capsule 通过 Portal 做小消息调用和 Capability 传递。
2. Operation + Continuation：实现异步请求、reply-once、timeout、cancel、late reply 错误。
3. EventPort / WaitSet：同一线程等待 Operation completion、timer、cancel、MemoryObject lost。
4. KernelChannel：实现内核治理的 batch push / batch pop、阻塞/唤醒、背压、peer lost 和审计。
5. SharedQueue + SDK TransferArena：两个 Capsule 通过 bounded ring 传递 `pool_id + offset + len + generation`，通过共享内存中的不对称用户态池完成 alloc/retire，并通过 Event 唤醒。
6. MemoryObject / IOBuffer payload：Operation 中传递 MemoryObject / IOBuffer Capability，实现权威大对象授权和撤销。
7. Fence / Timeline：让 SharedQueue、KernelChannel 和 IOQueue completion 进入同一等待模型。
8. Driver IOQueue 接入：验证普通 SharedQueue 与设备 IOQueue 共享同一套事件、撤销和观测语义。

---

## 8. 名词说明

本文中的 Portal、Operation、Continuation、EventPort、KernelChannel、SharedQueue、TransferArena、Communication Fabric 是 Ousia OS 的设计术语，用来描述本项目希望收敛出的通信原语族和 SDK 协议族。它们不是某个现有系统的专有技术名，也不表示已经存在的 ABI。完整术语解释见 [../glossary.md](../glossary.md)。
