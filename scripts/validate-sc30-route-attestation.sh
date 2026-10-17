#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

# SC-30 route attestation structural guard: keep the read-only metadata boundary, the
# deny-first reason codes and the fixture wiring visible to CI reviewers even when the
# manual lane is not dispatched, then run the offline deny-first fixtures.
python3 - <<'PY'
import json
from pathlib import Path

root = Path.cwd()
workflow = (root / ".github/workflows/sc30-route-attestation.yml").read_text(encoding="utf-8")
verifier = (root / "scripts/verify-route-attestation.py").read_text(encoding="utf-8")
fixtures = (root / "scripts/tests/test_sc30_route_attestation.py").read_text(encoding="utf-8")
validator = (root / "scripts/validate-sc30-route-attestation.sh").read_text(encoding="utf-8")
baseline = (root / "docs/roadmap/sc30-route-attestation-baseline.md").read_text(encoding="utf-8")
domain = (root / "kiana-domain/src/route_attestation.rs").read_text(encoding="utf-8")
domain_fixture = (root / "kiana-domain/tests/sc30_route_attestation.rs").read_text(encoding="utf-8")
guard = (root / "kiana-core/tests/sc30_route_attestation_guard.rs").read_text(encoding="utf-8")
ci = (root / ".github/workflows/ci.yml").read_text(encoding="utf-8")
shards = json.loads((root / "scripts/ci/test-shards.json").read_text(encoding="utf-8"))

for marker in [
    "workflow_dispatch:",
    "permissions:",
    "contents: read",
    "cargo fetch --locked",
    "cargo fmt --all --check",
    "python3 -B scripts/tests/test_sc30_route_attestation.py",
    "python3 scripts/verify-route-attestation.py --self-test",
    "bash scripts/validate-sc30-route-attestation.sh",
    "cargo test -p kiana-domain --test sc30_route_attestation --locked -- --test-threads=1",
    "cargo test -p kiana-core --test sc30_route_attestation_guard --locked -- --test-threads=1",
    "--no-run",
]:
    if marker not in workflow:
        raise SystemExit(f"SC-30 workflow marker missing: {marker}")
for forbidden in ["push:", "pull_request:", "schedule:", "continue-on-error", "--ignored", "|| true"]:
    if forbidden in workflow:
        raise SystemExit(f"SC-30 lane must stay manual and fail-closed: {forbidden}")

# The verifier is an offline evidence reader: it never resolves credentials, calls a provider,
# launches MCP, reaches the network or spawns a process.
for forbidden in ["subprocess", "socket", "urllib", "requests", "http.client", "os.system", "eval("]:
    if forbidden in verifier:
        raise SystemExit(f"SC-30 verifier must stay offline and out of process execution: {forbidden}")
for marker in [
    "route_attestation_route_mismatch",
    "route_attestation_provider_claim_mismatch",
    "route_attestation_prompt_pack_mismatch",
    "route_attestation_prompt_pack_untrusted",
    "route_attestation_prompt_pack_unknown",
    "route_attestation_mcp_route_mismatch",
    "route_attestation_mcp_transport_unsupported",
    "route_attestation_mcp_disabled",
    "route_attestation_data_policy_mismatch",
    "route_attestation_credential_mismatch",
    "route_attestation_account_mismatch",
    "route_attestation_audience_mismatch",
    "route_attestation_downstream_token_missing",
    "route_attestation_downstream_token_mismatch",
    "route_attestation_downstream_token_account_mismatch",
    "route_attestation_downstream_token_audience_mismatch",
    "route_attestation_downstream_token_purpose_mismatch",
    "route_attestation_release_binding_mismatch",
    "route_attestation_provider_unverified",
    "route_attestation_binding_mismatch",
    "route_downstream_token_header_invalid",
    "route_downstream_token_digest_mismatch",
    "self_test_mixed_downstream_token_accepted",
    "self_test_missing_downstream_token_not_reported",
    "self_test_forged_provider_claim_accepted",
]:
    if marker not in verifier:
        raise SystemExit(f"SC-30 verifier marker missing: {marker}")

for marker in [
    "DownstreamTokenAttestation",
    "with_downstream_token",
    "authorize_use",
    "RouteDataUseDecision",
    "route_attestation_mcp_disabled",
    "route_attestation_downstream_token_account_mismatch",
    "route_attestation_downstream_token_audience_mismatch",
    "route_attestation_downstream_token_purpose_mismatch",
    "route_attestation_downstream_token_missing",
    "route_attestation_downstream_token_mismatch",
    "route_attestation_purpose_not_permitted",
    "route_attestation_data_classes_invalid",
    "route_attestation_data_class_unknown",
    "route_attestation_data_class_not_permitted",
    "route_downstream_token_header_invalid",
    "route_downstream_token_digest_mismatch",
    "route_use_decision_header_invalid",
    "route_use_decision_state_invalid",
    "route_use_decision_reason_invalid",
    "route_use_decision_scope_invalid",
    "route_use_decision_data_class_invalid",
    "route_use_decision_digest_mismatch",
    "route_use_decision_epoch_invalid",
]:
    if marker not in domain:
        raise SystemExit(f"SC-30 domain marker missing: {marker}")
