//! 控制面的**结构化通信记录**：主体之间发送消息、确认交接、升级事故，并把每一次
//! 状态变化都写进事件流。
//!
//! # 这个文件在系统里的位置
//!
//! ```text
//! 入口发起 communication.send / communication.ack / communication.reject /
//!            communication.escalate
//!        ↓
//! 【本文件】校验 → 构造生命周期事件 → record_event 写入 EventLog
//!        ↓
//! kiana-eventlog（事实）
//!        ↓
//! UI / 审计查询（只读投影）
//! ```
//!
//! **上游**：`ControlPlane::handle_command` 把 `communication.*` 命令路由到这里。
//! **下游**：本文件只调用 `ControlPlane` 自己的 `record_event` / `events.read_stream`，
//! 不直接碰文件、网络或模型。
//!
//! # ⚠ 这不是「自由消息总线」
//!
//! roadmap 永久冻结了 TeamCreate / SendMessage 那种自由消息总线（冻结项 FZ-TEAM）。
//! 本模块**不是**那个东西，区别有三点：
//! 1. 每条消息都是一个 **versioned command**，有 schema、有校验、有稳定拒绝码；
//! 2. 每条消息都会产生一条**已提交事件**（`communication.*`），可审计、可重放；
//! 3. 消息有**受限的生命周期**：交接必须被接收方确认，事故只能由发起人升级。
//!
//! 换句话说：这里记录的是「有 accountability（可追责）的事实」，不是可以随便发出去的聊天。
//!
//! # 最重要的一条：通信不等于授权
//!
//! 本文件每个返回值和每条事件里都带着 `authority_granted: false`，这是**故意**的。
//! 收到一条「请帮我做 X」的消息，**不构成**做 X 的授权。
//! 真正执行仍然要过 `authorize_and_execute` → policy → gates → approval → broker。
//!
//! ⚠ 如果将来有人想「消息里说得很清楚就跳过审批直接做」——那正是 FZ-TEAM 想禁止的后门，
//!    也是 `authority_granted` 这个字段存在的唯一理由。
//!
//! # 数据流（以一次交接为例）
//!
//! ```text
//! A 发送 handoff 消息
//!    ↓ record_event("communication.handoff", { message, lifecycle: Sent, ... })
//! EventLog（事实）
//!    ↓ load_communication(message_id) 读回并折叠
//! B 调 communication.ack
//!    ↓ 校验：kind 必须是 Handoff / 必须是 B 本人 / 当前状态必须是 Sent
//!    ↓ record_event("communication.handoff_acknowledged", { accepted: true, ... })
//! EventLog（事实）
//! ```
//!
//! 注意这里**没有任何一张「消息表」**。消息和它的状态都是从事件流里重建出来的。
//! 这就是「EventLog 是唯一事实源」在代码里的样子。
use super::*;
use kiana_domain::{
    CommunicationLifecycleEvent, CommunicationLifecycleStatus, CommunicationMessage,
    CommunicationMessageKind,
};

