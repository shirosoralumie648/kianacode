# ER-34 local durable gate baseline (CI-only proof ceiling)

## Scope

ER-34 adds `Er34DurableGateEvidence`, which separates a pending/source gate from a durable gate.
It binds snapshot and command digests, origin, CI/local execution flags, event/frame/artifact/
process facts, cache deletion/rebuild and limitations. A `durable`/`passed` claim requires explicit
CI test execution, observed durable facts, cache rebuild and all three evidence digests; a pending
CI-only source record remains `source` and cannot be promoted by text or historical commands.

The current user instruction forbids local test/build/check execution. Therefore this implementation
does not claim ER-34 durable completion; the GitHub workflow only validates the proof ceiling and
source guard. A later authorized remote/target run may produce a separate durable evidence block.

## CI-only evidence

`.github/workflows/er34-durable-gate.yml` runs formatting, proof-ceiling fixtures, source guard and
affected test-target compilation on GitHub Actions. No local Cargo tests, builds, checks, clippy or
smoke scripts were run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| `ci_only_pending_gate_does_not_promote_durable_proof` | pending GitHub/source evidence remains source |
| `forged_durable_claim_missing_physical_facts_is_rejected` | missing facts or source-level pass cannot claim durable |
| `durable_gate_requires_ci_facts_cache_rebuild_and_hashes` | complete durable evidence shape is structurally accepted |

## Limitations and handoff

- The complete durable shape is only a contract fixture; no target host, local filesystem, process,
  lock, usage or EventLog restart evidence was collected here.
- ER-34 remains partial and must not be promoted from source until the authorized gate actually
  produces its own snapshot, command, hashes, exit code and limitations.
- CI results are intentionally not awaited; proof level remains `source`.
