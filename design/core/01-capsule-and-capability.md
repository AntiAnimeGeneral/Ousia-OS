# 01 — 沙盒与能力模型

> 承接 [target.md](../target.md) 中的默认沙盒、显式授权与能力权限目标。

## 讨论范围

Capsule 是 Ousia OS 的运行单元，Capability 是 Ousia OS 的权限单元。本文讨论两者的设计细节、交互方式，以及能力派生、转发和撤销的实现策略。

---

## Capsule：不仅仅是"沙盒化的进程"

### Capsule 的边界

一个 Capsule 不只是一个进程。它是一个**运行域**，包含：

```
┌─────────────────────────────────────────┐
│ Capsule                                  │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  │
│  │ Thread 1│  │ Thread 2│  │ Thread N│  │  ← 共享地址空间
│  └─────────┘  └─────────┘  └─────────┘  │
│                                          │
│  可见对象:                                │
│  ┌──────────┐  ┌──────────┐              │
│  │ Service A│  │ Object B │  ...         │  ← 能力句柄集合
│  └──────────┘  └──────────┘              │
│                                          │
│  资源预算:                                │
│  CPU: 2 cores | MEM: 512MB | GPU: 10%   │  ← QoS 约束
│  Network: 100Mbps | IOPS: 1000          │
└─────────────────────────────────────────┘
```

一个 Capsule 内可以有多线程，但共享同一个能力句柄集合和资源预算。这是有意的设计——Capsule 内的线程互相信任，但 Capsule 之间默认不信任。

### 为什么不是每个线程一个 Capsule？

- 线程共享地址空间是高效的编程模型，强行拆分没有价值
- 多线程程序（如数据库、浏览器）需要在同一个信任域内运行
- Capsule 的粒度是"一个应用的信任域"，不是"一个执行单元"

### 默认不可见

Capsule 启动时的默认视图：

- ❌ 没有文件系统命名空间
- ❌ 没有 `/proc`, `/sys`, `/dev`
- ❌ 没有网络接口
- ❌ 没有环境变量继承（除了显式声明的）
- ❌ 没有对其他 Capsule 的可见性
- ✅ 只有启动时被授予的能力句柄集合

---

## Capability：权限作为一等对象

### 能力句柄的语义

一个能力句柄是一个不可伪造的内核对象引用，携带：

```
Capability {
    object: ObjectReference,   // 指向的目标对象
    rights: RightsMask,        // 允许的操作（READ | WRITE | EXEC | GRANT | ...）
    parent: Option<CapabilityID>, // 内核可见派生来源
    generation: Generation,    // 缓存描述符和映射失效版本
    transfer: TransferPolicy,  // 是否可转发、可派生、需租约或 Broker
    audit_tag: AuditID,        // 传播链日志
}
```

关键性质：

- **不可伪造**：句柄由内核管理，用户态无法构造
- **按策略可传递**：只有携带相应转发权限的句柄才能通过 IPC 发送给其他 Capsule
- **可拆分**：从 READ|WRITE 句柄派生出 READ-only 句柄；派生后权限只能单调减少，不能增加
- **可硬撤销**：内核可见能力维护派生链，支持删除当前句柄、撤销所有后代句柄和销毁底层对象
- **可语义失效**：服务级授权按能力类别使用租约过期、generation 失效通知或 Broker 协议
- **可审计**：传播链日志记录句柄从哪里来到哪里去

### 能力类型体系

| 能力类型 | 绑定对象     | 示例                       |
| -------- | ------------ | -------------------------- |
| Service  | 服务接口     | `my-app-api` 的调用权      |
| Object   | 持久对象     | `user-data` 的读写         |
| Device   | 设备队列     | GPU 渲染队列的提交权       |
| Network  | 网络端点     | `*.example.com:443` 的 TCP |
| Memory   | 共享内存区域 | DMA buffer 的映射权        |
| Identity | 身份声明     | 用户身份句柄               |
| Timer    | 定时器       | 高精度定时器               |
| Stream   | 数据流       | 日志流、传感器流           |

### 浏览器摄像头权限的分层实现

浏览器申请摄像头权限这类功能可以实现，而且应当按层拆开：

