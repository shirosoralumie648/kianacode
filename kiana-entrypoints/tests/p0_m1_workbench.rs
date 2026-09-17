#[test]
fn workbench_surfaces_agree_on_terminal_state() {
    let workbench = include_str!("../src/workbench_chat.rs");
    let stream = include_str!("../src/stream_render.rs");
    let web = include_str!("../src/web.rs");
    let daemon = include_str!("../../kiana-daemon/src/run_stream.rs");
    let desktop = include_str!("../../contrib/desktop/main.js");
    for surface in [workbench, stream, web] {
        assert!(surface.contains("RunStreamEvent::Terminal"));
        assert!(surface.contains("ExecutionStatus"));
        assert!(!surface.contains("record_terminal_event"));
    }
    assert!(daemon.contains("RunStreamEvent::Terminal"));
    assert!(daemon.contains("run.completed"));
    assert!(desktop.contains("startHarness"));
    assert!(desktop.contains("loadURL"));
}

#[test]
fn terminal_state_rendering_does_not_hide_unknown_or_cancelled() {
    let workbench = include_str!("../src/workbench_chat.rs");
    let stream = include_str!("../src/stream_render.rs");
    let web = include_str!("../src/web.rs");
    assert!(workbench.contains("ResultUnknown"));
    assert!(workbench.contains("Cancelled"));
    assert!(stream.contains("response.status"));
    assert!(web.contains("result_unknown"));
}
