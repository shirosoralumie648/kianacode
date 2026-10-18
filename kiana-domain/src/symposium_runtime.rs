//! Bounded runtime-admission facts for role symposiums.
//!
//! The Runner remains the only model loop. This contract only accounts for finite limits and
//! decides whether a board may be closed; it does not dispatch a model or publish a decision.
//!
//! [`SymposiumRunBudget`] is the finite admission ledger: rounds, contributions/messages,
//! provider-reported tokens, wall time, stall rounds and cancellation. Core records against it
//! while the deterministic chair schedule runs and may only close the meeting while the ledger
//! is not exhausted.
//!
//! [`SymposiumContributionProposal`] is the only shape a speaker turn may return. Core parses it,
//! then validates it through the CO-14 governance board before anything reaches the blackboard.
//! Free text is never treated as a contribution on its own.
//!
//! Neither type executes a tool, opens a session or grants approval. Joint meetings stay frozen in
//! `symposiums.rs`; this module never composes one.
use crate::{SymposiumContributionKind, SymposiumVisibility};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SYMPOSIUM_RUN_BUDGET_SCHEMA: &str = "kiana.symposium-run-budget.v1";
pub const SYMPOSIUM_CONTRIBUTION_PROPOSAL_SCHEMA: &str = "kiana.symposium-contribution-proposal.v1";

/// Default token allowance reserved for one structured contribution.
pub const SYMPOSIUM_TOKENS_PER_CONTRIBUTION: u64 = 4_096;
/// Default wall-time ceiling for one bounded meeting (15 minutes).
pub const SYMPOSIUM_MAX_WALL_TIME_MS: u64 = 15 * 60 * 1_000;
/// Two consecutive silent rounds escalate without a decision.
pub const SYMPOSIUM_MAX_STALL_ROUNDS: u32 = 2;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymposiumRunBudget {
    pub schema: String,
    pub max_rounds: u32,
    pub max_messages: u32,
    pub max_tokens: u64,
    pub max_wall_time_ms: u64,
    pub max_stall_rounds: u32,
    pub rounds_used: u32,
    pub messages_used: u32,
    pub tokens_used: u64,
    #[serde(default)]
    pub wall_time_used_ms: u64,
    pub stall_rounds: u32,
    pub cancelled: bool,
}

