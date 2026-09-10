# CompanyOS 分层记忆架构设计（融合 mem0 / ruflo / RAG）

> 状态：待用户评审
> 日期：2026-09-10
> 性质：子系统设计文档。约束全部来自 canonical 文档，本文不推翻、只具体化。
> 规范来源：`COMPANY.md` §3/§4/§7；`company-os-spec-index.md` §4.4、§6.2；`company-os-implementation-outline.md` §Slice J3。
> 落点单元：roadmap `P1-J3-01`/`P1-J3-02`（已存在）、`P1-J3-03`（新）、`P1-J3-04`（新）、`P2-K7-01`（已存在）、`P4-E-03`（已存在）、`P4-J3-05`（新）。

## 0. 已拍板的决定

| # | 决定 | 说明 |
|---|---|---|
| D1 | 检索**直接 hybrid**（BM25 + 本地向量，RRF 融合 + MMR） | 用户选择，越过"词项先行"的保守路线；代价由 §11 的降级设计兜住 |
| D2 | **写入时自动抽取**（turn 结束，LLM 抽事实） | 用户选择；与「模型不能自批」的调和是三档准入（§7） |
| D3 | spec **全阶段详设**（P1 闭环 + P4 扩展） | P4 部分仍标概要级，实现前允许按当时代码事实修订 |
| D4 | 存储**保持 JSONL append-only**，不换 SQLite | 索引一律是可重建的派生物 |
| D5 | 新增 roadmap 单元 `P1-J3-03` / `P1-J3-04` / `P4-J3-05` | 用户确认编号方案 |

## 1. 目标与非目标

**目标**：把现有"分层 JSONL + grants ACL + AND 词项匹配"的记忆骨架，升级为一套完整的 CompanyOS 分层记忆：六层存储不变，检索达到 hybrid 质量，写入带自动事实抽取，命中全程可问责，晋升/delete 有审计，P4 接 ReasoningBank 式蒸馏。

**非目标**：图检索（multi-hop）实现（只留 triple 槽位）；分布式/多机同步；RL 训练；换存储引擎；新增模型可见工具（保持 5 个）；网络 embedding API（模型必须本地文件）。

## 2. 现状基线（2026-09-10 代码事实）

- 六层 JSONL 分文件已存在：home 侧 `company/user/user-prefs/user-private.jsonl`（`KIANA_HOME/memory/`），项目侧 `department/{id}` / `role/{id}` / `project/{project,code,docs,events}` / `instance/{session}`（`.kiana/memory/`），见 `kiana-daemon/src/harness_memory.rs:220-293`。
- `MemoryRecord`：`{schema, id, layer, collection, text, source, role_id, department_id, session_id, created_at_ms}`；写入强制 `source`。
- grants ACL 两端已接：写 `kiana-policy::memory_decision`（`allows_memory_write`），读 `requested_collections`（`allows_knowledge` / `granted_collections`，`MemoryCollection` 带 `covers()` 层级语义）。
- 检索是 AND 词项包含（`text_matches`），无打分；CLI 侧 `search_memory_records` 有 OR + 计数打分但与 harness 工具面**实现不一致**。
- `kiana-query` 有确定性 hash embedding（64 维，无语义），服务 context artifacts，不接记忆。
- `memory.search` / `memory.write` 是模型可见工具（5 个之列），经 broker 转 capability。

## 3. 总览

```text
          ┌─────────────────────────────────────────────┐
          │        六层存储（JSONL，唯一事实源）          │
          │  home: company / user / user-private         │
          │  project: department / role / project /      │
          │           instance scratch                   │
          └─────┬──────────────────────────▲────────────┘
                │ 读                        │ 写（append-only）
          ┌─────▼──────────────────────────┴────────────┐
          │   MemoryBroker（kiana-daemon，唯一入口）      │
          │   grants ACL · 密级 · 准入 · 收据 · 事件      │
          └─────┬──────────────────────────▲────────────┘
                │ memory.search             │ memory.write / 建议 / 晋升 / tombstone
        ┌───────▼────────┐          ┌───────┴─────────────────┐
        │ 检索管线        │          │ 抽取管线（turn 结束）    │
        │ BM25 + 向量     │          │ LLM 抽事实 → 建议包 →   │
        │ → RRF → MMR    │          │ candidate（三档准入 §7） │
        └───────┬────────┘          └─────────────────────────┘
                │ 命中
        ┌───────▼────────┐
        │ 派生索引（可重建）│ 内存 BM25 倒排 + 向量表（brute-force）
        └────────────────┘
```

