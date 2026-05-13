use crate::provider::ToolCall;
use regex::Regex;
use std::sync::LazyLock;

static INVOKE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"invoke\s*\(\s*"(\w+)"\s*,\s*(\{[^}]+\})"#).unwrap());

static CODE_BLOCK_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)```\s*(\w+)\s*(\{.*?\})\s*```"#).unwrap());

pub fn parse_text_as_tool_calls(text: &str) -> Option<Vec<ToolCall>> {
    let mut tool_calls = Vec::new();
    let text = text.trim();

    for cap in INVOKE_PATTERN.captures_iter(text) {
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
}
