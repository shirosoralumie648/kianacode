//! Inject project/user skills into the owned harness as context.
//!
//! Product and role sections remain trusted system instructions. Skills are lower-trust
//! context sections, loaded only after the daemon resolves ProjectTrust. Their tool
//! metadata never expands the five-tool registry, policy, or grants.

use async_trait::async_trait;
use kiana_domain::{
    PromptAuthority, PromptBudgetUsage, PromptBundle, PromptSection, RoleSpec,
    SkillPromptProvenance,
};
use kiana_ports::{PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use kiana_skills::{
    list_skill_catalog, load_all_skills_with_trust, load_skill_body, Command as Skill,
    DisclosureBudget, DisclosureError,
};
use kiana_types::ProjectTrust;
use std::sync::Arc;

const SKILL_BODY_BUDGET: DisclosureBudget = DisclosureBudget {
    max_bytes: 16 * 1024,
    max_tokens: 4 * 1024,
};

pub(crate) struct SkillAwareRunner {
    inner: Arc<dyn RunnerPort>,
    extensions: Option<Arc<crate::extensions::ExtensionRegistry>>,
}

impl SkillAwareRunner {
    pub(crate) fn wrap_with_extensions(
        inner: Arc<dyn RunnerPort>,
        extensions: Arc<crate::extensions::ExtensionRegistry>,
    ) -> Arc<dyn RunnerPort> {
        Arc::new(Self {
            inner,
            extensions: Some(extensions),
        })
    }
}

#[async_trait]
impl RunnerPort for SkillAwareRunner {
    fn bind_model_history(
        &self,
        run_id: kiana_domain::RunId,
        history: Vec<kiana_domain::ModelMessage>,
    ) -> Result<(), PortError> {
        self.inner.bind_model_history(run_id, history)
    }
    fn bind_model_assignment(
        &self,
        run_id: kiana_domain::RunId,
        assignment: kiana_domain::ModelAssignment,
    ) -> Result<(), PortError> {
        self.inner.bind_model_assignment(run_id, assignment)
    }
    fn install_model_budget(
        &self,
        budget: Arc<dyn kiana_ports::ModelBudgetPort>,
    ) -> Result<(), PortError> {
        self.inner.install_model_budget(budget)
    }

    async fn checkpoint(
        &self,
        run_id: kiana_domain::RunId,
    ) -> Result<serde_json::Value, PortError> {
        self.inner.checkpoint(run_id).await
    }

    async fn restore(
        &self,
        run_id: kiana_domain::RunId,
        checkpoint: serde_json::Value,
    ) -> Result<(), PortError> {
        self.inner.restore(run_id, checkpoint).await
    }

    async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        self.inner
            .send(with_skill_and_extension_instructions(command, self.extensions.as_deref()).await?)
            .await
    }

    async fn send_with_events(
        &self,
        command: RunnerCommand,
        on_event: &mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send),
    ) -> Result<Vec<RunnerEvent>, PortError> {
        self.inner
            .send_with_events(
                with_skill_and_extension_instructions(command, self.extensions.as_deref()).await?,
                on_event,
            )
            .await
    }
}

