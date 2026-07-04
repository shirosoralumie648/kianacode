use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn completion_generates_shell_scripts_and_writes_output_file() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-completion-{}-{unique}",
        std::process::id()
    ));
    let home = root.join("home");
    let project = root.join("project");
    let output_file = root.join("completion.zsh");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    let help = run_kiana_completion(&home, &project, &["completion", "--help"]);
    assert!(help.status.success(), "{}", help.stderr);
    assert!(
        help.stdout.contains("Usage: kiana completion <shell>"),
        "{}",
        help.stdout
    );

    let bash = run_kiana_completion(&home, &project, &["completion", "bash"]);
    assert!(bash.status.success(), "{}", bash.stderr);
    assert!(bash.stdout.contains("_kiana()"), "{}", bash.stdout);
    assert!(
        bash.stdout.contains("complete -F _kiana kiana"),
        "{}",
        bash.stdout
    );
    assert!(bash.stdout.contains("auth"), "{}", bash.stdout);
    assert!(bash.stdout.contains("agents"), "{}", bash.stdout);
    assert!(bash.stdout.contains("mcp"), "{}", bash.stdout);
    assert!(bash.stdout.contains("open"), "{}", bash.stdout);
    assert!(bash.stdout.contains("server"), "{}", bash.stdout);
    assert!(bash.stdout.contains("install uninstall"), "{}", bash.stdout);
    assert!(bash.stdout.contains("--scope"), "{}", bash.stdout);
    assert!(
        bash.stdout.contains("user project local"),
        "{}",
        bash.stdout
    );

    let zsh = run_kiana_completion(
        &home,
        &project,
        &[
            "completion",
            "zsh",
            "--output",
            output_file.to_str().unwrap(),
        ],
    );
    assert!(zsh.status.success(), "{}", zsh.stderr);
    assert!(
        zsh.stdout.contains("Wrote completion script"),
        "{}",
        zsh.stdout
    );
    let zsh_script = std::fs::read_to_string(&output_file).unwrap();
    assert!(zsh_script.contains("#compdef kiana"), "{zsh_script}");
    assert!(zsh_script.contains("_arguments"), "{zsh_script}");
    assert!(
        zsh_script.contains("'plugin command' list status json marketplace install uninstall"),
        "{zsh_script}"
    );
    assert!(
        zsh_script.contains("'plugin scope' user project local"),
        "{zsh_script}"
    );

    let fish = run_kiana_completion(&home, &project, &["completion", "fish"]);
    assert!(fish.status.success(), "{}", fish.stderr);
    assert!(
        fish.stdout.contains("__fish_seen_subcommand_from plugin"),
        "{}",
        fish.stdout
    );
    assert!(fish.stdout.contains("-l scope -s s"), "{}", fish.stdout);
    assert!(
        fish.stdout.contains("-a 'user project local'"),
        "{}",
        fish.stdout
    );

    let _ = std::fs::remove_dir_all(root);
}

struct KianaOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

fn run_kiana_completion(
    home: &std::path::Path,
    project: &std::path::Path,
    args: &[&str],
) -> KianaOutput {
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
