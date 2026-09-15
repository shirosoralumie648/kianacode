#!/usr/bin/env python3
"""Validate the source-only configuration/credential/identity baseline.

This is intentionally a deterministic CI guard, not a replacement for the
Rust integration tests. It checks that the migration fixtures remain explicit
and that the legacy parser cannot silently become a second production path.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "scripts" / "fixtures" / "config-credentials-identity"


def fail(message: str) -> None:
    print(f"ci-01 baseline failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def load(name: str) -> dict:
    path = FIXTURES / name
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        fail(f"invalid fixture {name}: {exc}")
    if not isinstance(value, dict):
        fail(f"fixture {name} must be an object")
    return value


def main() -> None:
    env = load("env-precedence.json")
    if env.get("provider_precedence") != ["explicit", "environment", "builtin"]:
        fail("provider precedence fixture changed")
    if env.get("provider_specific_env", {}).get("openai", {}).get("api_key") != "OPENAI_API_KEY":
        fail("production OpenAI key environment changed")

    profiles = load("profile-precedence.json")
    if profiles.get("profile_precedence") != ["profile", "explicit_default", "provider_env", "builtin"]:
        fail("profile precedence fixture changed")
    if profiles.get("unknown_field") != "model_profile_config_invalid":
        fail("unknown profile fields are not fail-closed in the fixture")

    local_user = load("legacy-local-user-event.json")
    if local_user.get("compatibility_only") is not True:
        fail("local-user fixture must remain compatibility-only")
    if local_user.get("migration_event_required") is not True:
        fail("local-user fixture must require an explicit migration event")
    if local_user.get("event_kind") is not None:
        fail("CI-01 must not invent an identity migration event")

    migration = load("config-migration-v0.json")
    if migration.get("status") != "deferred_to_ci_06":
        fail("config migration fixture must remain deferred to CI-06")
    if migration.get("implicit_migration") is not False:
        fail("config migration cannot be implicit")

    channels = load("secret-channel-sentinel.json")
    expected_channels = ["debug", "error", "event", "receipt", "argv", "env", "cache"]
    if channels.get("channels") != expected_channels:
        fail("secret-channel coverage changed")
    if channels.get("secret_ref") != "vault://ci01/reference-only":
        fail("secret reference fixture changed")

    model_client = (ROOT / "kiana-daemon" / "src" / "model_client.rs").read_text(
        encoding="utf-8"
    )
    legacy_marker = "#[cfg(test)]\nmod legacy_fixtures"
    if legacy_marker not in model_client:
        fail("legacy provider parser is no longer explicitly test-only")
    product_prefix, _, legacy_body = model_client.partition(legacy_marker)
    if "ProfileRouter" in product_prefix or "ConfiguredProfile" in product_prefix:
        fail("legacy profile parser leaked into the production prefix")
    if "ProfileRouter" not in legacy_body:
        fail("legacy parser fixture disappeared without a migration record")

    provider_config = (ROOT / "kiana-provider" / "src" / "config.rs").read_text(
        encoding="utf-8"
    )
    if "#[serde(deny_unknown_fields)]" not in provider_config:
        fail("provider profile parser is not fail-closed on unknown fields")
    if "KIANA_OPENAI_API_KEY" in provider_config:
        fail("legacy OpenAI environment alias was silently added to production parser")

    for schema_name, schema_id in (
        (
            "kiana-app-server-config-resolved.v1.schema.json",
            "kiana.app-server.config-resolved.v1",
        ),
        ("kiana-app-server-secrets.v1.schema.json", "kiana.app-server.secrets.v1"),
    ):
        schema = load_schema(schema_name)
        if schema.get("$id") != schema_id or schema.get("additionalProperties") is not True:
            fail(f"schema placeholder boundary changed: {schema_name}")

    protocol = (ROOT / "kiana-protocol" / "src" / "lib.rs").read_text(encoding="utf-8")
    daemon = (ROOT / "kiana-daemon" / "src" / "lib.rs").read_text(encoding="utf-8")
    if 'actor_id: Some("local-user".to_owned())' not in protocol:
        fail("protocol local-user compatibility default changed")
    if 'actor_id: "local-user".to_owned()' not in daemon:
        fail("daemon local-user principal baseline changed")

    print("ci-01 baseline fixtures and source boundaries are valid")


def load_schema(name: str) -> dict:
    path = ROOT / "docs" / "schemas" / name
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        fail(f"invalid schema {name}: {exc}")
    if not isinstance(value, dict):
        fail(f"schema {name} must be an object")
    return value


if __name__ == "__main__":
    main()
