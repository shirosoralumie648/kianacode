use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

pub const NATIVE_HOST_IDENTIFIER: &str = "com.anthropic.claude_code_browser_extension";
pub const NATIVE_HOST_MANIFEST_NAME: &str = "com.anthropic.claude_code_browser_extension.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NativeHostPlatform {
    Linux,
    Macos,
    Windows,
}

#[derive(Debug, Clone)]
pub struct NativeHostInstallOptions {
    pub platform: NativeHostPlatform,
    pub home_dir: PathBuf,
    pub binary_path: PathBuf,
    pub include_dev_origins: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeHostInstallPlan {
    pub platform: NativeHostPlatform,
    pub identifier: String,
    pub wrapper_path: PathBuf,
    pub wrapper_content: String,
    pub manifest_content: String,
    pub manifest_paths: Vec<PathBuf>,
    pub windows_registry_plans: Vec<WindowsRegistryPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowsRegistryPlan {
    pub browser: String,
    pub key: String,
    pub full_key: String,
    pub add: Vec<String>,
    pub query: Vec<String>,
    pub delete: Vec<String>,
    pub expected_value: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeHostInstallReport {
    pub plan: NativeHostInstallPlan,
    pub written_files: Vec<PathBuf>,
    pub skipped_files: Vec<PathBuf>,
}

pub fn default_install_options(binary_path: PathBuf) -> NativeHostInstallOptions {
    NativeHostInstallOptions {
        platform: current_platform(),
        home_dir: default_home_dir(),
        binary_path,
        include_dev_origins: is_ant_user(),
    }
}

pub fn plan_native_host_install(
    options: &NativeHostInstallOptions,
) -> Result<NativeHostInstallPlan> {
    let wrapper_path = wrapper_path(options);
    let wrapper_content = wrapper_content(options.platform, &options.binary_path);
    let manifest_paths = manifest_paths(options.platform, &options.home_dir);
    if manifest_paths.is_empty() {
        return Err(anyhow!(
            "Chrome native host manifests are not supported on this platform"
        ));
    }
    let manifest_path_for_registry = manifest_paths[0].clone();
    let manifest_content =
        serde_json::to_string_pretty(&manifest_json(&wrapper_path, options.include_dev_origins))?;
    let windows_registry_plans = if options.platform == NativeHostPlatform::Windows {
        windows_registry_keys()
            .into_iter()
            .map(|(browser, key)| windows_registry_plan(browser, key, &manifest_path_for_registry))
            .collect()
    } else {
        Vec::new()
    };

    Ok(NativeHostInstallPlan {
        platform: options.platform,
        identifier: NATIVE_HOST_IDENTIFIER.to_string(),
        wrapper_path,
        wrapper_content,
        manifest_content,
        manifest_paths,
        windows_registry_plans,
    })
}

pub fn install_native_host(options: &NativeHostInstallOptions) -> Result<NativeHostInstallReport> {
    let plan = plan_native_host_install(options)?;
    let mut written_files = Vec::new();
    let mut skipped_files = Vec::new();

    write_if_changed(
        &plan.wrapper_path,
        &plan.wrapper_content,
        options.platform,
        &mut written_files,
        &mut skipped_files,
    )?;
    for manifest_path in &plan.manifest_paths {
        write_if_changed(
            manifest_path,
            &plan.manifest_content,
            options.platform,
            &mut written_files,
            &mut skipped_files,
        )?;
    }

    Ok(NativeHostInstallReport {
        plan,
        written_files,
        skipped_files,
    })
}

fn write_if_changed(
    path: &Path,
    content: &str,
    platform: NativeHostPlatform,
    written_files: &mut Vec<PathBuf>,
    skipped_files: &mut Vec<PathBuf>,
) -> Result<()> {
    if std::fs::read_to_string(path).ok().as_deref() == Some(content) {
        skipped_files.push(path.to_path_buf());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    if platform != NativeHostPlatform::Windows
        && path.file_name().and_then(|name| name.to_str()) == Some("chrome-native-host")
    {
        set_executable(path)?;
    }
    written_files.push(path.to_path_buf());
    Ok(())
}

fn wrapper_path(options: &NativeHostInstallOptions) -> PathBuf {
    let chrome_dir = if let Ok(path) = std::env::var("KIANA_CHROME_NATIVE_HOST_WRAPPER_DIR") {
        PathBuf::from(path)
    } else if let Ok(path) = std::env::var("KIANA_HOME") {
        PathBuf::from(path).join("chrome")
    } else {
        options.home_dir.join(".kiana").join("chrome")
    };
    chrome_dir.join(if options.platform == NativeHostPlatform::Windows {
        "chrome-native-host.bat"
    } else {
        "chrome-native-host"
    })
}

fn wrapper_content(platform: NativeHostPlatform, binary_path: &Path) -> String {
    if platform == NativeHostPlatform::Windows {
        format!(
            "@echo off\r\nREM Chrome native host wrapper script\r\nREM Generated by Kiana Code - do not edit manually\r\n\"{}\" --chrome-native-host\r\n",
            binary_path.display()
        )
    } else {
        format!(
            "#!/bin/sh\n# Chrome native host wrapper script\n# Generated by Kiana Code - do not edit manually\nexec {} --chrome-native-host\n",
            shell_quote(&binary_path.display().to_string())
        )
    }
}

fn manifest_json(wrapper_path: &Path, include_dev_origins: bool) -> serde_json::Value {
    let mut allowed_origins = vec!["chrome-extension://fcoeoabgfenejglbffodgkkbkcdhcgfn/"];
    if include_dev_origins {
        allowed_origins.push("chrome-extension://dihbgbndebgnbjfmelmegjepbnkhlgni/");
        allowed_origins.push("chrome-extension://dngcpimnedloihjnnfngkgjoidhnaolf/");
    }
    json!({
        "name": NATIVE_HOST_IDENTIFIER,
        "description": "Kiana Code Browser Extension Native Host",
        "path": wrapper_path,
        "type": "stdio",
        "allowed_origins": allowed_origins,
    })
}

fn manifest_paths(platform: NativeHostPlatform, home: &Path) -> Vec<PathBuf> {
    match platform {
        NativeHostPlatform::Linux => linux_manifest_dirs(home)
            .into_iter()
            .map(|dir| dir.join(NATIVE_HOST_MANIFEST_NAME))
            .collect(),
        NativeHostPlatform::Macos => macos_manifest_dirs(home)
            .into_iter()
            .map(|dir| dir.join(NATIVE_HOST_MANIFEST_NAME))
            .collect(),
        NativeHostPlatform::Windows => {
            vec![windows_manifest_dir(home).join(NATIVE_HOST_MANIFEST_NAME)]
        }
    }
}

fn linux_manifest_dirs(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".config/google-chrome/NativeMessagingHosts"),
        home.join(".config/BraveSoftware/Brave-Browser/NativeMessagingHosts"),
        home.join(".config/chromium/NativeMessagingHosts"),
        home.join(".config/microsoft-edge/NativeMessagingHosts"),
        home.join(".config/vivaldi/NativeMessagingHosts"),
        home.join(".config/opera/NativeMessagingHosts"),
    ]
}

fn macos_manifest_dirs(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join("Library/Application Support/Google/Chrome/NativeMessagingHosts"),
        home.join("Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts"),
        home.join("Library/Application Support/Arc/User Data/NativeMessagingHosts"),
        home.join("Library/Application Support/Chromium/NativeMessagingHosts"),
        home.join("Library/Application Support/Microsoft Edge/NativeMessagingHosts"),
        home.join("Library/Application Support/Vivaldi/NativeMessagingHosts"),
        home.join("Library/Application Support/com.operasoftware.Opera/NativeMessagingHosts"),
    ]
}

fn windows_manifest_dir(home: &Path) -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata)
            .join("Claude Code")
            .join("ChromeNativeHost");
    }
    home.join("AppData")
        .join("Local")
        .join("Claude Code")
        .join("ChromeNativeHost")
}

