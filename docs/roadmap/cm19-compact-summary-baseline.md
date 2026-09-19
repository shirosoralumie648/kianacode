# CM-19 CompactSummary baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-19`](context-memory.md#step-cm-19) |
| feature_status | `implemented` (structured evidence/status validation and recent pending-pair fixture) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | source messages → evidence-only `CompactSummary` → bounded compacted history with latest complete group |
| authority | summary prose cannot mint completed action, approval, verification or business fact; authority remains events/receipts |

## Contract and behavior

`CompactSummaryEvidence` classifies decision/approval/completed-action/verification references and
requires a fact digest plus explicit `Confirmed/Pending/Denied/Unknown/Cancelled` status. Summary
decision, completion and verification references must point to matching confirmed evidence;
`validate_against_evidence` additionally requires the evidence to exist in the current authoritative
projection. Model-generated prose alone therefore cannot claim a completed tool or approval.

The Runner compaction fixture preserves the latest goal and the complete assistant/tool pending
pair while older history is eligible for omission; the summary remains a disposable context view.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `summary_cannot_forge_completed_tool_or_approval` | missing, pending and denied facts cannot become completed/approved summary refs |
| `cm19_compaction_keeps_latest_goal_and_pending_pairs` | compaction retains latest goal plus assistant/tool pair |
| `compact_summary_keeps_evidence_and_complete_recent_groups` | Core guard keeps evidence/status and no-second-loop boundaries |

## Proof ceiling and handoff

The CM-19 ceiling is `source` plus remote CI wiring. Real admitted summary-model generation,
EventLog-backed evidence projection, artifact/CAS commit, crash recovery and cross-process summary
hydration remain CM-20/21/ER/PD work; no live provider claim is made.
