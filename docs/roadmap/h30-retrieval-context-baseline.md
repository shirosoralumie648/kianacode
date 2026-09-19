# H30 检索、Memory 与代码索引进入同一 ContextPlan 基线

> 快照日期：2026-09-19。本页记录 H30 的 source slice 与 CI-only 夹具；本地不运行测试，GitHub Actions 负责运行域、Query adapter 和 Core source guard。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H30`](harness.md#step-h30) |
| feature_status | `implemented`（domain retrieval contract/query adapter/context-plan source；CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| authority | RetrievalCandidate/Pack only normalize provenance and ACL metadata; existing ContextPlan and ControlPlane/Broker remain the single admission path |
| this step does | Memory、RepoMap、CodeSearch、Artifact 共用 source snapshot/freshness/evidence/permission/ACL/relevance candidate；RetrievalPack 用 data epoch/query digest 去重并统一送入同一 `ContextPlan::compile`；repo map adapter 只做 read-only normalization |
| this step does not | 不建设第二向量库或第二 ACL/Memory 准入，不把 candidate/lesson 自动升级为 Product/Company fact；实际 provider retrieval、增量索引、撤销传播和 durable ContextIndex 仍留后续 CM/PD 工作 |

## 1. Contract

`RetrievalCandidate` 必须绑定正确 SourceKind、SourceSnapshot、permission scope、ACL digest、
freshness、evidence refs 和 relevance。LessonCandidate 没有 evidence 直接拒绝。`as_context_candidate`
只产生 `PromptAuthority::Context`；`RetrievalPack::compile_context` 只调用既有
`ContextPlan::compile`，所以选材、预算、Product/Context 分层仍由单一编译器负责。

Stale/Unknown freshness 会显式进入候选文本标签和 source snapshot，不伪造 current；data epoch
和 query digest 绑定 pack，foreign/duplicate/tampered candidates 在 plan 前拒绝。ACL 只是候选
准入条件，不授予文件、Memory 或工具访问权。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `memory_repo_code_and_artifact_candidates_share_one_context_plan` | four source kinds normalize into one lower-trust ContextPlan |
| `stale_snapshot_and_acl_are_visible_and_lesson_cannot_self_promote` | stale status is visible and evidence-less lesson cannot enter the pack |
| `forged_kind_duplicate_or_tampered_acl_is_rejected` | source kind, duplicate ID and ACL/digest drift fail closed |
| `repo_map_results_enter_the_shared_context_candidate_contract` | read-only repo map output carries source/ACL/freshness metadata before ContextPlan |

## 3. Proof ceiling and handoff

H30 proof ceiling is `source`: common candidate/pack shape, provenance, ACL/freshness fences and
single ContextPlan compilation are established. Live retrieval quality, durable index generation,
rename/delete invalidation, vector provider behavior, cross-process cache and external freshness
proof remain CM-07+ / PD / provider work.
