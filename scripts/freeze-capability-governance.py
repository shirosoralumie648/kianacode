#!/usr/bin/env python3
"""Freeze controlled governance inputs, check drift, and create refresh successors."""

from __future__ import annotations

import argparse
import base64
import copy
import json
import os
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
    canonical_json_bytes,
    canonical_sha256,
    detect_drift,
    file_sha256,
    load_fixture_bundle,
    load_json,
    load_manifest_bundle,
    repository_fingerprint,
    resolve_repository_path,
    sorted_errors,
    validate_governance,
)


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


def _run_git(args: list[str], cwd: Path, *, commit: bool = False) -> None:
    environment = os.environ.copy()
    if commit:
        environment.update(
            {
                "GIT_AUTHOR_DATE": "2026-07-15T00:00:00Z",
                "GIT_COMMITTER_DATE": "2026-07-15T00:00:00Z",
            }
        )
    result = subprocess.run(
        args,
        cwd=cwd,
        env=environment,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise GovernanceUsageError("fixture_git_failed: controlled Git setup failed")


def _materialize_fixture_world(
    request: Mapping[str, Any], root: Path
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
        if name == "git_repository":
            _run_git(["git", "init", "-q"], path)
            _run_git(["git", "add", "."], path)
            _run_git(
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
                path,
                commit=True,
            )
        fingerprint = repository_fingerprint(path)
        revision_kind = "git_commit" if name == "git_repository" else "content_tree_sha256"
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
        request, root
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


def main() -> int:
    args = _parser().parse_args()
    try:
        if args.command == "freeze":
            return _freeze(args)
        return _check_drift(args)
    except (GovernanceUsageError, OSError, ValueError, base64.binascii.Error) as exc:
        if isinstance(exc, GovernanceUsageError):
            message = str(exc)
        else:
            message = f"operation_failed: {type(exc).__name__}"
        print(message, file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
