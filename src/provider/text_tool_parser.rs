use crate::provider::ToolCall;
use regex::Regex;
use std::sync::LazyLock;

static INVOKE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"invoke\s*\(\s*"([A-Za-z0-9_:\-]+)"\s*,\s*"#).unwrap());

static CODE_BLOCK_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)```\s*(\w+)\s*(\{.*?\})\s*```"#).unwrap());

static XML_TOOL_CALL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<tool_call>\s*(\{.*?\})\s*</tool_call>"#).unwrap()
});

static XML_TOOL_CALL_NAME_ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<tool_call\s+name="([A-Za-z0-9_:\-]+)"\s*>(.*?)</tool_call>"#).unwrap()
});

static RAW_JSON_TOOL_CALL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)\{\s*"name"\s*:\s*"([A-Za-z0-9_:\-]+)"\s*,\s*"arguments"\s*:\s*(\{.*?\})\s*\}"#)
        .unwrap()
});

pub fn parse_text_as_tool_calls(text: &str) -> Option<Vec<ToolCall>> {
    let mut tool_calls = Vec::new();
    let text = text.trim();
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut push = |tc: ToolCall, tool_calls: &mut Vec<ToolCall>| {
        let key = format!("{}|{}", tc.name.as_ref(), tc.arguments);
        if seen_ids.insert(key) {
            tool_calls.push(tc);
        }
    };

    for cap in INVOKE_PATTERN.captures_iter(text) {
        let Some(m) = cap.get(0) else { continue };
        let Some(name_m) = cap.get(1) else { continue };
        let name = name_m.as_str().to_string();
        let tail = &text[m.end()..];
        let Some((args_str, _consumed)) = extract_first_json_object(tail) else {
            continue;
        };
        if let Ok(args) = serde_json::from_str::<serde_json::Value>(&args_str) {
            let id = uuid::Uuid::new_v4().to_string();
            push(
                ToolCall {
                    id: id.into(),
                    name: name.into(),
                    arguments: args,
                },
                &mut tool_calls,
            );
        }
    }

    for cap in CODE_BLOCK_PATTERN.captures_iter(text) {
        let name = cap.get(1)?.as_str().to_string();
        let args_str = cap.get(2)?.as_str();
        if let Ok(args) = serde_json::from_str(args_str) {
            let id = uuid::Uuid::new_v4().to_string();
            push(
                ToolCall {
                    id: id.into(),
                    name: name.into(),
                    arguments: args,
                },
                &mut tool_calls,
            );
        }
    }

    for cap in XML_TOOL_CALL_PATTERN.captures_iter(text) {
        let body = cap.get(1)?.as_str();
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
            let name = value
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let arguments = value
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
            if let Some(name) = name {
                let id = uuid::Uuid::new_v4().to_string();
                push(
                    ToolCall {
                        id: id.into(),
                        name: name.into(),
                        arguments,
                    },
                    &mut tool_calls,
                );
            }
        }
    }

    for cap in XML_TOOL_CALL_NAME_ATTR.captures_iter(text) {
        let name = cap.get(1)?.as_str().to_string();
        let inner = cap.get(2)?.as_str().trim();
        let arguments = if let Some((json_str, _)) = extract_first_json_object(inner) {
            serde_json::from_str(&json_str).unwrap_or_else(|_| {
                if inner.is_empty() {
                    serde_json::Value::Object(serde_json::Map::new())
                } else {
                    serde_json::Value::String(inner.to_string())
                }
            })
        } else if inner.is_empty() {
            serde_json::Value::Object(serde_json::Map::new())
        } else {
            serde_json::Value::String(inner.to_string())
        };
        let id = uuid::Uuid::new_v4().to_string();
        push(
            ToolCall {
                id: id.into(),
                name: name.into(),
                arguments,
            },
            &mut tool_calls,
        );
    }

    for cap in RAW_JSON_TOOL_CALL.captures_iter(text) {
        let name = cap.get(1)?.as_str().to_string();
        let args_str = cap.get(2)?.as_str();
        if let Ok(args) = serde_json::from_str::<serde_json::Value>(args_str) {
            let id = uuid::Uuid::new_v4().to_string();
            push(
                ToolCall {
                    id: id.into(),
                    name: name.into(),
                    arguments: args,
                },
                &mut tool_calls,
            );
        }
    }

    if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    }
}

