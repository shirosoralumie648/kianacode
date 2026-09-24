# INT-11 HTTPS connector endpoint baseline

## Scope

INT-11 adds a pure, server-owned HTTPS endpoint contract and a narrow transport port for future
connector adapters. The contract is deny-first: URL userinfo, non-HTTPS schemes, query/fragment
credentials, origin drift, unpinned DNS results, loopback/private/link-local/metadata addresses,
unlisted egress addresses, ambient proxies and cross-origin redirects are rejected before a
transport `send` call. The source slice reuses the existing `ConnectorPreparedPermit`,
`CredentialLease`, `ConnectorBindingSnapshot` and `ProviderReceipt` contracts; it does not create a
second ControlPlane/Broker path and does not open a production socket.

## Implemented source slice

- `kiana-domain/src/connector_https.rs` owns `ConnectorHttpsPolicy`,
  `ConnectorHttpsEndpoint` and `ConnectorHttpsResolution` with versioned strict serde contracts,
  canonical digests and redaction-safe `ConnectorHttpsErrorCode` values.
- A policy pins one exact `https://host:port` origin, uses an exact host egress allowlist and can
  optionally pin a public resolved-address set. DNS is adapter supplied; the domain never resolves
  names or reads proxy environment variables.
- Resolution observations are sorted/deduplicated and bound to the endpoint/policy digests.
  Unspecified, broadcast, loopback, private/unique-local, link-local, multicast, documentation,
  benchmark/reserved and `169.254.169.254` metadata addresses fail closed. Literal IP endpoints
  require the observation to contain exactly that address.
- Redirects are represented as endpoint projections and are accepted only when every hop remains
  on the pinned origin and stays within the bounded redirect count. Proxy origins are explicit and
  must be in the policy allowlist; no ambient proxy field exists.
- `kiana-ports/src/connector_https.rs` defines `ConnectorHttpsRequest` and
  `ConnectorHttpsTransport`. `send_checked` validates the ordinary connector permit/lease and
  canonical payload, then rechecks policy, endpoint, DNS observation, proxy and redirect fences
  before dispatch. A transport must claim TLS verification, DNS pinning and origin pinning; the
  default implementation is unsupported.
- GitHub-only domain/ports fixtures and a Core source guard cover the deny matrix and a fake HTTPS
  transport whose dispatch count remains zero for proxy/cross-origin rejection. Existing daemon
  binding remains `local_fixture` and continues to reject unsupported remote transport.

## CI-only evidence

`.github/workflows/int11-https-connector.yml` runs target rustfmt, the domain and ports fixtures,
the Core source guard and a targeted test-target compile in GitHub Actions. Local Cargo tests,
builds, checks, clippy and smoke commands are intentionally not run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: implemented
proof_level: source
```

The fake transport proves only that the checked wrapper calls an adapter after source-level
validation. No DNS resolver, TLS handshake, certificate verification, socket, proxy chain,
credential store, ControlPlane permit mint/consume, EventStore append, durable lease fence, live
provider receipt or external business outcome is exercised. A future adapter must bind the endpoint
policy into its server-owned invocation snapshot and enforce the same resolution at the socket
effect boundary before INT-31 live opt-in.
