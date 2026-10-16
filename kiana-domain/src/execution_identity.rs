//! Core 与 Runner 共用的 Run/Turn/Step/Invocation/ModelAttempt 身份契约。
//!
//! 这些 DTO 把“某个动作发生在什么运行位置、属于哪次调用、是第几次尝试”固化成可写入事件和恢复数据
//! 的字段。它们只构造/验证身份描述，不分配 Run 权限、不执行模型或工具，也不自行写 EventStore。
//!
//! ```text
//! Core 收到 start/new-turn/resume/legacy-continue
//!   -> TurnIdentity -> request/turn 事件 -> Core dispatch 校验 Turn 与 Run 的绑定
//!   -> InvocationIdentity（服务端 invocation/execution ID + 可选请求 call_id）-> capability 事实
//! Runner 每次模型循环
//!   -> StepIdentity -> ModelAttemptIdentity（每一次 provider/model 尝试）-> ModelTurn 事件 metadata
//! ```
//!
//! `call_id` 来自模型工具调用或请求中的关联信息，只用于把请求与回执串起来，不是服务端主键；稳定身份
//! 由服务端生成的 `InvocationId`、`ExecutionId` 等承担。各 `identity_digest` 是可重算的 JSON 摘要，
//! 不是带密钥的签名，也不替代调用方对字段来源和事件关系的核验。

use crate::{
    canonical_journal_bytes, json_digest, ExecutionId, InvocationId, ModelAttemptId, RequestId,
    RunId, SchemaVersion, SessionId, StepId, TurnId,
};
use serde::{Deserialize, Serialize};

/// TurnIdentity 事件载荷使用的稳定 schema 名称。
pub const TURN_IDENTITY_SCHEMA: &str = "kiana.turn-identity.v1";
/// InvocationIdentity 事件载荷使用的稳定 schema 名称。
pub const INVOCATION_IDENTITY_SCHEMA: &str = "kiana.invocation-identity.v1";
/// StepIdentity 事件 metadata 使用的稳定 schema 名称。
pub const STEP_IDENTITY_SCHEMA: &str = "kiana.step-identity.v1";
/// ModelAttemptIdentity 事件 metadata 使用的稳定 schema 名称。
pub const MODEL_ATTEMPT_IDENTITY_SCHEMA: &str = "kiana.model-attempt-identity.v1";
/// 本文件四类身份 DTO 共用的 major/minor 版本。
pub const EXECUTION_IDENTITY_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// 当前 Turn 与前序 Run 的业务关系；这是事件数据，不单独授予继续执行权限。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnSemantics {
    /// 创建一个没有 predecessor 的初始 Run。
    Start,
    /// 从一个已结束的旧 Run 创建新 Run，并要求 predecessor 存在且不同于当前 Run。
    NewTurn,
    /// 兼容旧式 `run.continue`：继续既有 Run，不按 NewTurn 的 predecessor 规则校验。
    LegacyContinue,
    /// 恢复一个 Run；当前 DTO 要求它没有 predecessor，恢复来源由 Core 事件链表达。
    Resume,
}

/// 校验非空、最大 UTF-8 字节数受限的关联字符串。
///
/// trim 只用于检查是否全空，不会改写保留值；本 helper 不拒绝 NUL，也不验证字符串的业务身份。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 检查 `sha256:<64位十六进制>` 的格式，不重算被引用对象的摘要。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnIdentity {
    /// 当前身份 DTO 的 schema 名称。
    pub schema: String,
    /// schema major/minor 版本。
    pub version: SchemaVersion,
    /// 向用户/入口展示的 Session 标识；不是 Run/Turn 的替代主键。
    pub session_id: SessionId,
    /// 该轮所处的运行身份。
    pub run_id: RunId,
    /// 本轮对话身份；当前 Core start 路径从 request ID 派生该 UUID。
    pub turn_id: TurnId,
    /// 创建新 Run 时指向被替代/续接的旧 Run；其他语义可缺省。
    /// serde(default) 使旧载荷没有该字段时解码为 None。
    #[serde(default)]
    pub previous_run_id: Option<RunId>,
    /// 本 Run 的创建/继续/恢复语义，供 Core 和恢复读端按场景检查。
    pub semantics: TurnSemantics,
    /// 同一 Run 内的逻辑轮次，从 1 开始；不是 EventStore stream version。
    pub turn_number: u64,
    /// 除自身外其余身份字段的 SHA-256 JSON 摘要。
    pub identity_digest: String,
}

