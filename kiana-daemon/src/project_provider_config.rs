//! 项目本地 provider 配置的 Daemon 侧接线（CI-06）。
//!
//! 生产配置解析只有一条路径：[`kiana_provider::ConfigResolver`]。本模块不引入第二套解析器，
//! 只补齐组合根缺失的两件事：
//! 1. 解析项目覆盖配置的路径（`<project_root>/.kiana/provider-config.json`）；
//! 2. 在**读取任何文件内容之前**先确认调用方给出的 ProjectTrust 事实，然后把文本交给
//!    [`kiana_provider::ProviderGateway::from_workspace`]，由
//!    `ConfigResolver::resolve_with_workspace` 完成 schema/version/unknown-field、端点、
//!    凭据引用等全部校验。
//!
//! ```text
//!   <project_root>/.kiana/provider-config.json
//!        |  未信任：只做存在性判断，文件存在即拒绝，不打开、不解析
//!        v
//!   【本模块 gateway_from_project_config()】 <- 项目覆盖配置的产品入口
//!        v
//!   kiana_provider::ProviderGateway::from_workspace()
//!        v
//!   ConfigResolver::resolve_with_workspace()   <- 唯一解析器
//! ```
//!
//! 信任事实永远由调用方的信任边界（DaemonHost/入口）提供，本模块不从配置文本推断信任，
//! 也不自己查询信任存储。项目覆盖文件不存在时返回 `Ok(None)`，调用方保持现有环境变量/
//! builtin 路径不变。返回的 gateway 仍要经过 `prepare_call`/admission 才能产生模型 effect。

use crate::LocalModelConfig;
use kiana_provider::{
    ProviderConfig, ProviderGateway, WorkspaceConfigTrust, MAX_WORKSPACE_CONFIG_BYTES,
};
use std::io::Read;
use std::path::{Path, PathBuf};

/// 项目本地 provider 覆盖配置的相对路径。
///
/// 与 ProjectTrust 记录的 `.kiana` 目录同级；本文件不是信任来源，是否加载只由调用方传入的
/// [`WorkspaceConfigTrust`] 决定。
pub const PROJECT_PROVIDER_CONFIG_RELATIVE_PATH: &str = ".kiana/provider-config.json";

/// 解析项目覆盖配置的路径；只做路径拼接，不访问文件系统、不判断信任。
pub fn project_provider_config_path(project_root: &Path) -> PathBuf {
    project_root.join(PROJECT_PROVIDER_CONFIG_RELATIVE_PATH)
}

/// 在信任边界之后读取项目 provider 覆盖配置，并统一经 resolver 构造网关。
///
/// 【调用者】组合根（DaemonHost 装配/入口）在已经取得 `ProjectTrustAuthority` 结论后调用。
///
/// 【返回】
/// - `Ok(None)`：项目没有覆盖配置文件；调用方继续环境变量/builtin 路径（现状不变）。
/// - `Ok(Some(gateway))`：覆盖配置已通过 `ConfigResolver` 校验并成为当前连接快照。
/// - `Err(code)`：稳定拒绝码。未信任项目即使带配置文件也只会得到
///   `config_workspace_untrusted`，文件内容不会被读取或解析。
///
/// 【边界】本函数不查询 ProjectTrust、不读取环境变量、不发起网络请求；`trust` 必须来自
/// 同一次信任观察，resolver 会在解析文本前再次校验信任与修订匹配。durable revision 存储
/// 不在本层，快照可在需要时通过 gateway 的 `workspace_configuration_snapshot()` 取得。
pub fn gateway_from_project_config(
    config: LocalModelConfig,
    project_root: &Path,
    trust: WorkspaceConfigTrust<'_>,
) -> Result<Option<ProviderGateway>, String> {
    let path = project_provider_config_path(project_root);
    if !trust.project_trusted {
        // 未信任项目：不打开文件。存在即拒绝，避免不可信内容进入内存；不存在时没有项目资源
        // 可拒绝，保持环境变量/builtin 路径。
        if path.exists() {
            return Err("config_workspace_untrusted".to_owned());
        }
        return Ok(None);
    }
    let Some(raw) = read_bounded_project_config(&path)? else {
        return Ok(None);
    };
    ProviderGateway::from_workspace(
        ProviderConfig {
            provider: config.provider,
            model: config.model,
            base_url: config.base_url,
            api_key: config.api_key,
        },
        &raw,
        trust,
    )
    .map(Some)
    .map_err(|error| error.code)
}

/// 有界读取项目覆盖配置：只接受普通文件，缺失返回 `Ok(None)`。
///
/// 读取上限是 `MAX_WORKSPACE_CONFIG_BYTES + 1`：在 resolver 之前拦住超大输入，避免把项目里
/// 任意大的文件整份读进内存。大小/编码错误的稳定码与 resolver 的输入边界保持一致。
fn read_bounded_project_config(path: &Path) -> Result<Option<String>, String> {
    let file = match open_project_config(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("config_workspace_unreadable".to_owned()),
    };
    let Ok(metadata) = file.metadata() else {
        return Err("config_workspace_unreadable".to_owned());
    };
    if !metadata.is_file() {
        return Err("config_workspace_not_regular_file".to_owned());
    }
    let mut bytes = Vec::new();
    let limit = MAX_WORKSPACE_CONFIG_BYTES as u64 + 1;
    if file.take(limit).read_to_end(&mut bytes).is_err() {
        return Err("config_workspace_unreadable".to_owned());
    }
    if bytes.len() > MAX_WORKSPACE_CONFIG_BYTES {
        return Err("config_workspace_too_large".to_owned());
    }
    let Ok(raw) = String::from_utf8(bytes) else {
        return Err("config_workspace_invalid".to_owned());
    };
    Ok(Some(raw))
}

/// 打开配置文件用于读取；Unix 上带 `O_NONBLOCK`，避免项目在路径上放置 FIFO 时阻塞启动。
///
/// 与 kiana-types 读取 trust 记录的打开方式保持同一思路（它额外使用 `O_NOFOLLOW`）；
/// 这里只防阻塞，不改变符号链接语义，普通文件上 `O_NONBLOCK` 没有副作用。
fn open_project_config(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    options.open(path)
}
