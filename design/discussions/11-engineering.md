# 11 — 工程化基础设施

> 对应 `target.md` §2.3 + §4.10 + §4.11 + §4.13 + §4.14

## 讨论范围

一个全新 OS 不仅需要好的设计，还需要好的工程化基础设施。本文讨论实现语言、构建系统、测试策略、内核更新模型、组件框架和硬件支持边界。

---

## 实现语言选择

### 内核：Rust

**为什么是 Rust**：

1. **Memory safety without GC**：所有权模型在编译期消除 use-after-free、double-free、buffer overflow，不需要 GC 运行时
2. **零成本抽象**：Rust 的高层抽象（enum、trait、iterator）编译后与手写 C 性能等同
3. **丰富的 unsafe 语义**：Rust 不禁止 unsafe，但要求显式标注。xos 的策略：unsafe 代码必须附带形式化理由文档
4. **已有先例**：Asterinas（Rust 内核框架）、Redox（Rust 微内核）证明了可行性
5. **类型系统适合能力模型**：能力句柄的"不可复制、不可伪造"属性与 Rust 的 move semantics 天然契合

### 用户态基础服务：Rust + WASM

- 核心系统服务（名字服务、Capsule 管理器、对象存储服务、驱动管理器等）：Rust
- 策略注入和受限扩展：WASM（WebAssembly System Interface）
  - WASM 提供沙盒化的代码执行环境
  - 适合过滤规则、观测 hook、简单转换
  - 不允许直接系统调用——通过 host 提供的受限 API

### 驱动：Rust SDK + 可选的 C

- 驱动 SDK 提供 Rust 绑定（safe wrapper over kernel IPC）
- 闭源驱动可用 C 编写，由 Driver Host 的隔离边界保护
- C 代码不进入内核地址空间

### 为什么不是 Zig / Nim / Go

| 语言 | 为什么不适合 xos 内核                             |
| ---- | ------------------------------------------------- |
| Zig  | 没有 borrow checker，memory safety 依赖运行时检查 |
| Nim  | GC 依赖，不适合内核                               |
| Go   | GC + 大 runtime，不适合微内核                     |
| C    | 没有 memory safety 保证                           |

---

## 构建系统

### 需求

- 内核（Rust + 少量汇编）
- 用户态基础服务（Rust）
- 驱动 SDK（Rust + C FFI）
- WASM 组件
- 交叉编译（host → aarch64-xos / x86_64-xos）
- 内容寻址构建（相同的输入 → 相同的输出）
- 增量构建

### 候选

- **Bazel**：Google 的构建系统，支持多语言、内容寻址、远程缓存。适合大型多语言项目。
- **Buck2**：Meta 的构建系统，类似 Bazel 但更简洁。
- **Cargo + custom build system**：Cargo 对纯 Rust 项目最好，但对 C FFI 和多语言支持较弱。

推荐：以 Cargo 管理 Rust 代码为主，用 Bazel/Buck2 做整体编排和交叉编译目标管理。

### 工具链

- **编译器**：rustc (基于 LLVM) + clang (用于 C 驱动)
- **链接器**：lld
- **调试器**：基于 LLDB 的自定义调试器（需要理解 xos 的 Capsule 模型）
- **QEMU**：第一阶段的主要运行平台（aarch64 / x86_64 模拟）

---

## 测试策略

### 四级测试金字塔

```
           ┌─────────────┐
           │  形式化模型   │  ← 关键安全路径
           │  检查        │
           ├─────────────┤
           │  驱动模拟     │  ← 录制回放
           │  测试        │
           ├─────────────┤
           │  集成测试     │  ← 用户态服务
           │              │
           ├─────────────┤
           │  单元测试     │  ← 内核逻辑
           └─────────────┘
```

### 1. 内核单元测试

```rust
// 在内核内部
#[test]
fn test_capability_derive_readonly() {
    let cap = Capability::new(obj, READ | WRITE);
    let derived = cap.derive(READ);
    assert!(derived.has_right(READ));
    assert!(!derived.has_right(WRITE));
}
```

这些测试在宿主系统上直接编译运行（不依赖硬件）。测试内核的纯逻辑部分：能力管理、调度算法、内存分配器等。

### 2. 用户态服务集成测试

在 QEMU 中运行完整的内核 + 基础服务栈，测试服务间交互：

```python
# 伪代码
def test_capsule_lifecycle():
    kernel.boot()
    naming_service = kernel.wait_for_service("naming")
    capsule_mgr = naming_service.resolve("capsule-manager")

    capsule = capsule_mgr.create(CellRef("test-app"))
    assert capsule.status == "running"

    capsule.stop()
    assert capsule.status == "stopped"
```

