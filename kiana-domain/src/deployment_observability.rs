//! DEP-14 生命周期观测信息的只读 DTO 合同，属于 `kiana-domain`。
//!
//! 【预期位置与数据流】
//! ```text
//! 外部采集/投影代码（当前仓库未找到 bundle 生产者）
//!     -> LifecycleMetricFact 列表 + trace/log/audit 引用或摘要
//!     -> LifecycleEvidenceBundle::new()/validate()
//!     -> DaemonHost::validate_observability()
//!     -> Core facade -> 本文件检查字段关系和摘要
//! ```
//!
//! 本文件不采集/发射 metric，不写 structured log 或 audit event，不导出 trace，不读 EventStore，也不证明外部 effect。
//! `secret_free` 是 bundle 中由调用方声明的布尔值；本代码不会扫描所有字段或对内容做脱敏，只会拒绝 `trace_ref` 中
//! 少数指定字符串标记。operation/revision/cursor 的“绑定”也只表示 DTO 字段相等，没有回查真实 operation 或 source object。
//!
//! Serde 用于这些带版本值与 JSON 等外部格式互转；`deny_unknown_fields` 拒绝当前 schema 没声明的字段。

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// `LifecycleEvidenceBundle` 的 wire schema 名称。
pub const DEPLOYMENT_OBSERVABILITY_SCHEMA: &str = "kiana.deployment-observability.v1";
/// 观测 bundle 当前使用的 schema 版本。
pub const DEPLOYMENT_OBSERVABILITY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 单个 bundle 最多接受的 metric fact 数量；代码只实施结构上限，不依赖它做采集或容量调度。
const MAX_OBSERVABILITY_ITEMS: usize = 64;

/// 一条由调用方提供的 operation lifecycle 指标事实。
///
/// 它把指标名、整数值、单位文本和 operation/revision/cursor 引用放在一起，便于与 bundle 做字段一致性比较；
/// 不连接 metrics registry，也不验证这条指标确实被采集或其单位/数值符合业务语义。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleMetricFact {
    /// 指标名称；必须以 `kiana.` 开头，最多 128 UTF-8 字节。
    pub name: String,
    /// 指标数值，类型范围为 u64；本合同不为具体指标规定取值范围。
    pub value: u64,
    /// 数值单位文本，最多 32 UTF-8 字节；不会验证其与指标名/数值含义匹配。
    pub unit: String,
    /// 指标声称对应的 operation 引用；只要求非空且最多 256 字节，不查询 operation 存储。
    pub operation_ref: String,
    /// 指标声称对应的 revision 摘要；只检查 SHA-256 文本格式。
    pub revision_digest: String,
    /// 指标声称读取的 source cursor；只要求非零，bundle 会检查它与外层 cursor 相等。
    pub source_cursor: u64,
    /// 除自身外指标字段的 SHA-256 内容摘要，不是指标源的签名。
    pub metric_digest: String,
}

