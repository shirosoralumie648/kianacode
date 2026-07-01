use crate::local_state::{load_user_config, save_user_config};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

pub const DEFAULT_OUTPUT_STYLE_NAME: &str = "default";

pub struct OutputStyleCommand;

#[derive(Debug, Clone, Serialize)]
pub struct OutputStyleConfig {
    pub name: String,
    pub description: String,
    pub prompt: Option<String>,
    pub source: String,
    pub keep_coding_instructions: Option<bool>,
    pub force_for_plugin: bool,
    pub path: Option<PathBuf>,
}

#[async_trait]
impl Command for OutputStyleCommand {
    fn name(&self) -> &str {
        "output-style"
    }

    fn description(&self) -> &str {
        "List and select output styles"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let (command, rest) = split_word(context.args.trim());
        match command.unwrap_or("list") {
            "" | "list" | "status" => {
                reject_unexpected_rest("output-style list", rest)?;
                list_styles(&context)
            }
            "json" => {
                reject_unexpected_rest("output-style json", rest)?;
                styles_json(&context)
            }
            "get" | "show" => show_style(&context, rest),
            "set" | "use" => set_style(&context, rest),
            "unset" | "reset" => reset_style(&context, rest),
            "help" | "--help" | "-h" => {
                reject_unexpected_rest("output-style help", rest)?;
                Ok(CommandResult::text(usage()))
            }
            other => Err(anyhow!(
                "unknown output-style command '{}'\n\n{}",
                other,
                usage()
            )),
        }
    }
}

fn reject_unexpected_rest(command: &str, rest: &str) -> Result<()> {
    if rest.trim().is_empty() {
        return Ok(());
    }
    Err(anyhow!("usage: kiana {command}\n\n{}", usage()))
}

pub fn available_output_style_names(cwd: &Path) -> Vec<String> {
    let mut names = load_all_output_styles(cwd)
        .into_iter()
        .map(|style| style.name)
        .collect::<Vec<_>>();
    names.sort();
    names
}

pub fn selected_output_style_name(cwd: &Path) -> String {
    let styles = load_all_output_styles(cwd);
    if let Some(style) = styles.iter().find(|style| style.force_for_plugin) {
        return style.name.clone();
    }

    let configured = kiana_bootstrap::config::load_config()
        .settings
        .output_style
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_OUTPUT_STYLE_NAME)
        .to_string();
    if styles
        .iter()
        .any(|style| style.name.eq_ignore_ascii_case(&configured))
    {
        configured
    } else {
        DEFAULT_OUTPUT_STYLE_NAME.to_string()
    }
}

pub fn load_all_output_styles(cwd: &Path) -> Vec<OutputStyleConfig> {
    let mut by_name = HashMap::new();
    for style in built_in_output_styles() {
        by_name.insert(style.name.to_ascii_lowercase(), style);
    }
    for style in load_plugin_output_styles() {
        by_name.insert(style.name.to_ascii_lowercase(), style);
    }
    for style in load_local_output_styles(cwd) {
        by_name.insert(style.name.to_ascii_lowercase(), style);
    }
    let mut styles = by_name.into_values().collect::<Vec<_>>();
    styles.sort_by(|a, b| a.name.cmp(&b.name));
    styles
}

fn list_styles(context: &CommandContext) -> Result<CommandResult> {
    let cwd = cwd(context);
    let selected = selected_output_style_name(&cwd);
    let styles = load_all_output_styles(&cwd);
    let mut lines = vec![
        format!("{} output style(s)", styles.len()),
        format!("current: {}", selected),
    ];
    for style in styles {
        let marker = if style.name.eq_ignore_ascii_case(&selected) {
            "*"
        } else {
            "-"
        };
        let forced = if style.force_for_plugin {
            " forced"
        } else {
            ""
        };
        lines.push(format!(
            "{marker} {} [{}{}] {}",
            style.name, style.source, forced, style.description
        ));
    }
    lines.push("usage: kiana output-style set <name> | show <name> | json | reset".into());
    Ok(CommandResult::text(lines.join("\n")))
}

