//! 本文件定义 AUT-18 的暂停/恢复 signal 与 checkpoint 事实格式，并校验单条 fact 的字段自洽性。
//!
//! # 在系统中的位置
//!
//! ```text
//! 当前可见调用边界：
//! 调用方准备 AutomationSignalFact
//!     -> 本文件校验 schema / 文本 / 摘要格式 / consumed 与 state 一致性
//!     -> Core 的 automation_signal facade 原样转调
//!     -> 返回 Ok 或稳定错误
//! ```
//!
//! 当前 workspace 的生产 Rust 源码中没有找到这个 validator 或 Core facade 的实际调用者；仓库
//! 状态账本也把 AUT-18 限定为 source-level、partial 合约。因此本文件不是现行 worker pause/resume
//! 执行链：它不验证审批人、不查 EventStore、不防跨记录重复 signal、不暂停/恢复线程，也不创建
//! checkpoint。字段可通过校验不等于事实已被授权、持久化或消费。
//!
//! 摘要使用无密钥 SHA-256，只能检查字段在同一结构中的摘要是否同步；提供者可以自行重算，不能
//! 由此认证 owner、action、path、authority epoch 或 checkpoint 的真实性。

use crate::json_digest;
use serde::{Deserialize, Serialize};

/// 此类 signal fact 的固定 wire schema 名称。
pub const AUTOMATION_SIGNAL_SCHEMA: &str = "kiana.automation-signal.v1";

/// 一条 signal fact 声称的状态；枚举只限制可用值，不校验跨记录状态转移。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationSignalState {
    /// execution 暂停的事实声明。
    Paused,
    /// execution 恢复的事实声明。
    Resumed,
    /// signal 已消费的事实声明。
    Consumed,
}

/// 一个 pause/resume/checkpoint signal 的单条绑定事实。
///
/// Serde 的 `deny_unknown_fields` 会拒绝未声明 JSON 字段，避免不同版本对同一载荷作不同解释。
/// `validate()` 只检查本结构本身；它不读取旧 fact，因此不能保证 signal_id 从未出现过，也不能
/// 验证 checkpoint_sequence 连续、当前 epoch 有效、owner 对应真实 approver 或 action/path 摘要
/// 对应实际请求。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSignalFact {
    /// 必须精确等于 `AUTOMATION_SIGNAL_SCHEMA`。
    pub schema: String,
    /// 关联的执行 ID；这里只检查文本格式，不在运行 registry 中查询该 execution。
    pub execution_id: String,
    /// 这条 signal 的标识；没有历史索引时，单条校验无法判定它是否已用过。
    pub signal_id: String,
    /// 声称拥有此 signal 的 actor ID；只检查文本，不确认实际身份或批准关系。
    pub owner_id: String,
    /// 受控 action 内容的 SHA-256 格式摘要；此处不接收 action 本体来重新计算。
    pub action_digest: String,
    /// 允许写入的路径范围摘要；这里只检查摘要格式，不检查实际文件路径。
    pub path_scope_digest: String,
    /// 创建事实时声称的 authority epoch；只要求非零，不与当前 ControlPlane epoch 比较。
    pub authority_epoch: u64,
    /// checkpoint 序号；只要求非零，不读取 checkpoint store 或检查序号连续性。
    pub checkpoint_sequence: u64,
    /// 这条 fact 声称的状态；本 validator 不限制 Paused/Resumed/Consumed 的前后顺序。
    pub state: AutomationSignalState,
    /// 冗余的一致性标记；必须且只在 state 为 Consumed 时为 true，但不维护 signal 的全局消费表。
    pub consumed: bool,
    /// 对本结构其余字段的 `sha256:` 摘要；不能认证创建者，也不防止提供者修改后重算。
    pub signal_digest: String,
}

impl AutomationSignalFact {
    /// 校验一条 signal fact 的 schema、标识格式、非零计数、消费标记和自摘要。
    ///
    /// 成功只表示单条 DTO 自洽。字段文本最长 512 UTF-8 字节且不允许任何空白字符；digest 必须是
    /// `sha256:` 加 64 个十六进制字符。`authority_epoch`/`checkpoint_sequence` 的非零检查没有
    /// 连接当前 authority 或持久 checkpoint 记录。任何条件失败统一返回
    /// `automation_signal_fact_invalid`；函数不写状态、日志或 EventStore，也不会触发 pause/resume。
    ///
    /// `consumed == (state == Consumed)` 只保证同一 fact 的两个字段一致，不能证明同一 signal_id
    /// 没有在另一条事实中重复消费；每种状态值作为单条 fact 都可以通过校验。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_SIGNAL_SCHEMA
            || !valid_text(&self.execution_id)
            || !valid_text(&self.signal_id)
            || !valid_text(&self.owner_id)
            || !valid_digest(&self.action_digest)
            || !valid_digest(&self.path_scope_digest)
            || self.authority_epoch == 0
            || self.checkpoint_sequence == 0
            || self.consumed != matches!(self.state, AutomationSignalState::Consumed)
            || !valid_digest(&self.signal_digest)
            || self.signal_digest != self.digest()
        {
            return Err("automation_signal_fact_invalid");
        }
        Ok(())
    }

    /// 计算除 `signal_digest` 外全部字段的 JSON 摘要，避免把摘要字段自身纳入输入形成自引用。
    /// 摘要没有密钥，因此不是 approver 身份证明或授权签名。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "signal_id": self.signal_id,
            "owner_id": self.owner_id,
            "action_digest": self.action_digest,
            "path_scope_digest": self.path_scope_digest,
            "authority_epoch": self.authority_epoch,
            "checkpoint_sequence": self.checkpoint_sequence,
            "state": self.state,
            "consumed": self.consumed,
        }))
    }
}

/// 对外提供的领域校验函数；目前只转调 `AutomationSignalFact::validate()`。
///
/// 由 `kiana-domain` crate 导出，Core facade 也会转调它；当前生产代码搜索未发现实际使用者，
/// 可见使用集中在测试。错误原样返回稳定静态字符串，不产生任何执行或持久化副作用。
pub fn validate_automation_signal(fact: &AutomationSignalFact) -> Result<(), &'static str> {
    fact.validate()
}

/// 检查 ID 类字段是否为非空、无空白、无 NUL/换行且不超过 512 UTF-8 字节的文本。
fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 512
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

/// 检查摘要是否为 `sha256:` 前缀加 64 个大小写均可的十六进制字符；不验证摘要来源。
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
