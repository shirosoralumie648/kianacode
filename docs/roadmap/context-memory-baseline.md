# CM-00 Context / Memory 可复核基线

> 快照日期：2026-09-14。本文是 `CM-00` 的 source-only 基线，不是运行时验收，也不改变产品行为。
> 验收测试 `context_memory_baseline_is_reproducible`、`reference_inventory_covers_all_directories` 已落地 `kiana-query/tests/context_memory_baseline.rs`，运行时回执由 GitHub CI 负责。
> 调研基准：`docs/roadmap/context-memory.md` §24.2 的观察固定在 `db77c24 + 2026-09-12 WIP`；本快照为 `e098cc8`，全部关键文件 hash 已复核。

## 1. 快照

| 项目 | 记录 |
|---|---|
| source snapshot | `e098cc8fcb84bcb2b062f07f054bcbbe02fc1f6d`（P4-J7-04 收口提交） |
| worktree 基线 | `master`，工作树干净；基线测试 + 本文件 + 账本回填是本步提交 |
| 验收测试 | `kiana-query/tests/context_memory_baseline.rs`：12 个入口文件 SHA-256 快照 + 72 目录 reference inventory，漂移即红 |

| 边界 | 文件 | SHA-256 |
|---|---|---|
| prompt 合同 | `kiana-domain/src/prompts.rs` | `ddcf19d5f449d11fac615a0e1ebff4c25900da183e359e9285a38e4f48bb69bd` |
| memory 记录 | `kiana-domain/src/memory.rs` | `7e7330b09e582af7871b0bf6cdfb11a9c62001326bb2f21fb829cff05dadde2f` |
| 上下文索引 | `kiana-query/src/index.rs` | `a9cfc4767f24a7f790686be8068554aac7ee84c6c8d58ee6c71e78b75afc3716` |
| repo map | `kiana-query/src/repo_map.rs` | `ead92a58e5de2b391779666007b4972bc79ef0e16226e8937338310ae9e39100` |
| context 命令（core） | `kiana-core/src/context_query.rs` | `c7694ad6ced9e4b7269c7e9961efb4216861b35c32a5bdaf016e931e281b02ae` |
| context handler（daemon） | `kiana-daemon/src/context_query.rs` | `4a85812ed61f04e81eb2a16bd1c01044b08540c6457aba289282975c97ff283b` |
| memory handler | `kiana-daemon/src/harness_memory.rs` | `6379bfb054a745f07f9977cf63122107f97e3753b5e15bcfc90217d8518b953c` |
| memory 检索 | `kiana-daemon/src/memory_retrieval.rs` | `c41a0a9af561904df76ba0391dd010b384e2ff9dcaaeb95e5e2b4d3be5a3dd5b` |
| 记忆提案 | `kiana-core/src/memory_proposals.rs` | `f59cc310bc5e16960e967da4e1dd62d4fc63cfb6ef7ee69d39d3faca2578e876` |
| 记忆蒸馏 | `kiana-core/src/memory_distillation.rs` | `a06724647ee589a998c47c11f97e3c646206d4a57fa32dcfc1c108974633b0b8` |
| 上下文压缩 | `kiana-runner/src/compact.rs` | `bd021351b20331da36762ffcb55348856d5c1429833c3e703593ab5017325a4d` |
| 数据治理 | `kiana-daemon/src/data_governance.rs` | `bbecc5f6c1e11d9e09de953ea2313f1c4976ee6aa07dc4833625b23931e2ccf3` |

reference inventory：72 个非隐藏项目目录 + 隐藏 `.claude-flow` + 4 个顶层文件（`agentdb.rvf`、`agentdb.rvf.lock`、`ruvector.db`、`COMPANYOS-REFERENCES.md`，只登记不计入）。与专项 §24.3 一致；完整清单编在验收测试里。

## 2. 关键入口（source-indexed）

| 链 | 入口 |
|---|---|
| Prompt | `prompts.rs:1-15` PromptAuthority{Product,Context}；`:16-29` PromptSection（fnv1a64 prompt_hash）；`:40-48` PromptBundle（`kiana.prompt-bundle.v1`）；`:85-104` system_prompt()/context_prompt() 按 authority 分离渲染；`:113-159` TokenBudget（UTF-8 字节 + framing 常量，非 provider tokenizer） |
| Context 命令 | `kiana-core/src/context_query.rs:4-39` `context.query.v1` 命令路由；`:73-228` 9 读操作（ReadOnly）+ 4 写操作（LocalWrite：index/artifacts/artifact_store cache write + artifact_ingest write）分野；daemon `context_query.rs:36-71` 注册 RepoMap/ReadQuery/Materialization 三组 handler，重活走 `spawn_blocking` |
| Memory 写入 | `harness_memory.rs:54-71` 注册 memory.search/write/review；`:201-302` 模型写入固定 `origin=model`、Candidate/Draft、supersedes=None；scratch 即时可见（`.kiana/memory/instance/{session}.jsonl`） |
| Memory 检索 | `memory_retrieval.rs` BM25+CJK、cosine、RRF60、MMR0.7；dense 需要 `KIANA_MEMORY_EMBEDDING_MANIFEST` 指向 `token-vectors` 格式 manifest（求和归一化，无 ONNX）；缺 manifest 降级 sparse |
| 蒸馏 | `memory_distillation.rs` 终态排队、CAS claim、内部模型 run、引文校验；`memory_proposals.rs` 人工接受候选 |
| 压缩 | `compact.rs:142-144` 保留 system + 最近 user，丢弃 assistant/tool 历史，放 `(no summary available)` 占位 |

