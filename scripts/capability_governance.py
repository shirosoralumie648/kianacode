#!/usr/bin/env python3
"""Fail-closed capability-governance validation and drift primitives.

This module is the production semantic authority for the Phase 1 governance
contracts.  Callers may render or test the result, but must not reimplement the
rules below.
"""

from __future__ import annotations

import base64
import binascii
import copy
import hashlib
import importlib.util
import json
import os
import re
import subprocess
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath
from typing import Any, Iterable, Iterator, Mapping, Sequence


VALIDATION_SCHEMA = "kiana.capability-governance-validation.v1"
MAX_JSON_BYTES = 16 * 1024 * 1024
MAX_COLLECTION_ITEMS = 50_000
MAX_TOTAL_NODES = 500_000
MAX_NESTING_DEPTH = 64
MAX_DIAGNOSTIC_CHARS = 512

PROOF_RANK = {
    "none": 0,
    "source": 1,
    "local_contract": 2,
    "local_behavior": 3,
    "target_environment": 4,
    "user_value": 5,
}

BUNDLE_KEYS = {
    "official_source_artifact",
    "public_baseline_revisions",
    "repository_registry_revisions",
    "capability_decision_revisions",
    "evidence_revisions",
    "legacy_authority_revisions",
    "bundle_manifest",
}

REVISION_FAMILIES = {
    "public_baseline_revisions": "public_baseline",
    "repository_registry_revisions": "repository_registry",
    "capability_decision_revisions": "capability_decisions",
    "evidence_revisions": "evidence_head",
    "legacy_authority_revisions": "legacy_authority",
}

SCHEMA_PATHS = {
    "kiana.official-source-artifact.v1": "docs/schemas/kiana-official-source-artifact.v1.schema.json",
    "kiana.public-parity-baseline.v1": "docs/schemas/kiana-public-parity-baseline.v1.schema.json",
    "kiana.reference-repository-registry.v1": "docs/schemas/kiana-reference-repository-registry.v1.schema.json",
    "kiana.capability-decisions.v1": "docs/schemas/kiana-capability-decisions.v1.schema.json",
    "kiana.capability-evidence-index.v1": "docs/schemas/kiana-capability-evidence-index.v1.schema.json",
    "kiana.legacy-authority-classification.v1": "docs/schemas/kiana-legacy-authority-classification.v1.schema.json",
    "kiana.capability-governance-bundle.v1": "docs/schemas/kiana-capability-governance-bundle.v1.schema.json",
}

_ALIAS_KEYS = {
    "kind",
    "value",
    "change_kind",
    "reason",
    "effective_at",
    "evidence_ids",
    "review_revision",
}
_ALIAS_KINDS = {"repo_id", "path"}
_ALIAS_CHANGE_KINDS = {"renamed", "removed", "replaced"}
_PREDECESSOR_KEYS = {
    "previous_revision_id",
    "previous_revision_path",
    "previous_revision_sha256",
}
_PROHIBITED_COMPLETION_KEYS = {
    "complete",
    "completion",
    "completion_status",
    "product_complete",
}
_SECRET_PATTERNS = (
    re.compile(r"AKIA[0-9A-Z]{16}"),
    re.compile(r"BEGIN [A-Z ]*PRIVATE KEY", re.IGNORECASE),
    re.compile(r"Bearer\s+[A-Za-z0-9._-]+", re.IGNORECASE),
    re.compile(r"\bsk-[A-Za-z0-9]{12,}"),
)
_UNSAFE_MARKUP_PATTERNS = (
    re.compile(r"<\s*(?:script|iframe|object|embed)\b", re.IGNORECASE),
    re.compile(r"javascript\s*:", re.IGNORECASE),
    re.compile(r"\bon[a-z]+\s*=", re.IGNORECASE),
)


@dataclass(frozen=True)
class GovernanceError:
    """Stable machine-readable semantic diagnostic."""

    code: str
    subject_id: str = ""
    path: str = ""
    detail: str = ""
    freshness: str | None = None

    def as_dict(self) -> dict[str, Any]:
        return {
            "code": self.code,
            "subject_id": _sanitize_text(self.subject_id),
            "path": _sanitize_path_for_output(self.path),
            "detail": _sanitize_text(self.detail),
            "freshness": self.freshness,
        }


class GovernanceUsageError(Exception):
    """A CLI usage or top-level read failure (exit status 2)."""


def canonical_json_bytes(value: Any) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")


def canonical_sha256(value: Any) -> str:
    return hashlib.sha256(canonical_json_bytes(value)).hexdigest()


def record_sha256(record: Mapping[str, Any]) -> str:
    return canonical_sha256(
        {key: value for key, value in record.items() if key != "record_sha256"}
    )


def _sanitize_text(value: Any) -> str:
    text = str(value or "")
    home = str(Path.home())
    if home:
        text = text.replace(home, "<home>")
    text = re.sub(r"(?<![A-Za-z0-9_.-])/(?:home|Users)/[^\s'\"]+", "<absolute-path>", text)
    text = re.sub(r"(?<![A-Za-z0-9_.-])/(?:tmp|var/tmp)/[^\s'\"]+", "<temporary-path>", text)
    text = re.sub(r"[A-Za-z]:\\[^\s'\"]+", "<absolute-path>", text)
    for pattern in _SECRET_PATTERNS:
        text = pattern.sub("<redacted>", text)
    text = "".join(char if char in "\n\r\t" or ord(char) >= 0x20 else "?" for char in text)
    if len(text) > MAX_DIAGNOSTIC_CHARS:
        text = text[: MAX_DIAGNOSTIC_CHARS - 3] + "..."
    return text


def _sanitize_path_for_output(value: Any) -> str:
    text = _sanitize_text(value)
    try:
        path_part = text.split("#", 1)[0]
        if path_part and (Path(path_part).is_absolute() or re.match(r"^[A-Za-z]:", path_part)):
            return "<absolute-path>"
    except (OSError, ValueError):
        return "<invalid-path>"
    return text


def sorted_errors(errors: Iterable[GovernanceError]) -> list[GovernanceError]:
    unique = {
        (error.code, error.subject_id, error.path, error.detail, error.freshness): error
        for error in errors
    }
    return sorted(
        unique.values(),
        key=lambda item: (item.code, item.subject_id, item.path, item.detail),
    )


def _error(
    errors: list[GovernanceError],
    code: str,
    subject_id: Any = "",
    path: Any = "",
    detail: Any = "",
    freshness: str | None = None,
) -> None:
    errors.append(
        GovernanceError(
            code=code,
            subject_id=str(subject_id or ""),
            path=str(path or ""),
            detail=str(detail or ""),
            freshness=freshness,
        )
    )


