# 06 — Pager-backed Memory Object

> 对应 `target.md` §3.7

## 讨论范围

Pager-backed Memory Object 是 Ousia OS 最关键的底层抽象——它是"用户态文件系统 + 原生虚拟内存"能否成立的支点。本文讨论它的设计、与内核 VM 的交互协议、崩溃模型，以及与传统 `mmap` 的对比。

---

## 为什么需要这个抽象

### 传统 mmap 的工作方式

```
应用调用 mmap(file, offset, size)
  → 内核在页表中建立 VMA (Virtual Memory Area)
  → 应用访问 addr → 缺页
  → 内核从文件系统 (page cache) 读取页面
  → 内核填充页表
  → 应用继续执行
```

关键：内核直接处理缺页，不需要回用户态。这是 `mmap` 高性能的来源。

### Ousia OS 的难题

Ousia OS 的文件系统在用户态。如果每次缺页都需要 RPC 到用户态存储服务，`mmap` 的性能优势就消失了。

Pager-backed Memory Object 的解决方案：**缺页处理仍然是内核驱动，但页面内容的来源是用户态 Pager 服务**，两者通过高效的共享内存协作，而不是每次缺页走 RPC。

---

## 设计细节

### Memory Object 的生命周期

```
1. 创建
   用户态存储服务调用 Kernel::create_memory_object(size, flags)
   → 内核返回 Memory Object 句柄 + Pager 通道

2. 映射
   Capsule 调用 Kernel::map_memory_object(mo_handle, addr, size, prot)
   → 内核在 Capsule 的地址空间中建立 VMA
   → 初始状态下所有页面都是"未填充"

3. 缺页处理
   Capsule 访问未填充页面
   → 内核发送 PageRequest{mo_id, offset, type: READ|WRITE} 到 Pager
   → Pager 提供页面数据（通过共享内存或直接填充）
   → 内核填充页表
   → Capsule 继续执行

4. 回写
   Capsule 修改了页面（dirty）
   → 内核在内存压力下或 Pager 请求时，发送 FlushRequest 给 Pager
   → Pager 将脏页写入存储
   → Pager 确认后，内核可以回收该页框

5. 销毁
   Pager 或 Capsule 调用 close(mo_handle)
   → 内核解除所有映射
   → 未回写的脏页→ MEMORY_OBJECT_LOST 通知给 Pager
```

### 缺页处理协议

```
Capsule                    Kernel                      Pager (用户态存储服务)
  │                          │                            │
  │ 访问 addr (未映射页)     │                            │
  │─────────────────────────►│                            │
  │                          │ PageRequest{offset, type}  │
  │                          │───────────────────────────►│
  │                          │                            │ 查找/生成页面数据
  │                          │         PageData{offset,   │
  │                          │          data_ptr, flags}  │
  │                          │◄───────────────────────────│
  │                          │ 映射 data_ptr → 物理页框   │
  │                          │ 填充页表                   │
  │  继续执行                │                            │
  │◄─────────────────────────│                            │
```

### 关键设计：共享内存快速路径

对于频繁缺页的场景（如顺序读取大文件），逐个 PageRequest → PageData 的往返开销太大。Ousia OS 提供两种优化：

**批量预取**：

```
Pager 调用 Kernel::prefetch(mo_handle, [offset1, offset2, ...], data)
→ 内核一次填充多个页表项
→ Capsule 访问这些页时不会缺页
```

**共享页池**：Pager 预先持有并可按权限映射给多个 Capsule 的物理页框集合。这里的“共享”指页框池可复用，不等于所有参与者都对同一页拥有可写共享。默认安全模型是：Pager/存储服务拥有供页和回收权限；普通 Capsule 只拿到符合映射权限的只读或私有写映射；共享页需要写入时走写保护缺页和 CoW，不能让 Capsule 直接修改 Pager 的权威数据页。DMA 场景使用 IOBuffer / registered memory 的 pin 生命周期，不把普通 Memory Object 当作设备可写缓冲区。

```
Pager 和多个 Capsule 共享一个物理页框池
Pager 直接将页面数据写入共享页框，更新页表映射
Capsule 通过 mmap 直接看到这些页面
```

这是零拷贝路径——数据从存储设备到 Pager 到 Capsule 的可见内存，不需要任何拷贝。