impl SymposiumRunBudget {
    pub fn new(
        max_rounds: u32,
        max_messages: u32,
        max_tokens: u64,
        max_wall_time_ms: u64,
        max_stall_rounds: u32,
    ) -> Result<Self, &'static str> {
        let budget = Self {
            schema: SYMPOSIUM_RUN_BUDGET_SCHEMA.to_owned(),
            max_rounds,
            max_messages,
            max_tokens,
            max_wall_time_ms,
            max_stall_rounds,
            rounds_used: 0,
            messages_used: 0,
            tokens_used: 0,
            wall_time_used_ms: 0,
            stall_rounds: 0,
            cancelled: false,
        };
        budget.validate()?;
        Ok(budget)
    }

    /// Budget for one deterministic schedule plus capacity for its closing decision.
    ///
    /// The schedule runs at most `max_rounds * attendees` speaker turns. The extra round and
    /// contribution are explicit admission headroom for the closing decision, so a meeting that
    /// consumes exactly its schedule is still allowed to close; anything beyond the schedule is
    /// refused instead of silently approving.
    pub fn for_schedule(max_rounds: u32, attendees: usize) -> Result<Self, &'static str> {
        if attendees == 0 {
            return Err("symposium_run_budget_invalid");
        }
        let attendees = u32::try_from(attendees).map_err(|_| "symposium_run_budget_invalid")?;
        let schedule_messages = max_rounds
            .checked_mul(attendees)
            .ok_or("symposium_run_budget_invalid")?;
        let capacity_messages = schedule_messages
            .checked_add(1)
            .ok_or("symposium_run_budget_invalid")?;
        let capacity_rounds = max_rounds
            .checked_add(1)
            .ok_or("symposium_run_budget_invalid")?;
        let max_tokens =
            u64::from(capacity_messages).saturating_mul(SYMPOSIUM_TOKENS_PER_CONTRIBUTION);
        Self::new(
            capacity_rounds,
            capacity_messages,
            max_tokens,
            SYMPOSIUM_MAX_WALL_TIME_MS,
            SYMPOSIUM_MAX_STALL_ROUNDS,
        )
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SYMPOSIUM_RUN_BUDGET_SCHEMA
            || self.max_rounds == 0
            || self.max_messages == 0
            || self.max_tokens == 0
            || self.max_wall_time_ms == 0
            || self.max_stall_rounds == 0
            || self.rounds_used > self.max_rounds
            || self.messages_used > self.max_messages
            || self.tokens_used > self.max_tokens
            || self.wall_time_used_ms > self.max_wall_time_ms
            || self.stall_rounds > self.max_stall_rounds
        {
            return Err("symposium_run_budget_invalid");
        }
        Ok(())
    }

    pub fn record_round(&mut self, made_progress: bool) -> Result<(), &'static str> {
        self.validate()?;
        if self.cancelled {
            return Err("symposium_cancelled");
        }
        if self.rounds_used >= self.max_rounds {
            return Err("symposium_round_budget_exhausted");
        }
        // Check before mutating: a refused stall round must leave a valid ledger that still
        // reports `symposium_stall_limit_exhausted` from `limit_reason`/`decision_allowed`.
        if !made_progress && self.stall_rounds >= self.max_stall_rounds {
            return Err("symposium_stall_limit_exhausted");
        }
        self.rounds_used += 1;
        if made_progress {
            self.stall_rounds = 0;
        } else {
            self.stall_rounds += 1;
        }
        Ok(())
    }

    pub fn record_message(&mut self, token_estimate: u64) -> Result<(), &'static str> {
        self.validate()?;
        if self.cancelled {
            return Err("symposium_cancelled");
        }
        if self.messages_used >= self.max_messages {
            return Err("symposium_message_budget_exhausted");
        }
        if token_estimate > self.max_tokens.saturating_sub(self.tokens_used) {
            return Err("symposium_token_budget_exhausted");
        }
        self.messages_used += 1;
        self.tokens_used += token_estimate;
        Ok(())
    }

    /// Record observed wall time. Reaching the ceiling is a stopping condition, not a decision.
    pub fn record_elapsed(&mut self, elapsed_ms: u64) -> Result<(), &'static str> {
        self.validate()?;
        if self.cancelled {
            return Err("symposium_cancelled");
        }
        self.wall_time_used_ms = self.wall_time_used_ms.max(elapsed_ms);
        if self.wall_time_used_ms >= self.max_wall_time_ms {
            return Err("symposium_wall_time_exhausted");
        }
        Ok(())
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// The concrete exhausted limit, if any. Core reports this stable code instead of approving.
    pub fn limit_reason(&self) -> Result<(), &'static str> {
        self.validate()?;
        if self.cancelled {
            return Err("symposium_cancelled");
        }
        if self.rounds_used >= self.max_rounds {
            return Err("symposium_round_budget_exhausted");
        }
        if self.messages_used >= self.max_messages {
            return Err("symposium_message_budget_exhausted");
        }
        if self.tokens_used >= self.max_tokens {
            return Err("symposium_token_budget_exhausted");
        }
        if self.wall_time_used_ms >= self.max_wall_time_ms {
            return Err("symposium_wall_time_exhausted");
        }
        if self.stall_rounds >= self.max_stall_rounds {
            return Err("symposium_stall_limit_exhausted");
        }
        Ok(())
    }

    /// Fail-closed admission check. Any exhausted limit or cancellation refuses the decision.
    pub fn decision_allowed(&self) -> Result<(), &'static str> {
        self.limit_reason()
    }
}

/// The only structured shape a symposium speaker turn may return.
///
/// `deny_unknown_fields` is deliberate: a model cannot smuggle an assignment, approval flag or
/// extra decision authority through an unrecognised field. Core supplies the assignment and
/// contribution identity from the deterministic schedule, never from the payload.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymposiumContributionProposal {
    pub kind: String,
    pub text: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
}

impl SymposiumContributionProposal {
    pub fn contribution_kind(&self) -> Result<SymposiumContributionKind, &'static str> {
        match self.kind.trim() {
            "claim" => Ok(SymposiumContributionKind::Claim),
            "vote" => Ok(SymposiumContributionKind::Vote),
            "draft" => Ok(SymposiumContributionKind::Draft),
            _ => Err("symposium_contribution_kind_invalid"),
        }
    }

    pub fn visibility(&self) -> Result<SymposiumVisibility, &'static str> {
        match self.visibility.as_deref().map(str::trim) {
            None | Some("") | Some("blackboard") => Ok(SymposiumVisibility::Blackboard),
            Some("private_brief") => Ok(SymposiumVisibility::PrivateBrief),
            Some(_) => Err("symposium_contribution_visibility_invalid"),
        }
    }
}

/// Parse a speaker turn's structured output.
///
/// Accepted input is one proposal object, a bare array of proposals, or `{"contributions": [...]}`.
/// An empty list is a valid abstention: it makes no progress and never fabricates a contribution.
/// Any other shape is refused with `symposium_contribution_invalid`.
pub fn parse_symposium_contributions(
    value: &Value,
) -> Result<Vec<SymposiumContributionProposal>, &'static str> {
    let items: Vec<&Value> = match value {
        Value::Array(items) => items.iter().collect(),
        Value::Object(object) => match object.get("contributions") {
            Some(Value::Array(items)) => items.iter().collect(),
            Some(_) => return Err("symposium_contribution_invalid"),
            None => vec![value],
        },
        _ => return Err("symposium_contribution_invalid"),
    };
    items
        .into_iter()
        .map(|item| {
            serde_json::from_value::<SymposiumContributionProposal>(item.clone())
                .map_err(|_| "symposium_contribution_invalid")
        })
        .collect()
}
