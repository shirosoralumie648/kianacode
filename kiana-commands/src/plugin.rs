use crate::local_state::kiana_home_dir;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use kiana_types::hooks::parse_hook_config;
use kiana_types::plugin::{
    all_plugin_roots_in, disabled_plugin_names, find_manifest_path, plugin_is_disabled,
    set_plugin_enabled,
};
use kiana_types::project_trust_from_app_state;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

const PLUGIN_INSTALL_RECEIPT_SCHEMA: &str = "kiana.plugin-install-receipt.v1";
const PLUGIN_INSTALL_RECEIPT_FILE: &str = ".kiana-install-receipt.json";
const KIANA_MANAGED_PLUGIN_POLICY_FILE_ENV: &str = "KIANA_MANAGED_PLUGIN_POLICY_FILE";

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
    let args = parse_plugin_read_args(query, "list")?;
    let root = scoped_plugin_root_dir(context, args.scope);
    let query = args.target.as_deref().unwrap_or("");
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
    let args = parse_plugin_read_args(query, "json")?;
    let root = scoped_plugin_root_dir(context, args.scope);
    let query = args.target.as_deref().unwrap_or("");
    let plugins = filtered_plugins(load_installed_plugins(&root)?, query);
    Ok(CommandResult::text(serde_json::to_string_pretty(&plugins)?))
}

pub fn installed_plugin_summaries(context: &CommandContext) -> Result<Value> {
    let plugins = installed_plugin_summaries_by_scope(context)?;
    Ok(Value::Array(plugins))
}

fn installed_plugin_summaries_by_scope(context: &CommandContext) -> Result<Vec<Value>> {
    let mut plugins = Vec::new();
    for scope in [
        MarketplaceScope::User,
        MarketplaceScope::Project,
        MarketplaceScope::Local,
    ] {
        let root = scoped_plugin_root_dir(context, scope);
        plugins.extend(
            load_installed_plugins(&root)?
                .iter()
                .map(|plugin| plugin_init_summary(plugin, scope)),
        );
    }
    Ok(plugins)
}

fn show_plugin(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let args = parse_plugin_read_args(rest, "show")?;
    let Some(target) = args.target.as_deref() else {
        return Err(anyhow!("usage: kiana plugin show <name>"));
    };
    let root = scoped_plugin_root_dir(context, args.scope);
    let plugin = resolve_installed_plugin_in_root(context, target, &root)?;
    Ok(CommandResult::text(serde_json::to_string_pretty(&plugin)?))
}

fn plugin_path(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let args = parse_plugin_read_args(rest, "path")?;
    let root = scoped_plugin_root_dir(context, args.scope);
    let Some(target) = args.target.as_deref() else {
        return Ok(CommandResult::text(root.display().to_string()));
    };
    let plugin = resolve_installed_plugin_in_root(context, target, &root)?;
    Ok(CommandResult::text(plugin.root.display().to_string()))
}

async fn install_plugin(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    if rest.trim().is_empty() || is_help_arg(rest.trim()) {
        return Ok(CommandResult::text(plugin_install_usage()));
    }
    let args = parse_plugin_action_args(rest, "install")?;
    let install_scope = args.scope.unwrap_or(MarketplaceScope::User);
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
    let managed_policy =
        enforce_managed_plugin_policy(source_info.display_name(), source.marketplace.as_deref())?;

    let install_name = safe_plugin_dir_name(source_info.display_name())?;
    let install_root = scoped_plugin_root_dir(context, install_scope);
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
    let receipt_path =
        write_plugin_install_receipt(&source_info, &source, &destination, managed_policy.as_ref())?;
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
    if let Some(decision) = managed_policy {
        lines.push(format!("managed_policy: {}", decision.summary()));
    }
    lines.push(format!("receipt: {}", receipt_path.display()));
    lines.push(format!("state: {}", state_path.display()));
    lines.push("LSP runtime: restarted on next use".into());
    Ok(CommandResult::text(lines.join("\n")))
}

async fn uninstall_plugin(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    if rest.trim().is_empty() || is_help_arg(rest.trim()) {
        return Ok(CommandResult::text(plugin_uninstall_usage()));
    }
    let args = parse_plugin_action_args(rest, "uninstall")?;
    let install_scope = args.scope.unwrap_or(MarketplaceScope::User);
    let install_root = scoped_plugin_root_dir(context, install_scope);
    let plugin = resolve_installed_plugin_in_root(context, &args.target, &install_root)?;
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
    let args = parse_plugin_read_args(rest, if enabled { "enable" } else { "disable" })?;
    let Some(target) = args.target.as_deref() else {
        return Err(anyhow!(
            "usage: kiana plugin {} <name|path> [--scope user|project|local]",
            if enabled { "enable" } else { "disable" }
        ));
    };
    let root = scoped_plugin_root_dir(context, args.scope);
    let plugin = resolve_installed_plugin_in_root(context, target, &root)?;
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
    let args = parse_plugin_read_args(rest, "validate")?;
    let root = scoped_plugin_root_dir(context, args.scope);
    if let Some(target) = args.target.as_deref() {
        return Ok(CommandResult::text(format_validation(
            &resolve_installed_plugin_in_root(context, target, &root)?,
        )));
    }

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
            signature: None,
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
                if let Some(root) =
                    find_plugin_in_marketplace_dir(Path::new(path), &plugin_name).await?
                {
                    return Ok(InstallSource {
                        root: root.root,
                        marketplace: Some(entry.name),
                        policy: root.policy,
                        signature: root.signature,
                    });
                }
            }
            MarketplaceSource::File { path } => {
                if let Some(root) =
                    find_plugin_in_marketplace_file(Path::new(path), &plugin_name).await?
                {
                    return Ok(InstallSource {
                        root: root.root,
                        marketplace: Some(entry.name),
                        policy: root.policy,
                        signature: root.signature,
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
                        signature: root.signature,
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
                marketplace_entry_remote_source_path(
                    &marketplace_name,
                    &entry.name,
                    &entry.source,
                    manifest_path.parent(),
                )
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
            signature: entry.signature,
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
    package_base: Option<&Path>,
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
        "npm" => {
            let package = object
                .get("package")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!(
                        "remote marketplace '{}' plugin '{}' npm source object requires 'package'",
                        marketplace_name,
                        entry_name
                    )
                })?;
            resolve_remote_file_package_source(
                marketplace_name,
                entry_name,
                "npm",
                package,
                package_base,
            )
        }
        "pip" => {
            let package = object
                .get("package")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!(
                        "remote marketplace '{}' plugin '{}' pip source object requires 'package'",
                        marketplace_name,
                        entry_name
                    )
                })?;
            resolve_remote_file_package_source(
                marketplace_name,
                entry_name,
                "pip",
                package,
                package_base,
            )
        }
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

fn resolve_remote_file_package_source(
    marketplace_name: &str,
    entry_name: &str,
    source_type: &str,
    package: &str,
    package_base: Option<&Path>,
) -> Result<PathBuf> {
    let path = package.strip_prefix("file:").ok_or_else(|| {
        anyhow!(
            "remote marketplace '{}' plugin '{}' {} package '{}' is not supported; use file:<relative-path> for offline package sources",
            marketplace_name,
            entry_name,
            source_type,
            package
        )
    })?;
    let path = Path::new(path);
    if path.is_absolute()
        || path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(anyhow!(
            "remote marketplace '{}' plugin '{}' {} file package must be a relative path inside the cached marketplace",
            marketplace_name,
            entry_name,
            source_type
        ));
    }
    let base = package_base.ok_or_else(|| {
        anyhow!(
            "remote marketplace '{}' plugin '{}' {} file package has no cached marketplace base",
            marketplace_name,
            entry_name,
            source_type
        )
    })?;
    Ok(base.join(path))
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

