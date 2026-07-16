#!/usr/bin/env python3
"""Validate Kiana capability governance and detect canonical drift."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

# The Rust supervisor invokes this script with Python isolated mode. Resolve the
# sibling production module from this trusted script directory, never from an
# ambient PYTHONPATH or the caller's working directory.
SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from capability_governance import (
    GovernanceError,
    GovernanceUsageError,
    detect_drift,
    load_fixture_bundle,
    load_json,
    load_manifest_bundle,
    validate_governance,
    validate_history_file,
    validation_report,
)


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)

    validate = commands.add_parser("validate", help="validate a fixture bundle or manifest")
    source = validate.add_mutually_exclusive_group(required=True)
    source.add_argument("--fixture-bundle", type=Path)
    source.add_argument("--manifest", type=Path)
    validate.add_argument("--live-reference-root", type=Path)
    validate.add_argument("--target-root", type=Path)
    validate.add_argument("--json", action="store_true", dest="json_output")

    history = commands.add_parser("validate-history", help="validate an immutable revision chain")
    history.add_argument("--head", type=Path, required=True)
    history.add_argument("--json", action="store_true", dest="json_output")

    drift = commands.add_parser("check-drift", help="compare frozen identities to live inputs")
    drift.add_argument("--manifest", type=Path, required=True)
    drift.add_argument("--live-reference-root", type=Path, required=True)
    drift.add_argument("--target-root", type=Path, required=True)
    drift.add_argument("--official-source-artifact", type=Path, required=True)
    drift.add_argument("--json", action="store_true", dest="json_output")
    return parser


def _render(report: dict[str, object], json_output: bool) -> None:
    if json_output:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True, separators=(",", ":")))
        return
    print(f"capability governance: {report['status']}")
    for error in report["errors"]:  # type: ignore[index]
        freshness = f" freshness={error['freshness']}" if error.get("freshness") else ""
        print(
            f"- {error['code']}: subject={error['subject_id']} path={error['path']}"
            f" detail={error['detail']}{freshness}"
        )


def _usage_report(exc: GovernanceUsageError) -> dict[str, object]:
    raw_code, separator, detail = str(exc).partition(":")
    code = {
        "input_too_large": "oversized_input",
    }.get(raw_code, raw_code if separator else "read_or_usage_failure")
    error = GovernanceError(code, detail=detail.strip() if separator else str(exc))
    return validation_report("error", [error])


def main() -> int:
    args = _parser().parse_args()
    try:
        if args.command == "validate":
            if args.fixture_bundle is not None:
                bundle = load_fixture_bundle(args.fixture_bundle.resolve())
            else:
                bundle = load_manifest_bundle(args.manifest.resolve())
            errors = validate_governance(bundle)
            report = validation_report("invalid" if errors else "valid", errors)
        elif args.command == "validate-history":
            errors = validate_history_file(args.head.resolve())
            report = validation_report("invalid" if errors else "valid", errors)
        else:
            bundle = load_manifest_bundle(args.manifest.resolve())
            semantic_errors = validate_governance(bundle)
            if semantic_errors:
                report = validation_report("invalid", semantic_errors)
            else:
                artifact = load_json(args.official_source_artifact.resolve())
                if not isinstance(artifact, dict):
                    raise GovernanceUsageError("official source artifact must be an object")
                errors = detect_drift(
                    bundle,
                    live_reference_root=args.live_reference_root.resolve(),
                    target_root=args.target_root.resolve(),
                    official_source_artifact=artifact,
                )
                report = validation_report("stale" if errors else "current", errors)
    except GovernanceUsageError as exc:
        report = _usage_report(exc)
        _render(report, getattr(args, "json_output", False))
        return 2
    _render(report, getattr(args, "json_output", False))
    return 0 if report["status"] in {"valid", "current"} else 1


if __name__ == "__main__":
    raise SystemExit(main())
