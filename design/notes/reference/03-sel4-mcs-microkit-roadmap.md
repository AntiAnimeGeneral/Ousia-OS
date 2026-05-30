# 03 — seL4 MCS、Microkit 与静态系统构建参考

> 状态：参考材料。本文用于理解 seL4 MCS 调度模型、Microkit 的保护域/通道/系统描述机制，以及这些机制对 Ousia 早期 kernel + OSDK 分层的启发。规范性设计见 [05-compute-and-scheduling.md](../../core/05-compute-and-scheduling.md)、[06-service-graph.md](../../core/06-service-graph.md) 与 [02-engineering.md](../../topics/02-engineering.md)。

## 1. seL4 社区当前主线

seL4 的 roadmap 不是只围绕内核本身。systems roadmap 明确把 Microkit、sDDF、LionsOS 这类系统框架放在核心位置；verification roadmap 则继续推进 MCS、AArch64、平台端口和 multikernel 的证明。

这对 Ousia 有一个直接提醒：

> 工业级微内核项目不能只写 kernel。kernel、系统描述、初始化器、驱动框架、测试镜像和文档路线必须一起演进。

我们现在把 `kernel/` 和 `ostd/` 拆开是对的，但下一步需要更明确地区分：

- 低层 kernel 机制。
- OSTD/OSDK 对 kernel 机制的安全封装。
- 系统构建工具和声明式配置。
- 用户态服务、驱动和测试系统镜像。

## 2. MCS：把 CPU 时间变成对象

seL4 MCS 的核心不是“换个调度算法”，而是把 scheduling context 变成显式对象。

从源码可以看到，`sched_context_t` 可以绑定到 TCB 或 notification。它承载预算、周期、消耗、yield 关系等调度状态。线程不再只是“有优先级就能运行”，而是需要具备可调度的 scheduling context。

这带来几个重要语义：

- CPU 时间是可控制、可转移、可回收的资源。
- passive server 可以借调用者或通知携带的 scheduling context 运行。
- real-time 系统可以用 budget / period 描述 CPU 占用上限。
- 调度授权从“线程属性”演进为“能力对象”。

Ousia 后续如果要支持实时、能耗、服务隔离和 QoS，就不能只做传统 priority queue。最好尽早把“执行预算”作为可建模资源，而不是等调度器写完后再补。

## 3. Passive server 的价值

Microkit 的 passive PD 继承了 MCS 的核心思想：一个保护域可以没有自己的长期 scheduling context，在收到 notification、protected procedure call 或 fault 时，借事件来源携带的上下文运行。

这适合系统服务：

- 服务自身不常驻消耗 CPU。
- 服务执行成本可以归属到调用者或触发源。
- 优先级反转和预算归属更容易分析。

Ousia 的 Service Graph 可以吸收这个模型：服务节点不一定都需要独立 worker。某些服务应是被动组件，只在被调用时运行，并把资源消耗记到触发方。

## 4. Microkit 的保护域模型

Microkit 把系统构造成一组 protection domains。每个 PD 是单线程、固定地址空间、事件驱动的组件。

它不是 Unix 进程模型。一个 PD 通常提供这些入口：

- `init`：初始化。
- `notified`：收到 notification。
- `protected`：被调用 protected procedure。
- `fault`：处理子 PD 或 VM 的 fault。

这让组件生命周期非常清楚：初始化后进入事件循环，事件到来时执行对应入口，返回后继续等待。

对 Ousia 的启发：

- 早期系统服务可以先采用单线程事件驱动组件，而不是立刻提供通用进程模型。
- 服务入口应该被系统框架固定下来，避免每个服务自己写主循环。
- fault handler 应该是一等关系，而不是全局 crash logger。

## 5. Channel 与 protected procedure

Microkit 的 channel 连接两个 PD。PD 不能直接引用另一个 PD，只能通过 channel id 间接交互。

channel 支持两类交互：

- notification：非阻塞信号。
- protected procedure：同步调用，调用者阻塞，callee 运行并返回结果。

protected procedure 有严格约束：只能调用更高优先级的 PD，调用图需要无环。这不是随意限制，而是为了防止死锁和简化实时分析。

Ousia 可以借鉴这个边界：

- 服务调用图应能静态检查一部分环路和优先级问题。
- 控制面同步调用和异步通知要分开建模。
- 对高可信服务，调用优先级和调用方向应成为系统配置的一部分。

## 6. System Description File 与 capDL initialiser

Microkit 的一个重要工程价值是：系统结构在构建期声明，工具把系统描述转换成 capDL specification，再由 capDL initialiser 创建对象、分发 capability、启动 PD。

这条链路的意义很大：

1. 系统拓扑不是启动后临时拼出来的。
2. capability 分发可以被构建工具审计。
3. 内存、IRQ、channel、PD、VM 的关系可以提前检查。
4. 在 ARM/RISC-V 上，工具甚至会模拟 seL4 boot 后的 untyped 分配条件，尽量把错误提前到构建期。

Ousia 后续也需要类似机制。否则随着服务增多，capability 分发会变成不可审计的手写启动脚本。

## 7. Loader、initialiser 与 monitor

Microkit 的启动链可以抽象成：

1. loader 准备镜像、硬件状态和内存布局。
2. seL4 kernel 启动并交给 initial task。
3. capDL initialiser 根据规格创建对象和 capability。
4. Monitor 作为最高优先级 PD 处理默认 fault。
5. 各个 PD 进入自己的事件循环。

这对 Ousia 当前 QEMU runner 很有帮助。我们现在只是让 kernel 打印启动消息，但下一阶段需要拆出：

- boot loader / image packer 责任。
- kernel initial task 责任。
- system graph initialiser 责任。
- fault monitor 责任。

这些不应该全塞进 kernel `main`。

## 8. Roadmap 上值得跟随的方向

seL4 roadmap 中对 Ousia 最值得跟随的方向：

- **Microkit**：小型、静态、事件驱动系统框架。
- **sDDF**：高性能用户态驱动框架。
- **MCS verification**：说明 MCS 是长期主线，而不是实验支线。
- **AArch64 verification**：和我们 AArch64-first 测试路线契合。
- **multikernel**：用多个单核 seL4 实例获得多核扩展和逐步验证路径。
- **PD templates**：在静态系统中引入受控运行时弹性。

Ousia 不需要复制这些路线，但应该把它们作为成熟社区的压力测试：如果我们的设计解释不了这些问题，就说明抽象还不够稳。

## 9. 对 Ousia 的近期建议

1. 在 `ostd` 之外规划一个系统构建工具，不要把启动拓扑硬编码进 kernel。
2. 给 Service Graph 增加静态配置格式，先覆盖 PD/service、channel、memory region、IRQ、fault handler。
3. 尽早把 scheduling context / execution budget 写入设计，而不是只保留 priority。
4. 把 qemu smoke 发展成“最小系统镜像”测试，而不是只测 kernel 打印。
5. 为 passive service 留出模型，避免每个服务都变成常驻线程。

## 10. 读源码时最值得看的位置

本地参考目录：

- `third_party/sel4/`
- `third_party/microkit/`

优先看这些文件和文档：

- `third_party/sel4/src/object/schedcontext.c`
- `third_party/sel4/src/kernel/sporadic.c`
- `third_party/sel4/src/object/notification.c`
- `third_party/microkit/README.md`
- Microkit manual 的 Protection Domain、Channel、System Description、Internals 章节
- Microkit roadmap 的 x86、multi-core、multi-kernel、PD templates 条目
