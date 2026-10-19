# UI-22 Web artifact, diff and receipt detail baseline

> Snapshot date: 2026-10-19 (completion slice, base `b172ed17`). UI-22 adds bounded, read-only Web
> detail projections for server-owned ArtifactRef/page metadata, diff statistics and receipt links.
> This slice closes the two gaps the previous baseline left open: the receipt digest now
> recomputes identically in a **fresh process** after its own output events, and the artifact /
> diff / receipt detail routes have **live loopback** refusal and projection fixtures. The row
> stays `🔄` because the card also asks for artifact text/patch bytes by ref, per-file (partial)
> approval and a server diff index, none of which exist yet (see Limitations).

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-22`](ui-entrypoints.md#step-ui-22) |
| source snapshot | `b172ed17` (integration merge of INT-24/UI-19) plus this UI-22 completion slice |
| feature_status | `partial` — metadata-only detail projection, bounded pages/cursors, typed receipt projection and denial matrix are implemented; artifact content/patch serving, per-file approval and server diff stats are not |
| proof_level | `local_behavior` for the focused fixture/guard targets and the two live loopback fixtures; no browser E2E, durable artifact store or live/physical proof |
| canonical path | Web read-only request → `DaemonHost`/EventLog/`ControlPlane::read_receipt` → typed detail JSON; no browser execution, no client diff, no URL/path fetch |
| row 438 | **unchanged (`🔄`)** — see "What is still missing" |

## Completion-slice changes

1. **Receipt source set.** `kiana-core/src/receipts.rs` added `receipt_source_events()`, which
   excludes `run.receipt` and `run.snapshot` from the receipt source set. Both events are appended
   *after* a run reaches its terminal outcome (`lifecycle.rs`: build receipt → append
   `run.receipt` → append `run.snapshot`), so the previous `events.len()` cursor and full
   `source_event_ids` changed the digest on every re-read: a receipt recomputed in a new process
   could never match the one the completing process returned. `versioning.rs` already excludes
   `run.snapshot` from golden capture for the same reason.
2. **Fresh-process recomputation fixture.** `kiana-core/tests/ui22_receipt_detail.rs` appends the
   two bookkeeping events, then re-execs the test binary (`--exact`) as a child that reopens the
   same JSONL journal and recomputes `receipt_digest`, `source_cursor` and `source_event_ids`.
   The child is read-only: journal bytes are compared before/after.
3. **Live loopback detail routes.** `kiana-entrypoints/src/web.rs` `mod tests` now drives the real
   `router`/`WebApp`/`DaemonHost` over loopback for `/api/artifact/detail`, `/api/diff` and
   `/api/receipt/detail`.
4. **Bounded surface:** server page cursors gain a 15-minute TTL (`web_page_cursor_expired`);
   detail pages carry `mime`/`size_bytes`, `content: null`, `content_encoding: "none"`,
   `truncated`, and explicit binary/oversize limitations; any `files=` selection on the artifact or
   diff route is refused (`web_partial_file_approval_unsupported`) rather than narrowing an
   approval payload client-side; the receipt page consumes only the typed
   `output.run_receipt.receipt_digest`/`source_cursor` and stays `unknown` when that projection is
   absent.

## Typed contracts

| Surface | Contract | Deny-first behavior |
|---|---|---|
| protocol | `UiDetailScopeV1`, `UiArtifactRefV1`, `UiArtifactPageV1`, `UiDiffDetailV1`, `UiReceiptDetailV1`, `UiArtifactDetailV1`, `UiDetailLinkV1` | unknown fields, invalid digest/MIME, cross-session link, stale revision, `none`-encoding-with-content and Unknown/completed conflict fail closed |
| client | `WebDetailCursor`, `WebArtifactViewer`, `WebDetailProjection` | page digest/revision/reference/count mismatch, cross-tab cursor, missing content (stays `Unknown`) and oversized/NUL pages are denied or remain Partial/Unknown; a page without content can never become `Ready` |
| Web server | `/api/artifact/detail`, `/api/diff`, `/api/receipt/detail` | token/host/session/tab checks, `artifact_not_found` for foreign/private refs, cursor TTL/replay/scope/limit fences, `files=` refusal, receipt digest from the typed projection only |
| browser | `loadArtifactDetail`, `loadDiffDetail`, `loadReceiptDetail` | `textContent` only; control/OSC sequences and executable URLs are refused by `sanitizeUntrustedContent`; no `innerHTML`, no artifact URL fetch, no client diff |

## Failure-first matrix

| Fixture / guard | Assertion |
|---|---|
| `ui22_detail_denials_precede_any_projection` (live, entrypoints) | `files=` refused on artifact and diff with `web_partial_file_approval_unsupported` (400) before any projection; missing token / foreign Origin → 401; cross-session artifact → `artifact_not_found`; unknown session → `session_unknown`; an expired server cursor → `web_page_cursor_expired` |
| `ui22_detail_success_keeps_server_refs_and_limitations` (live) | text detail keeps ref MIME/size/digest, `content: null`, `content_encoding: "none"`, `truncated: false`, explicit limitation; binary octet-stream adds `artifact_binary_content_not_rendered`; a 256 KiB page adds `artifact_size_limit_exceeded`; diff stays `unknown` with 0/0 stats and `server_diff_not_available`; receipt digest comes from `run_receipt` or the page is `unknown` with a zero digest |
| `kiana-client/tests/ui22_web_detail.rs` (7 tests) | forged page payload, stale revision, bad digest, HTML/SVG MIME, oversize/NUL page, `none`+content, cross-session page/link, cursor tab/cursor replay, diff stats not client-computed, Unknown receipt not promoted, partial→ready assembly only after digest match |
| `kiana-core/tests/ui22_receipt_detail.rs` (4 tests) | receipt digest stable after its own `run.receipt`/`run.snapshot`; Unknown cannot become completed; foreign session → `run_owner_mismatch` + null output; fresh-process child recomputes the identical digest/cursor/ids and does not modify the journal; two source guards (`receipt_source_events`, typed digest validation) |
| `ui22_web_detail.rs` / `ui22_web_detail_guard.rs` (fixture + guard) | 17 deny-first rows, 10 success rows, server markers, and the read-only route guard (no `Command::new`, no `std::fs`, no `harness_run::` in diff, no `innerHTML`/executor in the page) |

## Evidence block

```text
source_snapshot: worktree D:/code/kiana-steps/ui22, branch step/ui22-20261018-1, base b172ed17; UI-22 completion slice over the 2026-09-27 source slice
worktree_status: row 438 remains 🔄; changed: kiana-core/src/receipts.rs, kiana-core/tests/ui22_receipt_detail.rs, kiana-entrypoints/src/web.rs, kiana-entrypoints/tests/ui22_web_detail.rs, kiana-entrypoints/tests/ui22_web_detail_guard.rs, kiana-entrypoints/tests/fixtures/ui22-web-detail.json, kiana-client/tests/ui22_web_detail.rs, .github/workflows/ui22-web-detail.yml, this baseline, the UI-22 card, docs/roadmap.md §3 and CURRENT_STATUS.md. No Cargo.toml/Cargo.lock change; no push/merge.
command_argv (all from D:/code/kiana-steps/ui22, CARGO_TARGET_DIR=D:/code/kiana-steps/target-ui22, CARGO_BUILD_JOBS=4, RUST_MIN_STACK=33554432, --offline):
  cargo test -p kiana-entrypoints --test ui22_web_detail -- --test-threads=1        -> exit 0 (2 passed)
  cargo test -p kiana-entrypoints --test ui22_web_detail_guard -- --test-threads=1  -> exit 0 (1 passed)
  cargo test -p kiana-client --test ui22_web_detail -- --test-threads=1             -> exit 0 (7 passed)
  cargo test -p kiana-core --test ui22_receipt_detail -- --test-threads=1           -> exit 0 (4 passed; child re-exec 1 passed)
  cargo test -p kiana-entrypoints --lib -- ui22_detail -- --test-threads=1          -> exit 0 (2 passed, 485 filtered)
  cargo check -p kiana-core --tests / -p kiana-entrypoints --tests / -p kiana-client --tests -> exit 0 each (0 errors)
  cargo test -p kiana-core --test control_plane -- receipt -- --test-threads=1      -> exit 0 (30 passed)
  rustfmt --edition 2021 --check <touched Rust files>                               -> exit 0 (per-file)
