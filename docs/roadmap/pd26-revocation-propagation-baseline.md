# PD-26 - Revocation/delete/expire propagation baseline

> Snapshot date: 2026-09-28. This slice owns the *decision* that a revoke, delete or expire has
> reached every derived layer. Local Cargo test/build/check/clippy/smoke commands are intentionally
> not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`PD-26`](persistence-data-layer.md#step-pd-26) |
| source snapshot | master plus this PD-26 propagation slice |
| feature_status | `partial` for source-level ordering/epoch/receipt contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | layer observations -> ordered report -> per-read and per-write admission |

A revoke, delete or expire decision is not finished when the tombstone is committed. It is finished
when every derived layer has acknowledged that same tombstone and no longer serves the payload.
`RevocationLayer` fixes the order the tombstone has to travel:

```text
facts -> artifact -> memory -> index -> cache -> checkpoint
```

The order is not advisory. A layer that still answers with revoked data stays reachable for as long
as the layer before it in that list has not been fenced, so an acknowledgement that skips a layer
cannot be treated as progress.

## What the card rejects, and how

| Rejected | How |
|---|---|
| a derived layer still returning revoked data | any observation in `StillServing` makes the report `RevokedDataServed`, which is never a completion, whatever else it also reports (`revocation_derived_layer_still_serves_data:<layer>`) |
| writing under a superseded data_epoch | `admit_derived_write` refuses an epoch below the current one (`revocation_stale_data_epoch_write`) and an epoch ahead of it (`revocation_write_epoch_ahead`); a layer reporting the wrong epoch also blocks completion (`revocation_layer_epoch_stale:<layer>`) |
| a delete with no receipt | an observation claiming `Tombstoned` with no receipt is rejected at construction (`revocation_observation_receipt_required`), so acknowledgement without evidence cannot enter a report at all |
| a layer whose state was never established | `Unreachable` is an absence of proof, not an acknowledgement (`revocation_layer_state_unknown:<layer>`); it also cannot carry a receipt (`revocation_observation_unreachable_with_receipt`) |
| acknowledgement out of propagation order | credit is only the contiguous prefix `acknowledged_prefix`; a layer that acknowledged past an unreached one is named as `revocation_layer_order_violation:<layer>` |
| a retried receipt silently moving state | `merge_layer_observation` is idempotent for a replayed receipt, refuses a regression, and refuses a second, different receipt for the same tombstone (`revocation_receipt_conflict`) |
| a report that completes while naming pending layers | `validate_against` recomputes the whole decision and rejects a completion that still carries pending layers or a reason (`revocation_report_completion_with_pending`) |
| a tampered report or retargeted request | `validate_against` re-derives every field and binds the request digest, so an edited status, reason or epoch is rejected (`revocation_report_binding_invalid`) |
| a restored store reviving a tombstoned object | `admit_derived_read_after_recovery` refuses a layer that still serves the payload (`revocation_tombstone_revival_denied`) and a layer restored below the current epoch (`revocation_recovered_epoch_superseded`) |

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| propagation order | the layer set is exactly facts, artifact, memory, index, cache, checkpoint, in that order |
| kind symmetry | revoke, delete and expire are the same propagation with a different reason |
| full acknowledgement | every layer tombstoned with a receipt at the current epoch completes, with no pending layer and no reason |
| serving layer, each layer | every one of the six layers in `StillServing` produces `RevokedDataServed` and names itself |
| decision precedence | a serving layer outranks a stale epoch, an unreachable layer and a missing acknowledgement in the same attempt |
| stale write epoch | a write at `previous_epoch` against a current epoch is refused; one ahead is refused; zero is refused; equal is admitted |
| restored layer epoch | a layer reporting the previous epoch leaves the report incomplete and names `cache` |
| unreachable layer | `Unreachable` is incomplete and names the layer |
| receipt required | `Tombstoned` without a receipt is rejected at construction |
| pending with receipt | a layer that has not taken the tombstone cannot have receipted it |
| receipt idempotency | a replayed receipt is a no-op; a regression, a conflicting receipt and a cross-layer merge are each refused |
| partial propagation | the first unacknowledged layer is named and the acknowledged prefix is reported separately |
| order violation | two pending layers below four acknowledged ones names the gap and credits no layer past it |
| recovery revival | a rebuilt layer still serving the payload, and a layer restored below the current epoch, are both refused |
| tampered report | an edited status, an edited reason and a re-epoched request are all rejected |

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate. No separate workflow.

This is a source-only decision over adapter-reported observations. **It does not delete, invalidate,
compact, evict or restore anything.** No `std::fs`, no port call, no adapter call and no event append
exists in the module; the tombstone itself is appended through the existing EventLog path, and the
guarded test asserts that boundary. `RevocationLayerObservation` is a claim about a layer, not
evidence read from one: this slice cannot tell a truthful adapter from a lying one, and a report is
only as good as the receipts supplied to it. The decision order is fixed so the *reported* reason is
deterministic, not so the reported reason is correct.

Three things the card asks for are explicitly not established here. First, no real derived store is
fenced: the layers are named, ordered and gated in source only, so nothing here proves that an
artifact store, a memory projection, an index, a cache or a checkpoint actually stops serving. Second,
the idempotency contract is expressed as a pure fold over two receipts, and no tombstone is actually
written twice; concurrent writers still need the EventLog CAS path to make it hold. Third,
"recovery does not revive" is a predicate over a supplied observation, not a rebuild: no snapshot is
restored and no index is rebuilt here, so the restore-and-verify evidence remains PD-22/PD-23 work.

The six layers here do not line up one-to-one with the nine `DataPropagationTarget` values in the
ER-29 domain contract, which additionally covers receipt, audit and export; the mapping between the
two vocabularies is not asserted by any test in this slice and is recorded as follow-up.