fn styles_json(context: &CommandContext) -> Result<CommandResult> {
    let cwd = cwd(context);
    let styles = load_all_output_styles(&cwd);
    Ok(CommandResult::text(serde_json::to_string_pretty(&styles)?))
}

fn show_style(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let target = rest.trim();
    if target.is_empty() {
        return Err(anyhow!("usage: kiana output-style show <name>"));
    }
    let cwd = cwd(context);
    let style = resolve_style(&cwd, target)?;
    Ok(CommandResult::text(serde_json::to_string_pretty(&style)?))
}

fn set_style(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let target = rest.trim();
    if target.is_empty() {
        return Err(anyhow!("usage: kiana output-style set <name>"));
    }
    let cwd = cwd(context);
    let style = resolve_style(&cwd, target)?;
    let mut config = load_user_config();
    config.settings.output_style = if style.name == DEFAULT_OUTPUT_STYLE_NAME {
        None
    } else {
        Some(style.name.clone())
    };
    let path = save_user_config(&config)?;
    Ok(CommandResult::text(format!(
        "Output style updated\noutput_style: {}\nfile: {}",
        style.name,
        path.display()
    )))
}

fn reset_style(_context: &CommandContext, rest: &str) -> Result<CommandResult> {
    if !rest.trim().is_empty() {
        return Err(anyhow!("usage: kiana output-style reset"));
    }
    let mut config = load_user_config();
    config.settings.output_style = None;
    let path = save_user_config(&config)?;
    Ok(CommandResult::text(format!(
        "Output style reset\noutput_style: {}\nfile: {}",
        DEFAULT_OUTPUT_STYLE_NAME,
        path.display()
    )))
}

fn resolve_style(cwd: &Path, target: &str) -> Result<OutputStyleConfig> {
    let normalized = target.trim().trim_start_matches('/').to_ascii_lowercase();
    load_all_output_styles(cwd)
        .into_iter()
        .find(|style| style.name.to_ascii_lowercase() == normalized)
        .ok_or_else(|| anyhow!("unknown output style '{}'", target.trim()))
}

fn built_in_output_styles() -> Vec<OutputStyleConfig> {
    vec![
        OutputStyleConfig {
            name: DEFAULT_OUTPUT_STYLE_NAME.to_string(),
            description: "Default coding assistant behavior".to_string(),
            prompt: None,
            source: "built-in".to_string(),
            keep_coding_instructions: None,
            force_for_plugin: false,
            path: None,
        },
        OutputStyleConfig {
            name: "Explanatory".to_string(),
            description: "Explains implementation choices and codebase patterns".to_string(),
            prompt: Some(
                "Provide concise educational explanations while completing coding tasks."
                    .to_string(),
            ),
            source: "built-in".to_string(),
            keep_coding_instructions: Some(true),
            force_for_plugin: false,
            path: None,
        },
        OutputStyleConfig {
            name: "Learning".to_string(),
            description: "Uses small hands-on prompts when meaningful".to_string(),
            prompt: Some(
                "Help the user learn by asking for small contributions when appropriate."
                    .to_string(),
            ),
            source: "built-in".to_string(),
            keep_coding_instructions: Some(true),
            force_for_plugin: false,
            path: None,
        },
    ]
}

fn load_local_output_styles(cwd: &Path) -> Vec<OutputStyleConfig> {
    let mut styles = Vec::new();
    for dir in output_style_dirs(cwd) {
        for path in read_markdown_files_sorted(&dir) {
            if let Some(style) = load_output_style_file(&path, None, "local", false) {
                styles.push(style);
            }
        }
    }
    styles
}

