#!/usr/bin/env python3
"""Validate and regenerate the UI-09 schema lock and static Rust catalog.

The lock is intentionally small and deterministic.  JSON Schema files are the canonical wire
contract; this command checks their ids, hashes, frozen shapes, examples, redaction rules, the
Rust structs they are bound to, the compatibility matrix and the negative fixtures, then compares
the generated Rust catalog with the lock.  ``--write`` is the only mutating mode.

The gate also covers the assets themselves: every ``*.schema.json`` in the registry, every example
and every negative fixture must be locked (or bound by the compatibility matrix), so a new file
cannot sit outside the version contract unnoticed.  ``deprecated_fields`` is bound to the schema's
own ``"deprecated": true`` markers instead of being trusted as a free-form list.

The frozen shape of every contract is *not* refreshed by ``--write``: a shape change inside the
same version is a breaking change and must be reviewed by hand together with a version bump, so the
gate keeps failing until a human updates the recorded shape.

``--emitted <path>`` closes the cross-language round trip: it validates the report produced by
``cargo run -p kiana-client --example ui09_schema_emit`` (Rust decoded the locked examples and
re-encoded them) against the canonical JSON Schema, so a schema change that Rust no longer emits
fails here even when the checked-in example still matches.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
SCHEMA_ROOT = ROOT / "docs" / "schemas" / "ui"
LOCK_PATH = SCHEMA_ROOT / "schema-lock.v1.json"
MATRIX_PATH = SCHEMA_ROOT / "compatibility-matrix.v1.json"
VALIDATOR_PATH = ROOT / "scripts" / "validate-json-schema.py"

SCHEMA_DIALECT = "https://json-schema.org/draft/2020-12/schema"
HEX_DIGEST = re.compile(r"^sha256:[0-9a-f]{64}$")
SECRET_KEY = re.compile(
    r"(?:secret|token|password|passwd|api[_-]?key|authorization|private[_-]?key)", re.I
)
ABSOLUTE_PATH = re.compile(r"^(?:/|[A-Za-z]:[\\/])")
RUST_STRUCT = re.compile(r"(?ms)^pub struct (?P<name>\w+) \{\n(?P<body>.*?)^\}")
RUST_FIELD = re.compile(
    r"(?m)^ {4}(?:pub(?:\([^)]*\))? )?(?P<name>[a-z_][a-z0-9_]*): (?P<type>.+),$"
)

LOCK_FIELDS = {
    "$schema",
    "$id",
    "schema",
    "version",
    "source_of_truth",
    "canonical_owner",
    "generated_rust",
    "contracts",
}
CONTRACT_FIELDS = {
    "id",
    "version",
    "schema_file",
    "rust_type",
    "rust_source",
    "owner_crate",
    "unknown_fields",
    "max_bytes",
    "deprecated_fields",
    "frozen_shape",
    "valid_example",
    "unknown_field_example",
    "schema_sha256",
}
DEPRECATED_FIELD_KEYS = {
    "field",
    "since",
    "remove_in",
    "replacement",
}  # replacement is optional
EMITTED_REPORT_SCHEMA = "kiana.ui09-emitted.v1"
EMITTED_MAX_BYTES = 4 * 1024 * 1024
MATRIX_FIELDS = {
    "$schema",
    "$id",
    "schema",
    "version",
    "same_version_policy",
    "unknown_major_policy",
    "unknown_command_policy",
    "rules",
    "negative_cases",
}
NEGATIVE_CASE_FIELDS = {
    "case_id",
    "change",
    "disposition",
    "schema",
    "fixture",
    "expected_error",
    "enforced_by",
    "enforced_by_test",
}


class ContractError(RuntimeError):
    """A deterministic schema-lock violation."""


def reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ContractError(f"duplicate_json_key:{key}")
        result[key] = value
    return result


def read_json(path: Path) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys
        )
    except ContractError:
        raise
    except Exception as exc:  # noqa: BLE001
        raise ContractError(f"json_read_failed:{path.relative_to(ROOT)}:{exc}") from exc


def write_json(path: Path, value: Any) -> None:
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )


def canonical_digest(value: Any) -> str:
    payload = json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode()
    return f"sha256:{hashlib.sha256(payload).hexdigest()}"


def safe_relative(raw: Any, field: str) -> Path:
    if not isinstance(raw, str) or not raw or "\0" in raw:
        raise ContractError(f"{field}_invalid")
    path = Path(raw)
    if path.is_absolute() or ".." in path.parts:
        raise ContractError(f"{field}_outside_registry")
    return path


def walk_values(value: Any, path: str = "$") -> list[tuple[str, Any]]:
    values = [(path, value)]
    if isinstance(value, dict):
        for key, child in value.items():
            values.extend(walk_values(child, f"{path}.{key}"))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            values.extend(walk_values(child, f"{path}[{index}]"))
    return values


def load_schema_validator() -> Any:
    spec = importlib.util.spec_from_file_location(
        "kiana_schema_validator", VALIDATOR_PATH
    )
    if spec is None or spec.loader is None:
        raise ContractError("schema_validator_import_failed")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def generated_catalog(lock: dict[str, Any]) -> str:
    contracts = lock["contracts"]
    lines = [
        "//! Generated by scripts/validate-ui09-schemas.py --write; do not edit by hand.",
        "//!",
        "//! JSON Schema remains canonical. This catalog only exposes bounded metadata to clients.",
        "",
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]",
        "pub struct UiSchemaContract {",
        "    pub id: &'static str,",
        "    pub rust_type: &'static str,",
        "    pub max_bytes: usize,",
        "    pub unknown_fields: &'static str,",
        "}",
        "",
        'pub const UI_SCHEMA_LOCK_SCHEMA: &str = "kiana.ui-schema-lock.v1";',
        "",
        "pub const UI_SCHEMA_CONTRACTS: &[UiSchemaContract] = &[",
    ]
    for contract in contracts:
        lines.extend(
            [
                "    UiSchemaContract {",
                f"        id: {contract['id']!r},".replace("'", '"'),
                f"        rust_type: {contract['rust_type']!r},".replace("'", '"'),
                f"        max_bytes: {contract['max_bytes']},",
                f"        unknown_fields: {contract['unknown_fields']!r},".replace(
                    "'", '"'
                ),
                "    },",
            ]
        )
    lines.extend(
        [
            "];",
            "",
            "pub fn schema_contract(id: &str) -> Option<&'static UiSchemaContract> {",
            "    for contract in UI_SCHEMA_CONTRACTS {",
            "        if contract.id == id {",
            "            return Some(contract);",
            "        }",
            "    }",
            "    None",
            "}",
            "",
        ]
    )
    return "\n".join(lines)


def property_descriptor(property_schema: Any) -> str:
    """A small, stable descriptor so a same-version type or shape change is visible."""
    if not isinstance(property_schema, dict):
        return "invalid"
    if "$ref" in property_schema:
        return str(property_schema["$ref"])
    if "const" in property_schema:
        return "const"
    declared = property_schema.get("type")
    if isinstance(declared, list):
        return "|".join(sorted(str(option) for option in declared))
    if declared == "array":
        items = property_schema.get("items")
        return (
            f"array<{property_descriptor(items)}>"
            if isinstance(items, dict)
            else "array<any>"
        )
    if isinstance(declared, str):
        return declared
    return "any"


def shape_inventory(schema: dict[str, Any]) -> dict[str, Any]:
    properties = schema.get("properties", {})
    return {
        "deny_unknown_fields": schema.get("additionalProperties") is False,
        "required": sorted(schema.get("required", [])),
        "properties": {
            name: property_descriptor(properties[name]) for name in sorted(properties)
        },
    }


def expected_deprecated_fields(schema: dict[str, Any]) -> list[str]:
    """The property names the schema itself marks `"deprecated": true`."""
    properties = schema.get("properties", {})
    return sorted(
        name
        for name, spec in properties.items()
        if isinstance(spec, dict) and spec.get("deprecated") is True
    )


def validate_deprecated_fields(
    contract: dict[str, Any], schema: dict[str, Any]
) -> None:
    """`deprecated_fields` is bound to the schema markers, so it cannot drift or be forgotten.

    Every entry must be a reviewed record naming the field, the version that deprecated it and the
    version that removes it; a deprecated marker in the schema without a lock record (or the
    reverse) fails, so the list is never a free-form comment.
    """
    declared = contract["deprecated_fields"]
    if not isinstance(declared, list):
        raise ContractError(f"contract_deprecated_fields_invalid:{contract['id']}")
    names: list[str] = []
    for entry in declared:
        if not isinstance(entry, dict) or not {"field", "since", "remove_in"} <= set(
            entry
        ):
            raise ContractError(
                f"contract_deprecated_field_unreviewed:{contract['id']}:"
                "expected field/since/remove_in"
            )
        if not set(entry) <= DEPRECATED_FIELD_KEYS:
            raise ContractError(
                f"contract_deprecated_field_unreviewed:{contract['id']}:"
                f"unknown keys {sorted(set(entry) - DEPRECATED_FIELD_KEYS)}"
            )
        for key in ("field", "since", "remove_in"):
            if not isinstance(entry[key], str) or not entry[key]:
                raise ContractError(
                    f"contract_deprecated_field_unreviewed:{contract['id']}:{key}"
                )
        names.append(entry["field"])
    expected = expected_deprecated_fields(schema)
    if sorted(names) != expected:
        raise ContractError(
            f"contract_deprecated_fields_unbound:{contract['id']}:"
            f"lock={sorted(names)}:schema={expected}"
        )


def rust_struct(rust_source: str, rust_type: str) -> tuple[str, list[dict[str, Any]]]:
    """Read a hand-written struct header and its fields without compiling anything."""
    match = None
    for candidate in RUST_STRUCT.finditer(rust_source):
        if candidate.group("name") == rust_type:
            match = candidate
    if match is None:
        raise ContractError(f"rust_struct_missing:{rust_type}")
    header_lines: list[str] = []
    for line in reversed(rust_source[: match.start()].splitlines()):
        stripped = line.strip()
        if stripped.startswith("#["):
            header_lines.append(stripped)
            continue
        if not stripped or stripped.startswith("///") or stripped.startswith("//"):
            continue
        break
    fields: list[dict[str, Any]] = []
    pending_default = False
    for line in match.group("body").splitlines():
        stripped = line.strip()
        if stripped.startswith("#[serde("):
            pending_default = "default" in stripped
            continue
        if stripped.startswith("///") or stripped.startswith("//"):
            continue
        field = RUST_FIELD.match(line)
        if field is None:
            continue
        fields.append(
            {
                "name": field.group("name"),
                "type": field.group("type").strip(),
                "default": pending_default,
            }
        )
        pending_default = False
    return " ".join(reversed(header_lines)), fields


def scan_fixture(path: Path, max_bytes: int) -> Any:
    if not path.is_file():
        raise ContractError(f"fixture_missing:{path.relative_to(ROOT)}")
    fixture = read_json(path)
    fixture_bytes = len(
        json.dumps(fixture, ensure_ascii=False, separators=(",", ":")).encode()
    )
    if fixture_bytes > max_bytes:
        raise ContractError(f"fixture_max_bytes_exceeded:{path.relative_to(ROOT)}")
    for location, value in walk_values(fixture):
        if isinstance(value, str):
            if (
                ABSOLUTE_PATH.match(value)
                or "\\" in value
                and re.search(r"[A-Za-z]:", value)
            ):
                raise ContractError(f"fixture_path:{path.relative_to(ROOT)}:{location}")
        if isinstance(value, dict):
            for key in value:
                if SECRET_KEY.search(key):
                    raise ContractError(
                        f"fixture_secret_key:{path.relative_to(ROOT)}:{location}.{key}"
                    )
    return fixture


def validate_examples(
    validator: Any,
    schema: dict[str, Any],
    contract: dict[str, Any],
    schema_path: Path,
) -> None:
    valid_path = SCHEMA_ROOT / safe_relative(contract["valid_example"], "valid_example")
    unknown_path = SCHEMA_ROOT / safe_relative(
        contract["unknown_field_example"], "unknown_field_example"
    )
    scan_fixture(valid_path, contract["max_bytes"])
    scan_fixture(unknown_path, contract["max_bytes"])

    valid = read_json(valid_path)
    errors = validator.validate(schema, valid, schema, "$valid")
    if errors:
        raise ContractError(
            f"fixture_invalid:{valid_path.relative_to(ROOT)}:{errors[0]}"
        )

    unknown = read_json(unknown_path)
    errors = validator.validate(schema, unknown, schema, "$unknown")
    if not errors:
        raise ContractError(
            f"unknown_field_not_rejected:{unknown_path.relative_to(ROOT)}"
        )
    properties = schema.get("properties", {})
    unknown_keys = set(unknown) - set(properties)
    if not unknown_keys:
        raise ContractError(
            f"unknown_fixture_missing_top_level_field:{unknown_path.relative_to(ROOT)}"
        )
    if schema.get("additionalProperties") is not False:
        raise ContractError(f"schema_not_strict:{schema_path.relative_to(ROOT)}")


def validate_rust_binding(contract: dict[str, Any], schema: dict[str, Any]) -> None:
    """Reject drift between the locked schema fields and the hand-written Rust struct."""
    rust_path = ROOT / safe_relative(contract["rust_source"], "rust_source")
    if not rust_path.is_file():
        raise ContractError(f"rust_source_missing:{contract['id']}")
    header, fields = rust_struct(
        rust_path.read_text(encoding="utf-8"), contract["rust_type"]
    )
    if "deny_unknown_fields" not in header:
        raise ContractError(f"rust_unknown_fields_not_denied:{contract['id']}")
    names = [field["name"] for field in fields]
    if len(set(names)) != len(names):
        raise ContractError(f"rust_duplicate_field:{contract['id']}")
    properties = schema.get("properties", {})
    if set(names) != set(properties):
        missing = sorted(set(properties) - set(names))
        extra = sorted(set(names) - set(properties))
        raise ContractError(
            f"rust_field_drift:{contract['id']}:missing={missing}:extra={extra}"
        )
    required = set(schema.get("required", []))
    for field in fields:
        optional_rust = field["default"] or field["type"].startswith("Option<")
        if field["name"] in required or optional_rust:
            continue
        # The schema may omit the field but Rust cannot: a schema-valid payload would fail decode.
        raise ContractError(
            f"schema_optional_field_rust_required:{contract['id']}:{field['name']}"
        )
    for field in fields:
        optional_rust = field["default"] or field["type"].startswith("Option<")
        if optional_rust or field["name"] in required:
            continue
        # Rust cannot omit it, so the wire contract must require it too.
        raise ContractError(
            f"rust_required_field_schema_optional:{contract['id']}:{field['name']}"
        )


def validate_lock(lock: dict[str, Any], validator: Any) -> list[dict[str, Any]]:
    if set(lock) != LOCK_FIELDS:
        raise ContractError(f"lock_fields_invalid:{sorted(set(lock) ^ LOCK_FIELDS)}")
    if lock["$schema"] != SCHEMA_DIALECT or lock["$id"] != lock["schema"]:
        raise ContractError("lock_header_invalid")
    if lock["schema"] != "kiana.ui-schema-lock.v1" or lock["version"] != "1.0":
        raise ContractError("lock_version_invalid")
    if (
        lock["source_of_truth"] != "json_schema"
        or lock["canonical_owner"] != "kiana-protocol"
    ):
        raise ContractError("lock_owner_invalid")
    if not isinstance(lock["contracts"], list) or not lock["contracts"]:
        raise ContractError("lock_contracts_missing")

    contracts = lock["contracts"]
    ids = [entry.get("id") for entry in contracts if isinstance(entry, dict)]
    if any(not isinstance(identifier, str) for identifier in ids) or len(
        set(ids)
    ) != len(ids):
        raise ContractError("lock_contract_ids_invalid")
    if ids != sorted(ids):
        raise ContractError("lock_contracts_not_sorted")

    for contract in contracts:
        if not isinstance(contract, dict) or set(contract) != CONTRACT_FIELDS:
            identifier = (
                contract.get("id", "<unknown>")
                if isinstance(contract, dict)
                else "<unknown>"
            )
            raise ContractError(f"contract_fields_invalid:{identifier}")
        if contract["version"] != "1.0" or contract["owner_crate"] not in {
            "kiana-protocol",
            "kiana-client",
        }:
            raise ContractError(f"contract_header_invalid:{contract['id']}")
        if contract["unknown_fields"] != "deny":
            raise ContractError(f"contract_unknown_policy_invalid:{contract['id']}")
        if not isinstance(contract["max_bytes"], int) or contract["max_bytes"] <= 0:
            raise ContractError(f"contract_max_bytes_invalid:{contract['id']}")
        schema_path = SCHEMA_ROOT / safe_relative(
            contract["schema_file"], "schema_file"
        )
        rust_path = ROOT / safe_relative(contract["rust_source"], "rust_source")
        if not schema_path.is_file() or not rust_path.is_file():
            raise ContractError(f"contract_source_missing:{contract['id']}")
        if contract["rust_type"] not in rust_path.read_text(encoding="utf-8"):
            raise ContractError(f"rust_type_missing:{contract['id']}")
        schema = read_json(schema_path)
        if not isinstance(schema, dict):
            raise ContractError(f"schema_root_invalid:{contract['id']}")
        if (
            schema.get("$schema") != SCHEMA_DIALECT
            or schema.get("$id") != contract["id"]
        ):
            raise ContractError(f"schema_identity_invalid:{contract['id']}")
        if schema.get("additionalProperties") is not False:
            raise ContractError(f"schema_unknown_policy_invalid:{contract['id']}")
        if contract["frozen_shape"] != shape_inventory(schema):
            raise ContractError(
                f"schema_shape_drift:{contract['id']}: a same-version shape change requires a "
                "reviewed lock update and a new schema version"
            )
        validate_deprecated_fields(contract, schema)
        digest = canonical_digest(schema)
        if contract["schema_sha256"] != digest:
            raise ContractError(f"schema_hash_stale:{contract['id']}")
        schema_bytes = len(
            json.dumps(schema, ensure_ascii=False, separators=(",", ":")).encode()
        )
        if schema_bytes > contract["max_bytes"]:
            raise ContractError(f"schema_max_bytes_exceeded:{contract['id']}")
        validate_rust_binding(contract, schema)
        validate_examples(validator, schema, contract, schema_path)

    generated_path = ROOT / safe_relative(lock["generated_rust"], "generated_rust")
    if not generated_path.is_file():
        raise ContractError(
            f"generated_catalog_missing:{generated_path.relative_to(ROOT)}"
        )
    if generated_path.read_text(encoding="utf-8") != generated_catalog(lock):
        raise ContractError("generated_catalog_stale")
    return contracts


def validate_negative_cases(
    matrix: dict[str, Any],
    contracts: dict[str, dict[str, Any]],
    validator: Any,
) -> None:
    negative = matrix.get("negative_cases")
    if not isinstance(negative, list) or not negative:
        raise ContractError("compatibility_matrix_negative_cases_invalid")
    case_ids = [case.get("case_id") for case in negative if isinstance(case, dict)]
    if len(set(case_ids)) != len(negative) or any(
        not isinstance(case, str) for case in case_ids
    ):
        raise ContractError("compatibility_matrix_negative_cases_invalid")
    if not set(case_ids) >= {
        "remove_required_field",
        "change_field_type",
        "unknown_major",
        "unknown_command",
    }:
        raise ContractError("compatibility_matrix_negative_coverage_missing")
    for case in negative:
        if not isinstance(case, dict) or not set(case) <= NEGATIVE_CASE_FIELDS:
            raise ContractError(
                f"compatibility_matrix_negative_case_fields_invalid:{case}"
            )
        case_id = case["case_id"]
        if "fixture" in case:
            if not {"schema", "expected_error"} <= set(case):
                raise ContractError(
                    f"negative_case_fixture_binding_incomplete:{case_id}"
                )
            contract = contracts.get(case["schema"])
            if contract is None:
                raise ContractError(f"negative_case_schema_unknown:{case_id}")
            fixture_path = SCHEMA_ROOT / safe_relative(
                case["fixture"], "negative_case_fixture"
            )
            fixture = scan_fixture(fixture_path, contract["max_bytes"])
            schema = read_json(
                SCHEMA_ROOT / safe_relative(contract["schema_file"], "schema_file")
            )
            errors = validator.validate(schema, fixture, schema, "$negative")
            if not errors:
                raise ContractError(f"negative_case_not_rejected:{case_id}")
            if not any(case["expected_error"] in error for error in errors):
                raise ContractError(
                    f"negative_case_rejected_for_other_reason:{case_id}:{errors[0]}"
                )
        else:
            if not {"enforced_by", "enforced_by_test"} <= set(case):
                raise ContractError(
                    f"negative_case_runtime_binding_incomplete:{case_id}"
                )
            enforced_path = ROOT / safe_relative(case["enforced_by"], "enforced_by")
            if not enforced_path.is_file() or "tests" not in enforced_path.parts:
                raise ContractError(f"negative_case_enforced_by_missing:{case_id}")
            if f"fn {case['enforced_by_test']}(" not in enforced_path.read_text(
                encoding="utf-8"
            ):
                raise ContractError(f"negative_case_enforced_by_test_missing:{case_id}")


def validate_matrix(
    matrix: dict[str, Any],
    contract_ids: set[str],
    contracts: dict[str, Any],
    validator: Any,
) -> None:
    if set(matrix) != MATRIX_FIELDS or matrix.get("$schema") != SCHEMA_DIALECT:
        raise ContractError(
            f"compatibility_matrix_header_invalid:{sorted(set(matrix) ^ MATRIX_FIELDS)}"
        )
    if (
        matrix.get("$id") != matrix.get("schema")
        or matrix.get("schema") != "kiana.ui-compatibility-matrix.v1"
    ):
        raise ContractError("compatibility_matrix_identity_invalid")
    if (
        matrix.get("version") != "1.0"
        or matrix.get("same_version_policy") != "frozen_shape"
    ):
        raise ContractError("compatibility_matrix_policy_invalid")
    if (
        matrix.get("unknown_major_policy") != "reject_before_decode"
        or matrix.get("unknown_command_policy") != "reject_with_schema_unsupported"
    ):
        raise ContractError("compatibility_matrix_fail_open")
    rules = matrix.get("rules")
    if not isinstance(rules, list) or not rules:
        raise ContractError("compatibility_matrix_rules_missing")
    seen: set[tuple[str, str]] = set()
    for rule in rules:
        if not isinstance(rule, dict):
            raise ContractError("compatibility_matrix_rule_invalid")
        pair = (rule.get("from"), rule.get("to"))
        if pair in seen or pair[0] not in contract_ids or pair[1] not in contract_ids:
            raise ContractError("compatibility_matrix_rule_reference_invalid")
        seen.add(pair)
        if (
            pair[0] != pair[1]
            or rule.get("unknown_field_policy") != "deny"
            or rule.get("breaking_change") != "major_version_required"
        ):
            raise ContractError(
                f"compatibility_matrix_rule_not_fail_closed:{pair[0]}->{pair[1]}"
            )
        if (
            rule.get("old_client_reads_new_server") is not True
            or rule.get("new_server_reads_old_client") is not True
        ):
            raise ContractError(f"compatibility_matrix_direction_missing:{pair[0]}")
    if seen != {(identifier, identifier) for identifier in contract_ids}:
        raise ContractError("compatibility_matrix_rule_coverage_missing")
    validate_negative_cases(matrix, contracts, validator)


def validate_asset_coverage(lock: dict[str, Any], matrix: dict[str, Any]) -> None:
    """Every registry asset must be locked, and no lock entry may point at a missing file.

    A new ``*.schema.json`` that sits outside the lock is exactly the "generated file was not
    updated" failure mode: the version contract would not cover it.  The same rule applies to
    example and negative fixtures, so an unreferenced fixture cannot look like evidence.
    """
    locked_schemas = {
        safe_relative(contract["schema_file"], "schema_file").name
        for contract in lock["contracts"]
    }
    on_disk_schemas = {path.name for path in SCHEMA_ROOT.glob("*.schema.json")}
    if locked_schemas != on_disk_schemas:
        raise ContractError(
            f"unlocked_schema_file:"
            f"{sorted(on_disk_schemas - locked_schemas)}:"
            f"missing={sorted(locked_schemas - on_disk_schemas)}"
        )

    referenced_examples = {
        safe_relative(contract["valid_example"], "valid_example").name
        for contract in lock["contracts"]
    } | {
        safe_relative(contract["unknown_field_example"], "unknown_field_example").name
        for contract in lock["contracts"]
    }
    referenced_negative = {
        safe_relative(case["fixture"], "negative_case_fixture").name
        for case in matrix["negative_cases"]
        if isinstance(case, dict) and isinstance(case.get("fixture"), str)
    }
    on_disk_examples = {path.name for path in (SCHEMA_ROOT / "examples").glob("*.json")}
    on_disk_negative = {path.name for path in (SCHEMA_ROOT / "negative").glob("*.json")}
    unreferenced_examples = sorted(on_disk_examples - referenced_examples)
    unreferenced_negative = sorted(on_disk_negative - referenced_negative)
    if unreferenced_examples:
        raise ContractError(f"unreferenced_example_fixture:{unreferenced_examples}")
    if unreferenced_negative:
        raise ContractError(f"unreferenced_negative_fixture:{unreferenced_negative}")


def validate_emitted_report(
    emitted_path: Path,
    contracts: list[dict[str, Any]],
    validator: Any,
) -> None:
    """Validate the Rust-emitted payloads against the canonical JSON Schema.

    The report is produced by `cargo run -p kiana-client --example ui09_schema_emit`; it must carry
    exactly the locked ids, and each emitted payload must satisfy the schema its id is locked to.
    """
    if not emitted_path.is_file():
        raise ContractError(f"emitted_report_missing:{emitted_path}")
    scan_fixture(emitted_path, EMITTED_MAX_BYTES)
    emitted = read_json(emitted_path)
    if not isinstance(emitted, dict) or emitted.get("schema") != EMITTED_REPORT_SCHEMA:
        raise ContractError("emitted_report_invalid")
    values = emitted.get("contracts")
    if not isinstance(values, dict):
        raise ContractError("emitted_report_contracts_invalid")
    locked = {contract["id"]: contract for contract in contracts}
    if set(values) != set(locked):
        raise ContractError(
            f"emitted_contract_set_mismatch:"
            f"missing={sorted(set(locked) - set(values))}:"
            f"extra={sorted(set(values) - set(locked))}"
        )
    for identifier in sorted(values):
        contract = locked[identifier]
        schema = read_json(
            SCHEMA_ROOT / safe_relative(contract["schema_file"], "schema_file")
        )
        errors = validator.validate(
            schema, values[identifier], schema, f"$emitted.{identifier}"
        )
        if errors:
            raise ContractError(f"emitted_schema_mismatch:{identifier}:{errors[0]}")


def update_hashes(lock: dict[str, Any]) -> None:
    for contract in lock["contracts"]:
        schema = read_json(
            SCHEMA_ROOT / safe_relative(contract["schema_file"], "schema_file")
        )
        contract["schema_sha256"] = canonical_digest(schema)
    lock["contracts"] = sorted(lock["contracts"], key=lambda item: item["id"])


def run(write: bool, emitted: Path | None) -> None:
    lock = read_json(LOCK_PATH)
    if not isinstance(lock, dict):
        raise ContractError("lock_root_invalid")
    if write:
        # Only the content hash and the generated catalog are refreshed.  ``frozen_shape`` is
        # deliberately left alone so a same-version shape change keeps failing until reviewed.
        update_hashes(lock)
        write_json(LOCK_PATH, lock)
        generated_path = ROOT / safe_relative(lock["generated_rust"], "generated_rust")
        generated_path.write_text(generated_catalog(lock), encoding="utf-8")
    validator = load_schema_validator()
    contracts = validate_lock(lock, validator)
    matrix = read_json(MATRIX_PATH)
    if not isinstance(matrix, dict):
        raise ContractError("compatibility_matrix_root_invalid")
    validate_matrix(
        matrix,
        {contract["id"] for contract in contracts},
        {contract["id"]: contract for contract in contracts},
        validator,
    )
    validate_asset_coverage(lock, matrix)
    emitted_note = ""
    if emitted is not None:
        validate_emitted_report(emitted, contracts, validator)
        emitted_note = "; emitted payloads satisfy the canonical schema"
    print(
        f"UI-09 schema lock valid: {len(contracts)} contracts; generated catalog is current"
        f"{emitted_note}"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write", action="store_true", help="refresh hashes and generated Rust catalog"
    )
    parser.add_argument(
        "--emitted",
        type=Path,
        default=None,
        help="validate a `ui09_schema_emit` report against the canonical JSON Schema",
    )
    args = parser.parse_args()
    try:
        run(args.write, args.emitted)
    except ContractError as exc:
        print(f"UI-09 schema validation failed: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
