# 02 — 软件单元与依赖管理

> 对应 `target.md` §3.1 + §4.1

## 讨论范围

Package Cell 是 xos 最核心的抽象之一。本文讨论它的结构设计、依赖解析策略、生命周期管理，以及它和传统包管理器的本质区别。

---

## Package Cell 的结构设计

### 最小声明

一个 Package Cell 至少包含以下声明（JSON-like 伪格式，仅用于讨论）：

```yaml
cell:
  id: "sha256:abc123..." # 内容地址标识
  publisher: "did:xos:pubkey:..." # 发布者身份

  deps:
    "stdlib": ">=2.0, <3.0"
    "libpng": "=1.6.40"

  runtime:
    base: "xos-base-24.04" # 基础运行环境
    env:
      - name: "LOG_LEVEL"
        value: "info"
    # 注意：没有 PATH, LD_LIBRARY_PATH 等

  capabilities:
    - type: "network"
      scope: "*.example.com:443"
    - type: "filesystem:read"
      scope: "object:user-data"
    - type: "gpu:render"

  services:
    exposes:
      - name: "my-app-api"
        protocol: "xos.ipc.v1"
        version: "1.2.0"

  hooks:
    install: "cell:///hooks/install"
    activate: "cell:///hooks/activate"
    deactivate: "cell:///hooks/deactivate"
    healthcheck: "cell:///hooks/health"

  compat:
    linux: "kernel-abi-6.1" # 如果需要 Linux 兼容域
```

### 关键设计决策

#### 内容地址 vs 名称地址

使用内容地址（sha256）标识 Cell 有重要含义：

- **不可变性**：相同 ID = 相同内容，不可篡改
- **去重**：两个 Cell 依赖同一个库 → 同一个 ID → 只存储一份
- **可验证**：下载后验证哈希，不需要信任传输链路
- **CAS 存储**：系统可以用内容寻址存储（content-addressed storage）管理所有 Cell

但也有代价：对人类不友好。需要名字服务将版本号映射到内容地址。

#### 为什么没有 PATH / LD_LIBRARY_PATH

传统系统的环境变量是全局状态的典型。xos 禁止 Cell 声明依赖这些。运行时环境的生成逻辑：

1. 读取 Cell 的 `runtime.base` → 找到 base 环境快照
2. 读取所有 `deps` 的内容地址 → 挂载到 Capsule 的私有命名空间
3. 应用 `env` 中声明的环境变量（仅限该 Capsule 可见）
4. Capsule 启动后，其"文件系统视图"由这些声明拼接而成，不是全局 `/usr/lib`

---

## 依赖冲突策略

### 多版本并存：如何工作

```
Capsule A:                     Capsule B:
  deps:                          deps:
    libC: v1                       libC: v2

A 的命名空间中 libC = v1      B 的命名空间中 libC = v2
```

A 和 B 各自在自己的命名空间中看到自己声明的版本。系统存储中同时存在 libC v1 和 libC v2，互不干扰。

### 跨 Capsule 通信时的版本协商

当 A 需要调用 B 的服务时：

1. A 通过 Service Graph 发现 B
2. B 暴露了服务接口，声明了协议版本
3. Service Graph 做版本协商：检查 A 的依赖版本和 B 的接口版本是否兼容
4. 如果不兼容，Service Graph 可路由到 B 的兼容版本（如果存在），或返回错误

这比全局符号版本（如 Linux 的 soname）更精细：版本协商发生在接口/协议层面，不是二进制 ABI 层面。

### 确定性解析算法

xos 不追求 NP-complete 的通用 SAT 求解。算法概要：

1. 收集所有顶层依赖声明
2. 对每个依赖名，选择声明范围内的最新版本
3. 递归处理每个选中版本的传递依赖
4. 如果出现冲突（同名的两个不同版本被不同的传递路径需要），检查是否兼容（semver 兼容范围）
5. 兼容 → 选择最新；不兼容 → 两个版本并存

确定性的关键是：**相同输入 → 相同输出**。不依赖求解器的启发式搜索。

---

## 生命周期管理

### 状态机

```
  [downloaded] → [installed] → [active] → [inactive] → [removed]
                     ↑             |            |
                     +---[rollback]-+            |
                                  +---[upgrade]--+
```

### 原子切换

升级不是"停旧 → 装新 → 启新"，而是：

1. 下载新版本 Cell（新内容地址）
2. 预启动新版本（不接管流量/服务名）
3. 健康检查通过后，原子切换名字注册
4. 旧版本保持运行直到所有活跃连接关闭（drain）
5. 旧版本进入 inactive 状态，可被 GC

### 卸载的干净性

因为 Cell 的所有文件都在内容寻址存储中，且每个 Cell 的依赖是显式声明的：

1. 标记 Cell 为 removed
2. GC 检查哪些内容地址不再被任何 active Cell 引用
3. 删除这些内容
4. 不会残留：没有全局库目录，没有 shell 配置，没有符号链接

---

## 与传统方案对比

| 维度       | apt/dpkg            | Docker         | Nix/Guix     | xos Package Cell |
| ---------- | ------------------- | -------------- | ------------ | ---------------- |
| 依赖解析   | SAT (NP-hard)       | 无（全量快照） | 确定性       | 确定性           |
| 多版本并存 | 否                  | 是（全量）     | 是           | 是（原生）       |
| 沙盒       | 无                  | 有（opt-in）   | 有（opt-in） | 默认             |
| 能力声明   | 无                  | 无             | 无           | 有               |
| 运行环境   | 全局 PATH           | Dockerfile     | 声明式       | 声明式           |
| 卸载干净   | 经常不干净          | 是             | 是           | 是               |
| 服务编排   | systemd（独立）     | compose/swarm  | 无           | 系统原生         |
| 原子升级   | 否（dpkg 逐个文件） | 是（镜像切换） | 是           | 是               |

xos 从 Nix/Guix 借鉴了内容寻址和确定性解析；从 Docker 借鉴了环境封装思想；但把这些从附加工具提升为系统原生能力。

---

## 开放问题

1. **base 环境的版本管理**：`xos-base-24.04` 的更新如何不破坏已有 Cell？类似 NixOS 的 channel 模型？
2. **编译依赖 vs 运行依赖**：目前声明中没有区分。是否需要 `build-deps` 和 `runtime-deps`？
3. **GC 策略**：如何判断一个 Cell "不再被需要"？纯引用计数 vs 基于时间的保留策略？
4. **Cell 签名验证的信任根**：发布者公钥如何分发和验证？这需要 §4.8 的账户体系。

---

## 相关章节

- [03-capsule-and-capability.md](./03-capsule-and-capability.md) — Capsule 如何消费 Cell 的能力声明
- [04-service-graph.md](./04-service-graph.md) — 服务发现与版本协商
- [10-compatibility.md](./10-compatibility.md) — 账户与签名
- [12-roadmap.md](./12-roadmap.md) — Package Cell 原型的落地顺序
