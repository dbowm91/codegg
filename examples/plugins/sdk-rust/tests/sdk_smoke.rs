use codegg_plugin_sdk::builders::*;
use codegg_protocol::plugin::{
    PluginCapabilityInvocation, PluginContext, PluginDiagnosticLevel, PluginInvocation,
    PluginResponse, PLUGIN_PROTOCOL_VERSION,
};

fn sample_invocation(capability: PluginCapabilityInvocation) -> PluginInvocation {
    PluginInvocation {
        protocol_version: PLUGIN_PROTOCOL_VERSION,
        invocation_id: "test-inv-1".into(),
        plugin_id: "test-plugin".into(),
        capability,
        args: vec![],
        input: serde_json::Value::Null,
        context: PluginContext::default(),
    }
}

#[test]
fn text_node_returns_text_variant() {
    let node = text_node("hello");
    assert_eq!(
        node,
        UiNode::Text(TextNode {
            text: "hello".into()
        })
    );
}

#[test]
fn table_node_returns_table_variant() {
    let node = table_node(
        vec!["a".into(), "b".into()],
        vec![vec!["1".into(), "2".into()]],
    );
    assert_eq!(
        node,
        UiNode::Table(TableNode {
            columns: vec!["a".into(), "b".into()],
            rows: vec![vec!["1".into(), "2".into()]],
        })
    );
}

#[test]
fn key_value_node_returns_key_value_variant() {
    let node = key_value_node(vec![("k".into(), "v".into())]);
    assert_eq!(
        node,
        UiNode::KeyValue(KeyValueNode {
            entries: vec![KeyValueEntry {
                key: "k".into(),
                value: "v".into(),
            }]
        })
    );
}

#[test]
fn response_chat_markdown_has_emit_chat_effect() {
    let resp = response_chat_markdown("## Hello");
    assert!(resp.ok);
    assert_eq!(resp.effects.len(), 1);
    match &resp.effects[0] {
        UiEffect::EmitChat { block } => {
            assert_eq!(block.format, ChatFormat::Markdown);
            assert_eq!(block.content, "## Hello");
        }
        other => panic!("expected EmitChat, got {:?}", other),
    }
}

#[test]
fn response_dialog_has_open_dialog_effect() {
    let resp = response_dialog("d1", "Title", text_node("body"), true);
    assert!(resp.ok);
    assert_eq!(resp.effects.len(), 1);
    match &resp.effects[0] {
        UiEffect::OpenDialog { dialog } => {
            assert_eq!(dialog.id, "d1");
            assert_eq!(dialog.title, "Title");
            assert!(dialog.modal);
        }
        other => panic!("expected OpenDialog, got {:?}", other),
    }
}

#[test]
fn error_response_has_ok_false_and_diagnostics() {
    let resp = error_response("something broke");
    assert!(!resp.ok);
    assert!(resp.effects.is_empty());
    assert_eq!(resp.diagnostics.len(), 1);
    assert_eq!(resp.diagnostics[0].level, PluginDiagnosticLevel::Error);
    assert_eq!(resp.diagnostics[0].message, "something broke");
}

#[test]
fn diagnostic_levels_serialize_correctly() {
    let cases = vec![
        (PluginDiagnosticLevel::Debug, "\"debug\""),
        (PluginDiagnosticLevel::Info, "\"info\""),
        (PluginDiagnosticLevel::Warning, "\"warning\""),
        (PluginDiagnosticLevel::Error, "\"error\""),
    ];
    for (level, expected) in cases {
        let json = serde_json::to_string(&level).unwrap();
        assert_eq!(json, expected);
    }
}

#[test]
fn response_round_trip_json() {
    let resp = PluginResponse {
        ok: true,
        effects: vec![
            UiEffect::EmitChat {
                block: ChatBlock {
                    format: ChatFormat::Plain,
                    content: "hi".into(),
                },
            },
            UiEffect::ShowToast {
                toast: ToastSpec {
                    level: ToastLevel::Success,
                    message: "done".into(),
                },
            },
        ],
        data: serde_json::json!({"key": "value"}),
        diagnostics: vec![PluginDiagnostic {
            level: PluginDiagnosticLevel::Info,
            message: "ok".into(),
        }],
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: PluginResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back, resp);
}

#[test]
fn invocation_round_trip_json() {
    let inv = sample_invocation(PluginCapabilityInvocation::Command {
        name: "greet".into(),
    });
    let json = serde_json::to_string(&inv).unwrap();
    let back: PluginInvocation = serde_json::from_str(&json).unwrap();
    assert_eq!(back, inv);
}

#[test]
fn progress_node_returns_progress_variant() {
    let node = progress_node(Some("loading"), 42, Some(100));
    assert_eq!(
        node,
        UiNode::Progress(ProgressNode {
            label: Some("loading".into()),
            current: 42,
            total: Some(100),
        })
    );
}

#[test]
fn ok_response_with_effects() {
    let resp = ok_response(
        vec![UiEffect::CloseDialog { id: "d1".into() }],
        serde_json::json!({"closed": true}),
    );
    assert!(resp.ok);
    assert_eq!(resp.effects.len(), 1);
    assert_eq!(resp.data, serde_json::json!({"closed": true}));
}

#[ignore]
#[test]
fn abi_invoke_requires_wasm_target() {
    let inv = sample_invocation(PluginCapabilityInvocation::Command {
        name: "test".into(),
    });
    let json = serde_json::to_vec(&inv).unwrap();
    let ptr = json.as_ptr() as i32;
    let len = json.len() as i32;
    let _packed = codegg_plugin_sdk::abi::do_invoke(ptr, len, |_inv| PluginResponse {
        ok: true,
        effects: vec![],
        data: serde_json::Value::Null,
        diagnostics: vec![],
    });
}
