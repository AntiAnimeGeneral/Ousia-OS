# 02 — Ousia Driver SDK 草案

## 讨论范围

本文不是最终 API 规格，而是 Driver SDK 的草案轮廓。目标是先稳定对象模型和分层，再决定长期 ABI 细节。

## 1. 设计目标

Ousia Driver SDK 需要同时满足四个目标：

1. 让 GPU / NIC / NVMe / 加速器共享一套高性能数据面底座。
2. 让用户态驱动真正可写，而不是只有理论上的“可运行”。
3. 让 reset / revoke / device lost / hot-unplug 这些异常面成为正式契约。
4. 让 SDK、回放、观测、仿真和 profiling 从一开始就是体系的一部分。

## 2. 不做什么

- 不直接冻结一个庞大的 per-device syscall ABI。
- 不要求每个驱动自己实现 ring、descriptor、doorbell、fence 运行时。
- 不把 vendor blob 视为内核一等组件。
- 不把“性能”理解成默认允许不受治理的直通访问。

## 3. 三面分层

### 控制面

角色：Driver Manager、Device Manager、Driver Index、Device Service

职责：

- 枚举与匹配
- 设备 / function / queue claim
- 上下文创建与销毁
- 资源授权
- 版本协商
- 恢复和降级编排

推荐接口风格：Portal / Operation

### 数据面

角色：IOQueue、IOBuffer、Event、Fence、Timeline、Doorbell

职责：

- descriptor 提交
- completion 收割
- DMA 可达内存
- 零拷贝
- poll / interrupt hybrid
- 跨队列同步

推荐接口风格：kernel bypass substrate

### 异常面

角色：Hardware Core、Driver Manager、Device Service

职责：

- revoke
- reset
- isolate
- completion poison
- device lost 传播
- telemetry / crash dump / metrics

推荐接口风格：syscall + Portal / Operation 协作

## 4. SDK 包结构草案

### ousia-driver-core

定义安全对象与通用类型：

- DeviceHandle
- FunctionHandle
- QueueHandle
- BufferHandle
- EventHandle
- FenceHandle
- TimelineHandle
- MappingHandle
- ResetReport / DeviceLost

### ousia-driver-control

负责控制面协议：

- 设备发现
- 拓扑查询
- 资源 claim / release
- capability / authority 检查
- 恢复回调注册

### ousia-driver-datapath

负责数据面运行时：

- ring helper
- descriptor builder
- registered memory allocator / frame pool
- direct / proxy doorbell helper
- poll / interrupt hybrid runtime

### ousia-driver-sync

负责同步对象：

- Fence
- Timeline
- 跨队列依赖
- event wait helper

### ousia-driver-observe

负责观测：

- queue stats
- latency histogram
- timeout / revoke / reset event
- tracepoint binding
- health report

### ousia-driver-sim

负责开发与测试：

- mock queue
- software-only simulator
- PCI/MMIO/doorbell/interrupt 录制回放
- descriptor / fence / timeline 模型测试

### ousia-driver-c

为闭源或遗留实现提供最小 C ABI shim，但它应绑定在相同对象模型上，而不是另起一套语义。

## 5. 核心对象模型

### DeviceHandle

代表一个被授予的物理设备或逻辑设备控制权。

它不直接等于“全设备 root 权限”，而应进一步分解成 function、queue、memory、interrupt 等资源。

### FunctionHandle

代表设备上的一个可独立授权的功能块，例如：

- GPU render
- GPU display
- NIC rx / tx
- NVMe admin
- NVMe io namespace path

### QueueHandle

代表数据面的核心提交对象。

它应支持三种 profile：

- **Direct**：独占队列 + direct doorbell + 可选 polling
- **Managed**：共享队列 + proxy doorbell + 强预算治理
- **Hybrid**：direct submit + event / interrupt fallback

### BufferHandle / MemoryRegistration

代表已注册、已授权、设备可达的内存区。

关键属性：

