# CP-25 unified resource boundary baseline

## Delivered source slice

- Project skills/extensions are loaded only after the existing ProjectTrust/source resolver
  boundary; prompt sections remain context and `allowed_tools` is display/input metadata, not an
  authorization source. Extension scopes are rechecked by Broker `ExtensionAdmission`.
- Hooks use the existing bounded read-only confined path with input/output limits, timeout,
  cancellation, recursion-free sequential invocation and `hook.decision` facts; hook output cannot
  mutate authority or create a second model loop.
- MCP preparation pins project/actor/role/config/discovery/catalog/schema/health. Reconnect,
  config/catalog/tool-schema/argument/output drift rejects the request or preserves Unknown, and
  pending approval material is invalidated by action/environment digest changes.
- Memory search/write/review use server-derived scope, collection ACL, project/session/data policy
  and journal-before-projection checks. Model text, collection arguments and read-only query hints
  cannot widen scope or revive revoked data.
- SecretRef/CredentialLease carries only opaque reference metadata; purpose, audience, endpoint,
  actor and generation checks happen at the effect boundary and raw values never enter core,
  runner, EventLog or Broker receipts.

## Boundary and proof ceiling

This step adds a cross-layer source guard and CI workflow over existing paths; it does not create a
second permission engine or duplicate adapter logic. Provider live behavior, physical secret store
security, all-entrypoint production parity and durable cross-process recovery remain unclaimed.