fn windows_registry_keys() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "chrome",
            "HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts",
        ),
        (
            "brave",
            "HKCU\\Software\\BraveSoftware\\Brave-Browser\\NativeMessagingHosts",
        ),
        (
            "arc",
            "HKCU\\Software\\ArcBrowser\\Arc\\NativeMessagingHosts",
        ),
        ("chromium", "HKCU\\Software\\Chromium\\NativeMessagingHosts"),
        (
            "edge",
            "HKCU\\Software\\Microsoft\\Edge\\NativeMessagingHosts",
        ),
        ("vivaldi", "HKCU\\Software\\Vivaldi\\NativeMessagingHosts"),
        (
            "opera",
            "HKCU\\Software\\Opera Software\\Opera Stable\\NativeMessagingHosts",
        ),
    ]
}

fn windows_registry_plan(
    browser: &'static str,
    key: &'static str,
    manifest_path: &Path,
) -> WindowsRegistryPlan {
    let full_key = format!("{key}\\{NATIVE_HOST_IDENTIFIER}");
    let manifest = manifest_path.display().to_string();
    WindowsRegistryPlan {
        browser: browser.to_string(),
        key: key.to_string(),
        full_key: full_key.clone(),
        add: vec![
            "reg".to_string(),
            "add".to_string(),
            full_key.clone(),
            "/ve".to_string(),
            "/t".to_string(),
            "REG_SZ".to_string(),
            "/d".to_string(),
            manifest.clone(),
            "/f".to_string(),
        ],
        query: vec![
            "reg".to_string(),
            "query".to_string(),
            full_key.clone(),
            "/ve".to_string(),
        ],
        delete: vec![
            "reg".to_string(),
            "delete".to_string(),
            full_key,
            "/ve".to_string(),
            "/f".to_string(),
        ],
        expected_value: manifest_path.to_path_buf(),
    }
}