fn load_plugin_output_styles() -> Vec<OutputStyleConfig> {
    let mut styles = Vec::new();
    for plugin_root in kiana_types::plugin::installed_plugin_roots() {
        let Some(manifest_path) = kiana_types::plugin::find_manifest_path(&plugin_root) else {
            continue;
        };
        let manifest = read_json_file(&manifest_path);
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
        let mut sources = Vec::new();
        push_output_style_source(&mut sources, plugin_root.join("output-styles"));
        if let Some(manifest) = manifest.as_ref() {
            push_manifest_output_style_sources(&mut sources, &plugin_root, manifest);
        }
        dedupe_paths(&mut sources);
        let mut loaded_paths = HashSet::new();
        for source in sources {
            if source.is_file() {
                if loaded_paths.insert(path_key(&source)) {
                    if let Some(style) =
                        load_output_style_file(&source, Some(&plugin_name), "plugin", true)
                    {
                        styles.push(style);
                    }
                }
            } else if source.is_dir() {
                for path in read_markdown_files_recursive(&source) {
                    if loaded_paths.insert(path_key(&path)) {
                        if let Some(style) =
                            load_output_style_file(&path, Some(&plugin_name), "plugin", true)
                        {
                            styles.push(style);
                        }
                    }
                }
            }
        }
    }
    styles
}

fn load_output_style_file(
    path: &Path,
    plugin_name: Option<&str>,
    source: &str,
    allow_force: bool,
) -> Option<OutputStyleConfig> {
    let contents = std::fs::read_to_string(path).ok()?;
    let (frontmatter, body) = split_frontmatter(&contents);
    let file_name = path.file_stem()?.to_str()?.to_string();
    let name = frontmatter
        .as_ref()
        .and_then(|frontmatter| frontmatter.name.clone())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or(file_name);
    let name = plugin_name
        .map(|plugin| format!("{plugin}:{name}"))
        .unwrap_or(name);
    let prompt = body.trim().to_string();
    if prompt.is_empty() {
        return None;
    }
    let description = frontmatter
        .as_ref()
        .and_then(|frontmatter| frontmatter.description.clone())
        .map(|description| description.trim().to_string())
        .filter(|description| !description.is_empty())
        .or_else(|| first_markdown_line(&prompt))
        .unwrap_or_else(|| format!("Output style: {name}"));
    let keep_coding_instructions = frontmatter
        .as_ref()
        .and_then(|frontmatter| parse_bool_like(frontmatter.keep_coding_instructions.as_ref()));
    let force_for_plugin = allow_force
        && frontmatter
            .as_ref()
            .and_then(|frontmatter| parse_bool_like(frontmatter.force_for_plugin.as_ref()))
            .unwrap_or(false);

    Some(OutputStyleConfig {
        name,
        description,
        prompt: Some(prompt),
        source: source.to_string(),
        keep_coding_instructions,
        force_for_plugin,
        path: Some(path.to_path_buf()),
    })
}

fn output_style_dirs(cwd: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home_dir() {
        push_existing_dir(&mut dirs, home.join(".claude").join("output-styles"));
    }
    if let Some(home) = std::env::var_os("KIANA_HOME") {
        push_existing_dir(&mut dirs, PathBuf::from(home).join("output-styles"));
    }
    let mut project_dirs = Vec::new();
    let mut current = Some(cwd);
    while let Some(dir) = current {
        let styles_dir = dir.join(".claude").join("output-styles");
        if styles_dir.is_dir() {
            project_dirs.push(styles_dir);
        }
        current = dir.parent();
    }
    project_dirs.reverse();
    dirs.extend(project_dirs);
    dedupe_vec(dirs)
}

fn push_manifest_output_style_sources(
    sources: &mut Vec<PathBuf>,
    plugin_root: &Path,
    manifest: &Value,
) {
    let Some(output_styles) = manifest.get("outputStyles") else {
        return;
    };
    match output_styles {
        Value::String(path) => push_safe_output_style_source(sources, plugin_root, path),
        Value::Array(paths) => {
            for path in paths {
                if let Some(path) = path.as_str() {
                    push_safe_output_style_source(sources, plugin_root, path);
                }
            }
        }
        _ => {}
    }
}

