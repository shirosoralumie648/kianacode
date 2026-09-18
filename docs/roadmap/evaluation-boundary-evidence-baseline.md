# EQ-16 Evaluation Boundary Evidence Baseline

## Scope

EQ-16 adds scrubbed, eval-only boundary evidence to the existing capture adapter. Process-tree
records contain only PID relationships and command digests; file diffs contain relative paths and
content digests; network observation is a bounded syscall count; and secret findings are an
allow-listed pattern code (`api_key`, `authorization`, `password`, `private_key`, `token`) rather
than raw values.

Attaching evidence with network activity or secret matches marks the capture `SafetyViolation` with
stable `safety_violation` rather than allowing a flushed capture to look safe. No process table,
filesystem diff, network socket or environment secret is read by this adapter; a later CI harness
may provide sanitized observations through the value boundary.

## Evidence and limits

- `kiana-daemon/tests/eq16_boundary_evidence.rs` covers scrubbed process/file/network/secret
  evidence, unsafe capture blocking, relative-path validation and unknown-pattern rejection.
- `kiana-core/tests/eq16_boundary_evidence_guard.rs` protects the no-syscall/no-secret/no-runner
  boundary. GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not yet collect real
process/file/network observations, persist evidence manifests, or implement durable restart and
quality result gates. No live/physical effect proof is claimed.
