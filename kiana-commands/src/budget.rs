//! BQ-24 `/budget` CLI surface: a read-only card, queue, overage reason and correction intent.
//!
//! The command does not compute anything about money. It names a versioned command, hands the
//! request to the existing ControlPlane path, and renders whatever the server returned. A caller
//! that supplies its own actor or project is rejected: the server resolved both.
//!
//! Proof ceiling: `source`. Nothing here proves live billing, a durable ledger or a real provider
//! charge; the card is only as honest as the response the daemon hands back.

use crate::billing_card::{
    committed_spend, overage_reason_for_unknown, BudgetAmount, BudgetCard, BudgetCorrectionIntent,
    BudgetQueueRow, BudgetState, OverageReason, UNKNOWN_TOKEN,
};
use crate::types::{Command, CommandContext, CommandResult, CommandRoute, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;
use kiana_domain::{
    BillingQueryKind, BillingQueryRequest, BillingQueryResponse, BillingUnknownReason, Freshness,
    ProjectId, BILLING_QUERY_MAX_LIMIT, BILLING_QUERY_SCHEMA,
};

/// Versioned command name the surface emits. It never resolves billing itself.
pub const BUDGET_CARD_COMMAND: &str = "budget.query.v1";
/// Versioned command a correction submitter emits. It routes into the existing approval path.
pub const BUDGET_CORRECTION_COMMAND: &str = kiana_domain::COST_CORRECTION_COMMAND;
/// Argument schema sent with the card command. Mirrors the response schema so a surface cannot
/// ask for a shape the server does not produce.
pub const BUDGET_CARD_QUERY_SCHEMA: &str = "kiana.budget-card-query.v1";

const DEFAULT_LIMIT: u16 = 50;

pub struct BudgetCommand;

#[async_trait]
impl Command for BudgetCommand {
    fn name(&self) -> &str {
        "budget"
    }

    fn description(&self) -> &str {
        "Show the read-only budget card, queue and overage reason"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    fn route(&self, context: &CommandContext) -> anyhow::Result<CommandRoute> {
        Ok(match parse_budget_route(&context.args)? {
            BudgetRoute::Local => CommandRoute::Local,
            BudgetRoute::ControlPlane { name, arguments } => {
                CommandRoute::ControlPlane { name, arguments }
            }
        })
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        match parse_budget_route(&context.args)? {
            BudgetRoute::Local => Ok(CommandResult::text(usage())),
            BudgetRoute::ControlPlane { .. } => Err(anyhow!("command_requires_control_plane")),
        }
    }
}

enum BudgetRoute {
    Local,
    ControlPlane {
        name: String,
        arguments: serde_json::Value,
    },
}

fn parse_budget_route(args: &str) -> anyhow::Result<BudgetRoute> {
    let (subcommand, rest) = split_word(args.trim());
    match subcommand.unwrap_or("card") {
        "card" => budget_query_route(rest, BillingQueryKind::BudgetSummary),
        "queue" => budget_query_route(rest, BillingQueryKind::ReconciliationInbox),
        "overage" => budget_query_route(rest, BillingQueryKind::UsageBreakdown),
        "correction" => correction_route(rest),
        "help" | "--help" | "-h" | "" => Ok(BudgetRoute::Local),
        other => Err(anyhow!("unknown budget command '{other}'\n\n{}", usage())),
    }
}

fn budget_query_route(args: Option<&str>, kind: BillingQueryKind) -> anyhow::Result<BudgetRoute> {
    let mut limit = DEFAULT_LIMIT;
    let mut parts = args.unwrap_or("").split_whitespace();
    while let Some(arg) = parts.next() {
        match arg {
            "--json" => {}
            "--limit" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--limit requires a positive integer"))?;
                limit = parse_limit(value)?;
            }
            _ if arg.starts_with("--limit=") => {
                limit = parse_limit(arg.trim_start_matches("--limit="))?
            }
            "help" | "--help" | "-h" => return Ok(BudgetRoute::Local),
            // An identity flag is refused, not dropped. Silently ignoring `--actor` would leave the
            // user believing the card showed their own scope.
            _ if arg.starts_with("--actor") || arg.starts_with("--project") => {
                return Err(anyhow!("budget_identity_override_rejected"))
            }
            other => return Err(anyhow!("unknown budget option '{other}'\n\n{}", usage())),
        }
    }
    Ok(BudgetRoute::ControlPlane {
        name: BUDGET_CARD_COMMAND.to_owned(),
        arguments: serde_json::json!({
            "schema": BUDGET_CARD_QUERY_SCHEMA,
            "operation": "budget_card",
            "kind": kind,
            "output": "json",
            "limit": limit,
        }),
    })
}

fn correction_route(args: Option<&str>) -> anyhow::Result<BudgetRoute> {
    let (operation, rest) = split_word(args.unwrap_or(""));
    match operation.unwrap_or("request") {
        "request" => {
            let reason = rest
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    anyhow!("budget correction request requires a reason\n\n{}", usage())
                })?;
            Ok(BudgetRoute::ControlPlane {
                name: BUDGET_CORRECTION_COMMAND.to_owned(),
                arguments: serde_json::json!({
                    "schema": BUDGET_CARD_QUERY_SCHEMA,
                    "operation": "request_correction",
                    "command": BUDGET_CORRECTION_COMMAND,
                    "reason": reason,
                }),
            })
        }
        "help" | "--help" | "-h" => Ok(BudgetRoute::Local),
        other => Err(anyhow!(
            "unknown budget correction '{other}'\n\n{}",
            usage()
        )),
    }
}

