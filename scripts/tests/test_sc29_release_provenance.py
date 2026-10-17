"""Deny-first behavior fixtures for the SC-29 release provenance verifier.

These fixtures drive ``scripts/verify-release-provenance.py`` only, offline, over
synthetic release evidence written into a temporary directory: no signing, no
transparency service, no network, no process and no cargo execution anywhere in
this file.

Deny-first ordering: every evidence rejection is proven before the single
success fixture, which is the last test in this file.
"""

import argparse
import contextlib
import hashlib
import importlib.util
import io
import json
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "verify-release-provenance.py"
SPEC = importlib.util.spec_from_file_location("sc29_verify_release_provenance", SCRIPT)
VERIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)

DIGEST_PREFIX = "sha256:"
VERSION = {"major": 1, "minor": 0}
SOURCE_REVISION = "git:fixture-source"
BUILDER_ID = "builder://github-actions"
TOOLCHAIN = DIGEST_PREFIX + "d" * 64
ARTIFACT_BYTES = b"SC-29 fixture artifact\n"


def digest_bytes(value):
    return DIGEST_PREFIX + hashlib.sha256(value).hexdigest()


def digest_value(value):
    return digest_bytes(
        json.dumps(
            value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
        ).encode()
    )


def without(document, key):
    return {name: value for name, value in document.items() if name != key}


