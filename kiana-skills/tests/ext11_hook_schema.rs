use kiana_skills::{parse_hook_manifest, NormalizedHookEvent, NORMALIZED_HOOK_MANIFEST_SCHEMA};
use serde_json::json;

#[test]
fn normalized_hook_manifest_exposes_typed_events_and_semantics() {
    let manifest = json!({
        "schema": NORMALIZED_HOOK_MANIFEST_SCHEMA,
        "hooks": [
            {"id":"before-model","event":"BeforeModel","matcher":"*","phase":"guard","entry":"hooks/before.json"},
            {"id":"after-tool","event":"PostToolUse","matcher":"shell.*","phase":"observer","entry":"hooks/after.json"}
        ]
    });
    let parsed = parse_hook_manifest(&manifest.to_string()).unwrap();
    assert_eq!(parsed.descriptors.len(), 2);
    assert_eq!(
        parsed.descriptors[0].event,
        NormalizedHookEvent::BeforeModel
    );
    assert!(parsed.descriptors[0].deny_sticky);
    assert!(parsed.descriptors[0].ask_preserved);
    assert!(parsed.descriptors[0].update_requires_reauthorization);
    assert_eq!(
        parsed.descriptors[1].phase,
        kiana_domain::HookPhase::Observer
    );
    assert!(parsed.descriptors[1]
        .required_scope
        .contains("hook:post_tool_use"));
}

#[test]
fn unsupported_hook_event_fails_closed_through_adapter() {
    let manifest = json!({
        "schema": NORMALIZED_HOOK_MANIFEST_SCHEMA,
        "hooks": [{"id":"unknown","event":"UnknownEvent","matcher":"*","phase":"guard","entry":"hooks/unknown.json"}]
    });
    assert!(parse_hook_manifest(&manifest.to_string()).is_err());
}