impl TurnIdentity {
    /// 构造一个 Turn 关联对象并立即校验 predecessor 关系与自摘要。
    ///
    /// 【调用者】Core 的新 Run、显式 NewTurn、旧式 continue 和 recovery resume 路径。
    /// 【输入】Session 字符串、当前 Run/Turn IDs、可选前驱 Run、语义枚举、正数 turn number。
    /// 【输出】验证通过的身份 DTO，随后通常嵌入 Core 的 request/run 事件供 dispatch/recovery 读取。
    /// 【失败】Session 为空、turn_number 为 0、语义与 predecessor 不一致、版本/schema 或摘要无效时
    /// 返回字符串错误；本方法不写事件、不校验 caller 是否有权使用这些 ID。
    pub fn new(
        session_id: impl Into<String>,
        run_id: RunId,
        turn_id: TurnId,
        previous_run_id: Option<RunId>,
        semantics: TurnSemantics,
        turn_number: u64,
    ) -> Result<Self, String> {
        let mut identity = Self {
            schema: TURN_IDENTITY_SCHEMA.to_owned(),
            version: EXECUTION_IDENTITY_SCHEMA_VERSION,
            session_id: SessionId::new(session_id),
            run_id,
            turn_id,
            previous_run_id,
            semantics,
            turn_number,
            identity_digest: String::new(),
        };
        identity.identity_digest = identity.digest();
        identity.validate()?;
        Ok(identity)
    }

