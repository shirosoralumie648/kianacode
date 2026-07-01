use crate::tool::*;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

const STRUCTURED_OUTPUT_KEY: &str = "structured_output";
const STRUCTURED_OUTPUT_SCHEMA_KEY: &str = "structured_output_schema";

pub struct SyntheticOutputTool;

impl SyntheticOutputTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for SyntheticOutputTool {
    fn name(&self) -> &str {
        "StructuredOutput"
    }

    fn description(&self) -> &str {
        "Return the final response as structured JSON"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("return structured JSON output")
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": true
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" },
                "structured_output": {}
            },
            "required": ["message", "structured_output"]
        })
    }

    async fn validate_input(&self, input: &Value, context: &ToolContext) -> ValidationResult {
        if !input.is_object() {
            return ValidationResult::err(
                "StructuredOutput input must be a JSON object".to_string(),
                1,
            );
        }

        if let Some(schema) = context.app_state.get(STRUCTURED_OUTPUT_SCHEMA_KEY) {
            if let Err(error) = validate_json_schema_value(input, schema) {
                return ValidationResult::err(
                    format!("Output does not match required schema: {error}"),
                    2,
                );
            }
        }

        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        if !input.is_object() {
            return Err(ToolError::ValidationError(
                "StructuredOutput input must be a JSON object".to_string(),
            ));
        }

        if let Some(schema) = context.app_state.get(STRUCTURED_OUTPUT_SCHEMA_KEY) {
            validate_json_schema_value(input, schema).map_err(|error| {
                ToolError::ValidationError(format!(
                    "Output does not match required schema: {error}"
                ))
            })?;
        }

        Ok(ToolOutput {
            data: json!({
                "message": "Structured output provided successfully",
                "structured_output": input
            }),
            metadata: Some(HashMap::from([(
                STRUCTURED_OUTPUT_KEY.to_string(),
                input.clone(),
            )])),
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let message = output
            .data
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Structured output provided successfully");
        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": message
        })
    }
}

fn validate_json_schema_value(value: &Value, schema: &Value) -> Result<(), String> {
    validate_json_schema_value_at(value, schema, "root")
}

fn validate_json_schema_value_at(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
    let Some(schema_object) = schema.as_object() else {
        return Err(format!("{path}: schema must be an object"));
    };

    if let Some(enum_values) = schema_object.get("enum").and_then(Value::as_array) {
        if !enum_values.iter().any(|candidate| candidate == value) {
            return Err(format!(
                "{path}: value is not one of the allowed enum values"
            ));
        }
    }

    if let Some(type_value) = schema_object.get("type") {
        validate_type(value, type_value, path)?;
    }

    if let Some(required) = schema_object.get("required").and_then(Value::as_array) {
        let Some(object) = value.as_object() else {
            if required.is_empty() {
                return Ok(());
            }
            return Err(format!("{path}: required fields need an object"));
        };
        for field in required.iter().filter_map(Value::as_str) {
            if !object.contains_key(field) {
                return Err(format!("{path}: missing required field `{field}`"));
            }
        }
    }

    if let Some(properties) = schema_object.get("properties").and_then(Value::as_object) {
        let Some(object) = value.as_object() else {
            return Ok(());
        };
        for (field, field_schema) in properties {
            if let Some(field_value) = object.get(field) {
                validate_json_schema_value_at(
                    field_value,
                    field_schema,
                    &format!("{path}.{field}"),
                )?;
            }
        }
    }

    if let Some(items_schema) = schema_object.get("items") {
        let Some(array) = value.as_array() else {
            return Ok(());
        };
        for (index, item) in array.iter().enumerate() {
            validate_json_schema_value_at(item, items_schema, &format!("{path}[{index}]"))?;
        }
    }

    Ok(())
}

fn validate_type(value: &Value, type_value: &Value, path: &str) -> Result<(), String> {
    if let Some(type_name) = type_value.as_str() {
        if value_matches_type(value, type_name) {
            Ok(())
        } else {
            Err(format!(
                "{path}: expected {type_name}, got {}",
                value_kind(value)
            ))
        }
    } else if let Some(types) = type_value.as_array() {
        if types
            .iter()
            .filter_map(Value::as_str)
            .any(|type_name| value_matches_type(value, type_name))
        {
            Ok(())
        } else {
            let expected = types
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("|");
            Err(format!(
                "{path}: expected {expected}, got {}",
                value_kind(value)
            ))
        }
    } else {
        Err(format!(
            "{path}: schema type must be a string or string array"
        ))
    }
}

fn value_matches_type(value: &Value, type_name: &str) -> bool {
    match type_name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.as_i64().is_some() || number.as_u64().is_some() => {
            "integer"
        }
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::SyntheticOutputTool;
    use crate::{Tool, ToolContext};
    use serde_json::json;
    use std::collections::HashMap;

    fn context_with_schema(schema: serde_json::Value) -> ToolContext {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([("structured_output_schema".to_string(), schema)]),
            abort_signal: abort_rx,
        }
    }

    #[tokio::test]
    async fn returns_structured_output_metadata() {
        let mut context = context_with_schema(json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            },
            "required": ["name"]
        }));

        let output = SyntheticOutputTool::new()
            .call(&json!({ "name": "kiana", "tags": ["rust"] }), &mut context)
            .await
            .unwrap();

        assert_eq!(
            output.data["message"],
            "Structured output provided successfully"
        );
        assert_eq!(
            output
                .metadata
                .as_ref()
                .unwrap()
                .get("structured_output")
                .cloned()
                .unwrap(),
            json!({ "name": "kiana", "tags": ["rust"] })
        );

        let api_result = SyntheticOutputTool::new().map_to_api_result(&output, "toolu_structured");
        assert_eq!(api_result["type"], "tool_result");
        assert_eq!(api_result["tool_use_id"], "toolu_structured");
        assert_eq!(
            api_result["content"],
            "Structured output provided successfully"
        );
    }

    #[tokio::test]
    async fn rejects_output_that_does_not_match_schema() {
        let context = context_with_schema(json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name"]
        }));

        let validation = SyntheticOutputTool::new()
            .validate_input(&json!({ "name": 7 }), &context)
            .await;

        assert!(!validation.result);
        assert!(validation.message.unwrap().contains("root.name"));
    }
}