原则：JSONL 是事实源（§5）；一切操作过 broker 进账本（§8）；模型建议 ≠ 写入生效（§7）；可复现——embedding 模型钉版本 + hash，检索参数固定，同输入同命中。

## 4. 数据模型：`kiana.memory-record.v2`

v1 字段全部保留。新增字段（方向来自 spec-index §4.4 与 COMPANY.md §4/§7，不发明新规范）：

| 字段 | 类型 | 谁填 | 用途 |
|---|---|---|---|
| `origin` | `model / hook / git / user` | **服务端派生**，不读模型传值 | 审计与信任分级（J3-01 已定） |
| `admission_state` | `candidate / qualified / ephemeral` | broker | 持久层 candidate 默认不可检索（spec-index §4.4） |
| `review_state` | `pending / approved / rejected` | 审批流 | 晋升留痕 |
| `classification` | `public / company / department / role / project / packet / user-private / scratch` | broker 按层派生 | 密级（COMPANY.md §7） |
| `kind` | `fact / decision / lesson / preference / event` | 抽取器建议、晋升时定 | 蒸馏与决议的落点类型 |
| `evidence` | `{run_id, turn_id, quote}` | 抽取器 | 可回溯到来源对话句 |
| `embedding` | `{model_id, dim, vector}` | 写入时 | `model_id` 钉版本；未嵌入记录仍可走词项通道 |
| `content_hash` | hash | broker | 去重（建议期拦截重复）与 UPDATE 链 |
| `supersedes` | `record_id?` | 晋升时 | UPDATE 语义：追加新版本 + 指向旧 id，**不物理改写** |
| `triple` | `{subject, predicate, object}?` | 抽取器（可选） | 图检索槽位，本设计不实现检索 |

兼容：v1 记录缺省字段按默认值解释——`admission_state=qualified`（存量已在检索池，不突然收紧）；`origin` / `evidence` **缺失即 provenance 不可指认**，按 §8 规则不得被 Reviewer 引用为「已验证」。append-only 追加无需迁移。schema 注册走 `P0-A-01b`。

## 5. 存储引擎与派生索引

- **事实源**：分层 JSONL，append-only；删除 = tombstone 追加（§10）。
- **派生索引**：进程内，启动时从 JSONL 全量重建；词项倒排表（BM25 用）+ 向量表。索引损坏即弃用重建，无事实损失。
- **并发**：broker 是唯一写入方（ControlPlane 中介，与现状一致）；无跨进程写。
- **向量检索用精确 brute-force cosine**——单项目记忆量级为千~万条，精确检索确定且足够快。`AnnIndex` trait 留槽位，数据量超过 10 万再引入 HNSW（届时参考 `reference/agentdb.rvf` 的选型）。
- **embedding 模型**：本地 ONNX（ort / fastembed-rs 生态，bge-small 类小模型），文件 + `{name, version, sha256, dim}` 进模型注册表；启动校验 hash，不符 fail-closed（§11）。

## 6. 检索管线

```text
MemoryQuery { text, role_id, department_id, layers?, top_k }
  1. 预过滤：requested_collections（grants ACL，现状保留，fail-closed）
  2. 双通道并行：
     稀疏：BM25（k1=1.2, b=0.75；分词 = unicode 分词 + CJK 双字组）
     稠密：query 向量 vs 候选向量 cosine（精确）
  3. RRF 融合（k=60）：score = Σ 1/(60 + rank_channel)；融合器为 trait 槽位
  4. MMR 多样性重排（λ=0.7）
  5. top_k 命中，每条带 {record_id, layer, collection, classification,
     verified, score_components{bm25_rank, dense_rank}, embedding_model_id}
  6. 命中写收据（§8）
```

