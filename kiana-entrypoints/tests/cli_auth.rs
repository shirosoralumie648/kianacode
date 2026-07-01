use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn auth_login_status_and_logout_use_configured_account_state() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-auth-{}-{unique}",
        std::process::id()
    ));
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    let initial = run_kiana_auth(&home, &project, &["auth", "status", "--json"]);
    assert!(initial.status.success(), "{}", initial.stderr);
    let initial_json: serde_json::Value = serde_json::from_str(&initial.stdout).unwrap();
    assert_eq!(initial_json["api_key"], "missing");
    assert_eq!(initial_json["source"], "none");

    let login = run_kiana_auth(&home, &project, &["auth", "login", "sk-ant-auth-test-key"]);
    assert!(login.status.success(), "{}", login.stderr);
    assert!(login.stdout.contains("Login updated"), "{}", login.stdout);

    let status = run_kiana_auth(&home, &project, &["auth", "status", "--text"]);
    assert!(status.status.success(), "{}", status.stderr);
    assert!(status.stdout.contains("Auth status"), "{}", status.stdout);
    assert!(status.stdout.contains("api_key: set"), "{}", status.stdout);
    assert!(
        status.stdout.contains("source: config"),
        "{}",
        status.stdout
    );

    let logout = run_kiana_auth(&home, &project, &["auth", "logout"]);
    assert!(logout.status.success(), "{}", logout.stderr);
    assert!(
        logout.stdout.contains("Logout complete"),
        "{}",
        logout.stdout
    );

    let after_logout = run_kiana_auth(&home, &project, &["auth", "status", "--json"]);
    assert!(after_logout.status.success(), "{}", after_logout.stderr);
    let after_logout_json: serde_json::Value = serde_json::from_str(&after_logout.stdout).unwrap();
    assert_eq!(after_logout_json["api_key"], "missing");
    assert_eq!(after_logout_json["source"], "none");

    let _ = std::fs::remove_dir_all(root);
}

struct KianaOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

fn run_kiana_auth(home: &std::path::Path, project: &std::path::Path, args: &[&str]) -> KianaOutput {
    let kiana_home = home.join(".kiana");
    std::fs::create_dir_all(&kiana_home).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .args(args)
        .current_dir(project)
        .env("HOME", home)
        .env("KIANA_HOME", &kiana_home)
        .env("KIANA_CONFIG_FILE", kiana_home.join("config.toml"))
        .env("KIANA_HOOKS_FILE", kiana_home.join("hooks.json"))
        .env(
            "KIANA_PERMISSIONS_FILE",
            kiana_home.join("permissions.json"),
        )
        .env("KIANA_PLUGINS_DIR", kiana_home.join("plugins"))
        .env("KIANA_TASKS_ROOT", kiana_home.join("tasks"))
        .env_remove("ANTHROPIC_AUTH_TOKEN")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_BASE_URL")
        .env_remove("ANTHROPIC_MODEL")
        .env_remove("KIANA_REMOTE_ACCESS_TOKEN")
        .env_remove("CLAUDE_ACCESS_TOKEN")
        .output()
        .unwrap();

    KianaOutput {
        status: output.status,
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}