---

## Pager 崩溃模型

### 为什么不是"热备援"

业界有些系统（如某些分布式文件系统）支持 Pager 透明备援——一个 Pager 崩溃后，另一个无缝接管。Ousia OS 不这样做，原因：

1. **状态同步复杂**：备援 Pager 需要知道崩溃 Pager 的所有未完成缺页请求和脏页状态
2. **增加内核复杂度**：内核需要维护备援列表和切换逻辑
3. **违反 let-it-crash 原则**：恢复逻辑本身可能引入 bug

### Ousia OS 的契约

```
Pager 在超时内无响应
  → 内核判定 Pager 故障
  → 内核终结该 Pager 的所有 Memory Object 映射
  → 持有映射的 Capsule 收到 MEMORY_OBJECT_LOST 信号
  → Capsule 终止或进入错误处理路径
  → Pager 监督服务检测到 Pager 崩溃
  → 如配置了恢复策略：重启 Pager → 重新建立 Memory Object → 通知 Capsule
  → 如未配置：等待人工介入
```

### 脏页丢失的语义

Ousia OS 必须区分三种完成状态：

1. **已接收**：存储服务接收了 write 或 mmap 脏页通知，但尚未写入事务日志。
2. **已入日志**：修改进入 WAL / journal，服务崩溃后可重放，但可能尚未落到最终对象布局。
3. **已持久化确认**：`fsync`/`msync` 或等价持久化屏障返回成功，系统必须保证崩溃后可恢复。

Pager 崩溃时，可以丢失“已接收但未入日志”的修改；已经确认持久化的修改不能丢失。普通 buffered write 不应默认等价于同步落盘，否则会把吞吐成本强行压到每次写入上。mmap dirty page 的提交边界也必须由 `msync`、事务提交或对象级持久化屏障明确表达。

---

## 与传统 mmap 对比

| 维度       | 传统 mmap (Linux)   | Ousia OS Pager-backed MO     |
| ---------- | ------------------- | ---------------------------- |
| 缺页处理者 | 内核 (page cache)   | 内核 + 用户态 Pager          |
| 页缓存管理 | 内核                | 内核分配页框，Pager 管理策略 |
| 回写       | 内核 (flusher 线程) | Pager 主导                   |
| 崩溃恢复   | 内核保证一致性      | Pager 事务日志保证           |
| 零拷贝     | 是 (page cache)     | 是 (共享页池)                |
| 可定制性   | 低 (固定策略)       | 高 (Pager 可替换)            |
| 故障模式   | 内核 panic          | Pager 崩溃 → let it crash    |

---

## 内存压力下的行为

### 内核回收

当系统内存紧张时，内核需要回收页框。回收决策的层级：

1. **未修改的干净页**：直接回收（Pager 可以重新提供）
2. **已修改的脏页**：通知 Pager 回写 → 回写完成 → 回收
3. **被锁定的页**（如 DMA buffer）：不可回收
4. **最近频繁访问的页**：最后回收

内核维护页框的访问频率信息，Pager 可以提供额外的冷热分层提示（如"这个页面属于冷数据"）。

### Pager 的参与

Pager 可以主动：

- 请求内核回收特定页面（`evict(mo, offset)`）
- 标记页面为低优先级（`deprioritize(mo, [offsets])`）
- 请求内核锁定页面（`pin(mo, offset)` —— 用于 DMA 或关键元数据）

---

## 开放问题

1. **大页支持**：2MB/1GB huge page 是否可以由 Pager 统一管理？缺页处理粒度是否需要从 4KB 提升？
2. **NUMA 感知**：在多 socket 系统上，Pager 需要知道物理页框位于哪个 NUMA 节点，以优化数据放置吗？
3. **异构内存**：如果有 HBM + DDR + CXL 内存池，Pager 如何表达"把热数据放在 HBM 中"？

---

## 相关章节

- [05-data-and-filesystem.md](./05-data-and-filesystem.md) — 存储核心服务如何使用 Pager
- [07-compute-and-scheduling.md](./07-compute-and-scheduling.md) — 内存带宽调度
- [08-driver-and-kernel.md](./08-driver-and-kernel.md) — DMA 与内存注册
