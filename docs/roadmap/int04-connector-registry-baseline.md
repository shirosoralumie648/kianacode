# INT-04 Connector registry baseline

## Scope

INT-04 defines an immutable Connector registry snapshot with monotonic version, content digest,
signature digest and compare-and-swap replacement. Unknown adapters and content drift remain
closed; catalog projection is read-only metadata.

## Implemented source slice

- `ConnectorRegistrySnapshot` validates strict definition identity, local adapter allowlist,
  content/signature digests and registry version.
- `cas_replace` rejects stale writers, skipped versions and registry content drift.
- Existing daemon connector events remain the fact path; the new contract adds no direct adapter
  execution or automatic enablement.

## Evidence boundary

GitHub Actions is the test authority for the domain fixture and core source guard. Local tests are
intentionally not run. This step does not claim signed key verification, durable registry storage,
live adapters or external effects.
