//! # 文件职责
//!
//! 本模块把用户/平台配置转换为 Chrome Native Host 的 wrapper、manifest 和可选 Windows 注册表
//! 命令计划，并提供实际写入 wrapper/manifest 的安装函数。它不启动 MCP server，也不执行浏览器操作。
//!
//! ```text
//! kiana CLI 的 chrome plan/install 命令
//!   -> 默认选项 + CLI 覆盖项
//!   -> plan_native_host_install() 生成可检查计划
//!   -> install_native_host() 写 wrapper 与 manifest
//!   -> CLI 展示写入/跳过清单
//! ```
//!
//! 该安装入口会按显式 home/path 和当前环境变量写本机文件；它不是 ProjectTrust 资源加载器，
//! 也不经 ControlPlane。Windows registry command 目前只是计划数据，`install_native_host()` 不执行它们。
//! 调用者和用户必须把计划/写入视为对本机配置的副作用。

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

/// Chrome Native Messaging manifest 中使用的 host 名称。
pub const NATIVE_HOST_IDENTIFIER: &str = "com.anthropic.claude_code_browser_extension";
/// manifest 文件名；各浏览器依此名字发现 host 配置。
pub const NATIVE_HOST_MANIFEST_NAME: &str = "com.anthropic.claude_code_browser_extension.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
/// 安装计划中目标操作系统的分类；它也可以由 CLI 显式覆盖当前构建平台。
pub enum NativeHostPlatform {
    /// Linux 用户配置目录与 Unix shell wrapper。
    Linux,
    /// macOS 用户 Library/Application Support 目录与 Unix shell wrapper。
    Macos,
    /// Windows 用户目录、批处理 wrapper 和注册表命令计划。
    Windows,
}

