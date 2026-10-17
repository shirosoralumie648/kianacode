//! INT-14：CLI / Web / Workbench / MCP 共用的 connector 命令编码入口。
//!
//! # 这个文件解决什么问题
//!
//! 四个界面都能发出 `connector.manage` / `connector.invoke` / `connector.health` /
//! `connector.reconcile`。如果每个界面各自拼 JSON，就迟早会出现「A 界面拼的字段 B 界面没有」，
//! 于是同一件事在四个入口产生四种 wire 形状。本文件提供 CLI / Web / Workbench 共享的 typed
//! 编码入口，让它们在把命令交给 [`crate::command_dispatch`] / `DaemonHost` 之前先经过
//! **同一个**编码；MCP 发起的 connector 命令仍必须从 `DaemonHost` 进入（legacy MCP 工具面
//! 不得本地 mint/normalize 命令），因此四个入口到达控制面时携带同一种 wire 形状。
//! 调用方只给业务字段（binding、operation、payload、idempotency…），
//! `actor_id` / `role_id` / `risk` / `endpoint` / `binding_snapshot` 这类服务端所有值
//! 在 `kiana-domain::ConnectorCommandRequest::validate` 里被拒绝（`connector_server_owned_override`）。
//!
//! # 它不做什么
//!
//! 这里**不做**授权、不查 binding、不碰 Broker，也不决定风险等级。它只做两件事：
//! 1. 把 typed 请求编码成唯一 wire 形状（`ConnectorSurfaceCommand`）；
//! 2. 把通用命令路径上收到的 typed connector 参数重新编码成同一形状。
//! 真正的 normalize 发生在 ControlPlane（`kiana-core`），且只发生一次。
//!
//! # legacy 兼容
//!
//! 没有 `schema` 字段的历史参数（例如只带 `binding_id` 的旧 `/connector-health`）原样通过，
//! 继续走 Core 的兼容分支；本文件不会把它们偷偷升级成 typed 请求。

use kiana_domain::{
    ConnectorCommand, ConnectorCommandRequest, ConnectorSurface, ConnectorSurfaceCommand,
};
use serde_json::Value;

/// 一个界面命令交给唯一 dispatch 路径时的规范化结果。
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceCommandRoute {
    /// 协议命令名（`connector.health` 等；reconcile 保持公开名，由 Core 映射到 manage 路由）。
    pub name: String,
    /// typed 命令参数；与其它界面发出的同一逻辑请求逐字节一致。
    pub arguments: Value,
    /// 是否走了 INT-14 的 typed connector 编码；`false` 表示 legacy 兼容参数原样通过。
    pub typed: bool,
}

/// 把通用命令路径（`kiana command`、Web `/api/command`）上收到的参数编码成共享形状。
///
/// 只有带 `kiana.connector-command.v1` schema 的参数才会被 typed 重编码；其余原样返回，
/// 保持历史兼容。重编码会重新校验服务端所有字段并拒绝未知字段，因此伪造的
/// `actor_id` / `role_id` / `risk` / `endpoint` / `binding_snapshot` 在界面边界就失败。
pub fn surface_command_route(
    surface: ConnectorSurface,
    name: &str,
    arguments: Value,
) -> Result<SurfaceCommandRoute, String> {
    match kiana_domain::normalize_surface_connector_arguments(surface, name, &arguments)? {
        Some(encoded) => Ok(SurfaceCommandRoute {
            name: encoded.wire_name,
            arguments: encoded.arguments,
            typed: true,
        }),
        None => Ok(SurfaceCommandRoute {
            name: name.to_owned(),
            arguments,
            typed: false,
        }),
    }
}

/// 构造一次 typed read-only health 命令（Workbench `/connector-health` 使用）。
///
/// binding 解析、scope 检查与探测仍由 ControlPlane/Broker 完成；这里只产出 typed 请求。
pub fn connector_health_route(
    surface: ConnectorSurface,
    binding_id: &str,
) -> Result<SurfaceCommandRoute, String> {
    let mut request = ConnectorCommandRequest::new(ConnectorCommand::Health);
    request.binding_id = Some(binding_id.to_owned());
    let encoded = ConnectorSurfaceCommand::encode(surface, &request)?;
    Ok(SurfaceCommandRoute {
        name: encoded.wire_name,
        arguments: encoded.arguments,
        typed: true,
    })
}