def _repo_root(start: Path | None = None) -> Path:
    candidate = (start or Path.cwd()).resolve()
    result = subprocess.run(
        ["git", "-C", str(candidate), "rev-parse", "--show-toplevel"],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode == 0 and result.stdout.strip():
        return Path(result.stdout.strip()).resolve()
    return candidate


def _bounded_read(path: Path, max_bytes: int = MAX_JSON_BYTES) -> bytes:
    try:
        stat = path.stat()
    except OSError as exc:
        raise GovernanceUsageError(f"read_failed: {_sanitize_text(exc)}") from exc
    if not path.is_file():
        raise GovernanceUsageError(f"read_failed: {_sanitize_path_for_output(path)} is not a file")
    if stat.st_size > max_bytes:
        raise GovernanceUsageError(
            f"input_too_large: {_sanitize_path_for_output(path)} exceeds {max_bytes} bytes"
        )
    try:
        return path.read_bytes()
    except OSError as exc:
        raise GovernanceUsageError(f"read_failed: {_sanitize_text(exc)}") from exc


def load_json(path: Path, max_bytes: int = MAX_JSON_BYTES) -> Any:
    payload = _bounded_read(path, max_bytes)
    try:
        text = payload.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise GovernanceUsageError(
            f"utf8_required: {_sanitize_path_for_output(path)}"
        ) from exc
    try:
        value = json.loads(text)
    except json.JSONDecodeError as exc:
        raise GovernanceUsageError(
            f"json_invalid: {_sanitize_path_for_output(path)}:{exc.lineno}:{exc.colno}"
        ) from exc
    _enforce_bounds(value)
    return value


def _enforce_bounds(value: Any) -> None:
    nodes = 0

    def walk(current: Any, depth: int) -> None:
        nonlocal nodes
        nodes += 1
        if nodes > MAX_TOTAL_NODES:
            raise GovernanceUsageError("input_too_large: JSON node limit exceeded")
        if depth > MAX_NESTING_DEPTH:
            raise GovernanceUsageError("input_too_deep: JSON nesting limit exceeded")
        if isinstance(current, dict):
            if len(current) > MAX_COLLECTION_ITEMS:
                raise GovernanceUsageError("input_too_large: object member limit exceeded")
            for key, child in current.items():
                if not isinstance(key, str):
                    raise GovernanceUsageError("json_invalid: object keys must be strings")
                walk(child, depth + 1)
        elif isinstance(current, list):
            if len(current) > MAX_COLLECTION_ITEMS:
                raise GovernanceUsageError("input_too_large: array item limit exceeded")
            for child in current:
                walk(child, depth + 1)
        elif isinstance(current, str) and len(current.encode("utf-8")) > MAX_JSON_BYTES:
            raise GovernanceUsageError("input_too_large: string limit exceeded")

    walk(value, 0)


def _split_binding_path(raw: str) -> tuple[str, str | None]:
    file_part, marker, fragment = raw.partition("#")
    return file_part, fragment if marker else None


def validate_relative_path(raw: Any) -> bool:
    if not isinstance(raw, str) or not raw or "\x00" in raw:
        return False
    file_part, _ = _split_binding_path(raw)
    if not file_part or "\\" in file_part or re.match(r"^[A-Za-z]:", file_part):
        return False
    path = PurePosixPath(file_part)
    return not path.is_absolute() and ".." not in path.parts and "." not in path.parts


def resolve_repository_path(root: Path, raw: str, *, must_exist: bool = True) -> Path:
    if not validate_relative_path(raw):
        raise GovernanceUsageError(f"unsafe_path: {_sanitize_path_for_output(raw)}")
    file_part, _ = _split_binding_path(raw)
    root = root.resolve()
    candidate = root.joinpath(*PurePosixPath(file_part).parts)
    try:
        resolved = candidate.resolve(strict=must_exist)
    except OSError as exc:
        raise GovernanceUsageError(f"read_failed: {_sanitize_text(exc)}") from exc
    try:
        resolved.relative_to(root)
    except ValueError as exc:
        raise GovernanceUsageError(
            f"symlink_escape: {_sanitize_path_for_output(raw)}"
        ) from exc
    return resolved


def _json_fragment(value: Any, fragment: str | None) -> Any:
    if not fragment:
        return value
    current = value
    for raw_part in fragment.strip("/").split("/"):
        part = raw_part.replace("~1", "/").replace("~0", "~")
        if isinstance(current, list):
            if part == "current":
                current = current[-1]
            elif part == "genesis":
                current = current[0]
            else:
                try:
                    current = current[int(part)]
                except (ValueError, IndexError) as exc:
                    raise GovernanceUsageError(f"binding_fragment_invalid: {fragment}") from exc
        elif isinstance(current, dict) and part in current:
            current = current[part]
        else:
            raise GovernanceUsageError(f"binding_fragment_invalid: {fragment}")
    return current


def load_bound_json(root: Path, raw: str) -> Any:
    file_part, fragment = _split_binding_path(raw)
    path = resolve_repository_path(root, file_part)
    return _json_fragment(load_json(path), fragment)


def _json_pointer_parent(document: Any, pointer: str) -> tuple[Any, str]:
    if not isinstance(pointer, str) or not pointer.startswith("/"):
        raise GovernanceUsageError("mutation_path_invalid: expected JSON pointer")
    tokens = [token.replace("~1", "/").replace("~0", "~") for token in pointer[1:].split("/")]
    current = document
    for token in tokens[:-1]:
        if isinstance(current, list):
            try:
                current = current[int(token)]
            except (ValueError, IndexError) as exc:
                raise GovernanceUsageError(f"mutation_path_invalid: {pointer}") from exc
        elif isinstance(current, dict) and token in current:
            current = current[token]
        else:
            raise GovernanceUsageError(f"mutation_path_invalid: {pointer}")
    return current, tokens[-1]


def apply_semantic_mutations(document: Any, mutations: Sequence[Mapping[str, Any]]) -> Any:
    """Apply bounded RFC6902-like mutations without evaluating fixture code."""

    result = copy.deepcopy(document)
    if len(mutations) > MAX_COLLECTION_ITEMS:
        raise GovernanceUsageError("input_too_large: mutation limit exceeded")
    for mutation in mutations:
        if not isinstance(mutation, Mapping):
            raise GovernanceUsageError("mutation_invalid: mutation must be an object")
        operation = mutation.get("op")
        pointer = mutation.get("path")
        parent, token = _json_pointer_parent(result, pointer)
        if operation in {"add", "replace", "set"}:
            if "value" not in mutation:
                raise GovernanceUsageError("mutation_invalid: value is required")
            value = copy.deepcopy(mutation["value"])
            if isinstance(parent, list):
                if token == "-":
                    parent.append(value)
                else:
                    index = int(token)
                    if operation == "add":
                        parent.insert(index, value)
                    else:
                        parent[index] = value
            elif isinstance(parent, dict):
                if operation == "replace" and token not in parent:
                    raise GovernanceUsageError(f"mutation_path_invalid: {pointer}")
                parent[token] = value
            else:
                raise GovernanceUsageError(f"mutation_path_invalid: {pointer}")
        elif operation in {"remove", "delete"}:
            if isinstance(parent, list):
                del parent[int(token)]
            elif isinstance(parent, dict) and token in parent:
                del parent[token]
            else:
                raise GovernanceUsageError(f"mutation_path_invalid: {pointer}")
        elif operation == "append":
            target = parent[int(token)] if isinstance(parent, list) else parent.get(token)
            if not isinstance(target, list) or "value" not in mutation:
                raise GovernanceUsageError("mutation_invalid: append requires an array target")
            target.append(copy.deepcopy(mutation["value"]))
        elif operation == "reverse":
            target = parent[int(token)] if isinstance(parent, list) else parent.get(token)
            if not isinstance(target, list):
                raise GovernanceUsageError("mutation_invalid: reverse requires an array target")
            target.reverse()
        else:
            raise GovernanceUsageError(f"mutation_operation_invalid: {operation}")
    _enforce_bounds(result)
    return result


def load_fixture_bundle(path: Path) -> dict[str, Any]:
    value = load_json(path)
    if not isinstance(value, dict):
        raise GovernanceUsageError("fixture_invalid: root must be an object")
    if "base_fixture" in value and "mutations" in value:
        base_raw = value["base_fixture"]
        if not isinstance(base_raw, str) or not validate_relative_path(base_raw):
            raise GovernanceUsageError("mutation_base_invalid: base_fixture must be relative")
        base_path = (path.parent / base_raw).resolve()
        try:
            base_path.relative_to(_repo_root(path.parent))
        except ValueError as exc:
            raise GovernanceUsageError("unsafe_path: mutation base escapes repository") from exc
        base = load_json(base_path)
        mutations = value["mutations"]
        if not isinstance(mutations, list):
            raise GovernanceUsageError("mutation_invalid: mutations must be an array")
        value = apply_semantic_mutations(base, mutations)
    if not isinstance(value, dict):
        raise GovernanceUsageError("fixture_invalid: resulting bundle must be an object")
    return value


def _load_structural_helper(root: Path) -> Any:
    helper_path = root / "scripts/validate-json-schema.py"
    if not helper_path.is_file():
        raise GovernanceUsageError("structural_validator_missing: scripts/validate-json-schema.py")
    spec = importlib.util.spec_from_file_location("kiana_validate_json_schema", helper_path)
    if spec is None or spec.loader is None:
        raise GovernanceUsageError("structural_validator_unavailable: validate-json-schema.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def structural_errors(instance: Any, root: Path) -> list[GovernanceError]:
    if not isinstance(instance, dict):
        return [GovernanceError("schema_validation_failed", path="$", detail="root must be an object")]
    schema_id = instance.get("schema")
    schema_rel = SCHEMA_PATHS.get(schema_id)
    if schema_rel is None:
        return [GovernanceError("schema_unknown", subject_id=schema_id or "", path="$.schema")]
    try:
        schema = load_json(root / schema_rel)
        helper = _load_structural_helper(root)
        messages = helper.validate(schema, instance, schema, "$")
    except GovernanceUsageError:
        raise
    except Exception as exc:  # structural helper failures are fail closed
        raise GovernanceUsageError(f"structural_validator_failed: {_sanitize_text(exc)}") from exc
    errors = [
        GovernanceError("schema_validation_failed", subject_id=schema_id, path="$", detail=message)
        for message in messages
    ]
    try:
        from jsonschema import Draft202012Validator, FormatChecker

        validator = Draft202012Validator(schema, format_checker=FormatChecker())
        for error in validator.iter_errors(instance):
            path = "$" + "".join(
                f"[{part}]" if isinstance(part, int) else f".{part}"
                for part in error.absolute_path
            )
            _error(errors, "schema_validation_failed", schema_id, path, error.message)
    except ImportError:
        pass
    return sorted_errors(errors)


def _iter_strings(value: Any, path: str = "$") -> Iterator[tuple[str, str]]:
    if isinstance(value, str):
        yield path, value
    elif isinstance(value, dict):
        for key, child in value.items():
            yield from _iter_strings(child, f"{path}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from _iter_strings(child, f"{path}[{index}]")


def _iter_keys(value: Any) -> Iterator[str]:
    if isinstance(value, dict):
        for key, child in value.items():
            yield key
            yield from _iter_keys(child)
    elif isinstance(value, list):
        for child in value:
            yield from _iter_keys(child)


def _validate_safe_inputs(bundle: Mapping[str, Any], errors: list[GovernanceError]) -> None:
    for path, text in _iter_strings(bundle):
        if any(ord(char) < 0x20 and char not in "\n\r\t" for char in text):
            _error(errors, "unsafe_text", path=path, detail="control character")
        for pattern in _UNSAFE_MARKUP_PATTERNS:
            if pattern.search(text):
                _error(errors, "unsafe_markup", path=path, detail=pattern.pattern)
                break
        for pattern in _SECRET_PATTERNS:
            if pattern.search(text):
                _error(errors, "sensitive_value", path=path, detail="secret-shaped input")
                break


def _unique_index(
    rows: Any,
    key: str,
    errors: list[GovernanceError],
    path: str,
    code: str,
) -> dict[str, Mapping[str, Any]]:
    result: dict[str, Mapping[str, Any]] = {}
    if not isinstance(rows, list):
        _error(errors, "collection_required", path=path, detail="expected array")
        return result
    for index, row in enumerate(rows):
        if not isinstance(row, Mapping):
            _error(errors, "object_required", path=f"{path}[{index}]")
            continue
        value = row.get(key)
        if not isinstance(value, str) or not value:
            _error(errors, "stable_id_required", path=f"{path}[{index}].{key}")
            continue
        if value in result:
            _error(errors, code, value, f"{path}[{index}].{key}")
        else:
            result[value] = row
    return result


def _parse_time(value: Any) -> datetime | None:
    if not isinstance(value, str):
        return None
    try:
        parsed = datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError:
        return None
    return parsed.replace(tzinfo=timezone.utc)


def _effective_freshness(record: Mapping[str, Any], evaluation_time: datetime | None) -> str:
    freshness = record.get("freshness")
    expires_at = _parse_time(record.get("expires_at"))
    if expires_at is not None and evaluation_time is not None and expires_at <= evaluation_time:
        return "stale"
    if record.get("result") != "pass":
        return "stale"
    return freshness if isinstance(freshness, str) else "stale"


def current_matching_evidence(
    subject: Mapping[str, Any],
    records: Sequence[Mapping[str, Any]],
    evaluation_time: datetime | str | None = None,
    *,
    source_revision: str | None = None,
    target_revision: str | None = None,
    environment_id: str | None = None,
) -> Mapping[str, Any] | None:
    candidates = _matching_evidence_records(
        subject,
        records,
        source_revision=source_revision,
        target_revision=target_revision,
        environment_id=environment_id,
    )
    return candidates[-1] if candidates else None


def _matching_evidence_records(
    subject: Mapping[str, Any],
    records: Sequence[Mapping[str, Any]],
    *,
    source_revision: str | None = None,
    target_revision: str | None = None,
    environment_id: str | None = None,
) -> list[Mapping[str, Any]]:
    evidence_ids = set(subject.get("evidence_ids", []))
    subject_id = subject.get("capability_id") or subject.get("subject_id") or subject.get("decision_id")
    candidates: list[Mapping[str, Any]] = []
    for record in records:
        if evidence_ids and record.get("evidence_id") not in evidence_ids:
            continue
        if not evidence_ids and subject_id and record.get("subject_id") != subject_id:
            continue
        source_binding = record.get("source_binding") or {}
        target_binding = record.get("target_binding") or {}
        environment = record.get("environment") or {}
        if source_revision is not None and source_binding.get("revision_value") != source_revision:
            continue
        if target_revision is not None and target_binding.get("revision_value") != target_revision:
            continue
        if environment_id is not None and environment.get("environment_id") != environment_id:
            continue
        candidates.append(record)
    return sorted(candidates, key=lambda row: int(row.get("sequence", 0)))


def _evidence_binding_tuple(record: Mapping[str, Any]) -> tuple[str, str, str, str, str]:
    source_binding = record.get("source_binding") or {}
    target_binding = record.get("target_binding") or {}
    environment = record.get("environment") or {}
    return (
        str(record.get("subject_family", "")),
        str(record.get("subject_id", "")),
        str(source_binding.get("revision_value", "")),
        str(target_binding.get("revision_value", "")),
        str(environment.get("environment_id", "")),
    )


def _evidence_failure_code(
    subject: Mapping[str, Any],
    records: Sequence[Mapping[str, Any]],
    evaluation_time: datetime | str | None,
) -> str | None:
    if isinstance(evaluation_time, str):
        evaluation_time = _parse_time(evaluation_time)
    candidates = _matching_evidence_records(subject, records)
    if not candidates:
        return None
    latest = candidates[-1]
    latest_binding = _evidence_binding_tuple(latest)
    earlier_matching = [
        record
        for record in candidates[:-1]
        if _evidence_binding_tuple(record) == latest_binding
    ]
    if latest.get("result") != "pass" and any(
        record.get("result") == "pass" for record in earlier_matching
    ):
        return "newer_failed_retest"
    expires_at = _parse_time(latest.get("expires_at"))
    if (
        latest.get("result") == "pass"
        and expires_at is not None
        and evaluation_time is not None
        and expires_at <= evaluation_time
    ):
        return "evidence_expired"
    return None


def counts_as_complete(
    subject: Mapping[str, Any],
    records: Sequence[Mapping[str, Any]] | None = None,
    evaluation_time: datetime | str | None = None,
) -> bool:
    if subject.get("coverage_state") != "verified" or subject.get("freshness") != "current":
        return False
    proof = PROOF_RANK.get(subject.get("proof_level"), -1)
    required = PROOF_RANK.get(subject.get("required_proof_level"), MAX_COLLECTION_ITEMS)
    if proof < required:
        return False
    if records is None:
        return False
    record = current_matching_evidence(subject, records, evaluation_time)
    if record is None:
        return False
    if isinstance(evaluation_time, str):
        evaluation_time = _parse_time(evaluation_time)
    return (
        record.get("result") == "pass"
        and record.get("coverage_state") == "verified"
        and _effective_freshness(record, evaluation_time) == "current"
        and PROOF_RANK.get(record.get("proof_level"), -1) >= required
    )


def strict_journey_completion(
    journey: Mapping[str, Any],
    capabilities: Mapping[str, Mapping[str, Any]],
    records: Sequence[Mapping[str, Any]],
    evaluation_time: datetime | str | None,
) -> bool:
    required = journey.get("required_capability_ids")
    if not isinstance(required, list) or not required:
        return False
    return all(
        capability_id in capabilities
        and counts_as_complete(capabilities[capability_id], records, evaluation_time)
        for capability_id in required
    )


def validate_revision_ancestry(
    revisions: Sequence[Mapping[str, Any]],
    *,
    family: str = "revision",
) -> list[GovernanceError]:
    errors: list[GovernanceError] = []
    if not revisions:
        return [GovernanceError("ancestry_required", subject_id=family)]
    by_id = _unique_index(revisions, "revision_id", errors, f"$.{family}", "duplicate_revision_id")
    genesis_ids = [
        revision_id
        for revision_id, revision in by_id.items()
        if revision.get("revision_kind") == "genesis"
    ]
    if len(genesis_ids) != 1:
        _error(errors, "genesis_count", family, f"$.{family}", str(len(genesis_ids)))
    for revision_id, revision in by_id.items():
        kind = revision.get("revision_kind")
        present = _PREDECESSOR_KEYS.intersection(revision)
        if kind == "genesis":
            if not str(revision.get("genesis_reason", "")).strip() or present:
                _error(errors, "genesis_reason_required", revision_id, f"$.{family}")
        elif kind == "successor":
            if "genesis_reason" in revision or present != _PREDECESSOR_KEYS:
                _error(errors, "previous_revision_required", revision_id, f"$.{family}")
                continue
            previous_id = revision.get("previous_revision_id")
            previous = by_id.get(previous_id)
            if previous is None:
                _error(errors, "previous_revision_required", revision_id, f"$.{family}", previous_id)
            else:
                if revision.get("previous_revision_sha256") != canonical_sha256(previous):
                    _error(errors, "previous_revision_hash_mismatch", revision_id, f"$.{family}")
                if family == "evidence_revisions":
                    old_records = previous.get("records")
                    new_records = revision.get("records")
                    if not isinstance(old_records, list) or not isinstance(new_records, list):
                        _error(errors, "history_shape", revision_id, f"$.{family}")
                    else:
                        records_path = f"$.{family}.records"
                        old_ids = [record.get("evidence_id") for record in old_records]
                        new_ids = [record.get("evidence_id") for record in new_records]
                        new_prefix = new_records[: len(old_records)]
                        prefix_ids = [record.get("evidence_id") for record in new_prefix]
                        if not set(old_ids).issubset(new_ids):
                            _error(errors, "history_removal", revision_id, records_path)
                        elif prefix_ids != old_ids:
                            _error(errors, "history_reorder", revision_id, records_path)
                        elif new_prefix != old_records:
                            _error(errors, "history_rewrite", revision_id, records_path)
                        elif len(new_records) <= len(old_records):
                            _error(errors, "history_removal", revision_id, records_path)
            predecessor_path = revision.get("previous_revision_path")
            if not validate_relative_path(predecessor_path):
                _error(errors, "unsafe_path", revision_id, predecessor_path)
        else:
            _error(errors, "revision_kind", revision_id, f"$.{family}", kind)

    if revisions:
        head_id = revisions[-1].get("revision_id")
        visited: set[str] = set()
        current_id = head_id
        while isinstance(current_id, str) and current_id in by_id:
            if current_id in visited:
                _error(errors, "ancestry_cycle", current_id, f"$.{family}")
                break
            visited.add(current_id)
            current = by_id[current_id]
            if current.get("revision_kind") == "genesis":
                break
            current_id = current.get("previous_revision_id")
        if set(by_id) != visited:
            for orphan in sorted(set(by_id) - visited):
                _error(errors, "ancestry_orphan", orphan, f"$.{family}")
    return sorted_errors(errors)


def _collect_evidence_references(value: Any) -> set[str]:
    references: set[str] = set()
    if isinstance(value, dict):
        for key, child in value.items():
            if key.endswith("evidence_ids"):
                if isinstance(child, list):
                    references.update(item for item in child if isinstance(item, str))
            references.update(_collect_evidence_references(child))
    elif isinstance(value, list):
        for child in value:
            references.update(_collect_evidence_references(child))
    return references


def _add_expected_binding(
    expected: dict[str, tuple[str, str, str, str, str, str]],
    evidence_id: Any,
    binding: tuple[str, str, str, str, str, str],
    owner: str,
    errors: list[GovernanceError],
) -> None:
    if not isinstance(evidence_id, str):
        return
    previous = expected.get(evidence_id)
    if previous is not None and previous != binding:
        _error(errors, "evidence_binding_ambiguity", evidence_id, detail=owner)
    expected[evidence_id] = binding


def _expected_evidence_bindings(
    bundle: Mapping[str, Any], errors: list[GovernanceError]
) -> dict[str, tuple[str, str, str, str, str, str]]:
    expected: dict[str, tuple[str, str, str, str, str, str]] = {}
    artifact = bundle.get("official_source_artifact") or {}
    artifact_revision = str(artifact.get("content_sha256", ""))
    for revision in bundle.get("public_baseline_revisions", []):
        revision_id = str(revision.get("revision_id", ""))
        for capability in revision.get("capabilities", []):
            binding = (
                "capability",
                str(capability.get("capability_id", "")),
                revision_id,
                artifact_revision,
                revision_id,
                "local_contract",
            )
            for evidence_id in capability.get("evidence_ids", []):
                _add_expected_binding(expected, evidence_id, binding, binding[1], errors)
        for mapping in revision.get("official_source_index", []):
            exclusion = mapping.get("exclusion")
            if not isinstance(exclusion, dict):
                continue
            binding = (
                "public_baseline",
                str(revision.get("snapshot_id", "")),
                revision_id,
                artifact_revision,
                revision_id,
                "source_review",
            )
            for evidence_id in exclusion.get("evidence_ids", []):
                _add_expected_binding(expected, evidence_id, binding, str(mapping.get("source_entry_id", "")), errors)
    for revision in bundle.get("repository_registry_revisions", []):
        revision_id = str(revision.get("revision_id", ""))
        for repository in revision.get("repositories", []):
            binding = (
                "repository",
                str(repository.get("repo_id", "")),
                revision_id,
                str(repository.get("revision_value", "")),
                revision_id,
                "source_review",
            )
            evidence_groups = [repository.get("license_evidence_ids", [])]
            evidence_groups.extend(alias.get("evidence_ids", []) for alias in repository.get("aliases", []) if isinstance(alias, dict))
            for group in evidence_groups:
                for evidence_id in group:
                    _add_expected_binding(expected, evidence_id, binding, binding[1], errors)
    for revision in bundle.get("capability_decision_revisions", []):
        revision_id = str(revision.get("revision_id", ""))
        for decision in revision.get("decisions", []):
            binding = (
                "capability_decision",
                str(decision.get("decision_id", "")),
                revision_id,
                str(decision.get("source_revision_value", "")),
                str(decision.get("target_revision_value", "")),
                "source_review",
            )
            groups = [decision.get("evidence_ids", [])]
            security_review = decision.get("security_review")
            if isinstance(security_review, dict):
                groups.append(security_review.get("evidence_ids", []))
            for group in groups:
                for evidence_id in group:
                    _add_expected_binding(expected, evidence_id, binding, binding[1], errors)
    for revision in bundle.get("legacy_authority_revisions", []):
        revision_id = str(revision.get("revision_id", ""))
        for entry in revision.get("entries", []):
            binding = (
                "legacy_authority",
                revision_id,
                revision_id,
                str(entry.get("content_sha256", "")),
                revision_id,
                "source_review",
            )
            for evidence_id in entry.get("evidence_ids", []):
                _add_expected_binding(expected, evidence_id, binding, str(entry.get("path", "")), errors)
    return expected


def _validate_evidence(bundle: Mapping[str, Any], errors: list[GovernanceError]) -> list[Mapping[str, Any]]:
    revisions = bundle.get("evidence_revisions")
    if not isinstance(revisions, list) or not revisions:
        _error(errors, "evidence_required", path="$.evidence_revisions")
        return []
    head_records = revisions[-1].get("records")
    records_by_id = _unique_index(
        head_records,
        "evidence_id",
        errors,
        "$.evidence_revisions[-1].records",
        "duplicate_evidence_id",
    )
    records = list(head_records) if isinstance(head_records, list) else []
    sequences = [record.get("sequence") for record in records if isinstance(record, Mapping)]
    if sequences != list(range(1, len(records) + 1)):
        _error(errors, "evidence_sequence", revisions[-1].get("revision_id", ""))
    previous_hash = "genesis"
    tuple_latest: dict[tuple[str, str, str, str, str], Mapping[str, Any]] = {}
    tuple_previous: dict[tuple[str, str, str, str, str], Mapping[str, Any]] = {}
    for index, record in enumerate(records):
        evidence_id = record.get("evidence_id", f"record-{index}")
        if record.get("previous_record_sha256") != previous_hash:
            _error(errors, "record_chain", evidence_id, f"$.evidence_revisions[-1].records[{index}]")
        if record.get("record_sha256") != record_sha256(record):
            _error(errors, "record_sha256", evidence_id, f"$.evidence_revisions[-1].records[{index}]")
        previous_hash = str(record.get("record_sha256", ""))
        source = record.get("source_binding") or {}
        target = record.get("target_binding") or {}
        environment = record.get("environment") or {}
        key = (
            str(record.get("subject_family", "")),
            str(record.get("subject_id", "")),
            str(source.get("revision_value", "")),
            str(target.get("revision_value", "")),
            str(environment.get("environment_id", "")),
        )
        if key in tuple_latest:
            tuple_previous[key] = tuple_latest[key]
        tuple_latest[key] = record
        for superseded in record.get("supersedes_evidence_ids", []):
            if superseded not in records_by_id or records.index(records_by_id[superseded]) >= index:
                _error(errors, "invalid_supersession", evidence_id, detail=superseded)
    for key, latest in tuple_latest.items():
        previous = tuple_previous.get(key)
        if previous is None:
            continue
        if PROOF_RANK.get(latest.get("proof_level"), -1) < PROOF_RANK.get(previous.get("proof_level"), -1):
            event = latest.get("transition_event") or {}
            if event.get("event_type") != "proof_decrease":
                _error(errors, "proof_regression", latest.get("evidence_id", ""))
    referenced = _collect_evidence_references(
        {key: value for key, value in bundle.items() if key != "bundle_manifest"}
    )
    for evidence_id in sorted(referenced - set(records_by_id)):
        _error(errors, "unknown_reference", evidence_id, "$.evidence_revisions[-1].records")
    expected = _expected_evidence_bindings(bundle, errors)
    for evidence_id, binding in sorted(expected.items()):
        record = records_by_id.get(evidence_id)
        if record is None:
            continue
        source = record.get("source_binding") or {}
        target = record.get("target_binding") or {}
        environment = record.get("environment") or {}
        actual = (
            str(record.get("subject_family", "")),
            str(record.get("subject_id", "")),
            str(record.get("subject_revision_id", "")),
            str(source.get("revision_value", "")),
            str(target.get("revision_value", "")),
            str(environment.get("environment_kind", "")),
        )
        if actual != binding:
            _error(errors, "evidence_binding", evidence_id, detail=f"expected {binding}, got {actual}")
    return records


def _validate_manifest_bindings(bundle: Mapping[str, Any], errors: list[GovernanceError]) -> None:
    manifest = bundle.get("bundle_manifest")
    artifact = bundle.get("official_source_artifact")
    if not isinstance(manifest, Mapping) or not isinstance(artifact, Mapping):
        return
    if _PROHIBITED_COMPLETION_KEYS.intersection(_iter_keys(manifest)):
        _error(errors, "bundle_completion_authority", path="$.bundle_manifest")
    artifact_binding = manifest.get("official_source_artifact") or {}
    if artifact_binding.get("artifact_id") != artifact.get("artifact_id"):
        _error(errors, "artifact_binding_id", artifact_binding.get("artifact_id", ""))
    if artifact_binding.get("sha256") != canonical_sha256(artifact):
        _error(errors, "artifact_binding_sha256", artifact.get("artifact_id", ""))
    if not validate_relative_path(artifact_binding.get("path")):
        _error(errors, "unsafe_path", artifact.get("artifact_id", ""), artifact_binding.get("path", ""))
    for family_key, binding_key in REVISION_FAMILIES.items():
        revisions = bundle.get(family_key)
        if not isinstance(revisions, list) or not revisions:
            continue
        head = revisions[-1]
        binding = manifest.get(binding_key) or {}
        if binding.get("revision_id") != head.get("revision_id"):
            _error(errors, "head_binding_id", binding_key, detail=binding.get("revision_id", ""))
        if binding.get("sha256") != canonical_sha256(head):
            _error(errors, "head_binding_sha256", binding_key)
        if not validate_relative_path(binding.get("path")):
            _error(errors, "unsafe_path", binding_key, binding.get("path", ""))
    if _parse_time(manifest.get("evaluation_time")) is None:
        _error(errors, "evaluation_time_invalid", path="$.bundle_manifest.evaluation_time")


def _validate_official_source(bundle: Mapping[str, Any], errors: list[GovernanceError]) -> dict[str, Mapping[str, Any]]:
    artifact = bundle.get("official_source_artifact")
    if not isinstance(artifact, Mapping):
        _error(errors, "official_source_required", path="$.official_source_artifact")
        return {}
    entries = _unique_index(
        artifact.get("entries"),
        "source_entry_id",
        errors,
        "$.official_source_artifact.entries",
        "duplicate_source_entry_id",
    )
    normalization = artifact.get("normalization") or {}
    if normalization.get("method") == "inline-base64-entry-lf-join":
        normalized_entries: list[bytes] = []
        for entry_id, entry in entries.items():
            locator = entry.get("locator")
            if not isinstance(locator, str) or not locator.startswith("inline-base64:"):
                _error(errors, "source_locator_invalid", entry_id, detail=locator)
                continue
            try:
                decoded = base64.b64decode(locator.removeprefix("inline-base64:"), validate=True)
                decoded.decode("utf-8")
            except (binascii.Error, UnicodeDecodeError) as exc:
                _error(errors, "source_content_invalid", entry_id, detail=exc)
                continue
            if hashlib.sha256(decoded).hexdigest() != entry.get("entry_sha256"):
                _error(errors, "source_entry_sha256", entry_id)
            normalized_entries.append(decoded)
        content = b"\n".join(normalized_entries) + b"\n"
        if hashlib.sha256(content).hexdigest() != artifact.get("content_sha256"):
            _error(errors, "source_content_sha256", artifact.get("artifact_id", ""))
    return entries


def _validate_public_baselines(
    bundle: Mapping[str, Any],
    source_entries: Mapping[str, Mapping[str, Any]],
    evidence_records: Sequence[Mapping[str, Any]],
    errors: list[GovernanceError],
) -> None:
    artifact = bundle.get("official_source_artifact") or {}
    evaluation_time = (bundle.get("bundle_manifest") or {}).get("evaluation_time")
    for revision_index, revision in enumerate(bundle.get("public_baseline_revisions", [])):
        revision_id = revision.get("revision_id", f"revision-{revision_index}")
        path = f"$.public_baseline_revisions[{revision_index}]"
        binding = revision.get("source_artifact") or {}
        if binding.get("artifact_id") != artifact.get("artifact_id"):
            _error(errors, "source_artifact_id", revision_id, f"{path}.source_artifact")
        if binding.get("sha256") != canonical_sha256(artifact):
            _error(errors, "source_artifact_sha256", revision_id, f"{path}.source_artifact")
        if binding.get("content_sha256") != artifact.get("content_sha256"):
            _error(errors, "source_content_sha256", revision_id, f"{path}.source_artifact")
        if not validate_relative_path(binding.get("path")):
            _error(errors, "unsafe_path", revision_id, binding.get("path", ""))
        capabilities = _unique_index(
            revision.get("capabilities"),
            "capability_id",
            errors,
            f"{path}.capabilities",
            "duplicate_capability_id",
        )
        journeys = _unique_index(
            revision.get("journeys"),
            "journey_id",
            errors,
            f"{path}.journeys",
            "duplicate_journey_id",
        )
        mappings = _unique_index(
            revision.get("official_source_index"),
            "source_entry_id",
            errors,
            f"{path}.official_source_index",
            "duplicate_source_mapping",
        )
        for source_id in sorted(set(source_entries) - set(mappings)):
            _error(errors, "unmapped_source_entry", source_id, f"{path}.official_source_index")
        for source_id in sorted(set(mappings) - set(source_entries)):
            _error(errors, "unknown_source_entry", source_id, f"{path}.official_source_index")
        mapped_capabilities: set[str] = set()
        for source_id, mapping in mappings.items():
            has_caps = "capability_ids" in mapping
            has_exclusion = "exclusion" in mapping
            if has_caps == has_exclusion:
                _error(errors, "source_mapping_choice", source_id, f"{path}.official_source_index")
            if has_caps:
                capability_ids = mapping.get("capability_ids") or []
                for capability_id in capability_ids:
                    if capability_id not in capabilities:
                        _error(errors, "unknown_capability", capability_id, f"{path}.official_source_index")
                    mapped_capabilities.add(capability_id)
            else:
                exclusion = mapping.get("exclusion") or {}
                if not exclusion.get("rationale") or not exclusion.get("evidence_ids") or not exclusion.get("review_revision"):
                    _error(errors, "unjustified_exclusion", source_id, f"{path}.official_source_index")
        required_capabilities: set[str] = set()
        for journey_id, journey in journeys.items():
            required = journey.get("required_capability_ids") or []
            optional = journey.get("optional_capability_ids") or []
            if set(required).intersection(optional):
                _error(errors, "journey_capability_overlap", journey_id, f"{path}.journeys")
            for capability_id in list(required) + list(optional):
                if capability_id not in capabilities:
                    _error(errors, "unknown_capability", capability_id, f"{path}.journeys")
            required_capabilities.update(required)
            strict_journey_completion(journey, capabilities, evidence_records, evaluation_time)
        for capability_id in sorted(required_capabilities - mapped_capabilities):
            _error(errors, "missing_required_capability", capability_id, f"{path}.official_source_index")
        for capability_id, capability in capabilities.items():
            required_level = capability.get("required_proof_level")
            proof_level = capability.get("proof_level")
            if required_level not in PROOF_RANK or proof_level not in PROOF_RANK:
                _error(errors, "proof_level_invalid", capability_id, f"{path}.capabilities")
            if capability.get("coverage_state") == "verified":
                if not capability.get("evidence_ids"):
                    _error(errors, "verified_evidence_required", capability_id, f"{path}.capabilities")
                elif failure_code := _evidence_failure_code(
                    capability,
                    evidence_records,
                    evaluation_time,
                ):
                    _error(
                        errors,
                        failure_code,
                        capability_id,
                        f"{path}.capabilities",
                        freshness="stale",
                    )
                elif not counts_as_complete(capability, evidence_records, evaluation_time):
                    _error(errors, "capability_not_complete", capability_id, f"{path}.capabilities", freshness="stale")


def _validate_registry_and_decisions(bundle: Mapping[str, Any], errors: list[GovernanceError]) -> None:
    registry_revisions = bundle.get("repository_registry_revisions")
    decision_revisions = bundle.get("capability_decision_revisions")
    if not isinstance(registry_revisions, list) or not registry_revisions:
        _error(errors, "repository_registry_required")
        return
    if not isinstance(decision_revisions, list) or not decision_revisions:
        _error(errors, "capability_decisions_required")
        return
    for index, revision in enumerate(registry_revisions):
        revision_id = revision.get("revision_id", f"registry-{index}")
        repositories = _unique_index(
            revision.get("repositories"),
            "repo_id",
            errors,
            f"$.repository_registry_revisions[{index}].repositories",
            "duplicate_repository_id",
        )
        paths: dict[str, str] = {}
        expected_count = revision.get("expected_count")
        if expected_count != len(repositories):
            _error(errors, "summary_mismatch", revision_id, detail=len(repositories))
        for repo_id, repository in repositories.items():
            repo_path = repository.get("path")
            if repo_path in paths:
                _error(errors, "duplicate_repository_path", repo_id, repo_path, paths[repo_path])
            elif isinstance(repo_path, str):
                paths[repo_path] = repo_id
            if not validate_relative_path(repo_path):
                _error(errors, "unsafe_path", repo_id, repo_path)
            for alias in repository.get("aliases", []):
                if (
                    not isinstance(alias, Mapping)
                    or set(alias) != _ALIAS_KEYS
                    or alias.get("kind") not in _ALIAS_KINDS
                    or alias.get("change_kind") not in _ALIAS_CHANGE_KINDS
                ):
                    _error(
                        errors,
                        "alias_contract_invalid",
                        repo_id,
                        detail=sorted(alias) if isinstance(alias, Mapping) else "not object",
                    )
            domains = repository.get("domains")
            if (
                not isinstance(domains, list)
                or not domains
                or any(not isinstance(domain, str) or not re.fullmatch(r"D[0-9]{2}", domain) for domain in domains)
                or len(domains) != len(set(domains))
            ):
                _error(errors, "alias_contract_invalid", repo_id, detail="domains")
            if repository.get("revision_kind") == "git_commit" and repository.get("git_head") != repository.get("revision_value"):
                _error(errors, "git_head_mismatch", repo_id)
            if repository.get("revision_kind") == "content_tree_sha256" and "git_head" in repository:
                _error(errors, "content_git_head_forbidden", repo_id)
            if _PROHIBITED_COMPLETION_KEYS.intersection(_iter_keys(repository)):
                _error(errors, "registry_completion_authority", repo_id)

    registry = registry_revisions[-1]
    decisions_head = decision_revisions[-1]
    repositories = {
        row.get("repo_id"): row
        for row in registry.get("repositories", [])
        if isinstance(row, Mapping) and isinstance(row.get("repo_id"), str)
    }
    inventory_pairs = {
        (repo_id, capability_id)
        for repo_id, repository in repositories.items()
        for capability_id in repository.get("inventory_capability_ids", [])
    }
    current_rows = [
        row
        for row in decisions_head.get("decisions", [])
        if isinstance(row, Mapping) and row.get("freshness") == "current"
    ]
    pairs: dict[tuple[str, str], Mapping[str, Any]] = {}
    for decision in current_rows:
        decision_id = decision.get("decision_id", "")
        pair = (decision.get("repo_id"), decision.get("capability_id"))
        if pair in pairs:
            _error(errors, "duplicate_current_decision", decision_id, detail=pair)
        pairs[pair] = decision
        repository = repositories.get(decision.get("repo_id"))
        if repository is None:
            _error(errors, "unknown_repository", decision_id, detail=decision.get("repo_id"))
            continue
        if decision.get("source_revision_value") != repository.get("revision_value"):
            _error(errors, "source_revision_drift", decision_id, freshness="stale")
        source_path = decision.get("source_path")
        if not validate_relative_path(source_path) or not str(source_path).startswith(str(repository.get("path")) + "/"):
            _error(errors, "source_path_mismatch", decision_id, source_path)
        if not validate_relative_path(decision.get("target_path")):
            _error(errors, "unsafe_path", decision_id, decision.get("target_path", ""))
        if decision.get("decision") in {"adopt", "adapt"}:
            if decision.get("license_compatibility") != "compatible":
                _error(errors, "license_not_compatible", decision_id)
            security = decision.get("security_review") or {}
            if security.get("status") != "approved":
                _error(errors, "security_review_unapproved", decision_id)
            tests = decision.get("tests") or []
            if not tests or any(test.get("status") != "passing" for test in tests if isinstance(test, Mapping)):
                _error(errors, "tests_not_passing", decision_id)
        if decision.get("decision") == "reject" and not str(decision.get("reason", "")).strip():
            _error(errors, "reject_rationale_required", decision_id)
        if _PROHIBITED_COMPLETION_KEYS.intersection(_iter_keys(decision)):
            _error(errors, "decision_completion_authority", decision_id)
    # Missing decisions are an incomplete-governance state, not malformed data.
    # Real 38/38 completion is computed by consumers from this pair difference;
    # duplicate or out-of-inventory current decisions remain semantic errors.
    for pair in sorted(set(pairs) - inventory_pairs):
        _error(errors, "decision_unknown_capability", f"{pair[0]}:{pair[1]}")


def validate_governance(
    bundle: Mapping[str, Any],
    *,
    root: Path | None = None,
) -> list[GovernanceError]:
    root = _repo_root(root)
    errors: list[GovernanceError] = []
    if not isinstance(bundle, Mapping):
        return [GovernanceError("bundle_invalid", path="$", detail="root must be an object")]
    _enforce_bounds(bundle)
    if set(bundle) != BUNDLE_KEYS:
        _error(errors, "bundle_keys", path="$", detail=f"expected {sorted(BUNDLE_KEYS)}, got {sorted(bundle)}")
        return sorted_errors(errors)
    instances: list[Mapping[str, Any]] = []
    for key in BUNDLE_KEYS:
        value = bundle.get(key)
        if key.endswith("_revisions"):
            if isinstance(value, list):
                instances.extend(item for item in value if isinstance(item, Mapping))
        elif isinstance(value, Mapping):
            instances.append(value)
    for instance in instances:
        errors.extend(structural_errors(instance, root))
    _validate_safe_inputs(bundle, errors)
    source_entries = _validate_official_source(bundle, errors)
    for family_key in REVISION_FAMILIES:
        revisions = bundle.get(family_key)
        if isinstance(revisions, list):
            errors.extend(validate_revision_ancestry(revisions, family=family_key))
    evidence_records = _validate_evidence(bundle, errors)
    _validate_public_baselines(bundle, source_entries, evidence_records, errors)
    _validate_registry_and_decisions(bundle, errors)
    _validate_manifest_bindings(bundle, errors)
    return sorted_errors(errors)


def _revision_chain_from_head(head_path: Path, root: Path) -> list[Mapping[str, Any]]:
    head = load_json(head_path)
    if not isinstance(head, Mapping):
        raise GovernanceUsageError("revision_invalid: head must be an object")
    reverse: list[Mapping[str, Any]] = []
    seen: set[str] = set()
    current = head
    current_path = head_path
    while True:
        revision_id = current.get("revision_id")
        if not isinstance(revision_id, str) or revision_id in seen:
            if revision_id in seen:
                reverse.append(current)
            break
        seen.add(revision_id)
        reverse.append(current)
        if current.get("revision_kind") == "genesis":
            break
        previous_raw = current.get("previous_revision_path")
        if not isinstance(previous_raw, str):
            break
        previous = load_bound_json(root, previous_raw)
        if not isinstance(previous, Mapping):
            raise GovernanceUsageError("revision_invalid: predecessor must be an object")
        current = previous
        current_path = resolve_repository_path(root, _split_binding_path(previous_raw)[0])
    return list(reversed(reverse))


def validate_history_file(head_path: Path, *, root: Path | None = None) -> list[GovernanceError]:
    root = _repo_root(root or head_path.parent)
    chain = _revision_chain_from_head(head_path.resolve(), root)
    schema_id = chain[-1].get("schema", "revision") if chain else "revision"
    family = {
        "kiana.capability-evidence-index.v1": "evidence_revisions",
        "kiana.public-parity-baseline.v1": "public_baseline_revisions",
        "kiana.reference-repository-registry.v1": "repository_registry_revisions",
        "kiana.capability-decisions.v1": "capability_decision_revisions",
        "kiana.legacy-authority-classification.v1": "legacy_authority_revisions",
        "kiana.official-source-artifact.v1": "official_source_artifact_revisions",
    }.get(schema_id, "revision")
    errors: list[GovernanceError] = []
    for revision in chain:
        errors.extend(structural_errors(revision, root))
    errors.extend(validate_revision_ancestry(chain, family=family))
    return sorted_errors(errors)


def load_manifest_bundle(manifest_path: Path, *, root: Path | None = None) -> dict[str, Any]:
    root = _repo_root(root or manifest_path.parent)
    manifest = load_json(manifest_path)
    if not isinstance(manifest, Mapping):
        raise GovernanceUsageError("manifest_invalid: root must be an object")
    if set(manifest) >= BUNDLE_KEYS:
        return dict(manifest)
    if manifest.get("schema") != "kiana.capability-governance-bundle.v1":
        raise GovernanceUsageError("manifest_invalid: unexpected schema")
    artifact_binding = manifest.get("official_source_artifact") or {}
    artifact = load_bound_json(root, artifact_binding.get("path", ""))
    bundle: dict[str, Any] = {
        "official_source_artifact": artifact,
        "bundle_manifest": dict(manifest),
    }
    for family_key, binding_key in REVISION_FAMILIES.items():
        binding = manifest.get(binding_key) or {}
        head_path_raw = binding.get("path", "")
        file_part, fragment = _split_binding_path(head_path_raw)
        file_path = resolve_repository_path(root, file_part)
        if fragment:
            document = load_json(file_path)
            head = _json_fragment(document, fragment)
            if isinstance(document, Mapping) and family_key in document:
                revisions = document[family_key]
            else:
                revisions = [head]
        else:
            revisions = _revision_chain_from_head(file_path, root)
        bundle[family_key] = revisions
    return bundle


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def content_tree_sha256(root: Path) -> str:
    root = root.resolve()
    digest = hashlib.sha256()
    files = sorted(
        path
        for path in root.rglob("*")
        if path.is_file() and ".git" not in path.relative_to(root).parts
    )
    for path in files:
        if path.is_symlink():
            resolved = path.resolve()
            try:
                resolved.relative_to(root)
            except ValueError as exc:
                raise GovernanceUsageError(
                    f"symlink_escape: {_sanitize_path_for_output(path.relative_to(root))}"
                ) from exc
        relative = path.relative_to(root).as_posix().encode("utf-8")
        payload = _bounded_read(path)
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        digest.update(len(payload).to_bytes(8, "big"))
        digest.update(payload)
    return digest.hexdigest()


def repository_fingerprint(path: Path) -> dict[str, str]:
    path = path.resolve()
    result = subprocess.run(
        ["git", "-C", str(path), "rev-parse", "HEAD"],
        check=False,
        capture_output=True,
        text=True,
    )
    fingerprint = {"tree_sha256": content_tree_sha256(path)}
    if result.returncode == 0:
        fingerprint["git_head"] = result.stdout.strip()
    license_files = sorted(
        candidate
        for candidate in path.iterdir()
        if candidate.is_file()
        and candidate.name.lower().startswith(("license", "copying", "notice"))
    )
    if license_files:
        digest = hashlib.sha256()
        for candidate in license_files:
            digest.update(candidate.name.encode("utf-8"))
            digest.update(b"\x00")
            digest.update(_bounded_read(candidate))
        fingerprint["license_sha256"] = digest.hexdigest()
    return fingerprint


def detect_drift(
    bundle: Mapping[str, Any],
    *,
    live_reference_root: Path,
    target_root: Path,
    official_source_artifact: Mapping[str, Any],
) -> list[GovernanceError]:
    errors: list[GovernanceError] = []
    canonical_artifact = bundle.get("official_source_artifact") or {}
    if (
        official_source_artifact.get("artifact_id") != canonical_artifact.get("artifact_id")
        or official_source_artifact.get("content_sha256") != canonical_artifact.get("content_sha256")
        or canonical_sha256(official_source_artifact) != canonical_sha256(canonical_artifact)
    ):
        _error(
            errors,
            "official_source_hash_drift",
            canonical_artifact.get("artifact_id", ""),
            freshness="stale",
        )
    registry_revisions = bundle.get("repository_registry_revisions") or []
    if registry_revisions:
        for repository in registry_revisions[-1].get("repositories", []):
            repo_id = repository.get("repo_id", "")
            raw_path = repository.get("path", "")
            try:
                relative = PurePosixPath(str(raw_path)).relative_to("reference")
                live_path = live_reference_root.joinpath(*relative.parts).resolve()
                live_path.relative_to(live_reference_root.resolve())
                fingerprint = repository_fingerprint(live_path)
            except (ValueError, OSError, GovernanceUsageError) as exc:
                _error(errors, "source_unavailable", repo_id, raw_path, exc, "stale")
                continue
            if repository.get("revision_kind") == "git_commit" and fingerprint.get("git_head") != repository.get("revision_value"):
                _error(errors, "repository_head_drift", repo_id, raw_path, freshness="stale")
            if fingerprint.get("tree_sha256") != repository.get("tree_sha256"):
                tree_code = (
                    "content_tree_drift"
                    if repository.get("revision_kind") == "content_tree_sha256"
                    else "repository_tree_drift"
                )
                _error(errors, tree_code, repo_id, raw_path, freshness="stale")
            if fingerprint.get("license_sha256") != repository.get("license_sha256"):
                _error(errors, "license_hash_drift", repo_id, raw_path, freshness="stale")
    decision_revisions = bundle.get("capability_decision_revisions") or []
    if decision_revisions:
        for decision in decision_revisions[-1].get("decisions", []):
            if decision.get("freshness") != "current":
                continue
            decision_id = decision.get("decision_id", "")
            try:
                target = resolve_repository_path(target_root.resolve(), decision.get("target_path", ""))
                if decision.get("target_revision_kind") == "artifact_sha256":
                    actual = file_sha256(target)
                else:
                    git_root = target if target.is_dir() else target.parent
                    result = subprocess.run(
                        ["git", "-C", str(git_root), "rev-parse", "HEAD"],
                        check=False,
                        capture_output=True,
                        text=True,
                    )
                    actual = result.stdout.strip() if result.returncode == 0 else ""
                if actual != decision.get("target_revision_value"):
                    _error(errors, "target_revision_drift", decision_id, decision.get("target_path", ""), freshness="stale")
            except GovernanceUsageError as exc:
                _error(errors, "target_unavailable", decision_id, decision.get("target_path", ""), exc, "stale")
    return sorted_errors(errors)


def validation_report(status: str, errors: Iterable[GovernanceError]) -> dict[str, Any]:
    return {
        "schema": VALIDATION_SCHEMA,
        "status": status,
        "errors": [error.as_dict() for error in sorted_errors(errors)],
    }