    /// 验证 schema major、必填 Session/轮次、TurnSemantics 前驱约束和身份自摘要。
    ///
    /// NewTurn 必须带一个不同于当前 Run 的前驱；Start 和 Resume 不允许前驱；LegacyContinue 不额外
    /// 约束 `previous_run_id`。此方法没有检查 RunId/TurnId 底层 UUID 是否 nil，也不查询真实事件链，
    /// 因而只能证明 DTO 内部关系一致，跨事件关系由 Core dispatch/recovery 的调用点检查。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TURN_IDENTITY_SCHEMA
            || !self
                .version
                .is_compatible_with(&EXECUTION_IDENTITY_SCHEMA_VERSION)
            || self.session_id.is_empty()
            || self.turn_number == 0
        {
            return Err("turn_identity_header_invalid".to_owned());
        }
        if matches!(self.semantics, TurnSemantics::NewTurn)
            && self
                .previous_run_id
                .is_none_or(|previous| previous == self.run_id)
        {
            return Err("turn_identity_predecessor_required".to_owned());
        }
        if matches!(self.semantics, TurnSemantics::Start | TurnSemantics::Resume)
            && self.previous_run_id.is_some()
        {
            return Err("turn_identity_predecessor_unexpected".to_owned());
        }
        digest(&self.identity_digest, "turn_identity_digest")?;
        if self.identity_digest != self.digest() {
            return Err("turn_identity_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 返回稳定的 canonical JSON 字节表示，供需要确定性持久化/摘要的调用方使用。
    ///
    /// 对象键会排序、数组顺序保留；序列化失败或嵌套超过 journal 限制时返回字符串错误。本方法不写入
    /// EventStore；在当前生产构造链检索中未发现对此 identity helper 的直接调用。
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    /// 对身份字段计算摘要时先把 `identity_digest` 自身置空，避免递归依赖。
    ///
    /// 它只产生确定性指纹，不含密钥。若 `serde_json::to_value` 失败，当前实现回退到 JSON null 再摘要；
    /// 对这些简单 DTO 通常不会发生，但该方法本身没有把编码失败作为 Result 暴露。
    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "identity_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationIdentity {
    /// 当前 invocation 身份 schema。
    pub schema: String,
    /// schema major/minor 版本。
    pub version: SchemaVersion,
    /// 当前调用所属 Run，由 Core dispatch 路径提供。
    pub run_id: RunId,
    /// 当前调用所属 Turn，由已核验/上下文传入的 turn 身份提供。
    pub turn_id: TurnId,
    /// Core 为本次 capability invocation 生成的稳定身份。
    pub invocation_id: InvocationId,
    /// Core 为具体执行动作生成的身份，用于把准备、结果与恢复事实关联起来。
    pub execution_id: ExecutionId,
    /// 可选模型/客户端 call_id，只作关联提示；缺字段的旧载荷反序列化为 None。
    #[serde(default)]
    pub call_id: Option<String>,
    /// 此 invocation 的尝试序号；从 1 开始，0 在 validate 中拒绝。
    pub attempt: u32,
    /// 除自身外身份字段的 JSON SHA-256 摘要。
    pub identity_digest: String,
}

impl InvocationIdentity {
    /// 由 Core dispatch 绑定服务端 invocation/execution 身份和可选请求 call_id。
    ///
    /// 【输入】已确定的 Run/Turn、服务端生成的 InvocationId/ExecutionId、请求侧 call_id、attempt 序号。
    /// 【输出】通过 schema、attempt、call_id 形状和自摘要校验的 DTO；Core 随后序列化进 capability 事实。
    /// 【失败】attempt 为 0、call_id 空/超长、版本/schema 不符或摘要不一致时返回字符串错误。
    /// 【副作用】仅构造内存对象，不执行 capability、不写 EventStore；这里也不核验 IDs 是否属于事件流。
    pub fn new(
        run_id: RunId,
        turn_id: TurnId,
        invocation_id: InvocationId,
        execution_id: ExecutionId,
        call_id: Option<String>,
        attempt: u32,
    ) -> Result<Self, String> {
        let mut identity = Self {
            schema: INVOCATION_IDENTITY_SCHEMA.to_owned(),
            version: EXECUTION_IDENTITY_SCHEMA_VERSION,
            run_id,
            turn_id,
            invocation_id,
            execution_id,
            call_id,
            attempt,
            identity_digest: String::new(),
        };
        identity.identity_digest = identity.digest();
        identity.validate()?;
        Ok(identity)
    }

    /// 校验版本、非零 attempt、可选 call_id 和自摘要。
    ///
    /// `call_id` 只检查非空及最多 256 UTF-8 字节，不要求唯一；各强类型 UUID 是否 nil、Run/Turn 是否
    /// 与 EventStore 中的权威记录相符，需要 Core dispatch/projection 再核验。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INVOCATION_IDENTITY_SCHEMA
            || !self
                .version
                .is_compatible_with(&EXECUTION_IDENTITY_SCHEMA_VERSION)
            || self.attempt == 0
        {
            return Err("invocation_identity_header_invalid".to_owned());
        }
        if let Some(call_id) = &self.call_id {
            required(call_id, "invocation_call_id", 256)?;
        }
        digest(&self.identity_digest, "invocation_identity_digest")?;
        if self.identity_digest != self.digest() {
            return Err("invocation_identity_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 生成稳定的 journal JSON 字节；不负责持久化或登记 invocation。
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    /// 清空自引用字段后计算 InvocationIdentity 的 JSON digest。
    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "identity_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

/// 由 Runner 服务端维护的一个模型/工具驱动步骤身份。
///
/// 一个 Step 可以包含多次模型尝试或 capability invocation；不要把 step_number 当成尝试次数，
/// 后者由 `ModelAttemptIdentity::attempt` 单独表达。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepIdentity {
    /// Step identity schema。
    pub schema: String,
    /// schema major/minor 版本。
    pub version: SchemaVersion,
    /// 该步骤所属 Run。
    pub run_id: RunId,
    /// 该步骤所属 Turn。
    pub turn_id: TurnId,
    /// 服务端分配的本步骤稳定 ID。
    pub step_id: StepId,
    /// 当前 Run 中的逻辑步骤序号，从 1 开始。
    pub step_number: u32,
    /// 除自身外身份字段的 JSON SHA-256 摘要。
    pub identity_digest: String,
}

impl StepIdentity {
    /// 为 Runner 当前模型/工具驱动步骤创建身份并计算自摘要。
    ///
    /// 【调用者】Runner 在每轮模型请求之前创建；该 DTO 随 `ModelTurn` 事件 metadata 保存，便于把后续
    /// provider attempt 和工具动作归到同一个 step。
    /// 【输入】当前 Run/Turn、服务端生成的 StepId、非零逻辑步号。
    /// 【输出/失败】返回验证通过的 DTO，schema 或 step_number=0 / digest 错误时返回领域字符串错误。
    /// 【副作用】仅构造内存值，不驱动模型、不递增 `run.steps`，也不写事件。
    pub fn new(
        run_id: RunId,
        turn_id: TurnId,
        step_id: StepId,
        step_number: u32,
    ) -> Result<Self, String> {
        let mut identity = Self {
            schema: STEP_IDENTITY_SCHEMA.to_owned(),
            version: EXECUTION_IDENTITY_SCHEMA_VERSION,
            run_id,
            turn_id,
            step_id,
            step_number,
            identity_digest: String::new(),
        };
        identity.identity_digest = identity.digest();
        identity.validate()?;
        Ok(identity)
    }

    /// 校验 schema major、非零 step_number 与身份摘要；不验证 Run/Turn/Step UUID 的非 nil 性或对应关系。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STEP_IDENTITY_SCHEMA
            || !self
                .version
                .is_compatible_with(&EXECUTION_IDENTITY_SCHEMA_VERSION)
            || self.step_number == 0
        {
            return Err("step_identity_header_invalid".to_owned());
        }
        digest(&self.identity_digest, "step_identity_digest")?;
        if self.identity_digest != self.digest() {
            return Err("step_identity_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 返回字段键排序后的确定性 journal JSON 编码；不会执行存储操作。
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    /// 将自摘要字段置空后计算 StepIdentity JSON digest。
    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "identity_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

/// 一个 provider/model 请求尝试，并绑定到 Runner 服务端维护的 Step。
///
/// 同一逻辑 `model_call_id` 可能因安全重试出现多条 attempt；`model_attempt_id` 和 `attempt` 用来区分
/// 每一次实际尝试。它只描述身份，不代表请求已发到 provider 或已有结果。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAttemptIdentity {
    /// 模型尝试身份 schema。
    pub schema: String,
    /// schema major/minor 版本。
    pub version: SchemaVersion,
    /// 尝试所属 Run。
    pub run_id: RunId,
    /// 尝试所属 Turn。
    pub turn_id: TurnId,
    /// 承载该模型请求的逻辑步骤。
    pub step_id: StepId,
    /// 服务端为这一次 provider/model attempt 分配的身份。
    pub model_attempt_id: ModelAttemptId,
    /// 同一逻辑模型调用的关联 ID；重试共享此 ID，但每次重试会生成新的 model_attempt_id。
    pub model_call_id: RequestId,
    /// 本逻辑模型调用中的一基尝试序号；Runner 把 0-based 循环 index 加 1 后传入。
    pub attempt: u32,
    /// 除自身外身份字段的 JSON SHA-256 摘要。
    pub identity_digest: String,
}

impl ModelAttemptIdentity {
    /// 为一次即将执行的 provider/model 尝试构造稳定身份。
    ///
    /// 【调用者】Runner 的 `invoke_model()`：每进入一轮重试都会新建 `model_attempt_id`，并沿用同一
    /// 逻辑 `model_call_id`；随后把结构写入 `RunnerEvent::ModelTurn` metadata。
    /// 【输入/输出】Run/Turn/Step、attempt ID、逻辑 call ID 和非零一基序号，成功返回自摘要有效的 DTO。
    /// 【失败/副作用】序号为 0、schema/version 或自摘要异常时返回字符串错误；不发送 provider 请求、
    /// 不写 EventStore。它的存在不证明该次调用已开始或成功。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        run_id: RunId,
        turn_id: TurnId,
        step_id: StepId,
        model_attempt_id: ModelAttemptId,
        model_call_id: RequestId,
        attempt: u32,
    ) -> Result<Self, String> {
        let mut identity = Self {
            schema: MODEL_ATTEMPT_IDENTITY_SCHEMA.to_owned(),
            version: EXECUTION_IDENTITY_SCHEMA_VERSION,
            run_id,
            turn_id,
            step_id,
            model_attempt_id,
            model_call_id,
            attempt,
            identity_digest: String::new(),
        };
        identity.identity_digest = identity.digest();
        identity.validate()?;
        Ok(identity)
    }

    /// 检查 schema major、非零尝试序号和自摘要；不检查 IDs 非 nil，也不跨字段查询 Run/Turn/Step 关系。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MODEL_ATTEMPT_IDENTITY_SCHEMA
            || !self
                .version
                .is_compatible_with(&EXECUTION_IDENTITY_SCHEMA_VERSION)
            || self.attempt == 0
        {
            return Err("model_attempt_identity_header_invalid".to_owned());
        }
        digest(&self.identity_digest, "model_attempt_identity_digest")?;
        if self.identity_digest != self.digest() {
            return Err("model_attempt_identity_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 生成 deterministic journal JSON 字节；不执行 provider 调用或日志写入。
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    /// 将 `identity_digest` 置空后计算 ModelAttemptIdentity 的 JSON 指纹。
    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "identity_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}
