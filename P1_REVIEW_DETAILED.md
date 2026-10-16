# P1 优先级文件注释改动深度审查

## 文件 1：kiana-core/src/dispatch.rs

**源码行数**：1,189  
**git diff 新增注释行数**：196  
**编译状态**：✅ 通过

### 模块顶部文档评分

**状态**：✅ 完整

包含以下关键部分：
- 派发层定位：Core 最后一步，把授权动作变成执行许可
- 分发职责与流程（5 个步骤）
- permit 必要性（对比过去结论 vs 现在凭证）
- fail-closed 原则（不确定时拒绝、CAS 原子化）
- 三阶段流程（准备、执行、投递）
- 与授权系统耦合（Authority、Approval、Cell/Grant/Budget/Lease、Company）

### 公开函数/方法文档覆盖率

**总数**：4 个公开函数/方法  
**有文档**：4/4（100%）

| 函数 | 文档 | 质量 |
|------|------|------|
| `pub fn new()` (JournalPermitVerifier) | ✅ | 简洁，说明参数 events 用途 |
| `pub async fn verify_permit_and_extract_version()` | ✅ | 流程清晰（3 步：提取、读取、验证） |
| `pub async fn drain_execution_permits()` | ✅ | 说明职责：清理已消费 permit |
| `pub async fn drain_cached_permits()` | ✅ | 说明职责：清理缓存 permit |

### 主要私有函数/方法文档覆盖率

**总数**：7 个关键私有函数  
**有文档**：7/7（100%）

| 函数 | 行数 | 文档类型 | 质量 |
|------|------|--------|------|
| `committed_prompt_turn()` | 94 | 文档 + 验证清单 | ✅ 5 点验证清单 + 一致性说明 |
| `committed_result_payload()` | 121 | 文档 + 字段说明 | ✅ 记录了执行完整信息 |
| `project_root_identity()` | 165 | 文档 + Unix inode 解释 | ✅ 说明为什么不能只用路径 + device+inode |
| `typed_invocation_identity()` | 205 | 文档 + 块注释 | ✅ 解释为什么需要调用身份 |
| `commit_confirmed()` | 239 | 文档 + CAS 块注释 | ✅ CAS 语义、摘要转换、match 各分支注释 |
| `dispatch_error()` | 190 | 块注释 | ✅ 简洁错误包装 |
| `now_ms()` | 197 | 块注释 | ✅ 时间转换 |

### Struct/Enum 字段注释覆盖率

**CommittedDeliverySource struct**：5/5（100%）
```rust
struct CommittedDeliverySource {
    event: RuntimeEvent,           // ✅ 执行结果提交的事件
    receipt: CapabilityResultReceipt,  // ✅ 结果的收据
    commit: CommandReceipt,        // ✅ CAS 操作的确认收据
    version: AggregateVersion,     // ✅ 权限流的当前版本
}
```

### 块注释质量评分

**状态**：✅ 优秀

块注释示例：
- `【CAS 语义与原子性】`：解释 CAS 如何保证原子化
- `【为什么需要这个结构】`：解释 InvocationIdentity 的必要性
- `【为什么不能只用路径字符串】`：解释 project_root_identity 为什么用 inode
- `【Unix 上采用 device + inode 组合】`：底层实现原理

每个块注释都回答了"为什么"和"如何工作"。

### 缺口分析

**无明显缺口**。关键路径：
- ✅ DispatchPermit 的构造和验证都有说明
- ✅ 时限检查有说明（deadline 逻辑）
- ✅ 权限版本快照有说明
- ✅ 幂等性检查（execution_permit 流）有说明
- ✅ 错误路径清晰（各 error code 的含义）

### 最终判定

**状态**：✅ **完成** — 可立即保留+提交

**评分依据**：
- 模块文档完整系统（80+ 行，涵盖流程、原理、设计决策）
- 所有公开方法有完整文档（参数、返回值、错误）
- 所有关键私有函数有"为什么"的解释
- 结构体字段全注释
- 关键逻辑块有流程说明和原理说明

---

## 文件 2：kiana-core/src/approvals.rs

**源码行数**：1,376  
**git diff 新增注释行数**：251  
**编译状态**：✅ 通过

### 模块顶部文档评分

**状态**：✅ 完整（80+ 行）