fn current_platform() -> NativeHostPlatform {
    if cfg!(target_os = "windows") {
        NativeHostPlatform::Windows
    } else if cfg!(target_os = "macos") {
        NativeHostPlatform::Macos
    } else {
        NativeHostPlatform::Linux
    }
}

fn default_home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn is_ant_user() -> bool {
    std::env::var("USER_TYPE")
        .map(|value| value == "ant")
        .unwrap_or(false)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-native-install-{name}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn linux_plan_contains_wrapper_and_browser_manifests() {
        let home = temp_home("plan-linux");
        let options = NativeHostInstallOptions {
            platform: NativeHostPlatform::Linux,
            home_dir: home.clone(),
            binary_path: PathBuf::from("/opt/kiana/bin/kiana"),
            include_dev_origins: false,
        };

        let plan = plan_native_host_install(&options).unwrap();

        assert_eq!(plan.identifier, NATIVE_HOST_IDENTIFIER);
        assert_eq!(
            plan.wrapper_path,
            home.join(".kiana/chrome/chrome-native-host")
        );
        assert!(plan
            .wrapper_content
            .contains("exec '/opt/kiana/bin/kiana' --chrome-native-host"));
        assert!(plan.manifest_paths.iter().any(|path| path.ends_with(".config/google-chrome/NativeMessagingHosts/com.anthropic.claude_code_browser_extension.json")));
        let manifest: serde_json::Value = serde_json::from_str(&plan.manifest_content).unwrap();
        assert_eq!(manifest["name"], NATIVE_HOST_IDENTIFIER);
        assert_eq!(
            manifest["path"].as_str(),
            Some(plan.wrapper_path.to_string_lossy().as_ref())
        );
        assert_eq!(manifest["allowed_origins"].as_array().unwrap().len(), 1);
        assert!(plan.windows_registry_plans.is_empty());
    }

    #[test]
    fn windows_plan_contains_registry_commands() {
        let home = temp_home("plan-windows");
        let options = NativeHostInstallOptions {
            platform: NativeHostPlatform::Windows,
            home_dir: home.clone(),
            binary_path: PathBuf::from(r"C:\Kiana\kiana.exe"),
            include_dev_origins: true,
        };

        let plan = plan_native_host_install(&options).unwrap();

        assert_eq!(plan.manifest_paths.len(), 1);
        assert!(plan.wrapper_path.ends_with("chrome-native-host.bat"));
        assert_eq!(plan.windows_registry_plans.len(), 7);
        let chrome = plan
            .windows_registry_plans
            .iter()
            .find(|plan| plan.browser == "chrome")
            .unwrap();
        assert!(chrome.full_key.ends_with(NATIVE_HOST_IDENTIFIER));
        assert!(chrome.add.contains(&"REG_SZ".to_string()));
        assert_eq!(chrome.expected_value, plan.manifest_paths[0]);
        let manifest: serde_json::Value = serde_json::from_str(&plan.manifest_content).unwrap();
        assert_eq!(manifest["allowed_origins"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn install_writes_wrapper_and_manifests_idempotently() {
        let home = temp_home("install");
        let options = NativeHostInstallOptions {
            platform: NativeHostPlatform::Linux,
            home_dir: home.clone(),
            binary_path: PathBuf::from("/tmp/kiana"),
            include_dev_origins: false,
        };

        let first = install_native_host(&options).unwrap();
        assert!(first.written_files.contains(&first.plan.wrapper_path));
        assert_eq!(first.plan.manifest_paths.len(), 6);
        for manifest_path in &first.plan.manifest_paths {
            assert!(manifest_path.is_file());
        }

        let second = install_native_host(&options).unwrap();
        assert!(second.written_files.is_empty());
        assert_eq!(
            second.skipped_files.len(),
            first.plan.manifest_paths.len() + 1
        );

        let _ = std::fs::remove_dir_all(home);
    }
}
