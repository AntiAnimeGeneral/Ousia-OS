# 03 — Shell 与交互环境

> 补充 [target.md](../target.md) 中服务图、数据对象和交互工具体验目标。

## 传统 Shell 为什么不行

POSIX shell 的根本假设与 Ousia OS 全面冲突：一切是文件路径（Ousia OS 是 OID）、程序通过 `exec` + 环境变量启动（Ousia OS 是 Capsule + 能力声明）、管道是字节流（应该是类型化记录流）、配置是 dotfile（应去文件化）、权限继承 uid/gid（应是 Capability 句柄）。

最核心的矛盾：**管道传递文本 vs 管道传递结构化数据。** `ls -l | awk '{print $5}'` 依赖文本解析，文件名有空格就出错。Ousia OS 中 `ls` 返回 `Stream<{name, oid, size, type, tags}>`，后续管道直接访问字段——不需要 `awk`/`sed`/`cut`。

## Nushell：最接近的起点

Nushell 是唯一解决了"管道不传文本"的现实 shell。`ls` 输出 table，`where`/`sort-by` 操作结构化数据，变量有类型。但它仍假设路径文件系统和 POSIX 进程模型，需要在 Ousia OS 上替换这三层。

## Ousia OS Shell 的设计

**语法**：继承 Nushell 的 pipeline + 类型系统。

- `ls /photos/ | where size > 10MB | sort-by modified desc`
- `query type=image/png size>10MB | sort-by created`
- `ls tag://vacation/`

**原生理解 Ousia OS 抽象**：

- `let storage = resolve "storage-service"` — 服务发现返回能力句柄，不是 `localhost:8080`
- `spawn --cell my-app --cap network:*.example.com --priority interactive` — 启动 Capsule 附带能力
- `cap list` / `cap request` — 查看和请求能力
- `capsule list | where status == "running"` — Capsule 状态管理

**Stream 是一等类型**：`stream.open "app-logs" | where level == "error" | follow`

**配置不是 dotfile**：Shell 只提供 `config set` / `config rollback` 这类交互入口；配置服务的事务、校验和同步模型归属 [04-environment-and-config.md](./04-environment-and-config.md)。没有 `.bashrc`。

## 实现策略

第一阶段基于 Nushell 改造：替换 FS 层（走 Object Store）、进程启动（走 Capsule 管理器）、添加 `query`/`cap`/`config` 命令、添加 Stream 类型。第二阶段开发完全原生的 Shell。Shell 不重新定义 Object Store、配置服务或 Communication Fabric，只暴露它们的交互界面。

Linux 兼容域内保留 bash/POSIX shell，作为过渡而非终点。

## 开放问题

1. Shell 是 Capsule 吗？崩溃后子 Capsule 应清理还是脱离？
2. 远程 Shell：`xsh ssh://other-device` 是否通过 Service Graph 跨设备发现？
3. POSIX 兼容模式需支持到什么程度？`make`/`gcc`/`git` 能在兼容域内正常运行即可。

## 相关章节

- [06-service-graph.md](../core/06-service-graph.md) — 服务发现
- [07-data-and-filesystem.md](../core/07-data-and-filesystem.md) — Object Store 交互
- [04-environment-and-config.md](./04-environment-and-config.md) — 配置不是文件
