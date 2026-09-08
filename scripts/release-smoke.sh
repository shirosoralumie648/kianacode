#!/usr/bin/env bash
set -Eeuo pipefail

cd "$(dirname "$0")/.."

trap 'status=$?; echo "release smoke failed at line $LINENO: $BASH_COMMAND" >&2; exit "$status"' ERR

if [[ "${KIANA_RELEASE_SMOKE_SKIP_BUILD_GATES:-0}" != "1" ]]; then
  cargo fmt --all --check
  cargo test --workspace --locked --offline --no-fail-fast -- --test-threads=1
  cargo test -p kiana-entrypoints --locked --offline team_runtime_parity_smoke_links_team_tools_resident_loop_and_shutdown
  cargo test -p kiana-tools --locked --offline notebook_execute
  bash scripts/product-shell-smoke.sh
  cargo build --release --locked --offline -p kiana-entrypoints --bin kiana
fi

exe_ext() {
  case "$(uname -s)" in
    MSYS*|MINGW*|CYGWIN*) printf '.exe' ;;
    *) printf '' ;;
  esac
}

real_home="${HOME:-}"
if [[ -n "${CARGO_HOME:-}" ]]; then
  real_cargo_home="$CARGO_HOME"
else
  cargo_path="$(command -v cargo 2>/dev/null || true)"
  if [[ -n "$cargo_path" ]]; then
    real_cargo_home="$(dirname "$(dirname "$cargo_path")")"
  else
    real_cargo_home="$real_home/.cargo"
  fi
fi
if [[ -n "${RUSTUP_HOME:-}" ]]; then
  real_rustup_home="$RUSTUP_HOME"
elif [[ "$real_cargo_home" == */.cargo ]]; then
  real_rustup_home="${real_cargo_home%/.cargo}/.rustup"
else
  real_rustup_home="$real_home/.rustup"
fi
env_cmd="$(command -v env)"
binary_child_path="${PATH:-}"
case "$(uname -s)" in
  MSYS*|MINGW*|CYGWIN*)
    binary_child_path=""
    append_binary_dir() {
      local tool_dir="$1"
      if [[ -d "$tool_dir" ]]; then
        if [[ -z "$binary_child_path" ]]; then
          binary_child_path="$tool_dir"
        else
          binary_child_path="$binary_child_path:$tool_dir"
        fi
      fi
    }
    append_binary_tool_dir() {
      local tool_path
      tool_path="$(command -v "$1" 2>/dev/null || true)"
      if [[ -n "$tool_path" ]]; then
        append_binary_dir "$(dirname "$tool_path")"
      fi
    }
    append_binary_dir "$real_cargo_home/bin"
    append_binary_tool_dir cargo
    append_binary_tool_dir git
    binary_child_path="$binary_child_path:/usr/bin:/bin:/c/Windows/System32:/c/Windows"
    ;;
esac
if [[ "${KIANA_RELEASE_SMOKE_DEBUG:-0}" == "1" ]]; then
  echo "binary_child_path=$binary_child_path" >&2
fi
tmp_root="$(mktemp -d)"
install_dir="$tmp_root/install"
smoke_home="$tmp_root/home"
trap 'rm -rf "$tmp_root"' EXIT

native_env_path() {
  local path="$1"

  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$path" 2>/dev/null || printf '%s\n' "$path"
  else
    printf '%s\n' "$path"
  fi
}

path_variants() {
  local path="$1"

  printf '%s\n' "$path"
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -m "$path" 2>/dev/null || true
    native_env_path "$path"
  fi
}

assert_output_path() {
  local output="$1"
  local prefix="$2"
  local path="$3"
  local candidate

  while IFS= read -r candidate; do
    if [[ -n "$candidate" ]] && grep -Fq -- "$prefix$candidate" <<<"$output"; then
      return 0
    fi
  done < <(path_variants "$path")

  echo "release smoke path assertion failed: ${prefix}${path}" >&2
  echo "$output" >&2
  exit 1
}

assert_exact_output_path() {
  local output="$1"
  local path="$2"
  local candidate

  while IFS= read -r candidate; do
    if [[ -n "$candidate" ]] && grep -Fxq -- "$candidate" <<<"$output"; then
      return 0
    fi
  done < <(path_variants "$path")

  echo "release smoke exact path assertion failed: $path" >&2
  echo "$output" >&2
  exit 1
}

assert_json_path() {
  local output="$1"
  local field="$2"
  local path="$3"
  local candidate
  local escaped

  while IFS= read -r candidate; do
    if [[ -z "$candidate" ]]; then
      continue
    fi
    escaped="${candidate//\\/\\\\}"
    if grep -Fq -- "\"$field\": \"$escaped\"" <<<"$output"; then
      return 0
    fi
  done < <(path_variants "$path")

  echo "release smoke JSON path assertion failed: $field=$path" >&2
  echo "$output" >&2
  exit 1
}

run_clean_kiana() {
  local path_for_child="${PATH:-}"
  case "${1:-}" in
    *kiana.exe) path_for_child="$binary_child_path" ;;
  esac

  local status=0
  HOME="$smoke_home" \
  CARGO_HOME="$real_cargo_home" \
  RUSTUP_HOME="$real_rustup_home" \
  PATH="$path_for_child" \
  KIANA_HOME="$smoke_home/.kiana" \
  KIANA_CONFIG_FILE="$smoke_home/.kiana/config.toml" \
  KIANA_HOOKS_FILE="$smoke_home/.kiana/hooks.json" \
  KIANA_PERMISSIONS_FILE="$smoke_home/.kiana/permissions.json" \
  KIANA_PLUGINS_DIR="$smoke_home/.kiana/plugins" \
  KIANA_TASKS_ROOT="$smoke_home/.kiana/tasks" \
  "$env_cmd" \
    -u ANTHROPIC_AUTH_TOKEN \
    -u ANTHROPIC_API_KEY \
    -u ANTHROPIC_BASE_URL \
    -u ANTHROPIC_MODEL \
    -u KIANA_PROVIDER_SMOKE_LIVE \
    -u KIANA_PROVIDER_SMOKE_TOOLS \
    -u KIANA_MODEL_CATALOG_LIVE \
    -u KIANA_REMOTE_ACCESS_TOKEN \
    -u CLAUDE_ACCESS_TOKEN \
    -u KIANA_OAUTH_TOKENS_FILE \
    -u CLAUDE_CODE_OAUTH_TOKENS_FILE \
    -u KIANA_OAUTH_CLIENT_ID \
    -u KIANA_OAUTH_AUTH_URL \
    -u KIANA_OAUTH_TOKEN_URL \
    -u KIANA_OAUTH_REDIRECT_URI \
    "$@" || status=$?

  return "$status"
}

validate_project_trust_status_json() {
  local output="$1"
  local expected_trust="$2"
  local expected_source="$3"
  local expected_file_status="$4"
  local project="$5"
  local store_dir="$6"
  local python_bin

  python_bin="$(command -v python3 2>/dev/null || command -v python 2>/dev/null || true)"
  if [[ -z "$python_bin" ]]; then
    echo "project trust smoke requires python3 or python" >&2
    exit 1
  fi

  PROJECT_TRUST_JSON="$output" \
  EXPECTED_TRUST="$expected_trust" \
  EXPECTED_SOURCE="$expected_source" \
  EXPECTED_FILE_STATUS="$expected_file_status" \
  EXPECTED_PROJECT="$project" \
  EXPECTED_STORE_DIR="$store_dir" \
  "$python_bin" - <<'PY'
import json
import os
import re

payload = json.loads(os.environ["PROJECT_TRUST_JSON"])
expected_trust = os.environ["EXPECTED_TRUST"]
expected_source = os.environ["EXPECTED_SOURCE"]
expected_file_status = os.environ["EXPECTED_FILE_STATUS"]

def normalized(path):
    return str(path).replace("\\", "/").rstrip("/").casefold()

project = normalized(os.environ["EXPECTED_PROJECT"])
store_dir = normalized(os.environ["EXPECTED_STORE_DIR"])
file_info = payload["file"]
legacy = payload["legacy_project_file"]
trusted = expected_trust == "trusted"

checks = [
    payload.get("schema") == "kiana.app-server.trust-status.v1",
    payload.get("project_trust") == expected_trust,
    payload.get("project_trusted") is trusted,
    payload.get("allows_project_resources") is trusted,
    payload.get("source") == expected_source,
    isinstance(payload.get("project_id"), str)
    and re.fullmatch(r"[0-9a-f]{64}", payload["project_id"]) is not None,
    normalized(payload.get("project_root", "")) == project,
    file_info.get("status") == expected_file_status,
    file_info.get("exists") is (expected_file_status == "found"),
    file_info.get("error") is None,
    isinstance(file_info.get("path"), str),
    normalized(file_info.get("path", "")).startswith(store_dir + "/"),
    normalized(file_info.get("path", ""))
    == normalized(store_dir + "/" + payload["project_id"] + ".json"),
    normalized(legacy.get("path", "")) == project + "/.kiana/trust.json",
    legacy.get("exists") is True,
    legacy.get("ignored") is True,
    legacy.get("reason") == "project_local_trust_is_not_authoritative",
]
if not all(checks):
    raise SystemExit(
        "project trust lifecycle assertion failed\n"
        + json.dumps(payload, indent=2, sort_keys=True)
    )
PY
}

smoke_project_trust() {
  local binary="$1"
  local binary_path
  local project="$tmp_root/project-trust-smoke"
  local store_dir="$smoke_home/.kiana/trust/projects"
  local output

  binary_path="$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary")"
  rm -rf "$project"
  mkdir -p "$project/.git" "$project/.kiana"
  printf '%s\n' '{"trusted":true}' > "$project/.kiana/trust.json"

  output="$(cd "$project" && run_clean_kiana "$binary_path" trust json)"
  validate_project_trust_status_json "$output" unknown default missing "$project" "$store_dir"

  output="$(cd "$project" && run_clean_kiana "$binary_path" trust trust)"
  grep -Fq -- "project_trust: trusted" <<<"$output"
  output="$(cd "$project" && run_clean_kiana "$binary_path" trust json)"
  validate_project_trust_status_json "$output" trusted user_store found "$project" "$store_dir"

  output="$(cd "$project" && run_clean_kiana "$binary_path" trust reset)"
  grep -Fq -- "project_trust: unknown" <<<"$output"
  output="$(cd "$project" && run_clean_kiana "$binary_path" trust json)"
  validate_project_trust_status_json "$output" unknown default missing "$project" "$store_dir"
}

install_release_binary() {
  if command -v make >/dev/null 2>&1; then
    INSTALL_DIR="$install_dir" KIANA_SKIP_PATH_SETUP=1 CARGO_NET_OFFLINE=true run_clean_kiana make install
  else
    INSTALL_DIR="$install_dir" KIANA_SKIP_PATH_SETUP=1 CARGO_NET_OFFLINE=true run_clean_kiana bash install.sh
  fi
}

