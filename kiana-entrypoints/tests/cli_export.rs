use std::process::Command;

#[test]
fn export_help_is_available_from_top_level_cli() {
    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .arg("export")
        .arg("--help")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "kiana export --help failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Usage:"), "{stdout}");
    assert!(stdout.contains("kiana export"), "{stdout}");
    assert!(stdout.contains("--session"), "{stdout}");
}
