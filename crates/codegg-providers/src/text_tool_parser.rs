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
}

impl TextRepairProfile {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "hermes_xml" => Some(Self::HermesXml),
            "invoke_json" => Some(Self::InvokeJson),
            "raw_json_envelope" => Some(Self::RawJsonEnvelope),
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
    // An enabled adapter may explicitly support normal `stop`; all other
    // profiles still require a provider indication of malformed tool output.
    if !matches!(
        stop_reason,
        Some("stop" | "tool_calls" | "length" | "max_tokens")
    ) {
        return Ok(None);
    }

    let candidates = match profile {
        TextRepairProfile::HermesXml => parse_hermes_xml(text)?,
        TextRepairProfile::InvokeJson => parse_invoke_response(text)?,
        TextRepairProfile::RawJsonEnvelope => parse_raw_json_envelope(text)?,
    };
    if candidates.is_empty() {
        return Ok(None);
    }
    if candidates.len() > MAX_REPAIRED_CALLS {
        return Err(TextRepairError::CallLimitExceeded);
    }

    let surface: std::collections::HashMap<&str, &ToolDefinition> = tools
        .iter()
        .map(|tool| (tool.name.as_str(), tool))
        .collect();
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
    if !remaining.trim().is_empty() && !remaining.trim().is_empty() {
        // Trailing prose is allowed by the explicit Hermes compatibility
        // profile; it is never independently parsed.
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
    let object = arguments.as_object().expect("checked above");
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
}
