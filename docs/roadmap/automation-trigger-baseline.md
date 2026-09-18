# AUT-04 Trigger Definition Baseline

## Scope

AUT-04 hardens the existing Manual/Event/Interval trigger path before durable occurrence/CAS work.
`TriggerDefinition::validate_shape` bounds identity, owner/role/approval references, input JSON
size, expiry/firing budget, source event kind and interval arithmetic, and supplies a canonical
digest.  The pure planner then adds definition/version/input/role checks, server-owner and
approval-evidence checks, and existing concurrency/firing-key/interval due logic.

Event triggers require an owned `event:<id>` proof and exact source kind; Manual triggers reject an
event ref; Interval triggers use bounded `every_ms`/catch-up behavior.  Duplicate firing keys,
expired/over-quota triggers, cross-owner execution and active concurrency conflicts fail before an
instance is created.  The planner has no I/O or execution authority; ControlPlane resolves proof
and commits the command before routing any effect.

## Evidence and limits

- `kiana.domain::TriggerDefinition` is strict/deny-unknown-fields, schema-registered and digestable.
- GitHub-only fixtures cover all three schedule forms, malformed source/payload/expiry, unknown
  fields, owner/approval/source evidence, duplicate keys, quota and concurrency markers.
- Project/authority/policy revisions are still represented by the surrounding authenticated
  ControlPlane context rather than dedicated TriggerDefinition fields; AUT-05 owns durable
  occurrence/event/CAS query contracts.  No scheduler, live timer, external or physical proof is
  claimed.

This slice is `feature_status=implemented`, `proof_level=source`; local tests are intentionally not
executed.

