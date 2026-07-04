use crate::types::{Command, CommandContext, CommandResult, CommandType};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PluginPromptCommand {
    name: String,
    description: String,
    content: String,
    plugin_root: PathBuf,
    argument_names: Vec<String>,
    hidden: bool,
}

#[async_trait]
impl Command for PluginPromptCommand {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn command_type(&self) -> CommandType {
        CommandType::Prompt
    }

    fn is_hidden(&self) -> bool {
        self.hidden
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let session_id = context
            .app_state
            .get("session_id")
            .or_else(|| context.app_state.get("sessionId"))
            .and_then(Value::as_str);
        Ok(CommandResult::text(render_prompt_content(
            &self.content,
            &context.args,
            &self.argument_names,
            &self.plugin_root,
            session_id,
        )))
    }
}

pub fn load_plugin_prompt_commands() -> Vec<PluginPromptCommand> {
    let mut commands = Vec::new();
    for plugin_root in installed_plugin_roots() {
        let Some(manifest_path) = find_manifest_path(&plugin_root) else {
            continue;
        };
        let manifest = read_manifest(&manifest_path);
        let plugin_name = manifest
            .as_ref()
            .and_then(|value| value.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .or_else(|| {
                plugin_root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "plugin".to_string());

        let mut command_sources = Vec::new();
        push_command_source(&mut command_sources, plugin_root.join("commands"));
        if let Some(manifest) = manifest.as_ref() {
            push_manifest_command_sources(&mut command_sources, &plugin_root, manifest);
        }
        dedupe_paths(&mut command_sources);

        for source in command_sources {
            if source.is_file() {
                if let Some(command) =
                    load_plugin_command_file(&plugin_name, &plugin_root, &source, source.parent())
                {
                    commands.push(command);
                }
            } else {
                commands.extend(load_plugin_commands_from_dir(
                    &plugin_name,
                    &plugin_root,
                    &source,
                ));
            }
        }
    }

    commands.sort_by(|a, b| a.name.cmp(&b.name));
    dedupe_commands(commands)
}

fn load_plugin_commands_from_dir(
    plugin_name: &str,
    plugin_root: &Path,
    commands_dir: &Path,
) -> Vec<PluginPromptCommand> {
    let mut commands = Vec::new();
    for file in markdown_files(commands_dir) {
        if let Some(command) =
            load_plugin_command_file(plugin_name, plugin_root, &file, Some(commands_dir))
        {
            commands.push(command);
        }
    }
    commands
}

fn load_plugin_command_file(
    plugin_name: &str,
    plugin_root: &Path,
    file: &Path,
    base_dir: Option<&Path>,
) -> Option<PluginPromptCommand> {
    if file
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
    {
        return None;
    }
    let content = std::fs::read_to_string(file).ok()?;
    let (frontmatter, body) = parse_frontmatter(&content);
    let rel_name = command_name_from_path(
        file,
        base_dir.unwrap_or_else(|| file.parent().unwrap_or(file)),
    );
    let name = format!("{plugin_name}:{rel_name}");
    let description = frontmatter
        .description
        .clone()
        .or_else(|| first_markdown_line(&body))
        .unwrap_or_else(|| format!("Plugin command: {name}"));
    let argument_names = frontmatter
        .arguments
        .as_ref()
        .map(parse_argument_names)
        .unwrap_or_default();

    Some(PluginPromptCommand {
        name,
        description,
        content: body,
        plugin_root: plugin_root.to_path_buf(),
        argument_names,
        hidden: frontmatter.user_invocable == Some(false),
    })
}

fn markdown_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    visit_markdown_files(dir, &mut files);
    files.sort();
    files
}

fn visit_markdown_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.join("SKILL.md").is_file() {
                continue;
            }
            visit_markdown_files(&path, files);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            files.push(path);
        }
    }
}

fn command_name_from_path(file: &Path, base_dir: &Path) -> String {
    let rel = file.strip_prefix(base_dir).unwrap_or(file);
    let mut parts = rel
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str().map(str::to_string),
            _ => None,
        })
        .collect::<Vec<_>>();
    if let Some(last) = parts.last_mut() {
        if let Some(stripped) = last.strip_suffix(".md") {
            *last = stripped.to_string();
        }
    }
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(":")
}

