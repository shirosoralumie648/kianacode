//! Compatibility adapter from legacy `Command` values to the domain extension catalog.
//!
//! The adapter keeps the old runner-facing DTO while moving selection and duplicate diagnostics
//! to the deterministic domain projection. `allowed_tools` remains a declaration only.

use crate::types::{Command, LoadedFrom};
use kiana_domain::{
    json_digest, CatalogCandidate, CatalogEntry, CatalogEntryKind, EvidenceStatus,
    ExtensionCatalog, SourceKind, SourceRef,
};
use serde_json::json;
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub struct SkillCatalog {
    pub catalog: ExtensionCatalog,
    pub selected: Vec<Command>,
    pub shadowed: Vec<CatalogEntry>,
}

pub fn build_skill_catalog(commands: &[Command], generation: u64) -> Result<SkillCatalog, String> {
    let candidates = commands
        .iter()
        .map(command_candidate)
        .collect::<Result<Vec<_>, _>>()?;
    let catalog = ExtensionCatalog::new(generation, candidates)?;
    let selected_sources = catalog
        .selected()
        .map(|candidate| candidate.source.source_id.clone())
        .collect::<BTreeSet<_>>();
    let mut selected = commands
        .iter()
        .filter(|command| selected_sources.contains(&command_source_id(command)))
        .cloned()
        .collect::<Vec<_>>();
    selected.sort_by(|left, right| {
        command_namespace(left)
            .cmp(&command_namespace(right))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.description.cmp(&right.description))
    });
    let shadowed = catalog.shadowed().cloned().collect();
    Ok(SkillCatalog {
        catalog,
        selected,
        shadowed,
    })
}

fn command_candidate(command: &Command) -> Result<CatalogCandidate, String> {
    let content_digest = json_digest(&json!({
        "name": command.name,
        "description": command.description,
        "content": command.content,
        "allowed_tools": command.allowed_tools,
    }));
    let source_id = command_source_id_with_content(command, &content_digest);
    let source = SourceRef::new(
        source_id,
        SourceKind::Prompt,
        format!("skill:{}/{}", command_namespace(command), command.name),
        "legacy-command:v1",
        content_digest.clone(),
        None,
        EvidenceStatus::Attributed,
    )?;
    Ok(CatalogCandidate {
        kind: CatalogEntryKind::Skill,
        namespace: command_namespace(command),
        name: command.name.clone(),
        version: "legacy".to_owned(),
        content_digest,
        source,
        precedence: command_precedence(command),
    })
}

fn command_namespace(command: &Command) -> String {
    match command.loaded_from {
        LoadedFrom::Bundled => "bundled",
        LoadedFrom::Plugin => "plugin",
        LoadedFrom::Managed => "managed",
        LoadedFrom::Mcp => "mcp",
        // Skill names share one catalog namespace; source scope is expressed by precedence.
        LoadedFrom::Skills => "skills",
        LoadedFrom::CommandsDeprecated => "legacy",
    }
    .to_owned()
}

fn command_precedence(command: &Command) -> u16 {
    match command.loaded_from {
        LoadedFrom::Bundled => 10,
        LoadedFrom::Plugin => 30,
        LoadedFrom::Managed => 40,
        LoadedFrom::Skills if command.source == crate::types::SettingSource::ProjectSettings => 60,
        LoadedFrom::Skills => 50,
        LoadedFrom::Mcp => 70,
        LoadedFrom::CommandsDeprecated => 80,
    }
}

fn command_source_id(command: &Command) -> String {
    let content_digest = json_digest(&json!({
        "name": command.name,
        "description": command.description,
        "content": command.content,
        "allowed_tools": command.allowed_tools,
    }));
    command_source_id_with_content(command, &content_digest)
}

fn command_source_id_with_content(command: &Command, content_digest: &str) -> String {
    let root_digest = json_digest(&json!({
        "root": command
            .skill_root
            .as_ref()
            .map(|path| path.to_string_lossy().to_string()),
    }));
    format!(
        "skill-source:{}:{}:{}",
        command_namespace(command),
        root_digest.trim_start_matches("sha256:"),
        content_digest.trim_start_matches("sha256:")
    )
}
