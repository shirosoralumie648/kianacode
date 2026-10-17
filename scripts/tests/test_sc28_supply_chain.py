"""Deny-first behavior fixtures for the SC-28 supply-chain normalizer.

The shell wrapper (scripts/supply-chain-scan.sh) owns Cargo and scanner process
execution. These fixtures drive scripts/supply-chain-scan.py only, offline, over
synthetic Cargo metadata and scanner output in a temporary directory: no fixture
resolves a real dependency, runs cargo, installs a scanner, reaches the network
or grants a release permission.

Deny-first ordering: every quarantine path is proven before the single success
fixture, which is the last test in this file.
"""

import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "supply-chain-scan.py"
SPEC = importlib.util.spec_from_file_location("sc28_supply_chain_scan", SCRIPT)
SCAN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SCAN)

REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"


def package(name, version="1.0.0", license_id="MIT", source=REGISTRY, **extra):
    """A Cargo metadata package entry with a stable synthetic identity."""

    item = {
        "id": f"{name} {version} ({source})",
        "name": name,
        "version": version,
        "source": source,
        "manifest_path": f"/fixture/{name}-{version}/Cargo.toml",
    }
    if license_id is not None:
        item["license"] = license_id
    item.update(extra)
    return item


def metadata(packages, edges=()):
    """Cargo metadata with a resolve graph over the given packages."""

    by_name = {item["name"]: item["id"] for item in packages}
    return {
        "packages": list(packages),
        "workspace_members": [packages[0]["id"]] if packages else [],
        "resolve": {
            "nodes": [
                {"id": by_name[name], "dependencies": [by_name[d] for d in deps]}
                for name, deps in edges
                if name in by_name
            ]
        },
    }


def audit(severities, exit_code=0):
    """A cargo-audit style JSON document with the requested finding severities."""

    return {
        "vulnerabilities": {
            "list": [
                {
                    "advisory": {
                        "id": f"RUSTSEC-2026-{index:04d}",
                        "severity": severity,
                        "url": f"https://example.invalid/RUSTSEC-2026-{index:04d}",
                    },
                    "package": {"name": f"vulnerable-{index}", "version": "0.1.0"},
                    "versions": {"patched": [">=9.9.9"]},
                }
                for index, severity in enumerate(severities)
            ]
        },
        "settings": {"exit_code": exit_code},
    }


