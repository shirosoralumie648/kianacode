//! Priority and queue configuration types.

use std::collections::HashMap;
use std::time::Duration;

/// Task priority levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    /// Idle priority - only runs when system is idle.
    Idle = 0,
    /// Low priority - background tasks.
    Low = 1,
    /// Normal priority - regular tasks.
    Normal = 2,
    /// High priority - important tasks.
    High = 3,
    /// Critical priority - user-facing tasks.
    Critical = 4,
}

impl Default for Priority {
    fn default() -> Self {
        Self::Normal
    }
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Priority::Idle => write!(f, "Idle"),
            Priority::Low => write!(f, "Low"),
            Priority::Normal => write!(f, "Normal"),
            Priority::High => write!(f, "High"),
            Priority::Critical => write!(f, "Critical"),
        }
    }
}

/// Configuration for the task queue.
#[derive(Debug, Clone)]
pub struct QueueConfig {
    /// Maximum number of concurrent tasks.
    pub max_concurrent: usize,
    /// Maximum tasks per priority level (None = unlimited).
    pub max_per_priority: Option<usize>,
    /// Enable priority aging (prevent starvation).
    pub enable_aging: bool,
    /// How often to check for aging.
    pub aging_interval: Duration,
    /// How long a task must wait before aging starts.
    pub aging_threshold: Duration,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 10,
            max_per_priority: None,
            enable_aging: true,
            aging_interval: Duration::from_secs(30),
            aging_threshold: Duration::from_secs(60),
        }
    }
}

impl QueueConfig {
    /// Creates a new queue configuration.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            max_concurrent,
            ..Default::default()
        }
    }

    /// Sets the maximum tasks per priority level.
    pub fn with_max_per_priority(mut self, max: usize) -> Self {
        self.max_per_priority = Some(max);
        self
    }

    /// Sets whether to enable priority aging.
    pub fn with_aging(mut self, enable: bool) -> Self {
        self.enable_aging = enable;
        self
    }
}

/// Options for submitting a task.
#[derive(Debug, Clone)]
pub struct SubmitOptions {
    /// Task priority.
    pub priority: Priority,
    /// Delay before task can be scheduled.
    pub delay: Option<Duration>,
    /// IDs of tasks this task depends on.
    pub dependencies: Vec<crate::background::TaskId>,
    /// Optional task group (tasks in same group run serially).
    pub group: Option<String>,
}

impl Default for SubmitOptions {
    fn default() -> Self {
        Self {
            priority: Priority::Normal,
            delay: None,
            dependencies: Vec::new(),
            group: None,
        }
    }
}

impl SubmitOptions {
    /// Creates new submit options with the given priority.
    pub fn new(priority: Priority) -> Self {
        Self {
            priority,
            ..Default::default()
        }
    }

    /// Sets the delay.
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    /// Adds a dependency.
    pub fn with_dependency(mut self, task_id: crate::background::TaskId) -> Self {
        self.dependencies.push(task_id);
        self
    }

    /// Sets the task group.
    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }
}

/// Statistics about the task queue.
#[derive(Debug, Clone)]
pub struct QueueStats {
    /// Total number of queued tasks.
    pub total_queued: usize,
    /// Tasks per priority level.
    pub by_priority: HashMap<Priority, usize>,
    /// Number of currently running tasks.
    pub running: usize,
    /// Average wait time.
    pub avg_wait_time: Duration,
    /// Total completed tasks.
    pub total_completed: u64,
    /// Total failed tasks.
    pub total_failed: u64,
}

impl Default for QueueStats {
    fn default() -> Self {
        Self {
            total_queued: 0,
            by_priority: HashMap::new(),
            running: 0,
            avg_wait_time: Duration::ZERO,
            total_completed: 0,
            total_failed: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Critical > Priority::High);
        assert!(Priority::High > Priority::Normal);
        assert!(Priority::Normal > Priority::Low);
        assert!(Priority::Low > Priority::Idle);
    }

    #[test]
    fn test_queue_config() {
        let config = QueueConfig::new(5)
            .with_max_per_priority(10)
            .with_aging(false);

        assert_eq!(config.max_concurrent, 5);
        assert_eq!(config.max_per_priority, Some(10));
        assert!(!config.enable_aging);
    }

    #[test]
    fn test_submit_options() {
        let task_id = crate::background::TaskId::new();
        let options = SubmitOptions::new(Priority::High)
            .with_delay(Duration::from_secs(5))
            .with_dependency(task_id)
            .with_group("test");

        assert_eq!(options.priority, Priority::High);
        assert_eq!(options.delay, Some(Duration::from_secs(5)));
        assert_eq!(options.dependencies.len(), 1);
        assert_eq!(options.group, Some("test".to_string()));
    }
}
