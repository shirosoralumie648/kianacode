//! BQ-24 read-only budget card, queue, overage reason and correction-approval source contract.
//!
//! Everything in this module is a **display projection**. It renders what the server already
//! computed in the BQ-20 ledger projection and returned through the BQ-23 `BillingQuery`
//! response. It deliberately contains:
//!
//! * no budget arithmetic — `remaining` is a server field carried verbatim, never recomputed from
//!   a limit and a spent total, so a local remainder can never drift from the authoritative ledger;
//! * no optimistic charges — a reservation that was never committed to the ledger has no
//!   server-side amount, so it cannot be shown as spent;
//! * no `unknown` elision — an unknown amount or freshness is rendered as the explicit `unknown`
//!   token, never rounded to `0` or dropped from the card;
//! * no identity widening — `actor_id`/`project_id` on the card are the **server-resolved**
//!   values echoed back from the response, and a caller-supplied override is rejected outright.
//!
//! The correction row is not an apply path. It names the existing versioned `cost.correction`
//! command and the approval that a submitter must already hold; this module never mints an
//! approval, never decides `approve`/`deny`, and never reaches the daemon, the committed fact
//! store, a reservation or a broker.
//!
//! Proof ceiling: `source`. This module formats already-computed facts and proves nothing about
//! live billing, real spend, a real provider or a durable store.

use kiana_domain::{
    json_digest, redact_text, scan_secret_sentinels, BillingQueryKind, BillingQueryResponse,
    BillingRollupTotals, BillingUnknownReason, Freshness, Money, ProjectId, SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Schema id for the renderable card itself.
pub const BUDGET_CARD_SCHEMA: &str = "kiana.budget-card.v1";
/// Schema id for the four-surface parity frame that wraps the card.
pub const BUDGET_CARD_PARITY_SCHEMA: &str = "kiana.budget-card-parity.v1";
/// Schema id for the correction approval card and its submitter intent.
pub const BUDGET_CORRECTION_CARD_SCHEMA: &str = "kiana.budget-correction-card.v1";
/// Schema id for the versioned command a surface emits instead of applying a correction itself.
pub const BUDGET_CORRECTION_INTENT_SCHEMA: &str = "kiana.budget-correction-intent.v1";

pub const BUDGET_CARD_VERSION: kiana_domain::SchemaVersion = kiana_domain::SchemaVersion::new(1, 0);

/// Hard bounds. A card is a one-screen read model; an unbounded list is a denial-of-service on the
/// terminal, not information.
pub const MAX_BUDGET_CARD_QUEUE_ROWS: usize = 64;
pub const MAX_BUDGET_CARD_TEXT: usize = 256;
pub const MAX_BUDGET_CARD_EVIDENCE_REFS: usize = 32;

/// The single literal a surface must print wherever a value is genuinely unknown. Rounding an
/// unknown to `0` is a lie that reads as "you have budget left".
pub const UNKNOWN_TOKEN: &str = "unknown";

/// The four product entrypoints. Desktop renders the same frame as the other three; it is not an
/// independent source of truth.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetSurface {
    Cli,
    Web,
    Workbench,
    Desktop,
}

impl BudgetSurface {
    pub const ALL: [BudgetSurface; 4] = [
        BudgetSurface::Cli,
        BudgetSurface::Web,
        BudgetSurface::Workbench,
        BudgetSurface::Desktop,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Web => "web",
            Self::Workbench => "workbench",
            Self::Desktop => "desktop",
        }
    }
}

/// Why the current request is over budget. This is a display of an admission decision the server
/// already made; the card never re-derives it and never invents a reason it was not given.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum OverageReason {
    /// The cost is not known yet, so the request cannot be admitted against a hard limit.
    UnknownCost,
    /// The project/window budget is exhausted.
    BudgetExhausted,
    /// The provider queue is at its limit.
    QueueFull,
    /// The usage window has no remaining allowance.
    QuotaWindowExhausted,
    /// The committed spend already exceeds the limit; a correction is the only way back.
    OverCommitted,
    /// The prior attempt's result is unknown and needs reconciliation first.
    ReconciliationRequired,
    /// The request is not over budget.
    NotOverage,
}

