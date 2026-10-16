//! Skill 扩展 crate 的公开门面，重导出来源解析、加载、catalog、渐进披露、plugin、hook 与缓存 API。
//!
//! `load_all_skills*` 会解析 `SKILL.md` 并把正文存入内存中的 `Command.content`；它不执行正文。
//! `list_skill_catalog` / `search_skill_catalog` 返回的 `SkillCatalogEntry` 只含元数据；
//! `load_skill_body` 对已加载的 content 做预算校验后复制为披露结果，`activate_skill` 只创建
//! activation 记录，`read_skill_resource` 才会按需读取 skill root 下的资源文件。
//!
//! 来源信任由调用方边界传入：默认入口读取外部 ProjectTrust，缺失或读取失败按 Unknown
//! 处理；显式入口使用调用方传来的 trust。未知/不可信项目不会因此失去 user/bundled 来源，
//! 但项目级 skill 与受项目 trust 约束的 plugin 来源会被过滤。
//!
//! 加载结果来自解析器接受的目录 skill、plugin skill、bundled skill 与活动的 legacy
//! dynamic skill，再由 domain catalog 选择和报告 shadowed 项。只有 unconditional 项会进入
//! 返回并缓存的 Vec；带 path 条件的项会尝试写入 legacy dynamic store，但写入错误被忽略。
//!
//! catalog 构建失败时内部以空 catalog 兜底；调用方实际收到并缓存的是空命令 Vec。
//! 这与单个目录/skill 解析错误不同，后者由 loader 告警并跳过。缓存是进程内状态，不是持久
//! source of truth；全局 registry mutex 在 miss 加载过程中跨 await 持有，不同 cwd 的并发 miss 会串行等待。

pub mod bundled;
pub mod catalog;
pub mod disclosure;
pub mod dynamic;
pub mod hook_discovery;
pub mod loader;
pub mod manifest;
pub mod mcp;
pub mod plugins;
pub mod snapshot;
pub mod source_resolver;
pub mod types;

pub use bundled::{get_bundled_skills, register_bundled_skill, BundledSkill};
pub use catalog::{build_skill_catalog, SkillCatalog};
pub use disclosure::{
    activate_skill, list_skill_catalog, load_skill_body, read_skill_resource, search_skill_catalog,
    DisclosureBudget, DisclosureError, DisclosureQuota, SkillActivation, SkillActivationRequest,
    SkillActivationStatus, SkillBody, SkillCatalogEntry, SkillCatalogResponse,
    SkillDisclosureStatus, SkillResource, SkillSourceSummary, DEFAULT_BODY_MAX_BYTES,
    DEFAULT_BODY_MAX_TOKENS, DEFAULT_RESOURCE_MAX_BYTES, DEFAULT_RESOURCE_MAX_TOKENS,
    SKILL_ACTIVATION_SCHEMA, SKILL_DISCLOSURE_SCHEMA,
};
pub use dynamic::{
    activate_conditional_skills_for_paths, add_dynamic_skill, get_dynamic_skills,
    store_conditional_skill, DynamicActivationReceipt, DynamicActivationStatus, DynamicSkillError,
    DynamicSkillScope, DynamicSkillStore, PathGlobAst, SkillInvocation, SkillInvocationRequest,
    DYNAMIC_ACTIVATION_RECEIPT_SCHEMA, DYNAMIC_SKILL_SCHEMA, PATH_GLOB_AST_SCHEMA,
    SKILL_INVOCATION_SCHEMA,
};
pub use hook_discovery::{
    discover_hooks, HookDiscoveryCandidate, HookDiscoveryRequest, HookDiscoverySnapshot,
    HookMatchPhase, HookMatchRecord, HOOK_DISCOVERY_SCHEMA,
};
pub use loader::{
    get_skill_dirs, get_skill_dirs_with_trust, load_skills_from_dir, normalize_skill_name,
    parse_skill_document, SkillLoadError,
};
pub use manifest::{
    component_index, parse_hook_manifest, parse_plugin_manifest, parse_plugin_manifest_v2,
    HookEntry, NormalizedHookDescriptor, NormalizedHookEvent, NormalizedHookManifest,
    NormalizedPluginManifest, NormalizedPluginManifestV2, PluginComponent, PluginComponentKind,
    PluginManifestV2Component, MAX_MANIFEST_BYTES, NORMALIZED_HOOK_MANIFEST_SCHEMA,
    NORMALIZED_PLUGIN_MANIFEST_SCHEMA, NORMALIZED_PLUGIN_MANIFEST_V2_SCHEMA,
};
pub use mcp::fetch_mcp_skills_for_client;
pub use plugins::{
    get_plugin_skill_dirs, get_plugin_skill_dirs_for_cwd, load_plugin_skills,
    load_plugin_skills_for_cwd, plugin_skill_load_audit, plugin_skill_load_audit_for_cwd,
};
pub use snapshot::{
    invalidate_extension_snapshots, snapshot_generation, ExtensionSnapshotCache,
    SnapshotCacheEntry, SnapshotCacheKey, SnapshotCacheState, SNAPSHOT_CACHE_ENTRY_SCHEMA,
    SNAPSHOT_CACHE_KEY_SCHEMA, SNAPSHOT_SCHEMA_VERSION,
};
pub use source_resolver::{
    validate_root, ResolvedSourceRoot, SourceResolution, SourceResolutionSummary,
    SourceResolveError, SourceResolver, SourceRootKind, SourceRootSummary, SourceTrust,
    SOURCE_RESOLUTION_SCHEMA,
};
pub use types::{Command, ExecutionContext, Frontmatter, LoadedFrom, SettingSource};

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use tokio::sync::Mutex;
use tracing::{debug, warn};

