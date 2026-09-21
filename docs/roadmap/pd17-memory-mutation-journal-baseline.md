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
