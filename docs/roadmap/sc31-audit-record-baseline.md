# SC-31 AuditRecord append-only baseline

## Scope

SC-31 makes server-derived audit facts explicit at the EventLog boundary. `AuditRecord` now
requires a redaction-safe `reason`, binds the built-in Audit redaction profile and rejects
recoverable payloads. `AuditRecordEvent` is a strict, versioned, digest-bound append-only
envelope that repeats source cursor, source event IDs and redaction metadata. The domain reducer
normalizes a missing reason to `reason_unspecified`; it never accepts an `audit.*` payload as an
audit fact.

`kiana-eventlog` validates this envelope before legacy append, idempotent replay, and atomic
transition planning. `audit.record` is the only accepted audit kind; unknown or self-submitted
`audit.*` events, forged source/redaction bindings, recoverable payload metadata and mismatched
idempotency keys fail closed before storage mutation. The protocol re-exports the envelope and
the schema registry records `kiana.audit-event.v1` as a strict RuntimeEvent contract.

## Source slice and CI evidence

- `kiana-domain/src/observability.rs`, `kiana-domain/src/audit.rs` and
  `kiana-domain/src/contracts.rs` define and register the record/envelope invariants.
- `kiana-protocol/src/lib.rs` re-exports the versioned audit event contract.
- `kiana-eventlog/src/audit_contract.rs`, `event_store_core.rs` and `journal_core.rs` enforce the
  pre-write boundary for compatibility appends, idempotent retries and transition batches.
- `kiana-domain/tests/sc31_audit_record.rs`, `kiana-eventlog/tests/sc31_audit_event.rs` and
  `kiana-core/tests/sc31_audit_record_guard.rs` are GitHub-only fixtures and source guards.
- `.github/workflows/sc31-audit-record.yml` runs formatting, the three fixtures and workspace
  test-target compilation. Local tests/build/check/clippy/smoke are intentionally not executed.

## Evidence boundary

```text
source_snapshot: master=0cc07fd6 + SC-31 source slice
worktree_status: isolated branch sc-31-audit-record-schema-20260924; unrelated WIP preserved
command_argv: git diff --check; GitHub Actions runs cargo fmt, SC-31 fixtures/source guard and cargo check (not awaited)
cwd·environment: /tmp/kiana-step-sc31; Linux x86_64; local tests/build/check/clippy/smoke deliberately not run
fixture·cassette: GitHub-only SC-31 domain, EventLog and core source-guard fixtures in sc31-audit-record.yml
exit_code: local diff check only; remote CI result intentionally unobserved
status change: SC-31 source slice and CI wiring implemented; roadmap row/card remain 🔄 pending CI evidence
proof-level change: feature_status=implemented, proof_level=source; no local_behavior, durable, live or physical promotion
limitations: no audit projector/query/export, durable projection checkpoint, incident workflow, external effect or live/physical audit proof is claimed; EventLog remains the sole fact source
reviewer: Codex source review of deny-first audit kind filtering, strict envelope/digest/source binding, redaction metadata and no-write validation; no local runtime test reviewer
```