#[derive(Debug, Default, Deserialize)]
struct CommandFrontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(rename = "argument-hint")]
    argument_hint: Option<String>,
    #[serde(rename = "user-invocable")]
    user_invocable: Option<bool>,
    arguments: Option<ArgumentNames>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ArgumentNames {
    String(String),
    Array(Vec<String>),
}

fn parse_frontmatter(content: &str) -> (CommandFrontmatter, String) {
    let Some(rest) = content.strip_prefix("---\n") else {
        return (CommandFrontmatter::default(), content.trim().to_string());
    };
    let Some(end) = rest.find("\n---") else {
        return (CommandFrontmatter::default(), content.trim().to_string());
    };
    let frontmatter = serde_yaml::from_str::<CommandFrontmatter>(&rest[..end]).unwrap_or_default();
    let mut body = rest[end + 4..].trim_start_matches(['\r', '\n']).to_string();
    if let Some(name) = frontmatter
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
    {
        body = body.replace("${COMMAND_DISPLAY_NAME}", name.trim());
    }
    if let Some(hint) = frontmatter
        .argument_hint
        .as_deref()
        .filter(|hint| !hint.trim().is_empty())
    {
        body = body.replace("${COMMAND_ARGUMENT_HINT}", hint.trim());
    }
    (frontmatter, body)
}

fn first_markdown_line(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.trim_start_matches('#').trim().to_string())
        .filter(|line| !line.is_empty())
}

fn parse_argument_names(arguments: &ArgumentNames) -> Vec<String> {
    let names = match arguments {
        ArgumentNames::String(value) => value.split_whitespace().map(str::to_string).collect(),
        ArgumentNames::Array(values) => values.clone(),
    };
    names
        .into_iter()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty() && !name.chars().all(|ch| ch.is_ascii_digit()))
        .collect()
}

fn render_prompt_content(
    content: &str,
    args: &str,
    argument_names: &[String],
    plugin_root: &Path,
    session_id: Option<&str>,
) -> String {
    let mut rendered = content.to_string();
    let original = rendered.clone();
    let parsed_args = args.split_whitespace().collect::<Vec<_>>();

    for (index, name) in argument_names.iter().enumerate() {
        rendered = rendered.replace(
            &format!("${name}"),
            parsed_args.get(index).copied().unwrap_or(""),
        );
    }
    for (index, value) in parsed_args.iter().enumerate() {
        rendered = rendered.replace(&format!("$ARGUMENTS[{index}]"), value);
        rendered = rendered.replace(&format!("${index}"), value);
    }
    rendered = rendered.replace("$ARGUMENTS", args);
    if rendered == original && !args.trim().is_empty() {
        rendered.push_str("\n\nARGUMENTS: ");
        rendered.push_str(args);
    }

    let plugin_root = plugin_root.to_string_lossy();
    rendered = rendered.replace("${CLAUDE_PLUGIN_ROOT}", &plugin_root);
    rendered = rendered.replace("${KIANA_PLUGIN_ROOT}", &plugin_root);
    if let Some(session_id) = session_id {
        rendered = rendered.replace("${CLAUDE_SESSION_ID}", session_id);
        rendered = rendered.replace("${KIANA_SESSION_ID}", session_id);
    }
    rendered
}

fn push_manifest_command_sources(dirs: &mut Vec<PathBuf>, plugin_root: &Path, manifest: &Value) {
    let Some(commands) = manifest.get("commands") else {
        return;
    };
    match commands {
        Value::String(path) => push_safe_command_source(dirs, plugin_root, path),
        Value::Array(paths) => {
            for path in paths {
                if let Some(path) = path.as_str() {
                    push_safe_command_source(dirs, plugin_root, path);
                }
            }
        }
        _ => {}
    }
}

fn push_safe_command_source(dirs: &mut Vec<PathBuf>, plugin_root: &Path, path: &str) {
    let Some(path) = safe_relative_path(path) else {
        return;
    };
    push_command_source(dirs, plugin_root.join(path));
}

fn push_command_source(dirs: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_dir()
        || path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
            && path.is_file()
    {
        dirs.push(path);
    }
}

fn safe_relative_path(path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty() {
        return None;
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(path)
}

fn installed_plugin_roots() -> Vec<PathBuf> {
    kiana_types::plugin::installed_plugin_roots()
}

fn read_manifest(path: &Path) -> Option<Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
}