impl OverageReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownCost => "unknown_cost",
            Self::BudgetExhausted => "budget_exhausted",
            Self::QueueFull => "queue_full",
            Self::QuotaWindowExhausted => "quota_window_exhausted",
            Self::OverCommitted => "over_committed",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::NotOverage => "not_overage",
        }
    }
}

/// What the server said the current position is. `Unknown` is a first-class state, not an error
/// and not a zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BudgetState {
    WithinBudget,
    AtLimit,
    OverLimit,
    Unknown,
}

impl BudgetState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WithinBudget => "within_budget",
            Self::AtLimit => "at_limit",
            Self::OverLimit => "over_limit",
            Self::Unknown => UNKNOWN_TOKEN,
        }
    }
}

/// One amount as the server computed it. `None` is preserved as `None` end to end — a card with
/// `None` renders `unknown`, never `0` and never a subtraction.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetAmount {
    pub currency: String,
    pub micros: i128,
}

impl BudgetAmount {
    pub fn new(currency: impl Into<String>, micros: i128) -> Result<Self, String> {
        let currency = currency.into();
        // Reuse the domain Money validator so the card cannot invent a currency the ledger
        // rejects.
        Money::new(currency.clone(), micros)?;
        Ok(Self { currency, micros })
    }

    /// Project a server amount. `None` stays `None`; this function has no fallback branch.
    pub fn from_money(money: Option<&Money>) -> Option<Self> {
        money.map(|value| Self {
            currency: value.currency.clone(),
            micros: value.micros,
        })
    }

    /// The exact text a surface prints. An absent amount prints the shared `unknown` token.
    pub fn render(&self) -> String {
        format!("{} {}", self.micros, self.currency)
    }
}

/// One queue row as the server reported it. The card does not compute depth, does not predict a
/// wait and does not re-rank tickets.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetQueueRow {
    pub ticket: u64,
    pub session_ref: String,
    pub requested_at_unix_ms: u64,
    /// Server-provided wait estimate. `None` renders as `unknown`; the card never derives one.
    pub estimated_wait_ms: Option<u64>,
}

impl BudgetQueueRow {
    pub fn new(
        ticket: u64,
        session_ref: impl Into<String>,
        requested_at_unix_ms: u64,
        estimated_wait_ms: Option<u64>,
    ) -> Result<Self, String> {
        let row = Self {
            ticket,
            session_ref: session_ref.into(),
            requested_at_unix_ms,
            estimated_wait_ms,
        };
        row.validate()?;
        Ok(row)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.ticket == 0
            || self.requested_at_unix_ms == 0
            || !bounded_text(&self.session_ref, MAX_BUDGET_CARD_TEXT)
        {
            return Err("budget_queue_row_invalid".to_owned());
        }
        Ok(())
    }
}

/// The read-only budget card. Every field is server-computed; nothing here is a local estimate.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetCard {
    pub schema: String,
    pub version: kiana_domain::SchemaVersion,
    /// Server-resolved project. A caller may read it, never set it.
    pub project_id: ProjectId,
    /// Server-resolved actor. A caller may read it, never set it.
    pub actor_id: String,
    /// The BQ-23 response digest this card was rendered from. Binding the card to the response is
    /// what makes four-entrypoint parity checkable instead of aspirational.
    pub response_digest: String,
    /// The BQ-20 projection digest at render time.
    pub projection_digest: String,
    pub source_cursor: u64,
    pub freshness: Freshness,
    pub state: BudgetState,
    pub overage_reason: OverageReason,
    /// Committed spend, as folded by the server. `None` means the server did not know it.
    pub spent: Option<BudgetAmount>,
    /// Remaining allowance. **Carried verbatim from the server.** There is deliberately no
    /// `remaining()` helper on this type: a local `limit - spent` is exactly the drift this card
    /// exists to prevent.
    pub remaining: Option<BudgetAmount>,
    /// Committed limit, `None` when the server did not state one.
    pub limit: Option<BudgetAmount>,
    /// Counters the server computed. `None` stays `None`; these are never defaulted to `0`.
    pub pending_reservations: Option<u64>,
    pub queue_depth: Option<u64>,
    pub queue_limit: Option<u64>,
    pub unknown_cost_count: Option<u64>,
    pub queue: Vec<BudgetQueueRow>,
    pub card_digest: String,
}

