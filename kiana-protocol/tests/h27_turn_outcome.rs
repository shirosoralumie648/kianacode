use kiana_protocol::{
    OutputContract, OutputValueType, TurnOutcome, TurnOutcomeInput, TurnOutcomeKind,
    OUTPUT_CONTRACT_SCHEMA, TURN_OUTCOME_SCHEMA,
};
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn structured_outcome_contracts_are_wire_visible_but_not_authority() {
    let contract = OutputContract::new(
        "answer",
        1,
        vec!["answer".to_owned()],
        BTreeMap::from([("answer".to_owned(), OutputValueType::String)]),
        false,
    )
    .unwrap();
    let mut input =
        TurnOutcomeInput::completed(kiana_protocol::RunId::new(), json!({"answer":"ok"}));
    input.output_contract = Some(contract);
    let outcome = TurnOutcome::decide(input).unwrap();
    let encoded = serde_json::to_value(&outcome).unwrap();
    assert_eq!(encoded["schema"], TURN_OUTCOME_SCHEMA);
    assert_eq!(outcome.kind, TurnOutcomeKind::Completed);
    assert_eq!(outcome.contract_digest.is_some(), true);
    assert_eq!(OUTPUT_CONTRACT_SCHEMA, "kiana.output-contract.v1");
    assert!(encoded.get("approval_id").is_none());
    assert!(encoded.get("capability_grant").is_none());
}
