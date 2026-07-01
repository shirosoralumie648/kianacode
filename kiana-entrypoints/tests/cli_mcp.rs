use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn mcp_add_list_get_and_remove_use_project_mcp_json() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-mcp-{}-{unique}",
        std::process::id()
    ));
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    let add = run_kiana_mcp(
        &home,
        &project,
        &[
            "mcp",
            "add-json",
            "docs",
            r#"{"command":"node","args":["server.js"]}"#,
        ],
    );
    assert!(add.status.success(), "{}", add.stderr);
    assert!(add.stdout.contains("Added stdio MCP server docs"));
    assert!(project.join(".mcp.json").is_file());

    let list = run_kiana_mcp(&home, &project, &["mcp", "list"]);
    assert!(list.status.success(), "{}", list.stderr);
    assert!(list.stdout.contains("docs"), "{}", list.stdout);
    assert!(list.stdout.contains("node server.js"), "{}", list.stdout);

    let get = run_kiana_mcp(&home, &project, &["mcp", "get", "docs"]);
    assert!(get.status.success(), "{}", get.stderr);
    assert!(get.stdout.contains("\"name\": \"docs\""), "{}", get.stdout);
    assert!(
        get.stdout.contains("\"command\": \"node\""),
        "{}",
        get.stdout
    );

    let remove = run_kiana_mcp(&home, &project, &["mcp", "remove", "docs"]);
    assert!(remove.status.success(), "{}", remove.stderr);
    assert!(remove.stdout.contains("Removed MCP server docs"));

    let list_after_remove = run_kiana_mcp(&home, &project, &["mcp", "list"]);
    assert!(
        list_after_remove.status.success(),
        "{}",
        list_after_remove.stderr
    );
    assert!(
        list_after_remove
            .stdout
            .contains("No MCP servers configured"),
        "{}",
        list_after_remove.stdout
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mcp_project_scope_flags_match_reference_cli_shape() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-mcp-scope-{}-{unique}",
        std::process::id()
    ));
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    let add = run_kiana_mcp(
        &home,
        &project,
        &[
            "mcp",
            "add-json",
            "docs",
            r#"{"command":"node","args":["server.js"]}"#,
            "--scope",
            "project",
        ],
    );
    assert!(add.status.success(), "{}", add.stderr);
    assert!(add.stdout.contains("Added stdio MCP server docs"));

    let remove = run_kiana_mcp(&home, &project, &["mcp", "remove", "docs", "-s", "project"]);
    assert!(remove.status.success(), "{}", remove.stderr);
    assert!(remove.stdout.contains("Removed MCP server docs"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mcp_add_reference_cli_shapes_write_project_mcp_json() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-mcp-add-{}-{unique}",
        std::process::id()
    ));
    let home = root.join("home");
    let stdio_project = root.join("stdio-project");
    let http_project = root.join("http-project");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&stdio_project).unwrap();
    std::fs::create_dir_all(&http_project).unwrap();

    let stdio = run_kiana_mcp(
        &home,
        &stdio_project,
        &[
            "mcp",
            "add",
            "-e",
            "API_KEY=abc def",
            "docs",
            "--",
            "node",
            "server.js",
            "--watch",
        ],
    );
    assert!(stdio.status.success(), "{}", stdio.stderr);
    assert!(stdio.stdout.contains("Added stdio MCP server docs"));
    let stdio_config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(stdio_project.join(".mcp.json")).unwrap())
            .unwrap();
    assert_eq!(stdio_config["mcpServers"]["docs"]["type"], "stdio");
    assert_eq!(stdio_config["mcpServers"]["docs"]["command"], "node");
    assert_eq!(
        stdio_config["mcpServers"]["docs"]["args"],
        serde_json::json!(["server.js", "--watch"])
    );
    assert_eq!(
        stdio_config["mcpServers"]["docs"]["env"]["API_KEY"],
        "abc def"
    );

    let http = run_kiana_mcp(
        &home,
        &http_project,
        &[
            "mcp",
            "add",
            "--transport",
            "http",
            "docs",
            "https://example.test/mcp",
            "--header",
            "Authorization: Bearer abc",
            "--scope",
            "project",
        ],
    );
    assert!(http.status.success(), "{}", http.stderr);
    assert!(http.stdout.contains("Added HTTP MCP server docs"));
    let http_config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(http_project.join(".mcp.json")).unwrap())
            .unwrap();
    assert_eq!(http_config["mcpServers"]["docs"]["type"], "http");
    assert_eq!(
        http_config["mcpServers"]["docs"]["url"],
        "https://example.test/mcp"
    );
    assert_eq!(
        http_config["mcpServers"]["docs"]["headers"]["Authorization"],
        "Bearer abc"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mcp_add_from_claude_desktop_imports_project_mcp_json() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-mcp-desktop-{}-{unique}",
        std::process::id()
    ));
    let home = root.join("home");
    let project = root.join("project");
    let desktop_config = root.join("claude_desktop_config.json");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        &desktop_config,
        r#"{"mcpServers":{"desktop_docs":{"command":"node","args":["desktop-server.js"]}}}"#,
    )
    .unwrap();

    let import = run_kiana_mcp_with_env(
        &home,
        &project,
        &["mcp", "add-from-claude-desktop", "--scope", "project"],
        &[("KIANA_CLAUDE_DESKTOP_CONFIG", desktop_config.as_os_str())],
    );
    assert!(import.status.success(), "{}", import.stderr);
    assert!(import.stdout.contains("Imported 1 MCP server(s)"));
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(project.join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(config["mcpServers"]["desktop_docs"]["command"], "node");
    assert_eq!(
        config["mcpServers"]["desktop_docs"]["args"],
        serde_json::json!(["desktop-server.js"])
    );

    let list = run_kiana_mcp(&home, &project, &["mcp", "list"]);
    assert!(list.status.success(), "{}", list.stderr);
    assert!(
        list.stdout.contains("node desktop-server.js"),
        "{}",
        list.stdout
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mcp_list_and_get_inherit_parent_project_mcp_json() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-mcp-parent-{}-{unique}",
        std::process::id()
    ));
    let home = root.join("home");
    let project = root.join("project");
    let child = project.join("packages").join("app");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(
        project.join(".mcp.json"),
        r#"{"mcpServers":{"docs":{"command":"node","args":["root-server.js"]}}}"#,
    )
    .unwrap();

    let list = run_kiana_mcp(&home, &child, &["mcp", "list"]);
    assert!(list.status.success(), "{}", list.stderr);
    assert!(list.stdout.contains("docs"), "{}", list.stdout);
    assert!(
        list.stdout.contains("node root-server.js"),
        "{}",
        list.stdout
    );

    let get = run_kiana_mcp(&home, &child, &["mcp", "get", "docs"]);
    assert!(get.status.success(), "{}", get.stderr);
    assert!(
        get.stdout.contains("\"command\": \"node\""),
        "{}",
        get.stdout
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mcp_list_and_get_expand_project_env_placeholders() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-mcp-env-{}-{unique}",
        std::process::id()
    ));
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join(".mcp.json"),
        r#"{"mcpServers":{"docs":{"command":"${KIANA_TEST_MCP_COMMAND}","args":["${KIANA_TEST_MCP_ARG:-fallback.js}"]}}}"#,
    )
    .unwrap();
    std::env::set_var("KIANA_TEST_MCP_COMMAND", "node");
    std::env::remove_var("KIANA_TEST_MCP_ARG");

    let list = run_kiana_mcp(&home, &project, &["mcp", "list"]);
    assert!(list.status.success(), "{}", list.stderr);
    assert!(list.stdout.contains("node fallback.js"), "{}", list.stdout);

    let get = run_kiana_mcp(&home, &project, &["mcp", "get", "docs"]);
    assert!(get.status.success(), "{}", get.stderr);
    assert!(
        get.stdout.contains("\"command\": \"node\""),
        "{}",
        get.stdout
    );
    assert!(get.stdout.contains("\"args\": ["), "{}", get.stdout);

    std::env::remove_var("KIANA_TEST_MCP_COMMAND");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mcp_reset_project_choices_updates_project_choice_state() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-mcp-reset-{}-{unique}",
        std::process::id()
    ));
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(project.join(".kiana")).unwrap();
    std::fs::write(
        project.join(".kiana").join("mcp-project-choices.json"),
        r#"{"enabledMcpjsonServers":["docs"],"disabledMcpjsonServers":["shell"],"enableAllProjectMcpServers":true}"#,
    )
    .unwrap();

    let reset = run_kiana_mcp(&home, &project, &["mcp", "reset-project-choices"]);
    assert!(reset.status.success(), "{}", reset.stderr);
    assert!(
        reset
            .stdout
            .contains("project-scoped (.mcp.json) server approvals"),
        "{}",
        reset.stdout
    );
    let choices: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(project.join(".kiana").join("mcp-project-choices.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(choices["enabledMcpjsonServers"], serde_json::json!([]));
    assert_eq!(choices["disabledMcpjsonServers"], serde_json::json!([]));
    assert_eq!(
        choices["enableAllProjectMcpServers"],
        serde_json::json!(false)
    );

    let _ = std::fs::remove_dir_all(root);
}

struct KianaOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

fn run_kiana_mcp(home: &std::path::Path, project: &std::path::Path, args: &[&str]) -> KianaOutput {
    run_kiana_mcp_with_env(home, project, args, &[])
}

fn run_kiana_mcp_with_env(
    home: &std::path::Path,
    project: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &std::ffi::OsStr)],
) -> KianaOutput {
    let kiana_home = home.join(".kiana");
    std::fs::create_dir_all(&kiana_home).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_kiana"));
    command
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
        .env_remove("KIANA_MCP_SERVERS_JSON")
        .env_remove("ANTHROPIC_AUTH_TOKEN")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_BASE_URL")
        .env_remove("ANTHROPIC_MODEL")
        .env_remove("KIANA_REMOTE_ACCESS_TOKEN")
        .env_remove("CLAUDE_ACCESS_TOKEN");
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command.output().unwrap();

    KianaOutput {
        status: output.status,
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}
