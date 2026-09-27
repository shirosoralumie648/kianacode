# SW-07 fresh child materialization baseline

## Scope

SW-07 adds a strict materialization contract for child Cell/Session/Run/Attempt lineage. Parent and
child identities must be fresh, authorized input refs must match frozen input refs, the child scope
must be an explicit subset, and private parent history is never copied. The receipt binds authority,
template and policy revisions.

The Core facade is read-only. It does not create a runtime session, launch a runner, dispatch a
Broker capability, read Memory, or persist a DelegationPacket/EventLog fact.

## Evidence and limits

- `kiana-domain/tests/swarm_child.rs` covers fresh context, input authorization, parent session
  reuse, private-history and scope-expansion denial, and strict unknown fields.
- `kiana-core/tests/swarm_child_guard.rs` protects the no-history-copy/no-effect boundary.
- `.github/workflows/sw07-fresh-child.yml` runs fixtures, source guard, formatting and affected
  test-target compilation in GitHub Actions; local Cargo tests/build/check/clippy/smoke commands
  were not run.

This slice is `feature_status=partial`, `proof_level=source`: it does not prove daemon/runner
materialization, durable child lineage, cross-process recovery, provider execution or live/physical
outcomes.
