use serde_json::Value;
use std::process::Command;

#[test]
fn architecture_status_routes_through_the_real_binary() {
    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .args(["architecture", "status", "--json"])
        .output()
        .unwrap();
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
    assert_eq!(status["legacy_edges_remaining"], 9);
}
