//! EventLog checks for data-governance propagation facts.
//!
//! Governance receipts are ordinary immutable RuntimeEvent facts. The adapter validates their
//! typed envelope before CAS/idempotency/write and never edits or removes the source event.
//!
//! # 这个文件在系统里的位置
//!
//! Kiana 中的"数据治理"是指**谁可以看到、谁可以修改、数据流到哪里** 的一系列政策规则。
//! 这些规则不是在代码里硬编码的，而是**作为事件记录在不可变的事件日志中**。
//!
//! 本文件的工作是：当治理相关的事件想要写入日志时，在写入前进行**严格的格式和内容验证**。
//! 当前支持两类治理事件：
//!
//! 1. **数据传播收据（data.propagation_receipt）**
//!    - 记录"某条数据从 A 地传播到了 B 地"
//!    - 用于追踪数据流向、验证数据隐私政策
//!    - 示例：用户数据从"服务端"复制到"本地缓存"的时刻和上下文
//!
//! 2. **不可变事件封条（data.immutable_event_seal）**
//!    - 记录"这个事件从这一刻起被冻结，任何人都无法改变它"
//!    - 用于建立"事实链"，确保审计追踪无法被改写
//!    - 比在单个审计事件上加印章更强：这是**对整个事件的不可变性认证**
//!
//! # 最重要的一条不变量：治理事件是自证的
//!
//! 这两类事件都是**自证的**（self-validating）：
//! - 内容必须符合格式（格式错了直接拒绝）
//! - 内容必须满足业务规则（通过各自的 `.validate()` 方法检查）
//! - 某些字段有额外的约束（比如不可变封条不能包含工件引用）
//!
//! # 术语
//!
//! - **DataPropagationReceipt（数据传播收据）**：
//!   记录某条数据的"旅程"。包含源头、目标、传播时间、上下文信息。
//! - **ImmutableEventSeal（不可变事件封条）**：
//!   一个**加密的证明**，声称"从此刻起，前面那条事件无法被改变"。
//!   一旦签发，任何人都可以用这个证明来验证：该事件没有被篡改过。
//! - **data_epoch（数据纪元）**：
//!   EventLog 的版本号。在 EventLog 中每加一条事件，data_epoch 都会递增。
//!   所有治理事件必须声称与当前 data_epoch 一致。
//! - **payload_recoverable（有效负载可恢复性）**：
//!   false = "这条事件的内容完全不可恢复"（比如被脱敏后写入）。
//!   对于不可变封条，必须是 false（确保封条本身不会被复原）。
//! - **artifact_refs（工件引用）**：
//!   指向其他数据工件（比如"审计报告"、"用户档案"）的引用集合。
//!   不可变封条中这个字段必须为空（封条必须自成一体）。

use kiana_domain::{DataPropagationReceipt, ImmutableEventSeal, RuntimeEvent};

