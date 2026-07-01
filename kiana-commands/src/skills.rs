use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use kiana_skills::{
    clear_caches, get_plugin_skill_dirs, get_skill_dirs_with_trust, load_all_skills_with_trust,
    Command as SkillCommand, LoadedFrom, SettingSource,
};
use kiana_types::project_trust_from_app_state;
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;

pub struct SkillsCommand;

#[async_trait]
impl Command for SkillsCommand {
    fn name(&self) -> &str {
        "skills"
    }

    fn description(&self) -> &str {
        "Manage skills"
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
            "" | "list" | "status" => list_skills(&context, rest).await,
            "json" => skills_json(&context, rest).await,
            "show" | "get" => show_skill(&context, rest).await,
            "path" | "paths" => skill_paths(&context, rest).await,
            "query" | "search" | "find" => list_skills(&context, rest).await,
            "help" | "--help" | "-h" => Ok(CommandResult::text(usage())),
            other => Err(anyhow!("unknown skills command '{}'\n\n{}", other, usage())),
        }
    }
}

async fn list_skills(context: &CommandContext, query: &str) -> Result<CommandResult> {
    let cwd = cwd(context);
    let skills = filtered_skills(load_skills(context, &cwd).await, query);
    let dirs = skill_dirs(context, &cwd).await;

    if skills.is_empty() {
        let suffix = if query.trim().is_empty() {
            "Create skills in .claude/skills, KIANA_HOME/skills, or ~/.claude/skills."
        } else {
            "No skills matched the query."
        };
        return Ok(CommandResult::text(format!(
            "No skills.\ncwd: {}\nskill_dirs: {}\n{}",
            cwd.display(),
            format_dirs(&dirs),
            suffix
        )));
    }

    let mut lines = vec![
        format!("{} skill(s)", skills.len()),
        format!("cwd: {}", cwd.display()),
        format!("skill_dirs: {}", format_dirs(&dirs)),
    ];
    if !query.trim().is_empty() {
        lines.push(format!("query: {}", query.trim()));
    }

    render_skill_group(
        &mut lines,
        "plugin skills",
        skills
            .iter()
            .filter(|skill| skill.loaded_from == LoadedFrom::Plugin)
            .collect(),
    );
    for source in [
        SettingSource::ProjectSettings,
        SettingSource::UserSettings,
        SettingSource::PolicySettings,
    ] {
        render_skill_group(
            &mut lines,
            source_label(source),
            skills
                .iter()
                .filter(|skill| skill.loaded_from != LoadedFrom::Plugin && skill.source == source)
                .collect(),
        );
    }
    lines
        .push("usage: kiana skills show <name> | json [query] | path [name] | query <text>".into());
    Ok(CommandResult::text(lines.join("\n")))
}

async fn skills_json(context: &CommandContext, query: &str) -> Result<CommandResult> {
    let cwd = cwd(context);
    let skills = filtered_skills(load_skills(context, &cwd).await, query);
    let payload: Vec<SkillSummary> = skills.iter().map(SkillSummary::from).collect();
    Ok(CommandResult::text(serde_json::to_string_pretty(&payload)?))
}

async fn show_skill(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let name = rest.trim();
    if name.is_empty() {
        return Err(anyhow!("usage: kiana skills show <name>"));
    }

    let cwd = cwd(context);
    let skills = load_skills(context, &cwd).await;
    let Some(skill) = find_skill(&skills, name) else {
        return Err(anyhow!("skill '{}' was not found", name));
    };

    Ok(CommandResult::text(serde_json::to_string_pretty(
        &SkillDetail::from(skill),
    )?))
}