// 一个进程内共享的技能注册表。Mutex 覆盖所有 cwd；cache miss 时下面的加载路径会在持锁
// 状态下 await resolver、目录/plugin loader 和 catalog，因此不同 cwd 的并发 miss 也会串行。
static SKILL_REGISTRY: OnceLock<Mutex<SkillRegistry>> = OnceLock::new();

fn skill_registry() -> &'static Mutex<SkillRegistry> {
    SKILL_REGISTRY.get_or_init(|| Mutex::new(SkillRegistry::default()))
}

#[derive(Default)]
struct SkillRegistry {
    skills_by_cwd: HashMap<String, Vec<Command>>,
}

/// 便捷入口：根据 `cwd` 的外部 ProjectTrust 选择技能来源。
///
/// trust 记录缺失或读取失败时都降为 `ProjectTrust::Unknown`，然后委托给显式 trust 入口；
/// 这会过滤项目级来源，但不会把整个技能集合清空。缓存也不只按 cwd 区分，还包含 trust、
/// source/plugin/config 摘要及 snapshot generation。返回值只含 unconditional 项；条件项会尝试
/// 写入 legacy dynamic store，但存储失败被忽略，不会作为本方法的错误返回。
pub async fn load_all_skills(cwd: impl AsRef<Path>) -> Vec<Command> {
    let cwd = cwd.as_ref();
    let project_trust = kiana_types::read_project_trust(cwd)
        .ok()
        .flatten()
        .unwrap_or(kiana_types::ProjectTrust::Unknown);
    load_all_skills_with_trust(cwd, project_trust).await
}

