//! Owned harness 的两级输入 inbox。
//!
//! `next-turn` 用于下一轮模型开始前的唤醒输入，`next-step` 用于当前运行边界的 steering/
//! inject。每条输入都有 server-owned InputId、来源、目标、接收序号和目标 turn；队列状态
//! 可随 HarnessCheckpoint 序列化，claimed ledger 防止相同 InputId 重复消费。
//!
//! ===========================================================================
//! 【下面这一段是给「刚接手本模块」的读者做的导读】
//!
//! ## 本模块在系统里的位置
//!
//! Inbox 暂存用户/steering 文本，不解析控制命令，也不执行取消。
//! harness 通过 `dispatch`/`inject_message` 接收输入，在 Start/Continue 中领取 NextTurn，
//! 在 `model_step_once` 中领取 NextStep，再转换成 `ModelMessage::user` 加入历史。
//! `RunnerCommand::Cancel` 由独立的取消路径处理，不等待 inbox 消费。
//!
//! ## 两级时间边界
//!
//! **NextTurn**：为开始或继续一轮保留的文本输入。
//! **NextStep**：在后续模型步边界加入的 steering/inject 文本，不直接改工具调用或权限。
//!
//! 同一队列按插入顺序领取；两队列按各自边界消费，不保证跨队列的全局接收顺序。
//!
//! ## 防重复与幂等性
//!
//! 字段校验通过后，`insert_inner()` 按 InputId 查重：队列中已有则返回旧消息与 Duplicate；
//! 已领取则返回本次消息（复用旧 received_sequence）与 Claimed，不再次入队。
//! 已领取台账只保存 ID/序号，不保留旧正文，也不比较重传正文是否一致。
//! `insert()` 丢弃这些明细并返回 `Ok(())`；`insert_for_run()` 才返回带 disposition 的收据。
//!
//! ## 容量限制与反压
//!
//! `insert_inner()` 对新 ID 检查目标队列容量；已满时返回 `inbox_backpressure`。
//! 它拒绝新输入而不是挤掉已排队的消息，调用方需要处理这个反压错误。
//! 三个常数定义了上限：
//! * `INBOX_MAX_MESSAGES`：单个队列（next_turn 或 next_step）最多 256 条消息
//! * `INBOX_MAX_CLAIMED_IDS`：新消息入队时检查已领取台账是否达到 4096 条
//! * `INBOX_MAX_TEXT_BYTES`：单条消息文本最多 256 KiB（按 UTF-8 字节计）
//!
//! `claim()` 本身不做容量检查；多个待领消息可能把台账推过上限，随后 `validate()` 会拒绝。
//! 这些限制不能被描述成对所有公开队列操作都自动生效的总内存上限。

use kiana_domain::{InputDisposition, InputId, InputReceipt, RunId, TurnId};
use serde::{Deserialize, Serialize};

// ===== 容量上限常数 =====
//
// 入队时限制目标队列条数、已有 claim 台账长度与消息字节数，超限返回明确错误。
// validate() 还会检查序列化队列及台账；此处不推测常数背后的业务容量需求。
pub const INBOX_MAX_MESSAGES: usize = 256;
pub const INBOX_MAX_CLAIMED_IDS: usize = 4096;
pub const INBOX_MAX_TEXT_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
/// 新消息应插入或领取的时间边界。
pub enum InboxTarget {
    /// 在当前 turn 结束、下一 turn 开始时消费。
    NextTurn,
    /// 在当前运行的下一 step 边界消费。
    NextStep,
}

impl InboxTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NextTurn => "next-turn",
            Self::NextStep => "next-step",
        }
    }
}

/// Inbox 中的一条用户/steering 文本消息。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboxMessage {
    pub input_id: InputId,
    pub source: String,
    pub target: InboxTarget,
    #[serde(default)]
    pub target_turn_id: Option<TurnId>,
    pub received_sequence: u64,
    /// 原始文本；不会在 inbox 内部解析为命令或权限。
    pub text: String,
}