fn parse_limit(value: &str) -> anyhow::Result<u16> {
    let parsed: u16 = value
        .parse()
        .map_err(|_| anyhow!("--limit requires a positive integer"))?;
    if parsed == 0 || parsed > BILLING_QUERY_MAX_LIMIT {
        return Err(anyhow!(
            "--limit must be between 1 and {BILLING_QUERY_MAX_LIMIT}"
        ));
    }
    Ok(parsed)
}

/// Build the BQ-23 request for the card. `project_id` and `data_boundary_digest` are
/// **server-resolved**; the command surface never mints either from user input.
pub fn budget_query_request(
    project_id: ProjectId,
    data_boundary_digest: impl Into<String>,
    source_cursor: Option<u64>,
    limit: u16,
) -> Result<BillingQueryRequest, String> {
    let request = BillingQueryRequest {
        schema: BILLING_QUERY_SCHEMA.to_owned(),
        kind: BillingQueryKind::BudgetSummary,
        project_id,
        data_boundary_digest: data_boundary_digest.into(),
        source_cursor,
        after_cursor: 0,
        limit,
        cursor: None,
        read_only: true,
    };
    request
        .validate()
        .map_err(|error| format!("budget_query_request_invalid:{error}"))?;
    Ok(request)
}

/// Server-computed card inputs the BQ-23 response alone does not carry. `remaining` and `limit`
/// stay `None` until the daemon supplies them; the card renders `unknown` rather than computing
/// `limit - spent`.
#[derive(Clone, Debug, Default)]
pub struct ServerBudgetFacts {
    pub remaining: Option<BudgetAmount>,
    pub limit: Option<BudgetAmount>,
    pub state: Option<BudgetState>,
    pub overage_reason: Option<OverageReason>,
    pub pending_reservations: Option<u64>,
    pub queue_depth: Option<u64>,
    pub queue_limit: Option<u64>,
    pub unknown_cost_reason: Option<BillingUnknownReason>,
    pub queue: Vec<BudgetQueueRow>,
}

/// Render the card from a server response plus the server-computed facts. There is no local
/// limit/spend arithmetic anywhere in this path: `remaining` and `limit` are copied, never derived.
pub fn render_budget_card(
    response: &BillingQueryResponse,
    actor_id: &str,
    facts: &ServerBudgetFacts,
) -> Result<BudgetCard, String> {
    response
        .validate()
        .map_err(|_| "budget_card_response_invalid".to_owned())?;
    if response.kind != BillingQueryKind::BudgetSummary {
        return Err("budget_card_kind_invalid".to_owned());
    }
    let spent = committed_spend(&response.summary)?;
    let known = spent.is_some() && !matches!(response.freshness, Freshness::Unknown);
    let reason = facts.overage_reason.or_else(|| {
        (response.unknown_count > 0)
            .then(|| overage_reason_for_unknown(Some(BillingUnknownReason::Partial)))
    });
    let state = facts.state.unwrap_or(if known {
        BudgetState::WithinBudget
    } else {
        BudgetState::Unknown
    });
    let overage_reason = match state {
        BudgetState::OverLimit => reason.unwrap_or(OverageReason::UnknownCost),
        BudgetState::Unknown => OverageReason::UnknownCost,
        BudgetState::WithinBudget | BudgetState::AtLimit => OverageReason::NotOverage,
    };
    BudgetCard::from_response(
        response,
        actor_id,
        spent,
        facts.remaining.clone(),
        facts.limit.clone(),
        state,
        overage_reason,
        facts.pending_reservations,
        facts.queue_depth,
        facts.queue_limit,
        (response.unknown_count > 0).then_some(response.unknown_count),
        facts.queue.clone(),
    )
}

