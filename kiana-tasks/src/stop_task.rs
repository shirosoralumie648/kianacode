use crate::types::{TaskState, TaskStatus};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub enum StopTaskError {
    NotFound(String),
    NotRunning { task_id: String, status: TaskStatus },
    UnsupportedType(String),
}

impl std::fmt::Display for StopTaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StopTaskError::NotFound(id) => write!(f, "No task found with ID: {}", id),
            StopTaskError::NotRunning { task_id, status } => {
                write!(f, "Task {} is not running (status: {:?})", task_id, status)
            }
            StopTaskError::UnsupportedType(t) => write!(f, "Unsupported task type: {}", t),
        }
    }
}

impl std::error::Error for StopTaskError {}

#[async_trait]
pub trait TaskKiller: Send + Sync {
    async fn kill(&self, task_id: &str) -> Result<(), Box<dyn std::error::Error>>;
}

pub struct StopTaskResult {
    pub task_id: String,
    pub task_type: String,
    pub command: Option<String>,
}

pub struct TaskRegistry {
    tasks: Arc<RwLock<HashMap<String, TaskState>>>,
    killers: Arc<RwLock<HashMap<String, Arc<dyn TaskKiller>>>>,
}

impl TaskRegistry {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            killers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register_task(&self, task: TaskState) {
        let task_id = task.base().id.clone();
        self.tasks.write().await.insert(task_id, task);
    }

    pub async fn register_killer(&self, task_type: String, killer: Arc<dyn TaskKiller>) {
        self.killers.write().await.insert(task_type, killer);
    }

    pub async fn get_task(&self, task_id: &str) -> Option<TaskState> {
        self.tasks.read().await.get(task_id).cloned()
    }

    pub async fn update_task<F>(&self, task_id: &str, f: F)
    where
        F: FnOnce(&mut TaskState),
    {
        if let Some(task) = self.tasks.write().await.get_mut(task_id) {
            f(task);
        }
    }

    pub async fn stop_task(&self, task_id: &str) -> Result<StopTaskResult, StopTaskError> {
        let task = self
            .get_task(task_id)
            .await
            .ok_or_else(|| StopTaskError::NotFound(task_id.to_string()))?;

        let base = task.base();
        if base.status != TaskStatus::Running {
            return Err(StopTaskError::NotRunning {
                task_id: task_id.to_string(),
                status: base.status,
            });
        }

        let task_type = format!("{:?}", base.task_type);
        let killer = self
            .killers
            .read()
            .await
            .get(&task_type)
            .cloned()
            .ok_or_else(|| StopTaskError::UnsupportedType(task_type.clone()))?;

        killer
            .kill(task_id)
            .await
            .map_err(|e| StopTaskError::UnsupportedType(format!("Kill failed: {}", e)))?;

        let command = match &task {
            TaskState::LocalBash(s) => Some(s.command.clone()),
            _ => Some(base.description.clone()),
        };

        Ok(StopTaskResult {
            task_id: task_id.to_string(),
            task_type,
            command,
        })
    }
}

impl Default for TaskRegistry {
    fn default() -> Self {
        Self::new()
    }
}
