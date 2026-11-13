use crate::provider::{ChatRequest, ContentPart, Message, ToolDefinition};

use super::artifact::stable_hash_hex;

/// Canonical field framing for every request-identity digest: a decimal byte
/// length, a colon, the bytes, then a newline. Two different inputs can never
/// share a concatenation, and the digest is over bytes rather than a Debug
/// rendering.
fn push_field(buf: &mut String, value: &str) {
    buf.push_str(&value.len().to_string());
    buf.push(':');
    buf.push_str(value);
    buf.push('\n');
}

/// The system prompt a dispatched request carries: the explicit `system`
/// field when one is set, otherwise the leading `system` message the agent
/// loop projects it into. Digesting the projection is what makes this the
/// identity of the value the provider receives, not of a rebuild.
pub fn dispatched_system_prompt(request: &ChatRequest) -> Option<&str> {
    if let Some(system) = request.system.as_deref() {
        return Some(system);
    }
    match request.messages.first() {
        Some(Message::System { content }) => Some(content.as_str()),
        _ => None,
    }
}

/// Canonical system-prompt digest: presence tag, then the text as one field.
pub fn system_prompt_digest(request: &ChatRequest) -> String {
    let mut buf = String::from("system\n");
    match dispatched_system_prompt(request) {
        Some(text) => {
            buf.push_str("present\n");
            push_field(&mut buf, text);
        }
        None => buf.push_str("absent\n"),
    }
    stable_hash_hex(buf)
}

/// Canonical message-range digest: role tag, part count, then every part and
/// tool call in order. Order-sensitive by construction.
pub fn message_range_digest(messages: &[Message]) -> String {
    let mut buf = String::from("messages\n");
    for message in messages {
        match message {
            Message::System { content } => {
                buf.push_str("system\n");
                push_field(&mut buf, content);
            }
            Message::User { content } => {
                buf.push_str("user\n");
                push_parts(&mut buf, content);
            }
            Message::Assistant {
                content,
                tool_calls,
            } => {
                buf.push_str("assistant\n");
                push_parts(&mut buf, content);
                push_field(&mut buf, &tool_calls.len().to_string());
                for call in tool_calls {
                    push_field(&mut buf, &call.id);
                    push_field(&mut buf, &call.name);
                    push_field(&mut buf, &canonicalize_json(&call.arguments));
                }
            }
            Message::Tool {
                tool_call_id,
                content,
            } => {
                buf.push_str("tool\n");
                push_field(&mut buf, tool_call_id);
                push_field(&mut buf, content);
            }
        }
    }
    stable_hash_hex(buf)
}

/// Canonical digest of one prompt section: its kind, then each content in
/// order, so two sections of the same kind cannot be confused.
pub fn section_digest(kind: &str, contents: &[&str]) -> String {
    let mut buf = String::from("section\n");
    push_field(&mut buf, kind);
    for content in contents {
        push_field(&mut buf, content);
    }
    stable_hash_hex(buf)
}

fn push_parts(buf: &mut String, parts: &[ContentPart]) {
    push_field(buf, &parts.len().to_string());
    for part in parts {
        match part {
            ContentPart::Text { text } => {
                buf.push_str("text\n");
                push_field(buf, text);
            }
            ContentPart::Image { image_url } => {
                buf.push_str("image\n");
                push_field(buf, &image_url.url);
            }
            ContentPart::Reasoning { text, .. } => {
                buf.push_str("reasoning\n");
                push_field(buf, text);
            }
        }
    }
}

/// A bounded rendering of the dispatched request, for the ledger's
/// failure-only body. Secrets are redacted where the body is stored, not here.
pub fn request_body_text(request: &ChatRequest) -> String {
    let mut out = String::new();
    out.push_str("model: ");
    out.push_str(&request.model);
    out.push('\n');
    if let Some(system) = dispatched_system_prompt(request) {
        out.push_str("system:\n");
        out.push_str(system);
        out.push('\n');
    }
    for (index, message) in request.messages.iter().enumerate() {
        if out.len() >= crate::session::request_ledger::REQUEST_LEDGER_MAX_BODY_BYTES {
            out.push_str("[body truncated]\n");
            break;
        }
        out.push_str(&format!("message[{index}] "));
        match message {
            Message::System { content } => {
                out.push_str("system:\n");
                out.push_str(content);
            }
            Message::User { content } => {
                out.push_str("user:\n");
                push_parts_text(&mut out, content);
            }
            Message::Assistant {
                content,
                tool_calls,
            } => {
                out.push_str("assistant:\n");
                push_parts_text(&mut out, content);
                for call in tool_calls {
                    out.push_str(&format!(
                        "tool_call {} {} {}\n",
                        call.id,
                        call.name,
                        canonicalize_json(&call.arguments)
                    ));
                }
            }
            Message::Tool {
                tool_call_id,
                content,
            } => {
                out.push_str(&format!("tool {tool_call_id}:\n"));
                out.push_str(content);
            }
        }
        out.push('\n');
    }
    out
}