/// The text a CLI/Workbench/Desktop surface prints. It is the card's own renderer plus a provenance
/// line, so a surface cannot print a number the card does not carry.
pub fn render_budget_text(
    response: &BillingQueryResponse,
    actor_id: &str,
    facts: &ServerBudgetFacts,
) -> Result<String, String> {
    let card = render_budget_card(response, actor_id, facts)?;
    Ok(format!(
        "{}\nresponse: {}\nunknown_is_printed_as: {UNKNOWN_TOKEN}",
        card.render_text(),
        response.response_digest
    ))
}

/// Build a correction submitter intent. It names the versioned command and carries no decision:
/// approval stays in the existing approval path.
pub fn correction_intent(
    project_id: ProjectId,
    actor_id: &str,
    correction_id: kiana_domain::CostCorrectionId,
    reason: &str,
) -> Result<BudgetCorrectionIntent, String> {
    BudgetCorrectionIntent::new(
        project_id,
        actor_id,
        correction_id,
        BUDGET_CORRECTION_COMMAND,
        reason,
    )
}

fn usage() -> &'static str {
    "Usage: kiana budget [card|queue|overage] [--limit N]\n       kiana budget correction request <reason>"
}

fn split_word(value: &str) -> (Option<&str>, Option<&str>) {
    match value.split_once(char::is_whitespace) {
        Some((word, rest)) => (Some(word), Some(rest.trim_start())),
        None => (Some(value), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::{BillingRollupTotals, Money};

    const BOUNDARY: &str =
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const PROJECTION: &str =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn response() -> BillingQueryResponse {
        BillingQueryResponse::new(
            BillingQueryKind::BudgetSummary,
            ProjectId::new(),
            BOUNDARY,
            10,
            PROJECTION,
            Freshness::Current,
            0,
            BillingRollupTotals::default(),
            1,
            0,
            None,
            None,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn budget_rejects_an_identity_override_instead_of_ignoring_it() {
        let result = BudgetCommand
            .execute(CommandContext {
                args: "card --actor someone-else".to_string(),
                app_state: Default::default(),
            })
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn budget_card_routes_to_the_versioned_control_plane_command() {
        let route = BudgetCommand
            .route(&CommandContext {
                args: "card --limit 20".to_string(),
                app_state: Default::default(),
            })
            .unwrap();
        match route {
            CommandRoute::ControlPlane { name, arguments } => {
                assert_eq!(name, BUDGET_CARD_COMMAND);
                assert_eq!(arguments["limit"], 20);
            }
            CommandRoute::Local => panic!("budget card must route to the control plane"),
        }
    }

    #[test]
    fn correction_request_routes_to_the_existing_correction_command() {
        let route = BudgetCommand
            .route(&CommandContext {
                args: "correction request wrong-model-charge".to_string(),
                app_state: Default::default(),
            })
            .unwrap();
        match route {
            CommandRoute::ControlPlane { name, arguments } => {
                assert_eq!(name, kiana_domain::COST_CORRECTION_COMMAND);
                assert_eq!(arguments["reason"], "wrong-model-charge");
            }
            CommandRoute::Local => panic!("correction must route to the versioned command"),
        }
    }

    #[test]
    fn unknown_freshness_renders_the_unknown_token_not_zero() {
        let mut response = response();
        response.freshness = Freshness::Unknown;
        response.response_digest = response.digest();
        response.validate().unwrap();
        let text =
            render_budget_text(&response, "local-user", &ServerBudgetFacts::default()).unwrap();
        assert!(text.contains("state: unknown"), "{text}");
        assert!(text.contains("spent: unknown"), "{text}");
        assert!(!text.contains("spent: 0 "), "{text}");
    }

    #[test]
    fn card_never_computes_a_remaining_amount() {
        let card = render_budget_card(
            &response(),
            "local-user",
            &ServerBudgetFacts {
                limit: Some(BudgetAmount::new("USD", 1_000).unwrap()),
                ..ServerBudgetFacts::default()
            },
        )
        .unwrap();
        assert!(card.remaining.is_none());
        assert!(card.render_text().contains("remaining: unknown"));
    }

    #[test]
    fn committed_spend_fails_closed_on_a_mixed_currency_rollup() {
        let rollup = BillingRollupTotals {
            measured: Some(Money::new("USD", 100).unwrap()),
            estimated: Some(Money::new("EUR", 200).unwrap()),
            ..BillingRollupTotals::default()
        };
        assert!(committed_spend(&rollup).is_err());
    }
}
