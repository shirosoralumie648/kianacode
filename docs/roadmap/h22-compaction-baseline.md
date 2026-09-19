# H22 working-state compaction baseline

## Delivered source slice

- Versioned `CompactSummary` carries goal, pending work, next action, bounded constraints and
  evidence-only decision/completed/verification references. Plain prose cannot assert a completed
  tool or approval.
- Runner compaction now emits a validated summary and preserves system messages plus the newest
  complete user/assistant/tool group instead of discarding every assistant/tool pair.
- Summary failure or an overlarge latest group falls back to the original view; it does not append
  a fake “no summary available” continuation that could make the run proceed without state.
- CI source fixtures cover pending pair preservation, goal extraction, evidence-ref denial and the
  no-placeholder boundary.

## Boundary and proof ceiling

The current summary is deterministic local extraction; a real admitted ModelClient compaction
request, one bounded retry, durable summary event and cross-process recovery remain later H23/H24
work. No business completion, approval consumption or tool result is inferred from the summary.
