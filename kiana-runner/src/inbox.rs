//! Owned harness 的两级输入 inbox。
//!
//! `next-turn` 用于下一轮模型开始前的唤醒输入，`next-step` 用于当前运行边界的 steering/
//! inject。每条输入都有 server-owned InputId、来源、目标、接收序号和目标 turn；队列状态
//! 可随 HarnessCheckpoint 序列化，claimed ledger 防止相同 InputId 重复消费。

use kiana_domain::{InputDisposition, InputId, InputReceipt, RunId, TurnId};
use serde::{Deserialize, Serialize};

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
    /// 将消息追加到指定目标队列尾部，保持插入顺序；队满、文本过大或重复 ID 会拒绝。
    pub fn insert(&mut self, target: InboxTarget, message: InboxMessage) -> Result<(), String> {
        self.insert_inner(target, message).map(|_| ())
    }

    /// 带 Run 作用域的输入入口，返回可由 ControlPlane/EventLog 记录的 ACK。
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

    /// 原子取出并清空指定目标队列，并把 InputId 放入 bounded claim ledger。
    pub fn claim(&mut self, target: InboxTarget) -> Vec<InboxMessage> {
        let messages = std::mem::take(self.queue_mut(target));
        self.claimed_input_ids.extend(
            messages
                .iter()
                .map(|message| (message.input_id, message.received_sequence)),
        );
        messages
    }

    /// 返回每条已消费输入的 receipt，供上层在同一 command 中写入 ACK/consumed fact。
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
