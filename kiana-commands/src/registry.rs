use crate::types::Command;
use std::collections::HashMap;
use std::sync::Arc;

pub struct CommandRegistry {
    commands: HashMap<String, Arc<dyn Command>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    pub fn register(&mut self, command: Arc<dyn Command>) {
        self.commands.insert(command.name().to_string(), command);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Command>> {
        self.commands.get(name)
    }

    pub fn list(&self) -> Vec<&Arc<dyn Command>> {
        self.commands.values().filter(|c| !c.is_hidden()).collect()
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub fn create_default_command_registry() -> CommandRegistry {
    let mut registry = CommandRegistry::new();

    registry.register(Arc::new(crate::version::VersionCommand));
    registry.register(Arc::new(crate::help::HelpCommand));
    registry.register(Arc::new(crate::clear::ClearCommand));
    registry.register(Arc::new(crate::config::ConfigCommand));
    registry.register(Arc::new(crate::compact::CompactCommand));
    registry.register(Arc::new(crate::commit::CommitCommand));
    registry.register(Arc::new(crate::completion::CompletionCommand));
    registry.register(Arc::new(crate::init::InitCommand));
    registry.register(Arc::new(crate::advisor::AdvisorCommand));
    registry.register(Arc::new(crate::brief::BriefCommand));
    registry.register(Arc::new(crate::checkpoint::CheckpointCommand));
    registry.register(Arc::new(crate::checks::ChecksCommand));
    registry.register(Arc::new(crate::auto_mode::AutoModeCommand));
    registry.register(Arc::new(crate::context::ContextCommand));
    registry.register(Arc::new(crate::exit::ExitCommand));
    registry.register(Arc::new(crate::auth::AuthCommand));
    registry.register(Arc::new(crate::login::LoginCommand));
    registry.register(Arc::new(crate::logout::LogoutCommand));
    registry.register(Arc::new(crate::model::ModelCommand));
    registry.register(Arc::new(crate::output_style::OutputStyleCommand));
    registry.register(Arc::new(crate::skills::SkillsCommand));
    registry.register(Arc::new(crate::status::StatusCommand));
    registry.register(Arc::new(crate::tasks::TasksCommand));
    registry.register(Arc::new(crate::permissions::PermissionsCommand));
    registry.register(Arc::new(crate::hooks::HooksCommand));
    registry.register(Arc::new(crate::mcp::McpCommand));
    registry.register(Arc::new(crate::plugin::PluginCommand));
    registry.register(Arc::new(crate::reload_plugins::ReloadPluginsCommand));
    registry.register(Arc::new(crate::release::ReleaseCommand));
    registry.register(Arc::new(crate::review::ReviewCommand));
    registry.register(Arc::new(crate::session::SessionCommand));
    registry.register(Arc::new(crate::trust::TrustCommand));
    registry.register(Arc::new(crate::usage::UsageCommand));
    registry.register(Arc::new(crate::cost::CostCommand));
    registry.register(Arc::new(crate::stats::StatsCommand));
    registry.register(Arc::new(crate::diff::DiffCommand));
    registry.register(Arc::new(crate::export::ExportCommand));
    registry.register(Arc::new(crate::memory::MemoryCommand));
    registry.register(Arc::new(crate::theme::ThemeCommand));
    registry.register(Arc::new(crate::vim::VimCommand));
    registry.register(Arc::new(crate::doctor::DoctorCommand));
    registry.register(Arc::new(crate::feedback::FeedbackCommand));
    for command in crate::plugin_commands::load_plugin_prompt_commands() {
        registry.register(Arc::new(command));
    }

    registry
}

#[cfg(test)]
mod tests {
    use super::create_default_command_registry;
    use crate::local_state::env_lock;
    use crate::{CommandContext, CommandType};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;
    use std::process::Command as ProcessCommand;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn default_registry_includes_core_commands() {
        let registry = create_default_command_registry();

        for command in [
            "help",
            "version",
            "auth",
            "auto-mode",
            "completion",
            "config",
            "compact",
            "init",
            "commit",
            "checkpoint",
            "checks",
            "doctor",
            "release",
            "review",
            "reload-plugins",
            "trust",
        ] {
            assert!(registry.get(command).is_some(), "missing /{command}");
        }
    }

    #[tokio::test]
    async fn settings_commands_persist_to_config_file() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kiana-config-{}-{}.toml",
            std::process::id(),
            unique
        ));
        std::env::set_var("KIANA_CONFIG_FILE", &path);

        let registry = create_default_command_registry();
        for (name, args) in [
            ("model", "claude-opus-4-1"),
            ("theme", "dark"),
            ("vim", "on"),
            ("brief", "on"),
            ("advisor", "claude-haiku-4-5"),
        ] {
            let result = registry
                .get(name)
                .unwrap()
                .execute(CommandContext {
                    args: args.to_string(),
                    app_state: HashMap::<String, Value>::new(),
                })
                .await
                .unwrap();
            assert!(
                !result.value.contains(" UI"),
                "{name} still returned a UI stub"
            );
        }

        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("model = \"claude-opus-4-1\""));
        assert!(contents.contains("brief = true"));
        assert!(contents.contains("vim_mode = true"));
        assert!(contents.contains("theme = \"dark\""));
        assert!(contents.contains("advisor_model = \"claude-haiku-4-5\""));

