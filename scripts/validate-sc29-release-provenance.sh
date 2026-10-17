#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

# SC-29 release provenance structural guard: keep the read-only release boundary, the
# deny-first reason codes and the fixture wiring visible to CI reviewers even when the
# manual lane is not dispatched, then run the offline deny-first fixtures.
python3 - <<'PY'
from pathlib import Path

root = Path.cwd()
workflow = (root / ".github/workflows/sc29-release-provenance.yml").read_text(encoding="utf-8")
verifier = (root / "scripts/verify-release-provenance.py").read_text(encoding="utf-8")
fixtures = (root / "scripts/tests/test_sc29_release_provenance.py").read_text(encoding="utf-8")
validator = (root / "scripts/validate-sc29-release-provenance.sh").read_text(encoding="utf-8")
baseline = (root / "docs/roadmap/sc29-release-provenance-baseline.md").read_text(encoding="utf-8")
domain = (root / "kiana-domain/src/release_attestation.rs").read_text(encoding="utf-8")
domain_fixture = (root / "kiana-domain/tests/sc29_release_attestation.rs").read_text(encoding="utf-8")
guard = (root / "kiana-core/tests/sc29_release_attestation_guard.rs").read_text(encoding="utf-8")
ci = (root / ".github/workflows/ci.yml").read_text(encoding="utf-8")

for marker in [
    "workflow_dispatch:",
    "permissions:",
    "contents: read",
    "cargo fetch --locked",
    "cargo fmt --all --check",
    "python3 -B scripts/tests/test_sc29_release_provenance.py",
    "python3 scripts/verify-release-provenance.py --self-test",
    "bash scripts/validate-sc29-release-provenance.sh",
    "cargo test -p kiana-domain --test sc29_release_attestation --locked -- --test-threads=1",
    "cargo test -p kiana-core --test sc29_release_attestation_guard --locked -- --test-threads=1",
    "--no-run",
]:
    if marker not in workflow:
        raise SystemExit(f"SC-29 workflow marker missing: {marker}")
for forbidden in ["push:", "pull_request:", "schedule:", "continue-on-error", "--ignored", "|| true"]:
    if forbidden in workflow:
        raise SystemExit(f"SC-29 lane must stay manual and fail-closed: {forbidden}")

# The verifier is an offline evidence reader: it never signs, publishes, downloads,
# reaches the network or spawns a process, and it never treats a filename/tag as authority.
for forbidden in ["subprocess", "socket", "urllib", "requests", "http.client"]:
    if forbidden in verifier:
        raise SystemExit(f"SC-29 verifier must stay off the network and out of process execution: {forbidden}")
for marker in [
    "release_verification_tag_mismatch",
    "release_verification_source_revision_mismatch",
    "release_verification_builder_mismatch",
    "release_verification_toolchain_mismatch",
    "release_verification_provenance_subject_mismatch",
    "release_verification_signature_subject_mismatch",
    "release_verification_signature_unverified",
    "release_verification_transparency_missing",
    "release_verification_artifact_digest_mismatch",
    "release_verification_artifact_size_mismatch",
    "release_verification_artifact_name_mismatch",
    "self_test_forged_tag_accepted",
]:
    if marker not in verifier:
        raise SystemExit(f"SC-29 verifier marker missing: {marker}")
# Codes the verifier builds with an f-string are pinned as literals in the fixture file.
for code in [
    "release_verification_source_tree_digest_mismatch",
    "release_verification_cargo_lock_digest_mismatch",
    "release_verification_toolchain_digest_mismatch",
    "release_manifest_unknown_field",
    "release_provenance_unknown_field",
    "release_signature_unknown_field",
    "release_artifact_unknown_field",
    "release_material_unknown_field",
]:
    if code not in fixtures:
        raise SystemExit(f"SC-29 fixture does not pin refusal code: {code}")

for marker in [
    "release_manifest_artifact_tag_mismatch",
    "release_artifact_digest_mismatch",
    "release_artifact_size_mismatch",
    "release_artifact_observed_digest",
    "release_verification_source_tree_digest_mismatch",
    "release_verification_cargo_lock_digest_mismatch",
    "release_provenance_build_type_invalid",
]:
    if marker not in domain:
        raise SystemExit(f"SC-29 domain marker missing: {marker}")
for fixture in [
    "forged_artifact_filenames_cannot_claim_a_different_version_than_the_tag",
    "builder_source_and_toolchain_drift_are_blocked_before_subject_binding",
    "subject_digest_mismatch_is_blocked_for_provenance_signature_and_artifact_bytes",
    "unverified_or_missing_transparency_stays_unknown_and_unknown_fields_fail",
    "artifact_names_cannot_smuggle_paths_and_digest_drift_is_rejected",
    "release_contracts_stay_bounded_strict_and_digest_bound",
    "exact_manifest_provenance_signature_and_transparency_binding_verifies",
]:
    if fixture not in domain_fixture:
        raise SystemExit(f"SC-29 domain fixture missing: {fixture}")
if "sc29_deny_first_fixtures_codes_and_lane_are_wired" not in guard:
    raise SystemExit("SC-29 guard must bind the deny-first fixtures and lane wiring")
for fixture in [
    "test_missing_or_malformed_evidence_is_refused",
    "test_unknown_fields_are_refused_before_any_binding_check",
    "test_forged_tag_and_filename_are_refused",
    "test_builder_source_and_toolchain_drift_are_refused",
    "test_subject_and_recomputed_digest_mismatch_are_refused",
    "test_artifact_bytes_must_match_name_size_and_digest",
    "test_unverified_or_unlogged_signatures_are_refused",
    "test_version_and_bounds_are_enforced",
    "test_verifier_stays_offline_and_does_not_mutate_evidence",
    "test_valid_fixture_verifies_last_and_output_is_deterministic",
]:
    if fixture not in fixtures:
        raise SystemExit(f"SC-29 verifier fixture missing: {fixture}")
for marker in [
    "2026-10-17 completion slice (CI-only)",
    "deny-first",
    "test_sc29_release_provenance.py",
    "validate-sc29-release-provenance.sh",
    "release_manifest_artifact_tag_mismatch",
    "release_artifact_digest_mismatch",
    "source",
    "limitations:",
    "reviewer:",
]:
    if marker not in baseline:
        raise SystemExit(f"SC-29 baseline marker missing: {marker}")
if "test_sc29_release_provenance.py" not in validator or "verify-release-provenance.py --self-test" not in validator:
    raise SystemExit("SC-29 validator must run the offline fixtures and the verifier self-test")
if "bash scripts/validate-sc29-release-provenance.sh" not in ci:
    raise SystemExit("the unified ci gate must run the SC-29 validator")
print("SC-29 release manifest, provenance, signature and transparency boundaries are structurally fail-closed")
PY

# The deny-first fixtures and the self-test are pure Python and offline: no cargo, no
# signing, no transparency service and no network. Running them through this validator
# keeps the release boundary exercised by the unified `ci` gate as well as by the
# manual sc29-release-provenance lane.
python3 -B scripts/tests/test_sc29_release_provenance.py
python3 scripts/verify-release-provenance.py --self-test
