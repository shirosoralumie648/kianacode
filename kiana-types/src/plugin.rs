use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<PluginAuthor>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAuthor {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandMetadata {
    #[serde(flatten)]
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginComponent {
    Commands,
    Agents,
    Skills,
    Hooks,
    OutputStyles,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PluginError {
    #[serde(rename = "path-not-found")]
    PathNotFound {
        source: String,
        plugin: Option<String>,
        path: String,
        component: PluginComponent,
    },
    #[serde(rename = "git-auth-failed")]
    GitAuthFailed {
        source: String,
        plugin: Option<String>,
        git_url: String,
        auth_type: String,
    },
    #[serde(rename = "git-timeout")]
    GitTimeout {
        source: String,
        plugin: Option<String>,
        git_url: String,
        operation: String,
    },
    #[serde(rename = "network-error")]
    NetworkError {
        source: String,
        plugin: Option<String>,
        url: String,
        details: Option<String>,
    },
    #[serde(rename = "manifest-parse-error")]
    ManifestParseError {
        source: String,
        plugin: Option<String>,
        manifest_path: String,
        parse_error: String,
    },
    #[serde(rename = "manifest-validation-error")]
    ManifestValidationError {
        source: String,
        plugin: Option<String>,
        manifest_path: String,
        validation_errors: Vec<String>,
    },
    #[serde(rename = "plugin-not-found")]
    PluginNotFound {
        source: String,
        plugin_id: String,
        marketplace: String,
    },
    #[serde(rename = "marketplace-not-found")]
    MarketplaceNotFound {
        source: String,
        marketplace: String,
        available_marketplaces: Vec<String>,
    },
    #[serde(rename = "marketplace-load-failed")]
    MarketplaceLoadFailed {
        source: String,
        marketplace: String,
        reason: String,
    },
    #[serde(rename = "mcp-config-invalid")]
    McpConfigInvalid {
        source: String,
        plugin: String,
        server_name: String,
        validation_error: String,
    },
    #[serde(rename = "generic-error")]
    GenericError {
        source: String,
        plugin: Option<String>,
        error: String,
    },
}

pub const KIANA_PLUGINS_DIR_ENV: &str = "KIANA_PLUGINS_DIR";
pub const KIANA_HOME_ENV: &str = "KIANA_HOME";
pub const DISABLED_PLUGINS_FILE: &str = "disabled_plugins.json";

pub fn plugins_dir() -> PathBuf {
    user_plugins_dir().unwrap_or_else(|| PathBuf::from(".kiana").join("plugins"))
}

pub fn user_plugins_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(KIANA_PLUGINS_DIR_ENV) {
        return Some(PathBuf::from(path));
    }
    if let Some(home) = std::env::var_os(KIANA_HOME_ENV) {
        return Some(PathBuf::from(home).join("plugins"));
    }
    home_dir().map(|home| home.join(".kiana").join("plugins"))
}

pub fn installed_plugin_roots() -> Vec<PathBuf> {
    user_plugins_dir()
        .map(|dir| enabled_plugin_roots_in(&dir))
        .unwrap_or_default()
}

pub fn all_plugin_roots_in(plugins_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(plugins_dir) else {
        return Vec::new();
    };
    let mut roots = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    roots.sort();
    dedupe_paths(roots)
}

pub fn enabled_plugin_roots_in(plugins_dir: &Path) -> Vec<PathBuf> {
    let disabled = disabled_plugin_names(plugins_dir);
    all_plugin_roots_in(plugins_dir)
        .into_iter()
        .filter(|root| !plugin_is_disabled(root, &disabled))
        .collect()
}

pub fn disabled_plugin_names(plugins_dir: &Path) -> HashSet<String> {
    let Some(value) = fs::read_to_string(plugin_state_path(plugins_dir))
        .ok()
        .and_then(|contents| serde_json::from_str::<Value>(&contents).ok())
    else {
        return HashSet::new();
    };
    parse_disabled_plugin_names(&value)
}

pub fn plugin_state_path(plugins_dir: &Path) -> PathBuf {
    plugins_dir.join(DISABLED_PLUGINS_FILE)
}

pub fn set_plugin_enabled(
    plugins_dir: &Path,
    plugin_name: &str,
    enabled: bool,
) -> Result<PathBuf, String> {
    let name = normalize_plugin_name(plugin_name)
        .ok_or_else(|| "plugin name cannot be empty".to_string())?;
    let mut disabled = disabled_plugin_names(plugins_dir);
    if enabled {
        disabled.remove(&name);
    } else {
        disabled.insert(name);
    }
    write_disabled_plugin_names(plugins_dir, &disabled)
}

pub fn plugin_is_disabled(plugin_root: &Path, disabled: &HashSet<String>) -> bool {
    plugin_identity_names(plugin_root)
        .into_iter()
        .filter_map(|name| normalize_plugin_name(&name))
        .any(|name| disabled.contains(&name))
}

pub fn plugin_identity_names(plugin_root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(folder_name) = plugin_root.file_name().and_then(|name| name.to_str()) {
        names.push(folder_name.to_string());
    }
    if let Some(manifest_name) = plugin_manifest_name(plugin_root) {
        if !names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&manifest_name))
        {
            names.push(manifest_name);
        }
    }
    names
}

