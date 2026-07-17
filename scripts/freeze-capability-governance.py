#!/usr/bin/env python3
"""Freeze controlled governance inputs, check drift, and create refresh successors."""

from __future__ import annotations

import argparse
import base64
import copy
import json
import os
import re
import subprocess
import sys
import tempfile
from datetime import datetime
from pathlib import Path, PurePosixPath
from typing import Any, Mapping
from urllib.parse import urlsplit


SCRIPT_DIR = Path(__file__).resolve().parent
REPOSITORY_ROOT = SCRIPT_DIR.parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from capability_governance import (
    GovernanceError,
    GovernanceUsageError,
    canonical_sha256,
    detect_drift,
    file_sha256,
    load_fixture_bundle,
    load_json,
    load_manifest_bundle,
    repository_fingerprint,
    record_sha256,
    resolve_repository_path,
    sorted_errors,
    validate_governance,
    validate_revision_ancestry,
)
from capability_governance import _expected_evidence_bindings


FREEZE_SCHEMA = "kiana.capability-governance-freeze.v1"
DRIFT_SCHEMA = "kiana.capability-governance-drift-report.v1"
REQUEST_SCHEMA = "kiana.capability-governance-refresh-request.v1"
DRIFT_CASES = (
    "no_drift",
    "repository_head_drift",
    "repository_tree_drift",
    "license_hash_drift",
    "content_tree_drift",
    "official_source_hash_drift",
    "target_revision_drift",
    "source_unavailable",
)


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)

    freeze = commands.add_parser("freeze", help="freeze a controlled offline artifact")
    freeze.add_argument(
        "--kind",
        required=True,
        choices=("official-source", "repository-registry", "target-revision"),
    )
    freeze.add_argument("--input", type=Path, required=True)
    freeze.add_argument("--output", type=Path, required=True)
    freeze.add_argument("--artifact-id", required=True)
    freeze.add_argument("--retrieved-at", required=True)
    freeze.add_argument("--archive-uri", required=True)

    drift = commands.add_parser("check-drift", help="compare frozen and observed identities")
    drift.add_argument("--manifest", type=Path)
    drift.add_argument("--live-reference-root", type=Path)
    drift.add_argument("--target-root", type=Path)
    drift.add_argument("--official-source-artifact", type=Path)
    drift.add_argument("--fixture-bundle", type=Path)
    drift.add_argument("--case", choices=DRIFT_CASES)
    drift.add_argument("--output", type=Path, required=True)

    refresh = commands.add_parser("refresh", help="create immutable governance successors")
    refresh_source = refresh.add_mutually_exclusive_group(required=True)
    refresh_source.add_argument("--manifest", type=Path)
    refresh_source.add_argument("--fixture-bundle", type=Path)
    refresh.add_argument("--drift-report", type=Path, required=True)
    refresh.add_argument("--output-root", type=Path, required=True)
    refresh.add_argument("--revision-id", required=True)
    refresh.add_argument("--review-revision", required=True)
    return parser


def _require_rfc3339(value: str) -> str:
    try:
        datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError as exc:
        raise GovernanceUsageError("retrieved_at_invalid: expected RFC3339 UTC time") from exc
    return value


def _require_uri(value: Any, label: str) -> str:
    if not isinstance(value, str):
        raise GovernanceUsageError(f"{label}_required: absolute HTTP(S) URI is required")
    parsed = urlsplit(value)
    if parsed.scheme not in {"http", "https"} or not parsed.netloc:
        raise GovernanceUsageError(f"{label}_invalid: absolute HTTP(S) URI is required")
    return value


def _select_freeze_artifact(kind: str, value: Any) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise GovernanceUsageError("artifact_invalid: input must be a JSON object")
    if kind == "official-source":
        selected = value.get("official_source_artifact", value)
        expected_schema = "kiana.official-source-artifact.v1"
    elif kind == "repository-registry":
        revisions = value.get("repository_registry_revisions")
        selected = revisions[-1] if isinstance(revisions, list) and revisions else value
        expected_schema = "kiana.reference-repository-registry.v1"
    else:
        selected = value.get("target_revision", value)
        expected_schema = None
    if not isinstance(selected, Mapping):
        raise GovernanceUsageError(f"artifact_invalid: no {kind} object found")
    if expected_schema is not None and selected.get("schema") != expected_schema:
        raise GovernanceUsageError(
            f"artifact_kind_mismatch: expected schema {expected_schema}"
        )
    return selected


