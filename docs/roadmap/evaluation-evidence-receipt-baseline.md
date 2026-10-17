# EQ-30 evidence and receipt evaluator baseline

## Scope

EQ-30 adds `EvidenceReceiptEvaluator` to `kiana-quality`. The typed evidence envelope binds
artifact references and strict SHA-256 digests, receipt digest/cursor/artifact links, required
receipt assertions, redaction/secret-free status, provenance and source cursor/event IDs. Missing
artifacts, hash drift, unverified receipts, assertion gaps, redaction failure, provenance drift or
cursor mismatch emit stable blocking findings.

The evaluator consumes caller-supplied evidence only. It does not read artifact or receipt stores,
recompute EventLog facts, fetch provider receipts, execute a capability, or make a release or
authorization decision.

## Completion slice (2026-10-17, CI-only)

The original slice existed in source; this completion slice closes the card's exit conditions and
turns the first version into a full, reject-first evidence contract:

- Artifact/receipt link containment is checked in **both** directions: a receipt artifact without
  artifact evidence is `evidence.artifact_missing` (the card's acceptance condition), and artifact
  evidence the receipt never links is `evidence.artifact_unreferenced`. Duplicate artifact
  references are `evidence.artifact_ref_invalid` per duplicate entry.
- Artifact digests must be a strict `sha256:` + 64 hex digit pair; an unparsable pair is reported
  once as `evidence.artifact_hash_invalid` instead of also emitting a redundant mismatch, while a
  valid pair with different values is `evidence.artifact_hash_mismatch` (case-insensitive hex
  comparison). `evidence.artifact_hash_unverified` covers an unverified observed digest.
- The source cursor is a bounded, ordered range: positive start, start <= end, span wide enough to
  contain the declared `event_count`, `event_count` equal to the number of event IDs, and unique
  bounded event IDs (`evidence.source_cursor_invalid` / `evidence.source_event_id_invalid`).
  Receipt and artifact cursors must stay inside the range (`evidence.receipt_cursor_mismatch`,
  `evidence.artifact_cursor_invalid`).
- Provenance is bound on all three sides: bounded provenance/snapshot references
  (`evidence.provenance_missing`), strict fixture/environment digests
  (`evidence.provenance_invalid`), and `evidence.provenance_mismatch` for a receipt or artifact
  whose `provenance_ref` drifts from the envelope.
- Redaction is enforced on the receipt bundle *and* per artifact (`evidence.redaction_missing`,
  `evidence.artifact_unredacted`), and any non-secret-free status or raw payload count is
  `evidence.redaction_secret_detected`.
- Receipt assertions are ordered by canonical identity and evaluated per entry: a required
  assertion without a value is `evidence.receipt_assertion_missing`, a present but different value
  is `evidence.receipt_assertion_mismatch`, and unbounded/duplicate assertion identities or values
  are `evidence.assertion_invalid`.
- Hard input bounds moved into `EvidenceReceiptInput::validate`
  (`evidence_receipt_input_schema_invalid`, `evidence_receipt_artifact_limit_exceeded`,
  `evidence_receipt_assertion_limit_exceeded`, `evidence_receipt_event_limit_exceeded`) so
  structural drift fails closed before evaluation; entry-shape problems remain findings.
- Artifacts, event IDs and assertions are sorted before evaluation, so the finding set does not
  depend on the caller's array order while duplicate evidence stays duplicated and visible.
- Findings carry per-section evidence references (`evidence:provenance`, `evidence:source-cursor`,
  `evidence:receipt`, `evidence:redaction`, `evidence:artifact:<index>`,
  `evidence:assertion:<index>`) and every caller-supplied value goes through `Finding::redacted`,
  including secret-shaped references and assertion values.

## Evidence and limits

- `kiana-quality/tests/eq30_evidence.rs` (17 rejection-first fixtures) covers the complete bundle,
  the card's acceptance scenario `missing_evidence_is_blocking`, hash drift and unverified
  digests, receipt assertions (missing/mismatched/optional), redaction and secret-free status,
  provenance drift/missing/invalid, source cursor range and event ID bounds, receipt/artifact
  cursor containment, both link directions plus duplicates, schema drift on all six evidence
  sections, unknown-field/envelope drift failure, bounded references and assertion values,
  oversized input rejection, order independence plus duplicate evidence, the redaction boundary
  over caller values, typed round-trip, and deterministic evidence-bound output wired through
  `EvaluatorRegistry`.
- `kiana-quality/tests/eq30_evidence_guard.rs` protects the pure fail-closed boundary (no
  filesystem/network/process/provider/store/broker dependency), the required markers, the fixture
  matrix, the manual lane's exact steps and the automatic `kiana-quality` shard (`targets: null`).
- `.github/workflows/eq30-evidence-receipt.yml` runs `cargo fetch --locked`, the format gate, both
  fixtures targets and two `--no-run` compile steps in GitHub Actions; local tests are
  intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim durable
ArtifactStore/Receipt truth, real provider receipts, cross-process recovery, quality aggregation,
promotion authority, live or physical evidence. Later evaluator/gate and ER/PD/SC steps remain
open.

## Known limits

- Findings are diagnostics over caller-supplied values only; they never re-hash artifact content,
  read a receipt projection or derive facts from the EventLog. Mapping them into blocking rules
  belongs to EQ-42.
- `digest_verified` is caller-asserted evidence. The evaluator can refuse an unverified or invalid
  digest, but it cannot prove that a real store recomputed it.
- References are restricted to a bounded ASCII identity alphabet; unknown shapes are rejected as
  invalid rather than normalized.
- `Finding::redacted` covers known secret shapes; unmarked high-entropy values may still pass.
- The manual lane is `workflow_dispatch`-only; automatic coverage depends on the unified
  `ci.yml` `kiana-quality` shard (both targets are discovered because `targets` is null).
