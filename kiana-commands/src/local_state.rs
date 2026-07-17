use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub fn app_state_array_len(app_state: &HashMap<String, Value>, key: &str) -> usize {
    app_state
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

pub fn app_state_keys(app_state: &HashMap<String, Value>) -> Vec<String> {
    let mut keys: Vec<String> = app_state.keys().cloned().collect();
    keys.sort();
    keys
}

pub fn sdk_sessions_dir() -> PathBuf {
    if let Ok(path) = std::env::var("KIANA_SDK_SESSIONS_DIR") {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("KIANA_HOME") {
        return PathBuf::from(path).join("sdk-sessions");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".kiana").join("sdk-sessions");
    }
    PathBuf::from(".kiana").join("sdk-sessions")
}

pub fn kiana_home_dir() -> PathBuf {
    if let Ok(path) = std::env::var("KIANA_HOME") {
        return PathBuf::from(path);
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".kiana");
    }
    PathBuf::from(".kiana")
}

pub fn config_path() -> PathBuf {
    if let Some(path) = kiana_bootstrap::config::config_path() {
        return path;
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".kiana").join("config.toml");
    }
    PathBuf::from(".kiana").join("config.toml")
}

pub fn bool_label(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

pub fn on_off(value: bool) -> &'static str {
    if value {
        "on"
    } else {
        "off"
    }
}

pub fn load_user_config() -> kiana_bootstrap::config::Config {
    kiana_bootstrap::config::load_config_file()
}

pub fn save_user_config(config: &kiana_bootstrap::config::Config) -> anyhow::Result<PathBuf> {
    kiana_bootstrap::config::save_config_file(config).map_err(anyhow::Error::msg)
}

#[derive(Debug, Clone, Default)]
pub struct SdkSessionStats {
    pub session_count: usize,
    pub message_count: usize,
    pub latest_updated_at: Option<u64>,
}

pub fn sdk_session_stats() -> SdkSessionStats {
    let dir = sdk_sessions_dir();
    let Ok(entries) = fs::read_dir(dir) else {
        return SdkSessionStats::default();
    };

    let mut stats = SdkSessionStats::default();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&contents) else {
            continue;
        };
        stats.session_count += 1;
        stats.message_count += value
            .get("messages")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        if let Some(updated_at) = value.get("updated_at").and_then(Value::as_u64) {
            stats.latest_updated_at = Some(stats.latest_updated_at.unwrap_or(0).max(updated_at));
        }
    }
    stats
}

#[cfg(test)]
pub fn env_lock() -> &'static std::sync::Mutex<()> {
    crate::test_env_lock()
}

#[cfg(test)]
mod tests {
    #[test]
    fn command_tests_share_one_process_environment_lock() {
        assert!(std::ptr::eq(super::env_lock(), crate::test_env_lock()));
    }
}