impl BudgetCard {
    /// Render the card from a BQ-23 response. This is the only constructor, and it reads the
    /// server's numbers rather than computing any.
    #[allow(clippy::too_many_arguments)]
    pub fn from_response(
        response: &BillingQueryResponse,
        actor_id: impl Into<String>,
        spent: Option<BudgetAmount>,
        remaining: Option<BudgetAmount>,
        limit: Option<BudgetAmount>,
        state: BudgetState,
        overage_reason: OverageReason,
        pending_reservations: Option<u64>,
        queue_depth: Option<u64>,
        queue_limit: Option<u64>,
        unknown_cost_count: Option<u64>,
        queue: Vec<BudgetQueueRow>,
    ) -> Result<Self, String> {
        response
            .validate()
            .map_err(|_| "budget_card_response_invalid".to_owned())?;
        if queue.len() > MAX_BUDGET_CARD_QUEUE_ROWS {
            return Err("budget_card_queue_too_long".to_owned());
        }
        if response.kind != BillingQueryKind::BudgetSummary {
            return Err("budget_card_kind_invalid".to_owned());
        }
        let mut card = Self {
            schema: BUDGET_CARD_SCHEMA.to_owned(),
            version: BUDGET_CARD_VERSION,
            project_id: response.project_id,
            actor_id: actor_id.into(),
            response_digest: response.response_digest.clone(),
            projection_digest: response.projection_digest.clone(),
            source_cursor: response.source_cursor,
            freshness: response.freshness,
            state,
            overage_reason,
            spent,
            remaining,
            limit,
            pending_reservations,
            queue_depth,
            queue_limit,
            unknown_cost_count,
            queue,
            card_digest: String::new(),
        };
        card.card_digest = card.digest();
        card.validate()?;
        Ok(card)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BUDGET_CARD_SCHEMA
            || !self.version.is_compatible_with(&BUDGET_CARD_VERSION)
            || self.project_id.as_uuid().is_nil()
            || !bounded_text(&self.actor_id, MAX_BUDGET_CARD_TEXT)
            || !valid_digest(&self.response_digest)
            || !valid_digest(&self.projection_digest)
            || !valid_digest(&self.card_digest)
            || self.source_cursor == 0
            || self.queue.len() > MAX_BUDGET_CARD_QUEUE_ROWS
        {
            return Err("budget_card_header_invalid".to_owned());
        }
        for amount in [&self.spent, &self.remaining, &self.limit]
            .into_iter()
            .flatten()
        {
            BudgetAmount::new(amount.currency.clone(), amount.micros)?;
        }
        // A currency that the server did not state cannot be invented on a partially unknown card.
        if self.remaining.is_some() != self.limit.is_some() {
            return Err("budget_card_remaining_limit_binding_invalid".to_owned());
        }
        if self.state == BudgetState::OverLimit && self.overage_reason == OverageReason::NotOverage
        {
            return Err("budget_card_overage_reason_missing".to_owned());
        }
        if self.state != BudgetState::OverLimit && self.overage_reason != OverageReason::NotOverage
        {
            return Err("budget_card_overage_reason_unexpected".to_owned());
        }
        // The headline rejection: a card whose own state is `unknown` may not also print a definite
        // remainder. That pairing is how an unknown budget gets read as a real number.
        if self.state == BudgetState::Unknown && self.remaining.is_some() {
            return Err("budget_card_unknown_state_numeric".to_owned());
        }
        // An optimistic charge that was never committed has no server amount. Reporting pending
        // work next to a non-`WithinBudget` state would imply the pending amount is already inside
        // `spent`; the card must say "pending" instead of folding it in.
        if self.pending_reservations.is_some_and(|pending| pending > 0)
            && self.overage_reason == OverageReason::NotOverage
            && self.state != BudgetState::WithinBudget
        {
            return Err("budget_card_pending_state_invalid".to_owned());
        }
        if self
            .queue_depth
            .is_some_and(|depth| depth < self.queue.len() as u64)
        {
            return Err("budget_card_queue_depth_invalid".to_owned());
        }
        if self
            .queue_limit
            .is_some_and(|queue_cap| self.queue_depth.is_some_and(|depth| depth > queue_cap))
        {
            return Err("budget_card_queue_over_limit".to_owned());
        }
        let mut tickets = BTreeSet::new();
        for row in &self.queue {
            row.validate()?;
            if !tickets.insert(row.ticket) {
                return Err("budget_card_queue_duplicate_ticket".to_owned());
            }
        }
        if self.card_digest != self.digest() {
            return Err("budget_card_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "project_id": self.project_id,
            "actor_id": self.actor_id,
            "response_digest": self.response_digest,
            "projection_digest": self.projection_digest,
            "source_cursor": self.source_cursor,
            "freshness": self.freshness,
            "state": self.state,
            "overage_reason": self.overage_reason,
            "spent": self.spent,
            "remaining": self.remaining,
            "limit": self.limit,
            "pending_reservations": self.pending_reservations,
            "queue_depth": self.queue_depth,
            "queue_limit": self.queue_limit,
            "unknown_cost_count": self.unknown_cost_count,
            "queue": self.queue,
        }))
    }

    /// Plain text for CLI/Workbench/Desktop. One line per field, one `unknown` token, no
    /// arithmetic.
    pub fn render_text(&self) -> String {
        let mut lines = vec![
            format!("state: {}", self.state.as_str()),
            format!("overage_reason: {}", self.overage_reason.as_str()),
            format!("freshness: {}", freshness_text(self.freshness)),
            format!("limit: {}", render_amount(&self.limit)),
            format!("spent: {}", render_amount(&self.spent)),
            format!("remaining: {}", render_amount(&self.remaining)),
            format!(
                "pending_reservations: {}",
                render_count(self.pending_reservations)
            ),
            format!("queue_depth: {}", render_count(self.queue_depth)),
            format!("queue_limit: {}", render_count(self.queue_limit)),
            format!(
                "unknown_cost_count: {}",
                render_count(self.unknown_cost_count)
            ),
        ];
        for row in &self.queue {
            lines.push(format!(
                "queue[{}]: session={} requested_at={} wait_ms={}",
                row.ticket,
                row.session_ref,
                row.requested_at_unix_ms,
                render_count(row.estimated_wait_ms)
            ));
        }
        lines.push(format!("projection: {}", self.projection_digest));
        lines.join("\n")
    }

    /// Verify the card still describes the response it claims to render. A card carried across a
    /// request, or hand-edited to a friendlier number, fails here.
    pub fn validate_against(&self, response: &BillingQueryResponse) -> Result<(), String> {
        response
            .validate()
            .map_err(|_| "budget_card_response_invalid".to_owned())?;
        if self.project_id != response.project_id
            || self.response_digest != response.response_digest
            || self.projection_digest != response.projection_digest
            || self.source_cursor != response.source_cursor
            || self.freshness != response.freshness
        {
            return Err("budget_card_response_binding_invalid".to_owned());
        }
        self.validate()
    }
}

