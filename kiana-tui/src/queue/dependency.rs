//! Dependency graph for task scheduling.

use crate::background::TaskId;
use std::collections::{HashMap, HashSet, VecDeque};

/// A directed acyclic graph (DAG) of task dependencies.
pub struct DependencyGraph {
    /// Forward edges: task -> tasks that depend on it
    edges: HashMap<TaskId, HashSet<TaskId>>,
    /// Reverse edges: task -> tasks it depends on
    reverse_edges: HashMap<TaskId, HashSet<TaskId>>,
}

impl DependencyGraph {
    /// Creates a new empty dependency graph.
    pub fn new() -> Self {
        Self {
            edges: HashMap::new(),
            reverse_edges: HashMap::new(),
        }
    }

    /// Adds a dependency: `task` depends on `depends_on`.
    pub fn add_dependency(&mut self, task: TaskId, depends_on: TaskId) {
        self.edges
            .entry(depends_on)
            .or_insert_with(HashSet::new)
            .insert(task);

        self.reverse_edges
            .entry(task)
            .or_insert_with(HashSet::new)
            .insert(depends_on);
    }

    /// Removes a task and all its edges from the graph.
    pub fn remove_task(&mut self, task: TaskId) {
        // Remove forward edges
        if let Some(dependents) = self.edges.remove(&task) {
            for dependent in dependents {
                if let Some(deps) = self.reverse_edges.get_mut(&dependent) {
                    deps.remove(&task);
                }
            }
        }

        // Remove reverse edges
        if let Some(dependencies) = self.reverse_edges.remove(&task) {
            for dependency in dependencies {
                if let Some(deps) = self.edges.get_mut(&dependency) {
                    deps.remove(&task);
                }
            }
        }
    }

    /// Checks if a task is ready to run (all dependencies completed).
    pub fn is_ready(&self, task: TaskId, completed: &HashSet<TaskId>) -> bool {
        match self.reverse_edges.get(&task) {
            None => true, // No dependencies
            Some(deps) => deps.iter().all(|dep| completed.contains(dep)),
        }
    }

    /// Gets all tasks that depend on the given task.
    pub fn get_dependents(&self, task: TaskId) -> Vec<TaskId> {
        self.edges
            .get(&task)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Gets all tasks that the given task depends on.
    pub fn get_dependencies(&self, task: TaskId) -> Vec<TaskId> {
        self.reverse_edges
            .get(&task)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Detects if there's a cycle in the graph starting from the given task.
    ///
    /// Returns Some(cycle) if a cycle is found, None otherwise.
    pub fn detect_cycle(&self) -> Option<Vec<TaskId>> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut path = Vec::new();

        for &task in self.edges.keys() {
            if let Some(cycle) = self.dfs_cycle(task, &mut visited, &mut rec_stack, &mut path) {
                return Some(cycle);
            }
        }

        None
    }

    fn dfs_cycle(
        &self,
        task: TaskId,
        visited: &mut HashSet<TaskId>,
        rec_stack: &mut HashSet<TaskId>,
        path: &mut Vec<TaskId>,
    ) -> Option<Vec<TaskId>> {
        if rec_stack.contains(&task) {
            // Found a cycle
            let cycle_start = path.iter().position(|&t| t == task).unwrap();
            return Some(path[cycle_start..].to_vec());
        }

        if visited.contains(&task) {
            return None;
        }

        visited.insert(task);
        rec_stack.insert(task);
        path.push(task);

        if let Some(dependents) = self.edges.get(&task) {
            for &dependent in dependents {
                if let Some(cycle) = self.dfs_cycle(dependent, visited, rec_stack, path) {
                    return Some(cycle);
                }
            }
        }

        rec_stack.remove(&task);
        path.pop();
        None
    }

    /// Performs a topological sort of the graph.
    ///
    /// Returns Ok(sorted) if successful, or Err(cycle) if there's a cycle.
    pub fn topological_sort(&self) -> Result<Vec<TaskId>, Vec<TaskId>> {
        if let Some(cycle) = self.detect_cycle() {
            return Err(cycle);
        }

        let mut in_degree: HashMap<TaskId, usize> = HashMap::new();
        let mut all_tasks = HashSet::new();

        // Calculate in-degrees
        for (&task, deps) in &self.reverse_edges {
            all_tasks.insert(task);
            in_degree.insert(task, deps.len());
            for &dep in deps {
                all_tasks.insert(dep);
            }
        }

        // Add tasks with no dependencies
        for &task in &all_tasks {
            in_degree.entry(task).or_insert(0);
        }

        // Queue of tasks with no dependencies
        let mut queue: VecDeque<TaskId> = in_degree
            .iter()
            .filter(|(_, &degree)| degree == 0)
            .map(|(&task, _)| task)
            .collect();

        let mut sorted = Vec::new();

        while let Some(task) = queue.pop_front() {
            sorted.push(task);

            if let Some(dependents) = self.edges.get(&task) {
                for &dependent in dependents {
                    if let Some(degree) = in_degree.get_mut(&dependent) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(dependent);
                        }
                    }
                }
            }
        }