cwd·environment: D:/code/kiana-steps/ui22; Windows 11, rustc 1.96.1, Git Bash. `cargo fmt --all --check` fails on this host with os error 206 (path too long), so formatting was verified per-file with `rustfmt --edition 2021 --check`. No `cargo test --workspace`, clippy or smoke scripts were run. GitHub Actions lane is workflow_dispatch-only and its receipt is not awaited.
fixture·cassette: deny-first before success in every target. kiana-entrypoints/tests/fixtures/ui22-web-detail.json: 17 denial rows (client payload mutation, stale revision, page digest mismatch, missing content never Ready, cross-session enumeration, cross-tab cursor, cursor replay, Unknown→green, result_unknown auto-retry, raw secret/path leak, HTML/SVG/ANSI injection, client diff, arbitrary URL fetch, expired server cursor, partial-file approval, unsupported receipt cursor) and 10 success rows. Live fixtures seed only event-log facts; no provider, network or external effect is contacted, and the receipt fixture reopens a temp JSONL journal.
exit_code: 0 for every focused command listed above.
status_change: receipt source set excludes its own output events so the digest survives a fresh process; detail pages are bounded (15-min cursor TTL, 64 KiB detail limit) and state binary/oversize/truncation limitations explicitly; `files=` is refused instead of narrowing an approval; receipt detail reads only the typed `run_receipt` digest. No second execution loop; model-visible tools unchanged (shell, apply_patch, mcp, memory.search, memory.write).
proof-level change: `source` → `local_behavior` for the five focused targets and cargo check; no live/durable claim.
limitations: (1) the Web detail surface serves metadata only — every detail page returns `content: null`/`content_encoding: "none"` because artifact bytes live in a separate `LocalArtifactStore` that the Web router does not read, so "fetch text/patch by ref" is not delivered; (2) diff is a constant stub (`unknown`, 0/0, `files: []`, `server_diff_not_available`) with no server diff index or file status; (3) per-file (partial) approval is refused (`web_partial_file_approval_unsupported`) rather than implemented, and the protocol has no per-file approval payload; (4) artifacts have no expiry field, so "expired ref" is only proven as an expired server page cursor plus stale-revision refusal; (5) ANSI refusal for accepted `text` pages is enforced by the browser sanitizer (`sanitizeUntrustedContent`) and the UI-23 marker guard, not by an executed JS/DOM test; (6) no browser E2E, screenshot/DOM golden, concurrent cursor race or durable cross-process artifact store; (7) on this Windows host `path_lock_platform_unsupported` (Unix-only `flock`) makes 8 pre-existing `kiana-core/tests/control_plane.rs` packet/path-lock tests fail; the receipt subset (30 tests) passes and the failures are unrelated to this slice.
reviewer: local focused-test review (this session); checked the deny-before-success order, the receipt source-set regression (reverting the filter makes the digest test fail), the fresh-process child's read-only journal behavior, route authority/`files=` refusal ordering, no `Command::new`/`std::fs`/client diff in the detail path, and that the five model-visible tools and frozen items are untouched.
```

## What is still missing (why row 438 stays `🔄`)

- **Artifact text/patch bytes by ref.** The card's first step is "按 ref 分页拉取文本/patch/metadata".
  Only metadata is paged (`/api/artifact`, `/api/artifact/detail`): pages are always
  `content: null`, `content_encoding: "none"`. Serving bytes requires a server-owned,
  digest-checked artifact content route in front of `LocalArtifactStore`; it does not exist.
- **Partial-file approval.** The card lists it as a success case. Today the route refuses any
  `files=` selection; approving a subset of files would need a per-file approval payload in the
  capability/approval contract, which is not present.
- **Server diff index and file status.** `/api/diff` returns server-authored constants
  (`unknown`, 0/0, `files: []`, `server_diff_not_available`); the card asks to show file status and
  server stats.
- **Artifact-ref expiry.** There is no expiry on `UiArtifactRefV1`; only the server page cursor
  has a TTL and only stale revisions are fenced.
