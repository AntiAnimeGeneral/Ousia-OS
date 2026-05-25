# Ousia OS 术语表

本文解释 Ousia OS 文档中的项目自造术语或重新定义过的设计术语。除非特别说明，这些词不是某个现有系统的专有技术名，也不代表已经冻结的 ABI。

## 通信与 IPC

| 术语                 | 含义                                                                                                                | 备注                                                                        |
| -------------------- | ------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| Communication Fabric | Ousia OS 的统一通信基座，覆盖同步 RPC、异步请求、事件等待、共享队列、旁路数据面和跨队列同步。                       | 项目设计术语，不指代现有产品。                                              |
| Portal               | 服务入口能力。持有 Portal Capability 的 Capsule 才能向对应服务提交 Operation。                                      | 类似“服务入口 + 能力句柄”，不是 Fuchsia Channel 或 seL4 Endpoint 的同义词。 |
| Operation            | 一次请求的系统级生命周期对象，包含消息、Capability、deadline、cancel、completion target、调度上下文等。             | 同步调用和异步请求都可以表示为 Operation。                                  |
| Continuation         | 一次受限回复权，用于完成某个 Operation。支持 reply-once、超时、取消、late reply 错误和 pending quota。              | 受 seL4 SaveCaller / reply cap 启发，但是 Ousia 的抽象名。                  |
| EventPort / WaitSet  | 统一等待聚合器，可等待 Operation completion、timer、cancel、MemoryObject lost、queue event、device lost、Fence 等。 | 不是内核消息队列，而是事件等待入口。                                        |
| SharedQueue          | 受 Capability 授权的共享内存队列，用于高吞吐服务间通信或 descriptors 传递。                                         | 普通服务间的 bypass queue；设备侧对应 IOQueue。                             |
| Doorbell             | 通知消费者或设备“队列里有新工作”的触发机制。                                                                        | 可以是受控 MMIO、syscall assist 或 Event signal。                           |
| Fence                | 表示一个异步工作完成点的同步对象。                                                                                  | 不限定 GPU，适用于跨队列同步。                                              |
| Timeline             | 单调递增的 Fence 序列，用于表达多个有序完成点。                                                                     | 适合 GPU、IOQueue 和服务间批处理。                                          |

## 软件、运行与权限

| 术语                 | 含义                                                                                        | 备注                                                      |
| -------------------- | ------------------------------------------------------------------------------------------- | --------------------------------------------------------- |
| Package Cell         | Ousia OS 的软件交付单元，包含内容地址、依赖、运行环境、能力声明、服务暴露、生命周期钩子等。 | 不等同传统包、容器镜像或 Nix derivation，但吸收相关经验。 |
| Capsule              | Ousia OS 的运行隔离域，包含线程、地址空间、能力集合、资源预算和可见服务集合。               | 比传统进程更接近“带声明和能力边界的运行域”。              |
| Capability           | 不可伪造的权限对象或句柄，绑定对象和操作权限。                                              | 不是 Unix uid/gid 权限扩展。                              |
| Capability Broker    | 用于追踪跨 Capsule 能力转发、拆分和撤销通知的系统服务。                                     | 后续阶段能力，第一阶段不承诺完整级联撤销。                |
| Service Graph        | Ousia OS 的服务组织与发现模型。服务发现返回能力句柄，而不是路径或 IP。                      | 替代把系统命名全部压进文件树的做法。                      |
| Bootstrapping Handle | 内核启动第一个用户态服务时注入的初始能力句柄集合。                                          | 用于解决名字服务自身的 bootstrap 问题。                   |

## 数据、存储与内存

| 术语                        | 含义                                                                         | 备注                                                      |
| --------------------------- | ---------------------------------------------------------------------------- | --------------------------------------------------------- |
| Object Store                | Ousia OS 的原生持久对象层，使用 OID、元数据、版本、索引和关系描述数据。      | 不是完整 SQL 数据库。                                     |
| OID                         | Object ID，稳定对象标识，不依赖路径。                                        | 路径只是 OID 的命名索引之一。                             |
| NameBinding                 | 名称到 Object 或名称到名称的绑定关系。                                       | 用于统一路径引用、软/硬链接类语义。                       |
| Stream                      | 数据流动抽象，支持背压、取消、批量、优先级、多播等。                         | 不替代对象元数据、设备控制或服务发现。                    |
| Pager-backed Memory Object  | 可由用户态 Pager 供页、失效、回写并与内核 VM 协作的内存对象。                | 文件映射、共享映射和用户态 FS 的关键原语。                |
| Metadata Cache / 元数据快取 | 内核中由用户态 FS 推送维护的热路径只读缓存，如路径到 OID、OID 到基础元数据。 | 目标是让热路径查询不经过 IPC。                            |
| IOBuffer                    | 注册内存对象，面向 pin 生命周期、DMA 可达性、设备授权和零拷贝。              | 与 MemoryObject 可共享页框，但语义不同。                  |
| IOQueue                     | 面向设备或高性能数据面的提交/完成队列。                                      | 设备侧 SharedQueue，带 DMA、doorbell、irq、fence 等语义。 |

## 硬件、驱动与调度

| 术语                       | 含义                                                                                    | 备注                                           |
| -------------------------- | --------------------------------------------------------------------------------------- | ---------------------------------------------- |
| Hardware Core              | 内核中的最小可信硬件控制面，负责隔离、授权、复位、早期路径和 fast-path assist。         | 不是传统驱动层。                               |
| Device Graph               | 系统维护的硬件资源拓扑图，描述设备、function、queue、中断、电源状态及依赖。             | 用于授权和恢复编排。                           |
| Device Service             | 位于驱动之上的稳定资源服务接口层，向应用暴露 render、read、write、present 等语义。      | 厂商差异收敛在其后。                           |
| Driver Host                | 运行用户态驱动实例的隔离宿主。                                                          | 可按生命周期和信任边界共置或拆分。             |
| Driver Index               | 按设备 ID、class、ACPI 信息或能力匹配驱动 Package Cell 的索引服务。                     | 类似驱动包解析和匹配服务。                     |
| Compute Domain             | 对 CPU、GPU、NPU、DSP 等异构计算后端的统一资源描述。                                    | 包含能力、拓扑、内存、功耗和抢占粒度。         |
| Execution Class / 执行等级 | RT、INT、FG、BG、MAINT 等带语义的调度等级。                                             | 不是传统 nice 值。                             |
| Bypass Substrate           | 受治理的内核旁路数据面基座，如 SharedQueue、IOQueue、IOBuffer、Doorbell、Event、Fence。 | 旁路不是绕过权限，而是绕过逐请求 syscall/IPC。 |

## 兼容与系统镜像

| 术语                               | 含义                                                                       | 备注                                 |
| ---------------------------------- | -------------------------------------------------------------------------- | ------------------------------------ |
| Compatibility Domain / 兼容域      | 隔离运行旧生态的环境。第一阶段指类似 WSL2 的 Linux VM。                    | 不污染原生 API。                     |
| Compatibility Gateway / 兼容域网关 | 原生空间与兼容域之间的代理服务，负责文件、窗口、网络、剪贴板、设备等映射。 | 映射受 Capability 和资源预算约束。   |
| System Image                       | 内核与第 1 层关键服务组成的可签名、可验证、可 A/B 切换镜像。               | 与 Package Cell 更新机制共享信任链。 |

## 说明

术语表不是 ABI 规范。若某个术语在后续设计中被替换、拆分或合并，应同时更新相关章节和本文档，避免同一概念在不同文档中漂移。