- CJK 双字组必须存在：本项目记忆以中文为主，无它则 BM25 退化为子串匹配。
- 检索结果按 COMPANY.md §7：**本回合注入、下回合重查**；禁止静默拼进系统提示——注入路径必须把记忆内容标记为不可信数据（与 tool result 同级），该约束在 harness 注入点断言。
- CLI 侧与 harness 侧的检索实现**归一**为同一份（J3-02 顺手修掉现状不一致）。

## 7. 写入管线与三档准入

抽取时机：**turn 结束钩子**（harness 边界，一轮一次，不是每工具调用后）。抽取器为 LLM 结构化输出（`kiana.memory-proposal.v1`：facts[]，每条带 `kind`、`evidence{run_id, turn_id, quote}`、建议操作 `ADD/UPDATE/DELETE`、相似旧记录 top-3 由检索管线供给）。抽取失败/超时不阻塞 run——记 incident 事件，run 照常终止。

**三档准入**（调和 mem0 自动性与「模型不能自批」）：

| 档 | 层 | 抽取后行为 | 依据 |
|---|---|---|---|
| T0 自动生效 | instance scratch | 立即可检索，随 session 销毁 | 草稿层无跨会话风险 |
| T1 自动入 candidate | project / role / department / company / user | 落盘为 `candidate`（不可检索）+ 建议包进审批；**操作者或目标层 owner 批准**后转 `qualified` | spec-index §4.4；J3-01 |
| T2 仅人工 | user-private | candidate 照写；**只有人**可晋升 | 密级最高档 |

- UPDATE / DELETE 永远只是建议：批准卡展示新旧对比（`supersedes` 链）或待删记录，人点才执行。
- 建议期去重：`content_hash` 命中已有记录 → 不产生建议（或生成 NONE 类建议）。
- 审批入口复用 `P0-F-01` 的审批一等请求/应答（审批卡是记忆晋升的一种 HumanTask，不另起入口）。

## 8. 收据与问责

- 每轮检索命中 → `AgentInstance.retrieved`（COMPANY.md §4 已定义）：`[{record_id, layer, collection, classification, score_components}]` 进收据。
- Reviewer 的「已验证」结论只能引用 retrieved 中有 provenance 的条目；无法指认来源的内容不得进结论（COMPANY.md §7 原文落为断言）。
- 新事件 kinds：`memory.proposed` / `memory.promoted` / `memory.rejected` / `memory.deleted`，全部过 `redact_event_value`，走 ControlPlane 账本。`RuntimeEvent → LedgerEvent` 映射相应扩展（对齐 `P2-M5-02` 建立的保真断言风格）。

## 9. 蒸馏与部门教训（P4，概要级）

ReasoningBank 管线本地化：run 到终态 → 蒸馏器（LLM，结构化输出 `kiana.memory-distillation.v1`：lesson 候选 + verdict 成功/失败/原因）→ 部门层 candidate → 审批 → `kind=lesson` 入 Department RAG。`P4-E-03` 的 symposium 决议走同一通道（`kind=decision`，决议事件即 evidence）。triple 字段只收集不检索，图 multi-hop 后置。

## 10. 删除 / 过期 / 撤销（落点 `P2-K7-01`）

- tombstone 追加，不物理改写；索引重建跳过。
- 传播链：tombstone → 派生索引移除 → `supersedes` 链下游标记 → 收据中已引用条目标 `deleted_at`（不抹历史，标失效）。
- 过期按层配置：scratch 随 session 销毁（现状）；`planning:unreleased-debate` 保留既有"辩论不外流"语义；持久层默认不过期，expiry 是显式配置。

## 11. fail-closed 与降级

| 故障 | 行为 |
|---|---|
| embedding 模型缺失 / hash 不符 | 检索降级纯词项通道；命中带 `degraded: true` 进收据（降级可见，不静默） |
| 抽取失败 / 超时 | 本轮无建议；incident 事件；run 不受影响 |
| 索引损坏 | 弃用重建（派生物） |
| grants 解析失败 / 未知 collection | fail-closed 拒绝（现状保留） |
| 账本写失败（proposed/promoted 事件落盘失败） | 操作失败；不出现"状态变了但没记录"的中间态 |

## 12. 安全分析（记忆投毒面）

