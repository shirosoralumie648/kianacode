# SC-30 · Provider/model/prompt-pack/MCP route attestation baseline

SC-30 adds a deny-first metadata boundary for the route selected by ControlPlane. A provider
response or model supplied route claim is evidence to compare, never an authority. The exact
provider/model route, prompt-pack provenance, MCP descriptor, data/use policy and opaque
credential/account/audience identities must stay bound to the server-owned admission snapshot.

## Source contract

- `kiana-domain::RouteAttestationContext` binds `ModelRoute` and its digest to a prompt-pack
  provenance record, optional MCP route, data-policy revision/epoch/purpose/classes, credential
  reference digest, account, audience and the SC-29 release manifest/signature attestation
  digests. Raw credentials, prompt text and network endpoints are not represented.
- `ProviderRouteClaim` reconstructs the route digest from every provider/model/protocol/profile/
  configuration/streaming field. A forged model or downstream route token cannot be accepted by
  copying only one digest field.
- `RouteDataPolicyBinding` uses an exact policy digest and epoch plus a bounded data/use purpose
  and class set. `RouteAttestation::verify` compares the full server snapshot immediately before
  dispatch and returns structured `Verified`, `Blocked` or `Unknown` reports.
- Untrusted/revoked prompt packs, HTTP MCP transport, policy/credential/account/audience drift,
  provider claim drift and release binding drift are blocked. Unknown prompt trust or an
  externally unverified provider claim remains `Unknown`; this module does not resolve secrets,
  call providers, launch MCP or verify cryptographic signatures.
- `scripts/verify-route-attestation.py` mirrors the strict object and digest checks offline. Its
  deterministic fixture proves a valid binding, forged provider/model claim rejection and
  unknown prompt trust. It has no network client and only reads supplied JSON files.

## CI-only fixture catalog

`.github/workflows/sc30-route-attestation.yml` runs the following on GitHub Actions; no runtime
tests are run as a local development step:

- `kiana-domain/tests/sc30_route_attestation.rs`: exact provider/model/prompt/policy/MCP binding,
  provider claim and route drift, untrusted/unknown/unverified states, credential/account/audience
  drift, HTTP MCP denial, strict serde and digest fences.
- `scripts/verify-route-attestation.py --self-test`: valid fixture, actual JSON round-trip,
  forged provider/model claim denial and unknown prompt trust preservation.
- `kiana-core/tests/sc30_route_attestation_guard.rs`: source, verifier and workflow boundary
  guard, including no provider/network client in the domain contract.

This is `source` proof with CI wiring only: it does not claim production provider signatures,
credential resolution, live model identity, MCP server execution, prompt-pack publication,
reproducible release bytes, durable route snapshot storage or physical/live external effect
correctness. SC-41 remains responsible for integrating route attestation into the final release
gate and effect path.

source_snapshot: `a2309009` plus SC-30 source slice
worktree_status: isolated branch with unrelated worktrees preserved
command_argv: `git diff --check`; GitHub Actions runs formatting, fixtures and workspace test-target compilation; CI is not awaited
cwd/environment: Linux x86_64; local tests/build/check/clippy/smoke deliberately not run
fixture/cassette: committed domain/Python deny fixtures; no provider, credential or MCP external cassette
exit_code: local diff inspection only; remote CI result intentionally unobserved
status change: SC-30 source contracts and GitHub-only verifier wiring added
proof-level change: `feature_status=implemented`, `proof_level=source`
limitations: no production cryptographic verification, credential lookup, live provider/model or MCP effect, durable snapshot/recovery, or physical proof
reviewer: Codex source review; no local runtime test reviewer
