use crate::provider::ToolCall;
use regex::Regex;
use std::sync::LazyLock;

static INVOKE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"invoke\s*\(\s*"([A-Za-z0-9_:\-]+)"\s*,\s*"#).unwrap());

static CODE_BLOCK_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)```\s*(\w+)\s*(\{.*?\})\s*```"#).unwrap());

pub fn parse_text_as_tool_calls(text: &str) -> Option<Vec<ToolCall>> {
    let mut tool_calls = Vec::new();
    let text = text.trim();

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
            tool_calls.push(ToolCall {
                id: id.into(),
                name: name.into(),
                arguments: args,
            });
        }
    }

    for cap in CODE_BLOCK_PATTERN.captures_iter(text) {
        let name = cap.get(1)?.as_str().to_string();
        let args_str = cap.get(2)?.as_str();
        if let Ok(args) = serde_json::from_str(args_str) {
            let id = uuid::Uuid::new_v4().to_string();
            tool_calls.push(ToolCall {
                id: id.into(),
                name: name.into(),
                arguments: args,
            });
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
}
