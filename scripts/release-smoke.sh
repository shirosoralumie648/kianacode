#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ "${KIANA_RELEASE_SMOKE_SKIP_BUILD_GATES:-0}" != "1" ]]; then
  cargo fmt --all --check
  cargo test --workspace --locked --offline --no-fail-fast
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
    -u KIANA_REMOTE_ACCESS_TOKEN \
    -u CLAUDE_ACCESS_TOKEN \
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

  output="$(run_clean_kiana "$binary" auth status --json)"
  grep -Fq -- '"api_key": "missing"' <<<"$output"
  grep -Fq -- '"source": "none"' <<<"$output"

  output="$(run_clean_kiana "$binary" auth login sk-ant-smoke-auth-key)"
  grep -Fq -- "Login updated" <<<"$output"

  output="$(run_clean_kiana "$binary" auth status --text)"
  grep -Fq -- "Auth status" <<<"$output"
  grep -Fq -- "api_key: set" <<<"$output"
  grep -Fq -- "source: config" <<<"$output"

  output="$(run_clean_kiana "$binary" auth logout)"
  grep -Fq -- "Logout complete" <<<"$output"

  output="$(run_clean_kiana "$binary" auth status --json)"
  grep -Fq -- '"api_key": "missing"' <<<"$output"
  grep -Fq -- '"source": "none"' <<<"$output"
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
  test -f "$smoke_home/.kiana/plugins/review-tools/commands/audit.md"

  output="$(run_clean_kiana "$binary" plugin list review-tools)"
  grep -Fq -- "review-tools@1.0.0 [valid enabled]" <<<"$output"
  grep -Fq -- "commands=1" <<<"$output"

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
for entry in "${help_smoke_cases[@]}"; do
  smoke_help_usage "$installed_bin" "$entry"
done
smoke_auth_config "$installed_bin"
smoke_completion_scripts "$installed_bin"
smoke_plugin_marketplace "$installed_bin"
smoke_mcp_config "$installed_bin"
smoke_mcp_project_config "$installed_bin"
