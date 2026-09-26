# EQ-51 evaluation evidence archive baseline

## Scope

EQ-51 defines a strict, source-level index for the four evaluation outputs produced by the
preceding quality steps:

- `report.json` from EQ-48;
- `trace-diff.json` from EQ-21;
- `evidence-manifest.json` from EQ-48;
- `reproduction.txt` from EQ-48.

`QualityEvidenceArchive` also carries every field required by the `CURRENT_STATUS.md` evidence
block: source snapshot, worktree status, command argv, cwd/environment, fixture/cassette, exit
code, status change, proof-level change, limitations and reviewer.  Artifact references are
project-relative and must stay below `artifacts/eq51/`.  The domain contract only validates
caller-supplied facts and renders JSON; the Core facade does not execute a provider, evaluator,
broker, runner, filesystem write or release publish.

Each artifact carries its expected schema binding: EQ-48 `QualityReport`,
`QualityEvidenceManifest` and `QualityReproductionCommand`, plus EQ-21's
`kiana.quality-trace-diff.v1`.  A path or schema cannot be reassigned to another artifact kind.

## Failure-first boundary

The archive rejects unknown fields, missing evidence-block fields, duplicate or missing artifact
kinds, duplicate paths, absolute paths, `file://` references, path traversal, unredacted secret
markers, invalid SHA-256 digests, forged archive digests, and any proof level above `source`.
`feature_status=implemented` is also rejected because this slice is a source/CI contract and does
not prove a completed evaluation or an uploaded durable artifact.

## CI-only evidence

`.github/workflows/eq51-evidence-archive.yml` runs the shell static guard, domain fixture, Core
read-only guard, formatting and affected-target compilation in GitHub Actions.  The final step
uploads the four expected archive slots when a preceding CI lane has produced them; absent slots
warn rather than becoming a fabricated pass.  Local Cargo tests, builds, checks, clippy and smoke
commands were not run, and remote CI results are not awaited.

```text
feature_status: partial
proof_level: source
```

## Fixture matrix

| Fixture | Assertion |
|---|---|
| `archive_has_complete_source_evidence_block_and_four_artifacts` | all required fields, four kinds and source proof validate and render |
| `archive_rejects_missing_fields_paths_and_secrets` | blank reviewer, traversal path and secret marker fail closed |
| `archive_rejects_duplicate_or_unavailable_proof` | duplicate kind, `implemented` status and durable proof cannot pass |
| `eq51_archive_is_read_only_and_uses_existing_quality_facts` | Core/domain source guard finds the contract and rejects an execution path |

## Limitations and handoff

This is a source/CI-only archive contract.  It does not generate a report, run `TraceDiff`, read a
fixture, persist an EvalStore, prove GitHub runner success, or establish durable/live/physical
quality or business outcomes.  Artifact upload is conditional on upstream CI-produced files and
does not promote `proof_level`; a reviewer must use returned CI artifacts and independent evidence
before changing the status ledger.
