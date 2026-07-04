use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn stream_json_input_error_prints_typed_json_event() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .args([
            "-p",
            "--input-format=stream-json",
            "--output-format=stream-json",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"{bad json}\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let event: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();

    assert_eq!(event["schema"], "kiana.stream-json-input-error.v1");
    assert_eq!(event["code"], "invalid_json");
    assert_eq!(event["line"], 1);
    assert!(event["message"]
        .as_str()
        .unwrap()
        .contains("--input-format=stream-json line 1 is not valid JSON"));
    assert!(stderr.contains("not valid JSON"));
}