/// The JSON view a Web surface hydrates. It is a straight projection of the same fields; the
/// Web renderer must not add a computed remainder either.
pub fn budget_card_json(card: &BudgetCard) -> Result<serde_json::Value, String> {
    card.validate()?;
    serde_json::to_value(card).map_err(|_| "budget_card_encode_failed".to_owned())
}

/// A caller-supplied identity override. It exists to be rejected: a surface that lets `--actor`
/// or `--project` reach the card has widened authority, because the server already resolved both.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetCardIdentityOverride {
    pub actor_id: Option<String>,
    pub project_id: Option<ProjectId>,
}

impl BudgetCardIdentityOverride {
    /// Reject any caller-supplied identity. The card's actor/project are the server's.
    pub fn reject(&self) -> Result<(), String> {
        if self.actor_id.is_some() || self.project_id.is_some() {
            return Err("budget_card_identity_override_rejected".to_owned());
        }
        Ok(())
    }
}

/// What a submitter may say when asking for a correction. It names the existing versioned
/// `cost.correction` command and carries the approval id the submitter already holds; it is not
/// an approval and does not apply anything.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetCorrectionCard {
    pub schema: String,
    pub version: kiana_domain::SchemaVersion,
    /// The correction the card is about.
    pub correction_id: kiana_domain::CostCorrectionId,
    /// The cost ledger entry being corrected.
    pub target_entry_id: kiana_domain::LedgerEntryId,
    /// Digest of the target entry, bound server-side.
    pub target_entry_digest: String,
    /// The versioned command a surface must emit.
    pub command: String,
    /// The approval already held for that command. A card without one shows `pending_approval`
    /// instead of implying the correction is approved.
    pub approval_id: Option<kiana_domain::ApprovalId>,
    pub command_digest: String,
    /// Bounded, already-redacted evidence references. Raw provider text never lands here.
    pub evidence_refs: Vec<String>,
    pub card_digest: String,
}

