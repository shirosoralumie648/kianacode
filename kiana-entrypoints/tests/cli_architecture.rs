use serde_json::Value;
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn architecture_status_routes_through_the_real_binary() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("kiana-cli-architecture-{stamp}"));
    fs::create_dir_all(&home).unwrap();
    let previous_home = std::env::var_os("KIANA_HOME");
    std::env::set_var("KIANA_HOME", &home);
    kiana_types::write_project_trust(
        std::env::current_dir().unwrap(),
        kiana_types::ProjectTrust::Trusted,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .args(["architecture", "status", "--json"])
        .env("KIANA_HOME", &home)
        .output()
        .unwrap();
    match previous_home {
        Some(value) => std::env::set_var("KIANA_HOME", value),
        None => std::env::remove_var("KIANA_HOME"),
    }
    let _ = fs::remove_dir_all(home);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());

    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["schema"], "kiana.architecture-status.v1");
    assert_eq!(status["control_plane"], "kiana-core");
    assert_eq!(status["composition_root"], "kiana-daemon");
    assert_eq!(status["runner"], "kiana-runner");
    assert_eq!(status["harness"], "kiana-harness");
    assert_eq!(status["capability_mode"], "brokered");
    assert_eq!(status["legacy_prompt_loop"], false);
    assert_eq!(status["legacy_edges_remaining"], 9);
}

#[test]
fn product_surfaces_do_not_call_legacy_assistant_turn() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for name in [
        "sdk.rs",
        "cli.rs",
        "harness_run.rs",
        "repl.rs",
        "tui.rs",
        "bg.rs",
        "mcp.rs",
        "lib.rs",
    ] {
        let text = std::fs::read_to_string(root.join(name)).unwrap();
        assert!(
            !text.contains("run_assistant_turn"),
            "{name} must not call the legacy runner loop"
        );
    }
}
