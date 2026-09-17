# CI-10 Provider Policy / Credential Probe Baseline

## Scope

CI-10 closes the provider-use policy and user-facing credential diagnostics boundary after the
ConfigResolver, SecretStore/lease, route admission and OAuth lifecycle slices.  A provider policy
bundle is server-owned and default-deny; a matching rule is selected by explicit precedence and
declaration order.  Credential readiness is evaluated separately, so `missing_scope`, missing,
expired, re-auth-required, revoked, unsupported and unknown states remain distinct deny reasons.

The protocol carries only `SecretRef` metadata, digests, generations, expiry and display status.
The CLI, TUI and loopback app-server auth status route pass their compatibility command output
through one sanitizer that turns key/token fields into presence-only values and removes
`key_preview` suffixes.  This projection is non-authorizing: a configured credential or successful
probe never grants `provider.use` by itself.

## Evidence and limits

- `kiana-policy/src/provider.rs` contains strict, digest-bound policy rules/bundles/decisions and
  fail-closed evaluation.
- `kiana-protocol/src/lib.rs` contains strict probe request/response and policy-view DTOs with
  canonical scopes and response digests; no raw credential field is present.
- `kiana-entrypoints/src/provider_diagnostics.rs` is the shared display boundary used by CLI/TUI
  and the direct app-server auth status handler.
- Runtime tests are defined in `kiana-policy/tests/ci10_provider_policy.rs`,
  `kiana-protocol/tests/ci10_provider_probe.rs` and
  `kiana-entrypoints/tests/ci10_provider_diagnostics.rs`; GitHub Actions runs them.  Local work
  intentionally performs no test execution.

This slice is `feature_status=implemented` at source level and `proof_level=source`: the policy
bundle is not yet loaded from a durable per-project policy store, and the probe DTO is not a live
IdP/provider health call.  ProviderGateway admission does not yet consume this policy bundle;
CI-11 owns durable audit/redaction/recovery wiring.  No external provider, browser callback,
cross-process refresh or live/physical proof is claimed.