async fn find_plugin_in_marketplace_dir(
    marketplace_root: &Path,
    plugin_name: &str,
) -> Result<Option<MarketplacePluginResolution>> {
    if let Some(path) = find_plugin_from_marketplace_manifest(marketplace_root, plugin_name).await?
    {
        return Ok(Some(path));
    }
    if plugin_dir_matches(marketplace_root, plugin_name)? {
        return Ok(Some(MarketplacePluginResolution {
            root: marketplace_root.to_path_buf(),
            policy: MarketplacePluginPolicy::default(),
            signature: None,
        }));
    }
    for plugin_root in all_plugin_roots_in(marketplace_root) {
        if plugin_dir_matches(&plugin_root, plugin_name)? {
            return Ok(Some(MarketplacePluginResolution {
                root: plugin_root,
                policy: MarketplacePluginPolicy::default(),
                signature: None,
            }));
        }
    }
    Ok(None)
}

async fn find_plugin_in_marketplace_file(
    marketplace_file: &Path,
    plugin_name: &str,
) -> Result<Option<MarketplacePluginResolution>> {
    find_plugin_from_marketplace_manifest_file(marketplace_file, plugin_name).await
}

async fn find_plugin_from_marketplace_manifest(
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
                find_plugin_from_marketplace_manifest_file(&path, plugin_name).await?
            {
                return Ok(Some(plugin_root));
            }
        }
    }
    Ok(None)
}

async fn find_plugin_from_marketplace_manifest_file(
    manifest_path: &Path,
    plugin_name: &str,
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
    let base = marketplace_manifest_base_dir(manifest_path)?;
    for entry in manifest.plugins {
        if normalize_target(&entry.name) != normalize_target(plugin_name) {
            continue;
        }
        let plugin_root = if marketplace_entry_source_is_local(&entry.source) {
            marketplace_entry_source_path(&base, &entry.source)?
        } else {
            marketplace_entry_remote_source_path(
                &marketplace_name,
                &entry.name,
                &entry.source,
                manifest_path.parent(),
            )
            .await?
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
            signature: entry.signature,
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

fn resolve_installed_plugin_in_root(
    context: &CommandContext,
    target: &str,
    root: &Path,
) -> Result<PluginInfo> {
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

fn parse_plugin_read_args(rest: &str, command: &str) -> Result<PluginReadArgs> {
    let tokens = split_words(rest);
    let mut target = None;
    let mut scope = MarketplaceScope::User;
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index].as_str();
        if token == "--scope" || token == "-s" {
            index += 1;
            let value = tokens.get(index).ok_or_else(|| {
                anyhow!("usage: kiana plugin {command} [name|path] --scope <user|project|local>")
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
        if token.starts_with('-') {
            return Err(anyhow!(
                "unknown plugin {command} option '{}'\n\n{}",
                token,
                usage()
            ));
        }
        if target.replace(token.to_string()).is_some() {
            return Err(anyhow!("usage: kiana plugin {command} [name|path]"));
        }
        index += 1;
    }
    Ok(PluginReadArgs { target, scope })
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

fn write_plugin_install_receipt(
    source_info: &PluginInfo,
    source: &InstallSource,
    destination: &Path,
    managed_policy: Option<&ManagedPluginPolicyDecision>,
) -> Result<PathBuf> {
    let files = collect_plugin_receipt_files(destination)?;
    let receipt = PluginInstallReceipt {
        schema: PLUGIN_INSTALL_RECEIPT_SCHEMA,
        name: source_info.display_name().to_string(),
        version: source_info.version.clone(),
        source_type: if source.marketplace.is_some() {
            "marketplace"
        } else {
            "path"
        },
        source_path: source_info.root.display().to_string(),
        install_path: destination.display().to_string(),
        marketplace: source.marketplace.clone(),
        policy: source.policy.clone(),
        signature: source.signature.clone(),
        managed_policy: managed_policy.map(ManagedPluginPolicyReceipt::from),
        file_count: files.len(),
        content_hash: aggregate_receipt_hash(&files),
        files,
    };
    let receipt_path = destination.join(PLUGIN_INSTALL_RECEIPT_FILE);
    let mut receipt_value = serde_json::to_value(&receipt)?;
    let payload_hash = plugin_receipt_payload_hash(&receipt_value);
    if let Some(object) = receipt_value.as_object_mut() {
        object.insert(
            "integrity".to_string(),
            json!({
                "method": "stable-hash-v1",
                "payload_hash": payload_hash,
            }),
        );
    }
    let contents = serde_json::to_string_pretty(&receipt_value)?;
    std::fs::write(&receipt_path, format!("{contents}\n"))?;
    Ok(receipt_path)
}

fn collect_plugin_receipt_files(root: &Path) -> Result<Vec<PluginReceiptFile>> {
    let mut files = Vec::new();
    collect_plugin_receipt_files_recursive(root, root, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn collect_plugin_receipt_files_recursive(
    root: &Path,
    current: &Path,
    files: &mut Vec<PluginReceiptFile>,
) -> Result<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = std::fs::metadata(&path)?;
        if metadata.is_dir() {
            collect_plugin_receipt_files_recursive(root, &path, files)?;
        } else if metadata.is_file() {
            let relative = plugin_receipt_relative_path(root, &path)?;
            if relative == PLUGIN_INSTALL_RECEIPT_FILE {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            files.push(PluginReceiptFile {
                path: relative,
                bytes: metadata.len(),
                content_hash: stable_hash(&bytes),
            });
        }
    }
    Ok(())
}

fn plugin_receipt_relative_path(root: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(root)?
        .to_string_lossy()
        .replace('\\', "/"))
}

fn aggregate_receipt_hash(files: &[PluginReceiptFile]) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for file in files {
        update_stable_hash(&mut hash, file.path.as_bytes());
        update_stable_hash(&mut hash, &[0]);
        update_stable_hash(&mut hash, file.bytes.to_string().as_bytes());
        update_stable_hash(&mut hash, &[0]);
        update_stable_hash(&mut hash, file.content_hash.as_bytes());
        update_stable_hash(&mut hash, &[0]);
    }
    format!("{hash:016x}")
}

fn plugin_receipt_payload_hash(receipt: &Value) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    update_stable_json_hash(&mut hash, receipt, true);
    format!("{hash:016x}")
}

fn update_stable_json_hash(hash: &mut u64, value: &Value, skip_integrity: bool) {
    match value {
        Value::Null => update_stable_hash(hash, b"n"),
        Value::Bool(value) => {
            update_stable_hash(hash, b"b");
            update_stable_hash(hash, value.to_string().as_bytes());
        }
        Value::Number(value) => {
            update_stable_hash(hash, b"#");
            update_stable_hash(hash, value.to_string().as_bytes());
        }
        Value::String(value) => {
            update_stable_hash(hash, b"s");
            update_stable_hash(hash, value.as_bytes());
        }
        Value::Array(values) => {
            update_stable_hash(hash, b"[");
            for value in values {
                update_stable_json_hash(hash, value, skip_integrity);
                update_stable_hash(hash, &[0]);
            }
            update_stable_hash(hash, b"]");
        }
        Value::Object(object) => {
            update_stable_hash(hash, b"{");
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            for key in keys {
                if skip_integrity && key == "integrity" {
                    continue;
                }
                update_stable_hash(hash, key.as_bytes());
                update_stable_hash(hash, &[0]);
                if let Some(value) = object.get(key) {
                    update_stable_json_hash(hash, value, skip_integrity);
                }
                update_stable_hash(hash, &[0]);
            }
            update_stable_hash(hash, b"}");
        }
    }
}

fn stable_hash(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    update_stable_hash(&mut hash, bytes);
    format!("{hash:016x}")
}

fn update_stable_hash(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

fn enforce_managed_plugin_policy(
    plugin_name: &str,
    marketplace: Option<&str>,
) -> Result<Option<ManagedPluginPolicyDecision>> {
    let Some(path) = managed_plugin_policy_path() else {
        return Ok(None);
    };
    let contents = std::fs::read_to_string(&path).map_err(|error| {
        anyhow!(
            "failed to read managed plugin policy {}: {}",
            path.display(),
            error
        )
    })?;
    let value = serde_json::from_str::<Value>(&contents).map_err(|error| {
        anyhow!(
            "failed to parse managed plugin policy {}: {}",
            path.display(),
            error
        )
    })?;
    let Some(policy_value) = value
        .get("plugins")
        .or_else(|| value.get("pluginInstallPolicy"))
        .or_else(|| value.get("plugin_install_policy"))
        .or_else(|| value.get("plugin-install-policy"))
    else {
        return Ok(None);
    };
    let policy =
        serde_json::from_value::<ManagedPluginPolicy>(policy_value.clone()).map_err(|error| {
            anyhow!(
                "failed to parse managed plugin install policy {}: {}",
                path.display(),
                error
            )
        })?;
    if policy.is_empty() {
        return Ok(None);
    }
    policy.evaluate(plugin_name, marketplace, path)
}

fn managed_plugin_policy_path() -> Option<PathBuf> {
    std::env::var_os(KIANA_MANAGED_PLUGIN_POLICY_FILE_ENV)
        .or_else(|| std::env::var_os("KIANA_MANAGED_POLICY_FILE"))
        .map(PathBuf::from)
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
    let mut app_manifest = None;
    let mut manifest_name = None;
    let mut version = None;
    let mut description = None;
    let (install_receipt, install_receipt_integrity) =
        read_plugin_install_receipt(&root, &mut warnings);
    if let Some(integrity) = install_receipt_integrity.as_ref() {
        if integrity.status != "verified" {
            errors.push(format!(
                "plugin install receipt integrity {}: {}",
                integrity.status,
                integrity.summary()
            ));
        }
    }

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
    validate_plugin_component_json(&root, &mut errors);
    if components.apps > 0 {
        app_manifest = read_plugin_app_manifest(&root, &mut errors);
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
        app_manifest,
        install_receipt,
        install_receipt_integrity,
    })
}

fn read_plugin_app_manifest(root: &Path, errors: &mut Vec<String>) -> Option<Value> {
    let path = root.join("app.json");
    if !path.is_file() {
        return None;
    }
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) => {
            errors.push(format!("failed to read app.json: {error}"));
            return None;
        }
    };
    match serde_json::from_str::<Value>(&contents) {
        Ok(value) if value.is_object() => Some(value),
        Ok(_) => None,
        Err(_) => None,
    }
}

fn validate_plugin_component_json(root: &Path, errors: &mut Vec<String>) {
    validate_mcp_component_file(root, errors);
    validate_lsp_component_file(root, errors);
    validate_hooks_component_file(root, errors);
    validate_app_component_file(root, errors);
}

fn validate_component_json_file(
    path: &Path,
    label: &str,
    errors: &mut Vec<String>,
) -> Option<Value> {
    if !path.is_file() {
        return None;
    }
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) => {
            errors.push(format!("failed to read {label}: {error}"));
            return None;
        }
    };
    match serde_json::from_str::<Value>(&contents) {
        Ok(value) => Some(value),
        Err(error) => {
            errors.push(format!("{label} is not valid JSON: {error}"));
            None
        }
    }
}

