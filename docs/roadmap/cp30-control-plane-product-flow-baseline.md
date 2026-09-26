# CP-30 ControlPlane product flow and evidence baseline

## Scope

CP-30 adds a strict evidence contract for the three product flows required by ControlPlane §14.6:
read-only low-risk work, write-after-approval with a receipt, and cancel/restart/reconcile. Each
flow is projected to CLI, Workbench, Web and Desktop surface receipts carrying the same authority
digest, committed cursor, terminal status and receipt digest. The bundle requires all three flow
kinds and rejects stale or incomplete surface parity.

The contract consumes committed observations; it does not run a DaemonHost, approve an action,
create a Runner/Broker, or claim that a fake model/effect is a live or physical outcome. Existing
DaemonHost → ControlPlane → Harness/Broker → EventLog/Receipt, `EntrypointParityMatrix`,
`CompanySurfaceParity`, harness roundtrip and Company golden fixtures remain the runtime owners.

## Implemented source slice

- `Cp30ProductFlowEvidence` validates flow-specific effect/approval/recovery invariants.
- `Cp30SurfaceReceipt` requires exact four-surface parity for flow identity, authority digest,
  cursor, status and receipt digest.
- `Cp30ProductFlowBundle` requires one read-only, one approval/write and one cancel/recovery flow;
  CI fixtures cover missing approval, stale surface, read-only effect and blind-restart denial.
- Core exposes a read-only bundle validator and the source guard ties it to existing routes/receipts.

## CI-only evidence

`.github/workflows/cp30-control-plane-product-flow.yml` runs formatting, the domain product-flow
fixtures, the Core route/receipt guard and affected test-target compilation on GitHub Actions. Local
Cargo tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| `product_bundle_covers_read_only_approval_write_and_cancel_recovery_on_four_surfaces` | all three flow contracts and four-surface parity validate together |
| `approval_write_cannot_complete_without_consumed_approval_or_receipt` | approval/effect and stale surface drift fail closed |
| `read_only_and_cancel_recovery_never_claim_unbounded_write_or_blind_restart` | read-only cannot gain effects and recovery requires facts/cache removal |

## Limitations and handoff

- This is a source/fixture contract; it does not execute the three flows, restart a daemon, inspect
  physical files/processes or prove cross-process durable replay.
- The fake-model and local-effect receipts remain CI-only evidence and do not promote CO-48 live
  proof or external delivery confirmation.
- CI results are intentionally not awaited; proof level remains `source`.