impl BudgetCorrectionCard {
    pub fn new(
        correction_id: kiana_domain::CostCorrectionId,
        target_entry_id: kiana_domain::LedgerEntryId,
        target_entry_digest: impl Into<String>,
        command: impl Into<String>,
        approval_id: Option<kiana_domain::ApprovalId>,
        command_digest: impl Into<String>,
        evidence_refs: Vec<String>,
    ) -> Result<Self, String> {
        let mut card = Self {
            schema: BUDGET_CORRECTION_CARD_SCHEMA.to_owned(),
            version: BUDGET_CARD_VERSION,
            correction_id,
            target_entry_id,
            target_entry_digest: target_entry_digest.into(),
            command: command.into(),
            approval_id,
            command_digest: command_digest.into(),
            evidence_refs,
            card_digest: String::new(),
        };
        card.card_digest = card.digest();
        card.validate()?;
        Ok(card)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BUDGET_CORRECTION_CARD_SCHEMA
            || !self.version.is_compatible_with(&BUDGET_CARD_VERSION)
            || self.correction_id.as_uuid().is_nil()
            || self.target_entry_id.as_uuid().is_nil()
            || !valid_digest(&self.target_entry_digest)
            || !valid_digest(&self.command_digest)
            || !valid_digest(&self.card_digest)
            || self.command != kiana_domain::COST_CORRECTION_COMMAND
            || self.approval_id.is_some_and(|id| id.as_uuid().is_nil())
            || self.evidence_refs.len() > MAX_BUDGET_CARD_EVIDENCE_REFS
        {
            return Err("budget_correction_card_invalid".to_owned());
        }
        for reference in &self.evidence_refs {
            // Deny-first: a surface must not be the place a secret first becomes visible. The
            // value is redacted before it is scanned, and a raw shape is rejected outright.
            let redacted = redact_text(reference);
            if redacted != *reference {
                return Err("budget_correction_card_evidence_unredacted".to_owned());
            }
            scan_secret_sentinels(SecretScanChannel::Event, &redacted)
                .map_err(|_| "budget_correction_card_evidence_secret".to_owned())?;
            if !bounded_text(reference, MAX_BUDGET_CARD_TEXT) {
                return Err("budget_correction_card_evidence_invalid".to_owned());
            }
        }
        if self.card_digest != self.digest() {
            return Err("budget_correction_card_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "correction_id": self.correction_id,
            "target_entry_id": self.target_entry_id,
            "target_entry_digest": self.target_entry_digest,
            "command": self.command,
            "approval_id": self.approval_id,
            "command_digest": self.command_digest,
            "evidence_refs": self.evidence_refs,
        }))
    }

    /// The text a surface prints for the correction row.
    pub fn render_text(&self) -> String {
        format!(
            "correction: {}\ntarget_entry: {}\ncommand: {}\napproval: {}\nstate: {}",
            self.correction_id,
            self.target_entry_id,
            self.command,
            self.approval_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "pending_approval".to_owned()),
            if self.approval_id.is_some() {
                "approval_present"
            } else {
                "pending_approval"
            },
        )
    }

    /// Bind the card to the versioned command it names. The command is re-checked against the
    /// target ledger entry so a card cannot point at a command whose target drifted, and the card's
    /// approval id must be the one the command actually carries.
    pub fn validate_against_command(
        &self,
        command: &kiana_domain::CostCorrectionCommand,
        target: &kiana_domain::CostLedgerEntry,
    ) -> Result<(), String> {
        self.validate()?;
        command
            .validate_draft_against(target)
            .map_err(|_| "budget_correction_command_invalid".to_owned())?;
        if self.correction_id != command.correction_id
            || self.target_entry_id != command.target_entry_id
            || self.target_entry_digest != target.entry_digest
            || self.target_entry_digest != command.target_entry_digest
            || self.command_digest != command.command_digest
            || self.approval_id != command.approval.as_ref().map(|a| a.approval_id)
        {
            return Err("budget_correction_command_binding_mismatch".to_owned());
        }
        Ok(())
    }
}

