# PD-05 EventStore Adapter Conformance Baseline

## Scope

PD-05 adds a CI-only conformance fixture over the existing `EventStorePort` for the Memory and
JSONL adapters. The same transition batch must produce Committed, same-command Replay, stale
read-set Conflict, command receipt, bounded cursor page and aggregate stream results. A non-atomic
adapter remains explicitly unsupported for transition writes rather than silently appending facts.

The fixture does not change EventStore implementation or claim JSONL durability beyond its current
capability declaration. PD-06 owns frame/checksum/fsync/torn-tail hardening; PD-07 owns indexes and
page boundaries. Runtime tests run in GitHub Actions only.

## Evidence and limits

- `kiana-eventlog/tests/pd05_adapter_conformance.rs` covers Memory/JSONL parity and non-atomic
  rejection, including Committed/Replayed/Conflict/receipt/cursor semantics.
- `kiana-core/tests/pd05_eventstore_guard.rs` protects the single EventStorePort boundary.

This slice is `feature_status=implemented`, `proof_level=source`: no new durable guarantee,
cross-process race, kill/reopen recovery or physical storage proof is claimed; those remain PD-06+.