fn find_manifest_path(plugin_root: &Path) -> Option<PathBuf> {
    [
        plugin_root.join(".codex-plugin").join("plugin.json"),
        plugin_root.join(".claude-plugin").join("plugin.json"),
        plugin_root.join("plugin.json"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn dedupe_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = HashSet::new();
    paths.retain(|path| {
        seen.insert(
            path.canonicalize()
                .unwrap_or_else(|_| path.to_path_buf())
                .to_string_lossy()
                .to_string(),
        )
    });
}

fn dedupe_commands(commands: Vec<PluginPromptCommand>) -> Vec<PluginPromptCommand> {
    let mut seen = HashSet::new();
    commands
        .into_iter()
        .filter(|command| seen.insert(command.name.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_state::env_lock;
    use serde_json::json;
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-plugin-command-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn write_plugin_manifest(plugin_root: &Path, name: &str, extra: Value) {
        std::fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
        let mut manifest = serde_json::Map::new();
        manifest.insert("name".to_string(), json!(name));
        if let Some(extra) = extra.as_object() {
            for (key, value) in extra {
                manifest.insert(key.clone(), value.clone());
            }
        }
        std::fs::write(
            plugin_root.join(".codex-plugin").join("plugin.json"),
            Value::Object(manifest).to_string(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn loads_plugin_markdown_commands_into_prompt_commands() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("basic");
        let plugin_root = root.join("plugins").join("review-tools");
        write_plugin_manifest(&plugin_root, "review-tools", json!({}));
        std::fs::create_dir_all(plugin_root.join("commands").join("git")).unwrap();
        std::fs::write(
            plugin_root.join("commands").join("git").join("audit.md"),
            "---\ndescription: Audit git changes\narguments: target\n---\nReview $target in ${CLAUDE_PLUGIN_ROOT} for ${CLAUDE_SESSION_ID}.",
        )
        .unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", root.join("plugins"));

        let commands = load_plugin_prompt_commands();
        let command = commands
            .iter()
            .find(|command| command.name() == "review-tools:git:audit")
            .expect("plugin command loaded");
        assert_eq!(command.description(), "Audit git changes");
        let result = command
            .execute(CommandContext {
                args: "src/lib.rs".to_string(),
                app_state: HashMap::from([("session_id".to_string(), json!("session-1"))]),
            })
            .await
            .unwrap();
        assert!(result.value.contains("Review src/lib.rs"));
        assert!(result
            .value
            .contains(&plugin_root.to_string_lossy().to_string()));
        assert!(result.value.contains("session-1"));

        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn loads_manifest_extra_command_paths_and_hides_non_user_invocable() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("manifest");
        let plugin_root = root.join("plugins").join("ops");
        write_plugin_manifest(
            &plugin_root,
            "ops",
            json!({ "commands": ["extra/runbook.md", "../outside.md"] }),
        );
        std::fs::create_dir_all(plugin_root.join("extra")).unwrap();
        std::fs::write(
            plugin_root.join("extra").join("runbook.md"),
            "---\ndescription: Hidden runbook\nuser-invocable: false\n---\nRunbook $ARGUMENTS",
        )
        .unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", root.join("plugins"));

        let commands = load_plugin_prompt_commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name(), "ops:runbook");
        assert!(commands[0].is_hidden());
        let result = commands[0]
            .execute(CommandContext {
                args: "deploy now".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();
        assert_eq!(result.value, "Runbook deploy now");

        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn skips_disabled_plugin_prompt_commands() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("disabled");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("review-tools");
        write_plugin_manifest(&plugin_root, "review-tools", json!({}));
        std::fs::create_dir_all(plugin_root.join("commands")).unwrap();
        std::fs::write(
            plugin_root.join("commands").join("audit.md"),
            "---\ndescription: Audit git changes\n---\nReview changes.",
        )
        .unwrap();
        kiana_types::plugin::set_plugin_enabled(&plugins_dir, "review-tools", false).unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let commands = load_plugin_prompt_commands();
        assert!(commands
            .iter()
            .all(|command| command.name() != "review-tools:audit"));

        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = std::fs::remove_dir_all(root);
    }
}