1. **模型直写持久层**：memory.write 落 candidate 且不可检索（T1/T2）；唯一例外 scratch（T0）本就随会话销毁。负向测试断言 candidate 永不出现在检索结果。
2. **提示注入经记忆回流**：记忆内容注入时标记为不可信数据，与 tool result 同级；本回合注入下回合重查，杜绝"进系统提示当永久背景"。
3. **跨主体读取**：grants ACL + 密级 + `user-private` 仅人工晋升；负向测试：无 grant 角色查询被拒（现状已有，保留断言）。
4. **审计绕过**：全部状态变化有账本事件；事件写失败即操作失败（fail-closed）。
5. **投毒晋升链**：批准卡展示 evidence 引文与相似旧记录，人看到"这条要从哪句话抽出来的"才能批——建议包缺 evidence 的实现是 bug。

## 13. 测试策略

- **fixture embedder**：确定性 hash 实现 embedding trait，CI 全量单测；产品路径真 ONNX 模型，接口同源（cassette 哲学复用）。
- **确定性断言**：钉住的模型 + 固定参数 → 同输入同命中（含 RRF/MMR 打分快照）。
- **负向**：candidate 不可检索；无 grant 拒绝；`user-private` 无自动晋升；降级路径命中带 `degraded` 标记；tombstone 后检索不可见但收据历史保留。
- **一致性**：CLI 侧与 harness 侧检索同一份实现；`text_matches` AND 语义退役有迁移断言。
- **失败注入**：抽取超时、模型 hash 不符、索引损坏重建、账本写失败。

## 14. roadmap 映射与验收

| 设计件 | 落点单元 | 验收测试名 | 状态 |
|---|---|---|---|
| grants/密级检索、命中进收据、CLI 归一 | `P1-J3-02`（已存在） | `memory_hits_respect_knowledge_grants_and_reach_the_receipt` | ⏳ |
| v2 schema 字段 + candidate 准入 + origin 派生 | `P1-J3-01`（已存在） | `model_written_memory_stays_unsearchable_until_approved` | ⏳ |
| 抽取建议包 + 三档准入 + 审批卡接线 | `P1-J3-03`（新） | `extraction_proposals_carry_evidence_and_similar_records` | ⏳ |
| embedding 基建 + hybrid 检索 + 降级 | `P1-J3-04`（新） | `hybrid_retrieval_is_deterministic_for_a_pinned_model` | ⏳ |
| tombstone 传播 | `P2-K7-01`（已存在） | `deletion_propagates_to_memory_and_index` | ⏳ |
| 蒸馏与决议入库 | `P4-E-03` + `P4-J3-05`（新） | `department_resolutions_enter_the_department_memory_layer` / `run_distillation_lands_as_lesson_candidate` | ⏳ |

依赖顺序：`P1-J3-02` → `P1-J3-04`（先归一检索面，再上 hybrid）；`P1-J3-01` → `P1-J3-03`（先有准入，再接建议包）；`P4-E-03` 追加依赖 `P1-J3-03`（决议走建议包通道，`kind=decision`）；`P4-J3-05` 依赖 `P1-J3-03`；`P2-K7-01` 依赖 `P1-J3-04` 的派生索引。

## 15. 风险

| 风险 | 缓解 |
|---|---|
| hybrid 一步到位踩到打开条件（COMPANY.md §7：黄金路径绿之前记忆系统别做大） | J3-02（词项+收据）不依赖 embedding，可先落；J3-04 的 embedding 基建排在其后，实现顺序由 roadmap 阶段约束兜底 |
| ONNX 依赖进入 Rust 构建链 | ort 特性门控（feature flag），默认关；CI 用 fixture embedder，不在 CI 装 ONNX 运行时 |
| 中文分词质量影响 BM25 | CJK 双字组 + unicode 分词先行；分词器也是 trait，可后换 jieba 类实现 |
| 抽取质量差产生噪音建议 | 建议不自动生效（T1/T2），噪音止步于审批卡；content_hash 去重 |
| spec 与实现期代码事实漂移 | P4 部分本就标概要级；P1 部分实现时以 roadmap 卡片的「验收」为准，与 spec 冲突时先改 spec 再动代码 |