fn validate_mcp_component_file(root: &Path, errors: &mut Vec<String>) {
    let Some(value) = validate_component_json_file(&root.join(".mcp.json"), ".mcp.json", errors)
    else {
        return;
    };
    let servers = value
        .get("mcpServers")
        .or_else(|| value.get("mcp_servers"))
        .unwrap_or(&value);
    match servers {
        Value::Object(object) => {
            for (name, config) in object {
                validate_plugin_mcp_server(name, config, ".mcp.json", errors);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let Some(name) = item
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                else {
                    errors.push(format!(
                        ".mcp.json[{index}] missing non-empty string field 'name'"
                    ));
                    continue;
                };
                validate_plugin_mcp_server(name, item, ".mcp.json", errors);
            }
        }
        _ => errors.push(
            ".mcp.json must be an object, an array, or contain object/array mcpServers".into(),
        ),
    }
}

fn validate_plugin_mcp_server(name: &str, config: &Value, label: &str, errors: &mut Vec<String>) {
    let name = name.trim();
    if name.is_empty() {
        errors.push(format!("{label} contains an empty MCP server name"));
        return;
    }
    if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        errors.push(format!(
            "{label} MCP server '{name}' contains unsafe path characters"
        ));
    }
    let Some(object) = config.as_object() else {
        errors.push(format!(
            "{label} MCP server '{name}' config must be a JSON object"
        ));
        return;
    };
    let has_command = object
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    let has_url = object
        .get("url")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    let has_transport = object
        .get("transport")
        .or_else(|| object.get("type"))
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    if !has_command && !has_url && !has_transport {
        errors.push(format!(
            "{label} MCP server '{name}' requires command, url, type, or transport"
        ));
    }
}

fn validate_lsp_component_file(root: &Path, errors: &mut Vec<String>) {
    let Some(value) = validate_component_json_file(&root.join(".lsp.json"), ".lsp.json", errors)
    else {
        return;
    };
    let servers = value.get("lspServers").unwrap_or(&value);
    let Some(object) = servers.as_object() else {
        errors.push(".lsp.json must be an object or contain object lspServers".into());
        return;
    };
    for (name, config) in object {
        let name = name.trim();
        if name.is_empty() {
            errors.push(".lsp.json contains an empty LSP server name".into());
            continue;
        }
        let Some(config) = config.as_object() else {
            errors.push(format!(
                ".lsp.json LSP server '{name}' config must be a JSON object"
            ));
            continue;
        };
        if !config
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| !command.trim().is_empty())
        {
            errors.push(format!(
                ".lsp.json LSP server '{name}' requires non-empty string field 'command'"
            ));
        }
    }
}

fn validate_hooks_component_file(root: &Path, errors: &mut Vec<String>) {
    let label = "hooks/hooks.json";
    let Some(value) =
        validate_component_json_file(&root.join("hooks").join("hooks.json"), label, errors)
    else {
        return;
    };
    if let Err(error) = parse_hook_config(value) {
        errors.push(format!(
            "{label} invalid hook schema at {}: {}",
            error.location, error.message
        ));
    }
}

fn validate_app_component_file(root: &Path, errors: &mut Vec<String>) {
    let Some(value) = validate_component_json_file(&root.join("app.json"), "app.json", errors)
    else {
        return;
    };
    if !value.is_object() {
        errors.push("app.json must contain a JSON object".into());
    }
}

fn read_plugin_install_receipt(
    root: &Path,
    warnings: &mut Vec<String>,
) -> (Option<Value>, Option<PluginInstallReceiptIntegrityStatus>) {
    let receipt_path = root.join(PLUGIN_INSTALL_RECEIPT_FILE);
    if !receipt_path.is_file() {
        return (None, None);
    }
    match std::fs::read_to_string(&receipt_path)
        .ok()
        .and_then(|contents| serde_json::from_str::<Value>(&contents).ok())
    {
        Some(receipt) => {
            let integrity = verify_plugin_install_receipt(root, &receipt);
            if integrity.status != "verified" {
                warnings.push(format!(
                    "plugin install receipt integrity {}: {}",
                    integrity.status,
                    integrity.summary()
                ));
            }
            (Some(receipt), Some(integrity))
        }
        None => {
            warnings.push(format!(
                "{} is present but is not valid JSON",
                PLUGIN_INSTALL_RECEIPT_FILE
            ));
            (None, None)
        }
    }
}

