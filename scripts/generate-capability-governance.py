#!/usr/bin/env python3
"""Render deterministic human views and revision diffs from governance authority."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from capability_governance import (
    COMPAT_WARNING_PATH,
    GovernanceUsageError,
    build_revision_diff,
    canonical_sha256,
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


def verify_production_command(args: argparse.Namespace, root: Path) -> bool:
    bundle = load_validated_manifest_bundle(args.manifest, root=root)
    view_payloads = render_governance_views(bundle)
    view_targets = preflight_output_paths(args.generated_root, view_payloads)
    repository_outputs, generated_outputs = load_compat_output_manifest(
        args.compat_manifest
    )
    if set(generated_outputs) != set(view_payloads):
        raise GovernanceUsageError("compat_manifest_invalid: generated output mismatch")
    repository_payloads = {COMPAT_WARNING_PATH: render_compat_warning()}
    if set(repository_outputs) != set(repository_payloads):
        raise GovernanceUsageError("compat_manifest_invalid: repository output mismatch")
    repository_targets = preflight_output_paths(
        args.repository_output_root,
        repository_payloads,
    )

    public_document = validated_diff_document(
        "public-baseline",
        args.public_from,
        args.public_to,
        root,
    )
    registry_document = validated_diff_document(
        "repository-registry",
        args.registry_from,
        args.registry_to,
        root,
    )
    public_payloads = {
        args.public_diff.name: deterministic_json_bytes(public_document)
    }
    registry_payloads = {
        args.registry_diff.name: deterministic_json_bytes(registry_document)
    }
    public_targets = preflight_output_paths(args.public_diff.parent, public_payloads)
    registry_targets = preflight_output_paths(args.registry_diff.parent, registry_payloads)
    return all(
        (
            write_or_check_outputs(view_targets, view_payloads, check=True),
            write_or_check_outputs(
                repository_targets,
                repository_payloads,
                check=True,
            ),
            write_or_check_outputs(public_targets, public_payloads, check=True),
            write_or_check_outputs(registry_targets, registry_payloads, check=True),
        )
    )


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
