# CO-24 immutable Company EvidenceBundle baseline

## Scope

CO-24 adds a strict execution evidence contract linked to the actual Run, Invocation and request
receipt. `CompanyEvidenceBundle` requires a terminal `RuntimeReceiptRef`, command digest, exact
source/workspace revisions, exit code, file changes, project/packet-owned artifact hashes and
nonzero matched test results. A `CompanyEvidenceReady` fact binds the bundle digest and source
cursor for later review.

Model claims, ResultUnknown, missing exit codes, zero test matches, source drift and foreign
artifacts fail closed. The bundle remains immutable after digest creation; business Acceptance and
Review remain separate decisions.

## Implemented source slice

- `CompanyEvidenceFileChange`, `CompanyEvidenceTestResult` and `CompanyEvidenceArtifact` validate
  canonical paths, hashes, exit status, source revision and exact project/packet/run ownership.
- `CompanyEvidenceBundle` binds RuntimeReceipt request/status to invocation/run and rejects
  self-reported/model-only or unknown outcomes.
- Core `company_evidence.rs` is a pure ingest adapter producing `CompanyEvidenceReady`; existing
  invocation projection and dispatch receipt facts remain the runtime evidence source.
- GitHub-only fixtures cover model/Unknown/zero-match/source-drift/foreign-artifact rejection,
  successful ready projection, run/invocation binding and serialization.

## CI-only evidence

`.github/workflows/co24-company-evidence.yml` runs formatting, the domain evidence fixtures, the
Core source guard and domain/core/daemon test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- This is a typed source/ingest contract; full EventLog result ingestion, durable artifact bytes,
  workspace snapshot capture and cross-process evidence projector remain open.
- Test commands and hashes are supplied as bounded evidence fields and still require the actual
  receipt/artifact adapters to populate them. No external/live/physical outcome is claimed.
