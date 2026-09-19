# CAP-22 MCP result/schema drift and connection isolation baseline

CAP-22 records the existing invocation-scoped MCP correctness boundary. Prepared snapshots bind
server config hash, protocol/server metadata, catalog digest and exact tool input/output schema;
config drift, catalog/list-changed, tool schema or argument drift invalidates the call before
dispatch. Results validate JSON-RPC/structured content/output schema/size and preserve `isError`
as business failure; a sent call with invalid result or unconfirmed stop becomes Unknown and is
never automatically reissued.

Each MCP invocation owns its process and workspace scope (`per_invocation`); no cross-owner or
cross-scope pool is enabled. Resource links are references only, not implicit downloads, and
approval/action hashes are rechecked through the existing ControlPlane boundary.

GitHub Actions runs the existing MCP lifecycle fixtures, CAP-22 source guard and workspace
compilation. No local runtime tests or smoke commands were run.
