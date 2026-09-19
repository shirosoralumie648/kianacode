# SC-27 extension lifecycle and sandbox baseline

## Delivered source slice

- `kiana-domain` defines append-only lifecycle phases including revoke and uninstall,
  sandbox profiles, and callback permits bound to enabled extension/package/manifest digests.
- ReadOnly permits require a read-only sandbox and read-only capability set; revoked, uninstalled,
  stale-phase, network and write-capability callbacks fail closed.
- `kiana-daemon` now records uninstall as a lifecycle fact through the existing EventStore CAS
  path. It retains package references for audit/cleanup receipts rather than silently deleting the
  cache, and admission/skill context continue to accept only enabled states.
- Existing hook execution remains a read-only, bounded, cancellable `run_confined_cancellable`
  path with EventLog decision facts; bwrap/no-new-privileges and ambient-authority environment
  fencing remain the sandbox boundary.

## Boundary and proof ceiling

The lifecycle contract and daemon source guard do not claim physical cache deletion, production
hook semantics, cross-process lifecycle recovery, or a live external effect. Package cleanup,
secret/state retention and adapter receipts remain later lifecycle/retention work.
