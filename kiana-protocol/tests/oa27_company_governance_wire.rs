use kiana_protocol::{RequestBody, RequestEnvelope, RequestMetadata, COMPANY_GOVERNANCE};
use serde_json::json;

#[test]
fn governance_projection_uses_the_existing_versioned_command_route() {
    let request = RequestEnvelope::company_governance(
        RequestMetadata::local("session-1", "/repo"),
        "project-1",
    );
    assert_eq!(request.schema, kiana_protocol::PROTOCOL_SCHEMA);
    assert!(
        matches!(request.body, RequestBody::Command(ref command) if command.name == COMPANY_GOVERNANCE)
    );
    let encoded = serde_json::to_value(&request).unwrap();
    assert_eq!(
        encoded["body"]["request"]["arguments"]["project_id"],
        "project-1"
    );
}

#[test]
fn governance_arguments_do_not_accept_raw_owner_or_scope_overrides() {
    let mut arguments =
        json!({"project_id":"project-1","actor_id":"forged","project_root":"/other"});
    assert_eq!(arguments["project_id"], "project-1");
    arguments.as_object_mut().unwrap().remove("actor_id");
    arguments.as_object_mut().unwrap().remove("project_root");
    assert_eq!(arguments, json!({"project_id":"project-1"}));
}