class Sc29ReleaseProvenanceFixtures(unittest.TestCase):
    """Synthetic manifest/provenance/signature evidence for the offline verifier."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="kiana-sc29-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.paths = {
            "manifest": self.root / "manifest.json",
            "provenance": self.root / "provenance.json",
            "signature": self.root / "signature.json",
        }
        self.artifact_argument = [f"fixture.tar.gz={self.root / 'fixture.tar.gz'}"]
        self.rebuild()

    def rebuild(self):
        self.artifact_digest = digest_bytes(ARTIFACT_BYTES)
        manifest = {
            "schema": "kiana.release-manifest.v1",
            "version": VERSION,
            "release_id": "fixture-release",
            "tag": "v1.2.3",
            "source_revision": SOURCE_REVISION,
            "source_tree_digest": DIGEST_PREFIX + "a" * 64,
            "cargo_lock_digest": DIGEST_PREFIX + "b" * 64,
            "toolchain_digest": TOOLCHAIN,
            "builder_id": BUILDER_ID,
            "artifacts": [
                {
                    "name": "fixture.tar.gz",
                    "kind": "archive",
                    "target": "x86_64-unknown-linux-gnu",
                    "digest": self.artifact_digest,
                    "size_bytes": len(ARTIFACT_BYTES),
                    "media_type": "application/gzip",
                }
            ],
            "sbom_digest": DIGEST_PREFIX + "c" * 64,
        }
        manifest["manifest_digest"] = digest_value(manifest)
        provenance = {
            "schema": "kiana.release-provenance.v1",
            "version": VERSION,
            "build_type": "https://slsa.dev/provenance/v1",
            "builder_id": BUILDER_ID,
            "invocation_digest": manifest["cargo_lock_digest"],
            "source_revision": SOURCE_REVISION,
            "source_tree_digest": manifest["source_tree_digest"],
            "cargo_lock_digest": manifest["cargo_lock_digest"],
            "toolchain_digest": manifest["toolchain_digest"],
            "materials": [
                {"uri": "git:fixture", "digest": manifest["source_tree_digest"]}
            ],
            "subject_manifest_digest": manifest["manifest_digest"],
        }
        provenance["provenance_digest"] = digest_value(provenance)
        signature = {
            "schema": "kiana.release-signature-attestation.v1",
            "version": VERSION,
            "algorithm": "sigstore",
            "signer_id": "fixture-signer",
            "key_id": "fixture-key",
            "subject_manifest_digest": manifest["manifest_digest"],
            "signature_digest": DIGEST_PREFIX + "e" * 64,
            "transparency_log_entry_digest": DIGEST_PREFIX + "f" * 64,
            "verifier_id": "verifier://ci",
            "verified": True,
        }
        signature["attestation_digest"] = digest_value(signature)
        self.manifest = manifest
        self.provenance = provenance
        self.signature = signature
        (self.root / "fixture.tar.gz").write_bytes(ARTIFACT_BYTES)
        self.write()

    def write(self):
        for name in ("manifest", "provenance", "signature"):
            document = getattr(self, name)
            self.paths[name].write_text(json.dumps(document), encoding="utf-8")

    def re_sign_manifest(self):
        self.manifest["manifest_digest"] = digest_value(
            without(self.manifest, "manifest_digest")
        )

    def re_sign_provenance(self):
        self.provenance["provenance_digest"] = digest_value(
            without(self.provenance, "provenance_digest")
        )

    def re_sign_signature(self):
        self.signature["attestation_digest"] = digest_value(
            without(self.signature, "attestation_digest")
        )

    def args(
        self,
        *,
        tag="v1.2.3",
        source=SOURCE_REVISION,
        builder=BUILDER_ID,
        toolchain=TOOLCHAIN,
        artifact=None,
        manifest=None,
        provenance=None,
        signature=None,
    ):
        return argparse.Namespace(
            manifest=self.paths["manifest"] if manifest is None else manifest,
            provenance=self.paths["provenance"] if provenance is None else provenance,
            signature=self.paths["signature"] if signature is None else signature,
            expected_tag=tag,
            expected_source_revision=source,
            expected_builder_id=builder,
            expected_toolchain_digest=toolchain,
            artifact=list(self.artifact_argument if artifact is None else artifact),
        )

    def expect_reason(self, reason, **overrides):
        with self.assertRaises(VERIFY.VerificationError) as caught:
            VERIFY.verify(self.args(**overrides))
        self.assertEqual(str(caught.exception), reason)

    def test_missing_or_malformed_evidence_is_refused(self):
        self.expect_reason(
            "release_verification_artifact_missing",
            artifact=[f"fixture.tar.gz={self.root / 'missing.tar.gz'}"],
        )
        self.paths["manifest"].write_text("{", encoding="utf-8")
        with self.assertRaises(VERIFY.VerificationError) as caught:
            VERIFY.verify(self.args())
        self.assertTrue(
            str(caught.exception).startswith("release_manifest_read_failed")
        )
        self.rebuild()
        self.expect_reason(
            "release_verification_artifact_argument_invalid",
            artifact=["fixture.tar.gz"],
        )

    def test_unknown_fields_are_refused_before_any_binding_check(self):
        self.manifest["transcript"] = "model text is never release evidence"
        self.write()
        self.expect_reason("release_manifest_unknown_field")

        self.rebuild()
        self.manifest["artifacts"][0]["sha256"] = self.artifact_digest
        self.write()
        self.expect_reason("release_artifact_unknown_field")

        self.rebuild()
        self.provenance["builder"] = BUILDER_ID
        self.write()
        self.expect_reason("release_provenance_unknown_field")

        self.rebuild()
        self.provenance["materials"][0]["size"] = 1
        self.write()
        self.expect_reason("release_material_unknown_field")

        self.rebuild()
        self.signature["token"] = "not-a-real-token"
        self.write()
        self.expect_reason("release_signature_unknown_field")

    def test_forged_tag_and_filename_are_refused(self):
        # A forged tag is refused before provenance or signature is consulted, even when
        # the manifest is re-signed so its self-digest stays consistent.
        self.manifest["tag"] = "v9.9.9"
        self.re_sign_manifest()
        self.write()
        self.expect_reason("release_verification_tag_mismatch")

        self.rebuild()
        self.manifest["tag"] = "kiana-latest"
        self.write()
        self.expect_reason("release_manifest_tag_invalid")

        self.rebuild()
        self.manifest["artifacts"][0]["name"] = "../fixture.tar.gz"
        self.write()
        self.expect_reason("release_artifact_name_invalid")

        self.rebuild()
        self.manifest["artifacts"].append(dict(self.manifest["artifacts"][0]))
        self.write()
        self.expect_reason("release_manifest_artifact_duplicate")

        self.rebuild()
        self.expect_reason(
            "release_verification_artifact_name_mismatch",
            artifact=[f"forged-v9.9.9.tar.gz={self.root / 'fixture.tar.gz'}"],
        )

    def test_builder_source_and_toolchain_drift_are_refused(self):
        self.expect_reason(
            "release_verification_source_revision_mismatch",
            source="git:other-source",
        )
        self.expect_reason(
            "release_verification_builder_mismatch",
            builder="builder://untrusted",
        )
        self.expect_reason(
            "release_verification_toolchain_mismatch",
            toolchain=DIGEST_PREFIX + "9" * 64,
        )

        self.rebuild()
        self.provenance["source_revision"] = "git:other-source"
        self.re_sign_provenance()
        self.write()
        self.expect_reason("release_verification_source_revision_mismatch")

        self.rebuild()
        self.provenance["builder_id"] = "builder://untrusted"
        self.re_sign_provenance()
        self.write()
        self.expect_reason("release_verification_builder_mismatch")

        self.rebuild()
        self.provenance["toolchain_digest"] = DIGEST_PREFIX + "9" * 64
        self.re_sign_provenance()
        self.write()
        self.expect_reason("release_verification_toolchain_digest_mismatch")

        self.rebuild()
        self.provenance["source_tree_digest"] = DIGEST_PREFIX + "9" * 64
        self.re_sign_provenance()
        self.write()
        self.expect_reason("release_verification_source_tree_digest_mismatch")

        self.rebuild()
        self.provenance["cargo_lock_digest"] = DIGEST_PREFIX + "9" * 64
        self.re_sign_provenance()
        self.write()
        self.expect_reason("release_verification_cargo_lock_digest_mismatch")

        self.rebuild()
        self.provenance["build_type"] = "https://example.invalid/provenance/v1"
        self.re_sign_provenance()
        self.write()
        self.expect_reason("release_provenance_build_type_invalid")

    def test_subject_and_recomputed_digest_mismatch_are_refused(self):
        self.manifest["artifacts"][0]["digest"] = DIGEST_PREFIX + "9" * 64
        self.write()
        self.expect_reason("release_manifest_digest_mismatch")

        self.rebuild()
        self.provenance["source_tree_digest"] = DIGEST_PREFIX + "9" * 64
        self.write()
        self.expect_reason("release_provenance_digest_mismatch")

        self.rebuild()
        self.signature["signer_id"] = "forged-signer"
        self.write()
        self.expect_reason("release_signature_attestation_digest_mismatch")

        self.rebuild()
        self.provenance["subject_manifest_digest"] = DIGEST_PREFIX + "9" * 64
        self.re_sign_provenance()
        self.write()
        self.expect_reason("release_verification_provenance_subject_mismatch")

        self.rebuild()
        self.signature["subject_manifest_digest"] = DIGEST_PREFIX + "9" * 64
        self.re_sign_signature()
        self.write()
        self.expect_reason("release_verification_signature_subject_mismatch")

    def test_artifact_bytes_must_match_name_size_and_digest(self):
        # Tampered bytes with the same name are refused by the digest.
        (self.root / "fixture.tar.gz").write_bytes(b"SC-29 fixture ARTIFACT\n")
        self.expect_reason("release_verification_artifact_digest_mismatch")

        # Appending a byte keeps the manifest digest valid but changes the artifact digest.
        (self.root / "fixture.tar.gz").write_bytes(ARTIFACT_BYTES + b"x")
        self.expect_reason("release_verification_artifact_digest_mismatch")

        # A manifest that understates the size is refused by the size binding after the
        # digest matches; every dependent digest is re-signed to isolate the size check.
        self.rebuild()
        self.manifest["artifacts"][0]["size_bytes"] = len(ARTIFACT_BYTES) - 1
        self.re_sign_manifest()
        self.provenance["subject_manifest_digest"] = self.manifest["manifest_digest"]
        self.re_sign_provenance()
        self.signature["subject_manifest_digest"] = self.manifest["manifest_digest"]
        self.re_sign_signature()
        self.write()
        self.expect_reason("release_verification_artifact_size_mismatch")

    def test_unverified_or_unlogged_signatures_are_refused(self):
        self.signature["verified"] = False
        self.re_sign_signature()
        self.write()
        self.expect_reason("release_verification_signature_unverified")

        self.rebuild()
        self.signature["transparency_log_entry_digest"] = ""
        self.re_sign_signature()
        self.write()
        self.expect_reason("release_verification_transparency_missing")

        self.rebuild()
        self.signature["transparency_log_entry_digest"] = "not-a-digest"
        self.re_sign_signature()
        self.write()
        self.expect_reason("release_signature_transparency_digest_invalid")

        self.rebuild()
        self.signature["algorithm"] = "self-asserted"
        self.re_sign_signature()
        self.write()
        self.expect_reason("release_signature_algorithm_invalid")

        self.rebuild()
        self.signature["verified"] = "yes"
        self.re_sign_signature()
        self.write()
        self.expect_reason("release_signature_verified_invalid")

    def test_version_and_bounds_are_enforced(self):
        self.manifest["version"] = {"major": 2, "minor": 0}
        self.write()
        self.expect_reason("release_manifest_header_invalid")

        self.rebuild()
        self.provenance["version"] = {"major": 2, "minor": 0}
        self.write()
        self.expect_reason("release_provenance_header_invalid")

        self.rebuild()
        self.signature["schema"] = "kiana.release-signature-attestation.v2"
        self.write()
        self.expect_reason("release_signature_header_invalid")

        self.rebuild()
        self.manifest["artifacts"] = []
        self.write()
        self.expect_reason("release_manifest_artifact_count_invalid")

        self.rebuild()
        template = dict(self.manifest["artifacts"][0])
        self.manifest["artifacts"] = [
            dict(template, name=f"fixture-{index}.tar.gz") for index in range(129)
        ]
        self.write()
        self.expect_reason("release_manifest_artifact_count_invalid")

        self.rebuild()
        self.manifest["artifacts"][0]["size_bytes"] = -1
        self.write()
        self.expect_reason("release_artifact_size_invalid")

        self.rebuild()
        self.provenance["materials"] = []
        self.write()
        self.expect_reason("release_provenance_material_count_invalid")

        self.rebuild()
        self.provenance["materials"].append(
            {"uri": "git:fixture", "digest": DIGEST_PREFIX + "a" * 64}
        )
        self.write()
        self.expect_reason("release_provenance_material_duplicate")

        self.rebuild()
        self.manifest["builder_id"] = " \n "
        self.write()
        self.expect_reason("release_manifest_builder_id_invalid")

    def test_verifier_stays_offline_and_does_not_mutate_evidence(self):
        source = SCRIPT.read_text(encoding="utf-8")
        for forbidden in ("subprocess", "socket", "urllib", "requests", "http.client"):
            self.assertNotIn(forbidden, source)
        before = {
            path: path.read_bytes()
            for path in list(self.paths.values()) + [self.root / "fixture.tar.gz"]
        }
        with contextlib.redirect_stdout(io.StringIO()):
            VERIFY.verify(self.args())
        after = {
            path: path.read_bytes()
            for path in list(self.paths.values()) + [self.root / "fixture.tar.gz"]
        }
        self.assertEqual(before, after)

    def test_valid_fixture_verifies_last_and_output_is_deterministic(self):
        outputs = []
        for _ in range(2):
            buffer = io.StringIO()
            with contextlib.redirect_stdout(buffer):
                VERIFY.verify(self.args())
            outputs.append(buffer.getvalue())
        self.assertEqual(outputs[0], outputs[1])
        report = json.loads(outputs[0])
        self.assertEqual(report["schema"], "kiana.release-verification.v1")
        self.assertEqual(report["status"], "verified")
        self.assertEqual(report["manifest_digest"], self.manifest["manifest_digest"])
        self.assertEqual(
            report["provenance_digest"], self.provenance["provenance_digest"]
        )
        self.assertEqual(
            report["signature_attestation_digest"],
            self.signature["attestation_digest"],
        )
        # The bundled deterministic self-test (success plus a forged-tag denial) stays runnable.
        VERIFY.self_test()


if __name__ == "__main__":
    unittest.main()
