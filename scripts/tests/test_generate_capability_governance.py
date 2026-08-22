from __future__ import annotations

import copy
import importlib.util
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "scripts"
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import capability_governance as governance


CLI = SCRIPTS / "generate-capability-governance.py"
SPEC = importlib.util.spec_from_file_location(
    "kiana_generate_capability_governance_tests",
    CLI,
)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("capability governance generator module is unavailable")
generator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(generator)

GOVERNANCE_ROOT = ROOT / "docs/agent-program/kiana-completion/governance"

EXPECTED_PRODUCTION_CHECK_IDS = (
    "canonical-bundle",
    "current-selector",
    "evaluation-time",
    "repository-identities-38",
    "official-source-drift",
    "reference-drift",
    "target-drift",
    "integrity.evidence-expired",
    "integrity.newer-failed-retest",
    "legacy-authority",
    "generated-views",
    "compatibility-output",
    "public-baseline-diff",
    "repository-registry-diff",
)


def read_json(path: Path) -> dict[str, object]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise AssertionError(f"expected JSON object at {path}")
    return value


def write_json(path: Path, value: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


class CapabilityGovernanceGeneratorTests(unittest.TestCase):
    def test_diff_requires_the_current_selected_family_ancestry(self) -> None:
        selected = GOVERNANCE_ROOT / "repository-registry/references-2026-07-22.json"
        ancestor = GOVERNANCE_ROOT / "repository-registry/references-2026-07-15.json"
        unrelated = GOVERNANCE_ROOT / "capability-decisions/review-2026-07-18.json"

        generator.validate_diff_family_authority("repository-registry", selected, ROOT)
        generator.validate_diff_family_authority("repository-registry", ancestor, ROOT)
        with self.assertRaisesRegex(
            governance.GovernanceUsageError,
            "to revision is not in the selected family ancestry",
        ):
            generator.validate_diff_family_authority("repository-registry", unrelated, ROOT)

    def test_diff_structurally_validates_intermediate_predecessors(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            shutil.copy2(SCRIPTS / "validate-json-schema.py", root / "scripts")
            src = ROOT / "docs/schemas"
            if src.exists():
                shutil.copytree(src, root / "docs/schemas")
            else:
                (root / "docs/schemas").mkdir(parents=True)

            registry_root = (
                root
                / "docs/agent-program/kiana-completion/governance/repository-registry"
            )
            genesis_path = registry_root / "references-2026-07-15-genesis.json"
            predecessor_path = registry_root / "references-2026-07-15.json"
            head_path = registry_root / "references-2026-07-22.json"

            genesis = read_json(
                GOVERNANCE_ROOT
                / "repository-registry/references-2026-07-15-genesis.json"
            )
            predecessor = read_json(
                GOVERNANCE_ROOT / "repository-registry/references-2026-07-15.json"
            )
            head = read_json(
                GOVERNANCE_ROOT / "repository-registry/references-2026-07-22.json"
            )
            predecessor.pop("repositories")
            head["previous_revision_sha256"] = governance.canonical_sha256(predecessor)

            write_json(genesis_path, genesis)
            write_json(predecessor_path, predecessor)
            write_json(head_path, head)

            with self.assertRaisesRegex(
                governance.GovernanceUsageError,
                "schema_validation_failed",
            ):
                generator.validated_diff_document(
                    "repository-registry",
                    genesis_path,
                    head_path,
                    root,
                )


class ProductionReportAuthorityTests(unittest.TestCase):
    def _current_bundle_and_binding(self) -> tuple[dict[str, object], str, str]:
        bundle = governance.load_validated_manifest_bundle(
            GOVERNANCE_ROOT / "current.json",
            root=ROOT,
        )
        manifest = bundle["bundle_manifest"]
        self.assertIsInstance(manifest, dict)
        return (
            bundle,
            governance.canonical_sha256(manifest),
            str(manifest["evaluation_time"]),
        )

    def _passing_report(self, manifest_sha256: str, evaluation_time: str) -> dict[str, object]:
        return governance.build_production_report(
            manifest_sha256=manifest_sha256,
            evaluation_time=evaluation_time,
            checks=[
                {"id": check_id, "status": "pass"}
                for check_id in EXPECTED_PRODUCTION_CHECK_IDS
            ],
        )

    def test_production_report_is_a_closed_current_manifest_contract(self) -> None:
        _bundle, manifest_sha256, evaluation_time = self._current_bundle_and_binding()
        report = self._passing_report(manifest_sha256, evaluation_time)

        self.assertEqual(
            governance.PRODUCTION_REPORT_SCHEMA,
            "kiana.capability-governance-production-report.v1",
        )
        self.assertEqual(governance.PRODUCTION_REPORT_VERSION, "1.0")
        self.assertEqual(governance.PRODUCTION_CHECK_IDS, EXPECTED_PRODUCTION_CHECK_IDS)
        self.assertEqual(
            set(report),
            {
                "schema",
                "version",
                "status",
                "manifest_sha256",
                "evaluation_time",
                "checks",
            },
        )
        self.assertEqual(report["status"], "pass")
        self.assertEqual(
            [row["id"] for row in report["checks"]],
            list(EXPECTED_PRODUCTION_CHECK_IDS),
        )
        self.assertEqual(
            governance.validate_production_report(
                report,
                manifest_sha256=manifest_sha256,
                evaluation_time=evaluation_time,
            ),
            report,
        )

    def test_production_report_rejects_every_partial_or_nonpassing_mutation(self) -> None:
        _bundle, manifest_sha256, evaluation_time = self._current_bundle_and_binding()
        report = self._passing_report(manifest_sha256, evaluation_time)

        mutations = {
            "unknown_id": lambda value: value["checks"][0].update({"id": "unknown-check"}),
            "unknown_field": lambda value: value.update({"unexpected": "value"}),
            "missing_id": lambda value: value["checks"].pop(),
            "duplicate_id": lambda value: value["checks"][-1].update(
                {"id": EXPECTED_PRODUCTION_CHECK_IDS[0]}
            ),
            "stale_metadata": lambda value: value.update(
                {"evaluation_time": "2000-01-01T00:00:00Z"}
            ),
            "stale_row": lambda value: value["checks"][0].update({"status": "stale"}),
            "malformed_type": lambda value: value.update({"checks": {"id": "not-an-array"}}),
            "malformed_status": lambda value: value["checks"][0].update(
                {"status": "unknown"}
            ),
            "failing_row": lambda value: value["checks"][0].update({"status": "fail"}),
        }

        for label, mutate in mutations.items():
            with self.subTest(label=label):
                candidate = copy.deepcopy(report)
                mutate(candidate)
                with self.assertRaisesRegex(
                    governance.GovernanceUsageError,
                    "production_report_invalid",
                ):
                    governance.validate_production_report(
                        candidate,
                        manifest_sha256=manifest_sha256,
                        evaluation_time=evaluation_time,
                    )

    def test_verify_production_loads_the_complete_bundle_once(self) -> None:
        bundle, manifest_sha256, evaluation_time = self._current_bundle_and_binding()
        manifest = bundle["bundle_manifest"]
        self.assertIsInstance(manifest, dict)
        public_to = governance.resolve_repository_path(
            ROOT,
            str(manifest["public_baseline"]["path"]).split("#", 1)[0],
        )
        registry_to = governance.resolve_repository_path(
            ROOT,
            str(manifest["repository_registry"]["path"]).split("#", 1)[0],
        )

        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            args = SimpleNamespace(
                manifest=GOVERNANCE_ROOT / "current.json",
                generated_root=GOVERNANCE_ROOT / "generated",
                repository_output_root=ROOT,
                compat_manifest=GOVERNANCE_ROOT / "compat-outputs.json",
                public_from=GOVERNANCE_ROOT
                / "public-baselines/cc-public-2026-07-15-genesis.json",
                public_to=public_to,
                public_diff=GOVERNANCE_ROOT
                / "diffs/public-baseline/cc-public-2026-07-15.genesis-to-current.json",
                registry_from=GOVERNANCE_ROOT
                / "repository-registry/references-2026-07-15-genesis.json",
                registry_to=registry_to,
                registry_diff=GOVERNANCE_ROOT
                / "diffs/repository-registry/references-2026-07-15.genesis-to-current.json",
                report=output / "production-report.json",
                drift_report=output / "production-drift.json",
            )
            original_loader = generator.load_validated_manifest_bundle
            calls = 0

            def counted_loader(*arguments: object, **kwargs: object) -> dict[str, object]:
                nonlocal calls
                calls += 1
                return original_loader(*arguments, **kwargs)

            with mock.patch.object(
                generator,
                "load_validated_manifest_bundle",
                side_effect=counted_loader,
            ):
                self.assertTrue(generator.verify_production_command(args, ROOT))

            self.assertEqual(calls, 1)
            report = read_json(args.report)
            self.assertEqual(
                governance.validate_production_report(
                    report,
                    manifest_sha256=manifest_sha256,
                    evaluation_time=evaluation_time,
                ),
                report,
            )
            drift_report = read_json(args.drift_report)
            self.assertEqual(drift_report["status"], "current")
            self.assertEqual(drift_report["errors"], [])


    def test_verify_production_deterministic_json_bytes_call_count(self) -> None:
        """Profiling gate: deterministic_json_bytes must be called at most once per
        diff document per verify_production_command invocation.

        Pre-optimization baseline: 6 calls total —
          - 2 for public_document (preflight + write_or_check_outputs),
          - 2 for registry_document (preflight + write_or_check_outputs),
          - 2 from _write_json_output (drift_report + production report).
        Target after caching: at most 4 calls —
          - 1 for _public_bytes (cached, reused in both preflight and write),
          - 1 for _registry_bytes (cached, reused in both preflight and write),
          - 2 from _write_json_output (unchanged — one per JSON report write).

        This test fails RED before Task 2 caches the bytes; it becomes the
        immutable GREEN gate once caching is in place.
        """
        bundle, manifest_sha256, evaluation_time = self._current_bundle_and_binding()
        manifest = bundle["bundle_manifest"]
        self.assertIsInstance(manifest, dict)
        public_to = governance.resolve_repository_path(
            ROOT,
            str(manifest["public_baseline"]["path"]).split("#", 1)[0],
        )
        registry_to = governance.resolve_repository_path(
            ROOT,
            str(manifest["repository_registry"]["path"]).split("#", 1)[0],
        )

        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            args = SimpleNamespace(
                manifest=GOVERNANCE_ROOT / "current.json",
                generated_root=GOVERNANCE_ROOT / "generated",
                repository_output_root=ROOT,
                compat_manifest=GOVERNANCE_ROOT / "compat-outputs.json",
                public_from=GOVERNANCE_ROOT
                / "public-baselines/cc-public-2026-07-15-genesis.json",
                public_to=public_to,
                public_diff=GOVERNANCE_ROOT
                / "diffs/public-baseline/cc-public-2026-07-15.genesis-to-current.json",
                registry_from=GOVERNANCE_ROOT
                / "repository-registry/references-2026-07-15-genesis.json",
                registry_to=registry_to,
                registry_diff=GOVERNANCE_ROOT
                / "diffs/repository-registry/references-2026-07-15.genesis-to-current.json",
                report=output / "production-report.json",
                drift_report=output / "production-drift.json",
            )
            call_count = 0
            original_fn = generator.deterministic_json_bytes

            def counting_fn(*arguments: object, **kwargs: object) -> bytes:
                nonlocal call_count
                call_count += 1
                return original_fn(*arguments, **kwargs)

            with mock.patch.object(
                generator,
                "deterministic_json_bytes",
                side_effect=counting_fn,
            ):
                self.assertTrue(generator.verify_production_command(args, ROOT))

            # Target: at most 4 calls total —
            #   - 1 cached assignment for _public_bytes
            #   - 1 cached assignment for _registry_bytes
            #   - 2 from _write_json_output (drift_report + production report)
            # Pre-optimization baseline: 6 calls (double-computation for both
            # diff documents in preflight+write, plus 2 _write_json_output calls).
            self.assertLessEqual(
                call_count,
                4,
                f"deterministic_json_bytes called {call_count} times in "
                f"verify_production_command; expected at most 4 (one cached "
                f"bytes per diff document + 2 _write_json_output calls). "
                f"Pre-optimization baseline is 6 (double-computation for both "
                f"public_document and registry_document).",
            )


if __name__ == "__main__":
    unittest.main()
