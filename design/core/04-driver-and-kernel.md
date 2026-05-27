# 04 — 内核原语与通用驱动框架

> 承接 [target.md](../target.md) 中的 Hardware Core、用户态驱动与设备旁路目标。

## 讨论范围

本文讨论 Ousia OS 的内核/驱动边界。这里的立场不是“内核绝对没有任何硬件相关代码”，而是：

> 设备特化逻辑、厂商策略和复杂协议默认停留在用户态；内核只保留硬件安全、早期启动、故障恢复和可验证 fast-path assist 所需的通用机制。

目标不是把驱动“搬出去”就算完成，而是定义一套足够强的基座原语，让纯用户态驱动既不退化成高延迟 RPC，也不重新把策略塞回内核。

这里有一个进一步的判断需要明确：**Ousia 应把内核旁路视为第一公民的数据面模式。** 这不意味着默认绕开内核，而是由内核显式提供 queue、registered memory、event、doorbell、fence 等快路径对象，让高频数据面不再退化成“每次请求都 syscall”或“每次请求都 Portal/Operation”。

## 术语约定

- **Hardware Core**：内核中的最小可信硬件控制面，只负责隔离、授权、复位、早期路径和极小的 fast-path assist，不承载设备特化策略。
- **Device Graph**：系统维护的硬件资源拓扑图，节点包括设备、function、queue、中断和电源状态，边表示归属、共享和依赖关系；它不是 Service Graph 在硬件世界里的简单翻版，而是资源授权和恢复编排的依据。
- **Device Service**：位于驱动之上的稳定服务接口层。它把厂商差异收敛在驱动后面，对上暴露 render、submit、read、write、present 这类较稳定的资源语义。
- **Doorbell**：通知设备“submission ring 中有新工作可取”的最小触发机制。它通常是一次受控 MMIO 写，也可以由内核代理完成。

## 1. 内核边界

### 1.1 内核必须提供

- 调度和执行等级
- 地址空间、页表、缺页入口
- Communication Fabric、EventPort/WaitSet、Handle 传递
- Capability 快速检查与撤销入口
- IOMMU/SMMU 和 DMA 隔离
- MMIO 授权
- 中断路由
- 时钟
- 通用的 IOQueue/IOBuffer 基础
- MemoryObject / Pager 基础
- 设备 quiesce、reset、revoke、isolate
- early console / panic path / boot storage minimal path

### 1.2 内核不应包含

- 厂商驱动策略
- GPU 编译器和 shader 栈
- 大型 class driver
- 文件系统格式实现（若选择纯内核态 FS 方案，本条改为禁止 POSIX/VFS 式兼容语义进入内核；Object Store 核心可成为内核 ABI）
- 网络协议策略
- 包管理、名字解析、服务发现
- 可扩展策略插件的任意执行环境

这条边界和文件系统章节是一致的：Ousia OS 反对“把一切需要高性能的东西重新塞回内核”。内核保留的是不可替代的机制，不是方便实现时顺手留下的策略层。

## 2. Hardware Core

Hardware Core 是内核中允许存在的最小可信硬件控制面。它不是传统驱动层，也不是“为了性能预留的一块特权区”。

### 2.1 允许的内容

| 类别             | 内容                                     | 理由                             |
| ---------------- | ---------------------------------------- | -------------------------------- |
| 中断和时钟       | interrupt controller, timer              | 全局调度和等待机制基础           |
| IOMMU/SMMU       | domain, map, unmap, flush                | DMA 安全边界必须硬保证           |
| MMIO 授权        | BAR/寄存器范围映射                       | 用户态驱动只能访问授权范围       |
| reset/revoke     | FLR、总线复位、电源复位、设备隔离        | 驱动崩溃后内核必须能接管         |
| early path       | early console、boot storage minimal path | 用户态尚未启动时需要最小可用路径 |
| panic path       | 崩溃输出、最小诊断                       | 不能依赖用户态服务               |
| fast-path assist | 队列所有权检查、doorbell 代理、事件投递  | 避免关键路径被边界成本拖垮       |

### 2.2 early path 的定义

Early path 是系统启动到第 1 层基础服务可用之前必须存在的最小硬件路径。例如：

- early console
- initrd / boot image 读取
- panic 日志输出
- 基础 timer / interrupt
- 进入用户态前的最低设备枚举

