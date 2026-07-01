use crate::local_state::kiana_home_dir;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use kiana_types::plugin::{
    all_plugin_roots_in, disabled_plugin_names, find_manifest_path, plugin_is_disabled,
    set_plugin_enabled,
};
use kiana_types::project_trust_from_app_state;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct PluginCommand;

#[async_trait]
impl Command for PluginCommand {
    fn name(&self) -> &str {
        "plugin"
    }

    fn description(&self) -> &str {
        "Manage plugins"
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
            "" | "list" | "status" => list_plugins(&context, rest),
            "json" => plugins_json(&context, rest),
            "marketplace" => marketplace_command(&context, rest).await,
            "install" => install_plugin(&context, rest).await,
            "uninstall" | "remove" | "rm" => uninstall_plugin(&context, rest).await,
            "show" | "get" => show_plugin(&context, rest),
            "path" | "paths" => plugin_path(&context, rest),
            "enable" => set_plugin_state(&context, rest, true).await,
            "disable" => set_plugin_state(&context, rest, false).await,
            "validate" => validate_plugins(&context, rest),
            "help" | "--help" | "-h" => Ok(CommandResult::text(usage())),
            other => Err(anyhow!("unknown plugin command '{}'\n\n{}", other, usage())),
        }
    }
}

fn list_plugins(context: &CommandContext, query: &str) -> Result<CommandResult> {
    let root = plugin_root_dir(context);
    let plugins = filtered_plugins(load_installed_plugins(&root)?, query);

    if plugins.is_empty() {
        let suffix = if query.trim().is_empty() {
            "Install or place plugin directories under this path."
        } else {
            "No plugins matched the query."
        };
        return Ok(CommandResult::text(format!(
            "No plugins.\npath: {}\n{}",
            root.display(),
            suffix
        )));
    }

    let mut lines = vec![
        format!("{} plugin(s)", plugins.len()),
        format!("path: {}", root.display()),
    ];
    if !query.trim().is_empty() {
        lines.push(format!("query: {}", query.trim()));
    }
    for plugin in plugins {
        lines.push(format!(
            "- {}{} [{} {}] {} commands={} agents={} skills={} hooks={} output_styles={} lsp_servers={}{}",
            plugin.display_name(),
            plugin
                .version
                .as_ref()
                .map(|version| format!("@{}", version))
                .unwrap_or_default(),
            if plugin.valid { "valid" } else { "invalid" },
            if plugin.enabled {
                "enabled"
            } else {
                "disabled"
            },
            plugin.description.as_deref().unwrap_or(""),
            plugin.components.commands,
            plugin.components.agents,
            plugin.components.skills,
            plugin.components.hooks,
            plugin.components.output_styles,
            plugin.components.lsp_servers,
            if plugin.errors.is_empty() {
                String::new()
            } else {
                format!(" errors={}", plugin.errors.join("; "))
            }
        ));
    }
    lines.push(
        "usage: kiana plugin install <plugin> | uninstall <plugin> | show <name> | enable <name> | disable <name> | json [query] | path [name] | validate [name|path]"
            .into(),
    );
    Ok(CommandResult::text(lines.join("\n")))
}

fn plugins_json(context: &CommandContext, query: &str) -> Result<CommandResult> {
    let root = plugin_root_dir(context);
    let plugins = filtered_plugins(load_installed_plugins(&root)?, query);
    Ok(CommandResult::text(serde_json::to_string_pretty(&plugins)?))
}

pub fn installed_plugin_summaries(context: &CommandContext) -> Result<Value> {
    let root = plugin_root_dir(context);
    let plugins = load_installed_plugins(&root)?;
    Ok(Value::Array(
        plugins.iter().map(plugin_init_summary).collect::<Vec<_>>(),
    ))
}

fn show_plugin(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let target = rest.trim();
    if target.is_empty() {
        return Err(anyhow!("usage: kiana plugin show <name>"));
    }
    let plugin = resolve_plugin(context, target)?;
    Ok(CommandResult::text(serde_json::to_string_pretty(&plugin)?))
}

fn plugin_path(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let target = rest.trim();
    if target.is_empty() {
        return Ok(CommandResult::text(
            plugin_root_dir(context).display().to_string(),
        ));
    }
    let plugin = resolve_plugin(context, target)?;
    Ok(CommandResult::text(plugin.root.display().to_string()))
}

async fn install_plugin(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    if rest.trim().is_empty() || is_help_arg(rest.trim()) {
        return Ok(CommandResult::text(plugin_install_usage()));
    }
    let args = parse_plugin_action_args(rest, "install")?;
    ensure_runtime_plugin_scope(args.scope, "install")?;
    let source = resolve_install_source(context, &args.target).await?;
    let source_info = read_plugin(source.root.clone())?;
    if !source_info.valid {
        return Err(anyhow!(
            "plugin '{}' is invalid and cannot be installed: {}",
            source_info.display_name(),
            source_info.errors.join("; ")
        ));
    }
    if source.policy.is_not_available() {
        return Err(anyhow!(
            "plugin '{}' is marked not available by marketplace policy and cannot be installed",
            source_info.display_name()
        ));
    }

    let install_name = safe_plugin_dir_name(source_info.display_name())?;
    let install_root = plugin_root_dir(context);
    let destination = install_root.join(&install_name);
    if same_existing_path(&source_info.root, &destination) {
        return Ok(CommandResult::text(format!(
            "Plugin already installed: {}\npath: {}",
            source_info.display_name(),
            destination.display()
        )));
    }
    if destination.exists() {
        return Err(anyhow!(
            "plugin '{}' is already installed at {}; uninstall it first",
            install_name,
            destination.display()
        ));
    }

    copy_plugin_dir(&source_info.root, &destination)?;
    let state_path = set_plugin_enabled(&install_root, source_info.display_name(), true)
        .map_err(anyhow::Error::msg)?;
    kiana_tools::lsp_tool::shutdown_lsp_clients().await;

    let mut lines = vec![
        format!("Installed plugin: {}", source_info.display_name()),
        format!("path: {}", destination.display()),
    ];
    if let Some(marketplace) = source.marketplace {
        lines.push(format!("marketplace: {marketplace}"));
    } else {
        lines.push(format!("source: {}", source_info.root.display()));
    }
    if !source.policy.is_empty() {
        lines.push(format!("policy: {}", source.policy.summary()));
    }
    lines.push(format!("state: {}", state_path.display()));
    lines.push("LSP runtime: restarted on next use".into());
    Ok(CommandResult::text(lines.join("\n")))
}

async fn uninstall_plugin(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    if rest.trim().is_empty() || is_help_arg(rest.trim()) {
        return Ok(CommandResult::text(plugin_uninstall_usage()));
    }
    let args = parse_plugin_action_args(rest, "uninstall")?;
    ensure_runtime_plugin_scope(args.scope, "uninstall")?;
    let install_root = plugin_root_dir(context);
    let plugin = resolve_installed_plugin(context, &args.target)?;
    let plugin_name = plugin.display_name().to_string();
    std::fs::remove_dir_all(&plugin.root)?;
    let state_path =
        set_plugin_enabled(&install_root, &plugin_name, true).map_err(anyhow::Error::msg)?;
    kiana_tools::lsp_tool::shutdown_lsp_clients().await;
    Ok(CommandResult::text(format!(
        "Uninstalled plugin: {}\nremoved: {}\nstate: {}\nLSP runtime: restarted on next use",
        plugin_name,
        plugin.root.display(),
        state_path.display()
    )))
}

async fn set_plugin_state(
    context: &CommandContext,
    rest: &str,
    enabled: bool,
) -> Result<CommandResult> {
    let target = rest.trim();
    if target.is_empty() {
        return Err(anyhow!(
            "usage: kiana plugin {} <name|path>",
            if enabled { "enable" } else { "disable" }
        ));
    }
    let plugin = resolve_plugin(context, target)?;
    let root = plugin_root_dir(context);
    let state_path =
        set_plugin_enabled(&root, plugin.display_name(), enabled).map_err(anyhow::Error::msg)?;
    kiana_tools::lsp_tool::shutdown_lsp_clients().await;
    Ok(CommandResult::text(format!(
        "Plugin {}: {}\nstate: {}\nLSP runtime: restarted on next use",
        if enabled { "enabled" } else { "disabled" },
        plugin.display_name(),
        state_path.display()
    )))
}

fn validate_plugins(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let target = rest.trim();
    if !target.is_empty() {
        return Ok(CommandResult::text(format_validation(&resolve_plugin(
            context, target,
        )?)));
    }

    let root = plugin_root_dir(context);
    let plugins = load_installed_plugins(&root)?;
    if plugins.is_empty() {
        return Ok(CommandResult::text(format!(
            "No plugins to validate.\npath: {}",
            root.display()
        )));
    }
    Ok(CommandResult::text(
        plugins
            .iter()
            .map(format_validation)
            .collect::<Vec<_>>()
            .join("\n\n"),
    ))
}

async fn marketplace_command(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let (command, tail) = split_word(rest);
    match command.unwrap_or("list") {
        "" | "list" => marketplace_list(context, tail),
        "add" => marketplace_add(context, tail).await,
        "remove" | "rm" => marketplace_remove(context, tail),
        "update" => marketplace_update(context, tail).await,
        "help" | "--help" | "-h" => Ok(CommandResult::text(marketplace_usage())),
        other => Err(anyhow!(
            "unknown plugin marketplace command '{}'\n\n{}",
            other,
            marketplace_usage()
        )),
    }
}

