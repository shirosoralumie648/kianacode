#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ "${KIANA_RELEASE_SMOKE_SKIP_BUILD_GATES:-0}" != "1" ]]; then
  cargo fmt --all --check
  cargo test --workspace --locked --offline --no-fail-fast
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

smoke_context_index_search_json() {
  local binary="$1"
  local binary_path="$binary"
  local project_dir
  local index_output
  local search_output
  local path_search_output
  local pack_output
  local path_pack_output
  local root_pack_output
  local python_bin
  local artifact_dir

  if [[ "$binary_path" != /* ]]; then
    binary_path="$PWD/${binary_path#./}"
  fi
  project_dir="$(mktemp -d "$tmp_root/context-project.XXXXXX")"
  artifact_dir="$(mktemp -d "$tmp_root/context-artifacts.XXXXXX")"
  mkdir -p "$project_dir/src"
  mkdir -p "$project_dir/docs"
  mkdir -p "$artifact_dir/bundle"
  printf '%s\n' 'pub fn release_context_search() {}' '// release release context search' > "$project_dir/src/lib.rs"
  printf '%s\n' '# Context Guide' 'release context guide' > "$project_dir/README.md"
  printf '%s\n' 'first module summary' > "$project_dir/docs/path-only.md"
  printf '%s\n' 'first artifact line' > "$artifact_dir/bundle/notes.md"

  index_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context index --json 2>&1)"
  search_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context search release --json --limit 1 2>&1)"
  path_search_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context search docs/path-only.md --json --limit 1 2>&1)"
  pack_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context pack release --json --limit 1 --max-snippet-lines 1 2>&1)"
  path_pack_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context pack docs/path-only.md --json --limit 1 --max-snippet-lines 1 2>&1)"
  root_pack_output="$(cd "$project_dir" && run_clean_kiana "$binary_path" context pack bundle/notes.md --root "$artifact_dir" --json --limit 1 --max-snippet-lines 1 2>&1)"
  python_bin="$(doctor_json_python)"
  CONTEXT_INDEX_JSON="$index_output" CONTEXT_SEARCH_JSON="$search_output" CONTEXT_PATH_SEARCH_JSON="$path_search_output" CONTEXT_PACK_JSON="$pack_output" CONTEXT_PATH_PACK_JSON="$path_pack_output" CONTEXT_ROOT_PACK_JSON="$root_pack_output" "$python_bin" - <<'PY'
import json
import os
import sys

try:
    index = json.loads(os.environ["CONTEXT_INDEX_JSON"])
    search = json.loads(os.environ["CONTEXT_SEARCH_JSON"])
    path_search = json.loads(os.environ["CONTEXT_PATH_SEARCH_JSON"])
    pack = json.loads(os.environ["CONTEXT_PACK_JSON"])
    path_pack = json.loads(os.environ["CONTEXT_PATH_PACK_JSON"])
    root_pack = json.loads(os.environ["CONTEXT_ROOT_PACK_JSON"])
except Exception as exc:
    print(f"context JSON is not valid JSON: {exc}", file=sys.stderr)
    print(os.environ.get("CONTEXT_INDEX_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_SEARCH_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_PATH_SEARCH_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_PACK_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_PATH_PACK_JSON", ""), file=sys.stderr)
    print(os.environ.get("CONTEXT_ROOT_PACK_JSON", ""), file=sys.stderr)
    sys.exit(1)

checks = [
    index.get("schema") == "kiana.context-index.v1",
    index.get("files_indexed") == 3,
    any(item.get("path") == "src/lib.rs" and item.get("language") == "rust" for item in index.get("files", [])),
    search.get("schema") == "kiana.context-search.v1",
    search.get("terms") == ["release"],
    search.get("limit") == 1,
    len(search.get("hits", [])) == 1,
    search.get("hits", [{}])[0].get("path") == "src/lib.rs",
    "release" in search.get("hits", [{}])[0].get("matched_terms", []),
    path_search.get("schema") == "kiana.context-search.v1",
    path_search.get("hits", [{}])[0].get("path") == "docs/path-only.md",
    path_search.get("hits", [{}])[0].get("occurrences") == 0,
    path_search.get("hits", [{}])[0].get("line") == "first module summary",
    pack.get("schema") == "kiana.context-pack.v1",
    pack.get("terms") == ["release"],
    pack.get("limit") == 1,
    pack.get("max_snippet_lines") == 1,
    len(pack.get("snippets", [])) == 1,
    pack.get("snippets", [{}])[0].get("path") == "src/lib.rs",
    "release" in pack.get("snippets", [{}])[0].get("matched_terms", []),
    "release" in pack.get("snippets", [{}])[0].get("excerpt", ""),
    path_pack.get("schema") == "kiana.context-pack.v1",
    path_pack.get("snippets", [{}])[0].get("path") == "docs/path-only.md",
    path_pack.get("snippets", [{}])[0].get("occurrences") == 0,
    path_pack.get("snippets", [{}])[0].get("excerpt") == "first module summary",
    root_pack.get("schema") == "kiana.context-pack.v1",
    root_pack.get("snippets", [{}])[0].get("path") == "bundle/notes.md",
    root_pack.get("snippets", [{}])[0].get("occurrences") == 0,
    root_pack.get("snippets", [{}])[0].get("excerpt") == "first artifact line",
]
if not all(checks):
    print("context index/search/pack JSON failed smoke checks", file=sys.stderr)
    print(json.dumps(index, indent=2, sort_keys=True), file=sys.stderr)
    print(json.dumps(search, indent=2, sort_keys=True), file=sys.stderr)
    print(json.dumps(path_search, indent=2, sort_keys=True), file=sys.stderr)
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
  local output

  output="$(KIANA_MCP_SERVERS_JSON='{"docs":{"url":"http://127.0.0.1:9000/mcp"},"shell":{"command":"node","args":["server.js"]}}' run_clean_kiana "$binary" mcp list)"
  grep -q "2 MCP server(s)" <<<"$output"
  grep -q "docs" <<<"$output"
  grep -q "node server.js" <<<"$output"

  output="$(KIANA_MCP_SERVERS_JSON='{"docs":{"url":"http://127.0.0.1:9000/mcp"}}' run_clean_kiana "$binary" mcp get docs)"
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

  output="$(run_clean_kiana "$binary" completion fish)"
  grep -Fq -- "complete -c kiana" <<<"$output"
  grep -Fq -- "__fish_seen_subcommand_from auto-mode" <<<"$output"
  grep -Fq -- "__fish_seen_subcommand_from mcp" <<<"$output"
  grep -Fq -- "__fish_seen_subcommand_from plugin" <<<"$output"

  output="$(run_clean_kiana "$binary" completion zsh --output "$output_file")"
  grep -Fq -- "Wrote completion script" <<<"$output"
  grep -Fq -- "#compdef kiana" "$output_file"
  grep -Fq -- "_arguments" "$output_file"
  grep -Fq -- "'auto-mode:kiana command'" "$output_file"
  grep -Fq -- "'plugin command' list status json marketplace install uninstall" "$output_file"
}

smoke_plugin_marketplace() {
  local binary="$1"
  local marketplace_dir="$tmp_root/tools-marketplace"
  local output

  mkdir -p "$marketplace_dir/review-tools/.codex-plugin" "$marketplace_dir/review-tools/commands"
  cat > "$marketplace_dir/review-tools/.codex-plugin/plugin.json" <<'JSON'
{
  "name": "review-tools",
  "version": "1.0.0",
  "description": "Review smoke plugin"
}
JSON
  printf '# audit\n' > "$marketplace_dir/review-tools/commands/audit.md"

  output="$(run_clean_kiana "$binary" plugin marketplace list --json)"
  grep -Fq -- "[]" <<<"$output"

  output="$(run_clean_kiana "$binary" plugin marketplace add "$marketplace_dir")"
  grep -Fq -- "Successfully added marketplace: tools-marketplace" <<<"$output"
  grep -Fq -- "scope: user" <<<"$output"

  output="$(run_clean_kiana "$binary" plugin marketplace list --json)"
  grep -Fq -- '"name": "tools-marketplace"' <<<"$output"
  grep -Fq -- '"source": "directory"' <<<"$output"

  output="$(run_clean_kiana "$binary" plugin install review-tools@tools-marketplace)"
  grep -Fq -- "Installed plugin: review-tools" <<<"$output"
  grep -Fq -- "marketplace: tools-marketplace" <<<"$output"
  grep -Fq -- "receipt: " <<<"$output"
  test -f "$smoke_home/.kiana/plugins/review-tools/commands/audit.md"
  test -f "$smoke_home/.kiana/plugins/review-tools/.kiana-install-receipt.json"
  grep -Fq -- '"schema": "kiana.plugin-install-receipt.v1"' "$smoke_home/.kiana/plugins/review-tools/.kiana-install-receipt.json"
  grep -Fq -- '"path": "commands/audit.md"' "$smoke_home/.kiana/plugins/review-tools/.kiana-install-receipt.json"
  grep -Fq -- '"integrity": {' "$smoke_home/.kiana/plugins/review-tools/.kiana-install-receipt.json"
  grep -Fq -- '"method": "stable-hash-v1"' "$smoke_home/.kiana/plugins/review-tools/.kiana-install-receipt.json"

  output="$(run_clean_kiana "$binary" plugin list review-tools)"
  grep -Fq -- "review-tools@1.0.0 [valid enabled]" <<<"$output"
  grep -Fq -- "commands=1" <<<"$output"
  output="$(run_clean_kiana "$binary" plugin show review-tools)"
  grep -Fq -- '"install_receipt": {' <<<"$output"
  grep -Fq -- '"schema": "kiana.plugin-install-receipt.v1"' <<<"$output"
  grep -Fq -- '"install_receipt_integrity": {' <<<"$output"
  grep -Fq -- '"status": "verified"' <<<"$output"

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
  "completion --help::Usage: kiana completion <shell>"
  "plugin --help::usage: kiana plugin"
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
smoke_doctor "$release_bin"
smoke_doctor_json "$release_bin"
smoke_model_smoke_json "$release_bin"
smoke_model_catalog_json "$release_bin"
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
smoke_doctor "$installed_bin"
smoke_doctor_json "$installed_bin"
smoke_model_smoke_json "$installed_bin"
smoke_model_catalog_json "$installed_bin"
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
