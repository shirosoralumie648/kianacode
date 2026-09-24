# UI-15 Workbench inbox, Diff and Receipt baseline

> Snapshot date: 2026-09-25. The typed inbox/review, server-artifact viewer and receipt display
> contracts are wired into GitHub Actions. Local tests, builds and checks are intentionally not
> run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-15`](ui-entrypoints.md#step-ui-15) |
| source snapshot | `88959eb5` (`origin/master` after UI-14) plus this UI-15 source slice |
| feature_status | `implemented` (bounded inbox/diff/receipt display contract, deny-first fixtures and CI wiring) |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | `UiSnapshot`/server action card → `WorkbenchInbox` → typed decision intent; `ArtifactRef` page → `ArtifactViewer`; EventLog/Receipt projection → `WorkbenchReceipt` |

The module is a read-only presenter boundary. It does not decide policy, consume approval, fetch a
path, compute a replacement diff, write a file, execute a command, or reconcile an external result.
The existing Workbench controller and typed client remain responsible for command identity and the
`DaemonHost → ControlPlane` submission path.

## Contracts

| Surface | Contract | Deny-first behavior |
|---|---|---|
| Human inbox | `WorkbenchInboxCard` and `WorkbenchInbox` | bounded cards/fields, stable action IDs, reason/scope/expiry/fields/allowed decisions visible; duplicate cards rejected |
| Approval decision | `prepare_decision` → `WorkbenchInboxDecision` | expired/revoked card, stale revision, disallowed decision and client payload mutation are rejected before an action exists |
| Diff/artifact | `ArtifactPage` → `ArtifactViewer` | server `ArtifactRef`, page digest, page bounds, complete digest and expected revision are checked; path/URL fetching is absent |
| Receipt | `WorkbenchReceipt` | files, cost confidence, `ResultUnknown`, limitations and provenance stay explicit; unknown cannot be green completed |

Artifact pages are accepted only when their bytes match the server page digest and the assembled
pages match the immutable reference digest. A page revision mismatch never updates the viewer.
Receipt `is_green()` is true only for a validated `Completed` status with no limitations and no
unknown marker; it does not assert that an external file or provider effect is real.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| inbox fixture | reason, scope paths/digest, expiry, required fields, allowed decisions and opaque server payload are present |
| approval deny paths | expired/revoked approval, revision mismatch and client payload mutation return stable errors before action creation |
| artifact deny paths | wrong page digest, wrong artifact reference, revision mismatch, page conflict and assembled digest mismatch fail closed |
| artifact happy path | a complete server page is retained only after ref/revision/page/content digest checks; duplicate page is idempotent |
| receipt projection | files/cost/limitations/provenance survive projection; `ResultUnknown` displays as unknown and never green |
| source guard | no daemon, ControlPlane, broker, harness, filesystem, network, process or execution authority is introduced |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the UI-15 inbox/diff/receipt
fixtures, the source guard and `cargo check --workspace --tests --locked`. Local Cargo tests,
builds, checks, clippy and smoke commands are not run; CI results are not awaited.

Limitations: the inbox and receipt are process-local projections; artifact pages are synthetic and
do not prove a durable artifact store, cross-process replay, real filesystem state, provider cost,
external effect or reconciliation. No Web/Desktop/IDE adapter, browser rendering, live provider,
physical PTY or second execution path is added.
