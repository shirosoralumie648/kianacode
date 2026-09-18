# SC-14 Network Policy / Sandbox Baseline

## Scope

SC-14 adds a server-owned `NetworkPolicy` and `NetworkEndpointObservation` contract. A network
adapter must provide a bounded resolved-address set; the domain contract validates the exact HTTPS
endpoint, allowlisted host, credentials/query absence, literal-IP agreement and every resolved
address, including loopback/private/link-local and metadata rejection. The Broker consumes only
the `ExecutionScope.network_policy` host set and rejects endpoint observations without a policy,
resolution set or matching policy digest. Existing bubblewrap planning and the reviewed HTTP policy
remain deny-first; stdio MCP still has no HTTP endpoint path.

DNS and network I/O are intentionally outside this slice. An observation is a server adapter fact,
not permission, and a later effect adapter must re-resolve or retain the observation fence before
connecting.

## Evidence and limits

- `kiana-domain/tests/sc14_network_policy.rs` covers allowlisted HTTPS success plus local,
  metadata, unallowlisted, credential/query, empty-resolution and mixed-address denial.
- `kiana-core/tests/sc14_network_policy_guard.rs` pins Broker/sandbox/legacy network-policy
  markers and rejects hidden network I/O in the pure boundary.
- GitHub Actions runs the fixtures, source guard and workspace compile; local tests are
  intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: DNS resolver ownership,
per-connection egress enforcement, credential/secret delivery, crash recovery and external/live
network proof remain SC-15+ / SC-17 / SC-18+ work.
