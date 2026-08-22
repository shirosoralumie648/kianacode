//! Fold state persistence
//!
//! This module provides functionality to save and restore fold states across sessions.

use super::FoldRange;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Represents the fold state for a single file
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FoldState {
    /// Absolute path to the file
    pub file_path: String,
    /// List of folded ranges (start_line, end_line)
    pub folded_ranges: Vec<(usize, usize)>,
    /// Timestamp when this state was saved
    pub timestamp: i64,
}

impl FoldState {
    /// Create a new fold state
    pub fn new(file_path: String, folded_ranges: Vec<(usize, usize)>) -> Self {
        Self {
            file_path,
            folded_ranges,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create from fold regions
    pub fn from_regions(file_path: String, regions: &[FoldRange]) -> Self {
        let folded_ranges = regions
            .iter()
            .filter(|r| r.is_folded)
            .map(|r| (r.start_line, r.end_line))
            .collect();

        Self::new(file_path, folded_ranges)
    }

    /// Apply this state to a list of fold regions
    pub fn apply_to_regions(&self, regions: &mut [FoldRange]) {
        for region in regions.iter_mut() {
            region.is_folded = self
                .folded_ranges
                .contains(&(region.start_line, region.end_line));
        }
    }
}

/// Container for all fold states
#[derive(Clone, Debug, Serialize, Deserialize)]
struct FoldStatesContainer {
    version: u32,
    states: HashMap<String, FoldState>,
}

impl Default for FoldStatesContainer {
    fn default() -> Self {
        Self {
            version: 1,
            states: HashMap::new(),
        }
    }
}

/// Manages fold state persistence
pub struct FoldStateStore {
    /// Map of file path to fold state
    states: HashMap<String, FoldState>,
    /// Path to the storage file
    storage_path: PathBuf,
}

impl FoldStateStore {
    /// Create a new fold state store
    pub fn new(storage_path: PathBuf) -> Self {
        Self {
            states: HashMap::new(),
            storage_path,
        }
    }

    /// Create with default storage location (~/.kiana/fold_states.json)
    pub fn with_default_path() -> anyhow::Result<Self> {
        let config_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
            .join(".kiana");

        // Create directory if it doesn't exist
        fs::create_dir_all(&config_dir)?;

        let storage_path = config_dir.join("fold_states.json");
        Ok(Self::new(storage_path))
    }

    /// Load states from disk
    pub fn load_from_disk(&mut self) -> anyhow::Result<()> {
        if !self.storage_path.exists() {
            // No file yet, that's okay
            return Ok(());
        }

        let content = fs::read_to_string(&self.storage_path)?;
        let container: FoldStatesContainer = serde_json::from_str(&content)?;

        self.states = container.states;
        Ok(())
    }

    /// Save states to disk
    pub fn save_to_disk(&self) -> anyhow::Result<()> {
        let container = FoldStatesContainer {
            version: 1,
            states: self.states.clone(),
        };

        let content = serde_json::to_string_pretty(&container)?;

        // Ensure parent directory exists
        if let Some(parent) = self.storage_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&self.storage_path, content)?;
        Ok(())
    }

    /// Get the fold state for a file
    pub fn get_state(&self, file_path: &str) -> Option<&FoldState> {
        self.states.get(file_path)
    }

    /// Set the fold state for a file
    pub fn set_state(&mut self, state: FoldState) {
        self.states.insert(state.file_path.clone(), state);
    }

    /// Remove the fold state for a file
    pub fn remove_state(&mut self, file_path: &str) -> Option<FoldState> {
        self.states.remove(file_path)
    }

    /// Get the number of stored states
    pub fn state_count(&self) -> usize {
        self.states.len()
    }

    /// Clear all states
    pub fn clear(&mut self) {
        self.states.clear();
    }

    /// Clean up old states (older than specified days)
    pub fn cleanup_old_states(&mut self, max_age_days: i64) {
        let now = chrono::Utc::now().timestamp();
        let max_age_seconds = max_age_days * 24 * 60 * 60;

        self.states
            .retain(|_, state| now - state.timestamp < max_age_seconds);
    }

    /// Get storage path
    pub fn storage_path(&self) -> &Path {
        &self.storage_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::folding::FoldKind;
    use std::env;

    fn temp_storage_path() -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let temp_dir = env::temp_dir();
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        temp_dir.join(format!(
            "test_fold_states_{}_{}.json",
            std::process::id(),
            count
        ))
    }

    #[test]
    fn test_fold_state_creation() {
        let state = FoldState::new("/path/to/file.rs".to_string(), vec![(0, 10), (20, 30)]);

        assert_eq!(state.file_path, "/path/to/file.rs");
        assert_eq!(state.folded_ranges.len(), 2);
        assert!(state.timestamp > 0);
    }

    #[test]
    fn test_fold_state_from_regions() {
        let mut regions = vec![
            FoldRange::new(0, 10, FoldKind::Function),
            FoldRange::new(20, 30, FoldKind::Block),
        ];
        regions[0].is_folded = true;

        let state = FoldState::from_regions("/test.rs".to_string(), &regions);

        assert_eq!(state.folded_ranges.len(), 1);
        assert_eq!(state.folded_ranges[0], (0, 10));
    }

    #[test]
    fn test_fold_state_apply_to_regions() {
        let state = FoldState::new("/test.rs".to_string(), vec![(0, 10)]);

        let mut regions = vec![
            FoldRange::new(0, 10, FoldKind::Function),
            FoldRange::new(20, 30, FoldKind::Block),
        ];

        state.apply_to_regions(&mut regions);

        assert!(regions[0].is_folded);
        assert!(!regions[1].is_folded);
    }

    #[test]
    fn test_fold_state_store_creation() {
        let path = temp_storage_path();
        let store = FoldStateStore::new(path.clone());

        assert_eq!(store.storage_path(), path);
        assert_eq!(store.state_count(), 0);
    }

    #[test]
    fn test_fold_state_store_set_get() {
        let path = temp_storage_path();
        let mut store = FoldStateStore::new(path);

        let state = FoldState::new("/test.rs".to_string(), vec![(0, 10)]);
        store.set_state(state.clone());

        let retrieved = store.get_state("/test.rs");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().file_path, state.file_path);
    }

