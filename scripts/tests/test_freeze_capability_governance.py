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


if __name__ == "__main__":
    unittest.main()
