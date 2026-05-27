# 05 — 身份与信任模型

> 补充 [target.md](../target.md) 中身份句柄、账户同步与可信发布目标。

## 为什么需要系统级身份

跨设备数据同步、软件商店和付费分发、发布者身份验证、端到端加密通信——这些都需要一个系统级的"谁是谁"的答案。不需要中心化服务器，但需要一个可移植的身份模型。

**定位**：属于第 2 层（平台服务），不是内核能力。第一阶段不实现完整系统，但内核的能力模型预留身份句柄类型的支持。

## 身份模型

```
Identity = 设备无关的标识符
  ├── 公私钥对 (ed25519)
  ├── 可验证声明 (Verifiable Claims)
  │   ├── "我是 alice@example.com" (由 example.com 签名证明)
  │   ├── "我是此设备的拥有者" (由 TPM/安全元件证明)
  │   └── "我可以发布软件包" (由发布者网络签名证明)
  └── 设备绑定
      ├── 主设备 (持有主私钥)
      └── 次级设备 (持有派生密钥，可被主设备撤销)
```

参考联邦身份、DID、硬件密钥和可验证声明，但不把链上化当成默认前提。允许中心化托管实现，但协议不绑定单一服务商。

Identity 回答"谁是这个主体"，Capability 回答"这个 Capsule 当前能做什么"。身份可以参与授权决策，但授权结果必须落成能力句柄、租约或密钥解封装权限；不能把身份本身当成全局权限。

## 账户、管理员与设备所有权

Ousia 不应复刻 Unix `root`。系统需要的是可拆分、可审计、可委托、可撤销的管理权威，而不是一个永远万能的管理员账号。

```
Device Owner / Policy Authority
  ├── DeviceOwnerCapability
  ├── SystemUpdateCapability
  ├── RecoveryCapability
  ├── PolicyAdminCapability
  ├── NamespaceAdminCapability
  └── KeyRecoveryCapability
```

这些管理能力可以由个人去中心化 Identity、组织 Identity、本地恢复密钥或硬件根共同持有。家用设备通常有一个主要 Device Owner；组织设备可以由组织 Identity 持有策略权威；维修、恢复、系统更新和 FS 解密不应默认绑定到同一个主体。

因此，Ousia 的立场是：没有传统意义上的 `root`，但有 Device Owner 和 Policy Authority。它们是高权限 Capability 的集合，不是内核内置的 uid。

## PIN、私钥与 Key Agent

快捷 PIN 码或生物识别是本地解锁因子，不是身份私钥本身。私钥应由硬件安全模块、TPM/Secure Enclave 或受保护的 Key Agent 持有，并尽量不可导出。

```
PIN / biometric
  -> 解锁本地 Key Agent
  -> Key Agent 执行受限签名或解密
  -> 私钥不离开受保护边界
```

PIN 适合批准短期、本地、可审计的操作：登录会话、安装软件、授予敏感 Capability、解锁本地 FS key、对一次身份声明做短期签名。PIN 不能单独恢复长期身份，也不能直接解密所有跨设备备份；恢复必须走多设备、恢复密钥、社交恢复或组织托管策略。

## 加密 FS 与身份解封装

加密 FS 默认应满足离线拆盘不可读。能否在另一台机器读取，取决于 FS 创建时声明的密钥策略，而不是取决于是否能绕过原 OS。

```
FSKeyPolicy =
  DeviceBound
  IdentityBound
  IdentityOrRecoveryKey
  OrganizationManaged
  Plaintext
```

FS 不应直接用 Identity 私钥加密所有数据。推荐 envelope encryption：每个 FS 有随机 FS Master Key，对象或 extent 再派生数据密钥；FS Master Key 被一个或多个 recipient 包装。

```
WrappedKey {
  recipient: identity | device | recovery_key | organization
  encrypted_fs_key: Encrypt(recipient_public_key, fs_master_key)
  policy: read | write | admin | recovery
}
```

当加密 FS 被挂载到另一台机器时，Object Namespace / FS Provider 读取 WrappedKey 列表，找到当前 Identity、设备密钥或恢复密钥可解的 recipient，通过 Key Agent 解封装 FS Master Key，再把 ProviderRoot 挂入 tree view。解密成功只说明可以打开存储；对象级访问仍要落到 ObjectHandle、Capability、lease 和审计策略。

## 对内核的影响：身份句柄

内核只新增一种能力类型——身份句柄（Identity Capability）：

```
IdentityHandle { identity_id, claims: Set<Claim>, proof: Signature }
```

Capsule 持有身份句柄，在与其他服务交互时出示。内核不实现"账户"概念——账户管理在用户态的身份服务中。

## 对 Package Cell 的影响

发布者身份绑定到 Cell 签名。系统安装时验证签名 → 检查发布者信任策略 → 安装或拒绝。

## 第一阶段只预留

- 身份句柄类型（内核能力模型）
- 发布者签名字段（Package Cell 格式）
- 信任策略配置接口
- Device Owner / Policy Authority 的能力类型预留
- Key Agent / WrappedKey / FSKeyPolicy 的元数据形状预留

**不做**：完整账户系统、完整跨设备绑定、完整密钥恢复、区块链/DID 存储、付费分发。

## 开放问题

1. 主设备丢失后的身份恢复：社交恢复？备份密钥？组织托管？
2. 发布者信任根：CA 层次还是 web of trust？
3. Device Owner 与组织 Policy Authority 冲突时，谁拥有最终恢复权？
4. FSKeyPolicy 是否允许强制组织 escrow，还是必须由 FS owner 明确选择？

## 相关章节

- [08-package-cell.md](../core/08-package-cell.md) — 发布者签名验证
- [01-capsule-and-capability.md](../core/01-capsule-and-capability.md) — 身份句柄作为能力类型
- [07-data-and-filesystem.md](../core/07-data-and-filesystem.md) — 加密 FS 与 FS Provider 挂载
