# CM-23 Memory negative gates baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-23`](context-memory.md#step-cm-23) |
| feature_status | `implemented` (server-owned candidate/scratch/private write and visibility gates) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | model payload → protected-field rejection → `MemoryModelWriteGate` derived lifecycle → record visibility/ACL |
| authority | model cannot set origin/actor/classification/admission/state/review fields or self-approve user-private memory |

## Contract and behavior

`MemoryModelWriteGate` derives `origin=model`, classification, purpose and lifecycle from the
server-owned collection/session: persistent writes are `candidate/draft` and require operator
approval; instance scratch is `ephemeral/active` and is visible only in its exact session; user
private writes are rejected on the model path. Protected model-supplied fields are rejected.

`MemoryRecord::visibility` now distinguishes searchable, session-only, review-only and denied;
candidate/draft records never become searchable and scratch cannot survive a session boundary.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `candidate_never_appears_before_approval` | model candidate is review-only and not searchable |
| `scratch_does_not_survive_session_retirement` | scratch is visible only to its active session |
| `model_cannot_self_approve_private_memory` | user-private model writes and protected-field overrides are rejected |
| `memory_negative_gates_keep_server_owned_lifecycle` | Core source guard keeps lifecycle/authority derivation out of model text |

## Proof ceiling and handoff

The CM-23 ceiling is `source` plus remote CI wiring. Full daemon/query migration to this gate,
durable session retirement, operator approval projection and deletion/retention propagation remain
open; no live provider or external effect claim is made.
