use crate::tool::*;
use async_trait::async_trait;
use kiana_types::{project_trust_from_app_state, project_trust_root, ProjectTrust};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct DiscoverSkillsInput {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    include_content: bool,
    #[serde(default)]
    max_results: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    when_to_use: Option<String>,
    #[serde(rename = "argument-hint")]
    argument_hint: Option<String>,
    #[serde(rename = "allowed-tools")]
    allowed_tools: Option<Vec<String>>,
    #[serde(rename = "user-invocable")]
    user_invocable: Option<bool>,
    model: Option<String>,
    context: Option<String>,
    paths: Option<String>,
}

#[derive(Debug)]
struct SkillRecord {
    name: String,
    display_name: Option<String>,
    description: String,
    when_to_use: Option<String>,
    argument_hint: Option<String>,
    allowed_tools: Vec<String>,
    user_invocable: bool,
    model: Option<String>,
    context: Option<String>,
    paths: Option<Vec<String>>,
    source: String,
    skill_root: PathBuf,
    content: String,
}

pub struct DiscoverSkillsTool;

impl DiscoverSkillsTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for DiscoverSkillsTool {
    fn name(&self) -> &str {
        "discover_skills"
    }

    fn description(&self) -> &str {
        "Discover available project and user skills"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("list available skills")
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Filter skills by name, description, or usage text"
                },
                "include_content": {
                    "type": "boolean",
                    "description": "Include SKILL.md body content in each result"
                },
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 200
                }
            }
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "skills": { "type": "array" },
                "count": { "type": "integer" },
                "skill_dirs": { "type": "array" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: DiscoverSkillsInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(error) => return ValidationResult::err(format!("Invalid input: {error}"), 1),
        };
        if input
            .query
            .as_deref()
            .is_some_and(|query| query.len() > 512)
        {
            return ValidationResult::err("query must be at most 512 characters".to_string(), 2);
        }
        if input
            .max_results
            .is_some_and(|max_results| max_results == 0 || max_results > 200)
        {
            return ValidationResult::err("max_results must be between 1 and 200".to_string(), 3);
        }
        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: DiscoverSkillsInput = serde_json::from_value(input.clone())?;
        let dirs = discover_skill_dirs_with_trust(
            &context.cwd,
            project_trust_from_app_state(&context.app_state),
        );
        let mut records = load_skill_records(&dirs)?;
        let query = input
            .query
            .as_deref()
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .map(str::to_ascii_lowercase);

        if let Some(query) = query {
            records.retain(|record| skill_matches_query(record, &query));
        }

        records.truncate(input.max_results.unwrap_or(50));
        let skills = records
            .into_iter()
            .map(|record| skill_record_to_json(record, input.include_content))
            .collect::<Vec<_>>();
        let count = skills.len();

        Ok(ToolOutput {
            data: json!({
                "skills": skills,
                "count": count,
                "skill_dirs": dirs
                    .into_iter()
                    .map(|dir| dir.to_string_lossy().to_string())
                    .collect::<Vec<_>>()
            }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": format_discovered_skills_for_model(&output.data)
        })
    }
}

fn format_discovered_skills_for_model(data: &Value) -> String {
    let count = data
        .get("count")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            data.get("skills")
                .and_then(Value::as_array)
                .map(|skills| skills.len() as u64)
                .unwrap_or(0)
        });
    let mut lines = vec![format!("{count} skill(s) found.")];

    if let Some(skills) = data.get("skills").and_then(Value::as_array) {
        for skill in skills {
            let name = skill
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let display_name = skill
                .get("display_name")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty());
            let description = skill
                .get("description")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("No description");
            let title = display_name.unwrap_or(name);
            lines.push(format!("- {name} ({title}): {description}"));
            push_skill_field(&mut lines, "When to use", skill, "when_to_use");
            push_skill_field(&mut lines, "Argument hint", skill, "argument_hint");
            push_skill_array_field(&mut lines, "Allowed tools", skill, "allowed_tools");
            push_skill_field(&mut lines, "Source", skill, "source");
            push_skill_field(&mut lines, "Root", skill, "skill_root");
            if let Some(content) = skill
                .get("content")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|content| !content.is_empty())
            {
                lines.push(format!("  Content:\n{}", indent_lines(content, "    ")));
            }
        }
    }

    lines.join("\n")
}