/// 验证治理相关事件的格式和完整性，分门别类地检查两种治理事件。
///
/// 这个函数是 EventLog 对**治理事实的防火墙**。它根据事件类型（kind）分别验证：
///
/// 1. **数据传播收据** (`data.propagation_receipt`)：
///    - 解析 JSON 成 `DataPropagationReceipt` 类型
///    - 调用内部 `.validate()` 检查业务规则
///    - 验证 data_epoch 与声称的值一致
///
/// 2. **不可变事件封条** (`data.immutable_event_seal`)：
///    - 解析 JSON 成 `ImmutableEventSeal` 类型
///    - 调用内部 `.validate()` 检查业务规则
///    - 验证封条的 payload_recoverable 必须是 false
///    - 验证封条的 artifact_refs 必须为空（不能引用其他工件）
///    - 验证 data_epoch 与声称的值一致
///
/// 3. **其他事件**：不处理，直接返回 OK（让其他合约检查器处理）
///
/// # 返回值
/// - `Ok(())` ：事件通过所有检查，可以写入日志
/// - `Err(String)` ：包含诊断代码的错误，比如 `"eventlog_data_propagation_payload_invalid"`
///
/// # 举例
///
/// **✅ 通过的数据传播收据：**
/// ```text
/// RuntimeEvent {
///   kind: "data.propagation_receipt",
///   data: DataPropagationReceipt {
///     source: "server",
///     destination: "local_cache",
///     data_epoch: 42,
///     ...
///   },
///   data_epoch: Some(42),  // 必须与内层一致
/// }
/// ```
///
/// **✅ 通过的不可变封条：**
/// ```text
/// RuntimeEvent {
///   kind: "data.immutable_event_seal",
///   data: ImmutableEventSeal {
///     sealed_event_id: "evt:12345",
///     signature: "...",
///     data_epoch: 42,
///     ...
///   },
///   payload_recoverable: Some(false),  // 必须是 false
///   artifact_refs: [],                 // 必须为空
///   data_epoch: Some(42),              // 必须与内层一致
/// }
/// ```
///
/// **❌ 被拒的治理事件（几个例子）：**
/// ```text
/// 1. kind: "data.propagation_receipt"
///    data: "{ invalid json }"
///    → Err: "eventlog_data_propagation_payload_invalid"
///
/// 2. kind: "data.immutable_event_seal"
///    payload_recoverable: Some(true)  // 违反"不可恢复"约束
///    → Err: "eventlog_immutable_event_seal_payload_boundary"
///
/// 3. kind: "data.immutable_event_seal"
///    artifact_refs: ["ref:some_artifact"]  // 封条不能引用工件
///    → Err: "eventlog_immutable_event_seal_payload_boundary"
///
/// 4. kind: "data.propagation_receipt"
///    data_epoch: Some(99) vs receipt.data_epoch: 42
///    → Err: "eventlog_data_propagation_epoch_mismatch"
/// ```
pub(crate) fn validate_runtime_event(event: &RuntimeEvent) -> Result<(), String> {
    match event.kind.as_str() {
        // 【分支 1：数据传播收据】
        // 当事件类型是"有数据从 A 地流向了 B 地"的记录时，执行这个验证
        "data.propagation_receipt" => {
            // 【格式解析】尝试将事件数据 JSON 反序列化成 DataPropagationReceipt
            // 如果 JSON 格式错误或字段缺失，直接拒绝
            let receipt: DataPropagationReceipt = serde_json::from_value(event.data.clone())
                .map_err(|_| "eventlog_data_propagation_payload_invalid".to_owned())?;

            // 【内部验证】调用 DataPropagationReceipt::validate() 进行业务规则检查
            // 比如检查源头和目标是否有效、传播链是否合理等
            receipt
                .validate()
                .map_err(|error| format!("eventlog_data_propagation_receipt:{error}"))?;

            // 【data_epoch 绑定】
            // RuntimeEvent 中声称的 data_epoch，必须与收据内部声称的一致
            // 不一致 = 数据可能在传播过程中被篡改了
            if event.data_epoch != Some(receipt.data_epoch) {
                return Err("eventlog_data_propagation_epoch_mismatch".to_owned());
            }
        }

        // 【分支 2：不可变事件封条】
        // 当事件类型是"某条事件从此刻起被冻结"的证明时，执行这个验证
        "data.immutable_event_seal" => {
            // 【格式解析】尝试将事件数据 JSON 反序列化成 ImmutableEventSeal
            // 如果 JSON 格式错误或必要字段缺失，直接拒绝
            let seal: ImmutableEventSeal = serde_json::from_value(event.data.clone())
                .map_err(|_| "eventlog_immutable_event_seal_payload_invalid".to_owned())?;

            // 【内部验证】调用 ImmutableEventSeal::validate() 进行业务规则检查
            // 比如检查签名是否有效、被封条的事件是否存在等
            seal.validate()
                .map_err(|error| format!("eventlog_immutable_event_seal:{error}"))?;

            // 【界限检查：不可恢复性和工件引用】
            // 不可变封条有两个严格的约束：
            // 1. payload_recoverable 必须是 false（封条本身不可恢复）
            // 2. artifact_refs 必须为空（封条自成一体，不依赖其他工件）
            // 任何一个违反都拒绝
            if event.payload_recoverable != Some(false) || !event.artifact_refs.is_empty() {
                return Err("eventlog_immutable_event_seal_payload_boundary".to_owned());
            }

            // 【data_epoch 绑定】
            // RuntimeEvent 中声称的 data_epoch，必须与封条内部声称的一致
            // 不一致 = 可能在签发后被篡改
            if event.data_epoch != Some(seal.data_epoch) {
                return Err("eventlog_immutable_event_seal_epoch_mismatch".to_owned());
            }
        }

        // 【默认分支】
        // 如果事件类型不是治理事件（既不是传播收据，也不是不可变封条），
        // 这个函数就不处理，直接返回 OK 让其他合约检查器处理
        _ => {}
    }

    // 【通过】事件符合治理合约的所有要求，可以写入日志
    Ok(())
}