impl LifecycleMetricFact {
    /// 从调用方给定字段创建 metric fact、计算自摘要并校验。
    ///
    /// 【调用者】预期由外部 metrics projector/adapter 组装；当前生产源码没有该构造器调用者，测试直接创建合成值。
    /// 【输出/失败】成功返回内部字段自洽的 fact；名称/单位/operation ref/cursor 或摘要格式不合约时返回稳定 `String` 错误。
    /// 【副作用/限制】只在内存中转换字符串和计算摘要；不会读取计量器、EventStore、operation 或 revision 对象。
    pub fn new(
        name: impl Into<String>,
        value: u64,
        unit: impl Into<String>,
        operation_ref: impl Into<String>,
        revision_digest: impl Into<String>,
        source_cursor: u64,
    ) -> Result<Self, String> {
        let mut fact = Self {
            name: name.into(),
            value,
            unit: unit.into(),
            operation_ref: operation_ref.into(),
            revision_digest: revision_digest.into(),
            source_cursor,
            metric_digest: String::new(),
        };
        fact.metric_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    /// 检查 metric 字段的文本边界、名称前缀、正 source cursor 和摘要自洽性。
    ///
    /// 通过仅证明 DTO 满足当前结构规则；value 没有业务上下界，unit 不做语义校验，摘要不证明事实来源。
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty()
            || self.name.len() > 128
            || !self.name.starts_with("kiana.")
            || self.unit.trim().is_empty()
            || self.unit.len() > 32
            || self.operation_ref.trim().is_empty()
            || self.operation_ref.len() > 256
            || self.source_cursor == 0
        {
            return Err("deployment_metric_fact_header_invalid".to_owned());
        }
        valid_digest(&self.revision_digest, "deployment_metric_revision_digest")?;
        valid_digest(&self.metric_digest, "deployment_metric_digest")?;
        if self.metric_digest != self.digest() {
            return Err("deployment_metric_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对名称、值、单位和三项来源引用计算内容摘要，不包含 `metric_digest` 本身。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "name": self.name,
            "value": self.value,
            "unit": self.unit,
            "operation_ref": self.operation_ref,
            "revision_digest": self.revision_digest,
            "source_cursor": self.source_cursor,
        }))
    }
}

/// 绑定一组 lifecycle metric 与 opaque trace/log/audit 元数据的只读 bundle。
///
/// 这里的 operation/revision/source cursor 绑定，是 bundle 内各 DTO 字段之间的相等检查；trace、日志、审计内容不会被读取。
/// 空 metrics 列表合法，因此 bundle 通过校验也不代表已采集任何指标。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleEvidenceBundle {
    /// 固定 wire schema 名称。
    pub schema: String,
    /// wire schema 版本。
    pub version: SchemaVersion,
    /// bundle 声称对应的 operation 引用；本地不查询 operation 或提交记录。
    pub operation_ref: String,
    /// bundle 声称对应的 revision 摘要；只查格式，并与每条 metric 的引用比较。
    pub revision_digest: String,
    /// bundle 的来源 cursor；只要求非零，并要求每条 metric 使用相同数值，不证明该 cursor 已持久化。
    pub source_cursor: u64,
    /// trace 的 opaque 引用文本；不要求特定 scheme，也不查询 trace store。
    pub trace_ref: String,
    /// structured log 内容的摘要引用；只检查格式，不读取日志或验证其 operation 归属。
    pub structured_log_digest: String,
    /// audit event 内容的摘要引用；只检查格式，不读取 EventLog/audit store。
    pub audit_event_digest: String,
    /// 附属指标集合；最多 64 项，允许为空，名字按字符串精确相等去重。
    pub metrics: Vec<LifecycleMetricFact>,
    /// 调用方声称 bundle 不含 secret；构造器固定设为 true，validate 仅检查该位，不能据此断言整体已脱敏。
    pub secret_free: bool,
    /// 除自身摘要外其余 bundle 字段的 SHA-256 内容摘要，不是来源签名。
    pub bundle_digest: String,
}

impl LifecycleEvidenceBundle {
    /// 把调用方给定的引用、摘要和 metrics 装入 bundle 后计算摘要并校验。
    ///
    /// 【调用者】预期由外部生命周期观测 projector/adapter 创建；当前生产源码没有该生产调用者，Domain fixture 会直接调用。
    /// 【输入】operation/revision/cursor、trace 引用、log/audit digest 和 metric 列表，均由调用方负责收集与脱敏。
    /// 【输出/失败】构造器把 `secret_free` 写为 true、计算 bundle digest 并运行 validate；不符合 DTO 规则时返回稳定 `String`。
    /// 【重要边界】构造器不扫描输入是否真的无 secret；log/audit 只有 digest，trace_ref 也只接受有限 marker 检查。
    /// 该字段声明不是脱敏器或数据来源认证。
    pub fn new(
        operation_ref: impl Into<String>,
        revision_digest: impl Into<String>,
        source_cursor: u64,
        trace_ref: impl Into<String>,
        structured_log_digest: impl Into<String>,
        audit_event_digest: impl Into<String>,
        metrics: Vec<LifecycleMetricFact>,
    ) -> Result<Self, String> {
        let mut bundle = Self {
            schema: DEPLOYMENT_OBSERVABILITY_SCHEMA.to_owned(),
            version: DEPLOYMENT_OBSERVABILITY_VERSION,
            operation_ref: operation_ref.into(),
            revision_digest: revision_digest.into(),
            source_cursor,
            trace_ref: trace_ref.into(),
            structured_log_digest: structured_log_digest.into(),
            audit_event_digest: audit_event_digest.into(),
            metrics,
            secret_free: true,
            bundle_digest: String::new(),
        };
        bundle.bundle_digest = bundle.digest();
        bundle.validate()?;
        Ok(bundle)
    }