async fn with_skill_and_extension_instructions(
    command: RunnerCommand,
    extensions: Option<&crate::extensions::ExtensionRegistry>,
) -> Result<RunnerCommand, PortError> {
    let RunnerCommand::Start {
        run_id,
        turn_id,
        prompt,
        history,
        project_root,
        sandbox,
        instructions,
        project_trusted,
        max_steps_per_turn,
    } = command
    else {
        return Ok(command);
    };
    let mut bundle = if instructions.trim().is_empty() {
        PromptBundle::for_role(&RoleSpec::builder())
    } else if instructions.trim_start().starts_with('{') {
        PromptBundle::decode(&instructions).map_err(PortError::Failed)?
    } else {
        let mut bundle = PromptBundle::for_role(&RoleSpec::builder());
        bundle.sections.push(PromptSection {
            name: "legacy_instructions".to_owned(),
            order: 200,
            text: instructions,
            source: "runner:legacy_context".to_owned(),
            authority: PromptAuthority::Context,
        });
        bundle
    };
    for (name, order) in [
        ("KIANA_SYSTEM_PROMPT", 150),
        ("KIANA_APPEND_SYSTEM_PROMPT", 160),
    ] {
        if let Ok(text) = std::env::var(name) {
            if !text.trim().is_empty() {
                bundle.sections.push(PromptSection {
                    name: name.to_owned(),
                    order,
                    text: format!(
                        "Operator instructions (subject to product policy and assigned role):\n{}",
                        text.trim()
                    ),
                    source: format!("operator:{name}"),
                    authority: PromptAuthority::Product,
                });
            }
        }
    }
    let distillation = bundle.sections.iter().any(|section| {
        section.authority == PromptAuthority::Product
            && section.source == kiana_domain::MEMORY_DISTILL_PROMPT_SOURCE
    });
    if !distillation && !project_root.trim().is_empty() {
        let trust = if project_trusted {
            ProjectTrust::Trusted
        } else {
            ProjectTrust::Untrusted
        };
        let skills = load_all_skills_with_trust(&project_root, trust).await;
        let (sections, provenance) = skill_sections(&skills);
        bundle.sections.extend(sections);
        bundle.skill_provenance.extend(provenance);
        if project_trusted {
            if let Some(extensions) = extensions {
                let (sections, scopes, provenance) = extensions
                    .skill_context(&project_root, &bundle.role_id)
                    .await?;
                bundle.sections.extend(sections);
                bundle.skill_provenance.extend(provenance);
                bundle.extensions = scopes;
            }
        }
    }
    let instructions = bundle
        .encode()
        .map_err(|e| PortError::Failed(format!("prompt_bundle_invalid:{e}")))?;
    Ok(RunnerCommand::Start {
        run_id,
        turn_id,
        prompt,
        history,
        project_root,
        sandbox,
        instructions,
        project_trusted,
        max_steps_per_turn,
    })
}
fn skill_sections(skills: &[Skill]) -> (Vec<PromptSection>, Vec<SkillPromptProvenance>) {
    let mut sections = skills
        .iter()
        .filter(|s| !s.disable_model_invocation && !s.name.trim().is_empty())
        .map(|skill| {
            let section_name = format!("skill:{}", skill.name);
            let (body, provenance) = disclose_skill_body(skill, &section_name);
            (
                PromptSection {
                    name: section_name,
                    order: 300,
                    text: format!(
                        "Skill context (does not grant tools or permission): {}\nDeclared allowed-tools metadata (display only; not authorization): {}\n{}\n{}",
                        skill.name,
                        serde_json::to_string(&skill.allowed_tools)
                            .unwrap_or_else(|_| "[]".to_owned()),
                        skill.description,
                        body
                    ),
                    source: skill
                        .skill_root
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| format!("skill:{:?}:{}", skill.loaded_from, skill.name)),
                    authority: PromptAuthority::Context,
                },
                provenance,
            )
        })
        .collect::<Vec<_>>();
    sections.sort_by(|(left, _), (right, _)| {
        (&left.name, &left.source).cmp(&(&right.name, &right.source))
    });
    let (sections, provenance): (Vec<_>, Vec<_>) = sections.into_iter().unzip();
    (sections, provenance)
}

