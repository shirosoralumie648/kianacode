#[test]
fn hook_process_execution_has_supervised_environment_and_output_bounds() {
    let hooks = include_str!("../../kiana-query/src/stop_hooks.rs");
    for marker in [
        "env_clear",
        "current_dir(cwd)",
        "process_group(0)",
        "kill_on_drop(true)",
        "bounded_hook_output",
        "hook_output_budget",
        "timeout_duration",
    ] {
        assert!(hooks.contains(marker), "missing EXT-13 marker: {marker}");
    }
    assert!(!hooks.contains("std::env::vars()"));
}