/// Load skills using the trust decision supplied by the caller.
///
/// Unknown/Untrusted 保留用户级与 bundled 来源，并过滤项目级 skill 目录以及受项目 trust
/// 控制的 plugin 资源；传入 Trusted 也不绕过 loader、catalog 和后续披露校验。
///
/// 缓存键绑定规范化 cwd、trust、resolved source-root 摘要、plugin root 路径集摘要、
/// `KIANA_SYSTEM_PROMPT`/`KIANA_APPEND_SYSTEM_PROMPT` 摘要和 snapshot generation。
/// 结构化 key 构造失败时，fallback key 只剩 cwd 与 generation，不应描述为保留了上述全部隔离维度。
///
/// 返回 catalog 选择的 unconditional 命令；path 条件项会尝试写入 legacy dynamic store，
/// 但 store facade 丢弃写入错误，因而不保证条件项一定已入库。
/// 单个目录或 skill 解析失败会告警并跳过；catalog 整体构建失败则本轮返回空命令 Vec。
/// registry 的单个全局 mutex 在缓存 miss 的异步加载期间保持锁定。
pub async fn load_all_skills_with_trust(
    cwd: impl AsRef<Path>,
    project_trust: kiana_types::ProjectTrust,
) -> Vec<Command> {
    let cwd = cwd.as_ref();
    // 根目录解析摘要只是复合 cache key 的一部分；resolve 失败时以固定 sentinel 参与 key，
    // 不是在此处直接返回错误。实际目录与 plugin 加载仍走下方 trust-aware loader。
    let source_roots_digest = SourceResolver::new(cwd, project_trust)
        .resolve()
        .map(|resolution| resolution.summary.root_set_digest)
        .unwrap_or_else(|_| {
            kiana_domain::json_digest(&serde_json::json!({"source_roots": "unavailable"}))
        });
    let package_registry_generation = kiana_domain::json_digest(&serde_json::json!({
        "plugins": plugins::plugin_skill_cache_key_for_cwd(cwd, project_trust),
    }));
    let config_digest = kiana_domain::json_digest(&serde_json::json!({
        "system": std::env::var("KIANA_SYSTEM_PROMPT").ok(),
        "append": std::env::var("KIANA_APPEND_SYSTEM_PROMPT").ok(),
    }));
    // SnapshotCacheKey 汇总 trust、source roots、plugin registry 和 prompt 配置；生成失败时
    // fallback 只保留 cwd + generation。该失败路径会缩减 key 维度，不能称为完整复合键。
    let cache_key = SnapshotCacheKey::new(
        &cwd_cache_key(cwd),
        match project_trust {
            kiana_types::ProjectTrust::Trusted => "trusted",
            kiana_types::ProjectTrust::Untrusted => "untrusted",
            kiana_types::ProjectTrust::Unknown => "unknown",
        },
        &source_roots_digest,
        &package_registry_generation,
        &config_digest,
    )
    .map(|key| format!("{}:generation={}", key.key_digest, snapshot_generation()))
    .unwrap_or_else(|_| {
        format!(
            "{}:generation={}",
            cwd_cache_key(cwd),
            snapshot_generation()
        )
    });
    // Mutex guard 故意覆盖从目录扫描到 catalog 完成的整个 miss 路径；这保证同一 registry
    // 不会并发重复填充，但也意味着锁范围内的 await 会阻塞其他 cwd 的注册表查询。
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
    all.extend(load_plugin_skills_for_cwd(cwd, project_trust).await);
    all.extend(get_bundled_skills());
    all.extend(get_dynamic_skills());

    // 目录/单个 skill 的解析问题已由 loader 告警并跳过；这里处理的是聚合 catalog 构建失败。
    // 构建失败时清空本轮候选，避免返回未经过统一 identity/precedence 选择的部分结果。
    // 正常重复项由 catalog 以 shadowed 诊断处理，不等同于这个 Err 分支。
    let catalog = match build_skill_catalog(&all, 1) {
        Ok(catalog) => catalog,
        Err(error) => {
            warn!("Failed to build extension catalog: {error}");
            all.clear();
            SkillCatalog {
                catalog: kiana_domain::ExtensionCatalog::new(1, Vec::new())
                    .expect("empty catalog is valid"),
                selected: Vec::new(),
                shadowed: Vec::new(),
            }
        }
    };
    let all = catalog.selected;

    // `paths` 非空的项不会出现在下面缓存和返回的 unconditional Vec 中；store facade 会尝试
    // 写入 legacy DynamicSkillStore，但内部丢弃 Result，此处无法报告实际存储成功或失败。
    let (unconditional, conditional): (Vec<_>, Vec<_>) = all
        .into_iter()
        .partition(|s| s.paths.as_ref().map_or(true, |p| p.is_empty()));

    for skill in conditional {
        store_conditional_skill(skill);
    }

    let combined = unconditional;

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

/// 返回首个完整命令名匹配项：去掉查询名开头的所有 `/` 后做 ASCII 大小写不敏感比较。
/// 不裁剪空白、不做 namespace 后缀匹配；若短名匹配由调用方另行实现。
pub fn find_command<'a>(name: &str, commands: &'a [Command]) -> Option<&'a Command> {
    let name = name.trim_start_matches('/');
    commands.iter().find(|c| c.name.eq_ignore_ascii_case(name))
}