/// A submitter's typed intent to request a correction. It carries no `approve`/`deny` decision:
/// the decision belongs to the existing approval path, not to a surface.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetCorrectionIntent {
    pub schema: String,
    pub version: kiana_domain::SchemaVersion,
    pub project_id: ProjectId,
    /// Server-resolved actor. A surface echoes it; it does not choose it.
    pub actor_id: String,
    pub correction_id: kiana_domain::CostCorrectionId,
    pub command: String,
    pub reason: String,
    pub intent_digest: String,
}

impl BudgetCorrectionIntent {
    pub fn new(
        project_id: ProjectId,
        actor_id: impl Into<String>,
        correction_id: kiana_domain::CostCorrectionId,
        command: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let mut intent = Self {
            schema: BUDGET_CORRECTION_INTENT_SCHEMA.to_owned(),
            version: BUDGET_CARD_VERSION,
            project_id,
            actor_id: actor_id.into(),
            correction_id,
            command: command.into(),
            reason: reason.into(),
            intent_digest: String::new(),
        };
        intent.intent_digest = intent.digest();
        intent.validate()?;
        Ok(intent)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BUDGET_CORRECTION_INTENT_SCHEMA
            || !self.version.is_compatible_with(&BUDGET_CARD_VERSION)
            || self.project_id.as_uuid().is_nil()
            || self.correction_id.as_uuid().is_nil()
            || !bounded_text(&self.actor_id, MAX_BUDGET_CARD_TEXT)
            || !bounded_text(&self.reason, MAX_BUDGET_CARD_TEXT)
            || self.command != kiana_domain::COST_CORRECTION_COMMAND
            || !valid_digest(&self.intent_digest)
            || self.intent_digest != self.digest()
        {
            return Err("budget_correction_intent_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "project_id": self.project_id,
            "actor_id": self.actor_id,
            "correction_id": self.correction_id,
            "command": self.command,
            "reason": self.reason,
        }))
    }
}