Early path 不应扩展成完整设备驱动。第 1 层服务启动后，应尽快把设备控制权移交给 Driver Manager 或其等价角色。

### 2.3 verified fast-path assist 的定义

Fast-path assist 是内核中极小、稳定、可审计的热路径辅助，例如：

- IOQueue doorbell 提交
- completion event 唤醒
- IOMMU 映射快速撤销
- 中断到事件对象投递
- GPU/NVMe 类队列的最小仲裁

进入 fast-path assist 的代码必须同时满足：

- 不包含厂商策略
- 不包含复杂协议解析
- 输入输出结构稳定
- 有明确不变量，可 fuzz 或模型检查
- 有性能理由，而不是为了减少用户态工程量

## 3. 用户态驱动框架

### 3.1 运行实体

- **Device Manager**：枚举硬件，维护 Device Graph 和拓扑信息
- **Driver Manager**：绑定驱动、授予设备 Handle、编排崩溃恢复
- **Driver Index**：按 VID/PID/Class/ACPI/能力匹配 Driver Package
- **Driver Host**：运行隔离的驱动实例
- **Device Service**：向上层暴露稳定的 Resource 接口

这是逻辑角色，不要求第一阶段就是五个独立服务。早期原型完全可以把 Device Manager 折叠进 Driver Manager，把 Device Service 与 Driver Host 共置；稳定后再按生命周期和信任边界拆开。

### 3.2 Driver Package Cell

驱动作为签名 Package Cell 分发：

- supported devices
- required authorities
- firmware
- service interfaces
- crash policy
- rollback policy
- ABI version

闭源厂商逻辑可以在用户态共享库中，但不能进入内核地址空间。这不是妥协，而是纯用户态驱动架构的工程优势：系统仍掌握能力授予、审计、回滚、隔离和恢复，而不会把专有二进制直接塞进最高特权层。

## 4. Device Resource 模型

物理设备不是单一文件节点，而是一组可授权、可撤销、可观测的 Resource：

```text
Physical Device
	├─ DeviceFunction
	├─ DeviceQueue
	├─ DeviceMemory
	├─ DeviceInterrupt
	├─ DevicePowerState
	└─ ResetControl
```

例子：

- GPU：display、compute、video encode、memory management
- NIC：rx queue、tx queue、timestamp、offload
- NVMe：admin queue、io queue、namespace
- Audio：control、playback stream、capture stream

应用或系统服务拿到的是具体 Resource 的 Handle，而不是 `/dev/gpu0` 这类全局节点。这样能力边界绑定的是对象和操作，不是路径习惯和隐式共享。

## 5. 统一 IO 原语

驱动、存储、网络、GPU、Pager 共享的是一组通用对象家族，而不是一套“每个子系统各自发明 ring buffer”的私有机制：

- **IOBuffer**：注册内存、pin、权限、DMA 可达性
- **IOQueue**：提交队列，可映射共享环
- **IOCompletion**：完成队列
- **IOEvent**：中断、轮询、取消、超时
- **Doorbell**：受控 MMIO 或内核代理提交
- **TransferPlan**：描述 copy、map、DMA、zero-copy 路径以及所有权如何流转；它不承载数据本体，只描述数据如何移动

这些对象的目标是统一授权、调度和观测，而不是在第一阶段就冻结一整套巨大的设备 ABI。Ousia OS 应先稳定对象模型、权限语义和故障契约，再收敛成长期 ABI。

### 5.1 设计判断：旁路是一等公民，但只限于数据面

Ousia 不应把 kernel bypass 理解成“少量特权应用可以偷偷直通硬件”的例外能力，而应把它设计成标准 substrate：

- **控制面**默认走 Portal / Operation：枚举、绑定、策略、恢复、版本协商
- **机制面**默认走 syscall：映射、授权、等待、复位、撤销
- **数据面**默认走 queue/buffer/event/fence/doorbell：避免逐请求 syscall 或 Portal/Operation

因此，IOQueue/IOBuffer/Event/Fence 这些对象不是“性能增强件”，而是驱动框架的中心抽象。

### 5.2 Kernel Device Substrate API 草图

下面的调用名只是对象模型草图，用来约束能力边界和热路径，不代表第一阶段就必须冻结成正式 ABI：