async fn marketplace_add(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let tokens = split_words(rest);
    if tokens.is_empty() || is_help_arg(&tokens[0]) {
        return Ok(CommandResult::text(marketplace_usage()));
    }
    let mut source = None;
    let mut scope = MarketplaceScope::User;
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index].as_str();
        if token == "--scope" || token == "-s" {
            index += 1;
            let value = tokens.get(index).ok_or_else(|| {
                anyhow!("usage: kiana plugin marketplace add <source> --scope <user|project|local>")
            })?;
            scope = MarketplaceScope::parse(value)?;
            index += 1;
            continue;
        }
        if let Some(value) = token
            .strip_prefix("--scope=")
            .or_else(|| token.strip_prefix("-s="))
        {
            scope = MarketplaceScope::parse(value)?;
            index += 1;
            continue;
        }
        if token == "--sparse" {
            return Err(anyhow!(
                "plugin marketplace --sparse is only supported for git/github materialization; this build records local marketplace declarations only"
            ));
        }
        if token.starts_with('-') {
            return Err(anyhow!(
                "unknown plugin marketplace add option '{}'\n\n{}",
                token,
                marketplace_usage()
            ));
        }
        if source.replace(token.to_string()).is_some() {
            return Err(anyhow!(
                "usage: kiana plugin marketplace add <source> [--scope user|project|local]"
            ));
        }
        index += 1;
    }
    let source = source.ok_or_else(|| anyhow!("usage: kiana plugin marketplace add <source>"))?;
    let (mut name, source) = parse_marketplace_source(context, &source)?;
    let mut install_location = None;
    if let Some(materialized) = materialize_marketplace_source(&source).await? {
        name = materialized.name;
        install_location = Some(materialized.path);
    }
    let file = marketplace_config_file(context, scope);
    let mut config = read_marketplace_config(&file)?;
    config.marketplaces.insert(
        name.clone(),
        MarketplaceEntry {
            scope,
            source,
            install_location,
        },
    );
    write_marketplace_config(&file, &config)?;
    let mut lines = vec![format!(
        "Successfully added marketplace: {name}\nscope: {}\nfile: {}",
        scope.as_str(),
        file.display()
    )];
    if let Some(path) = config
        .marketplaces
        .get(&name)
        .and_then(|entry| entry.install_location.as_ref())
    {
        lines.push(format!("cache: {}", path.display()));
    }
    Ok(CommandResult::text(lines.join("\n")))
}

fn marketplace_list(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let tokens = split_words(rest);
    let mut json = false;
    for token in &tokens {
        match token.as_str() {
            "--json" => json = true,
            "help" | "--help" | "-h" => return Ok(CommandResult::text(marketplace_usage())),
            other => {
                return Err(anyhow!(
                    "unknown plugin marketplace list option '{}'\n\n{}",
                    other,
                    marketplace_usage()
                ))
            }
        }
    }

    let marketplaces = load_marketplace_entries(context)?;
    if json {
        return Ok(CommandResult::text(serde_json::to_string_pretty(
            &marketplaces,
        )?));
    }
    if marketplaces.is_empty() {
        return Ok(CommandResult::text("No marketplaces configured"));
    }
    let mut lines = vec!["Configured marketplaces:".to_string()];
    for marketplace in marketplaces {
        lines.push(format!(
            "- {} [{}] {}",
            marketplace.name,
            marketplace.scope,
            marketplace.source.summary()
        ));
    }
    Ok(CommandResult::text(lines.join("\n")))
}

fn marketplace_remove(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let tokens = split_words(rest);
    if tokens.is_empty() || is_help_arg(&tokens[0]) {
        return Ok(CommandResult::text(marketplace_usage()));
    }
    let mut name = None;
    let mut scope = None;
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index].as_str();
        if token == "--scope" || token == "-s" {
            index += 1;
            let value = tokens.get(index).ok_or_else(|| {
                anyhow!(
                    "usage: kiana plugin marketplace remove <name> --scope <user|project|local>"
                )
            })?;
            scope = Some(MarketplaceScope::parse(value)?);
            index += 1;
            continue;
        }
        if let Some(value) = token
            .strip_prefix("--scope=")
            .or_else(|| token.strip_prefix("-s="))
        {
            scope = Some(MarketplaceScope::parse(value)?);
            index += 1;
            continue;
        }
        if token.starts_with('-') {
            return Err(anyhow!(
                "unknown plugin marketplace remove option '{}'\n\n{}",
                token,
                marketplace_usage()
            ));
        }
        if name.replace(token.to_string()).is_some() {
            return Err(anyhow!("usage: kiana plugin marketplace remove <name>"));
        }
        index += 1;
    }
    let name = name.ok_or_else(|| anyhow!("usage: kiana plugin marketplace remove <name>"))?;
    let scopes = scope.map(|scope| vec![scope]).unwrap_or_else(|| {
        vec![
            MarketplaceScope::Local,
            MarketplaceScope::Project,
            MarketplaceScope::User,
        ]
    });
    for scope in scopes {
        let file = marketplace_config_file(context, scope);
        let mut config = read_marketplace_config(&file)?;
        if config.marketplaces.remove(&name).is_some() {
            write_marketplace_config(&file, &config)?;
            return Ok(CommandResult::text(format!(
                "Successfully removed marketplace: {name}\nfile: {}",
                file.display()
            )));
        }
    }
    Err(anyhow!("marketplace '{}' was not found", name))
}

async fn marketplace_update(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let name = rest.trim();
    if is_help_arg(name) {
        return Ok(CommandResult::text(marketplace_usage()));
    }
    let entries = load_marketplace_entries(context)?;
    if entries.is_empty() {
        return Ok(CommandResult::text("No marketplaces configured"));
    }
    if !name.is_empty() && !entries.iter().any(|entry| entry.name == name) {
        return Err(anyhow!("marketplace '{}' was not found", name));
    }
    if name.is_empty() {
        Ok(CommandResult::text(format!(
            "Successfully updated {} marketplace(s)",
            entries.len()
        )))
    } else {
        Ok(CommandResult::text(format!(
            "Successfully updated marketplace: {name}"
        )))
    }
}

#[derive(Debug)]
struct MaterializedMarketplace {
    name: String,
    path: PathBuf,
}

async fn materialize_marketplace_source(
    source: &MarketplaceSource,
) -> Result<Option<MaterializedMarketplace>> {
    match source {
        MarketplaceSource::Url { url } => Ok(Some(materialize_remote_marketplace(url).await?)),
        MarketplaceSource::Git { .. } | MarketplaceSource::Github { .. } => {
            Ok(Some(materialize_git_marketplace(source).await?))
        }
        MarketplaceSource::Directory { .. } | MarketplaceSource::File { .. } => Ok(None),
    }
}

async fn materialize_remote_marketplace(url: &str) -> Result<MaterializedMarketplace> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, "Kiana-Plugin-Manager")
        .send()
        .await?
        .error_for_status()?;
    let contents = response.text().await?;
    let manifest: LocalMarketplaceManifest = serde_json::from_str(&contents)?;
    let name = manifest
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "remote marketplace at '{}' is missing a non-empty name",
                url
            )
        })?
        .to_string();
    let cache_dir = kiana_home_dir().join("plugin-marketplace-cache");
    std::fs::create_dir_all(&cache_dir)?;
    let safe_name = safe_plugin_dir_name(&name)?;
    let cache_path = cache_dir.join(format!("{safe_name}.json"));
    std::fs::write(&cache_path, format!("{contents}\n"))?;
    Ok(MaterializedMarketplace {
        name,
        path: cache_path,
    })
}

async fn materialize_git_marketplace(
    source: &MarketplaceSource,
) -> Result<MaterializedMarketplace> {
    let (cache_name, clone_url) = match source {
        MarketplaceSource::Git { url } => (source_tail_name(url), url.to_string()),
        MarketplaceSource::Github { repo } => (
            repo.split('/')
                .next_back()
                .unwrap_or(repo)
                .trim_end_matches(".git")
                .to_string(),
            format!("https://github.com/{}.git", repo.trim_end_matches(".git")),
        ),
        _ => return Err(anyhow!("marketplace source is not git-backed")),
    };
    let cache_root = kiana_home_dir()
        .join("plugin-marketplace-cache")
        .join("marketplaces")
        .join(safe_plugin_dir_name(&cache_name)?);
    if cache_root.exists() {
        std::fs::remove_dir_all(&cache_root)?;
    }
    if let Some(parent) = cache_root.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let clone_args = vec![
        "clone".to_string(),
        "--depth".to_string(),
        "1".to_string(),
        git_clone_source(&clone_url),
        cache_root.display().to_string(),
    ];
    run_git_command(&clone_args, None).await?;

    let manifest_path = find_marketplace_manifest_path(&cache_root).ok_or_else(|| {
        anyhow!(
            "git marketplace at {} does not contain .codex-plugin/marketplace.json, .claude-plugin/marketplace.json, or marketplace.json",
            cache_root.display()
        )
    })?;
    let contents = std::fs::read_to_string(&manifest_path)?;
    let manifest: LocalMarketplaceManifest = serde_json::from_str(&contents)?;
    let name = manifest
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "git marketplace at {} is missing a non-empty name",
                manifest_path.display()
            )
        })?
        .to_string();
    Ok(MaterializedMarketplace {
        name,
        path: manifest_path,
    })
}

