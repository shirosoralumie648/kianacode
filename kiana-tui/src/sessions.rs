use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

impl Message {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            timestamp: Utc::now(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub messages: Vec<Message>,
    pub acp_session_id: Option<String>,
}

impl Session {
    pub fn new(id: String) -> Self {
        Self {
            id,
            created_at: Utc::now(),
            messages: Vec::new(),
            acp_session_id: None,
        }
    }

    pub fn fork(&self, new_id: String) -> Self {
        Self {
            id: new_id,
            created_at: Utc::now(),
            messages: self.messages.clone(),
            acp_session_id: None, // Forked session needs new ACP session
        }
    }
}

pub struct SessionManager {
    current: Session,
    history: HashMap<String, Session>,
    sessions_dir: PathBuf,
}

impl SessionManager {
    pub fn new() -> Result<Self> {
        let sessions_dir = dirs::home_dir()
            .context("Failed to get home directory")?
            .join(".kiana")
            .join("sessions");

        fs::create_dir_all(&sessions_dir)?;

        let initial_id = Self::generate_id();
        let current = Session::new(initial_id.clone());

        let mut manager = Self {
            current,
            history: HashMap::new(),
            sessions_dir,
        };

        // Load existing sessions
        manager.load_sessions()?;

        Ok(manager)
    }

    fn generate_id() -> String {
        let now = Utc::now();
        format!("session_{}", now.format("%Y%m%d_%H%M%S"))
    }

    fn session_path(&self, id: &str) -> PathBuf {
        self.sessions_dir.join(format!("{}.json", id))
    }

    fn load_sessions(&mut self) -> Result<()> {
        let entries = fs::read_dir(&self.sessions_dir)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(contents) = fs::read_to_string(&path) {
                    if let Ok(session) = serde_json::from_str::<Session>(&contents) {
                        self.history.insert(session.id.clone(), session);
                    }
                }
            }
        }

        Ok(())
    }

    fn save_session(&self, session: &Session) -> Result<()> {
        let path = self.session_path(&session.id);
        let contents = serde_json::to_string_pretty(session)?;
        fs::write(path, contents)?;
        Ok(())
    }

    pub fn new_session(&mut self) -> Result<String> {
        // Save current session
        self.save_session(&self.current)?;
        self.history.insert(self.current.id.clone(), self.current.clone());

        // Create new session
        let new_id = Self::generate_id();
        self.current = Session::new(new_id.clone());
        self.save_session(&self.current)?;

        Ok(new_id)
    }

    pub fn fork_session(&mut self) -> Result<String> {
        // Save current session
        self.save_session(&self.current)?;

        // Fork current session
        let new_id = format!("{}_fork", Self::generate_id());
        let forked = self.current.fork(new_id.clone());

        self.history.insert(self.current.id.clone(), self.current.clone());
        self.current = forked;
        self.save_session(&self.current)?;

        Ok(new_id)
    }

    pub fn switch_session(&mut self, id: &str) -> Result<()> {
        // Check if session exists
        if id == self.current.id {
            anyhow::bail!("Already in session {}", id);
        }

        let session = if let Some(sess) = self.history.get(id) {
            sess.clone()
        } else {
            // Try loading from disk if not in memory
            let path = self.session_path(id);
            let contents = fs::read_to_string(&path)
                .context("Session not found")?;
            serde_json::from_str::<Session>(&contents)
                .context("Failed to parse session")?
        };

        // Save current session
        self.save_session(&self.current)?;
        self.history.insert(self.current.id.clone(), self.current.clone());

        // Switch to new session
        self.current = session;

        Ok(())
    }

    pub fn list_sessions(&self) -> Vec<(String, DateTime<Utc>)> {
        let mut sessions: Vec<_> = self.history
            .values()
            .map(|s| (s.id.clone(), s.created_at))
            .collect();

        // Add current session
        sessions.push((self.current.id.clone(), self.current.created_at));

        // Sort by creation time (newest first)
        sessions.sort_by(|a, b| b.1.cmp(&a.1));

        sessions
    }

    pub fn current_session(&self) -> &Session {
        &self.current
    }

    pub fn current_session_mut(&mut self) -> &mut Session {
        &mut self.current
    }

    pub fn save_current(&self) -> Result<()> {
        self.save_session(&self.current)
    }
}