```text
device.enumerate()
device.claim(device, authority)
device.release(device)
device.query_topology(device)

mmio.map(device, bar, offset, length, rights, cache_policy)
mmio.unmap(region)

dma.register_buffer(buffer, direction, lifetime)
dma.map(buffer, device, rights)
dma.unmap(mapping)
dma.revoke(mapping)

queue.create(function, type, depth, flags)
queue.map_rings(queue)
queue.submit(queue, descriptors)
queue.arm_event(queue)
queue.poll(queue, budget)
queue.cancel(queue, token)

irq.bind(device_irq, event)
irq.unbind(device_irq)

device.quiesce(device)
device.reset(target, mode)
device.revoke_all(device)
device.isolate(device)
```

这些 API 的方向不是“让应用直接控制硬件”，而是给 Driver Manager、Driver Host 和 Device Service 提供统一基座。

### 5.3 IOBuffer

IOBuffer 是统一的内存注册模型，用来表达 DMA、零拷贝和跨设备传输的生命周期：

```text
IOBuffer {
	virtual_range,
	page_list,
	pin_state,
	iommu_mappings,
	cache_policy,
	numa_node,
	lifetime,
	owner,
	sharing_policy,
}
```

它覆盖：NVMe PRP/SGL、NIC packet buffer、GPU buffer object、audio ring、camera frame buffer 等典型场景。

但 IOBuffer 不应和 MemoryObject 被草率地视为“同一种对象”。更准确的关系是：

- **Memory Object** 面向 VM 映射、缺页、共享和回写
- **IOBuffer** 面向设备可达性、pin 生命周期和 DMA 授权
- 两者可以共享页框和映射元数据，但第一阶段不强行合并成单一万能类型

这样才能和 [03-pager-and-memory.md](./03-pager-and-memory.md) 的 VM 语义保持一致。

### 5.4 IOQueue

IOQueue 是内核一等 Resource。没有它，每个驱动都会各自实现 ring、event、doorbell、取消和回收，系统无法统一权限、调度和观测。

```text
IOQueue {
	submission_ring,
	completion_ring,
	doorbell,
	event,
	mode: interrupt | polling | hybrid,
	priority,
	budget,
	owner,
	device_function,
}
```

IOQueue 至少应支持：

- 共享环
- 批量提交
- completion coalescing（把多个完成事件合并成一次中断或一次唤醒）
- 取消和超时
- backpressure（消费者跟不上时，把降速信号传回生产者）
- priority / QoS
- 中断与轮询混合
- 跨 Capsule 的受控共享

### 5.5 Doorbell 策略

Doorbell 是高性能设备的关键，但不应无条件暴露给用户态。

| 场景                     | 模式                 |
| ------------------------ | -------------------- |
| 独占队列、低风险、高性能 | 允许 direct doorbell |
| 共享队列、需要公平性     | proxy doorbell       |
| 全局寄存器或安全敏感路径 | 禁止用户态直接访问   |

Direct doorbell 依赖严格 MMIO 授权、队列所有权和 IOMMU 隔离。Proxy doorbell 牺牲少量延迟，换来调度、限速和审计。

### 5.6 Fence / Timeline

Fence 和 Timeline 不应长期停留为 GPU 私有概念，而应成为跨设备同步 Resource 的方向性设计。

用途包括：

- GPU command completion
- DMA copy completion
- display present
- video encode frame done
- NPU inference done
- NVMe → GPU → NPU → Display 的跨设备流水线

这部分在第一阶段不必冻结成复杂 ABI，但对象语义和等待模型应提前预留，以避免未来又回到“每个设备各自定义同步原语”的老路。

## 6. 性能判断

这个设计不会天然产生性能问题，但有硬前提：**热路径不能退化为频繁 IPC 和系统调用。** 性能来自共享队列、共享内存、doorbell、批处理、IOMMU 隔离和直接映射，而不是来自“驱动在用户态”这个事实本身。

### 6.1 热路径应长什么样

理想热路径：

```text
App / Device Service
	-> 写 submission ring
	-> 写 doorbell 或请求 proxy doorbell
	-> 设备 DMA 访问 IOBuffer
	-> completion ring 写回
	-> poll 或 event 唤醒
```

在 direct doorbell + polling 模式下，提交和完成可以没有 syscall；在 proxy doorbell + interrupt 模式下，会多一次受控边界，但换来公平性、省电和安全。

### 6.2 性能风险和对策

