# SC-22 retention and legal-hold baseline

## Delivered source slice

- `kiana-domain` defines versioned `RetentionPolicy`, `LegalHold`, `LegalHoldReceipt`,
  `RetentionDecision`, and `RetentionScan` contracts.
- A policy requires an explicit finite default window; zero/unbounded defaults are rejected.
- `kiana-core::scan_retention` evaluates only a validated DataGovernanceSnapshot, rejects
  project/policy/data-epoch/cursor drift, treats unknown source state as `Unknown`, and gives an
  active legal hold precedence over expiry.
- Every active hold produces a receipt bound to the same policy revision, source cursor, source
  event IDs, and projection cursor as the scan.
- `kiana-eventlog::MemoryRetentionStore` stores validated scans/receipts and exposes a bounded
  retention plan through `RetentionStorePort`; it is explicitly non-durable.

## Boundary and proof ceiling

The scan is a source-backed plan, not a deletion result. EventLog facts are not erased, the
projection is not an authority source, and tombstone/purge remain explicitly deferred to SC-23.
The adapter is an in-memory CI fixture; durable restart recovery, physical deletion, external
replica propagation, and live compliance evidence are not claimed here.
