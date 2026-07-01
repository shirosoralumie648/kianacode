use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsEvent {
    pub event_name: String,
    pub metadata: HashMap<String, serde_json::Value>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

pub trait AnalyticsSink: Send + Sync {
    fn log_event(&self, event_name: &str, metadata: HashMap<String, serde_json::Value>);
    fn log_event_async(
        &self,
        event_name: &str,
        metadata: HashMap<String, serde_json::Value>,
    ) -> impl std::future::Future<Output = ()> + Send;
}

pub struct NoOpSink;

impl AnalyticsSink for NoOpSink {
    fn log_event(&self, _event_name: &str, _metadata: HashMap<String, serde_json::Value>) {}

    async fn log_event_async(
        &self,
        _event_name: &str,
        _metadata: HashMap<String, serde_json::Value>,
    ) {
    }
}

pub struct EventQueue {
    events: tokio::sync::Mutex<Vec<AnalyticsEvent>>,
}

impl EventQueue {
    pub fn new() -> Self {
        Self {
            events: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    pub async fn push(&self, event: AnalyticsEvent) {
        self.events.lock().await.push(event);
    }

    pub async fn drain(&self) -> Vec<AnalyticsEvent> {
        std::mem::take(&mut *self.events.lock().await)
    }
}

impl Default for EventQueue {
    fn default() -> Self {
        Self::new()
    }
}
