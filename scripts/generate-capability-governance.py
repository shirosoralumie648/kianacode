#!/usr/bin/env python3
"""Render deterministic human views and revision diffs from governance authority."""

from __future__ import annotations

import argparse
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Mapping

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from capability_governance import (
    COMPAT_WARNING_PATH,
    PRODUCTION_CHECK_IDS,
    GovernanceUsageError,
    build_production_report,
    build_revision_diff,
    canonical_sha256,
    detect_drift,
    deterministic_json_bytes,
    file_sha256,
    load_compat_output_manifest,
    load_json,
    load_revision_chain_from_head,
    load_validated_manifest_bundle,
    preflight_output_paths,
    render_compat_warning,
    render_governance_views,
    resolve_repository_path,
    structural_errors,
    validate_production_report,
    validate_revision_ancestry,
    validation_report,
    write_or_check_outputs,
)


def repository_root() -> Path:
    root = Path(__file__).resolve().parent.parent
    if not (root / "docs/agent-program/kiana-completion/governance/current.json").is_file():
        raise GovernanceUsageError("repository_root_invalid: governance/current.json is missing")
    return root


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    subparsers = result.add_subparsers(dest="command", required=True)

    render = subparsers.add_parser("render")
    render.add_argument("--manifest", type=Path, required=True)
    render.add_argument("--output-root", type=Path, required=True)
    render.add_argument("--check", action="store_true")

    compat = subparsers.add_parser("render-compat")
    compat.add_argument("--manifest", type=Path, required=True)
    compat.add_argument("--output-root", type=Path, required=True)
    compat.add_argument("--repository-output-root", type=Path, required=True)
    compat.add_argument("--compat-manifest", type=Path, required=True)
    compat.add_argument("--check", action="store_true")

    diff = subparsers.add_parser("diff")
    diff.add_argument(
        "--family",
        choices=("public-baseline", "repository-registry"),
        required=True,
    )
    diff.add_argument("--from", dest="from_path", type=Path, required=True)
    diff.add_argument("--to", dest="to_path", type=Path, required=True)
    diff.add_argument("--output", type=Path, required=True)
    diff.add_argument("--check", action="store_true")

    verify = subparsers.add_parser("verify-production")
    verify.add_argument("--manifest", type=Path, required=True)
    verify.add_argument("--generated-root", type=Path, required=True)
    verify.add_argument("--repository-output-root", type=Path, required=True)
    verify.add_argument("--compat-manifest", type=Path, required=True)
    verify.add_argument("--public-from", type=Path, required=True)
    verify.add_argument("--public-to", type=Path, required=True)
    verify.add_argument("--public-diff", type=Path, required=True)
    verify.add_argument("--registry-from", type=Path, required=True)
    verify.add_argument("--registry-to", type=Path, required=True)
    verify.add_argument("--registry-diff", type=Path, required=True)
    verify.add_argument("--report", type=Path, required=True)
    verify.add_argument("--drift-report", type=Path, required=True)
    return result


def render_command(args: argparse.Namespace, root: Path) -> bool:
    bundle = load_validated_manifest_bundle(args.manifest, root=root)
    payloads = render_governance_views(bundle)
    targets = preflight_output_paths(args.output_root, payloads)
    return write_or_check_outputs(targets, payloads, check=args.check)


def render_compat_command(args: argparse.Namespace, root: Path) -> bool:
    bundle = load_validated_manifest_bundle(args.manifest, root=root)
    repository_outputs, generated_outputs = load_compat_output_manifest(args.compat_manifest)
    view_payloads = render_governance_views(bundle)
    if set(generated_outputs) != set(view_payloads):
        raise GovernanceUsageError("compat_manifest_invalid: generated output mismatch")
    repository_payloads = {COMPAT_WARNING_PATH: render_compat_warning()}
    if set(repository_outputs) != set(repository_payloads):
        raise GovernanceUsageError("compat_manifest_invalid: repository output mismatch")
    view_targets = preflight_output_paths(args.output_root, view_payloads)
    repository_targets = preflight_output_paths(
        args.repository_output_root,
        repository_payloads,
    )
    if set(view_targets.values()).intersection(repository_targets.values()):
        raise GovernanceUsageError("output_collision: output roots overlap")
    if args.check:
        return write_or_check_outputs(view_targets, view_payloads, check=True) and write_or_check_outputs(
            repository_targets,
            repository_payloads,
            check=True,
        )
    write_or_check_outputs(view_targets, view_payloads, check=False)
    return write_or_check_outputs(repository_targets, repository_payloads, check=False)


