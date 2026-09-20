use kiana_skills::{
    discover_hooks, parse_hook_manifest, HookDiscoveryCandidate, HookDiscoveryRequest,
    NormalizedHookEvent, NORMALIZED_HOOK_MANIFEST_SCHEMA,
};
use serde_json::json;

#[test]
fn hook_discovery_is_deterministic_and_separates_guard_observer() {
    let manifest = json!({
        "schema": NORMALIZED_HOOK_MANIFEST_SCHEMA,
        "hooks": [
            {"id":"observer","event":"PreToolUse","matcher":"shell.*","phase":"observer","entry":"hooks/observer.json"},
            {"id":"guard","event":"PreToolUse","matcher":"shell.exec","phase":"guard","entry":"hooks/guard.json"},
            {"id":"other","event":"PostToolUse","matcher":"shell.*","phase":"guard","entry":"hooks/other.json"}
        ]
    });
    let parsed = parse_hook_manifest(&manifest.to_string()).unwrap();
    let candidates = parsed
        .descriptors
        .into_iter()
        .enumerate()
        .map(|(index, descriptor)| HookDiscoveryCandidate {
            descriptor,
            source_priority: if index == 1 { 10 } else { 20 },
            declaration_order: index as u32,
        })
        .collect::<Vec<_>>();
    let snapshot = discover_hooks(
        &candidates,
        &HookDiscoveryRequest {
            event: NormalizedHookEvent::PreToolUse,
            subject: "shell.exec".to_owned(),
            input: json!({"tool":"shell.exec"}),
        },
    )
    .unwrap();
    assert_eq!(snapshot.guard_ids, vec!["guard"]);
    assert_eq!(snapshot.observer_ids, vec!["observer"]);
    assert_eq!(snapshot.matched_ids, vec!["guard", "observer"]);
    assert!(snapshot.snapshot_digest.starts_with("sha256:"));
    assert!(snapshot
        .ordered
        .iter()
        .any(|record| record.hook_id == "other" && record.reason == "event_mismatch"));
}

#[test]
fn invalid_regex_is_recorded_as_unmatched_reason() {
    let manifest = json!({
        "schema": NORMALIZED_HOOK_MANIFEST_SCHEMA,
        "hooks": [{"id":"broken","event":"PreToolUse","matcher":"re:[","phase":"guard","entry":"hooks/broken.json"}]
    });
    let parsed = parse_hook_manifest(&manifest.to_string()).unwrap();
    let snapshot = discover_hooks(
        &[HookDiscoveryCandidate {
            descriptor: parsed.descriptors[0].clone(),
            source_priority: 1,
            declaration_order: 0,
        }],
        &HookDiscoveryRequest {
            event: NormalizedHookEvent::PreToolUse,
            subject: "shell.exec".to_owned(),
            input: json!({}),
        },
    )
    .unwrap();
    assert_eq!(snapshot.matched_ids, Vec::<String>::new());
    assert_eq!(snapshot.ordered[0].reason, "matcher_invalid");
}
