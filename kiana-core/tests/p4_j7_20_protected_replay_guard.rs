use std::fs;

#[test]
fn protected_replay_source_never_serializes_private_bytes_or_bypasses_scope() {
    let source = fs::read_to_string("../kiana-domain/src/protected_replay.rs")
        .expect("protected replay source");
    assert!(source.contains("missing_reasoning_material_blocks_resume"));
    assert!(source.contains("replay_material_cannot_cross_connection_or_model_scope"));
    assert!(source.contains("Private bytes are intentionally not serializable"));
    // The private material type must stay outside serde; only its digest-bound public summary
    // may be serialized.
    assert!(!source.contains("Serialize for ProtectedReplayMaterial"));
    assert!(!source.contains("Deserialize for ProtectedReplayMaterial"));
    assert!(source.contains("public_replay_summary_is_not_resume_material"));
    assert!(source.contains("protected_replay_ttl_widened"));
    assert!(source.contains("protected_continuation_not_recoverable_from_compacted_view"));
    assert!(source.contains("model_switch"));

    let provider = fs::read_to_string("../kiana-provider/src/request.rs").expect("request source");
    assert!(provider.contains("model_replay_storage_unavailable"));
    assert!(provider.contains("validate_for_call"));
    assert!(provider.contains("compile_with_protected_replay"));
    assert!(provider.contains("protected_replay_item_count_invalid"));
    assert!(provider.contains("signed_reasoning_survives_tool_continuation_byte_for_byte"));

    // The provider capture type is intentionally not serializable either, and its Debug form is
    // only allowed to report the redaction marker.
    let replay =
        fs::read_to_string("../kiana-provider/src/replay.rs").expect("provider replay source");
    assert!(!replay.contains("Serialize for CapturedReplay"));
    assert!(!replay.contains("Deserialize for CapturedReplay"));
    assert!(!replay.contains("Serialize for ProtectedReplayMaterial"));
    assert!(replay.contains("plan_injection"));
    assert!(replay.contains("strip_private_items"));
    assert!(replay.contains("capture_from_value"));
    assert!(replay.contains("missing_reasoning_material_blocks_resume"));
    assert!(replay.contains("replay_material_unbound_to_history"));
    assert!(replay.contains("RingReplaySealer"));
    assert!(replay.contains("SEALED_REPLAY_ARTIFACT_SCHEMA"));
    assert!(replay.contains("replay_material_authentication_failed"));
    assert!(replay.contains("SealedReplayArtifactStore"));
    assert!(replay.contains("field(\"key\", &\"redacted\")"));

    // The streaming accumulator captures private items into the bounded buffer instead of
    // projecting them as public deltas, and only an enabled capture may decode them.
    let response =
        fs::read_to_string("../kiana-provider/src/response.rs").expect("response source");
    assert!(response.contains("push_capturing"));
    assert!(response.contains("finish_capturing"));
    assert!(response.contains("provider_private_replay_incomplete"));
    assert!(response.contains("replay_stream_fixture_capturing"));
}

#[test]
fn protected_replay_fixtures_and_lane_are_wired() {
    let domain = fs::read_to_string("../kiana-domain/tests/p4_j7_20_protected_replay.rs")
        .expect("domain replay fixtures");
    for fixture in [
        "signed_reasoning_survives_tool_continuation_byte_for_byte_in_process",
        "replay_scope_and_deletion_fail_closed",
        "expired_replay_material_is_not_resumable",
        "public_replay_summary_is_digest_only_and_cannot_resume",
        "material_is_bound_to_the_declared_reasoning_kind",
        "model_switch_revokes_route_bound_material",
        "compaction_boundary_keeps_continuations_out_of_the_summary",
    ] {
        assert!(domain.contains(fixture), "missing domain fixture {fixture}");
    }

    let provider = fs::read_to_string("../kiana-provider/tests/p4_j7_20_protected_replay.rs")
        .expect("provider replay fixtures");
    for fixture in [
        "captured_reasoning_is_resolved_into_the_continuation_plan",
        "missing_reasoning_material_blocks_resume",
        "replay_payload_deletion_invalidates_resume",
        "expiry_and_revocation_fail_closed_before_dispatch",
        "replay_material_cannot_cross_connection_or_model_scope",
        "plan_injection_refuses_unbound_and_duplicate_references",
        "reasoning_payload_never_enters_receipt_memory_or_logs",
        "sealed_short_lived_artifact_survives_a_process_restart_but_not_tampering",
        "streaming_capture_moves_signed_thinking_into_protected_material",
        "streaming_chat_reasoning_content_is_captured_but_never_emitted_as_delta",
        "streaming_gemini_thought_signature_is_captured_without_public_leak",
        "incomplete_private_signature_never_becomes_replay_material",
    ] {
        assert!(
            provider.contains(fixture),
            "missing provider fixture {fixture}"
        );
    }

    // The dedicated lane is manual-only (the automatic lanes are ci.yml's kiana-provider
    // whole-crate shard and kiana-core-s5/6) and keeps the fmt gate plus every fixture target.
    let lane = fs::read_to_string("../.github/workflows/p4-j7-20-protected-replay.yml")
        .expect("protected replay lane");
    assert!(lane.contains("workflow_dispatch:"));
    assert!(!lane.contains("\n  push:"));
    assert!(lane.contains("cargo fmt --all --check"));
    assert!(lane.contains("cargo test -p kiana-domain --test p4_j7_20_protected_replay"));
    assert!(lane.contains("cargo test -p kiana-provider --test p4_j7_20_protected_replay"));
    assert!(lane.contains("cargo test -p kiana-provider --lib"));
    assert!(lane.contains("cargo test -p kiana-core --test p4_j7_20_protected_replay_guard"));
}
