#!/usr/bin/env python3
"""Execute the post-implementation capability-governance negative corpus."""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import importlib.util
import io
import json
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any, Mapping


ROOT = Path(__file__).resolve().parent.parent
SCRIPTS = ROOT / "scripts"
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import capability_governance as governance


INTEGRITY_ROOT = SCRIPTS / "fixtures/capability-governance/invalid/integrity"
COVERAGE_ROOT = SCRIPTS / "fixtures/capability-governance/invalid/coverage"
VALID_ROOT = SCRIPTS / "fixtures/capability-governance/valid"
VALIDATOR_PATH = SCRIPTS / "validate-capability-governance.py"
MAX_CAPTURE_CHARS = 1 << 20
PROTECTED_INPUTS = (
    VALID_ROOT / "minimal-graph.json",
    VALID_ROOT / "full-38-repositories.json",
    VALID_ROOT / "offline-source-identity.json",
    VALID_ROOT / "hostile-rendering.json",
    SCRIPTS / "validate-json-schema.py",
)


class CorpusFailure(RuntimeError):
    pass


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def protected_hashes() -> dict[str, str]:
    return {
        path.relative_to(ROOT).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in PROTECTED_INPUTS
    }


def load_cli_module() -> Any:
    spec = importlib.util.spec_from_file_location("kiana_capability_governance_cli", VALIDATOR_PATH)
    if spec is None or spec.loader is None:
        raise CorpusFailure("cli_unavailable: validator module could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def run_cli(cli: Any, args: list[str]) -> tuple[int, dict[str, Any], str, str]:
    old_argv = sys.argv
    stdout = io.StringIO()
    stderr = io.StringIO()
    try:
        sys.argv = [str(VALIDATOR_PATH), *args]
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            try:
                exit_code = int(cli.main())
            except SystemExit as exc:
                exit_code = int(exc.code or 0)
    finally:
        sys.argv = old_argv
    stdout_text = stdout.getvalue()
    stderr_text = stderr.getvalue()
    if len(stdout_text) > MAX_CAPTURE_CHARS or len(stderr_text) > MAX_CAPTURE_CHARS:
        raise CorpusFailure("diagnostic_oversized: CLI output exceeded the corpus bound")
    try:
        report = json.loads(stdout_text)
    except json.JSONDecodeError as exc:
        raise CorpusFailure(f"diagnostic_invalid_json: {exc}") from exc
    return exit_code, report, stdout_text, stderr_text


def assert_redacted(stdout: str, stderr: str, temp_root: Path) -> None:
    combined = stdout + stderr
    forbidden = (
        "Traceback (most recent call last)",
        str(Path.home()),
        str(temp_root),
        "sk-abcdefghijklmnop",
    )
    leaked = [token for token in forbidden if token and token in combined]
    if leaked:
        raise CorpusFailure(f"diagnostic_leak: {leaked}")


def assert_case(case: Mapping[str, Any], exit_code: int, report: Mapping[str, Any]) -> None:
    expected_exit = int(case.get("expected_exit", 1))
    if exit_code != expected_exit:
        raise CorpusFailure(
            f"{case['case_id']}: expected exit {expected_exit}, got {exit_code}"
        )
    if report.get("status") != case.get("expected_status"):
        raise CorpusFailure(
            f"{case['case_id']}: expected status {case.get('expected_status')}, "
            f"got {report.get('status')}"
        )
    matches = [
        error
        for error in report.get("errors", [])
        if error.get("code") == case.get("expected_code")
        and error.get("subject_id", "") == case.get("subject_id", "")
        and error.get("freshness") == case.get("expected_freshness")
    ]
    if not matches:
        raise CorpusFailure(
            f"{case['case_id']}: exact diagnostic missing; errors={report.get('errors')}"
        )


def drift_scenarios() -> dict[str, Mapping[str, Any]]:
    scenarios: dict[str, Mapping[str, Any]] = {}
    for name in (
        "repository-drift.json",
        "official-source-drift.json",
        "target-revision-drift.json",
    ):
        document = load_json(INTEGRITY_ROOT.parent / "drift" / name)
        for case in document["cases"]:
            case_id = case["case_id"]
            if case_id in scenarios:
                raise CorpusFailure(f"duplicate_scenario: {case_id}")
            scenarios[case_id] = case
    return scenarios


def run_drift_case(
    scenario: Mapping[str, Any],
    temp_root: Path,
) -> tuple[int, dict[str, Any], str, str]:
    live_reference_root = temp_root / "reference"
    target_root = temp_root / "target"
    target_root.mkdir(exist_ok=True)
    artifact = {
        "artifact_id": "official-source-fixture",
        "content_sha256": "a" * 64,
    }
    scenario_kind = scenario["scenario"]
    if scenario_kind == "repository":
        observed_fingerprint = {
            "git_head": "1" * 40,
            "tree_sha256": "2" * 64,
            "license_sha256": "3" * 64,
        }
        revision_kind = scenario["revision_kind"]
        frozen = {
            "repo_id": scenario["subject_id"],
            "path": "reference/fixture-repository",
            "revision_kind": revision_kind,
            "revision_value": (
                observed_fingerprint["git_head"]
                if revision_kind == "git_commit"
                else observed_fingerprint["tree_sha256"]
            ),
            "tree_sha256": observed_fingerprint["tree_sha256"],
            "license_sha256": observed_fingerprint["license_sha256"],
        }
        frozen.update(scenario.get("frozen_override", {}))
        errors = governance.compare_repository_fingerprint(frozen, observed_fingerprint)
    elif scenario_kind == "source_unavailable":
        frozen = {
            "repo_id": scenario["subject_id"],
            "path": "reference/missing-repository",
            "revision_kind": "git_commit",
            "revision_value": "0" * 40,
        }
        errors = governance.compare_repository_fingerprint(
            frozen,
            None,
            observation_error=governance.GovernanceUsageError("read_failed: unavailable"),
        )
    elif scenario_kind == "official_source":
        bundle = {
            "official_source_artifact": dict(scenario["canonical"]),
            "repository_registry_revisions": [],
            "capability_decision_revisions": [],
        }
        observed_artifact = scenario["observed"]
        errors = governance.detect_drift(
            bundle,
            live_reference_root=live_reference_root,
            target_root=target_root,
            official_source_artifact=observed_artifact,
        )
    elif scenario_kind == "target_artifact":
        target_path = target_root / str(scenario["target_path"])
        target_path.write_bytes(str(scenario["target_bytes"]).encode("utf-8"))
        bundle = {
            "official_source_artifact": artifact,
            "repository_registry_revisions": [],
            "capability_decision_revisions": [
                {
                    "decisions": [
                        {
                            "decision_id": scenario["subject_id"],
                            "freshness": "current",
                            "target_path": scenario["target_path"],
                            "target_revision_kind": "artifact_sha256",
                            "target_revision_value": scenario["frozen_revision"],
                        }
                    ]
                }
            ],
        }
        observed_artifact = artifact
        errors = governance.detect_drift(
            bundle,
            live_reference_root=live_reference_root,
            target_root=target_root,
            official_source_artifact=observed_artifact,
        )
    else:
        raise CorpusFailure(f"unknown_drift_scenario: {scenario_kind}")
    report = governance.validation_report("stale" if errors else "current", errors)
    return (1 if errors else 0), report, json.dumps(report, sort_keys=True), ""


def run_symlink_case(cli: Any, temp_root: Path) -> tuple[int, dict[str, Any], str, str]:
    link_root = temp_root / "symlink-root"
    outside_root = temp_root / "symlink-outside"
    link_root.mkdir()
    outside_root.mkdir()
    outside = outside_root / "outside.json"
    outside.write_text("{}", encoding="utf-8")
    (link_root / "escape.json").symlink_to(outside)
    try:
        governance.resolve_repository_path(link_root, "escape.json")
    except governance.GovernanceUsageError as exc:
        report = cli._usage_report(exc)
        return 2, report, json.dumps(report, sort_keys=True), ""
    raise CorpusFailure("security.symlink-escape: production resolver accepted escape")


def run_oversized_case(
    cli: Any,
    temp_root: Path,
) -> tuple[int, dict[str, Any], str, str]:
    fixture = temp_root / "oversized.json"
    fixture.write_bytes(b" " * (governance.MAX_JSON_BYTES + 1))
    return run_cli(
        cli,
        ["validate", "--fixture-bundle", str(fixture), "--json"],
    )


def run() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--temp-root", type=Path)
    parser.add_argument("--case", action="append", dest="selected_cases")
    args = parser.parse_args()
    cli = load_cli_module()
    integrity_manifest = load_json(INTEGRITY_ROOT / "expected-errors.json")
    coverage_manifest = load_json(COVERAGE_ROOT / "expected-errors.json")
    cases = [*coverage_manifest["cases"], *integrity_manifest["cases"]]
    case_ids = [case["case_id"] for case in cases]
    if len(case_ids) != len(set(case_ids)):
        raise CorpusFailure("duplicate_case_id: corpus case IDs must be unique")
    if args.selected_cases:
        unknown = sorted(set(args.selected_cases) - set(case_ids))
        if unknown:
            raise CorpusFailure(f"unknown_case_id: {','.join(unknown)}")
        selected = set(args.selected_cases)
        cases = [case for case in cases if case["case_id"] in selected]
    before = protected_hashes()
    scenarios = drift_scenarios()
    with tempfile.TemporaryDirectory(
        dir=args.temp_root.resolve() if args.temp_root else None
    ) as directory:
        temp_root = Path(directory)
        for case in cases:
            execution = case.get("execution", "cli")
            if execution == "cli":
                result = run_cli(
                    cli,
                    [case.get("command", "validate"), *case["args"], "--json"],
                )
            elif execution == "production_drift_module":
                scenario_id = case["scenario_id"]
                if scenario_id not in scenarios:
                    raise CorpusFailure(f"missing_scenario: {scenario_id}")
                result = run_drift_case(
                    scenarios[scenario_id],
                    temp_root,
                )
            elif execution == "temporary_symlink":
                result = run_symlink_case(cli, temp_root)
            elif execution == "temporary_oversized":
                result = run_oversized_case(cli, temp_root)
            else:
                raise CorpusFailure(f"unknown_execution: {execution}")
            exit_code, report, stdout, stderr = result
            assert_redacted(stdout, stderr, temp_root)
            assert_case(case, exit_code, report)
            preserved_id = case.get("preserves_evidence_id")
            if preserved_id:
                bundle = governance.load_fixture_bundle(ROOT / case["args"][1])
                record_ids = {
                    record["evidence_id"]
                    for record in bundle["evidence_revisions"][-1]["records"]
                }
                if preserved_id not in record_ids:
                    raise CorpusFailure(
                        f"{case['case_id']}: historical evidence {preserved_id} was removed"
                    )
        positive_fixtures = (
            ()
            if args.selected_cases
            else (
                VALID_ROOT / "minimal-graph.json",
                VALID_ROOT / "full-38-repositories.json",
                VALID_ROOT / "offline-source-identity.json",
                VALID_ROOT / "hostile-rendering.json",
            )
        )
        for fixture in positive_fixtures:
            exit_code, report, stdout, stderr = run_cli(
                cli,
                ["validate", "--fixture-bundle", str(fixture), "--json"],
            )
            assert_redacted(stdout, stderr, temp_root)
            if exit_code != 0 or report.get("status") != "valid" or report.get("errors") != []:
                raise CorpusFailure(f"positive_fixture_failed: {fixture.name}: {report}")
    after = protected_hashes()
    if before != after:
        raise CorpusFailure("protected_input_modified: corpus execution changed protected bytes")
    print(
        f"OK: capability governance corpus cases={len(cases)} "
        f"valid_fixtures={len(positive_fixtures)} protected_inputs=12"
    )


if __name__ == "__main__":
    try:
        run()
    except (CorpusFailure, OSError, subprocess.SubprocessError) as exc:
        raise SystemExit(str(exc)) from exc
