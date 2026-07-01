#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

mode="${1:---required}"
if [[ "$mode" != "--required" && "$mode" != "--skip-if-unconfigured" ]]; then
  echo "usage: $0 [--required|--skip-if-unconfigured]" >&2
  exit 2
fi

out_dir="${KIANA_PROVIDER_LIVE_SMOKE_DIR:-${KIANA_LIVE_SMOKE_DIR:-target/live-smoke}/provider}"
mkdir -p "$out_dir"

python_bin() {
  command -v python3 2>/dev/null || command -v python 2>/dev/null || {
    echo "provider live smoke requires python3 or python" >&2
    exit 1
  }
}

run_kiana() {
  if [[ -n "${KIANA_BIN:-}" ]]; then
    "$KIANA_BIN" "$@"
  else
    cargo run -p kiana-entrypoints --bin kiana -- "$@"
  fi
}

provider_configured() {
  [[ -n "${ANTHROPIC_API_KEY:-}" ]] ||
    [[ -n "${KIANA_OPENAI_API_KEY:-}" ]] ||
    [[ -n "${OPENAI_API_KEY:-}" ]] ||
    [[ -n "${KIANA_OLLAMA_BASE_URL:-}" ]] ||
    [[ -n "${OLLAMA_BASE_URL:-}" ]] ||
    [[ -n "${KIANA_OLLAMA_MODEL:-}" ]] ||
    [[ -n "${OLLAMA_MODEL:-}" ]] ||
    [[ "${KIANA_OLLAMA_LIVE_REQUIRED:-0}" == "1" ]]
}

if [[ "$mode" == "--skip-if-unconfigured" ]] && ! provider_configured; then
  echo "provider live smoke skipped: no live provider environment is configured"
  exit 0
fi

if [[ "$mode" == "--required" ]] && ! provider_configured; then
  echo "provider live smoke requires at least one configured live provider: ANTHROPIC_API_KEY, KIANA_OPENAI_API_KEY/OPENAI_API_KEY, or explicit Ollama env" >&2
  exit 1
fi

catalog_json="$out_dir/model-catalog-live.json"
smoke_json="$out_dir/model-smoke-live-tools.json"

run_kiana model catalog --live --json >"$catalog_json"
run_kiana model smoke --live --tools --json >"$smoke_json"

PROVIDER_CATALOG_JSON="$catalog_json" \
PROVIDER_SMOKE_JSON="$smoke_json" \
PROVIDER_LIVE_REQUIRED="$([[ "$mode" == "--required" ]] && printf 1 || printf 0)" \
"$(python_bin)" - <<'PY'
import json
import os
import sys

catalog_path = os.environ["PROVIDER_CATALOG_JSON"]
smoke_path = os.environ["PROVIDER_SMOKE_JSON"]
required = os.environ.get("PROVIDER_LIVE_REQUIRED") == "1"

with open(catalog_path, "r", encoding="utf-8") as handle:
    catalog = json.load(handle)
with open(smoke_path, "r", encoding="utf-8") as handle:
    smoke = json.load(handle)

configured = {
    "anthropic": bool(os.environ.get("ANTHROPIC_API_KEY")),
    "openai-compatible": bool(os.environ.get("KIANA_OPENAI_API_KEY") or os.environ.get("OPENAI_API_KEY")),
    "ollama": bool(
        os.environ.get("KIANA_OLLAMA_BASE_URL")
        or os.environ.get("OLLAMA_BASE_URL")
        or os.environ.get("KIANA_OLLAMA_MODEL")
        or os.environ.get("OLLAMA_MODEL")
        or os.environ.get("KIANA_OLLAMA_LIVE_REQUIRED") == "1"
    ),
}

if catalog.get("schema") != "kiana.model-catalog.v1" or catalog.get("live") is not True:
    print("provider live catalog has an unexpected schema or live flag", file=sys.stderr)
    print(json.dumps(catalog, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)

if (
    smoke.get("schema") != "kiana.model-smoke.v1"
    or smoke.get("live") is not True
    or smoke.get("tools") is not True
):
    print("provider live smoke has an unexpected schema, live flag, or tools flag", file=sys.stderr)
    print(json.dumps(smoke, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)

if required and not any(configured.values()):
    print(
        "provider live smoke requires at least one configured live provider "
        "(ANTHROPIC_API_KEY, KIANA_OPENAI_API_KEY/OPENAI_API_KEY, or explicit Ollama env)",
        file=sys.stderr,
    )
    sys.exit(1)

results = smoke.get("results", [])
providers = catalog.get("providers", [])

def configured_provider_failed(item):
    provider_id = item.get("provider_id")
    return configured.get(provider_id, False) and item.get("status") != "passed"

failed_smoke = [item for item in results if configured_provider_failed(item)]
if failed_smoke:
    print("configured provider smoke did not pass", file=sys.stderr)
    print(json.dumps(failed_smoke, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)

failed_catalog = [
    item
    for item in providers
    if item.get("provider_id") in {"openai-compatible", "ollama"}
    and configured.get(item.get("provider_id"), False)
    and item.get("status") != "passed"
]
if failed_catalog:
    print("configured live model catalog lookup did not pass", file=sys.stderr)
    print(json.dumps(failed_catalog, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)

live_text_passed = any(
    item.get("provider_id") != "fake"
    and item.get("live") is True
    and item.get("capability") == "text"
    and item.get("status") == "passed"
    for item in results
)
live_tools_passed = any(
    item.get("provider_id") != "fake"
    and item.get("live") is True
    and item.get("capability") == "tools"
    and item.get("status") == "passed"
    for item in results
)

if required and not live_text_passed:
    print("provider live smoke did not pass any real text provider", file=sys.stderr)
    print(json.dumps(smoke, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)

if required and not live_tools_passed:
    print("provider live smoke did not pass any real tool-call provider", file=sys.stderr)
    print(json.dumps(smoke, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)

print(
    "provider live smoke passed: "
    f"text={live_text_passed} tools={live_tools_passed} "
    f"catalog={catalog_path} smoke={smoke_path}"
)
PY