pub fn find_manifest_path(plugin_root: &Path) -> Option<PathBuf> {
    [
        plugin_root.join(".codex-plugin").join("plugin.json"),
        plugin_root.join(".claude-plugin").join("plugin.json"),
        plugin_root.join("plugin.json"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn plugin_manifest_name(plugin_root: &Path) -> Option<String> {
    let manifest_path = find_manifest_path(plugin_root)?;
    let value = fs::read_to_string(manifest_path)
        .ok()
        .and_then(|contents| serde_json::from_str::<Value>(&contents).ok())?;
    value
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn parse_disabled_plugin_names(value: &Value) -> HashSet<String> {
    let source = value
        .get("disabled")
        .or_else(|| value.get("disabledPlugins"))
        .unwrap_or(value);
    let mut disabled = HashSet::new();
    if let Some(names) = source.as_array() {
        for name in names {
            if let Some(name) = name.as_str().and_then(normalize_plugin_name) {
                disabled.insert(name);
            }
        }
    }
    disabled
}

fn write_disabled_plugin_names(
    plugins_dir: &Path,
    disabled: &HashSet<String>,
) -> Result<PathBuf, String> {
    fs::create_dir_all(plugins_dir)
        .map_err(|error| format!("failed to create {}: {error}", plugins_dir.display()))?;
    let mut disabled = disabled.iter().cloned().collect::<Vec<_>>();
    disabled.sort();
    let state = serde_json::json!({ "disabled": disabled });
    let path = plugin_state_path(plugins_dir);
    let contents = serde_json::to_string_pretty(&state)
        .map_err(|error| format!("failed to serialize plugin state: {error}"))?;
    fs::write(&path, contents)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    Ok(path)
}

fn normalize_plugin_name(name: &str) -> Option<String> {
    let name = name.trim().trim_start_matches('/').to_ascii_lowercase();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| {
            seen.insert(
                path.canonicalize()
                    .unwrap_or_else(|_| path.to_path_buf())
                    .to_string_lossy()
                    .to_string(),
            )
        })
        .collect()
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct EnvSnapshot {
        values: Vec<(&'static str, Option<OsString>)>,
    }

    impl EnvSnapshot {
        fn take(keys: &[&'static str]) -> Self {
            Self {
                values: keys
                    .iter()
                    .map(|key| (*key, std::env::var_os(key)))
                    .collect(),
            }
        }
    }

    impl Drop for EnvSnapshot {
        fn drop(&mut self) {
            for (key, value) in self.values.iter().rev() {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    struct CurrentDirSnapshot(PathBuf);

    impl CurrentDirSnapshot {
        fn set(path: &Path) -> Self {
            let previous = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self(previous)
        }
    }

    impl Drop for CurrentDirSnapshot {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    #[test]
    fn installed_plugin_roots_do_not_fall_back_to_project_relative_plugins() {
        let _process_env_lock = crate::process_env_lock();
        let _env =
            EnvSnapshot::take(&[KIANA_PLUGINS_DIR_ENV, KIANA_HOME_ENV, "HOME", "USERPROFILE"]);
        std::env::remove_var(KIANA_PLUGINS_DIR_ENV);
        std::env::remove_var(KIANA_HOME_ENV);
        std::env::remove_var("HOME");
        std::env::remove_var("USERPROFILE");
        let root = std::env::temp_dir().join(format!(
            "kiana-plugin-no-home-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let plugin_root = root
            .join(".kiana")
            .join("plugins")
            .join("project-controlled");
        fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
        fs::write(
            plugin_root.join(".codex-plugin").join("plugin.json"),
            r#"{"name":"project-controlled"}"#,
        )
        .unwrap();
        let _cwd = CurrentDirSnapshot::set(&root);

        assert!(installed_plugin_roots().is_empty());

        let _ = fs::remove_dir_all(root);
    }
}