fn push_safe_output_style_source(sources: &mut Vec<PathBuf>, plugin_root: &Path, path: &str) {
    let Some(path) = safe_relative_path(path) else {
        return;
    };
    push_output_style_source(sources, plugin_root.join(path));
}

fn push_output_style_source(sources: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_dir()
        || path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            && path.is_file()
    {
        sources.push(path);
    }
}

fn read_markdown_files_sorted(dir: &Path) -> Vec<PathBuf> {
    let mut files = std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn read_markdown_files_recursive(dir: &Path) -> Vec<PathBuf> {
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
            visit_markdown_files(&path, files);
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        {
            files.push(path);
        }
    }
}

fn split_frontmatter(content: &str) -> (Option<StyleFrontmatter>, String) {
    let Some(rest) = content.strip_prefix("---\n") else {
        return (None, content.trim().to_string());
    };
    let Some(end) = rest.find("\n---") else {
        return (None, content.trim().to_string());
    };
    let frontmatter = serde_yaml::from_str::<StyleFrontmatter>(&rest[..end]).ok();
    let body = rest[end + 4..].trim_start_matches(['\r', '\n']).to_string();
    (frontmatter, body)
}

#[derive(Debug, Default, Deserialize)]
struct StyleFrontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(rename = "keep-coding-instructions", alias = "keepCodingInstructions")]
    keep_coding_instructions: Option<Value>,
    #[serde(rename = "force-for-plugin", alias = "forceForPlugin")]
    force_for_plugin: Option<Value>,
}

fn parse_bool_like(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Bool(value) => Some(*value),
        Value::String(value) => match value.trim() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn first_markdown_line(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.trim_start_matches('#').trim().to_string())
        .filter(|line| !line.is_empty())
}

fn read_json_file(path: &Path) -> Option<Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
}

fn safe_relative_path(path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return None;
    }
    Some(path)
}

fn push_existing_dir(dirs: &mut Vec<PathBuf>, dir: PathBuf) {
    if dir.is_dir() {
        dirs.push(dir);
    }
}

fn dedupe_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = HashSet::new();
    paths.retain(|path| seen.insert(path_key(path)));
}

fn dedupe_vec(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path_key(path)))
        .collect()
}

fn path_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn cwd(context: &CommandContext) -> PathBuf {
    context
        .app_state
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn split_word(input: &str) -> (Option<&str>, &str) {
    let input = input.trim();
    if input.is_empty() {
        return (None, "");
    }
    match input.find(char::is_whitespace) {
        Some(index) => (Some(&input[..index]), input[index..].trim()),
        None => (Some(input), ""),
    }
}

fn usage() -> &'static str {
    "usage: kiana output-style [list|json|show <name>|set <name>|reset]"
}

