//! Bounded textual tool-call repair for explicitly classified model adapters.
//!
//! Structured provider calls remain canonical. This module deliberately does
//! not search arbitrary prose for JSON: callers must provide an adapter grammar
//! and the current model-facing tool surface.

use crate::{ToolCall, ToolDefinition};
use serde_json::{Map, Value};
use std::collections::HashSet;

pub const MAX_REPAIR_INPUT_BYTES: usize = 64 * 1024;
pub const MAX_REPAIRED_CALLS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRepairProfile {
    HermesXml,
    InvokeJson,
    RawJsonEnvelope,
    FencedCode,
}

impl TextRepairProfile {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "hermes_xml" => Some(Self::HermesXml),
            "invoke_json" => Some(Self::InvokeJson),
            "raw_json_envelope" => Some(Self::RawJsonEnvelope),
            "fenced_code" => Some(Self::FencedCode),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextRepairError {
    UnknownProfile(String),
    InputTooLarge,
    MalformedEnvelope,
    UnknownTool(String),
    ArgumentsMustBeObject(String),
    SchemaViolation(String),
    CallLimitExceeded,
}

/// Repair one provider response. The response must be explicitly enabled by
/// the resolved adapter; this function is not a generic text-to-action parser.
pub fn repair_text_as_tool_calls(
    profile_name: &str,
    text: &str,
    stop_reason: Option<&str>,
    tools: &[ToolDefinition],
) -> Result<Option<Vec<ToolCall>>, TextRepairError> {
    let profile = TextRepairProfile::parse(profile_name)
        .ok_or_else(|| TextRepairError::UnknownProfile(profile_name.to_string()))?;
    if text.len() > MAX_REPAIR_INPUT_BYTES {
        return Err(TextRepairError::InputTooLarge);
    }
    if !matches!(
        stop_reason,
        Some("stop" | "tool_calls" | "length" | "max_tokens")
    ) {
        return Ok(None);
    }

    let surface: std::collections::HashMap<&str, &ToolDefinition> = tools
        .iter()
        .map(|tool| (tool.name.as_str(), tool))
        .collect();

    let candidates = match profile {
        TextRepairProfile::HermesXml => parse_hermes_xml(text)?,
        TextRepairProfile::InvokeJson => parse_invoke_response(text)?,
        TextRepairProfile::RawJsonEnvelope => parse_raw_json_envelope(text)?,
        TextRepairProfile::FencedCode => parse_fenced_code_calls(text, &surface),
    };
    if candidates.is_empty() {
        return Ok(None);
    }
    if candidates.len() > MAX_REPAIRED_CALLS {
        return Err(TextRepairError::CallLimitExceeded);
    }
    let mut seen = HashSet::new();
    let mut repaired = Vec::with_capacity(candidates.len());
    for (name, arguments) in candidates {
        let Some(definition) = surface.get(name.as_str()) else {
            return Err(TextRepairError::UnknownTool(name));
        };
        if !arguments.is_object() {
            return Err(TextRepairError::ArgumentsMustBeObject(name));
        }
        validate_object_schema(&name, &arguments, &definition.parameters)?;
        let key = format!("{name}|{arguments}");
        if seen.insert(key) {
            repaired.push(ToolCall {
                id: format!("text-repair-{}", repaired.len()).into(),
                name: name.into(),
                arguments,
            });
        }
    }
    Ok(Some(repaired))
}

fn parse_hermes_xml(text: &str) -> Result<Vec<(String, Value)>, TextRepairError> {
    let mut remaining = text.trim();
    let mut out = Vec::new();
    while let Some(start) = remaining.find("<tool_call>") {
        if !remaining[..start].trim().is_empty() {
            // Hermes permits a short lead-in, but executable syntax must still
            // be an actual tag rather than a fenced/documentation example.
        }
        let body_start = start + "<tool_call>".len();
        let Some(end_rel) = remaining[body_start..].find("</tool_call>") else {
            return Err(TextRepairError::MalformedEnvelope);
        };
        let body = remaining[body_start..body_start + end_rel].trim();
        let value: Value =
            serde_json::from_str(body).map_err(|_| TextRepairError::MalformedEnvelope)?;
        let object = value
            .as_object()
            .ok_or(TextRepairError::MalformedEnvelope)?;
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .ok_or(TextRepairError::MalformedEnvelope)?;
        let args = object
            .get("arguments")
            .cloned()
            .unwrap_or_else(empty_object);
        out.push((name.to_string(), args));
        remaining = &remaining[body_start + end_rel + "</tool_call>".len()..];
    }
    if !remaining.trim().is_empty() && out.is_empty() {
        return Ok(Vec::new());
    }
    Ok(out)
}

fn parse_invoke_response(text: &str) -> Result<Vec<(String, Value)>, TextRepairError> {
    let mut input = text.trim();
    let mut out = Vec::new();
    while !input.is_empty() {
        let Some(rest) = input.strip_prefix("invoke(") else {
            return Err(TextRepairError::MalformedEnvelope);
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('"') else {
            return Err(TextRepairError::MalformedEnvelope);
        };
        let Some(name_end) = rest.find('"') else {
            return Err(TextRepairError::MalformedEnvelope);
        };
        let name = &rest[..name_end];
        let rest = rest[name_end + 1..].trim_start();
        let Some(rest) = rest.strip_prefix(',') else {
            return Err(TextRepairError::MalformedEnvelope);
        };
        let (args, consumed) = extract_first_json_object(rest.trim_start())
            .ok_or(TextRepairError::MalformedEnvelope)?;
        let tail = rest.trim_start()[consumed..].trim_start();
        let Some(tail) = tail.strip_prefix(')') else {
            return Err(TextRepairError::MalformedEnvelope);
        };
        out.push((name.to_string(), args));
        input = tail.trim_start();
    }
    Ok(out)
}

fn parse_raw_json_envelope(text: &str) -> Result<Vec<(String, Value)>, TextRepairError> {
    let value: Value =
        serde_json::from_str(text.trim()).map_err(|_| TextRepairError::MalformedEnvelope)?;
    let object = value
        .as_object()
        .ok_or(TextRepairError::MalformedEnvelope)?;
    if object.keys().any(|key| key != "name" && key != "arguments") {
        return Err(TextRepairError::MalformedEnvelope);
    }
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .ok_or(TextRepairError::MalformedEnvelope)?;
    let args = object
        .get("arguments")
        .cloned()
        .ok_or(TextRepairError::MalformedEnvelope)?;
    Ok(vec![(name.to_string(), args)])
}

/// Recover tool calls from fenced code blocks whose info string names a tool.
fn parse_fenced_code_calls(
    text: &str,
    surface: &std::collections::HashMap<&str, &ToolDefinition>,
) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for (info, body) in fenced_blocks(text) {
        let Some(name) = info.split_whitespace().next() else {
            continue;
        };
        let name = name.trim_end_matches(':').to_ascii_lowercase();
        let Some(definition) = surface.get(name.as_str()).copied() else {
            continue;
        };
        let arguments = match serde_json::from_str::<Value>(body.trim()) {
            Ok(value) if value.is_object() => value,
            _ => {
                let Some(parameter) = single_string_parameter(&definition.parameters) else {
                    continue;
                };
                let mut object = Map::new();
                object.insert(
                    parameter.to_string(),
                    Value::String(body.trim().to_string()),
                );
                Value::Object(object)
            }
        };
        out.push((name, arguments));
    }
    out
}

fn fenced_blocks(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        let Some(marker) = fence_marker(trimmed) else {
            continue;
        };
        let info = trimmed[marker.len()..].trim().to_string();
        let mut body = String::new();
        let mut closed = false;
        for inner in lines.by_ref() {
            let inner_trimmed = inner.trim_start();
            if inner_trimmed.starts_with(marker) && inner_trimmed[marker.len()..].trim().is_empty()
            {
                closed = true;
                break;
            }
            body.push_str(inner);
            body.push('\n');
        }
        if !closed {
            break;
        }
        out.push((info, body));
    }
    out
}

