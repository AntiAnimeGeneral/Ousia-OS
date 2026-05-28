# 01 — 现代驱动架构参考模式

## 讨论方式

本文不试图复制任何一个现有系统的全部接口，而是抽取那些对 Ousia 真正有用的结构性启示。

重点看四件事：

- 用户态 / 内核态如何分工
- 控制面和数据面如何分离
- 共享队列 / 注册内存 / doorbell 是否是核心
- SDK、调试、回放、恢复是否被当作架构一部分

## 1. Windows WDDM

参考：

- https://learn.microsoft.com/en-us/windows-hardware/drivers/display/windows-vista-and-later-display-driver-model-architecture

### 值得吸收的点

- GPU 厂商主逻辑可以留在用户态
- 内核保留调度、显存安全、功耗和显示链路等不可替代职责
- 用户态 / 内核态的分工以“系统职责”为准，而不是以“代码量”划分

### 对 Ousia 的启示

- GPU 栈最合理的方向不是“内核万能驱动”，而是**用户态厂商栈 + 最小可信内核仲裁层**
- Device Service 可以承接稳定接口层，而 Driver Host 承接厂商特化逻辑

### 不应照搬的点

- WDDM 的内核子系统很大，历史兼容负担重。Ousia 只该吸收它的分工原则，不该复制其规模。

## 2. Linux DRM + libdrm + 用户态图形栈

参考：

- https://docs.kernel.org/gpu/drm-uapi.html

### 值得吸收的点

- render node 把渲染访问和显示控制拆开
- lease 把显示资源按对象切分，而不是全局共享
- syncobj / timeline / dma-fence 让同步变成一等对象
- hot-unplug / reset / wedged 明确要求快速传播错误，避免 hang 死用户态

### 对 Ousia 的启示

- Device Resource 模型是正确方向：按 function、queue、memory、interrupt、display object 等对象授权
- Device lost、completion poison、reset/revoke 这些语义必须明确
- 同步对象不应被 GPU 私有化

### 不应照搬的点

- DRM 的 ioctl 面极宽，历史包袱重。Ousia 应避免重新发明一个更大的 ioctl 世界。

## 3. Fuchsia Driver Framework DFv2

参考：

- https://fuchsia.dev/fuchsia-src/concepts/drivers/driver_framework

### 值得吸收的点

- Driver Manager / Driver Index / Driver Host 的逻辑分工清晰
- 节点拓扑是驱动绑定和恢复编排的中心
- 驱动宿主共置是必要能力
- SDK、工具链、组件模型和驱动生命周期一起设计

### 对 Ousia 的启示

- Device Graph / Driver Index / Driver Host 这些角色不是多余抽象，而是框架化驱动所需的管理面
- 共置能力必须被正式支持，不能假设所有驱动都跨进程通信

### 不应照搬的点

- FIDL/channel 适合控制面，但不足以直接承载 GPU/NIC/NVMe 的热路径。

## 4. Apple DriverKit

参考：

- https://developer.apple.com/documentation/driverkit

### 值得吸收的点

- 驱动分发、签名、权限、升级与框架一体化
- IODispatchQueue、IODataQueue、Memory Descriptor、Memory Map 都是正式 SDK 对象
- 用户态驱动需要一整套事件、内存和队列运行时

### 对 Ousia 的启示

- Driver Package Cell、entitlement / authority、升级和回滚必须和驱动框架一起设计
- SDK 不能只暴露“底层系统调用”，而应给出完整对象层

### 不应照搬的点

- DriverKit 更偏平台 family framework。Ousia 需要更通用的 queue / buffer / event / sync substrate。

## 5. io_uring

参考：

- https://man7.org/linux/man-pages/man7/io_uring.7.html

### 值得吸收的点

- 共享 SQ/CQ ring 是现代高性能内核接口的重要模式
- 批量提交减少 syscall
- SQPOLL 说明“减少通知开销”本身就是核心设计点

### 对 Ousia 的启示

- IOQueue 作为一等原语是正确方向
- 即使保留 syscall，也应让它退回 setup / notify / fallback 路径，而不是主数据面

### 不应照搬的点

- io_uring 仍是内核主导 opcode 接口。它更像通用 queue substrate 参考，而不是完整驱动框架。

## 6. AF_XDP

参考：

- https://www.kernel.org/doc/html/latest/networking/af_xdp.html

### 值得吸收的点

- UMEM 把注册内存做成正式概念
- RX/TX 与 Fill/Completion ring 把 buffer 所有权流转模型显式化
- need_wakeup 让旁路不必永远 busy poll
- libbpf helper 说明 SDK 对数据面细节封装很关键

### 对 Ousia 的启示

- NIC 风格数据面应采用 registered memory + ownership ring 模型
- Fill / Completion 不只是网络技巧，适合成为通用 buffer 生命周期参考

### 不应照搬的点

- AF_XDP 的 SPSC 假设和 queue steering 复杂度不应直接暴露给所有上层。

## 7. SPDK

参考：

- https://spdk.io/doc/

### 值得吸收的点

- 用户态 NVMe 队列 + 轮询 / 中断混合可以获得极高性能
- 管理面和数据面显式分离
- tracing、benchmark、scheduler、tooling 都是官方体系的一部分

### 对 Ousia 的启示

- 存储类高性能路径完全可以采用用户态队列 + 注册内存 + poll/interrupt hybrid
- 高性能框架必须把工具链当成架构一部分

### 不应照搬的点

- SPDK 高性能很依赖独占设备与专核轮询。Ousia 不能把这种模式当成系统默认，只能把它做成可声明、可预算、可回退的模式。

## 8. Asterinas

参考：

- https://github.com/asterinas/asterinas

### 值得吸收的点

- 用 Rust 和框架化 unsafe 封装构建可信内核基座
- 把安全关键原语收拢成更小、更可审计的核心

### 对 Ousia 的启示

- Hardware Core 和 queue / memory / mapping 这类原语层，应尽量以可审计方式收口 unsafe
- 驱动 SDK 的安全绑定不应只是语法糖，而应体现所有权、生命周期和权限模型

## 9. 收束出来的共同结构

从这些实现里反复出现的不是某个具体接口，而是这些模式：

1. 控制面与数据面分离
2. 共享 ring 取代逐请求调用
3. 注册内存取代临时 pin / map / copy
4. 完成对象化
5. 恢复语义显式化
6. SDK、测试、回放、性能工具是一体设计

这正是 Ousia 应该吸收的“现代而优雅”的部分。

## 相关文件

- [00-bypass-first-class.md](./00-bypass-first-class.md)
- [02-driver-sdk-draft.md](./02-driver-sdk-draft.md)
- [03-subsystem-path-matrix.md](./03-subsystem-path-matrix.md)
