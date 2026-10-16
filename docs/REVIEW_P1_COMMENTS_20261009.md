# P1 优先级文件注释改动审查报告

**审查时间**：2026-10-09  
**评审范围**：dispatch.rs、approvals.rs、jsonl.rs  
**标准参考**：Batch 1-2 教学级深度

---

## 文件 1：kiana-core/src/dispatch.rs

### 基本指标

| 指标 | 数值 |
|-----|------|
| 源码行数 | 1,189 |
| git diff 行数 | 303 |
| 新增注释行数 | 196（65%） |
| 新增 /// 文档注释 | 98 行 |

### 评估详情

#### 模块顶部文档：✅ 完整

派发层职责清晰：把「已授权的动作」变成一次真实的执行许可（permit）。

**关键概念**：
- 【分发的职责与流程】— 5 个工作步骤列举
- 【为什么 permit 是必须的】— 安全漏洞论证（授权被撤销后的问题）
- 【fail-closed 原则】— 不确定时拒绝，版本冲突时拒绝
- 【派发的三阶段】— 准备、执行、结果投递，逐步说明
- 【与授权系统的耦合】— Authority、Approval、Cell、Company 等关系

#### 公开 API 覆盖率：2/2 ✅

```rust
pub fn project_root_identity(root: &str) -> Result<Value, PortError>
  ├─ 【为什么不能只用路径字符串】— 目录可被改名、移动、替换
  ├─ 【Unix 上采用 device + inode 组合】— 内核给的物理身份
  └─ 33 行详细说明

pub struct JournalPermitVerifier
  ├─ 职责：验证 permit 真实存在、未过期、权限版本未变
  ├─ 【为什么要单独验证】— Core 向 Broker 提供的离线验证工具
  └─ 参数、字段完整说明
```

#### Struct 字段注释：✅ 完整

**CommittedDeliverySource**：
```rust
struct CommittedDeliverySource {
    event: RuntimeEvent,              // 执行结果提交的事件（execution.result_committed）
    receipt: CapabilityResultReceipt, // 结果的收据，包含执行状态、效果标记
    commit: CommandReceipt,           // CAS 操作的确认收据，证明提交成功
    version: AggregateVersion,        // 权限流的当前版本，用于检测并发修改
}
```

**JournalPermitVerifier**：
```rust
pub struct JournalPermitVerifier {
    events: Arc<dyn EventStorePort>, // 事件存储的引用，用于读取 execution_permit 流
}
```

#### 块注释质量：✅ 6+ 处

| 函数 | 块注释 | 内容 |
|------|-------|------|
| `committed_prompt_turn()` | 8 行 | 【为什么需要这个结构】+ 5 个验证条件 |
| `typed_invocation_identity()` | 8 行 | 【为什么需要这个结构】+ 调用身份包含内容 |
| `committed_result_payload()` | 9 行 | 结果负载包含的完整信息列表 |
| `commit_confirmed()` | 3 行 | 【CAS 语义与原子性】+ 版本检查 |
| `project_root_identity()` 中 | 6 行 | 【为什么不能只用路径字符串】 |

#### 错误处理：✅ 完整

```rust
"turn_identity_invalid"          // 5 个验证条件
"clock_untrusted"                // 时钟不可信
"invocation_identity_required"   // 为什么 Run 必须存在
"result_unknown:control_commit_unconfirmed" // CAS 冲突
```

### 当前状态：✅ 完成标准

**达成指标**：
- 模块文档 + 概念图 + 三阶段流程
- 所有公开 API 有文档
- 所有 struct 字段有解释
- 关键逻辑块都有【】标记的设计说明
- fail-closed 原则在代码中体现

**建议**：✅ 立即保留 + 提交

---

## 文件 2：kiana-core/src/approvals.rs

### 基本指标

| 指标 | 数值 |
|-----|------|
| 源码行数 | 1,376 |
| git diff 行数 | ~450 |
| 新增注释行数 | 251（56%） |
| 新增 /// 文档注释 | 96 行 |

### 评估详情

