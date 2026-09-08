//! 入口层读取和序列化的沙箱配置 DTO。
//!
//! 这些结构只表达可选配置值，不直接执行命令、创建网络规则或授予文件权限。`None` 表示
//! 调用方未提供该设置，而不是自动允许对应能力；实际默认值和 fail-closed 行为由消费
//! 配置的控制平面或 sandbox adapter 决定。

use serde::{Deserialize, Serialize};

/// 网络相关的可选沙箱限制。
///
/// 每个字段采用 `Option` 以保留配置文件中的“未指定”与显式空列表/`false` 的区别。将
/// 这些字段组合为最终网络策略时，调用方必须避免把缺失字段解释成无条件放行。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxNetworkConfig {
    /// 允许访问的域名列表；`None` 表示本 DTO 未指定域名策略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_domains: Option<Vec<String>>,
    /// 是否只允许由受控配置管理的域名。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_managed_domains_only: Option<bool>,
    /// 允许连接的 Unix socket 路径列表。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_unix_sockets: Option<Vec<String>>,
    /// 是否允许连接任意 Unix socket；高权限含义由消费者额外审查。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_all_unix_sockets: Option<bool>,
    /// 是否允许子进程监听本地端口。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_local_binding: Option<bool>,
    /// HTTP 代理监听端口，而非远端 URL。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_proxy_port: Option<u16>,
    /// SOCKS 代理监听端口，而非远端 URL。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socks_proxy_port: Option<u16>,
}

/// 文件系统读写规则的可选配置。
///
/// 路径字符串尚未在该 DTO 层完成规范化、符号链接解析或与 WorkPacket 写集的交集；
/// 下游执行器必须在实际访问前完成这些检查。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxFilesystemConfig {
    /// 请求允许写入的路径集合。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_write: Option<Vec<String>>,
    /// 明确拒绝写入的路径集合，通常应优先于 allow 规则。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deny_write: Option<Vec<String>>,
    /// 明确拒绝读取的路径集合。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deny_read: Option<Vec<String>>,
    /// 请求允许读取的路径集合。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_read: Option<Vec<String>>,
    /// 是否仅允许平台管理的可读路径。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_managed_read_paths_only: Option<bool>,
}

/// 为仓库搜索指定的 `rg` 可执行文件及可选固定参数。
///
/// 该配置并不把任意命令提升为模型工具；搜索仍必须沿既有 shell 能力、策略和沙箱边界
/// 执行。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RipgrepConfig {
    /// 已配置的 `rg` 命令或绝对路径。
    pub command: String,
    /// 调用时附加的固定参数；`None` 与空参数列表在序列化中保持可区分。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
}

/// 汇总入口层可识别的沙箱配置开关。
///
/// 这是传输/配置形状，不是安全策略的最终事实源。尤其是 `enabled = Some(false)` 或
/// `allow_unsandboxed_commands = Some(true)` 不能绕过 ControlPlane、审批或平台禁止项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxSettings {
    /// 是否请求启用沙箱。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// 沙箱后端不可用时是否要求失败而不是继续。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fail_if_unavailable: Option<bool>,
    /// 是否在确认已沙箱化后自动允许 shell 类命令；仍受上游能力策略约束。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_allow_bash_if_sandboxed: Option<bool>,
    /// 是否请求允许未沙箱化命令；该高风险请求不等于实际授权。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_unsandboxed_commands: Option<bool>,
    /// 网络规则子配置。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<SandboxNetworkConfig>,
    /// 文件系统规则子配置。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesystem: Option<SandboxFilesystemConfig>,
    /// 是否允许在嵌套环境中使用较弱的兼容性沙箱。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_weaker_nested_sandbox: Option<bool>,
    /// 是否允许较弱的网络隔离兼容模式。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_weaker_network_isolation: Option<bool>,
    /// 不进入沙箱包装的命令名称列表；消费者需要验证命令解析方式。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded_commands: Option<Vec<String>>,
    /// 仓库搜索命令配置。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ripgrep: Option<RipgrepConfig>,
}
