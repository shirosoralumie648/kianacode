# SC-24 Query data boundary baseline

## Scope

SC-24 makes query-side memory/index/cache/export visibility an explicit server-derived decision.
Scope digest, project root, data epoch, revocation and retention state are checked before derived
views are exposed.

## Implemented source slice

- `QueryDataBoundary` returns one digest-bound decision for memory, index, cache and explicit export.
- Revoked or retention-blocked data denies every derived view; cache policy remains a non-authority
  optimization and cannot re-enable export or memory visibility.
- The contract is pure query metadata and does not authorize capabilities or mutate the EventLog.

## Evidence boundary

GitHub Actions is the test authority for the focused query fixtures and source guard. Local tests
are intentionally not run. This step does not claim live export, durable index deletion or external
side effects.