#[cfg(test)]
mod tests {
    use super::{available_output_style_names, selected_output_style_name, OutputStyleCommand};
    use crate::local_state::env_lock;
    use crate::{Command, CommandContext};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-output-style-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn context(args: &str, cwd: &Path) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::from([("cwd".to_string(), json!(cwd))]),
        }
    }

    fn write_plugin_manifest(plugin_root: &Path, name: &str, extra: Value) {
        fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
        let mut manifest = serde_json::Map::new();
        manifest.insert("name".to_string(), json!(name));
        if let Some(extra) = extra.as_object() {
            for (key, value) in extra {
                manifest.insert(key.clone(), value.clone());
            }
        }
        fs::write(
            plugin_root.join(".codex-plugin").join("plugin.json"),
            Value::Object(manifest).to_string(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn output_style_lists_local_and_plugin_styles() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("list");
        let cwd = root.join("project");
        let local_dir = cwd.join(".claude").join("output-styles");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("review-tools");
        fs::create_dir_all(&local_dir).unwrap();
        fs::write(
            local_dir.join("Concise.md"),
            "---\ndescription: Keep answers short\n---\nUse concise responses.\n",
        )
        .unwrap();
        fs::create_dir_all(plugin_root.join("output-styles")).unwrap();
        write_plugin_manifest(&plugin_root, "review-tools", json!({}));
        fs::write(
            plugin_root.join("output-styles").join("Strict.md"),
            "---\ndescription: Strict review mode\nforce-for-plugin: true\n---\nReview strictly.\n",
        )
        .unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let result = OutputStyleCommand
            .execute(context("list", &cwd))
            .await
            .unwrap();
        assert!(result.value.contains("current: review-tools:Strict"));
        assert!(result.value.contains("Concise [local] Keep answers short"));
        assert!(result
            .value
            .contains("review-tools:Strict [plugin forced] Strict review mode"));
        let names = available_output_style_names(&cwd);
        assert!(names.iter().any(|name| name == "Concise"));
        assert!(names.iter().any(|name| name == "review-tools:Strict"));
        assert_eq!(selected_output_style_name(&cwd), "review-tools:Strict");

        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn output_style_set_persists_config_and_manifest_paths_are_safe() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("set");
        let cwd = root.join("project");
        let config_path = root.join("config.toml");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("ops");
        fs::create_dir_all(cwd.join(".claude").join("output-styles")).unwrap();
        fs::write(
            cwd.join(".claude").join("output-styles").join("Project.md"),
            "---\ndescription: Project style\n---\nUse project style.\n",
        )
        .unwrap();
        fs::create_dir_all(plugin_root.join("extra")).unwrap();
        write_plugin_manifest(
            &plugin_root,
            "ops",
            json!({ "outputStyles": ["extra/Runbook.md", "../outside.md"] }),
        );
        fs::write(
            plugin_root.join("extra").join("Runbook.md"),
            "---\ndescription: Runbook style\n---\nUse runbook style.\n",
        )
        .unwrap();
        fs::write(
            plugins_dir.join("outside.md"),
            "---\nname: outside\n---\nShould not load.\n",
        )
        .unwrap();
        std::env::set_var("KIANA_CONFIG_FILE", &config_path);
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let output = OutputStyleCommand
            .execute(context("set Project", &cwd))
            .await
            .unwrap();
        assert!(output.value.contains("output_style: Project"));
        assert_eq!(selected_output_style_name(&cwd), "Project");
        let names = available_output_style_names(&cwd);
        assert!(names.iter().any(|name| name == "ops:Runbook"));
        assert!(!names.iter().any(|name| name == "ops:outside"));

        OutputStyleCommand
            .execute(context("reset", &cwd))
            .await
            .unwrap();
        assert_eq!(selected_output_style_name(&cwd), "default");

        std::env::remove_var("KIANA_CONFIG_FILE");
        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn output_style_reset_rejects_extra_args_without_mutating_config() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("reset-extra");
        let cwd = root.join("project");
        let config_path = root.join("config.toml");
        fs::create_dir_all(cwd.join(".claude").join("output-styles")).unwrap();
        fs::write(
            cwd.join(".claude").join("output-styles").join("Project.md"),
            "---\ndescription: Project style\n---\nUse project style.\n",
        )
        .unwrap();
        std::env::set_var("KIANA_CONFIG_FILE", &config_path);

        OutputStyleCommand
            .execute(context("set Project", &cwd))
            .await
            .unwrap();

        let reset = OutputStyleCommand
            .execute(context("reset please", &cwd))
            .await;

        assert!(reset.is_err());
        assert_eq!(selected_output_style_name(&cwd), "Project");

        std::env::remove_var("KIANA_CONFIG_FILE");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn output_style_no_arg_subcommands_reject_extra_args() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("no-arg-extra");
        let cwd = root.join("project");

        for args in [
            "list Project",
            "status Project",
            "json Project",
            "help Project",
        ] {
            let error = OutputStyleCommand
                .execute(context(args, &cwd))
                .await
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("usage: kiana output-style"),
                "{args} returned wrong error: {error}"
            );
        }

        let _ = fs::remove_dir_all(root);
    }
}