def _atomic_create_json(output: Path, value: Mapping[str, Any]) -> None:
    parent = output.parent.resolve(strict=True)
    if output.name in {"", ".", ".."}:
        raise GovernanceUsageError("output_invalid: output filename is required")
    target = parent / output.name
    if target.exists() or target.is_symlink():
        raise GovernanceUsageError("output_exists: refusing to overwrite existing output")
    payload = json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True).encode("utf-8") + b"\n"
    descriptor, temporary_name = tempfile.mkstemp(prefix=f".{output.name}.", dir=parent)
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
        try:
            os.link(temporary, target, follow_symlinks=False)
        except FileExistsError as exc:
            raise GovernanceUsageError(
                "output_exists: refusing to overwrite existing output"
            ) from exc
        directory_fd = os.open(parent, os.O_RDONLY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    finally:
        temporary.unlink(missing_ok=True)


def _freeze(args: argparse.Namespace) -> int:
    source = load_json(args.input.resolve())
    artifact = dict(_select_freeze_artifact(args.kind, source))
    archive_uri = _require_uri(args.archive_uri, "archive_uri")
    official_uri = _require_uri(artifact.get("official_uri"), "official_uri")
    if args.kind == "official-source":
        _require_uri(artifact.get("archive_uri"), "source_archive_uri")
        _require_rfc3339(str(artifact.get("retrieved_at", "")))
    report = {
        "schema": FREEZE_SCHEMA,
        "version": "1.0",
        "kind": args.kind,
        "artifact_id": args.artifact_id,
        "retrieved_at": _require_rfc3339(args.retrieved_at),
        "official_uri": official_uri,
        "archive_uri": archive_uri,
        "normalization": {
            "method": "canonical-json-rfc8785-compatible",
            "version": "1",
            "encoding": "utf-8",
        },
        "content_sha256": canonical_sha256(artifact),
        "artifact": artifact,
        "retrieval_metadata": {
            "controlled_offline_input": True,
            "source_archive_uri": artifact.get("archive_uri"),
            "source_retrieved_at": artifact.get("retrieved_at"),
        },
    }
    _atomic_create_json(args.output, report)
    return 0


def _repository_binding(repository: Mapping[str, Any]) -> dict[str, Any]:
    return {
        key: repository[key]
        for key in (
            "path",
            "revision_kind",
            "revision_value",
            "tree_sha256",
            "license_sha256",
        )
        if key in repository
    }


def _frozen_fingerprints(bundle: Mapping[str, Any]) -> dict[str, Any]:
    artifact = bundle.get("official_source_artifact") or {}
    repositories = (bundle.get("repository_registry_revisions") or [{}])[-1].get(
        "repositories", []
    )
    decisions = (bundle.get("capability_decision_revisions") or [{}])[-1].get(
        "decisions", []
    )
    return {
        "official_source": {
            "artifact_id": artifact.get("artifact_id"),
            "content_sha256": artifact.get("content_sha256"),
            "canonical_sha256": canonical_sha256(artifact),
        },
        "repositories": {
            str(repository.get("repo_id", "")): _repository_binding(repository)
            for repository in repositories
        },
        "targets": {
            str(decision.get("decision_id", "")): {
                "path": decision.get("target_path"),
                "revision_kind": decision.get("target_revision_kind"),
                "revision_value": decision.get("target_revision_value"),
            }
            for decision in decisions
            if decision.get("freshness") == "current"
        },
    }


def _observe_fingerprints(
    bundle: Mapping[str, Any],
    *,
    live_reference_root: Path,
    target_root: Path,
    official_source_artifact: Mapping[str, Any],
) -> dict[str, Any]:
    repositories: dict[str, Any] = {}
    rows = (bundle.get("repository_registry_revisions") or [{}])[-1].get(
        "repositories", []
    )
    for repository in rows:
        repo_id = str(repository.get("repo_id", ""))
        try:
            relative = PurePosixPath(str(repository.get("path", ""))).relative_to(
                "reference"
            )
            live_path = live_reference_root.joinpath(*relative.parts).resolve()
            live_path.relative_to(live_reference_root.resolve())
            repositories[repo_id] = repository_fingerprint(live_path)
        except (ValueError, OSError, GovernanceUsageError):
            repositories[repo_id] = {"availability": "unavailable"}
    targets: dict[str, Any] = {}
    decisions = (bundle.get("capability_decision_revisions") or [{}])[-1].get(
        "decisions", []
    )
    for decision in decisions:
        if decision.get("freshness") != "current":
            continue
        decision_id = str(decision.get("decision_id", ""))
        try:
            target = resolve_repository_path(target_root, decision.get("target_path", ""))
            if decision.get("target_revision_kind") == "artifact_sha256":
                value = file_sha256(target)
            else:
                git_root = target if target.is_dir() else target.parent
                result = subprocess.run(
                    ["git", "-C", str(git_root), "rev-parse", "HEAD"],
                    check=False,
                    capture_output=True,
                    text=True,
                )
                value = result.stdout.strip() if result.returncode == 0 else ""
            targets[decision_id] = {
                "revision_kind": decision.get("target_revision_kind"),
                "revision_value": value,
            }
        except GovernanceUsageError:
            targets[decision_id] = {"availability": "unavailable"}
    return {
        "official_source": {
            "artifact_id": official_source_artifact.get("artifact_id"),
            "content_sha256": official_source_artifact.get("content_sha256"),
            "canonical_sha256": canonical_sha256(official_source_artifact),
        },
        "repositories": repositories,
        "targets": targets,
    }


def _drift_report(
    *,
    case: str,
    bundle: Mapping[str, Any],
    observed: Mapping[str, Any],
    errors: list[GovernanceError],
) -> dict[str, Any]:
    errors = sorted_errors(errors)
    codes = {error.code for error in errors}
    unavailable = "source_unavailable" in codes
    status = "unavailable" if unavailable else "stale" if errors else "current"
    freshness = "unavailable" if unavailable else "stale" if errors else "current"
    events = [
        {
            "event_type": (
                "blocked_observation" if error.code == "source_unavailable" else "stale"
            ),
            "code": error.code,
            "subject_id": error.subject_id,
            "freshness": freshness,
        }
        for error in errors
    ]
    return {
        "schema": DRIFT_SCHEMA,
        "version": "1.0",
        "case": case,
        "status": status,
        "current": not errors,
        "freshness": freshness,
        "errors": [error.as_dict() for error in errors],
        "affected_subjects": sorted(
            {error.subject_id for error in errors if error.subject_id}
        ),
        "events": events,
        "frozen_fingerprints": _frozen_fingerprints(bundle),
        "observed_fingerprints": dict(observed),
    }


def _materialize_fixture_world(
    request: Mapping[str, Any], root: Path, *, require_git_head: bool
) -> tuple[dict[str, Any], Path, Path, dict[str, Any], dict[str, Any]]:
    fixture = request.get("fixture")
    if not isinstance(fixture, Mapping):
        raise GovernanceUsageError("fixture_invalid: fixture object is required")
    reference_root = root / "reference"
    target_root = root / "target"
    reference_root.mkdir()
    target_root.mkdir()

    repositories: dict[str, dict[str, Any]] = {}
    for name in ("git_repository", "content_repository"):
        spec = fixture.get(name)
        if not isinstance(spec, Mapping):
            raise GovernanceUsageError(f"fixture_invalid: {name} is required")
        relative = PurePosixPath(str(spec.get("path", ""))).relative_to("reference")
        path = reference_root.joinpath(*relative.parts)
        path.mkdir(parents=True)
        (path / "payload.txt").write_text(str(spec.get("payload", "")), encoding="utf-8")
        (path / "LICENSE").write_text(str(spec.get("license", "")), encoding="utf-8")
        if name == "git_repository" and require_git_head:
            (path / ".git").mkdir()
            (path / ".git" / "HEAD").write_text("1" * 40 + "\n", encoding="ascii")
        fingerprint = repository_fingerprint(path)
        revision_kind = (
            "git_commit"
            if name == "git_repository" and require_git_head
            else "content_tree_sha256"
            if name == "content_repository"
            else "controlled_snapshot"
        )
        repositories[name] = {
            "repo_id": spec.get("repo_id"),
            "path": spec.get("path"),
            "revision_kind": revision_kind,
            "revision_value": (
                fingerprint["git_head"]
                if revision_kind == "git_commit"
                else fingerprint["tree_sha256"]
            ),
            "tree_sha256": fingerprint["tree_sha256"],
            "license_sha256": fingerprint["license_sha256"],
        }

    target = fixture.get("target")
    if not isinstance(target, Mapping):
        raise GovernanceUsageError("fixture_invalid: target is required")
    target_path = target_root / str(target.get("path"))
    target_path.write_bytes(base64.b64decode(str(target.get("content_base64")), validate=True))
    artifact = fixture.get("official_source_artifact")
    if not isinstance(artifact, Mapping):
        raise GovernanceUsageError("fixture_invalid: official source artifact is required")
    bundle = {
        "official_source_artifact": dict(artifact),
        "repository_registry_revisions": [
            {"repositories": list(repositories.values())}
        ],
        "capability_decision_revisions": [
            {
                "decisions": [
                    {
                        "decision_id": target.get("decision_id"),
                        "freshness": "current",
                        "target_path": target.get("path"),
                        "target_revision_kind": "artifact_sha256",
                        "target_revision_value": file_sha256(target_path),
                    }
                ]
            }
        ],
    }
    observed = _observe_fingerprints(
        bundle,
        live_reference_root=reference_root,
        target_root=target_root,
        official_source_artifact=artifact,
    )
    return bundle, reference_root, target_root, dict(artifact), observed


def _select_fixture_case(
    request: Mapping[str, Any], case: str, root: Path
) -> tuple[dict[str, Any], Path, Path, dict[str, Any], dict[str, Any]]:
    listed = request.get("cases")
    if request.get("schema") != REQUEST_SCHEMA or listed != list(DRIFT_CASES):
        raise GovernanceUsageError("fixture_invalid: exact drift case set is required")
    bundle, reference_root, target_root, artifact, observed = _materialize_fixture_world(
        request,
        root,
        require_git_head=case in {"no_drift", "repository_head_drift"},
    )
    git_repository, content_repository = bundle["repository_registry_revisions"][0][
        "repositories"
    ]
    target = bundle["capability_decision_revisions"][0]["decisions"][0]
    if case == "no_drift":
        pass
    elif case == "repository_head_drift":
        bundle["repository_registry_revisions"][0]["repositories"] = [git_repository]
        git_repository["revision_value"] = "0" * 40
        bundle["capability_decision_revisions"][0]["decisions"] = []
    elif case == "repository_tree_drift":
        bundle["repository_registry_revisions"][0]["repositories"] = [git_repository]
        git_repository["tree_sha256"] = "0" * 64
        bundle["capability_decision_revisions"][0]["decisions"] = []
    elif case == "license_hash_drift":
        bundle["repository_registry_revisions"][0]["repositories"] = [git_repository]
        git_repository["license_sha256"] = "0" * 64
        bundle["capability_decision_revisions"][0]["decisions"] = []
    elif case == "content_tree_drift":
        bundle["repository_registry_revisions"][0]["repositories"] = [content_repository]
        content_repository["tree_sha256"] = "0" * 64
        bundle["capability_decision_revisions"][0]["decisions"] = []
    elif case == "official_source_hash_drift":
        bundle["repository_registry_revisions"][0]["repositories"] = []
        bundle["capability_decision_revisions"][0]["decisions"] = []
        artifact["content_sha256"] = "f" * 64
    elif case == "target_revision_drift":
        bundle["repository_registry_revisions"][0]["repositories"] = []
        target["target_revision_value"] = "0" * 64
    elif case == "source_unavailable":
        bundle["repository_registry_revisions"][0]["repositories"] = [
            {
                **git_repository,
                "repo_id": "repo-missing",
                "path": "reference/missing",
            }
        ]
        bundle["capability_decision_revisions"][0]["decisions"] = []
    observed = _observe_fingerprints(
        bundle,
        live_reference_root=reference_root,
        target_root=target_root,
        official_source_artifact=artifact,
    )
    return bundle, reference_root, target_root, artifact, observed


def _load_official_artifact(path: Path) -> dict[str, Any]:
    value = load_json(path.resolve())
    if not isinstance(value, Mapping):
        raise GovernanceUsageError("official_source_invalid: JSON object is required")
    selected = value.get("artifact", value.get("official_source_artifact", value))
    if not isinstance(selected, Mapping):
        raise GovernanceUsageError("official_source_invalid: artifact object is required")
    return dict(selected)


def _check_drift(args: argparse.Namespace) -> int:
    production_args = {
        "--manifest": args.manifest,
        "--live-reference-root": args.live_reference_root,
        "--target-root": args.target_root,
        "--official-source-artifact": args.official_source_artifact,
    }
    if args.fixture_bundle is not None:
        mixed = [name for name, value in production_args.items() if value is not None]
        if mixed:
            raise GovernanceUsageError(
                "mode_invalid: --fixture-bundle cannot be combined with production inputs"
            )
        if args.case is None:
            raise GovernanceUsageError("case_required: --case is required in fixture mode")
        request = load_json(args.fixture_bundle.resolve())
        if not isinstance(request, Mapping):
            raise GovernanceUsageError("fixture_invalid: root must be an object")
        with tempfile.TemporaryDirectory(prefix="kiana-governance-drift-") as directory:
            bundle, reference_root, target_root, artifact, observed = _select_fixture_case(
                request, args.case, Path(directory)
            )
            errors = detect_drift(
                bundle,
                live_reference_root=reference_root,
                target_root=target_root,
                official_source_artifact=artifact,
            )
    else:
        missing = [name for name, value in production_args.items() if value is None]
        if missing:
            raise GovernanceUsageError(
                "production_inputs_required: production check-drift requires "
                + ", ".join(missing)
            )
        if args.case is not None:
            raise GovernanceUsageError("mode_invalid: --case is test-only")
        bundle = load_manifest_bundle(args.manifest.resolve(), root=REPOSITORY_ROOT)
        semantic_errors = validate_governance(bundle, root=REPOSITORY_ROOT)
        if semantic_errors:
            observed = {}
            report = _drift_report(
                case="production",
                bundle=bundle,
                observed=observed,
                errors=semantic_errors,
            )
            report["status"] = "invalid"
            report["freshness"] = "invalid"
            _atomic_create_json(args.output, report)
            return 1
        reference_root = args.live_reference_root.resolve()
        target_root = args.target_root.resolve()
        artifact = _load_official_artifact(args.official_source_artifact)
        observed = _observe_fingerprints(
            bundle,
            live_reference_root=reference_root,
            target_root=target_root,
            official_source_artifact=artifact,
        )
        errors = detect_drift(
            bundle,
            live_reference_root=reference_root,
            target_root=target_root,
            official_source_artifact=artifact,
        )
        args.case = "production"
    report = _drift_report(
        case=args.case,
        bundle=bundle,
        observed=observed,
        errors=errors,
    )
    _atomic_create_json(args.output, report)
    return 0 if not errors else 1


def _require_stable_id(value: str, label: str) -> str:
    if not re.fullmatch(r"[a-z0-9][a-z0-9._-]*", value):
        raise GovernanceUsageError(f"{label}_invalid: stable lowercase ID is required")
    return value


def _successor_revision(
    previous: Mapping[str, Any],
    *,
    revision_id: str,
    previous_path: str,
    timestamp: str,
) -> dict[str, Any]:
    successor = copy.deepcopy(dict(previous))
    successor.pop("genesis_reason", None)
    successor.update(
        {
            "revision_id": revision_id,
            "revision_kind": "successor",
            "previous_revision_id": previous.get("revision_id"),
            "previous_revision_path": previous_path,
            "previous_revision_sha256": canonical_sha256(previous),
        }
    )
    for time_key in ("created_at", "frozen_at", "reviewed_at"):
        if time_key in successor:
            successor[time_key] = timestamp
    return successor


def _collect_evidence_ids(
    value: Any,
    *,
    ignored_keys: frozenset[str] = frozenset(),
) -> set[str]:
    result: set[str] = set()
    if isinstance(value, Mapping):
        for key, child in value.items():
            if key in ignored_keys:
                continue
            if key.endswith("evidence_ids") and key != "supersedes_evidence_ids":
                if isinstance(child, list):
                    result.update(item for item in child if isinstance(item, str))
            else:
                result.update(_collect_evidence_ids(child, ignored_keys=ignored_keys))
    elif isinstance(value, list):
        for child in value:
            result.update(_collect_evidence_ids(child, ignored_keys=ignored_keys))
    return result


def _replace_evidence_ids(
    value: Any,
    replacements: Mapping[str, str],
    *,
    ignored_keys: frozenset[str] = frozenset(),
) -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            if key in ignored_keys:
                continue
            if key.endswith("evidence_ids") and key != "supersedes_evidence_ids":
                if isinstance(child, list):
                    value[key] = [replacements.get(item, item) for item in child]
            else:
                _replace_evidence_ids(child, replacements, ignored_keys=ignored_keys)
    elif isinstance(value, list):
        for child in value:
            _replace_evidence_ids(child, replacements, ignored_keys=ignored_keys)


def _append_record(records: list[dict[str, Any]], record: dict[str, Any]) -> None:
    record["sequence"] = len(records) + 1
    record["previous_record_sha256"] = (
        records[-1]["record_sha256"] if records else "genesis"
    )
    record["record_sha256"] = record_sha256(record)
    records.append(record)


def _base_evidence_record(
    *,
    evidence_id: str,
    subject_family: str,
    subject_id: str,
    subject_revision_id: str,
    source_revision: str,
    target_revision: str,
    environment_kind: str,
    timestamp: str,
    drift_sha256: str,
    drift_size: int,
    freshness: str,
    result: str,
    coverage_state: str,
    proof_level: str,
    supersedes: list[str],
    event_type: str,
    reason: str,
) -> dict[str, Any]:
    return {
        "sequence": 0,
        "evidence_id": evidence_id,
        "subject_family": subject_family,
        "subject_id": subject_id,
        "subject_revision_id": subject_revision_id,
        "evidence_type": (
            "supersession_event" if event_type == "supersession" else "freshness_event"
        ),
        "result": result,
        "coverage_state": coverage_state,
        "proof_level": proof_level,
        "freshness": freshness,
        "source_binding": {
            "binding_id": f"source.{evidence_id}",
            "revision_kind": "external_revision",
            "revision_value": source_revision,
            "path": "drift-report.json",
            "sha256": canonical_sha256(source_revision),
        },
        "target_binding": {
            "binding_id": f"target.{evidence_id}",
            "revision_kind": "revision_id",
            "revision_value": target_revision,
            "path": "current.json",
            "sha256": canonical_sha256(target_revision),
        },
        "environment": {
            "environment_id": "capability-governance-refresh",
            "environment_kind": environment_kind,
            "platform": "offline",
            "runtime": "python-standard-library",
            "fingerprint_sha256": canonical_sha256(
                {
                    "environment_id": "capability-governance-refresh",
                    "offline": True,
                }
            ),
        },
        "method": {
            "method_kind": "artifact_inspection",
            "name": "immutable capability governance refresh",
            "command": "python3 scripts/freeze-capability-governance.py refresh",
        },
        "observed_at": timestamp,
        "artifact": {
            "artifact_kind": "repository_path",
            "path": "drift-report.json",
            "sha256": drift_sha256,
            "media_type": "application/json",
            "byte_size": drift_size,
        },
        "supersedes_evidence_ids": supersedes,
        "transition_event": {
            "event_type": event_type,
            "reason": reason,
            "related_evidence_ids": supersedes,
        },
        "previous_record_sha256": "genesis",
        "record_sha256": "0" * 64,
    }


def _load_refresh_bundle(args: argparse.Namespace) -> tuple[dict[str, Any], Path]:
    if args.fixture_bundle is not None:
        source_path = args.fixture_bundle.resolve()
        bundle = load_fixture_bundle(source_path, root=REPOSITORY_ROOT)
    else:
        source_path = args.manifest.resolve()
        bundle = load_manifest_bundle(source_path, root=REPOSITORY_ROOT)
    errors = validate_governance(bundle, root=REPOSITORY_ROOT)
    if errors:
        codes = ",".join(error.code for error in errors[:5])
        raise GovernanceUsageError(f"predecessor_invalid: {codes}")
    return bundle, source_path


def _validate_refresh_report(
    report: Any,
    bundle: Mapping[str, Any],
) -> tuple[list[Mapping[str, Any]], dict[str, set[str]]]:
    if not isinstance(report, Mapping):
        raise GovernanceUsageError("drift_report_invalid: root must be an object")
    errors = report.get("errors")
    if report.get("current") is not False or report.get("status") == "current" or not errors:
        raise GovernanceUsageError("drift_required: refresh requires a non-current drift report")
    if not isinstance(errors, list) or any(not isinstance(error, Mapping) for error in errors):
        raise GovernanceUsageError("drift_report_invalid: errors must be an object array")
    allowed_codes = set(DRIFT_CASES) - {"no_drift"}
    if any(error.get("code") not in allowed_codes for error in errors):
        raise GovernanceUsageError("drift_report_invalid: unsupported drift code")
    by_subject: dict[str, set[str]] = {}
    for error in errors:
        subject_id = error.get("subject_id")
        if not isinstance(subject_id, str) or not subject_id:
            raise GovernanceUsageError("drift_report_invalid: subject_id is required")
        by_subject.setdefault(subject_id, set()).add(str(error.get("code")))
    affected = report.get("affected_subjects")
    if not isinstance(affected, list) or set(affected) != set(by_subject):
        raise GovernanceUsageError(
            "affected_subject_inconsistent: affected subjects must match drift errors"
        )
    known = {str((bundle.get("official_source_artifact") or {}).get("artifact_id", ""))}
    registry = (bundle.get("repository_registry_revisions") or [{}])[-1]
    known.update(
        str(repository.get("repo_id", ""))
        for repository in registry.get("repositories", [])
    )
    decisions = (bundle.get("capability_decision_revisions") or [{}])[-1]
    known.update(
        str(decision.get("decision_id", ""))
        for decision in decisions.get("decisions", [])
    )
    unknown = sorted(set(by_subject) - known)
    if unknown:
        raise GovernanceUsageError(
            f"affected_subject_unknown: {','.join(unknown)}"
        )
    return list(errors), by_subject


def _predecessor_binding_path(
    bundle: Mapping[str, Any], binding_key: str
) -> str:
    binding = (bundle.get("bundle_manifest") or {}).get(binding_key) or {}
    path = binding.get("path")
    if not isinstance(path, str):
        raise GovernanceUsageError(
            f"predecessor_missing: {binding_key} path is required"
        )
    return path


def _build_refresh_successor(
    bundle: Mapping[str, Any],
    *,
    errors: list[Mapping[str, Any]],
    by_subject: Mapping[str, set[str]],
    revision_id: str,
    review_revision: str,
    drift_sha256: str,
    drift_size: int,
) -> tuple[dict[str, Any], list[str]]:
    successor = copy.deepcopy(dict(bundle))
    timestamp = str((bundle.get("bundle_manifest") or {}).get("evaluation_time", ""))
    _require_rfc3339(timestamp)
    artifact_id = str((bundle.get("official_source_artifact") or {}).get("artifact_id", ""))
    official_codes = by_subject.get(artifact_id, set())

    previous_source = bundle["official_source_artifact"]
    registry_previous = bundle["repository_registry_revisions"][-1]
    repository_ids = {
        str(repository.get("repo_id", ""))
        for repository in registry_previous.get("repositories", [])
    }
    decisions_previous = bundle["capability_decision_revisions"][-1]
    decision_ids = {
        str(decision.get("decision_id", ""))
        for decision in decisions_previous.get("decisions", [])
    }
    touched_repositories = repository_ids.intersection(by_subject)
    touched_decisions = decision_ids.intersection(by_subject)
    touch_source = bool(official_codes)
    touch_registry = bool(touched_repositories)
    touch_decisions = bool(touched_decisions or touched_repositories)

    if touch_source:
        new_source = _successor_revision(
            previous_source,
            revision_id=f"{revision_id}.official-source",
            previous_path=_predecessor_binding_path(bundle, "official_source_artifact"),
            timestamp=timestamp,
        )
        successor["official_source_artifact"] = new_source
    else:
        new_source = successor["official_source_artifact"]

    family_specs = (
        ("public_baseline_revisions", "public_baseline", "public-baseline", touch_source),
        ("repository_registry_revisions", "repository_registry", "repository-registry", touch_registry),
        ("capability_decision_revisions", "capability_decisions", "capability-decisions", touch_decisions),
        ("evidence_revisions", "evidence_head", "evidence", True),
    )
    new_heads: dict[str, dict[str, Any]] = {}
    for family, binding_key, suffix, touched in family_specs:
        if not touched:
            continue
        revisions = successor.get(family)
        if not isinstance(revisions, list) or not revisions:
            raise GovernanceUsageError(f"predecessor_missing: {family}")
        previous = revisions[-1]
        head = _successor_revision(
            previous,
            revision_id=f"{revision_id}.{suffix}",
            previous_path=_predecessor_binding_path(bundle, binding_key),
            timestamp=timestamp,
        )
        new_heads[family] = head

    public_head = new_heads.get("public_baseline_revisions")
    if public_head is not None:
        public_head["snapshot_id"] = f"{revision_id}.public-snapshot"
        public_head["source_artifact"] = {
            "artifact_id": new_source["artifact_id"],
            "path": "current.json#official_source_artifact",
            "sha256": canonical_sha256(new_source),
            "content_sha256": new_source["content_sha256"],
        }
        for capability in public_head.get("capabilities", []):
            capability["coverage_state"] = "blocked"
            capability["freshness"] = "stale"

    registry_head = new_heads.get("repository_registry_revisions")
    if registry_head is not None:
        registry_head["snapshot_id"] = f"{revision_id}.repository-snapshot"
        for repository in registry_head.get("repositories", []):
            codes = by_subject.get(str(repository.get("repo_id", "")), set())
            if codes:
                repository["freshness"] = "stale"
                if "source_unavailable" in codes:
                    repository["availability"] = "missing"

    decision_head = new_heads.get("capability_decision_revisions")
    decision_to_repo: dict[str, str] = {}
    decision_rows = (
        decision_head.get("decisions", [])
        if decision_head is not None
        else decisions_previous.get("decisions", [])
    )
    if decision_head is not None:
        decision_head["review_revision"] = review_revision
    for decision in decision_rows:
        decision_id = str(decision.get("decision_id", ""))
        repo_id = str(decision.get("repo_id", ""))
        decision_to_repo[decision_id] = repo_id
        if decision_head is not None and (by_subject.get(decision_id) or by_subject.get(repo_id)):
            decision["freshness"] = "stale"
            decision["review_revision"] = review_revision

    evidence_head = new_heads["evidence_revisions"]
    old_records = bundle["evidence_revisions"][-1]["records"]
    evidence_head["records"] = copy.deepcopy(old_records)

    evidence_sources = [head for head in (public_head, registry_head) if head is not None]
    old_ids = set().union(*(_collect_evidence_ids(head) for head in evidence_sources))
    if decision_head is not None:
        old_ids.update(
            _collect_evidence_ids(
                decision_head,
                ignored_keys=frozenset({"license_evidence_ids"}),
            )
        )
    replacements = {
        old_id: f"{revision_id}.rebind.{old_id}" for old_id in sorted(old_ids)
    }
    for head in evidence_sources:
        _replace_evidence_ids(head, replacements)
    if decision_head is not None:
        _replace_evidence_ids(
            decision_head,
            replacements,
            ignored_keys=frozenset({"license_evidence_ids"}),
        )

    for family, head in new_heads.items():
        successor[family].append(head)

    binding_errors: list[GovernanceError] = []
    expected_bindings = _expected_evidence_bindings(successor, binding_errors)
    if binding_errors:
        raise GovernanceUsageError("evidence_binding_ambiguity: refresh bindings conflict")

    records = evidence_head["records"]
    for old_id, new_id in sorted(replacements.items()):
        binding = expected_bindings.get(new_id)
        if binding is None:
            continue
        (
            subject_family,
            subject_id,
            subject_revision_id,
            source_revision,
            target_revision,
            environment_kind,
        ) = binding
        codes = set(by_subject.get(subject_id, set()))
        if subject_family in {"public_baseline", "capability"}:
            codes.update(official_codes)
        if subject_family == "capability_decision":
            codes.update(by_subject.get(decision_to_repo.get(subject_id, ""), set()))
        affected = bool(codes)
        reason = f"drift:{sorted(codes)[0]}" if affected else "revision_rebind"
        record = _base_evidence_record(
            evidence_id=new_id,
            subject_family=subject_family,
            subject_id=subject_id,
            subject_revision_id=subject_revision_id,
            source_revision=source_revision,
            target_revision=target_revision,
            environment_kind=environment_kind,
            timestamp=timestamp,
            drift_sha256=drift_sha256,
            drift_size=drift_size,
            freshness="stale" if affected else "current",
            result="blocked" if affected else "pass",
            coverage_state="blocked" if affected else "verified",
            proof_level=("local_contract" if environment_kind == "local_contract" else "source"),
            supersedes=[old_id],
            event_type="stale" if affected else "supersession",
            reason=reason,
        )
        _append_record(records, record)

    event_codes: list[str] = []
    for error in errors:
        code = str(error.get("code"))
        subject_id = str(error.get("subject_id"))
        event_codes.append(code)
        if subject_id == artifact_id:
            subject_family = "official_source_artifact"
            subject_revision_id = new_source["revision_id"]
        elif subject_id in repository_ids:
            subject_family = "repository"
            if registry_head is None:
                raise GovernanceUsageError("successor_invalid: registry successor missing")
            subject_revision_id = registry_head["revision_id"]
        elif subject_id in decision_ids:
            subject_family = "capability_decision"
            if decision_head is None:
                raise GovernanceUsageError("successor_invalid: decision successor missing")
            subject_revision_id = decision_head["revision_id"]
        else:
            raise GovernanceUsageError(f"affected_subject_unknown: {subject_id}")
        event = _base_evidence_record(
            evidence_id=f"{revision_id}.event.{code}.{subject_id}",
            subject_family=subject_family,
            subject_id=subject_id,
            subject_revision_id=subject_revision_id,
            source_revision=code,
            target_revision=subject_revision_id,
            environment_kind="source_review",
            timestamp=timestamp,
            drift_sha256=drift_sha256,
            drift_size=drift_size,
            freshness="stale",
            result="blocked" if code == "source_unavailable" else "fail",
            coverage_state="blocked",
            proof_level="source",
            supersedes=[],
            event_type="stale",
            reason=f"drift:{code}",
        )
        _append_record(records, event)

    manifest = copy.deepcopy(dict(bundle["bundle_manifest"]))
    manifest["evidence_head"] = {
        "revision_id": evidence_head["revision_id"],
        "path": "current.json#evidence_revisions/current",
        "sha256": canonical_sha256(evidence_head),
    }
    if touch_source:
        manifest.update(
            {
                "official_source_artifact": {
                "artifact_id": new_source["artifact_id"],
                "path": "current.json#official_source_artifact",
                "sha256": canonical_sha256(new_source),
                },
                "public_baseline": {
                "revision_id": public_head["revision_id"],
                "path": "current.json#public_baseline_revisions/current",
                "sha256": canonical_sha256(public_head),
                },
            }
        )
    if registry_head is not None:
        manifest["repository_registry"] = {
                "revision_id": registry_head["revision_id"],
                "path": "current.json#repository_registry_revisions/current",
                "sha256": canonical_sha256(registry_head),
            }
    if decision_head is not None:
        manifest["capability_decisions"] = {
                "revision_id": decision_head["revision_id"],
                "path": "current.json#capability_decision_revisions/current",
                "sha256": canonical_sha256(decision_head),
            }
    successor["bundle_manifest"] = manifest
    validation_errors = validate_governance(successor, root=REPOSITORY_ROOT)
    if validation_errors:
        codes = ",".join(error.code for error in validation_errors[:10])
        raise GovernanceUsageError(f"successor_invalid: {codes}")
    if touch_source:
        source_ancestry = validate_revision_ancestry(
            [previous_source, new_source],
            family="official_source_artifact_revisions",
        )
        if source_ancestry:
            raise GovernanceUsageError(
                f"successor_invalid: {source_ancestry[0].code}"
            )
    return successor, event_codes


def _refresh_result(
    *,
    successor: Mapping[str, Any],
    source_bundle: Mapping[str, Any],
    drift_sha256: str,
    revision_id: str,
    review_revision: str,
    event_codes: list[str],
    affected_subjects: list[str],
) -> dict[str, Any]:
    manifest = successor["bundle_manifest"]
    return {
        "schema": "kiana.capability-governance-refresh-result.v1",
        "version": "1.0",
        "revision_id": revision_id,
        "review_revision": review_revision,
        "source_bundle_sha256": canonical_sha256(source_bundle),
        "drift_report_sha256": drift_sha256,
        "current": {
            "path": "current.json",
            "sha256": canonical_sha256(successor),
        },
        "bundle_selector": copy.deepcopy(manifest),
        "event_codes": event_codes,
        "affected_subjects": affected_subjects,
    }


def _write_staged_json(path: Path, value: Mapping[str, Any]) -> None:
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def _publish_refresh(
    output_root: Path,
    *,
    successor: Mapping[str, Any],
    result: Mapping[str, Any],
    drift_report_path: Path,
) -> None:
    if output_root.is_symlink():
        raise GovernanceUsageError("output_escape: symlink output root is forbidden")
    parent = output_root.parent.resolve(strict=True)
    target = parent / output_root.name
    if target.exists():
        if not target.is_dir() or any(target.iterdir()):
            raise GovernanceUsageError("output_exists: output root must be absent or empty")
    stage = Path(tempfile.mkdtemp(prefix=f".{target.name}.refresh-", dir=parent))
    published = False
    try:
        _write_staged_json(stage / "current.json", successor)
        _write_staged_json(stage / "refresh-result.json", result)
        (stage / "drift-report.json").write_bytes(drift_report_path.read_bytes())
        _write_staged_json(stage / "official-source.json", successor["official_source_artifact"])
        for family, filename in (
            ("public_baseline_revisions", "public-baseline.json"),
            ("repository_registry_revisions", "repository-registry.json"),
            ("capability_decision_revisions", "capability-decisions.json"),
            ("evidence_revisions", "evidence-index.json"),
        ):
            _write_staged_json(stage / filename, successor[family][-1])
        staged = load_json(stage / "current.json")
        if canonical_sha256(staged) != canonical_sha256(successor):
            raise GovernanceUsageError("successor_invalid: staged bundle hash mismatch")
        if target.exists():
            target.rmdir()
        try:
            os.rename(stage, target)
        except FileExistsError as exc:
            raise GovernanceUsageError("output_exists: output root appeared during publish") from exc
        published = True
    finally:
        if not published:
            for child in stage.iterdir() if stage.exists() else []:
                child.unlink()
            stage.rmdir() if stage.exists() else None


def _refresh(args: argparse.Namespace) -> int:
    revision_id = _require_stable_id(args.revision_id, "revision_id")
    review_revision = _require_stable_id(args.review_revision, "review_revision")
    bundle, _source_path = _load_refresh_bundle(args)
    drift_path = args.drift_report.resolve()
    report = load_json(drift_path)
    errors, by_subject = _validate_refresh_report(report, bundle)
    drift_sha256 = file_sha256(drift_path)
    successor, event_codes = _build_refresh_successor(
        bundle,
        errors=errors,
        by_subject=by_subject,
        revision_id=revision_id,
        review_revision=review_revision,
        drift_sha256=drift_sha256,
        drift_size=drift_path.stat().st_size,
    )
    result = _refresh_result(
        successor=successor,
        source_bundle=bundle,
        drift_sha256=drift_sha256,
        revision_id=revision_id,
        review_revision=review_revision,
        event_codes=event_codes,
        affected_subjects=list(report["affected_subjects"]),
    )
    _publish_refresh(
        args.output_root,
        successor=successor,
        result=result,
        drift_report_path=drift_path,
    )
    return 0


def main() -> int:
    args = _parser().parse_args()
    try:
        if args.command == "freeze":
            return _freeze(args)
        if args.command == "check-drift":
            return _check_drift(args)
        return _refresh(args)
    except (GovernanceUsageError, OSError, ValueError, base64.binascii.Error) as exc:
        if isinstance(exc, GovernanceUsageError):
            message = str(exc)
        else:
            message = f"operation_failed: {type(exc).__name__}"
        print(message, file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
