# CM-00 Context / Memory 可复核基线

> 快照日期：2026-09-14。本文是 `CM-00` 的 source-only 基线，不是运行时验收，也不改变产品行为。
> 验收测试 `context_memory_baseline_is_reproducible`、`reference_inventory_covers_all_directories` 已落地 `kiana-query/tests/context_memory_baseline.rs`，运行时回执由 GitHub CI 负责。
> 调研基准：`docs/roadmap/context-memory.md` §24.2 的观察固定在 `db77c24 + 2026-09-12 WIP`；本快照为 `e098cc8`，全部关键文件 hash 已复核。
> 2026-10-16 重新钉扎：WIP merge 后按下表当前工作树的 SHA-256 重新钉扎，并与验收测试的期望值同步；
> 原 source snapshot 提交仅作历史参照。CI checkout 不含 `reference/` 时 inventory 测试会跳过目录比对，
> 但仍校验固定清单长度；`reference/` 存在时照常做完整比对。
> 2026-10-17 重新钉扎：CAP-19 server-owned memory scope 落地后只重钉 `harness_memory.rs` 一行 SHA-256，
> 并与验收测试期望同步；下表其余哈希未变。
> 2026-10-17（CM-12）重新钉扎：unified retrieval 落地后 `memory_retrieval.rs` 把排名委托给 Domain
> `rank_retrieval()`，重钉该行 SHA-256；同时补钉 `harness_memory.rs`（`0cc42418` 对 H17/CAP-19 daemon
> 源码套 rustfmt 后遗留的过期钉扎，非 CM-12 行为改动）。下表其余哈希未变。
> 2026-10-17（CM-14）重新钉扎：检索结果 provenance/health 落地后 `memory_retrieval.rs`（候选来源证据、
> 结果 health、显式重试边界）与 `harness_memory.rs`（快照标签与 `retrieval_health` 输出、同一次检索
> 共用时间基准）重新钉扎两行 SHA-256，并与验收测试期望同步；下表其余哈希未变。
> 2026-10-17（CM-14 合入 master）重新钉扎：CM-14 分支与 master 侧的 PD-17 夹具合并后，
> `harness_memory.rs` 同时携带 CM-14 的检索 health 接线与 PD-17 的 in-module 拒伪造夹具，
> 与两条分支各自的钉扎都不一致，故重钉该行 SHA-256 并与验收测试期望同步；下表其余哈希未变。
> 2026-10-17（H22）重新钉扎：working-state compaction 落地后只重钉 `compact.rs` 一行 SHA-256，
> 并与验收测试期望同步；§2/§3 的压缩行按 H22 现状加注，下表其余哈希未变。

## 1. 快照

| 项目 | 记录 |
|---|---|
| source snapshot | `e098cc8fcb84bcb2b062f07f054bcbbe02fc1f6d`（P4-J7-04 收口提交；2026-10-16 按 WIP merge 后工作树重新钉扎下表哈希） |
| worktree 基线 | `master`，工作树干净；基线测试 + 本文件 + 账本回填是本步提交 |
| 验收测试 | `kiana-query/tests/context_memory_baseline.rs`：12 个入口文件 SHA-256 快照 + 72 目录 reference inventory，漂移即红 |

