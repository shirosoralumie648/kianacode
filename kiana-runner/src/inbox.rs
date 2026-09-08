//! Owned harness 的两级输入 inbox。
//!
//! `next-turn` 用于下一轮模型开始前的唤醒输入，`next-step` 用于当前运行边界的 steering/
//! inject。`claim` 采用“取出即删除”的语义：一旦某个轮次声明了消息，即使后续 pre-step
//! 被拒绝，也不会在下一次重复处理同一条输入。Inbox 只保存进程内展示/调度状态，不是
//! EventLog 或跨进程恢复事实。

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
/// 新消息应插入或领取的时间边界。
pub enum InboxTarget {
    /// 在当前 turn 结束、下一 turn 开始时消费。
    NextTurn,
    /// 在当前 turn 的下一 step 边界消费。
    NextStep,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
/// Inbox 中的一条用户/steering 文本消息。
pub struct InboxMessage {
    /// 原始文本；不会在 inbox 内部解析为命令或权限。
    pub text: String,
}

impl InboxMessage {
    /// 构造一条用户文本消息。
    pub fn user(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// 按 turn 和 step 分开的进程内消息队列。
pub struct Inbox {
    /// 等待下一轮模型的消息。
    pub next_turn: Vec<InboxMessage>,
    /// 等待当前运行下一 step 的消息。
    pub next_step: Vec<InboxMessage>,
}

impl Inbox {
    /// 将消息追加到指定目标队列尾部，保持插入顺序。
    pub fn insert(&mut self, target: InboxTarget, message: InboxMessage) {
        self.queue_mut(target).push(message);
    }

    /// 原子取出并清空指定目标队列。
    ///
    /// 该操作是 delete-only splice；返回后队列中不再保留这些消息，调用方不得因后续
    /// 执行失败而自动重复插回，除非业务明确创建一条新的可审计输入。
    pub fn claim(&mut self, target: InboxTarget) -> Vec<InboxMessage> {
        std::mem::take(self.queue_mut(target))
    }

    #[allow(dead_code)]
    /// 清空两个队列；通常只用于生命周期结束或测试。
    pub fn clear(&mut self) {
        self.next_turn.clear();
        self.next_step.clear();
    }

    #[allow(dead_code)]
    /// 判断任一时间边界是否仍有待处理消息。
    pub fn has_pending(&self) -> bool {
        !self.next_turn.is_empty() || !self.next_step.is_empty()
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
        inbox.insert(InboxTarget::NextTurn, InboxMessage::user("one"));
        inbox.insert(InboxTarget::NextStep, InboxMessage::user("steer"));
        assert_eq!(inbox.claim(InboxTarget::NextTurn)[0].text, "one");
        assert!(inbox.next_turn.is_empty());
        assert_eq!(inbox.claim(InboxTarget::NextStep)[0].text, "steer");
        assert!(!inbox.has_pending());
    }
}
