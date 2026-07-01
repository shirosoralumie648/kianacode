# ✅ 配置功能已添加

## 🎯 使用方法

### 1. 初始化配置（推荐）
```bash
kiana config init
kiana login "sk-ant-xxx"
```

也可以直接写入单个配置项：

```bash
kiana config set api_key "sk-ant-xxx"
kiana config set base_url "https://api.anthropic.com"
kiana model list
kiana model list --json
kiana model smoke --json
kiana license status --json
```

配置保存到 `~/.kiana/config.toml`，运行 `kiana config status` 可查看当前生效值。

### 2. 手动编辑配置文件
```bash
mkdir -p ~/.kiana
cat > ~/.kiana/config.toml << EOF
api_key = "sk-ant-xxx"
base_url = "https://api.anthropic.com"  # 可选
model = "claude-sonnet-4-6"             # 可选
EOF
```

### 3. 常规环境变量
```bash
export ANTHROPIC_API_KEY="sk-ant-xxx"
export ANTHROPIC_BASE_URL="https://your-proxy.com"  # 可选
export ANTHROPIC_MODEL="claude-sonnet-4-6"          # 可选
```

这些环境变量高于普通配置文件和 settings overlay，但会被托管配置策略覆盖。

OpenAI-compatible 文本 provider 需要显式选择 provider，并使用独立环境变量，避免误用 Anthropic key：

```bash
export KIANA_PROVIDER="openai-compatible"
export KIANA_OPENAI_API_KEY="sk-xxx"
export KIANA_OPENAI_BASE_URL="https://api.openai.com/v1" # 可选
export KIANA_OPENAI_MODEL="gpt-4.1"                      # 可选
```

当前 OpenAI-compatible provider 只支持 text-only chat/completions；未显式配置 tools 时 runner 会自动使用空工具集，显式启用 `--tools` 时会在发出网络请求前返回 `unsupported_tools`。

本地 Ollama 文本 provider 不需要 API key：

```bash
export KIANA_PROVIDER="ollama"
export KIANA_OLLAMA_BASE_URL="http://localhost:11434" # 可选
export KIANA_OLLAMA_MODEL="llama3.1"                  # 可选
```

当前 Ollama provider 只支持 text-only `/api/chat`；未显式配置 tools 时 runner 会自动使用空工具集，显式启用 `--tools` 时会在发出网络请求前返回 `unsupported_tools`。

Provider smoke 默认无网络，只验证 fake provider 并报告 live provider skip reason。设置 `KIANA_PROVIDER_SMOKE_LIVE=1` 或使用 `kiana model smoke --live --json` 后，才会尝试真实 Anthropic、OpenAI-compatible 和 Ollama provider。

### 4. 托管配置策略

管理员或受控运行环境可以提供最终配置 overlay：

```bash
export KIANA_MANAGED_SETTINGS_FILE="/etc/kiana/managed-settings.json"
```

也可以使用共享策略路径：

```bash
export KIANA_MANAGED_POLICY_FILE="/etc/kiana/policy.json"
```

托管配置支持 JSON 或 TOML，字段形状和普通配置 overlay 一致。示例：

```json
{
  "apiKey": "managed-key",
  "baseUrl": "https://managed-api.example.com",
  "model": "managed-model",
  "settings": {
    "brief": true
  }
}
```

### 5. 权限预设
```bash
kiana permissions profile read-only
kiana permissions profile workspace
kiana permissions profile full
kiana permissions profile ask
kiana permissions profile plan
```

`profile` 是预设，`mode` 是显式模式。`read-only` 会把默认 mode 收敛到 `plan`，`full` 会映射到 `bypassPermissions`。

### 6. 托管权限策略

权限托管策略使用 JSON。`KIANA_MANAGED_PERMISSIONS_FILE` 只影响权限，并优先于共享的 `KIANA_MANAGED_POLICY_FILE`：

```bash
export KIANA_MANAGED_PERMISSIONS_FILE="/etc/kiana/managed-permissions.json"
```

策略可以直接写权限字段，也可以放在 `permissions` 下：

```json
{
  "permissions": {
    "profile": "read-only",
    "disallowedTools": ["Bash"],
    "askTools": ["Write"],
    "allowedTools": ["Read"]
  }
}
```

托管权限规则在用户权限文件、环境变量、会话/app state 之后生效：managed deny > user/session/env deny > managed ask > managed allow > normal allow > normal ask > mode。运行 `kiana permissions status` 可以查看 `managed_policy_file`、`managed_policy_status`、托管 allow/deny/ask 规则和错误信息。

### 7. Bash sandbox

```toml
[sandbox]
enabled = true
failIfUnavailable = true
allowUnsandboxedCommands = false
```

`enabled` 为真时，Bash 命令默认走 `bwrap` 沙箱；`failIfUnavailable` 要求沙箱不可用时直接失败；`allowUnsandboxedCommands` 允许在明确要求 `require_escalated` 时退回到非沙箱执行。`kiana config set sandbox` 接受 JSON 对象，runner 也会把配置文件里的 `sandbox = true` 规范化成 `{"enabled": true}`。运行 `kiana doctor` 可以查看 `bash_sandbox` 的 enabled/status/runtime、`bwrap` 路径和不可用 warning。

## 🌐 Base URL 使用场景

1. **使用代理**
   ```toml
   base_url = "https://your-proxy.com"
   ```

2. **第三方兼容服务**
   ```toml
   base_url = "https://compatible-service.com"
   ```

3. **内网部署**
   ```toml
   base_url = "http://internal-api:8080"
   ```

## ✨ 优先级

普通配置优先级：

```text
base config (~/.kiana/config.toml)
-> KIANA_REMOTE_SETTINGS_FILE
-> KIANA_SETTINGS_FILE
-> KIANA_SETTINGS_JSON
-> ANTHROPIC_API_KEY / ANTHROPIC_BASE_URL / ANTHROPIC_MODEL
-> KIANA_MANAGED_SETTINGS_FILE 或 KIANA_MANAGED_POLICY_FILE
```

例如：
- 配置文件设置了 `api_key = "key1"`
- 环境变量 `ANTHROPIC_API_KEY="key2"`
- 托管配置 `KIANA_MANAGED_SETTINGS_FILE` 设置了 `apiKey = "key3"`
- 实际使用：`key3`（托管配置是最终 overlay）

## 🚀 快速开始

```bash
# 首次使用
kiana config init
kiana login "sk-ant-xxx"

# 之后直接运行
kiana

# 或手动
cargo run -p kiana-entrypoints --bin kiana
```

## 📝 配置示例

`.kiana-example.toml` 包含完整示例。