/// 清理本进程的 legacy dynamic 与 bundled 注册表，并使 extension snapshot generation 前进。
///
/// skills 命令缓存通过 `try_lock` 尝试清除：若加载路径正持有全局 mutex，本调用不等待，
/// 此时该 map 本身不会立即清空；generation 变化会使后续构造的复合 key 与旧 key 分离。
/// 这不是用户自建 `DynamicSkillStore` 或每个 `ExtensionSnapshotCache` 实例的通用清空器。
pub fn clear_caches() {
    dynamic::clear_dynamic_skills();
    bundled::clear_bundled_skills();
    if let Some(registry) = SKILL_REGISTRY.get() {
        if let Ok(mut reg) = registry.try_lock() {
            reg.skills_by_cwd.clear();
        }
    }
    invalidate_extension_snapshots("explicit_clear");
}

#[cfg(test)]
mod tests {
    use super::{
        clear_caches, get_skill_dirs, get_skill_dirs_with_trust, load_all_skills,
        load_all_skills_with_trust, register_bundled_skill, BundledSkill,
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

        let first_skills = load_all_skills_with_trust(&first, ProjectTrust::Trusted).await;
        let second_skills = load_all_skills_with_trust(&second, ProjectTrust::Trusted).await;

        assert!(first_skills.iter().any(|skill| skill.name == first_name));
        assert!(!first_skills.iter().any(|skill| skill.name == second_name));
        assert!(second_skills.iter().any(|skill| skill.name == second_name));
        assert!(!second_skills.iter().any(|skill| skill.name == first_name));

        clear_caches();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn load_all_skills_default_uses_external_project_trust() {
        let _guard = env_guard().await;
        clear_caches();
        let root =
            std::env::temp_dir().join(format!("kiana-skills-default-trust-{}", Uuid::new_v4()));
        let cwd = root.join("project");
        let kiana_home = root.join("kiana-home");
        let project_skill = cwd.join(".claude").join("skills").join("project-audit");
        let user_skill = kiana_home.join("skills").join("home-review");
        fs::create_dir_all(cwd.join(".git")).unwrap();
        fs::create_dir_all(&project_skill).unwrap();
        fs::create_dir_all(&user_skill).unwrap();
        fs::write(project_skill.join("SKILL.md"), "# Project audit\n").unwrap();
        fs::write(user_skill.join("SKILL.md"), "# Home review\n").unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);

        let unknown_dirs = get_skill_dirs(&cwd).await;
        assert!(!unknown_dirs
            .iter()
            .any(|dir| dir.starts_with(cwd.join(".claude"))));
        let unknown_skills = load_all_skills(&cwd).await;
        assert!(unknown_skills
            .iter()
            .any(|skill| skill.name == "home-review"));
        assert!(!unknown_skills
            .iter()
            .any(|skill| skill.name == "project-audit"));

        kiana_types::write_project_trust(&cwd, ProjectTrust::Trusted).unwrap();
        clear_caches();
        let trusted_dirs = get_skill_dirs(&cwd).await;
        assert!(trusted_dirs
            .iter()
            .any(|dir| dir.starts_with(cwd.join(".claude"))));
        let trusted_skills = load_all_skills(&cwd).await;
        assert!(trusted_skills
            .iter()
            .any(|skill| skill.name == "project-audit"));

        std::env::remove_var("KIANA_HOME");
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
    async fn unknown_and_untrusted_projects_filter_project_skills_but_keep_safe_skills() {
        let _guard = env_guard().await;
        clear_caches();
        let root = std::env::temp_dir().join(format!("kiana-skills-trust-{}", Uuid::new_v4()));
        let cwd = root.join("project");
        let kiana_home = root.join("kiana-home");
        let project_skill = cwd.join(".claude").join("skills").join("project-audit");
        let user_skill = kiana_home.join("skills").join("home-review");
        let bundled_name = format!("bundled-review-{}", Uuid::new_v4());
        fs::create_dir_all(&project_skill).unwrap();
        fs::create_dir_all(&user_skill).unwrap();
        fs::write(project_skill.join("SKILL.md"), "# Project audit\n").unwrap();
        fs::write(user_skill.join("SKILL.md"), "# Home review\n").unwrap();
        register_bundled_skill(BundledSkill {
            name: bundled_name.clone(),
            description: "Bundled review".to_string(),
            when_to_use: None,
            argument_hint: None,
            allowed_tools: Vec::new(),
            model: None,
            disable_model_invocation: false,
            user_invocable: true,
            context: None,
            content: "Bundled review body".to_string(),
        });

        std::env::set_var("KIANA_HOME", &kiana_home);

        let project_skills_dir = cwd.join(".claude").join("skills");
        let trusted_dirs = get_skill_dirs_with_trust(&cwd, ProjectTrust::Trusted).await;
        assert!(trusted_dirs.contains(&project_skills_dir));
        for project_trust in [ProjectTrust::Unknown, ProjectTrust::Untrusted] {
            let dirs = get_skill_dirs_with_trust(&cwd, project_trust).await;
            assert!(!dirs.contains(&project_skills_dir));

            let skills = load_all_skills_with_trust(&cwd, project_trust).await;
            assert!(skills.iter().any(|skill| skill.name == "home-review"));
            assert!(skills.iter().any(|skill| skill.name == bundled_name));
            assert!(!skills.iter().any(|skill| skill.name == "project-audit"));
        }

        std::env::remove_var("KIANA_HOME");
        clear_caches();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn trusted_nested_git_project_skills_stop_at_project_trust_root() {
        let _guard = env_guard().await;
        clear_caches();
        let root = std::env::temp_dir().join(format!("kiana-skills-trust-root-{}", Uuid::new_v4()));
        let parent = root.join("parent");
        let child = parent.join("child");
        let work = child.join("work");
        let parent_skills = parent.join(".claude").join("skills");
        let child_root_skills = child.join(".claude").join("skills");
        let child_cwd_skills = work.join(".claude").join("skills");

        fs::create_dir_all(parent_skills.join("parent-skill")).unwrap();
        fs::create_dir_all(child.join(".git")).unwrap();
        fs::create_dir_all(child_root_skills.join("child-root-skill")).unwrap();
        fs::create_dir_all(child_cwd_skills.join("child-cwd-skill")).unwrap();
        fs::write(
            parent_skills.join("parent-skill").join("SKILL.md"),
            "# Parent skill\n",
        )
        .unwrap();
        fs::write(
            child_root_skills.join("child-root-skill").join("SKILL.md"),
            "# Child root skill\n",
        )
        .unwrap();
        fs::write(
            child_cwd_skills.join("child-cwd-skill").join("SKILL.md"),
            "# Child cwd skill\n",
        )
        .unwrap();

        let dirs = get_skill_dirs_with_trust(&work, ProjectTrust::Trusted).await;
        assert!(!dirs.contains(&parent_skills));
        assert!(dirs.contains(&child_root_skills));
        assert!(dirs.contains(&child_cwd_skills));

        let skills = load_all_skills_with_trust(&work, ProjectTrust::Trusted).await;
        assert!(!skills.iter().any(|skill| skill.name == "parent-skill"));
        assert!(skills.iter().any(|skill| skill.name == "child-root-skill"));
        assert!(skills.iter().any(|skill| skill.name == "child-cwd-skill"));

        clear_caches();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn kiana_project_skills_follow_the_same_trust_gate_as_claude_skills() {
        let _guard = env_guard().await;
        clear_caches();
        let root = std::env::temp_dir().join(format!("kiana-skills-kiana-dir-{}", Uuid::new_v4()));
        let cwd = root.join("project");
        let project_skill = cwd.join(".kiana").join("skills").join("code03-kiana-dir");
        fs::create_dir_all(&project_skill).unwrap();
        fs::write(project_skill.join("SKILL.md"), "# Kiana project skill\n").unwrap();

        let project_skills_dir = cwd.join(".kiana").join("skills");
        let trusted_dirs = get_skill_dirs_with_trust(&cwd, ProjectTrust::Trusted).await;
        assert!(trusted_dirs.contains(&project_skills_dir));
        let trusted = load_all_skills_with_trust(&cwd, ProjectTrust::Trusted).await;
        assert!(trusted.iter().any(|skill| skill.name == "code03-kiana-dir"));

        for project_trust in [ProjectTrust::Unknown, ProjectTrust::Untrusted] {
            let dirs = get_skill_dirs_with_trust(&cwd, project_trust).await;
            assert!(!dirs.contains(&project_skills_dir));
            let skills = load_all_skills_with_trust(&cwd, project_trust).await;
            assert!(!skills.iter().any(|skill| skill.name == "code03-kiana-dir"));
        }

        clear_caches();
        let _ = fs::remove_dir_all(root);
    }
}