#[derive(Debug, Clone)]
/// 输入给 planner/installer 的本机路径和平台选项。
pub struct NativeHostInstallOptions {
    /// 目标平台；可用于 dry-run 生成其他平台计划。
    pub platform: NativeHostPlatform,
    /// 用户 home 根目录；用于默认安装位置。
    pub home_dir: PathBuf,
    /// 被 wrapper 启动的 Kiana 可执行文件路径。
    pub binary_path: PathBuf,
    /// 是否把开发版浏览器扩展 origin 也写入允许列表。
    pub include_dev_origins: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 纯数据安装计划：描述拟写内容和拟执行命令，本身不产生文件系统或注册表副作用。
pub struct NativeHostInstallPlan {
    /// 计划目标平台。
    pub platform: NativeHostPlatform,
    /// manifest 与浏览器注册表关联使用的 host ID。
    pub identifier: String,
    /// wrapper 文件的目标路径。
    pub wrapper_path: PathBuf,
    /// wrapper 的完整文本内容。
    pub wrapper_content: String,
    /// Native Messaging manifest 的 JSON 文本。
    pub manifest_content: String,
    /// 需要安装 manifest 的浏览器配置路径列表。
    pub manifest_paths: Vec<PathBuf>,
    /// Windows 浏览器注册表的 add/query/delete argv 计划；仅描述，不由本模块执行。
    pub windows_registry_plans: Vec<WindowsRegistryPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 单个 Windows 浏览器 host 注册表项的命令行计划。
pub struct WindowsRegistryPlan {
    /// 面向用户展示的浏览器键名。
    pub browser: String,
    /// 浏览器 NativeMessagingHosts 父键。
    pub key: String,
    /// 包含具体 host ID 的完整 registry key。
    pub full_key: String,
    /// 创建或更新注册表值的 argv；这里只生成，不调用 `reg.exe`。
    pub add: Vec<String>,
    /// 查询该值的 argv。
    pub query: Vec<String>,
    /// 删除该值的 argv。
    pub delete: Vec<String>,
    /// 预期写入 registry 默认值的 manifest 路径。
    pub expected_value: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 安装执行后的汇总：包含原计划以及实际写入/因内容相同而跳过的文件。
pub struct NativeHostInstallReport {
    /// 本次操作使用的完整计划，便于 CLI 展示目标路径和内容意图。
    pub plan: NativeHostInstallPlan,
    /// 本次实际写入成功的 wrapper/manifest 路径。
    pub written_files: Vec<PathBuf>,
    /// 文件内容已与计划相同，未重新写入的路径。
    pub skipped_files: Vec<PathBuf>,
}

/// 根据当前环境生成默认安装选项；显式 CLI 参数可以在调用方覆盖这些默认值。
///
/// 平台取编译时目标、home 取环境变量、wrapper 文件由 binary_path 决定；`USER_TYPE=ant` 时默认
/// 包含开发 origin。读取环境变量只是构造选项，不写文件或检查外部审批。
pub fn default_install_options(binary_path: PathBuf) -> NativeHostInstallOptions {
    NativeHostInstallOptions {
        platform: current_platform(),
        home_dir: default_home_dir(),
        binary_path,
        include_dev_origins: is_ant_user(),
    }
}

/// 从选项构造可显示/序列化的安装计划，不在磁盘创建目录或文件。
///
/// 【数据流】决定 wrapper 路径和内容 -> 找到平台的 manifest 目录 -> 编码 manifest JSON ->
/// Windows 时额外生成注册表命令向量 -> 返回完整计划。
///
/// 如果目标平台没有受支持的 manifest 路径，或 manifest JSON 编码失败则返回 Err。Windows registry
/// 命令保存在计划中供上层展示/执行，本 crate 的 installer 不运行这些命令。
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

/// 执行计划中的文件安装：写 wrapper 与每个浏览器 manifest，并返回实际写入/跳过列表。
///
/// 【副作用与失败】可能创建父目录、覆盖 wrapper/manifest 内容并调整 Unix wrapper 权限；任何一次
/// 文件操作失败立即返回 Err。前面已成功的写入不会自动回滚，因此失败可能留下部分安装状态。
///
/// ⚠ Windows registry plans 只随 report 返回，不会在这里调用 `reg.exe`。此外，若 wrapper 内容相同，
/// `write_if_changed()` 会提前跳过，也不会检查或修复该文件的 executable bit。
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

/// 内容相同时跳过文件写入，否则创建父目录并写入完整内容。
///
/// Unix 平台下，文件名恰为 `chrome-native-host` 时再设置可执行权限。读文件失败会继续尝试写入；
/// 写入或权限设置失败返回 Err。函数不做临时文件替换或事务回滚。
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

/// 按专用 wrapper 环境变量、KIANA_HOME、显式 home 的顺序选择 wrapper 目录。
///
/// 默认位置仅针对本 Native Host 安装器：`<home>/.kiana/chrome`；若调用方给出环境变量，它可以
/// 改变写入目录。这里不解析普通项目内 `.kiana` 路径。
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

/// 生成由浏览器调用的脚本内容。
///
/// Windows 使用 bat wrapper；Unix 使用 `shell_quote()` 单引号包裹 binary path，并通过 `exec` 让
/// wrapper 进程替换为 Kiana。返回的是待写入文本，不会立即启动 binary。
fn wrapper_content(platform: NativeHostPlatform, binary_path: &Path) -> String {
    if platform == NativeHostPlatform::Windows {
        // ⚠ Windows 分支只在路径两侧添加双引号，没有额外转义路径内部的双引号或批处理元字符；
        // 不能据此认为任意用户输入路径都能安全表达为 batch 命令。
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

/// 构造 Native Messaging manifest JSON，包括 host ID、wrapper path、stdio transport 和允许 origin。
///
/// 开发 origin 仅在显式 `include_dev_origins=true` 时追加；这里没有检查扩展是否实际安装或可信。
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

/// 把平台对应的浏览器配置目录转换成同名 manifest 文件路径。
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

/// 返回当前支持的 Linux 浏览器 NativeMessagingHosts 配置目录。
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

/// 返回当前支持的 macOS 浏览器 NativeMessagingHosts 配置目录。
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

/// 选取 Windows manifest 目录；APPDATA 存在时优先使用它，否则落在传入 home 下。
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

/// 列出生成 Windows registry 计划时支持的浏览器父键。
///
/// 这个列表只决定报告中包含哪些 `reg` 命令，不会自动证明相应浏览器存在。
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

/// 为一个浏览器拼出具体 host 注册表键以及 add/query/delete 参数向量。
///
/// 使用 argv 列表而不是 shell 命令字符串，是为了让调用方可以按参数边界展示或启动子进程；
/// 本函数本身不执行这些操作。
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

/// 按当前编译目标返回默认平台；未匹配 Windows/macOS 的目标按 Linux 处理。
fn current_platform() -> NativeHostPlatform {
    if cfg!(target_os = "windows") {
        NativeHostPlatform::Windows
    } else if cfg!(target_os = "macos") {
        NativeHostPlatform::Macos
    } else {
        NativeHostPlatform::Linux
    }
}

/// 读取常见 home 环境变量；两者都缺失时使用当前相对目录 `.`。
fn default_home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 判断默认配置是否应追加开发扩展 origin；只精确接受 `USER_TYPE=ant`。
fn is_ant_user() -> bool {
    std::env::var("USER_TYPE")
        .map(|value| value == "ant")
        .unwrap_or(false)
}

/// 将单引号字符串转换为 shell 单引号包裹形式，内部单引号使用 shell 片段拼接转义。
///
/// 该 helper 只用于 Unix wrapper；Windows bat wrapper 使用另一套字符串格式化路径。
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(unix)]
/// 将 Unix wrapper 文件 mode 设置为 0755（所有者可写，所有用户可读/执行）。
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
/// 非 Unix 平台不需要本模块使用的 POSIX executable bit 设置。
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
/// 安装计划和文件副作用的本地测试夹具；它不访问真实浏览器配置目录。
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