fn find_marketplace_manifest_path(root: &Path) -> Option<PathBuf> {
    [
        root.join(".codex-plugin").join("marketplace.json"),
        root.join(".claude-plugin").join("marketplace.json"),
        root.join("marketplace.json"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

async fn resolve_install_source(context: &CommandContext, target: &str) -> Result<InstallSource> {
    let path = resolve_path(context, target);
    if path.exists() {
        return Ok(InstallSource {
            root: plugin_root_from_target(&path),
            marketplace: None,
            policy: MarketplacePluginPolicy::default(),
        });
    }

    let (plugin_name, requested_marketplace) = parse_plugin_identifier(target)?;
    let entries = load_marketplace_entries(context)?;
    if entries.is_empty() {
        return Err(anyhow!(
            "plugin '{}' was not found: no marketplaces configured",
            plugin_name
        ));
    }

    for entry in entries {
        if requested_marketplace
            .as_deref()
            .is_some_and(|marketplace| marketplace != entry.name)
        {
            continue;
        }
        match &entry.source {
            MarketplaceSource::Directory { path } => {
                if let Some(root) = find_plugin_in_marketplace_dir(Path::new(path), &plugin_name)? {
                    return Ok(InstallSource {
                        root: root.root,
                        marketplace: Some(entry.name),
                        policy: root.policy,
                    });
                }
            }
            MarketplaceSource::File { path } => {
                if let Some(root) = find_plugin_in_marketplace_file(Path::new(path), &plugin_name)?
                {
                    return Ok(InstallSource {
                        root: root.root,
                        marketplace: Some(entry.name),
                        policy: root.policy,
                    });
                }
            }
            MarketplaceSource::Url { .. }
            | MarketplaceSource::Git { .. }
            | MarketplaceSource::Github { .. } => {
                if let Some(root) = find_plugin_in_remote_marketplace(&entry, &plugin_name).await? {
                    return Ok(InstallSource {
                        root: root.root,
                        marketplace: Some(entry.name),
                        policy: root.policy,
                    });
                }
            }
        }
    }

    if let Some(marketplace) = requested_marketplace {
        return Err(anyhow!(
            "plugin '{}' was not found in marketplace '{}'",
            plugin_name,
            marketplace
        ));
    }
    Err(anyhow!(
        "plugin '{}' was not found in configured marketplaces",
        plugin_name
    ))
}

async fn find_plugin_in_remote_marketplace(
    entry: &MarketplaceListEntry,
    plugin_name: &str,
) -> Result<Option<MarketplacePluginResolution>> {
    let manifest_path = match entry.install_location.as_ref() {
        Some(path) if path.is_file() => path.clone(),
        Some(path) if path.is_dir() => find_marketplace_manifest_path(path).ok_or_else(|| {
            anyhow!(
                "marketplace cache at {} does not contain a marketplace.json",
                path.display()
            )
        })?,
        _ => match &entry.source {
            MarketplaceSource::Url { url } => materialize_remote_marketplace(url).await?.path,
            MarketplaceSource::Git { .. } | MarketplaceSource::Github { .. } => {
                materialize_git_marketplace(&entry.source).await?.path
            }
            _ => return Ok(None),
        },
    };
    let local_base = match &entry.source {
        MarketplaceSource::Git { .. } | MarketplaceSource::Github { .. } => {
            Some(marketplace_manifest_base_dir(&manifest_path)?)
        }
        _ => None,
    };

    find_plugin_from_remote_marketplace_manifest_file(
        &manifest_path,
        plugin_name,
        local_base.as_deref(),
    )
    .await
}

async fn find_plugin_from_remote_marketplace_manifest_file(
    manifest_path: &Path,
    plugin_name: &str,
    local_base: Option<&Path>,
) -> Result<Option<MarketplacePluginResolution>> {
    let contents = std::fs::read_to_string(manifest_path)?;
    let manifest: LocalMarketplaceManifest = serde_json::from_str(&contents)?;
    let marketplace_name = manifest.name.clone().unwrap_or_else(|| {
        manifest_path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("marketplace")
            .to_string()
    });
    for entry in manifest.plugins {
        if normalize_target(&entry.name) != normalize_target(plugin_name) {
            continue;
        }
        let plugin_root = match local_base {
            Some(base) if marketplace_entry_source_is_local(&entry.source) => {
                marketplace_entry_source_path(base, &entry.source)?
            }
            _ => {
                marketplace_entry_remote_source_path(&marketplace_name, &entry.name, &entry.source)
                    .await?
            }
        };
        if !plugin_root.exists() {
            return Err(anyhow!(
                "marketplace entry '{}' points to missing plugin path {}",
                entry.name,
                plugin_root.display()
            ));
        }
        if !plugin_dir_matches(&plugin_root, plugin_name)? {
            return Err(anyhow!(
                "marketplace entry '{}' resolved to {}, but that directory does not contain a matching plugin manifest",
                entry.name,
                plugin_root.display()
            ));
        }
        return Ok(Some(MarketplacePluginResolution {
            root: plugin_root,
            policy: MarketplacePluginPolicy::from_entry(&entry),
        }));
    }
    Ok(None)
}

fn marketplace_entry_source_is_local(source: &Value) -> bool {
    if source.is_string() {
        return true;
    }
    source
        .as_object()
        .and_then(|object| object.get("source"))
        .and_then(Value::as_str)
        .map(|source_type| matches!(source_type, "directory" | "file" | "path" | "local"))
        .unwrap_or(true)
}

async fn marketplace_entry_remote_source_path(
    marketplace_name: &str,
    entry_name: &str,
    source: &Value,
) -> Result<PathBuf> {
    if let Some(path) = source.as_str() {
        return Err(anyhow!(
            "remote marketplace '{}' plugin '{}' uses local source '{}'; use a remote source object instead",
            marketplace_name,
            entry_name,
            path
        ));
    }
    let object = source.as_object().ok_or_else(|| {
        anyhow!(
            "remote marketplace '{}' plugin '{}' source must be a string or object",
            marketplace_name,
            entry_name
        )
    })?;
    let source_type = object
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("directory");
    match source_type {
        "url" => {
            let url = object
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!(
                        "remote marketplace '{}' plugin '{}' source object requires 'url'",
                        marketplace_name,
                        entry_name
                    )
                })?;
            clone_remote_plugin_source(
                marketplace_name,
                entry_name,
                url,
                object.get("ref").and_then(Value::as_str),
                object.get("sha").and_then(Value::as_str),
                None,
            )
            .await
        }
        "git" => {
            let url = object
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!(
                        "remote marketplace '{}' plugin '{}' source object requires 'url'",
                        marketplace_name,
                        entry_name
                    )
                })?;
            clone_remote_plugin_source(
                marketplace_name,
                entry_name,
                url,
                object.get("ref").and_then(Value::as_str),
                object.get("sha").and_then(Value::as_str),
                None,
            )
            .await
        }
        "github" => {
            let repo = object
                .get("repo")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!(
                        "remote marketplace '{}' plugin '{}' source object requires 'repo'",
                        marketplace_name,
                        entry_name
                    )
                })?;
            clone_remote_plugin_source(
                marketplace_name,
                entry_name,
                &format!("https://github.com/{repo}.git"),
                object.get("ref").and_then(Value::as_str),
                object.get("sha").and_then(Value::as_str),
                None,
            )
            .await
        }
        "git-subdir" => {
            let url = object
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!(
                        "remote marketplace '{}' plugin '{}' source object requires 'url'",
                        marketplace_name,
                        entry_name
                    )
                })?;
            let subdir = object
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!(
                        "remote marketplace '{}' plugin '{}' source object requires 'path'",
                        marketplace_name,
                        entry_name
                    )
                })?;
            clone_remote_plugin_source(
                marketplace_name,
                entry_name,
                url,
                object.get("ref").and_then(Value::as_str),
                object.get("sha").and_then(Value::as_str),
                Some(subdir),
            )
            .await
        }
        "npm" | "pip" => Err(anyhow!(
            "remote marketplace '{}' plugin '{}' source type '{}' is not implemented yet",
            marketplace_name,
            entry_name,
            source_type
        )),
        "directory" | "file" | "path" | "local" => Err(anyhow!(
            "remote marketplace '{}' plugin '{}' uses local source type '{}' which is not supported",
            marketplace_name,
            entry_name,
            source_type
        )),
        other => Err(anyhow!(
            "remote marketplace '{}' plugin '{}' uses unsupported source type '{}'",
            marketplace_name,
            entry_name,
            other
        )),
    }
}

async fn clone_remote_plugin_source(
    marketplace_name: &str,
    entry_name: &str,
    url: &str,
    ref_name: Option<&str>,
    sha: Option<&str>,
    subdir: Option<&str>,
) -> Result<PathBuf> {
    let cache_root = kiana_home_dir()
        .join("plugin-marketplace-cache")
        .join("plugins")
        .join(safe_plugin_dir_name(marketplace_name)?)
        .join(safe_plugin_dir_name(entry_name)?);
    if cache_root.exists() {
        std::fs::remove_dir_all(&cache_root)?;
    }
    if let Some(parent) = cache_root.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut clone_args = vec!["clone".to_string(), "--depth".to_string(), "1".to_string()];
    if let Some(branch) = ref_name {
        clone_args.push("--branch".to_string());
        clone_args.push(branch.to_string());
    }
    clone_args.push(git_clone_source(url));
    clone_args.push(cache_root.display().to_string());
    run_git_command(&clone_args, None).await?;

    if let Some(sha) = sha {
        let checkout_args = vec!["checkout".to_string(), sha.to_string()];
        run_git_command(&checkout_args, Some(&cache_root)).await?;
    }

    let plugin_root = match subdir {
        Some(subdir) => resolve_remote_plugin_subdir(&cache_root, subdir)?,
        None => cache_root.clone(),
    };
    if !plugin_root.exists() {
        return Err(anyhow!(
            "remote plugin source for '{}' resolved to missing path {}",
            entry_name,
            plugin_root.display()
        ));
    }
    Ok(plugin_root)
}

fn resolve_remote_plugin_subdir(base: &Path, subdir: &str) -> Result<PathBuf> {
    let path = Path::new(subdir);
    if path.is_absolute()
        || subdir.is_empty()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(anyhow!(
            "invalid remote plugin subdir '{}'; use a relative path inside the repository",
            subdir
        ));
    }
    Ok(base.join(path))
}

