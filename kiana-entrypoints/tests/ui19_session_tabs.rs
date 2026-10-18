//! UI-19 source/contract tests for Web session ownership and multi-tab concurrency.
//!
//! The declarative fixture names one runtime fixture per deny-first/success case; behavior is
//! exercised there.  This file binds the fixture, the typed protocol contracts, the client
//! conflict-copy catalogue and the embedded page to the same reviewed names, so a rename, a
//! dropped refusal or a stray second display catalogue cannot pass silently.  Local runs of the
//! crate are CI-only (`kiana-entrypoints` keeps a whole-crate shard).

use kiana_client::{
    web_tab_conflict_codes, WEB_TAB_CONFLICT_COPY, WEB_TAB_CONFLICT_MAX_REASONS,
    WEB_TIMELINE_PAGE_MAX_BYTES,
};
use kiana_entrypoints::web::{
    WEB_ACTION_PAYLOAD_SCHEMA, WEB_ACTION_SUBMISSION_SCHEMA, WEB_ERROR_MATRIX,
    WEB_TAB_RELATIONS_SCHEMA, WEB_TAB_SESSION_SCHEMA,
};
use kiana_protocol::{
    MAX_UI_TAB_RELATIONS, UI_ACTION_SUBMISSION_SCHEMA, UI_TAB_RELATION_GRAPH_SCHEMA,
    UI_TAB_SESSION_SCHEMA,
};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/ui19-session-tabs.json"))
        .expect("valid UI-19 session/tabs fixture")
}

#[test]
fn ui19_fixture_binds_every_case_to_a_named_fixture_and_a_known_code() {
    let fixture = fixture();
    assert_eq!(fixture["schema"], "kiana.ui19-session-tabs.v1");
    assert_eq!(fixture["authority"], "DaemonHost -> ControlPlane -> Broker");
    assert_eq!(fixture["proof_level"], "source");
    for (key, schema) in [
        ("tab_session", "kiana.ui-tab-session.v1"),
        ("tab_relations", "kiana.ui-tab-relations.v1"),
        ("action_submission", "kiana.ui-action-submission.v1"),
        ("action_payload", "kiana.web-action-payload.v1"),
        ("conflict_copy", "kiana.web-tab-conflicts.v1"),
    ] {
        assert_eq!(fixture["contracts"][key], schema, "contract {key}");
    }
    for field in [
        "principal",
        "session_id",
        "tab_id",
        "lease_epoch",
        "token_generation",
    ] {
        assert!(
            fixture["scope"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item == field),
            "scope {field}"
        );
    }
    assert_eq!(fixture["limits"]["session_tabs"], MAX_UI_TAB_RELATIONS);
    assert_eq!(
        fixture["limits"]["conflict_reasons"],
        WEB_TAB_CONFLICT_MAX_REASONS
    );
    // The client catalogue is the reviewed superset of the tab-visible refusals; the page mirror
    // is checked separately and must carry the same codes.
    assert!(
        WEB_TAB_CONFLICT_COPY.len() >= WEB_TAB_CONFLICT_MAX_REASONS,
        "the catalogue must explain at least as many refusals as one tab may retain"
    );
    assert_eq!(
        WEB_TAB_CONFLICT_COPY.len() as u64,
        fixture["limits"]["conflict_copy_count"].as_u64().unwrap(),
        "the fixture records the reviewed catalogue size"
    );
    for entry in WEB_TAB_CONFLICT_COPY {
        entry
            .validate()
            .unwrap_or_else(|error| panic!("catalogue copy invalid: {error}"));
    }

    let web = include_str!("../src/web.rs");
    let guard = include_str!("ui19_session_tabs_guard.rs");
    let catalogue = web_tab_conflict_codes();

    let mut cases: Vec<String> = Vec::new();
    for list in ["deny_first", "success"] {
        let entries = fixture[list]
            .as_array()
            .unwrap_or_else(|| panic!("{list} cases"));
        assert!(!entries.is_empty(), "{list} must name at least one case");
        for entry in entries {
            let case = entry["case"].as_str().expect("case id").to_owned();
            assert!(
                cases.iter().all(|seen| seen != &case),
                "duplicate UI-19 case {case}"
            );
            let code = entry["code"].as_str().expect("stable code");
            assert!(
                catalogue.iter().any(|known| *known == code)
                    || WEB_ERROR_MATRIX
                        .iter()
                        .any(|contract| contract.code == code),
                "case {case} names an unknown code {code}"
            );
            let name = entry["fixture"].as_str().expect("named fixture");
            assert!(
                web.contains(&format!("async fn {name}")) || guard.contains(name),
                "case {case} names a missing fixture {name}"
            );
            cases.push(case);
        }
    }
}

