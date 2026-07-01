use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn routed_help_flags_print_usage_instead_of_running_commands() {
    let cases = [
        (&["--help"][..], "Kiana Code", "--permission-profile"),
        (&["ps", "--help"][..], "Usage: kiana --bg", "kiana ps"),
        (
            &["url", "register", "--help"][..],
            "Usage: kiana url",
            "register [scheme]",
        ),
        (
            &["remote-session", "status", "--help"][..],
            "Usage: kiana remote-session",
            "status",
        ),
        (
            &["remote-session", "environments", "--help"][..],
            "Usage: kiana remote-session environments",
            "create-default",
        ),
        (
            &["mcp-server", "--help"][..],
            "Usage: kiana mcp-server",
            "mcp-server-http",
        ),
        (
            &["mcp-server-http", "--help"][..],
            "Usage: kiana mcp-server",
            "--port",
        ),
        (
            &["mcp", "serve", "--help"][..],
            "Usage: kiana mcp serve",
            "Run the Kiana MCP server over stdio",
        ),
        (
            &["computer-mcp", "--help"][..],
            "Usage: kiana computer-mcp",
            "stdio",
        ),
        (
            &["bridge", "status", "--help"][..],
            "Usage: kiana bridge",
            "status",
        ),
        (
            &["bridge", "start", "--help"][..],
            "Usage: kiana bridge",
            "start",
        ),
        (
            &["remote", "start", "--help"][..],
            "Usage: kiana bridge",
            "start",
        ),
        (
            &["agents", "--help"][..],
            "Usage: kiana agents",
            "configured",
        ),
        (
            &["chrome-native-host", "--help"][..],
            "Usage: kiana chrome",
            "install-native-host",
        ),
        (
            &["chrome-native-host", "install", "--help"][..],
            "Usage: kiana chrome",
            "install-native-host",
        ),
        (
            &["--print", "--help"][..],
            "Usage: kiana -p",
            "--json-schema",
        ),
        (&["-p", "--help"][..], "Usage: kiana -p", "--record-only"),
        (
            &["--output-format", "json", "--print", "--help"][..],
            "Usage: kiana -p",
            "--output-format",
        ),
        (
            &["--print", "--output-format", "json", "--help"][..],
            "Usage: kiana -p",
            "--output-format",
        ),
        (
            &[
                "--input-format=stream-json",
                "--output-format=stream-json",
                "--print",
                "--help",
            ][..],
            "Usage: kiana -p",
            "--sdk-url",
        ),
        (
            &["--print", "--record-only", "--help"][..],
            "Usage: kiana -p",
            "--record-only",
        ),
        (
            &["--continue", "--help"][..],
            "Usage: kiana --continue",
            "--resume",
        ),
        (
            &["--resume", "--help"][..],
            "Usage: kiana --continue",
            "--resume <session_id>",
        ),
        (
            &["--continue", "--record-only", "--help"][..],
            "Usage: kiana --continue",
            "--record-only",
        ),
        (
            &["--resume", "abc", "--help"][..],
            "Usage: kiana --continue",
            "--resume <session_id>",
        ),
        (
            &["--output-format", "json", "--continue", "--help"][..],
            "Usage: kiana --continue",
            "--output-format",
        ),
        (
            &["--continue", "--output-format", "json", "--help"][..],
            "Usage: kiana --continue",
            "--output-format",
        ),
        (
            &["--resume", "abc", "--output-format", "json", "--help"][..],
            "Usage: kiana --continue",
            "--output-format",
        ),
        (
            &["url", "handle", "--help"][..],
            "Usage: kiana url",
            "handle <url>",
        ),
        (
            &["url", "plan", "--help"][..],
            "Usage: kiana url",
            "plan [scheme]",
        ),
        (
            &["url", "status", "--help"][..],
            "Usage: kiana url",
            "status [scheme]",
        ),
        (
            &["remote-session", "url", "--help"][..],
            "Usage: kiana remote-session",
            "url",
        ),
        (
            &["remote-session", "list", "--help"][..],
            "Usage: kiana remote-session",
            "list",
        ),
        (
            &["remote-session", "show", "--help"][..],
            "Usage: kiana remote-session",
            "show",
        ),
        (
            &["remote-session", "create", "--help"][..],
            "Usage: kiana remote-session",
            "create",
        ),
        (
            &["remote-session", "listen", "--help"][..],
            "Usage: kiana remote-session",
            "listen",
        ),
        (
            &["remote-session", "send", "--help"][..],
            "Usage: kiana remote-session",
            "send",
        ),
        (
            &["remote-session", "code-session", "create", "--help"][..],
            "Usage: kiana remote-session code-session",
            "create",
        ),
        (
            &["remote-session", "code-session", "bridge", "--help"][..],
            "Usage: kiana remote-session code-session",
            "bridge",
        ),
        (
            &["remote-session", "code-session", "sdk-url", "--help"][..],
            "Usage: kiana remote-session code-session",
            "sdk-url",
        ),
        (
            &["plugin", "--help"][..],
            "usage: kiana plugin",
            "install <plugin>",
        ),
        (
            &["plugin", "install", "--help"][..],
            "Usage: kiana plugin install",
            "plugin@marketplace",
        ),
        (
            &["open", "--help"][..],
            "Usage: kiana open <cc-url>",
            "headless direct-connect mode",
        ),
        (
            &["server", "--help"][..],
            "Usage: kiana server",
            "direct-connect server",
        ),
        (
            &["daemon", "status", "--help"][..],
            "Usage: kiana daemon",
            "status",
        ),
        (
            &["daemon", "enqueue", "--help"][..],
            "Usage: kiana daemon",
            "enqueue <prompt>",
        ),
        (
            &["session", "list", "--help"][..],
            "Usage: kiana session",
            "list",
        ),
        (
            &["session", "show", "--help"][..],
            "Usage: kiana session",
            "show <id>",
        ),
        (
            &["session", "rename", "--help"][..],
            "Usage: kiana session",
            "rename <id>",
        ),
        (&["list", "--help"][..], "Usage: kiana session", "list"),
        (&["show", "--help"][..], "Usage: kiana session", "show <id>"),
        (
            &["rename", "--help"][..],
            "Usage: kiana session",
            "rename <id>",
        ),
    ];

    for (args, usage, detail) in cases {
        let output = run_isolated_kiana(args);

        assert!(
            output.status.success(),
            "kiana {} failed\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            output.stdout,
            output.stderr
        );
        assert!(
            output.stdout.contains(usage),
            "kiana {} did not print usage {usage:?}\nstdout:\n{}",
            args.join(" "),
            output.stdout
        );
        assert!(
            output.stdout.contains(detail),
            "kiana {} did not print expected detail {detail:?}\nstdout:\n{}",
            args.join(" "),
            output.stdout
        );
    }
}

struct KianaOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

fn run_isolated_kiana(args: &[&str]) -> KianaOutput {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-help-{}-{unique}",
        std::process::id()
    ));
    let home = root.join("home");
    let kiana_home = home.join(".kiana");
    std::fs::create_dir_all(&kiana_home).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .args(args)
        .env("HOME", &home)
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

    let _ = std::fs::remove_dir_all(&root);

    KianaOutput {
        status: output.status,
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}
