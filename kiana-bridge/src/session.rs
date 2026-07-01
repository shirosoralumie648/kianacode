use crate::types::{SessionActivity, SessionHandle};
use std::collections::HashMap;
use tokio::sync::RwLock;

pub struct SessionManager {
    sessions: RwLock<HashMap<String, SessionState>>,
}

struct SessionState {
    handle: SessionHandle,
    start_time: std::time::Instant,
    activities: Vec<SessionActivity>,
}

#[derive(Debug, Clone)]
pub struct SessionStatus {
    pub handle: SessionHandle,
    pub uptime_ms: u128,
    pub activities: Vec<SessionActivity>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub async fn add_session(&self, session_id: String, handle: SessionHandle) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(
            session_id,
            SessionState {
                handle,
                start_time: std::time::Instant::now(),
                activities: Vec::new(),
            },
        );
    }

    pub async fn get_session(&self, session_id: &str) -> Option<SessionHandle> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).map(|s| s.handle.clone())
    }

    pub async fn update_access_token(
        &self,
        session_id: &str,
        access_token: String,
    ) -> Option<SessionHandle> {
        let mut sessions = self.sessions.write().await;
        let state = sessions.get_mut(session_id)?;
        state.handle.access_token = access_token;
        Some(state.handle.clone())
    }

    pub async fn update_credentials(
        &self,
        session_id: &str,
        access_token: String,
        worker_epoch: Option<u64>,
    ) -> Option<SessionHandle> {
        let mut sessions = self.sessions.write().await;
        let state = sessions.get_mut(session_id)?;
        state.handle.access_token = access_token;
        if let Some(worker_epoch) = worker_epoch {
            state.handle.use_ccr_v2 = true;
            state.handle.worker_epoch = Some(worker_epoch);
        }
        Some(state.handle.clone())
    }

    pub async fn remove_session(&self, session_id: &str) -> Option<SessionHandle> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id).map(|s| s.handle)
    }

    pub async fn add_activity(&self, session_id: &str, activity: SessionActivity) {
        let mut sessions = self.sessions.write().await;
        if let Some(state) = sessions.get_mut(session_id) {
            state.activities.push(activity);
            if state.activities.len() > 10 {
                state.activities.remove(0);
            }
        }
    }

    pub async fn status(&self, session_id: &str) -> Option<SessionStatus> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).map(|state| SessionStatus {
            handle: state.handle.clone(),
            uptime_ms: state.start_time.elapsed().as_millis(),
            activities: state.activities.clone(),
        })
    }

    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}