fn push_parts_text(out: &mut String, parts: &[ContentPart]) {
    for part in parts {
        match part {
            ContentPart::Text { text } => out.push_str(text),
            ContentPart::Image { image_url } => out.push_str(&format!("[image {}]", image_url.url)),
            ContentPart::Reasoning { text, .. } => out.push_str(text),
        }
        out.push('\n');
    }
}

pub fn tool_definitions_hash(defs: &[ToolDefinition]) -> String {
    let mut sorted: Vec<&ToolDefinition> = defs.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    // Build canonical string form then feed to stable sha256 (full 64-hex).
    // This replaces the DefaultHasher path entirely.
    let mut buf = String::new();
    for def in sorted {
        buf.push_str(&def.name);
        buf.push(':');
        buf.push_str(&def.description);
        buf.push(':');

        let canonical = canonicalize_json(&def.parameters);
        buf.push_str(&canonical);
        buf.push(':');

        if let Some(d) = def.defer_loading {
            buf.push_str(&d.to_string());
        }
        buf.push(';');
    }

    stable_hash_hex(buf)
}

fn canonicalize_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted: Vec<_> = map.iter().collect();
            sorted.sort_by_key(|(k, _)| (*k).clone());
            let mut parts = Vec::new();
            for (k, v) in sorted {
                parts.push(format!("{}:{}", k, canonicalize_json(v)));
            }
            format!("{{{}}}", parts.join(","))
        }
        serde_json::Value::Array(arr) => {
            let inner: Vec<_> = arr.iter().map(canonicalize_json).collect();
            format!("[{}]", inner.join(","))
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool(name: &str, desc: &str, params: serde_json::Value) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: desc.to_string(),
            parameters: params,
            defer_loading: None,
        }
    }

    #[test]
    fn same_definitions_produce_same_hash() {
        let defs = vec![
            tool("bash", "Run bash", json!({})),
            tool("read", "Read file", json!({})),
        ];
        assert_eq!(tool_definitions_hash(&defs), tool_definitions_hash(&defs));
    }

    #[test]
    fn description_change_alters_hash() {
        let a = vec![tool("bash", "Run bash", json!({}))];
        let b = vec![tool("bash", "Execute bash", json!({}))];
        assert_ne!(tool_definitions_hash(&a), tool_definitions_hash(&b));
    }

    #[test]
    fn reordering_does_not_change_hash() {
        let a = vec![
            tool("bash", "Run bash", json!({})),
            tool("read", "Read file", json!({})),
        ];
        let b = vec![
            tool("read", "Read file", json!({})),
            tool("bash", "Run bash", json!({})),
        ];
        assert_eq!(tool_definitions_hash(&a), tool_definitions_hash(&b));
    }

    #[test]
    fn parameter_change_alters_hash() {
        let a = vec![tool("bash", "Run bash", json!({"type": "object"}))];
        let b = vec![tool("bash", "Run bash", json!({"type": "string"}))];
        assert_ne!(tool_definitions_hash(&a), tool_definitions_hash(&b));
    }

    #[test]
    fn defer_loading_change_alters_hash() {
        let mut a = tool("bash", "Run bash", json!({}));
        a.defer_loading = Some(false);
        let mut b = tool("bash", "Run bash", json!({}));
        b.defer_loading = Some(true);
        assert_ne!(tool_definitions_hash(&[a]), tool_definitions_hash(&[b]));
    }

    #[test]
    fn empty_definitions_produce_hash() {
        let hash = tool_definitions_hash(&[]);
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn json_key_order_insensitive() {
        let a = vec![tool("bash", "Run", json!({"a": 1, "b": 2}))];
        let b = vec![tool("bash", "Run", json!({"b": 2, "a": 1}))];
        assert_eq!(tool_definitions_hash(&a), tool_definitions_hash(&b));
    }

    fn request(system: Option<&str>, messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            messages,
            model: "test-model".to_string(),
            tools: None,
            system: system.map(str::to_string),
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: crate::provider::ProviderRequestContext::default(),
        }
    }

    fn system_message(text: &str) -> Message {
        Message::System {
            content: std::sync::Arc::new(text.to_string()),
        }
    }

    fn user_message(text: &str) -> Message {
        Message::User {
            content: vec![ContentPart::Text {
                text: std::sync::Arc::new(text.to_string()),
            }],
        }
    }

    #[test]
    fn the_system_digest_is_stable_and_moves_on_one_byte() {
        let base = request(Some("You are codegg."), vec![]);
        assert_eq!(
            system_prompt_digest(&base),
            system_prompt_digest(&base),
            "two identical inputs digest identically"
        );
        let changed = request(Some("You are codegg!"), vec![]);
        assert_ne!(
            system_prompt_digest(&base),
            system_prompt_digest(&changed),
            "a one-byte change moves the digest"
        );
        assert_ne!(
            system_prompt_digest(&request(None, vec![])),
            system_prompt_digest(&base),
            "an absent prompt is not an empty one"
        );
    }

    #[test]
    fn the_system_digest_reads_the_prompt_the_loop_projects_into_messages() {
        let projected = request(None, vec![system_message("You are codegg.")]);
        let explicit = request(Some("You are codegg."), vec![]);
        assert_eq!(
            system_prompt_digest(&projected),
            system_prompt_digest(&explicit),
            "the dispatched projection is digested, not a rebuild"
        );
        assert_eq!(
            dispatched_system_prompt(&projected),
            Some("You are codegg.")
        );
    }

    #[test]
    fn the_message_digest_is_stable_and_order_sensitive() {
        let forward = request(None, vec![user_message("first"), user_message("second")]);
        assert_eq!(
            message_range_digest(&forward.messages),
            message_range_digest(&forward.messages)
        );
        let reversed = request(None, vec![user_message("second"), user_message("first")]);
        assert_ne!(
            message_range_digest(&forward.messages),
            message_range_digest(&reversed.messages),
            "reordering the same messages is a different request"
        );
        let one_byte = request(None, vec![user_message("first"), user_message("second.")]);
        assert_ne!(
            message_range_digest(&forward.messages),
            message_range_digest(&one_byte.messages),
            "a one-byte change moves the digest"
        );
    }

    #[test]
    fn the_message_digest_covers_tool_calls_and_results() {
        let call = Message::Assistant {
            content: vec![],
            tool_calls: vec![crate::provider::ToolCall {
                id: std::sync::Arc::new("call-1".to_string()),
                name: std::sync::Arc::new("bash".to_string()),
                arguments: json!({"command": "cargo check"}),
            }],
        };
        let with_call = request(None, vec![call]);
        let without = request(None, vec![]);
        assert_ne!(
            message_range_digest(&with_call.messages),
            message_range_digest(&without.messages)
        );
        let result = request(
            None,
            vec![Message::Tool {
                tool_call_id: std::sync::Arc::new("call-1".to_string()),
                content: std::sync::Arc::new("ok".to_string()),
            }],
        );
        assert_ne!(
            message_range_digest(&result.messages),
            message_range_digest(&without.messages)
        );
    }

    #[test]
    fn the_section_digest_separates_kind_and_content() {
        let harness = section_digest("harness_contract", &["same text"]);
        let role = section_digest("role_contract", &["same text"]);
        assert_ne!(harness, role, "the kind is part of the identity");
        assert_eq!(harness, section_digest("harness_contract", &["same text"]));
        assert_ne!(
            harness,
            section_digest("harness_contract", &["same text."]),
            "a one-byte change moves the section digest"
        );
        assert_ne!(
            section_digest("k", &["a", "b"]),
            section_digest("k", &["ab"]),
            "length framing keeps concatenations distinct"
        );
    }

    #[test]
    fn the_body_rendering_carries_tool_arguments_for_redaction() {
        let body = request_body_text(&request(
            Some("prompt"),
            vec![
                user_message("run it"),
                Message::Assistant {
                    content: vec![],
                    tool_calls: vec![crate::provider::ToolCall {
                        id: std::sync::Arc::new("call-1".to_string()),
                        name: std::sync::Arc::new("bash".to_string()),
                        arguments: json!({"command": "export TOKEN=sk-live-42"}),
                    }],
                },
            ],
        ));
        assert!(body.contains("export TOKEN=sk-live-42"));
        assert!(body.contains("message[0] user:"));
        assert!(body.contains("tool_call call-1 bash"));
    }
}
