from __future__ import annotations

import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "scripts"
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import capability_governance as governance


VALID_FIXTURE = (
    ROOT
    / "scripts"
    / "fixtures"
    / "capability-governance"
    / "valid"
    / "minimal-graph.json"
)
VALIDATOR = ROOT / "scripts" / "validate-capability-governance.py"


def load_minimal_bundle() -> dict[str, Any]:
    return json.loads(VALID_FIXTURE.read_text(encoding="utf-8"))


def error_codes(errors: list[governance.GovernanceError]) -> set[str]:
    return {error.code for error in errors}


def refresh_head_binding(bundle: dict[str, Any], family: str) -> None:
    binding_key = governance.REVISION_FAMILIES[family]
    head = bundle[family][-1]
    bundle["bundle_manifest"][binding_key]["sha256"] = governance.canonical_sha256(head)


def refresh_evidence_record_chain(bundle: dict[str, Any]) -> None:
    records = bundle["evidence_revisions"][-1]["records"]
    previous_hash = "genesis"
    for record in records:
        record["previous_record_sha256"] = previous_hash
        record["record_sha256"] = governance.record_sha256(record)
        previous_hash = record["record_sha256"]
    refresh_head_binding(bundle, "evidence_revisions")


class GovernanceCorrectionContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.bundle = load_minimal_bundle()

    def validate_codes(self, bundle: dict[str, Any]) -> set[str]:
        return error_codes(governance.validate_governance(bundle, root=ROOT))

    def ancestry_codes(self, revisions: list[dict[str, Any]]) -> set[str]:
        return error_codes(
            governance.validate_revision_ancestry(
                revisions,
                family="evidence_revisions",
            )
        )

    def test_genesis_without_reason_has_exact_code(self) -> None:
        revisions = copy.deepcopy(self.bundle["evidence_revisions"])
        revisions[0].pop("genesis_reason")

        self.assertIn("genesis_reason_required", self.ancestry_codes(revisions))

    def test_successor_without_predecessor_field_has_exact_code(self) -> None:
        revisions = copy.deepcopy(self.bundle["evidence_revisions"])
        revisions[-1].pop("previous_revision_path")

        self.assertIn("previous_revision_required", self.ancestry_codes(revisions))

    def test_predecessor_hash_mismatch_has_exact_code(self) -> None:
        revisions = copy.deepcopy(self.bundle["evidence_revisions"])
        revisions[-1]["previous_revision_sha256"] = "0" * 64

        self.assertIn("previous_revision_hash_mismatch", self.ancestry_codes(revisions))

    def test_removed_historical_record_has_exact_code(self) -> None:
        revisions = copy.deepcopy(self.bundle["evidence_revisions"])
        del revisions[-1]["records"][0]

        self.assertIn("history_removal", self.ancestry_codes(revisions))

    def test_reordered_historical_records_have_exact_code(self) -> None:
        revisions = copy.deepcopy(self.bundle["evidence_revisions"])
        records = revisions[-1]["records"]
        records[0], records[1] = records[1], records[0]

        self.assertIn("history_reorder", self.ancestry_codes(revisions))

    def test_rewritten_historical_record_keeps_exact_code(self) -> None:
        revisions = copy.deepcopy(self.bundle["evidence_revisions"])
        revisions[-1]["records"][0]["result"] = "fail"

        self.assertIn("history_rewrite", self.ancestry_codes(revisions))

    def test_reject_without_rationale_has_exact_code(self) -> None:
        decisions = self.bundle["capability_decision_revisions"][-1]["decisions"]
        rejected = next(row for row in decisions if row["decision"] == "reject")
        rejected["reason"] = ""
        refresh_head_binding(self.bundle, "capability_decision_revisions")

        self.assertIn("reject_rationale_required", self.validate_codes(self.bundle))

    def test_verified_capability_without_evidence_has_exact_code(self) -> None:
        capabilities = self.bundle["public_baseline_revisions"][-1]["capabilities"]
        verified = next(row for row in capabilities if row["coverage_state"] == "verified")
        verified["evidence_ids"] = []
        refresh_head_binding(self.bundle, "public_baseline_revisions")

        self.assertIn("verified_evidence_required", self.validate_codes(self.bundle))

    def test_expired_evidence_has_exact_stale_diagnostic(self) -> None:
        records = self.bundle["evidence_revisions"][-1]["records"]
        record = next(row for row in records if row["evidence_id"] == "ev.cap.public.current")
        record["expires_at"] = "2026-07-15T00:01:30Z"
        refresh_evidence_record_chain(self.bundle)

        errors = governance.validate_governance(self.bundle, root=ROOT)
        matching = [
            error
            for error in errors
            if error.code == "evidence_expired"
            and error.subject_id == "cap.public.cli"
            and error.freshness == "stale"
        ]
        self.assertEqual(1, len(matching), [error.as_dict() for error in errors])

    def test_newer_failed_retest_has_exact_stale_diagnostic(self) -> None:
        records = self.bundle["evidence_revisions"][-1]["records"]
        previous = next(row for row in records if row["evidence_id"] == "ev.cap.public.current")
        failed = copy.deepcopy(previous)
        failed.update(
            {
                "sequence": len(records) + 1,
                "evidence_id": "ev.cap.public.failed-retest",
                "evidence_type": "failed_retest",
                "result": "fail",
                "observed_at": "2026-07-15T00:01:30Z",
                "supersedes_evidence_ids": [],
            }
        )
        failed.pop("expires_at", None)
        records.append(failed)
        capabilities = self.bundle["public_baseline_revisions"][-1]["capabilities"]
        capability = next(row for row in capabilities if row["capability_id"] == "cap.public.cli")
        capability["evidence_ids"].append(failed["evidence_id"])
        refresh_evidence_record_chain(self.bundle)
        refresh_head_binding(self.bundle, "public_baseline_revisions")

        errors = governance.validate_governance(self.bundle, root=ROOT)
        matching = [
            error
            for error in errors
            if error.code == "newer_failed_retest"
            and error.subject_id == "cap.public.cli"
            and error.freshness == "stale"
        ]
        self.assertEqual(1, len(matching), [error.as_dict() for error in errors])
        self.assertIn("ev.cap.public.current", {row["evidence_id"] for row in records})

    def test_invalid_alias_has_exact_code(self) -> None:
        aliases = self.bundle["repository_registry_revisions"][-1]["repositories"][0]["aliases"]
        aliases[0].pop("reason")
        refresh_head_binding(self.bundle, "repository_registry_revisions")

        self.assertIn("alias_contract_invalid", self.validate_codes(self.bundle))

    def test_secret_shaped_value_has_exact_code(self) -> None:
        decisions = self.bundle["capability_decision_revisions"][-1]["decisions"]
        decisions[0]["reason"] = "synthetic sk-abcdefghijklmnop value"
        refresh_head_binding(self.bundle, "capability_decision_revisions")

        self.assertIn("sensitive_value", self.validate_codes(self.bundle))

    def test_registry_count_mismatch_has_exact_summary_code(self) -> None:
        registry = self.bundle["repository_registry_revisions"][-1]
        registry["expected_count"] = 37
        refresh_head_binding(self.bundle, "repository_registry_revisions")

        self.assertIn("summary_mismatch", self.validate_codes(self.bundle))