fn fence_marker(line: &str) -> Option<&'static str> {
    if line.starts_with("```") {
        Some("```")
    } else if line.starts_with("~~~") {
        Some("~~~")
    } else {
        None
    }
}

fn single_string_parameter(schema: &Value) -> Option<&str> {
    let object = schema.as_object()?;
    let required = object.get("required")?.as_array()?;
    if required.len() != 1 {
        return None;
    }
    let name = required.first()?.as_str()?;
    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
        if let Some(property) = properties.get(name) {
            if property.get("type").and_then(Value::as_str) != Some("string") {
                return None;
            }
        }
    }
    Some(name)
}

fn extract_first_json_object(input: &str) -> Option<(Value, usize)> {
    let start = input.find('{')?;
    let bytes = input.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate().skip(start) {
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' if in_string => escaped = true,
            b'"' => in_string = !in_string,
            b'{' if !in_string => depth += 1,
            b'}' if !in_string => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = index + 1;
                    return Some((serde_json::from_str(&input[start..end]).ok()?, end));
                }
            }
            _ => {}
        }
    }
    None
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

fn validate_object_schema(
    name: &str,
    arguments: &Value,
    schema: &Value,
) -> Result<(), TextRepairError> {
    let object = arguments.as_object().ok_or_else(|| {
        TextRepairError::SchemaViolation(format!("{name}: arguments not an object"))
    })?;
    if schema.get("type").and_then(Value::as_str) == Some("object") {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    return Err(TextRepairError::SchemaViolation(format!(
                        "{name}: missing required argument {key}"
                    )));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tools() -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "bash".into(),
            description: "".into(),
            parameters: json!({"type":"object", "required":["command"]}),
            defer_loading: None,
        }]
    }

    #[test]
    fn structured_only_has_no_repair_entry_point() {
        assert!(TextRepairProfile::parse("structured_only").is_none());
    }

    #[test]
    fn hermes_repairs_and_validates_surface() {
        let result = repair_text_as_tool_calls("hermes_xml", "explain\n<tool_call>{\"name\":\"bash\",\"arguments\":{\"command\":\"ls\"}}</tool_call>", Some("stop"), &tools()).unwrap();
        assert_eq!(result.unwrap()[0].name.as_ref(), "bash");
    }

    #[test]
    fn prose_json_and_fences_are_not_raw_repairs() {
        assert!(repair_text_as_tool_calls(
            "raw_json_envelope",
            "Here: {\"name\":\"bash\",\"arguments\":{\"command\":\"ls\"}}",
            Some("stop"),
            &tools()
        )
        .is_err());
        assert!(repair_text_as_tool_calls(
            "hermes_xml",
            "```bash\n{\"command\":\"ls\"}\n```",
            Some("stop"),
            &tools()
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn unknown_tool_and_missing_required_argument_are_rejected() {
        let unknown = repair_text_as_tool_calls(
            "raw_json_envelope",
            r#"{"name":"write","arguments":{}}"#,
            Some("stop"),
            &tools(),
        );
        assert!(matches!(unknown, Err(TextRepairError::UnknownTool(_))));
        let missing = repair_text_as_tool_calls(
            "raw_json_envelope",
            r#"{"name":"bash","arguments":{}}"#,
            Some("stop"),
            &tools(),
        );
        assert!(matches!(missing, Err(TextRepairError::SchemaViolation(_))));
    }

    #[test]
    fn schema_validation_rejects_non_object_arguments_without_panicking() {
        let result = validate_object_schema("bash", &json!(null), &tools()[0].parameters);
        assert!(matches!(result, Err(TextRepairError::SchemaViolation(_))));
    }

    #[test]
    fn a_fenced_block_that_names_a_tool_is_repaired() {
        let result = repair_text_as_tool_calls(
            "fenced_code",
            "```bash\ncat > probe.txt <<'EOF'\nhello\nEOF\n```",
            Some("stop"),
            &tools(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name.as_ref(), "bash");
        assert_eq!(
            result[0]
                .arguments
                .get("command")
                .and_then(|value| value.as_str()),
            Some("cat > probe.txt <<'EOF'\nhello\nEOF")
        );
    }

    #[test]
    fn a_fenced_block_that_names_no_tool_is_not_a_call() {
        let result = repair_text_as_tool_calls(
            "fenced_code",
            "Here is an example:\n```python\nprint('hi')\n```",
            Some("stop"),
            &tools(),
        )
        .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn a_fenced_block_with_json_arguments_is_repaired() {
        let result = repair_text_as_tool_calls(
            "fenced_code",
            "```bash\n{\"command\":\"ls -la\"}\n```",
            Some("stop"),
            &tools(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            result[0]
                .arguments
                .get("command")
                .and_then(|value| value.as_str()),
            Some("ls -la")
        );
    }
}
