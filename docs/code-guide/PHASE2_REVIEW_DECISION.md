# Batch 3 Phase 2 审查决策 — 立即可操作

**审查完成时间**：2026-10-09 02:46 UTC+8  
**审查员**：p1_workarea_review Agent  
**决策级别**：核心 P1 文件，可立即行动

---

## 🎯 三个文件的决策

### ✅ 立即 Commit（2 个文件）

#### 1. **kiana-core/src/dispatch.rs**（1,189 行）

**审查结果**：✅ **完成标准**

| 检查项 | 结果 |
|--------|------|
| 模块顶部文档 | ✅ 完整（职责、概念、fail-closed、三阶段流程） |
| 公开函数文档 | ✅ 2/2 完整 |
| Struct 字段注释 | ✅ 完整 |
| 块注释（设计说明） | ✅ 6+ 处（【CAS 语义】、【为什么需要 permit】等） |
| 新增注释行数 | +196 行（质量优秀） |

**核心亮点**：
- `【为什么 permit 是必须的】`段落解释了从"过去的结论"到"现在的凭证"的转变
- `【fail-closed 原则】`明确说明了拒绝的条件
- `【与授权系统的耦合】`清晰展示了依赖关系

**建议**：立即保留 + Commit

---

#### 2. **kiana-core/src/approvals.rs**（1,376 行）

**审查结果**：✅ **完成标准**

| 检查项 | 结果 |
|--------|------|
| 模块顶部文档 | ✅ 完整生命周期（4 个阶段 + 并发分析） |
| 公开函数文档 | ✅ 4/4 完整 |
| 块注释（流程说明） | ✅ 6+ 处 |
| Error 分类 | ✅ 8 种完整列举 |
| 新增注释行数 | +251 行（密度适中） |

**核心亮点**：
- 完整的生命周期说明（pending → decided → replayed → terminal）
- 并发问题分析清晰
- 错误分类与设计决策的对应关系明确

**建议**：立即保留 + Commit

---

### ⚠️ 补充 30-40 分钟后提交（1 个文件）

#### 3. **kiana-eventlog/src/jsonl.rs**（1,201 行）

**审查结果**：⚠️ **大部分完成，缺核心安全说明**

| 检查项 | 结果 | 说明 |
|--------|------|------|
| 模块顶部文档 | ✅ 完整 | - |
| 函数覆盖 | ⚠️ 70% | 关键函数缺说明 |
| 块注释 | ❌ 缺核心 | visit_map() 无安全说明 |
| Struct 字段 | ❌ 缺 | 需补充 |
| 新增注释行数 | +132 行 | 总体不足 |

**具体缺口**：

```rust
// ❌ 缺少的关键注释位置：

fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
// 应该有 6-8 行块注释说明：
//   1. 为什么拒绝重复键（防止 Silent 覆盖）
//   2. 与标库 serde 的区别（标库保留最后值）
//   3. 威胁模型（多读者一致性问题）
//   4. 为什么是 fail-closed 而不是 warn

// ❌ 缺少的字段说明
pub struct StrictValueSeed { }
// 应该有结构体职责说明

pub struct StrictValueVisitor { }
// 应该有访问者模式的说明

fn visit_seq<A>()
// 应该有递归调用的说明（防止理解困难）
```

**为什么这些很关键**：
- `visit_map()` 是防护核心，防止日志被静默覆盖
- 与标库区别是理解"为什么要自己实现"的关键
- 多读者一致性问题是日志系统的根本约束

**补充时间**：30-40 分钟

**补充步骤**：
1. 在 `visit_map()` 前加 6-8 行块注释
2. 在 `StrictValueSeed` 和 `StrictValueVisitor` 结构体前加类型文档
3. 在 `visit_seq()` 前说明递归
4. 运行 `cargo fmt --check` 验证

---

## 🎯 决策矩阵

### Option A：激进 — 现在提交 2 个，jsonl.rs 开独立 PR

| 优点 | 缺点 |
|------|------|
| 快速推进（即刻提交） | jsonl.rs 核心防护说明缺失 |
| dispatch + approvals 不阻塞 | 后续维护者理解日志安全困难 |
| | 与"教学级"标准不符 |

**不推荐**：核心安全说明不能遗漏

---

### Option B：稳妥 — 补充 jsonl.rs 后一起提交（推荐）

| 优点 | 缺点 |
|------|------|
| 三个文件都达到教学级标准 | 延迟 30-40 分钟 |
| 日志安全防护说明完整 | |
| 一致的质量水平 | |
| 对齐 Batch 1-2-3 的标准 | |

**推荐理由**：
- jsonl.rs 是日志系统的核心，防护说明不能妥协
- 30-40 分钟的投资对理解日志安全至关重要
- 三个文件都是 Phase 2 的重点，应该一起完成

---

