# SC-20 Redaction / Secret Egress Baseline

## Scope

SC-20 closes the recursive redaction boundary from domain signal profiles through EventLog,
Capability receipts, Runner model output, Provider errors and daemon process/MCP output. The domain
redactor handles nested values, bearer/basic/API-key/token/secret markers, bounded depth/bytes and
stream-split secrets; a redaction profile binds signal/data class and digest. Core prepares every
event payload, rejects unstable or oversized redaction, marks redacted snapshots non-recoverable
and redacts capability results/receipts. Runner applies text/value redaction to model turns and
stream deltas; provider errors are safe-text only; shell/MCP output is bounded and stderr is kept as
digest/metadata; sandbox clears inherited secret environments.

Redaction never turns a secret into authorization and a digest never reconstructs the value. Raw
provider headers, raw response bodies and secret-bearing process diagnostics are outside durable
facts.

## Evidence and limits

- `kiana-domain/tests/er03_redaction.rs` covers nested/bearer/API-key/sentinel, redaction metadata,
  size/depth and artifact-reference behavior.
- `kiana-core/tests/er03_redaction_guard.rs` and `kiana-core/tests/sc20_redaction_boundary_guard.rs`
  pin EventLog/Receipt/Runner/Provider/daemon egress markers and reject raw-secret fields.
- GitHub Actions runs fixtures, source guards and workspace compile; local tests are intentionally
  not executed.

This slice is `feature_status=implemented`, `proof_level=source`: complete OS/kernel/provider
memory erasure, external sink verification and live/physical proof remain SC-21+ / OA / ER work.
