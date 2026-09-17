# P1-J3-02 分层检索与密级基线

> 快照日期：2026-09-18。本页回填 Memory ACL、相关性评分与 receipt provenance；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P1-J3-02`](../roadmap.md#step-p1-j3-02) |
| feature_status | `implemented`（domain/daemon/core memory source；CI-only retrieval guards） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | server-derived MemoryScope/RoleSpec grants and EventLog receipt projection own memory visibility; model arguments cannot widen scope |
| this step does | layered collection paths are selected from server role grants, requested collection is checked against MemoryScope and RoleSpec knowledge ACL, searchable records receive deterministic sparse/dense/RRF/MMR score metadata, and receipt folds each hit with retrieval event/request/query/role provenance |
| this step does not | 不把检索命中静默拼入 system prompt、不让未经 provenance 的命中变成 verified 结论、不引入网络 embedding；hybrid model lifecycle and durable index remain P1-J3-04/CM/PD |

## 1. Contract

The ControlPlane-derived execution scope determines principal/project/session and allowed memory
collections. The handler rejects unknown or unauthorized collections before reading files, filters
unsearchable/revoked/scratch-out-of-session records, then ranks only admitted records. Each returned
hit carries score/matched terms and bounded retrieval metadata; `receipt_from_events` adds the
source event/request so a UI or reviewer can distinguish retrieved from cited material.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `memory_hits_respect_knowledge_grants_and_reach_the_receipt` | ACL/scope checks, deterministic scoring and receipt retrieval provenance are all on the owned path |
| `unauthorized_memory_collection_is_rejected_before_read` | user-private/unreleased collections are rejected before adapter reads by both scope and role grants |
| `builder_project_search_hits_land_on_receipt` | existing daemon runtime regression returns a scored project hit in the completed receipt |

## 3. Proof ceiling and handoff

P1-J3-02 proof ceiling is `source` plus CI fixtures: layered ACL, ranking metadata, revoked/searchable
filters and receipt provenance are explicit. Exact hybrid model pinning/health, durable index/cache,
retrieval-to-context selection and deletion propagation remain P1-J3-04/CM/PD/SC work.