| 边界 | 文件 | SHA-256 |
|---|---|---|
| prompt 合同 | `kiana-domain/src/prompts.rs` | `68d6c8136a08e36b01fc299215d3906bf0dc6c68795e56ee0e546fc3bb34f69a` |
| memory 记录 | `kiana-domain/src/memory.rs` | `7664fd2c0675cf59ff958f75407eb272a30f1bd2c768d1f4594162b140a4d157` |
| 上下文索引 | `kiana-query/src/index.rs` | `0515f9a2cce54f3d99edac018f3dc079bfe6d907e0523a365cd42cd44647d56c` |
| repo map | `kiana-query/src/repo_map.rs` | `25e3fe3130ca30ab5e73567c4f0903a24210af1f0a774b1bfb597ff59260fa14` |
| context 命令（core） | `kiana-core/src/context_query.rs` | `e4865d44418b195f5fabe8a24d1d9f846f356b87acd2a5bbcdd0215c911282e9` |
| context handler（daemon） | `kiana-daemon/src/context_query.rs` | `f862a66b00ef8db99ac4c010c65bbc42cbc689944a5e546f87da67fe7025b6f7` |
| memory handler | `kiana-daemon/src/harness_memory.rs` | `d92608513bc2562fefc5c1a00482c94511212449d885be08b9d93f2e64a3926f` |
| memory 检索 | `kiana-daemon/src/memory_retrieval.rs` | `06dfbbab7f45cc94826279876e3d97a008fb42b00f154855e8bd2b0ff68a36f9` |
| 记忆提案 | `kiana-core/src/memory_proposals.rs` | `5f0a041c09235c80c29f097c5afbe6200d00615e8706020a425478cbb648daa7` |
| 记忆蒸馏 | `kiana-core/src/memory_distillation.rs` | `1e201441f411e96a978b12338ecdb8459553bd4ced6e7d6e32930d17cf11768d` |
| 上下文压缩 | `kiana-runner/src/compact.rs` | `595f2385fe1602c0300ce14c60311532166774af4776fdce76eac107bcc293db` |
| 数据治理 | `kiana-daemon/src/data_governance.rs` | `b701f7e5ba539addedb89e824488da3cd62fca23af449da26d53043d76265fe4` |

reference inventory：72 个非隐藏项目目录 + 隐藏 `.claude-flow` + 4 个顶层文件（`agentdb.rvf`、`agentdb.rvf.lock`、`ruvector.db`、`COMPANYOS-REFERENCES.md`，只登记不计入）。与专项 §24.3 一致；完整清单编在验收测试里。

## 2. 关键入口（source-indexed）

| 链 | 入口 |
|---|---|
| Prompt | `prompts.rs:1-15` PromptAuthority{Product,Context}；`:16-29` PromptSection（fnv1a64 prompt_hash）；`:40-48` PromptBundle（`kiana.prompt-bundle.v1`）；`:85-104` system_prompt()/context_prompt() 按 authority 分离渲染；`:113-159` TokenBudget（UTF-8 字节 + framing 常量，非 provider tokenizer） |
| Context 命令 | `kiana-core/src/context_query.rs:4-39` `context.query.v1` 命令路由；`:73-228` 9 读操作（ReadOnly）+ 4 写操作（LocalWrite：index/artifacts/artifact_store cache write + artifact_ingest write）分野；daemon `context_query.rs:36-71` 注册 RepoMap/ReadQuery/Materialization 三组 handler，重活走 `spawn_blocking` |
| Memory 写入 | `harness_memory.rs:54-71` 注册 memory.search/write/review；`:201-302` 模型写入固定 `origin=model`、Candidate/Draft、supersedes=None；scratch 即时可见（`.kiana/memory/instance/{session}.jsonl`） |
| Memory 检索 | `memory_retrieval.rs`（CM-12 起把排名委托给 Domain `rank_retrieval()`：exact/BM25-CJK → 可选 dense → RRF60 → MMR）负责 dense 模型加载、manifest/hash 校验与降级原因；dense 需要 `KIANA_MEMORY_EMBEDDING_MANIFEST` 指向 `token-vectors` 格式 manifest（求和归一化，无 ONNX）；缺 manifest 降级 sparse |
| 蒸馏 | `memory_distillation.rs` 终态排队、CAS claim、内部模型 run、引文校验；`memory_proposals.rs` 人工接受候选 |
| 压缩 | `compact.rs:142-144` 保留 system + 最近 user，丢弃 assistant/tool 历史，放 `(no summary available)` 占位（H22 起改为 plan/generate/validate：最新完整组、最新目标与未决 assistant/tool 对保留，摘要由准入模型生成并绑定服务端权威证据，不再有本地占位摘要） |

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
13. 压缩产出占位符而非真实摘要；源 cursor/CAS 与删除失效缺失。（占位摘要已由 H22 解决：摘要来自准入模型且只认服务端权威证据；源 cursor/CAS 与删除失效仍属 H23，缺口后半段成立。）
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