        let _ = fs::remove_file(path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn local_status_commands_do_not_return_ui_stubs() {
        let registry = create_default_command_registry();
        let app_state = HashMap::from([
            ("tasks".to_string(), serde_json::json!([{ "id": "t1" }])),
            ("teams".to_string(), serde_json::json!([{ "id": "team" }])),
            ("total_cost_usd".to_string(), serde_json::json!(0.25)),
            ("input_tokens".to_string(), serde_json::json!(12)),
            ("output_tokens".to_string(), serde_json::json!(8)),
        ]);

        for name in ["context", "cost", "stats", "usage", "export"] {
            let result = registry
                .get(name)
                .unwrap()
                .execute(CommandContext {
                    args: String::new(),
                    app_state: app_state.clone(),
                })
                .await
                .unwrap();
            assert!(!result.value.contains(" UI"), "{name} returned a UI stub");
            assert!(
                !result.value.trim().is_empty(),
                "{name} returned empty output"
            );
        }
    }

    #[tokio::test]
    async fn local_read_only_commands_accept_help_flags() {
        let registry = create_default_command_registry();
        for name in [
            "context",
            "cost",
            "checkpoint",
            "checks",
            "diff",
            "doctor",
            "exit",
            "feedback",
            "help",
            "mcp",
            "release",
            "reload-plugins",
            "review",
            "stats",
            "status",
            "usage",
            "version",
        ] {
            let result = registry
                .get(name)
                .unwrap()
                .execute(CommandContext {
                    args: "--help".to_string(),
                    app_state: HashMap::<String, Value>::new(),
                })
                .await
                .unwrap();
            assert!(
                result.value.contains("Usage:") || result.value.contains("usage:"),
                "{name} help did not return usage text; output was:\n{}",
                result.value
            );
        }
    }

    #[tokio::test]
    async fn release_command_reports_executable_smoke_gates() {
        let registry = create_default_command_registry();
        let command = registry.get("release").expect("missing /release");

        let result = command
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::<String, Value>::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains("Release readiness"));
        assert!(result
            .value
            .contains("smoke_script: scripts/release-smoke.sh (found)"));
        for gate in [
            "cargo fmt --all --check",
            "cargo test --workspace --locked --offline --no-fail-fast",
            "cargo build --release --locked --offline -p kiana-entrypoints --bin kiana",
            "./target/release/kiana --version",
            "./target/release/kiana doctor",
        ] {
            assert!(result.value.contains(gate), "missing gate: {gate}");
        }
    }