class ScanFixture(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def run_scan(
        self,
        *,
        metadata_object=None,
        lock_text="# locked\n",
        lock_present=True,
        metadata_exit=0,
        audit_document=None,
        audit_exit=0,
        deny_exit=0,
        audit_tool="cargo-audit",
        deny_tool="cargo-deny",
        audit_version="cargo-audit 0.21.2",
        deny_version="cargo-deny 0.18.4",
        lock_before=None,
        lock_after=None,
        lock_dirty="false",
        expected_digest="",
        max_high=0,
        max_critical=0,
        out_name="out",
    ):
        """Run the normalizer over synthetic inputs and return its artifacts."""

        out_dir = self.root / out_name
        lockfile = self.root / "Cargo.lock"
        if lock_present:
            lockfile.write_text(lock_text, encoding="utf-8")
        digest = (
            hashlib.sha256(lock_text.encode("utf-8")).hexdigest()
            if lock_present
            else "missing"
        )
        # Mirror the shell wrapper: a scanner that is not installed contributes no
        # version, so the report can never name a tool that did not run.
        effective_audit_version = (
            "unknown" if audit_tool == "missing" else audit_version
        )
        effective_deny_version = "unknown" if deny_tool == "missing" else deny_version
        metadata_file = self.root / "cargo-metadata.json"
        if metadata_object is not None:
            metadata_file.write_text(
                json.dumps(metadata_object, sort_keys=True), encoding="utf-8"
            )
        audit_file = self.root / "cargo-audit.json"
        if audit_document is not None:
            audit_file.write_text(
                json.dumps(audit_document, sort_keys=True), encoding="utf-8"
            )
        argv = [
            "supply-chain-scan.py",
            "--metadata",
            str(metadata_file),
            "--lockfile",
            str(lockfile),
            "--out-dir",
            str(out_dir),
            "--source-revision",
            "fixture-revision",
            "--lock-before",
            digest if lock_before is None else lock_before,
            "--lock-after",
            digest if lock_after is None else lock_after,
            "--lock-present",
            "true" if lock_present else "false",
            "--metadata-exit",
            str(metadata_exit),
            "--lock-dirty",
            lock_dirty,
            "--expected-lock-digest",
            expected_digest,
            "--audit-json",
            str(audit_file),
            "--audit-exit",
            str(audit_exit),
            "--deny-exit",
            str(deny_exit),
            "--audit-tool",
            audit_tool,
            "--deny-tool",
            deny_tool,
            "--audit-version",
            effective_audit_version,
            "--deny-version",
            effective_deny_version,
            "--max-high",
            str(max_high),
            "--max-critical",
            str(max_critical),
        ]
        stdout = io.StringIO()
        with patch.object(sys, "argv", argv), contextlib.redirect_stdout(stdout):
            exit_code = SCAN.main()
        report = json.loads(
            (out_dir / "supply-chain-report.json").read_text(encoding="utf-8")
        )
        quarantine = json.loads(
            (out_dir / "quarantine.json").read_text(encoding="utf-8")
        )
        return {
            "exit_code": exit_code,
            "stdout": stdout.getvalue(),
            "report": report,
            "quarantine": quarantine,
            "out_dir": out_dir,
            "digest": digest,
        }

    def codes(self, result):
        return [reason["code"] for reason in result["quarantine"]["reasons"]]

    def assertQuarantined(self, result, *expected_codes):
        self.assertEqual(result["exit_code"], 1)
        self.assertEqual(result["report"]["status"], "quarantined")
        self.assertEqual(result["report"]["quarantine"]["status"], "quarantined")
        self.assertFalse(result["quarantine"]["automatic_release_allowed"])
        self.assertNotEqual(result["report"]["limitations"], [])
        for code in expected_codes:
            self.assertIn(code, self.codes(result))

    def assertReleased(self, result):
        self.assertEqual(result["exit_code"], 0)
        self.assertEqual(result["report"]["status"], "passed")
        self.assertEqual(result["report"]["quarantine"]["reasons"], [])
        self.assertEqual(result["quarantine"]["status"], "clear")
        self.assertTrue(result["quarantine"]["automatic_release_allowed"])

    def clean_metadata(self):
        return metadata(
            [
                package("kiana", source=None, license_id="MIT"),
                package("anyhow", license_id="MIT OR Apache-2.0"),
            ],
            edges=[("kiana", ["anyhow"])],
        )


class DenyFirstFixtures(ScanFixture):
    def test_missing_lockfile_quarantines_before_any_resolution(self):
        result = self.run_scan(
            lock_present=False,
            metadata_object=None,
            metadata_exit=127,
            audit_document=audit([]),
        )
        self.assertQuarantined(
            result,
            "lockfile_missing",
            "dependency_graph_empty",
        )
        # An absent lockfile is not also reported as drift: the missing-lock
        # reason already describes it, and a duplicate reason would let a reader
        # believe two independent controls fired.
        self.assertNotIn("lockfile_drift", self.codes(result))
        self.assertEqual(result["report"]["lockfile"]["sha256"], "missing")
        self.assertFalse(result["report"]["lockfile"]["present"])
        self.assertFalse(result["report"]["dependency_graph"]["resolved"])
        for name in ("sbom.cdx.json", "sbom.spdx.json"):
            self.assertTrue((result["out_dir"] / name).is_file())

    def test_an_unlocked_dependency_graph_quarantines_without_a_silent_pass(self):
        result = self.run_scan(
            metadata_object=None,
            metadata_exit=101,
            audit_document=audit([]),
        )
        self.assertQuarantined(
            result,
            "lockfile_resolution_failed",
            "dependency_graph_empty",
        )
        self.assertEqual(result["report"]["dependency_graph"]["package_count"], 0)
        self.assertEqual(
            result["report"]["dependency_graph"]["metadata_exit_code"], 101
        )
        self.assertFalse(result["report"]["dependency_graph"]["resolved"])

    def test_lockfile_drift_between_resolution_and_scan_quarantines(self):
        rewritten = self.run_scan(
            metadata_object=self.clean_metadata(),
            audit_document=audit([]),
            lock_before="a" * 64,
            lock_after="b" * 64,
        )
        self.assertQuarantined(rewritten, "lockfile_drift")
        dirty = self.run_scan(
            metadata_object=self.clean_metadata(),
            audit_document=audit([]),
            lock_dirty="true",
        )
        self.assertQuarantined(dirty, "lockfile_drift")
        self.assertTrue(dirty["report"]["lockfile"]["dirty"])

    def test_expected_lockfile_digest_mismatch_quarantines(self):
        result = self.run_scan(
            metadata_object=self.clean_metadata(),
            audit_document=audit([]),
            expected_digest="c" * 64,
        )
        self.assertQuarantined(result, "lockfile_digest_mismatch")
        self.assertEqual(result["report"]["lockfile"]["expected_sha256"], "c" * 64)

    def test_unknown_license_metadata_quarantines_and_stays_noassertion(self):
        result = self.run_scan(
            metadata_object=metadata(
                [
                    package("kiana", source=None),
                    package("unlicensed", license_id=None),
                ]
            ),
            audit_document=audit([]),
        )
        self.assertQuarantined(result, "license_unknown")
        self.assertEqual(result["report"]["license"]["unknown_count"], 1)
        self.assertEqual(
            result["report"]["license"]["unknown"][0]["name"], "unlicensed"
        )
        cyclonedx = json.loads(
            (result["out_dir"] / "sbom.cdx.json").read_text(encoding="utf-8")
        )
        self.assertIn(
            {"license": {"name": "NOASSERTION"}},
            [component["licenses"][0] for component in cyclonedx["components"]],
        )
        spdx = json.loads(
            (result["out_dir"] / "sbom.spdx.json").read_text(encoding="utf-8")
        )
        per_package = {item["name"]: item for item in spdx["packages"]}
        self.assertEqual(per_package["unlicensed"]["licenseConcluded"], "NOASSERTION")
        self.assertEqual(per_package["unlicensed"]["licenseDeclared"], "NOASSERTION")

    def test_missing_scanner_binaries_never_count_as_a_pass(self):
        result = self.run_scan(
            metadata_object=self.clean_metadata(),
            audit_document=None,
            audit_tool="missing",
            deny_tool="missing",
        )
        self.assertQuarantined(
            result,
            "advisory_scanner_missing",
            "policy_scanner_missing",
        )
        self.assertEqual(result["report"]["advisory"]["count"], 0)
        self.assertEqual(result["report"]["scanner_tools"]["cargo_audit"], "unknown")

    def test_scanner_exit_codes_are_recorded_as_stable_reasons(self):
        result = self.run_scan(
            metadata_object=self.clean_metadata(),
            audit_document=audit(["high", "high", "medium"]),
            audit_exit=1,
            deny_exit=1,
        )
        self.assertQuarantined(
            result,
            "advisory_scan_failed",
            "advisory_found",
            "high_threshold_exceeded",
            "dependency_policy_failed",
        )
        self.assertEqual(result["report"]["advisory"]["severity_counts"]["high"], 2)
        self.assertEqual(result["report"]["dependency_policy"]["exit_code"], 1)

    def test_default_budget_tolerates_no_advisory_of_any_severity(self):
        result = self.run_scan(
            metadata_object=self.clean_metadata(),
            audit_document=audit(["low"]),
        )
        self.assertQuarantined(result, "advisory_found")
        # The zero-tolerance default is the count guard, not a severity guard: a
        # single low finding must not slip through a threshold comparison.
        self.assertNotIn("high_threshold_exceeded", self.codes(result))
        self.assertNotIn("critical_threshold_exceeded", self.codes(result))
        self.assertEqual(result["report"]["advisory"]["budget"], 0)

    def test_a_reviewed_budget_is_the_only_path_that_tolerates_classified_findings(
        self,
    ):
        tolerated = self.run_scan(
            metadata_object=self.clean_metadata(),
            audit_document=audit(["high", "critical"]),
            max_high=1,
            max_critical=1,
        )
        self.assertReleased(tolerated)
        self.assertEqual(tolerated["report"]["advisory"]["budget"], 2)
        exceeded = self.run_scan(
            metadata_object=self.clean_metadata(),
            audit_document=audit(["high", "critical", "critical"]),
            max_high=1,
            max_critical=1,
            out_name="out-exceeded",
        )
        self.assertQuarantined(
            exceeded,
            "critical_threshold_exceeded",
            "advisory_found",
        )

    def test_unclassified_severity_is_never_tolerated_even_with_a_raised_budget(self):
        result = self.run_scan(
            metadata_object=self.clean_metadata(),
            audit_document=audit(["not-a-severity"]),
            max_high=5,
            max_critical=5,
        )
        self.assertQuarantined(result, "advisory_severity_unclassified")
        self.assertNotIn("advisory_found", self.codes(result))

    def test_findings_and_licenses_stay_bounded_in_the_report(self):
        result = self.run_scan(
            metadata_object=metadata(
                [
                    package(f"unlicensed-{index}", license_id=None)
                    for index in range(150)
                ]
            ),
            audit_document=audit(["high"] * 1200),
        )
        self.assertQuarantined(result, "license_unknown", "advisory_found")
        self.assertEqual(result["report"]["license"]["unknown_count"], 150)
        self.assertEqual(len(result["report"]["license"]["unknown"]), 100)
        self.assertEqual(result["report"]["advisory"]["count"], 1000)
        self.assertEqual(len(result["report"]["advisory"]["findings"]), 1000)

    def test_every_reason_comes_from_the_closed_reason_code_set(self):
        results = [
            self.run_scan(
                lock_present=False,
                metadata_exit=127,
                audit_document=audit([]),
                out_name="missing",
            ),
            self.run_scan(
                metadata_object=None,
                metadata_exit=1,
                audit_document=audit(["low"]),
                audit_exit=1,
                deny_exit=1,
                out_name="unresolved",
            ),
            self.run_scan(
                metadata_object=self.clean_metadata(),
                audit_document=audit(["unheard-of"]),
                lock_dirty="true",
                expected_digest="d" * 64,
                out_name="drift",
            ),
        ]
        observed = set()
        for result in results:
            observed.update(self.codes(result))
            self.assertEqual(
                result["quarantine"]["reason_codes"], list(SCAN.REASON_CODES)
            )
            for reason in result["quarantine"]["reasons"]:
                self.assertIn(reason["code"], SCAN.REASON_CODES)
                self.assertNotEqual(reason["detail"], "")
        self.assertIn("lockfile_missing", observed)
        self.assertIn("lockfile_resolution_failed", observed)
        self.assertIn("lockfile_drift", observed)
        self.assertIn("lockfile_digest_mismatch", observed)
        self.assertIn("advisory_severity_unclassified", observed)

    def test_the_normalizer_never_runs_processes_or_reaches_the_network(self):
        source = SCRIPT.read_text(encoding="utf-8")
        forbidden_modules = {
            "subprocess",
            "socket",
            "urllib",
            "http",
            "requests",
            "shutil",
            "ssl",
            "ftplib",
            "ctypes",
            "asyncio",
        }
        for line in source.splitlines():
            stripped = line.strip()
            if not stripped.startswith(("import ", "from ")):
                continue
            module = stripped.split()[1].split(".")[0]
            self.assertNotIn(module, forbidden_modules, stripped)
        for forbidden in ("subprocess", "socket", "urllib"):
            self.assertNotIn(forbidden, SCAN.__dict__)
        # The SBOM writer binds bytes only: it never fetches a manifest either.
        self.assertNotIn("urlopen", source)
        self.assertNotIn("requests.get", source)

    def test_clean_locked_snapshot_passes_and_emits_deterministic_manifests(self):
        first = self.run_scan(
            metadata_object=self.clean_metadata(),
            audit_document=audit([]),
            out_name="first",
        )
        second = self.run_scan(
            metadata_object=self.clean_metadata(),
            audit_document=audit([]),
            out_name="second",
        )
        self.assertReleased(first)
        self.assertReleased(second)
        self.assertEqual(first["exit_code"], second["exit_code"])
        self.assertEqual(first["report"]["lockfile"]["sha256"], first["digest"])
        self.assertEqual(
            first["report"]["scanner_tools"]["cargo_audit"], "cargo-audit 0.21.2"
        )
        self.assertEqual(
            first["report"]["scanner_tools"]["cargo_deny"], "cargo-deny 0.18.4"
        )
        self.assertEqual(first["report"]["schema"], SCAN.SCHEMA)
        self.assertEqual(first["quarantine"]["schema"], SCAN.QUARANTINE_SCHEMA)
        self.assertTrue(first["report"]["dependency_graph"]["resolved"])
        self.assertEqual(first["report"]["dependency_graph"]["package_count"], 2)
        for name in ("sbom.cdx.json", "sbom.spdx.json"):
            left = (first["out_dir"] / name).read_bytes()
            right = (second["out_dir"] / name).read_bytes()
            self.assertEqual(left, right, f"{name} is not deterministic")
        cyclonedx = json.loads(
            (first["out_dir"] / "sbom.cdx.json").read_text(encoding="utf-8")
        )
        self.assertEqual(cyclonedx["bomFormat"], "CycloneDX")
        self.assertEqual(cyclonedx["specVersion"], "1.5")
        self.assertTrue(cyclonedx["serialNumber"].startswith("urn:uuid:"))
        properties = {
            item["name"]: item["value"] for item in cyclonedx["metadata"]["properties"]
        }
        self.assertEqual(properties["kiana:lockfile-sha256"], first["digest"])
        self.assertEqual(properties["kiana:source-revision"], "fixture-revision")
        self.assertEqual(
            cyclonedx["dependencies"],
            sorted(cyclonedx["dependencies"], key=lambda item: item["ref"]),
        )
        self.assertEqual(
            cyclonedx["dependencies"][0]["dependsOn"], ["pkg:cargo/anyhow@1.0.0"]
        )
        spdx = json.loads(
            (first["out_dir"] / "sbom.spdx.json").read_text(encoding="utf-8")
        )
        self.assertEqual(spdx["spdxVersion"], "SPDX-2.3")
        self.assertIn(first["digest"], spdx["documentNamespace"])
        self.assertEqual(
            spdx["relationships"][0]["relatedSpdxElement"], "SPDXRef-Application"
        )
        self.assertIn(
            "DEPENDS_ON",
            [item["relationshipType"] for item in spdx["relationships"]],
        )
        self.assertIn(
            {"name": "kiana", "version": "1.0.0"},
            [
                {"name": item["name"], "version": item["versionInfo"]}
                for item in spdx["packages"]
            ],
        )
        self.assertEqual(
            spdx["annotations"][0]["comment"], f"lockfile-sha256={first['digest']}"
        )


if __name__ == "__main__":
    unittest.main()