        Ok(sorted)
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_dependency() {
        let mut graph = DependencyGraph::new();
        let t1 = TaskId::new();
        let t2 = TaskId::new();

        graph.add_dependency(t2, t1);

        assert_eq!(graph.get_dependencies(t2), vec![t1]);
        assert_eq!(graph.get_dependents(t1), vec![t2]);
    }

    #[test]
    fn test_is_ready() {
        let mut graph = DependencyGraph::new();
        let t1 = TaskId::new();
        let t2 = TaskId::new();
        let t3 = TaskId::new();

        graph.add_dependency(t2, t1);
        graph.add_dependency(t3, t2);

        let mut completed = HashSet::new();
        assert!(graph.is_ready(t1, &completed));
        assert!(!graph.is_ready(t2, &completed));

        completed.insert(t1);
        assert!(graph.is_ready(t2, &completed));
        assert!(!graph.is_ready(t3, &completed));

        completed.insert(t2);
        assert!(graph.is_ready(t3, &completed));
    }

    #[test]
    fn test_remove_task() {
        let mut graph = DependencyGraph::new();
        let t1 = TaskId::new();
        let t2 = TaskId::new();

        graph.add_dependency(t2, t1);
        graph.remove_task(t1);

        assert!(graph.get_dependencies(t2).is_empty());
    }

    #[test]
    fn test_detect_cycle() {
        let mut graph = DependencyGraph::new();
        let t1 = TaskId::new();
        let t2 = TaskId::new();
        let t3 = TaskId::new();

        // No cycle
        graph.add_dependency(t2, t1);
        graph.add_dependency(t3, t2);
        assert!(graph.detect_cycle().is_none());

        // Create a cycle: t1 -> t2 -> t3 -> t1
        graph.add_dependency(t1, t3);
        assert!(graph.detect_cycle().is_some());
    }

    #[test]
    fn test_topological_sort() {
        let mut graph = DependencyGraph::new();
        let t1 = TaskId::new();
        let t2 = TaskId::new();
        let t3 = TaskId::new();

        graph.add_dependency(t2, t1);
        graph.add_dependency(t3, t2);

        let sorted = graph.topological_sort().unwrap();
        assert_eq!(sorted.len(), 3);

        // t1 should come before t2, t2 before t3
        let pos1 = sorted.iter().position(|&t| t == t1).unwrap();
        let pos2 = sorted.iter().position(|&t| t == t2).unwrap();
        let pos3 = sorted.iter().position(|&t| t == t3).unwrap();

        assert!(pos1 < pos2);
        assert!(pos2 < pos3);
    }

    #[test]
    fn test_topological_sort_with_cycle() {
        let mut graph = DependencyGraph::new();
        let t1 = TaskId::new();
        let t2 = TaskId::new();

        graph.add_dependency(t1, t2);
        graph.add_dependency(t2, t1);

        let result = graph.topological_sort();
        assert!(result.is_err());
    }
}
