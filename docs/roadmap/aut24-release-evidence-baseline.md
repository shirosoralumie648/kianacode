# AUT-24 durable/live release evidence baseline

## Scope

AUT-24 adds a fail-closed release evidence manifest. Every row carries feature status, proof level,
evidence reference, next gate and limitations. Duplicate rows, blank next gates, blanket completion
and live/physical proof without implemented status are rejected. Source/CI evidence remains source;
durable/live/physical requires independent artifacts and approval in a later handoff.

The manifest is a read-only index and does not publish, release, run a provider, consume a claim or
change a feature state.

## CI-only evidence

`.github/workflows/aut24-release-evidence.yml` runs formatting, domain proof-manifest fixtures, the
Core release boundary guard and affected test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| `release_gate_keeps_source_rows_and_next_gates_explicit` | source rows remain source with limitations and next gates |
| `blanket_completion_live_without_evidence_and_duplicate_steps_are_rejected` | blanket, unimplemented live and duplicate rows fail closed |
| `durable_gate_requires_ci_facts_cache_rebuild_and_hashes` | durable shape requires all evidence fields |

## Limitations and handoff

- No release artifact, durable restart, live provider, physical target or operator approval was
  executed; the manifest cannot raise proof level itself.
- AUT-24 remains partial until each downstream gate has its own inspectable evidence block.
- CI results are intentionally not awaited; proof level remains `source`.