impl ControlPlane {
    /// 通信命令的统一入口：两道前置闸门 + 按操作名分派。
    ///
    /// 【作用】
    /// 所有 `communication.*` 命令的必经之路。先做**与具体操作无关**的检查，
    /// 再把请求转给 send / acknowledge / escalate 三个处理函数。
    ///
    /// 【调用者】
    /// `ControlPlane::handle_command`（见 `commands.rs`）里的 `communication.*` 分支。
    /// 标成 `pub(crate)` 是因为它是**控制面内部**入口，外部 crate 不应直接调用——
    /// 否则就绕过了 `handle_command` 里的其它前置校验。
    ///
    /// 【输入】
    /// - `context: RequestContext`：服务端解析出的调用者身份；
    /// - `operation`：`send` / `ack` / `reject` / `escalate` 四种操作名；
    /// - `arguments`：JSON 参数。
    ///
    /// 【输出】
    /// `CoreResponse`。未知操作返回 `blocked("communication_operation_invalid")`
    /// ——**既不报错也不静默忽略**，而是明确告诉调用方「没有这个操作」。
    ///
    /// 【失败情况】
    /// 两道闸门（都在分派之前）：
    /// - `communication_sender_required`：`actor_id` 缺失或空白。匿名通信无法追责；
    /// - `project_untrusted`：**项目未被信任**。项目本地的 hook / skill / plugin 配置
    ///   能注入指令，若在未信任项目里放行通信，等于让项目内容获得发言权。
    ///   这是「Trust 边界」在通信路径上的具体体现。
    ///
    /// 【副作用】
    /// 间接产生：本函数自己不写事件，三个处理函数会调用 `record_event`。
    ///
    /// 【为什么闸门放在分派之前】
    /// 因为它们对四个操作**都成立**。放在分派之后，就会有某个新操作忘记检查；
    /// 放在之前，新增操作自动继承这两道闸门——这是「默认安全」的写法。
    pub(crate) async fn handle_communication_command(
        &self,
        context: RequestContext,
        operation: &str,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        if context.actor_id.as_deref().is_none_or(str::is_empty) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "communication_sender_required",
            ));
        }
        if !context.project_trusted {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "project_untrusted",
            ));
        }
        match operation {
            "communication.send" => self.send_communication(context, arguments).await,
            "communication.ack" | "communication.reject" => {
                self.acknowledge_communication(context, operation, arguments)
                    .await
            }
            "communication.escalate" => self.escalate_communication(context, arguments).await,
            _ => Ok(CoreResponse::blocked(
                context.request_id,
                "communication_operation_invalid",
            )),
        }
    }

    /// 发送一条通信消息。
    ///
    /// 【作用】
    /// 校验消息合法性、**确认发送者身份没有被冒充**，然后把消息和它的第一条生命周期事件
    /// 一起写入事件流。
    ///
    /// 【调用者】
    /// `ControlPlane::handle_communication_command`（`operation == "communication.send"`）。
    ///
    /// 【输入】
    /// - `arguments`：必须含 `message` 字段，形状是 `CommunicationMessage`；
    /// - `context`：提供 actor / role / project 身份。
    ///
    /// 【输出】
    /// `CoreResponse::completed`，载荷含 schema、消息本体、`authority_granted: false`。
    ///
    /// 【失败情况】（全部 `blocked`，不是 `Err`）
    /// - `communication_message_required`：没有 `message` 字段；
    /// - `communication_message_invalid`：反序列化失败，**或**结构校验没过
    ///   （两者合并成一个码，因为对调用方的处置一致：消息没发出去）；
    /// - `communication_sender_role_invalid`：`context.role_id` 不在角色目录里。
    ///   没有合法角色就不知道该按谁的权限记这笔账；
    /// - `communication_sender_mismatch`：见下文。
    ///
    /// 【核心流程】
    /// 1. 取 `message` → 2. 反序列化 → 3. `validate()` 结构校验 →
    /// 4. 校验调用方角色存在 → 5. **校验 `message.sender_id` 等于 `context.actor_id`** →
    /// 6. 按消息类型映射事件名 → 7. 构造生命周期事件（状态 `Sent`，序号 1）→
    /// 8. `record_event` 写事件 → 9. 返回响应。
    ///
    /// 【副作用】
    /// 写入一条 `communication.*` 事件——本函数**唯一**的副作用，且是不可逆的事实追加。
    async fn send_communication(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let Some(raw) = arguments.get("message") else {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "communication_message_required",
            ));
        };
        let message: CommunicationMessage = match serde_json::from_value(raw.clone()) {
            Ok(message) => message,
            Err(_) => {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "communication_message_invalid",
                ));
            }
        };
        if message.validate().is_err() {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "communication_message_invalid",
            ));
        }
        if RoleSpec::lookup(&context.role_id).is_none() {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "communication_sender_role_invalid",
            ));
        }
        // 防冒充：消息里自称的发送者，必须就是**服务端认定**的调用者。
        //
        // ⚠ 去掉这一句，任何人都能以别人的名义发消息，而事件里记下的
        //    `sender_id` 看起来还完全合法。事后审计会去查一个无辜的人。
        //
        //    比较的是 `message.sender_id`（客户端填的）与 `context.actor_id`
        //    （服务端从认证链路拿到的）——只有后者可信。
        if message.sender_id != context.actor_id.clone().unwrap_or_default() {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "communication_sender_mismatch",
            ));
        }
        let kind = communication_event_kind(message.kind);
        let lifecycle = CommunicationLifecycleEvent::new(
            &message,
            None,
            CommunicationLifecycleStatus::Sent,
            context.actor_id.clone().unwrap_or_default(),
            "",
            Vec::new(),
            1,
        )
        .map_err(|_| PortError::Failed("communication_lifecycle_invalid".to_owned()))?;
        let mut sequence = 1;
        self.record_event(
            context.request_id,
            &mut sequence,
            kind,
            json!({
                "message": message.clone(),
                "message_id": message.message_id,
                "lifecycle": lifecycle,
                "authority_granted": false,
                "project_root": context.project_root,
                "actor_id": context.actor_id,
                "session_id": context.session_id,
                "request_id": context.request_id,
            }),
        )
        .await?;
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"schema": kiana_domain::COMMUNICATION_MESSAGE_SCHEMA, "message": message, "authority_granted": false}),
        ))
    }

    /// 确认或拒绝一次**交接（handoff）**。
    ///
    /// 【作用】
    /// 交接是「我把一件事交给你」。只有**接收方本人**、且交接确实还挂在待确认状态时，
    /// 才能把它标记为已接收或已拒绝。
    ///
    /// 【调用者】
    /// `handle_communication_command`，`operation` 为 `ack` 或 `reject`。
    /// 这两个操作共用本函数：它们的差别只在**目标状态**，不在校验。
    ///
    /// 【输入】
    /// - `arguments`：必须有 `message_id`（非空）；`reason` 可选（拒绝时建议写）。
    ///
    /// 【输出】
    /// `completed`，载荷带 lifecycle 与 `accepted` 标记。
    ///
    /// 【失败情况】
    /// - `communication_message_id_required`（`Err` 而非 blocked）：**参数缺失属于请求错误**，
    ///   不是业务拒绝。缺 message_id 意味着调用方连「在处理哪件事」都没说清楚；
    /// - `communication_handoff_not_pending`（blocked）：见下文。
    ///
    /// 【为什么 ack 与 reject 共用一个函数】
    /// 从系统角度看，「被接收」和「被拒绝」是同一个状态机的两个分支，
    /// 共享同样的前置条件。拆成两个函数会让校验逻辑出现两份，迟早漂移。
    async fn acknowledge_communication(
        &self,
        context: RequestContext,
        operation: &str,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let message_id = arguments
            .get("message_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| PortError::Failed("communication_message_id_required".to_owned()))?;
        let reason = arguments
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let (message, status, mut sequence) = self.load_communication(message_id).await?;
        // 三个条件必须同时成立才允许确认：
        //   1. 这条消息确实是「交接」——不是聊天、不是事故升级；
        //   2. 确认人是**收件人本人**（`recipient_id == context.actor_id`）。
        //      ⚠ 发送方不能自己确认「我已交给你」——那会让交接凭空闭环、责任凭空消失；
        //   3. 当前状态仍是 `Sent`。已确认或已拒绝的不能再确认。
        //
        // 合并成一个拒绝码是刻意的：这三者表达的是同一件事
        // ——「现在不处于可确认的待办交接状态」。分开报会让调用方以为自己
        // 只需要修其中一个字段。
        if message.kind != CommunicationMessageKind::Handoff
            || message.recipient_id.as_deref() != context.actor_id.as_deref()
            || status != Some(CommunicationLifecycleStatus::Sent)
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "communication_handoff_not_pending",
            ));
        }
        let to_status = if operation == "communication.ack" {
            CommunicationLifecycleStatus::Acknowledged
        } else {
            CommunicationLifecycleStatus::Rejected
        };
        let lifecycle = CommunicationLifecycleEvent::new(
            &message,
            Some(CommunicationLifecycleStatus::Sent),
            to_status,
            context.actor_id.clone().unwrap_or_default(),
            reason,
            Vec::new(),
            sequence,
        )
        .map_err(|_| PortError::Failed("communication_handoff_ack_invalid".to_owned()))?;
        let kind = if to_status == CommunicationLifecycleStatus::Acknowledged {
            "communication.handoff_acknowledged"
        } else {
            "communication.handoff_rejected"
        };
        self.record_event(
            context.request_id,
            &mut sequence,
            kind,
            json!({
                "message": message,
                "message_id": message_id,
                "lifecycle": lifecycle.clone(),
                "accepted": to_status == CommunicationLifecycleStatus::Acknowledged,
                "reason": reason,
                "authority_granted": false,
                "project_root": context.project_root,
                "actor_id": context.actor_id,
                "session_id": context.session_id,
                "request_id": context.request_id,
            }),
        )
        .await?;
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"schema": kiana_domain::COMMUNICATION_LIFECYCLE_SCHEMA, "lifecycle": lifecycle, "authority_granted": false}),
        ))
    }

    /// 升级一条**事故（incident）**通信。
    ///
    /// 【作用】
    /// 把一条事故消息从「已发送」推进到「已升级」。与交接确认相反，
    /// 这里要求操作者是**发起人本人**，而不是收件人。
    ///
    /// 【为什么发起人才能升级，而交接要收件人才能确认】
    /// 因为语义相反：
    /// - 交接的责任在**接收方**——「我接手了」只能由接手的人说；
    /// - 事故升级的责任在**发现方**——「我把它升级了」只能由报告的人说。
    ///   如果允许第三方升级，任何路过的人都能把一条事故消息改成 escalated、
    ///   触发下游告警，而真正的报告人反而不知情。
    ///
    /// 【输入】
    /// - `arguments`：`message_id` 必填；`reason` 可选；`evidence_refs` 可选（证据引用列表）。
    ///
    /// 【输出】
    /// `completed`，载荷带 lifecycle 与 `evidence_refs`。
    ///
    /// 【失败情况】
    /// - `communication_message_id_required`；
    /// - `communication_incident_escalation_denied`：消息不是事故类型、
    ///   操作者不是发起人、或状态已不是 `Sent`；
    /// - `communication_incident_escalation_invalid`：构造出的生命周期事件自检没过
    ///   （`Err`，属于内部不一致）。
    async fn escalate_communication(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let message_id = arguments
            .get("message_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| PortError::Failed("communication_message_id_required".to_owned()))?;
        let reason = arguments
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let evidence_refs = match arguments.get("evidence_refs") {
            None => Vec::new(),
            Some(raw_evidence_refs) => {
                let Some(values) = raw_evidence_refs.as_array() else {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "communication_lifecycle_evidence_invalid",
                    ));
                };
                let Some(evidence_refs) = values
                    .iter()
                    .map(Value::as_str)
                    .collect::<Option<Vec<_>>>()
                else {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "communication_lifecycle_evidence_invalid",
                    ));
                };
                evidence_refs
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            }
        };
        let (message, status, mut sequence) = self.load_communication(message_id).await?;
        if message.kind != CommunicationMessageKind::Incident
            || message.sender_id != context.actor_id.clone().unwrap_or_default()
            || status != Some(CommunicationLifecycleStatus::Sent)
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "communication_incident_escalation_denied",
            ));
        }
        if evidence_refs.is_empty() {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "communication_lifecycle_evidence_required",
            ));
        }
        let lifecycle = CommunicationLifecycleEvent::new(
            &message,
            Some(CommunicationLifecycleStatus::Sent),
            CommunicationLifecycleStatus::Escalated,
            context.actor_id.clone().unwrap_or_default(),
            reason,
            evidence_refs,
            sequence,
        )
        .map_err(|_| PortError::Failed("communication_incident_escalation_invalid".to_owned()))?;
        self.record_event(
            context.request_id,
            &mut sequence,
            "communication.incident_escalated",
            json!({
                "message": message,
                "message_id": message_id,
                "lifecycle": lifecycle.clone(),
                "evidence_refs": lifecycle.evidence_refs.clone(),
                "reason": reason,
                "authority_granted": false,
                "project_root": context.project_root,
                "actor_id": context.actor_id,
                "session_id": context.session_id,
                "request_id": context.request_id,
            }),
        )
        .await?;
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"schema": kiana_domain::COMMUNICATION_LIFECYCLE_SCHEMA, "lifecycle": lifecycle, "authority_granted": false}),
        ))
    }

    /// 从**事件流**里重建一条消息的当前状态。
    ///
    /// 【作用】
    /// 本模块最重要也最容易被误解的函数：它证明「消息本身不需要被存储」。
    /// 消息、状态、下一条序号，全部从 `communication` 事件流**折叠**出来。
    ///
    /// 【调用者】
    /// `acknowledge_communication` 与 `escalate_communication`。
    /// 两者都必须先知道「现在是什么状态」才能决定能不能改。
    ///
    /// 【输入】
    /// - `message_id`：要重建的消息标识。
    ///
    /// 【输出】
    /// 三元组 `(CommunicationMessage, Option<CommunicationLifecycleStatus>, u64)`：
    /// - 消息本体；
    /// - 当前生命周期状态。`None` 表示**只发过消息、还没有任何生命周期事件**，
    ///   调用方据此区分「还没确认过」与「确认过但状态未知」；
    /// - 下一条事件应使用的序号（已见最大序号 + 1）。
    ///
    /// 【核心流程】
    /// 读该 `message_id` 的全部事件，边遍历边：
    ///   1. 推进 `sequence`；
    ///   2. 遇到 `message` 字段就反序列化、**校验 id 匹配**、再 `validate()`；
    ///   3. 遇到 `lifecycle` 字段就用「当前消息」校验它并取其 `to_status`。
    ///
    /// 【失败情况】
    /// - `communication_message_not_found`：事件流里根本没有这条消息（`Err`）；
    /// - `communication_message_id_mismatch`：事件里的 message_id 和请求对不上。
    ///   ⚠ 防的是**串线**：若按 id 读取的流里混进别的消息，确认操作会作用到错误对象上；
    /// - `communication_message_invalid` / `communication_lifecycle_invalid`：事件内容坏了。
    ///
    /// 【为什么每读一条都重新 validate】
    /// 因为事件流是**外部可写**的最终事实：从磁盘恢复、从网络同步来的事件都可能有问题。
    /// 折叠时逐条校验，等于把「事件流可信」这个假设变成「逐条验证过」。
    async fn load_communication(
        &self,
        message_id: &str,
    ) -> Result<
        (
            CommunicationMessage,
            Option<CommunicationLifecycleStatus>,
            u64,
        ),
        CoreError,
    > {
        let events = self.events.read_stream("communication", message_id).await?;
        let mut message = None;
        let mut status = None;
        let mut sequence = 1;
        for event in events {
            // 下一条事件的序号 = 已见最大序号 + 1。
            //
            // 用 `max(...)` 而不是直接赋值：事件流**可能乱序**（并发写入、恢复重放），
            // 直接取最后一条的序号会在乱序时把序号往回退，
            // 进而破坏「同一消息流内序号单调」这个不变量。
            //
            // 用 `saturating_add(1)`：序号已到 `u64::MAX` 时停在最大值，
            // 既不 panic 也不回绕成 0（回绕会让新事件看起来比旧事件更早）。
            sequence = sequence.max(event.sequence.saturating_add(1));
            if let Some(raw) = event.data.get("message") {
                let candidate: CommunicationMessage = serde_json::from_value(raw.clone())
                    .map_err(|_| PortError::Failed("communication_message_invalid".to_owned()))?;
                if candidate.message_id != message_id {
                    return Err(
                        PortError::Failed("communication_message_id_mismatch".to_owned()).into(),
                    );
                }
                candidate
                    .validate()
                    .map_err(|_| PortError::Failed("communication_message_invalid".to_owned()))?;
                message = Some(candidate);
            }
            if let (Some(raw), Some(current)) = (event.data.get("lifecycle"), message.as_ref()) {
                let lifecycle: CommunicationLifecycleEvent = serde_json::from_value(raw.clone())
                    .map_err(|_| PortError::Failed("communication_lifecycle_invalid".to_owned()))?;
                lifecycle
                    .validate(current)
                    .map_err(|_| PortError::Failed("communication_lifecycle_invalid".to_owned()))?;
                status = Some(lifecycle.to_status);
            // 兼容分支：早期事件只有 `kind` 而没有 `lifecycle` 字段。
            //
            // 这种事件被推定为「已发送」。这是**历史兼容**，不是本意——
            // 理想情况下每条通信事件都该带 lifecycle。
            //
            // ⚠ 为什么不改成「缺 lifecycle 就拒绝」？那会让所有旧事件流直接不可读，
            //    而已提交的事实不允许重写。兼容是较小的恶，代价是：老事件流里的
            //    状态推断比新事件流弱。
            //    判定条件加上 `status.is_none()`，是为了保证**新格式优先**——
            //    一旦见过带 lifecycle 的事件，兼容分支就不再插手。
            } else if status.is_none() && event.kind.starts_with("communication.") {
                status = Some(CommunicationLifecycleStatus::Sent);
            }
        }
        let message = message
            .ok_or_else(|| PortError::Failed("communication_message_not_found".to_owned()))?;
        Ok((message, status, sequence))
    }
}

/// 把消息类型映射成事件名。
///
/// 【作用】
/// 事件名是**协议的一部分**：EventLog 里存的就是这些字符串，投影和查询都按它识别。
/// 把映射集中在一个函数，而不是在每个分支各写一个字面量，是为了避免
/// 「同一个事件名在两处写成两个拼法」——那种错误在运行期只表现为「投影查不到」。
///
/// 【七种类型】
/// 聊天 / 命令 / 交接 / 决策 / 状态汇报 / 证据 / 事故。
/// 它们的生命周期规则各不相同（见上面三个处理函数），
/// 名字必须与语义一一对应，不能随意复用。
fn communication_event_kind(kind: CommunicationMessageKind) -> &'static str {
    match kind {
        CommunicationMessageKind::Chat => "communication.chat",
        CommunicationMessageKind::Command => "communication.command",
        CommunicationMessageKind::Handoff => "communication.handoff",
        CommunicationMessageKind::Decision => "communication.decision",
        CommunicationMessageKind::StatusReport => "communication.status_report",
        CommunicationMessageKind::Evidence => "communication.evidence",
        CommunicationMessageKind::Incident => "communication.incident",
    }
}
