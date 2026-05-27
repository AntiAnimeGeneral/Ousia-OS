# 03 — FS / GPU / NIC / NVMe 路径矩阵

## 讨论范围

本文把同一个判断压到四类子系统上：

- 文件系统
- GPU
- NIC
- NVMe / 存储设备

问题不是“它们应不应该用户态化”，而是：

- 控制面走什么（Portal / Operation）
- 机制面走什么
- 数据面走什么（SharedQueue / IOQueue / MemoryObject / IOBuffer / Fence）
- 谁默认拥有设备或资源
- 出错后如何恢复

## 1. 总矩阵

| 子系统 | 控制面（默认 Portal/Operation）                    | 机制面（默认 syscall）                                    | 数据面（默认 bypass）                                        | 默认拥有者                   | 何时允许直通                       | 恢复契约                                          |
| ------ | -------------------------------------------------- | --------------------------------------------------------- | ------------------------------------------------------------ | ---------------------------- | ---------------------------------- | ------------------------------------------------- |
| FS     | 纯用户态 FS：Portal/Operation 到 FS 服务；纯内核态 FS：内核 Object Store 原语 | 纯用户态 FS：MemoryObject、Pager 通道、页框、回写确认；纯内核态 FS：Object Store + page cache | 纯用户态 FS：SharedQueue/Pager-backed MemoryObject；纯内核态 FS：IOQueue/IOBuffer/MemoryObject | FS 服务 / Pager 或内核 Object Store | 数据库、存储服务、专用 Capsule     | `MEMORY_OBJECT_LOST` 或内核 Object Store journal 恢复 |
| GPU    | 上下文创建、资源授权、编译策略、显示策略、恢复编排 | MMIO 授权、显存隔离、IOMMU、irq、reset、调度仲裁          | command queue、mapped buffer、doorbell、fence / timeline     | Device Service + Driver Host | 高性能图形 / 计算服务              | `DEVICE_LOST`、queue poison、reset 后重建上下文   |
| NIC    | 网络策略、路由、防火墙、队列授权、服务发现         | queue bind、memory register、irq、IOMMU、revoke           | RX/TX/Fill/Completion ring、UMEM 风格 registered memory      | 网络服务                     | 包处理器、交换面、特权网络服务     | queue revoke、drop stats、device lost、queue 迁移 |
| NVMe   | namespace 管理、flush policy、多路径、资源分配     | queue create、DMA map/revoke、irq bind、reset             | admin / io queue、registered buffer、poll / interrupt hybrid | 存储服务                     | 数据库、target service、专用存储栈 | queue revoke、namespace lost、reset、重试或降级   |

## 2. 文件系统不是“纯旁路子系统”

### 控制面

- create / unlink / rename
- 事务和日志
- 索引维护
- 路径投影
- 跨设备同步策略

纯用户态 FS 方案中，这些停留在用户态存储服务。纯内核态 FS 方案中，它们成为内核 Object Store 原语，但仍不等同于 POSIX VFS。

### 机制面

- Memory Object 创建与映射
- 缺页入口
- 页框分配与回收
- 等待对象
- 脏页回写确认
纯用户态 FS 方案中，机制面不包含内核元数据缓存 fast-path；性能依赖 Portal fast call、批量接口、SDK/兼容域缓存和 bypass session。纯内核态 FS 方案中，元数据缓存是内核 Object Store 的自然组成部分。

这些应停留在内核。

### 数据面

- 顺序数据访问：异步 IO / Stream
- 随机共享访问：Memory Object + Pager
- 极端高性能块路径：受控块设备直通

### 设计判断

FS 的候选表达不是“混合缓存文件系统”，而是两个纯方案：

- 纯用户态 FS：所有 FS 语义在服务内，热路径靠 IPC/batch/cache/bypass/Pager 协议
- 纯内核态 FS：Object Store 核心在内核内，热路径靠 IOQueue/IOBuffer/MemoryObject/page cache

## 3. GPU 是最典型的控制面 / 数据面分离子系统

### 控制面

- 驱动绑定
- API 适配
- 编译器和 shader 栈
- 上下文与资源分配
- 显示策略与恢复编排

### 机制面

- 显存安全
- IOMMU / DMA 授权
- 中断绑定
- reset / isolate
- 最小调度仲裁

### 数据面

- command queue
- mapped buffer object
- direct / proxy doorbell
- completion
- fence / timeline

### 设计判断

GPU 不应走“每个提交一次 IPC”，也不应走“一个巨型内核 ioctl 宇宙”。

它应是：

- 用户态厂商主栈
- 内核最小可信仲裁层
- 共享队列和同步对象构成的数据面

## 4. NIC 的旁路价值很高，但默认归网络服务所有

### 控制面

- 路由、策略、防火墙、命名、服务暴露
- queue 授权与分配
- 多队列编排

### 机制面

- UMEM / registered memory 注册
- queue bind
- interrupt / poll 切换
- revoke / isolate

### 数据面

- RX ring
- TX ring
- Fill ring
- Completion ring

### 设计判断

NIC 是最适合吸收 AF_XDP / DPDK 思路的地方，但默认不应由普通应用直接持有裸队列。

默认模式应是：

- 普通应用走网络服务
- 高性能网络服务、交换面、包处理器经授权拿到队列所有权

## 5. NVMe / 存储设备天然适合队列数据面

### 控制面

- admin queue 管理
- namespace 生命周期
- flush / durability policy
- multipath / failover

### 机制面

- queue create / destroy
- DMA map / revoke
- irq bind
- reset / quiesce

### 数据面

- admin queue
- io queue
- registered buffer
- poll / interrupt hybrid

### 设计判断

NVMe 是最适合吸收 SPDK 风格模型的子系统之一，但默认拥有者仍应是存储服务，而不是普通应用。

只有数据库、专用 target service、缓存层等明确声明的 Capsule，才应获得受控直通路径。

## 6. 什么时候可以允许直通

不是所有子系统都应该默认放开直通。

### 允许直通的条件

- 有明确的 Capability / authority
- 有 IOMMU / DMA 隔离
- 有队列所有权和预算
- 有可传播的 device lost / revoke 语义
- 有 tracing / metrics / reset 观测能力

### 不应允许直通的场景

- 共享资源没有公平性模型
- 无法做 revoke / reset
- 数据面需要依赖复杂全局策略
- 旁路会破坏前台交互和功耗预算

## 7. 统一设计判断

从四个子系统里可以压出同一条总规则：

1. **控制面优先 Portal / Operation。**
2. **机制面优先 syscall。**
3. **高频数据面优先 SharedQueue / IOQueue / IOBuffer / Fence 等 bypass substrate。**

这条规则不是抽象口号，而是 Ousia 在 FS、GPU、NIC、NVMe 上都能重复成立的系统边界。

## 8. 接下来可落地的原型顺序

如果要验证这套矩阵，建议原型顺序如下：

1. 通用 IOQueue + Event + Doorbell 原型
2. IOBuffer / registered memory + revoke 原型
3. Fence / Timeline 原型
4. 一个 NVMe 队列原型
5. 一个 NIC RX/TX ring 原型
6. 一个 GPU command queue 原型
7. 一个 Pager + 块设备直通协作原型

这样能验证“统一 substrate”是否真的能覆盖多个子系统，而不是只适合一个设备类别。

## 相关文件

- [00-bypass-first-class.md](./00-bypass-first-class.md)
- [01-modern-driver-patterns.md](./01-modern-driver-patterns.md)
- [02-driver-sdk-draft.md](./02-driver-sdk-draft.md)