def validated_diff_document(
    family: str,
    before_path: Path,
    after_path: Path,
    root: Path,
) -> dict[str, object]:
    before = load_json(before_path)
    after = load_json(after_path)
    if not isinstance(before, dict) or not isinstance(after, dict):
        raise GovernanceUsageError("diff_revision_invalid: roots must be objects")
    family_key = {
        "public-baseline": "public_baseline_revisions",
        "repository-registry": "repository_registry_revisions",
    }[family]
    chain = load_revision_chain_from_head(after_path, root=root)
    errors = [
        error
        for revision in chain
        for error in structural_errors(revision, root)
    ]
    errors.extend(validate_revision_ancestry(chain, family=family_key))
    matching_ancestors = [
        revision
        for revision in chain
        if revision.get("revision_id") == before.get("revision_id")
    ]
    if (
        len(matching_ancestors) != 1
        or canonical_sha256(matching_ancestors[0]) != canonical_sha256(before)
        or canonical_sha256(chain[-1]) != canonical_sha256(after)
    ):
        raise GovernanceUsageError(
            "diff_revision_invalid: supplied revisions are not exact ancestor and head"
        )
    if errors:
        raise GovernanceUsageError(f"diff_revision_invalid: {errors[0].code}")
    document = build_revision_diff(
        family,
        before,
        after,
        from_sha256=file_sha256(before_path),
        to_sha256=file_sha256(after_path),
    )
    diff_errors = structural_errors(document, root)
    if diff_errors:
        raise GovernanceUsageError(f"diff_invalid: {diff_errors[0].code}")
    return document


def validate_diff_family_authority(
    family: str,
    after_path: Path,
    root: Path,
) -> None:
    manifest_path = root / "docs/agent-program/kiana-completion/governance/current.json"
    manifest = load_json(manifest_path)
    if not isinstance(manifest, dict):
        raise GovernanceUsageError("manifest_invalid: root must be an object")
    errors = structural_errors(manifest, root)
    if errors:
        raise GovernanceUsageError(f"manifest_invalid: {errors[0].code}")
    binding_key, family_key = {
        "public-baseline": ("public_baseline", "public_baseline_revisions"),
        "repository-registry": (
            "repository_registry",
            "repository_registry_revisions",
        ),
    }[family]
    binding = manifest.get(binding_key)
    if not isinstance(binding, dict):
        raise GovernanceUsageError(f"manifest_invalid: {binding_key} binding is required")
    selected_path = resolve_repository_path(root, str(binding.get("path", "")))
    selected = load_json(selected_path)
    if not isinstance(selected, dict) or (
        binding.get("revision_id") != selected.get("revision_id")
        or binding.get("sha256") != canonical_sha256(selected)
    ):
        raise GovernanceUsageError("diff_revision_invalid: selected head binding mismatch")
    chain = load_revision_chain_from_head(selected_path, root=root)
    chain_errors = [
        error
        for revision in chain
        for error in structural_errors(revision, root)
    ]
    chain_errors.extend(validate_revision_ancestry(chain, family=family_key))
    if chain_errors:
        raise GovernanceUsageError(
            f"diff_revision_invalid: selected family {chain_errors[0].code}"
        )
    target = load_json(after_path.resolve())
    if not isinstance(target, dict) or sum(
        revision.get("revision_id") == target.get("revision_id")
        and canonical_sha256(revision) == canonical_sha256(target)
        for revision in chain
    ) != 1:
        raise GovernanceUsageError(
            "diff_revision_invalid: to revision is not in the selected family ancestry"
        )


def diff_command(args: argparse.Namespace, root: Path) -> bool:
    # The closed production selector must select the exact family ancestry used
    # by this command. Unrelated families are validated by render/production.
    validate_diff_family_authority(args.family, args.to_path, root)
    document = validated_diff_document(
        args.family,
        args.from_path,
        args.to_path,
        root,
    )
    payloads = {args.output.name: deterministic_json_bytes(document)}
    targets = preflight_output_paths(args.output.parent, payloads)
    return write_or_check_outputs(targets, payloads, check=args.check)


def _write_json_output(path: Path, value: Mapping[str, Any]) -> None:
    payloads = {path.name: deterministic_json_bytes(value)}
    targets = preflight_output_paths(path.parent, payloads)
    write_or_check_outputs(targets, payloads, check=False)


