# H25 replay, fault injection and Unknown reconciliation baseline

## Delivered source slice

- Existing `fault_matrix` is replay-only and deterministic across Prepare/Commit/Dispatch/Result/
  Flush/Projector/Export/Shutdown windows. It never kills a process, invokes Model/Broker,
  contacts a provider or appends a new fact.
- Each case binds source cursor/event IDs, effect_started/effect_known and explicit Unknown or
  Rejected status; domain validation prevents Unknown from being treated as safe or complete.
- `replay_diagnostics` compares deterministic projections, records first safe divergence, preserves
  Unknown/terminal conflicts and never performs recovery or retry. Existing recovery paths require
  evidence and explicit continuation.
- CI source guard covers the fault matrix, replay-only/no-effect boundary and reconciliation
  markers.

## Boundary and proof ceiling

This is deterministic source/CI evidence, not physical process-kill or cross-process crash proof.
Real executor evidence, job handles, filesystem revisions, provider effects and live replay remain
later PD/ER/SC rehearsal work.
