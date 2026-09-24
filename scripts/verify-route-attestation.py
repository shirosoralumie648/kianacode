#!/usr/bin/env python3
"""Offline verifier for SC-30 provider/model/prompt/MCP route attestations.

The verifier consumes metadata only.  It does not resolve credentials, call a provider, launch
an MCP server, or treat a provider supplied route claim as authority.  The expected bindings are
server-owned command-line inputs and must be compared again at the effect boundary.
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


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        fail(f"route_attestation_read_failed:{error}")
    return value


def verify_route_shape(route: dict[str, Any]) -> str:
    strict_object(
        route,
        {
            "provider_id",
            "protocol",
            "connection_id",
            "model_id",
            "profile",
            "configuration_revision",
            "streaming",
        },
        "route",
    )
    for key in ("provider_id", "protocol", "connection_id", "model_id", "profile", "configuration_revision"):
        require_text(route.get(key), f"route_{key}")
    if route["protocol"] not in {
        "legacy",
        "anthropic_messages",
        "open_ai_chat",
        "open_ai_responses",
        "ollama_chat",
        "gemini_interactions",
    }:
        fail("route_protocol_invalid")
    if not isinstance(route.get("streaming"), bool):
        fail("route_streaming_invalid")
    return digest_value(
        {
            "provider_id": route["provider_id"],
            "protocol": route["protocol"],
            "model_id": route["model_id"],
            "profile": route["profile"],
            "configuration_revision": route["configuration_revision"],
            "streaming": route["streaming"],
        }
    )


def verify_prompt(prompt: dict[str, Any]) -> None:
    strict_object(
        prompt,
        {"schema", "version", "pack_id", "pack_version", "content_digest", "provenance_digest", "trust"},
        "route_prompt",
    )
    if prompt.get("schema") != "kiana.route-attestation-prompt.v1" or prompt.get("version") != VERSION:
        fail("route_prompt_header_invalid")
    require_text(prompt.get("pack_id"), "route_prompt_pack_id")
    require_text(prompt.get("pack_version"), "route_prompt_pack_version")
    require_digest(prompt.get("content_digest"), "route_prompt_content_digest")
    require_digest(prompt.get("provenance_digest"), "route_prompt_provenance_digest")
    if prompt.get("trust") not in {"product", "signed", "untrusted", "unknown", "revoked"}:
        fail("route_prompt_trust_invalid")


def verify_mcp(mcp: dict[str, Any]) -> str:
    strict_object(
        mcp,
        {
            "schema",
            "version",
            "server_id",
            "transport",
            "tool_catalog_digest",
            "credential_audience",
            "network_audience",
            "enabled",
            "route_digest",
        },
        "route_mcp",
    )
    if mcp.get("schema") != "kiana.route-attestation-mcp.v1" or mcp.get("version") != VERSION:
        fail("route_mcp_header_invalid")
    require_text(mcp.get("server_id"), "route_mcp_server")
    if mcp.get("transport") not in {"stdio", "http"}:
        fail("route_mcp_transport_invalid")
    require_digest(mcp.get("tool_catalog_digest"), "route_mcp_tool_catalog_digest")
    require_text(mcp.get("credential_audience"), "route_mcp_credential_audience")
    require_text(mcp.get("network_audience"), "route_mcp_network_audience")
    if not isinstance(mcp.get("enabled"), bool):
        fail("route_mcp_enabled_invalid")
    route_digest = require_digest(mcp.get("route_digest"), "route_mcp_digest")
    expected = dict(mcp)
    expected.pop("route_digest", None)
    if route_digest != digest_value(expected):
        fail("route_mcp_digest_mismatch")
    return route_digest


def verify_policy(policy: dict[str, Any]) -> None:
    strict_object(
        policy,
        {
            "schema",
            "version",
            "policy_digest",
            "policy_revision",
            "data_epoch",
            "purpose",
            "allowed_classes",
            "use_policy_digest",
        },
        "route_policy",
    )
    if policy.get("schema") != "kiana.route-attestation-policy.v1" or policy.get("version") != VERSION:
        fail("route_policy_header_invalid")
    require_digest(policy.get("policy_digest"), "route_policy_digest")
    if not isinstance(policy.get("policy_revision"), int) or policy["policy_revision"] < 0:
        fail("route_policy_revision_invalid")
    if not isinstance(policy.get("data_epoch"), int) or policy["data_epoch"] == 0:
        fail("route_policy_epoch_invalid")
    require_text(policy.get("purpose"), "route_policy_purpose")
    classes = policy.get("allowed_classes")
    if not isinstance(classes, list) or len(classes) > 4 or len(set(classes)) != len(classes):
        fail("route_policy_scope_invalid")
    if any(value not in {"public", "internal", "confidential", "restricted"} for value in classes):
        fail("route_policy_data_class_invalid")
    require_digest(policy.get("use_policy_digest"), "route_policy_use_digest")
    expected = dict(policy)
    expected.pop("use_policy_digest", None)
    if policy["use_policy_digest"] != digest_value(expected):
        fail("route_policy_use_digest_mismatch")


def verify_claim(claim: dict[str, Any]) -> str:
    strict_object(
        claim,
        {
            "provider_id",
            "protocol",
            "connection_id",
            "model_id",
            "profile",
            "configuration_revision",
            "streaming",
            "route_digest",
        },
        "route_claim",
    )
    for key in ("provider_id", "protocol", "connection_id", "model_id", "profile", "configuration_revision"):
        require_text(claim.get(key), f"route_claim_{key}")
    if claim["protocol"] not in {
        "legacy",
        "anthropic_messages",
        "open_ai_chat",
        "open_ai_responses",
        "ollama_chat",
        "gemini_interactions",
    }:
        fail("route_claim_protocol_invalid")
    if not isinstance(claim.get("streaming"), bool):
        fail("route_claim_streaming_invalid")
    route_digest = require_digest(claim.get("route_digest"), "route_claim_digest")
    expected = {
        "provider_id": claim["provider_id"],
        "protocol": claim["protocol"],
        "model_id": claim["model_id"],
        "profile": claim["profile"],
        "configuration_revision": claim["configuration_revision"],
        "streaming": claim["streaming"],
    }
    if route_digest != digest_value(expected):
        fail("route_claim_digest_mismatch")
    return route_digest


def verify_attestation(value: dict[str, Any], expected: dict[str, Any]) -> dict[str, Any]:
    strict_object(
        value,
        {"schema", "version", "context", "provider_claim", "provider_verified", "attestation_digest"},
        "route_attestation",
    )
    if value.get("schema") != "kiana.route-attestation.v1" or value.get("version") != VERSION:
        fail("route_attestation_header_invalid")
    context = strict_object(
        value.get("context"),
        {
            "schema",
            "version",
            "route",
            "route_digest",
            "prompt_pack",
            "mcp_route",
            "data_policy",
            "credential_ref_digest",
            "account_id",
            "audience",
            "release_manifest_digest",
            "release_signature_attestation_digest",
        },
        "route_attestation_context",
    )
    if context.get("schema") != "kiana.route-attestation-context.v1" or context.get("version") != VERSION:
        fail("route_attestation_context_header_invalid")
    route_digest = verify_route_shape(context.get("route"))
    if require_digest(context.get("route_digest"), "route_attestation_route_digest") != route_digest:
        fail("route_attestation_route_digest_mismatch")
    verify_prompt(context.get("prompt_pack"))
    mcp_digest = None
    if context.get("mcp_route") is not None:
        mcp_digest = verify_mcp(context["mcp_route"])
    verify_policy(context.get("data_policy"))
    require_digest(context.get("credential_ref_digest"), "route_attestation_credential_digest")
    require_text(context.get("account_id"), "route_attestation_account")
    require_text(context.get("audience"), "route_attestation_audience")
    require_digest(context.get("release_manifest_digest"), "route_attestation_release_manifest_digest")
    require_digest(
        context.get("release_signature_attestation_digest"),
        "route_attestation_release_signature_digest",
    )
    expected_context_digest = digest_value(context)
    claim = value.get("provider_claim")
    verify_claim(claim)
    if not isinstance(value.get("provider_verified"), bool):
        fail("route_attestation_provider_verified_invalid")
    attestation_digest = require_digest(value.get("attestation_digest"), "route_attestation_digest")
    expected_attestation = dict(value)
    expected_attestation.pop("attestation_digest", None)
    if attestation_digest != digest_value(expected_attestation):
        fail("route_attestation_digest_mismatch")

    expected_context = expected
    expected_context_digest = digest_value(expected_context)
    if context != expected_context:
        reason = "route_attestation_binding_mismatch"
    elif claim != {
        "provider_id": expected_context["route"]["provider_id"],
        "protocol": expected_context["route"]["protocol"],
        "connection_id": expected_context["route"]["connection_id"],
        "model_id": expected_context["route"]["model_id"],
        "profile": expected_context["route"]["profile"],
        "configuration_revision": expected_context["route"]["configuration_revision"],
        "streaming": expected_context["route"]["streaming"],
        "route_digest": expected_context["route_digest"],
    }:
        reason = "route_attestation_provider_claim_mismatch"
    elif context["prompt_pack"]["trust"] in {"untrusted", "revoked"}:
        reason = "route_attestation_prompt_pack_untrusted"
    elif context["prompt_pack"]["trust"] == "unknown":
        reason = "route_attestation_prompt_pack_unknown"
    elif context["mcp_route"] != expected_context.get("mcp_route"):
        reason = "route_attestation_mcp_route_mismatch"
    elif context.get("mcp_route", {}).get("transport") == "http":
        reason = "route_attestation_mcp_transport_unsupported"
    elif context["data_policy"] != expected_context["data_policy"]:
        reason = "route_attestation_data_policy_mismatch"
    elif context["credential_ref_digest"] != expected_context["credential_ref_digest"]:
        reason = "route_attestation_credential_mismatch"
    elif context["account_id"] != expected_context["account_id"]:
        reason = "route_attestation_account_mismatch"
    elif context["audience"] != expected_context["audience"]:
        reason = "route_attestation_audience_mismatch"
    elif context["release_manifest_digest"] != expected_context["release_manifest_digest"] or context[
        "release_signature_attestation_digest"
    ] != expected_context["release_signature_attestation_digest"]:
        reason = "route_attestation_release_binding_mismatch"
    elif not value["provider_verified"]:
        reason = "route_attestation_provider_unverified"
    else:
        reason = "ok"

    status = "verified" if reason == "ok" else "unknown" if reason.endswith("unknown") or reason.endswith("unverified") else "blocked"
    return {
        "schema": "kiana.route-attestation-report.v1",
        "status": status,
        "reason": reason,
        "attestation_digest": attestation_digest,
        "route_digest": route_digest,
        "prompt_pack_digest": context["prompt_pack"]["content_digest"],
        "mcp_route_digest": mcp_digest,
        "data_policy_digest": context["data_policy"]["policy_digest"],
        "expected_context_digest": expected_context_digest,
    }


def fixture() -> tuple[dict[str, Any], dict[str, Any]]:
    route = {
        "provider_id": "provider-fixture",
        "protocol": "open_ai_responses",
        "connection_id": "connection-fixture",
        "model_id": "model-fixture",
        "profile": "builder",
        "configuration_revision": "config-revision-1",
        "streaming": True,
    }
    route_digest = verify_route_shape(route)
    prompt = {
        "schema": "kiana.route-attestation-prompt.v1",
        "version": VERSION,
        "pack_id": "role-builder",
        "pack_version": "1.0.0",
        "content_digest": "sha256:" + "a" * 64,
        "provenance_digest": "sha256:" + "b" * 64,
        "trust": "signed",
    }
    mcp = {
        "schema": "kiana.route-attestation-mcp.v1",
        "version": VERSION,
        "server_id": "mcp-fixture",
        "transport": "stdio",
        "tool_catalog_digest": "sha256:" + "d" * 64,
        "credential_audience": "audience:mcp-fixture",
        "network_audience": "network:mcp-fixture",
        "enabled": True,
    }
    mcp["route_digest"] = digest_value(mcp)
    policy = {
        "schema": "kiana.route-attestation-policy.v1",
        "version": VERSION,
        "policy_digest": "sha256:" + "c" * 64,
        "policy_revision": 4,
        "data_epoch": 7,
        "purpose": "code-generation",
        "allowed_classes": ["confidential", "internal"],
    }
    policy["use_policy_digest"] = digest_value(policy)
    context = {
        "schema": "kiana.route-attestation-context.v1",
        "version": VERSION,
        "route": route,
        "route_digest": route_digest,
        "prompt_pack": prompt,
        "mcp_route": mcp,
        "data_policy": policy,
        "credential_ref_digest": "sha256:" + "e" * 64,
        "account_id": "account-fixture",
        "audience": "audience:provider-fixture",
        "release_manifest_digest": "sha256:" + "a" * 64,
        "release_signature_attestation_digest": "sha256:" + "f" * 64,
    }
    claim = dict(route)
    claim["route_digest"] = route_digest
    attestation = {
        "schema": "kiana.route-attestation.v1",
        "version": VERSION,
        "context": context,
        "provider_claim": claim,
        "provider_verified": True,
    }
    attestation["attestation_digest"] = digest_value(attestation)
    return attestation, context


def self_test() -> None:
    attestation, expected = fixture()
    report = verify_attestation(attestation, expected)
    if report["status"] != "verified":
        fail("self_test_valid_route_not_verified")

    forged = json.loads(json.dumps(attestation))
    forged["provider_claim"]["model_id"] = "forged-model"
    forged["provider_claim"]["route_digest"] = digest_value(
        {
            "provider_id": forged["provider_claim"]["provider_id"],
            "protocol": forged["provider_claim"]["protocol"],
            "model_id": forged["provider_claim"]["model_id"],
            "profile": forged["provider_claim"]["profile"],
            "configuration_revision": forged["provider_claim"]["configuration_revision"],
            "streaming": forged["provider_claim"]["streaming"],
        }
    )
    forged["attestation_digest"] = digest_value(
        {key: value for key, value in forged.items() if key != "attestation_digest"}
    )
    report = verify_attestation(forged, expected)
    if report["reason"] != "route_attestation_provider_claim_mismatch":
        fail("self_test_forged_provider_claim_accepted")

    unknown = json.loads(json.dumps(attestation))
    unknown["context"]["prompt_pack"]["trust"] = "unknown"
    unknown["attestation_digest"] = digest_value(
        {key: value for key, value in unknown.items() if key != "attestation_digest"}
    )
    unknown_expected = json.loads(json.dumps(unknown["context"]))
    report = verify_attestation(unknown, unknown_expected)
    if report["status"] != "unknown":
        fail("self_test_unknown_prompt_not_preserved")

    with tempfile.TemporaryDirectory(prefix="kiana-sc30-") as directory:
        path = Path(directory) / "attestation.json"
        path.write_text(json.dumps(attestation), encoding="utf-8")
        verify_attestation(read_json(path), expected)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--attestation")
    parser.add_argument("--expected")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    try:
        if args.self_test:
            self_test()
        elif not args.attestation or not args.expected:
            parser.error("--attestation and --expected are required")
        else:
            report = verify_attestation(read_json(Path(args.attestation)), read_json(Path(args.expected)))
            print(json.dumps(report, sort_keys=True))
    except VerificationError as error:
        print(f"route attestation verification failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
