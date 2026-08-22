/// Field definition system
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Field type for validation and comparison
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldType {
    String,
    Number,
    Boolean,
    Date,
}

/// Field definition with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDefinition {
    /// Field name
    pub name: String,

    /// Alternative names for this field
    pub aliases: Vec<String>,

    /// Type of the field
    pub field_type: FieldType,

    /// Human-readable description
    pub description: String,

    /// Example values
    pub examples: Vec<String>,
}

impl FieldDefinition {
    /// Create a new field definition
    pub fn new(name: impl Into<String>, field_type: FieldType) -> Self {
        Self {
            name: name.into(),
            aliases: Vec::new(),
            field_type,
            description: String::new(),
            examples: Vec::new(),
        }
    }

    /// Add an alias for this field
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Set the description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Add an example value
    pub fn with_example(mut self, example: impl Into<String>) -> Self {
        self.examples.push(example.into());
        self
    }

    /// Check if a name matches this field (including aliases)
    pub fn matches_name(&self, name: &str) -> bool {
        let name_lower = name.to_lowercase();
        self.name.to_lowercase() == name_lower
            || self.aliases.iter().any(|a| a.to_lowercase() == name_lower)
    }
}

/// Registry for field definitions
pub struct FieldRegistry {
    fields: HashMap<String, FieldDefinition>,
    aliases: HashMap<String, String>, // alias -> canonical name
}

impl FieldRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            fields: HashMap::new(),
            aliases: HashMap::new(),
        }
    }

    /// Register a field definition
    pub fn register(&mut self, def: FieldDefinition) {
        let name = def.name.clone();

        // Register aliases
        for alias in &def.aliases {
            self.aliases.insert(alias.to_lowercase(), name.clone());
        }

        self.fields.insert(name.to_lowercase(), def);
    }

    /// Get a field definition by name (including aliases)
    pub fn get(&self, name: &str) -> Option<&FieldDefinition> {
        let name_lower = name.to_lowercase();

        // Try direct lookup first
        if let Some(def) = self.fields.get(&name_lower) {
            return Some(def);
        }

        // Try alias lookup
        if let Some(canonical) = self.aliases.get(&name_lower) {
            return self.fields.get(canonical);
        }

        None
    }

    /// Get all field definitions
    pub fn all(&self) -> Vec<&FieldDefinition> {
        self.fields.values().collect()
    }

    /// Suggest field names based on a prefix
    pub fn suggest(&self, prefix: &str) -> Vec<String> {
        let prefix_lower = prefix.to_lowercase();
        let mut suggestions = Vec::new();

        for def in self.fields.values() {
            // Check main name
            if def.name.to_lowercase().starts_with(&prefix_lower) {
                suggestions.push(def.name.clone());
            }

            // Check aliases
            for alias in &def.aliases {
                if alias.to_lowercase().starts_with(&prefix_lower) {
                    suggestions.push(format!("{} ({})", alias, def.name));
                }
            }
        }

        suggestions.sort();
        suggestions
    }

    /// Check if a field exists
    pub fn has_field(&self, name: &str) -> bool {
        self.get(name).is_some()
    }
}

impl Default for FieldRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_definition() {
        let field = FieldDefinition::new("status", FieldType::String)
            .with_alias("st")
            .with_description("Task status")
            .with_example("error")
            .with_example("success");

        assert_eq!(field.name, "status");
        assert_eq!(field.aliases, vec!["st"]);
        assert_eq!(field.description, "Task status");
        assert_eq!(field.examples.len(), 2);

        assert!(field.matches_name("status"));
        assert!(field.matches_name("st"));
        assert!(field.matches_name("STATUS"));
        assert!(!field.matches_name("state"));
    }

    #[test]
    fn test_registry_register() {
        let mut registry = FieldRegistry::new();

        let field = FieldDefinition::new("status", FieldType::String).with_alias("st");

        registry.register(field);

        assert!(registry.has_field("status"));
        assert!(registry.has_field("st"));
        assert!(!registry.has_field("unknown"));
    }

    #[test]
    fn test_registry_get() {
        let mut registry = FieldRegistry::new();

        let field = FieldDefinition::new("priority", FieldType::Number)
            .with_alias("pri")
            .with_description("Task priority");

        registry.register(field);

        let def = registry.get("priority").unwrap();
        assert_eq!(def.name, "priority");

        let def = registry.get("pri").unwrap();
        assert_eq!(def.name, "priority");

        assert!(registry.get("unknown").is_none());
    }

    #[test]
    fn test_registry_suggest() {
        let mut registry = FieldRegistry::new();

        registry.register(FieldDefinition::new("status", FieldType::String).with_alias("st"));

        registry.register(FieldDefinition::new("state", FieldType::String));

        registry.register(FieldDefinition::new("priority", FieldType::Number));

        let suggestions = registry.suggest("st");
        assert_eq!(suggestions.len(), 3);
        assert!(suggestions.iter().any(|s| s == "status"));
        assert!(suggestions.iter().any(|s| s == "state"));
        assert!(suggestions.iter().any(|s| s == "st (status)"));

        let suggestions = registry.suggest("pri");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0], "priority");
    }

    #[test]
    fn test_registry_all() {
        let mut registry = FieldRegistry::new();

        registry.register(FieldDefinition::new("field1", FieldType::String));
        registry.register(FieldDefinition::new("field2", FieldType::Number));
        registry.register(FieldDefinition::new("field3", FieldType::Boolean));

        let all = registry.all();
        assert_eq!(all.len(), 3);
    }
}
