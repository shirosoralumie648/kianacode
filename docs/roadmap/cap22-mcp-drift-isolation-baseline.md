# CAP-22 MCP result/schema drift and connection isolation baseline

CAP-22 records the existing invocation-scoped MCP correctness boundary. Prepared snapshots bind
server config hash, scope identity, protocol/server metadata, catalog digest, discovery version and
the exact tool input/output schema; config/env/executable drift, catalog/list-changed, tool schema
or argument drift invalidates the call before dispatch. Results validate JSON-RPC bounds, structured
content, content types/encodings and output schema; `isError` stays a tool business failure while
an invalid result after a sent call becomes Unknown and is never automatically reissued.

Each MCP invocation owns its process (`per_invocation`) and its workspace scope; no cross-owner or
cross-scope pool exists. The discovery key binds actor/role/department/project so a prepared
authorization cannot be replayed under another owner or scope, and resource links are references
only, not implicit downloads.

## Behavioural fixtures

The fixtures live in the `#[cfg(test)] mod tests` of `kiana-daemon/src/harness_mcp.rs` (Linux
only). They drive the production `McpRegistry::prepare` + `McpHandler::run` path and the same
bwrap-confined, per-invocation `crate::mcp_stdio::ConfinedMcpClient` that `McpHandler` uses, against
a stdlib-only Python stdio server (`CAP22_MCP_SERVER`). The server selects a behaviour from
`sys.argv[2]`, carries a unique marker as `sys.argv[1]` (the host scans `/proc/*/cmdline` to prove
that no marked descendant survives a stop path instead of trusting the child's own report), and
reads cap22-catalog.json to select catalog generation 1 or 2 so re-discovery can commit a drifted
catalog under the same discovery key. `read-only` is the default fixture scope; `workspace-write`
is used only where the host needs the server's own `cap22-trace.jsonl` record of what the confined
process actually received.

Deny-first fixtures, in the order the focused lane runs them:

| fixture | what it proves |
|---|---|
| `mcp_output_schema_failure_after_call_is_not_safe_retry` | three invalid structured results (`structured_missing`, `structured_wrong`, `bad_content`) received after a call was sent all stay `ResultUnknown` with `commit`/`effect` Unknown and `reconciliation_required`, retain the private workspace with reason `mcp_call_unconfirmed`, write their reconciliation record, and reach the server exactly once with no automatic retry |
| `mcp_schema_drift_invalidates_pending_approval` | catalog v1→v2 committed under the same discovery key changes digest/version/`capability_action_digest`/tool metadata; the stale v1 authorization is refused with `mcp_catalog_changed` before any process starts; restoring v1 commits version 3 so the old authorization stays refused, and only a freshly prepared authorization runs (`cap22-catalog\|v2` then `cap22-catalog\|v1`) |
| `mcp_pool_never_crosses_owner_or_scope` | owner B cannot reuse owner A's discovery (`mcp_discovery_required`), the discovery key binds the owner scope, tampering with `actor_id`/`role_id`/`department_id`/`project_root` fails with `mcp_snapshot_scope_changed` before dispatch, and two owners running at once are observed as two simultaneous confined processes (`per_invocation`, `bwrap`) each receiving only its own result instead of one pooled session |
| `mcp_explicit_revoke_and_config_change_fence_prepared_authorization` | an untrusted project is refused at preparation (`mcp_project_untrusted`) without starting a process; an added explicit env reference fences the old authorization (`mcp_config_changed`) and the changed configuration has no committed discovery (`mcp_discovery_required`); a swapped binary fences the old authorization (`mcp_config_changed`) and a new call preparation requires a fresh discovery (`mcp_config_drift_requires_discovery`) |

Bounded-result regression fixtures:

| fixture | what it proves |
|---|---|
| `mcp_legal_results_errors_and_resource_links_stay_bounded` | the discovery catalog is sorted by name; a schema-matching structured result is successful with `structuredContent`, `output_schema_hash=sha256:…`, `untrusted_data=true` and `resource_links_fetched=false`; `isError` is a tool business failure (`Committed`/`Failed`, no reconciliation); a resource link is returned as a reference without any embedded fetch |
| `mcp_result_validator_bounds_structured_content_and_content_types` | the validator itself refuses a missing or mismatched `structuredContent` (`mcp_structured_content_required`/`mcp_output_schema_mismatch`), an invalid base64 image, an unknown content type and a scheme-less URI (`mcp_content_invalid`), and more than 128 content items (`mcp_content_limit`), while legal structured and `isError` results still pass |

## CI wiring

`.github/workflows/cap22-mcp-drift-isolation.yml` installs and verifies `bubblewrap` and `python3`,
then runs the `harness_mcp::tests::` module, each CAP-22 fixture by name with `--exact` and
`--test-threads=1`, the P1-J4-01 lifecycle target, the CAP-22 source guard and the `--no-run`
compilations. The unified `ci.yml` `kiana-daemon` shard runs the whole library test suite with the
same runtime installed, so the fixtures also gate regular pushes. A missing `bwrap`/`python3` fails
the fixtures instead of degrading to an unsandboxed call.

`kiana-core/tests/cap22_mcp_drift_isolation_guard.rs` adds
`cap22_behavioural_fixtures_are_wired_and_deny_first`: every fixture name must exist in the daemon
source, in the focused lane and in this baseline, the fixture markers and runtime verification stay
wired, no fixture may be `#[ignore]`d, the scope/catalog checks must stay ahead of the
per-invocation connect, and the lane must not grant sandbox bypasses.

## Evidence and limitations

- Proof level: `source`. The fixtures are compiled and run only in CI. No local runtime tests or
  smoke commands were run while authoring them, and no local `cargo test`/`fmt` execution is
  claimed.
- The `/proc` marker scan is a bounded host observation of the current namespace; it does not
  claim anything about descendants unreachable from that namespace. The two-owner overlap fixture
  observes two simultaneous confined processes; it does not prove per-owner kernel isolation
  beyond the existing bwrap boundary.
- Fixtures require `bwrap` and a Python 3 interpreter, and only compile on Linux.
- The fixtures cover the stdio transport only; HTTP/SSE MCP remains explicitly unsupported.
- Catalog/result drift is proven at handler level against `MemoryEventLog`; cross-process
  durability, reconciliation execution, live interoperability, the card's pooled-connection suffix
  and later CAP-24 evidence remain open.
