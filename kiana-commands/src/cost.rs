use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;
use serde_json::Value;

pub struct CostCommand;

#[async_trait]
impl Command for CostCommand {
    fn name(&self) -> &str {
        "cost"
    }

    fn description(&self) -> &str {
        "Show cost statistics"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        match context.args.trim() {
            "" => {}
            "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
            _ => return Err(anyhow!(usage())),
        }

        let cost = number_from_state(
            &context.app_state,
            &["total_cost_usd", "session_cost_usd", "cost_usd", "cost"],
        );
        let input_tokens = integer_from_state(
            &context.app_state,
            &["input_tokens", "total_input_tokens", "prompt_tokens"],
        );
        let output_tokens = integer_from_state(
            &context.app_state,
            &["output_tokens", "total_output_tokens", "completion_tokens"],
        );

        Ok(CommandResult::text(format!(
            "Cost\ncost_usd: {:.6}\ninput_tokens: {}\noutput_tokens: {}\nsource: current command context",
            cost.unwrap_or(0.0),
            input_tokens.unwrap_or(0),
            output_tokens.unwrap_or(0)
        )))
    }
}

fn usage() -> &'static str {
    "Usage: kiana cost"
}

fn number_from_state(
    app_state: &std::collections::HashMap<String, Value>,
    keys: &[&str],
) -> Option<f64> {
    keys.iter()
        .find_map(|key| app_state.get(*key).and_then(Value::as_f64))
        .or_else(|| {
            app_state.get("usage").and_then(|usage| {
                keys.iter()
                    .find_map(|key| usage.get(*key).and_then(Value::as_f64))
            })
        })
}

fn integer_from_state(
    app_state: &std::collections::HashMap<String, Value>,
    keys: &[&str],
) -> Option<u64> {
    keys.iter()
        .find_map(|key| app_state.get(*key).and_then(Value::as_u64))
        .or_else(|| {
            app_state.get("usage").and_then(|usage| {
                keys.iter()
                    .find_map(|key| usage.get(*key).and_then(Value::as_u64))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::CostCommand;
    use crate::{Command, CommandContext};
    use serde_json::json;
    use std::collections::HashMap;

    #[tokio::test]
    async fn cost_rejects_unknown_args_instead_of_returning_totals() {
        let result = CostCommand
            .execute(CommandContext {
                args: "json".to_string(),
                app_state: HashMap::from([("total_cost_usd".to_string(), json!(0.25))]),
            })
            .await;

        assert!(result.is_err());
    }
}
