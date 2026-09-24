#!/usr/bin/env python3
"""Verify a release manifest, SLSA-style provenance and signature receipt.

The verifier binds the exact bytes named by the manifest to the manifest digest, then binds that
digest to provenance, an externally-produced signature verification receipt and a transparency
log entry. It never signs, publishes, downloads, or treats a filename/tag as authority.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tempfile
from pathlib import Path
from typing import Any


DIGEST_PREFIX = "sha256:"
VERSION = {"major": 1, "minor": 0}


class VerificationError(Exception):
    pass


def fail(reason: str) -> None:
    raise VerificationError(reason)


def digest_bytes(value: bytes) -> str:
    return DIGEST_PREFIX + hashlib.sha256(value).hexdigest()


def digest_value(value: Any) -> str:
    return digest_bytes(
        json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    )


def require_digest(value: Any, field: str) -> str:
    if not isinstance(value, str) or len(value) != len(DIGEST_PREFIX) + 64:
        fail(f"{field}_invalid")
    if not value.startswith(DIGEST_PREFIX) or any(c not in "0123456789abcdef" for c in value[7:]):
        fail(f"{field}_invalid")
    return value


def require_text(value: Any, field: str, maximum: int = 512) -> str:
    if not isinstance(value, str) or not value.strip() or len(value) > maximum:
        fail(f"{field}_invalid")
    if any(ch in value for ch in "\x00\r\n"):
        fail(f"{field}_invalid")
    return value


def strict_object(value: Any, allowed: set[str], field: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        fail(f"{field}_object_invalid")
    unknown = set(value) - allowed
    if unknown:
        fail(f"{field}_unknown_field")
    return value


def read_json(path: Path, field: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        fail(f"{field}_read_failed:{error}")
    return value


def verify_manifest(manifest: dict[str, Any]) -> None:
    allowed = {
        "schema",
        "version",
        "release_id",
        "tag",
        "source_revision",
        "source_tree_digest",
        "cargo_lock_digest",
        "toolchain_digest",
        "builder_id",
        "artifacts",
        "sbom_digest",
        "manifest_digest",
    }
    strict_object(manifest, allowed, "release_manifest")
    if manifest.get("schema") != "kiana.release-manifest.v1" or manifest.get("version") != VERSION:
        fail("release_manifest_header_invalid")
    require_text(manifest.get("release_id"), "release_manifest_release_id")
    tag = require_text(manifest.get("tag"), "release_manifest_tag")
    if not tag.startswith("v") or any(ch in tag for ch in "/\\ "):
        fail("release_manifest_tag_invalid")
    require_text(manifest.get("source_revision"), "release_manifest_source_revision")
    for key in ("source_tree_digest", "cargo_lock_digest", "toolchain_digest", "sbom_digest"):
        require_digest(manifest.get(key), f"release_manifest_{key}")
    require_text(manifest.get("builder_id"), "release_manifest_builder_id")
    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, list) or not artifacts or len(artifacts) > 128:
        fail("release_manifest_artifact_count_invalid")
    names: set[str] = set()
    for artifact in artifacts:
        strict_object(
            artifact,
            {"name", "kind", "target", "digest", "size_bytes", "media_type"},
            "release_artifact",
        )
        name = require_text(artifact.get("name"), "release_artifact_name")
        if name in {".", ".."} or "/" in name or "\\" in name:
            fail("release_artifact_name_invalid")
        if name in names:
            fail("release_manifest_artifact_duplicate")
        names.add(name)
        require_text(artifact.get("kind"), "release_artifact_kind")
        require_text(artifact.get("target"), "release_artifact_target")
        require_digest(artifact.get("digest"), "release_artifact_digest")
        if not isinstance(artifact.get("size_bytes"), int) or artifact["size_bytes"] < 0:
            fail("release_artifact_size_invalid")
        require_text(artifact.get("media_type"), "release_artifact_media_type")
    expected = dict(manifest)
    manifest_digest = require_digest(manifest.get("manifest_digest"), "release_manifest_digest")
    expected.pop("manifest_digest", None)
    if manifest_digest != digest_value(expected):
        fail("release_manifest_digest_mismatch")


def verify_provenance(provenance: dict[str, Any], manifest: dict[str, Any]) -> None:
    allowed = {
        "schema",
        "version",
        "build_type",
        "builder_id",
        "invocation_digest",
        "source_revision",
        "source_tree_digest",
        "cargo_lock_digest",
        "toolchain_digest",
        "materials",
        "subject_manifest_digest",
        "provenance_digest",
    }
    strict_object(provenance, allowed, "release_provenance")
    if provenance.get("schema") != "kiana.release-provenance.v1" or provenance.get("version") != VERSION:
        fail("release_provenance_header_invalid")
    build_type = require_text(provenance.get("build_type"), "release_provenance_build_type")
    if not build_type.startswith("https://slsa.dev/"):
        fail("release_provenance_build_type_invalid")
    for key in ("builder_id", "source_revision"):
        require_text(provenance.get(key), f"release_provenance_{key}")
    for key in ("invocation_digest", "source_tree_digest", "cargo_lock_digest", "toolchain_digest", "subject_manifest_digest"):
        require_digest(provenance.get(key), f"release_provenance_{key}")
    materials = provenance.get("materials")
    if not isinstance(materials, list) or not materials or len(materials) > 128:
        fail("release_provenance_material_count_invalid")
    uris: set[str] = set()
    for material in materials:
        strict_object(material, {"uri", "digest"}, "release_material")
        uri = require_text(material.get("uri"), "release_material_uri")
        require_digest(material.get("digest"), "release_material_digest")
        if uri in uris:
            fail("release_provenance_material_duplicate")
        uris.add(uri)
    expected = dict(provenance)
    provenance_digest = require_digest(provenance.get("provenance_digest"), "release_provenance_digest")
    expected.pop("provenance_digest", None)
    if provenance_digest != digest_value(expected):
        fail("release_provenance_digest_mismatch")
    if provenance["subject_manifest_digest"] != manifest["manifest_digest"]:
        fail("release_verification_provenance_subject_mismatch")
    for key in ("source_tree_digest", "cargo_lock_digest", "toolchain_digest"):
        if provenance[key] != manifest[key]:
            fail(f"release_verification_{key}_mismatch")


def verify_signature(signature: dict[str, Any], manifest: dict[str, Any]) -> str:
    allowed = {
        "schema",
        "version",
        "algorithm",
        "signer_id",
        "key_id",
        "subject_manifest_digest",
        "signature_digest",
        "transparency_log_entry_digest",
        "verifier_id",
        "verified",
        "attestation_digest",
    }
    strict_object(signature, allowed, "release_signature")
    if signature.get("schema") != "kiana.release-signature-attestation.v1" or signature.get("version") != VERSION:
        fail("release_signature_header_invalid")
    if signature.get("algorithm") not in {"ed25519", "sigstore", "external"}:
        fail("release_signature_algorithm_invalid")
    for key in ("signer_id", "key_id", "verifier_id"):
        require_text(signature.get(key), f"release_signature_{key}")
    for key in ("subject_manifest_digest", "signature_digest"):
        require_digest(signature.get(key), f"release_signature_{key}")
    transparency = signature.get("transparency_log_entry_digest")
    if transparency:
        require_digest(transparency, "release_signature_transparency_digest")
    else:
        fail("release_verification_transparency_missing")
    if not isinstance(signature.get("verified"), bool):
        fail("release_signature_verified_invalid")
    expected = dict(signature)
    attestation_digest = require_digest(signature.get("attestation_digest"), "release_signature_attestation_digest")
    expected.pop("attestation_digest", None)
    if attestation_digest != digest_value(expected):
        fail("release_signature_attestation_digest_mismatch")
    if signature["subject_manifest_digest"] != manifest["manifest_digest"]:
        fail("release_verification_signature_subject_mismatch")
    if not signature["verified"]:
        fail("release_verification_signature_unverified")
    return attestation_digest


def verify(args: argparse.Namespace) -> None:
    manifest = read_json(Path(args.manifest), "release_manifest")
    provenance = read_json(Path(args.provenance), "release_provenance")
    signature = read_json(Path(args.signature), "release_signature")
    verify_manifest(manifest)
    if manifest["tag"] != args.expected_tag:
        fail("release_verification_tag_mismatch")
    if manifest["source_revision"] != args.expected_source_revision:
        fail("release_verification_source_revision_mismatch")
    if manifest["builder_id"] != args.expected_builder_id:
        fail("release_verification_builder_mismatch")
    if manifest["toolchain_digest"] != args.expected_toolchain_digest:
        fail("release_verification_toolchain_mismatch")
    verify_provenance(provenance, manifest)
    if provenance["source_revision"] != args.expected_source_revision:
        fail("release_verification_source_revision_mismatch")
    if provenance["builder_id"] != args.expected_builder_id:
        fail("release_verification_builder_mismatch")
    if provenance["toolchain_digest"] != args.expected_toolchain_digest:
        fail("release_verification_toolchain_mismatch")
    verify_signature(signature, manifest)
    for item in args.artifact:
        name, _, path_value = item.partition("=")
        if not name or not path_value:
            fail("release_verification_artifact_argument_invalid")
        file_path = Path(path_value)
        if not file_path.is_file():
            fail("release_verification_artifact_missing")
        matched = next((row for row in manifest["artifacts"] if row["name"] == name), None)
        if matched is None:
            fail("release_verification_artifact_name_mismatch")
        data = file_path.read_bytes()
        if matched["size_bytes"] != len(data) or matched["digest"] != digest_bytes(data):
            fail("release_verification_artifact_digest_mismatch")
    print(
        json.dumps(
            {
                "schema": "kiana.release-verification.v1",
                "status": "verified",
                "manifest_digest": manifest["manifest_digest"],
                "provenance_digest": provenance["provenance_digest"],
                "signature_attestation_digest": signature["attestation_digest"],
            },
            sort_keys=True,
        )
    )


def self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="kiana-sc29-") as directory:
        root = Path(directory)
        artifact = b"SC-29 fixture artifact\n"
        artifact_digest = digest_bytes(artifact)
        common = {
            "schema": "kiana.release-manifest.v1",
            "version": VERSION,
            "release_id": "fixture-release",
            "tag": "v1.2.3",
            "source_revision": "git:fixture-source",
            "source_tree_digest": "sha256:" + "a" * 64,
            "cargo_lock_digest": "sha256:" + "b" * 64,
            "toolchain_digest": "sha256:" + "d" * 64,
            "builder_id": "builder://github-actions",
            "artifacts": [
                {
                    "name": "fixture.tar.gz",
                    "kind": "archive",
                    "target": "x86_64-unknown-linux-gnu",
                    "digest": artifact_digest,
                    "size_bytes": len(artifact),
                    "media_type": "application/gzip",
                }
            ],
            "sbom_digest": "sha256:" + "c" * 64,
        }
        common["manifest_digest"] = digest_value(common)
        provenance = {
            "schema": "kiana.release-provenance.v1",
            "version": VERSION,
            "build_type": "https://slsa.dev/provenance/v1",
            "builder_id": common["builder_id"],
            "invocation_digest": common["cargo_lock_digest"],
            "source_revision": common["source_revision"],
            "source_tree_digest": common["source_tree_digest"],
            "cargo_lock_digest": common["cargo_lock_digest"],
            "toolchain_digest": common["toolchain_digest"],
            "materials": [{"uri": "git:fixture", "digest": common["source_tree_digest"]}],
            "subject_manifest_digest": common["manifest_digest"],
        }
        provenance["provenance_digest"] = digest_value(provenance)
        signature = {
            "schema": "kiana.release-signature-attestation.v1",
            "version": VERSION,
            "algorithm": "sigstore",
            "signer_id": "fixture-signer",
            "key_id": "fixture-key",
            "subject_manifest_digest": common["manifest_digest"],
            "signature_digest": "sha256:" + "e" * 64,
            "transparency_log_entry_digest": "sha256:" + "f" * 64,
            "verifier_id": "verifier://ci",
            "verified": True,
        }
        signature["attestation_digest"] = digest_value(signature)
        for name, value in (("manifest", common), ("provenance", provenance), ("signature", signature)):
            (root / f"{name}.json").write_text(json.dumps(value), encoding="utf-8")
        (root / "fixture.tar.gz").write_bytes(artifact)
        args = argparse.Namespace(
            manifest=root / "manifest.json",
            provenance=root / "provenance.json",
            signature=root / "signature.json",
            expected_tag="v1.2.3",
            expected_source_revision="git:fixture-source",
            expected_builder_id="builder://github-actions",
            expected_toolchain_digest=common["toolchain_digest"],
            artifact=[f"fixture.tar.gz={root / 'fixture.tar.gz'}"],
        )
        verify(args)
        forged = dict(common)
        forged["tag"] = "v9.9.9"
        forged["manifest_digest"] = digest_value(forged)
        (root / "manifest.json").write_text(json.dumps(forged), encoding="utf-8")
        try:
            verify(args)
        except VerificationError as error:
            if str(error) != "release_verification_tag_mismatch":
                raise
        else:
            fail("self_test_forged_tag_accepted")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest")
    parser.add_argument("--provenance")
    parser.add_argument("--signature")
    parser.add_argument("--expected-tag")
    parser.add_argument("--expected-source-revision")
    parser.add_argument("--expected-builder-id")
    parser.add_argument("--expected-toolchain-digest")
    parser.add_argument("--artifact", action="append", default=[])
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    try:
        if args.self_test:
            self_test()
        elif not all(
            getattr(args, key)
            for key in (
                "manifest",
                "provenance",
                "signature",
                "expected_tag",
                "expected_source_revision",
                "expected_builder_id",
                "expected_toolchain_digest",
            )
        ):
            parser.error("manifest, provenance, signature and expected binding flags are required")
        else:
            verify(args)
    except VerificationError as error:
        print(f"release provenance verification failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