async fn skill_paths(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let cwd = cwd(context);
    let name = rest.trim();
    if !name.is_empty() {
        let skills = load_skills(context, &cwd).await;
        let Some(skill) = find_skill(&skills, name) else {
            return Err(anyhow!("skill '{}' was not found", name));
        };
        let Some(root) = &skill.skill_root else {
            return Err(anyhow!("skill '{}' does not have a filesystem root", name));
        };
        return Ok(CommandResult::text(root.display().to_string()));
    }

    let dirs = skill_dirs(context, &cwd).await;
    if dirs.is_empty() {
        return Ok(CommandResult::text(format!(
            "No skill directories found.\ncwd: {}\nsearched: .claude/skills, KIANA_HOME/skills, ~/.claude/skills, KIANA_HOME/plugins/*/skills",
            cwd.display()
        )));
    }
    Ok(CommandResult::text(format_dirs(&dirs)))
}

async fn load_skills(context: &CommandContext, cwd: &std::path::Path) -> Vec<SkillCommand> {
    clear_caches();
    load_all_skills_with_trust(cwd, project_trust_from_app_state(&context.app_state)).await
}

async fn skill_dirs(context: &CommandContext, cwd: &std::path::Path) -> Vec<PathBuf> {
    let mut dirs =
        get_skill_dirs_with_trust(cwd, project_trust_from_app_state(&context.app_state)).await;
    dirs.extend(get_plugin_skill_dirs().await);
    dirs
}

fn filtered_skills(mut skills: Vec<SkillCommand>, query: &str) -> Vec<SkillCommand> {
    let query = query.trim().to_lowercase();
    if !query.is_empty() {
        skills.retain(|skill| skill_matches(skill, &query));
    }
    skills.sort_by(|a, b| {
        source_rank(a.source)
            .cmp(&source_rank(b.source))
            .then_with(|| skill_label(a).cmp(&skill_label(b)))
    });
    skills
}

fn find_skill<'a>(skills: &'a [SkillCommand], name: &str) -> Option<&'a SkillCommand> {
    let expected = name.trim().trim_start_matches('/').to_lowercase();
    skills.iter().find(|skill| {
        skill.name.eq_ignore_ascii_case(&expected)
            || skill
                .display_name
                .as_deref()
                .is_some_and(|display_name| display_name.eq_ignore_ascii_case(&expected))
    })
}

fn skill_matches(skill: &SkillCommand, query: &str) -> bool {
    [
        Some(skill.name.as_str()),
        skill.display_name.as_deref(),
        Some(skill.description.as_str()),
        skill.when_to_use.as_deref(),
        skill.skill_root.as_ref().and_then(|path| path.to_str()),
    ]
    .into_iter()
    .flatten()
    .any(|field| field.to_lowercase().contains(query))
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

fn format_dirs(dirs: &[PathBuf]) -> String {
    if dirs.is_empty() {
        return "none".to_string();
    }
    dirs.iter()
        .map(|dir| dir.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn skill_label(skill: &SkillCommand) -> String {
    skill
        .display_name
        .as_ref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(&skill.name)
        .to_string()
}

fn source_label(source: SettingSource) -> &'static str {
    match source {
        SettingSource::PolicySettings => "policySettings skills",
        SettingSource::UserSettings => "userSettings skills",
        SettingSource::ProjectSettings => "projectSettings skills",
    }
}

fn render_skill_group(lines: &mut Vec<String>, title: &str, mut group: Vec<&SkillCommand>) {
    if group.is_empty() {
        return;
    }
    group.sort_by(|a, b| skill_label(a).cmp(&skill_label(b)));
    lines.push(format!("{}:", title));
    for skill in group {
        lines.push(format!(
            "- {}{} [{}] {}",
            skill_label(skill),
            skill
                .argument_hint
                .as_ref()
                .map(|hint| format!(" {}", hint))
                .unwrap_or_default(),
            skill
                .skill_root
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "-".to_string()),
            skill.description
        ));
    }
}

fn source_rank(source: SettingSource) -> u8 {
    match source {
        SettingSource::ProjectSettings => 0,
        SettingSource::UserSettings => 1,
        SettingSource::PolicySettings => 2,
    }
}

fn loaded_from_label(loaded_from: LoadedFrom) -> &'static str {
    match loaded_from {
        LoadedFrom::CommandsDeprecated => "commands_DEPRECATED",
        LoadedFrom::Skills => "skills",
        LoadedFrom::Plugin => "plugin",
        LoadedFrom::Managed => "managed",
        LoadedFrom::Bundled => "bundled",
        LoadedFrom::Mcp => "mcp",
    }
}

