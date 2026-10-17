# PD-17 Memory mutation journal baseline

## Scope

PD-17 binds the existing server-owned `MemoryMutation` contract to the existing EventStore-backed
`memory.fact` stream before the JSONL memory projection becomes visible. The journal carries the
mutation digest, authority class, lifecycle stage, exact target revision, scope digest and
idempotency key without creating a second execution path.

## Implemented source slice

- `MemoryMutationAuthority` is server-derived; agent mutations may create candidates, while
  approval and tombstone stages require human or service authority.
- `MemoryMutationJournalStage` validates candidate/draft/ephemeral, qualify/approve,
  supersede and tombstone state combinations against the materialized `MemoryRecord`.
- `MemoryJournalFact` optionally carries a typed mutation journal. Legacy facts remain replayable,
  while new daemon writes require the mutation digest and key to match the fact.
- Projection rebuild rejects duplicate mutation keys, origin-less native records, target/revision
  drift and invalid lifecycle transitions while preserving collection, state and record metadata.
- The daemon stamps human authority only on the operator review path and commits the journal fact
  before appending the JSONL projection.

## Evidence boundary

GitHub Actions is the test authority for the focused domain fixture and daemon source guard. Local
tests are intentionally not run. This step proves source and remote-CI wiring only; it does not
claim durable restart recovery, physical filesystem correctness, live approvals or external effects.

## 2026-10-17 completion slice (CI-only)

- `MemoryMutationJournalStage::validate_record` now rejects a non-Native import mode
  (`memory_mutation_journal_import_mode_invalid`), an unresolved origin
  (`memory_mutation_journal_origin_unresolved`) and, for agent-authorised mutations, any origin
  other than `Model` (`memory_mutation_journal_origin_forged`). A legacy import can no longer be
  materialized by a mutation, and a `hook`/`git`/`user` label can no longer ride the model path
  into an authoritative fact.
- `kiana-domain/tests/pd17_memory_mutation_journal.rs` now pins the deny column (forged origin,
  legacy import, missing evidence, forged actor, unknown authority, changed payload under a
  replayed ledger key) and the success column: a six-layer fixture replays eleven committed facts
  across candidate/draft/ephemeral/qualify/approve/supersede/tombstone and rebuilds every record
  with its collection, lifecycle state, provenance, reviewer, supersedes pointer and revision,
  while a replayed mutation key returns the original receipt without advancing the revision.
- `kiana-daemon/src/harness_memory.rs` adds two in-module fixtures (Unix): the model path keeps
  the server-stamped `Model`/`Candidate`/`Draft` row and Agent authority in the committed journal
  fact even when the arguments forge origin/admission/state/reviewer/evidence, while the
  operator-only review path refuses a missing operator stamp (`memory_review_operator_required`)
  and cannot promote an evidence-free candidate (`memory_review_evidence_required`) without
  mutating the row or the journal; a replayed mutation key produces exactly one fact and one
  JSONL row while a changed payload returns `memory_idempotency_conflict`.
- `kiana-eventlog/tests/pd17_memory_mutation_journal.rs` (new) pins the store contract: a
  replayed fact keeps the original event identity and version without a second row, changed
  payloads and stale expected versions conflict (`event_idempotency_key_payload_mismatch`,
  `event_stream_version_mismatch`), duplicate mutation keys under distinct fact keys fail the
  rebuild with `memory_mutation_journal_duplicate`, and a `JsonlEventLog` reopen rebuilds the
  promoted record.

Limitations: local tests were not run (CI-only session); compilation and runtime correctness
depend on GitHub Actions. The `JsonlEventLog` fixtures are Unix-only and reopen within one
process. Cross-process store isolation, crash/disk-failure recovery and durable/live/physical
proof remain open.