- ownership
- rights
- pin state
- mapping lifetime
- sharing policy
- cache policy

### EventHandle

统一中断、completion、timeout、cancel 的等待入口。

### Fence / Timeline

统一跨设备同步，不应是 GPU 私有对象。

### MappingHandle

代表 MMIO、doorbell 或 registered memory 的具体映射。它应明确可撤销、可失效、可观测。

## 6. Rust API 轮廓草图

```rust
let runtime = DriverRuntime::attach()?;

let device = runtime
    .devices()
    .claim(DeviceSelector::pci("vendor:device"), Authority::Render)?;

let function = device.open_function(FunctionClass::Render)?;

let queue = function.open_queue(QueueProfile::Direct {
    depth: 1024,
    completions: CompletionMode::Hybrid,
})?;

let buffer = queue.register_buffer(BufferSpec {
    pages: UserPages::from_slice(&mut command_buffer),
    rights: BufferRights::device_read_write(),
    sharing: SharingPolicy::Private,
})?;

let timeline = runtime.sync().create_timeline()?;

let batch = queue
    .batch()
    .push(CommandDescriptor::render(buffer.slice(..)))
    .signal(timeline.point(42));

queue.submit(batch)?;

let completion = queue.wait(WaitPolicy::event_or_poll())?;
completion.check()?;
```

这个例子要表达的重点不是语法，而是：

- 控制面 claim 与数据面 submit 分开
- buffer 注册是显式步骤
- queue profile 是正式概念
- completion 与 timeline 是一等对象

## 7. 运行时与调度

SDK 不能只暴露对象，还要给出最小运行时约束。

### poll / interrupt hybrid

运行时应允许驱动声明：

- 在高负载下优先 polling
- 在低负载或省电模式下优先 interrupt
- 用 need_wakeup 风格提示减少无意义 syscall

### queue budget / priority / power hints

每个 queue 都应能声明：

- 预算
- 优先级
- 期望延迟
- 功耗偏好

它们不是调度器的命令，而是正式 hint / contract 输入。

## 8. 恢复模型

所有高性能驱动都必须显式处理以下事件：

- device lost
- queue revoked
- DMA mapping revoked
- reset completed
- isolate entered

SDK 应提供统一的 callback / report 通路，而不是让每个厂商自定义错误传播方式。

示意：

```rust
runtime.on_device_lost(|report| {
    log::error!("device lost: {:?}", report.reason());
    RecoveryAction::PropagateToService
});
```

## 9. 观测与调试

高性能驱动框架如果没有内建观测，最终只能靠猜。

SDK 应至少暴露：

- queue depth
- completion latency
- drops / invalid descriptors
- reset / revoke counters
- poll to interrupt transitions
- timeline / fence wait breakdown

并支持：

- tracepoints
- benchmark harness
- queue dump / descriptor dump
- software simulator
- record / replay

## 10. ABI 收敛策略

Ousia 不应一开始就承诺一个巨型稳定 ABI，而应分三层稳定：

1. **对象语义稳定**：Queue、Buffer、Fence、Timeline、Event、ResetReport 的意义稳定。
2. **控制面协议版本化**：Driver Manager / Device Service 协议可演进。
3. **设备 dialect 延后收敛**：descriptor 格式、queue dialect、vendor 扩展在更高层版本化。

也就是说，先稳定“平台抽象”，再稳定“设备方言”。

## 11. 需要主文档最终吸收的点

1. SDK 是架构一部分，不是系统调用绑定层。
2. kernel bypass 只有在 queue / buffer / event / sync / recovery 这组对象齐全时才算成立。
3. Driver SDK 的最小单位应是对象模型，而不是 ioctl 名字表。

## 相关文件

- [00-bypass-first-class.md](./00-bypass-first-class.md)
- [01-modern-driver-patterns.md](./01-modern-driver-patterns.md)
- [03-subsystem-path-matrix.md](./03-subsystem-path-matrix.md)