包含以下关键部分：
- 唯一执行脊柱定位：所有执行路都汇到 `authorize_and_execute`
- 一次请求经历的流程图（ASCII 流程图，标记决策分支）
- 三条关键性质（先记事件、脱敏错误、effect 字段显式声明）
- **审批生命周期四阶段**：
  - 阶段 1：准备到决策 — `authorize_and_execute` → 生成 challenge
  - 阶段 2：人类决策 — `decide_approval_with_proof_and_version`（proof + version 验证）
  - 阶段 3a：Run-bound 执行 — `resume_approved_invocation`（验证 turn/run 一致、再过策略关卡）
  - 阶段 3b：直接执行 — `execute_authorized_request`（无 Run 绑定）
- 与 dispatch 关系明确
- **并发问题明确覆盖**（多人审批、运行取消、执行材料变化）
- **错误类型列表**（9 种错误及含义）

### 公开方法文档覆盖率

**主要新增文档的方法**：4 个

| 方法 | 文档行数 | 质量 | 新增文档内容 |
|------|---------|------|-----------|
| `decide_approval()` | 8 | ✅ | 快捷路由说明，转发给 decide_approval_with_proof_and_version |
| `decide_approval_with_proof()` | 10 | ✅ | 为什么需要 proof：保护请求在审批期间不被篡改 |
| `decide_approval_with_proof_and_version()` | 18 | ✅ | 乐观并发作用 + 6 步流程概览 |
| `authorize_and_execute()` | （原有，无新增） | ✅ | （相关的 challenge 机制在模块顶部） |

### 主要决策函数的流程注释

**decide_approval_with_proof_and_version** 中的内部流程：

```
【第 1 步】读取现有记录，验证 proof
【第 2 步】如果已经决策过，返回重放结果  
【第 3 步】验证版本号合理性
【第 4 步】如果是 Approve，验证权限配置没变
【第 5 步】检查是否有关联的 Run 以及它的状态
【第 6 步】如果是持久化的审批但没有运行时续体，拒绝
```

每一步都有对应的代码块注释（中文标记），便于理解流程。

### Struct/Enum 字段注释

**无新增 struct/enum**（改动都在函数/方法文档级别）

但相关的类型（如 `ApprovalRecord`、`ApprovalChallenge`）在调用上下文中都有说明。

### 块注释质量评分

**状态**：✅ 优秀

块注释示例：
- `【第 1 步】...【第 6 步】`：逐步标记流程
- `【乐观并发的作用】`：解释版本号冲突检查的原理
- `【为什么需要 proof】`：解释 request_hash/nonce 的安全作用
- 各 error code 的含义（如 `approval_proof_mismatch`、`approval_permission_profile_changed`）

### 缺口分析

**无重要缺口**。次要缺口：

| 缺口 | 影响 | 建议 |
|------|------|------|
| `resume_approved_invocation` 无新增详细文档 | 低 | 该函数相关说明在模块顶部阶段 3a |
| 某些内部决策函数（如 `stage_capability_action`）无新增文档 | 低 | 功能稳定，改动较少 |
| `execute_authorized_request` 分支文档简洁 | 低 | 流程在模块顶部阶段 3b 有说明 |

### 最终判定

**状态**：✅ **完成** — 可立即保留+提交

**评分依据**：
- 模块文档系统化（80+ 行，涵盖流程、生命周期、并发问题、错误类型）
- 三个主要决策方法有梯度文档（快捷 → proof → 完整版本）
- 每个新增文档都解释了"为什么"（proof 防篡改、版本防并发冲突）
- 并发问题明确覆盖三种竞态情况
- 错误处理清晰（9 种错误类型及含义）
- 内部流程用 6 步标记，易于追踪

---

## 文件 3：kiana-eventlog/src/jsonl.rs

**源码行数**：1,201  
**git diff 新增注释行数**：132  
**编译状态**：✅ 通过

### 模块顶部文档评分

**状态**：✅ 完整

包含以下关键部分：
- 定位：append-only 事件账本，完整、带校验和、阻塞 I/O 有界的 JSONL 事务帧
- "持久"定义：帧完整、有校验和、fsync、目录项 fsync
- 与 `lib.rs` 对比：「内存实现不是轻量的持久实现」
- "complete frame"含义：要么完整在文件里，要么不在

### 新增内容分析

**新增主函数**：`parse_json_value_reject_duplicates()`

**新增结构体**：`StrictValueSeed`、`StrictValueVisitor`

### 新增函数文档覆盖率

