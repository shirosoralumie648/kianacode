//! 启动配置的读取、合并和持久化 helper。
//!
//! # 文件在系统中的位置
//!
//! `kiana-entrypoints` 在 init、Runner 和部分 CLI 查询中调用本文件；它把多个低信任来源
//! 合并成一个进程内 [`Config`]，供模型/界面 adapter 读取。配置来源的优先级是代码中固定的
//! 顺序：
//!
//! ```text
//! KIANA_CONFIG_FILE 或 ~/.kiana/config.toml
//!        ↓ 基础 Config
//! KIANA_REMOTE_SETTINGS_FILE
//!        ↓ 远程/受管 overlay
//! KIANA_SETTINGS_FILE
//!        ↓ 用户 overlay
//! KIANA_SETTINGS_JSON
//!        ↓ JSON overlay
//! ANTHROPIC_API_KEY / ANTHROPIC_BASE_URL / ANTHROPIC_MODEL
//!        ↓ 环境变量覆盖
//! KIANA_MANAGED_SETTINGS_FILE 或 KIANA_MANAGED_POLICY_FILE
//!        ↓ 最后应用 managed overlay
//! Config（只在当前调用返回，不自动写 EventLog）
//! ```
//!
//! 这里做的是格式解析和优先级合并，不是权限判定、项目 trust 检查或 provider 请求。文件中
//! 允许读取 `api_key` 只表示调用方拿到了一个配置字符串；真正发送请求、选择 provider、
//! 记录执行事实仍由上层 Core/Daemon/Runner 负责。解析失败的文件/JSON 会被忽略并继续使用
//! 当前配置或默认值，这是兼容层的容错行为；调用方若需要“配置必须有效”的强保证，必须在
//! 更高层显式检查，而不能把 `load_config()` 的返回值当成已验证配置。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 上游 provider 使用的 API key；`None` 表示配置层没有提供凭证，不等于匿名请求被授权。
    #[serde(default)]
    pub api_key: Option<String>,
    /// 上游 API base URL；配置层只保存字符串，不在这里验证协议、主机 allowlist 或可达性。
    #[serde(default)]
    pub base_url: Option<String>,
    /// API 请求超时毫秒数；正数 overlay 才会覆盖旧值，具体请求阶段和默认值由调用方解释。
    #[serde(default, alias = "apiTimeoutMs")]
    pub api_timeout_ms: Option<u64>,
    /// 默认模型标识；缺失时由 [`default_model`] 提供兼容默认值。
    #[serde(default = "default_model")]
    pub model: String,
    /// CLI/UI 的显示和自动模式偏好；这些设置不直接改变 ControlPlane 授权。
    #[serde(default)]
    pub settings: Settings,
    /// 旧 Runner 使用的 sandbox 描述；这里保留 JSON 形状，实际是否可执行由受控 adapter 决定。
    #[serde(default)]
    pub sandbox: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    /// 是否输出额外诊断信息，供入口/UI 展示层读取。
    #[serde(default)]
    pub verbose: bool,
    /// 是否使用简略输出模式；不改变服务端响应事实。
    #[serde(default)]
    pub brief: bool,
    /// 是否启用 Vim 风格编辑行为。
    #[serde(default)]
    pub vim_mode: bool,
    /// UI 主题名称；仅是展示偏好。
    #[serde(default)]
    pub theme: Option<String>,
    /// 可选的 advisor 模型标识。
    #[serde(default)]
    pub advisor_model: Option<String>,
    /// 输出风格名称；`outputStyle` 是历史配置文件的兼容别名。
    #[serde(default, alias = "outputStyle")]
    pub output_style: Option<String>,
    /// 自动模式的允许、软拒绝和环境规则集合。
    #[serde(default, alias = "autoMode")]
    pub auto_mode: AutoModeSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutoModeSettings {
    /// 自动模式明确允许的规则/命令名称；这里只保存列表，是否允许仍由实际策略层决定。
    #[serde(default)]
    pub allow: Vec<String>,
    /// 自动模式的软拒绝列表；历史 `deny` 字段会映射到同一列表。
    #[serde(default, alias = "softDeny", alias = "deny")]
    pub soft_deny: Vec<String>,
    /// 自动模式需要考虑的环境标签。
    #[serde(default)]
    pub environment: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ConfigOverlay {
    #[serde(default, alias = "apiKey")]
    api_key: Option<String>,
    #[serde(default, alias = "baseUrl")]
    base_url: Option<String>,
    #[serde(default, alias = "apiTimeoutMs")]
    api_timeout_ms: Option<u64>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    settings: SettingsOverlay,
    #[serde(default)]
    sandbox: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct SettingsOverlay {
    #[serde(default)]
    verbose: Option<bool>,
    #[serde(default)]
    brief: Option<bool>,
    #[serde(default, alias = "vimMode")]
    vim_mode: Option<bool>,
    #[serde(default)]
    theme: Option<String>,
    #[serde(default, alias = "advisorModel")]
    advisor_model: Option<String>,
    #[serde(default, alias = "outputStyle")]
    output_style: Option<String>,
    #[serde(default, alias = "autoMode")]
    auto_mode: AutoModeSettingsOverlay,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct AutoModeSettingsOverlay {
    #[serde(default)]
    allow: Option<Vec<String>>,
    #[serde(default, alias = "softDeny")]
    soft_deny: Option<Vec<String>>,
    #[serde(default)]
    deny: Option<Vec<String>>,
    #[serde(default)]
    environment: Option<Vec<String>>,
}

fn default_model() -> String {
    // 默认模型只是兼容层的 fallback；它不代表 provider 已配置、模型可用或调用已获授权。
    "claude-sonnet-4-6".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: None,
            api_timeout_ms: None,
            model: default_model(),
            settings: Settings::default(),
            sandbox: None,
        }
    }
}

/// 按固定优先级读取并合并当前进程可见的配置来源。
///
/// 【调用者】`kiana-entrypoints::init`、`runner` 和若干 CLI 状态/诊断路径调用；同一调用会
/// 重新读取文件和环境变量，不使用全局缓存，因此调用期间环境变化可能导致不同结果。
///
/// 【输入】基础配置文件、四类 overlay 文件/JSON、Anthropic 兼容环境变量以及最后应用的
/// managed settings 文件。每个来源都可以缺失；文件读取、TOML/JSON 解析失败时当前实现
/// 忽略该来源并继续合并。
///
/// 【输出】一个始终可返回的 [`Config`]：至少包含默认模型和默认 `Settings`。`Option` 字段
/// 仍可能为空，表示配置没有提供值；调用方必须决定这是否足以启动请求。
///
/// 【副作用】只读环境变量/文件，不写文件、不执行网络请求、不写 EventLog。`managed` overlay
/// 放在最后是刻意的优先级：受管配置可以覆盖用户/环境结果；改变顺序会改变部署策略。
pub fn load_config() -> Config {
    let mut config = read_config_file().unwrap_or_default();

    if let Ok(path) = env::var("KIANA_REMOTE_SETTINGS_FILE") {
        if let Some(overlay) = read_config_overlay_file(Path::new(&path)) {
            apply_config_overlay(&mut config, overlay);
        }
    }
    if let Ok(path) = env::var("KIANA_SETTINGS_FILE") {
        if let Some(overlay) = read_config_overlay_file(Path::new(&path)) {
            apply_config_overlay(&mut config, overlay);
        }
    }
    if let Ok(json) = env::var("KIANA_SETTINGS_JSON") {
        if let Some(overlay) = parse_json_config_overlay(&json) {
            apply_config_overlay(&mut config, overlay);
        }
    }

    // 这些环境变量是基础/用户 overlay 之后的直接覆盖项；空 model 不覆盖已有值，避免一次
    // 空环境变量把可用模型替换成不可调用的空标识。API key/base URL 当前只按存在覆盖，
    // 空值是否可用由上层 provider 检查负责。
    if let Ok(key) = env::var("ANTHROPIC_API_KEY") {
        config.api_key = Some(key);
    }
    if let Ok(url) = env::var("ANTHROPIC_BASE_URL") {
        config.base_url = Some(url);
    }
    if let Ok(model) = env::var("ANTHROPIC_MODEL") {
        if !model.trim().is_empty() {
            config.model = model;
        }
    }
    if let Some(path) = managed_settings_file_path() {
        if let Some(overlay) = read_config_overlay_file(&path) {
            apply_config_overlay(&mut config, overlay);
        }
    }

    config
}

/// 便捷读取 API key；每次调用都会重新执行 [`load_config`]，不会缓存或验证凭证。
pub fn get_api_key() -> Option<String> {
    load_config().api_key
}

/// 便捷读取 base URL；返回的是配置字符串，不代表 endpoint 已通过网络/安全检查。
pub fn get_base_url() -> Option<String> {
    load_config().base_url
}

fn read_config_file() -> Option<Config> {
    // 基础配置的失败采用 Option 传播，最终由 load_config 的 `unwrap_or_default()` 回退到默认
    // Config。这样旧安装没有配置文件仍能启动；需要 fail-closed 的入口不能只依赖这个 helper。
    let path = config_path()?;
    let contents = fs::read_to_string(path).ok()?;
    toml::from_str(&contents).ok()
}

fn managed_settings_file_path() -> Option<PathBuf> {
    // 两个环境变量是同一“最后应用的 managed overlay”槽位；优先使用更明确的 settings 名称，
    // policy 名称保留给旧部署。这里只选路径，不读取或校验其内容。
    env::var_os("KIANA_MANAGED_SETTINGS_FILE")
        .or_else(|| env::var_os("KIANA_MANAGED_POLICY_FILE"))
        .map(PathBuf::from)
}

fn read_config_overlay_file(path: &Path) -> Option<ConfigOverlay> {
    // overlay 文件先整体读入，再根据扩展名或首字符选择 JSON/TOML；解析失败返回 None，
    // 不会把部分字段混入配置，避免半解析状态。
    let contents = fs::read_to_string(path).ok()?;
    parse_config_overlay(path, &contents)
}

fn parse_config_overlay(path: &Path, contents: &str) -> Option<ConfigOverlay> {
    // JSON 既可由 `.json` 扩展名标识，也可由 `{` 开头识别；其余内容按 TOML 处理。
    // 这是格式选择，不是 schema/权限验证；未知字段是否被接受由 serde 结构决定。
    if path.extension().and_then(|ext| ext.to_str()) == Some("json")
        || contents.trim_start().starts_with('{')
    {
        return parse_json_config_overlay(contents);
    }
    toml::from_str(contents).ok()
}

fn parse_json_config_overlay(contents: &str) -> Option<ConfigOverlay> {
    // serde_json 只把文本转换成 overlay DTO；这里不执行配置效果，也不读取外部资源。
    serde_json::from_str(contents).ok()
}

fn apply_config_overlay(config: &mut Config, overlay: ConfigOverlay) {
    // 只覆盖 overlay 明确且非空的标量；列表采用追加而不是替换，目的是让远程、用户和
    // managed 来源可以逐层补充自动模式规则。调用者若需要清空列表，当前契约没有单独的
    // clear 标记，不能误以为传空列表会撤销之前来源的值。
    if let Some(api_key) = non_empty(overlay.api_key) {
        config.api_key = Some(api_key);
    }
    if let Some(base_url) = non_empty(overlay.base_url) {
        config.base_url = Some(base_url);
    }
    if let Some(api_timeout_ms) = overlay.api_timeout_ms.filter(|value| *value > 0) {
        config.api_timeout_ms = Some(api_timeout_ms);
    }
    if let Some(model) = non_empty(overlay.model) {
        config.model = model;
    }
    if let Some(verbose) = overlay.settings.verbose {
        config.settings.verbose = verbose;
    }
    if let Some(brief) = overlay.settings.brief {
        config.settings.brief = brief;
    }
    if let Some(vim_mode) = overlay.settings.vim_mode {
        config.settings.vim_mode = vim_mode;
    }
    if let Some(theme) = non_empty(overlay.settings.theme) {
        config.settings.theme = Some(theme);
    }
    if let Some(advisor_model) = non_empty(overlay.settings.advisor_model) {
        config.settings.advisor_model = Some(advisor_model);
    }
    if let Some(output_style) = non_empty(overlay.settings.output_style) {
        config.settings.output_style = Some(output_style);
    }
    append_non_empty_strings(
        &mut config.settings.auto_mode.allow,
        overlay.settings.auto_mode.allow,
    );
    append_non_empty_strings(
        &mut config.settings.auto_mode.soft_deny,
        overlay.settings.auto_mode.soft_deny,
    );
    append_non_empty_strings(
        &mut config.settings.auto_mode.soft_deny,
        overlay.settings.auto_mode.deny,
    );
    append_non_empty_strings(
        &mut config.settings.auto_mode.environment,
        overlay.settings.auto_mode.environment,
    );
    if let Some(sandbox) = overlay.sandbox {
        config.sandbox = Some(sandbox);
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    // 配置字符串统一 trim；空字符串视为“未提供”，从而保留更低优先级来源的值。
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn append_non_empty_strings(target: &mut Vec<String>, values: Option<Vec<String>>) {
    // 列表元素逐项 trim 并过滤空白项；不去重，因此同一规则从多个来源出现时仍保留来源
    // 次序和重复，实际策略层若需要集合语义必须自己处理。
    let Some(values) = values else {
        return;
    };
    target.extend(
        values
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    );
}

/// 只读取基础配置文件，不应用任何 overlay 或环境变量。
///
/// 这个 helper 供需要查看“磁盘基线”的调用方使用；需要最终运行时配置应调用
/// [`load_config`]，否则会漏掉远程、用户、环境和 managed 来源。
pub fn load_config_file() -> Config {
    read_config_file().unwrap_or_default()
}

/// 将给定配置序列化为 TOML 并写入 [`config_path`] 返回的位置。
///
/// 【副作用】会创建父目录并覆盖目标文件；本函数不做原子临时文件替换、不更新远程/managed
/// overlay，也不通知 ControlPlane。写入失败返回带路径的字符串，调用方不能在失败后声称
/// 配置已经持久化。
pub fn save_config_file(config: &Config) -> Result<PathBuf, String> {
    let path = config_path().ok_or_else(|| "home directory is not available".to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {}", parent.display(), error))?;
    }
    let content = toml::to_string_pretty(config)
        .map_err(|error| format!("failed to serialize config: {}", error))?;
    fs::write(&path, content)
        .map_err(|error| format!("failed to write {}: {}", path.display(), error))?;
    Ok(path)
}

pub fn config_path() -> Option<PathBuf> {
    // 显式 `KIANA_CONFIG_FILE` 优先于平台 home 推导路径；这里只计算位置，不保证父目录存在，
    // 也不检查路径是否属于当前项目。
    if let Some(path) = env::var_os("KIANA_CONFIG_FILE") {
        return Some(PathBuf::from(path));
    }
    dirs::home_dir().map(|home| home.join(".kiana").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: Mutex<()> = Mutex::new(());
        &LOCK
    }

    fn temp_path(name: &str, extension: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!(
            "kiana-config-{name}-{}-{unique}.{extension}",
            std::process::id()
        ))
    }

    fn clear_config_env() {
        env::remove_var("KIANA_CONFIG_FILE");
        env::remove_var("KIANA_REMOTE_SETTINGS_FILE");
        env::remove_var("KIANA_SETTINGS_FILE");
        env::remove_var("KIANA_SETTINGS_JSON");
        env::remove_var("KIANA_MANAGED_SETTINGS_FILE");
        env::remove_var("KIANA_MANAGED_POLICY_FILE");
        env::remove_var("ANTHROPIC_API_KEY");
        env::remove_var("ANTHROPIC_BASE_URL");
        env::remove_var("ANTHROPIC_MODEL");
        env::remove_var("KIANA_API_TIMEOUT_MS");
    }

    #[test]
    fn load_config_applies_json_settings_overlay() {
        let _guard = env_lock().lock().unwrap();
        clear_config_env();
        let base = temp_path("base", "toml");
        fs::write(
            &base,
            r#"
api_key = "base-key"
model = "base-model"

[settings]
brief = true
"#,
        )
        .unwrap();
        env::set_var("KIANA_CONFIG_FILE", &base);
        env::set_var(
            "KIANA_SETTINGS_JSON",
            r#"{"model":"json-model","apiTimeoutMs":1234,"settings":{"vimMode":true,"theme":"dark"}}"#,
        );

        let config = load_config();

        assert_eq!(config.api_key.as_deref(), Some("base-key"));
        assert_eq!(config.api_timeout_ms, Some(1234));
        assert_eq!(config.model, "json-model");
        assert!(config.settings.brief);
        assert!(config.settings.vim_mode);
        assert_eq!(config.settings.theme.as_deref(), Some("dark"));
        assert!(config.sandbox.is_none());

        let _ = fs::remove_file(base);
        clear_config_env();
    }

    #[test]
    fn load_config_preserves_sandbox_overlay() {
        let _guard = env_lock().lock().unwrap();
        clear_config_env();
        env::set_var(
            "KIANA_SETTINGS_JSON",
            r#"{"sandbox":{"enabled":true,"failIfUnavailable":true,"allowUnsandboxedCommands":false}}"#,
        );

        let config = load_config();

        assert_eq!(config.sandbox.as_ref().unwrap()["enabled"], true);
        assert_eq!(config.sandbox.as_ref().unwrap()["failIfUnavailable"], true);
        assert_eq!(
            config.sandbox.as_ref().unwrap()["allowUnsandboxedCommands"],
            false
        );

        clear_config_env();
    }

    #[test]
    fn load_config_applies_settings_file_and_env_overrides() {
        let _guard = env_lock().lock().unwrap();
        clear_config_env();
        let base = temp_path("base", "toml");
        let overlay = temp_path("overlay", "toml");
        fs::write(&base, "api_key = \"base-key\"\nmodel = \"base-model\"\n").unwrap();
        fs::write(
            &overlay,
            r#"
base_url = "https://overlay.example"
model = "overlay-model"

[settings]
advisor_model = "advisor"
"#,
        )
        .unwrap();
        env::set_var("KIANA_CONFIG_FILE", &base);
        env::set_var("KIANA_SETTINGS_FILE", &overlay);
        env::set_var("ANTHROPIC_MODEL", "env-model");

        let config = load_config();

        assert_eq!(config.api_key.as_deref(), Some("base-key"));
        assert_eq!(config.base_url.as_deref(), Some("https://overlay.example"));
        assert_eq!(config.model, "env-model");
        assert_eq!(config.settings.advisor_model.as_deref(), Some("advisor"));

        let _ = fs::remove_file(base);
        let _ = fs::remove_file(overlay);
        clear_config_env();
    }

    #[test]
    fn load_config_applies_remote_settings_file_before_user_overlay_and_env() {
        let _guard = env_lock().lock().unwrap();
        clear_config_env();
        let base = temp_path("base", "toml");
        let remote = temp_path("remote-settings", "json");
        let user = temp_path("user-settings", "json");
        fs::write(&base, "model = \"base-model\"\n").unwrap();
        fs::write(
            &remote,
            r#"{"model":"remote-model","settings":{"brief":true,"theme":"dark"}}"#,
        )
        .unwrap();
        fs::write(&user, r#"{"model":"user-model"}"#).unwrap();
        env::set_var("KIANA_CONFIG_FILE", &base);
        env::set_var("KIANA_REMOTE_SETTINGS_FILE", &remote);
        env::set_var("KIANA_SETTINGS_FILE", &user);
        env::set_var("ANTHROPIC_MODEL", "env-model");

        let config = load_config();

        assert_eq!(config.model, "env-model");
        assert!(config.settings.brief);
        assert_eq!(config.settings.theme.as_deref(), Some("dark"));

        let _ = fs::remove_file(base);
        let _ = fs::remove_file(remote);
        let _ = fs::remove_file(user);
        clear_config_env();
    }

    #[test]
    fn load_config_applies_managed_settings_file_after_env_and_user_overlays() {
        let _guard = env_lock().lock().unwrap();
        clear_config_env();
        let base = temp_path("base", "toml");
        let remote = temp_path("remote-settings", "json");
        let user = temp_path("user-settings", "json");
        let managed = temp_path("managed-settings", "json");
        fs::write(&base, "model = \"base-model\"\n").unwrap();
        fs::write(
            &remote,
            r#"{"model":"remote-model","settings":{"brief":true}}"#,
        )
        .unwrap();
        fs::write(
            &user,
            r#"{"model":"user-model","settings":{"theme":"user-theme"}}"#,
        )
        .unwrap();
        fs::write(
            &managed,
            r#"{"model":"managed-model","settings":{"theme":"managed-theme","vimMode":true}}"#,
        )
        .unwrap();
        env::set_var("KIANA_CONFIG_FILE", &base);
        env::set_var("KIANA_REMOTE_SETTINGS_FILE", &remote);
        env::set_var("KIANA_SETTINGS_FILE", &user);
        env::set_var("ANTHROPIC_MODEL", "env-model");
        env::set_var("KIANA_MANAGED_SETTINGS_FILE", &managed);

        let config = load_config();

        assert_eq!(config.model, "managed-model");
        assert_eq!(config.settings.theme.as_deref(), Some("managed-theme"));
        assert!(config.settings.vim_mode);
        assert!(config.settings.brief);

        let _ = fs::remove_file(base);
        let _ = fs::remove_file(remote);
        let _ = fs::remove_file(user);
        let _ = fs::remove_file(managed);
        clear_config_env();
    }

    #[test]
    fn config_getters_use_managed_settings_overlay_after_env() {
        let _guard = env_lock().lock().unwrap();
        clear_config_env();
        let managed = temp_path("managed-getters", "json");
        fs::write(
            &managed,
            r#"{"apiKey":"managed-key","baseUrl":"https://managed.example.com"}"#,
        )
        .unwrap();
        env::set_var("ANTHROPIC_API_KEY", "env-key");
        env::set_var("ANTHROPIC_BASE_URL", "https://env.example.com");
        env::set_var("KIANA_MANAGED_SETTINGS_FILE", &managed);

        assert_eq!(get_api_key().as_deref(), Some("managed-key"));
        assert_eq!(
            get_base_url().as_deref(),
            Some("https://managed.example.com")
        );

        let _ = fs::remove_file(managed);
        clear_config_env();
    }
}