fn push_skill_field(lines: &mut Vec<String>, label: &str, skill: &Value, key: &str) {
    if let Some(value) = skill
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("  {label}: {value}"));
    }
}

fn push_skill_array_field(lines: &mut Vec<String>, label: &str, skill: &Value, key: &str) {
    let values = skill
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !values.is_empty() {
        lines.push(format!("  {label}: {}", values.join(", ")));
    }
}

fn indent_lines(value: &str, indent: &str) -> String {
    value
        .lines()
        .map(|line| format!("{indent}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn discover_skill_dirs_with_trust(cwd: &str, project_trust: ProjectTrust) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        push_existing_dir(&mut dirs, home.join(".claude").join("skills"));
    }
    if let Ok(kiana_home) = std::env::var("KIANA_HOME") {
        push_existing_dir(&mut dirs, PathBuf::from(kiana_home).join("skills"));
    }

    if project_trust.allows_project_resources() {
        let Ok(cwd) = std::fs::canonicalize(cwd) else {
            return dedupe_dirs(dirs);
        };
        let trust_root = project_trust_root(&cwd);
        let mut ancestors = Vec::new();
        for ancestor in cwd.ancestors() {
            ancestors.push(ancestor.to_path_buf());
            if ancestor == trust_root {
                break;
            }
        }
        if !ancestors
            .last()
            .is_some_and(|ancestor| ancestor == &trust_root)
        {
            return dedupe_dirs(dirs);
        }
        ancestors.reverse();
        for ancestor in ancestors {
            push_existing_dir(&mut dirs, ancestor.join(".claude").join("skills"));
        }
    }

    dedupe_dirs(dirs)
}

fn push_existing_dir(dirs: &mut Vec<PathBuf>, dir: PathBuf) {
    if dir.is_dir() {
        dirs.push(dir);
    }
}

fn dedupe_dirs(dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for dir in dirs {
        let key = dir
            .canonicalize()
            .unwrap_or_else(|_| dir.clone())
            .to_string_lossy()
            .to_string();
        if seen.insert(key) {
            deduped.push(dir);
        }
    }
    deduped
}

fn load_skill_records(dirs: &[PathBuf]) -> ToolResult<Vec<SkillRecord>> {
    let mut records = Vec::new();
    let mut seen_names = HashSet::new();
    for dir in dirs {
        for entry in read_dir_sorted(dir)? {
            if !entry.is_dir() {
                continue;
            }
            let skill_file = entry.join("SKILL.md");
            if !skill_file.is_file() {
                continue;
            }
            let content = std::fs::read_to_string(&skill_file)?;
            let record = parse_skill_record(&entry, &content, source_for_skill_dir(dir));
            if seen_names.insert(record.name.to_ascii_lowercase()) {
                records.push(record);
            }
        }
    }
    Ok(records)
}

fn read_dir_sorted(dir: &Path) -> ToolResult<Vec<PathBuf>> {
    let mut entries = std::fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    Ok(entries)
}

fn parse_skill_record(skill_root: &Path, content: &str, source: String) -> SkillRecord {
    let (frontmatter, body) = split_frontmatter(content);
    let frontmatter = frontmatter
        .and_then(|frontmatter| serde_yaml::from_str::<SkillFrontmatter>(frontmatter).ok())
        .unwrap_or_default();
    let body = body.trim().to_string();
    let name = skill_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("skill")
        .to_string();
    let description = frontmatter
        .description
        .clone()
        .or_else(|| first_non_empty_markdown_line(&body))
        .unwrap_or_else(|| format!("Skill: {name}"));

    SkillRecord {
        name,
        display_name: frontmatter.name,
        description,
        when_to_use: frontmatter.when_to_use,
        argument_hint: frontmatter.argument_hint,
        allowed_tools: frontmatter.allowed_tools.unwrap_or_default(),
        user_invocable: frontmatter.user_invocable.unwrap_or(true),
        model: frontmatter.model,
        context: frontmatter.context,
        paths: frontmatter.paths.map(|paths| {
            paths
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        }),
        source,
        skill_root: skill_root.to_path_buf(),
        content: body,
    }
}

fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    let Some(rest) = content.strip_prefix("---\n") else {
        return (None, content);
    };
    let Some(end) = rest.find("\n---") else {
        return (None, content);
    };
    let frontmatter = &rest[..end];
    let body = rest[end + 4..].trim_start_matches(['\r', '\n']);
    (Some(frontmatter), body)
}