#[test]
fn ui19_runtime_fixtures_are_present_and_typed() {
    let web = include_str!("../src/web.rs");
    for name in [
        "ui19_owner_and_observer_share_one_snapshot_without_leaking_drafts",
        "ui19_duplicate_click_replays_the_original_result_and_refuses_a_changed_payload",
        "ui19_missing_or_stale_action_envelopes_are_refused_without_consuming_the_key",
        "ui19_owner_exit_and_token_rotation_keep_observers_read_only",
    ] {
        assert!(
            web.contains(&format!("async fn {name}")),
            "runtime fixture missing: {name}"
        );
    }
    // The runtime fixtures re-parse the projections as typed values, compare the replay payload
    // against the canonical digest and drive real loopback requests.
    for marker in [
        "UiTabSessionV1",
        "UiTabRelationGraphV1",
        "json_digest",
        "ui17_json",
        "ui19_sandbox_request",
        "ui16_page_token",
    ] {
        assert!(
            web.contains(marker),
            "UI-19 runtime typing marker missing: {marker}"
        );
    }
}

#[test]
fn ui19_page_catalogue_and_schemas_mirror_the_reviewed_contracts() {
    let page = include_str!("../src/web_page.html");
    let web = include_str!("../src/web.rs");
    let protocol = include_str!("../../kiana-protocol/src/ui_contracts.rs");

    // The page validates exactly the server-owned schema names and reviewed bounds.
    assert!(page.contains(&format!(
        "const TAB_SESSION_SCHEMA = '{WEB_TAB_SESSION_SCHEMA}'"
    )));
    assert!(page.contains(&format!(
        "const TAB_RELATIONS_SCHEMA = '{WEB_TAB_RELATIONS_SCHEMA}'"
    )));
    assert!(page.contains(&format!("const MAX_TAB_RELATIONS = {MAX_UI_TAB_RELATIONS}")));
    assert!(page.contains(&format!(
        "const MAX_TAB_CONFLICT_REASONS = {WEB_TAB_CONFLICT_MAX_REASONS}"
    )));
    assert_eq!(
        WEB_TAB_SESSION_SCHEMA, UI_TAB_SESSION_SCHEMA,
        "the Web re-export must mirror the protocol schema"
    );
    assert_eq!(
        WEB_TAB_RELATIONS_SCHEMA, UI_TAB_RELATION_GRAPH_SCHEMA,
        "the Web re-export must mirror the protocol relation schema"
    );
    assert_eq!(
        WEB_ACTION_SUBMISSION_SCHEMA, UI_ACTION_SUBMISSION_SCHEMA,
        "the Web submission schema must mirror the protocol one"
    );
    assert_eq!(WEB_ACTION_PAYLOAD_SCHEMA, "kiana.web-action-payload.v1");

    // Every reviewed copy line is present verbatim, and the page declares no extra entries.
    for entry in WEB_TAB_CONFLICT_COPY {
        assert!(
            page.contains(&format!("code:'{}'", entry.code)),
            "the page lost the conflict copy for {}",
            entry.code
        );
        assert!(
            page.contains(entry.message),
            "the page lost the copy message for {}",
            entry.code
        );
        assert!(
            page.contains(entry.recovery),
            "the page lost the recovery line for {}",
            entry.code
        );
    }
    assert_eq!(
        page.matches("code:'").count(),
        WEB_TAB_CONFLICT_COPY.len(),
        "the page catalogue must mirror the client catalogue exactly"
    );

    // Server-side markers: the typed projection is derived, validated and only then published.
    for marker in [
        "WEB_TAB_RELATIONS_SCHEMA",
        "WEB_ACTION_PAYLOAD_SCHEMA",
        "MAX_WEB_SESSION_TABS: usize = 16",
        "register_session_tab",
        "encode_tab_projection",
        "ui_action_payload_mismatch",
        "web_action_replay_payload",
        "web_tab_projection_invalid",
        "owner_tab_id",
        "claim_session_tab",
        "require_session_owner",
        "authorize_feed_tab",
        "rotate_web_token",
        "DaemonHost",
        "ControlPlane",
    ] {
        assert!(web.contains(marker), "UI-19 web marker missing: {marker}");
    }
    for marker in [
        "UI_TAB_RELATION_GRAPH_SCHEMA",
        "UiTabRelationGraphV1",
        "UiTabRelationKind",
        "MAX_UI_TAB_RELATIONS",
    ] {
        assert!(
            protocol.contains(marker),
            "UI-19 protocol marker missing: {marker}"
        );
    }

    // Page-side behavior markers: tab-scoped drafts, typed ownership, bounded diagnostics and a
    // close handler that only detaches the feed.
    for marker in [
        "tabOwnerId",
        "tabDrafts",
        "submissionIds",
        "stableSubmissionId",
        "x-kiana-ui-tab",
        "tab_id=",
        "TAB_CONFLICT_COPY",
        "tabConflictText",
        "tabSessionTrusted",
        "tabSessionDisposition",
        "__KIANA_TAB_STATE__",
        "web_action_replay",
        "closeEventStream",
        "owner tab",
        "original command receipt",
    ] {
        assert!(page.contains(marker), "UI-19 page marker missing: {marker}");
    }

    // The embedded page stays inside the reviewed bundle budget recorded by the fixture.
    let budget = fixture()["limits"]["page_bytes"].as_u64().unwrap();
    assert_eq!(budget, WEB_TIMELINE_PAGE_MAX_BYTES as u64);
    assert!(
        page.len() <= budget as usize,
        "the embedded page left its reviewed budget: {} bytes",
        page.len()
    );
}
