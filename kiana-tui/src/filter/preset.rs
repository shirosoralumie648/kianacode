/// Filter preset management
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// A saved filter preset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterPreset {
    /// Unique identifier
    pub id: String,

    /// Human-readable name
    pub name: String,

    /// Filter expression
    pub expression: String,

    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Tags for categorization
    #[serde(default)]
    pub tags: Vec<String>,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last used timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used: Option<DateTime<Utc>>,

    /// Number of times used
    #[serde(default)]
    pub use_count: u32,
}

impl FilterPreset {
    /// Create a new preset
    pub fn new(name: impl Into<String>, expression: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            expression: expression.into(),
            description: None,
            tags: Vec::new(),
            created_at: Utc::now(),
            last_used: None,
            use_count: 0,
        }
    }

    /// Set description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Record usage
    pub fn record_use(&mut self) {
        self.last_used = Some(Utc::now());
        self.use_count += 1;
    }
}

/// Manages filter presets
pub struct PresetManager {
    presets: Vec<FilterPreset>,
    config_path: PathBuf,
    dirty: bool,
}

impl PresetManager {
    /// Create a new preset manager
    pub fn new(config_path: PathBuf) -> Result<Self, std::io::Error> {
        let presets = if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok(Self {
            presets,
            config_path,
            dirty: false,
        })
    }

    /// Save a new preset or update existing one
    pub fn save_preset(&mut self, preset: FilterPreset) -> Result<(), std::io::Error> {
        // Check if preset with same ID exists
        if let Some(existing) = self.presets.iter_mut().find(|p| p.id == preset.id) {
            *existing = preset;
        } else {
            self.presets.push(preset);
        }

        self.dirty = true;
        self.persist()
    }

    /// Load a preset by ID
    pub fn load_preset(&self, id: &str) -> Option<&FilterPreset> {
        self.presets.iter().find(|p| p.id == id)
    }

    /// Load a preset by ID (mutable)
    pub fn load_preset_mut(&mut self, id: &str) -> Option<&mut FilterPreset> {
        self.presets.iter_mut().find(|p| p.id == id)
    }