fn usage() -> &'static str {
    "usage: kiana skills [list|status|json [query]|show <name>|path [name]|query <text>]"
}

#[derive(Serialize)]
struct SkillSummary {
    name: String,
    display_name: Option<String>,
    description: String,
    when_to_use: Option<String>,
    argument_hint: Option<String>,
    allowed_tools: Vec<String>,
    model: Option<String>,
    disable_model_invocation: bool,
    user_invocable: bool,
    source: SettingSource,
    loaded_from: &'static str,
    root: Option<String>,
}

impl From<&SkillCommand> for SkillSummary {
    fn from(skill: &SkillCommand) -> Self {
        Self {
            name: skill.name.clone(),
            display_name: skill.display_name.clone(),
            description: skill.description.clone(),
            when_to_use: skill.when_to_use.clone(),
            argument_hint: skill.argument_hint.clone(),
            allowed_tools: skill.allowed_tools.clone(),
            model: skill.model.clone(),
            disable_model_invocation: skill.disable_model_invocation,
            user_invocable: skill.user_invocable,
            source: skill.source,
            loaded_from: loaded_from_label(skill.loaded_from),
            root: skill
                .skill_root
                .as_ref()
                .map(|path| path.display().to_string()),
        }
    }
}

#[derive(Serialize)]
struct SkillDetail {
    #[serde(flatten)]
    summary: SkillSummary,
    context: Option<kiana_skills::ExecutionContext>,
    paths: Option<Vec<String>>,
    content: String,
}

impl From<&SkillCommand> for SkillDetail {
    fn from(skill: &SkillCommand) -> Self {
        Self {
            summary: SkillSummary::from(skill),
            context: skill.context.clone(),
            paths: skill.paths.clone(),
            content: skill.content.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SkillsCommand;
    use crate::local_state::env_lock;
    use crate::{Command, CommandContext};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-skills-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn context(args: &str, app_state: HashMap<String, Value>) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state,
        }
    }

