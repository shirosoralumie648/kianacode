# DEP-20 - Quiesce and snapshot consistency baseline

> Snapshot date: 2026-09-27. This slice decides whether a storage snapshot is coherent enough to
> take. Local Cargo test/build/check/clippy/smoke commands are intentionally not run; GitHub Actions
> owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-20`](../roadmap.md#step-dep-20) |
| source snapshot | master plus this DEP-20 quiesce gate slice |
| feature_status | `partial` for source-level quiesce/consistency contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | adapter write-state facts -> ordered consistency gate -> snapshot admission |

## Relationship to PD-22 and DEP-19 (read this before extending)

The roadmap contains three adjacent backup contracts. They are deliberately **not** merged in this
slice, and the boundaries are:

| Contract | Answers | Owner |
|---|---|---|
| `SnapshotManifest` ([PD-22](persistence-data-layer.md#step-pd-22)) | what a finished backup looks like, and whether its files verify | existing; consumed by `kiana-daemon/src/restore_verifier.rs` |
| `BackupManifest` (DEP-19) | a verifiable, replayable, redacted-exportable description of a snapshot, with per-chunk integrity | `kiana-domain/src/backup_manifest.rs` |
| `QuiesceGate` (DEP-20) | **whether the store is in a state where a snapshot is coherent at all** | `kiana-domain/src/quiesce_gate.rs` |

DEP-20 deliberately does **not** describe backup contents, hash files, or seal a manifest. It
consumes adapter-reported write state and answers one question: may a snapshot be taken now? Adding
a third manifest shape here would duplicate PD-22 rather than advance the roadmap. Reconciling
`SnapshotManifest` and `BackupManifest` into one type is recorded as a follow-up, not done silently.

## The gate

`QuiesceRequest` names an operation, actor, target cursor, data/authority epochs, the highest
cursor any projector has consumed, and one `QuiesceObservation` per component. All five components
must be observed exactly once: `EventLogMain`, `EventLogWal`, `ArtifactStore`,
`ProjectionCheckpoint`, `MigrationRegistry`.

`QuiesceReport::evaluate` applies a fixed rule order, so the same facts always yield the same first
violated reason:

1. **Unknown wins over everything.** A component whose write state was never established is
   `Unknown`, not `Draining` and not `Inconsistent` — an unestablished state cannot be diagnosed.
2. **Active writer** -> `Draining` (`quiesce_active_writer`).
3. **Draining / unflushed remainder** -> `Draining` (`quiesce_unflushed_remainder`).
4. **A component not durable to the target cursor** -> `Inconsistent`
   (`quiesce_target_not_durable`).
5. **A WAL that disagrees with the main file** -> `Inconsistent` (`quiesce_wal_main_divergence`).
6. Otherwise `Quiesced` at the target cursor.

Two structural rules make the decision safe to consume:

- A refused decision returns `snapshot_cursor: 0`. A caller can never be handed a cursor for a
  store the gate did not admit (`quiesce_report_cursor_leak`).
- A projector whose cursor exceeds the store's source cursor is rejected when the *request* is
  built, before any decision is derived (`quiesce_projector_cursor_ahead_of_source`). A projector
  that claims to have consumed more than the store durably holds has either regressed or fabricated
  a position, and the snapshot boundary is not trustworthy either way.

A WAL observation must state whether it agrees with the main file. Silence is not consistency, so a
missing `wal_consistent` is rejected; a non-WAL component may not report one.

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| admitted | a fully idle, durable store is `Quiesced` at the target cursor |
| active writer | `ActiveWriter` drains and returns no cursor |
| unflushed | `Draining` drains and returns no cursor |
| unknown | an unestablished component is `Unknown`, and outranks the other reasons |
| durability | a component durable only to an earlier cursor is `Inconsistent` |
| WAL divergence | a WAL disagreeing with the main file is `Inconsistent` |
| projector | a projector cursor ahead of the source is rejected at request construction |
| coverage | a missing or duplicated component is rejected |
| WAL consistency | missing `wal_consistent` is rejected; an unexpected one is rejected |
| cursor regression | `durable_cursor > visible_cursor` is rejected |
| tamper | a forged snapshot cursor or a stale report digest fails closed |
| determinism | identical facts give an identical report, and the reason does not depend on component order |

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate. This slice adds no
separate workflow.

This is a source-only decision over adapter-supplied facts. It does not pause admission, stop a
writer, flush a buffer, fsync a file, copy a chunk, write a manifest, or resume work. It cannot
prove that a real store reached a consistent state: the write states, durable cursors and WAL
agreement are all reported by an adapter, and a lying adapter produces a confidently wrong
`Quiesced`. Proving that end to end needs a fake store that drives
quiesce -> manifest -> fsync -> verify -> resume, which is the success path this card names and
remains open. RPO/RTO measurement and fault injection stay with DEP-24.