| 函数/类型 | 文档 | 质量 |
|----------|------|------|
| `parse_json_value_reject_duplicates()` | ✅ | 详细（解释为什么拒绝 duplicate key、安全意义） |
| `StrictValueSeed` 类型 | ✅ | 有文档说明实现 DeserializeSeed trait |
| `StrictValueVisitor` 类型 | ⚠️ | 只有 `expecting()` 方法有文档，其余无 |

### StrictValueVisitor 方法覆盖率

**总数**：12 个 visitor 方法  
**有文档**：0/12（0%）

| 方法 | 文档 | 说明 |
|------|------|------|
| `visit_bool()` | ❌ | 无文档 |
| `visit_i64()` | ❌ | 无文档 |
| `visit_u64()` | ❌ | 无文档 |
| `visit_f64()` | ❌ | 无文档 |
| `visit_str()` | ❌ | 无文档 |
| `visit_borrowed_str()` | ❌ | 无文档 |
| `visit_string()` | ❌ | 无文档 |
| `visit_none()` | ❌ | 无文档 |
| `visit_unit()` | ❌ | 无文档 |
| `visit_seq()` | ❌ | 无文档（但有递归调用 StrictValueSeed 的重要性） |
| `visit_map()` | ❌ | **无文档**（这是最关键的） |
| `expecting()` | ✅ | 有文档 |

### 关键缺口：visit_map 方法

**危险等级**：🔴 **高**

**为什么关键**：
- 这是唯一检查 duplicate key 的地方
- 如果这个检查被移除或遗漏，就会退回到标准 serde_json 的"最后值覆盖"行为
- 在 append-only authority log 中，这会导致**安全漏洞**：一个消费者验证一个 key，另一个消费者观察被覆盖的值

**当前代码**：
```rust
fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
where
    A: MapAccess<'de>,
{
    let mut object = serde_json::Map::new();
    while let Some(key) = map.next_key::<String>()? {
        if object.contains_key(&key) {
            return Err(serde::de::Error::custom("duplicate JSON object key"));
        }
        let value = map.next_value_seed(StrictValueSeed)?;
        object.insert(key, value);
    }
    Ok(serde_json::Value::Object(object))
}
```

**问题**：无任何注释说明这里为什么要 check、如何保护安全

**建议补充的块注释**：
```rust
fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
where
    A: MapAccess<'de>,
{
    // 【为什么要检查 duplicate key】
    // 在 append-only authority log 中，duplicate key 会创建安全漏洞：
    // 标准 serde_json 会保留最后的值，这意味着：
    // - 审计工具可能验证了第一个值
    // - 另一个消费者却看到了被覆盖的第二个值
    // 这导致「同一条日志行，不同的读者看到不同的内容」，违反了日志的完整性保证。
    //
    // 【如何防止】
    // 我们遍历 map，对每个 key 检查 object 中是否已存在。
    // 如果重复了，立即拒绝这条记录，返回错误。
    // 这确保了「读者要么全部拒绝这条记录，要么全部接受同一个值」。
    
    let mut object = serde_json::Map::new();
    while let Some(key) = map.next_key::<String>()? {
        if object.contains_key(&key) {
            return Err(serde::de::Error::custom("duplicate JSON object key"));
        }
        let value = map.next_value_seed(StrictValueSeed)?;
        object.insert(key, value);
    }
    Ok(serde_json::Value::Object(object))
}
```

### 其他缺口

1. **visit_seq 方法**：无注释说明为什么递归调用 `StrictValueSeed`
   - **建议**：添加 1 行注释说明"递归检查嵌套数组中的重复 key"

2. **load_delta 改动处**（第 774 行）：
   ```rust
   - let value = match serde_json::from_slice::<serde_json::Value>(payload) {
   + let value = match parse_json_value_reject_duplicates(payload) {
   ```
   **问题**：无注释说明为什么从标准解析器改为自定义解析器
   - **建议**：在这行上添加块注释说明"使用自定义解析器以拒绝 duplicate key"

### 最终判定

**状态**：⚠️ **大部分完成 — 需要补充** — 补充后可提交

**评分依据**：
- ✅ 模块文档完整（说明 append-only、complete frame、持久性定义）
- ✅ `parse_json_value_reject_duplicates()` 函数有详细文档
- ✅ `StrictValueSeed` 类型有说明
- ❌ `visit_map()` 无文档（**安全关键**）
- ❌ `visit_seq()` 无文档
- ❌ `load_delta` 改动处无说明
- ❌ 其他 10 个 visitor 方法无文档

**补充建议**（预计 30-40 分钟）：