smoke_help_usage() {
  local binary="$1"
  local entry="$2"
  local command="${entry%%::*}"
  local expected="${entry#*::}"
  local output
  # shellcheck disable=SC2206
  local args=($command)

  output="$(run_clean_kiana "$binary" "${args[@]}" 2>&1 || true)"
  if ! grep -Fq -- "$expected" <<<"$output"; then
    echo "help smoke failed for: $command" >&2
    echo "missing expected text: $expected" >&2
    echo "$output" >&2
    exit 1
  fi
}

smoke_doctor() {
  local binary="$1"
  local output

  output="$(run_clean_kiana "$binary" doctor 2>&1 || true)"
  for expected in \
    "Doctor" \
    "cargo:" \
    "api_key_set: no" \
    "remote_code_session: live_smoke_token=no"
  do
    if ! grep -Fq -- "$expected" <<<"$output"; then
      echo "doctor smoke failed for: $binary" >&2
      echo "missing expected text: $expected" >&2
      echo "$output" >&2
      exit 1
    fi
  done
}

doctor_json_python() {
  command -v python3 2>/dev/null || command -v python 2>/dev/null || {
    echo "doctor JSON smoke requires python3 or python" >&2
    exit 1
  }
}

smoke_doctor_json() {
  local binary="$1"
  local output
  local python_bin

  output="$(run_clean_kiana "$binary" doctor --json 2>&1)"
  python_bin="$(doctor_json_python)"
  DOCTOR_JSON="$output" "$python_bin" - <<'PY'
import json
import os
import sys

try:
    report = json.loads(os.environ["DOCTOR_JSON"])
except Exception as exc:
    print(f"doctor JSON is not valid JSON: {exc}", file=sys.stderr)
    print(os.environ.get("DOCTOR_JSON", ""), file=sys.stderr)
    sys.exit(1)

required = [
    "schema",
    "status",
    "cargo",
    "git_root",
    "remote_code_session",
    "oauth_token_file",
    "bash_sandbox",
    "commercial_security",
    "tool_parity",
    "reference_capabilities",
    "warnings",
]
missing = [key for key in required if key not in report]
if missing:
    print(f"doctor JSON missing keys: {missing}", file=sys.stderr)
    sys.exit(1)

checks = [
    report.get("schema") == "kiana.doctor.v1",
    isinstance(report.get("warnings"), list),
    isinstance(report.get("cargo", {}).get("available"), bool),
    isinstance(report.get("remote_code_session", {}).get("live_smoke_token"), str),
    isinstance(report.get("oauth_token_file", {}).get("status"), str),
    isinstance(report.get("bash_sandbox", {}).get("enabled"), bool),
    isinstance(report.get("commercial_security", {}).get("platform"), str),
    report.get("commercial_security", {}).get("isolation") in [
        "linux_bwrap",
        "windows_exec_policy",
        "macos_exec_policy",
        "unsupported_platform",
    ],
    isinstance(report.get("commercial_security", {}).get("controls"), list),
    isinstance(report.get("commercial_security", {}).get("issues"), list),
    report.get("tool_parity", {}).get("schema") == "kiana.tool-parity.v1",
    report.get("tool_parity", {}).get("source") == "builtin-registry",
    report.get("tool_parity", {}).get("plugin_tools_included") is False,
    isinstance(report.get("tool_parity", {}).get("built_in_tools"), list),
    report.get("tool_parity", {}).get("total_tools", 0) >= 40,
    any(
        isinstance(item, dict)
        and item.get("name") == "Read"
        and item.get("source") == "builtin"
        and item.get("read_only") is True
        and item.get("concurrency_safe") is True
        for item in report.get("tool_parity", {}).get("built_in_tools", [])
    ),
    any(
        isinstance(item, dict)
        and item.get("name") == "MCP"
        and item.get("workbench") == "mcp"
        for item in report.get("tool_parity", {}).get("built_in_tools", [])
    ),
    not any(
        isinstance(item, dict)
        and item.get("name") == "review-tools:code-audit"
        for item in report.get("tool_parity", {}).get("built_in_tools", [])
    ),
    isinstance(report.get("reference_capabilities"), list),
    any(
        isinstance(item, dict)
        and item.get("id") == "provider-registry"
        and item.get("status") == "local_ready_external_required"
        for item in report.get("reference_capabilities", [])
    ),
    any(
        isinstance(item, dict)
        and item.get("id") == "remote-commercial-release"
        and item.get("status") in ["external_required", "local_ready_external_required"]
        for item in report.get("reference_capabilities", [])
    ),
]
if not all(checks):
    print("doctor JSON failed schema smoke checks", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
PY
}

smoke_commercial_security_doctor_json() {
  local binary="$1"
  local output
  local python_bin
  local fake_bwrap=""
  local settings_json=""

  case "$(uname -s)" in
    Linux*)
      fake_bwrap="$tmp_root/commercial-security-bwrap"
      printf '#!/bin/sh\n' > "$fake_bwrap"
      chmod +x "$fake_bwrap"
      settings_json="{\"sandbox\":{\"enabled\":true,\"failIfUnavailable\":true,\"allowUnsandboxedCommands\":false,\"bwrapPath\":\"$fake_bwrap\"}}"
      ;;
  esac

  if [[ -n "$settings_json" ]]; then
    output="$(
      unset KIANA_PERMISSION_MODE
      KIANA_PERMISSION_PROFILE=commercial \
        KIANA_SETTINGS_JSON="$settings_json" \
        run_clean_kiana "$binary" doctor --json 2>&1
    )"
  else
    output="$(
      unset KIANA_PERMISSION_MODE KIANA_SETTINGS_JSON
      KIANA_PERMISSION_PROFILE=commercial \
        run_clean_kiana "$binary" doctor --json 2>&1
    )"
  fi
  python_bin="$(doctor_json_python)"
  DOCTOR_JSON="$output" "$python_bin" - <<'PY'
import json
import os
import sys

try:
    report = json.loads(os.environ["DOCTOR_JSON"])
except Exception as exc:
    print(f"commercial doctor JSON is not valid JSON: {exc}", file=sys.stderr)
    print(os.environ.get("DOCTOR_JSON", ""), file=sys.stderr)
    sys.exit(1)

commercial = report.get("commercial_security", {})
capabilities = report.get("reference_capabilities", [])
controls = set(commercial.get("controls", []))
required_controls = {
    "permission_profile:commercial",
    "permission_mode:ask",
    "exec_policy:bash+powershell",
    "exec_policy:destructive-root-sync-deny",
}
security_ready = any(
    isinstance(item, dict)
    and item.get("id") == "security-policy"
    and item.get("status") == "ready"
    for item in capabilities
)
checks = [
    report.get("schema") == "kiana.doctor.v1",
    commercial.get("ready") is True,
    commercial.get("status") == "ready",
    commercial.get("issues") == [],
    required_controls.issubset(controls),
    commercial.get("isolation")
    in ["linux_bwrap", "windows_exec_policy", "macos_exec_policy"],
    security_ready,
]
if not all(checks):
    print("commercial doctor JSON failed readiness smoke checks", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
PY
}