- **内核层**：只负责摄像头相关 Device Resource 的能力授予、IOMMU/DMA 隔离、队列和缓冲区的硬撤销、设备 reset/isolate。
- **系统服务层**：负责权限弹窗、origin / tab / window / session 关联、用户选择记忆、关闭页面后的回收、恢复时重新申请。
- **浏览器层**：把网页的 `getUserMedia` 请求映射成系统服务请求，不直接绕过权限系统。

这意味着“关闭网页后收回，再次使用时重新申请”不是内核单独能完成的完整功能，它需要：

1. 浏览器把页面或 tab 的摄像头请求转成一个受控的摄像头能力申请。
2. 系统服务把该能力绑定到当前 page/session，并展示用户确认弹窗。
3. 内核只看见一个可撤销的摄像头资源句柄和对应 buffer / queue / interrupt 权限。
4. 当页面关闭、tab 结束、session 退出或授权过期时，系统服务撤销该句柄；内核负责硬撤销设备路径，浏览器收到失效通知后必须再次申请。

因此，摄像头权限的**授权决策和生命周期状态**放在系统服务中更合适，**设备访问的硬约束和 DMA 撤销**放在内核中更合适。内核不应直接存储“某个网页是否允许使用摄像头”的高层策略；它只应保证一旦策略层要求撤销，设备与缓冲区路径能够立即失效。

### 与 uid/gid 的本质区别

```
Unix:
  alice (uid=1000) 可以读 /home/alice/*
  因为她是 alice

Ousia OS:
  Capsule X 可以读 Object "user-data"
  因为它持有 Capability{object: user-data, rights: READ}
  （这个能力可能是用户交互后显式授予的）
```

权限从"身份属性"变成了"可传递的对象"。

### 身份、管理员与能力

Identity 只证明"谁是这个主体"，不直接赋予运行时权限。用户、设备所有者、组织和发布者身份都只能参与授权决策；决策结果必须变成 Capability、租约、策略记录或密钥解封装权限。

Ousia 不定义 Unix `root`。需要高权限管理时，系统授予可拆分的管理能力，例如 DeviceOwnerCapability、SystemUpdateCapability、RecoveryCapability、PolicyAdminCapability、NamespaceAdminCapability 和 KeyRecoveryCapability。这些能力可以由去中心化 Identity、组织 Identity、本地恢复密钥或硬件根持有，并且必须可审计、可委托、可撤销。

PIN 或生物识别只解锁本地 Key Agent，用于批准短期签名、解密或敏感能力授予；它不是身份私钥，也不是全局管理员口令。完整身份与恢复策略归属 [05-identity-and-accounts.md](../topics/05-identity-and-accounts.md)。

---

## 与 seL4 的关系：底层不应弱于 seL4

seL4 的优雅之处在于边界极窄：内核只管理少量对象和 capability，所有 capability 位于 CSpace 中，复制、派生和删除都由内核记录；`CNode_Revoke` 删除某个 capability 的所有子 capability。它强的是**内核可见能力派生树上的硬撤销**，不是服务状态回滚、缓存数据追回或业务语义撤销。

Ousia OS 不应宣称自己在微内核能力核心上天然比 seL4 更强。最佳方案是：第 0 层内核能力核心采用不弱于 seL4 的硬派生与硬撤销模型；在它之上，Ousia 再提供更面向现代系统的类型化资源、Service Graph、租约、对象 generation、IOBuffer/IOMMU 和 Package Cell 语义。换句话说，Ousia 的强大不来自替代 seL4 的能力核心，而来自把这个硬核心扩展成完整平台语义。

因此，Capability Broker 不能替代内核派生树。Broker 只用于服务级委托、跨服务审计、租约续期和失效通知；凡是内核可见对象，撤销必须能在内核内完成。

---

## 能力撤销：硬核心与语义外层

### 为什么不能只做直接撤销

假设：

1. Capsule A 持有能力 C（READ|WRITE|GRANT）
2. A 将 C 拆分为 C1（READ）和 C2（WRITE）
3. A 将 C1 发送给 B，C2 发送给 C
4. B 将 C1 转发给 D
5. 现在 A 的授予者想要撤销最初授予 A 的能力

需要追踪的链：`origin → A → {B → D, C}`

如果只删除 A 手中的原始句柄，B、C、D 手中的派生句柄仍然可能有效。这不是能力系统，而只是引用计数。内核可见能力必须至少支持：

