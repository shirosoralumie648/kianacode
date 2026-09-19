# CM-35 external context/resource adapter baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-35`](context-memory.md#step-cm-35) |
| feature_status | `implemented` (untrusted external request/snapshot scope, quota, provenance and revocation contract) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | MCP/resource/connector/import intent → processing grant + quota + equal scope digests → attributed SourceSnapshot → TTL/epoch/revocation gate |
| authority | external content is context evidence only; it cannot widen Memory scope, grant write authority or become Product instructions |

## Contract and behavior

`ExternalResourceRequest` normalizes MCP resources, connector results and user imports. It requires
content/locator/grant/quota/scope digests and bounded bytes/items, rejects scope widening and memory
write flags, and preserves untrusted semantics. `ExternalResourceSnapshot` binds the request to a
revision, source snapshot, data epoch and expiry; connector/import snapshots can be revoked, after
which `readable_at` is false.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `external_resource_cannot_widen_scope` | external request cannot change scope or enable memory write |
| `connector_source_is_revocable` | connector snapshot is readable only inside epoch/TTL and becomes unreadable after revoke |
| `external_resource_is_untrusted_scoped_quota_bound_and_revocable` | MCP/connector/context source guards retain untrusted, quota and revocation boundaries |

## Proof ceiling and handoff

The CM-35 ceiling is `source` plus remote CI wiring. No external network call, MCP transport,
connector business effect, import persistence or live remote provenance is claimed; adapters must
obtain real grants/quotas and feed the same snapshot/revocation contract.
