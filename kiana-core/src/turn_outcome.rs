//! Core-side boundary for the Runner's structured output and turn outcome proposal.
//!
//! The reducer is deterministic and evidence-shaped.  It does not let output text close a turn
//! while an invocation, approval, clarification or background job is still outstanding.

use kiana_domain::{TurnOutcome, TurnOutcomeInput};
use serde_json::Value;

pub const TURN_OUTCOME_CORE_SCHEMA: &str = "kiana.core-turn-outcome.v1";

pub fn propose_turn_outcome(input: TurnOutcomeInput) -> Result<TurnOutcome, String> {
    let outcome = TurnOutcome::decide(input)?;
    outcome.validate()?;
    Ok(outcome)
}

/// Add the server-derived outcome to a structured harness result without allowing the result
/// payload to replace or edit the outcome fields.
pub fn annotate_output(mut output: Value, outcome: &TurnOutcome) -> Result<Value, String> {
    outcome.validate()?;
    let object = output
        .as_object_mut()
        .ok_or_else(|| "turn_output_object_required".to_owned())?;
    if object.contains_key("turn_outcome") {
        return Err("turn_output_outcome_already_present".to_owned());
    }
    object.insert(
        "turn_outcome".to_owned(),
        serde_json::to_value(outcome).map_err(|_| "turn_outcome_encode_failed".to_owned())?,
    );
    Ok(output)
}