fn first_non_empty_markdown_line(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.trim_start_matches('#').trim().to_string())
        .filter(|line| !line.is_empty())
}

fn source_for_skill_dir(dir: &Path) -> String {
    let dir = dir.to_string_lossy();
    if dir.contains(".claude/skills") || dir.contains(".claude\\skills") {
        "project".to_string()
    } else {
        "user".to_string()
    }
}

fn skill_matches_query(record: &SkillRecord, query: &str) -> bool {
    [
        Some(record.name.as_str()),
        record.display_name.as_deref(),
        Some(record.description.as_str()),
        record.when_to_use.as_deref(),
        record.argument_hint.as_deref(),
        Some(record.content.as_str()),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_ascii_lowercase().contains(query))
}

fn skill_record_to_json(record: SkillRecord, include_content: bool) -> Value {
    let mut value = json!({
        "name": record.name,
        "display_name": record.display_name,
        "description": record.description,
        "when_to_use": record.when_to_use,
        "argument_hint": record.argument_hint,
        "allowed_tools": record.allowed_tools,
        "user_invocable": record.user_invocable,
        "model": record.model,
        "context": record.context,
        "paths": record.paths,
        "source": record.source,
        "skill_root": record.skill_root.to_string_lossy().to_string()
    });
    if include_content {
        value["content"] = Value::String(record.content);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::DiscoverSkillsTool;
    use crate::{Tool, ToolContext};
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use uuid::Uuid;

    fn test_context_with_app_state(
        cwd: String,
        app_state: HashMap<String, serde_json::Value>,
    ) -> ToolContext {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd,
            read_file_state: HashMap::new(),
            app_state,
            abort_signal: abort_rx,
        }
    }

    #[tokio::test]
    async fn discovers_project_skills_from_parent_directories() {
        let _guard = crate::test_support::lock_env();
        let root = std::env::temp_dir().join(format!("kiana-skills-{}", Uuid::new_v4()));
        let nested = root.join("repo").join("src");
        let skill = root
            .join("repo")
            .join(".claude")
            .join("skills")
            .join("refactor");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(root.join("repo").join(".git")).unwrap();
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: Refactor Helper\ndescription: Improve Rust module structure\nwhen_to_use: Use when refactoring Rust code\nallowed-tools:\n  - Read\n  - Edit\n---\n# Refactor helper\nDetailed instructions.\n",
        )
        .unwrap();

        let mut context = test_context_with_app_state(
            nested.to_string_lossy().to_string(),
            HashMap::from([("project_trusted".to_string(), json!(true))]),
        );
        let output = DiscoverSkillsTool::new()
            .call(&json!({"query": "rust"}), &mut context)
            .await
            .unwrap();

        assert_eq!(output.data["count"], 1);
        assert_eq!(output.data["skills"][0]["name"], "refactor");
        assert_eq!(output.data["skills"][0]["display_name"], "Refactor Helper");
        assert_eq!(output.data["skills"][0]["allowed_tools"][1], "Edit");
        assert!(output.data["skills"][0].get("content").is_none());

        let api_result = DiscoverSkillsTool::new().map_to_api_result(&output, "toolu_skills");
        assert_eq!(api_result["type"], "tool_result");
        assert_eq!(api_result["tool_use_id"], "toolu_skills");
        let content = api_result["content"].as_str().unwrap();
        assert!(content.contains("1 skill(s) found."));
        assert!(content.contains("refactor (Refactor Helper): Improve Rust module structure"));
        assert!(content.contains("Allowed tools: Read, Edit"));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn can_include_skill_content() {
        let _guard = crate::test_support::lock_env();
        let unique = Uuid::new_v4().to_string();
        let root = std::env::temp_dir().join(format!("kiana-skills-{}", Uuid::new_v4()));
        let skill_name = format!("audit-{unique}");
        let skill = root.join(".claude").join("skills").join(&skill_name);
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "# Audit\nReview critical paths.\n").unwrap();

        let mut context = test_context_with_app_state(
            root.to_string_lossy().to_string(),
            HashMap::from([("project_trusted".to_string(), json!(true))]),
        );
        let output = DiscoverSkillsTool::new()
            .call(
                &json!({"query": unique, "include_content": true}),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["count"], 1);
        assert_eq!(output.data["skills"][0]["name"], skill_name);
        assert_eq!(output.data["skills"][0]["description"], "Audit");
        assert!(output.data["skills"][0]["content"]
            .as_str()
            .unwrap()
            .contains("Review critical paths"));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn trusted_nested_git_repo_does_not_discover_project_skills_above_trust_root() {
        let _guard = crate::test_support::lock_env();
        let unique = Uuid::new_v4().to_string();
        let root = std::env::temp_dir().join(format!("kiana-skills-trust-root-{unique}"));
        let parent = root.join("parent");
        let child = parent.join("child");
        let work = child.join("work");
        let outside = parent
            .join(".claude")
            .join("skills")
            .join(format!("outside-{unique}"));
        let inside = work
            .join(".claude")
            .join("skills")
            .join(format!("inside-{unique}"));
        fs::create_dir_all(child.join(".git")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(&inside).unwrap();
        fs::write(
            outside.join("SKILL.md"),
            format!("---\ndescription: outside {unique}\n---\nOutside body\n"),
        )
        .unwrap();
        fs::write(
            inside.join("SKILL.md"),
            format!("---\ndescription: inside {unique}\n---\nInside body\n"),
        )
        .unwrap();

        let mut context = test_context_with_app_state(
            work.to_string_lossy().to_string(),
            HashMap::from([("project_trusted".to_string(), json!(true))]),
        );
        let output = DiscoverSkillsTool::new()
            .call(&json!({"query": unique}), &mut context)
            .await
            .unwrap();

        assert_eq!(output.data["count"], 1);
        assert_eq!(output.data["skills"][0]["name"], format!("inside-{unique}"));
        let dirs = output.data["skill_dirs"].as_array().unwrap();
        assert!(!dirs
            .iter()
            .filter_map(serde_json::Value::as_str)
            .any(|dir| dir == outside.parent().unwrap().to_string_lossy()));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn unknown_and_untrusted_projects_hide_project_skills_but_keep_user_skills() {
        let _guard = crate::test_support::lock_env();
        let unique = Uuid::new_v4().to_string();
        let root = std::env::temp_dir().join(format!("kiana-skills-trust-{unique}"));
        let cwd = root.join("repo");
        let kiana_home = root.join("kiana-home");
        let project_skill = cwd
            .join(".claude")
            .join("skills")
            .join(format!("project-{unique}"));
        let user_skill = kiana_home.join("skills").join(format!("home-{unique}"));
        fs::create_dir_all(&project_skill).unwrap();
        fs::create_dir_all(&user_skill).unwrap();
        fs::write(
            project_skill.join("SKILL.md"),
            format!("---\ndescription: project {unique}\n---\nProject body\n"),
        )
        .unwrap();
        fs::write(
            user_skill.join("SKILL.md"),
            format!("---\ndescription: home {unique}\n---\nHome body\n"),
        )
        .unwrap();
        let previous_kiana_home = std::env::var_os("KIANA_HOME");
        std::env::set_var("KIANA_HOME", &kiana_home);
        let project_skills_dir = project_skill.parent().unwrap().to_string_lossy();

        for app_state in [
            HashMap::new(),
            HashMap::from([("project_trusted".to_string(), json!(false))]),
        ] {
            let mut context =
                test_context_with_app_state(cwd.to_string_lossy().to_string(), app_state);
            let output = DiscoverSkillsTool::new()
                .call(&json!({"query": unique}), &mut context)
                .await
                .unwrap();

            assert_eq!(output.data["count"], 1);
            assert_eq!(output.data["skills"][0]["name"], format!("home-{unique}"));
            let dirs = output.data["skill_dirs"].as_array().unwrap();
            assert!(!dirs
                .iter()
                .filter_map(serde_json::Value::as_str)
                .any(|dir| dir == project_skills_dir));
        }

        match previous_kiana_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        let _ = fs::remove_dir_all(root);
    }
}
