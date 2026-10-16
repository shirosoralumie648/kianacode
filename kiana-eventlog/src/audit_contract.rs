//! EventStore boundary validation for server-derived audit facts.
//!
//! `AuditRecord` rows are projections of committed facts.  A caller can therefore not append an
//! arbitrary `audit.*` RuntimeEvent and make it look like a server decision.  The one accepted
//! audit kind carries a strict, digest-bound `AuditRecordEvent` envelope and repeats the metadata
//! that the EventStore must see before it mutates either memory or durable JSONL state.
//!
//! # 这个文件在系统里的位置
//!
//! Kiana 的事件日志（EventLog）是**不可改、不可删、只可追加** 的可信链。
//! 本文件是这个链的**审计合约检查员** —— 它在审计事件写入日志之前，验证这个事件是否真的是来自服务端，
//! 而不是某个客户端伪造的。
//!
//! 一个运行时事件要成为"审计事实"，必须经过一系列**强制性的检查**。这些检查保障：
//! 1. 事件类型必须是服务端认可的审计类型（不是任意的 `audit.*`）
//! 2. 事件的**格式和内容**必须符合 `AuditRecordEvent` 合约
//! 3. 事件中声称的**元数据**（过期纪元、脱敏配置）与包装信封必须**完全一致**
//! 4. 事件中**不允许**包含任何工件引用（artifact refs）—— 审计记录必须自成一体
//!
//! # 最重要的一条不变量：审计事件不能被伪造
//!
//! 文件头那句"A caller can therefore not append an arbitrary `audit.*` RuntimeEvent and make
//! it look like a server decision"是本文件的核心防线。
//!
//! **只有服务端可以产生真实的审计记录。** 这不是靠权限管理，而是靠严格的**格式验证和签名绑定**强制的：
//!
//! - `AuditRecordEvent` 包装着真实的 `AuditRecord`（哪条决策、什么时候、谁做的）
//! - 这个包装必须被 [`validate()`] 检查通过 —— 确保内部格式合法
//! - 包装中的字段（脱敏配置、数据纪元、幂等性键）必须与 RuntimeEvent 外层的字段**逐个匹配**
//! - 任何一个字段不匹配 → 整个事件被拒 → 不写入日志
//!
//! 这样，即使客户端成功将事件发送到 EventStore，EventStore 也会在写入前**全部拦截**伪造的审计事件。
//!
//! # 术语
//!
//! - **AuditRecord（审计记录）**：某条系统决策的服务端原始记录。
//!   比如"用户 Alice 在时刻 T 被批准访问路径 /src"。
//! - **AuditRecordEvent（审计事件包装）**：把 AuditRecord 和必要元数据装在一起的信封。
//!   格式由合约定义，违反格式就无法通过验证。
//! - **RuntimeEvent（运行时事件）**：事件日志里的通用事件格式。
//!   可以表达多种事件（审计、授权变更、工件创建等），但只有审计事件需要这套严格检查。
//! - **幂等性键（idempotency_key）**：用来检测"这个事件我是否已经处理过"。
//!   对审计事件，这个键必须遵循格式 `"audit-record:{audit_id}"` —— 可以被推导验证。
//! - **脱敏配置（redaction_profile）**：标记这条审计记录对谁是可见的、什么内容会被遮蔽。
//!   比如"财务部能看，但工资单数字会打码"。这个配置必须在包装内外**一致**。
//! - **数据纪元（data_epoch）**：一个单调递增的版本号。
//!   EventStore 的任何变更都会推进这个纪元。审计事件中声称的纪元必须与 RuntimeEvent 一致。

use kiana_domain::{AuditRecordEvent, RuntimeEvent, AUDIT_EVENT_KIND};

