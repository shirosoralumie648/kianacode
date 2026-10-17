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

## 2026-10-17 completion slice (CI-only)

This slice closes the card's three denial paths (provider self-reported model/route, unreviewed
prompt pack, downstream token mixing) and its deliverables (route digest, data/use policy,
credential/account/audience binding, MCP route) in source, deny-first, and wires them into CI.
No runtime test was run locally; the GitHub Actions lane and the unified `ci` gate are the only
executors.

- `kiana-domain/src/route_attestation.rs`:
  - `DownstreamTokenAttestation` binds an opaque downstream token reference digest to the
    server-owned account, audience and data-policy purpose. `RouteAttestationContext
    ::with_downstream_token` refuses a token minted for another account
    (`route_attestation_downstream_token_account_mismatch`), audience
    (`route_attestation_downstream_token_audience_mismatch`) or purpose
    (`route_attestation_downstream_token_purpose_mismatch`), and `RouteAttestation::verify`
    refuses a missing expected token (`route_attestation_downstream_token_missing`) or another
    token reference (`route_attestation_downstream_token_mismatch`).
  - `RouteDataPolicyBinding::authorize_use` returns a digest-bound `RouteDataUseDecision`
    (`route_attestation_purpose_not_permitted`, `route_attestation_data_classes_invalid`,
    `route_attestation_data_class_unknown`, `route_attestation_data_class_not_permitted`); a
    refusal never mutates the binding and stays deterministic.
  - `RouteAttestation::verify` now documents its fixed deny order: route → provider claim →
    prompt pack equality → prompt trust → MCP route → MCP transport → MCP enabled
    (`route_attestation_mcp_disabled`) → data policy → credential → account → audience →
    downstream token → release binding → provider verification → `ok`.
- `kiana-domain/tests/sc30_route_attestation.rs`: 5 → 7 deny-first fixtures, success last.
- `scripts/verify-route-attestation.py`: mirrors the downstream-token contract, the
  MCP-disabled refusal and the field-wise deny order (the coarse binding fence stays last); its
  `--self-test` now also proves downstream token mixing and token omission.
- `scripts/tests/test_sc30_route_attestation.py`: 10 offline deny-first fixtures, success last
  and deterministic.
- `scripts/validate-sc30-route-attestation.sh`: marker/reason-code/fixture-name matrix,
  deny-first order, manual-lane negatives, `kiana-domain-s4/4` and `kiana-core-s6/6` shard
  binding and the unified `ci.yml` invocation; it runs the fixtures and the verifier self-test.
- `.github/workflows/sc30-route-attestation.yml`: `workflow_dispatch`-only, `permissions:
  contents: read`, `cargo fetch --locked` + `cargo fmt --all --check` + fixtures + self-test +
  validator + two `--no-run` compiles + two serial targets.
- `kiana-core/tests/sc30_route_attestation_guard.rs`: two guards (source boundary, and
  deny-first fixtures/codes/lane/shard wiring).

Acceptance fixtures (deny-first, success last):

| target | fixtures |
|---|---|
| `kiana-domain/tests/sc30_route_attestation.rs` | `provider_claim_and_route_drift_are_blocked_before_dispatch`, `unreviewed_prompt_packs_and_unverified_providers_are_denied_or_unknown`, `downstream_token_mixing_across_account_audience_and_purpose_is_refused`, `data_use_policy_denies_out_of_scope_purpose_and_class_escalation`, `policy_credential_account_audience_and_mcp_transport_drift_is_denied`, `strict_serde_and_digest_fences_reject_forged_values`, `exact_provider_model_prompt_policy_and_mcp_binding_verifies` |
| `scripts/tests/test_sc30_route_attestation.py` | `test_missing_or_malformed_evidence_is_refused`, `test_unknown_fields_are_refused_before_any_binding_check`, `test_provider_reported_models_and_routes_cannot_override_the_snapshot`, `test_unreviewed_prompt_pack_trust_and_drift_are_refused_or_unknown`, `test_downstream_token_mixing_accounts_audiences_and_purposes_is_refused`, `test_data_use_policy_scope_and_digest_fences_are_enforced`, `test_mcp_descriptor_transport_and_disabled_routes_are_refused`, `test_release_binding_version_and_bounds_are_enforced`, `test_verifier_stays_offline_and_does_not_mutate_evidence`, `test_valid_fixture_verifies_last_and_output_is_deterministic` |
| `kiana-core/tests/sc30_route_attestation_guard.rs` | `sc30_attestation_binds_route_policy_credentials_and_audience`, `sc30_deny_first_fixtures_codes_and_lane_are_wired` |

Evidence boundary for this slice: `feature_status=implemented`, `proof_level=source`. The
contracts still do not resolve credentials, contact a provider, launch MCP, verify cryptographic
signatures or store a durable snapshot; every fixture uses synthetic digests.
limitations: no live provider/model identity, no real downstream token issuance or provider-side
audience enforcement, no durable route snapshot/recovery, and no physical/live external effect
proof. SC-41 still owns integrating route attestation into the final release gate and effect path.

reviewer: this session's source review; no local runtime test reviewer; CI receipt not awaited.
