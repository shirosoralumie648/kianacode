use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn eval_help_and_packaged_fixture_route_through_the_real_binary() {
    let help = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .args(["eval", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success(), "{}", stderr(&help));
    let help_stdout = String::from_utf8(help.stdout).unwrap();
    assert!(
        help_stdout.contains("Usage: kiana eval run"),
        "{help_stdout}"
    );

    let suite = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/eval/fixtures/basic-runtime-suite.json");
    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .args([
            "eval",
            "run",
            "--suite",
            suite.to_str().unwrap(),
            "--json",
            "--fail-on-failure",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));

    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "kiana.eval-report.v1");
    assert_eq!(report["suite_id"], "basic-runtime");
    assert_eq!(report["status"], "passed");
    assert_eq!(report["summary"]["total"], 1);
    assert_eq!(report["summary"]["failed"], 0);
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
