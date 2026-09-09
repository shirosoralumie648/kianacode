//! Task queue implementation.

use super::dependency::DependencyGraph;
use super::types::{Priority, QueueConfig, QueueStats, SubmitOptions};
use crate::background::{BackgroundTask, TaskExecutor, TaskId};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;

/// Information about a queued task.
struct QueuedTaskInfo {
    id: TaskId,
    priority: Priority,
    submitted_at: Instant,
    scheduled_at: Option<Instant>,
    dependencies: Vec<TaskId>,
    group: Option<String>,
}

/// A priority-based task queue with dependency support.
pub struct TaskQueue {
    config: QueueConfig,
    // Priority queues (one per priority level)
    queues: BTreeMap<Priority, VecDeque<QueuedTaskInfo>>,
    // Currently running tasks
    running: HashMap<TaskId, Instant>,
    // Completed tasks
    completed: HashSet<TaskId>,
    // Failed tasks
    failed: HashSet<TaskId>,
    // Dependency graph
    dependencies: DependencyGraph,
    // Task executor
    executor: Arc<TaskExecutor>,
    // Statistics
    stats: QueueStats,
    // Active task groups (group name -> task ID)
    active_groups: HashMap<String, TaskId>,
}

impl TaskQueue {
    /// Creates a new task queue.
    pub fn new(config: QueueConfig) -> Self {
        Self {
            config,
            queues: BTreeMap::new(),
            running: HashMap::new(),
            completed: HashSet::new(),
            failed: HashSet::new(),
            dependencies: DependencyGraph::new(),
            executor: Arc::new(TaskExecutor::new()),
            stats: QueueStats::default(),
            active_groups: HashMap::new(),
        }
    }