/// 验证审计事件的格式和完整性，拒绝所有伪造的审计事件。
///
/// 这个函数是 EventStore 的**最后一道防线**。它在事件被写入日志前检查：
///
/// 1. **事件类型检查**：如果这不是审计类型（`audit.*`），直接通过（让其他验证器处理）
/// 2. **类型精确匹配**：如果是 `audit.*` 但不是唯一认可的 `AUDIT_EVENT_KIND`，拒绝 ❌
/// 3. **脱敏元数据验证**：检查脱敏配置是否合法
/// 4. **信封格式验证**：尝试解析 JSON 成 `AuditRecordEvent`，格式错误直接拒绝
/// 5. **信封内部验证**：调用 `AuditRecordEvent::validate()`，规则由合约定义
/// 6. **字段逐个绑定检查**：以下任何一个不匹配都拒绝：
///    - 脱敏配置（RuntimeEvent 外层 vs AuditRecordEvent 内层）
///    - payload_recoverable 必须是 false（审计记录不可恢复 = 永久）
///    - 数据纪元（RuntimeEvent vs AuditRecord）
///    - 工件引用必须为空（审计记录自成体系，不引用其他工件）
///    - 幂等性键必须遵循 `"audit-record:{audit_id}"` 格式
///
/// # 返回值
/// - `Ok(())` ：事件通过所有检查，可以写入日志
/// - `Err(String)` ：包含诊断代码的错误，比如 `"eventlog_audit_kind_untrusted"`
///
/// # 举例
///
/// **✅ 通过的审计事件：**
/// ```text
/// RuntimeEvent {
///   kind: "audit.control_plane_approval",  // 唯一认可的审计类型
///   data: AuditRecordEvent { ... },        // 正确序列化的包装
///   redaction_profile: "department:finance",
///   payload_recoverable: Some(false),      // 永久
///   data_epoch: Some(42),                  // 与内层 AuditRecord 一致
///   idempotency_key: "audit-record:67890", // 正确格式
///   artifact_refs: [],                     // 必须为空
/// }
/// ```
///
/// **❌ 被拒的审计事件（几个例子）：**
/// ```text
/// 1. kind: "audit.custom_type"  // 不在 AUDIT_EVENT_KIND 中
///    → Err: "eventlog_audit_kind_untrusted"
///
/// 2. redaction_profile: "public" vs envelope.redaction_profile: "internal"
///    → Err: "eventlog_audit_redaction_binding_mismatch"
///
/// 3. payload_recoverable: Some(true)  // 审计记录必须不可恢复
///    → Err: "eventlog_audit_payload_recoverable"
///
/// 4. artifact_refs: ["ref:some_artifact"]  // 审计记录不能引用工件
///    → Err: "eventlog_audit_artifact_refs_forbidden"
/// ```
pub(crate) fn validate_runtime_event(event: &RuntimeEvent) -> Result<(), String> {
    // 【快速通路】非审计类型的事件，直接放行
    // 比如"工件创建事件""授权变更"等，这里不处理，让各自的合约检查器去验证
    if !event.kind.starts_with("audit.") {
        return Ok(());
    }

    // 【类型守卫】"audit.*" 里只有**唯一**一个被认可的类型
    // 所有其他以 "audit." 开头的类型都是伪造，一律拒绝
    if event.kind != AUDIT_EVENT_KIND {
        return Err("eventlog_audit_kind_untrusted".to_owned());
    }

    // 【脱敏元数据检查】审计记录的脱敏配置必须本身合法
    // （比如不能是空字符串，格式必须符合部门规范等）
    event
        .validate_redaction_metadata()
        .map_err(|error| format!("eventlog_audit_redaction:{error}"))?;

    // 【格式解析】尝试将事件数据 JSON 反序列化成 AuditRecordEvent
    // 如果 JSON 格式错误，这步就失败 → 拒绝
    let envelope: AuditRecordEvent = serde_json::from_value(event.data.clone())
        .map_err(|_| "eventlog_audit_payload_invalid".to_owned())?;

    // 【信封内部验证】调用 AuditRecordEvent::validate() 进行深层检查
    // 比如检查审计ID是否有效、时间戳是否合理等
    envelope
        .validate()
        .map_err(|error| format!("eventlog_audit_envelope:{error}"))?;

    // 【字段绑定 1：脱敏配置一致性】
    // 外层 RuntimeEvent 声称的脱敏配置，必须与内层 AuditRecordEvent 中的一致
    // 这样可以防止中间人篡改脱敏级别
    if event.redaction_profile.as_deref() != Some(envelope.redaction_profile.as_str()) {
        return Err("eventlog_audit_redaction_binding_mismatch".to_owned());
    }

    // 【字段绑定 2：不可恢复性】
    // 审计记录必须标记为"不可恢复"（payload_recoverable = false）
    // 这意味着一旦写入，就永不改变、永不删除 → 完全的审计追踪
    if event.payload_recoverable != Some(false) {
        return Err("eventlog_audit_payload_recoverable".to_owned());
    }

    // 【字段绑定 3：数据纪元一致性】
    // EventStore 的每次变更都会推进 data_epoch
    // RuntimeEvent 中声称的纪元必须与 AuditRecord 中的一致
    // 不一致 = 数据被篡改或乱序
    if event.data_epoch != Some(envelope.record.data_epoch) {
        return Err("eventlog_audit_data_epoch_mismatch".to_owned());
    }

    // 【字段绑定 4：工件引用禁止】
    // 审计记录必须**自成一体**，不能引用其他工件
    // 这保证了审计事实的"可回溯性" —— 不会因为依赖的工件被删而失效
    if !event.artifact_refs.is_empty() {
        return Err("eventlog_audit_artifact_refs_forbidden".to_owned());
    }

    // 【字段绑定 5：幂等性键绑定】
    // EventStore 使用幂等性键来检测重复提交
    // 对审计事件，这个键必须遵循严格格式：`"audit-record:{audit_id}"`
    // 这样做的好处是可以从幂等性键**推导**出审计ID，用于一致性校验
    let expected_key = format!("audit-record:{}", envelope.record.audit_id);
    if event.idempotency_key.as_deref() != Some(expected_key.as_str()) {
        return Err("eventlog_audit_idempotency_binding_mismatch".to_owned());
    }

    // 【通过】所有检查都符合合约，审计事件可以写入日志
    Ok(())
}
