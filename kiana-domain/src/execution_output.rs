//! 执行输出预算与完整输出引用的领域契约。
//!
//! 本文件定义输出采集、预览与归档的上限，以及如何描述一份已保存的输出；不负责读取 pipe、写文件、
//! 检查当前用户权限或执行过期清理。Daemon shell handler 使用预算后将输出字节保存在
//! EventLog/模型正文之外，再用 `ExecutionOutputRef` 绑定请求、Run、调用、内容摘要、所有者范围和
//! 过期时刻。`ToolOutputSpill` 也复用该引用，并在外层绑定 Turn、Capability 和预览。
//!
//! ```text
//! child stdout/stderr -> daemon reader 按单流预算采集 -> 脱敏/预览
//!                                      -> 有界本地归档 + ExecutionOutputRef
//!                                                    -> Broker/Core 校验后的分页读取
//! ```
//!
//! Domain 验证只检查字段形状、数值边界和摘要一致性；实际资源执行、owner/策略核验、文件访问和
//! 过期拒绝仍由调用方负责。引用本身不是访问许可。

use crate::{json_digest, InvocationId, RequestId, RunId, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// 完整输出引用的稳定 wire/schema 名称。
pub const EXECUTION_OUTPUT_REF_SCHEMA: &str = "kiana.execution-output-ref.v1";
/// 当前引用结构版本；兼容性只要求 major 一致，字段可解码性仍受 serde 严格字段规则约束。
pub const EXECUTION_OUTPUT_REF_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 引用声明允许的绝对最大内容字节数；实际调用通常还受更小的持久化预算限制。
pub const MAX_EXECUTION_OUTPUT_REF_BYTES: u64 = 16 * 1024 * 1024;
/// 输出预算 DTO 使用的稳定 schema 名称。
pub const EXECUTION_OUTPUT_BUDGET_SCHEMA: &str = "kiana.execution-output-budget.v1";

/// 输出处理各阶段分别采用的限制值。
///
/// 这些字段不是一个总量：stdout 和 stderr reader 各自应用预算；预览限制返回给模型/界面的短文本，
/// persist 限制最终归档记录，observed/max_lines 决定 reader 何时停止继续读取。此 DTO 本身不会启动
/// reader、写文件或强制操作系统限额。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionOutputBudget {
    /// schema 标识；反序列化后必须匹配当前预算格式。
    pub schema: String,
    /// 每条 stdout/stderr 最多保留在内存中的原始字节数；达到它后 reader 仍可能继续 drain pipe。
    pub collect_max_bytes: usize,
    /// 返回给模型/界面的预览正文最大字节数；调用方追加的截断提示可能不计入此值。
    pub preview_max_bytes: usize,
    /// 最终归档记录的字节上限，通常包含 JSON 包装与引用元数据。
    pub persist_max_bytes: usize,
    /// 每条输出流允许观察的原始字节上限；上层 reader 到达后停止或请求停止进程。
    pub observed_max_bytes: usize,
    /// 每条输出流可接受的 LF 换行符数量上限；不是按 CR 或终端视觉折行计算。
    pub max_lines: usize,
}

impl Default for ExecutionOutputBudget {
    /// 返回当前 Daemon shell 路径采用的默认值；提供 DTO 默认值本身不保证每个调用者都执行它。
    ///
    /// 每条流收集 1 MiB、预览正文 8 KiB、持久化记录最多 4 MiB；单条流最多观察 32 MiB 或
    /// 1,000,000 个 LF。stdout/stderr 分别运行 reader，所以两流合计量可能接近各自上限之和。
    fn default() -> Self {
        Self {
            schema: EXECUTION_OUTPUT_BUDGET_SCHEMA.to_owned(),
            collect_max_bytes: 1024 * 1024,
            preview_max_bytes: 8 * 1024,
            persist_max_bytes: 4 * 1024 * 1024,
            observed_max_bytes: 32 * 1024 * 1024,
            max_lines: 1_000_000,
        }
    }
}

impl ExecutionOutputBudget {
    /// 校验预算 DTO 的 schema、正数关系与引用可承载范围。
    ///
    /// 【调用者】Daemon 在采集或存档前调用；调用者负责把返回的字符串错误映射到自己的协议错误。
    /// 【通过条件】采集、预览、持久化和行数均非零；持久化/观察上限不小于采集上限；采集与持久化
    /// 不超过 `MAX_EXECUTION_OUTPUT_REF_BYTES`。
    /// 【注意】此处不限制 preview/observed/max_lines 的绝对上界，也不检查实际 pipe、进程或文件大小。
    /// 因而“validate 成功”只说明这些字段满足本 DTO 的约束，不等于操作系统已执行预算。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXECUTION_OUTPUT_BUDGET_SCHEMA
            || self.collect_max_bytes == 0
            || self.preview_max_bytes == 0
            || self.persist_max_bytes < self.collect_max_bytes
            || self.observed_max_bytes < self.collect_max_bytes
            || self.max_lines == 0
            || self.collect_max_bytes > MAX_EXECUTION_OUTPUT_REF_BYTES as usize
            || self.persist_max_bytes > MAX_EXECUTION_OUTPUT_REF_BYTES as usize
        {
            return Err("execution_output_budget_invalid".to_owned());
        }
        Ok(())
    }
}