## 📋 立即行动计划

### 如果选择 Option B（推荐）

#### Step 1：补充 jsonl.rs 的关键注释（30-40 min）

```bash
# 1. 打开文件
vim kiana-eventlog/src/jsonl.rs

# 2. 找到 visit_map() 函数，在 fn 前添加块注释
# （见下面的"补充模板"）

# 3. 找到 StrictValueSeed 和 StrictValueVisitor，添加类型文档

# 4. 保存并验证
cargo fmt --check -p kiana-eventlog
cargo check -p kiana-eventlog
```

**补充模板**（复制粘贴）：

```rust
// 【JSONL 键冲突防护】
// serde 标准的 map 访问会保留最后一个重复键的值（silent override）。
// 这在 JSONL 中是严重的数据完整性问题：多读者查询同一 key 可能看到不同的值。
// 本实现采用 fail-closed：任何重复键都被拒绝，迫使上层修复数据源。
// 这确保了「所有看到的数据一致性」——代价是可能拒绝某些曾经成功的解析。
// 权衡理由：日志不能无声地丢失信息，宁可显式失败。
fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
```

#### Step 2：验证 & 提交（10 min）

```bash
# 1. 格式检查
cargo fmt --all --check

# 2. 编译检查
cargo check -p kiana-eventlog

# 3. 差异审查
git diff kiana-eventlog/src/jsonl.rs | head -50

# 4. Commit 三个文件
git add kiana-core/src/dispatch.rs kiana-core/src/approvals.rs kiana-eventlog/src/jsonl.rs

git commit -m "docs: 注释 Core dispatch/approvals 与 EventLog jsonl —— 派发与日志的核心防护设计

- kiana-core/src/dispatch.rs：派发层的许可管理与 CAS 语义
  * 为什么 permit 必须存在（从「过去的结论」到「现在的凭证」）
  * fail-closed 原则贯穿全过程
  * 与授权系统的耦合（Authority/Approval/Cell/Company）

- kiana-core/src/approvals.rs：审批生命周期与并发设计
  * 四个阶段（pending/decided/replayed/terminal）
  * 并发安全和决策幂等性
  * 八种错误类型的完整分类

- kiana-eventlog/src/jsonl.rs：JSONL 序列化与键冲突防护
  * 为什么拒绝重复键（防止 Silent 覆盖）
  * 与 serde 标库的区别（fail-closed vs silent）
  * 多读者一致性问题与权衡理由

质量指标：Phase 2 全部通过教学级标准
对标参考：超过 Batch 1-2，对齐 Batch 3 Phase 1"

# 5. 验证
git status
git log --oneline -1
```

---

## 📊 最终统计

### 立即可 Commit

```
✅ Batch 1：8 个文件，4,051 行（已提交）
✅ Batch 2：15 个文件，8,059 行（已提交）
✅ Phase 1：9 个文件，2,125 行（审查 94%，待 commit）
✅ Phase 2：3 个文件，3,766 行（审查通过，待补充 + commit）

累计：35 个文件，17,001 行
```

### 工时投资

```
补充 jsonl.rs：30-40 分钟
验证 + commit：10 分钟
───────────────────
总计：40-50 分钟
```

### 收益

```
✅ 三个核心 P1 文件的教学级说明完整
✅ 日志系统的防护设计明确
✅ 派发与审批的关键概念清晰
✅ 对齐 Batch 3 的质量标准
✅ 后续维护者的理解成本大幅降低
```

---

## 🔄 后续衔接

### 补充并 Commit 后

1. **更新任务清单**（ANNOTATION_TASK_MANIFEST.md）
   ```
   Phase 2 的 4 个文件（包括 automation_dispatch.rs）
   状态从 "🔄 工作区改动" 改为 "✅ 已提交"
   ```

2. **启动 P1 关键路径**（预计 25-30 小时）
   - capabilities.rs（1,200+ 行）
   - lifecycle.rs（1,500+ 行，已部分处理）
   - harness.rs（1,800+ 行）
   - tools.rs（1,400+ 行）

3. **清理工作区**
   - Phase 1-2 全部 commit 后，工作区应只剩下 P1-P4 待处理的新文件

---

## ✅ 决策确认清单

- [ ] 理解 Phase 2 的三个文件状态
- [ ] 同意 Option B（补充 jsonl.rs 后一起提交）
- [ ] 准备时间投资（40-50 分钟）
- [ ] 按"立即行动计划"执行补充 + Commit

---

**推荐**：立即执行 Option B，在 1 小时内完成 Phase 1-2 的全部 commit

**时间表**：
- 现在 - 10min：补充 jsonl.rs
- 10-50min：验证 + 提交所有 Phase 2 文件
- 50min：工作区清洁，启动 P1 关键路径

**结果**：35 个文件（17,001 行）完全通过教学级标准 ✅