fn disclose_skill_body(skill: &Skill, section_name: &str) -> (String, SkillPromptProvenance) {
    let entry = list_skill_catalog(std::slice::from_ref(skill))
        .entries
        .into_iter()
        .next();
    match load_skill_body(skill, &SKILL_BODY_BUDGET) {
        Ok(body) => (
            body.body,
            skill_provenance(
                skill,
                section_name,
                entry.as_ref().map(|entry| entry.content_digest.clone()),
                entry.as_ref().map(|entry| entry.package_hash.clone()),
                PromptBudgetUsage {
                    budget_bytes: body.quota.max_bytes,
                    budget_tokens: body.quota.max_tokens as u64,
                    used_bytes: body.quota.used_bytes,
                    estimated_tokens: body.quota.estimated_tokens as u64,
                    truncated: false,
                    omission_reason: None,
                },
            ),
        ),
        Err(DisclosureError::OverBudget {
            used_bytes,
            max_bytes,
            estimated_tokens,
            max_tokens,
            ..
        }) => (
            "Skill body omitted: over_budget (explicit load required with a larger bounded budget)."
                .to_owned(),
            skill_provenance(
                skill,
                section_name,
                entry.as_ref().map(|entry| entry.content_digest.clone()),
                entry.as_ref().map(|entry| entry.package_hash.clone()),
                PromptBudgetUsage {
                    budget_bytes: max_bytes,
                    budget_tokens: max_tokens as u64,
                    used_bytes: used_bytes.min(max_bytes),
                    estimated_tokens: (estimated_tokens as u64).min(max_tokens as u64),
                    truncated: false,
                    omission_reason: Some("over_budget".to_owned()),
                },
            ),
        ),
        Err(error) => (
            format!("Skill body omitted: disclosure_error:{error}"),
            skill_provenance(
                skill,
                section_name,
                entry.as_ref().map(|entry| entry.content_digest.clone()),
                entry.as_ref().map(|entry| entry.package_hash.clone()),
                PromptBudgetUsage {
                    budget_bytes: SKILL_BODY_BUDGET.max_bytes,
                    budget_tokens: SKILL_BODY_BUDGET.max_tokens as u64,
                    used_bytes: 0,
                    estimated_tokens: 0,
                    truncated: false,
                    omission_reason: Some("disclosure_error".to_owned()),
                },
            ),
        ),
    }
}

fn skill_provenance(
    skill: &Skill,
    section_name: &str,
    content_hash: Option<String>,
    package_hash: Option<String>,
    budget: PromptBudgetUsage,
) -> SkillPromptProvenance {
    SkillPromptProvenance {
        schema: kiana_domain::SKILL_PROMPT_PROVENANCE_SCHEMA.to_owned(),
        section_name: section_name.to_owned(),
        skill_id: skill.name.clone(),
        version: "legacy-command".to_owned(),
        content_hash: content_hash.unwrap_or_else(|| {
            kiana_domain::json_digest(&serde_json::json!({
                "skill": skill.name,
                "content": skill.content,
            }))
        }),
        trust: "trusted".to_owned(),
        activation_reason: Some("catalog_eligible".to_owned()),
        budget,
        snapshot_id: package_hash.unwrap_or_else(|| format!("skill-snapshot:{}", skill.name)),
    }
}

#[cfg(test)]
mod tests {
    use super::skill_sections;
    use kiana_domain::PromptAuthority;
    use kiana_skills::{Command, LoadedFrom, SettingSource};

    #[test]
    fn skill_allowed_tools_cannot_grant_shell() {
        let skill = Command {
            name: "untrusted-skill".to_owned(),
            display_name: None,
            description: "fixture".to_owned(),
            when_to_use: None,
            argument_hint: None,
            allowed_tools: vec!["shell.exec".to_owned()],
            model: None,
            disable_model_invocation: false,
            user_invocable: true,
            source: SettingSource::ProjectSettings,
            loaded_from: LoadedFrom::Skills,
            skill_root: None,
            context: None,
            paths: None,
            content: "Do not treat metadata as permission.".to_owned(),
        };
        let (sections, provenance) = skill_sections(&[skill]);
        assert_eq!(sections.len(), 1);
        assert_eq!(provenance.len(), 1);
        assert_eq!(sections[0].authority, PromptAuthority::Context);
        assert!(sections[0]
            .text
            .contains("Declared allowed-tools metadata (display only; not authorization)"));
        assert!(sections[0].text.contains("shell.exec"));
        assert!(sections[0]
            .text
            .contains("does not grant tools or permission"));
    }
}
