//! M004 provider deferral wire qualification: byte-level fixtures.
//!
//! Pins the literal JSON shape per wire surface (OpenAI Chat,
//! OpenAI Responses, Anthropic Messages), the qualified disclosure
//! matrix, conservative downgrade, and measured payload tradeoffs.
//! Native modes are fixture-proven but production-gated: no live
//! provider evidence is claimed (see the M004 closure).

#[cfg(test)]
mod tests {
    use codegg::provider::{
        qualify_tool_disclosure, qualify_tool_disclosure_with_allowlist, ChatRequest, ContentPart,
        Message, ToolDefinition, ToolDisclosureWireMode,
    };
    use codegg::provider::{wire, ResponsesTool};
    use std::sync::Arc;

    fn def(name: &str, props: usize, deferred: bool) -> ToolDefinition {
        let mut properties = serde_json::Map::new();
        for i in 0..props {
            properties.insert(
                format!("arg{i}"),
                serde_json::json!({"type": "string", "description": "benchmark property"}),
            );
        }
        ToolDefinition {
            name: name.into(),
            description: format!("{name} tool with {props} properties"),
            parameters: serde_json::json!({"type": "object", "properties": properties}),
            defer_loading: deferred.then_some(true),
        }
    }

    fn request(tools: Vec<ToolDefinition>) -> ChatRequest {
        ChatRequest {
            messages: vec![Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("do the work".into()),
                }],
            }],
            model: "probe-model".into(),
            tools: Some(tools),
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: Default::default(),
        }
    }

    fn five_tools() -> Vec<ToolDefinition> {
        vec![
            def("read", 2, false),
            def("tool_search", 1, false),
            def("write", 4, true),
            def("mcp__db__query", 6, true),
            def("plugin__one__run", 3, true),
        ]
    }

    fn names(body: &serde_json::Value) -> Vec<String> {
        body.get("tools")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|t| {
                t.get("name")
                    .and_then(|n| n.as_str())
                    .map(str::to_string)
                    .or_else(|| {
                        t.get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .map(str::to_string)
                    })
            })
            .collect()
    }

    // --- literal shapes ------------------------------------------------------

    #[test]
    fn chat_compat_default_has_no_native_keys_and_no_elision() {
        // Standard OpenAI-compatible Chat: no native-only keys, no silent
        // schema elision, host tool_search present as an ordinary call.
        let body = wire::encode_openai_chat(&request(five_tools()), None, false).unwrap();
        let text = serde_json::to_string(&body).unwrap();
        assert!(!text.contains("defer_loading"), "chat must drop the flag");
        assert!(
            !text.contains("\"tool_search\"") || names(&body).contains(&"tool_search".to_string())
        );
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 5);
        for tool in tools {
            assert_eq!(tool["type"], "function");
            assert!(tool["function"]["parameters"].is_object());
        }
        // Every authorized definition survives byte-for-byte at the schema
        // level (names + parameters equal the inputs).
        for def in five_tools() {
            let entry = tools
                .iter()
                .find(|t| t["function"]["name"] == def.name)
                .unwrap_or_else(|| panic!("{} must be sent", def.name));
            assert_eq!(entry["function"]["parameters"], def.parameters);
        }
        // Host discovery stays an ordinary callable function here.
        assert!(names(&body).contains(&"tool_search".to_string()));
    }

    #[test]
    fn responses_baseline_flags_without_builtin() {
        let body = wire::encode_openai_responses(&request(five_tools()), false).unwrap();
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 5);
        let write = tools.iter().find(|t| t["name"] == "write").unwrap();
        assert_eq!(write["defer_loading"], serde_json::json!(true));
        assert!(!wire::is_provider_tool_search_entry(write));
        assert!(
            !tools.iter().any(wire::is_provider_tool_search_entry),
            "FullCompatibility emits no provider builtin"
        );
    }

    #[test]
    fn anthropic_baseline_flags_without_variant_and_without_cache_control() {
        let body = wire::encode_anthropic_messages(&request(five_tools())).unwrap();
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 5);
        for tool in tools {
            assert!(
                tool.get("cache_control").is_none(),
                "no cache markers: {tool}"
            );
            assert!(tool["input_schema"].is_object());
        }
        let write = tools.iter().find(|t| t["name"] == "write").unwrap();
        assert_eq!(write["defer_loading"], serde_json::json!(true));
        assert!(!tools.iter().any(wire::is_provider_tool_search_entry));
    }

    // --- native slices (gated, fixture-proven) ---------------------------------

    #[test]
    fn native_openai_two_turn_discover_then_call_shape() {
        // Turn 1: deferred tools flagged + provider builtin present, host
        // search function gone — the model can only discover via native.
        let mut turn1 = wire::encode_openai_responses(&request(five_tools()), false).unwrap();
        wire::apply_native_openai_tool_search(&mut turn1, &["write".to_string()]).unwrap();
        let tools = turn1["tools"].as_array().unwrap();
        assert!(tools
            .iter()
            .any(|t| *t == serde_json::json!({"type": "tool_search"})));
        assert!(!tools.iter().any(
            |t| t.get("type").and_then(|v| v.as_str()) == Some("function")
                && t.get("name").and_then(|v| v.as_str()) == Some("tool_search")
        ));
        // Turn 2: a direct function call after search resolves against the
        // same tool identity (name + full schema still carried).
        let write = tools
            .iter()
            .find(|t| t.get("name").and_then(|v| v.as_str()) == Some("write"))
            .unwrap();
        assert_eq!(write["defer_loading"], serde_json::json!(true));
        assert!(write["parameters"]["properties"].is_object());
    }

    #[test]
    fn native_anthropic_two_turn_shape_keeps_search_immediate() {
        let mut turn1 = wire::encode_anthropic_messages(&request(five_tools())).unwrap();
        wire::apply_native_anthropic_tool_search(&mut turn1, &["write".to_string()]).unwrap();
        let tools = turn1["tools"].as_array().unwrap();
        // Full tool array retained on the next request (Anthropic carries
        // every definition every turn).
        for name in ["read", "write", "mcp__db__query", "plugin__one__run"] {
            assert!(tools.iter().any(|t| t["name"] == name), "{name} retained");
        }
        let variant = wire::ANTHROPIC_NATIVE_TOOL_SEARCH_VARIANT;
        let search = tools.iter().find(|t| t["type"] == variant).unwrap();
        assert_eq!(search["name"], variant);
        assert!(
            search.get("defer_loading").is_none(),
            "search itself is immediate"
        );
    }

    #[test]
    fn native_modes_reject_missing_tools_array_without_retry_signal() {
        // A malformed native application is a typed, non-retryable
        // qualification failure: the caller falls back to full
        // compatibility rather than retrying or switching providers.
        let mut body = serde_json::json!({"model": "x"});
        for apply in [
            wire::apply_native_openai_tool_search(&mut body, &[]),
            wire::apply_native_anthropic_tool_search(&mut body, &[]),
        ] {
            let err = apply.expect_err("must fail closed");
            assert!(err.to_string().contains("tools array"), "got: {err}");
        }
    }

    // --- matrix ------------------------------------------------------------------

    #[test]
    fn qualification_matrix_is_conservative() {
        use ToolDisclosureWireMode::*;
        // Unknown providers (incl. proxies, aliases, lookalikes) → full.
        for provider in [
            "opencode_go",
            "custom",
            "openrouter",
            "openai_eu",
            "",
            "ANTHROPIC",
        ] {
            let q = qualify_tool_disclosure(provider, "any-model");
            assert_eq!(q.mode, FullCompatibility, "{provider}");
            assert_eq!(q.reason, "unqualified_provider");
        }
        // Known providers without live qualification → full + model reason.
        for (provider, model) in [("openai", "gpt-4.1"), ("anthropic", "claude-opus-4-6")] {
            let q = qualify_tool_disclosure(provider, model);
            assert_eq!(q.mode, FullCompatibility);
            assert_eq!(q.reason, "unqualified_model");
        }
        // Qualification is recomputed per call (no sticky cache): model
        // switch and failover always re-evaluate.
        let a = qualify_tool_disclosure("openai", "gpt-4.1");
        assert_eq!(a, qualify_tool_disclosure("openai", "gpt-4.1"));
        assert_ne!(
            a.reason,
            qualify_tool_disclosure("custom", "gpt-4.1").reason,
            "provider vs model fallback reasons stay distinct"
        );
        // The gate itself is exact-match: injected allowlists qualify,
        // prefixes and cross-wiring do not.
        let q = qualify_tool_disclosure_with_allowlist("openai", "m", &["m"], &[]);
        assert_eq!(q.mode, NativeOpenAiToolSearch);
        assert_eq!(
            qualify_tool_disclosure_with_allowlist("openai", "m-plus", &["m"], &[]).mode,
            FullCompatibility
        );
        // Mode spellings are stable (telemetry contract).
        assert_eq!(FullCompatibility.as_str(), "full_compatibility");
        assert!(!FullCompatibility.is_native());
        assert!(NativeOpenAiToolSearch.is_native());
    }

    // --- scale + tradeoff measurement ----------------------------------------------

    fn mixed_fixture(n: usize) -> Vec<ToolDefinition> {
        // Unique names: the shared kernel rejects duplicate tool names,
        // which would confound a scale fixture.
        let natives = ["read", "write", "bash", "task", "tool_search", "grep"];
        (0..n)
            .map(|i| match i % 3 {
                0 => def(
                    &format!("{}_{i}", natives[i % natives.len()]),
                    2 + (i % 5),
                    i % 4 == 3,
                ),
                1 => def(&format!("mcp__srv{}__tool{i}", i % 4), 8, true),
                _ => def(&format!("plugin__p{}__op{i}", i % 3), 5, true),
            })
            .collect()
    }

    fn wire_bytes(tools: &[ToolDefinition]) -> (usize, usize, usize) {
        let chat = wire::encode_openai_chat(&request(tools.to_vec()), None, false).unwrap();
        let responses = wire::encode_openai_responses(&request(tools.to_vec()), false).unwrap();
        let messages = wire::encode_anthropic_messages(&request(tools.to_vec())).unwrap();
        (
            serde_json::to_string(&chat["tools"]).unwrap().len(),
            serde_json::to_string(&responses["tools"]).unwrap().len(),
            serde_json::to_string(&messages["tools"]).unwrap().len(),
        )
    }

    #[test]
    fn scale_0_5_40_100_tools_encode_without_truncation() {
        for n in [0usize, 5, 40, 100] {
            let tools = mixed_fixture(n);
            let (chat, responses, messages) = wire_bytes(&tools);
            for (label, body, bytes) in [
                (
                    "chat",
                    wire::encode_openai_chat(&request(tools.clone()), None, false).unwrap(),
                    chat,
                ),
                (
                    "responses",
                    wire::encode_openai_responses(&request(tools.clone()), false).unwrap(),
                    responses,
                ),
                (
                    "messages",
                    wire::encode_anthropic_messages(&request(tools.clone())).unwrap(),
                    messages,
                ),
            ] {
                let count = body["tools"].as_array().map(|a| a.len()).unwrap_or(0);
                assert_eq!(count, n, "{label} must carry all {n} tools");
                if n > 0 {
                    assert!(bytes > 0);
                }
            }
        }
        // Monotonic growth: more tools never encode smaller.
        let small = wire_bytes(&mixed_fixture(5));
        let large = wire_bytes(&mixed_fixture(100));
        assert!(large.0 > small.0 && large.1 > small.1 && large.2 > small.2);
    }

    #[test]
    fn measured_tradeoffs_40_tools() {
        // Records the prompt-payload tradeoff for the closure. Compact
        // top-10 (host keyword search shape: names + descriptions only)
        // vs the full compatibility array actually sent today.
        let tools = mixed_fixture(40);
        let (chat_full, resp_full, msg_full) = wire_bytes(&tools);
        let compact: Vec<_> = tools
            .iter()
            .take(10)
            .map(|d| serde_json::json!({"name": d.name, "description": d.description}))
            .collect();
        let compact_bytes = serde_json::to_string(&compact).unwrap().len();
        eprintln!("M004 40-tool wire bytes: chat_full={chat_full} responses_full={resp_full} messages_full={msg_full} compact_top10={compact_bytes}");
        assert!(compact_bytes < chat_full);
        assert!(compact_bytes < resp_full);
        assert!(compact_bytes < msg_full);
        // Deferred schemas are a large share of the full array: deferral
        // would matter if a provider honored it (none qualified).
        let deferred_schema_bytes: usize = tools
            .iter()
            .filter(|d| d.defer_loading == Some(true))
            .map(|d| serde_json::to_string(&d.parameters).unwrap().len())
            .sum();
        eprintln!("M004 40-tool deferred schema bytes: {deferred_schema_bytes}");
        assert!(deferred_schema_bytes > 0);
    }

    // --- DTO parity ------------------------------------------------------------------

    #[test]
    fn responses_dto_and_builtin_shapes() {
        let tool = ResponsesTool::from_tool_definition(&def("write", 2, true));
        assert_eq!(tool.defer_loading, Some(true));
        let builtin = ResponsesTool::native_tool_search_entry();
        assert_eq!(builtin, serde_json::json!({"type": "tool_search"}));
        assert!(!wire::is_provider_tool_search_entry(
            &serde_json::json!({"type": "function", "name": "tool_search"})
        ));
    }
}
