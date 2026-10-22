use codegg::context::{CacheIdentity, ContextPlan, ContextPlanMode};
use codegg::provider::{ChatRequest, ContentPart, Message, ToolCall, ToolDefinition};
use serde_json::json;
use std::sync::Arc;

fn request(user: &str) -> ChatRequest {
    ChatRequest {
        messages: vec![
            Message::System {
                content: Arc::new("stable harness contract".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new(user.to_string()),
                }],
            },
            Message::Assistant {
                content: vec![],
                tool_calls: vec![ToolCall {
                    id: Arc::new("call-1".to_string()),
                    name: Arc::new("read".to_string()),
                    arguments: json!({"path": "src/lib.rs"}),
                }],
            },
            Message::Tool {
                tool_call_id: Arc::new("call-1".to_string()),
                content: Arc::new("bounded result".to_string()),
            },
        ],
        model: "example-model".to_string(),
        tools: Some(vec![ToolDefinition {
            name: "read".to_string(),
            description: "Read a file".to_string(),
            parameters: json!({"type": "object"}),
            defer_loading: None,
        }]),
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

#[test]
fn provider_request_is_lossless_and_chronological_after_plan_application() {
    let mut planned_request = request("first turn");
    let plan = ContextPlan::from_request(
        &planned_request,
        "example-provider",
        "adapter-v1",
        "compiler-v1",
        ContextPlanMode::Full,
    )
    .unwrap();
    plan.apply_to_request(&mut planned_request);

    assert!(matches!(
        planned_request.messages[2],
        Message::Assistant { .. }
    ));
    assert!(matches!(planned_request.messages[3], Message::Tool { .. }));
    assert_eq!(planned_request.tools.as_ref().unwrap().len(), 1);
    assert_eq!(plan.diagnostics().omitted_blocks, 0);
}

#[test]
fn stable_prefix_and_compound_cache_identity_ignore_volatile_tail_changes() {
    let first = ContextPlan::from_request(
        &request("first turn"),
        "example-provider",
        "adapter-v1",
        "compiler-v1",
        ContextPlanMode::Full,
    )
    .unwrap();
    let second = ContextPlan::from_request(
        &request("second turn"),
        "example-provider",
        "adapter-v1",
        "compiler-v1",
        ContextPlanMode::Full,
    )
    .unwrap();
    assert_eq!(first.stable_prefix_hash, second.stable_prefix_hash);
    assert_ne!(first.plan_fingerprint, second.plan_fingerprint);
    assert!(first.cache_key().contains("provider=example-provider"));
    assert!(first.cache_key().contains("adapter=adapter-v1"));
}

#[test]
fn diagnostics_do_not_include_private_content() {
    let mut req = request("SECRET_USER_CONTENT");
    req.messages.push(Message::Assistant {
        content: vec![ContentPart::Reasoning {
            text: Arc::new("PRIVATE_REASONING".to_string()),
            visibility: codegg::provider::ReasoningVisibility::Private,
        }],
        tool_calls: vec![],
    });
    let plan = ContextPlan::from_request(
        &req,
        "example-provider",
        "adapter-v1",
        "compiler-v1",
        ContextPlanMode::Full,
    )
    .unwrap();
    let diagnostics = format!("{:?}", plan.diagnostics());
    assert!(!diagnostics.contains("SECRET_USER_CONTENT"));
    assert!(!diagnostics.contains("PRIVATE_REASONING"));
}

#[test]
fn cache_identity_is_bounded_to_fingerprints() {
    let identity = CacheIdentity {
        provider: "p".into(),
        model: "m".into(),
        adapter: "a".into(),
        compiler: "c".into(),
        tool_surface: "t".into(),
        mode: ContextPlanMode::Full,
    };
    let key = identity.key();
    assert!(key.len() < 256);
    assert!(!key.contains("SECRET"));
}
