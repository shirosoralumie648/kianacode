#!/usr/bin/env python3
"""Build deterministic SBOMs and a fail-closed SC-28 result report.

The shell wrapper owns Cargo and scanner process execution.  This module only
normalizes Cargo metadata and scanner output into bounded, reviewable artifacts;
it never resolves a dependency or grants a release permission itself.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import uuid
from typing import Any


SCHEMA = "kiana.supply-chain-scan.v1"
QUARANTINE_SCHEMA = "kiana.supply-chain-quarantine.v1"


def read_json(path: pathlib.Path, default: Any) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (FileNotFoundError, json.JSONDecodeError, OSError):
        return default


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def package_ref(package: dict[str, Any]) -> str:
    name = package.get("name") or "unknown"
    version = package.get("version") or "0.0.0"
    source = package.get("source") or "workspace"
    if source.startswith("registry+"):
        return f"pkg:cargo/{name}@{version}"
    if source.startswith("git+"):
        digest = hashlib.sha256(source.encode("utf-8")).hexdigest()[:16]
        return f"pkg:cargo/{name}@{version}?source={digest}"
    return f"pkg:cargo/{name}@{version}?source=workspace"


def spdx_id(package_id: str) -> str:
    return "SPDXRef-Package-" + hashlib.sha256(package_id.encode("utf-8")).hexdigest()[:24]


def package_license(package: dict[str, Any]) -> tuple[str, bool, str | None]:
    license_id = package.get("license")
    if isinstance(license_id, str) and license_id.strip():
        return license_id.strip(), False, None
    license_file = package.get("license_file")
    if isinstance(license_file, str) and license_file.strip():
        return "SEE-LICENSE-FILE", False, license_file.strip()
    return "NOASSERTION", True, None


def normalize_advisories(path: pathlib.Path) -> dict[str, Any]:
    payload = read_json(path, {})
    vulnerabilities = payload.get("vulnerabilities", {}) if isinstance(payload, dict) else {}
    listed = vulnerabilities.get("list", []) if isinstance(vulnerabilities, dict) else []
    if not isinstance(listed, list):
        listed = []

    normalized: list[dict[str, Any]] = []
    severity_counts = {"critical": 0, "high": 0, "medium": 0, "low": 0, "unknown": 0}
    for item in listed[:1000]:
        if not isinstance(item, dict):
            continue
        advisory = item.get("advisory") if isinstance(item.get("advisory"), dict) else item
        advisory_id = str(advisory.get("id") or advisory.get("url") or "unknown")
        severity = str(advisory.get("severity") or "unknown").lower()
        if severity not in severity_counts:
            severity = "unknown"
        severity_counts[severity] += 1
        package = item.get("package") if isinstance(item.get("package"), dict) else {}
        normalized.append(
            {
                "id": advisory_id[:160],
                "package": str(package.get("name") or "unknown")[:160],
                "version": str(package.get("version") or "unknown")[:80],
                "severity": severity,
                "url": str(advisory.get("url") or "")[:512],
            }
        )
    return {
        "count": len(normalized),
        "severity_counts": severity_counts,
        "findings": normalized,
        "json_present": path.is_file(),
    }


def make_sbom(
    metadata: dict[str, Any], lock_digest: str, source_revision: str, version: str
) -> tuple[dict[str, Any], dict[str, Any]]:
    packages = [item for item in metadata.get("packages", []) if isinstance(item, dict)]
    packages.sort(key=lambda item: (str(item.get("name", "")), str(item.get("version", "")), str(item.get("id", ""))))
    workspace_members = set(metadata.get("workspace_members", []))
    refs = {str(item.get("id")): package_ref(item) for item in packages if item.get("id")}
    spdx_refs = {str(item.get("id")): spdx_id(str(item.get("id"))) for item in packages if item.get("id")}

    components: list[dict[str, Any]] = []
    spdx_packages: list[dict[str, Any]] = []
    unknown_license: list[dict[str, str]] = []
    for package in packages:
        package_id = str(package.get("id") or "")
        ref = refs.get(package_id, package_ref(package))
        license_value, unknown, license_file = package_license(package)
        if unknown:
            unknown_license.append(
                {"name": str(package.get("name") or "unknown"), "version": str(package.get("version") or "unknown")}
            )
        component: dict[str, Any] = {
            "bom-ref": ref,
            "name": package.get("name") or "unknown",
            "type": "library",
            "version": package.get("version") or "0.0.0",
            "scope": "required" if package_id in workspace_members else "optional",
            "licenses": (
                [{"license": {"name": license_value, "url": license_file}}]
                if license_file
                else ([{"expression": license_value}] if license_value != "NOASSERTION" else [{"license": {"name": "NOASSERTION"}}])
            ),
        }
        source = package.get("source")
        if isinstance(source, str) and source.startswith("registry+"):
            component["purl"] = ref
        repository = package.get("repository")
        if repository:
            component["externalReferences"] = [{"type": "vcs", "url": repository}]
        components.append(component)

        spdx_license = "NOASSERTION" if license_file else (license_value if not unknown else "NOASSERTION")
        spdx_packages.append(
            {
                "SPDXID": spdx_refs.get(package_id, spdx_id(package_id)),
                "name": package.get("name") or "unknown",
                "versionInfo": package.get("version") or "0.0.0",
                "downloadLocation": source if isinstance(source, str) and source.startswith(("http://", "https://")) else "NOASSERTION",
                "licenseConcluded": spdx_license,
                "licenseDeclared": spdx_license,
                "filesAnalyzed": False,
                "externalRefs": ([{"referenceCategory": "PACKAGE-MANAGER", "referenceType": "purl", "referenceLocator": ref}] if ref.startswith("pkg:") else []),
            }
        )

    relationships: list[dict[str, str]] = []
    dependencies: list[dict[str, Any]] = []
    resolve = metadata.get("resolve") or {}
    for node in resolve.get("nodes", []) if isinstance(resolve, dict) else []:
        if not isinstance(node, dict):
            continue
        node_id = str(node.get("id") or "")
        if node_id not in refs:
            continue
        depends_on = sorted(refs[item] for item in node.get("dependencies", []) if item in refs)
        dependencies.append({"ref": refs[node_id], "dependsOn": depends_on})
        for dependency in depends_on:
            relationships.append(
                {"spdxElementId": spdx_refs[node_id], "relationshipType": "DEPENDS_ON", "relatedSpdxElement": spdx_id(next((key for key, value in refs.items() if value == dependency), dependency))}
            )

    root_name = "kiana"
    root_version = version.strip() or "0.0.0"
    serial_seed = uuid.uuid5(uuid.NAMESPACE_URL, f"kiana:{lock_digest}:{source_revision}")
    properties = [
        {"name": "kiana:lockfile-sha256", "value": lock_digest},
        {"name": "kiana:source-revision", "value": source_revision},
        {"name": "kiana:license-unknown-count", "value": str(len(unknown_license))},
    ]
    cyclonedx = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": f"urn:uuid:{serial_seed}",
        "version": 1,
        "metadata": {
            "component": {"type": "application", "name": root_name, "version": root_version, "bom-ref": "kiana"},
            "properties": properties,
        },
        "components": components,
        "dependencies": sorted(dependencies, key=lambda item: item["ref"]),
    }
    spdx = {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": f"kiana-{root_version}",
        "documentNamespace": f"https://kiana.dev/sbom/{lock_digest}",
        "creationInfo": {"created": "1970-01-01T00:00:00Z", "creators": ["Tool: kiana-supply-chain-scan"]},
        "packages": spdx_packages,
        "relationships": [
            {"spdxElementId": "SPDXRef-DOCUMENT", "relationshipType": "DESCRIBES", "relatedSpdxElement": "SPDXRef-Application"},
            *relationships,
        ],
        "annotations": [
            {"annotationDate": "1970-01-01T00:00:00Z", "annotationType": "OTHER", "annotator": "Tool: kiana-supply-chain-scan", "comment": f"lockfile-sha256={lock_digest}"}
        ],
    }
    # SPDX requires an application package for the DESCRIBES relationship.  It
    # is intentionally a stable synthetic package, separate from Cargo's root.
    spdx["packages"].insert(
        0,
        {
            "SPDXID": "SPDXRef-Application",
            "name": root_name,
            "versionInfo": root_version,
            "downloadLocation": "NOASSERTION",
            "licenseConcluded": "MIT OR Apache-2.0",
            "licenseDeclared": "MIT OR Apache-2.0",
            "filesAnalyzed": False,
        },
    )
    return cyclonedx, {"spdx": spdx, "unknown_license": unknown_license}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--metadata", required=True, type=pathlib.Path)
    parser.add_argument("--lockfile", required=True, type=pathlib.Path)
    parser.add_argument("--out-dir", required=True, type=pathlib.Path)
    parser.add_argument("--source-revision", default="unknown")
    parser.add_argument("--lock-before", required=True)
    parser.add_argument("--lock-after", required=True)
    parser.add_argument("--lock-dirty", default="false")
    parser.add_argument("--expected-lock-digest", default="")
    parser.add_argument("--audit-json", type=pathlib.Path, required=True)
    parser.add_argument("--audit-exit", type=int, default=127)
    parser.add_argument("--deny-exit", type=int, default=127)
    parser.add_argument("--audit-tool", default="missing")
    parser.add_argument("--deny-tool", default="missing")
    parser.add_argument("--max-high", type=int, default=0)
    parser.add_argument("--max-critical", type=int, default=0)
    args = parser.parse_args()

    out_dir = args.out_dir
    out_dir.mkdir(parents=True, exist_ok=True)
    metadata = read_json(args.metadata, {})
    lock_digest = sha256_file(args.lockfile)
    version_path = args.lockfile.parent / "VERSION"
    version = version_path.read_text(encoding="utf-8").strip() if version_path.is_file() else "0.0.0"
    cyclonedx, spdx_data = make_sbom(metadata, lock_digest, args.source_revision, version)
    (out_dir / "sbom.cdx.json").write_text(json.dumps(cyclonedx, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "sbom.spdx.json").write_text(json.dumps(spdx_data["spdx"], indent=2, sort_keys=True) + "\n", encoding="utf-8")

    advisory = normalize_advisories(args.audit_json)
    lock_drift = args.lock_before != args.lock_after or args.lock_dirty.lower() == "true"
    expected_mismatch = bool(args.expected_lock_digest and args.expected_lock_digest != lock_digest)
    high_count = advisory["severity_counts"].get("high", 0)
    critical_count = advisory["severity_counts"].get("critical", 0)
    reasons: list[dict[str, str]] = []
    if lock_drift:
        reasons.append({"code": "lockfile_drift", "detail": "Cargo.lock changed during metadata resolution or is dirty in the checkout"})
    if expected_mismatch:
        reasons.append({"code": "lockfile_digest_mismatch", "detail": "Cargo.lock does not match SC28_EXPECTED_LOCKFILE_SHA256"})
    if spdx_data["unknown_license"]:
        reasons.append({"code": "license_unknown", "detail": f"{len(spdx_data['unknown_license'])} package(s) have no license metadata"})
    if args.audit_tool == "missing":
        reasons.append({"code": "advisory_scanner_missing", "detail": "cargo-audit is required in the CI gate"})
    elif args.audit_exit != 0:
        reasons.append({"code": "advisory_scan_failed", "detail": f"cargo audit exited {args.audit_exit}"})
    if advisory["count"] > 0:
        reasons.append({"code": "advisory_found", "detail": f"cargo-audit reported {advisory['count']} vulnerable package(s)"})
    if critical_count > args.max_critical:
        reasons.append({"code": "critical_threshold_exceeded", "detail": f"critical={critical_count}, allowed={args.max_critical}"})
    if high_count > args.max_high:
        reasons.append({"code": "high_threshold_exceeded", "detail": f"high={high_count}, allowed={args.max_high}"})
    if args.deny_tool == "missing":
        reasons.append({"code": "policy_scanner_missing", "detail": "cargo-deny is required in the CI gate"})
    elif args.deny_exit != 0:
        reasons.append({"code": "dependency_policy_failed", "detail": f"cargo deny exited {args.deny_exit}"})

    status = "quarantined" if reasons else "passed"
    report = {
        "schema": SCHEMA,
        "status": status,
        "source_revision": args.source_revision,
        "lockfile": {
            "path": str(args.lockfile),
            "sha256": lock_digest,
            "before_sha256": args.lock_before,
            "after_sha256": args.lock_after,
            "dirty": args.lock_dirty.lower() == "true",
            "expected_sha256": args.expected_lock_digest or None,
        },
        "sbom": {"cyclonedx": "sbom.cdx.json", "spdx": "sbom.spdx.json"},
        "license": {"unknown_count": len(spdx_data["unknown_license"]), "unknown": spdx_data["unknown_license"][:100]},
        "advisory": {
            "tool": args.audit_tool,
            "exit_code": args.audit_exit,
            "count": advisory["count"],
            "severity_counts": advisory["severity_counts"],
            "max_high": args.max_high,
            "max_critical": args.max_critical,
            "findings": advisory["findings"],
        },
        "dependency_policy": {"tool": args.deny_tool, "exit_code": args.deny_exit, "config": "deny.toml"},
        "quarantine": {"status": "quarantined" if reasons else "clear", "reasons": reasons},
        "limitations": [
            "CI scanner output is source and dependency metadata evidence; it is not a signed release provenance proof.",
            "A clean local fixture cannot prove production artifact, registry, signer or runtime effect integrity.",
        ],
    }
    (out_dir / "supply-chain-report.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    quarantine = {
        "schema": QUARANTINE_SCHEMA,
        "status": report["quarantine"]["status"],
        "source_revision": args.source_revision,
        "lockfile_sha256": lock_digest,
        "reasons": reasons,
        "automatic_release_allowed": not reasons,
    }
    (out_dir / "quarantine.json").write_text(json.dumps(quarantine, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"status": status, "report": str(out_dir / "supply-chain-report.json"), "quarantine_reasons": reasons}, sort_keys=True))
    return 1 if reasons else 0


if __name__ == "__main__":
    raise SystemExit(main())