def _production_manifest_context(
    bundle: Mapping[str, Any],
) -> tuple[Mapping[str, Any], str, str]:
    manifest = bundle.get("bundle_manifest")
    if not isinstance(manifest, Mapping):
        raise GovernanceUsageError("production_manifest_invalid: bundle manifest is required")
    evaluation_time = manifest.get("evaluation_time")
    if not isinstance(evaluation_time, str):
        raise GovernanceUsageError("production_evaluation_time_invalid")
    return manifest, canonical_sha256(manifest), evaluation_time


def _check_current_selector(args: argparse.Namespace, root: Path) -> bool:
    current = (
        root / "docs/agent-program/kiana-completion/governance/current.json"
    ).resolve()
    if args.manifest.resolve() != current:
        raise GovernanceUsageError("production_current_selector_invalid")
    return True


def _check_evaluation_time(evaluation_time: str) -> bool:
    try:
        parsed = datetime.strptime(evaluation_time, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError as exc:
        raise GovernanceUsageError("production_evaluation_time_invalid") from exc
    if parsed.replace(tzinfo=timezone.utc).tzinfo is not timezone.utc:
        raise GovernanceUsageError("production_evaluation_time_invalid")
    return True


def _check_current_repository_identities(bundle: Mapping[str, Any]) -> bool:
    revisions = bundle.get("repository_registry_revisions")
    if not isinstance(revisions, list) or not revisions:
        raise GovernanceUsageError("production_repository_registry_invalid")
    head = revisions[-1]
    if not isinstance(head, Mapping):
        raise GovernanceUsageError("production_repository_registry_invalid")
    rows = head.get("repositories")
    if (
        head.get("expected_count") != 38
        or not isinstance(rows, list)
        or len(rows) != 38
    ):
        raise GovernanceUsageError("production_repository_count")
    repository_ids = [row.get("repo_id") for row in rows if isinstance(row, Mapping)]
    repository_paths = [row.get("path") for row in rows if isinstance(row, Mapping)]
    if (
        len(repository_ids) != 38
        or len(repository_paths) != 38
        or len(set(repository_ids)) != 38
        or len(set(repository_paths)) != 38
        or any(
            not isinstance(row, Mapping) or row.get("freshness") != "current"
            for row in rows
        )
    ):
        raise GovernanceUsageError("production_repository_identity")
    return True


def _run_production_corpus(args: argparse.Namespace, root: Path) -> bool:
    temporary_root = args.report.parent
    temporary_root.mkdir(parents=True, exist_ok=True)
    result = subprocess.run(
        [
            sys.executable,
            "-I",
            str(root / "scripts/run-capability-governance-corpus.py"),
            "--temp-root",
            str(temporary_root),
            "--case",
            "integrity.evidence-expired",
            "--case",
            "integrity.newer-failed-retest",
        ],
        check=False,
        stdin=subprocess.DEVNULL,
    )
    if result.returncode != 0:
        raise GovernanceUsageError("production_corpus_failed")
    return True


def _check_legacy_authority(bundle: Mapping[str, Any], root: Path) -> bool:
    revisions = bundle.get("legacy_authority_revisions")
    if not isinstance(revisions, list) or not revisions or not isinstance(revisions[-1], Mapping):
        raise GovernanceUsageError("production_legacy_authority_invalid")
    entries = revisions[-1].get("entries")
    if not isinstance(entries, list):
        raise GovernanceUsageError("production_legacy_authority_invalid")
    expected_paths = {
        path.relative_to(root).as_posix()
        for path in (root / "docs/reference_audit").glob("*.md")
    }
    expected_paths.update(
        {
            "docs/reference-feature-matrix.md",
            "docs/reference-migration-roadmap.md",
            "docs/commercial-release-readiness.md",
            "docs/agent-program/kiana-completion/references.json",
        }
    )
    by_path = {
        row.get("path"): row
        for row in entries
        if isinstance(row, Mapping) and isinstance(row.get("path"), str)
    }
    if len(by_path) != len(entries) or set(by_path) != expected_paths:
        raise GovernanceUsageError("production_legacy_inventory_mismatch")
    for relative_path, entry in by_path.items():
        path = resolve_repository_path(root, relative_path)
        if (
            file_sha256(path) != entry.get("content_sha256")
            or not str(entry.get("rationale", "")).strip()
            or not str(entry.get("replacement_view", "")).strip()
        ):
            raise GovernanceUsageError("production_legacy_authority_invalid")

    manifest, _manifest_sha256, _evaluation_time = _production_manifest_context(bundle)
    selected_heads = {
        "official_source_artifact": bundle.get("official_source_artifact"),
        "public_baseline": bundle.get("public_baseline_revisions", [None])[-1],
        "repository_registry": bundle.get("repository_registry_revisions", [None])[-1],
        "capability_decisions": bundle.get("capability_decision_revisions", [None])[-1],
        "evidence_head": bundle.get("evidence_revisions", [None])[-1],
        "legacy_authority": revisions[-1],
    }
    for binding_key, selected_head in selected_heads.items():
        binding = manifest.get(binding_key)
        if (
            not isinstance(binding, Mapping)
            or not isinstance(selected_head, Mapping)
            or binding.get("sha256") != canonical_sha256(selected_head)
        ):
            raise GovernanceUsageError("production_current_head_invalid")
    return True


def _production_diff_document(
    bundle: Mapping[str, Any],
    family: str,
    before_path: Path,
    after_path: Path,
    root: Path,
) -> dict[str, object]:
    family_key = {
        "public-baseline": "public_baseline_revisions",
        "repository-registry": "repository_registry_revisions",
    }[family]
    chain = bundle.get(family_key)
    if not isinstance(chain, list) or not chain or any(
        not isinstance(revision, Mapping) for revision in chain
    ):
        raise GovernanceUsageError("production_diff_invalid: selected ancestry is required")
    try:
        before_path.resolve().relative_to(root)
        after_path.resolve().relative_to(root)
    except ValueError as exc:
        raise GovernanceUsageError("production_diff_invalid: path outside repository") from exc
    before = load_json(before_path.resolve())
    after = load_json(after_path.resolve())
    if not isinstance(before, Mapping) or not isinstance(after, Mapping):
        raise GovernanceUsageError("production_diff_invalid: revisions must be objects")
    matching_before = [
        revision
        for revision in chain
        if revision.get("revision_id") == before.get("revision_id")
        and canonical_sha256(revision) == canonical_sha256(before)
    ]
    if (
        len(matching_before) != 1
        or canonical_sha256(chain[-1]) != canonical_sha256(after)
    ):
        raise GovernanceUsageError("production_diff_invalid: selected ancestry mismatch")
    document = build_revision_diff(
        family,
        before,
        after,
        from_sha256=file_sha256(before_path),
        to_sha256=file_sha256(after_path),
    )
    errors = structural_errors(document, root)
    if errors:
        raise GovernanceUsageError(f"production_diff_invalid: {errors[0].code}")
    return document


def _check_generated_views(bundle: Mapping[str, Any], args: argparse.Namespace) -> bool:
    payloads = render_governance_views(bundle)
    targets = preflight_output_paths(args.generated_root, payloads)
    return write_or_check_outputs(targets, payloads, check=True)


def _check_compatibility_output(
    bundle: Mapping[str, Any],
    args: argparse.Namespace,
) -> bool:
    repository_outputs, generated_outputs = load_compat_output_manifest(args.compat_manifest)
    view_payloads = render_governance_views(bundle)
    if set(generated_outputs) != set(view_payloads):
        raise GovernanceUsageError("compat_manifest_invalid: generated output mismatch")
    repository_payloads = {COMPAT_WARNING_PATH: render_compat_warning()}
    if set(repository_outputs) != set(repository_payloads):
        raise GovernanceUsageError("compat_manifest_invalid: repository output mismatch")
    targets = preflight_output_paths(args.repository_output_root, repository_payloads)
    return write_or_check_outputs(targets, repository_payloads, check=True)


def _check_production_obligation(action: Callable[[], bool]) -> str:
    try:
        return "pass" if action() else "fail"
    except (GovernanceUsageError, OSError, subprocess.SubprocessError, ValueError):
        return "fail"


def verify_production_command(args: argparse.Namespace, root: Path) -> bool:
    # The complete canonical bundle is deliberately loaded once. Every later
    # obligation consumes this immutable in-memory authority rather than
    # reloading the selector or semantic graph through another CLI process.
    bundle = load_validated_manifest_bundle(args.manifest, root=root)
    manifest, manifest_sha256, evaluation_time = _production_manifest_context(bundle)

    drift_errors = detect_drift(
        bundle,
        live_reference_root=root / "reference",
        target_root=root,
        official_source_artifact=bundle["official_source_artifact"],
    )
    drift_report = validation_report("stale" if drift_errors else "current", drift_errors)
    _write_json_output(args.drift_report, drift_report)
    drift_codes = {error.code for error in drift_errors}
    official_codes = {"official_source_hash_drift"}
    reference_codes = {
        "repository_head_drift",
        "repository_tree_drift",
        "content_tree_drift",
        "license_hash_drift",
        "source_unavailable",
    }
    target_codes = {"target_revision_drift", "target_unavailable"}
    unknown_drift_codes = drift_codes - official_codes - reference_codes - target_codes
    if unknown_drift_codes:
        raise GovernanceUsageError("production_drift_unclassified")

    public_document = _production_diff_document(
        bundle,
        "public-baseline",
        args.public_from,
        args.public_to,
        root,
    )
    registry_document = _production_diff_document(
        bundle,
        "repository-registry",
        args.registry_from,
        args.registry_to,
        root,
    )

    checks = [
        {"id": "canonical-bundle", "status": "pass"},
        {
            "id": "current-selector",
            "status": _check_production_obligation(
                lambda: _check_current_selector(args, root)
            ),
        },
        {
            "id": "evaluation-time",
            "status": _check_production_obligation(
                lambda: _check_evaluation_time(evaluation_time)
            ),
        },
        {
            "id": "repository-identities-38",
            "status": _check_production_obligation(
                lambda: _check_current_repository_identities(bundle)
            ),
        },
        {
            "id": "official-source-drift",
            "status": "stale" if drift_codes.intersection(official_codes) else "pass",
        },
        {
            "id": "reference-drift",
            "status": "stale" if drift_codes.intersection(reference_codes) else "pass",
        },
        {
            "id": "target-drift",
            "status": "stale" if drift_codes.intersection(target_codes) else "pass",
        },
    ]
    corpus_status = _check_production_obligation(
        lambda: _run_production_corpus(args, root)
    )
    checks.extend(
        [
            {"id": "integrity.evidence-expired", "status": corpus_status},
            {"id": "integrity.newer-failed-retest", "status": corpus_status},
            {
                "id": "legacy-authority",
                "status": _check_production_obligation(
                    lambda: _check_legacy_authority(bundle, root)
                ),
            },
            {
                "id": "generated-views",
                "status": _check_production_obligation(
                    lambda: _check_generated_views(bundle, args)
                ),
            },
            {
                "id": "compatibility-output",
                "status": _check_production_obligation(
                    lambda: _check_compatibility_output(bundle, args)
                ),
            },
            {
                "id": "public-baseline-diff",
                "status": _check_production_obligation(
                    lambda: write_or_check_outputs(
                        preflight_output_paths(
                            args.public_diff.parent,
                            {args.public_diff.name: deterministic_json_bytes(public_document)},
                        ),
                        {args.public_diff.name: deterministic_json_bytes(public_document)},
                        check=True,
                    )
                ),
            },
            {
                "id": "repository-registry-diff",
                "status": _check_production_obligation(
                    lambda: write_or_check_outputs(
                        preflight_output_paths(
                            args.registry_diff.parent,
                            {args.registry_diff.name: deterministic_json_bytes(registry_document)},
                        ),
                        {args.registry_diff.name: deterministic_json_bytes(registry_document)},
                        check=True,
                    )
                ),
            },
        ]
    )
    if tuple(row["id"] for row in checks) != PRODUCTION_CHECK_IDS:
        raise GovernanceUsageError("production_report_invalid: producer check ordering")
    report = build_production_report(
        manifest_sha256=manifest_sha256,
        evaluation_time=evaluation_time,
        checks=checks,
    )
    _write_json_output(args.report, report)
    validate_production_report(
        report,
        manifest_sha256=manifest_sha256,
        evaluation_time=evaluation_time,
    )
    return True


def main() -> int:
    args = parser().parse_args()
    try:
        root = repository_root()
        if args.command == "render":
            matches = render_command(args, root)
        elif args.command == "render-compat":
            matches = render_compat_command(args, root)
        elif args.command == "verify-production":
            matches = verify_production_command(args, root)
        else:
            matches = diff_command(args, root)
        return 0 if matches else 1
    except GovernanceUsageError as exc:
        print(str(exc), file=sys.stderr)
        return 1
    except OSError as exc:
        print(f"io_failed: {exc.strerror or 'operation failed'}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