/// 检查 `sha256:<64位十六进制>` 的文本形状；不会重算摘要，也不证明数据来源可信。
fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 指向受限完整输出的元数据引用，不在事件/模型载荷中内嵌完整输出字节。
///
/// 【用途/数据流】Daemon 保存归档记录后返回此对象；后续读取请求以 `output_id` 找到记录，再由
/// Broker/Core 和 adapter 重新检查调用上下文、owner、数据策略与过期时间。`ToolOutputSpill` 也将
/// 它作为完整内容引用，并在外层再绑定 turn/capability/scope。
/// 【安全边界】引用不是 capability 或授权凭证；知道 ID、摘要或合法 `reference_digest` 都不能单独
/// 获得读取权限。这里的 SHA-256 只用于内容/字段一致性，不是签名或防伪认证。
/// 【可空字段】`run_id=None` 允许没有 Run 归属的执行输出；具体生产读取路径可进一步要求关联条件。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionOutputRef {
    /// 引用 schema 标识。
    pub schema: String,
    /// 引用格式版本；验证时要求与当前 major 兼容。
    pub version: SchemaVersion,
    /// 输出记录 ID，也用于定位归档文件；读取端仍要检查其与记录内容的绑定。
    pub output_id: RequestId,
    /// 可选 Run 身份；为 None 表示该输出没有绑定到某个 Run。
    pub run_id: Option<RunId>,
    /// Capability 调用身份；当前构造器要求其 UUID 与 `output_id` 的 UUID 相同。
    pub invocation_id: InvocationId,
    /// 归档内容本身的 SHA-256 文本摘要，不是权限凭证。
    pub content_digest: String,
    /// 完整输出内容的字节数；应由实际序列化内容计算，不能用预览长度替代。
    pub size_bytes: u64,
    /// 所有者/数据范围摘要；adapter 需对照当前请求重新计算或核验。
    pub scope_digest: String,
    /// Unix 毫秒绝对过期时刻；读端要比较当前时间，不能只依赖后台清理文件。
    pub expires_at_unix_ms: u64,
    /// 对本引用其余字段计算的自摘要，用于发现字段被意外改动。
    pub reference_digest: String,
}

impl ExecutionOutputRef {
    /// 构造、计算自摘要并验证输出引用。
    ///
    /// 【输入】输出/调用身份、可选 Run、内容摘要与大小、owner scope 摘要、Unix 毫秒过期时刻。
    /// 【输出】字段自洽的引用；内容字节并不由本构造器接收或保存。
    /// 【失败】ID 关系、schema/version、摘要形状、大小或过期字段不合法时返回字符串错误。
    /// 【副作用】只分配并修改当前内存对象，不写文件/EventLog，也不验证授权或归档确实存在。
    pub fn new(
        output_id: RequestId,
        run_id: Option<RunId>,
        invocation_id: InvocationId,
        content_digest: impl Into<String>,
        size_bytes: u64,
        scope_digest: impl Into<String>,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut reference = Self {
            schema: EXECUTION_OUTPUT_REF_SCHEMA.to_owned(),
            version: EXECUTION_OUTPUT_REF_VERSION,
            output_id,
            run_id,
            invocation_id,
            content_digest: content_digest.into(),
            size_bytes,
            scope_digest: scope_digest.into(),
            expires_at_unix_ms,
            reference_digest: String::new(),
        };
        reference.reference_digest = reference.digest();
        reference.validate()?;
        Ok(reference)
    }

    /// 检查引用的结构、身份绑定、大小/过期边界和摘要一致性。
    ///
    /// `output_id` 与 `invocation_id` 必须非 nil 且共享 UUID；可选 `run_id` 在此处没有额外校验。
    /// 三个内容/范围/引用摘要只检查 SHA-256 文本格式，随后重算并比较引用自摘要。此检查不读取归档，
    /// 不证明内容摘要确实对应文件内容，也不代替 adapter 的 owner、策略和授权检查。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXECUTION_OUTPUT_REF_SCHEMA
            || !self
                .version
                .is_compatible_with(&EXECUTION_OUTPUT_REF_VERSION)
            || self.output_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.invocation_id != InvocationId::from_uuid(self.output_id.as_uuid())
            || self.size_bytes > MAX_EXECUTION_OUTPUT_REF_BYTES
            || self.expires_at_unix_ms == 0
        {
            return Err("execution_output_ref_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.content_digest, "execution_output_content_digest"),
            (&self.scope_digest, "execution_output_scope_digest"),
            (&self.reference_digest, "execution_output_reference_digest"),
        ] {
            valid_digest(value, field)?;
        }
        if self.reference_digest != self.digest() {
            return Err("execution_output_reference_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对引用中除 `reference_digest` 自身外的字段生成 canonical JSON SHA-256 摘要。
    ///
    /// `json_digest()` 会排序 JSON object key，因此摘要不依赖 map 的迭代顺序；这仍是完整性指纹，
    /// 不是有密钥的签名。新建时先写入此摘要，再调用 `validate()` 检查它与其余字段是否一致。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "output_id": self.output_id,
            "run_id": self.run_id,
            "invocation_id": self.invocation_id,
            "content_digest": self.content_digest,
            "size_bytes": self.size_bytes,
            "scope_digest": self.scope_digest,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}
