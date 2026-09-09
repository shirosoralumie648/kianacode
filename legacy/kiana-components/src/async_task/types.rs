//! Core types for async task management

use std::fmt;
use std::time::Instant;

/// Unique identifier for a task
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(u64);

impl TaskId {
    pub(crate) fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "task-{}", self.0)
    }
}

/// Task execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task is waiting to be executed
    Pending,
    /// Task is currently running
    Running,
    /// Task completed successfully
    Completed,
    /// Task failed with an error
    Failed,
    /// Task was cancelled
    Cancelled,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskStatus::Pending => write!(f, "pending"),
            TaskStatus::Running => write!(f, "running"),
            TaskStatus::Completed => write!(f, "completed"),
            TaskStatus::Failed => write!(f, "failed"),
            TaskStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl TaskStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }

    pub fn is_active(&self) -> bool {
        matches!(self, TaskStatus::Running)
    }
}

/// Task priority level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

impl Default for TaskPriority {
    fn default() -> Self {
        TaskPriority::Normal
    }
}

impl fmt::Display for TaskPriority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskPriority::Low => write!(f, "low"),
            TaskPriority::Normal => write!(f, "normal"),
            TaskPriority::High => write!(f, "high"),
            TaskPriority::Critical => write!(f, "critical"),
        }
    }
}

/// Task metadata
#[derive(Debug, Clone)]
pub struct TaskMetadata {
    pub id: TaskId,
    pub name: String,
    pub description: Option<String>,
    pub priority: TaskPriority,
    pub created_at: Instant,
    pub started_at: Option<Instant>,
    pub completed_at: Option<Instant>,
}

impl TaskMetadata {
    pub fn new(id: TaskId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            description: None,
            priority: TaskPriority::default(),
            created_at: Instant::now(),
            started_at: None,
            completed_at: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }

    pub fn duration(&self) -> Option<std::time::Duration> {
        match (self.started_at, self.completed_at) {
            (Some(start), Some(end)) => Some(end.duration_since(start)),
            _ => None,
        }
    }

    pub fn elapsed(&self) -> std::time::Duration {
        match self.started_at {
            Some(start) => Instant::now().duration_since(start),
            None => std::time::Duration::from_secs(0),
        }
    }
}

/// Task progress information
#[derive(Debug, Clone)]
pub struct TaskProgress {
    /// Current progress value
    pub current: u64,
    /// Total value (None for indeterminate progress)
    pub total: Option<u64>,
    /// Optional progress message
    pub message: Option<String>,
    /// Computed percentage (0.0 - 100.0)
    pub percentage: Option<f32>,
}

impl TaskProgress {
    /// Create a new determinate progress
    pub fn new(current: u64, total: u64) -> Self {
        let percentage = if total > 0 {
            Some((current as f32 / total as f32) * 100.0)
        } else {
            Some(100.0)
        };

        Self {
            current,
            total: Some(total),
            message: None,
            percentage,
        }
    }

    /// Create an indeterminate progress
    pub fn indeterminate() -> Self {
        Self {
            current: 0,
            total: None,
            message: None,
            percentage: None,
        }
    }

    /// Create a completed progress
    pub fn completed() -> Self {
        Self {
            current: 100,
            total: Some(100),
            message: None,
            percentage: Some(100.0),
        }
    }

    /// Add a message to the progress
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Update current value and recalculate percentage
    pub fn update(&mut self, current: u64) {
        self.current = current;
        if let Some(total) = self.total {
            self.percentage = if total > 0 {
                Some((current as f32 / total as f32) * 100.0)
            } else {
                Some(100.0)
            };
        }
    }

    /// Check if progress is complete
    pub fn is_complete(&self) -> bool {
        match (self.current, self.total) {
            (_, Some(total)) => self.current >= total,
            _ => false,
        }
    }
}

impl Default for TaskProgress {
    fn default() -> Self {
        Self::indeterminate()
    }
}

impl fmt::Display for TaskProgress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.total, &self.message) {
            (Some(total), Some(msg)) => {
                write!(
                    f,
                    "{}/{} ({:.1}%) - {}",
                    self.current,
                    total,
                    self.percentage.unwrap_or(0.0),
                    msg
                )
            }
            (Some(total), None) => {
                write!(
                    f,
                    "{}/{} ({:.1}%)",
                    self.current,
                    total,
                    self.percentage.unwrap_or(0.0)
                )
            }
            (None, Some(msg)) => write!(f, "{}", msg),
            (None, None) => write!(f, "in progress..."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_id() {
        let id = TaskId::new(42);
        assert_eq!(id.as_u64(), 42);
        assert_eq!(id.to_string(), "task-42");
    }

    #[test]
    fn test_task_status() {
        assert!(TaskStatus::Completed.is_terminal());
        assert!(TaskStatus::Failed.is_terminal());
        assert!(TaskStatus::Cancelled.is_terminal());
        assert!(!TaskStatus::Running.is_terminal());
        assert!(!TaskStatus::Pending.is_terminal());

        assert!(TaskStatus::Running.is_active());
        assert!(!TaskStatus::Completed.is_active());
    }

    #[test]
    fn test_task_priority_ordering() {
        assert!(TaskPriority::Critical > TaskPriority::High);
        assert!(TaskPriority::High > TaskPriority::Normal);
        assert!(TaskPriority::Normal > TaskPriority::Low);
    }

    #[test]
    fn test_task_metadata() {
        let meta = TaskMetadata::new(TaskId::new(1), "test-task")
            .with_description("A test task")
            .with_priority(TaskPriority::High);

        assert_eq!(meta.id.as_u64(), 1);
        assert_eq!(meta.name, "test-task");
        assert_eq!(meta.description, Some("A test task".to_string()));
        assert_eq!(meta.priority, TaskPriority::High);
    }

    #[test]
    fn test_task_progress_determinate() {
        let mut progress = TaskProgress::new(50, 100);
        assert_eq!(progress.current, 50);
        assert_eq!(progress.total, Some(100));
        assert_eq!(progress.percentage, Some(50.0));
        assert!(!progress.is_complete());

        progress.update(100);
        assert_eq!(progress.percentage, Some(100.0));
        assert!(progress.is_complete());
    }

    #[test]
    fn test_task_progress_indeterminate() {
        let progress = TaskProgress::indeterminate().with_message("Processing...");

        assert_eq!(progress.total, None);
        assert_eq!(progress.percentage, None);
        assert_eq!(progress.message, Some("Processing...".to_string()));
        assert!(!progress.is_complete());
    }

    #[test]
    fn test_task_progress_completed() {
        let progress = TaskProgress::completed();
        assert!(progress.is_complete());
        assert_eq!(progress.percentage, Some(100.0));
    }
}
