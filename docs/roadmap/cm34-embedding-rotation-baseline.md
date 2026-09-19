# CM-34 offline embedding manifest and rotation baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-34`](context-memory.md#step-cm-34) |
| feature_status | `implemented` (offline package manifest, index binding and generation rotation contract) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | installed local package manifest → exact embedding/index binding → new generation rotation with old read-only |
| authority | manifest/generation contracts gate local derived index readers; no runtime package download is authorized |

## Contract and behavior

`EmbeddingManifest` pins model/package/weights/tokenizer/config digests, dimensions, pooling,
provider, device, normalization and `network_allowed=false`. `EmbeddingIndexBinding` requires the
exact manifest digest and source/index component digests. `EmbeddingRotationPlan` requires a strictly
new generation, distinct embedding digest and keeps the old generation read-only until the new one
is validated and switched.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `embedding_manifest_mismatch_fails_closed` | a rotated manifest or network-enabled manifest cannot validate an old index binding |
| `model_rotation_keeps_generation_consistent` | old/new generation readers accept only their exact embedding manifest digest |
| `embedding_manifest_and_rotation_are_offline_and_generation_bound` | source guard preserves hash/dimension/provider/device and no runtime download boundary |

## Proof ceiling and handoff

The CM-34 ceiling is `source` plus remote CI wiring. No ONNX runtime execution, package signature
verification, hardware performance, semantic embedding quality or production index rebuild claim is
made; adapters must provide installed local artifacts and continue using the binding contract.
