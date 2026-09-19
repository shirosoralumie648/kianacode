use kiana_domain::*;
use kiana_protocol::{RequestEnvelope, RequestMetadata, CLARIFICATION_ANSWER_COMMAND};

#[test]
fn clarification_answer_uses_regular_input_command_not_approval() {
    let request = ClarificationRequest::new(
        InteractionId::new(),
        RunId::new(),
        TurnId::new(),
        None,
        "继续吗？",
        Vec::new(),
        true,
        100,
        1_000,
        ClarificationCancelPolicy::UserOrRuntime,
        "builder",
        vec!["user".to_owned()],
    )
    .unwrap();
    let answer = ClarificationAnswer::new(
        RequestId::new(),
        request.interaction_id,
        request.run_id,
        request.turn_id,
        None,
        None,
        "是",
        "human-1",
        "user",
        ClarificationSource::Web,
        200,
        request.request_digest,
    )
    .unwrap();
    let envelope =
        RequestEnvelope::answer_clarification(RequestMetadata::local("s", "/repo"), answer)
            .unwrap();
    let value = serde_json::to_value(envelope).unwrap();
    assert_eq!(value["body"]["type"], "command");
    assert_eq!(
        value["body"]["request"]["name"],
        CLARIFICATION_ANSWER_COMMAND
    );
    assert!(value["body"]["request"].get("approval_id").is_none());
    assert!(value["body"]["request"].get("capability_request").is_none());
}