    /// 按固定顺序验证 header/大小、摘要外形、trace_ref 限定、metric 自校验及跨字段绑定，最后重算 bundle digest。
    ///
    /// 【调用者】`new()` 返回前以及 Core/Daemon 的只读验证路由都会调用。
    /// 【失败】返回首个稳定 `String` 错误；其中 secret marker 检查只覆盖 trace_ref 的 NUL/CR/LF、`secret` 和 `token=`，
    /// 不扫描 metric name/unit/operation_ref，也无法扫描 log/audit 的原文。
    /// 【副作用】只读遍历并做摘要计算；不会脱敏、写 audit、发 metric、导 trace 或查询引用对象。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_OBSERVABILITY_SCHEMA
            || self.version != DEPLOYMENT_OBSERVABILITY_VERSION
            || !self.secret_free
            || self.operation_ref.trim().is_empty()
            || self.operation_ref.len() > 256
            || self.source_cursor == 0
            || self.trace_ref.trim().is_empty()
            || self.trace_ref.len() > 256
            || self.metrics.len() > MAX_OBSERVABILITY_ITEMS
        {
            return Err("deployment_observability_header_invalid".to_owned());
        }
        valid_digest(
            &self.revision_digest,
            "deployment_observability_revision_digest",
        )?;
        valid_digest(
            &self.structured_log_digest,
            "deployment_observability_log_digest",
        )?;
        valid_digest(
            &self.audit_event_digest,
            "deployment_observability_audit_digest",
        )?;
        valid_digest(
            &self.bundle_digest,
            "deployment_observability_bundle_digest",
        )?;
        if self.trace_ref.contains(['\0', '\r', '\n'])
            || self.trace_ref.to_ascii_lowercase().contains("secret")
            || self.trace_ref.to_ascii_lowercase().contains("token=")
        {
            return Err("deployment_observability_secret_marker".to_owned());
        }
        // 每条 metric 必须与外层 bundle 的三个 scope 字段完全一致；名称集合按大小写敏感的原始字符串去重。
        let mut names = BTreeSet::new();
        for metric in &self.metrics {
            metric.validate()?;
            if metric.operation_ref != self.operation_ref
                || metric.revision_digest != self.revision_digest
                || metric.source_cursor != self.source_cursor
                || !names.insert(metric.name.as_str())
            {
                return Err("deployment_observability_metric_binding_invalid".to_owned());
            }
        }
        if self.bundle_digest != self.digest() {
            return Err("deployment_observability_bundle_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 bundle header、引用/摘要、metrics 和 `secret_free` 声明计算 SHA-256 内容摘要。
    ///
    /// digest 可以发现未同步更新摘要的字段改动；调用方可改值后重新计算，因此不证明发送者身份、对象存在或数据已持久化。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "operation_ref": self.operation_ref,
            "revision_digest": self.revision_digest,
            "source_cursor": self.source_cursor,
            "trace_ref": self.trace_ref,
            "structured_log_digest": self.structured_log_digest,
            "audit_event_digest": self.audit_event_digest,
            "metrics": self.metrics,
            "secret_free": self.secret_free,
        }))
    }
}

/// 检查 `sha256:` 加 64 个十六进制字符的外形；不会加载被摘要引用的 trace/log/audit/revision 对象。
fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
