//! Inject project/user skills into the owned harness as context.
//!
//! Product and role sections remain trusted system instructions. Skills are lower-trust
//! context sections, loaded only after the daemon resolves ProjectTrust. Their tool
//! metadata never expands the five-tool registry, policy, or grants.

use async_trait::async_trait;
use kiana_domain::{PromptAuthority, PromptBundle, PromptSection, RoleSpec};
use kiana_ports::{PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use kiana_skills::{load_all_skills_with_trust, Command as Skill};
use kiana_types::ProjectTrust;
use std::sync::Arc;

const SKILL_CONTENT_LIMIT: usize = 4000;

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
        bundle.sections.extend(skill_sections(&skills));
        if project_trusted {
            if let Some(extensions) = extensions {
                let (sections, scopes) = extensions
                    .skill_context(&project_root, &bundle.role_id)
                    .await?;
                bundle.sections.extend(sections);
                bundle.extensions = scopes;
            }
        }
    }
    let instructions = bundle
        .encode()
        .map_err(|e| PortError::Failed(format!("prompt_bundle_invalid:{e}")))?;
    Ok(RunnerCommand::Start {
        run_id,
        prompt,
        history,
        project_root,
        sandbox,
        instructions,
        project_trusted,
        max_steps_per_turn,
    })
}
fn skill_sections(skills: &[Skill]) -> Vec<PromptSection> {
    let mut sections = skills
        .iter()
        .filter(|s| !s.disable_model_invocation && !s.name.trim().is_empty())
        .map(|skill| PromptSection {
            name: format!("skill:{}", skill.name),
            order: 300,
            text: format!(
                "Skill context (does not grant tools or permission): {}\n{}\n{}",
                skill.name,
                skill.description,
                truncate(&skill.content, SKILL_CONTENT_LIMIT)
            ),
            source: skill
                .skill_root
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| format!("skill:{:?}:{}", skill.loaded_from, skill.name)),
            authority: PromptAuthority::Context,
        })
        .collect::<Vec<_>>();
    sections.sort_by(|a, b| (&a.name, &a.source).cmp(&(&b.name, &b.source)));
    sections
}
fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}