    /// Submits a task with the given priority.
    pub fn submit<T: Send + 'static>(
        &mut self,
        task: impl BackgroundTask<Output = T> + 'static,
        priority: Priority,
    ) -> TaskId {
        self.submit_with_options(
            task,
            SubmitOptions {
                priority,
                ..Default::default()
            },
        )
    }

    /// Submits a task with custom options.
    pub fn submit_with_options<T: Send + 'static>(
        &mut self,
        task: impl BackgroundTask<Output = T> + 'static,
        options: SubmitOptions,
    ) -> TaskId {
        // Scheduling currently tracks metadata and dependencies. Execution is
        // driven later via mark_completed/mark_failed; keep the task bound so
        // callers can still submit BackgroundTask values.
        let _task = task;
        let id = TaskId::new();

        // Add dependencies to graph
        for &dep in &options.dependencies {
            self.dependencies.add_dependency(id, dep);
        }

        // Create queued task info
        let info = QueuedTaskInfo {
            id,
            priority: options.priority,
            submitted_at: Instant::now(),
            scheduled_at: options.delay.map(|d| Instant::now() + d),
            dependencies: options.dependencies,
            group: options.group,
        };

        // Add to appropriate priority queue
        self.queues
            .entry(options.priority)
            .or_insert_with(VecDeque::new)
            .push_back(info);

        // Update stats
        self.stats.total_queued += 1;
        *self.stats.by_priority.entry(options.priority).or_insert(0) += 1;

        // Try to schedule tasks
        self.schedule_next();

        id
    }

    /// Cancels a queued or running task.
    pub fn cancel(&mut self, id: TaskId) -> anyhow::Result<()> {
        // Try to remove from queues
        for queue in self.queues.values_mut() {
            if let Some(pos) = queue.iter().position(|info| info.id == id) {
                queue.remove(pos);
                self.dependencies.remove_task(id);
                return Ok(());
            }
        }

        // Try to cancel running task
        self.executor.cancel_task(id)
    }

    /// Changes the priority of a queued task.
    pub fn set_priority(&mut self, id: TaskId, new_priority: Priority) -> anyhow::Result<()> {
        // Find and remove task from current queue
        let mut task_info = None;
        for (priority, queue) in &mut self.queues {
            if let Some(pos) = queue.iter().position(|info| info.id == id) {
                task_info = Some((queue.remove(pos).unwrap(), *priority));
                break;
            }
        }

        if let Some((mut info, old_priority)) = task_info {
            // Update priority
            info.priority = new_priority;

            // Add to new queue
            self.queues
                .entry(new_priority)
                .or_insert_with(VecDeque::new)
                .push_back(info);

            // Update stats
            *self.stats.by_priority.entry(old_priority).or_insert(0) -= 1;
            *self.stats.by_priority.entry(new_priority).or_insert(0) += 1;

            Ok(())
        } else {
            anyhow::bail!("Task not found in queue")
        }
    }

    /// Gets current queue statistics.
    pub fn stats(&self) -> QueueStats {
        let mut stats = self.stats.clone();
        stats.running = self.running.len();
        stats.total_queued = self.queues.values().map(|q| q.len()).sum();
        stats
    }

    /// Clears all queued tasks (does not affect running tasks).
    pub fn clear(&mut self) {
        self.queues.clear();
        self.stats.by_priority.clear();
        self.stats.total_queued = 0;
    }

    /// Schedules the next batch of tasks.
    fn schedule_next(&mut self) {
        while self.running.len() < self.config.max_concurrent {
            if let Some(info) = self.select_next_task() {
                // Start the task
                // Note: In a real implementation, we'd spawn the actual task here
                self.running.insert(info.id, Instant::now());
                self.dependencies.remove_task(info.id);
            } else {
                break;
            }
        }
    }

    /// Selects the next task to run based on priority and dependencies.
    fn select_next_task(&mut self) -> Option<QueuedTaskInfo> {
        let now = Instant::now();

        // Iterate through priorities from highest to lowest
        for (_, queue) in self.queues.iter_mut().rev() {
            let mut i = 0;
            while i < queue.len() {
                let info = &queue[i];

                // Check if scheduled time has passed
                if let Some(scheduled_at) = info.scheduled_at {
                    if now < scheduled_at {
                        i += 1;
                        continue;
                    }
                }

                // Check group constraint
                if let Some(ref group) = info.group {
                    if self.active_groups.contains_key(group) {
                        i += 1;
                        continue;
                    }
                }

                // Check dependencies
                if self.dependencies.is_ready(info.id, &self.completed) {
                    let task = queue.remove(i).unwrap();

                    // Register group if any
                    if let Some(group) = task.group.clone() {
                        self.active_groups.insert(group, task.id);
                    }

                    return Some(task);
                }

                i += 1;
            }
        }

        None
    }

    /// Marks a task as completed.
    pub fn mark_completed(&mut self, id: TaskId) {
        self.running.remove(&id);
        self.completed.insert(id);
        self.stats.total_completed += 1;

        // Remove from active groups
        self.active_groups.retain(|_, &mut task_id| task_id != id);

        // Try to schedule next tasks
        self.schedule_next();
    }

    /// Marks a task as failed.
    pub fn mark_failed(&mut self, id: TaskId) {
        self.running.remove(&id);
        self.failed.insert(id);
        self.stats.total_failed += 1;

        // Remove from active groups
        self.active_groups.retain(|_, &mut task_id| task_id != id);

        // Try to schedule next tasks
        self.schedule_next();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::background::FunctionTask;

    #[test]
    fn test_queue_creation() {
        let config = QueueConfig::default();
        let queue = TaskQueue::new(config);
        assert_eq!(queue.stats().total_queued, 0);
    }

    #[test]
    fn test_submit_task() {
        let config = QueueConfig::default();
        let mut queue = TaskQueue::new(config);

        let task = FunctionTask::new("test", |_token, _progress| async { Ok(42) });
        let _id = queue.submit(task, Priority::Normal);

        let stats = queue.stats();
        assert_eq!(stats.by_priority.get(&Priority::Normal), Some(&1));
    }

    #[test]
    fn test_priority_ordering() {
        let config = QueueConfig::new(1); // Only 1 concurrent task
        let mut queue = TaskQueue::new(config);

        let task1 = FunctionTask::new("low", |_token, _progress| async { Ok(1) });
        let task2 = FunctionTask::new("high", |_token, _progress| async { Ok(2) });

        queue.submit(task1, Priority::Low);
        queue.submit(task2, Priority::High);

        // High priority should be selected first
        let next = queue.select_next_task().unwrap();
        assert_eq!(next.priority, Priority::High);
    }

    #[test]
    fn test_set_priority() {
        // Keep the task queued so priority can be changed before scheduling.
        let config = QueueConfig::new(0);
        let mut queue = TaskQueue::new(config);

        let task = FunctionTask::new("test", |_token, _progress| async { Ok(42) });
        let id = queue.submit(task, Priority::Low);

        queue.set_priority(id, Priority::High).unwrap();

        let stats = queue.stats();
        assert_eq!(stats.by_priority.get(&Priority::High), Some(&1));
        assert_eq!(stats.by_priority.get(&Priority::Low), Some(&0));
    }

    #[test]
    fn test_dependencies() {
        let config = QueueConfig::default();
        let mut queue = TaskQueue::new(config);

        let task1 = FunctionTask::new("t1", |_token, _progress| async { Ok(1) });
        let task2 = FunctionTask::new("t2", |_token, _progress| async { Ok(2) });

        let id1 = queue.submit(task1, Priority::Normal);
        let id2 = queue.submit_with_options(
            task2,
            SubmitOptions::new(Priority::Normal).with_dependency(id1),
        );

        // t1 should be ready, t2 should not
        assert!(queue.dependencies.is_ready(id1, &queue.completed));
        assert!(!queue.dependencies.is_ready(id2, &queue.completed));

        // After t1 completes, t2 should be ready
        queue.mark_completed(id1);
        assert!(queue.dependencies.is_ready(id2, &queue.completed));
    }
}
