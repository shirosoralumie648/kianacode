# PD-07 EventStore Index / Page Baseline

## Scope

PD-07 adds CI conformance for the existing EventStore indexes and cursor page boundary. Command
lookup, request index, aggregate stream index and committed cursor pages must agree after Memory
commit and JSONL reopen. A page limit may not split a committed TransitionBatch; a cursor inside a
transaction is rejected, while `has_more` and the next boundary remain deterministic.

No second index authority is introduced: the indexes are rebuildable adapter state over committed
facts. PD-08+ owns integrity/quarantine/health and PD-09 owns projector checkpoints.

## Evidence and limits

- `kiana-eventlog/tests/pd07_indexes_page.rs` covers multi-event transaction pages, mid-frame cursor
  rejection, command/request/stream lookups and JSONL reopen parity.
- `kiana-core/tests/pd07_indexes_guard.rs` protects the source-cursor/index-only boundary.
  GitHub Actions runs fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no new durable recovery,
cross-process index proof, integrity quarantine or projector persistence is claimed.