fn verify_plugin_install_receipt(
    root: &Path,
    receipt: &Value,
) -> PluginInstallReceiptIntegrityStatus {
    let method = receipt
        .get("integrity")
        .and_then(|integrity| integrity.get("method"))
        .and_then(Value::as_str)
        .unwrap_or("missing")
        .to_string();
    let expected_payload_hash = receipt
        .get("integrity")
        .and_then(|integrity| integrity.get("payload_hash"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let actual_payload_hash = plugin_receipt_payload_hash(receipt);
    let expected_content_hash = receipt
        .get("content_hash")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let expected_file_count = receipt
        .get("file_count")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or_default();

    match collect_plugin_receipt_files(root) {
        Ok(files) => {
            let actual_content_hash = aggregate_receipt_hash(&files);
            let payload_hash_matches =
                method == "stable-hash-v1" && expected_payload_hash == actual_payload_hash;
            let content_hash_matches =
                expected_content_hash == actual_content_hash && expected_file_count == files.len();
            let status = if method == "missing" {
                "unsigned"
            } else if payload_hash_matches && content_hash_matches {
                "verified"
            } else {
                "tampered"
            };
            PluginInstallReceiptIntegrityStatus {
                method,
                status: status.to_string(),
                payload_hash_matches,
                content_hash_matches,
                expected_payload_hash,
                actual_payload_hash,
                expected_content_hash,
                actual_content_hash,
                expected_file_count,
                actual_file_count: files.len(),
            }
        }
        Err(error) => PluginInstallReceiptIntegrityStatus {
            method,
            status: "error".to_string(),
            payload_hash_matches: false,
            content_hash_matches: false,
            expected_payload_hash,
            actual_payload_hash,
            expected_content_hash,
            actual_content_hash: format!("error: {error}"),
            expected_file_count,
            actual_file_count: 0,
        },
    }
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
        apps: usize::from(root.join("app.json").is_file()),
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

fn plugin_init_summary(plugin: &PluginInfo, scope: MarketplaceScope) -> Value {
    serde_json::json!({
        "id": &plugin.id,
        "name": plugin.display_name(),
        "scope": scope.as_str(),
        "version": &plugin.version,
        "description": &plugin.description,
        "root": plugin.root.display().to_string(),
        "enabled": plugin.enabled,
        "valid": plugin.valid,
        "components": &plugin.components,
        "app_manifest": &plugin.app_manifest,
        "errors": &plugin.errors,
        "warnings": &plugin.warnings,
    })
}

fn scoped_plugin_root_dir(context: &CommandContext, scope: MarketplaceScope) -> PathBuf {
    match scope {
        MarketplaceScope::User => user_plugin_root_dir(context),
        MarketplaceScope::Project => cwd(context).join(".kiana").join("plugins"),
        MarketplaceScope::Local => cwd(context).join(".kiana").join("plugins.local"),
    }
}

fn user_plugin_root_dir(context: &CommandContext) -> PathBuf {
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
    "usage: kiana plugin [list|status|json [query]|marketplace <add|list|remove|update>|install <plugin>|uninstall <plugin>|show <name>|enable <name>|disable <name>|path [name]|validate [name|path]]\n       scoped roots: add --scope user|project|local to install, uninstall, list, json, show, path, validate, enable, or disable"
}

fn marketplace_usage() -> &'static str {
    "Usage: kiana plugin marketplace [list|add|remove|update]\n       kiana plugin marketplace list [--json]\n       kiana plugin marketplace add <source> [--scope user|project|local]\n       kiana plugin marketplace remove <name> [--scope user|project|local]\n       kiana plugin marketplace update [name]"
}

fn plugin_install_usage() -> &'static str {
    "Usage: kiana plugin install <plugin|plugin@marketplace|path> [--scope user|project|local]"
}

fn plugin_uninstall_usage() -> &'static str {
    "Usage: kiana plugin uninstall <plugin|path> [--scope user|project|local] [--keep-data]"
}

#[derive(Debug)]
struct PluginActionArgs {
    target: String,
    scope: Option<MarketplaceScope>,
}

#[derive(Debug)]
struct PluginReadArgs {
    target: Option<String>,
    scope: MarketplaceScope,
}

#[derive(Debug)]
struct InstallSource {
    root: PathBuf,
    marketplace: Option<String>,
    policy: MarketplacePluginPolicy,
    signature: Option<Value>,
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
    signature: Option<Value>,
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
    signature: Option<Value>,
}

#[derive(Debug, Serialize)]
struct PluginInstallReceipt {
    schema: &'static str,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    source_type: &'static str,
    source_path: String,
    install_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    marketplace: Option<String>,
    policy: MarketplacePluginPolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    managed_policy: Option<ManagedPluginPolicyReceipt>,
    file_count: usize,
    content_hash: String,
    files: Vec<PluginReceiptFile>,
}

#[derive(Debug, Serialize)]
struct PluginReceiptFile {
    path: String,
    bytes: u64,
    content_hash: String,
}

#[derive(Debug, Clone, Serialize)]
struct PluginInstallReceiptIntegrityStatus {
    method: String,
    status: String,
    payload_hash_matches: bool,
    content_hash_matches: bool,
    expected_payload_hash: String,
    actual_payload_hash: String,
    expected_content_hash: String,
    actual_content_hash: String,
    expected_file_count: usize,
    actual_file_count: usize,
}

impl PluginInstallReceiptIntegrityStatus {
    fn summary(&self) -> String {
        format!(
            "payload_hash_matches={} content_hash_matches={} expected_files={} actual_files={}",
            self.payload_hash_matches,
            self.content_hash_matches,
            self.expected_file_count,
            self.actual_file_count
        )
    }
}

#[derive(Debug, Clone)]
struct ManagedPluginPolicyDecision {
    source: PathBuf,
    decision: &'static str,
    reason: String,
}

impl ManagedPluginPolicyDecision {
    fn summary(&self) -> String {
        format!(
            "{} ({}) from {}",
            self.decision,
            self.reason,
            self.source.display()
        )
    }
}

#[derive(Debug, Serialize)]
struct ManagedPluginPolicyReceipt {
    source: String,
    decision: &'static str,
    reason: String,
}

