//! UI-19 deny-first guard: multi-tab ownership never becomes execution authority.
//!
//! The runtime fixtures prove behavior; this guard keeps the boundary markers that must survive
//! refactors — server-side ownership checks in the mutation handlers, a payload-bound action
//! registry that refuses a changed intent before any effect, a fail-closed typed projection, and
//! a close/reconnect path that only ever detaches the display feed.

#[test]
fn ui19_deny_first_guard_keeps_tabs_out_of_execution_authority() {
    let web = include_str!("../src/web.rs");
    let page = include_str!("../src/web_page.html");

    // Mutation handlers resolve the owner and claim one submission before running anything.
    let run = web.split("async fn run_turn(").nth(1).expect("run handler");
    let run = run.split("async fn cancel_turn(").next().expect("run body");
    assert!(run.contains("require_session_owner"));
    assert!(run.contains("claim_action_submission"));
    assert!(run.contains("complete_action_submission"));
    assert!(!run.contains("tab_id = body.session_id"));

    let cancel = web
        .split("async fn cancel_turn(")
        .nth(1)
        .expect("cancel handler");
    let cancel = cancel
        .split("async fn trust_folder(")
        .next()
        .expect("cancel body");
    assert!(cancel.contains("require_session_owner"));
    assert!(!cancel.contains("owner_exit"));

    // The action registry binds every key to the canonical payload digest and refuses a reused
    // key with a different intent before the daemon fence and before any handler.
    let claim = web
        .split("fn claim_action_submission(")
        .nth(1)
        .and_then(|rest| rest.split("fn web_action_replay_payload(").next())
        .expect("action claim");
    assert!(claim.contains("kiana_domain::json_digest(payload)"));
    assert!(claim.contains("original.payload_digest != payload_digest"));
    assert!(claim.contains("ui_action_payload_mismatch"));
    assert!(!claim.contains("harness_run::"));

    // The replay annotation only adds the server-owned marker to the cached response; it cannot
    // execute, store a turn or touch a lease.
    let replay = web
        .split("fn web_action_replay_payload(")
        .nth(1)
        .and_then(|rest| rest.split("fn complete_action_submission(").next())
        .expect("replay annotation");
    assert!(replay.contains("web_action_replay"));
    assert!(replay.contains("WEB_ACTION_PAYLOAD_SCHEMA"));
    assert!(!replay.contains("harness_run::"));
    assert!(!replay.contains("store_turn"));
    assert!(!replay.contains("sessions"));

    // A read-only snapshot never registers a tab: `/api/state` cannot grow the relation roster
    // or leak another tab's registration into an unregistered read.
    let state = web
        .split("async fn state(")
        .nth(1)
        .and_then(|rest| rest.split("async fn list_sessions(").next())
        .expect("state handler");
    assert!(state.contains("attach_hydrate_tab"));
    assert!(!state.contains("register_session_tab"));
    assert!(!state.contains("claim_session_tab"));

    // The typed projection is re-derived per tab, and a closed projection stays closed.
    assert!(web.contains("if tab_session.disposition != UiTabSessionDisposition::Closed"));
    assert!(web.contains("UiTabSessionDisposition::Owner"));
    assert!(web.contains("encode_tab_projection"));

    // Closing a browser tab only detaches its feed. It never submits /api/cancel and therefore
    // cannot turn a presentation lifecycle event into an execution mutation.
    let unload = page
        .split("beforeunload")
        .nth(1)
        .expect("tab close handler");
    assert!(unload.contains("closeEventStream"));
    assert!(!unload.contains("/api/cancel"));
    assert!(!unload.contains("sendBeacon"));
    assert!(!unload.contains("submitCommand"));

    let reconnect = page
        .split("function scheduleEventStreamReconnect")
        .nth(1)
        .and_then(|rest| rest.split("async function requestStreamHydrate").next())
        .expect("reconnect state machine");
    assert!(!reconnect.contains("/api/run"));
    assert!(!reconnect.contains("/api/cancel"));
}

#[test]
fn ui19_source_and_page_bind_the_tab_lease_contract() {
    let web = include_str!("../src/web.rs");
    let page = include_str!("../src/web_page.html");

    // Bootstrapping and opening a feed are the only ways a tab becomes visible; the owner edge is
    // registered server-side and a tab header can never widen the projection.
    let bootstrap = web
        .split("async fn bootstrap(")
        .nth(1)
        .and_then(|rest| rest.split("async fn parity(").next())
        .expect("bootstrap handler");
    assert!(bootstrap.contains("claim_session_tab"));
    let feed = web
        .split("async fn events(")
        .nth(1)
        .and_then(|rest| rest.split("fn stream_attach_state").next())
        .expect("events handler");
    assert!(feed.contains("resolve_feed_session"));
    assert!(feed.contains("authorize_feed_tab"));
    assert!(!feed.contains("harness_run::run_envelope_on_host"));
    assert!(!feed.contains("harness_run::cancel_envelope_on_host"));

    // Ownership is only ever derived from the typed server projection plus the server relation
    // graph; anything missing or mismatched degrades to `unverified`, never to owner.
    let disposition = page
        .split("function tabSessionDisposition(snapshot)")
        .nth(1)
        .and_then(|rest| rest.split("function tabSessionTrusted").next())
        .expect("page disposition");
    for marker in [
        "TAB_SESSION_SCHEMA",
        "TAB_RELATIONS_SCHEMA",
        "feed_only",
        "principal_id",
        "'unverified'",
        "owner_tab_id",
    ] {
        assert!(
            disposition.contains(marker),
            "page tab ownership marker missing: {marker}"
        );
    }
    assert!(page.contains("const ownsSession = tabSessionTrusted(s)"));
    assert!(page.contains("humanActionsAllowed = !!s.human_actions_allowed && ownsSession"));

    // The conflict copy is display-only: it never issues a request and never retries by itself.
    let copy = page
        .split("function tabConflictCopy(code)")
        .nth(1)
        .and_then(|rest| rest.split("const tabState =").next())
        .expect("conflict copy lookup");
    assert!(copy.contains("TAB_CONFLICT_COPY"));
    assert!(!copy.contains("fetch("));
    assert!(!copy.contains("/api/"));
    assert!(!copy.contains("announce("));

    // The bounded tab diagnostics never carry the credential or artifact bytes.
    let tab_state = page
        .split("const tabState = {")
        .nth(1)
        .and_then(|rest| rest.split("function recordTabConflict").next())
        .expect("tab state");
    assert!(!tab_state.contains("__KIANA_WEB_TOKEN__"));
    assert!(!tab_state.contains("token"));
    assert!(!tab_state.contains("artifact"));
}