for forbidden in ["reqwest", "std::net", "std::process", "std::fs", "tokio::", "Command::new", "unsafe"]:
    if forbidden in domain:
        raise SystemExit(f"SC-30 domain contract must stay pure metadata: {forbidden}")

domain_fixtures = [
    "provider_claim_and_route_drift_are_blocked_before_dispatch",
    "unreviewed_prompt_packs_and_unverified_providers_are_denied_or_unknown",
    "downstream_token_mixing_across_account_audience_and_purpose_is_refused",
    "data_use_policy_denies_out_of_scope_purpose_and_class_escalation",
    "policy_credential_account_audience_and_mcp_transport_drift_is_denied",
    "strict_serde_and_digest_fences_reject_forged_values",
    "exact_provider_model_prompt_policy_and_mcp_binding_verifies",
]
for fixture in domain_fixtures:
    if fixture not in domain_fixture:
        raise SystemExit(f"SC-30 domain fixture missing: {fixture}")
success = domain_fixtures[-1]
for fixture in domain_fixtures[:-1]:
    if domain_fixture.index(fixture) > domain_fixture.index(success):
        raise SystemExit(f"SC-30 domain fixture is not deny-first: {fixture}")

verifier_fixtures = [
    "test_missing_or_malformed_evidence_is_refused",
    "test_unknown_fields_are_refused_before_any_binding_check",
    "test_provider_reported_models_and_routes_cannot_override_the_snapshot",
    "test_unreviewed_prompt_pack_trust_and_drift_are_refused_or_unknown",
    "test_downstream_token_mixing_accounts_audiences_and_purposes_is_refused",
    "test_data_use_policy_scope_and_digest_fences_are_enforced",
    "test_mcp_descriptor_transport_and_disabled_routes_are_refused",
    "test_release_binding_version_and_bounds_are_enforced",
    "test_verifier_stays_offline_and_does_not_mutate_evidence",
    "test_valid_fixture_verifies_last_and_output_is_deterministic",
]
for fixture in verifier_fixtures:
    if fixture not in fixtures:
        raise SystemExit(f"SC-30 verifier fixture missing: {fixture}")
success = verifier_fixtures[-1]
for fixture in verifier_fixtures[:-1]:
    if fixtures.index(fixture) > fixtures.index(success):
        raise SystemExit(f"SC-30 verifier fixture is not deny-first: {fixture}")

if "sc30_attestation_binds_route_policy_credentials_and_audience" not in guard:
    raise SystemExit("SC-30 guard must keep the source boundary fixture")
if "sc30_deny_first_fixtures_codes_and_lane_are_wired" not in guard:
    raise SystemExit("SC-30 guard must bind the deny-first fixtures and lane wiring")

# The automatic lanes own the two Rust targets; the shard book must keep claiming them.
domain_shard = shards.get("kiana-domain-s4/4", {})
core_shard = shards.get("kiana-core-s6/6", {})
if "sc30_route_attestation" not in (domain_shard.get("targets") or []):
    raise SystemExit("kiana-domain-s4/4 must keep sc30_route_attestation")
if "sc30_route_attestation_guard" not in (core_shard.get("targets") or []):
    raise SystemExit("kiana-core-s6/6 must keep sc30_route_attestation_guard")

for marker in [
    "2026-10-17 completion slice (CI-only)",
    "deny-first",
    "test_sc30_route_attestation.py",
    "validate-sc30-route-attestation.sh",
    "route_attestation_downstream_token_mismatch",
    "route_attestation_purpose_not_permitted",
    "source",
    "limitations:",
    "reviewer:",
]:
    if marker not in baseline:
        raise SystemExit(f"SC-30 baseline marker missing: {marker}")
if "test_sc30_route_attestation.py" not in validator or "verify-route-attestation.py --self-test" not in validator:
    raise SystemExit("SC-30 validator must run the offline fixtures and the verifier self-test")
if "bash scripts/validate-sc30-route-attestation.sh" not in ci:
    raise SystemExit("the unified ci gate must run the SC-30 validator")
print("SC-30 provider, prompt-pack, policy, credential and MCP route boundaries are structurally fail-closed")
PY

# The deny-first fixtures and the self-test are pure Python and offline: no cargo, no
# credential, no provider call and no network. Running them through this validator keeps the
# route boundary exercised by the unified `ci` gate as well as by the manual sc30 lane.
python3 -B scripts/tests/test_sc30_route_attestation.py
python3 scripts/verify-route-attestation.py --self-test
