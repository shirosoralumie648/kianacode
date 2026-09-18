# CP-28 migration / compatibility / bypass baseline

## Scope

CP-28 keeps compatibility explicit and non-authorizing.  Versioned event/storage registries own
named migrations; legacy facts remain queryable but missing authority requires reauthorization;
JSONL old writers cannot mutate an upgraded journal; daemon/entrypoint/client constructors all
reuse the same ControlPlane spine and dependency-boundary rules.

## Evidence gate

- `cp_legacy_authority_records_require_reauthorization` checks event/storage upcasters, old
  Continue/identity/approval records and recovery read-only/reauthorization boundaries.
- `cp_old_writer_cannot_mutate_new_journal` checks JSONL/journal/memory writer-version fences,
  atomic-transition capability denial, migration dry-run/read-only limits and corruption handling.
- `cp_every_product_constructor_enforces_the_same_boundary` checks DaemonHost/harness/CLI/client
  constructors, architecture status and strict internal dependency/legacy-edge guards.

The GitHub workflow runs the existing schema, event-contract, JSONL, resume, dependency and
architecture fixtures, then the CP-28 source guard and workspace test-target compilation.
Local runtime tests are not run.

## Limits

This is source plus CI-fixture evidence only.  It does not claim a production upgrade rehearsal,
cross-version distributed writer race proof, physical backup/restore proof or removal of every
legacy compatibility crate; legacy edges remain explicitly measured until later cleanup gates.
