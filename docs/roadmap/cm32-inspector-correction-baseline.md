# CM-32 Context/Memory Inspector and user correction baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-32`](context-memory.md#step-cm-32) |
| feature_status | `implemented` (redacted inspector projection and mutation-bound correction contract) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | receipt/plan/projection/invalidation → redacted Inspector manifest; user correction → approval-bound MemoryMutation |
| authority | Inspector is read-only projection; EventLog and MemoryMutation remain the only mutation authority |

## Contract and behavior

`ContextMemoryInspectorSnapshot` exposes receipt/context-plan digests, source IDs/revisions,
freshness, evidence, locator digests, omission reasons, candidate state digests, projection lag,
data epoch, invalidation and rebuild status. It does not serialize source locators or body text.

`UserMemoryCorrection` binds a current inspector snapshot digest to an existing `MemoryMutation`,
requires UPDATE/DELETE/REVOKE semantics, expected revisions/evidence/scope/idempotency and explicit
operator approval. UI/client code cannot write EventLog directly or widen the mutation operation.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `inspector_matches_receipt_without_private_leak` | inspector metadata follows the receipt while omitting query/source locator body text |
| `user_correction_requires_governed_mutation` | correction without operator approval fails; approved correction validates against current inspector digest |
| `inspector_is_redacted_and_correction_reuses_memory_mutation` | protocol/domain/daemon source guard keeps read-only projection and shared mutation boundary |

## Proof ceiling and handoff

The CM-32 ceiling is `source` plus remote CI wiring. Full protocol/client/UI rendering, durable
cross-process inspector hydration, user-facing deletion/rebuild controls and live approval UX remain
UI/PD/SC work; no private source body or provider secret is claimed exposed.