    #[test]
    fn test_fold_state_store_persistence() {
        let path = temp_storage_path();

        // Create and save
        {
            let mut store = FoldStateStore::new(path.clone());
            let state = FoldState::new("/test.rs".to_string(), vec![(0, 10)]);
            store.set_state(state);
            store.save_to_disk().unwrap();
        }

        // Load in new instance
        {
            let mut store = FoldStateStore::new(path.clone());
            store.load_from_disk().unwrap();

            assert_eq!(store.state_count(), 1);
            let state = store.get_state("/test.rs");
            assert!(state.is_some());
        }

        // Cleanup
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_fold_state_store_remove() {
        let path = temp_storage_path();
        let mut store = FoldStateStore::new(path);

        let state = FoldState::new("/test.rs".to_string(), vec![(0, 10)]);
        store.set_state(state);

        assert_eq!(store.state_count(), 1);

        let removed = store.remove_state("/test.rs");
        assert!(removed.is_some());
        assert_eq!(store.state_count(), 0);
    }

    #[test]
    fn test_fold_state_store_clear() {
        let path = temp_storage_path();
        let mut store = FoldStateStore::new(path);

        store.set_state(FoldState::new("/test1.rs".to_string(), vec![]));
        store.set_state(FoldState::new("/test2.rs".to_string(), vec![]));

        assert_eq!(store.state_count(), 2);

        store.clear();
        assert_eq!(store.state_count(), 0);
    }

    #[test]
    fn test_cleanup_old_states() {
        let path = temp_storage_path();
        let mut store = FoldStateStore::new(path);

        // Add a recent state
        let recent = FoldState::new("/recent.rs".to_string(), vec![]);
        store.set_state(recent);

        // Add an old state
        let mut old = FoldState::new("/old.rs".to_string(), vec![]);
        old.timestamp = chrono::Utc::now().timestamp() - (31 * 24 * 60 * 60); // 31 days ago
        store.set_state(old);

        assert_eq!(store.state_count(), 2);

        // Cleanup states older than 30 days
        store.cleanup_old_states(30);

        assert_eq!(store.state_count(), 1);
        assert!(store.get_state("/recent.rs").is_some());
        assert!(store.get_state("/old.rs").is_none());
    }

    #[test]
    fn test_load_nonexistent_file() {
        let path = temp_storage_path();
        let mut store = FoldStateStore::new(path);

        // Should not error on non-existent file
        let result = store.load_from_disk();
        assert!(result.is_ok());
        assert_eq!(store.state_count(), 0);
    }

    #[test]
    fn test_multiple_folded_ranges() {
        let state = FoldState::new("/test.rs".to_string(), vec![(0, 10), (20, 30), (40, 50)]);

        let mut regions = vec![
            FoldRange::new(0, 10, FoldKind::Function),
            FoldRange::new(20, 30, FoldKind::Block),
            FoldRange::new(40, 50, FoldKind::Function),
            FoldRange::new(60, 70, FoldKind::Block),
        ];

        state.apply_to_regions(&mut regions);

        assert!(regions[0].is_folded);
        assert!(regions[1].is_folded);
        assert!(regions[2].is_folded);
        assert!(!regions[3].is_folded);
    }
}
