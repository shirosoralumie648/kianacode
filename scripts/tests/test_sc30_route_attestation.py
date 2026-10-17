"""Deny-first behavior fixtures for the SC-30 route attestation verifier.

These fixtures drive ``scripts/verify-route-attestation.py`` only, offline, over synthetic
provider/model/prompt/MCP metadata: no credential resolution, no provider call, no MCP
server, no network, no process and no cargo execution anywhere in this file.

Deny-first ordering: every refusal is proven before the single success fixture, which is the
last test in this file.
"""

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "verify-route-attestation.py"
SPEC = importlib.util.spec_from_file_location("sc30_verify_route_attestation", SCRIPT)
VERIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)

VERSION = {"major": 1, "minor": 0}
DIGEST_PREFIX = "sha256:"


def digest(value):
    return VERIFY.digest_value(value)


def without(document, key):
    return {name: value for name, value in document.items() if name != key}


class Sc30RouteAttestationFixtures(unittest.TestCase):
    """Synthetic route attestation evidence for the offline verifier."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="kiana-sc30-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.rebuild()

    def rebuild(self):
        attestation, expected = VERIFY.fixture()
        self.attestation = json.loads(json.dumps(attestation))
        self.expected = json.loads(json.dumps(expected))

    def ressign_attestation(self):
        self.attestation["attestation_digest"] = digest(
            without(self.attestation, "attestation_digest")
        )

    def ressign_context(self, context):
        context["route_digest"] = digest(
            {
                key: context["route"][key]
                for key in (
                    "provider_id",
                    "protocol",
                    "model_id",
                    "profile",
                    "configuration_revision",
                    "streaming",
                )
            }
        )
        context["data_policy"]["use_policy_digest"] = digest(
            without(context["data_policy"], "use_policy_digest")
        )
        context["mcp_route"]["route_digest"] = digest(
            without(context["mcp_route"], "route_digest")
        )
        token = context.get("downstream_token")
        if token is not None:
            token["token_scope_digest"] = digest(without(token, "token_scope_digest"))
        return context

    def ressign_claim(self, claim):
        claim["route_digest"] = digest(
            {
                key: claim[key]
                for key in (
                    "provider_id",
                    "protocol",
                    "model_id",
                    "profile",
                    "configuration_revision",
                    "streaming",
                )
            }
        )
        return claim

    def expect_failure(self, reason, attestation=None, expected=None):
        with self.assertRaises(VERIFY.VerificationError) as caught:
            VERIFY.verify_attestation(
                self.attestation if attestation is None else attestation,
                self.expected if expected is None else expected,
            )
        self.assertEqual(str(caught.exception), reason)

    def expect_reason(self, reason, attestation=None, expected=None):
        report = VERIFY.verify_attestation(
            self.attestation if attestation is None else attestation,
            self.expected if expected is None else expected,
        )
        self.assertEqual(report["reason"], reason)
        return report

    def test_missing_or_malformed_evidence_is_refused(self):
        with self.assertRaises(VERIFY.VerificationError) as caught:
            VERIFY.read_json(self.root / "missing.json")
        self.assertTrue(
            str(caught.exception).startswith("route_attestation_read_failed")
        )

        broken = self.root / "broken.json"
        broken.write_text("{", encoding="utf-8")
        with self.assertRaises(VERIFY.VerificationError) as caught:
            VERIFY.read_json(broken)
        self.assertTrue(
            str(caught.exception).startswith("route_attestation_read_failed")
        )

        self.expect_failure(
            "route_attestation_context_object_invalid",
            attestation={"schema": "kiana.route-attestation.v1", "version": VERSION},
        )
        self.expect_failure(
            "route_attestation_header_invalid",
            attestation={
                "schema": "kiana.route-attestation.v1",
                "version": {"major": 9, "minor": 9},
            },
        )
        self.expect_failure(
            "route_attestation_context_header_invalid",
            attestation={
                "schema": "kiana.route-attestation.v1",
                "version": VERSION,
                "context": {
                    "schema": "kiana.route-attestation-context.v2",
                    "version": VERSION,
                },
            },
        )

    def test_unknown_fields_are_refused_before_any_binding_check(self):
        # A transcript, prompt or endpoint smuggled into the envelope is refused by name.
        envelope = json.loads(json.dumps(self.attestation))
        envelope["transcript"] = "model text is never route evidence"
        self.expect_failure("route_attestation_unknown_field", attestation=envelope)

        self.rebuild()
        self.attestation["context"]["endpoint"] = "https://provider.invalid"
        self.expect_failure("route_attestation_context_unknown_field")

        self.rebuild()
        self.attestation["context"]["route"]["endpoint"] = "https://provider.invalid"
        self.expect_failure("route_unknown_field")

        self.rebuild()
        self.attestation["provider_claim"]["token"] = "copied-downstream-token"
        self.expect_failure("route_claim_unknown_field")

        self.rebuild()
        self.attestation["context"]["prompt_pack"]["prompt_text"] = "raw prompt"
        self.expect_failure("route_prompt_unknown_field")

        self.rebuild()
        self.attestation["context"]["mcp_route"]["server_command"] = "sh -c true"
        self.expect_failure("route_mcp_unknown_field")

        self.rebuild()
        self.attestation["context"]["data_policy"]["raw_classes"] = ["secret"]
        self.expect_failure("route_policy_unknown_field")

        self.rebuild()
        self.attestation["context"]["downstream_token"]["token"] = "raw-secret-value"
        self.expect_failure("route_downstream_token_unknown_field")

    def test_provider_reported_models_and_routes_cannot_override_the_snapshot(self):
        for key, value in (
            ("provider_id", "other-provider"),
            ("protocol", "anthropic_messages"),
            ("connection_id", "other-connection"),
            ("model_id", "other-model"),
            ("profile", "other-profile"),
            ("configuration_revision", "config-revision-2"),
            ("streaming", False),
        ):
            self.rebuild()
            self.attestation["provider_claim"][key] = value
            self.ressign_claim(self.attestation["provider_claim"])
            self.ressign_attestation()
            self.expect_reason(
                "route_attestation_provider_claim_mismatch",
                attestation=self.attestation,
                expected=self.expected,
            )

        # A claim changed without re-deriving its own digest is not well formed.
        self.rebuild()
        self.attestation["provider_claim"]["model_id"] = "forged-model"
        self.expect_failure("route_claim_digest_mismatch")

        # A forged route with a consistent claim is still a route mismatch.
        self.rebuild()
        self.attestation["context"]["route"]["model_id"] = "forged-model"
        self.ressign_context(self.attestation["context"])
        self.attestation["provider_claim"] = dict(self.attestation["context"]["route"])
        self.ressign_claim(self.attestation["provider_claim"])
        self.ressign_attestation()
        self.expect_reason("route_attestation_route_mismatch")

        # Tampering with the snapshot route digest alone is refused.
        self.rebuild()
        self.attestation["context"]["route_digest"] = DIGEST_PREFIX + "9" * 64
        self.expect_failure("route_attestation_route_digest_mismatch")

    def test_unreviewed_prompt_pack_trust_and_drift_are_refused_or_unknown(self):
        for trust, reason in (
            ("untrusted", "route_attestation_prompt_pack_untrusted"),
            ("revoked", "route_attestation_prompt_pack_untrusted"),
        ):
            self.rebuild()
            self.attestation["context"]["prompt_pack"]["trust"] = trust
            self.ressign_attestation()
            report = self.expect_reason(reason, expected=self.attestation["context"])
            self.assertEqual(report["status"], "blocked")

        self.rebuild()
        self.attestation["context"]["prompt_pack"]["trust"] = "unknown"
        self.ressign_attestation()
        report = self.expect_reason(
            "route_attestation_prompt_pack_unknown",
            expected=self.attestation["context"],
        )
        self.assertEqual(report["status"], "unknown")

        # A pack whose content changed after review is a hard mismatch, not Unknown.
        self.rebuild()
        self.attestation["context"]["prompt_pack"]["pack_version"] = "1.0.1"
        self.ressign_attestation()
        self.expect_reason("route_attestation_prompt_pack_mismatch")

        # An unverifiable provider stays Unknown rather than Verified.
        self.rebuild()
        self.attestation["provider_verified"] = False
        self.ressign_attestation()
        report = self.expect_reason(
            "route_attestation_provider_unverified",
            expected=self.attestation["context"],
        )
        self.assertEqual(report["status"], "unknown")

        self.rebuild()
        self.attestation["provider_verified"] = "yes"
        self.expect_failure("route_attestation_provider_verified_invalid")

    def test_downstream_token_mixing_accounts_audiences_and_purposes_is_refused(self):
        for key, value, reason in (
            (
                "account_id",
                "other-account",
                "route_attestation_downstream_token_account_mismatch",
            ),
            (
                "audience",
                "audience:other",
                "route_attestation_downstream_token_audience_mismatch",
            ),
            (
                "purpose",
                "other-purpose",
                "route_attestation_downstream_token_purpose_mismatch",
            ),
        ):
            self.rebuild()
            # The token is re-signed so only the cross-field binding can refuse it.
            self.attestation["context"]["downstream_token"][key] = value
            self.ressign_context(self.attestation["context"])
            self.ressign_attestation()
            self.expect_failure(reason, expected=self.attestation["context"])

        # A token whose own scope digest does not match its fields is refused outright.
        self.rebuild()
        self.attestation["context"]["downstream_token"]["account_id"] = "other-account"
        self.expect_failure("route_downstream_token_digest_mismatch")

        # Dropping the expected token is a blocked report, not a silent pass.
        self.rebuild()
        del self.attestation["context"]["downstream_token"]
        self.ressign_attestation()
        report = self.expect_reason("route_attestation_downstream_token_missing")
        self.assertEqual(report["status"], "blocked")

        # A different token reference for the same account/audience/purpose is refused.
        self.rebuild()
        self.attestation["context"]["downstream_token"]["token_ref_digest"] = (
            DIGEST_PREFIX + "9" * 64
        )
        self.ressign_context(self.attestation["context"])
        self.ressign_attestation()
        self.expect_reason("route_attestation_downstream_token_mismatch")

        # The token contract header is versioned.
        self.rebuild()
        self.attestation["context"]["downstream_token"]["schema"] = (
            "kiana.route-attestation-downstream-token.v2"
        )
        self.ressign_attestation()
        self.expect_failure(
            "route_downstream_token_header_invalid",
            expected=self.attestation["context"],
        )

    def test_data_use_policy_scope_and_digest_fences_are_enforced(self):
        self.rebuild()
        self.attestation["context"]["data_policy"]["purpose"] = "marketing"
        self.ressign_attestation()
        self.expect_failure("route_policy_use_digest_mismatch")

        self.rebuild()
        self.attestation["context"]["data_policy"]["use_policy_digest"] = (
            DIGEST_PREFIX + "9" * 64
        )
        self.expect_failure("route_policy_use_digest_mismatch")

        self.rebuild()
        self.attestation["context"]["data_policy"]["data_epoch"] = 0
        self.expect_failure("route_policy_epoch_invalid")

        self.rebuild()
        self.attestation["context"]["data_policy"]["allowed_classes"] = ["pii"]
        self.expect_failure("route_policy_data_class_invalid")

        self.rebuild()
        self.attestation["context"]["data_policy"]["allowed_classes"] = [
            "public",
            "internal",
            "confidential",
            "restricted",
            "pii",
        ]
        self.expect_failure("route_policy_scope_invalid")

        self.rebuild()
        self.attestation["context"]["data_policy"]["schema"] = (
            "kiana.route-attestation-policy.v2"
        )
        self.expect_failure("route_policy_header_invalid")

        # Policy drift against the server snapshot is a report reason.
        self.rebuild()
        self.attestation["context"]["data_policy"]["policy_revision"] = 5
        self.ressign_context(self.attestation["context"])
        self.ressign_attestation()
        self.expect_reason("route_attestation_data_policy_mismatch")

    def test_mcp_descriptor_transport_and_disabled_routes_are_refused(self):
        self.rebuild()
        self.attestation["context"]["mcp_route"]["transport"] = "http"
        self.ressign_context(self.attestation["context"])
        self.ressign_attestation()
        self.expect_reason(
            "route_attestation_mcp_transport_unsupported",
            expected=self.attestation["context"],
        )

        self.rebuild()
        self.attestation["context"]["mcp_route"]["enabled"] = False
        self.ressign_context(self.attestation["context"])
        self.ressign_attestation()
        self.expect_reason(
            "route_attestation_mcp_disabled", expected=self.attestation["context"]
        )

        self.rebuild()
        self.attestation["context"]["mcp_route"]["tool_catalog_digest"] = (
            DIGEST_PREFIX + "9" * 64
        )
        self.ressign_context(self.attestation["context"])
        self.ressign_attestation()
        self.expect_reason("route_attestation_mcp_route_mismatch")

        self.rebuild()
        self.attestation["context"]["mcp_route"]["transport"] = "websocket"
        self.expect_failure("route_mcp_transport_invalid")

        self.rebuild()
        self.attestation["context"]["mcp_route"]["route_digest"] = (
            DIGEST_PREFIX + "9" * 64
        )
        self.expect_failure("route_mcp_digest_mismatch")

        self.rebuild()
        del self.attestation["context"]["mcp_route"]
        self.ressign_attestation()
        self.expect_reason("route_attestation_mcp_route_mismatch")

    def test_release_binding_version_and_bounds_are_enforced(self):
        self.rebuild()
        self.attestation["context"]["release_manifest_digest"] = (
            DIGEST_PREFIX + "9" * 64
        )
        self.ressign_attestation()
        self.expect_reason("route_attestation_release_binding_mismatch")

        self.rebuild()
        self.attestation["context"]["release_signature_attestation_digest"] = (
            DIGEST_PREFIX + "9" * 64
        )
        self.ressign_attestation()
        self.expect_reason("route_attestation_release_binding_mismatch")

        self.rebuild()
        self.attestation["version"] = {"major": 2, "minor": 0}
        self.expect_failure("route_attestation_header_invalid")

        self.rebuild()
        self.attestation["context"]["version"] = {"major": 2, "minor": 0}
        self.expect_failure("route_attestation_context_header_invalid")

        self.rebuild()
        self.attestation["context"]["account_id"] = "a" * 513
        self.expect_failure("route_attestation_account_invalid")

        self.rebuild()
        self.attestation["context"]["audience"] = "audience:provider\nInjected"
        self.expect_failure("route_attestation_audience_invalid")

        self.rebuild()
        self.attestation["context"]["credential_ref_digest"] = "not-a-digest"
        self.expect_failure("route_attestation_credential_digest_invalid")

        # A re-labelled attestation is refused by its own digest.
        self.rebuild()
        self.attestation["provider_verified"] = False
        self.expect_failure("route_attestation_digest_mismatch")

    def test_verifier_stays_offline_and_does_not_mutate_evidence(self):
        source = SCRIPT.read_text(encoding="utf-8")
        for forbidden in (
            "subprocess",
            "socket",
            "urllib",
            "requests",
            "http.client",
            "os.system",
            "eval(",
        ):
            self.assertNotIn(forbidden, source)

        attestation_path = self.root / "attestation.json"
        expected_path = self.root / "expected.json"
        attestation_path.write_text(json.dumps(self.attestation), encoding="utf-8")
        expected_path.write_text(json.dumps(self.expected), encoding="utf-8")
        before = (attestation_path.read_bytes(), expected_path.read_bytes())

        report = VERIFY.verify_attestation(
            VERIFY.read_json(attestation_path), VERIFY.read_json(expected_path)
        )
        self.assertEqual(report["status"], "verified")
        self.assertEqual(
            before, (attestation_path.read_bytes(), expected_path.read_bytes())
        )

    def test_valid_fixture_verifies_last_and_output_is_deterministic(self):
        report = VERIFY.verify_attestation(self.attestation, self.expected)
        self.assertEqual(report["status"], "verified")
        self.assertEqual(report["reason"], "ok")
        self.assertEqual(
            report["route_digest"], self.attestation["context"]["route_digest"]
        )
        self.assertEqual(
            report["prompt_pack_digest"],
            self.attestation["context"]["prompt_pack"]["content_digest"],
        )
        self.assertEqual(
            report["mcp_route_digest"],
            self.attestation["context"]["mcp_route"]["route_digest"],
        )
        self.assertEqual(
            report["data_policy_digest"],
            self.attestation["context"]["data_policy"]["policy_digest"],
        )
        self.assertEqual(
            json.dumps(report, sort_keys=True), json.dumps(report, sort_keys=True)
        )

        # The verifier's own self-test (valid route, forged claim, unknown trust and
        # downstream token mixing) must pass unchanged.
        VERIFY.self_test()


if __name__ == "__main__":
    unittest.main()
