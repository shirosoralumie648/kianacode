use kiana_entrypoints::provider_diagnostics::{sanitize_auth_status, sanitize_command_output};

#[test]
fn diagnostics_are_presence_only_and_redact_nested_token_text() {
    let input = serde_json::json!({
        "api_key": "sk-live-provider-secret",
        "key_preview": "sk-li...cret",
        "oauth": {
            "access_token": "access-secret",
            "refresh_token": "refresh-secret",
            "token_count": 4
        },
        "error": "{\"api_key\":\"embedded-secret\"}",
        "provider_id": "openai"
    });
    let output = sanitize_auth_status(&input);
    let text = output.to_string();
    assert!(!text.contains("sk-live-provider-secret"));
    assert!(!text.contains("access-secret"));
    assert!(!text.contains("refresh-secret"));
    assert!(!text.contains("embedded-secret"));
    assert_eq!(output["api_key"], "configured");
    assert_eq!(output["key_preview"], "redacted");
    assert_eq!(output["oauth"]["token_count"], 4);
}

#[test]
fn auth_command_output_is_sanitized_but_other_commands_are_unchanged() {
    let auth = r#"{"api_key":"cli-secret","status":"configured"}"#;
    let sanitized = sanitize_command_output("auth", auth);
    assert!(!sanitized.contains("cli-secret"));
    assert!(sanitized.contains("configured"));

    let other = "plain output";
    assert_eq!(sanitize_command_output("doctor", other), other);
}
