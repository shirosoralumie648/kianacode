use kiana_commands::{create_default_command_registry, CommandContext};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn context(args: &str) -> CommandContext {
    CommandContext {
        args: args.to_string(),
        app_state: HashMap::new(),
    }
}

fn isolate_config_env(label: &str) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "kiana-auto-mode-{label}-{}-{unique}.toml",
        std::process::id()
    ));
    std::env::set_var("KIANA_CONFIG_FILE", path);
    std::env::remove_var("KIANA_REMOTE_SETTINGS_FILE");
    std::env::remove_var("KIANA_SETTINGS_FILE");
    std::env::remove_var("KIANA_SETTINGS_JSON");
}

#[tokio::test]
async fn auto_mode_defaults_and_config_emit_reference_shaped_json() {
    let _guard = env_lock().lock().unwrap();
    isolate_config_env("defaults");
    let registry = create_default_command_registry();
    let command = registry
        .get("auto-mode")
        .expect("missing auto-mode command");

    let defaults = command.execute(context("defaults")).await.unwrap();
    let defaults_json: Value = serde_json::from_str(&defaults.value).unwrap();
    assert!(defaults_json["allow"].as_array().unwrap().len() >= 3);
    assert!(defaults_json["soft_deny"].as_array().unwrap().len() >= 5);
    assert!(defaults_json["environment"].as_array().unwrap().len() >= 3);
    assert!(defaults_json["allow"][0]
        .as_str()
        .unwrap()
        .contains("Read files"));

    let config = command.execute(context("config")).await.unwrap();
    let config_json: Value = serde_json::from_str(&config.value).unwrap();
    assert_eq!(config_json, defaults_json);
}

#[tokio::test]
async fn auto_mode_critique_reports_missing_custom_rules_without_network() {
    let _guard = env_lock().lock().unwrap();
    isolate_config_env("critique");
    let registry = create_default_command_registry();
    let command = registry
        .get("auto-mode")
        .expect("missing auto-mode command");

    let result = command.execute(context("critique")).await.unwrap();

    assert!(result.value.contains("No custom auto mode rules found."));
    assert!(result
        .value
        .contains("autoMode.{allow, soft_deny, environment}"));
    assert!(result.value.contains("kiana auto-mode defaults"));
}

#[tokio::test]
async fn auto_mode_config_replaces_only_sections_with_custom_rules() {
    let _guard = env_lock().lock().unwrap();
    isolate_config_env("custom");
    std::env::set_var(
        "KIANA_SETTINGS_JSON",
        r#"{"settings":{"autoMode":{"allow":["Run cargo test in this repository"],"environment":["Linux CI shell"]}}}"#,
    );

    let registry = create_default_command_registry();
    let command = registry
        .get("auto-mode")
        .expect("missing auto-mode command");
    let config = command.execute(context("config")).await.unwrap();
    let config_json: Value = serde_json::from_str(&config.value).unwrap();

    assert_eq!(
        config_json["allow"],
        serde_json::json!(["Run cargo test in this repository"])
    );
    assert_eq!(
        config_json["environment"],
        serde_json::json!(["Linux CI shell"])
    );
    assert!(config_json["soft_deny"].as_array().unwrap().len() >= 5);

    std::env::remove_var("KIANA_SETTINGS_JSON");
}
