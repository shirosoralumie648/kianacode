from __future__ import annotations

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


CLI = ROOT / "scripts" / "freeze-capability-governance.py"
OFFLINE_SOURCE = (
    ROOT
    / "scripts"
    / "fixtures"
    / "capability-governance"
    / "valid"
    / "offline-source-identity.json"
)
REFRESH_REQUEST = (
    ROOT
    / "scripts"
    / "fixtures"
    / "capability-governance"
    / "valid"
    / "refresh-request.json"
)
REFRESH_RESULT = REFRESH_REQUEST.with_name("refresh-result.json")
MINIMAL_GRAPH = REFRESH_REQUEST.with_name("minimal-graph.json")
VALIDATOR = ROOT / "scripts" / "validate-capability-governance.py"

CASES = [
    "no_drift",
    "repository_head_drift",
    "repository_tree_drift",
    "license_hash_drift",
    "content_tree_drift",
    "official_source_hash_drift",
    "target_revision_drift",
    "source_unavailable",
]


def run_cli(*args: object) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(CLI), *(str(arg) for arg in args)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )


def read_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise AssertionError(f"expected JSON object at {path}")
    return value


class CapabilityGovernanceFreezeTests(unittest.TestCase):
    def test_freeze_writes_canonical_offline_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "source.json"
            result = run_cli(
                "freeze",
                "--kind",
                "official-source",
                "--input",
                OFFLINE_SOURCE,
                "--output",
                output,
                "--artifact-id",
                "fixture-source",
                "--retrieved-at",
                "2026-07-15T00:00:00Z",
                "--archive-uri",
                "https://archive.example.invalid/kiana/fixture-source",
            )

            self.assertEqual(0, result.returncode, result.stderr)
            report = read_json(output)

        source = read_json(OFFLINE_SOURCE)["official_source_artifact"]
        self.assertEqual("kiana.capability-governance-freeze.v1", report["schema"])
        self.assertEqual("official-source", report["kind"])
        self.assertEqual("fixture-source", report["artifact_id"])
        self.assertEqual("2026-07-15T00:00:00Z", report["retrieved_at"])
        self.assertEqual(
            "https://archive.example.invalid/kiana/fixture-source",
            report["archive_uri"],
        )
        self.assertEqual(source["official_uri"], report["official_uri"])
        self.assertEqual(source, report["artifact"])
        self.assertEqual(governance.canonical_sha256(source), report["content_sha256"])
        self.assertEqual("canonical-json-rfc8785-compatible", report["normalization"]["method"])

    def test_freeze_refuses_existing_output_without_changing_it(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "source.json"
            output.write_bytes(b"protected predecessor\n")
            before = output.read_bytes()

            result = run_cli(
                "freeze",
                "--kind",
                "official-source",
                "--input",
                OFFLINE_SOURCE,
                "--output",
                output,
                "--artifact-id",
                "fixture-source",
                "--retrieved-at",
                "2026-07-15T00:00:00Z",
                "--archive-uri",
                "https://archive.example.invalid/kiana/fixture-source",
            )

            self.assertEqual(2, result.returncode, result.stderr)
            self.assertEqual(before, output.read_bytes())
            self.assertIn("output_exists", result.stderr)

    def test_freeze_refuses_symlink_output_without_touching_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "protected.json"
            target.write_bytes(b"protected target\n")
            output = root / "source.json"
            output.symlink_to(target)

            result = run_cli(
                "freeze",
                "--kind",
                "official-source",
                "--input",
                OFFLINE_SOURCE,
                "--output",
                output,
                "--artifact-id",
                "fixture-source",
                "--retrieved-at",
                "2026-07-15T00:00:00Z",
                "--archive-uri",
                "https://archive.example.invalid/kiana/fixture-source",
            )

            self.assertEqual(2, result.returncode, result.stderr)
            self.assertEqual(b"protected target\n", target.read_bytes())
            self.assertIn("output_exists", result.stderr)


class CapabilityGovernanceDriftCliTests(unittest.TestCase):
    def test_fixture_cases_have_exact_status_code_and_event_contracts(self) -> None:
        expected = {
            "no_drift": (0, "current", []),
            **{
                case: (1, "stale", [case])
                for case in CASES[1:-1]
            },
            "source_unavailable": (1, "unavailable", ["source_unavailable"]),
        }

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for case in CASES:
                with self.subTest(case=case):
                    output = root / f"{case}.json"
                    result = run_cli(
                        "check-drift",
                        "--fixture-bundle",
                        REFRESH_REQUEST,
                        "--case",
                        case,
                        "--output",
                        output,
                    )
                    expected_exit, expected_status, expected_codes = expected[case]
                    self.assertEqual(expected_exit, result.returncode, result.stderr)
                    report = read_json(output)
                    self.assertEqual(
                        "kiana.capability-governance-drift-report.v1",
                        report["schema"],
                    )
                    self.assertEqual(case, report["case"])
                    self.assertEqual(expected_status, report["status"])
                    self.assertEqual(case == "no_drift", report["current"])
                    self.assertEqual(
                        expected_codes,
                        [error["code"] for error in report["errors"]],
                    )
                    self.assertIsInstance(report["frozen_fingerprints"], dict)
                    self.assertIsInstance(report["observed_fingerprints"], dict)
                    self.assertIsInstance(report["affected_subjects"], list)
                    self.assertIsInstance(report["events"], list)
                    if case == "no_drift":
                        self.assertEqual("current", report["freshness"])
                        self.assertEqual([], report["events"])
                    elif case == "source_unavailable":
                        self.assertNotEqual("current", report["freshness"])
                        self.assertEqual(
                            ["blocked_observation"],
                            [event["event_type"] for event in report["events"]],
                        )
                    else:
                        self.assertEqual("stale", report["freshness"])
                        self.assertEqual(
                            ["stale"],
                            [event["event_type"] for event in report["events"]],
                        )

    def test_production_check_drift_requires_every_live_input(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "drift.json"
            result = run_cli(
                "check-drift",
                "--manifest",
                OFFLINE_SOURCE,
                "--output",
                output,
            )

            self.assertEqual(2, result.returncode)
            self.assertFalse(output.exists())
            self.assertIn("live-reference-root", result.stderr)
            self.assertIn("target-root", result.stderr)
            self.assertIn("official-source-artifact", result.stderr)

    def test_fixture_mode_cannot_mix_production_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "drift.json"
            result = run_cli(
                "check-drift",
                "--fixture-bundle",
                REFRESH_REQUEST,
                "--case",
                "no_drift",
                "--manifest",
                OFFLINE_SOURCE,
                "--live-reference-root",
                ROOT / "reference",
                "--target-root",
                ROOT,
                "--official-source-artifact",
                OFFLINE_SOURCE,
                "--output",
                output,
            )

            self.assertEqual(2, result.returncode)
            self.assertFalse(output.exists())
            self.assertIn("cannot be combined", result.stderr)


class CapabilityGovernanceRefreshTests(unittest.TestCase):
    def run_refresh(self, output_root: Path) -> subprocess.CompletedProcess[str]:
        return run_cli(
            "refresh",
            "--fixture-bundle",
            MINIMAL_GRAPH,
            "--drift-report",
            REFRESH_REQUEST,
            "--output-root",
            output_root,
            "--revision-id",
            "fixture-refresh",
            "--review-revision",
            "fixture-review",
        )

    def test_refresh_creates_valid_immutable_successors_and_exact_result(self) -> None:
        predecessor_bytes = MINIMAL_GRAPH.read_bytes()
        predecessor = read_json(MINIMAL_GRAPH)
        with tempfile.TemporaryDirectory() as directory:
            output_root = Path(directory) / "refresh"
            result = self.run_refresh(output_root)

            self.assertEqual(0, result.returncode, result.stderr)
            self.assertEqual(predecessor_bytes, MINIMAL_GRAPH.read_bytes())
            current = read_json(output_root / "current.json")
            actual_result = (output_root / "refresh-result.json").read_bytes()
            self.assertEqual(REFRESH_RESULT.read_bytes(), actual_result)
            validation = subprocess.run(
                [
                    sys.executable,
                    str(VALIDATOR),
                    "validate",
                    "--manifest",
                    str(output_root / "current.json"),
                    "--json",
                ],
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(0, validation.returncode, validation.stdout + validation.stderr)
            self.assertEqual("valid", json.loads(validation.stdout)["status"])

        source = current["official_source_artifact"]
        old_source = predecessor["official_source_artifact"]
        self.assertEqual("successor", source["revision_kind"])
        self.assertEqual(old_source["revision_id"], source["previous_revision_id"])
        self.assertEqual(
            governance.canonical_sha256(old_source),
            source["previous_revision_sha256"],
        )

        for family in (
            "public_baseline_revisions",
            "repository_registry_revisions",
            "capability_decision_revisions",
            "evidence_revisions",
        ):
            with self.subTest(family=family):
                old_head = predecessor[family][-1]
                new_head = current[family][-1]
                self.assertEqual("successor", new_head["revision_kind"])
                self.assertEqual(old_head["revision_id"], new_head["previous_revision_id"])
                self.assertEqual(
                    governance.canonical_sha256(old_head),
                    new_head["previous_revision_sha256"],
                )

        old_records = predecessor["evidence_revisions"][-1]["records"]
        new_records = current["evidence_revisions"][-1]["records"]
        self.assertEqual(old_records, new_records[: len(old_records)])
        appended = [
            record
            for record in new_records[len(old_records) :]
            if ".event." in record["evidence_id"]
        ]
        expected_codes = [error["code"] for error in read_json(REFRESH_REQUEST)["errors"]]
        self.assertEqual(
            expected_codes,
            [record["transition_event"]["reason"].removeprefix("drift:") for record in appended],
        )
        self.assertTrue(all(record["freshness"] == "stale" for record in appended))
        self.assertTrue(
            all(record["transition_event"]["event_type"] == "stale" for record in appended)
        )

    def test_refresh_request_subjects_exist_in_predecessor(self) -> None:
        request = read_json(REFRESH_REQUEST)
        predecessor = read_json(MINIMAL_GRAPH)
        known = {predecessor["official_source_artifact"]["artifact_id"]}
        known.update(
            repository["repo_id"]
            for repository in predecessor["repository_registry_revisions"][-1]["repositories"]
        )
        known.update(
            decision["decision_id"]
            for decision in predecessor["capability_decision_revisions"][-1]["decisions"]
        )

        self.assertEqual(set(request["affected_subjects"]), set(request["affected_subjects"]) & known)
        self.assertEqual(
            set(request["affected_subjects"]),
            {error["subject_id"] for error in request["errors"]},
        )

    def test_refresh_rejects_clean_report_and_existing_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            clean = read_json(REFRESH_REQUEST)
            clean.update(
                {
                    "affected_subjects": [],
                    "current": True,
                    "errors": [],
                    "events": [],
                    "freshness": "current",
                    "status": "current",
                }
            )
            clean_path = root / "clean.json"
            clean_path.write_text(json.dumps(clean), encoding="utf-8")
            clean_output = root / "clean-output"
            clean_result = run_cli(
                "refresh",
                "--fixture-bundle",
                MINIMAL_GRAPH,
                "--drift-report",
                clean_path,
                "--output-root",
                clean_output,
                "--revision-id",
                "fixture-refresh",
                "--review-revision",
                "fixture-review",
            )
            self.assertEqual(2, clean_result.returncode)
            self.assertFalse(clean_output.exists())
            self.assertIn("drift_required", clean_result.stderr)

            existing = root / "existing"
            existing.mkdir()
            sentinel = existing / "protected.txt"
            sentinel.write_bytes(b"protected predecessor\n")
            existing_result = self.run_refresh(existing)
            self.assertEqual(2, existing_result.returncode)
            self.assertEqual(b"protected predecessor\n", sentinel.read_bytes())
            self.assertIn("output_exists", existing_result.stderr)

    def test_refresh_rejects_unknown_affected_subject(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            request = read_json(REFRESH_REQUEST)
            request["affected_subjects"].append("unknown-subject")
            request["errors"].append(
                {
                    "code": "repository_tree_drift",
                    "detail": "",
                    "freshness": "stale",
                    "path": "reference/unknown",
                    "subject_id": "unknown-subject",
                }
            )
            request_path = root / "unknown.json"
            request_path.write_text(json.dumps(request), encoding="utf-8")
            output_root = root / "output"
            result = run_cli(
                "refresh",
                "--fixture-bundle",
                MINIMAL_GRAPH,
                "--drift-report",
                request_path,
                "--output-root",
                output_root,
                "--revision-id",
                "fixture-refresh",
                "--review-revision",
                "fixture-review",
            )

            self.assertEqual(2, result.returncode)
            self.assertFalse(output_root.exists())
            self.assertIn("affected_subject_unknown", result.stderr)

    def test_single_target_drift_only_advances_affected_families(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            request = read_json(REFRESH_REQUEST)
            request["affected_subjects"] = ["decision.repo-git"]
            request["errors"] = [
                error
                for error in request["errors"]
                if error["code"] == "target_revision_drift"
            ]
            request["events"] = [
                event
                for event in request["events"]
                if event["code"] == "target_revision_drift"
            ]
            request_path = root / "target-drift.json"
            request_path.write_text(json.dumps(request), encoding="utf-8")
            output_root = root / "output"
            result = run_cli(
                "refresh",
                "--fixture-bundle",
                MINIMAL_GRAPH,
                "--drift-report",
                request_path,
                "--output-root",
                output_root,
                "--revision-id",
                "target-refresh",
                "--review-revision",
                "target-review",
            )

            self.assertEqual(0, result.returncode, result.stderr)
            predecessor = read_json(MINIMAL_GRAPH)
            current = read_json(output_root / "current.json")

        self.assertEqual(
            predecessor["official_source_artifact"],
            current["official_source_artifact"],
        )
        self.assertEqual(
            len(predecessor["public_baseline_revisions"]),
            len(current["public_baseline_revisions"]),
        )
        self.assertEqual(
            len(predecessor["repository_registry_revisions"]),
            len(current["repository_registry_revisions"]),
        )
        self.assertEqual(
            len(predecessor["capability_decision_revisions"]) + 1,
            len(current["capability_decision_revisions"]),
        )
        self.assertEqual(
            len(predecessor["evidence_revisions"]) + 1,
            len(current["evidence_revisions"]),
        )


if __name__ == "__main__":
    unittest.main()
