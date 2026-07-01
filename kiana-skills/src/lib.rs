pub mod bundled;
pub mod dynamic;
pub mod loader;
pub mod mcp;
pub mod plugins;
pub mod types;

pub use bundled::{get_bundled_skills, register_bundled_skill, BundledSkill};
pub use dynamic::{
    activate_conditional_skills_for_paths, add_dynamic_skill, get_dynamic_skills,
    store_conditional_skill,
};
pub use loader::{get_skill_dirs, get_skill_dirs_with_trust, load_skills_from_dir, SkillLoadError};
pub use mcp::fetch_mcp_skills_for_client;
pub use plugins::{get_plugin_skill_dirs, load_plugin_skills};
pub use types::{Command, ExecutionContext, Frontmatter, LoadedFrom, SettingSource};

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use tokio::sync::Mutex;
use tracing::{debug, warn};

static SKILL_REGISTRY: OnceLock<Mutex<SkillRegistry>> = OnceLock::new();

fn skill_registry() -> &'static Mutex<SkillRegistry> {
    SKILL_REGISTRY.get_or_init(|| Mutex::new(SkillRegistry::default()))
}

#[derive(Default)]
struct SkillRegistry {
    skills_by_cwd: HashMap<String, Vec<Command>>,
}

/// Load all skills from the standard directories (user + project).
/// Results are cached per working directory for the process lifetime.
pub async fn load_all_skills(cwd: impl AsRef<Path>) -> Vec<Command> {
    load_all_skills_with_trust(cwd, kiana_types::ProjectTrust::Trusted).await
}

/// Load all skills while honoring the caller's project trust decision.
///
/// User-scoped and bundled skills remain available when a project is untrusted;
/// project-scoped `.claude/skills` are withheld until the session marks the
/// project trusted.
pub async fn load_all_skills_with_trust(
    cwd: impl AsRef<Path>,
    project_trust: kiana_types::ProjectTrust,
) -> Vec<Command> {
    let cwd = cwd.as_ref();
    let cache_key = format!(
        "{}::{project_trust:?}::plugins={}",
        cwd_cache_key(cwd),
        plugin_skill_cache_key()
    );
    let mut registry = skill_registry().lock().await;

    if let Some(skills) = registry.skills_by_cwd.get(&cache_key) {
        return skills.clone();
    }

    let dirs = get_skill_dirs_with_trust(cwd, project_trust).await;
    let mut all = Vec::new();

    for dir in dirs {
        let source = loader::setting_source_for_skill_dir(&dir);
        match load_skills_from_dir(&dir, source).await {
            Ok(skills) => all.extend(skills),
            Err(e) => warn!("Failed to load skills from {:?}: {}", dir, e),
        }
    }
    all.extend(load_plugin_skills().await);

    // Deduplicate by name (first wins, which is shallowest dir / highest priority)
    let mut seen = std::collections::HashSet::new();
    all.retain(|s| seen.insert(s.name.clone()));

    // Separate conditional skills from unconditional ones
    let (unconditional, conditional): (Vec<_>, Vec<_>) = all
        .into_iter()
        .partition(|s| s.paths.as_ref().map_or(true, |p| p.is_empty()));

    for skill in conditional {
        store_conditional_skill(skill);
    }

    let bundled = get_bundled_skills();
    let dynamic = get_dynamic_skills();

    let mut combined = unconditional;
    combined.extend(bundled);
    combined.extend(dynamic);

    debug!("Loaded {} skills total", combined.len());

    registry.skills_by_cwd.insert(cache_key, combined.clone());
    combined
}