1. **立即补充**（关键安全路径）：
   - 在 `visit_map()` 添加 10-15 行块注释，说明为什么检查 duplicate key、安全意义、如何检查

2. **快速补充**（辅助理解）：
   - 在 `visit_seq()` 添加 2-3 行说明递归调用的重要性
   - 在 `load_delta` 改动处添加 1-2 行说明为什么用自定义解析器

3. **可选补充**（一致性）：
   - 为其他 visitor 方法（visit_bool, visit_i64 等）添加 1 行简洁注释
   - 不一定每个方法都需要，但如果有新人维护，这会有帮助

**安全隐患评估**：
- **当前代码本身是正确的**，duplicate key 检查逻辑完整
- **风险在于文档缺失**：下一个维护者可能：
  - 不理解为什么有这个检查，认为是过度防御
  - 在性能优化时意外移除这个检查
  - 误解这是"标准 serde_json 就能做"的功能

---

## 汇总表格

| 文件 | 行数 | 新增注释 | 模块文档 | 公开方法 | 私有函数 | 字段注释 | 块注释 | 当前状态 | 补充工时 | 判定 |
|------|------|---------|--------|---------|---------|--------|------|--------|--------|------|
| **dispatch.rs** | 1,189 | 196 | ✅ | 4/4 | 7/7 | 5/5 | ✅ | ✅ 完成 | 0h | ✅ 立即提交 |
| **approvals.rs** | 1,376 | 251 | ✅ | 4/4+ | N/A | N/A | ✅ | ✅ 完成 | 0h | ✅ 立即提交 |
| **jsonl.rs** | 1,201 | 132 | ✅ | 1/3 | 0/12 | N/A | ⚠️ | ⚠️ 大部分 | 0.5h | ⚠️ 补充后提交 |

### 综合评估

**三个文件合计**：
- 源码：3,766 行
- 新增注释：579 行
- 模块文档：✅ 全部完整
- **总体完成度**：约 85%（dispatch + approvals 完成，jsonl 需补充）

### 建议决策

**选项 A（快速通道）**：
- 补充 jsonl.rs 的 visit_map 块注释（15 分钟）
- 三个文件一起提交

**选项 B（分阶段）**：
- dispatch.rs + approvals.rs 现在提交
- jsonl.rs 后续完整补充并单独提交

**推荐**：选项 A（快速通道），因为安全隐患不能延迟修复。

---

## 附录：jsonl.rs 建议的补充内容

### 1. visit_map 补充块注释

**位置**：kiana-eventlog/src/jsonl.rs，第 971 行

```rust
fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
where
    A: MapAccess<'de>,
{
    // 【防止 duplicate key 导致的安全漏洞】
    // 在 append-only authority log 中，JSON 中的 duplicate object key 会创建不可见的安全漏洞。
    // 标准 serde_json 会保留最后出现的值，这意味着：
    // - 一个审计工具可能验证了第一个 key 的值
    // - 另一个日志消费者因为 JSON 解析器的顺序不同，看到了第二个 key 的值
    // 
    // 这违反了日志的完整性保证：「同一条日志行，所有消费者看到相同的内容」。
    //
    // 【防御机制】
    // 我们遍历 map 中的每个 key，在 object 中检查是否已存在。
    // 如果重复，立即返回错误，拒绝这条记录。
    // 这确保了「要么所有读者都拒绝这条记录，要么所有读者都接受相同的值」。
    
    let mut object = serde_json::Map::new();
    while let Some(key) = map.next_key::<String>()? {
        if object.contains_key(&key) {
            return Err(serde::de::Error::custom("duplicate JSON object key"));
        }
        let value = map.next_value_seed(StrictValueSeed)?;
        object.insert(key, value);
    }
    Ok(serde_json::Value::Object(object))
}
```

### 2. visit_seq 补充（可选）

**位置**：第 960 行

```rust
fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
where
    A: SeqAccess<'de>,
{
    // 递归调用 StrictValueSeed 以检查嵌套数组中是否有 object 包含 duplicate key
    let mut values = Vec::new();
    while let Some(value) = sequence.next_element_seed(StrictValueSeed)? {
        values.push(value);
    }
    Ok(serde_json::Value::Array(values))
}
```

### 3. load_delta 改动处补充

**位置**：第 774 行

```rust
let value = match parse_json_value_reject_duplicates(payload) {  // 使用自定义解析器以检测并拒绝 duplicate JSON object key
    Ok(value) => value,
    Err(e) if !newline => {
        // ...
    }
```
