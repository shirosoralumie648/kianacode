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

## 2026-10-17 completion slice (CI-only)

The card's refusal paths (an audit fact missing actor/decision/reason/source cursor, and raw
secret/prompt material entering a record) are now pinned as executable fixtures instead of
relying on generic serde errors.

- `AuditRecordEvent::from_wire_value` is the strict wire decode for the only appended audit
  fact shape: missing (or `null`) required keys are reported by name as
  `audit_wire_required_field_missing:<field>` (`record.actor_ref`, `record.decision`,
  `record.reason`, `record.source_cursor`, `record.source_event_ids`, envelope
  `source_cursor`/`source_event_ids`/`record`), smuggled keys such as `prompt`, `transcript`,
  `messages` or `content` as `audit_wire_unknown_field:<field>`, any remaining shape error as
  `audit_wire_payload_invalid`, and the decoded value is re-validated before it is returned.
  `AUDIT_EVENT_WIRE_FIELDS`/`AUDIT_RECORD_WIRE_FIELDS` (plus the explicitly required subsets)
  are the single whitelist the decode uses; `payload_recoverable`, `redaction_profile` and
  `attributes` must be stated explicitly on the wire instead of falling back to serde defaults.
- `kiana-eventlog/src/audit_contract.rs` now decodes through that entry point and maps a
  missing required key to `eventlog_audit_field_missing:<field>`, keeping
  `eventlog_audit_payload_invalid` for unknown/smuggled keys and
  `eventlog_audit_envelope:<reason>` for envelope invariants (including
  `audit_reason_unredacted` and `audit_event_schema_version_incompatible`).
- Redacted-field contract: every audit text field (actor, target kind/ref, retention class,
  reason, reason_code, correlation/causation refs, attribute keys and values) refuses raw
  secret-shaped text with its own stable code (`audit_<field>_unredacted`).
- Versioned event tests: the registered `kiana.audit-event.v1` contract is checked
  (`RuntimeEvent` layer, `kiana-domain` owner, `BackwardCompatible`, unknown fields refused),
  a minor bump re-signed by the producer stays valid and appendable, and a major bump or schema
  name drift is refused both by `validate()` and by the wire decode.
- Fixtures: `kiana-domain/tests/sc31_audit_record.rs` (6 tests),
  `kiana-eventlog/tests/sc31_audit_event.rs` (4 tests, deny-first before the accepted append)
  and `kiana-protocol/tests/sc31_audit_record_wire.rs` (3 tests). The core guard
  `kiana-core/tests/sc31_audit_record_guard.rs` binds the fixture names, refusal-code matrix,
  single-authority negatives (no `audit.record` payload contract in the generic domain event
  registry, no update/delete audit operation) and the shard/lane wiring.
- `.github/workflows/sc31-audit-record.yml` is still `workflow_dispatch`-only with
  `permissions: contents: read`; it now fetches locked dependencies, runs the format gate, all
  four targets with `--test-threads=1` and a `--no-run` compile pass. Automatic coverage stays
  with the unified `ci.yml` shards (not changed): `kiana-domain-s4/4` (`sc31_audit_record`),
  `kiana-core-s6/6` (`sc31_audit_record_guard`) and the whole-crate `kiana-eventlog` /
  `kiana-protocol` shards (`targets: null`).

Not claimed: no `audit` row is written by this slice, the reducer's server-side normalization of
a missing reason (`reason_unspecified`) is unchanged, and there is no change to the audit query
projector (SC-32), export, retention or incident workflows. Local tests were not run
(CI-only session).

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

