//! AUT-23 五入口 DaemonHost/ControlPlane UAT parity 声明与静态校验。
//!
//! # 系统位置和数据流
//!
//! ```text
//! 外部 UAT harness / 测试调用方收集 CLI、Web、Workbench、Desktop、MCP 的观测值
//!     -> 组装五个 AutomationSurfaceUatCase
//!     -> AutomationSurfaceUat::validate()
//!     -> 检查入口集合、共同 command/snapshot/version、拒绝时零调用、禁止 direct route 和摘要
//!     -> 返回成功或 aut23_* 错误
//! ```
//!
//! 本文件只有报告 DTO 和纯校验，没有启动 DaemonHost、调用五种 adapter、执行命令或读取真实快照的代码。
//! Core facade 也只转发校验；当前没有在生产入口源码中找到生产 UAT 消费者。因此通过表示“传入的观测
//! 声明符合矩阵规则”，不证明五个入口真实访问同一 daemon、执行过 Broker/handler 或拒绝路径实际无副作用。
//! `command_digest`、`snapshot_digest` 和本对象 digest 都是无密钥 SHA-256 内容指纹，不是签名或回执。

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

/// AUT-23 五入口 UAT 矩阵的固定序列化 schema 名称。
pub const AUT23_SURFACE_UAT_SCHEMA: &str = "kiana.aut23-surface-uat.v1";

/// 校验必填文本的长度与控制字符边界；不验证它指向的 command 或 artifact 是否存在。
fn required(value: &str, field: &'static str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 4096 || value.contains(['\0', '\r', '\n']) {
        Err(field.to_owned())
    } else {
        Ok(())
    }
}

/// 校验 `sha256:` 加 64 位十六进制文本；这里只验格式，不重算被引用对象。
fn digest(value: &str, field: &'static str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field.to_owned());
    };
    if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field.to_owned())
    }
}

/// UAT 观测所属的五种产品入口。
///
/// `rename_all = "snake_case"` 是 Serde 的序列化规则，例如 `Workbench` 在线路 JSON 中写作
/// `workbench`；这里只命名入口，不持有各入口 adapter 或 daemon 连接。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationUatSurface {
    /// 命令行入口。
    Cli,
    /// HTTP/Web 入口。
    Web,
    /// 本地 workbench 入口。
    Workbench,
    /// 桌面壳入口。
    Desktop,
    /// Model Context Protocol 入口。
    Mcp,
}

/// 某一个入口对同一次自动化命令/快照测试的观测声明。
///
/// 所有计数和布尔值由 harness 提供；校验器不会亲自调用 Broker 或 handler 来核对计数是否真实。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSurfaceUatCase {
    /// 必须与本模块 schema 常量一致。
    pub schema: String,
    /// 该条观测来自哪个产品入口；完整矩阵最终要求五种值各出现一次。
    pub surface: AutomationUatSurface,
    /// 各入口实际执行的命令内容指纹；外层要求五行完全相同。
    pub command_digest: String,
    /// 各入口查看的自动化状态快照指纹；外层要求五行完全相同。
    pub snapshot_digest: String,
    /// 观测对应的源事件游标；每行必须非零，外层要求版本一致。
    pub source_cursor: u64,
    /// 观测时引用的授权 epoch；每行必须非零，外层要求版本一致。
    pub authority_epoch: u64,
    /// 此入口的请求最终是否被拒绝；拒绝请求必须没有调用 Broker 或 handler。
    pub denied: bool,
    /// harness 声称本入口请求触达 Broker 的次数；拒绝时必须为零。
    pub broker_calls: u32,
    /// harness 声称具体操作 handler 被调用的次数；拒绝时必须为零。
    pub handler_calls: u32,
    /// harness 声称是否绕过 DaemonHost/ControlPlane 直达执行层；必须为 false。
    pub direct_route: bool,
    /// 对本行除 digest 外所有字段的 SHA-256 canonical digest。
    pub digest: String,
}

impl AutomationSurfaceUatCase {
    /// 检查单个入口观测的 schema、版本、deny/no-direct-route 声明和自摘要。
    ///
    /// 【调用者/输入】由外层 `AutomationSurfaceUat::validate()` 遍历；参数是 UAT harness 收集并组装的
    /// 单行数据。这里无法读取 harness 原始 trace。
    ///
    /// 【核心流程】schema 匹配；source cursor 与 authority epoch 非零；禁止 direct_route；若 denied=true，
    /// broker_calls 和 handler_calls 都必须为零；command/snapshot/row digest 格式有效；row digest 与本行
    /// canonical digest 完全相同。
    ///
    /// 【输出/失败】返回 `Ok(())` 或 route/deny、各 digest 的稳定 `aut23_*` 错误字符串。无 I/O、重试、
    /// 日志或副作用。
    ///
    /// ⚠ 当前 validator 对 denied=false 的成功行没有要求 broker_calls 或 handler_calls 至少为 1；因此
    /// 全零成功行也可通过本行规则。是否应由 UAT 契约要求成功路径被真实触达，需由验收设计确认。
    fn validate(&self) -> Result<(), String> {
        if self.schema != AUT23_SURFACE_UAT_SCHEMA
            || self.source_cursor == 0
            || self.authority_epoch == 0
            || self.direct_route
            || (self.denied && (self.broker_calls != 0 || self.handler_calls != 0))
        {
            return Err("aut23_surface_route_or_deny_invalid".to_owned());
        }
        digest(&self.command_digest, "aut23_command_digest_invalid")?;
        digest(&self.snapshot_digest, "aut23_snapshot_digest_invalid")?;
        digest(&self.digest, "aut23_case_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut23_case_digest_mismatch".to_owned());
        }
        Ok(())
    }
    /// 对入口、共同命令/快照和观测计数/路由/拒绝声明计算 canonical digest，不含自身。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "surface": self.surface,
            "command_digest": self.command_digest,
            "snapshot_digest": self.snapshot_digest,
            "source_cursor": self.source_cursor,
            "authority_epoch": self.authority_epoch,
            "denied": self.denied,
            "broker_calls": self.broker_calls,
            "handler_calls": self.handler_calls,
            "direct_route": self.direct_route,
        }))
    }
}