smoke_model_smoke_json() {
  local binary="$1"
  local output
  local python_bin

  output="$(run_clean_kiana "$binary" model smoke --json 2>&1)"
  python_bin="$(doctor_json_python)"
  MODEL_SMOKE_JSON="$output" "$python_bin" - <<'PY'
import json
import os
import sys

try:
    report = json.loads(os.environ["MODEL_SMOKE_JSON"])
except Exception as exc:
    print(f"model smoke JSON is not valid JSON: {exc}", file=sys.stderr)
    print(os.environ.get("MODEL_SMOKE_JSON", ""), file=sys.stderr)
    sys.exit(1)

if report.get("schema") != "kiana.model-smoke.v1":
    print("model smoke schema mismatch", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)

if report.get("tools") is not False:
    print("default model smoke should not run tool smoke unless --tools is set", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)

results = report.get("results")
if not isinstance(results, list):
    print("model smoke results missing", file=sys.stderr)
    sys.exit(1)

fake = next((item for item in results if item.get("provider_id") == "fake"), None)
if fake is None or fake.get("status") != "passed":
    print("fake provider smoke did not pass", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)

summary = report.get("summary", {})
if summary.get("failed") != 0 or summary.get("passed", 0) < 1:
    print("model smoke summary failed default gate", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
PY
}

smoke_model_catalog_json() {
  local binary="$1"
  local output
  local python_bin

  output="$(run_clean_kiana "$binary" model catalog --json 2>&1)"
  python_bin="$(doctor_json_python)"
  MODEL_CATALOG_JSON="$output" "$python_bin" - <<'PY'
import json
import os
import sys

try:
    report = json.loads(os.environ["MODEL_CATALOG_JSON"])
except Exception as exc:
    print(f"model catalog JSON is not valid JSON: {exc}", file=sys.stderr)
    print(os.environ.get("MODEL_CATALOG_JSON", ""), file=sys.stderr)
    sys.exit(1)

if report.get("schema") != "kiana.model-catalog.v1":
    print("model catalog schema mismatch", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)

if report.get("live") is not False:
    print("default model catalog should stay offline unless --live is set", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)

providers = report.get("providers")
if not isinstance(providers, list):
    print("model catalog providers missing", file=sys.stderr)
    sys.exit(1)

summary = report.get("summary", {})
if summary.get("failed") != 0 or summary.get("providers") != 4:
    print("model catalog summary failed default gate", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)

fake = next((item for item in providers if item.get("provider_id") == "fake"), None)
openai = next((item for item in providers if item.get("provider_id") == "openai-compatible"), None)
ollama = next((item for item in providers if item.get("provider_id") == "ollama"), None)
if fake is None or fake.get("status") != "static":
    print("fake provider catalog should be static", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
if openai is None or openai.get("status") != "skipped":
    print("openai-compatible catalog should skip live lookup by default", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
if ollama is None or ollama.get("status") != "skipped":
    print("ollama catalog should skip live lookup by default", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
PY
}

smoke_auto_mode_fake_critique() {
  local binary="$1"
  local output
  local settings_json

  settings_json='{"settings":{"autoMode":{"allow":["Run cargo test in this repository"],"soft_deny":["Do not delete user files without explicit approval"],"environment":["Use deterministic fake provider critique in release smoke"]}}}'
  output="$(KIANA_SETTINGS_JSON="$settings_json" run_clean_kiana "$binary" auto-mode critique --model fake 2>&1)"
  for expected in \
    "mode: fake provider critique" \
    "provider: fake" \
    "model: fake-model" \
    "model_findings:" \
    "Fake provider critique: reviewed custom auto mode rules without network."
  do
    if ! grep -Fq -- "$expected" <<<"$output"; then
      echo "auto-mode fake provider critique smoke failed for: $binary" >&2
      echo "missing expected text: $expected" >&2
      echo "$output" >&2
      exit 1
    fi
  done
  if grep -Fq -- "not contacted" <<<"$output" ||
    grep -Fq -- "remaining parity gap" <<<"$output"; then
    echo "auto-mode fake provider critique stayed on local-only fallback for: $binary" >&2
    echo "$output" >&2
    exit 1
  fi
}

smoke_release_blockers_json() {
  local binary="$1"
  local output
  local python_bin

  python_bin="$(doctor_json_python)"
  if ! output="$(KIANA_PYTHON_BIN="$python_bin" run_clean_kiana "$binary" release blockers --json 2>&1)"; then
    echo "release blockers CLI failed for: $binary" >&2
    echo "$output" >&2
    return 1
  fi
  RELEASE_BLOCKERS_JSON="$output" "$python_bin" - <<'PY'
import json
import os
import sys

try:
    report = json.loads(os.environ["RELEASE_BLOCKERS_JSON"])
except Exception as exc:
    print(f"release blockers JSON is not valid JSON: {exc}", file=sys.stderr)
    print(os.environ.get("RELEASE_BLOCKERS_JSON", ""), file=sys.stderr)
    sys.exit(1)

if report.get("schema") != "kiana.commercial-release-blockers.v1":
    print("release blockers schema mismatch", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)

summary = report.get("summary", {})
if not isinstance(summary, dict) or "blocking" not in summary or "local_blocking" not in summary:
    print("release blockers summary missing blocker counts", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
if not isinstance(report.get("checks"), list):
    print("release blockers checks missing", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
PY
}

smoke_release_evidence_json() {
  local binary="$1"
  local fixture_dist="$tmp_root/release-evidence-fixture"
  local output
  local python_bin

  rm -rf "$fixture_dist"
  mkdir -p "$fixture_dist/proofs"
  cat > "$fixture_dist/proofs/local-rc-evidence.json" <<'JSON'
{
  "schema": "kiana.local-rc-evidence.v1",
  "status": "local_rc_ready",
  "dist_dir": "release-smoke-fixture",
  "summary": {
    "release_artifacts": 1,
    "manifests": 2,
    "proofs": 3,
    "blockers_total": 4,
    "local_blockers": 0,
    "external_blockers": 4
  },
  "readiness": {
    "ready": true
  },
  "blockers": {
    "handoff_status": "external_action_required",
    "blocking_by_resolution_scope": {
      "release-owner": 2,
      "live-service": 2
    }
  }
}
JSON

  if ! output="$(run_clean_kiana "$binary" release evidence --json --dist-dir "$fixture_dist" 2>&1)"; then
    echo "release evidence CLI failed for: $binary" >&2
    echo "$output" >&2
    return 1
  fi
  python_bin="$(doctor_json_python)"
  RELEASE_EVIDENCE_JSON="$output" "$python_bin" - <<'PY'
import json
import os
import sys

try:
    report = json.loads(os.environ["RELEASE_EVIDENCE_JSON"])
except Exception as exc:
    print(f"release evidence JSON is not valid JSON: {exc}", file=sys.stderr)
    print(os.environ.get("RELEASE_EVIDENCE_JSON", ""), file=sys.stderr)
    sys.exit(1)

if report.get("schema") != "kiana.local-rc-evidence.v1":
    print("release evidence schema mismatch", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
if report.get("readiness", {}).get("ready") is not True:
    print("release evidence readiness missing", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
summary = report.get("summary", {})
if summary.get("local_blockers") != 0:
    print("release evidence local blocker count mismatch", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
scopes = report.get("blockers", {}).get("blocking_by_resolution_scope")
if not isinstance(scopes, dict) or not scopes:
    print("release evidence resolution-scope counts missing", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
PY
}

smoke_eval_json() {
  local binary="$1"
  local suite
  local baseline
  local output
  local python_bin
  local fixture_dir

  fixture_dir="$(mktemp -d)"
  cat > "$fixture_dir/basic-runtime-suite.json" <<'JSON'
{
  "schema": "kiana.eval-suite.v1",
  "id": "basic-runtime",
  "description": "Offline RuntimeEvent replay contract",
  "cases": [
    {
      "id": "tool-success",
      "kind": "runtime_event_replay",
      "fixture": "basic-tool-success.jsonl",
      "expect": {
        "final_status": "completed",
        "stop_reason": "end_turn",
        "min_event_count": 5,
        "tool_call_count": 1,
        "tool_error_count": 0,
        "required_tool_names": ["Read"],
        "final_text_contains": ["done"],
        "max_input_tokens": 32,
        "max_output_tokens": 16
      }
    }
  ]
}
JSON
  cat > "$fixture_dir/basic-runtime-baseline.json" <<'JSON'
{
  "schema": "kiana.eval-baseline.v1",
  "suite_id": "basic-runtime",
  "description": "Local protocol-regression baseline for the packaged RuntimeEvent replay fixture.",
  "cases": {
    "tool-success": {
      "max_event_count": 5,
      "max_tool_call_count": 1,
      "max_tool_result_count": 1,
      "max_tool_error_count": 0,
      "max_input_tokens": 32,
      "max_output_tokens": 16,
      "required_status": "completed",
      "required_stop_reason": "end_turn"
    }
  }
}
JSON
  cat > "$fixture_dir/basic-tool-success.jsonl" <<'JSONL'
{"type":"assistant","assistant_text":"starting"}
{"type":"tool_call","tool_name":"Read","tool_call_id":"call-1"}
{"type":"tool_result","tool_name":"Read","tool_call_id":"call-1","is_error":false}
{"type":"usage","input_tokens":12,"output_tokens":4}
{"type":"result","status":"completed","stop_reason":"end_turn","assistant_text":"done"}
JSONL
  suite="$fixture_dir/basic-runtime-suite.json"
  baseline="$fixture_dir/basic-runtime-baseline.json"
  if ! output="$(run_clean_kiana "$binary" eval run --suite "$suite" --baseline "$baseline" --json --fail-on-failure 2>&1)"; then
    echo "offline eval CLI failed for: $binary" >&2
    echo "$output" >&2
    return 1
  fi
  python_bin="$(doctor_json_python)"
  KIANA_EVAL_JSON="$output" "$python_bin" - <<'PY'
import json
import os
import sys

try:
    report = json.loads(os.environ["KIANA_EVAL_JSON"])
except Exception as exc:
    print(f"offline eval JSON is not valid JSON: {exc}", file=sys.stderr)
    print(os.environ.get("KIANA_EVAL_JSON", ""), file=sys.stderr)
    sys.exit(1)

if report.get("schema") != "kiana.eval-report.v1":
    print("offline eval schema mismatch", file=sys.stderr)
    sys.exit(1)
if report.get("status") != "passed":
    print("offline eval status mismatch", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
baseline = report.get("baseline")
if not baseline or baseline.get("schema") != "kiana.eval-baseline.v1":
    print("offline eval baseline missing", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
if baseline.get("status") != "passed":
    print("offline eval baseline status mismatch", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
PY
  rm -rf "$fixture_dir"
}

smoke_eda_json() {
  local binary="$1"
  local binary_path
  local project_dir
  local output
  local python_bin
  local eda_identity
  local run_id
  local review_id
  local review_path

  binary_path="$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary")"
  project_dir="$(mktemp -d)"
  mkdir -p "$project_dir/hardware/gerber"
  printf '%s\n' '5 V input; 3.3 V rail; SWD bring-up' > "$project_dir/hardware/requirements.md"
  printf '%s\n' '(kicad_sch (version 20231120) (generator release-smoke))' > "$project_dir/hardware/main.kicad_sch"
  printf '%s\n' 'Designator,MPN,Package,Quantity' 'R1,RC0402FR-0710KL,0402,1' 'U1,STM32F103C8T6,LQFP48,1' > "$project_dir/hardware/bom.csv"
  printf '%s\n' 'Designator,Package,Mid X,Mid Y,Rotation,Layer' 'R1,0402,10,8,0,Top' 'U1,LQFP48,20,15,90,Top' > "$project_dir/hardware/cpl.csv"
  printf '%s\n' 'G04 copper*' 'M02*' > "$project_dir/hardware/gerber/demo-F_Cu.gbr"
  printf '%s\n' 'G04 edge*' 'M02*' > "$project_dir/hardware/gerber/demo-Edge_Cuts.gbr"
  printf '%s\n' 'M48' 'M30' > "$project_dir/hardware/gerber/demo-PTH.drl"
  printf '%s\n' '2 layers; minimum trace 0.15 mm' > "$project_dir/hardware/constraints.md"
  printf '%s\n' \
    '<?xml version="1.0" encoding="UTF-8"?>' \
    '<export>' \
    '  <components>' \
    '    <comp ref="R1"><value>10k</value></comp>' \
    '    <comp ref="U1"><value>STM32F103C8T6</value></comp>' \
    '  </components>' \
    '  <nets>' \
    '    <net code="1" name="+3V3">' \
    '      <node ref="U1" pin="1" pintype="power_in" />' \
    '      <node ref="U1" pin="2" pintype="power_out" />' \
    '    </net>' \
    '    <net code="2" name="SWDIO">' \
    '      <node ref="U1" pin="3" pintype="bidirectional" />' \
    '      <node ref="R1" pin="1" pintype="passive" />' \
    '    </net>' \
    '  </nets>' \
    '</export>' > "$project_dir/hardware/main.xml"
  if ! output="$(
    cd "$project_dir"
    run_clean_kiana "$binary_path" eda review --json \
      --requirements hardware/requirements.md \
      --schematic hardware/main.kicad_sch \
      --bom hardware/bom.csv \
      --gerber hardware/gerber \
      --cpl hardware/cpl.csv \
      --constraints hardware/constraints.md \
      --netlist hardware/main.xml 2>&1
  )"; then
    echo "EDA CLI failed for: $binary" >&2
    echo "$output" >&2
    rm -rf "$project_dir"
    return 1
  fi
  python_bin="$(doctor_json_python)"
  eda_identity="$(
    KIANA_EDA_JSON="$output" \
      KIANA_EDA_PROJECT="$(native_env_path "$project_dir")" \
      "$python_bin" - <<'PY'
import json
import os
import sys
from pathlib import Path

try:
    report = json.loads(os.environ["KIANA_EDA_JSON"])
except Exception as exc:
    print(f"EDA JSON is not valid JSON: {exc}", file=sys.stderr)
    sys.exit(1)
if report.get("schema") != "kiana.eda-review.v1" or report.get("status") != "pass":
    print("EDA review status mismatch", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
summary = report.get("summary", {})
expected_summary = {
    "netlist_components": 2,
    "net_count": 2,
    "power_net_count": 1,
    "interface_net_count": 1,
    "dangling_net_count": 0,
}
if report.get("rule_version") != "eda-review-rules.v2" or any(
    summary.get(key) != value for key, value in expected_summary.items()
):
    print("EDA netlist summary mismatch", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
if not any(source.get("kind") == "netlist" for source in report.get("sources", [])):
    print("EDA netlist source missing", file=sys.stderr)
    sys.exit(1)
if not any(
    check.get("check_id") == "netlist_structure" and check.get("status") == "pass"
    for check in report.get("checks", [])
):
    print("EDA netlist_structure pass check missing", file=sys.stderr)
    sys.exit(1)
root = Path(os.environ["KIANA_EDA_PROJECT"])
run_dir = root / ".kiana" / "workflows" / report["run_id"]
review_dir = run_dir / "eda" / "reviews" / report["review_id"]
missing = [name for name in ("eda_review.json", "bom_risk.md", "bringup-plan.md") if not (review_dir / name).is_file()]
if missing:
    print(f"EDA review artifacts missing: {missing}", file=sys.stderr)
    sys.exit(1)
artifact_report = json.loads((review_dir / "eda_review.json").read_text(encoding="utf-8"))
if artifact_report != report:
    print("EDA CLI output differs from persisted eda_review.json", file=sys.stderr)
    sys.exit(1)

eventlog_path = run_dir / "eventlog.jsonl"
if not eventlog_path.is_file():
    print(f"EDA eventlog missing: {eventlog_path}", file=sys.stderr)
    sys.exit(1)
event_kinds = set()
for line_number, line in enumerate(eventlog_path.read_text(encoding="utf-8").splitlines(), start=1):
    if not line.strip():
        continue
    try:
        record = json.loads(line)
    except json.JSONDecodeError as exc:
        print(f"EDA eventlog line {line_number} is invalid JSON: {exc}", file=sys.stderr)
        sys.exit(1)
    event = record.get("event", record) if isinstance(record, dict) else None
    if isinstance(event, dict) and isinstance(event.get("kind"), str):
        event_kinds.add(event["kind"])
required_events = {"artifact_written", "evidence_recorded", "verification_completed"}
missing_events = sorted(required_events - event_kinds)
if missing_events:
    print(f"EDA eventlog missing events: {missing_events}", file=sys.stderr)
    sys.exit(1)

verification_dir = run_dir / "verification"
packet_paths = sorted(verification_dir.glob("*.json")) if verification_dir.is_dir() else []
if not packet_paths:
    print(f"EDA verification packet missing: {verification_dir}", file=sys.stderr)
    sys.exit(1)
matching_packets = []
for packet_path in packet_paths:
    try:
        packet = json.loads(packet_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        print(f"EDA verification packet is invalid JSON: {packet_path}: {exc}", file=sys.stderr)
        sys.exit(1)
    if (
        packet.get("profile") == "eda_review"
        and packet.get("run_id") == report.get("run_id")
        and packet.get("workflow_id") == report.get("workflow_id")
    ):
        matching_packets.append(packet)
if not any(packet.get("final_status") == "pass" for packet in matching_packets):
    print("EDA pass VerificationPacket missing", file=sys.stderr)
    sys.exit(1)

print(report["run_id"], report["review_id"], sep="\t")
PY
  )"
  IFS=$'\t' read -r run_id review_id <<<"$eda_identity"
  review_path="$project_dir/.kiana/workflows/$run_id/eda/reviews/$review_id/eda_review.json"
  if [[ -f docs/schemas/kiana-eda-review.v1.schema.json ]]; then
    "$python_bin" \
      "$(native_env_path "$(pwd)/scripts/validate-json-schema.py")" \
      "$(native_env_path "$(pwd)/docs/schemas/kiana-eda-review.v1.schema.json")" \
      "$(native_env_path "$review_path")" >/dev/null
  fi
  rm -rf "$project_dir"
}

smoke_context_index_search_json() {
  local binary="$1"
  local binary_path="$binary"
  local project_dir
  local index_output
  local search_output
  local path_search_output
  local vector_search_output
  local pack_output
  local path_pack_output
  local root_pack_output
  local python_bin
  local artifact_source
  local artifact_dir
  local artifacts_output
  local unapproved_cached_artifacts_output
  local cached_artifacts_output
  local artifact_ingest_output
  local cached_artifact_store_output

  if [[ "$binary_path" != /* ]]; then
    binary_path="$PWD/${binary_path#./}"
  fi
  project_dir="$(mktemp -d "$tmp_root/context-project.XXXXXX")"
  artifact_source=".release-context-artifacts"
  artifact_dir="$project_dir/$artifact_source"
  mkdir -p "$project_dir/src"
  mkdir -p "$project_dir/docs"
  mkdir -p "$project_dir/tests"
  mkdir -p "$artifact_dir/bundle"
  printf '%s\n' 'pub fn release_context_search() {}' '// release release context search' > "$project_dir/src/lib.rs"
  printf '%s\n' 'use kiana::release_context_search;' > "$project_dir/tests/lib_test.rs"
  printf '%s\n' '# Context Guide' 'release context guide' > "$project_dir/README.md"
  printf '%s\n' 'first module summary referencing src/lib.rs' > "$project_dir/docs/path-only.md"
  printf '%s\n' 'first artifact line' > "$artifact_dir/bundle/notes.md"

  # Context queries are product capabilities and require an explicit project trust decision.
  (cd "$project_dir" && run_clean_kiana "$binary_path" trust trust >/dev/null)

  index_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context index --json 2>&1)"
  artifacts_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context artifacts --json 2>&1)"
  if unapproved_cached_artifacts_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context artifacts --json --cache .kiana/context-artifacts.json 2>&1)"; then
    echo "context artifacts cache unexpectedly succeeded without local-write approval" >&2
    return 1
  fi
  if ! grep -Fq -- "control_plane_command_awaiting_approval" <<<"$unapproved_cached_artifacts_output"; then
    echo "context artifacts cache did not return the expected approval challenge" >&2
    echo "$unapproved_cached_artifacts_output" >&2
    return 1
  fi
  if [[ -e "$project_dir/.kiana/context-artifacts.json" ]]; then
    echo "context artifacts cache was created before approval" >&2
    return 1
  fi
  cached_artifacts_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" --approve-local-write context artifacts --json --cache .kiana/context-artifacts.json 2>&1)"
  artifact_ingest_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" --approve-local-write context ingest --source "$artifact_source" --json 2>&1)"
  artifact_graph_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context artifact-graph --json 2>&1)"
  artifact_store_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context artifact-store --json 2>&1)"
  artifact_readiness_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context artifact-readiness --json 2>&1)"
  cached_artifact_store_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" --approve-local-write context artifact-store --json --cache .kiana/context-artifact-store.json 2>&1)"
  search_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context search release --json --limit 1 2>&1)"
  path_search_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context search docs/path-only.md --json --limit 1 2>&1)"
  vector_search_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context vector-search release flow --json --limit 1 2>&1)"
  pack_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context pack release --json --limit 1 --max-snippet-lines 1 2>&1)"
  path_pack_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context pack docs/path-only.md --json --limit 1 --max-snippet-lines 1 2>&1)"
  root_pack_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context pack bundle/notes.md --root "$artifact_source" --json --limit 1 --max-snippet-lines 1 2>&1)"
  python_bin="$(doctor_json_python)"
  CONTEXT_INDEX_JSON="$index_output" CONTEXT_ARTIFACTS_JSON="$artifacts_output" CONTEXT_CACHED_ARTIFACTS_JSON="$cached_artifacts_output" CONTEXT_ARTIFACT_INGEST_JSON="$artifact_ingest_output" CONTEXT_ARTIFACT_GRAPH_JSON="$artifact_graph_output" CONTEXT_ARTIFACT_STORE_JSON="$artifact_store_output" CONTEXT_ARTIFACT_READINESS_JSON="$artifact_readiness_output" CONTEXT_CACHED_ARTIFACT_STORE_JSON="$cached_artifact_store_output" CONTEXT_SEARCH_JSON="$search_output" CONTEXT_PATH_SEARCH_JSON="$path_search_output" CONTEXT_VECTOR_SEARCH_JSON="$vector_search_output" CONTEXT_PACK_JSON="$pack_output" CONTEXT_PATH_PACK_JSON="$path_pack_output" CONTEXT_ROOT_PACK_JSON="$root_pack_output" "$python_bin" - <<'PY'
import json
import os
import sys

try:
    index = json.loads(os.environ["CONTEXT_INDEX_JSON"])
    artifacts = json.loads(os.environ["CONTEXT_ARTIFACTS_JSON"])
    cached_artifacts = json.loads(os.environ["CONTEXT_CACHED_ARTIFACTS_JSON"])
    artifact_ingest = json.loads(os.environ["CONTEXT_ARTIFACT_INGEST_JSON"])
    artifact_graph = json.loads(os.environ["CONTEXT_ARTIFACT_GRAPH_JSON"])
    artifact_store = json.loads(os.environ["CONTEXT_ARTIFACT_STORE_JSON"])
    artifact_readiness = json.loads(os.environ["CONTEXT_ARTIFACT_READINESS_JSON"])
    cached_artifact_store = json.loads(os.environ["CONTEXT_CACHED_ARTIFACT_STORE_JSON"])
    search = json.loads(os.environ["CONTEXT_SEARCH_JSON"])
    path_search = json.loads(os.environ["CONTEXT_PATH_SEARCH_JSON"])
    vector_search = json.loads(os.environ["CONTEXT_VECTOR_SEARCH_JSON"])
    pack = json.loads(os.environ["CONTEXT_PACK_JSON"])
    path_pack = json.loads(os.environ["CONTEXT_PATH_PACK_JSON"])
    root_pack = json.loads(os.environ["CONTEXT_ROOT_PACK_JSON"])
except Exception as exc:
    print(f"context JSON is not valid JSON: {exc}", file=sys.stderr)
    print(os.environ.get("CONTEXT_INDEX_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_ARTIFACTS_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_CACHED_ARTIFACTS_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_ARTIFACT_INGEST_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_ARTIFACT_GRAPH_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_ARTIFACT_STORE_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_ARTIFACT_READINESS_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_CACHED_ARTIFACT_STORE_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_SEARCH_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_PATH_SEARCH_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_VECTOR_SEARCH_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_PACK_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_PATH_PACK_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_ROOT_PACK_JSON", ""), file=sys.stderr)
    sys.exit(1)

vector_hits = vector_search.get("hits", [])
vector_hit = vector_hits[0] if vector_hits else {}

checks = [
    index.get("schema") == "kiana.context-index.v1",
    index.get("files_indexed") == 4,
    any(item.get("path") == "src/lib.rs" and item.get("language") == "rust" for item in index.get("files", [])),
    artifacts.get("schema") == "kiana.context-artifacts.v1",
    artifacts.get("files_indexed") == 4,
    any(item.get("path") == "src/lib.rs" and item.get("kind") == "file" and str(item.get("id", "")).startswith("file:src/lib.rs:") for item in artifacts.get("artifacts", [])),
    cached_artifacts.get("schema") == "kiana.context-artifacts.v1",
    cached_artifacts.get("cache", {}).get("status") == "created",
    cached_artifacts.get("cache", {}).get("added_artifacts") == 4,
    str(cached_artifacts.get("cache", {}).get("path", "")).endswith(".kiana/context-artifacts.json"),
    artifact_ingest.get("schema") == "kiana.context-artifact-ingest.v1",
    artifact_ingest.get("artifacts_schema") == "kiana.context-artifacts.v1",
    artifact_ingest.get("ingested_files") == 1,
    artifact_ingest.get("skipped_files") == 0,
    artifact_ingest.get("manifest_path") == ".kiana/context-ingest/manifest.json",
    artifact_ingest.get("store_dir") == ".kiana/context-ingest",
    artifact_ingest.get("sync", {}).get("path") == ".kiana/context-ingest/manifest.json",
    artifact_ingest.get("sync", {}).get("status") == "created",
    artifact_ingest.get("sync", {}).get("added_files") == 1,
    artifact_ingest.get("sync", {}).get("reused_files") == 0,
    any(item.get("source_path") == "bundle/notes.md" and str(item.get("stored_path", "")).startswith(".kiana/context-ingest/files/") for item in artifact_ingest.get("artifacts", [])),
    artifact_graph.get("schema") == "kiana.context-artifact-dependency-graph.v1",
    len(artifact_graph.get("nodes", [])) == 4,
    any(node.get("path") == "tests/lib_test.rs" for node in artifact_graph.get("nodes", [])),
    any(edge.get("relation") == "test_of" and edge.get("evidence") == "tests/lib_test.rs matches src/lib.rs" for edge in artifact_graph.get("edges", [])),
    any(edge.get("relation") == "path_reference" and edge.get("evidence") == "docs/path-only.md references src/lib.rs" for edge in artifact_graph.get("edges", [])),
    artifact_store.get("schema") == "kiana.context-artifact-store.v1",
    artifact_store.get("artifacts_schema") == "kiana.context-artifacts.v1",
    artifact_store.get("dependency_graph_schema") == "kiana.context-artifact-dependency-graph.v1",
    artifact_store.get("artifact_count") == 4,
    artifact_store.get("dependency_count") == 2,
    any(role.get("role") == "source" and role.get("count") == 1 for role in artifact_store.get("artifact_roles", [])),
    any(role.get("role") == "test" and role.get("count") == 1 for role in artifact_store.get("artifact_roles", [])),
    any(role.get("role") == "artifact" and role.get("count") == 2 for role in artifact_store.get("artifact_roles", [])),
    artifact_store.get("artifacts", {}).get("schema") == "kiana.context-artifacts.v1",
    artifact_store.get("dependency_graph", {}).get("schema") == "kiana.context-artifact-dependency-graph.v1",
    any(edge.get("relation") == "path_reference" for edge in artifact_store.get("dependency_graph", {}).get("edges", [])),
    artifact_readiness.get("schema") == "kiana.context-artifact-readiness.v1",
    artifact_readiness.get("artifact_store_schema") == "kiana.context-artifact-store.v1",
    artifact_readiness.get("status") == "incomplete",
    artifact_readiness.get("missing_roles") == ["prd", "design", "tasks"],
    any(role.get("role") == "source" and role.get("present") is True and role.get("count") == 1 for role in artifact_readiness.get("required_roles", [])),
    any(role.get("role") == "prd" and role.get("present") is False and role.get("count") == 0 for role in artifact_readiness.get("required_roles", [])),
    cached_artifact_store.get("schema") == "kiana.context-artifact-store.v1",
    any(role.get("role") == "source" and role.get("count") == 1 for role in cached_artifact_store.get("artifact_roles", [])),
    cached_artifact_store.get("cache", {}).get("status") == "created",
    cached_artifact_store.get("cache", {}).get("added_artifacts") == 4,
    cached_artifact_store.get("cache", {}).get("added_dependencies") == 2,
    str(cached_artifact_store.get("cache", {}).get("path", "")).endswith(".kiana/context-artifact-store.json"),
    search.get("schema") == "kiana.context-search.v1",
    search.get("terms") == ["release"],
    search.get("limit") == 1,
    len(search.get("hits", [])) == 1,
    search.get("hits", [{}])[0].get("path") == "src/lib.rs",
    "release" in search.get("hits", [{}])[0].get("matched_terms", []),
    path_search.get("schema") == "kiana.context-search.v1",
    path_search.get("hits", [{}])[0].get("path") == "docs/path-only.md",
    path_search.get("hits", [{}])[0].get("occurrences") == 0,
    path_search.get("hits", [{}])[0].get("line") == "first module summary referencing src/lib.rs",
    vector_search.get("schema") == "kiana.context-vector-search.v1",
    vector_search.get("embedding_model") == "kiana.deterministic-hash-embedding.v1",
    vector_search.get("dimensions") == 64,
    vector_search.get("terms") == ["flow", "release"],
    vector_search.get("limit") == 1,
    len(vector_hits) == 1,
    vector_hit.get("path") in {"README.md", "src/lib.rs"},
    vector_hit.get("score", 0) > 0,
    vector_hit.get("token_overlap", 0) >= 1,
    pack.get("schema") == "kiana.context-pack.v1",
    pack.get("terms") == ["release"],
    pack.get("limit") == 1,
    pack.get("max_snippet_lines") == 1,
    len(pack.get("snippets", [])) == 1,
    pack.get("snippets", [{}])[0].get("path") == "src/lib.rs",
    "release" in pack.get("snippets", [{}])[0].get("matched_terms", []),
    "release" in pack.get("snippets", [{}])[0].get("excerpt", ""),
    pack.get("artifact_graph", {}).get("schema") == "kiana.context-artifact-graph.v1",
    len(pack.get("artifact_graph", {}).get("nodes", [])) == 1,
    len(pack.get("artifact_graph", {}).get("edges", [])) == 1,
    pack.get("artifact_graph", {}).get("nodes", [{}])[0].get("kind") == "snippet",
    pack.get("artifact_graph", {}).get("nodes", [{}])[0].get("path") == "src/lib.rs",
    pack.get("artifact_graph", {}).get("nodes", [{}])[0].get("content_hash") == pack.get("snippets", [{}])[0].get("content_hash"),
    pack.get("artifact_graph", {}).get("edges", [{}])[0].get("source") == "query:release",
    pack.get("artifact_graph", {}).get("edges", [{}])[0].get("target") == pack.get("artifact_graph", {}).get("nodes", [{}])[0].get("id"),
    pack.get("artifact_graph", {}).get("edges", [{}])[0].get("relation") == "matched",
    pack.get("artifact_graph", {}).get("edges", [{}])[0].get("matched_terms") == ["release"],
    path_pack.get("schema") == "kiana.context-pack.v1",
    path_pack.get("snippets", [{}])[0].get("path") == "docs/path-only.md",
    path_pack.get("snippets", [{}])[0].get("occurrences") == 0,
    path_pack.get("snippets", [{}])[0].get("excerpt") == "first module summary referencing src/lib.rs",
    path_pack.get("artifact_graph", {}).get("schema") == "kiana.context-artifact-graph.v1",
    path_pack.get("artifact_graph", {}).get("nodes", [{}])[0].get("path") == "docs/path-only.md",
    root_pack.get("schema") == "kiana.context-pack.v1",
    root_pack.get("snippets", [{}])[0].get("path") == "bundle/notes.md",
    root_pack.get("snippets", [{}])[0].get("occurrences") == 0,
    root_pack.get("snippets", [{}])[0].get("excerpt") == "first artifact line",
    root_pack.get("artifact_graph", {}).get("schema") == "kiana.context-artifact-graph.v1",
    root_pack.get("artifact_graph", {}).get("nodes", [{}])[0].get("path") == "bundle/notes.md",
]
if not all(checks):
    print("context index/ingest/search/vector-search/pack JSON failed smoke checks", file=sys.stderr)
    print(json.dumps(index, indent=2, sort_keys=True), file=sys.stderr)
    print(json.dumps(artifacts, indent=2, sort_keys=True), file=sys.stderr)
    print(json.dumps(cached_artifacts, indent=2, sort_keys=True), file=sys.stderr)
    print(json.dumps(artifact_ingest, indent=2, sort_keys=True), file=sys.stderr)
    print(json.dumps(artifact_graph, indent=2, sort_keys=True), file=sys.stderr)
    print(json.dumps(artifact_store, indent=2, sort_keys=True), file=sys.stderr)
    print(json.dumps(artifact_readiness, indent=2, sort_keys=True), file=sys.stderr)
    print(json.dumps(cached_artifact_store, indent=2, sort_keys=True), file=sys.stderr)
    print(json.dumps(search, indent=2, sort_keys=True), file=sys.stderr)
    print(json.dumps(path_search, indent=2, sort_keys=True), file=sys.stderr)
    print(json.dumps(vector_search, indent=2, sort_keys=True), file=sys.stderr)
    print(json.dumps(pack, indent=2, sort_keys=True), file=sys.stderr)
    print(json.dumps(path_pack, indent=2, sort_keys=True), file=sys.stderr)
    print(json.dumps(root_pack, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
PY
}

smoke_license_status_json() {
  local binary="$1"
  local output
  local configured
  local python_bin

  output="$(
    unset KIANA_LICENSE_FILE KIANA_LICENSE_KEY KIANA_LICENSE_PLAN KIANA_LICENSE_ENTITLEMENTS
    unset KIANA_LICENSE_OFFLINE KIANA_ENTERPRISE_ACCOUNT_ID KIANA_SUPPORT_CONTACT
    run_clean_kiana "$binary" license status --json 2>&1
  )"
  python_bin="$(doctor_json_python)"
  LICENSE_STATUS_JSON="$output" "$python_bin" - <<'PY'
import json
import os
import sys

try:
    report = json.loads(os.environ["LICENSE_STATUS_JSON"])
except Exception as exc:
    print(f"license status JSON is not valid JSON: {exc}", file=sys.stderr)
    print(os.environ.get("LICENSE_STATUS_JSON", ""), file=sys.stderr)
    sys.exit(1)

if report.get("schema") != "kiana.license-status.v1":
    print("license status schema mismatch", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
if report.get("status") != "missing" or report.get("license_key") != "missing":
    print("default license status should be missing", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
if "no enterprise license configured" not in report.get("issues", []):
    print("missing license issue was not reported", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
PY

  configured="$(
    KIANA_LICENSE_KEY="smoke-license-secret-7777" \
    KIANA_LICENSE_PLAN="enterprise" \
    KIANA_LICENSE_ENTITLEMENTS="audit,policy" \
    KIANA_ENTERPRISE_ACCOUNT_ID="acct_smoke" \
    run_clean_kiana "$binary" license status --json 2>&1
  )"
  LICENSE_STATUS_JSON="$configured" "$python_bin" - <<'PY'
import json
import os
import sys

report = json.loads(os.environ["LICENSE_STATUS_JSON"])
checks = [
    report.get("schema") == "kiana.license-status.v1",
    report.get("status") == "configured",
    report.get("source") == "KIANA_LICENSE_KEY",
    report.get("license_key") == "set",
    report.get("license_key_preview") == "redacted-7777",
    report.get("account_id") == "acct_smoke",
    report.get("plan") == "enterprise",
    report.get("issues") == [],
]
if not all(checks):
    print("configured license status failed smoke checks", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
if "smoke-license-secret" in os.environ["LICENSE_STATUS_JSON"]:
    print("license status leaked the raw license key", file=sys.stderr)
    sys.exit(1)
PY
}

smoke_version() {
  local binary="$1"
  local output

  output="$(run_clean_kiana "$binary" --version 2>&1 || true)"
  if ! grep -Fq -- "Kiana Code" <<<"$output"; then
    echo "version smoke failed for: $binary" >&2
    echo "$output" >&2
    exit 1
  fi
}

smoke_mcp_config() {
  local binary="$1"
  local binary_path="$binary"
  local project_dir
  local output

  if [[ "$binary_path" != /* ]]; then
    binary_path="$PWD/${binary_path#./}"
  fi
  project_dir="$(mktemp -d "$tmp_root/mcp-env.XXXXXX")"
  output="$(cd "$project_dir" && KIANA_MCP_SERVERS_JSON='{"docs":{"url":"http://127.0.0.1:9000/mcp"},"shell":{"command":"node","args":["server.js"]}}' run_clean_kiana "$binary_path" mcp list)"
  grep -q "2 MCP server(s)" <<<"$output"
  grep -q "docs" <<<"$output"
  grep -q "node server.js" <<<"$output"

  output="$(cd "$project_dir" && KIANA_MCP_SERVERS_JSON='{"docs":{"url":"http://127.0.0.1:9000/mcp"}}' run_clean_kiana "$binary_path" mcp get docs)"
  grep -q '"name": "docs"' <<<"$output"
  grep -q '"url": "http://127.0.0.1:9000/mcp"' <<<"$output"
}

smoke_mcp_project_config() {
  local binary="$1"
  local binary_path="$binary"
  local project_dir
  local output

  if [[ "$binary_path" != /* ]]; then
    binary_path="$PWD/${binary_path#./}"
  fi
  project_dir="$(mktemp -d "$tmp_root/mcp-project.XXXXXX")"

  output="$(cd "$project_dir" && KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp add-json docs '{"command":"node","args":["server.js"]}' --scope project)"
  grep -q "Added stdio MCP server docs" <<<"$output"
  test -f "$project_dir/.mcp.json"
  grep -q '"mcpServers"' "$project_dir/.mcp.json"
  grep -q '"docs"' "$project_dir/.mcp.json"

  output="$(cd "$project_dir" && KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp list)"
  grep -q "1 MCP server(s)" <<<"$output"
  grep -q "node server.js" <<<"$output"

  output="$(cd "$project_dir" && KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp get docs)"
  grep -q '"name": "docs"' <<<"$output"
  grep -q '"command": "node"' <<<"$output"

  output="$(cd "$project_dir" && KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp remove docs -s project)"
  grep -q "Removed MCP server docs" <<<"$output"
  if grep -q '"docs"' "$project_dir/.mcp.json"; then
    echo "expected docs to be removed from .mcp.json" >&2
    exit 1
  fi

  output="$(cd "$project_dir" && KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp list)"
  grep -q "No MCP servers configured." <<<"$output"

  mkdir -p "$project_dir/packages/app"
  printf '%s\n' '{"mcpServers":{"docs":{"command":"node","args":["root-server.js"]}}}' > "$project_dir/.mcp.json"
  output="$(cd "$project_dir/packages/app" && KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp list)"
  grep -q "docs" <<<"$output"
  grep -q "node root-server.js" <<<"$output"

  output="$(cd "$project_dir/packages/app" && KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp get docs)"
  grep -q '"command": "node"' <<<"$output"

  printf '%s\n' '{"mcpServers":{"docs":{"command":"${KIANA_TEST_MCP_COMMAND}","args":["${KIANA_TEST_MCP_ARG:-fallback.js}"]}}}' > "$project_dir/.mcp.json"
  output="$(cd "$project_dir" && KIANA_TEST_MCP_COMMAND=node KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp list)"
  grep -q "node fallback.js" <<<"$output"
  output="$(cd "$project_dir" && KIANA_TEST_MCP_COMMAND=node KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp get docs)"
  grep -q '"command": "node"' <<<"$output"

  output="$(cd "$project_dir" && KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp remove docs -s project)"
  grep -q "Removed MCP server docs" <<<"$output"

  output="$(cd "$project_dir" && KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp add -e 'API_KEY=abc def' docs -- node server.js --watch)"
  grep -q "Added stdio MCP server docs" <<<"$output"
  grep -q '"type": "stdio"' "$project_dir/.mcp.json"
  grep -q '"API_KEY": "abc def"' "$project_dir/.mcp.json"
  grep -q -- '"--watch"' "$project_dir/.mcp.json"

  output="$(cd "$project_dir" && KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp remove docs -s project)"
  grep -q "Removed MCP server docs" <<<"$output"

  output="$(cd "$project_dir" && KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp add --transport http docs https://example.test/mcp --header 'Authorization: Bearer abc' --scope project)"
  grep -q "Added HTTP MCP server docs" <<<"$output"
  grep -q '"type": "http"' "$project_dir/.mcp.json"
  grep -q '"url": "https://example.test/mcp"' "$project_dir/.mcp.json"
  grep -q '"Authorization": "Bearer abc"' "$project_dir/.mcp.json"

  output="$(cd "$project_dir" && KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp get docs)"
  grep -q '"type": "http"' <<<"$output"
  grep -q '"Authorization": "Bearer abc"' <<<"$output"

  output="$(cd "$project_dir" && KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp remove docs -s project)"
  grep -q "Removed MCP server docs" <<<"$output"

  printf '%s\n' '{"mcpServers":{"desktop_docs":{"command":"node","args":["desktop-server.js"]}}}' > "$tmp_root/claude_desktop_config.json"
  output="$(cd "$project_dir" && KIANA_CLAUDE_DESKTOP_CONFIG="$tmp_root/claude_desktop_config.json" KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp add-from-claude-desktop --scope project)"
  grep -q "Imported 1 MCP server(s)" <<<"$output"
  grep -q '"desktop_docs"' "$project_dir/.mcp.json"
  grep -q '"desktop-server.js"' "$project_dir/.mcp.json"
  output="$(cd "$project_dir" && KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp remove desktop_docs -s project)"
  grep -q "Removed MCP server desktop_docs" <<<"$output"

  mkdir -p "$project_dir/.kiana"
  printf '%s\n' '{"enabledMcpjsonServers":["docs"],"disabledMcpjsonServers":["shell"],"enableAllProjectMcpServers":true}' > "$project_dir/.kiana/mcp-project-choices.json"
  output="$(cd "$project_dir" && KIANA_MCP_SERVERS_JSON='' run_clean_kiana "$binary_path" mcp reset-project-choices)"
  grep -q "project-scoped (.mcp.json) server approvals" <<<"$output"
  grep -q '"enabledMcpjsonServers": \[\]' "$project_dir/.kiana/mcp-project-choices.json"
  grep -q '"disabledMcpjsonServers": \[\]' "$project_dir/.kiana/mcp-project-choices.json"
  grep -q '"enableAllProjectMcpServers": false' "$project_dir/.kiana/mcp-project-choices.json"
}

smoke_auth_config() {
  local binary="$1"
  local output

  output="$(
    unset KIANA_OPENAI_API_KEY OPENAI_API_KEY KIANA_OPENAI_BASE_URL OPENAI_BASE_URL
    unset KIANA_OPENAI_MODEL OPENAI_MODEL KIANA_OLLAMA_BASE_URL OLLAMA_BASE_URL
    unset KIANA_OLLAMA_MODEL OLLAMA_MODEL
    run_clean_kiana "$binary" auth status --json
  )"
  grep -Fq -- '"api_key": "missing"' <<<"$output"
  grep -Fq -- '"source": "none"' <<<"$output"
  grep -Fq -- '"access_token": "missing"' <<<"$output"
  grep -Fq -- '"refresh_token": "missing"' <<<"$output"
  grep -Fq -- '"provider_id": "openai-compatible"' <<<"$output"
  grep -Fq -- '"auth": "not_required"' <<<"$output"

  output="$(run_clean_kiana "$binary" auth login sk-ant-smoke-auth-key)"
  grep -Fq -- "Login updated" <<<"$output"

  output="$(run_clean_kiana "$binary" auth status --text)"
  grep -Fq -- "Auth status" <<<"$output"
  grep -Fq -- "api_key: set" <<<"$output"
  grep -Fq -- "source: config" <<<"$output"

  output="$(run_clean_kiana "$binary" auth logout)"
  grep -Fq -- "Logout complete" <<<"$output"

  output="$(
    unset KIANA_OPENAI_API_KEY OPENAI_API_KEY KIANA_OPENAI_BASE_URL OPENAI_BASE_URL
    unset KIANA_OPENAI_MODEL OPENAI_MODEL KIANA_OLLAMA_BASE_URL OLLAMA_BASE_URL
    unset KIANA_OLLAMA_MODEL OLLAMA_MODEL
    run_clean_kiana "$binary" auth status --json
  )"
  grep -Fq -- '"api_key": "missing"' <<<"$output"
  grep -Fq -- '"source": "none"' <<<"$output"
  grep -Fq -- '"access_token": "missing"' <<<"$output"
  grep -Fq -- '"refresh_token": "missing"' <<<"$output"

  output="$(
    KIANA_OPENAI_API_KEY="smoke-openai-secret-4242" \
    KIANA_OPENAI_MODEL="gpt-smoke" \
    run_clean_kiana "$binary" auth status --json
  )"
  grep -Fq -- '"provider_id": "openai-compatible"' <<<"$output"
  grep -Fq -- '"status": "configured"' <<<"$output"
  grep -Fq -- '"key_preview": "redacted-4242"' <<<"$output"
  if grep -Fq -- "smoke-openai-secret" <<<"$output"; then
    echo "auth status leaked OpenAI-compatible API key" >&2
    exit 1
  fi
}

smoke_completion_scripts() {
  local binary="$1"
  local output
  local output_file="$tmp_root/completion.zsh"

  output="$(run_clean_kiana "$binary" completion bash)"
  grep -Fq -- "_kiana()" <<<"$output"
  grep -Fq -- "complete -F _kiana kiana" <<<"$output"
  grep -Fq -- "auth" <<<"$output"
  grep -Fq -- "auto-mode" <<<"$output"
  grep -Fq -- "mcp" <<<"$output"
  grep -Fq -- "open" <<<"$output"
  grep -Fq -- "install uninstall" <<<"$output"
  grep -Fq -- "--scope" <<<"$output"
  grep -Fq -- "user project local" <<<"$output"

  output="$(run_clean_kiana "$binary" completion fish)"
  grep -Fq -- "complete -c kiana" <<<"$output"
  grep -Fq -- "__fish_seen_subcommand_from auto-mode" <<<"$output"
  grep -Fq -- "__fish_seen_subcommand_from mcp" <<<"$output"
  grep -Fq -- "__fish_seen_subcommand_from plugin" <<<"$output"
  grep -Fq -- "-l scope -s s" <<<"$output"
  grep -Fq -- "-a 'user project local'" <<<"$output"

  output="$(run_clean_kiana "$binary" completion zsh --output "$output_file")"
  grep -Fq -- "Wrote completion script" <<<"$output"
  grep -Fq -- "#compdef kiana" "$output_file"
  grep -Fq -- "_arguments" "$output_file"
  grep -Fq -- "'auto-mode:kiana command'" "$output_file"
  grep -Fq -- "'plugin command' list status json marketplace install uninstall" "$output_file"
  grep -Fq -- "'plugin scope' user project local" "$output_file"
}

smoke_plugin_marketplace() {
  local binary
  binary="$(cd "$(dirname "$1")" && pwd -P)/$(basename "$1")"
  local marketplace_dir="$tmp_root/tools-marketplace"
  local plugin_project="$tmp_root/plugin-project"
  local output

  mkdir -p "$marketplace_dir/.codex-plugin" "$marketplace_dir/review-tools/.codex-plugin" "$marketplace_dir/review-tools/commands"
  mkdir -p "$plugin_project/.git"
  cat > "$marketplace_dir/review-tools/.codex-plugin/plugin.json" <<'JSON'
{
  "name": "review-tools",
  "version": "1.0.0",
  "description": "Review smoke plugin"
}
JSON
  printf '# audit\n' > "$marketplace_dir/review-tools/commands/audit.md"
  receipt_source_hash="$("$(doctor_json_python)" - "$marketplace_dir/review-tools" <<'PY'
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
hash_value = 0xcbf29ce484222325

def update(data: bytes):
    global hash_value
    for byte in data:
        hash_value ^= byte
        hash_value = (hash_value * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF

entries = []
for path in sorted(p for p in root.rglob("*") if p.is_file()):
    rel = path.relative_to(root).as_posix()
    if rel == ".kiana-install-receipt.json":
        continue
    file_hash = 0xcbf29ce484222325
    for byte in path.read_bytes():
        file_hash ^= byte
        file_hash = (file_hash * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    entries.append((rel, path.stat().st_size, f"{file_hash:016x}"))

for rel, size, file_hash in entries:
    update(rel.encode())
    update(b"\0")
    update(str(size).encode())
    update(b"\0")
    update(file_hash.encode())
    update(b"\0")

print(f"{hash_value:016x}")
PY
)"
  cat > "$marketplace_dir/.codex-plugin/marketplace.json" <<JSON
{
  "name": "tools-marketplace",
  "plugins": [
    {
      "name": "review-tools",
      "source": "./review-tools",
      "signature": {
        "scheme": "external-signature-v1",
        "signer": "Kiana Smoke Marketplace",
        "keyId": "smoke-key-1",
        "contentHash": "$receipt_source_hash",
        "signature": "smoke-signature"
      }
    }
  ]
}
JSON

  output="$(run_clean_kiana "$binary" plugin marketplace list --json)"
  grep -Fq -- "[]" <<<"$output"

  output="$(run_clean_kiana "$binary" plugin marketplace add "$marketplace_dir")"
  grep -Fq -- "Successfully added marketplace: tools-marketplace" <<<"$output"
  grep -Fq -- "scope: user" <<<"$output"

  output="$(run_clean_kiana "$binary" plugin marketplace list --json)"
  grep -Fq -- '"name": "tools-marketplace"' <<<"$output"
  grep -Fq -- '"source": "directory"' <<<"$output"

  managed_policy_file="$tmp_root/managed-plugin-policy.json"
  if [[ "${OS:-}" == "Windows_NT" || "${OSTYPE:-}" == msys* || "${OSTYPE:-}" == cygwin* ]]; then
    signature_verification_command='if x%KIANA_PLUGIN_SIGNATURE_VALUE%==xsmoke-signature (exit /b 0) else (exit /b 2)'
  else
    signature_verification_command='test x$KIANA_PLUGIN_SIGNATURE_VALUE = xsmoke-signature'
  fi
  cat > "$managed_policy_file" <<JSON
{
  "schema": "kiana.managed-plugin-policy.v1",
  "plugins": {
    "requireSignature": true,
    "requireSignatureVerification": true,
    "signatureVerificationCommand": "$signature_verification_command",
    "allow": ["review-tools@tools-marketplace"],
    "allowMarketplaces": ["tools-marketplace"]
  }
}
JSON

  output="$(KIANA_MANAGED_PLUGIN_POLICY_FILE="$managed_policy_file" run_clean_kiana "$binary" plugin install review-tools@tools-marketplace)"
  grep -Fq -- "Installed plugin: review-tools" <<<"$output"
  grep -Fq -- "marketplace: tools-marketplace" <<<"$output"
  grep -Fq -- "managed_policy: allowed (managed plugin policy matched)" <<<"$output"
  grep -Fq -- "receipt: " <<<"$output"
  test -f "$smoke_home/.kiana/plugins/review-tools/commands/audit.md"
  test -f "$smoke_home/.kiana/plugins/review-tools/.kiana-install-receipt.json"
  grep -Fq -- '"schema": "kiana.plugin-install-receipt.v1"' "$smoke_home/.kiana/plugins/review-tools/.kiana-install-receipt.json"
  grep -Fq -- '"path": "commands/audit.md"' "$smoke_home/.kiana/plugins/review-tools/.kiana-install-receipt.json"
  grep -Fq -- '"signature": {' "$smoke_home/.kiana/plugins/review-tools/.kiana-install-receipt.json"
  grep -Fq -- '"scheme": "external-signature-v1"' "$smoke_home/.kiana/plugins/review-tools/.kiana-install-receipt.json"
  grep -Fq -- '"signer": "Kiana Smoke Marketplace"' "$smoke_home/.kiana/plugins/review-tools/.kiana-install-receipt.json"
  grep -Fq -- '"contentHash": "' "$smoke_home/.kiana/plugins/review-tools/.kiana-install-receipt.json"
  grep -Fq -- '"integrity": {' "$smoke_home/.kiana/plugins/review-tools/.kiana-install-receipt.json"
  grep -Fq -- '"method": "stable-hash-v1"' "$smoke_home/.kiana/plugins/review-tools/.kiana-install-receipt.json"

  output="$(run_clean_kiana "$binary" plugin list review-tools)"
  grep -Fq -- "review-tools@1.0.0 [valid enabled]" <<<"$output"
  grep -Fq -- "commands=1" <<<"$output"
  output="$(run_clean_kiana "$binary" skills audit --json)"
  grep -Fq -- '"schema": "kiana.skills-audit.v1"' <<<"$output"
  grep -Fq -- '"plugin_load_audit": [' <<<"$output"
  grep -Fq -- '"plugin": "review-tools"' <<<"$output"
  grep -Fq -- '"status": "no-skills"' <<<"$output"
  output="$(run_clean_kiana "$binary" reload-plugins)"
  grep -Fq -- "Reloaded: 1 plugins" <<<"$output"
  grep -Fq -- "1 commands" <<<"$output"
  assert_output_path "$output" "user: " "$smoke_home/.kiana/plugins"
  output="$(run_clean_kiana "$binary" plugin show review-tools)"
  grep -Fq -- '"install_receipt": {' <<<"$output"
  grep -Fq -- '"schema": "kiana.plugin-install-receipt.v1"' <<<"$output"
  grep -Fq -- '"install_receipt_integrity": {' <<<"$output"
  grep -Fq -- '"status": "verified"' <<<"$output"
  grep -Fq -- '"managed_policy": {' <<<"$output"
  grep -Fq -- '"reason": "managed plugin policy matched"' <<<"$output"

  printf '\n# tampered\n' >> "$smoke_home/.kiana/plugins/review-tools/commands/audit.md"
  output="$(run_clean_kiana "$binary" plugin show review-tools)"
  grep -Fq -- '"install_receipt_integrity": {' <<<"$output"
  grep -Fq -- '"status": "tampered"' <<<"$output"
  output="$(run_clean_kiana "$binary" plugin validate review-tools)"
  grep -Fq -- "plugin install receipt integrity tampered" <<<"$output"
  grep -Fq -- "Validation failed" <<<"$output"

  output="$(run_clean_kiana "$binary" plugin disable review-tools)"
  grep -Fq -- "Plugin disabled: review-tools" <<<"$output"

  output="$(run_clean_kiana "$binary" plugin uninstall review-tools)"
  grep -Fq -- "Uninstalled plugin: review-tools" <<<"$output"
  test ! -e "$smoke_home/.kiana/plugins/review-tools"

  output="$(cd "$plugin_project" && run_clean_kiana "$binary" trust trust)"
  grep -Fq -- "project_trust: trusted" <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" trust json)"
  grep -Fq -- '"project_trust": "trusted"' <<<"$output"
  grep -Fq -- '"source": "user_store"' <<<"$output"
  output="$(cd "$plugin_project" && KIANA_MANAGED_PLUGIN_POLICY_FILE="$managed_policy_file" run_clean_kiana "$binary" plugin install review-tools@tools-marketplace --scope project)"
  grep -Fq -- "Installed plugin: review-tools" <<<"$output"
  assert_output_path "$output" "path: " "$plugin_project/.kiana/plugins/review-tools"
  grep -Fq -- "managed_policy: allowed (managed plugin policy matched)" <<<"$output"
  test -f "$plugin_project/.kiana/plugins/review-tools/commands/audit.md"
  test -f "$plugin_project/.kiana/plugins/review-tools/.kiana-install-receipt.json"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin list --scope project review-tools)"
  assert_output_path "$output" "path: " "$plugin_project/.kiana/plugins"
  grep -Fq -- "review-tools@1.0.0 [valid enabled]" <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin path review-tools --scope project)"
  assert_exact_output_path "$output" "$plugin_project/.kiana/plugins/review-tools"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin show review-tools --scope project)"
  grep -Fq -- '"manifest_name": "review-tools"' <<<"$output"
  grep -Fq -- '"status": "verified"' <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin validate review-tools --scope project)"
  grep -Fq -- "Validating plugin: review-tools" <<<"$output"
  grep -Fq -- "Validation passed" <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" skills audit --json)"
  grep -Fq -- '"schema": "kiana.skills-audit.v1"' <<<"$output"
  if ! grep -Fq -- '"plugin": "review-tools"' <<<"$output"; then
    echo "trusted project did not expose installed project plugin resources" >&2
    echo "$output" >&2
    exit 1
  fi
  grep -Fq -- '"status": "no-skills"' <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" trust reset)"
  grep -Fq -- "project_trust: unknown" <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" skills audit --json)"
  if grep -Fq -- '"plugin": "review-tools"' <<<"$output"; then
    echo "reset project unexpectedly exposed project plugin resources" >&2
    exit 1
  fi
  if output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin list --scope project review-tools 2>&1)"; then
    echo "unknown project unexpectedly read project-scoped plugins" >&2
    exit 1
  fi
  grep -Fq -- "project trust is unknown" <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" trust trust)"
  grep -Fq -- "project_trust: trusted" <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" skills audit --json)"
  grep -Fq -- '"schema": "kiana.skills-audit.v1"' <<<"$output"
  if ! grep -Fq -- '"plugin": "review-tools"' <<<"$output"; then
    echo "trusted project did not expose installed project plugin resources" >&2
    echo "$output" >&2
    exit 1
  fi
  grep -Fq -- '"status": "no-skills"' <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" reload-plugins)"
  grep -Fq -- "Reloaded: 1 plugins" <<<"$output"
  grep -Fq -- "1 commands" <<<"$output"
  assert_output_path "$output" "project: " "$plugin_project/.kiana/plugins"
  assert_output_path "$output" "local: " "$plugin_project/.kiana/plugins.local"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin disable review-tools --scope project)"
  grep -Fq -- "Plugin disabled: review-tools" <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin list --scope project review-tools)"
  grep -Fq -- "review-tools@1.0.0 [valid disabled]" <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin enable review-tools --scope project)"
  grep -Fq -- "Plugin enabled: review-tools" <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin list --scope project review-tools)"
  grep -Fq -- "review-tools@1.0.0 [valid enabled]" <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin uninstall review-tools --scope project)"
  grep -Fq -- "Uninstalled plugin: review-tools" <<<"$output"
  test ! -e "$plugin_project/.kiana/plugins/review-tools"

  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin install review-tools@tools-marketplace --scope local)"
  grep -Fq -- "Installed plugin: review-tools" <<<"$output"
  assert_output_path "$output" "path: " "$plugin_project/.kiana/plugins.local/review-tools"
  test -f "$plugin_project/.kiana/plugins.local/review-tools/commands/audit.md"
  test -f "$plugin_project/.kiana/plugins.local/review-tools/.kiana-install-receipt.json"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin json --scope local review-tools)"
  grep -Fq -- '"manifest_name": "review-tools"' <<<"$output"
  assert_json_path "$output" "root" "$plugin_project/.kiana/plugins.local/review-tools"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin path review-tools --scope local)"
  assert_exact_output_path "$output" "$plugin_project/.kiana/plugins.local/review-tools"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin validate --scope local)"
  grep -Fq -- "Validating plugin: review-tools" <<<"$output"
  grep -Fq -- "Validation passed" <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" skills audit --json)"
  grep -Fq -- '"schema": "kiana.skills-audit.v1"' <<<"$output"
  grep -Fq -- '"plugin": "review-tools"' <<<"$output"
  grep -Fq -- '"status": "no-skills"' <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" reload-plugins)"
  grep -Fq -- "Reloaded: 1 plugins" <<<"$output"
  grep -Fq -- "1 commands" <<<"$output"
  assert_output_path "$output" "local: " "$plugin_project/.kiana/plugins.local"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin disable review-tools --scope local)"
  grep -Fq -- "Plugin disabled: review-tools" <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin json --scope local review-tools)"
  grep -Fq -- '"enabled": false' <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin enable review-tools --scope local)"
  grep -Fq -- "Plugin enabled: review-tools" <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin json --scope local review-tools)"
  grep -Fq -- '"enabled": true' <<<"$output"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" plugin uninstall review-tools --scope local)"
  grep -Fq -- "Uninstalled plugin: review-tools" <<<"$output"
  test ! -e "$plugin_project/.kiana/plugins.local/review-tools"
  output="$(cd "$plugin_project" && run_clean_kiana "$binary" trust reset)"
  grep -Fq -- "project_trust: unknown" <<<"$output"

  output="$(run_clean_kiana "$binary" plugin marketplace remove tools-marketplace)"
  grep -Fq -- "Successfully removed marketplace: tools-marketplace" <<<"$output"

  output="$(run_clean_kiana "$binary" plugin marketplace list --json)"
  grep -Fq -- "[]" <<<"$output"
}

help_smoke_cases=(
  "--help::kiana mcp serve"
  "auto-mode --help::Usage: kiana auto-mode"
  "auth --help::Usage: kiana auth"
  "auth status --help::Usage: kiana auth status"
  "license --help::Usage: kiana license"
  "license status --help::Usage: kiana license status"
  "release --help::Usage: kiana release"
  "completion --help::Usage: kiana completion <shell>"
  "plugin --help::--scope user|project|local"
  "plugin install --help::Usage: kiana plugin install"
  "agents --help::Usage: kiana agents"
  "open --help::Usage: kiana open <cc-url>"
  "server --help::Usage: kiana server"
  "plugin marketplace --help::Usage: kiana plugin marketplace"
  "mcp --help::kiana mcp serve [--debug] [--verbose]"
  "mcp serve --help::Usage: kiana mcp serve"
  "mcp add-from-claude-desktop --help::Usage: kiana mcp add-from-claude-desktop"
  "mcp-server --help::Usage: kiana mcp-server"
  "mcp-server-http --help::Usage: kiana mcp-server"
  "computer-mcp --help::Usage: kiana computer-mcp"
  "bridge status --help::Usage: kiana bridge"
  "bridge start --help::Usage: kiana bridge"
  "remote start --help::Usage: kiana bridge"
  "chrome-native-host --help::Usage: kiana chrome"
  "chrome-native-host install --help::Usage: kiana chrome"
  "--print --help::Usage: kiana -p"
  "-p --help::Usage: kiana -p"
  "--output-format json --print --help::Usage: kiana -p"
  "--print --output-format json --help::Usage: kiana -p"
  "--print --record-only --help::Usage: kiana -p"
  "--continue --help::Usage: kiana --continue"
  "--resume --help::Usage: kiana --continue"
  "--continue --record-only --help::Usage: kiana --continue"
  "--resume abc --output-format json --help::Usage: kiana --continue"
  "url handle --help::Usage: kiana url"
  "url plan --help::Usage: kiana url"
  "url status --help::Usage: kiana url"
  "remote-session url --help::Usage: kiana remote-session"
  "remote-session list --help::Usage: kiana remote-session"
  "remote-session create --help::Usage: kiana remote-session"
  "remote-session listen --help::Usage: kiana remote-session"
  "remote-session send --help::Usage: kiana remote-session"
  "remote-session code-session create --help::Usage: kiana remote-session code-session"
  "remote-session code-session bridge --help::Usage: kiana remote-session code-session"
  "remote-session code-session smoke --help::Usage: kiana remote-session code-session"
  "remote-session code-session hydrate --help::Usage: kiana remote-session code-session"
  "remote-session code-session sdk-url --help::Usage: kiana remote-session code-session"
  "daemon status --help::Usage: kiana daemon"
  "daemon start --help::Usage: kiana daemon"
  "daemon enqueue --help::Usage: kiana daemon"
  "daemon ps --help::Usage: kiana daemon"
  "session list --help::Usage: kiana session"
  "session show --help::Usage: kiana session"
  "session rename --help::Usage: kiana session"
  "list --help::Usage: kiana session"
  "show --help::Usage: kiana session"
  "rename --help::Usage: kiana session"
)

release_bin="./target/release/kiana$(exe_ext)"
installed_bin="$install_dir/kiana$(exe_ext)"

smoke_version "$release_bin"
smoke_project_trust "$release_bin"
smoke_doctor "$release_bin"
smoke_doctor_json "$release_bin"
smoke_commercial_security_doctor_json "$release_bin"
smoke_model_smoke_json "$release_bin"
smoke_model_catalog_json "$release_bin"
smoke_auto_mode_fake_critique "$release_bin"
smoke_release_blockers_json "$release_bin"
smoke_release_evidence_json "$release_bin"
smoke_eval_json "$release_bin"
smoke_eda_json "$release_bin"
smoke_context_index_search_json "$release_bin"
smoke_license_status_json "$release_bin"
for entry in "${help_smoke_cases[@]}"; do
  smoke_help_usage "$release_bin" "$entry"
done
smoke_auth_config "$release_bin"
smoke_completion_scripts "$release_bin"
smoke_plugin_marketplace "$release_bin"
smoke_mcp_config "$release_bin"
smoke_mcp_project_config "$release_bin"

install_release_binary
smoke_version "$installed_bin"
smoke_project_trust "$installed_bin"
smoke_doctor "$installed_bin"
smoke_doctor_json "$installed_bin"
smoke_commercial_security_doctor_json "$installed_bin"
smoke_model_smoke_json "$installed_bin"
smoke_model_catalog_json "$installed_bin"
smoke_auto_mode_fake_critique "$installed_bin"
smoke_release_blockers_json "$installed_bin"
smoke_release_evidence_json "$installed_bin"
smoke_eval_json "$installed_bin"
smoke_eda_json "$installed_bin"
smoke_context_index_search_json "$installed_bin"
smoke_license_status_json "$installed_bin"
for entry in "${help_smoke_cases[@]}"; do
  smoke_help_usage "$installed_bin" "$entry"
done
smoke_auth_config "$installed_bin"
smoke_completion_scripts "$installed_bin"
smoke_plugin_marketplace "$installed_bin"
smoke_mcp_config "$installed_bin"
smoke_mcp_project_config "$installed_bin"