async fn run_git_command(args: &[String], cwd: Option<&Path>) -> Result<()> {
    let mut command = tokio::process::Command::new("git");
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env("GIT_ASKPASS", "");
    if let Some(cwd) = cwd {
        command.arg("-C").arg(cwd);
    }
    command.args(args);
    let output = command.output().await?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(anyhow!("git {} failed: {}", args.join(" "), stderr.trim()))
}

fn find_plugin_in_marketplace_dir(
    marketplace_root: &Path,
    plugin_name: &str,
) -> Result<Option<MarketplacePluginResolution>> {
    if let Some(path) = find_plugin_from_marketplace_manifest(marketplace_root, plugin_name)? {
        return Ok(Some(path));
    }
    if plugin_dir_matches(marketplace_root, plugin_name)? {
        return Ok(Some(MarketplacePluginResolution {
            root: marketplace_root.to_path_buf(),
            policy: MarketplacePluginPolicy::default(),
        }));
    }
    for plugin_root in all_plugin_roots_in(marketplace_root) {
        if plugin_dir_matches(&plugin_root, plugin_name)? {
            return Ok(Some(MarketplacePluginResolution {
                root: plugin_root,
                policy: MarketplacePluginPolicy::default(),
            }));
        }
    }
    Ok(None)
}

fn find_plugin_in_marketplace_file(
    marketplace_file: &Path,
    plugin_name: &str,
) -> Result<Option<MarketplacePluginResolution>> {
    find_plugin_from_marketplace_manifest_file(marketplace_file, plugin_name)
}

fn find_plugin_from_marketplace_manifest(
    marketplace_root: &Path,
    plugin_name: &str,
) -> Result<Option<MarketplacePluginResolution>> {
    for path in [
        marketplace_root
            .join(".codex-plugin")
            .join("marketplace.json"),
        marketplace_root
            .join(".claude-plugin")
            .join("marketplace.json"),
        marketplace_root.join("marketplace.json"),
    ] {
        if path.is_file() {
            if let Some(plugin_root) =
                find_plugin_from_marketplace_manifest_file(&path, plugin_name)?
            {
                return Ok(Some(plugin_root));
            }
        }
    }
    Ok(None)
}

fn find_plugin_from_marketplace_manifest_file(
    manifest_path: &Path,
    plugin_name: &str,
) -> Result<Option<MarketplacePluginResolution>> {
    let contents = std::fs::read_to_string(manifest_path)?;
    let manifest: LocalMarketplaceManifest = serde_json::from_str(&contents)?;
    let base = marketplace_manifest_base_dir(manifest_path)?;
    for entry in manifest.plugins {
        if normalize_target(&entry.name) != normalize_target(plugin_name) {
            continue;
        }
        let plugin_root = marketplace_entry_source_path(&base, &entry.source)?;
        if !plugin_root.exists() {
            return Err(anyhow!(
                "marketplace entry '{}' points to missing plugin path {}",
                entry.name,
                plugin_root.display()
            ));
        }
        if !plugin_dir_matches(&plugin_root, plugin_name)? {
            return Err(anyhow!(
                "marketplace entry '{}' resolved to {}, but that directory does not contain a matching plugin manifest",
                entry.name,
                plugin_root.display()
            ));
        }
        return Ok(Some(MarketplacePluginResolution {
            root: plugin_root,
            policy: MarketplacePluginPolicy::from_entry(&entry),
        }));
    }
    Ok(None)
}

fn marketplace_manifest_base_dir(manifest_path: &Path) -> Result<PathBuf> {
    let parent = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("marketplace manifest path has no parent"))?;
    if parent
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == ".codex-plugin" || name == ".claude-plugin")
    {
        return Ok(parent.parent().unwrap_or(parent).to_path_buf());
    }
    Ok(parent.to_path_buf())
}

fn marketplace_entry_source_path(base: &Path, source: &Value) -> Result<PathBuf> {
    if let Some(path) = source.as_str() {
        return resolve_marketplace_child_path(base, path);
    }
    if let Some(object) = source.as_object() {
        let source_type = object
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("directory");
        let path = object
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("local marketplace plugin source object requires 'path'"))?;
        return match source_type {
            "directory" | "file" | "path" | "local" => resolve_marketplace_child_path(base, path),
            "url" | "git" | "github" | "npm" | "pip" | "git-subdir" => Err(anyhow!(
                "plugin source type '{}' is remote and is not implemented yet",
                source_type
            )),
            other => Err(anyhow!(
                "unsupported marketplace plugin source type '{}'",
                other
            )),
        };
    }
    Err(anyhow!(
        "marketplace plugin source must be a local path string or object"
    ))
}

fn resolve_marketplace_child_path(base: &Path, raw_path: &str) -> Result<PathBuf> {
    let path = Path::new(raw_path);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    if raw_path.is_empty()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(anyhow!(
            "invalid local marketplace plugin path '{}'; use a relative path inside the marketplace or an absolute path",
            raw_path
        ));
    }
    Ok(base.join(path))
}

fn plugin_dir_matches(root: &Path, target: &str) -> Result<bool> {
    if find_manifest_path(root).is_none() {
        return Ok(false);
    }
    Ok(read_plugin(root.to_path_buf())?.target_matches(target))
}

fn resolve_installed_plugin(context: &CommandContext, target: &str) -> Result<PluginInfo> {
    let root = plugin_root_dir(context);
    let plugins = load_installed_plugins(&root)?;
    let path = resolve_path(context, target);
    if path.exists() {
        let target_root = plugin_root_from_target(&path);
        let target_root = target_root.canonicalize().unwrap_or(target_root);
        return plugins
            .into_iter()
            .find(|plugin| {
                plugin
                    .root
                    .canonicalize()
                    .unwrap_or_else(|_| plugin.root.clone())
                    == target_root
            })
            .ok_or_else(|| {
                anyhow!(
                    "plugin path '{}' is not installed under {}",
                    path.display(),
                    root.display()
                )
            });
    }
    plugins
        .into_iter()
        .find(|plugin| plugin.target_matches(target))
        .ok_or_else(|| anyhow!("plugin '{}' was not found in {}", target, root.display()))
}

fn parse_plugin_identifier(target: &str) -> Result<(String, Option<String>)> {
    let target = target.trim();
    let (name, marketplace) = target
        .split_once('@')
        .map(|(name, marketplace)| (name.trim(), Some(marketplace.trim())))
        .unwrap_or((target, None));
    if name.is_empty() {
        return Err(anyhow!("plugin name cannot be empty"));
    }
    if marketplace.is_some_and(str::is_empty) {
        return Err(anyhow!("marketplace name cannot be empty"));
    }
    Ok((name.to_string(), marketplace.map(str::to_string)))
}

fn parse_plugin_action_args(rest: &str, command: &str) -> Result<PluginActionArgs> {
    let tokens = split_words(rest);
    let mut target = None;
    let mut scope = None;
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index].as_str();
        if token == "--scope" || token == "-s" {
            index += 1;
            let value = tokens
                .get(index)
                .ok_or_else(|| anyhow!("usage: kiana plugin {command} <plugin> --scope <user>"))?;
            scope = Some(MarketplaceScope::parse(value)?);
            index += 1;
            continue;
        }
        if let Some(value) = token
            .strip_prefix("--scope=")
            .or_else(|| token.strip_prefix("-s="))
        {
            scope = Some(MarketplaceScope::parse(value)?);
            index += 1;
            continue;
        }
        if token == "--keep-data" && command == "uninstall" {
            index += 1;
            continue;
        }
        if token.starts_with('-') {
            return Err(anyhow!(
                "unknown plugin {command} option '{}'\n\n{}",
                token,
                if command == "install" {
                    plugin_install_usage()
                } else {
                    plugin_uninstall_usage()
                }
            ));
        }
        if target.replace(token.to_string()).is_some() {
            return Err(anyhow!("usage: kiana plugin {command} <plugin>"));
        }
        index += 1;
    }
    let target = target.ok_or_else(|| anyhow!("usage: kiana plugin {command} <plugin>"))?;
    Ok(PluginActionArgs { target, scope })
}

fn ensure_runtime_plugin_scope(scope: Option<MarketplaceScope>, command: &str) -> Result<()> {
    match scope.unwrap_or(MarketplaceScope::User) {
        MarketplaceScope::User => Ok(()),
        other => Err(anyhow!(
            "kiana plugin {command} --scope {} is not implemented yet; this build installs into the active user plugin path",
            other
        )),
    }
}

fn safe_plugin_dir_name(name: &str) -> Result<String> {
    let name = name.trim().trim_start_matches('/');
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.chars().any(char::is_whitespace)
    {
        return Err(anyhow!(
            "plugin name '{}' cannot be used as an install directory",
            name
        ));
    }
    Ok(name.to_string())
}

fn same_existing_path(left: &Path, right: &Path) -> bool {
    left.exists()
        && right.exists()
        && left
            .canonicalize()
            .ok()
            .zip(right.canonicalize().ok())
            .is_some_and(|(left, right)| left == right)
}

fn copy_plugin_dir(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_dir() {
        return Err(anyhow!(
            "plugin source is not a directory: {}",
            source.display()
        ));
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    copy_dir_recursive(source, destination)
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = std::fs::metadata(&source_path)?;
        if metadata.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            std::fs::copy(&source_path, &destination_path)?;
        } else {
            return Err(anyhow!(
                "unsupported plugin file type at {}",
                source_path.display()
            ));
        }
    }
    Ok(())
}

fn resolve_plugin(context: &CommandContext, target: &str) -> Result<PluginInfo> {
    let path = resolve_path(context, target);
    if path.exists() {
        let root = plugin_root_from_target(&path);
        return read_plugin(root);
    }

    let root = plugin_root_dir(context);
    let plugins = load_installed_plugins(&root)?;
    plugins
        .into_iter()
        .find(|plugin| plugin.target_matches(target))
        .ok_or_else(|| anyhow!("plugin '{}' was not found in {}", target, root.display()))
}