### 3. 驱动模拟测试

录制真实设备的 PCI 配置空间、MMIO 交互和中断行为，在测试中回放：

```
Record Mode:
  Driver → Kernel → Real Hardware  (capture all interactions → trace file)

Replay Mode:
  Test Driver → Kernel → Replay Engine (feeds trace file)
```

这对回归测试非常有价值——厂商驱动更新后，可以回放旧的交互 trace 验证行为没有退化。

### 4. 形式化模型检查

目标范围（第一阶段）：

- 能力传递不越权（capability delegation doesn't escalate rights）
- IOMMU 映射不重叠（两个设备不能 DMA 到同一块内存除非明确允许）
- 缺页处理不泄漏内存（page fault handler 不会导致内存泄漏）

工具候选：TLA+, Verus (Rust 验证工具), Creusot

---

## 内核更新模型

### A/B 启动分区

```
┌─────────────────┐     ┌─────────────────┐
│  分区 A (active) │     │  分区 B (standby)│
│                  │     │                  │
│  Kernel v1.0     │     │  Kernel v1.1     │
│  + 第1层服务     │     │  + 第1层服务     │
│  System Image    │     │  System Image    │
└─────────────────┘     └─────────────────┘
```

更新流程：

1. 下载新 System Image → 写入非活跃分区 B
2. 验证完整性和签名 → 通过 → 标记 B 为 "pending"
3. 系统重启 → bootloader 尝试启动 B
4. B 启动后运行健康检查（核心服务全部就绪）
5. 健康检查超时未通过 → bootloader 自动回退到 A
6. 健康检查通过 → B 标记为 active，A 变为 standby

### System Image 的组成

不同于传统 OS 的"内核 + initramfs"，xos 的 System Image 包含：

- 内核镜像
- 第 1 层基础服务（名字服务、Capsule 管理器、对象存储服务等）
- 初始能力配置（启动时注入哪些句柄）
- 硬件仲裁配置（IOMMU 默认策略）

这些作为一个整体原子更新，因为第 1 层服务和内核之间的接口（Pager 协议、能力 ABI）是紧密耦合的。

---

## 组件框架

### 设计目标

xos 的用户态服务、驱动、策略模块不应该各自为政。它们需要一个统一的组件模型。

参考 Fuchsia 的 Component Framework，xos 需要：

- **生命周期管理**：create → start → stop → destroy
- **能力声明**：组件声明它需要什么能力和暴露什么能力
- **资源管理**：每个组件有 CPU/内存/IO 预算
- **热更新**：组件可以替换而不重启系统
- **依赖解析**：组件之间的启动顺序由依赖图决定

### WASM 的用途

WASM 适合以下场景：

- 策略注入（如"当 CPU 温度 >80°C 时限制 BG 任务"）
- 数据过滤和转换（如日志管道中的过滤规则）
- 安全扩展（用户编写的插件不会逃逸沙盒）

不适合：

- 高性能 IO 路径
- 需要直接访问硬件的场景
- 复杂的系统服务

---

## 硬件支持边界

### 第一阶段明确支持

| 架构    | 要求                                                  |
| ------- | ----------------------------------------------------- |
| AArch64 | ARMv8.1+, UEFI, GICv3+, SMMUv3+                       |
| x86-64  | x86-64-v3 (Haswell+), UEFI, xAPIC/X2APIC, VT-d/AMD-Vi |

### 第一阶段明确不支持

- BIOS/Legacy 启动（只支持 UEFI）
- 32 位 x86 或 ARMv7
- 无 IOMMU/SMMU 的系统
- 传统 PCI 不带 ACS 的设备
- MBR 分区表（只支持 GPT）

---

## 开放问题

1. **自举编译器**：xos 什么时候能在自己上面编译自己的内核？这是衡量系统成熟度的经典指标。
2. **调试体验**：一个崩溃的 Capsule 如何被调试？需要什么级别的调试信息（core dump? live debugger?）？
3. **WASM 运行时选择**：Wasmtime? Wasmer? 自研轻量运行时？
4. **System Image 的签名信任链**：bootloader 如何验证内核签名？需要安全启动（如 UEFI Secure Boot）吗？

---

## 相关章节

- [08-driver-and-kernel.md](./08-driver-and-kernel.md) — 驱动 SDK 和 ABI
- [12-roadmap.md](./12-roadmap.md) — 工程化基础设施的落地顺序
- [02-package-cell.md](./02-package-cell.md) — Cell 与 System Image 的更新共享签名链