| 风险                   | 原因                        | 对策                                              |
| ---------------------- | --------------------------- | ------------------------------------------------- |
| IPC 往返过多           | 每个请求都找 Driver Host    | 队列 mmap、批量提交、共享 completion              |
| 拷贝过多               | 应用、服务、驱动各有 buffer | IOBuffer 注册、zero-copy TransferPlan             |
| IOMMU 开销             | 频繁 map/unmap              | 长生命周期映射、batch map、IOTLB-aware 分配       |
| 中断风暴               | 高频 completion             | interrupt moderation、polling、hybrid mode        |
| 轮询耗电               | 高性能 busy polling         | execution class + power budget 控制               |
| 共享设备不公平         | 直接 doorbell 绕过调度      | proxy doorbell、queue budget、timeline/fence 仲裁 |
| 驱动崩溃残留队列和 DMA | 队列与映射仍然存活          | revoke、IOMMU unmap、reset、completion poison     |
| 小 IO 尾延迟           | 队列过深或跨进程协调        | per-CPU/per-NUMA queue、优先级队列、自适应合并    |

这里的两个词需要特别说明：

- **queue budget**：调度器授予某个队列的提交/运行配额，用来保证共享设备上的公平性和前台活性。
- **completion poison**：当驱动崩溃或队列失效后，内核把 completion 路径标记为“只返回明确失败”，让等待者尽快收到错误，而不是无限阻塞。

### 6.3 与现代实践的对应

| 实践              | 对 Ousia OS 的含义                                               |
| ----------------- | ---------------------------------------------------------------- |
| DPDK              | 用户态 NIC 高性能依赖 queue ownership、polling、DMA 隔离         |
| SPDK              | NVMe 用户态路径可接近裸机，但需要用户态队列和轮询模型            |
| io_uring          | 统一 submission/completion queue 可显著减少 syscall 和上下文切换 |
| RDMA              | memory region、queue pair、completion queue 是可泛化模型         |
| WDDM              | GPU 用户态厂商逻辑可行，但内核必须保留调度和显存安全仲裁         |
| DriverKit/Fuchsia | 用户态驱动提升隔离和生命周期治理，但框架必须给出低开销数据通道   |

### 6.4 结论

通用驱动框架不会成为性能瓶颈，前提是：

1. IOQueue/IOBuffer/Event 是内核一等原语。
2. 高频路径使用 mmap ring、批量提交和共享 completion。
3. DMA 使用长生命周期授权，避免每次请求 map/unmap。
4. doorbell 同时支持 direct 和 proxy 两种模式。
5. 中断和轮询可按负载动态切换。
6. 调度器能管理 queue budget、priority 和 power budget。
7. Driver Host 不在每个 IO 的同步路径上。

如果这些条件不满足，用户态驱动会慢；如果满足，用户态驱动可以接近或达到内核路径性能，同时获得隔离、可恢复和可治理性。

## 7. 驱动崩溃和撤销

当 Driver Host 崩溃：

1. 内核撤销相关 Device Handle。
2. IOMMU 取消 DMA 映射。
3. 中断路由解绑。
4. 必要时执行 quiesce、reset 或 isolate。
5. Driver Manager 根据策略重启、回滚或降级。
6. 上层 Device Service 通知使用者 `DEVICE_LOST` 或 `DEVICE_DEGRADED`。

不尝试在内核中恢复厂商状态。内核负责收敛现场和恢复硬件边界，策略恢复留在用户态。

## 开放问题

1. 通用驱动 ABI 的稳定期应该多长？
2. GPU 多上下文调度：内核只做仲裁，还是提供最小可验证调度器？
3. 不支持 FLR 的设备如何安全 reset？
4. IOQueue 是否允许用户态直接 doorbell，还是必须经过内核代理？
5. fast-path assist 的准入标准由谁审核？
6. IOBuffer 的 pin 预算如何与全局内存回收协调？
7. Device Service 和 Driver Host 何时应拆分，何时允许共置？

## 相关章节

- [01-capsule-and-capability.md](./01-capsule-and-capability.md) — 设备能力句柄和资源授权
- [03-pager-and-memory.md](./03-pager-and-memory.md) — Memory Object 与 DMA/注册内存的边界
- [05-compute-and-scheduling.md](./05-compute-and-scheduling.md) — GPU 调度、功耗预算与执行等级
- [02-engineering.md](../topics/02-engineering.md) — 驱动 SDK、回放测试和 ABI 收敛策略
- [06-roadmap.md](../topics/06-roadmap.md) — 驱动框架原型的分阶段落地
