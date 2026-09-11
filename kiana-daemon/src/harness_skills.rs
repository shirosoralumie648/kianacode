//! Inject project/user skills into the owned harness as context.
//!
//! Skills are not a fourth model-visible tool. The wrapper fills empty
//! `RunnerCommand::Start.instructions`; `KianaHarness` turns that into the
//! first System message. Project `.claude/skills` and `.kiana/skills` follow
//! the caller's trust flag.

use async_trait::async_trait;
use kiana_ports::{PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use kiana_skills::{load_all_skills_with_trust, Command as Skill};
use kiana_types::ProjectTrust;
use std::sync::Arc;

const SKILL_CONTENT_LIMIT: usize = 4000;

pub(crate) struct SkillAwareRunner {
    inner: Arc<dyn RunnerPort>,
}

impl SkillAwareRunner {
    pub(crate) fn wrap(inner: Arc<dyn RunnerPort>) -> Arc<dyn RunnerPort> {
        Arc::new(Self { inner })
    }
}

#[async_trait]
impl RunnerPort for SkillAwareRunner {
    async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        self.inner
            .send(with_skill_instructions(command).await)
            .await
    }

    async fn send_with_events(
        &self,
        command: RunnerCommand,
        on_event: &mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send),
    ) -> Result<Vec<RunnerEvent>, PortError> {
        self.inner
            .send_with_events(with_skill_instructions(command).await, on_event)
            .await
    }
}

async fn with_skill_instructions(command: RunnerCommand) -> RunnerCommand {
    match command {
        RunnerCommand::Start {
            run_id,
            prompt,
            history,
            project_root,
            sandbox,
            instructions,
            project_trusted,
            max_steps_per_turn,
        } if instructions.trim().is_empty() => RunnerCommand::Start {
            run_id,
            prompt,
            history,
            project_root: project_root.clone(),
            sandbox,
            instructions: load_skill_pack(&project_root, project_trusted).await,
            project_trusted,
            max_steps_per_turn,
        },
        other => other,
    }
}

async fn load_skill_pack(project_root: &str, project_trusted: bool) -> String {
    if project_root.trim().is_empty() {
        return String::new();
    }
    let trust = if project_trusted {
        ProjectTrust::Trusted
    } else {
        ProjectTrust::Untrusted
    };
    let skills = load_all_skills_with_trust(project_root, trust).await;
    format_skill_pack(&skills)
}

fn format_skill_pack(skills: &[Skill]) -> String {
    let mut body = String::new();
    for skill in skills {
        if skill.disable_model_invocation || skill.name.trim().is_empty() {
            continue;
        }
        body.push_str("\n## ");
        body.push_str(&skill.name);
        body.push('\n');
        body.push_str(&skill.description);
        body.push('\n');
        body.push_str(&truncate(&skill.content, SKILL_CONTENT_LIMIT));
        body.push('\n');
    }
    if body.is_empty() {
        String::new()
    } else {
        format!("Available skills:{body}")
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    text.chars().take(limit).collect()
}