impl From<&ManagedPluginPolicyDecision> for ManagedPluginPolicyReceipt {
    fn from(value: &ManagedPluginPolicyDecision) -> Self {
        Self {
            source: value.source.display().to_string(),
            decision: value.decision,
            reason: value.reason.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedPluginPolicy {
    #[serde(default, alias = "allow_plugins", alias = "allow-plugins")]
    allow: Vec<String>,
    #[serde(default, alias = "deny_plugins", alias = "deny-plugins")]
    deny: Vec<String>,
    #[serde(default, alias = "allow_marketplaces", alias = "allow-marketplaces")]
    allow_marketplaces: Vec<String>,
    #[serde(default, alias = "deny_marketplaces", alias = "deny-marketplaces")]
    deny_marketplaces: Vec<String>,
    #[serde(default, alias = "deny_path_installs", alias = "deny-path-installs")]
    deny_path_installs: bool,
}

impl ManagedPluginPolicy {
    fn is_empty(&self) -> bool {
        self.allow.is_empty()
            && self.deny.is_empty()
            && self.allow_marketplaces.is_empty()
            && self.deny_marketplaces.is_empty()
            && !self.deny_path_installs
    }

    fn evaluate(
        &self,
        plugin_name: &str,
        marketplace: Option<&str>,
        source: PathBuf,
    ) -> Result<Option<ManagedPluginPolicyDecision>> {
        let plugin_candidates = plugin_policy_candidates(plugin_name, marketplace);
        let marketplace_key = marketplace.map(normalize_target);

        if self.deny_path_installs && marketplace.is_none() {
            return Err(anyhow!(
                "plugin '{}' is denied by managed plugin policy {}: path installs are disabled",
                plugin_name,
                source.display()
            ));
        }
        if marketplace_rule_matches(&self.deny_marketplaces, marketplace_key.as_deref()) {
            return Err(anyhow!(
                "plugin '{}' is denied by managed plugin policy {}: marketplace '{}' is denied",
                plugin_name,
                source.display(),
                marketplace.unwrap_or("none")
            ));
        }
        if plugin_rule_matches(&self.deny, &plugin_candidates) {
            return Err(anyhow!(
                "plugin '{}' is denied by managed plugin policy {}",
                plugin_name,
                source.display()
            ));
        }
        if !self.allow_marketplaces.is_empty()
            && marketplace.is_some()
            && !marketplace_rule_matches(&self.allow_marketplaces, marketplace_key.as_deref())
        {
            return Err(anyhow!(
                "plugin '{}' is not allowed by managed plugin policy {}: marketplace '{}' is not allowed",
                plugin_name,
                source.display(),
                marketplace.unwrap_or("none")
            ));
        }
        if !self.allow.is_empty() && !plugin_rule_matches(&self.allow, &plugin_candidates) {
            return Err(anyhow!(
                "plugin '{}' is not allowed by managed plugin policy {}",
                plugin_name,
                source.display()
            ));
        }

        Ok(Some(ManagedPluginPolicyDecision {
            source,
            decision: "allowed",
            reason: "managed plugin policy matched".to_string(),
        }))
    }
}

fn plugin_policy_candidates(plugin_name: &str, marketplace: Option<&str>) -> Vec<String> {
    let plugin = normalize_target(plugin_name);
    let mut candidates = vec![plugin.clone()];
    if let Some(marketplace) = marketplace.map(normalize_target) {
        candidates.push(format!("{plugin}@{marketplace}"));
        candidates.push(format!("{marketplace}/{plugin}"));
    }
    candidates
}

fn plugin_rule_matches(rules: &[String], candidates: &[String]) -> bool {
    rules.iter().any(|rule| {
        let rule = normalize_target(rule);
        rule == "*" || candidates.iter().any(|candidate| candidate == &rule)
    })
}

fn marketplace_rule_matches(rules: &[String], marketplace: Option<&str>) -> bool {
    let Some(marketplace) = marketplace else {
        return false;
    };
    rules.iter().any(|rule| {
        let rule = normalize_target(rule);
        rule == "*" || rule == marketplace
    })
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
    #[serde(skip_serializing_if = "Option::is_none")]
    app_manifest: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    install_receipt: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    install_receipt_integrity: Option<PluginInstallReceiptIntegrityStatus>,
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
    if !cfg!(windows) {
        return path.to_string();
    }
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
    async fn plugin_validate_rejects_invalid_component_json_files() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("component-json-validation");
        let cwd = root.join("project");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("contract-tools");
        write_manifest(&plugin_root, "contract-tools");
        fs::write(plugin_root.join(".mcp.json"), "{ bad json").unwrap();
        fs::write(
            plugin_root.join(".lsp.json"),
            serde_json::to_string_pretty(&json!({
                "lspServers": {
                    "rust-analyzer": {}
                }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::create_dir_all(plugin_root.join("hooks")).unwrap();
        fs::write(
            plugin_root.join("hooks").join("hooks.json"),
            serde_json::to_string_pretty(&json!({
                "PreToolUse": [
                    {"command": "echo invalid nested object"}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(plugin_root.join("app.json"), "[]").unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let validation = PluginCommand
            .execute(context("validate contract-tools", &cwd))
            .await
            .unwrap();

        assert!(validation.value.contains("Validation failed"));
        assert!(validation.value.contains(".mcp.json is not valid JSON"));
        assert!(validation.value.contains(
            ".lsp.json LSP server 'rust-analyzer' requires non-empty string field 'command'"
        ));
        assert!(validation
            .value
            .contains("hooks/hooks.json invalid hook schema at PreToolUse[0]"));
        assert!(validation
            .value
            .contains("app.json must contain a JSON object"));

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
        assert!(installed.value.contains("receipt: "));
        assert!(plugins_dir
            .join("review-tools")
            .join("commands")
            .join("audit.md")
            .is_file());
        let receipt_path = plugins_dir
            .join("review-tools")
            .join(".kiana-install-receipt.json");
        let receipt: Value =
            serde_json::from_str(&fs::read_to_string(&receipt_path).unwrap()).unwrap();
        assert_eq!(receipt["schema"], "kiana.plugin-install-receipt.v1");
        assert_eq!(receipt["name"], "review-tools");
        assert_eq!(receipt["version"], "1.2.3");
        assert_eq!(receipt["source_type"], "marketplace");
        assert_eq!(receipt["marketplace"], "tools-marketplace");
        assert_eq!(receipt["file_count"], 2);
        assert_eq!(receipt["content_hash"].as_str().unwrap().len(), 16);
        assert_eq!(receipt["integrity"]["method"], "stable-hash-v1");
        assert_eq!(
            receipt["integrity"]["payload_hash"].as_str().unwrap().len(),
            16
        );
        assert!(receipt["files"].as_array().unwrap().iter().any(|file| {
            file["path"] == "commands/audit.md"
                && file["content_hash"].as_str().unwrap().len() == 16
        }));

        let list = PluginCommand
            .execute(context("list review-tools", &cwd))
            .await
            .unwrap();
        assert!(list.value.contains("review-tools@1.2.3 [valid enabled]"));
        assert!(list.value.contains("commands=1"));
        let shown = PluginCommand
            .execute(context("show review-tools", &cwd))
            .await
            .unwrap();
        let shown_json: Value = serde_json::from_str(&shown.value).unwrap();
        assert_eq!(
            shown_json["install_receipt"]["schema"],
            "kiana.plugin-install-receipt.v1"
        );
        assert_eq!(
            shown_json["install_receipt_integrity"]["status"],
            "verified"
        );
        assert_eq!(
            shown_json["install_receipt_integrity"]["payload_hash_matches"],
            true
        );
        assert_eq!(
            shown_json["install_receipt_integrity"]["content_hash_matches"],
            true
        );

        fs::write(
            plugins_dir
                .join("review-tools")
                .join("commands")
                .join("audit.md"),
            "# audit\n# tampered",
        )
        .unwrap();
        let tampered = PluginCommand
            .execute(context("show review-tools", &cwd))
            .await
            .unwrap();
        let tampered_json: Value = serde_json::from_str(&tampered.value).unwrap();
        assert_eq!(
            tampered_json["install_receipt_integrity"]["status"],
            "tampered"
        );
        assert_eq!(
            tampered_json["install_receipt_integrity"]["content_hash_matches"],
            false
        );
        assert!(tampered_json["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning
                .as_str()
                .unwrap()
                .contains("plugin install receipt integrity tampered")));
        let tampered_validation = PluginCommand
            .execute(context("validate review-tools", &cwd))
            .await
            .unwrap();
        assert!(tampered_validation
            .value
            .contains("plugin install receipt integrity tampered"));
        assert!(tampered_validation.value.contains("Validation failed"));

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
    async fn plugin_install_records_marketplace_signature_metadata_in_receipt() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let previous_plugins_dir = std::env::var_os("KIANA_PLUGINS_DIR");
        let root = temp_root("install-signature-metadata");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let plugins_dir = kiana_home.join("plugins");
        let marketplace_root = root.join("marketplaces").join("signed-marketplace");
        let marketplace_manifest_dir = marketplace_root.join(".codex-plugin");
        let source_plugin = marketplace_root.join("review-tools");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&marketplace_manifest_dir).unwrap();
        write_manifest(&source_plugin, "review-tools");
        fs::create_dir_all(source_plugin.join("commands")).unwrap();
        fs::write(source_plugin.join("commands").join("audit.md"), "# audit").unwrap();
        let expected_content_hash = super::aggregate_receipt_hash(
            &super::collect_plugin_receipt_files(&source_plugin).unwrap(),
        );
        fs::write(
            marketplace_manifest_dir.join("marketplace.json"),
            serde_json::to_string_pretty(&json!({
                "name": "signed-marketplace",
                "plugins": [
                    {
                        "name": "review-tools",
                        "source": "./review-tools",
                        "signature": {
                            "scheme": "external-signature-v1",
                            "signer": "ACME Marketplace",
                            "keyId": "acme-prod-1",
                            "contentHash": expected_content_hash,
                            "signature": "sig-ed25519-test"
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

        PluginCommand
            .execute(context("install review-tools@signed-marketplace", &cwd))
            .await
            .unwrap();

        let receipt_path = plugins_dir
            .join("review-tools")
            .join(".kiana-install-receipt.json");
        let receipt: Value =
            serde_json::from_str(&fs::read_to_string(&receipt_path).unwrap()).unwrap();
        assert_eq!(receipt["signature"]["scheme"], "external-signature-v1");
        assert_eq!(receipt["signature"]["signer"], "ACME Marketplace");
        assert_eq!(receipt["signature"]["keyId"], "acme-prod-1");
        assert_eq!(receipt["signature"]["contentHash"], expected_content_hash);
        assert_eq!(receipt["signature"]["signature"], "sig-ed25519-test");

        let shown = PluginCommand
            .execute(context("show review-tools", &cwd))
            .await
            .unwrap();
        let shown_json: Value = serde_json::from_str(&shown.value).unwrap();
        assert_eq!(
            shown_json["install_receipt"]["signature"]["scheme"],
            "external-signature-v1"
        );
        assert_eq!(
            shown_json["install_receipt_integrity"]["status"],
            "verified"
        );

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        match previous_plugins_dir {
            Some(value) => std::env::set_var("KIANA_PLUGINS_DIR", value),
            None => std::env::remove_var("KIANA_PLUGINS_DIR"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_install_and_uninstall_support_project_scope() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let previous_plugins_dir = std::env::var_os("KIANA_PLUGINS_DIR");
        let root = temp_root("install-project-scope");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let user_plugins_dir = kiana_home.join("plugins");
        let project_plugins_dir = cwd.join(".kiana").join("plugins");
        let marketplace_root = root.join("marketplaces").join("tools-marketplace");
        let source_plugin = marketplace_root.join("review-tools");
        fs::create_dir_all(&cwd).unwrap();
        write_manifest(&source_plugin, "review-tools");
        fs::create_dir_all(source_plugin.join("commands")).unwrap();
        fs::write(source_plugin.join("commands").join("audit.md"), "# audit").unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &user_plugins_dir);

        PluginCommand
            .execute(context(
                &format!("marketplace add {}", marketplace_root.display()),
                &cwd,
            ))
            .await
            .unwrap();

        let installed = PluginCommand
            .execute(context(
                "install review-tools@tools-marketplace --scope project",
                &cwd,
            ))
            .await
            .unwrap();
        assert!(installed.value.contains("Installed plugin: review-tools"));
        assert!(installed.value.contains(&format!(
            "path: {}",
            project_plugins_dir.join("review-tools").display()
        )));
        assert!(project_plugins_dir
            .join("review-tools")
            .join("commands")
            .join("audit.md")
            .is_file());
        assert!(!user_plugins_dir.join("review-tools").exists());
        assert!(project_plugins_dir.join("disabled_plugins.json").is_file());

        let uninstalled = PluginCommand
            .execute(context("uninstall review-tools --scope project", &cwd))
            .await
            .unwrap();
        assert!(uninstalled
            .value
            .contains("Uninstalled plugin: review-tools"));
        assert!(!project_plugins_dir.join("review-tools").exists());

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        match previous_plugins_dir {
            Some(value) => std::env::set_var("KIANA_PLUGINS_DIR", value),
            None => std::env::remove_var("KIANA_PLUGINS_DIR"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_install_and_uninstall_support_local_scope() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let previous_plugins_dir = std::env::var_os("KIANA_PLUGINS_DIR");
        let root = temp_root("install-local-scope");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let user_plugins_dir = kiana_home.join("plugins");
        let local_plugins_dir = cwd.join(".kiana").join("plugins.local");
        let marketplace_root = root.join("marketplaces").join("tools-marketplace");
        let source_plugin = marketplace_root.join("review-tools");
        fs::create_dir_all(&cwd).unwrap();
        write_manifest(&source_plugin, "review-tools");
        fs::create_dir_all(source_plugin.join("commands")).unwrap();
        fs::write(source_plugin.join("commands").join("audit.md"), "# audit").unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &user_plugins_dir);

        PluginCommand
            .execute(context(
                &format!("marketplace add {}", marketplace_root.display()),
                &cwd,
            ))
            .await
            .unwrap();

        let installed = PluginCommand
            .execute(context(
                "install review-tools@tools-marketplace --scope local",
                &cwd,
            ))
            .await
            .unwrap();
        assert!(installed.value.contains("Installed plugin: review-tools"));
        assert!(installed.value.contains(&format!(
            "path: {}",
            local_plugins_dir.join("review-tools").display()
        )));
        assert!(local_plugins_dir
            .join("review-tools")
            .join("commands")
            .join("audit.md")
            .is_file());
        assert!(!user_plugins_dir.join("review-tools").exists());
        assert!(local_plugins_dir.join("disabled_plugins.json").is_file());

        let uninstalled = PluginCommand
            .execute(context("uninstall review-tools --scope local", &cwd))
            .await
            .unwrap();
        assert!(uninstalled
            .value
            .contains("Uninstalled plugin: review-tools"));
        assert!(!local_plugins_dir.join("review-tools").exists());

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        match previous_plugins_dir {
            Some(value) => std::env::set_var("KIANA_PLUGINS_DIR", value),
            None => std::env::remove_var("KIANA_PLUGINS_DIR"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_read_commands_support_project_scope() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let previous_plugins_dir = std::env::var_os("KIANA_PLUGINS_DIR");
        let root = temp_root("read-project-scope");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let user_plugins_dir = kiana_home.join("plugins");
        let project_plugins_dir = cwd.join(".kiana").join("plugins");
        let marketplace_root = root.join("marketplaces").join("tools-marketplace");
        let source_plugin = marketplace_root.join("review-tools");
        fs::create_dir_all(&cwd).unwrap();
        write_manifest(&source_plugin, "review-tools");
        fs::create_dir_all(source_plugin.join("commands")).unwrap();
        fs::write(source_plugin.join("commands").join("audit.md"), "# audit").unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &user_plugins_dir);

        PluginCommand
            .execute(context(
                &format!("marketplace add {}", marketplace_root.display()),
                &cwd,
            ))
            .await
            .unwrap();
        PluginCommand
            .execute(context(
                "install review-tools@tools-marketplace --scope project",
                &cwd,
            ))
            .await
            .unwrap();

        let default_list = PluginCommand
            .execute(context("list review-tools", &cwd))
            .await
            .unwrap();
        assert!(default_list.value.contains("No plugins."));

        let scoped_list = PluginCommand
            .execute(context("list --scope project review-tools", &cwd))
            .await
            .unwrap();
        assert!(scoped_list
            .value
            .contains(&format!("path: {}", project_plugins_dir.display())));
        assert!(scoped_list
            .value
            .contains("review-tools@1.2.3 [valid enabled]"));

        let scoped_json = PluginCommand
            .execute(context("json --scope project review-tools", &cwd))
            .await
            .unwrap();
        let scoped_json: Value = serde_json::from_str(&scoped_json.value).unwrap();
        assert_eq!(scoped_json[0]["id"], "review-tools");
        assert_eq!(scoped_json[0]["manifest_name"], "review-tools");
        assert_eq!(
            scoped_json[0]["root"],
            project_plugins_dir
                .join("review-tools")
                .display()
                .to_string()
        );

        let scoped_show = PluginCommand
            .execute(context("show review-tools --scope project", &cwd))
            .await
            .unwrap();
        let scoped_show: Value = serde_json::from_str(&scoped_show.value).unwrap();
        assert_eq!(scoped_show["manifest"]["name"], "review-tools");
        assert_eq!(
            scoped_show["install_receipt_integrity"]["status"],
            "verified"
        );

        let scoped_path = PluginCommand
            .execute(context("path review-tools --scope project", &cwd))
            .await
            .unwrap();
        assert_eq!(
            scoped_path.value,
            project_plugins_dir
                .join("review-tools")
                .display()
                .to_string()
        );

        let scoped_validate = PluginCommand
            .execute(context("validate review-tools --scope project", &cwd))
            .await
            .unwrap();
        assert!(scoped_validate.value.contains("Validation passed"));

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        match previous_plugins_dir {
            Some(value) => std::env::set_var("KIANA_PLUGINS_DIR", value),
            None => std::env::remove_var("KIANA_PLUGINS_DIR"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_read_commands_support_local_scope() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let previous_plugins_dir = std::env::var_os("KIANA_PLUGINS_DIR");
        let root = temp_root("read-local-scope");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let user_plugins_dir = kiana_home.join("plugins");
        let local_plugins_dir = cwd.join(".kiana").join("plugins.local");
        let marketplace_root = root.join("marketplaces").join("tools-marketplace");
        let source_plugin = marketplace_root.join("review-tools");
        fs::create_dir_all(&cwd).unwrap();
        write_manifest(&source_plugin, "review-tools");
        fs::create_dir_all(source_plugin.join("commands")).unwrap();
        fs::write(source_plugin.join("commands").join("audit.md"), "# audit").unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &user_plugins_dir);

        PluginCommand
            .execute(context(
                &format!("marketplace add {}", marketplace_root.display()),
                &cwd,
            ))
            .await
            .unwrap();
        PluginCommand
            .execute(context(
                "install review-tools@tools-marketplace --scope local",
                &cwd,
            ))
            .await
            .unwrap();

        let scoped_list = PluginCommand
            .execute(context("list --scope local review-tools", &cwd))
            .await
            .unwrap();
        assert!(scoped_list
            .value
            .contains(&format!("path: {}", local_plugins_dir.display())));
        assert!(scoped_list
            .value
            .contains("review-tools@1.2.3 [valid enabled]"));

        let scoped_path = PluginCommand
            .execute(context("path review-tools --scope local", &cwd))
            .await
            .unwrap();
        assert_eq!(
            scoped_path.value,
            local_plugins_dir.join("review-tools").display().to_string()
        );

        let scoped_validate = PluginCommand
            .execute(context("validate --scope local", &cwd))
            .await
            .unwrap();
        assert!(scoped_validate
            .value
            .contains("Validating plugin: review-tools"));
        assert!(scoped_validate.value.contains("Validation passed"));

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        match previous_plugins_dir {
            Some(value) => std::env::set_var("KIANA_PLUGINS_DIR", value),
            None => std::env::remove_var("KIANA_PLUGINS_DIR"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_state_commands_support_project_scope() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let previous_plugins_dir = std::env::var_os("KIANA_PLUGINS_DIR");
        let root = temp_root("state-project-scope");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let user_plugins_dir = kiana_home.join("plugins");
        let project_plugins_dir = cwd.join(".kiana").join("plugins");
        let marketplace_root = root.join("marketplaces").join("tools-marketplace");
        let source_plugin = marketplace_root.join("review-tools");
        fs::create_dir_all(&cwd).unwrap();
        write_manifest(&source_plugin, "review-tools");
        fs::create_dir_all(source_plugin.join("commands")).unwrap();
        fs::write(source_plugin.join("commands").join("audit.md"), "# audit").unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &user_plugins_dir);

        PluginCommand
            .execute(context(
                &format!("marketplace add {}", marketplace_root.display()),
                &cwd,
            ))
            .await
            .unwrap();
        PluginCommand
            .execute(context(
                "install review-tools@tools-marketplace --scope project",
                &cwd,
            ))
            .await
            .unwrap();

        let disabled = PluginCommand
            .execute(context("disable review-tools --scope project", &cwd))
            .await
            .unwrap();
        assert!(disabled.value.contains("Plugin disabled: review-tools"));
        assert!(disabled.value.contains(&format!(
            "state: {}",
            project_plugins_dir.join("disabled_plugins.json").display()
        )));

        let scoped_list = PluginCommand
            .execute(context("list --scope project review-tools", &cwd))
            .await
            .unwrap();
        assert!(scoped_list
            .value
            .contains("review-tools@1.2.3 [valid disabled]"));

        let default_list = PluginCommand
            .execute(context("list review-tools", &cwd))
            .await
            .unwrap();
        assert!(default_list.value.contains("No plugins."));

        let enabled = PluginCommand
            .execute(context("enable review-tools --scope project", &cwd))
            .await
            .unwrap();
        assert!(enabled.value.contains("Plugin enabled: review-tools"));
        let scoped_list = PluginCommand
            .execute(context("list --scope project review-tools", &cwd))
            .await
            .unwrap();
        assert!(scoped_list
            .value
            .contains("review-tools@1.2.3 [valid enabled]"));

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        match previous_plugins_dir {
            Some(value) => std::env::set_var("KIANA_PLUGINS_DIR", value),
            None => std::env::remove_var("KIANA_PLUGINS_DIR"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_state_commands_support_local_scope() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let previous_plugins_dir = std::env::var_os("KIANA_PLUGINS_DIR");
        let root = temp_root("state-local-scope");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let user_plugins_dir = kiana_home.join("plugins");
        let local_plugins_dir = cwd.join(".kiana").join("plugins.local");
        let marketplace_root = root.join("marketplaces").join("tools-marketplace");
        let source_plugin = marketplace_root.join("review-tools");
        fs::create_dir_all(&cwd).unwrap();
        write_manifest(&source_plugin, "review-tools");
        fs::create_dir_all(source_plugin.join("commands")).unwrap();
        fs::write(source_plugin.join("commands").join("audit.md"), "# audit").unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &user_plugins_dir);

        PluginCommand
            .execute(context(
                &format!("marketplace add {}", marketplace_root.display()),
                &cwd,
            ))
            .await
            .unwrap();
        PluginCommand
            .execute(context(
                "install review-tools@tools-marketplace --scope local",
                &cwd,
            ))
            .await
            .unwrap();

        let disabled = PluginCommand
            .execute(context("disable review-tools --scope local", &cwd))
            .await
            .unwrap();
        assert!(disabled.value.contains("Plugin disabled: review-tools"));
        assert!(disabled.value.contains(&format!(
            "state: {}",
            local_plugins_dir.join("disabled_plugins.json").display()
        )));

        let scoped_json = PluginCommand
            .execute(context("json --scope local review-tools", &cwd))
            .await
            .unwrap();
        let scoped_json: Value = serde_json::from_str(&scoped_json.value).unwrap();
        assert_eq!(scoped_json[0]["enabled"], false);

        let enabled = PluginCommand
            .execute(context("enable review-tools --scope local", &cwd))
            .await
            .unwrap();
        assert!(enabled.value.contains("Plugin enabled: review-tools"));
        let scoped_json = PluginCommand
            .execute(context("json --scope local review-tools", &cwd))
            .await
            .unwrap();
        let scoped_json: Value = serde_json::from_str(&scoped_json.value).unwrap();
        assert_eq!(scoped_json[0]["enabled"], true);

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        match previous_plugins_dir {
            Some(value) => std::env::set_var("KIANA_PLUGINS_DIR", value),
            None => std::env::remove_var("KIANA_PLUGINS_DIR"),
        }
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
    async fn plugin_install_rejects_managed_plugin_policy_deny() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let previous_policy = std::env::var_os("KIANA_MANAGED_PLUGIN_POLICY_FILE");
        let root = temp_root("managed-plugin-deny");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let plugins_dir = kiana_home.join("plugins");
        let marketplace_root = root.join("marketplaces").join("tools-marketplace");
        let source_plugin = marketplace_root.join("review-tools");
        let policy_file = root.join("managed-plugin-policy.json");
        fs::create_dir_all(&cwd).unwrap();
        write_manifest(&source_plugin, "review-tools");
        fs::write(
            &policy_file,
            serde_json::to_string_pretty(&json!({
                "plugins": {
                    "deny": ["review-tools@tools-marketplace"]
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);
        std::env::set_var("KIANA_MANAGED_PLUGIN_POLICY_FILE", &policy_file);

        PluginCommand
            .execute(context(
                &format!("marketplace add {}", marketplace_root.display()),
                &cwd,
            ))
            .await
            .unwrap();

        let error = PluginCommand
            .execute(context("install review-tools@tools-marketplace", &cwd))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("plugin 'review-tools' is denied by managed plugin policy"));
        assert!(!plugins_dir.join("review-tools").exists());

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        match previous_policy {
            Some(value) => std::env::set_var("KIANA_MANAGED_PLUGIN_POLICY_FILE", value),
            None => std::env::remove_var("KIANA_MANAGED_PLUGIN_POLICY_FILE"),
        }
        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_install_records_managed_plugin_policy_allow_decision() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let previous_policy = std::env::var_os("KIANA_MANAGED_PLUGIN_POLICY_FILE");
        let root = temp_root("managed-plugin-allow");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let plugins_dir = kiana_home.join("plugins");
        let marketplace_root = root.join("marketplaces").join("tools-marketplace");
        let source_plugin = marketplace_root.join("review-tools");
        let policy_file = root.join("managed-plugin-policy.json");
        fs::create_dir_all(&cwd).unwrap();
        write_manifest(&source_plugin, "review-tools");
        fs::write(
            &policy_file,
            serde_json::to_string_pretty(&json!({
                "plugins": {
                    "allow": ["review-tools@tools-marketplace"],
                    "allowMarketplaces": ["tools-marketplace"]
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);
        std::env::set_var("KIANA_MANAGED_PLUGIN_POLICY_FILE", &policy_file);

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
        assert!(installed
            .value
            .contains("managed_policy: allowed (managed plugin policy matched)"));
        let receipt_path = plugins_dir
            .join("review-tools")
            .join(".kiana-install-receipt.json");
        let receipt: Value =
            serde_json::from_str(&fs::read_to_string(receipt_path).unwrap()).unwrap();
        assert_eq!(receipt["managed_policy"]["decision"], "allowed");
        assert_eq!(
            receipt["managed_policy"]["reason"],
            "managed plugin policy matched"
        );

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        match previous_policy {
            Some(value) => std::env::set_var("KIANA_MANAGED_PLUGIN_POLICY_FILE", value),
            None => std::env::remove_var("KIANA_MANAGED_PLUGIN_POLICY_FILE"),
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
    async fn plugin_install_from_remote_marketplace_npm_file_package_source() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let root = temp_root("npm-file-plugin-source");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let plugins_dir = kiana_home.join("plugins");
        let package_root = kiana_home
            .join("plugin-marketplace-cache")
            .join("packages")
            .join("packaged-tools");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(package_root.join("commands")).unwrap();
        write_manifest(&package_root, "packaged-tools");
        fs::write(package_root.join("commands").join("audit.md"), "# audit").unwrap();
        let marketplace_url = serve_json_once(
            serde_json::to_string_pretty(&json!({
                "name": "package-marketplace",
                "owner": { "name": "Kiana Tests" },
                "plugins": [
                    {
                        "name": "packaged-tools",
                        "source": {
                            "source": "npm",
                            "package": "file:packages/packaged-tools"
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

        let installed = PluginCommand
            .execute(context("install packaged-tools@package-marketplace", &cwd))
            .await
            .unwrap();
        assert!(installed.value.contains("Installed plugin: packaged-tools"));
        assert!(installed.value.contains("marketplace: package-marketplace"));
        assert!(plugins_dir
            .join("packaged-tools")
            .join("commands")
            .join("audit.md")
            .is_file());

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_install_from_remote_marketplace_pip_file_package_source() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let root = temp_root("pip-file-plugin-source");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let plugins_dir = kiana_home.join("plugins");
        let package_root = kiana_home
            .join("plugin-marketplace-cache")
            .join("packages")
            .join("python-tools");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(package_root.join("commands")).unwrap();
        write_manifest(&package_root, "python-tools");
        fs::write(package_root.join("commands").join("audit.md"), "# audit").unwrap();
        let marketplace_url = serve_json_once(
            serde_json::to_string_pretty(&json!({
                "name": "python-marketplace",
                "owner": { "name": "Kiana Tests" },
                "plugins": [
                    {
                        "name": "python-tools",
                        "source": {
                            "source": "pip",
                            "package": "file:packages/python-tools"
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

        let installed = PluginCommand
            .execute(context("install python-tools@python-marketplace", &cwd))
            .await
            .unwrap();
        assert!(installed.value.contains("Installed plugin: python-tools"));
        assert!(installed.value.contains("marketplace: python-marketplace"));
        assert!(plugins_dir
            .join("python-tools")
            .join("commands")
            .join("audit.md")
            .is_file());

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
    async fn plugin_install_from_local_marketplace_file_remote_git_source() {
        let _guard = env_lock().lock().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let root = temp_root("local-marketplace-remote-git-source");
        let cwd = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        let plugins_dir = kiana_home.join("plugins");
        let marketplace_root = root.join("marketplaces").join("local-remote-marketplace");
        let plugin_repo = root.join("git").join("review-tools");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&marketplace_root).unwrap();
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
        fs::write(
            marketplace_root.join("marketplace.json"),
            serde_json::to_string_pretty(&json!({
                "name": "local-remote-marketplace",
                "owner": { "name": "Kiana Tests" },
                "plugins": [
                    {
                        "name": "review-tools",
                        "source": {
                            "source": "git",
                            "url": plugin_repo_url
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

        let installed = PluginCommand
            .execute(context(
                "install review-tools@local-remote-marketplace",
                &cwd,
            ))
            .await
            .unwrap();
        assert!(installed.value.contains("Installed plugin: review-tools"));
        assert!(installed
            .value
            .contains("marketplace: local-remote-marketplace"));
        assert!(plugins_dir
            .join("review-tools")
            .join("commands")
            .join("audit.md")
            .is_file());

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
