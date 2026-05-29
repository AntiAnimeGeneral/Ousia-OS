# 需求与抽象推导

本文是 Ousia OS 的需求库和抽象推导索引。它承接 [target.md](./target.md) 的愿景目标，把第一阶段必须满足的能力写成可追踪需求，并记录这些需求如何推出核心抽象。全文档地图和语义归属表见 [outline.md](./outline.md)。

`target.md` 只保留摘要；本文保存可增长的需求、推导和落点。稳定后的抽象结论必须落到对应 `core/` 主设计中，本文只保留证据链和追踪关系。

## 1. 需求规则

硬需求使用 `R#` 编号，作为第一阶段验收入口。抽象推导使用 `D#` 编号，记录哪些需求组合推出哪些抽象。稳定后的抽象定义、边界和协议必须写入对应 owning 文档；owning 关系由 [outline.md](./outline.md) 的语义归属表维护。本文只保存需求、推导和落点追踪。

## 2. 第一阶段硬需求

硬需求是第一阶段验收条件，不是实现细节偏好。一个主线设计如果不能说明自己满足哪条硬需求，就不应成为第一阶段核心设计；一个抽象如果不能自然满足相关硬需求，就说明抽象边界需要重画。

| 编号 | 需求                                   | 验收条件                                                                                                                                                                        | 主线承接                                   |
| ---- | -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| R1   | 类 FUSE 的存储接入                     | 本地、远程、加密、同步和兼容投影存储都能作为 provider 接入系统                                                                                                                  | FS Provider                                |
| R2   | FS 可挂载到目录                        | native provider 的目录下可挂载 remote provider；路径解析跨 provider 后仍返回统一 ObjectHandle                                                                                   | Object Namespace                           |
| R3   | mmap 必须是原生能力                    | 文件/对象可映射为 MemoryObject；缺页路径有明确供页者、故障模型、优先级和回写边界                                                                                                | MemoryObject、Pager                        |
| R4   | 大数据路径支持 zero-copy / low-copy    | 大块 IO、mmap、设备 DMA、共享缓冲区和 provider fast path 不因抽象边界被迫复制                                                                                                   | IOBuffer、SharedQueue、TransferArena       |
| R5   | 同步与异步都是一等调用形态             | 小控制面支持低延迟 sync call；长请求可取消、超时、等待、late reply                                                                                                              | Communication Fabric                       |
| R6   | 用户态服务不能牺牲热路径               | 高频 FS/driver/network 路径具备 fast call、batch、queue、bypass 或 direct descriptor 形态                                                                                       | Portal fast call、bypass session           |
| R7   | 权限必须可组合、可审计、可分层撤销     | 打开、挂载、mmap、DMA、跨 Capsule 传递都必须由 Capability 表达；内核可见能力必须支持派生链硬撤销，服务语义授权通过租约、generation 或 Broker 失效，未登记语义委托不承诺全局回滚 | Capability、ObjectHandle、DMA capability   |
| R8   | 兼容性不能污染原生 API                 | POSIX open/read/write/mount/fuse 由兼容域或网关翻译，不成为原生对象模型                                                                                                         | Compatibility Domain、POSIX projection     |
| R9   | 远程资源必须是一等场景                 | 远程 FS、远程服务和远程对象的延迟、断连、一致性和 durability fence 有系统级表达                                                                                                 | Remote-backed MemoryObject、Lease、Fence   |
| R10  | 安装、升级、回滚和多版本并存必须系统化 | 应用不依赖 PATH/bashrc/profile 拼装；依赖和生命周期由系统记录、激活和回滚                                                                                                       | Package Cell、Environment / Config Service |
| R11  | 身份、设备所有权和密钥策略必须分层     | Identity、Device Owner、Key Agent、FSKeyPolicy 和 Capability 边界清楚；PIN 不等于私钥或 root                                                                                    | Identity、Key Agent、FSKeyPolicy           |