fn load_installed_plugins(root: &Path) -> Result<Vec<PluginInfo>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let disabled = disabled_plugin_names(root);
    let mut plugins = Vec::new();
    for path in all_plugin_roots_in(root) {
        let mut plugin = read_plugin(path)?;
        plugin.enabled = !plugin_is_disabled(&plugin.root, &disabled);
        plugins.push(plugin);
    }
    plugins.sort_by(|a, b| a.display_name().cmp(&b.display_name()));
    Ok(plugins)
}

fn read_plugin(root: PathBuf) -> Result<PluginInfo> {
    let folder_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("plugin")
        .to_string();
    let manifest_path = find_manifest_path(&root);
    let components = count_components(&root);
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut manifest = None;
    let mut manifest_name = None;
    let mut version = None;
    let mut description = None;

    match manifest_path.as_ref() {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(contents) => match serde_json::from_str::<Value>(&contents) {
                Ok(value) => {
                    if let Some(object) = value.as_object() {
                        manifest_name = object
                            .get("name")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_string);
                        version = object
                            .get("version")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_string);
                        description = object
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_string);
                        if manifest_name.is_none() {
                            errors.push("plugin.json missing non-empty string field 'name'".into());
                        } else if manifest_name.as_deref() != Some(folder_name.as_str()) {
                            warnings.push(format!(
                                "plugin.json name '{}' differs from folder '{}'",
                                manifest_name.as_deref().unwrap_or_default(),
                                folder_name
                            ));
                        }
                    } else {
                        errors.push("plugin.json must contain a JSON object".into());
                    }
                    manifest = Some(value);
                }
                Err(error) => errors.push(format!("plugin.json is not valid JSON: {}", error)),
            },
            Err(error) => errors.push(format!("failed to read plugin.json: {}", error)),
        },
        None => {
            errors.push("missing .codex-plugin/plugin.json or .claude-plugin/plugin.json".into())
        }
    }

    if components.total() == 0 {
        warnings.push(
            "no commands, agents, skills, hooks, output styles, apps, or MCP config found".into(),
        );
    }

    Ok(PluginInfo {
        id: manifest_name.clone().unwrap_or(folder_name),
        manifest_name,
        version,
        description,
        root,
        manifest_path,
        enabled: true,
        components,
        valid: errors.is_empty(),
        errors,
        warnings,
        manifest,
    })
}

fn plugin_root_from_target(path: &Path) -> PathBuf {
    if path.is_file() {
        if path.file_name().and_then(|name| name.to_str()) == Some("plugin.json") {
            if let Some(parent) = path.parent() {
                if parent
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name == ".codex-plugin" || name == ".claude-plugin")
                {
                    return parent.parent().unwrap_or(parent).to_path_buf();
                }
            }
        }
        return path.parent().unwrap_or(path).to_path_buf();
    }
    path.to_path_buf()
}

fn count_components(root: &Path) -> PluginComponents {
    PluginComponents {
        commands: count_files(&root.join("commands"), &["md", "json", "toml"]),
        agents: count_files(&root.join("agents"), &["md", "json", "toml"]),
        skills: count_skill_dirs(&root.join("skills")),
        hooks: usize::from(root.join("hooks").join("hooks.json").is_file()),
        output_styles: count_files(&root.join("output-styles"), &["md"]),
        lsp_servers: usize::from(root.join(".lsp.json").is_file()),
        apps: count_files(root, &["app.json"]),
        mcp_servers: usize::from(root.join(".mcp.json").is_file()),
    }
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

fn filtered_plugins(mut plugins: Vec<PluginInfo>, query: &str) -> Vec<PluginInfo> {
    let query = query.trim().to_lowercase();
    if !query.is_empty() {
        plugins.retain(|plugin| plugin.matches(&query));
    }
    plugins
}

fn format_validation(plugin: &PluginInfo) -> String {
    let mut lines = vec![format!(
        "Validating plugin: {}\nroot: {}\nmanifest: {}",
        plugin.display_name(),
        plugin.root.display(),
        plugin
            .manifest_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "missing".to_string())
    )];
    if plugin.errors.is_empty() {
        lines.push("errors: none".into());
    } else {
        lines.push(format!("errors: {}", plugin.errors.len()));
        for error in &plugin.errors {
            lines.push(format!("- {}", error));
        }
    }
    if plugin.warnings.is_empty() {
        lines.push("warnings: none".into());
    } else {
        lines.push(format!("warnings: {}", plugin.warnings.len()));
        for warning in &plugin.warnings {
            lines.push(format!("- {}", warning));
        }
    }
    lines.push(if plugin.valid {
        "Validation passed".into()
    } else {
        "Validation failed".into()
    });
    lines.join("\n")
}

fn plugin_init_summary(plugin: &PluginInfo) -> Value {
    serde_json::json!({
        "id": &plugin.id,
        "name": plugin.display_name(),
        "version": &plugin.version,
        "description": &plugin.description,
        "root": plugin.root.display().to_string(),
        "enabled": plugin.enabled,
        "valid": plugin.valid,
        "components": &plugin.components,
        "errors": &plugin.errors,
        "warnings": &plugin.warnings,
    })
}

fn plugin_root_dir(context: &CommandContext) -> PathBuf {
    if let Ok(path) = std::env::var("KIANA_PLUGINS_DIR") {
        return resolve_path(context, &path);
    }
    kiana_home_dir().join("plugins")
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

fn resolve_path(context: &CommandContext, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        cwd(context).join(path)
    }
}

fn split_words(input: &str) -> Vec<String> {
    input.split_whitespace().map(str::to_string).collect()
}

fn is_help_arg(arg: &str) -> bool {
    matches!(arg, "help" | "--help" | "-h")
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
    "usage: kiana plugin [list|status|json [query]|marketplace <add|list|remove|update>|install <plugin>|uninstall <plugin>|show <name>|enable <name>|disable <name>|path [name]|validate [name|path]]"
}

fn marketplace_usage() -> &'static str {
    "Usage: kiana plugin marketplace [list|add|remove|update]\n       kiana plugin marketplace list [--json]\n       kiana plugin marketplace add <source> [--scope user|project|local]\n       kiana plugin marketplace remove <name> [--scope user|project|local]\n       kiana plugin marketplace update [name]"
}

fn plugin_install_usage() -> &'static str {
    "Usage: kiana plugin install <plugin|plugin@marketplace|path> [--scope user]"
}

fn plugin_uninstall_usage() -> &'static str {
    "Usage: kiana plugin uninstall <plugin|path> [--scope user] [--keep-data]"
}

#[derive(Debug)]
struct PluginActionArgs {
    target: String,
    scope: Option<MarketplaceScope>,
}

#[derive(Debug)]
struct InstallSource {
    root: PathBuf,
    marketplace: Option<String>,
    policy: MarketplacePluginPolicy,
}

#[derive(Debug, Deserialize)]
struct LocalMarketplaceManifest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    plugins: Vec<LocalMarketplacePluginEntry>,
}

#[derive(Debug, Deserialize)]
struct LocalMarketplacePluginEntry {
    name: String,
    source: Value,
    #[serde(default)]
    interface: Option<Value>,
    #[serde(default)]
    policy: MarketplacePluginPolicy,
    #[serde(default, alias = "installPolicy", alias = "install-policy")]
    install_policy: Option<String>,
    #[serde(default, alias = "authPolicy", alias = "auth-policy")]
    auth_policy: Option<String>,
    #[serde(default)]
    availability: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketplacePluginPolicy {
    #[serde(
        default,
        alias = "install_policy",
        alias = "install-policy",
        skip_serializing_if = "Option::is_none"
    )]
    install_policy: Option<String>,
    #[serde(
        default,
        alias = "auth_policy",
        alias = "auth-policy",
        skip_serializing_if = "Option::is_none"
    )]
    auth_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    availability: Option<String>,
}

impl MarketplacePluginPolicy {
    fn from_entry(entry: &LocalMarketplacePluginEntry) -> Self {
        let mut policy = entry.policy.clone();
        if policy.install_policy.is_none() {
            policy.install_policy = entry.install_policy.clone();
        }
        if policy.auth_policy.is_none() {
            policy.auth_policy = entry.auth_policy.clone();
        }
        if policy.availability.is_none() {
            policy.availability = entry.availability.clone();
        }
        policy
    }

    fn availability_key(&self) -> Option<String> {
        self.availability
            .as_deref()
            .map(|value| value.trim().replace(['-', ' '], "_").to_ascii_lowercase())
    }

    fn is_not_available(&self) -> bool {
        self.availability_key()
            .is_some_and(|availability| availability == "not_available")
    }

    fn is_empty(&self) -> bool {
        self.install_policy.is_none() && self.auth_policy.is_none() && self.availability.is_none()
    }

    fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(value) = &self.availability {
            parts.push(format!("availability={value}"));
        }
        if let Some(value) = &self.install_policy {
            parts.push(format!("installPolicy={value}"));
        }
        if let Some(value) = &self.auth_policy {
            parts.push(format!("authPolicy={value}"));
        }
        parts.join(" ")
    }
}

#[derive(Debug, Clone, Serialize)]
struct MarketplacePluginSummary {
    name: String,
    source: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    interface: Option<Value>,
    policy: MarketplacePluginPolicy,
}

#[derive(Debug)]
struct MarketplacePluginResolution {
    root: PathBuf,
    policy: MarketplacePluginPolicy,
}

