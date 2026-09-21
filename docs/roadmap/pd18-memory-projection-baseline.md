# PD-18 Memory projection baseline

## Scope

PD-18 makes the projected Memory read surface obey the same server-owned governance epoch and
scope that produced the projection. A row is filtered before ranking or prompt-visible retrieval;
this contract does not mutate EventLog facts, JSONL, indexes or caches.

## Implemented source slice

- `MemoryProjectionFence` binds record id/revision, requested project, policy revision and data
  epoch to a digestable allow/deny decision.
- Candidate, rejected/tombstoned, stale-epoch, cross-project, revoked-source and expired-retention
  rows are denied before lexical ranking; approved active rows remain searchable.
- The daemon applies the fence in `memory.search` and returns the server policy `data_epoch` in the
  result envelope so callers cannot mistake a stale projection for an empty store.
- The core governance projector rechecks the same epoch contract after rebuilding the governance
  snapshot; no second authority or index/cache execution path is added.

## Evidence boundary

GitHub Actions is the test authority for the focused domain fixture and daemon/core source guards.
Local tests are intentionally not run. This step proves source and remote-CI wiring only; it does
not claim durable index/vector/cache deletion, cross-process projector recovery, live policy effects
or physical erasure correctness.