## 3. 抽象推导索引

| 编号 | 需求组合      | 推导结论                                                                                                                                           | 结论落点                                                                                                                                 |
| ---- | ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| D1   | R1 + R2 + R3  | 系统不能只提供私有 RPC 文件服务；必须有 Object Namespace、FS Provider、ObjectHandle 和 MemoryObject。                                              | [07-data-and-filesystem.md](./core/07-data-and-filesystem.md), [03-pager-and-memory.md](./core/03-pager-and-memory.md)                   |
| D2   | R1 + R2 + R8  | 系统不能内置 POSIX VFS 作为原生模型；需要 Object/Provider/Capability VFS-like 层，POSIX 由兼容域投影。                                             | [07-data-and-filesystem.md](./core/07-data-and-filesystem.md), [01-compatibility.md](./topics/01-compatibility.md)                       |
| D3   | R3 + R4 + R9  | 远程 FS 的 mmap 必须落成本地 Remote-backed MemoryObject；CPU fault 不能直接等价为任意远程 RPC。                                                    | [03-pager-and-memory.md](./core/03-pager-and-memory.md), [07-data-and-filesystem.md](./core/07-data-and-filesystem.md)                   |
| D4   | R4 + R6 + R7  | zero-copy 不是裸共享内存；必须由 Capability 授权 MemoryDescriptor、IOBuffer、SharedQueue 和 IOMMU 映射。                                           | [04-driver-and-kernel.md](./core/04-driver-and-kernel.md), [02-communication-fabric.md](./core/02-communication-fabric.md)               |
| D5   | R5 + R6       | Communication Fabric 不能只是 Future 框架，也不能只是阻塞 IPC；必须同时有 Portal fast call、Operation、Continuation、EventPort 和 bypass session。 | [02-communication-fabric.md](./core/02-communication-fabric.md)                                                                          |
| D6   | R7 + R8       | 兼容域不能绕过原生权限模型；兼容域网关必须把 POSIX 资源翻译为受 Capability 约束的原生对象，并按能力类别处理内核硬撤销、租约或失效通知。            | [01-compatibility.md](./topics/01-compatibility.md), [01-capsule-and-capability.md](./core/01-capsule-and-capability.md)                 |
| D7   | R10 + R7      | Package Cell 的依赖、环境和服务暴露必须生成可审计的能力请求，而不是修改全局 PATH 或 profile。                                                      | [08-package-cell.md](./core/08-package-cell.md), [04-environment-and-config.md](./topics/04-environment-and-config.md)                   |
| D8   | R7 + R11      | 身份不能直接等于运行时权限；去中心化 Identity 只证明主体，授权结果必须落成 Capability、租约、策略或密钥解封装权限。                                | [05-identity-and-accounts.md](./topics/05-identity-and-accounts.md), [01-capsule-and-capability.md](./core/01-capsule-and-capability.md) |
| D9   | R1 + R2 + R11 | 加密 FS 的离线可读性必须由 FSKeyPolicy 决定；可迁移加密 FS 需要 WrappedKey 和 Key Agent 解封装，再通过 ProviderRoot 挂入 tree view。               | [05-identity-and-accounts.md](./topics/05-identity-and-accounts.md), [07-data-and-filesystem.md](./core/07-data-and-filesystem.md)       |

## 4. 维护规则

新增需求时：

1. 在本文增加新的 `R#`，写清验收条件和主线承接。
2. 如果需求会改变抽象边界，增加或更新 `D#` 推导。
3. 如果推导结论已经被接受，把稳定契约写入对应 `core/` 主设计。
4. 如果只是外部参考、论证、性能比较或候选方案，放入 `notes/reference/` 或 `notes/analysis/`，不要塞回 `target.md`。
5. 更新 [06-roadmap.md](./topics/06-roadmap.md) 中的 phase 到需求编号映射。

`target.md` 应保持可读入口，不承担完整需求库职责。