fn cwd_cache_key(cwd: &Path) -> String {
    cwd.canonicalize()
        .unwrap_or_else(|_| cwd.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn plugin_skill_cache_key() -> String {
    kiana_types::plugin::installed_plugin_roots()
        .into_iter()
        .map(|path| {
            path.canonicalize()
                .unwrap_or(path)
                .to_string_lossy()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("|")
}

pub fn find_command<'a>(name: &str, commands: &'a [Command]) -> Option<&'a Command> {
    let name = name.trim_start_matches('/');
    commands.iter().find(|c| c.name.eq_ignore_ascii_case(name))
}

pub fn clear_caches() {
    dynamic::clear_dynamic_skills();
    bundled::clear_bundled_skills();
    if let Some(registry) = SKILL_REGISTRY.get() {
        if let Ok(mut reg) = registry.try_lock() {
            reg.skills_by_cwd.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clear_caches, get_skill_dirs_with_trust, load_all_skills, load_all_skills_with_trust,
    };
    use kiana_types::trust::ProjectTrust;
    use std::fs;
    use std::sync::OnceLock;
    use uuid::Uuid;

    static ENV_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

    async fn env_guard() -> tokio::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await
    }

    fn write_plugin_skill(root: &std::path::Path, plugin_name: &str, skill_name: &str) {
        let plugin_root = root.join(plugin_name);
        fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
        fs::write(
            plugin_root.join(".codex-plugin").join("plugin.json"),
            format!(r#"{{"name":"{plugin_name}","version":"1.0.0"}}"#),
        )
        .unwrap();
        fs::create_dir_all(plugin_root.join("skills").join(skill_name)).unwrap();
        fs::write(
            plugin_root.join("skills").join(skill_name).join("SKILL.md"),
            "---\ndescription: Plugin skill\n---\nPlugin skill body\n",
        )
        .unwrap();
    }

    #[tokio::test]
    async fn load_all_skills_cache_is_scoped_by_cwd() {
        let _guard = env_guard().await;
        clear_caches();
        let root = std::env::temp_dir().join(format!("kiana-skills-cache-{}", Uuid::new_v4()));
        let first = root.join("first");
        let second = root.join("second");
        let first_name = format!("first-{}", Uuid::new_v4());
        let second_name = format!("second-{}", Uuid::new_v4());

        fs::create_dir_all(first.join(".claude").join("skills").join(&first_name)).unwrap();
        fs::create_dir_all(second.join(".claude").join("skills").join(&second_name)).unwrap();
        fs::write(
            first
                .join(".claude")
                .join("skills")
                .join(&first_name)
                .join("SKILL.md"),
            "# First\n",
        )
        .unwrap();
        fs::write(
            second
                .join(".claude")
                .join("skills")
                .join(&second_name)
                .join("SKILL.md"),
            "# Second\n",
        )
        .unwrap();

        let first_skills = load_all_skills(&first).await;
        let second_skills = load_all_skills(&second).await;

        assert!(first_skills.iter().any(|skill| skill.name == first_name));
        assert!(!first_skills.iter().any(|skill| skill.name == second_name));
        assert!(second_skills.iter().any(|skill| skill.name == second_name));
        assert!(!second_skills.iter().any(|skill| skill.name == first_name));

        clear_caches();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn load_all_skills_includes_manifest_backed_plugin_skills() {
        let _guard = env_guard().await;
        clear_caches();
        let root = std::env::temp_dir().join(format!("kiana-plugin-skills-{}", Uuid::new_v4()));
        let cwd = root.join("project");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("review-tools");
        fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
        fs::write(
            plugin_root.join(".codex-plugin").join("plugin.json"),
            r#"{"name":"review-tools","version":"1.0.0"}"#,
        )
        .unwrap();
        fs::create_dir_all(plugin_root.join("skills").join("code-audit")).unwrap();
        fs::write(
            plugin_root
                .join("skills")
                .join("code-audit")
                .join("SKILL.md"),
            "---\ndescription: Audit code from a plugin\n---\nPlugin skill body\n",
        )
        .unwrap();

        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);
        let skills = load_all_skills(&cwd).await;
        let plugin_skill = skills
            .iter()
            .find(|skill| skill.name == "review-tools:code-audit")
            .expect("plugin skill loaded");

        assert_eq!(plugin_skill.loaded_from, super::LoadedFrom::Plugin);
        assert_eq!(plugin_skill.description, "Audit code from a plugin");
        assert!(plugin_skill
            .skill_root
            .as_ref()
            .unwrap()
            .ends_with("code-audit"));

        std::env::remove_var("KIANA_PLUGINS_DIR");
        clear_caches();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_skill_cache_refreshes_after_plugin_install() {
        let _guard = env_guard().await;
        clear_caches();
        let root = std::env::temp_dir().join(format!(
            "kiana-plugin-skills-install-cache-{}",
            Uuid::new_v4()
        ));
        let cwd = root.join("project");
        let plugins_dir = root.join("plugins");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&plugins_dir).unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let before_install = load_all_skills(&cwd).await;
        assert!(!before_install
            .iter()
            .any(|skill| skill.name == "review-tools:code-audit"));

        write_plugin_skill(&plugins_dir, "review-tools", "code-audit");

        let after_install = load_all_skills(&cwd).await;
        assert!(after_install
            .iter()
            .any(|skill| skill.name == "review-tools:code-audit"));

        std::env::remove_var("KIANA_PLUGINS_DIR");
        clear_caches();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_skill_cache_refreshes_after_plugin_disable() {
        let _guard = env_guard().await;
        clear_caches();
        let root = std::env::temp_dir().join(format!(
            "kiana-plugin-skills-disable-cache-{}",
            Uuid::new_v4()
        ));
        let cwd = root.join("project");
        let plugins_dir = root.join("plugins");
        fs::create_dir_all(&cwd).unwrap();
        write_plugin_skill(&plugins_dir, "review-tools", "code-audit");
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let before_disable = load_all_skills(&cwd).await;
        assert!(before_disable
            .iter()
            .any(|skill| skill.name == "review-tools:code-audit"));

        kiana_types::plugin::set_plugin_enabled(&plugins_dir, "review-tools", false).unwrap();

        let after_disable = load_all_skills(&cwd).await;
        assert!(!after_disable
            .iter()
            .any(|skill| skill.name == "review-tools:code-audit"));

        std::env::remove_var("KIANA_PLUGINS_DIR");
        clear_caches();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn untrusted_project_filters_project_skills_but_keeps_user_skills() {
        let _guard = env_guard().await;
        clear_caches();
        let root = std::env::temp_dir().join(format!("kiana-skills-trust-{}", Uuid::new_v4()));
        let cwd = root.join("project");
        let kiana_home = root.join("kiana-home");
        let project_skill = cwd.join(".claude").join("skills").join("project-audit");
        let user_skill = kiana_home.join("skills").join("home-review");
        fs::create_dir_all(&project_skill).unwrap();
        fs::create_dir_all(&user_skill).unwrap();
        fs::write(project_skill.join("SKILL.md"), "# Project audit\n").unwrap();
        fs::write(user_skill.join("SKILL.md"), "# Home review\n").unwrap();

        std::env::set_var("KIANA_HOME", &kiana_home);

        let trusted_dirs = get_skill_dirs_with_trust(&cwd, ProjectTrust::Trusted).await;
        let untrusted_dirs = get_skill_dirs_with_trust(&cwd, ProjectTrust::Untrusted).await;
        assert!(trusted_dirs
            .iter()
            .any(|dir| dir.ends_with(".claude/skills")));
        assert!(!untrusted_dirs
            .iter()
            .any(|dir| dir.ends_with(".claude/skills")));

        let skills = load_all_skills_with_trust(&cwd, ProjectTrust::Untrusted).await;
        assert!(skills.iter().any(|skill| skill.name == "home-review"));
        assert!(!skills.iter().any(|skill| skill.name == "project-audit"));

        std::env::remove_var("KIANA_HOME");
        clear_caches();
        let _ = fs::remove_dir_all(root);
    }
}
