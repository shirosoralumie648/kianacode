use crate::local_state::kiana_home_dir;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use async_trait::async_trait;
use kiana_types::plugin::enabled_plugin_roots_in;
use std::path::{Path, PathBuf};

pub struct ReloadPluginsCommand;

#[async_trait]
impl Command for ReloadPluginsCommand {
    fn name(&self) -> &str {
        "reload-plugins"
    }

    fn description(&self) -> &str {
        "Reload plugin runtime state"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        match context.args.trim() {
            "" => {}
            "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
            _ => return Err(anyhow::anyhow!(usage())),
        }
        let root = plugin_root_dir(&context);
        let roots = enabled_plugin_roots_in(&root);
        let counts = PluginRuntimeCounts::from_roots(&roots);
        kiana_tools::lsp_tool::shutdown_lsp_clients().await;

        Ok(CommandResult::text(format!(
            "Reloaded: {} plugins · {} commands · {} agents · {} skills · {} hooks · {} plugin MCP servers · {} plugin LSP servers\npath: {}\nLSP runtime: restarted on next use",
            counts.plugins,
            counts.commands,
            counts.agents,
            counts.skills,
            counts.hooks,
            counts.mcp_servers,
            counts.lsp_servers,
            root.display()
        )))
    }
}

fn usage() -> &'static str {
    "Usage: kiana reload-plugins"
}

#[derive(Debug, Clone, Default)]
struct PluginRuntimeCounts {
    plugins: usize,
    commands: usize,
    agents: usize,
    skills: usize,
    hooks: usize,
    mcp_servers: usize,
    lsp_servers: usize,
}

impl PluginRuntimeCounts {
    fn from_roots(roots: &[PathBuf]) -> Self {
        let mut counts = Self {
            plugins: roots.len(),
            ..Self::default()
        };
        for root in roots {
            counts.commands += count_files(&root.join("commands"), &["md", "json", "toml"]);
            counts.agents += count_files(&root.join("agents"), &["md", "json", "toml"]);
            counts.skills += count_skill_dirs(&root.join("skills"));
            counts.hooks += usize::from(root.join("hooks").join("hooks.json").is_file());
            counts.mcp_servers += usize::from(root.join(".mcp.json").is_file());
            counts.lsp_servers += usize::from(root.join(".lsp.json").is_file());
        }
        counts
    }
}

fn plugin_root_dir(context: &CommandContext) -> PathBuf {
    if let Ok(path) = std::env::var("KIANA_PLUGINS_DIR") {
        return resolve_path(context, &path);
    }
    kiana_home_dir().join("plugins")
}

fn resolve_path(context: &CommandContext, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        cwd(context).join(path)
    }
}

fn cwd(context: &CommandContext) -> PathBuf {
    context
        .app_state
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn count_files(dir: &Path, extensions: &[&str]) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            let path = entry.path();
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| extensions.contains(&ext))
        })
        .count()
}

fn count_skill_dirs(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| entry.path().join("SKILL.md").is_file())
        .count()
}

#[cfg(test)]
mod tests {
    use super::ReloadPluginsCommand;
    use crate::local_state::env_lock;
    use crate::{Command, CommandContext};
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-reload-plugins-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn context(args: &str, cwd: &std::path::Path) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::from([("cwd".to_string(), json!(cwd))]),
        }
    }

    fn write_manifest(plugin_root: &std::path::Path, name: &str) {
        let manifest_dir = plugin_root.join(".codex-plugin");
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::write(
            manifest_dir.join("plugin.json"),
            serde_json::to_string_pretty(&json!({ "name": name })).unwrap(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn reload_plugins_reports_enabled_runtime_counts() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("counts");
        let cwd = root.join("project");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("alpha");
        write_manifest(&plugin_root, "alpha");
        fs::create_dir_all(plugin_root.join("commands")).unwrap();
        fs::create_dir_all(plugin_root.join("agents")).unwrap();
        fs::create_dir_all(plugin_root.join("skills").join("audit")).unwrap();
        fs::create_dir_all(plugin_root.join("hooks")).unwrap();
        fs::write(plugin_root.join("commands").join("hello.md"), "# hello").unwrap();
        fs::write(plugin_root.join("agents").join("review.md"), "# review").unwrap();
        fs::write(
            plugin_root.join("skills").join("audit").join("SKILL.md"),
            "# Audit",
        )
        .unwrap();
        fs::write(plugin_root.join("hooks").join("hooks.json"), "{}").unwrap();
        fs::write(plugin_root.join(".mcp.json"), "{}").unwrap();
        fs::write(plugin_root.join(".lsp.json"), "{}").unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let result = ReloadPluginsCommand
            .execute(context("", &cwd))
            .await
            .unwrap();

        assert!(result.value.contains("1 plugins"));
        assert!(result.value.contains("1 commands"));
        assert!(result.value.contains("1 agents"));
        assert!(result.value.contains("1 skills"));
        assert!(result.value.contains("1 hooks"));
        assert!(result.value.contains("1 plugin MCP servers"));
        assert!(result.value.contains("1 plugin LSP servers"));
        assert!(result.value.contains("LSP runtime: restarted on next use"));

        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn reload_plugins_rejects_extra_args() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("extra-args");
        let cwd = root.join("project");
        let plugins_dir = root.join("plugins");
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let result = ReloadPluginsCommand.execute(context("please", &cwd)).await;

        assert!(result.is_err());

        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }
}
