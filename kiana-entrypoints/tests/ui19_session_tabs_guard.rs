#[test]
fn ui19_deny_first_guard_keeps_tabs_out_of_execution_authority() {
    let web = include_str!("../src/web.rs");
    let page = include_str!("../src/web_page.html");
    let run = web.split("async fn run_turn(").nth(1).expect("run handler");
    let run = run.split("async fn cancel_turn(").next().expect("run body");
    assert!(run.contains("require_session_owner"));
    assert!(run.contains("claim_action_submission"));
    assert!(run.contains("complete_action_submission"));
    assert!(!run.contains("tab_id = body.session_id"));

    let cancel = web.split("async fn cancel_turn(").nth(1).expect("cancel handler");
    let cancel = cancel.split("async fn trust_folder(").next().expect("cancel body");
    assert!(cancel.contains("require_session_owner"));
    assert!(!cancel.contains("owner_exit"));

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
fn ui19_owner_and_cursor_fences_are_server_side() {
    let web = include_str!("../src/web.rs");
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
}
