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

## Focused local receipt (2026-10-08)

The ports boundary now calls `CredentialLease::validate_for` with the binding account,
`connector.invoke` purpose, connector audience and `effect_target_digest(binding)`. A forged
endpoint digest with a recomputed lease digest was accepted by the previous implementation (RED,
exit 101); the corrected `int11_https_connector` target passes 3/3, the `int07_connector_ports`
regression passes 4/4, and the Core `int11_https_connector_guard` passes 2/2. Maximum RSS was
463692 KiB, 292700 KiB and 131388 KiB respectively; all runs reported zero swaps. This is
bounded local behavior for exact metadata binding only; effect-time expiry, cross-clone replay and
durable/restart lease fences remain open.

## 2026-10-17 completion slice (CI-only)

The existing source slice is closed out with deny-first fixtures rather than new behaviour:

- `kiana-domain/tests/int11_https_connector.rs` (11 fixtures) pins the policy origin/allowlist
  bounds and unknown fields, sorted/deduplicated/digest-bound DNS observations, every SSRF address
  class (loopback, private, link-local, multicast, CGNAT, documentation, benchmark/reserved,
  metadata and the IPv6 equivalents), literal-IP endpoint matching, the exact host allowlist
  (subdomain/suffix/port drift denied), same-origin bounded redirects, explicit proxy origins and
  the stable redaction-safe `ConnectorHttpsErrorCode` vocabulary.
- `kiana-ports/tests/int11_https_connector.rs` (10 fixtures) adds default-deny capabilities and
  `send`, capability-claim denial before the implementation runs, an 11-row request deny matrix
  (schema, policy digest, payload digest, binding revision, lease expiry/audience, endpoint digest,
  DNS observation, proxy, cross-origin redirect, redirect limit) with a zero dispatch count for
  every row, a bounded same-origin redirect success path, explicit proxy support checks and
  post-dispatch receipt binding, plus strict unknown-field / absent-proxy serialization.
- `kiana-core/tests/int11_https_connector_guard.rs` binds the fixture obligations, the single
  `self.send(` dispatch site (capability/request validation before, receipt binding after), the
  purity and ambient-proxy negative assertions, the daemon/core no-transport-selection assertions
  and the manual lane commands.
- `.github/workflows/int11-https-connector.yml` (still `workflow_dispatch`-only,
  `permissions: contents: read`) now runs `cargo fetch --locked`, `cargo fmt --all --check`, the
  domain/ports/guard targets with `--locked --test-threads=1` and three `--no-run` compile steps.

Automatic coverage stays with the unified `ci.yml` shards (`kiana-domain-s3/4` →
`int11_https_connector`, `kiana-core-s4/6` → `int11_https_connector_guard`, `kiana-ports`
whole-crate shard). No manifest/lockfile change, no new dependency or test target and no second
execution loop. Local builds/tests were not run (CI-only) and CI receipts are not awaited, so the
proof level stays `source`; the fake transport still proves only that the checked wrapper calls an
adapter after source-level validation.
