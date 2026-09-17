//! The canonical model-tool schema catalog shared by model, policy and broker adapters.
use serde_json::{json, Value};

/// 模型可见的 shell 工具名称。
pub const TOOL_SHELL: &str = "shell";
/// 模型可见的补丁工具名称。
pub const TOOL_APPLY_PATCH: &str = "apply_patch";
/// 模型可见的 MCP 工具名称。
pub const TOOL_MCP: &str = "mcp";
/// 记忆搜索工具名称。
pub const TOOL_MEMORY_SEARCH: &str = "memory.search";
/// 记忆写入工具名称。
pub const TOOL_MEMORY_WRITE: &str = "memory.write";

/// 返回当前受支持的模型工具 schema 列表。
///
/// schema 只约束模型输出形状；每个字段仍会在能力映射、策略、审批和 Broker 中再次
/// 校验。新增工具必须同时更新版本化 `ToolCatalogSnapshot`、角色目录、策略映射和拒绝
/// 测试，不能只在这里追加 schema。
pub fn tool_schemas() -> Vec<Value> {
    vec![
        json!({
            "name": TOOL_SHELL,
            "description": "Run a shell command in the project sandbox. Codex-compatible command_execution item. Linux commands are confined with bubblewrap; read-only cannot write the workspace.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "array", "items": { "type": "string" } }
                        ]
                    },
                    "workdir": { "type": "string" },
                    "timeout_ms": { "type": "integer", "minimum": 1 }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": TOOL_APPLY_PATCH,
            "description": "Apply a file patch in the project workspace. Codex-compatible file_change item.",
            "parameters": {
                "type": "object",
                "properties": {
                    "patch": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["patch"]
            }
        }),
        json!({
            "name": TOOL_MCP,
            "description": "Call a configured MCP server tool through the daemon broker. stdio only. Untrusted projects are denied.",
            "parameters": {
                "type": "object",
                "properties": {
                    "server": { "type": "string" },
                    "tool": { "type": "string" },
                    "tool_name": { "type": "string" },
                    "arguments": { "type": "object" }
                },
                "required": ["tool"]
            }
        }),
        json!({
            "name": TOOL_MEMORY_SEARCH,
            "description": "Search a Kiana memory collection through the daemon broker. Hits include layer, collection, and source. Unsourced hits are not verified conclusions.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "collection": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1 }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": TOOL_MEMORY_WRITE,
            "description": "Write an explicit memory record to one collection. Instance scratch does not promote. Chat is never auto-ingested.",
            "parameters": {
                "type": "object",
                "properties": {
                    "collection": { "type": "string" },
                    "text": { "type": "string" },
                    "source": { "type": "string" },
                    "promote_to": { "type": "string" }
                },
                "required": ["collection", "text", "source"]
            }
        }),
    ]
}

/// 按当前工具 schema 校验一次模型工具调用参数。
///
/// 使用有界的本地 JSON Schema 子集；未知关键字和外部引用显式拒绝。
/// 额外字段是否允许由各 schema 的 additionalProperties 声明决定。
pub fn validate_tool_arguments(name: &str, arguments: &Value) -> Result<(), String> {
    let schema_name =
        model_tool_name(name).ok_or_else(|| format!("invalid_arguments:{name}:<arguments>"))?;
    let schemas = tool_schemas();
    let Some(parameters) = schemas
        .iter()
        .find(|schema| schema.get("name").and_then(Value::as_str) == Some(schema_name))
        .and_then(|schema| schema.get("parameters"))
    else {
        return Err(format!("invalid_arguments:{name}:<arguments>"));
    };

    validate_schema_value(arguments, parameters)
        .map_err(|field| format!("invalid_arguments:{name}:{field}"))
}

pub fn model_tool_name(name: &str) -> Option<&'static str> {
    crate::tool_authority::tool_spec(name).map(|spec| spec.name)
}

pub const TOOL_JSON_MAX_BYTES: usize = 512 * 1024;
pub const TOOL_JSON_MAX_DEPTH: usize = 32;
pub const TOOL_JSON_MAX_ITEMS: usize = 4096;
pub const TOOL_SCHEMA_DIALECT: &str = "kiana.json-schema-subset.v1";

pub fn validate_json_limits(value: &Value) -> Result<(), String> {
    let mut pending = vec![(value, 0usize)];
    let mut nodes = 0usize;
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if depth > TOOL_JSON_MAX_DEPTH || nodes > 32_768 {
            return Err("json_complexity_limit".to_owned());
        }
        match value {
            Value::Array(values) => {
                if values.len() > TOOL_JSON_MAX_ITEMS {
                    return Err("json_array_limit".to_owned());
                }
                pending.extend(values.iter().map(|value| (value, depth + 1)));
            }
            Value::Object(values) => {
                if values.len() > TOOL_JSON_MAX_ITEMS {
                    return Err("json_object_limit".to_owned());
                }
                pending.extend(values.values().map(|value| (value, depth + 1)));
            }
            _ => {}
        }
    }
    if serde_json::to_vec(value).map_err(|_| "json_invalid")?.len() > TOOL_JSON_MAX_BYTES {
        return Err("json_bytes_limit".to_owned());
    }
    Ok(())
}

