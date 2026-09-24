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