#### 模块顶部文档：✅ 完整

审批链的职责：任何有副作用的能力请求都必须经过 `authorize_and_execute`。

**关键内容**：
- 【这个文件是「唯一执行脊柱」的收窄点】— 所有路都通向同一函数
- 【一次请求在这里经历什么】— 流程图（请求 → 决策 → 执行）
- 【三条值得单独记住的性质】
  - 先记事件，后做判断
  - 拒绝路径脱敏再写进事件
  - 决策事件显式声明 effect 字段
- 【审批完整生命周期】— 4 个阶段（准备、人类决策、Run-bound 执行、直接执行）
- 【与 dispatch 的关系】— 分离授权凭证与决策记录
- 【并发与竞态问题】— 3 种场景详细分析
- 【Error 类型与含义】— 8 种 error code 列举

#### 公开 API 覆盖率：4/4 ✅

```rust
pub async fn authorize_and_execute(...)
  └─ 职责、权限检查、决策路由

pub async fn decide_approval(...)
  └─ 【快捷路由】— 最常见的使用场景

pub async fn decide_approval_with_proof(...)
  └─ 【为什么需要 proof】— request_hash + nonce 验证

pub async fn decide_approval_with_proof_and_version(...)
  └─ 【乐观并发的作用】— 版本冲突拒绝
  └─ 【流程概览】— 6 步逐步说明
```

#### 块注释质量：✅ 6+ 处

| 函数 | 块注释数 | 内容 |
|------|---------|------|
| `decide_approval_with_proof_and_version()` | 6 | 【第 1-6 步】详细步骤 |
| 内部逻辑 | 多处 | CAS 冲突、Run 状态、权限变化 |

**6 步决策流程**：
1. 【第 1 步】读取现有记录，验证 proof
2. 【第 2 步】如果已经决策过，返回重放（replay）结果
3. 【第 3 步】验证版本号合理性
4. 【第 4 步】如果是 Approve，验证权限配置没变
5. 【第 5 步】检查是否有关联的 Run 以及它的状态
6. 【第 6 步】如果是持久化的审批但没有运行时续体，拒绝

### 当前状态：✅ 完成标准

**达成指标**：
- 模块文档 + 完整生命周期 + 并发问题分析
- 所有公开 API 有详细文档
- 主要流程有逐步块注释（6 步）
- 错误情况完整列举（8 种 error code）
- 与其他模块的关系明确

**建议**：✅ 立即保留 + 提交

---

## 文件 3：kiana-eventlog/src/jsonl.rs

### 基本指标

| 指标 | 数值 |
|-----|------|
| 源码行数 | 1,201 |
| git diff 行数 | ~220 |
| 新增注释行数 | 132（60%） |
| 新增 /// 文档注释 | 6 行（**偏少**） |

### 评估详情

#### 模块顶部文档：✅ 完整

append-only 事件账本的职责和概念清晰。

**关键内容**：
- 【这个文件是「持久」这个词的落地处】— durable 等级由此决定
- 【Complete frame 的含义】— 完整写入、校验、fsync

#### 公开 API 覆盖率：⚠️ 部分（70%）

**✅ 有文档**：
- `parse_json_value_reject_duplicates()` — 3 行块注释（职责清晰）

**❌ 无文档**（**关键问题**）：
- `StrictValueSeed` struct — 无职责说明
- `StrictValueVisitor` struct — 无职责说明
- `visit_map()` 方法 — **无安全说明**（防护核心！）
- `visit_seq()` 方法 — 无递归说明

#### 关键问题：visit_map() 缺失安全说明

```rust
fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
where
    A: MapAccess<'de>,
{
    let mut object = serde_json::Map::new();
    while let Some(key) = map.next_key::<String>()? {
        if object.contains_key(&key) {
            return Err(serde::de::Error::custom("duplicate JSON object key"));  // ← 为什么？
        }
        let value = map.next_value_seed(StrictValueSeed)?;
        object.insert(key, value);
    }
    Ok(serde_json::Value::Object(object))
}
```