/// Used by untrusted wire/config readers before any duplicate key can be silently discarded.
pub fn parse_bounded_json(raw: &[u8]) -> Result<Value, String> {
    if raw.len() > TOOL_JSON_MAX_BYTES {
        return Err("json_bytes_limit".to_owned());
    }
    let value = serde_json::from_slice::<UniqueJson>(raw)
        .map_err(|error| format!("json_invalid:{error}"))?
        .0;
    validate_json_limits(&value)?;
    Ok(value)
}
struct UniqueJson(Value);
impl<'de> serde::Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> serde::de::Visitor<'de> for UniqueVisitor {
            type Value = UniqueJson;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("JSON without duplicate object keys")
            }
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::Bool(value)))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|value| UniqueJson(Value::Number(value)))
                    .ok_or_else(|| E::custom("json_number_invalid"))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::String(value.to_owned())))
            }
            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::String(value)))
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::Null))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<UniqueJson>()? {
                    if values.len() >= TOOL_JSON_MAX_ITEMS {
                        return Err(serde::de::Error::custom("json_array_limit"));
                    }
                    values.push(value.0);
                }
                Ok(UniqueJson(Value::Array(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(serde::de::Error::custom("json_duplicate_key"));
                    }
                    if values.len() >= TOOL_JSON_MAX_ITEMS {
                        return Err(serde::de::Error::custom("json_object_limit"));
                    }
                    values.insert(key, map.next_value::<UniqueJson>()?.0);
                }
                Ok(UniqueJson(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}

pub fn validate_schema_contract(schema: &Value) -> Result<(), String> {
    validate_json_limits(schema)?;
    validate_schema_node(schema, 0)
}
fn validate_schema_node(schema: &Value, depth: usize) -> Result<(), String> {
    if depth > TOOL_JSON_MAX_DEPTH {
        return Err("schema_depth_limit".to_owned());
    }
    if schema.is_boolean() {
        return Ok(());
    }
    let object = schema.as_object().ok_or("schema_object_required")?;
    const KEYS: &[&str] = &[
        "type",
        "properties",
        "required",
        "additionalProperties",
        "items",
        "enum",
        "const",
        "description",
        "title",
        "default",
        "minLength",
        "maxLength",
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "minItems",
        "maxItems",
        "anyOf",
        "oneOf",
        "allOf",
        "$schema",
    ];
    if object.keys().any(|key| !KEYS.contains(&key.as_str())) {
        return Err("schema_keyword_unsupported".to_owned());
    }
    if let Some(value) = object.get("$schema") {
        if !matches!(
            value.as_str(),
            Some(
                "https://json-schema.org/draft/2020-12/schema"
                    | "http://json-schema.org/draft-07/schema#"
                    | "https://json-schema.org/draft-07/schema#"
            )
        ) {
            return Err("schema_dialect_unsupported".to_owned());
        }
    }
    if let Some(value) = object.get("type") {
        let allowed = |kind: &str| {
            matches!(
                kind,
                "object" | "array" | "string" | "number" | "integer" | "boolean" | "null"
            )
        };
        if !(value.as_str().is_some_and(allowed)
            || value.as_array().is_some_and(|items| {
                !items.is_empty()
                    && items
                        .iter()
                        .all(|value| value.as_str().is_some_and(allowed))
            }))
        {
            return Err("schema_type_invalid".to_owned());
        }
    }
    if let Some(properties) = object.get("properties") {
        for value in properties
            .as_object()
            .ok_or("schema_properties_invalid")?
            .values()
        {
            validate_schema_node(value, depth + 1)?;
        }
    }
    if let Some(required) = object.get("required") {
        let fields = required.as_array().ok_or("schema_required_invalid")?;
        let mut seen = std::collections::HashSet::new();
        if fields.iter().any(|field| {
            field
                .as_str()
                .is_none_or(|field| field.is_empty() || !seen.insert(field))
        }) {
            return Err("schema_required_invalid".to_owned());
        }
    }
    for key in ["items", "additionalProperties"] {
        if let Some(value) = object.get(key) {
            validate_schema_node(value, depth + 1)?;
        }
    }
    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(branches) = object.get(key) {
            let branches = branches.as_array().ok_or("schema_combinator_invalid")?;
            if branches.is_empty() || branches.len() > 8 {
                return Err("schema_combinator_limit".to_owned());
            }
            for branch in branches {
                validate_schema_node(branch, depth + 1)?;
            }
        }
    }
    for key in ["minLength", "maxLength", "minItems", "maxItems"] {
        if object
            .get(key)
            .is_some_and(|value| value.as_u64().is_none())
        {
            return Err("schema_limit_invalid".to_owned());
        }
    }
    for key in ["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum"] {
        if object
            .get(key)
            .is_some_and(|value| value.as_f64().is_none_or(|n| !n.is_finite()))
        {
            return Err("schema_number_invalid".to_owned());
        }
    }
    if object
        .get("enum")
        .is_some_and(|value| value.as_array().is_none_or(Vec::is_empty))
    {
        return Err("schema_enum_invalid".to_owned());
    }
    Ok(())
}

#[doc(hidden)]
pub fn validate_schema_value(value: &Value, schema: &Value) -> Result<(), String> {
    validate_json_limits(value)?;
    validate_schema_contract(schema)?;
    validate_schema_value_inner(value, schema, &mut 100_000)
}
fn validate_schema_value_inner(
    value: &Value,
    schema: &Value,
    remaining: &mut usize,
) -> Result<(), String> {
    if *remaining == 0 {
        return Err("schema_evaluation_limit".to_owned());
    }
    *remaining -= 1;
    if schema == &Value::Bool(true) {
        return Ok(());
    }
    if schema == &Value::Bool(false) {
        return Err("<arguments>".to_owned());
    }
    for (key, mode) in [("anyOf", 0), ("oneOf", 1), ("allOf", 2)] {
        if let Some(branches) = schema[key].as_array() {
            let mut matches = 0;
            for branch in branches {
                if validate_schema_value_inner(value, branch, remaining).is_ok() {
                    matches += 1;
                }
                if *remaining == 0 {
                    return Err("schema_evaluation_limit".to_owned());
                }
            }
            if (mode == 0 && matches == 0)
                || (mode == 1 && matches != 1)
                || (mode == 2 && matches != branches.len())
            {
                return Err("<arguments>".to_owned());
            }
        }
    }
    if let Some(kind) = schema.get("type") {
        let valid = kind
            .as_str()
            .is_some_and(|kind| value_matches_type(value, kind))
            || kind.as_array().is_some_and(|kinds| {
                kinds.iter().any(|kind| {
                    kind.as_str()
                        .is_some_and(|kind| value_matches_type(value, kind))
                })
            });
        if !valid {
            return Err("<arguments>".to_owned());
        }
    }
    if let Some(number) = value.as_f64() {
        for (key, invalid) in [
            ("minimum", 0),
            ("maximum", 1),
            ("exclusiveMinimum", 2),
            ("exclusiveMaximum", 3),
        ] {
            if let Some(limit) = schema[key].as_f64() {
                if match invalid {
                    0 => number < limit,
                    1 => number > limit,
                    2 => number <= limit,
                    _ => number >= limit,
                } {
                    return Err("<arguments>".to_owned());
                }
            }
        }
    }
    if schema
        .get("const")
        .is_some_and(|constant| constant != value)
        || schema["enum"]
            .as_array()
            .is_some_and(|options| !options.contains(value))
    {
        return Err("<arguments>".to_owned());
    }
    if let Some(text) = value.as_str() {
        let length = text.chars().count() as u64;
        if schema["minLength"].as_u64().is_some_and(|n| length < n)
            || schema["maxLength"].as_u64().is_some_and(|n| length > n)
        {
            return Err("<arguments>".to_owned());
        }
    }
    if let Some(items) = value.as_array() {
        if schema["minItems"]
            .as_u64()
            .is_some_and(|n| items.len() < (n as usize))
            || schema["maxItems"]
                .as_u64()
                .is_some_and(|n| items.len() > (n as usize))
        {
            return Err("<arguments>".to_owned());
        }
        if let Some(item_schema) = schema.get("items") {
            for item in items {
                validate_schema_value_inner(item, item_schema, remaining)?;
            }
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(fields) = schema["required"].as_array() {
            for field in fields.iter().filter_map(Value::as_str) {
                if !object.contains_key(field) {
                    return Err(field.to_owned());
                }
            }
        }
        for (name, value) in object {
            if let Some(property) = schema["properties"].get(name) {
                validate_schema_value_inner(value, property, remaining)
                    .map_err(|_| name.clone())?;
            } else if let Some(additional) = schema.get("additionalProperties") {
                validate_schema_value_inner(value, additional, remaining)
                    .map_err(|_| name.clone())?;
            }
        }
    }
    Ok(())
}

fn value_matches_type(value: &Value, expected: &str) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    }
}
