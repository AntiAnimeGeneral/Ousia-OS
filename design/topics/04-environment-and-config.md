# 04 — 环境与配置管理

> 补充 [target.md](../target.md) 中运行环境、配置服务和安装污染治理目标。
>
> 依赖解析、Package Cell 生命周期、多版本并存的权威设计见 [08-package-cell.md](../core/08-package-cell.md)。本文只讨论 Capsule 启动时环境如何生成，以及系统/用户配置为什么不应退化为 shell 初始化脚本或配置文件碎片。

## 传统环境管理的混乱

一个 Linux 进程的运行时环境由 `/etc/environment`、`~/.profile`、`~/.bashrc`、`LD_LIBRARY_PATH`、`PATH` 和父进程继承拼凑而成。结果：不可复现、隐式继承、冲突无解、卸载残留、无法审计。

## Ousia OS 的三层环境模型

**系统环境**：硬件路径、时区、locale。只读，由系统服务维护，通过 `system_env.query()` 查询，不是 `getenv()`。

**用户环境**：语言偏好、主题、默认应用。类型化声明式配置，由配置服务管理——可校验、可原子更新、可回滚、可跨设备同步。**不是 `.bashrc` 文本文件。**

**Capsule 沙盒环境**：每个 Capsule 从 Package Cell 声明独立生成。**不继承父进程、不继承 shell、不继承全局配置。**

- `env` 字段声明此 Capsule 内的环境变量
- `user_prefs: [language, theme]` 白名单方式注入用户偏好
- `system_env: [timezone]` 白名单方式注入系统环境
- `deps` 声明库依赖，精确到版本约束

## 环境冲突：隔离优于合并

不尝试合并不同来源的环境变量。每个 Capsule 从声明独立生成——应用 A 的 `LOG_LEVEL=debug` 不影响应用 B。

库依赖冲突由 Package Cell 解析和多版本并存处理。环境层只消费解析结果：两个 Capsule 需要同一个库的不同版本时，它们各自看到自己的依赖视图，不在环境变量层做合并。

用户偏好冲突：用户偏好是 advisory 的，应用可以声明不使用或自行降级。不是 `LANG=zh_CN` 全局覆盖一切。

## 动态库的环境视图

**核心立场：库 = Package Cell。** 库和应用是同一种东西的不同用途。库的安装、卸载、依赖管理、版本控制归属 [08-package-cell.md](../core/08-package-cell.md)。本文只约束运行环境如何引用这些 Cell。

| 操作       | 传统                                | Ousia OS                                         |
| ---------- | ----------------------------------- | ------------------------------------------------ |
| 安装       | `apt install libssl-dev` → 文件散落 | `pkg install libssl@3.2.1` → Cell 在内容寻址存储 |
| 卸载       | 可能残留 `.so`                      | Cell removed → GC 回收                           |
| 版本共存   | 全局只有一个大版本                  | 多版本各自独立 OID                               |
| 更新       | 全局替换，可能破坏其他应用          | 原子切换，应用可锁定旧版本                       |
| 运行时加载 | `ld.so` + `LD_LIBRARY_PATH` 搜索    | Capsule 启动时从声明生成精确加载清单             |

**dev Cell**：头文件和运行时文件拆为两个 Cell。编译时用 dev Cell，分发时只带运行时 Cell。

**消除 `LD_LIBRARY_PATH` 和 `ldconfig`**：库解析走依赖声明 + 内容地址，不走搜索路径。没有 soname 符号链接链。编译和运行引用同一个 OID，不再有版本漂移。

## Linux 兼容域中的库

Linux 兼容域（类似 WSL2 的 VM）内部保持传统 `ld.so` 模型。原生 Ousia OS 空间不受此影响。兼容层不污染原生抽象。

## 配置服务

配置不是文件——是类型化声明，由配置服务管理：

- `config user set language="zh_CN"` — 原子更新
- `config user rollback --to 2024-07-14` — 回滚
- `config apply` — 通知所有相关 Capsule 环境已变更
- 无需 `source ~/.bashrc`，无需重启终端

## 开放问题

1. 环境变更的事务性：语言变更通知是同步等待所有 Capsule 确认还是 fire-and-forget？
2. 跨设备用户环境同步冲突：离线修改后的合并策略？
3. 系统环境损坏的恢复：需要类似 A/B 分区的机制吗？

## 相关章节

- [08-package-cell.md](../core/08-package-cell.md) — Cell 的依赖声明
- [01-capsule-and-capability.md](../core/01-capsule-and-capability.md) — Capsule 沙盒边界
- [00-fs-vm.md](../deep-dives/00-fs-vm.md) — Object Store 不承担配置数据库职责
- [03-shell-and-tools.md](./03-shell-and-tools.md) — xsh 中的配置命令