class GovernanceDriftAndUsageContractTests(unittest.TestCase):
    def initialize_git_repository(self, path: Path) -> dict[str, str]:
        path.mkdir(parents=True)
        (path / "payload.txt").write_text("fixture payload\n", encoding="utf-8")
        (path / "LICENSE").write_text("Fixture License\n", encoding="utf-8")
        subprocess.run(["git", "init", "-q"], cwd=path, check=True)
        subprocess.run(["git", "add", "."], cwd=path, check=True)
        subprocess.run(
            [
                "git",
                "-c",
                "user.name=Kiana Fixture",
                "-c",
                "user.email=kiana-fixture@example.invalid",
                "commit",
                "-qm",
                "fixture",
            ],
            cwd=path,
            check=True,
        )
        return governance.repository_fingerprint(path)

    def repository_drift_codes(
        self,
        frozen_overrides: dict[str, str],
        *,
        revision_kind: str = "git_commit",
    ) -> set[str]:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            live_reference_root = root / "reference"
            target_root = root / "target"
            target_root.mkdir()
            repository_path = live_reference_root / "fixture-repository"
            fingerprint = self.initialize_git_repository(repository_path)
            artifact = {
                "artifact_id": "official-source-fixture",
                "content_sha256": "a" * 64,
            }
            frozen = {
                "repo_id": "fixture-repository",
                "path": "reference/fixture-repository",
                "revision_kind": revision_kind,
                "revision_value": (
                    fingerprint["git_head"]
                    if revision_kind == "git_commit"
                    else fingerprint["tree_sha256"]
                ),
                "tree_sha256": fingerprint["tree_sha256"],
                "license_sha256": fingerprint["license_sha256"],
            }
            frozen.update(frozen_overrides)
            bundle = {
                "official_source_artifact": artifact,
                "repository_registry_revisions": [{"repositories": [frozen]}],
                "capability_decision_revisions": [{"decisions": []}],
            }
            return error_codes(
                governance.detect_drift(
                    bundle,
                    live_reference_root=live_reference_root,
                    target_root=target_root,
                    official_source_artifact=artifact,
                )
            )

    def test_repository_head_drift_keeps_exact_code(self) -> None:
        self.assertIn(
            "repository_head_drift",
            self.repository_drift_codes({"revision_value": "0" * 40}),
        )

    def test_repository_tree_drift_keeps_exact_code(self) -> None:
        self.assertIn(
            "repository_tree_drift",
            self.repository_drift_codes({"tree_sha256": "0" * 64}),
        )

    def test_license_hash_drift_has_exact_code(self) -> None:
        self.assertIn(
            "license_hash_drift",
            self.repository_drift_codes({"license_sha256": "0" * 64}),
        )

    def test_content_tree_drift_has_exact_code(self) -> None:
        self.assertIn(
            "content_tree_drift",
            self.repository_drift_codes(
                {"tree_sha256": "0" * 64},
                revision_kind="content_tree_sha256",
            ),
        )

    def test_official_source_hash_drift_has_exact_code(self) -> None:
        artifact = {
            "artifact_id": "official-source-fixture",
            "content_sha256": "a" * 64,
        }
        observed = copy.deepcopy(artifact)
        observed["content_sha256"] = "b" * 64
        bundle = {
            "official_source_artifact": artifact,
            "repository_registry_revisions": [],
            "capability_decision_revisions": [],
        }

        codes = error_codes(
            governance.detect_drift(
                bundle,
                live_reference_root=ROOT / "reference",
                target_root=ROOT,
                official_source_artifact=observed,
            )
        )

        self.assertIn("official_source_hash_drift", codes)

    def test_unavailable_source_has_exact_code(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            live_reference_root = root / "reference"
            live_reference_root.mkdir()
            target_root = root / "target"
            target_root.mkdir()
            artifact = {
                "artifact_id": "official-source-fixture",
                "content_sha256": "a" * 64,
            }
            bundle = {
                "official_source_artifact": artifact,
                "repository_registry_revisions": [
                    {
                        "repositories": [
                            {
                                "repo_id": "missing-repository",
                                "path": "reference/missing-repository",
                                "revision_kind": "git_commit",
                                "revision_value": "0" * 40,
                            }
                        ]
                    }
                ],
                "capability_decision_revisions": [{"decisions": []}],
            }

            codes = error_codes(
                governance.detect_drift(
                    bundle,
                    live_reference_root=live_reference_root,
                    target_root=target_root,
                    official_source_artifact=artifact,
                )
            )

        self.assertIn("source_unavailable", codes)

    def test_target_revision_drift_keeps_exact_code(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target_root = root / "target"
            target_root.mkdir()
            (target_root / "artifact.bin").write_bytes(b"current artifact")
            artifact = {
                "artifact_id": "official-source-fixture",
                "content_sha256": "a" * 64,
            }
            bundle = {
                "official_source_artifact": artifact,
                "repository_registry_revisions": [],
                "capability_decision_revisions": [
                    {
                        "decisions": [
                            {
                                "decision_id": "decision.target",
                                "freshness": "current",
                                "target_path": "artifact.bin",
                                "target_revision_kind": "artifact_sha256",
                                "target_revision_value": "0" * 64,
                            }
                        ]
                    }
                ],
            }

            codes = error_codes(
                governance.detect_drift(
                    bundle,
                    live_reference_root=root,
                    target_root=target_root,
                    official_source_artifact=artifact,
                )
            )

        self.assertIn("target_revision_drift", codes)

    def test_symlink_escape_has_typed_usage_code(self) -> None:
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as outside:
            root = Path(directory)
            outside_file = Path(outside) / "outside.json"
            outside_file.write_text("{}", encoding="utf-8")
            (root / "escape.json").symlink_to(outside_file)

            with self.assertRaisesRegex(
                governance.GovernanceUsageError,
                "^symlink_escape:",
            ):
                governance.resolve_repository_path(root, "escape.json")

    def test_oversized_input_has_exact_code_and_usage_exit(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = Path(directory) / "oversized.json"
            fixture.write_bytes(b" " * (governance.MAX_JSON_BYTES + 1))
            result = subprocess.run(
                [
                    sys.executable,
                    str(VALIDATOR),
                    "validate",
                    "--fixture-bundle",
                    str(fixture),
                    "--json",
                ],
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=False,
            )

        report = json.loads(result.stdout)
        self.assertEqual(2, result.returncode)
        self.assertEqual("error", report["status"])
        self.assertIn("oversized_input", {error["code"] for error in report["errors"]})


if __name__ == "__main__":
    unittest.main()