/// One entrypoint's rendering of the same card, bound to the card it rendered.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetCardFrame {
    pub schema: String,
    pub surface: BudgetSurface,
    pub card_digest: String,
    pub response_digest: String,
    pub state: BudgetState,
    pub overage_reason: OverageReason,
    /// Exactly what this surface printed. Two surfaces that disagree cannot both be honest, so the
    /// parity check compares these rather than trusting a shared label.
    pub rendered_text: String,
    pub frame_digest: String,
}

impl BudgetCardFrame {
    pub fn new(
        surface: BudgetSurface,
        card: &BudgetCard,
        rendered_text: impl Into<String>,
    ) -> Result<Self, String> {
        card.validate()?;
        let mut frame = Self {
            schema: BUDGET_CARD_PARITY_SCHEMA.to_owned(),
            surface,
            card_digest: card.card_digest.clone(),
            response_digest: card.response_digest.clone(),
            state: card.state,
            overage_reason: card.overage_reason,
            rendered_text: rendered_text.into(),
            frame_digest: String::new(),
        };
        frame.frame_digest = frame.digest();
        frame.validate()?;
        Ok(frame)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BUDGET_CARD_PARITY_SCHEMA
            || !valid_digest(&self.card_digest)
            || !valid_digest(&self.response_digest)
            || !valid_digest(&self.frame_digest)
            || self.rendered_text.trim().is_empty()
        {
            return Err("budget_card_frame_invalid".to_owned());
        }
        if self.frame_digest != self.digest() {
            return Err("budget_card_frame_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "surface": self.surface,
            "card_digest": self.card_digest,
            "response_digest": self.response_digest,
            "state": self.state,
            "overage_reason": self.overage_reason,
            "rendered_text": self.rendered_text,
        }))
    }
}

/// All four entrypoints' frames for one card. This is the "同一 Run 在四入口显示一致" evidence: a
/// missing surface or a differing render is refused rather than tolerated.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetCardParity {
    pub schema: String,
    pub card_digest: String,
    pub response_digest: String,
    pub frames: Vec<BudgetCardFrame>,
    pub parity_digest: String,
}

impl BudgetCardParity {
    pub fn from_card(card: &BudgetCard, frames: Vec<BudgetCardFrame>) -> Result<Self, String> {
        card.validate()?;
        if frames.len() != BudgetSurface::ALL.len() {
            return Err("budget_card_parity_surface_count".to_owned());
        }
        let mut seen = BTreeSet::new();
        for frame in &frames {
            frame.validate()?;
            if !seen.insert(frame.surface) {
                return Err("budget_card_parity_duplicate_surface".to_owned());
            }
        }
        let mut parity = Self {
            schema: BUDGET_CARD_PARITY_SCHEMA.to_owned(),
            card_digest: card.card_digest.clone(),
            response_digest: card.response_digest.clone(),
            frames,
            parity_digest: String::new(),
        };
        parity.parity_digest = parity.digest();
        parity.validate_against(card)?;
        Ok(parity)
    }

    pub fn validate_against(&self, card: &BudgetCard) -> Result<(), String> {
        card.validate()?;
        if self.schema != BUDGET_CARD_PARITY_SCHEMA
            || self.card_digest != card.card_digest
            || self.response_digest != card.response_digest
            || self.frames.len() != BudgetSurface::ALL.len()
        {
            return Err("budget_card_parity_header_invalid".to_owned());
        }
        let mut seen = BTreeSet::new();
        for frame in &self.frames {
            frame.validate()?;
            if frame.card_digest != card.card_digest
                || frame.response_digest != card.response_digest
                || frame.state != card.state
                || frame.overage_reason != card.overage_reason
            {
                return Err("budget_card_parity_frame_binding_invalid".to_owned());
            }
            if !seen.insert(frame.surface) {
                return Err("budget_card_parity_duplicate_surface".to_owned());
            }
            // The render itself must agree, not just the labels. Otherwise a surface could print a
            // fabricated remainder while reporting the correct state.
            if frame.rendered_text != card.render_text() {
                return Err("budget_card_parity_render_mismatch".to_owned());
            }
        }
        for surface in BudgetSurface::ALL {
            if !seen.contains(&surface) {
                return Err("budget_card_parity_surface_missing".to_owned());
            }
        }
        if self.parity_digest != self.digest() {
            return Err("budget_card_parity_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "card_digest": self.card_digest,
            "response_digest": self.response_digest,
            "frames": self.frames,
        }))
    }
}

