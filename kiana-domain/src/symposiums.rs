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
pub struct Blackboard {
    #[serde(default)]
    pub claims: Vec<SymposiumClaim>,
    #[serde(default)]
    pub votes: Vec<SymposiumVote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_decision: Option<String>,
}

impl Blackboard {
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
