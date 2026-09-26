# PD-35 persistence/data-layer closeout and migration handoff

PD-35 is a documentation and release-gate slice. It indexes PD-00..PD-34 evidence without
creating a second status authority or upgrading any implementation. The detailed persistence
design remains [`persistence-data-layer.md`](persistence-data-layer.md); the current proof ledger
remains [`CURRENT_STATUS.md`](../../CURRENT_STATUS.md).

Covered identifiers: `PD-00`, `PD-01`, `PD-02`, `PD-03`, `PD-04`, `PD-05`, `PD-06`, `PD-07`,
`PD-08`, `PD-09`, `PD-10`, `PD-11`, `PD-12`, `PD-13`, `PD-14`, `PD-15`, `PD-16`, `PD-17`,
`PD-18`, `PD-19`, `PD-20`, `PD-21`, `PD-22`, `PD-23`, `PD-24`, `PD-25`, `PD-26`, `PD-27`,
`PD-28`, `PD-29`, `PD-30`, `PD-31`, `PD-32`, `PD-33`, `PD-34`.

## Status and proof ceiling

| Range | feature_status | proof_level | Handoff |
|---|---|---|---|
| PD-00..04 contracts/ports | partial | source | typed boundaries and unsupported defaults; no production adapters |
| PD-05..09 EventStore/JSONL/projector | partial | source | local adapter/source guards; no cross-process/power-loss durable proof |
| PD-10..21 startup, artifacts, memory/index and cache | partial | source | projections/cache remain rebuildable; no unified durable root/retention |
| PD-22..26 backup/restore/retention/delete | target/partial | source | manifests and gates are not backup/restore/delete effects |
| PD-27..32 backpressure, security, diagnostics, faults and conformance | partial | source | budgets/guards do not prove platform or crash behavior |
| PD-33 lifecycle UAT | partial | source | CI matrix only; no real backup/restore/restart/delete |
| PD-34 capacity budget | partial | source | supplied P95/P99 and pressure facts; no production stress measurement |
| PD-35 closeout | partial | source | this handoff gate validates documentation structure only |

The closeout evidence index includes `PersistenceUatEvidence` for PD-33 and
`PersistenceCapacityEvidence` for PD-34. These manifests bind matrix/report/source digests to
receipts and explicit proof ceilings; their CI fixtures remain source evidence and do not alter
the underlying PD-33/PD-34 status.

No row is promoted to `durable`, `live` or `physical` by this table. Unknown, stale, corrupt,
unverified or unsupported states remain visible and block authorization.

## Migration/backup operator handoff

1. Resolve `StorageRoot`, owner/scope, store identity, schema/store/projection versions and current
   authority/data epochs through the existing resolver and ControlPlane.
2. Run read-only preflight. Refuse unknown major versions, cursor gaps, active writers, stale
   leases, insufficient space, checksum drift, missing verified backup or unresolved effects.
3. Quiesce and verify the snapshot before migration. Bind every manifest/file/chunk to source
   cursor, generation, epoch, owner and digest; do not treat a path or mtime as identity.
4. Apply only ordered, bounded, idempotent forward steps under the migration lease/fence. Persist
   step facts and quarantine failures; do not silently downgrade or retry an Unknown effect.
5. Rebuild projector/index/Receipt views from facts, verify source/projection cursor and generation
   parity, then re-admit authorization/approval state. A restored root is not active until an
   explicit approved activation.
6. Keep the old root read-only through the retention/legal-hold dependency graph. Governance delete
   requires a dry-run, dependent-artifact proof, tombstone/data-epoch propagation and a committed
   deletion receipt; cleanup failure is not a successful delete.

The commands, approvals, environment, fixture/cassette, exit code, limitations and reviewer for
each actual operation must be recorded in a `CURRENT_STATUS.md` evidence block. CI fixtures and
source guards are not durable storage or production migration receipts.

## Release gate

The PD-35 CI gate checks this file, the detailed PD roadmap, the evidence ledger, the module map,
the DEP-41 operator runbook/matrix and all PD-00..PD-34 identifiers. It also requires explicit
`feature_status`, `proof_level`, limitations and reviewer fields, and parses every closeout table row
for four non-empty columns and allowed status/proof values. It cannot certify a backup,
restore, migration, deletion, capacity run or platform filesystem behavior.