/// Build a parity set from one card, rendering the card's own text for every surface. A real
/// surface substitutes its own presenter; this is the reference the presenters must match.
pub fn budget_card_parity(card: &BudgetCard) -> Result<BudgetCardParity, String> {
    let text = card.render_text();
    let frames = BudgetSurface::ALL
        .iter()
        .map(|surface| BudgetCardFrame::new(*surface, card, text.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    BudgetCardParity::from_card(card, frames)
}

/// Derive the card's committed-spend amount from a server rollup. The rollup is the BQ-20 fold;
/// this reads it and does not add an optimistic reservation into it. A rollup that mixes
/// currencies cannot be summed, so it fails closed rather than picking one.
pub fn committed_spend(rollup: &BillingRollupTotals) -> Result<Option<BudgetAmount>, String> {
    rollup
        .validate()
        .map_err(|error| format!("budget_card_rollup_invalid:{error}"))?;
    let mut total: Option<Money> = None;
    for amount in [
        rollup.measured.as_ref(),
        rollup.estimated.as_ref(),
        rollup.correction_measured.as_ref(),
        rollup.correction_estimated.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        total = Some(match total {
            Some(existing) => existing.checked_add(amount)?,
            None => amount.clone(),
        });
    }
    Ok(BudgetAmount::from_money(total.as_ref()))
}

/// Map a server unknown reason onto the overage vocabulary. A reason the card does not know is
/// still shown as `unknown`, never dropped and never silently re-labelled.
pub fn overage_reason_for_unknown(reason: Option<BillingUnknownReason>) -> OverageReason {
    match reason {
        Some(BillingUnknownReason::RateCardMissing) => OverageReason::UnknownCost,
        Some(BillingUnknownReason::ReconciliationRequired)
        | Some(BillingUnknownReason::ResultUnknown) => OverageReason::ReconciliationRequired,
        Some(BillingUnknownReason::ProviderUnreported) | Some(BillingUnknownReason::Partial) => {
            OverageReason::UnknownCost
        }
        Some(_) => OverageReason::UnknownCost,
        None => OverageReason::NotOverage,
    }
}

pub fn validate_budget_card(card: &BudgetCard) -> Result<(), &'static str> {
    card.validate().map_err(|_| "budget_card_invalid")
}

pub fn validate_budget_card_parity(
    parity: &BudgetCardParity,
    card: &BudgetCard,
) -> Result<(), &'static str> {
    parity
        .validate_against(card)
        .map_err(|_| "budget_card_parity_invalid")
}

fn render_amount(amount: &Option<BudgetAmount>) -> String {
    amount
        .as_ref()
        .map_or(UNKNOWN_TOKEN.to_owned(), BudgetAmount::render)
}

fn render_count(count: Option<u64>) -> String {
    count.map_or_else(|| UNKNOWN_TOKEN.to_owned(), |value| value.to_string())
}

fn freshness_text(freshness: Freshness) -> &'static str {
    match freshness {
        Freshness::Current => "current",
        Freshness::Stale => "stale",
        Freshness::Unknown => UNKNOWN_TOKEN,
    }
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn bounded_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains(['\0', '\r', '\n'])
}
