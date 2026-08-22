use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use super::error::{MacroError, Result};
use super::manager::Macro;

const MAX_FILE_SIZE: u64 = 1024 * 1024; // 1MB

/// Macro storage manager for persistence
pub struct MacroStorage {
    config_path: PathBuf,
}

impl MacroStorage {
    /// Create a new macro storage manager
    pub fn new() -> Result<Self> {
        let config_dir = dirs::home_dir()
            .ok_or(MacroError::NoHomeDir)?
            .join(".kiana");

        fs::create_dir_all(&config_dir)?;

        Ok(Self {
            config_path: config_dir.join("macros.toml"),
        })
    }

    /// Create with custom path (for testing)
    pub fn with_path(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(Self { config_path: path })
    }

    /// Load macros from disk
    pub fn load(&self) -> Result<HashMap<char, Macro>> {
        if !self.config_path.exists() {
            return Ok(HashMap::new());
        }

        // Check file size
        let metadata = fs::metadata(&self.config_path)?;
        if metadata.len() > MAX_FILE_SIZE {
            return Err(MacroError::FileTooLarge);
        }

        let contents = fs::read_to_string(&self.config_path)?;

        if contents.trim().is_empty() {
            return Ok(HashMap::new());
        }

        // TOML keys must be strings; registers are stored as single-character keys.
        let raw: HashMap<String, Macro> = toml::from_str(&contents)?;
        let mut macros = HashMap::new();
        for (key, value) in raw {
            let mut chars = key.chars();
            let Some(register) = chars.next() else {
                return Err(MacroError::InvalidRegister('?'));
            };
            if chars.next().is_some() || !register.is_ascii_lowercase() {
                return Err(MacroError::InvalidRegister(register));
            }
            macros.insert(register, value);
        }
        Ok(macros)
    }

    /// Save macros to disk
    pub fn save(&self, macros: &HashMap<char, Macro>) -> Result<()> {
        let serializable: HashMap<String, Macro> = macros
            .iter()
            .map(|(register, macro_def)| (register.to_string(), macro_def.clone()))
            .collect();
        let contents = toml::to_string_pretty(&serializable)?;

        // Check serialized size
        if contents.len() > MAX_FILE_SIZE as usize {
            return Err(MacroError::FileTooLarge);
        }

        fs::write(&self.config_path, contents)?;
        Ok(())
    }

    /// Get the config file path
    pub fn path(&self) -> &PathBuf {
        &self.config_path
    }

    /// Clear all saved macros
    pub fn clear(&self) -> Result<()> {
        if self.config_path.exists() {
            fs::remove_file(&self.config_path)?;
        }
        Ok(())
    }
}

impl Default for MacroStorage {
    fn default() -> Self {
        Self::new().expect("Failed to create macro storage")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    use tempfile::TempDir;

    use crate::macro_system::manager::MacroAction;

    #[test]
    fn test_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_macros.toml");
        let storage = MacroStorage::with_path(path).unwrap();

        let mut macros = HashMap::new();
        macros.insert(
            'a',
            Macro {
                register: 'a',
                actions: vec![
                    MacroAction::KeyPress {
                        code: KeyCode::Char('j'),
                        modifiers: KeyModifiers::empty(),
                    },
                    MacroAction::KeyPress {
                        code: KeyCode::Char('j'),
                        modifiers: KeyModifiers::empty(),
                    },
                ],
                description: Some("Move down twice".to_string()),
                created_at: Utc::now(),
                modified_at: Utc::now(),
            },
        );

        storage.save(&macros).unwrap();
        let loaded = storage.load().unwrap();

        assert_eq!(loaded.len(), 1);
        assert!(loaded.contains_key(&'a'));
        assert_eq!(loaded[&'a'].actions.len(), 2);
    }

    #[test]
    fn test_load_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("empty.toml");
        let storage = MacroStorage::with_path(path.clone()).unwrap();

        // Create empty file
        fs::write(&path, "").unwrap();

        let loaded = storage.load().unwrap();
        assert_eq!(loaded.len(), 0);
    }

    #[test]
    fn test_load_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("nonexistent.toml");
        let storage = MacroStorage::with_path(path).unwrap();

        let loaded = storage.load().unwrap();
        assert_eq!(loaded.len(), 0);
    }

    #[test]
    fn test_clear() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("clear_test.toml");
        let storage = MacroStorage::with_path(path.clone()).unwrap();

        let macros = HashMap::new();
        storage.save(&macros).unwrap();
        assert!(path.exists());

        storage.clear().unwrap();
        assert!(!path.exists());
    }
}