**缺失的注释**：
1. 【为什么拒绝重复键】— 权限日志的安全原则
2. 【与 serde_json::from_slice 的区别】— 标准库保留最后一个值（不安全）
3. 【何时会遇到重复键】— 数据篡改检测场景
4. 【对应的威胁模型】— 多读者观察不一致问题

#### Struct 字段注释：❌ 缺失

```rust
struct DiskCache {
    // 无字段注释
}

struct Inner {
    // 无字段注释
}
```

### 当前状态：⚠️ 大部分完成

**已完成** (70%)：
- 模块顶部文档完整
- `parse_json_value_reject_duplicates()` 有清晰说明
- 框架结构完整

**缺口** (30%)：
- `visit_map()` 无安全说明（这是防护核心）
- `StrictValueSeed/StrictValueVisitor` 无职责文档
- `visit_seq()` 无递归说明
- struct 字段无注释

### 补充建议

**工时**：30-40 分钟

**补充内容**：

```rust
/// 从 JSON 对象映射反序列化，拒绝重复键。
///
/// 【为什么要拒绝重复键】
/// 标准库 `serde_json::from_slice::<Value>` 对重复键采用「保留最后一个」策略。
/// 这对权限日志（append-only authority log）是致命的：不同的读者会看到不同的值，
/// 导致验证和观察之间的不一致。例如一个字段同时包含权限和取消标记，
/// 读者看到的哪一个就决定了该行为的合法性。
///
/// 【防护原理】
/// 这个 visitor 对每个对象键做逐一验证。一旦发现重复，立即返回错误。
/// 这样「要么完整解析，要么完全拒绝」——没有"中间状态"的歧义。
///
/// 【应用场景】
/// - 检测对手篡改 JSON 日志（通过重复键进行偷梁换柱）
/// - 防止日志读取时的观察差异（consistency under concurrency）
fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
```

**建议**：✅ 补充后可提交

---

## 汇总表格

| 文件 | 行数 | 新增注释 | 函数覆盖 | Struct 字段 | 块注释 | 当前状态 | 补充工时 | 建议 |
|------|------|---------|---------|-----------|-------|--------|---------|------|
| dispatch.rs | 1,189 | 196 (65%) | 2/2 (100%) | ✅ 完整 | ✅ 6+ 处 | ✅ 完成 | 0h | 立即提交 |
| approvals.rs | 1,376 | 251 (56%) | 4/4 (100%) | N/A | ✅ 6+ 处 | ✅ 完成 | 0h | 立即提交 |
| jsonl.rs | 1,201 | 132 (60%) | 部分 (70%) | ❌ 缺 | ⚠️ 缺核心 | ⚠️ 大部分 | 0.5h | 补后提交 |

---

## 按照教学级标准对标

| 标准 | dispatch.rs | approvals.rs | jsonl.rs |
|------|------------|------------|----------|
| 模块文档深度 | **教学级** ✅ | **教学级** ✅ | **教学级** ✅ |
| 函数文档覆盖 | **100%** ✅ | **100%** ✅ | **70%** ⚠️ |
| 块注释解释设计 | **6+ 处** ✅ | **6+ 处** ✅ | **0 处关键** ❌ |
| Struct 字段注释 | **完整** ✅ | **N/A** ✅ | **缺失** ❌ |
| 可读性/可维护性 | **优秀** ✅ | **优秀** ✅ | **良好，需补** ⚠️ |

---

## 最终行动建议

### Option A（激进）
现在即刻提交 dispatch.rs + approvals.rs，jsonl.rs 开独立 PR

### Option B（稳妥，推荐）
快速补充 jsonl.rs（30-40 分钟），三个文件一起提交

**推荐 Option B**，因为：
1. visit_map() 是防护核心，补充注释对理解日志安全至关重要
2. 工时极短，整体质量会更均衡
3. 一次提交三个文件，便于评审管理

---

## 验证清单

编译：✅ `cargo check --workspace` 通过  
格式：✅ `cargo fmt --all --check` 通过  
diff 检查：✅ `git diff --check` 通过
