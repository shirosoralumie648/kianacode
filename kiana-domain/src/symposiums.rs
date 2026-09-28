//! 「开一次会」这件事被落成了类型：议题、主持、与会者、发言（Claim）、投票（Vote）、
//! 黑板、以及最后产出的决策记录。
//!
//! # 为什么值得一个专门的文件
//!
//! 因为「多角色讨论」很容易滑向**自由消息总线**——大家随便发、随便读、
//! 结论从聊天记录里捞出来。`AGENTS.md` 的 FZ-TEAM 永久冻结了那种东西。
//!
//! 这里的设计正相反：讨论被拆成**结构化的几种记录**
//! （Claim 是发言、Vote 是立场、Blackboard 是当前累积、DecisionRecord 是结论），
//! 每一种都有明确的字段与归属。聊天记录不是事实，DecisionRecord 才是。
//!
//! # 与「自由群聊」的区别，一句话
//!
//! 自由群聊里，**说过什么**就是事实；
//! 这里，**通过了什么**才是事实，说过什么只是一份可被复核的输入。
use crate::{
    DepartmentSpec, RoleSpec, WorkPacket, DECISION_RECORD_PATH, DECISION_RECORD_SCHEMA,
    DEPARTMENT_EXECUTING, DEPARTMENT_PLANNING, REVIEW_PACKET_SCHEMA, ROLE_BUILDER, ROLE_PM,
    SYMPOSIUM_SCHEMA, SYMPOSIUM_STATUS_CLOSED, SYMPOSIUM_STATUS_SKIPPED, SYMPOSIUM_TYPE_DECISION,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReviewPacket {
    pub schema: String,
    pub id: String,
    pub author_session_id: String,
    pub author_role_id: String,
    pub reviewer_session_id: String,
    pub verdict: String,
    pub summary: String,
    #[serde(default)]
    pub files_reviewed: Vec<String>,
}

impl ReviewPacket {
    pub fn closed(
        id: impl Into<String>,
        author_session_id: impl Into<String>,
        author_role_id: impl Into<String>,
        reviewer_session_id: impl Into<String>,
        verdict: impl Into<String>,
        summary: impl Into<String>,
        files_reviewed: Vec<String>,
    ) -> Self {
        Self {
            schema: REVIEW_PACKET_SCHEMA.to_owned(),
            id: id.into(),
            author_session_id: author_session_id.into(),
            author_role_id: author_role_id.into(),
            reviewer_session_id: reviewer_session_id.into(),
            verdict: verdict.into(),
            summary: summary.into(),
            files_reviewed,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != REVIEW_PACKET_SCHEMA {
            return Err("review_packet_invalid");
        }
        if self.id.trim().is_empty() {
            return Err("review_id_required");
        }
        if self.author_session_id.trim().is_empty() {
            return Err("review_author_required");
        }
        if self.reviewer_session_id.trim().is_empty() {
            return Err("review_session_required");
        }
        if self.author_session_id.trim() == self.reviewer_session_id.trim() {
            return Err("review_author_session_denied");
        }
        if self.author_role_id.trim() != ROLE_BUILDER {
            return Err("review_author_must_be_builder");
        }
        match self.verdict.trim() {
            "pass" | "fail" | "needs_change" => Ok(()),
            _ => Err("review_verdict_invalid"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SymposiumClaim {
    pub speaker: String,
    pub text: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

impl SymposiumClaim {
    pub fn new(speaker: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            speaker: speaker.into(),
            text: text.into(),
            evidence_refs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SymposiumVote {
    pub role: String,
    pub stance: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
/// 会议进行到当前为止的全部发言与投票。
///
/// 【它是「当前累积」，不是「最终结论」】
/// 黑板随时可以被改写。真正的结论在 `DecisionRecord` 里，
/// 而且结论必须能指回它依据的那些发言与投票。
///
/// 【⚠ `draft_decision` 为什么是可选的】
/// 因为**很多会议不会达成结论**。允许「开完什么也没定」是一个诚实的状态；
/// 强迫每次会议都产出一个结论，就会让会议变成一个走过场的仪式。
pub struct Blackboard {
    #[serde(default)]
    pub claims: Vec<SymposiumClaim>,
    #[serde(default)]
    pub votes: Vec<SymposiumVote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_decision: Option<String>,
}

impl Blackboard {
    /// 把黑板渲染成一段提示词，交给模型。
    ///
    /// 【这一步值得警惕的地方】
    /// 它是「结构化数据」变成「自然语言」的地方。一旦发生，
    /// 下游看到的就是一段文本，发言人与投票的对应关系只存在于渲染者的心智里。
    ///
    /// 之所以仍然可以接受，是因为：黑板本身是**被保存的**（`claims` / `votes` 是字段），
    /// 渲染只是其中一次消费。事后要复核「当时谁投了什么」，读的是字段而不是这段文本。
    ///
    /// **如果哪天有人把这段 prompt 当成唯一记录保存，审计能力就没了。**
    ///
    /// 【空黑板也要显式说明】
    /// `claims` 为空时它写下 "Blackboard claims: (none)" 而不是省略这一行。
    /// 省略的话，模型无法区分「没人发言」和「这一部分没被渲染出来」。
    pub fn as_prompt(&self) -> String {
        let mut lines = Vec::new();
        if self.claims.is_empty() {
            lines.push("Blackboard claims: (none)".to_owned());
        } else {
            lines.push("Blackboard claims:".to_owned());
            for claim in &self.claims {
                lines.push(format!("- {}: {}", claim.speaker.trim(), claim.text.trim()));
            }
        }
        if !self.votes.is_empty() {
            lines.push("Blackboard votes:".to_owned());
            for vote in &self.votes {
                lines.push(format!(
                    "- {}: {} ({})",
                    vote.role.trim(),
                    vote.stance.trim(),
                    vote.reason.trim()
                ));
            }
        }
        if let Some(draft) = self
            .draft_decision
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            lines.push(format!("Draft decision: {draft}"));
        }
        lines.join("\n")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub schema: String,
    pub id: String,
    pub symposium_id: String,
    pub summary: String,
    pub decision: String,
    pub work_packet_id: String,
    pub skipped_meeting: bool,
}

impl DecisionRecord {
    pub fn closed(
        id: impl Into<String>,
        symposium_id: impl Into<String>,
        summary: impl Into<String>,
        decision: impl Into<String>,
        work_packet_id: impl Into<String>,
        skipped_meeting: bool,
    ) -> Self {
        Self {
            schema: DECISION_RECORD_SCHEMA.to_owned(),
            id: id.into(),
            symposium_id: symposium_id.into(),
            summary: summary.into(),
            decision: decision.into(),
            work_packet_id: work_packet_id.into(),
            skipped_meeting,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Symposium {
    pub schema: String,
    pub id: String,
    pub department_id: String,
    #[serde(rename = "type")]
    pub symposium_type: String,
    pub agenda: String,
    pub chair: String,
    pub attendees: Vec<String>,
    pub max_rounds: u32,
    pub blackboard: Blackboard,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_packet_id: Option<String>,
}

impl Symposium {
    /// 默认最多讨论 4 轮。
    ///
    /// 【为什么必须有上限】
    /// 一场没有轮次上限的会议不会自己结束：每个角色都能继续发言，
    /// 而每次发言又会产生新的发言点。会议因此变成一个不会收敛的循环，
    /// 而且每一次循环都在消耗模型调用。
    ///
    /// 4 轮不是「差不多够」的估计，而是一条**明确的退出线**：
    /// 到了就走 `DecisionRecord`（哪怕结论是「没定」），
    /// 而不是继续开会。
    pub const DEFAULT_MAX_ROUNDS: u32 = 4;
    pub const MAX_ROUNDS_CAP: u32 = 8;

    pub fn planning(id: impl Into<String>, agenda: impl Into<String>, max_rounds: u32) -> Self {
        Self::department(DEPARTMENT_PLANNING, id, agenda, max_rounds)
            .expect("planning department can convene")
    }

    pub fn department(
        department_id: impl AsRef<str>,
        id: impl Into<String>,
        agenda: impl Into<String>,
        max_rounds: u32,
    ) -> Result<Self, &'static str> {
        let department =
            DepartmentSpec::lookup(department_id.as_ref()).ok_or("department_unknown")?;
        if !department.can_convene {
            return Err("symposium_department_cannot_convene");
        }
        let chair = department
            .convene_chair_id()
            .ok_or("symposium_chair_cannot_convene")?;
        Ok(Self {
            schema: SYMPOSIUM_SCHEMA.to_owned(),
            id: id.into(),
            department_id: department.department_id.clone(),
            symposium_type: SYMPOSIUM_TYPE_DECISION.to_owned(),
            agenda: agenda.into(),
            chair,
            attendees: department.roles.clone(),
            max_rounds,
            blackboard: Blackboard::default(),
            status: "proposed".to_owned(),
            decision_id: None,
            work_packet_id: None,
        })
    }

    pub fn validate_max_rounds(max_rounds: u32) -> Result<u32, &'static str> {
        if max_rounds == 0 || max_rounds > Self::MAX_ROUNDS_CAP {
            Err("symposium_max_rounds_invalid")
        } else {
            Ok(max_rounds)
        }
    }

    pub fn speaker_prompt(&self, role_id: &str) -> String {
        let builder_rule = if self.department_id == DEPARTMENT_EXECUTING {
            "Stay on this department's artifacts. Do not rewrite planning packets."
        } else {
            "Do not invite the Builder."
        };
        format!(
            "{} symposium {}\nChair: {}\nAttendees: {}\nSpeak as {}\nAgenda: {}\n{}\nReply with a claim or vote. Do not patch source. {}",
            self.department_id.trim(),
            self.id.trim(),
            self.chair.trim(),
            self.attendees.join(", "),
            role_id.trim(),
            self.agenda.trim(),
            self.blackboard.as_prompt(),
            builder_rule
        )
    }

    pub fn speaker_session_id(&self, role_id: &str) -> String {
        format!("{}-{}", self.id.trim(), role_id.trim())
    }

    pub fn builder_present(&self) -> bool {
        self.attendees
            .iter()
            .any(|role| role.trim() == ROLE_BUILDER)
    }

    pub fn decision_path(&self) -> &'static str {
        DepartmentSpec::lookup(&self.department_id)
            .map(|department| department.decision_path())
            .unwrap_or(DECISION_RECORD_PATH)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        Self::validate_max_rounds(self.max_rounds)?;
        if self.agenda.trim().is_empty() {
            return Err("symposium_goal_required");
        }
        let department = DepartmentSpec::lookup(&self.department_id).ok_or("department_unknown")?;
        if !department.can_convene {
            return Err("symposium_department_cannot_convene");
        }
        if self.department_id == DEPARTMENT_PLANNING && self.chair.trim() != ROLE_PM {
            return Err("symposium_chair_must_be_pm");
        }
        let Some(chair) = RoleSpec::lookup(&self.chair) else {
            return Err("symposium_chair_cannot_convene");
        };
        if !chair.can_convene || chair.department_id != department.department_id {
            return Err("symposium_chair_cannot_convene");
        }
        if self.department_id != DEPARTMENT_EXECUTING
            && self
                .attendees
                .iter()
                .any(|role| role.trim() == ROLE_BUILDER)
        {
            return Err("symposium_builder_not_attendee");
        }
        for role_id in &self.attendees {
            let Some(role) = RoleSpec::lookup(role_id) else {
                return Err("symposium_attendees_invalid");
            };
            if role.department_id != department.department_id {
                return Err("joint_symposium_frozen");
            }
        }
        if self.attendees.len() != department.roles.len()
            || self
                .attendees
                .iter()
                .zip(department.roles.iter())
                .any(|(got, want)| got.trim() != want.trim())
        {
            return Err("symposium_attendees_invalid");
        }
        if !self
            .attendees
            .iter()
            .any(|role| role.trim() == self.chair.trim())
        {
            return Err("symposium_attendees_invalid");
        }
        Ok(())
    }

    pub fn close(
        &mut self,
        skipped_meeting: bool,
    ) -> Result<(DecisionRecord, Option<WorkPacket>), &'static str> {
        self.validate()?;
        let decision_id = format!("dec-{}", self.id.trim());
        let decision_text = self
            .blackboard
            .draft_decision
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| self.agenda.trim())
            .to_owned();
        if decision_text.is_empty() {
            return Err("symposium_decision_required");
        }
        let summary = if skipped_meeting {
            format!("Anti-meeting: proceed with {decision_text}")
        } else {
            format!(
                "{} symposium closed: {decision_text}",
                self.department_id.trim()
            )
        };
        let packet = if self.department_id == DEPARTMENT_PLANNING {
            let packet_id = format!("wp-{}", self.id.trim());
            let packet = WorkPacket::builder_task(packet_id, self.agenda.trim());
            packet.validate()?;
            Some(packet)
        } else {
            None
        };
        let packet_id = packet
            .as_ref()
            .map(|packet| packet.id.clone())
            .unwrap_or_default();
        let decision = DecisionRecord::closed(
            decision_id,
            self.id.clone(),
            summary,
            decision_text,
            packet_id.clone(),
            skipped_meeting,
        );
        self.status = if skipped_meeting {
            SYMPOSIUM_STATUS_SKIPPED.to_owned()
        } else {
            SYMPOSIUM_STATUS_CLOSED.to_owned()
        };
        self.decision_id = Some(decision.id.clone());
        self.work_packet_id = if packet_id.is_empty() {
            None
        } else {
            Some(packet_id)
        };
        Ok((decision, packet))
    }
}
