#!/usr/bin/env python3
"""Render deterministic human views and revision diffs from governance authority."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from capability_governance import (
    COMPAT_WARNING_PATH,
    GovernanceUsageError,
    build_revision_diff,
    deterministic_json_bytes,
    file_sha256,
    load_compat_output_manifest,
    load_json,
    load_validated_manifest_bundle,
    preflight_output_paths,
    render_compat_warning,
    render_governance_views,
    structural_errors,
    validate_revision_ancestry,
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


def diff_command(args: argparse.Namespace, root: Path) -> bool:
    # The production selector remains the closed authority even when callers
    # request a diff between two explicitly supplied immutable revisions.
    load_validated_manifest_bundle(
        root / "docs/agent-program/kiana-completion/governance/current.json",
        root=root,
    )
    before = load_json(args.from_path)
    after = load_json(args.to_path)
    if not isinstance(before, dict) or not isinstance(after, dict):
        raise GovernanceUsageError("diff_revision_invalid: roots must be objects")
    errors = [*structural_errors(before, root), *structural_errors(after, root)]
    family_key = {
        "public-baseline": "public_baseline_revisions",
        "repository-registry": "repository_registry_revisions",
    }[args.family]
    errors.extend(validate_revision_ancestry([before, after], family=family_key))
    if errors:
        raise GovernanceUsageError(f"diff_revision_invalid: {errors[0].code}")
    document = build_revision_diff(
        args.family,
        before,
        after,
        from_sha256=file_sha256(args.from_path),
        to_sha256=file_sha256(args.to_path),
    )
    diff_errors = structural_errors(document, root)
    if diff_errors:
        raise GovernanceUsageError(f"diff_invalid: {diff_errors[0].code}")
    payloads = {args.output.name: deterministic_json_bytes(document)}
    targets = preflight_output_paths(args.output.parent, payloads)
    return write_or_check_outputs(targets, payloads, check=args.check)


def main() -> int:
    args = parser().parse_args()
    try:
        root = repository_root()
        if args.command == "render":
            matches = render_command(args, root)
        elif args.command == "render-compat":
            matches = render_compat_command(args, root)
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