fn extract_first_json_object(input: &str) -> Option<(String, usize)> {
    let bytes = input.as_bytes();
    let start = bytes.iter().position(|b| *b == b'{')?;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;

    for (i, b) in bytes.iter().enumerate().skip(start) {
        if escaped {
            escaped = false;
            continue;
        }
        match *b {
            b'\\' if in_str => escaped = true,
            b'"' => in_str = !in_str,
            b'{' if !in_str => depth += 1,
            b'}' if !in_str => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = i + 1;
                    let json = input[start..end].to_string();
                    return Some((json, end));
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_invoke_pattern() {
        let text = r#"invoke("bash", {"command": "ls"})"#;
        let result = parse_text_as_tool_calls(text);
        assert!(result.is_some());
        let calls = result.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name.as_ref(), "bash");
    }

    #[test]
    fn test_parse_code_block_pattern() {
        let text = r#"```bash
{"command": "ls"}
```"#;
        let result = parse_text_as_tool_calls(text);
        assert!(result.is_some());
        let calls = result.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name.as_ref(), "bash");
    }

    #[test]
    fn test_no_tool_calls() {
        let text = "Hello, this is just text.";
        let result = parse_text_as_tool_calls(text);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_invoke_pattern_mcp_tool_name() {
        let text = r#"invoke("mcp__github__list_issues", {"repo": "openai/codegg"})"#;
        let result = parse_text_as_tool_calls(text);
        assert!(result.is_some());
        let calls = result.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name.as_ref(), "mcp__github__list_issues");
        assert_eq!(calls[0].arguments["repo"], "openai/codegg");
    }

    #[test]
    fn test_parse_invoke_pattern_nested_json() {
        let text = r#"invoke("bash", {"command":"rg -n test src","options":{"timeout":60,"env":{"A":"B"}}})"#;
        let result = parse_text_as_tool_calls(text);
        assert!(result.is_some());
        let calls = result.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name.as_ref(), "bash");
        assert_eq!(calls[0].arguments["options"]["timeout"], 60);
        assert_eq!(calls[0].arguments["options"]["env"]["A"], "B");
    }

    #[test]
    fn test_parse_hermes_xml_tool_call() {
        let text = r#"I'll read the file.
<tool_call>
{"name": "read", "arguments": {"filePath": "README.md"}}
</tool_call>"#;
        let result = parse_text_as_tool_calls(text);
        assert!(result.is_some(), "should parse Hermes-style tool call");
        let calls = result.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name.as_ref(), "read");
        assert_eq!(calls[0].arguments["filePath"], "README.md");
    }

    #[test]
    fn test_parse_xml_tool_call_with_name_attr() {
        let text = r#"<tool_call name="bash">{"command": "ls -la"}</tool_call>"#;
        let result = parse_text_as_tool_calls(text);
        assert!(result.is_some(), "should parse name-attr tool call");
        let calls = result.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name.as_ref(), "bash");
        assert_eq!(calls[0].arguments["command"], "ls -la");
    }

    #[test]
    fn test_parse_xml_tool_call_name_attr_no_args() {
        let text = r#"<tool_call name="todowrite">
</tool_call>"#;
        let result = parse_text_as_tool_calls(text);
        assert!(result.is_some());
        let calls = result.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name.as_ref(), "todowrite");
    }

    #[test]
    fn test_parse_raw_json_tool_call() {
        let text =
            r#"Here is the call: {"name": "bash", "arguments": {"command": "pwd"}} end."#;
        let result = parse_text_as_tool_calls(text);
        assert!(result.is_some(), "should parse raw JSON tool call");
        let calls = result.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name.as_ref(), "bash");
        assert_eq!(calls[0].arguments["command"], "pwd");
    }

    #[test]
    fn test_parse_multiple_hermes_tool_calls() {
        let text = r#"
<tool_call>
{"name": "read", "arguments": {"filePath": "a.rs"}}
</tool_call>
<tool_call>
{"name": "read", "arguments": {"filePath": "b.rs"}}
</tool_call>
"#;
        let result = parse_text_as_tool_calls(text);
        assert!(result.is_some());
        let calls = result.unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].arguments["filePath"], "a.rs");
        assert_eq!(calls[1].arguments["filePath"], "b.rs");
    }

    #[test]
    fn test_deduplicates_identical_calls() {
        let text = r#"invoke("bash", {"command": "ls"})invoke("bash", {"command": "ls"})"#;
        let result = parse_text_as_tool_calls(text);
        assert!(result.is_some());
        let calls = result.unwrap();
        assert_eq!(calls.len(), 1, "duplicate calls should be collapsed");
    }
}
