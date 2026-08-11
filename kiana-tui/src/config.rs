use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub provider: ProviderConfig,
    pub parameters: ParametersConfig,
    #[serde(default)]
    pub keybindings: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,  // "anthropic" / "openai" / "ollama"
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParametersConfig {
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

fn default_temperature() -> f32 {
    0.7
}

fn default_max_tokens() -> u32 {
    4096
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: ProviderConfig {
                name: "anthropic".to_string(),
                model: "claude-3-5-sonnet-20241022".to_string(),
            },
            parameters: ParametersConfig {
                temperature: default_temperature(),
                max_tokens: default_max_tokens(),
            },
            keybindings: HashMap::new(),
        }
    }
}

impl Config {
    /// Get the config file path (~/.kiana/config.toml)
    pub fn config_path() -> Result<PathBuf> {
        let home = dirs::home_dir()
            .context("Failed to get home directory")?;
        Ok(home.join(".kiana").join("config.toml"))
    }

    /// Load config from ~/.kiana/config.toml
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;

        if !path.exists() {
            // Return default config if file doesn't exist
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: Config = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        Ok(config)
    }

    /// Save config to ~/.kiana/config.toml
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;

        // Create directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
        }

        let contents = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;

        fs::write(&path, contents)
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;

        Ok(())
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        // Validate provider name
        if !["anthropic", "openai", "ollama"].contains(&self.provider.name.as_str()) {
            anyhow::bail!("Invalid provider: {}. Must be one of: anthropic, openai, ollama", self.provider.name);
        }

        // Validate model name is not empty
        if self.provider.model.trim().is_empty() {
            anyhow::bail!("Model name cannot be empty");
        }

        // Validate temperature range
        if self.parameters.temperature < 0.0 || self.parameters.temperature > 2.0 {
            anyhow::bail!("Temperature must be between 0.0 and 2.0");
        }

        // Validate max_tokens
        if self.parameters.max_tokens < 1 || self.parameters.max_tokens > 200_000 {
            anyhow::bail!("max_tokens must be between 1 and 200,000");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.provider.name, "anthropic");
        assert_eq!(config.parameters.temperature, 0.7);
        assert_eq!(config.parameters.max_tokens, 4096);
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.provider.name, config.provider.name);
        assert_eq!(parsed.provider.model, config.provider.model);
    }

    #[test]
    fn test_config_validation() {
        let mut config = Config::default();
        assert!(config.validate().is_ok());

        // Invalid provider
        config.provider.name = "invalid".to_string();
        assert!(config.validate().is_err());
        config.provider.name = "anthropic".to_string();

        // Invalid temperature
        config.parameters.temperature = 3.0;
        assert!(config.validate().is_err());
        config.parameters.temperature = 0.7;

        // Invalid max_tokens
        config.parameters.max_tokens = 0;
        assert!(config.validate().is_err());
    }
}
