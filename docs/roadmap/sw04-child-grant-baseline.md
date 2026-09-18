# SW-04 Child Grant Admission Baseline

## Scope

SW-04 closes the first Swarm authority-admission slice.  A controller remains an admission and
supervision Cell only; it does not create a second model/runtime loop.  Before a child Cell can be
reserved, the child grant is derived by intersecting six server-owned layers:

`parent ∩ template ∩ department ∩ project ∩ packet ∩ approval`.

The result is a fresh, non-delegable `coding / builder.packet` grant with the partition's paths and
data scope, bounded expiry and parent resources.  A secret/network/provider grant, forged role or
template capability, cross-project packet, stale authority epoch, path superset or budget superset
fails before CellRegistry admission.  CellRegistry repeats parent containment and budget-subset
checks during both live reserve and snapshot restore.

`SwarmController` now carries server-generated authority epoch and opaque principal/project IDs;
packet ownership and current authority epoch are rechecked when a child is materialized.  The
controller's union path is never copied into a child as authority.

## Evidence and limits

- `kiana-policy::GrantScope::intersect_all` is the pure monotonic policy operation; conversion to
  the compatibility `CapabilityGrant` checks capability/operation/secret/external dimensions.
- `kiana-core::derive_swarm_child_grant` is pure and creates no Broker permit, Cell, session or
  model execution.  `reserve_packet_cell` is the only caller in the product path.
- CI fixtures cover narrow partition paths, non-delegation, forged project/path/role/epoch and
  source-level CellRegistry fences.  Local tests are intentionally not executed; GitHub Actions
  runs the runtime fixtures.

This slice is `feature_status=implemented`, `proof_level=source`.  The principal/project fields
remain local opaque compatibility IDs until the durable identity/SharingGrant projector is wired;
DispatchIntent/QueueEntry atomic admission, cross-process grant/budget recovery, complete approval
material and fresh child Session/Run/Attempt remain SW-05+ and CP/ER/PD/SC work.  No external/live/
physical effect proof is claimed.

