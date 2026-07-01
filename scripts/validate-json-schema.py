#!/usr/bin/env python3
"""Validate JSON files against the JSON Schema subset used by Kiana schemas."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


class ValidationError(Exception):
    pass


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def type_matches(instance: Any, expected: str) -> bool:
    if expected == "object":
        return isinstance(instance, dict)
    if expected == "array":
        return isinstance(instance, list)
    if expected == "string":
        return isinstance(instance, str)
    if expected == "integer":
        return isinstance(instance, int) and not isinstance(instance, bool)
    if expected == "number":
        return isinstance(instance, (int, float)) and not isinstance(instance, bool)
    if expected == "boolean":
        return isinstance(instance, bool)
    if expected == "null":
        return instance is None
    raise ValidationError(f"unsupported schema type {expected!r}")


def resolve_ref(root: dict[str, Any], ref: str) -> dict[str, Any]:
    if not ref.startswith("#/"):
        raise ValidationError(f"unsupported non-local $ref {ref!r}")
    current: Any = root
    for raw_part in ref[2:].split("/"):
        part = raw_part.replace("~1", "/").replace("~0", "~")
        if not isinstance(current, dict) or part not in current:
            raise ValidationError(f"unresolvable $ref {ref!r}")
        current = current[part]
    if not isinstance(current, dict):
        raise ValidationError(f"$ref {ref!r} did not resolve to a schema object")
    return current


def validate(schema: dict[str, Any], instance: Any, root: dict[str, Any], path: str) -> list[str]:
    errors: list[str] = []

    if "$ref" in schema:
        try:
            referenced = resolve_ref(root, schema["$ref"])
        except ValidationError as exc:
            return [f"{path}: {exc}"]
        return validate(referenced, instance, root, path)

    if "allOf" in schema:
        for index, subschema in enumerate(schema["allOf"]):
            errors.extend(validate(subschema, instance, root, f"{path}.allOf[{index}]"))

    if "if" in schema and "then" in schema:
        if not validate(schema["if"], instance, root, path):
            errors.extend(validate(schema["then"], instance, root, f"{path}.then"))

    if "const" in schema and instance != schema["const"]:
        errors.append(f"{path}: expected const {schema['const']!r}, got {instance!r}")

    if "enum" in schema and instance not in schema["enum"]:
        errors.append(f"{path}: expected one of {schema['enum']!r}, got {instance!r}")

    expected_type = schema.get("type")
    if isinstance(expected_type, str):
        try:
            type_ok = type_matches(instance, expected_type)
        except ValidationError as exc:
            errors.append(f"{path}: {exc}")
            type_ok = True
        if not type_ok:
            errors.append(f"{path}: expected type {expected_type}, got {type(instance).__name__}")
            return errors

    if isinstance(instance, dict):
        required = schema.get("required", [])
        for key in required:
            if key not in instance:
                errors.append(f"{path}: missing required property {key!r}")

        properties = schema.get("properties", {})
        if isinstance(properties, dict):
            for key, value in instance.items():
                if key in properties:
                    prop_schema = properties[key]
                    if isinstance(prop_schema, dict):
                        errors.extend(validate(prop_schema, value, root, f"{path}.{key}"))

        additional = schema.get("additionalProperties")
        if additional is False and isinstance(properties, dict):
            allowed = set(properties)
            for key in instance:
                if key not in allowed:
                    errors.append(f"{path}: unexpected property {key!r}")
        elif isinstance(additional, dict) and isinstance(properties, dict):
            for key, value in instance.items():
                if key not in properties:
                    errors.extend(validate(additional, value, root, f"{path}.{key}"))

    if isinstance(instance, list):
        min_items = schema.get("minItems")
        if isinstance(min_items, int) and len(instance) < min_items:
            errors.append(f"{path}: expected at least {min_items} items, got {len(instance)}")
        item_schema = schema.get("items")
        if isinstance(item_schema, dict):
            for index, item in enumerate(instance):
                errors.extend(validate(item_schema, item, root, f"{path}[{index}]"))

    if isinstance(instance, str):
        min_length = schema.get("minLength")
        if isinstance(min_length, int) and len(instance) < min_length:
            errors.append(f"{path}: expected string length >= {min_length}")
        pattern = schema.get("pattern")
        if isinstance(pattern, str) and re.search(pattern, instance) is None:
            errors.append(f"{path}: string does not match pattern {pattern!r}")

    if isinstance(instance, (int, float)) and not isinstance(instance, bool):
        minimum = schema.get("minimum")
        if isinstance(minimum, (int, float)) and instance < minimum:
            errors.append(f"{path}: expected value >= {minimum}, got {instance!r}")

    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("schema", type=Path)
    parser.add_argument("instance", type=Path)
    args = parser.parse_args()

    try:
        schema = load_json(args.schema)
        instance = load_json(args.instance)
    except Exception as exc:  # noqa: BLE001
        print(f"failed to read JSON: {exc}", file=sys.stderr)
        return 2

    if not isinstance(schema, dict):
        print(f"{args.schema}: schema root must be an object", file=sys.stderr)
        return 2

    errors = validate(schema, instance, schema, "$")
    if errors:
        print(f"{args.instance} failed {args.schema}", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1

    print(f"OK: {args.instance} conforms to {args.schema}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