#[derive(Debug, Clone, Serialize)]
struct PluginInfo {
    id: String,
    manifest_name: Option<String>,
    version: Option<String>,
    description: Option<String>,
    root: PathBuf,
    manifest_path: Option<PathBuf>,
    enabled: bool,
    components: PluginComponents,
    valid: bool,
    errors: Vec<String>,
    warnings: Vec<String>,
    manifest: Option<Value>,
}

impl PluginInfo {
    fn display_name(&self) -> &str {
        self.manifest_name.as_deref().unwrap_or(&self.id)
    }

    fn target_matches(&self, target: &str) -> bool {
        let target = normalize_target(target);
        if target.is_empty() {
            return false;
        }
        [
            Some(self.id.as_str()),
            self.manifest_name.as_deref(),
            self.root.file_name().and_then(|name| name.to_str()),
        ]
        .into_iter()
        .flatten()
        .any(|field| normalize_target(field) == target)
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.trim().trim_start_matches('/').to_lowercase();
        [
            Some(self.id.as_str()),
            self.manifest_name.as_deref(),
            self.description.as_deref(),
            self.root.to_str(),
            self.manifest_path.as_ref().and_then(|path| path.to_str()),
        ]
        .into_iter()
        .flatten()
        .any(|field| field.to_lowercase().contains(&query))
    }
}

fn normalize_target(value: &str) -> String {
    value.trim().trim_start_matches('/').to_ascii_lowercase()
}

#[derive(Debug, Clone, Default, Serialize)]
struct PluginComponents {
    commands: usize,
    agents: usize,
    skills: usize,
    hooks: usize,
    output_styles: usize,
    lsp_servers: usize,
    apps: usize,
    mcp_servers: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum MarketplaceScope {
    User,
    Project,
    Local,
}

impl MarketplaceScope {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "user" => Ok(Self::User),
            "project" => Ok(Self::Project),
            "local" => Ok(Self::Local),
            _ => Err(anyhow!(
                "invalid marketplace scope '{}'; expected user, project, or local",
                value
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
            Self::Local => "local",
        }
    }
}

impl std::fmt::Display for MarketplaceScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MarketplaceConfig {
    #[serde(default)]
    marketplaces: BTreeMap<String, MarketplaceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MarketplaceEntry {
    scope: MarketplaceScope,
    source: MarketplaceSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    install_location: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "lowercase")]
enum MarketplaceSource {
    Directory { path: String },
    File { path: String },
    Url { url: String },
    Git { url: String },
    Github { repo: String },
}

impl MarketplaceSource {
    fn summary(&self) -> String {
        match self {
            Self::Directory { path } => format!("Directory ({path})"),
            Self::File { path } => format!("File ({path})"),
            Self::Url { url } => format!("URL ({url})"),
            Self::Git { url } => format!("Git ({url})"),
            Self::Github { repo } => format!("GitHub ({repo})"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct MarketplaceListEntry {
    name: String,
    scope: MarketplaceScope,
    source: MarketplaceSource,
    config_file: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    install_location: Option<PathBuf>,
    #[serde(default)]
    plugins: Vec<MarketplacePluginSummary>,
}

fn read_marketplace_config(path: &Path) -> Result<MarketplaceConfig> {
    if !path.exists() {
        return Ok(MarketplaceConfig {
            marketplaces: BTreeMap::new(),
        });
    }
    let contents = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}

fn write_marketplace_config(path: &Path, config: &MarketplaceConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let contents = serde_json::to_string_pretty(config)?;
    std::fs::write(path, format!("{contents}\n"))?;
    Ok(())
}

fn load_marketplace_entries(context: &CommandContext) -> Result<Vec<MarketplaceListEntry>> {
    let mut entries = Vec::new();
    let project_resources_allowed =
        project_trust_from_app_state(&context.app_state).allows_project_resources();
    for scope in [
        MarketplaceScope::User,
        MarketplaceScope::Project,
        MarketplaceScope::Local,
    ] {
        if !project_resources_allowed
            && matches!(scope, MarketplaceScope::Project | MarketplaceScope::Local)
        {
            continue;
        }
        let file = marketplace_config_file(context, scope);
        let config = read_marketplace_config(&file)?;
        for (name, entry) in config.marketplaces {
            let plugins =
                marketplace_plugin_summaries(&entry.source, entry.install_location.as_deref())?;
            entries.push(MarketplaceListEntry {
                name,
                scope,
                source: entry.source,
                config_file: file.clone(),
                install_location: entry.install_location,
                plugins,
            });
        }
    }
    entries.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.scope.as_str().cmp(b.scope.as_str()))
    });
    Ok(entries)
}

fn marketplace_plugin_summaries(
    source: &MarketplaceSource,
    install_location: Option<&Path>,
) -> Result<Vec<MarketplacePluginSummary>> {
    let manifest_path = match source {
        MarketplaceSource::Directory { path } => find_marketplace_manifest_path(Path::new(path)),
        MarketplaceSource::File { path } => Some(PathBuf::from(path)),
        MarketplaceSource::Url { .. }
        | MarketplaceSource::Git { .. }
        | MarketplaceSource::Github { .. } => install_location.and_then(|path| {
            if path.is_file() {
                Some(path.to_path_buf())
            } else if path.is_dir() {
                find_marketplace_manifest_path(path)
            } else {
                None
            }
        }),
    };
    let Some(manifest_path) = manifest_path else {
        return Ok(Vec::new());
    };
    let contents = std::fs::read_to_string(manifest_path)?;
    let manifest: LocalMarketplaceManifest = serde_json::from_str(&contents)?;
    Ok(manifest
        .plugins
        .into_iter()
        .map(|entry| {
            let policy = MarketplacePluginPolicy::from_entry(&entry);
            MarketplacePluginSummary {
                name: entry.name,
                source: entry.source,
                interface: entry.interface,
                policy,
            }
        })
        .collect())
}

fn marketplace_config_file(context: &CommandContext, scope: MarketplaceScope) -> PathBuf {
    match scope {
        MarketplaceScope::User => kiana_home_dir().join("plugin-marketplaces.json"),
        MarketplaceScope::Project => cwd(context).join(".kiana").join("plugin-marketplaces.json"),
        MarketplaceScope::Local => cwd(context)
            .join(".kiana")
            .join("plugin-marketplaces.local.json"),
    }
}

fn parse_marketplace_source(
    context: &CommandContext,
    raw_source: &str,
) -> Result<(String, MarketplaceSource)> {
    let path = resolve_path(context, raw_source);
    if path.is_dir() {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("marketplace directory must have a name"))?;
        return Ok((
            name,
            MarketplaceSource::Directory {
                path: path.display().to_string(),
            },
        ));
    }
    if path.is_file() {
        let name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("marketplace file must have a name"))?;
        return Ok((
            name,
            MarketplaceSource::File {
                path: path.display().to_string(),
            },
        ));
    }
    if raw_source.starts_with("http://") || raw_source.starts_with("https://") {
        let name = source_tail_name(raw_source);
        let source = if raw_source.ends_with(".git") {
            MarketplaceSource::Git {
                url: raw_source.to_string(),
            }
        } else {
            MarketplaceSource::Url {
                url: raw_source.to_string(),
            }
        };
        return Ok((name, source));
    }
    if raw_source.contains("://") || raw_source.ends_with(".git") {
        return Ok((
            source_tail_name(raw_source),
            MarketplaceSource::Git {
                url: raw_source.to_string(),
            },
        ));
    }
    if looks_like_github_repo(raw_source) {
        return Ok((
            raw_source
                .split('/')
                .next_back()
                .unwrap_or(raw_source)
                .trim_end_matches(".git")
                .to_string(),
            MarketplaceSource::Github {
                repo: raw_source.trim_end_matches(".git").to_string(),
            },
        ));
    }
    Err(anyhow!(
        "invalid marketplace source '{}'; expected directory, file, URL, git URL, or owner/repo",
        raw_source
    ))
}

fn source_tail_name(source: &str) -> String {
    source
        .trim_end_matches(|ch| ch == '/' || ch == '\\')
        .split(|ch| ch == '/' || ch == '\\')
        .next_back()
        .unwrap_or("marketplace")
        .trim_end_matches(".git")
        .to_string()
}

fn git_clone_source(url: &str) -> String {
    let Some(path) = url.strip_prefix("file://") else {
        return url.to_string();
    };
    normalize_file_url_path_for_git(path)
}

fn normalize_file_url_path_for_git(path: &str) -> String {
    let mut path = path.replace('/', "\\");
    if path.as_bytes().get(0) == Some(&b'\\') && path.as_bytes().get(2) == Some(&b':') {
        path.remove(0);
    }
    if let Some(stripped) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{stripped}");
    }
    path.strip_prefix(r"\\?\")
        .map(str::to_string)
        .unwrap_or(path)
}

fn looks_like_github_repo(source: &str) -> bool {
    let mut parts = source.split('/');
    let Some(owner) = parts.next() else {
        return false;
    };
    let Some(repo) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && !owner.trim().is_empty()
        && !repo.trim().is_empty()
        && !source.contains(char::is_whitespace)
}

impl PluginComponents {
    fn total(&self) -> usize {
        self.commands
            + self.agents
            + self.skills
            + self.hooks
            + self.output_styles
            + self.lsp_servers
            + self.apps
            + self.mcp_servers
    }
}