/// 汇总五个产品入口针对同一命令与状态快照的 parity/UAT 观测矩阵。
///
/// 此结构不启动任何产品 surface；它检查的是外部收集后提供的矩阵是否在输入层一致。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSurfaceUat {
    /// 必须匹配 AUT-23 schema。
    pub schema: String,
    /// 所有 case 必须共同引用的命令内容摘要。
    pub command_digest: String,
    /// 所有 case 必须共同引用的状态快照摘要；只校验格式，不加载真实 snapshot。
    pub snapshot_digest: String,
    /// 所有 case 共用的事件源 cursor；validate 通过每行非零并比较相等来约束。
    pub source_cursor: u64,
    /// 所有 case 共用的 authority epoch；validate 通过每行非零并比较相等来约束。
    pub authority_epoch: u64,
    /// 每种 surface 一行的测试观测，完整校验要求长度恰为 5 且集合包含所有 surface。
    pub cases: Vec<AutomationSurfaceUatCase>,
    /// 对汇总矩阵字段和有序 cases 列表计算的 SHA-256 canonical digest。
    pub digest: String,
}

impl AutomationSurfaceUat {
    /// 用第一条 case 的 command/snapshot/version 字段初始化矩阵 header，并计算顶层 digest。
    ///
    /// 【输入】调用方提供任意长度的 case 列表；构造器本身不先验证每条 case 是否有效或互相一致。
    ///
    /// 【空列表处理】没有第一条 case 时将摘要字段设为空串、游标/epoch 设为 0，再生成一个结构上可计算
    /// 但必然不能通过 `validate()` 的对象。这样构造不会 panic；真正可用性由后续显式 validate 判定。
    ///
    /// 【副作用】纯内存数据整理和摘要计算，不运行 UAT、不调用产品入口。
    pub fn new(cases: Vec<AutomationSurfaceUatCase>) -> Self {
        let first = cases.first();
        let mut value = Self {
            schema: AUT23_SURFACE_UAT_SCHEMA.to_owned(),
            command_digest: first.map(|c| c.command_digest.clone()).unwrap_or_default(),
            snapshot_digest: first.map(|c| c.snapshot_digest.clone()).unwrap_or_default(),
            source_cursor: first.map(|c| c.source_cursor).unwrap_or(0),
            authority_epoch: first.map(|c| c.authority_epoch).unwrap_or(0),
            cases,
            digest: String::new(),
        };
        value.digest = value.canonical_digest();
        value
    }
    /// 校验五入口矩阵的覆盖完整性、共同输入版本、拒绝边界和两层 digest。
    ///
    /// 【调用者】Core `validate_automation_surface_uat()` facade 转发；当前没有生产 UAT consumer，通常由
    /// 测试或外部收集器在矩阵组装后显式调用。
    ///
    /// 【核心流程】要求 schema 正确且 case 数量精确为 5；校验命令摘要和快照摘要格式；逐个验证 case，
    /// 并要求 command digest、snapshot digest、source cursor、authority epoch 与 header 一致；通过 BTreeSet
    /// 拒绝重复 surface，最后与固定的 CLI/Web/Workbench/Desktop/MCP 五项集合比较；再核对矩阵摘要。
    ///
    /// 【失败/副作用】第一个失败处返回稳定 `aut23_*` 错误字符串；无日志、重试、EventStore、daemon 或
    /// broker 调用。`source_cursor` 和 `authority_epoch` 没有独立 header 非零条件，但所有五条 case 都
    /// 要非零且相等，因此有效矩阵间接保证 header 非零。
    ///
    /// 【解释限制】相同摘要/版本和 `direct_route=false` 都是提交进来的声明；校验不读取入口 trace、不
    /// 证明其实际经过 ControlPlane、不重新执行拒绝请求，也不要求非拒绝 case 真的触达 Broker/handler。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUT23_SURFACE_UAT_SCHEMA || self.cases.len() != 5 {
            return Err("aut23_uat_header_invalid".to_owned());
        }
        required(&self.command_digest, "aut23_command_required")?;
        digest(&self.command_digest, "aut23_command_digest_invalid")?;
        digest(&self.snapshot_digest, "aut23_snapshot_digest_invalid")?;
        let mut surfaces = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if case.command_digest != self.command_digest
                || case.snapshot_digest != self.snapshot_digest
                || case.source_cursor != self.source_cursor
                || case.authority_epoch != self.authority_epoch
                || !surfaces.insert(case.surface)
            {
                return Err("aut23_surface_snapshot_parity_invalid".to_owned());
            }
        }
        let expected = [
            AutomationUatSurface::Cli,
            AutomationUatSurface::Web,
            AutomationUatSurface::Workbench,
            AutomationUatSurface::Desktop,
            AutomationUatSurface::Mcp,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        if surfaces != expected {
            return Err("aut23_surface_set_incomplete".to_owned());
        }
        digest(&self.digest, "aut23_uat_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut23_uat_digest_mismatch".to_owned());
        }
        Ok(())
    }
    /// 对 header 和 cases（包括每条 case 自带 digest）按当前顺序计算汇总摘要，不含顶层 digest。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "command_digest": self.command_digest,
            "snapshot_digest": self.snapshot_digest,
            "source_cursor": self.source_cursor,
            "authority_epoch": self.authority_epoch,
            "cases": self.cases,
        }))
    }
}
