use serde::{Deserialize, Serialize};

/// A session ID uniquely identifies a Claude Code session.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(id: String) -> Self {
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for SessionId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

/// An agent ID uniquely identifies a subagent within a session.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(String);

impl AgentId {
    const PATTERN: &'static str = r"^a(?:.+-)?[0-9a-f]{16}$";

    pub fn new(id: String) -> Self {
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Validate and brand a string as AgentId.
    pub fn from_string(s: String) -> Option<Self> {
        let re = regex::Regex::new(Self::PATTERN).ok()?;
        if re.is_match(&s) {
            Some(Self(s))
        } else {
            None
        }
    }
}

impl From<String> for AgentId {
    fn from(id: String) -> Self {
        Self(id)
    }
}
