# PD-25 Retention watermark baseline

## Scope

PD-25 adds a finite retention watermark contract. A sweep can only commit after legal-hold and
tombstone gates pass; cursor advancement is monotonic and bounded, and blocked/unknown work keeps a
visible reason instead of silently deleting facts.

## Implemented source slice

- `RetentionWatermark` binds store, policy revision, data epoch, source cursor, upper bound and
  batch limit.
- Planned → Committed requires both legal-hold and tombstone confirmation; missing gates produce a
  blocked watermark.
- Cursor advancement cannot regress or exceed the finite upper bound, and every state carries a
  stable digest.
- Existing EventLog `RetentionStorePort` remains the only fact/receipt boundary; no purge worker or
  unbounded delete loop is introduced.

## Evidence boundary

GitHub Actions is the test authority for the focused domain fixture and core/eventlog source guard.
Local tests are intentionally not run. This step does not claim physical pruning, archive storage,
cross-process scheduling, or live deletion effects.
