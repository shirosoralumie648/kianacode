# UI-22 Web artifact, diff and receipt detail baseline

> Snapshot date: 2026-09-25. UI-22 adds bounded, read-only Web detail projections for server-owned
> ArtifactRef/page metadata, diff statistics and receipt links. GitHub Actions owns fixture and
> compile verification; local Cargo tests, builds, checks, clippy and smoke commands are not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-22`](ui-entrypoints.md#step-ui-22) |
| source snapshot | `b74e7332` (UI-21) plus UI-22 source slice; rebase to latest integration before merge |
| feature_status | `implemented` (typed projection, bounded pages, detail routes, renderer, deny-first fixtures and CI wiring) |
| proof_level | `source`; CI is configured but its result is not awaited |
| canonical path | Web read-only request → DaemonHost/EventLog/Receipt projection; no browser execution or client authority |

## Typed contracts

| Surface | Contract | Deny-first behavior |
|---|---|---|
| protocol | `UiDetailScopeV1`, `UiArtifactRefV1`, `UiArtifactPageV1`, `UiDiffDetailV1`, `UiReceiptDetailV1`, `UiArtifactDetailV1` | unknown fields, invalid digest/MIME, cross-session links, stale revision and Unknown/completed conflict fail closed |
| client | `WebDetailCursor`, `WebArtifactViewer`, `WebDetailProjection` | page digest/revision/reference mismatch, replay scope, HTML/SVG/ANSI/raw content and incomplete/missing content are denied or remain Partial/Unknown; a `none` page can never become Ready |
| Web server | `/api/artifact/detail`, `/api/diff`, `/api/receipt/detail` | token/host/session/tab checks, artifact reference enumeration returns `artifact_not_found`, source cursor changes conflict, diff stats remain server-owned |
| browser | artifact/diff/receipt detail controls | only server refs and textContent are rendered; no arbitrary URL/path fetch, no client diff calculation and Unknown cannot be green or auto-retried |

## Failure-first matrix

| Fixture / guard | Assertion |
|---|---|
| detail fixture | server ref/page/revision/digest/MIME/provenance and timeline/diff/receipt link fields are explicit |
| artifact denial | client payload mutation, stale ref, bad page digest, cross-session enumeration, cursor replay and tab scope mismatch fail closed |
| content policy | raw secret/path, HTML/SVG/ANSI injection and arbitrary URL fetch do not enter the browser projection |
| diff/receipt | additions/deletions/status come from server; missing diff remains Unknown; `result_unknown` stays visible and is not retried |
| read-only path | detail handlers use existing `DaemonHost`/EventLog/Receipt paths and contain no runner, broker, filesystem or command execution |

## Evidence block

```text
source_snapshot: b74e7332 (UI-21) plus UI-22 source slice; rebase target aa4063bf before integration
worktree_status: isolated /tmp/kiana-step-ui22; protocol/client/Web detail contracts, routes, renderer, fixture, guards, workflow and baseline
command_argv: target-only rustfmt --edition 2021 on UI-22 Rust files; git diff --check; GitHub Actions will run cargo fetch --locked, cargo fmt --all --check, UI-22 fixtures/guards and cargo check --workspace --tests --locked
cwd·environment: /tmp/kiana-step-ui22; Linux/bash; local Cargo test/build/check/clippy/smoke deliberately not run; GitHub Actions is the test authority and is not awaited
fixture·cassette: kiana-entrypoints/tests/fixtures/ui22-web-detail.json; ref/page/revision/digest/MIME/cursor/owner/Unknown/path/HTML/URL/client-diff cases; no provider, filesystem or external effect contacted
exit_code: target-only rustfmt and git diff --check only; remote fixtures, workspace compile and CI exit codes are pending/unobserved
status_change: UI-22 ⏳ → 🔄; typed artifact/diff/receipt detail projections, bounded server routes, text-only renderer, deny-first source guards and GitHub workflow added
proof-level change: feature_status=implemented for bounded Web source contracts; proof_level=source only
limitations: no browser E2E, screenshot/golden, real HTTP/session lease race, durable cross-process artifact store, binary/content serving, complete server diff index, receipt entry reconstruction, provider/Broker effect, approval identity authentication or live/physical proof; Web remains a read-only projection and Unknown remains conservative
reviewer: Codex UI-22 source review; checked server-owned refs/revision/digests/MIME/stats, session/tab/cursor fences, cross-location links, no arbitrary fetch/client diff/second execution loop, Unknown visibility; no local runtime test reviewer
```
