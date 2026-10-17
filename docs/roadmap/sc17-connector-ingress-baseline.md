# SC-17 Connector / MCP Ingress Baseline

## Scope

SC-17 closes the typed ingress boundary for the currently supported local connector and MCP
adapters. Connector commands are server-normalized only after ProjectTrust and operator checks,
then re-enter ControlPlane authorization; binding snapshots pin connector/account/project and
operation scopes, external writes require the declared risk/approval, payloads are bounded and
idempotent, rate limits are enforced, and ProviderReceipt/EffectObservation bind account,
audience, idempotency and unknown outcomes. Unknown connector outcomes are listed and reconciled
with a separately verified receipt rather than retried.

MCP discovery/calls use the same Broker permit and per-invocation stdio sandbox. Project/config
drift, tool schema/health drift, untrusted projects, operator-less discovery, process stop failure
and replay are denied or retained as Unknown. HTTP/SSE/WS MCP and external connector transports
remain explicitly unsupported; no token or credential passthrough is introduced.

A verified webhook/A2A occurrence is bridged into the connector spine only through the typed
`ConnectorIngressIntent`. It binds the server-owned account, connector audience and purpose,
keeps `broker_calls` at zero, and requires the ingress token audience and the downstream connector
audience to differ, so the inbound token can never be reused as the downstream credential.

## Evidence and limits

- `kiana-domain/tests/p4_k8_01_connector.rs` covers typed connector binding/operation scope,
  local transport and unknown-field denial.
- `kiana-domain/tests/sc17_connector_intent.rs` covers the typed intent derived from a verified
  occurrence: account/purpose/audience binding, ingress-audience mismatch, token passthrough
  denial, unregistered operation / missing scope / revoked binding denial, invalid idempotency
  key, and tamper/unknown-field fail-closed.
- `kiana-daemon/tests/sc17_connector_ingress.rs` covers verifier -> occurrence -> intent end to
  end: bad signature, unknown source, replay and token passthrough denial, plus a normalized
  ControlPlane request with a forged authority field rejected at zero broker calls.
- `kiana-daemon/tests/p1_j4_01_mcp.rs` covers MCP schema/health/config/stdio/reconciliation
  boundaries.
- `kiana-core/tests/sc17_connector_ingress_guard.rs` pins cross-layer trust, account, approval,
  idempotency, receipt, stop and unsupported-transport markers.
- GitHub Actions runs these fixtures, the source guard and workspace compile; local tests are
  intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: signed external webhook
verification, live connector transport, downstream token issuance and external/live/physical
effect proof remain SC-18+ / INT / provider work.
