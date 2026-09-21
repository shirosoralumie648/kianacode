# PD-24 Migration record baseline

## Scope

PD-24 binds an ordered migration registry and read-only preflight to a verified backup snapshot,
owner and checksum before a runner can report progress. The record is a pure fact contract and
never acquires locks or executes an upcaster itself.

## Implemented source slice

- `MigrationRecord` binds registry digest, preflight digest, backup snapshot ID, owner and ordered
  format-version range.
- The state machine permits only Planned→Running→Applied/Failed/Quarantined, with bounded retry
  attempts and mandatory failure reasons.
- Registry checksum drift, missing verified-backup identity and invalid transitions fail closed;
  existing MigrationRegistry/Preflight/Runner contracts remain the source of order and fencing.

## Evidence boundary

GitHub Actions is the test authority for the focused domain fixture and daemon source guard. Local
tests are intentionally not run. This step does not claim a durable lock, physical upcaster,
backup restore, cross-process runner or live migration effect.