## 3. 基线缺口（29-agent survey + 24 项对抗复核，全部 still-true）

以下缺口在当前源码逐项确认成立，是 CM-01+ 的输入（**不是本步修复**）：

**Context 侧**
1. 选材清单/来源版本/生命周期：`prompts.rs` 全文件无 selection registry、无 per-source versioning、无 lifecycle 状态；`source` 是自由字符串。
2. chunk 级溯源：整个 `kiana-query/src` 无 chunk 概念（grep 0 命中）；溯源仅整文件 sha256 + snippet 行区间。
3. 总扫描上限：只有 per-file 128KiB 和 limit；无文件数/累计字节/时长上限，每次全量重扫。
4. 授权数据快照：索引结果不绑定授权决定或不可变快照；ingest 的 content-addressed 存储不等于快照；持久层自述无 fsync/跨进程锁/崩溃恢复保证。
5. 索引代际切换：cache 仅 diff 报告，无 generation ID / 版本化存储。
6. 向量检索固定 64 维确定性 hash embedding（`kiana.deterministic-hash-embedding.v1`），维度和模型标识硬编码，无可配置后端。

**Memory 侧**
7. `MemoryRecord` 无类型化 subject/logical scope（仅 layer/collection 字符串）；无敏感标签（`MemoryClassification` 由 collection layer 推导，全工作区无 "sensitivity" 命中）；无 purpose/expiry/TTL。
8. 撤销源过滤是 `source.contains` 子串匹配（三处），非精确 SourceRef；宽范围 purge + JSONL 原地截断仍在。
9. 文件写入（fsync'd JSONL append）与 capability 结果事件是两个提交点，无跨存储原子性。
10. promote/reject 不幂等：成功后重放同 revision 撞 guard 返回 Conflict 而非已提升记录。
11. `content_hash` 仅 sha256(text)：同文本不同 source/evidence 不区分。
12. 检索相关性为子串包含计数（`memory_score_match`）+ rank_records 叠加。
13. 压缩产出占位符而非真实摘要；源 cursor/CAS 与删除失效缺失。
14. 检索命中收据不区分 retrieved/selected/sent/cited。

**测试覆盖（RED 记录）**
15. `kiana-core/src/context_query.rs` 的 normalize/validate 帮助函数无单元测试（仅经 daemon 集成测试间接覆盖）；`kiana-daemon/src/context_query.rs`（1228 行）零 `cfg(test)`；confinement 帮助函数仅集成间接覆盖。
16. `PromptBundle` authority 分离无直接测试（唯一直接测试在 harness_runtime.rs:211 只测 role_id/model_profile roundtrip）。
17. `memory.rs`（183 行）无 `cfg(test)`；CJK bigram tokenizer / memory_match_terms 无专测。
18. `memory_retrieval.rs`（299 行）无内联测试；bm25/rrf/mmr/manifest 校验/降级原因全工作区无测试引用；`KIANA_MEMORY_EMBEDDING_MANIFEST` 仅源码一处出现，无测试/脚本设置。
19. **记忆蒸馏全工作区零测试**（产品路径真实可达：commands.rs:246 路由 + `KIANA_MEMORY_DISTILL_AUTO` 自动消费）。
20. ONNX 推理明确未实现（全工作区唯一 ONNX 提及是 memory_retrieval.rs:2 的「未实现」注释）；dense 仅 token-vectors JSON 求和路径。

## 4. 交接

- `CM-01/02`（来源与 scope 值对象、MemoryRecord 生命周期）以 §3.7 为直接输入。
- `CM-07/11`（代码快照与增量失效）以 §3.2-5 为输入。
- `CM-04-06`（mutation 幂等与原子批次）以 §3.9/10 为输入。
- `CM-22-24`（hybrid 检索）以 §3.12/18 为输入；ONNX 目标（§3.20）未被 hash embedding 关闭。
- `CM-18-20`（压缩）以 §3.13 为输入。
- 测试补齐归属：§3.15-19 的单元测试缺口由对应 CM 卡顺带补，不在本步扩围。
