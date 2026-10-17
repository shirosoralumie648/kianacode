# INT-13 Secret redaction and echo sentinel baseline

## Scope

INT-13 adds one domain-owned deny-first scan after producer redaction. The scanner classifies
token/API-key/header/JWT/URL-userinfo/credential-lease/provider-error shapes without retaining the
candidate value, and fixture supplied echo sentinels are checked across prompt, transcript,
EventLog, receipt, stdout, stderr, argv, environment and cache projections. Raw provider errors
use a bounded digest/code projection; no provider body is serialized or returned to UI.

## Implemented source slice

- `kiana-domain/src/redaction.rs` owns `SecretScanChannel`, `SecretSentinelKind`, bounded
  `scan_secret_sentinels`/`scan_secret_value`/`scan_secret_channels` and the secret-free
  `RedactedErrorProjection`. Existing text redaction now masks URL userinfo and JWT-shaped values
  before the final scan. Opaque `secret_ref` and numeric token metrics remain allowed.
- `kiana-domain/src/contracts.rs` registers the scanner and bounded provider-error projection as
  closed projection schemas, so unknown fields cannot silently enter a serialized finding.
- `kiana-core/src/events.rs` scans the recursively redacted event payload before append; the
  receipt/result redaction boundary replaces any residual unscannable output with a stable error
  marker. `kiana-daemon/src/connectors.rs` projects provider probe errors through the bounded
  error contract before health classification.
- `kiana-runner/src/harness.rs` applies the same final scan to prompt and transcript text before
  queueing or emitting it, with an opaque `[REDACTED]` fallback.
- `kiana-daemon/src/execution_output.rs` and `harness_capabilities.rs` apply channel-specific
  stdout/stderr scans after bounded capture and terminal-control filtering, preserving metadata
  while replacing residual unsafe previews.
- `kiana-entrypoints/src/workbench_render.rs` reuses the protocol-re-exported scanner after
  control/ANSI stripping and fails closed to `[REDACTED]` if a stream row still contains a secret
  shape. No UI path gains execution or authorization authority.
- Domain fixtures cover every channel, URL/header/JWT/key shapes, opaque credential lease
  metadata, explicit echo sentinels and provider raw-error projection. Daemon, Event/Receipt and
  Workbench source guards keep the shared scanner and existing Broker/EventLog spine visible.

## 2026-10-17 completion slice (CI-only)

- `kiana-eventlog/src/event_store_core.rs`: `validate_event_for_storage()` now adds
  `scan_secret_value_with_limit(SecretScanChannel::Event, ...)` after the existing
  `validate_secret_free()` comparison, so the storage adapter itself refuses shapes that
  `redact_text()` does not rewrite (`sk-` token prefixes, a bare `proxy-authorization:` text
  marker, NUL/oversized input) before any fact or journal file exists. Production producers still
  redact and scan first in `kiana-core::prepare_event_payload`; this is defence in depth for any
  writer that reaches the adapter directly.
- Event fixtures: `kiana-eventlog/tests/int13_event_redaction.rs` proves the deny path on both
  adapters with zero facts and no journal file (and no candidate value in the denial message), the
  byte-stable round-trip of an already-redacted payload with opaque `secret_ref`/numeric token
  metrics, and pins both secret checks at the storage boundary.
- Domain fixtures: `kiana-domain/tests/int13_secret_redaction.rs` now covers the nine channels by
  six credential shapes (finding Display/JSON never retain the candidate), structured secret keys
  and NUL bytes on every channel with opaque metadata still projectable, bounded echo-sentinel
  sets, and the provider raw-error projection (shaped → `provider_error_redacted`,
  benign → `provider_error`, stable digests, channel-bound digests, oversized input bounded).
- Daemon fixtures: `kiana-daemon/src/execution_output.rs` adds the `int13_channels` unit fixtures
  driving stdout/stderr previews (shape replacement, residual `sk-` falling back to `[REDACTED]`,
  channel-bound profile digests). The existing `kiana-daemon/tests/daemon_host.rs` end-to-end
  fixtures for argv/stdout/stderr/env projection stay pinned by the daemon guard.
- UI fixtures: `kiana-entrypoints/tests/int13_secret_redaction.rs` renders every stream event kind
  and asserts the row is redacted (and re-scannable) before display, that the `Error` projection is
  kept, and that a terminal response's error/artifacts are scrubbed without rewriting the `failed`
  status.
- Guards: the core/daemon/entrypoint `int13_*_guard` targets pin the new eventlog scan, the daemon
  runtime fixtures, the daemon_host argv/stdout/stderr/env fixtures and the UI fixture names.
- Lane: new manual `.github/workflows/int13-secret-redaction.yml` (`workflow_dispatch`-only,
  `contents: read`, `cargo fetch --locked` + fmt gate, six targets and seven `--no-run` compile
  steps). Automatic coverage remains the unified `ci.yml` shards: `kiana-domain-s3/4` lists
  `int13_secret_redaction`, `kiana-core-s4/6` lists `int13_secret_redaction_guard`, and the
  whole-crate `kiana-daemon`/`kiana-eventlog`/`kiana-entrypoints` shards discover the new targets.
- No `Cargo.toml`/`Cargo.lock`/`package.json` changes, no new dependencies/test targets, no second
  execution loop; the five model-visible tools are unchanged.
- Evidence for this slice: feature_status `implemented`, proof_level `source`; CI receipt not
  awaited. Still open: process memory, bytes already emitted outside Kiana-owned projections,
  arbitrary high-entropy values without a known shape, external provider logs, durable
  cross-process cache deletion, live connector success and physical outcome proof.

## Evidence boundary

```text
feature_status: implemented
proof_level: source
```

Local Cargo tests, builds, checks, clippy and smoke commands are intentionally not run. GitHub
Actions is the test authority and is not awaited. This slice does not inspect process memory,
already emitted bytes outside Kiana-owned projections, arbitrary high-entropy values without a
known shape or external provider logs; it does not claim durable cross-process cache deletion,
live connector success, or physical outcome proof. Redaction failure remains fail-closed and
returns only a stable code/digest.

### Optional null fields at storage boundaries

Sensitive-key classification treats JSON `null` as an absent optional value: the value scanner
already skips null fields, and recursive redaction now preserves them so `validate_secret_free`
does not report a false positive. Non-null values under sensitive keys are still replaced with
`[REDACTED]` and rejected at the storage boundary. The focused regression fixture reuses
`kiana-domain/tests/pd28_storage_security.rs`; its CI receipt is pending. This correction does not
broaden the allowlist for credential digests or other non-null values.