    fn write_skill(root: &std::path::Path, name: &str, body: &str) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), body).unwrap();
    }

    fn write_plugin_skill(
        plugins_dir: &std::path::Path,
        plugin_name: &str,
        skill_name: &str,
        body: &str,
    ) {
        let plugin_root = plugins_dir.join(plugin_name);
        fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
        fs::write(
            plugin_root.join(".codex-plugin").join("plugin.json"),
            serde_json::to_string_pretty(&json!({ "name": plugin_name })).unwrap(),
        )
        .unwrap();
        let skill_dir = plugin_root.join("skills").join(skill_name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), body).unwrap();
    }

    #[tokio::test]
    async fn skills_lists_project_and_kiana_home_skills() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("list");
        let project = root.join("project");
        let kiana_home = root.join("kiana-home");
        let project_skills = project.join(".claude").join("skills");
        let home_skills = kiana_home.join("skills");
        write_skill(
            &project_skills,
            "project-audit",
            "---\nname: Project Audit\ndescription: Project skill description\nargument-hint: <path>\n---\nProject skill body\n",
        );
        write_skill(
            &home_skills,
            "home-review",
            "---\ndescription: Home skill description\n---\nHome skill body\n",
        );

        std::env::set_var("KIANA_HOME", &kiana_home);
        let app_state = HashMap::from([("cwd".to_string(), json!(project))]);

        let result = SkillsCommand
            .execute(context("", app_state.clone()))
            .await
            .unwrap();
        assert!(result.value.contains("projectSettings skills"));
        assert!(result.value.contains("Project Audit <path>"));
        assert!(result.value.contains("userSettings skills"));
        assert!(result.value.contains("home-review"));

        let json_result = SkillsCommand
            .execute(context("json project", app_state))
            .await
            .unwrap();
        let value: Value = serde_json::from_str(&json_result.value).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 1);
        assert_eq!(value[0]["name"], "project-audit");
        assert_eq!(value[0]["source"], "projectSettings");

        std::env::remove_var("KIANA_HOME");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn skills_command_hides_project_skills_when_project_is_untrusted() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string();
        let root = temp_root("untrusted");
        let project = root.join("project");
        let kiana_home = root.join("kiana-home");
        let project_skills = project.join(".claude").join("skills");
        let home_skills = kiana_home.join("skills");
        write_skill(
            &project_skills,
            &format!("project-{unique}"),
            &format!("---\ndescription: project {unique}\n---\nProject skill body\n"),
        );
        write_skill(
            &home_skills,
            &format!("home-{unique}"),
            &format!("---\ndescription: home {unique}\n---\nHome skill body\n"),
        );

        std::env::set_var("KIANA_HOME", &kiana_home);
        let app_state = HashMap::from([
            ("cwd".to_string(), json!(project)),
            ("project_trusted".to_string(), json!(false)),
        ]);

        let json_result = SkillsCommand
            .execute(context(&format!("json {unique}"), app_state))
            .await
            .unwrap();
        let value: Value = serde_json::from_str(&json_result.value).unwrap();

        assert_eq!(value.as_array().unwrap().len(), 1);
        assert_eq!(value[0]["name"], format!("home-{unique}"));
        assert_eq!(value[0]["source"], "userSettings");

        std::env::remove_var("KIANA_HOME");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn skills_show_and_path_use_loaded_skill_roots() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("show");
        let project = root.join("project");
        let project_skills = project.join(".claude").join("skills");
        write_skill(
            &project_skills,
            "explain",
            "---\ndescription: Explain code paths\nallowed-tools:\n  - Read\n  - Grep\n---\nUse this for code explanation.\n",
        );
        std::env::set_var("KIANA_HOME", root.join("empty-home"));
        let app_state = HashMap::from([("cwd".to_string(), json!(project))]);

        let shown = SkillsCommand
            .execute(context("show explain", app_state.clone()))
            .await
            .unwrap();
        let value: Value = serde_json::from_str(&shown.value).unwrap();
        assert_eq!(value["name"], "explain");
        assert_eq!(value["allowed_tools"], json!(["Read", "Grep"]));
        assert!(value["content"]
            .as_str()
            .unwrap()
            .contains("Use this for code explanation."));

        let path = SkillsCommand
            .execute(context("path explain", app_state))
            .await
            .unwrap();
        assert!(std::path::Path::new(&path.value).ends_with(
            std::path::Path::new(".claude")
                .join("skills")
                .join("explain")
        ));

        std::env::remove_var("KIANA_HOME");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn skills_lists_plugin_skills_as_plugin_group() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("plugin");
        let project = root.join("project");
        let plugins_dir = root.join("plugins");
        write_plugin_skill(
            &plugins_dir,
            "review-tools",
            "code-audit",
            "---\ndescription: Audit code from plugin\n---\nPlugin body\n",
        );
        std::env::set_var("KIANA_HOME", root.join("empty-home"));
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);
        let app_state = HashMap::from([("cwd".to_string(), json!(project))]);

        let result = SkillsCommand
            .execute(context("", app_state.clone()))
            .await
            .unwrap();
        assert!(result.value.contains("plugin skills"));
        assert!(result.value.contains("review-tools:code-audit"));

        let path = SkillsCommand
            .execute(context("path review-tools:code-audit", app_state))
            .await
            .unwrap();
        assert!(std::path::Path::new(&path.value)
            .ends_with(std::path::Path::new("skills").join("code-audit")));

        std::env::remove_var("KIANA_HOME");
        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }
}