- 追踪每个能力句柄的派生来源
- 跨 Capsule 追踪内核可见的 handle passing
- 处理"部分撤销"（如只撤销 WRITE 派生链，但保留 READ-only 派生链）
- 让缓存描述符、共享队列 descriptor 和映射在 generation 变化后明确失败

### 最佳方案：两层撤销模型

Ousia OS 采用两层模型：

1. **硬能力层**：Portal、Continuation、EventPort、MemoryObject、IOBuffer、IOQueue、Device Resource、IOMMU mapping 等内核可见对象，必须维护 capability derivation tree。内核提供 `delete(handle)`、`revoke_descendants(handle)`、`destroy_object(object)` 和 `invalidate_generation(object)` 等语义。撤销后，后代句柄、映射、等待者和 fast-path descriptor 必须进入明确错误状态。
2. **语义授权层**：ObjectHandle lease、Service Graph session、Package Cell activation、业务委托和用户态服务内部状态，不承诺被内核自动回滚。它们通过 lease、generation、watch、Broker 通知、服务重启或应用级补偿来失效。

这条边界让 Ousia 同时获得两种能力：底层像 seL4 一样硬，平台层又能表达现代 OS 需要的服务、数据和生命周期语义。

### 第一阶段必须冻结的能力合同

第一阶段不需要实现所有服务语义撤销，但第 0 层内核能力合同必须先冻结：

- `GRANT` 或等价权限控制派生和转发；没有该权限时只能使用，不能复制或降权派生
- 派生 capability 的 rights 必须是父 capability rights 的子集
- `delete(handle)` 只删除当前 slot 中的句柄
- `revoke_descendants(handle)` 删除所有内核可见后代句柄，但不删除当前句柄
- `destroy_object(object)` 销毁底层对象，并使所有指向它的 capability、映射和等待者失败
- `invalidate_generation(object)` 让已经发出的 fast-path descriptor、缓存映射或 ObjectHandle lease 失效
- 撤销不会追回已经复制到用户态的数据，也不会撤销已经完成的外部 side effect

Capability Broker 是第二阶段的语义层增强，不是第一阶段硬撤销的替代品。正式 ABI 前需要决定哪些能力默认不可转发，哪些能力允许租约续期，哪些能力必须通过 Broker 转发后才承诺跨服务撤销通知。硬件资源、身份声明、持久数据写权限等敏感能力应默认走不可转发、短租约或 Broker 强制路径。

### IOMMU 路径的特殊性

硬件资源能力（DMA 映射）的撤销不能依赖用户态协商——它必须在硬件层面生效：

1. 内核收到 DMA 能力撤销请求
2. 内核立即停止相关 queue 或标记 completion poison
3. 内核修改 IOMMU 页表，移除该设备的 DMA 映射并完成必要的 TLB flush
4. 设备后续的任何 DMA 访问 → IOMMU fault → 设备隔离
5. 这是硬撤销路径，不依赖用户态配合

---

## 开发体验的挑战

### "默认无权限"如何不影响开发效率

如果开发者每写一个程序都要手动声明 20 个能力，开发体验会很差。需要：

- **开发模式**：`Ousia OS run --dev my-app` 自动授予常见开发能力（本地网络、项目目录读写、调试接口）
- **能力模板**：常见场景（"web 应用"、"CLI 工具"、"数据库"）的预定义能力集合
- **渐进式收紧**：开发时宽松 → 测试时 medium → 发布时最小权限
- **审计工具**：`Ousia OS audit my-cell` 显示实际使用了哪些能力，帮助剪裁

---

## 开放问题

1. **Capsule 之间共享内存的权限模型？** 如果两个 Capsule 需要共享一个内存区域，能力句柄如何表达"你可以读写这个区域"？
2. **定时器能力的粒度？** "每 10ms 一次的高精度定时器"是一个能力吗？如何防止恶意 Capsule 消耗所有定时器资源？
3. **开发模式的权限边界？** 开发模式授予了网络访问，但开发中的代码可能包含恶意依赖——如何平衡？
4. **Capability Broker 的可信性？** Broker 不能替代内核派生树，但它仍会影响语义级授权通知。它需要多大程度的特权和隔离？

---

## 相关章节

- [08-package-cell.md](./08-package-cell.md) — Cell 如何声明能力需求
- [06-service-graph.md](./06-service-graph.md) — 通过 Service Graph 发现和授权服务
- [04-driver-and-kernel.md](./04-driver-and-kernel.md) — 设备能力句柄和 IOMMU
