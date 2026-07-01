use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default, alias = "apiTimeoutMs")]
    pub api_timeout_ms: Option<u64>,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub sandbox: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub verbose: bool,
    #[serde(default)]
    pub brief: bool,
    #[serde(default)]
    pub vim_mode: bool,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub advisor_model: Option<String>,
    #[serde(default, alias = "outputStyle")]
    pub output_style: Option<String>,
    #[serde(default, alias = "autoMode")]
    pub auto_mode: AutoModeSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutoModeSettings {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default, alias = "softDeny", alias = "deny")]
    pub soft_deny: Vec<String>,
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

    // Environment variable overrides
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

pub fn get_api_key() -> Option<String> {
    load_config().api_key
}

pub fn get_base_url() -> Option<String> {
    load_config().base_url
}

fn read_config_file() -> Option<Config> {
    let path = config_path()?;
    let contents = fs::read_to_string(path).ok()?;
    toml::from_str(&contents).ok()
}

fn managed_settings_file_path() -> Option<PathBuf> {
    env::var_os("KIANA_MANAGED_SETTINGS_FILE")
        .or_else(|| env::var_os("KIANA_MANAGED_POLICY_FILE"))
        .map(PathBuf::from)
}

fn read_config_overlay_file(path: &Path) -> Option<ConfigOverlay> {
    let contents = fs::read_to_string(path).ok()?;
    parse_config_overlay(path, &contents)
}

fn parse_config_overlay(path: &Path, contents: &str) -> Option<ConfigOverlay> {
    if path.extension().and_then(|ext| ext.to_str()) == Some("json")
        || contents.trim_start().starts_with('{')
    {
        return parse_json_config_overlay(contents);
    }
    toml::from_str(contents).ok()
}

fn parse_json_config_overlay(contents: &str) -> Option<ConfigOverlay> {
    serde_json::from_str(contents).ok()
}

fn apply_config_overlay(config: &mut Config, overlay: ConfigOverlay) {
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
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn append_non_empty_strings(target: &mut Vec<String>, values: Option<Vec<String>>) {
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

pub fn load_config_file() -> Config {
    read_config_file().unwrap_or_default()
}

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
