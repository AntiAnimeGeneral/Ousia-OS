# 04 — 服务图与 Bootstrap

> 对应 `target.md` §3.4

## 讨论范围

Service Graph 取代了 Unix 的全局文件树和全局命名空间。本文讨论它的结构、名字解析、版本协商，以及最关键的 bootstrap 问题。

---

## 为什么用服务图替代文件树

### 文件树的问题

Unix 的全局文件树作为"系统命名空间"有几个根本缺陷：

1. **一切都是路径**：配置文件（`/etc/nginx/nginx.conf`）、设备（`/dev/nvme0`）、运行时状态（`/proc/1234/status`）、IPC 端点（`/var/run/docker.sock`）都在同一棵树下
2. **路径即身份**：重命名文件 = 改变其身份
3. **树结构 = 人为层级**：`/usr/bin/` vs `/usr/local/bin/` vs `/opt/` vs `~/.local/bin/` 是历史演化的结果，不是理性设计
4. **全局可见**：任何进程（默认）都能看到整棵树

### Service Graph 的结构

```
    ┌─────────────────────────────────────────┐
    │           Service Graph                  │
    │                                          │
    │  ┌──────────┐    ┌──────────┐           │
    │  │ 名字服务  │◄──►│ 版本协商 │           │
    │  └──────────┘    └──────────┘           │
    │       │                                  │
    │  ┌────┼──────────────────────┐          │
    │  ▼    ▼                      ▼          │
    │ ┌──────┐  ┌──────┐  ┌──────────────┐   │
    │ │存储   │  │网络   │  │ 窗口系统     │   │
    │ │服务   │  │服务   │  │              │   │
    │ └──┬───┘  └──────┘  └──────────────┘   │
    │    │                                     │
    │    ├── 版本 1 (active)                   │
    │    ├── 版本 2 (standby)                  │
    │    └── 版本 3 (draining)                 │
    └─────────────────────────────────────────┘
```

关键特性：

- 服务的身份是名字，不是路径
- 一个名字可以有多个版本同时存在
- 服务发现返回能力句柄，不是路径字符串
- 服务之间的连接是能力句柄传递，不是 socket 路径约定

---

## Bootstrap：鸡与蛋的解法

### 问题

如果一切服务都通过 Service Graph 发现，那第一个服务（名字服务本身）如何被找到？

### Ousia OS 的答案：启动句柄注入

```
内核启动
  │
  ├─ 创建初始地址空间
  ├─ 创建启动句柄集合 (Bootstrapping Handles)
  │   ├─ handle_naming_service   → (capability to naming service)
  │   ├─ handle_memory_object    → (capability to initial memory object)
  │   └─ handle_kernel_channel   → (capability to kernel IPC channel)
  │
  └─ 启动第一个用户态进程 (init / sys-bootstrap)
       │
       ├─ 通过 handle_naming_service 注册自己为 "naming"
       ├─ 通过 handle_kernel_channel 创建更多 Capsule
       └─ 启动 Capsule Manager
            │
            └─ 启动其余基础服务（通过名字服务发现）
```

名字服务是第一个用户态服务，但它不需要"发现自己"——它的身份就是内核注入的那个句柄本身。这解决了 bootstrap 的循环依赖。

### 为什么不像 Unix 那样用固定的路径？

Unix 的解法：`/sbin/init` 是硬编码路径（或通过 initramfs 搜索）。但这意味着 init 的位置是全局可见的，且路径不能变。

Ousia OS 的解法更符合能力模型：内核注入 `handle_init` 给启动进程，这个句柄指向什么由内核映像决定，不依赖路径。

---

## 名字解析流程

### 从名字到能力句柄

```
Capsule A                          Service Graph (名字服务)
    │                                      │
    │ resolve("storage-service", v1)       │
    │─────────────────────────────────────►│
    │                                      │ 查询注册表
    │                                      │ 版本协商
    │                                      │ 权限校验 (A 是否有 resolve 权?)
    │                                      │
    │ capability{storage-service, v1, READ}│
    │◄─────────────────────────────────────│
    │                                      │
    │ IPC 直接连接到 storage-service      │
    │ 使用刚获得的能力句柄                 │
```

关键点：

- A 不需要知道 storage-service 在哪个进程/哪个位置
- 返回的是能力句柄，不是路径
- 权限校验在名字解析时就已经发生（A 有没有权利"发现"这个服务？）
- 版本协商是名字服务的内建能力

### 版本协商规则

1. 请求者声明可接受的版本范围（如 `>=1.0, <2.0`）
2. 名字服务查询有哪些版本的 storage-service 在运行
3. 匹配规则：优先兼容的最新稳定版
4. 如果只有一个版本且在范围内 → 直接返回
5. 如果有多个 → 返回最新的（已在注册时标记为 `stable`）
6. 如果没有匹配 → 错误
7. 可选：如果兼容层有旧版本适配器，作为 fallback

---

## 服务注册与健康检查

### 注册

服务启动后向名字服务注册：

```
NameService.register({
    name: "storage-service",
    version: "1.2.0",
    protocol: "Ousia OS.object-store.v2",
    status: "starting",          // starting → healthy → draining → stopped
    capability: handle_to_self,  // 其他 Capsule 调用时使用的句柄
    health_endpoint: handle_to_health_check,
    metadata: {
        load: 0.3,
        capacity: "10TB",
    }
})
```

### 健康检查

名字服务不自己做健康检查——它通过 `health_endpoint` 句柄委托给服务自身或一个独立的健康检查服务。如果健康检查连续失败：

1. 标记服务为 `unhealthy`
2. 新的 resolve 请求不会被路由到此实例
3. 已有连接收到 `SERVICE_DEGRADED` 通知
4. 如果服务有 standby 实例，自动切换

---

## 与传统方案对比

| 维度      | Unix (文件树)         | DNS          | etcd/Consul     | Ousia OS Service Graph |
| --------- | --------------------- | ------------ | --------------- | ---------------------- |
| 身份      | 路径字符串            | 域名         | key             | 服务名                 |
| 发现      | PATH 搜索             | DNS 解析     | HTTP API        | 名字服务 resolve       |
| 返回      | 路径                  | IP:port      | value           | 能力句柄               |
| 权限      | 文件权限位            | 无           | 无（额外 RBAC） | 内建能力校验           |
| 版本      | 无（soname 是补丁）   | 无           | 无              | 一等支持               |
| 健康      | 无（supervisor 分离） | 无           | health check    | 内建                   |
| bootstrap | 硬编码 /sbin/init     | root servers | 静态配置        | 内核句柄注入           |

---

## 开放问题

1. **名字服务自身的故障恢复**：如果名字服务崩溃，所有服务发现都中断。如何做名字服务的高可用？（热备？重启时保留注册表状态？）
2. **循环依赖**：服务 A 依赖服务 B，B 依赖 A。在启动时如何打破？需要在声明中标记"可延迟初始化"吗？
3. **服务爆炸**：每个小功能都是一个服务 → 服务图变得非常庞大。如何控制复杂度？是否需要"服务组"聚合概念？
4. **跨设备服务发现**：如果一台设备上的应用需要发现另一台设备上的服务，Service Graph 如何扩展？（与 §4.8 去中心化账户的交互）

---

## 相关章节

- [00-philosophy.md](./00-philosophy.md) — 反"路径即身份"的哲学立场
- [02-package-cell.md](./02-package-cell.md) — Cell 如何声明服务暴露
- [03-capsule-and-capability.md](./03-capsule-and-capability.md) — 能力句柄传递
- [12-roadmap.md](./12-roadmap.md) — 名字服务在第一阶段的落地