impl InboxMessage {
    /// 构造一条新的 runner 输入；目标和接收序号由 Inbox::insert server-stamp。
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            input_id: InputId::new(),
            source: "runner".to_owned(),
            target: InboxTarget::NextStep,
            target_turn_id: None,
            received_sequence: 0,
            text: text.into(),
        }
    }

    pub fn from_source(source: impl Into<String>, text: impl Into<String>) -> Self {
        let mut message = Self::user(text);
        message.source = source.into();
        message
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 按 turn 和 step 分开的、可 checkpoint 的输入队列。
pub struct Inbox {
    /// 等待下一轮模型的消息。
    pub next_turn: Vec<InboxMessage>,
    /// 等待当前运行下一 step 的消息。
    pub next_step: Vec<InboxMessage>,
    #[serde(default)]
    next_sequence: u64,
    #[serde(default)]
    claimed_input_ids: Vec<(InputId, u64)>,
    #[serde(default)]
    accepted_receipts: Vec<InputReceipt>,
}

impl Inbox {
    /// 接收文本输入；新 ID 追加到目标队列，已排队或已领取的 ID 不重复入队。
    ///
    /// 返回 `Ok(())` 可能表示新输入或有效重复输入，不能据此区分 Accepted/Duplicate/Claimed。
    /// 字段无效、队满、claim 台账已满或序号溢出分别返回 `inbox_message_invalid`、
    /// `inbox_backpressure`、`inbox_claim_ledger_full`、`inbox_sequence_exhausted`。
    pub fn insert(&mut self, target: InboxTarget, message: InboxMessage) -> Result<(), String> {
        self.insert_inner(target, message).map(|_| ())
    }

    /// 带 Run 作用域的输入入口，返回可由 ControlPlane/EventLog 记录的 ACK 收据。
    ///
    /// 与 insert 共用 insert_inner，并为结果构造带 RunId 的 InputReceipt。
    /// 只有 Accepted 收据会追加到 accepted_receipts；本方法不自行写 EventLog。
    /// 收据构造发生在队列操作之后，若构造失败不会回滚已完成的入队。
    pub fn insert_for_run(
        &mut self,
        run_id: RunId,
        target: InboxTarget,
        message: InboxMessage,
    ) -> Result<InputReceipt, String> {
        let (message, disposition) = self.insert_inner(target, message)?;
        let receipt = InputReceipt::new(
            message.input_id,
            run_id,
            message.source.clone(),
            message.target.as_str(),
            message.received_sequence,
            disposition,
        )?;
        if disposition == InputDisposition::Accepted {
            self.accepted_receipts.push(receipt.clone());
        }
        Ok(receipt)
    }

    // ===== 内部细节：insert_inner 的决策树 =====
    //
    // insert_inner 是 Inbox 的**防守线**。它按以下顺序检查，第一条失败的返回错误：
    //
    // 1. **字段合法性**（消息本身格式）
    //    - InputId 不能是 nil UUID
    //    - source 不能为空、不能超 256 字节、不能含 \0
    //    - text 不能为空、不能超 INBOX_MAX_TEXT_BYTES
    //
    // 2. **重复检测**（已在队列里的消息）
    //    - 用 find() 在两个队列里扫一遍，有就返回 Duplicate
    //
    // 3. **Claim 台账检测**（已经被取走过的消息）
    //    - 只保存了 ID/序号：返回本次消息，覆盖为传入 target 和旧序号，标记 Claimed
    //
    // 4. **容量检测**
    //    - 目标队列是否已满（≥ INBOX_MAX_MESSAGES）
    //    - claimed_input_ids 是否已满（≥ INBOX_MAX_CLAIMED_IDS）
    //
    // 5. **序号分配并入队**
    //    - 递增 next_sequence
    //    - 新消息推进目标队列
    //    - 返回 Accepted
    fn insert_inner(
        &mut self,
        target: InboxTarget,
        mut message: InboxMessage,
    ) -> Result<(InboxMessage, InputDisposition), String> {
        if message.input_id.as_uuid().is_nil()
            || message.source.trim().is_empty()
            || message.source.len() > 256
            || message.source.contains('\0')
            || message.text.trim().is_empty()
            || message.text.len() > INBOX_MAX_TEXT_BYTES
        {
            return Err("inbox_message_invalid".to_owned());
        }
        if let Some(existing) = self.find(message.input_id).cloned() {
            return Ok((existing, InputDisposition::Duplicate));
        }
        if let Some((_, sequence)) = self
            .claimed_input_ids
            .iter()
            .find(|(input_id, _)| *input_id == message.input_id)
        {
            message.target = target;
            message.received_sequence = *sequence;
            return Ok((message, InputDisposition::Claimed));
        }
        if self.queue(target).len() >= INBOX_MAX_MESSAGES {
            return Err("inbox_backpressure".to_owned());
        }
        if self.claimed_input_ids.len() >= INBOX_MAX_CLAIMED_IDS {
            return Err("inbox_claim_ledger_full".to_owned());
        }
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| "inbox_sequence_exhausted".to_owned())?;
        message.target = target;
        message.received_sequence = self.next_sequence;
        self.queue_mut(target).push(message.clone());
        Ok((message, InputDisposition::Accepted))
    }

    /// 在独占借用下取出并清空目标队列，把 ID/序号追加到 claim ledger，再返回消息。
    ///
    /// mem::take 替换 Vec，不是并发原子操作或持久事务。此方法不检查台账容量；
    /// 后续有效输入若复用这些 ID，insert_inner 会返回 Claimed 而不重新入队。
    pub fn claim(&mut self, target: InboxTarget) -> Vec<InboxMessage> {
        let messages = std::mem::take(self.queue_mut(target));
        self.claimed_input_ids.extend(
            messages
                .iter()
                .map(|message| (message.input_id, message.received_sequence)),
        );
        messages
    }

    /// 领取消息后，为每条消息构造 Claimed 收据；不证明消息已被模型处理或写入 EventLog。
    ///
    /// claim 先清空队列并更新台账，再构造收据。如果收据构造失败，队列不会自动恢复。
    pub fn claim_with_receipts(
        &mut self,
        run_id: RunId,
        target: InboxTarget,
    ) -> Result<Vec<(InboxMessage, InputReceipt)>, String> {
        self.claim(target)
            .into_iter()
            .map(|message| {
                let receipt = InputReceipt::new(
                    message.input_id,
                    run_id,
                    message.source.clone(),
                    target.as_str(),
                    message.received_sequence,
                    InputDisposition::Claimed,
                )?;
                Ok((message, receipt))
            })
            .collect()
    }

    #[allow(dead_code)]
    /// 清空两个队列和 claim ledger；通常只用于生命周期结束。
    pub fn clear(&mut self) {
        self.next_turn.clear();
        self.next_step.clear();
        self.claimed_input_ids.clear();
        self.accepted_receipts.clear();
    }

    #[allow(dead_code)]
    /// 判断任一时间边界是否仍有待处理消息。
    pub fn has_pending(&self) -> bool {
        !self.next_turn.is_empty() || !self.next_step.is_empty()
    }

    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    pub fn claimed(&self, input_id: InputId) -> bool {
        self.claimed_input_ids
            .iter()
            .any(|(claimed, _)| *claimed == input_id)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.next_turn.len() > INBOX_MAX_MESSAGES
            || self.next_step.len() > INBOX_MAX_MESSAGES
            || self.claimed_input_ids.len() > INBOX_MAX_CLAIMED_IDS
        {
            return Err("inbox_limit_exceeded".to_owned());
        }
        let mut ids = std::collections::HashSet::new();
        for message in self.next_turn.iter().chain(self.next_step.iter()) {
            if message.input_id.as_uuid().is_nil()
                || message.source.trim().is_empty()
                || message.text.trim().is_empty()
                || message.text.len() > INBOX_MAX_TEXT_BYTES
                || message.received_sequence == 0
                || message.received_sequence > self.next_sequence
                || message.target
                    != if self
                        .next_turn
                        .iter()
                        .any(|item| item.input_id == message.input_id)
                    {
                        InboxTarget::NextTurn
                    } else {
                        InboxTarget::NextStep
                    }
                || !ids.insert(message.input_id)
            {
                return Err("inbox_message_invalid".to_owned());
            }
        }
        for (input_id, sequence) in &self.claimed_input_ids {
            if input_id.as_uuid().is_nil()
                || *sequence == 0
                || *sequence > self.next_sequence
                || !ids.insert(*input_id)
            {
                return Err("inbox_claim_ledger_invalid".to_owned());
            }
        }
        if self.accepted_receipts.len() > INBOX_MAX_CLAIMED_IDS
            || self
                .accepted_receipts
                .iter()
                .any(|receipt| receipt.validate().is_err())
        {
            return Err("inbox_receipt_ledger_invalid".to_owned());
        }
        Ok(())
    }

    fn find(&self, input_id: InputId) -> Option<&InboxMessage> {
        self.next_turn
            .iter()
            .chain(self.next_step.iter())
            .find(|message| message.input_id == input_id)
    }

    fn queue(&self, target: InboxTarget) -> &Vec<InboxMessage> {
        match target {
            InboxTarget::NextTurn => &self.next_turn,
            InboxTarget::NextStep => &self.next_step,
        }
    }

    fn queue_mut(&mut self, target: InboxTarget) -> &mut Vec<InboxMessage> {
        match target {
            InboxTarget::NextTurn => &mut self.next_turn,
            InboxTarget::NextStep => &mut self.next_step,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_is_delete_only_and_does_not_restore_messages() {
        let mut inbox = Inbox::default();
        inbox
            .insert(InboxTarget::NextTurn, InboxMessage::user("one"))
            .unwrap();
        inbox
            .insert(InboxTarget::NextStep, InboxMessage::user("steer"))
            .unwrap();
        assert_eq!(inbox.claim(InboxTarget::NextTurn)[0].text, "one");
        assert!(inbox.next_turn.is_empty());
        assert_eq!(inbox.claim(InboxTarget::NextStep)[0].text, "steer");
        assert!(!inbox.has_pending());
    }

    #[test]
    fn duplicate_input_is_idempotent_and_claim_is_persisted() {
        let mut inbox = Inbox::default();
        let message = InboxMessage::user("same");
        let id = message.input_id;
        inbox
            .insert(InboxTarget::NextStep, message.clone())
            .unwrap();
        inbox.insert(InboxTarget::NextStep, message).unwrap();
        assert_eq!(inbox.next_step.len(), 1);
        inbox.claim(InboxTarget::NextStep);
        assert!(inbox.claimed(id));
        assert!(inbox
            .insert(
                InboxTarget::NextStep,
                InboxMessage {
                    input_id: id,
                    ..InboxMessage::user("same")
                }
            )
            .is_ok());
        assert!(inbox.next_step.is_empty());
    }

    #[test]
    fn full_queue_returns_backpressure_without_overwrite() {
        let mut inbox = Inbox::default();
        for index in 0..INBOX_MAX_MESSAGES {
            inbox
                .insert(InboxTarget::NextStep, InboxMessage::user(index.to_string()))
                .unwrap();
        }
        assert_eq!(
            inbox.insert(InboxTarget::NextStep, InboxMessage::user("overflow")),
            Err("inbox_backpressure".to_owned())
        );
        assert_eq!(inbox.next_step.len(), INBOX_MAX_MESSAGES);
    }
}
