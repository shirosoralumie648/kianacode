# P1-J3-03 抽取建议包与三档准入基线

> 快照日期：2026-09-18。本页回填 Memory distillation/proposal/evidence 与 Candidate→Draft/Qualified/Rejected；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P1-J3-03`](../roadmap.md#step-p1-j3-03) |
| feature_status | `implemented`（domain/core/daemon memory proposal source；CI-only proposal fixtures） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | EventLog distillation/proposal facts and human-approved memory.review own admission; model output is candidate evidence only |
| this step does | terminal/meeting output queues bounded distillation, strict `MemoryProposal` carries ADD/UPDATE/DELETE facts, evidence quotes and up to three similar records, extraction errors emit incidents without changing the source run, persistent records remain Candidate/Draft until review, approved records become Qualified/Active, rejected records stay invisible, and scratch is the explicit ephemeral exception |
| this step does not | 不让模型直接晋升 memory、不接受缺 evidence/target 的 update/delete、不把相似记录当作证明、不另起抽取模型 loop；durable cross-process projector and hybrid index remain CM/PD/P1-J3-04 |

## 1. Contract

Distillation is queued from committed source events and claimed through the existing bounded
ControlPlane memory-distillation run. Output validation requires evidence quotes to match source
material, caps facts/evidence/similar records, and creates a Candidate proposal. The `memory.review`
capability is the only path that materializes a persistent Qualified/Active or Rejected record;
acceptance binds collection/project/role and target revision. Extraction failure records an incident
and never rewrites the run's terminal outcome.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `extraction_proposals_carry_evidence_and_similar_records` | strict proposal facts require evidence and cap similar records at top-3 |
| `proposal_operations_require_targets_except_add` | UPDATE/DELETE require an explicit target record while ADD may create a new candidate |
| `extraction_proposals_are_evidence_bound_and_three_tiered` | queue/output/review source uses evidence-bound proposal and Candidate/Draft→Qualified/Active/Rejected tiers |

## 3. Proof ceiling and handoff

P1-J3-03 proof ceiling is `source` plus CI domain fixtures: evidence-bound proposal schema,
similar-record cap, extraction incident isolation and three-tier admission are explicit. Full
provider-generated durable extraction behavior, cross-process queue/projector, hybrid retrieval and
deletion propagation remain P1-J3-04/CM/PD/SC work.
