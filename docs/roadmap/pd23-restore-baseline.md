# PD-23 restore verification baseline

> Snapshot date: 2026-09-27. This slice adds a read-only restore verifier over the sealed PD-22
> manifest and a replacement-fence fact. Local Cargo test/build/check/clippy/smoke commands are
> intentionally not run; GitHub Actions owns the fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`PD-23`](../roadmap/persistence-data-layer.md#step-pd-23) |
| source snapshot | `b507e505` plus this PD-23 source slice |
| feature_status | `partial` for restore manifest/fact verification and replacement fencing |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | sealed SnapshotManifest → RestoreVerificationFact → Core validation → daemon read-only verifier |

`RestoreVerificationFact` binds the sealed manifest digest, owner/store/old instance, source and
projector cursor, projection digest, artifact refs, data/authority epochs and pending approval,
lease, trigger and external-effect counts. A restored instance must use strictly newer data and
authority epochs, fence the old instance/authority, keep the new process paused and show all
projectors caught up before `Verified`. Pending resources or Unknown effects remain
`NeedsReconciliation` with explicit evidence; they cannot revive automatically.

The daemon adapter validates the manifest/fact identity and epoch transition only. It does not
copy files, replace an active root, acquire a lock, replay a trigger, append EventLog facts or
claim that an external effect succeeded.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| verified restore | new data/authority epochs, equal source/projector cursor, artifact refs and old fences are required |
| reconciliation gate | pending approval/lease/trigger or Unknown effect stays `NeedsReconciliation` with refs |
| rollback fence | authority/data epoch rollback and unfenced old instance fail closed |
| activation fence | `default_paused` is mandatory; verified facts cannot carry pending resources or refs |
| manifest binding | daemon rejects owner/store/instance/cursor/epoch/digest drift |
| read-only guard | no root copy/rename, lock acquisition, trigger revival, EventLog append or external effect path |

## CI and limitations

GitHub Actions runs `.github/workflows/pd23-restore.yml` with domain/Core/daemon fixtures, source
guards and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke
commands are intentionally not run, and CI results are not awaited.

Limitations: this is a source-level gate. It does not perform a real backup restore, fsync/hash
scan, multi-process lock handoff, projector rebuild, ACL rehydration, external provider/OS
reconciliation or active-root cutover; durable/live/physical restore evidence remains unproven.
