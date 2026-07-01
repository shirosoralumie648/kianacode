use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub type SessionId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ChannelEntry {
    #[serde(rename = "plugin")]
    Plugin {
        name: String,
        marketplace: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        dev: Option<bool>,
    },
    #[serde(rename = "server")]
    Server {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        dev: Option<bool>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub web_search_requests: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCronTask {
    pub id: String,
    pub cron: String,
    pub prompt: String,
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurring: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvokedSkillInfo {
    pub skill_name: String,
    pub skill_path: String,
    pub content: String,
    pub invoked_at: u64,
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlowOperation {
    pub operation: String,
    pub duration_ms: u64,
    pub timestamp: u64,
}

#[derive(Debug)]
pub struct State {
    pub original_cwd: PathBuf,
    pub project_root: PathBuf,
    pub total_cost_usd: f64,
    pub total_api_duration: u64,
    pub total_api_duration_without_retries: u64,
    pub total_tool_duration: u64,
    pub turn_hook_duration_ms: u64,
    pub turn_tool_duration_ms: u64,
    pub turn_classifier_duration_ms: u64,
    pub turn_tool_count: u32,
    pub turn_hook_count: u32,
    pub turn_classifier_count: u32,
    pub start_time: u64,
    pub last_interaction_time: u64,
    pub total_lines_added: u64,
    pub total_lines_removed: u64,
    pub has_unknown_model_cost: bool,
    pub cwd: PathBuf,
    pub model_usage: HashMap<String, ModelUsage>,
    pub is_interactive: bool,
    pub kairos_active: bool,
    pub strict_tool_result_pairing: bool,
    pub sdk_agent_progress_summaries_enabled: bool,
    pub user_msg_opt_in: bool,
    pub client_type: String,
    pub session_source: Option<String>,
    pub session_id: SessionId,
    pub parent_session_id: Option<SessionId>,
    pub session_bypass_permissions_mode: bool,
    pub scheduled_tasks_enabled: bool,
    pub session_cron_tasks: Vec<SessionCronTask>,
    pub session_created_teams: HashSet<String>,
    pub session_trust_accepted: bool,
    pub session_persistence_disabled: bool,
    pub has_exited_plan_mode: bool,
    pub needs_plan_mode_exit_attachment: bool,
    pub needs_auto_mode_exit_attachment: bool,
    pub lsp_recommendation_shown_this_session: bool,
    pub plan_slug_cache: HashMap<String, String>,
    pub invoked_skills: HashMap<String, InvokedSkillInfo>,
    pub slow_operations: Vec<SlowOperation>,
    pub main_thread_agent_type: Option<String>,
    pub is_remote_mode: bool,
    pub direct_connect_server_url: Option<String>,
    pub system_prompt_section_cache: HashMap<String, Option<String>>,
    pub last_emitted_date: Option<String>,
    pub additional_directories_for_claude_md: Vec<String>,
    pub allowed_channels: Vec<ChannelEntry>,
    pub has_dev_channels: bool,
    pub session_project_dir: Option<PathBuf>,
    pub prompt_cache_1h_allowlist: Option<Vec<String>>,
    pub prompt_cache_1h_eligible: Option<bool>,
    pub afk_mode_header_latched: Option<bool>,
    pub fast_mode_header_latched: Option<bool>,
    pub cache_editing_header_latched: Option<bool>,
    pub thinking_clear_latched: Option<bool>,
    pub prompt_id: Option<String>,
    pub last_main_request_id: Option<String>,
    pub last_api_completion_timestamp: Option<u64>,
    pub pending_post_compaction: bool,
}

impl Default for State {
    fn default() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        Self {
            original_cwd: cwd.clone(),
            project_root: cwd.clone(),
            total_cost_usd: 0.0,
            total_api_duration: 0,
            total_api_duration_without_retries: 0,
            total_tool_duration: 0,
            turn_hook_duration_ms: 0,
            turn_tool_duration_ms: 0,
            turn_classifier_duration_ms: 0,
            turn_tool_count: 0,
            turn_hook_count: 0,
            turn_classifier_count: 0,
            start_time: now,
            last_interaction_time: now,
            total_lines_added: 0,
            total_lines_removed: 0,
            has_unknown_model_cost: false,
            cwd,
            model_usage: HashMap::new(),
            is_interactive: false,
            kairos_active: false,
            strict_tool_result_pairing: false,
            sdk_agent_progress_summaries_enabled: false,
            user_msg_opt_in: false,
            client_type: "cli".to_string(),
            session_source: None,
            session_id: Uuid::new_v4().to_string(),
            parent_session_id: None,
            session_bypass_permissions_mode: false,
            scheduled_tasks_enabled: false,
            session_cron_tasks: Vec::new(),
            session_created_teams: HashSet::new(),
            session_trust_accepted: false,
            session_persistence_disabled: false,
            has_exited_plan_mode: false,
            needs_plan_mode_exit_attachment: false,
            needs_auto_mode_exit_attachment: false,
            lsp_recommendation_shown_this_session: false,
            plan_slug_cache: HashMap::new(),
            invoked_skills: HashMap::new(),
            slow_operations: Vec::new(),
            main_thread_agent_type: None,
            is_remote_mode: false,
            direct_connect_server_url: None,
            system_prompt_section_cache: HashMap::new(),
            last_emitted_date: None,
            additional_directories_for_claude_md: Vec::new(),
            allowed_channels: Vec::new(),
            has_dev_channels: false,
            session_project_dir: None,
            prompt_cache_1h_allowlist: None,
            prompt_cache_1h_eligible: None,
            afk_mode_header_latched: None,
            fast_mode_header_latched: None,
            cache_editing_header_latched: None,
            thinking_clear_latched: None,
            prompt_id: None,
            last_main_request_id: None,
            last_api_completion_timestamp: None,
            pending_post_compaction: false,
        }
    }
}

pub struct GlobalState {
    state: Arc<Mutex<State>>,
}

impl GlobalState {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    pub fn get_session_id(&self) -> SessionId {
        self.state.lock().unwrap().session_id.clone()
    }

    pub fn regenerate_session_id(&self, set_current_as_parent: bool) -> SessionId {
        let mut state = self.state.lock().unwrap();
        if set_current_as_parent {
            state.parent_session_id = Some(state.session_id.clone());
        }
        let old_session_id = state.session_id.clone();
        state.plan_slug_cache.remove(&old_session_id);
        state.session_id = Uuid::new_v4().to_string();
        state.session_project_dir = None;
        state.session_id.clone()
    }

    pub fn switch_session(&self, session_id: SessionId, project_dir: Option<PathBuf>) {
        let mut state = self.state.lock().unwrap();
        let old_session_id = state.session_id.clone();
        state.plan_slug_cache.remove(&old_session_id);
        state.session_id = session_id;
        state.session_project_dir = project_dir;
    }

    pub fn get_original_cwd(&self) -> PathBuf {
        self.state.lock().unwrap().original_cwd.clone()
    }

    pub fn get_project_root(&self) -> PathBuf {
        self.state.lock().unwrap().project_root.clone()
    }

    pub fn set_original_cwd(&self, cwd: PathBuf) {
        self.state.lock().unwrap().original_cwd = cwd;
    }

    pub fn set_project_root(&self, root: PathBuf) {
        self.state.lock().unwrap().project_root = root;
    }

    pub fn get_cwd(&self) -> PathBuf {
        self.state.lock().unwrap().cwd.clone()
    }

    pub fn set_cwd(&self, cwd: PathBuf) {
        self.state.lock().unwrap().cwd = cwd;
    }

    pub fn add_to_total_duration(&self, duration: u64, duration_without_retries: u64) {
        let mut state = self.state.lock().unwrap();
        state.total_api_duration += duration;
        state.total_api_duration_without_retries += duration_without_retries;
    }

    pub fn add_to_total_cost(&self, cost: f64, usage: ModelUsage, model: String) {
        let mut state = self.state.lock().unwrap();
        state.model_usage.insert(model, usage);
        state.total_cost_usd += cost;
    }

    pub fn get_total_cost_usd(&self) -> f64 {
        self.state.lock().unwrap().total_cost_usd
    }

    pub fn get_total_api_duration(&self) -> u64 {
        self.state.lock().unwrap().total_api_duration
    }

    pub fn get_total_duration(&self) -> u64 {
        let state = self.state.lock().unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        now - state.start_time
    }

    pub fn add_to_tool_duration(&self, duration: u64) {
        let mut state = self.state.lock().unwrap();
        state.total_tool_duration += duration;
        state.turn_tool_duration_ms += duration;
        state.turn_tool_count += 1;
    }

    pub fn add_to_turn_hook_duration(&self, duration: u64) {
        let mut state = self.state.lock().unwrap();
        state.turn_hook_duration_ms += duration;
        state.turn_hook_count += 1;
    }

    pub fn reset_turn_hook_duration(&self) {
        let mut state = self.state.lock().unwrap();
        state.turn_hook_duration_ms = 0;
        state.turn_hook_count = 0;
    }

    pub fn reset_turn_tool_duration(&self) {
        let mut state = self.state.lock().unwrap();
        state.turn_tool_duration_ms = 0;
        state.turn_tool_count = 0;
    }

    pub fn add_to_total_lines_changed(&self, added: u64, removed: u64) {
        let mut state = self.state.lock().unwrap();
        state.total_lines_added += added;
        state.total_lines_removed += removed;
    }

    pub fn get_total_input_tokens(&self) -> u64 {
        self.state
            .lock()
            .unwrap()
            .model_usage
            .values()
            .map(|u| u.input_tokens)
            .sum()
    }

    pub fn get_total_output_tokens(&self) -> u64 {
        self.state
            .lock()
            .unwrap()
            .model_usage
            .values()
            .map(|u| u.output_tokens)
            .sum()
    }

    pub fn set_is_interactive(&self, value: bool) {
        self.state.lock().unwrap().is_interactive = value;
    }

    pub fn get_is_interactive(&self) -> bool {
        self.state.lock().unwrap().is_interactive
    }

    pub fn add_invoked_skill(
        &self,
        skill_name: String,
        skill_path: String,
        content: String,
        agent_id: Option<String>,
    ) {
        let mut state = self.state.lock().unwrap();
        let key = format!("{}:{}", agent_id.as_deref().unwrap_or(""), skill_name);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        state.invoked_skills.insert(
            key,
            InvokedSkillInfo {
                skill_name,
                skill_path,
                content,
                invoked_at: now,
                agent_id,
            },
        );
    }

    pub fn clear_invoked_skills(&self, preserved_agent_ids: Option<&HashSet<String>>) {
        let mut state = self.state.lock().unwrap();
        if let Some(preserved) = preserved_agent_ids {
            if preserved.is_empty() {
                state.invoked_skills.clear();
                return;
            }
            state.invoked_skills.retain(|_, skill| {
                skill
                    .agent_id
                    .as_ref()
                    .map_or(false, |id| preserved.contains(id))
            });
        } else {
            state.invoked_skills.clear();
        }
    }

    pub fn mark_post_compaction(&self) {
        self.state.lock().unwrap().pending_post_compaction = true;
    }

    pub fn consume_post_compaction(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        let was = state.pending_post_compaction;
        state.pending_post_compaction = false;
        was
    }

    pub fn reset_cost_state(&self) {
        let mut state = self.state.lock().unwrap();
        state.total_cost_usd = 0.0;
        state.total_api_duration = 0;
        state.total_api_duration_without_retries = 0;
        state.total_tool_duration = 0;
        state.start_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        state.total_lines_added = 0;
        state.total_lines_removed = 0;
        state.has_unknown_model_cost = false;
        state.model_usage.clear();
        state.prompt_id = None;
    }

    pub fn clear_beta_header_latches(&self) {
        let mut state = self.state.lock().unwrap();
        state.afk_mode_header_latched = None;
        state.fast_mode_header_latched = None;
        state.cache_editing_header_latched = None;
        state.thinking_clear_latched = None;
    }
}

impl Default for GlobalState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_id_generation() {
        let state = GlobalState::new();
        let id1 = state.get_session_id();
        let id2 = state.regenerate_session_id(false);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_cost_tracking() {
        let state = GlobalState::new();
        let usage = ModelUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 10,
            cache_creation_input_tokens: 5,
            web_search_requests: 2,
        };
        state.add_to_total_cost(1.5, usage, "test-model".to_string());
        assert_eq!(state.get_total_cost_usd(), 1.5);
        assert_eq!(state.get_total_input_tokens(), 100);
        assert_eq!(state.get_total_output_tokens(), 50);
    }

    #[test]
    fn test_invoked_skills() {
        let state = GlobalState::new();
        state.add_invoked_skill(
            "test_skill".to_string(),
            "/path/to/skill".to_string(),
            "content".to_string(),
            None,
        );
        assert_eq!(state.state.lock().unwrap().invoked_skills.len(), 1);
    }

    #[test]
    fn test_post_compaction_flag() {
        let state = GlobalState::new();
        assert!(!state.consume_post_compaction());
        state.mark_post_compaction();
        assert!(state.consume_post_compaction());
        assert!(!state.consume_post_compaction());
    }
}
