# UI-15 Workbench inbox, Diff and Receipt baseline

> Snapshot date: 2026-09-25 for the first source slice; **2026-10-17 completion slice (CI-only)** for
> the card exit conditions. The typed inbox/review, server-artifact viewer and receipt display
> contracts are wired into GitHub Actions. Local tests, builds and checks are intentionally not
> run; CI receipts are not awaited.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-15`](ui-entrypoints.md#step-ui-15) |
| source snapshot | `f66bdcbf8a47` (`master` after UI-14) plus this UI-15 completion slice |
| feature_status | `implemented` (bounded inbox/diff/receipt display contract, deny-first fixtures and CI wiring) |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | `UiSnapshot`/server action card → `WorkbenchInbox` → typed decision intent; `ArtifactRef` page → `ArtifactViewer`; EventLog/Receipt projection → `WorkbenchReceipt` → `ReceiptGolden` |

The module is a read-only presenter boundary. It does not decide policy, consume approval, fetch a
path, compute a replacement diff, write a file, execute a command, or reconcile an external result.
The existing Workbench controller and typed client remain responsible for command identity and the
`DaemonHost → ControlPlane` submission path.

## Contracts

| Surface | Contract | Deny-first behavior |
|---|---|---|
| Human inbox | `WorkbenchInboxCard` and `WorkbenchInbox` | bounded cards/fields, stable action IDs, reason/scope/expiry/fields/allowed decisions visible; canonical payload digest must match the retained payload; duplicate cards rejected |
| Card vocabulary | `WorkbenchDecision` | closed five-word vocabulary (`approve`/`deny`/`edit`/`restore`/`continue`); a card offering anything else is refused before it can render a button |
| Card display state | `InboxCardState` (`actionable`/`expired`/`revoked`) | `display_state` is derived from server facts only and is the same state `prepare_decision` refuses from; expiry and revocation are facts, not local toggles |
| Inbox updates | `WorkbenchInbox::upsert` / `revoke` / `actionable` | a refreshed card may move forward but a stale expected revision or a revoked→live payload is refused (`inbox_card_revision_stale`, `inbox_card_revocation_regression`); an unknown action ID is refused, never silently dropped |
| Approval decision | `prepare_decision` → `WorkbenchInboxDecision` | expired/revoked card, stale revision, disallowed decision and client payload mutation are rejected before an action exists; a hand-built decision must still match its own payload digest |
| Diff/artifact | `ArtifactPage` → `ArtifactViewer` | server `ArtifactRef`, page digest, page bounds, complete digest and expected revision are checked; path/URL fetching and client-side diff are absent |
| Artifact display | `ArtifactViewState` (`empty`/`partial`/`complete`) / `missing_pages` / `retained_bytes` | partial content is never the artifact; the viewer names the server pages still missing instead of assembling a diff |
| Receipt | `WorkbenchReceipt` | files, cost confidence, `ResultUnknown`, limitations and provenance stay explicit; unknown cannot be green completed |
| Receipt update | `ReceiptUpdate` + `apply_server_receipt` | a stale cursor, a same-cursor replay with different facts and a terminal→non-terminal regression are refused; only a validated server receipt replaces the retained one |
| Receipt golden | `ReceiptGolden` (`kiana.workbench-receipt-golden.v1`) | deterministic display summary (sorted files, normalized digests) that keeps `green`, `unknown`, limitations and provenance visible |
| Review recovery | `REVIEW_REFUSALS` + `ReviewRefusalStage` | review_recovery notes for every presenter-visible refusal, grouped by inbox/decision/artifact/receipt stage; no note resubmits, retries, approves or reconciles |

Artifact pages are accepted only when their bytes match the server page digest and the assembled
pages match the immutable reference digest. A page revision mismatch never updates the viewer.
Receipt `is_green()` is true only for a validated `Completed` status with no limitations and no
unknown marker; it does not assert that an external file or provider effect is real.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| inbox fixture | reason, scope paths/digest, expiry, required fields, allowed decisions and opaque server payload are present |
| approval deny paths | expired/revoked approval, revision mismatch, disallowed decision and client payload mutation return stable errors before action creation |
| decision digest | a locally edited decision payload can never ride the original digest (`inbox_decision_payload_digest_mismatch`), and a non-empty payload with the right shape is required |
| inbox update paths | duplicate cards, stale expected revisions, revoked→live downgrades and unknown action IDs fail closed |
| artifact deny paths | wrong page digest, wrong artifact reference, revision mismatch, page conflict, page-count mismatch and assembled digest mismatch fail closed |
| artifact happy path | a complete server page set is retained only after ref/revision/page/content digest checks; duplicate page is idempotent; the viewer reports `empty`→`partial`→`complete` and the missing page list |
| artifact bounds | page index/count/revision/size bounds and the 16 MiB total budget refuse oversized input without retaining bytes |
| receipt projection | files/cost/limitations/provenance survive projection; `ResultUnknown` displays as unknown and never green |
| receipt replay | identical replay is idempotent, stale cursors and same-cursor conflicts are refused, and a terminal receipt never regresses |
| receipt recompute | a fragmented recompute that would not validate is refused instead of drawn |
| receipt golden | the golden is byte-stable, order-independent, keeps unknown/limitations, and refuses unknown fields |
| source guard | no daemon, ControlPlane, broker, harness, filesystem, network, process or execution authority is introduced; the diff view stays server-provided |
| completion-slice guard | budgets/vocabulary/fixture names/scenarios/denials/templates/lane/shard/baseline stay wired together |

## Review error recovery notes

`REVIEW_REFUSALS` is the single presenter-visible refusal catalogue: 32 codes across the `inbox`,
`decision`, `artifact` and `receipt` stages, each with a bounded recovery note. The notes are
read-only by construction — reload the snapshot, re-request the exact server page, keep the
terminal receipt and ask for the next cursor, keep the revocation and request a new approval. None
of them resubmits a decision, retries a command, approves, restores or reconciles an effect.

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the UI-15 inbox/diff/receipt
fixtures, the display boundary plus completion-slice guards and
`cargo test -p kiana-entrypoints --no-run --locked` in the manual lane
[`.github/workflows/ui15-workbench-review.yml`](../../.github/workflows/ui15-workbench-review.yml);
the unified `ci.yml` still runs the whole `kiana-entrypoints` crate shard. Local Cargo tests,
builds, checks, clippy and smoke commands are not run; CI results are not awaited.

Limitations: the inbox and receipt are process-local projections; artifact pages are synthetic and
do not prove a durable artifact store, cross-process replay, real filesystem state, provider cost,
external effect or reconciliation. The refusal catalogue is a presenter contract, not an
authorization source: only ControlPlane can consume an approval. No Web/Desktop/IDE adapter,
browser rendering, live provider, physical PTY or second execution path is added.