    /// Delete a preset by ID
    pub fn delete_preset(&mut self, id: &str) -> Result<bool, std::io::Error> {
        let original_len = self.presets.len();
        self.presets.retain(|p| p.id != id);

        if self.presets.len() < original_len {
            self.dirty = true;
            self.persist()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// List all presets
    pub fn list_presets(&self) -> &[FilterPreset] {
        &self.presets
    }

    /// List presets matching a tag
    pub fn list_by_tag(&self, tag: &str) -> Vec<&FilterPreset> {
        self.presets
            .iter()
            .filter(|p| p.tags.iter().any(|t| t == tag))
            .collect()
    }

    /// List presets sorted by usage
    pub fn list_by_usage(&self) -> Vec<&FilterPreset> {
        let mut presets: Vec<_> = self.presets.iter().collect();
        presets.sort_by(|a, b| b.use_count.cmp(&a.use_count));
        presets
    }

    /// List presets sorted by recency
    pub fn list_by_recency(&self) -> Vec<&FilterPreset> {
        let mut presets: Vec<_> = self.presets.iter().collect();
        presets.sort_by(|a, b| match (a.last_used, b.last_used) {
            (Some(a_time), Some(b_time)) => b_time.cmp(&a_time),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => b.created_at.cmp(&a.created_at),
        });
        presets
    }

    /// Persist presets to disk
    pub fn persist(&mut self) -> Result<(), std::io::Error> {
        if !self.dirty {
            return Ok(());
        }

        // Ensure parent directory exists
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(&self.presets)?;
        fs::write(&self.config_path, json)?;

        self.dirty = false;
        Ok(())
    }

    /// Import presets from a JSON file
    pub fn import_from_file(&mut self, path: &PathBuf) -> Result<usize, std::io::Error> {
        let content = fs::read_to_string(path)?;
        let imported: Vec<FilterPreset> = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let count = imported.len();
        for preset in imported {
            // Generate new ID to avoid conflicts
            let mut new_preset = preset;
            new_preset.id = uuid::Uuid::new_v4().to_string();
            self.presets.push(new_preset);
        }

        self.dirty = true;
        self.persist()?;
        Ok(count)
    }

    /// Export presets to a JSON file
    pub fn export_to_file(&self, path: &PathBuf) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(&self.presets)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Validate a preset's expression
    pub fn validate_preset(&self, preset: &FilterPreset) -> Result<(), String> {
        use crate::filter::parse_filter;

        parse_filter(&preset.expression)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_config_path() -> PathBuf {
        let mut path = env::temp_dir();
        path.push(format!("kiana-filter-test-{}.json", uuid::Uuid::new_v4()));
        path
    }

    #[test]
    fn test_preset_creation() {
        let preset = FilterPreset::new("Critical Issues", "status=error AND priority>3")
            .with_description("High priority errors")
            .with_tag("errors")
            .with_tag("critical");

        assert_eq!(preset.name, "Critical Issues");
        assert_eq!(preset.expression, "status=error AND priority>3");
        assert_eq!(preset.description, Some("High priority errors".to_string()));
        assert_eq!(preset.tags, vec!["errors", "critical"]);
        assert_eq!(preset.use_count, 0);
    }

    #[test]
    fn test_preset_record_use() {
        let mut preset = FilterPreset::new("Test", "error");
        assert_eq!(preset.use_count, 0);
        assert!(preset.last_used.is_none());

        preset.record_use();
        assert_eq!(preset.use_count, 1);
        assert!(preset.last_used.is_some());

        preset.record_use();
        assert_eq!(preset.use_count, 2);
    }

    #[test]
    fn test_manager_save_load() {
        let path = temp_config_path();
        let mut manager = PresetManager::new(path.clone()).unwrap();

        let preset = FilterPreset::new("Test Preset", "status:error");
        let id = preset.id.clone();

        manager.save_preset(preset).unwrap();

        let loaded = manager.load_preset(&id).unwrap();
        assert_eq!(loaded.name, "Test Preset");
        assert_eq!(loaded.expression, "status:error");

        // Cleanup
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_manager_delete() {
        let path = temp_config_path();
        let mut manager = PresetManager::new(path.clone()).unwrap();

        let preset = FilterPreset::new("To Delete", "test");
        let id = preset.id.clone();

        manager.save_preset(preset).unwrap();
        assert!(manager.load_preset(&id).is_some());

        let deleted = manager.delete_preset(&id).unwrap();
        assert!(deleted);
        assert!(manager.load_preset(&id).is_none());

        // Cleanup
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_manager_list_by_tag() {
        let path = temp_config_path();
        let mut manager = PresetManager::new(path.clone()).unwrap();

        let preset1 = FilterPreset::new("Preset 1", "error").with_tag("errors");
        let preset2 = FilterPreset::new("Preset 2", "warning").with_tag("warnings");
        let preset3 = FilterPreset::new("Preset 3", "critical").with_tag("errors");

        manager.save_preset(preset1).unwrap();
        manager.save_preset(preset2).unwrap();
        manager.save_preset(preset3).unwrap();

        let error_presets = manager.list_by_tag("errors");
        assert_eq!(error_presets.len(), 2);

        let warning_presets = manager.list_by_tag("warnings");
        assert_eq!(warning_presets.len(), 1);

        // Cleanup
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_manager_list_by_usage() {
        let path = temp_config_path();
        let mut manager = PresetManager::new(path.clone()).unwrap();

        let mut preset1 = FilterPreset::new("Preset 1", "test1");
        preset1.use_count = 5;

        let mut preset2 = FilterPreset::new("Preset 2", "test2");
        preset2.use_count = 10;

        let mut preset3 = FilterPreset::new("Preset 3", "test3");
        preset3.use_count = 2;

        manager.save_preset(preset1).unwrap();
        manager.save_preset(preset2).unwrap();
        manager.save_preset(preset3).unwrap();

        let sorted = manager.list_by_usage();
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].name, "Preset 2"); // Most used
        assert_eq!(sorted[1].name, "Preset 1");
        assert_eq!(sorted[2].name, "Preset 3"); // Least used

        // Cleanup
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_manager_persistence() {
        let path = temp_config_path();

        {
            let mut manager = PresetManager::new(path.clone()).unwrap();
            let preset = FilterPreset::new("Persistent", "test");
            manager.save_preset(preset).unwrap();
        }

        {
            let manager = PresetManager::new(path.clone()).unwrap();
            let presets = manager.list_presets();
            assert_eq!(presets.len(), 1);
            assert_eq!(presets[0].name, "Persistent");
        }

        // Cleanup
        let _ = fs::remove_file(path);
    }
}