    #[tokio::test]
    async fn checkpoint_json_creates_git_checkpoint_outside_worktree() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-checkpoint-repo-{}-{unique}",
            std::process::id()
        ));
        let home = std::env::temp_dir().join(format!(
            "kiana-checkpoint-home-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@example.com"]);
        run_git(&root, &["config", "user.name", "Kiana Test"]);
        fs::write(root.join("staged.txt"), "base staged\n").unwrap();
        fs::write(root.join("worktree.txt"), "base worktree\n").unwrap();
        run_git(&root, &["add", "staged.txt", "worktree.txt"]);
        run_git(&root, &["commit", "-m", "initial"]);

        fs::write(root.join("staged.txt"), "staged change\n").unwrap();
        run_git(&root, &["add", "staged.txt"]);
        fs::write(root.join("worktree.txt"), "worktree change\n").unwrap();
        fs::write(root.join("notes.txt"), "untracked note\n").unwrap();
        std::env::set_var("KIANA_HOME", &home);

        let registry = create_default_command_registry();
        let command = registry.get("checkpoint").expect("missing /checkpoint");
        assert!(matches!(command.command_type(), CommandType::Local));
        assert!(command.supports_non_interactive());

        let result = command
            .execute(CommandContext {
                args: "--json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();
        let checkpoint_dir = Path::new(value["checkpoint_dir"].as_str().unwrap());

        assert_eq!(value["inside_git_repo"], true);
        assert_eq!(value["dirty"], true);
        assert!(checkpoint_dir.starts_with(home.join("checkpoints")));
        assert!(checkpoint_dir.join("manifest.json").is_file());
        assert!(fs::read_to_string(checkpoint_dir.join("staged.diff"))
            .unwrap()
            .contains("staged change"));
        assert!(fs::read_to_string(checkpoint_dir.join("unstaged.diff"))
            .unwrap()
            .contains("worktree change"));
        assert_eq!(
            fs::read_to_string(checkpoint_dir.join("untracked").join("notes.txt")).unwrap(),
            "untracked note\n"
        );
        assert!(!root.join(".kiana").join("checkpoints").exists());

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(home);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn checkpoint_restore_recovers_removed_untracked_file() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-checkpoint-restore-repo-{}-{unique}",
            std::process::id()
        ));
        let home = std::env::temp_dir().join(format!(
            "kiana-checkpoint-restore-home-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@example.com"]);
        run_git(&root, &["config", "user.name", "Kiana Test"]);
        fs::write(root.join("tracked.txt"), "base\n").unwrap();
        run_git(&root, &["add", "tracked.txt"]);
        run_git(&root, &["commit", "-m", "initial"]);
        fs::write(root.join("notes.txt"), "checkpoint note\n").unwrap();
        std::env::set_var("KIANA_HOME", &home);

        let registry = create_default_command_registry();
        let command = registry.get("checkpoint").expect("missing /checkpoint");
        let checkpoint = command
            .execute(CommandContext {
                args: "--json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let checkpoint_json: serde_json::Value = serde_json::from_str(&checkpoint.value).unwrap();
        let checkpoint_dir = checkpoint_json["checkpoint_dir"].as_str().unwrap();
        fs::remove_file(root.join("notes.txt")).unwrap();

        let result = command
            .execute(CommandContext {
                args: format!("restore {checkpoint_dir} --json"),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(
            fs::read_to_string(root.join("notes.txt")).unwrap(),
            "checkpoint note\n"
        );
        assert_eq!(value["conflicts"].as_array().unwrap().len(), 0);
        assert_eq!(value["restored_untracked"][0], "notes.txt");

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(home);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn checkpoint_restore_refuses_to_overwrite_user_changes() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-checkpoint-conflict-repo-{}-{unique}",
            std::process::id()
        ));
        let home = std::env::temp_dir().join(format!(
            "kiana-checkpoint-conflict-home-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@example.com"]);
        run_git(&root, &["config", "user.name", "Kiana Test"]);
        fs::write(root.join("tracked.txt"), "base\n").unwrap();
        run_git(&root, &["add", "tracked.txt"]);
        run_git(&root, &["commit", "-m", "initial"]);
        fs::write(root.join("notes.txt"), "checkpoint note\n").unwrap();
        std::env::set_var("KIANA_HOME", &home);

        let registry = create_default_command_registry();
        let command = registry.get("checkpoint").expect("missing /checkpoint");
        let checkpoint = command
            .execute(CommandContext {
                args: "--json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let checkpoint_json: serde_json::Value = serde_json::from_str(&checkpoint.value).unwrap();
        let checkpoint_dir = checkpoint_json["checkpoint_dir"].as_str().unwrap();
        fs::write(root.join("notes.txt"), "user later change\n").unwrap();

        let result = command
            .execute(CommandContext {
                args: format!("restore {checkpoint_dir} --json"),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(
            fs::read_to_string(root.join("notes.txt")).unwrap(),
            "user later change\n"
        );
        assert_eq!(value["restored_untracked"].as_array().unwrap().len(), 0);
        assert_eq!(value["conflicts"][0]["path"], "notes.txt");

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(home);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn checkpoint_restore_reapplies_clean_unstaged_patch() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-checkpoint-patch-repo-{}-{unique}",
            std::process::id()
        ));
        let home = std::env::temp_dir().join(format!(
            "kiana-checkpoint-patch-home-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@example.com"]);
        run_git(&root, &["config", "user.name", "Kiana Test"]);
        fs::write(root.join("tracked.txt"), "base\n").unwrap();
        run_git(&root, &["add", "tracked.txt"]);
        run_git(&root, &["commit", "-m", "initial"]);
        fs::write(root.join("tracked.txt"), "checkpoint change\n").unwrap();
        std::env::set_var("KIANA_HOME", &home);

        let registry = create_default_command_registry();
        let command = registry.get("checkpoint").expect("missing /checkpoint");
        let checkpoint = command
            .execute(CommandContext {
                args: "--json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let checkpoint_json: serde_json::Value = serde_json::from_str(&checkpoint.value).unwrap();
        let checkpoint_dir = checkpoint_json["checkpoint_dir"].as_str().unwrap();
        fs::write(root.join("tracked.txt"), "base\n").unwrap();

        let result = command
            .execute(CommandContext {
                args: format!("restore {checkpoint_dir} --json"),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(
            fs::read_to_string(root.join("tracked.txt")).unwrap(),
            "checkpoint change\n"
        );
        assert_eq!(value["conflicts"].as_array().unwrap().len(), 0);
        assert_eq!(value["applied_patches"][0], "unstaged.diff");

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(home);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn diff_from_checkpoint_json_reports_changes_after_checkpoint() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-diff-checkpoint-repo-{}-{unique}",
            std::process::id()
        ));
        let home = std::env::temp_dir().join(format!(
            "kiana-diff-checkpoint-home-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@example.com"]);
        run_git(&root, &["config", "user.name", "Kiana Test"]);
        fs::write(root.join("tracked.txt"), "base\n").unwrap();
        run_git(&root, &["add", "tracked.txt"]);
        run_git(&root, &["commit", "-m", "initial"]);

        fs::write(root.join("tracked.txt"), "base\nuser\n").unwrap();
        std::env::set_var("KIANA_HOME", &home);

        let registry = create_default_command_registry();
        let checkpoint = registry.get("checkpoint").expect("missing /checkpoint");
        let checkpoint_result = checkpoint
            .execute(CommandContext {
                args: "--json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let checkpoint_json: Value = serde_json::from_str(&checkpoint_result.value).unwrap();
        let checkpoint_dir = checkpoint_json["checkpoint_dir"].as_str().unwrap();

        fs::write(root.join("tracked.txt"), "base\nuser\nassistant\n").unwrap();
        fs::write(root.join("assistant.txt"), "new assistant file\n").unwrap();

        let diff = registry.get("diff").expect("missing /diff");
        let result = diff
            .execute(CommandContext {
                args: format!("--from-checkpoint {checkpoint_dir} --json"),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let value: Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.diff.from_checkpoint.v1");
        assert_eq!(value["changed"], true);
        assert_eq!(value["checkpoint"]["id"], checkpoint_json["id"]);
        let files = value["files"].as_array().unwrap();
        assert!(files.iter().any(|file| file["path"] == "tracked.txt"));
        assert!(files.iter().any(|file| file["path"] == "assistant.txt"));
        let patch = value["patch"].as_str().unwrap();
        assert!(patch.contains("+assistant"));
        assert!(patch.contains("+new assistant file"));
        assert!(
            !patch.contains("+user"),
            "checkpoint baseline user changes must not be reported as new assistant diff:\n{patch}"
        );

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(home);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn diff_last_assistant_json_uses_latest_assistant_turn_checkpoint() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-diff-last-assistant-repo-{}-{unique}",
            std::process::id()
        ));
        let home = std::env::temp_dir().join(format!(
            "kiana-diff-last-assistant-home-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@example.com"]);
        run_git(&root, &["config", "user.name", "Kiana Test"]);
        fs::write(root.join("tracked.txt"), "base\n").unwrap();
        run_git(&root, &["add", "tracked.txt"]);
        run_git(&root, &["commit", "-m", "initial"]);

        fs::write(root.join("tracked.txt"), "base\nuser\n").unwrap();
        std::env::set_var("KIANA_HOME", &home);

        let registry = create_default_command_registry();
        let checkpoint = registry.get("checkpoint").expect("missing /checkpoint");
        let checkpoint_result = checkpoint
            .execute(CommandContext {
                args: "--assistant-turn --session-id session-1 --turn-id turn-7 --json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let checkpoint_json: Value = serde_json::from_str(&checkpoint_result.value).unwrap();
        assert_eq!(checkpoint_json["kind"], "assistant_turn");
        assert_eq!(checkpoint_json["session_id"], "session-1");
        assert_eq!(checkpoint_json["turn_id"], "turn-7");

        fs::write(root.join("tracked.txt"), "base\nuser\nassistant\n").unwrap();

        let diff = registry.get("diff").expect("missing /diff");
        let result = diff
            .execute(CommandContext {
                args: "--last-assistant --json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let value: Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.diff.from_checkpoint.v1");
        assert_eq!(value["checkpoint"]["id"], checkpoint_json["id"]);
        assert_eq!(value["checkpoint"]["kind"], "assistant_turn");
        assert_eq!(value["checkpoint"]["session_id"], "session-1");
        assert_eq!(value["checkpoint"]["turn_id"], "turn-7");
        let patch = value["patch"].as_str().unwrap();
        assert!(patch.contains("+assistant"));
        assert!(
            !patch.contains("+user"),
            "pre-checkpoint user changes must not be included in last-assistant diff:\n{patch}"
        );

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(home);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn checkpoint_undo_last_assistant_restores_checkpoint_baseline() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-checkpoint-undo-last-assistant-repo-{}-{unique}",
            std::process::id()
        ));
        let home = std::env::temp_dir().join(format!(
            "kiana-checkpoint-undo-last-assistant-home-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@example.com"]);
        run_git(&root, &["config", "user.name", "Kiana Test"]);
        fs::write(root.join("tracked.txt"), "base\n").unwrap();
        run_git(&root, &["add", "tracked.txt"]);
        run_git(&root, &["commit", "-m", "initial"]);

        fs::write(root.join("tracked.txt"), "base\nuser\n").unwrap();
        fs::write(root.join("user-notes.txt"), "pre-existing user note\n").unwrap();
        std::env::set_var("KIANA_HOME", &home);

        let registry = create_default_command_registry();
        let checkpoint = registry.get("checkpoint").expect("missing /checkpoint");
        let checkpoint_result = checkpoint
            .execute(CommandContext {
                args: "--assistant-turn --session-id session-undo --turn-id turn-undo --json"
                    .to_string(),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let checkpoint_json: Value = serde_json::from_str(&checkpoint_result.value).unwrap();

        fs::write(root.join("tracked.txt"), "base\nuser\nassistant\n").unwrap();
        fs::write(root.join("assistant.txt"), "assistant created\n").unwrap();

        let undo = checkpoint
            .execute(CommandContext {
                args: "undo --last-assistant --json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let value: Value = serde_json::from_str(&undo.value).unwrap();

        assert_eq!(value["schema"], "kiana.checkpoint.undo.v1");
        assert_eq!(value["checkpoint"]["id"], checkpoint_json["id"]);
        assert_eq!(value["checkpoint"]["kind"], "assistant_turn");
        assert_eq!(value["checkpoint"]["session_id"], "session-undo");
        assert_eq!(value["checkpoint"]["turn_id"], "turn-undo");
        assert_eq!(
            fs::read_to_string(root.join("tracked.txt")).unwrap(),
            "base\nuser\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("user-notes.txt")).unwrap(),
            "pre-existing user note\n"
        );
        assert!(!root.join("assistant.txt").exists());
        let undone_paths = value["undone_files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["path"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(undone_paths.contains(&"tracked.txt"));
        assert!(undone_paths.contains(&"assistant.txt"));
        assert_eq!(value["conflicts"].as_array().unwrap().len(), 0);

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(home);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn review_dry_run_json_reports_deterministic_patch_plan() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-review-dry-run-repo-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@example.com"]);
        run_git(&root, &["config", "user.name", "Kiana Test"]);
        fs::write(root.join("staged.txt"), "base staged\n").unwrap();
        fs::write(root.join("worktree.txt"), "base worktree\n").unwrap();
        run_git(&root, &["add", "staged.txt", "worktree.txt"]);
        run_git(&root, &["commit", "-m", "initial"]);

        fs::write(root.join("staged.txt"), "staged change\n").unwrap();
        run_git(&root, &["add", "staged.txt"]);
        fs::write(root.join("worktree.txt"), "worktree change\n").unwrap();
        fs::write(root.join("notes.txt"), "untracked note\n").unwrap();

        let registry = create_default_command_registry();
        let command = registry.get("review").expect("missing /review");
        assert!(matches!(command.command_type(), CommandType::Local));
        assert!(command.supports_non_interactive());

        let first = command
            .execute(CommandContext {
                args: "--dry-run --json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let second = command
            .execute(CommandContext {
                args: "--dry-run --json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        assert_eq!(first.value, second.value);

        let value: serde_json::Value = serde_json::from_str(&first.value).unwrap();
        assert_eq!(value["schema"], "kiana.review.dry_run.v1");
        assert_eq!(value["dry_run"], true);
        assert_eq!(value["inside_git_repo"], true);
        assert_eq!(value["dirty"], true);
        assert_eq!(value["planned_steps"][0], "create_isolated_worktree");
        assert!(value["patches"]["staged"]["text"]
            .as_str()
            .unwrap()
            .contains("staged change"));
        assert!(value["patches"]["unstaged"]["text"]
            .as_str()
            .unwrap()
            .contains("worktree change"));
        let files = value["files"].as_array().unwrap();
        assert!(files.iter().any(|file| {
            file["path"] == "staged.txt" && file["index"] == "M" && file["worktree"] == " "
        }));
        assert!(files.iter().any(|file| {
            file["path"] == "worktree.txt" && file["index"] == " " && file["worktree"] == "M"
        }));
        assert!(files.iter().any(|file| {
            file["path"] == "notes.txt" && file["index"] == "?" && file["worktree"] == "?"
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn review_json_runs_checks_in_isolated_worktree_without_mutating_current_tree() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-review-isolated-run-repo-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(root.join("tracked.txt"), "base\n").unwrap();
        fs::write(
            root.join("scripts").join("release-smoke.sh"),
            "#!/usr/bin/env bash\nset -euo pipefail\nprintf 'tracked=%s\\n' \"$(cat tracked.txt)\"\nprintf 'untracked=%s\\n' \"$(cat untracked.txt)\"\nprintf 'review-mutated\\n' > tracked.txt\nprintf 'generated\\n' > generated-by-review.txt\n",
        )
        .unwrap();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@example.com"]);
        run_git(&root, &["config", "user.name", "Kiana Test"]);
        run_git(&root, &["add", "."]);
        run_git(&root, &["commit", "-m", "initial"]);
        fs::write(root.join("tracked.txt"), "base\nworking\n").unwrap();
        fs::write(root.join("untracked.txt"), "review notes\n").unwrap();

        let registry = create_default_command_registry();
        let command = registry.get("review").expect("missing /review");
        let result = command
            .execute(CommandContext {
                args: "--json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.review.run.v1");
        assert_eq!(value["dry_run"], false);
        assert_eq!(value["dirty"], true);
        assert_eq!(value["checks"]["execution"]["isolation"], "git_worktree");
        assert_eq!(
            value["checks"]["execution"]["applied_current_changes"],
            true
        );
        assert_eq!(value["checks"]["summary"]["passed"], 1);
        assert!(value["checks"]["results"][0]["stdout"]
            .as_str()
            .unwrap()
            .contains("tracked=base\nworking"));
        assert!(value["checks"]["results"][0]["stdout"]
            .as_str()
            .unwrap()
            .contains("untracked=review notes"));
        assert_eq!(value["findings"].as_array().unwrap().len(), 0);
        assert_eq!(
            fs::read_to_string(root.join("tracked.txt")).unwrap(),
            "base\nworking\n"
        );
        assert!(!root.join("generated-by-review.txt").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn checks_dry_run_json_reports_deterministic_discovered_gates() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-checks-dry-run-repo-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = []\nresolver = \"2\"\n",
        )
        .unwrap();
        fs::write(
            root.join("scripts").join("release-smoke.sh"),
            "#!/usr/bin/env bash\n",
        )
        .unwrap();
        run_git(&root, &["init"]);

        let registry = create_default_command_registry();
        let command = registry.get("checks").expect("missing /checks");
        assert!(matches!(command.command_type(), CommandType::Local));
        assert!(command.supports_non_interactive());

        let first = command
            .execute(CommandContext {
                args: "--dry-run --json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let second = command
            .execute(CommandContext {
                args: "--dry-run --json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        assert_eq!(first.value, second.value);

        let value: serde_json::Value = serde_json::from_str(&first.value).unwrap();
        assert_eq!(value["schema"], "kiana.checks.dry_run.v1");
        assert_eq!(value["dry_run"], true);
        assert_eq!(value["inside_git_repo"], true);
        let checks = value["checks"].as_array().unwrap();
        assert_eq!(checks[0]["id"], "rustfmt");
        assert_eq!(checks[0]["command"], "cargo fmt --all --check");
        assert_eq!(checks[1]["id"], "cargo_check");
        assert_eq!(checks[1]["command"], "cargo check --workspace");
        assert_eq!(checks[2]["id"], "cargo_test");
        assert_eq!(
            checks[2]["command"],
            "cargo test --workspace --no-fail-fast"
        );
        assert_eq!(checks[3]["id"], "release_smoke");
        assert_eq!(checks[3]["command"], "bash scripts/release-smoke.sh");

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn checks_json_executes_discovered_release_smoke_gate() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-checks-run-repo-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(
            root.join("scripts").join("release-smoke.sh"),
            "#!/usr/bin/env bash\nprintf 'smoke-ok\\n'\n",
        )
        .unwrap();
        run_git(&root, &["init"]);

        let registry = create_default_command_registry();
        let command = registry.get("checks").expect("missing /checks");
        let result = command
            .execute(CommandContext {
                args: "--json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.checks.run.v1");
        assert_eq!(value["dry_run"], false);
        assert_eq!(value["inside_git_repo"], true);
        assert_eq!(value["summary"]["total"], 1);
        assert_eq!(value["summary"]["passed"], 1);
        assert_eq!(value["summary"]["failed"], 0);
        assert_eq!(value["results"][0]["id"], "release_smoke");
        assert_eq!(value["results"][0]["status"], "passed");
        assert_eq!(value["results"][0]["exit_code"], 0);
        assert_eq!(value["results"][0]["stdout"], "smoke-ok\n");
        assert_eq!(value["results"][0]["stderr"], "");

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn checks_json_runs_gates_in_isolated_worktree_without_mutating_current_tree() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-checks-isolated-run-repo-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(root.join("tracked.txt"), "base\n").unwrap();
        fs::write(
            root.join("scripts").join("release-smoke.sh"),
            "#!/usr/bin/env bash\nset -euo pipefail\nprintf 'cwd=%s\\n' \"$PWD\"\nprintf 'tracked=%s\\n' \"$(cat tracked.txt)\"\nprintf 'untracked=%s\\n' \"$(cat untracked.txt)\"\nprintf 'mutated\\n' > tracked.txt\nprintf 'generated\\n' > generated-by-check.txt\n",
        )
        .unwrap();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@example.com"]);
        run_git(&root, &["config", "user.name", "Kiana Test"]);
        run_git(&root, &["add", "."]);
        run_git(&root, &["commit", "-m", "initial"]);
        fs::write(root.join("tracked.txt"), "base\nworking\n").unwrap();
        fs::write(root.join("untracked.txt"), "notes\n").unwrap();

        let registry = create_default_command_registry();
        let command = registry.get("checks").expect("missing /checks");
        let result = command
            .execute(CommandContext {
                args: "--json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), serde_json::json!(root.clone()))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.checks.run.v1");
        assert_eq!(value["dry_run"], false);
        assert_eq!(value["execution"]["isolation"], "git_worktree");
        assert_eq!(value["execution"]["applied_current_changes"], true);
        assert_eq!(value["summary"]["passed"], 1);
        assert!(value["results"][0]["stdout"]
            .as_str()
            .unwrap()
            .contains("tracked=base\nworking"));
        assert!(value["results"][0]["stdout"]
            .as_str()
            .unwrap()
            .contains("untracked=notes"));
        assert!(!value["results"][0]["stdout"]
            .as_str()
            .unwrap()
            .contains(&format!("cwd={}", root.display())));
        assert_eq!(
            fs::read_to_string(root.join("tracked.txt")).unwrap(),
            "base\nworking\n"
        );
        assert!(!root.join("generated-by-check.txt").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn local_user_commands_do_not_return_ui_stubs() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("kiana-home-{}-{}", std::process::id(), unique));
        let config_path = root.join("config.toml");
        std::env::set_var("KIANA_HOME", &root);
        std::env::set_var("KIANA_CONFIG_FILE", &config_path);

        let registry = create_default_command_registry();
        for name in ["login", "logout", "feedback", "memory", "skills", "plugin"] {
            let result = registry
                .get(name)
                .unwrap()
                .execute(CommandContext {
                    args: String::new(),
                    app_state: HashMap::<String, Value>::new(),
                })
                .await
                .unwrap();
            assert!(!result.value.contains(" UI"), "{name} returned a UI stub");
            assert!(
                !result.value.trim().is_empty(),
                "{name} returned empty output"
            );
        }

        let _ = fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn default_registry_includes_plugin_prompt_commands() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-registry-plugin-{}-{unique}",
            std::process::id()
        ));
        let plugin_root = root.join("plugins").join("review-tools");
        fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
        fs::write(
            plugin_root.join(".codex-plugin").join("plugin.json"),
            serde_json::json!({ "name": "review-tools" }).to_string(),
        )
        .unwrap();
        fs::create_dir_all(plugin_root.join("commands")).unwrap();
        fs::write(
            plugin_root.join("commands").join("audit.md"),
            "---\ndescription: Audit command\n---\nAudit $ARGUMENTS",
        )
        .unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", root.join("plugins"));

        let registry = create_default_command_registry();
        let command = registry
            .get("review-tools:audit")
            .expect("plugin command registered");
        assert!(matches!(command.command_type(), CommandType::Prompt));
        let result = command
            .execute(CommandContext {
                args: "src".to_string(),
                app_state: HashMap::<String, Value>::new(),
            })
            .await
            .unwrap();
        assert_eq!(result.value, "Audit src");

        let _ = fs::remove_dir_all(root);
        std::env::remove_var("KIANA_PLUGINS_DIR");
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = ProcessCommand::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout={}\nstderr={}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if args == ["init"] {
            for config_args in [
                ["config", "core.autocrlf", "false"],
                ["config", "core.eol", "lf"],
            ] {
                let output = ProcessCommand::new("git")
                    .current_dir(root)
                    .args(config_args)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "git {:?} failed\nstdout={}\nstderr={}",
                    config_args,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }
}
