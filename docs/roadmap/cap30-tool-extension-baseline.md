# CAP-30 dynamic tool search and extension admission baseline

CAP-30 currently has a partial source slice. The immutable ToolCatalogSnapshot/ToolAuthority
contracts bind model-visible schemas, aliases, capability/effect/risk, brokered execution,
replay class, scheduling/resource metadata and catalog digest. The current `tool.search` executor
filters the built-in catalog by server-owned role tools and returns `does_not_grant_execution`.

Extension manifests and daemon admission already bind publisher signature, package/content hash,
license, effect, required capabilities, policy/platform/dependency constraints, capability
namespace, registry CAS version, upgrade/revoke/rollback and extension execution scope. Tool
search now uses deterministic exact/prefix/token ranking with result/schema-byte bounds and a
catalog digest; it remains discovery-only. The bounded search response also binds catalog
version/health, optional replay-safe filtering and selected-schema context-token accounting.
Skill context is trust/role filtered and extension activation is an EventLog fact; extensions
cannot self-grant or replace a built-in binding.

Remaining CAP-30 work is explicit rather than silently promoted: BM25/tag ranking, extension
descriptor indexing and a CI fixture for a read-only extension plus an approved side-effect extension.
GitHub Actions runs the current catalog/extension fixtures and source guards. No local runtime
tests or smoke commands were run; this step remains `partial`.