#[cfg(test)]
mod tests {
    use super::PluginCommand;
    use crate::local_state::env_lock;
    use crate::{Command, CommandContext};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::fs;
    use std::process::Command as ProcessCommand;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn temp_root(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-plugin-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn context(args: &str, cwd: &std::path::Path) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::from([("cwd".to_string(), json!(cwd))]),
        }
    }

    fn context_with_state(
        args: &str,
        cwd: &std::path::Path,
        app_state: HashMap<String, Value>,
    ) -> CommandContext {
        let mut app_state = app_state;
        app_state.insert("cwd".to_string(), json!(cwd));
        CommandContext {
            args: args.to_string(),
            app_state,
        }
    }

    fn write_manifest(plugin_root: &std::path::Path, name: &str) {
        let manifest_dir = plugin_root.join(".codex-plugin");
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::write(
            manifest_dir.join("plugin.json"),
            serde_json::to_string_pretty(&json!({
                "name": name,
                "version": "1.2.3",
                "description": "Test plugin"
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn run_git(cwd: &std::path::Path, args: &[&str]) {
        let output = ProcessCommand::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    async fn serve_json_once(body: String) -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request_buffer = [0; 2048];
            let _ = socket.read(&mut request_buffer).await.unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        format!("http://{address}/marketplace.json")
    }

    #[tokio::test]
    async fn plugin_lists_manifest_backed_plugins_and_invalid_dirs() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("list");
        let cwd = root.join("project");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("alpha");
        write_manifest(&plugin_root, "alpha");
        fs::create_dir_all(plugin_root.join("commands")).unwrap();
        fs::write(plugin_root.join("commands").join("hello.md"), "# hello").unwrap();
        fs::create_dir_all(plugins_dir.join("broken")).unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let result = PluginCommand.execute(context("", &cwd)).await.unwrap();
        assert!(result.value.contains("alpha@1.2.3 [valid enabled]"));
        assert!(result.value.contains("commands=1"));
        assert!(result.value.contains("broken [invalid enabled]"));

        let json_result = PluginCommand
            .execute(context("json alpha", &cwd))
            .await
            .unwrap();
        let value: Value = serde_json::from_str(&json_result.value).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 1);
        assert_eq!(value[0]["id"], "alpha");
        assert_eq!(value[0]["valid"], true);

        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_enable_disable_updates_state_and_listing() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("state");
        let cwd = root.join("project");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("gamma");
        write_manifest(&plugin_root, "gamma");
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let disabled = PluginCommand
            .execute(context("disable gamma", &cwd))
            .await
            .unwrap();
        assert!(disabled.value.contains("Plugin disabled: gamma"));
        assert!(disabled
            .value
            .contains("LSP runtime: restarted on next use"));
        let list = PluginCommand
            .execute(context("list gamma", &cwd))
            .await
            .unwrap();
        assert!(list.value.contains("gamma@1.2.3 [valid disabled]"));
        let json_result = PluginCommand
            .execute(context("json gamma", &cwd))
            .await
            .unwrap();
        let value: Value = serde_json::from_str(&json_result.value).unwrap();
        assert_eq!(value[0]["enabled"], false);

        let enabled = PluginCommand
            .execute(context("enable gamma", &cwd))
            .await
            .unwrap();
        assert!(enabled.value.contains("Plugin enabled: gamma"));
        assert!(enabled.value.contains("LSP runtime: restarted on next use"));
        let list = PluginCommand
            .execute(context("list gamma", &cwd))
            .await
            .unwrap();
        assert!(list.value.contains("gamma@1.2.3 [valid enabled]"));

        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_show_path_and_validate_resolve_by_name_or_path() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("show");
        let cwd = root.join("project");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("beta");
        write_manifest(&plugin_root, "beta");
        fs::create_dir_all(plugin_root.join("skills").join("audit")).unwrap();
        fs::write(
            plugin_root.join("skills").join("audit").join("SKILL.md"),
            "# Audit",
        )
        .unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let shown = PluginCommand
            .execute(context("show beta", &cwd))
            .await
            .unwrap();
        let value: Value = serde_json::from_str(&shown.value).unwrap();
        assert_eq!(value["manifest"]["name"], "beta");
        assert_eq!(value["components"]["skills"], 1);

        let path = PluginCommand
            .execute(context("path beta", &cwd))
            .await
            .unwrap();
        assert_eq!(path.value, plugin_root.display().to_string());

        let validation = PluginCommand
            .execute(context(
                &format!("validate {}", plugin_root.display()),
                &cwd,
            ))
            .await
            .unwrap();
        assert!(validation.value.contains("Validation passed"));

        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_marketplace_add_list_and_remove_manage_local_config() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let root = temp_root("marketplace");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let marketplace_root = root.join("marketplaces").join("tools");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&marketplace_root).unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);

        let empty = PluginCommand
            .execute(context("marketplace list --json", &cwd))
            .await
            .unwrap();
        let empty_json: Value = serde_json::from_str(&empty.value).unwrap();
        assert_eq!(empty_json.as_array().unwrap().len(), 0);

        let added = PluginCommand
            .execute(context(
                &format!("marketplace add {}", marketplace_root.display()),
                &cwd,
            ))
            .await
            .unwrap();
        assert!(added
            .value
            .contains("Successfully added marketplace: tools"));
        assert!(added.value.contains("scope: user"));

        let listed = PluginCommand
            .execute(context("marketplace list --json", &cwd))
            .await
            .unwrap();
        let listed_json: Value = serde_json::from_str(&listed.value).unwrap();
        assert_eq!(listed_json[0]["name"], "tools");
        assert_eq!(listed_json[0]["source"]["source"], "directory");
        assert_eq!(
            listed_json[0]["source"]["path"],
            marketplace_root.display().to_string()
        );

        let removed = PluginCommand
            .execute(context("marketplace remove tools", &cwd))
            .await
            .unwrap();
        assert!(removed
            .value
            .contains("Successfully removed marketplace: tools"));

        let empty_after_remove = PluginCommand
            .execute(context("marketplace list --json", &cwd))
            .await
            .unwrap();
        let empty_after_remove_json: Value =
            serde_json::from_str(&empty_after_remove.value).unwrap();
        assert_eq!(empty_after_remove_json.as_array().unwrap().len(), 0);

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_marketplace_list_ignores_project_config_when_project_is_untrusted() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let root = temp_root("marketplace-trust");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let user_marketplace = root.join("marketplaces").join("user-tools");
        let project_marketplace = root.join("marketplaces").join("project-tools");
        let local_marketplace = root.join("marketplaces").join("local-tools");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&user_marketplace).unwrap();
        fs::create_dir_all(&project_marketplace).unwrap();
        fs::create_dir_all(&local_marketplace).unwrap();
        fs::create_dir_all(cwd.join(".kiana")).unwrap();
        fs::create_dir_all(&kiana_home).unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);
        fs::write(
            kiana_home.join("plugin-marketplaces.json"),
            serde_json::to_string_pretty(&json!({
                "marketplaces": {
                    "user-tools": {
                        "scope": "user",
                        "source": {
                            "source": "directory",
                            "path": user_marketplace.display().to_string()
                        }
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            cwd.join(".kiana").join("plugin-marketplaces.json"),
            serde_json::to_string_pretty(&json!({
                "marketplaces": {
                    "project-tools": {
                        "scope": "project",
                        "source": {
                            "source": "directory",
                            "path": project_marketplace.display().to_string()
                        }
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            cwd.join(".kiana").join("plugin-marketplaces.local.json"),
            serde_json::to_string_pretty(&json!({
                "marketplaces": {
                    "local-tools": {
                        "scope": "local",
                        "source": {
                            "source": "directory",
                            "path": local_marketplace.display().to_string()
                        }
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let result = PluginCommand
            .execute(context_with_state(
                "marketplace list --json",
                &cwd,
                HashMap::from([("project_trusted".to_string(), json!(false))]),
            ))
            .await
            .unwrap();
        let value: Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value.as_array().unwrap().len(), 1);
        assert_eq!(value[0]["name"], "user-tools");
        assert_eq!(value[0]["scope"], "user");

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_marketplace_list_surfaces_plugin_interface_and_policy() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let root = temp_root("marketplace-policy-list");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let marketplace_root = root.join("marketplaces").join("policy-marketplace");
        let marketplace_manifest_dir = marketplace_root.join(".codex-plugin");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&marketplace_manifest_dir).unwrap();
        fs::write(
            marketplace_manifest_dir.join("marketplace.json"),
            serde_json::to_string_pretty(&json!({
                "name": "policy-marketplace",
                "plugins": [
                    {
                        "name": "review-tools",
                        "source": "./review-tools",
                        "interface": {
                            "kind": "agent-pack",
                            "shareContext": true
                        },
                        "policy": {
                            "availability": "available",
                            "installPolicy": "on_install",
                            "authPolicy": "on_use"
                        }
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);

        PluginCommand
            .execute(context(
                &format!("marketplace add {}", marketplace_root.display()),
                &cwd,
            ))
            .await
            .unwrap();

        let listed = PluginCommand
            .execute(context("marketplace list --json", &cwd))
            .await
            .unwrap();
        let value: Value = serde_json::from_str(&listed.value).unwrap();
        assert_eq!(value[0]["plugins"][0]["name"], "review-tools");
        assert_eq!(value[0]["plugins"][0]["interface"]["kind"], "agent-pack");
        assert_eq!(
            value[0]["plugins"][0]["policy"]["availability"],
            "available"
        );
        assert_eq!(
            value[0]["plugins"][0]["policy"]["installPolicy"],
            "on_install"
        );
        assert_eq!(value[0]["plugins"][0]["policy"]["authPolicy"], "on_use");

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_install_and_uninstall_from_local_marketplace_directory() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let root = temp_root("install");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let plugins_dir = kiana_home.join("plugins");
        let marketplace_root = root.join("marketplaces").join("tools-marketplace");
        let source_plugin = marketplace_root.join("review-tools");
        fs::create_dir_all(&cwd).unwrap();
        write_manifest(&source_plugin, "review-tools");
        fs::create_dir_all(source_plugin.join("commands")).unwrap();
        fs::write(source_plugin.join("commands").join("audit.md"), "# audit").unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        PluginCommand
            .execute(context(
                &format!("marketplace add {}", marketplace_root.display()),
                &cwd,
            ))
            .await
            .unwrap();

        let installed = PluginCommand
            .execute(context("install review-tools@tools-marketplace", &cwd))
            .await
            .unwrap();
        assert!(installed.value.contains("Installed plugin: review-tools"));
        assert!(plugins_dir
            .join("review-tools")
            .join("commands")
            .join("audit.md")
            .is_file());

        let list = PluginCommand
            .execute(context("list review-tools", &cwd))
            .await
            .unwrap();
        assert!(list.value.contains("review-tools@1.2.3 [valid enabled]"));
        assert!(list.value.contains("commands=1"));

        PluginCommand
            .execute(context("disable review-tools", &cwd))
            .await
            .unwrap();
        let uninstalled = PluginCommand
            .execute(context("uninstall review-tools", &cwd))
            .await
            .unwrap();
        assert!(uninstalled
            .value
            .contains("Uninstalled plugin: review-tools"));
        assert!(!plugins_dir.join("review-tools").exists());

        let list_after = PluginCommand.execute(context("list", &cwd)).await.unwrap();
        assert!(list_after.value.contains("No plugins."));

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_install_rejects_not_available_marketplace_policy() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let root = temp_root("marketplace-policy-install");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let plugins_dir = kiana_home.join("plugins");
        let marketplace_root = root.join("marketplaces").join("policy-marketplace");
        let marketplace_manifest_dir = marketplace_root.join(".codex-plugin");
        let source_plugin = marketplace_root.join("review-tools");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&marketplace_manifest_dir).unwrap();
        write_manifest(&source_plugin, "review-tools");
        fs::write(
            marketplace_manifest_dir.join("marketplace.json"),
            serde_json::to_string_pretty(&json!({
                "name": "policy-marketplace",
                "plugins": [
                    {
                        "name": "review-tools",
                        "source": "./review-tools",
                        "policy": {
                            "availability": "not_available",
                            "installPolicy": "blocked"
                        }
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        PluginCommand
            .execute(context(
                &format!("marketplace add {}", marketplace_root.display()),
                &cwd,
            ))
            .await
            .unwrap();

        let error = PluginCommand
            .execute(context("install review-tools@policy-marketplace", &cwd))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("plugin 'review-tools' is marked not available by marketplace policy")
        );
        assert!(!plugins_dir.join("review-tools").exists());

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_install_resolves_marketplace_json_relative_plugin_sources() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let root = temp_root("marketplace-json-install");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let plugins_dir = kiana_home.join("plugins");
        let marketplace_root = root.join("marketplaces").join("ops-marketplace");
        let source_plugin = marketplace_root.join("packages").join("ops-tools");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(marketplace_root.join(".codex-plugin")).unwrap();
        write_manifest(&source_plugin, "ops-tools");
        fs::create_dir_all(source_plugin.join("skills").join("runbook")).unwrap();
        fs::write(
            source_plugin
                .join("skills")
                .join("runbook")
                .join("SKILL.md"),
            "# Runbook",
        )
        .unwrap();
        fs::write(
            marketplace_root
                .join(".codex-plugin")
                .join("marketplace.json"),
            serde_json::to_string_pretty(&json!({
                "name": "ops-marketplace",
                "plugins": [
                    {
                        "name": "ops-tools",
                        "source": "./packages/ops-tools"
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        PluginCommand
            .execute(context(
                &format!("marketplace add {}", marketplace_root.display()),
                &cwd,
            ))
            .await
            .unwrap();

        let installed = PluginCommand
            .execute(context("install ops-tools", &cwd))
            .await
            .unwrap();
        assert!(installed.value.contains("Installed plugin: ops-tools"));
        assert!(plugins_dir
            .join("ops-tools")
            .join("skills")
            .join("runbook")
            .join("SKILL.md")
            .is_file());

        let list = PluginCommand
            .execute(context("list ops-tools", &cwd))
            .await
            .unwrap();
        assert!(list.value.contains("skills=1"));

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_install_reports_unsupported_remote_plugin_source_types() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let root = temp_root("unsupported-remote-plugin-source");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let plugins_dir = kiana_home.join("plugins");
        fs::create_dir_all(&cwd).unwrap();
        let marketplace_url = serve_json_once(
            serde_json::to_string_pretty(&json!({
                "name": "package-marketplace",
                "owner": { "name": "Kiana Tests" },
                "plugins": [
                    {
                        "name": "packaged-tools",
                        "source": {
                            "source": "npm",
                            "package": "@example/packaged-tools"
                        }
                    }
                ]
            }))
            .unwrap(),
        )
        .await;
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        PluginCommand
            .execute(context(&format!("marketplace add {marketplace_url}"), &cwd))
            .await
            .unwrap();

        let error = PluginCommand
            .execute(context("install packaged-tools@package-marketplace", &cwd))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(
            "remote marketplace 'package-marketplace' plugin 'packaged-tools' source type 'npm' is not implemented yet"
        ));

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_install_from_remote_http_marketplace_git_url_source() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let root = temp_root("remote-http-marketplace-install");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let plugins_dir = kiana_home.join("plugins");
        let plugin_repo = root.join("git").join("review-tools");
        fs::create_dir_all(&cwd).unwrap();
        write_manifest(&plugin_repo, "review-tools");
        fs::create_dir_all(plugin_repo.join("commands")).unwrap();
        fs::write(plugin_repo.join("commands").join("audit.md"), "# audit").unwrap();
        run_git(&plugin_repo, &["init"]);
        run_git(&plugin_repo, &["add", "."]);
        run_git(
            &plugin_repo,
            &[
                "-c",
                "user.name=Kiana Test",
                "-c",
                "user.email=kiana@example.invalid",
                "commit",
                "-m",
                "initial plugin",
            ],
        );
        let plugin_repo_url = format!("file://{}", plugin_repo.canonicalize().unwrap().display());
        let marketplace_url = serve_json_once(
            serde_json::to_string_pretty(&json!({
                "name": "remote-marketplace",
                "owner": { "name": "Kiana Tests" },
                "plugins": [
                    {
                        "name": "review-tools",
                        "source": {
                            "source": "url",
                            "url": plugin_repo_url
                        }
                    }
                ]
            }))
            .unwrap(),
        )
        .await;
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let added = PluginCommand
            .execute(context(&format!("marketplace add {marketplace_url}"), &cwd))
            .await
            .unwrap();
        assert!(added
            .value
            .contains("Successfully added marketplace: remote-marketplace"));

        let installed = PluginCommand
            .execute(context("install review-tools@remote-marketplace", &cwd))
            .await
            .unwrap();
        assert!(installed.value.contains("Installed plugin: review-tools"));
        assert!(installed.value.contains("marketplace: remote-marketplace"));
        assert!(plugins_dir
            .join("review-tools")
            .join("commands")
            .join("audit.md")
            .is_file());

        let list = PluginCommand
            .execute(context("list review-tools", &cwd))
            .await
            .unwrap();
        assert!(list.value.contains("review-tools@1.2.3 [valid enabled]"));
        assert!(list.value.contains("commands=1"));

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_install_from_git_marketplace_relative_source() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let root = temp_root("git-marketplace-install");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let plugins_dir = kiana_home.join("plugins");
        let marketplace_repo = root.join("git").join("ops-marketplace");
        let marketplace_manifest_dir = marketplace_repo.join(".codex-plugin");
        let source_plugin = marketplace_repo.join("packages").join("ops-tools");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&marketplace_manifest_dir).unwrap();
        write_manifest(&source_plugin, "ops-tools");
        fs::create_dir_all(source_plugin.join("skills").join("runbook")).unwrap();
        fs::write(
            source_plugin
                .join("skills")
                .join("runbook")
                .join("SKILL.md"),
            "# Runbook",
        )
        .unwrap();
        fs::write(
            marketplace_manifest_dir.join("marketplace.json"),
            serde_json::to_string_pretty(&json!({
                "name": "ops-marketplace",
                "owner": { "name": "Kiana Tests" },
                "plugins": [
                    {
                        "name": "ops-tools",
                        "source": "./packages/ops-tools"
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        run_git(&marketplace_repo, &["init"]);
        run_git(&marketplace_repo, &["add", "."]);
        run_git(
            &marketplace_repo,
            &[
                "-c",
                "user.name=Kiana Test",
                "-c",
                "user.email=kiana@example.invalid",
                "commit",
                "-m",
                "initial marketplace",
            ],
        );
        let marketplace_url = format!(
            "file://{}",
            marketplace_repo.canonicalize().unwrap().display()
        );
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let added = PluginCommand
            .execute(context(&format!("marketplace add {marketplace_url}"), &cwd))
            .await
            .unwrap();
        assert!(added
            .value
            .contains("Successfully added marketplace: ops-marketplace"));

        let installed = PluginCommand
            .execute(context("install ops-tools@ops-marketplace", &cwd))
            .await
            .unwrap();
        assert!(installed.value.contains("Installed plugin: ops-tools"));
        assert!(installed.value.contains("marketplace: ops-marketplace"));
        assert!(plugins_dir
            .join("ops-tools")
            .join("skills")
            .join("runbook")
            .join("SKILL.md")
            .is_file());

        let list = PluginCommand
            .execute(context("list ops-tools", &cwd))
            .await
            .unwrap();
        assert!(list.value.contains("ops-tools@1.2.3 [valid enabled]"));
        assert!(list.value.contains("skills=1"));

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_target_commands_require_exact_name_or_path() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("exact");
        let cwd = root.join("project");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("review-tools");
        write_manifest(&plugin_root, "review-tools");
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let error = PluginCommand
            .execute(context("show review-tool", &cwd))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("plugin 'review-tool' was not found"));

        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }
}
