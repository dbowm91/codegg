/// Protocol version for the remote TUI WebSocket interface.
///
/// Bumped to 2 in Phase 10: added `PluginUiEffect` variant for
/// frontend-neutral plugin UI transport. Bumped to 3 in Phase 15:
/// `PluginUiEffect` now carries a [`UiEffectEnvelope`] (typed source)
/// and `RemotePanelView`/`RemoteStatusItemView` carry an optional body
/// for durable reconnect fidelity. Old clients safely ignore unknown
/// `#[serde(tag = "type")]` variants, so this is informational.
pub const REMOTE_TUI_PROTOCOL_VERSION: u32 = 3;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(tag = "type")]
#[allow(clippy::large_enum_variant)]
pub enum TuiMessage {
    EventEnvelope {
        event_seq: u64,
        payload: Box<TuiMessage>,
    },
    Input {
        text: String,
    },
    KeyDown {
        key: String,
        modifiers: Vec<String>,
    },
    MouseClick {
        x: u16,
        y: u16,
    },
    Resize {
        w: u16,
        h: u16,
    },
    Resume {
        from_event_seq: u64,
    },
    PermissionResponse {
        id: String,
        choice: String,
    },
    QuestionResponse {
        id: String,
        answers: serde_json::Value,
    },
    RenderFrame {
        content: String,
    },
    TextDelta {
        delta: String,
    },
    PermissionPending {
        id: String,
        tool: String,
        path: Option<String>,
    },
    QuestionPending {
        id: String,
        questions: Vec<QuestionSpec>,
    },
    SessionInfo {
        id: String,
        model: String,
    },
    SessionEnded {
        stop_reason: String,
    },
    ToolCallStarted {
        tool_name: String,
        tool_id: String,
        arguments: String,
    },
    ToolResult {
        tool_id: String,
        output: String,
        success: bool,
    },
    /// A plugin produced a UI effect. Consumed by the TUI to apply
    /// through the same `apply_plugin_ui_effect` path as local effects.
    /// Phase 15: carries a typed [`crate::ui::UiEffectEnvelope`] so the
    /// origin (Plugin/Core/Tui) is encoded on the wire and ownership
    /// checks can be enforced uniformly across embedded and remote TUI.
    #[serde(rename = "PluginUiEffect")]
    PluginUiEffect {
        envelope: crate::ui::UiEffectEnvelope,
    },
    Error {
        message: String,
    },
    StateSnapshot {
        sequence: u64,
        snapshot: RemoteTuiStateSnapshot,
    },
    RequestSnapshot {
        reason: Option<String>,
    },
    #[serde(rename = "resync_required")]
    ResyncRequired {
        reason: Option<String>,
        pending_permissions: Vec<String>,
        pending_questions: Vec<String>,
    },
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct QuestionSpec {
    pub id: String,
    pub prompt: String,
    pub default: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct RemoteTuiStateSnapshot {
    pub protocol_version: u32,
    pub sequence: u64,
    pub session_id: Option<String>,
    pub route: String,
    pub model: String,
    pub agent: String,
    pub status: String,
    pub messages: Vec<RemoteMessageView>,
    pub prompt: String,
    pub dialog: Option<String>,
    pub toasts: Vec<RemoteToastView>,
    /// Cached git sidebar state (root, branch, dirty). Refreshed
    /// asynchronously on the server; the value here is the most
    /// recent successful refresh.
    pub git: Option<RemoteGitInfo>,
    /// Durable plugin panels open in the current session.
    #[serde(default)]
    pub plugin_panels: Vec<RemotePanelView>,
    /// Durable plugin status items in the current session.
    #[serde(default)]
    pub plugin_status_items: Vec<RemoteStatusItemView>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct RemotePanelView {
    pub id: String,
    pub title: String,
    pub placement: String,
    /// Plugin that owns this surface. Optional to allow the snapshot to
    /// represent built-in / synthetic panels; populated for any panel
    /// owned by an installed or builtin plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_plugin_id: Option<String>,
    /// Body payload, included only when it fits within the snapshot
    /// body size cap (see [`crate::ui::UiLimits::max_snapshot_body_bytes`]).
    /// `None` means metadata only and the client should resync on demand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<crate::ui::UiNode>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct RemoteStatusItemView {
    pub id: String,
    pub label: Option<String>,
    pub placement: String,
    /// Plugin that owns this status item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_plugin_id: Option<String>,
    /// Body payload, included only when it fits within the snapshot
    /// body size cap. `None` means metadata only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<crate::ui::UiNode>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct RemoteGitInfo {
    pub root: Option<String>,
    pub branch: Option<String>,
    pub dirty: bool,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct RemoteMessageView {
    pub role: String,
    pub content_preview: String,
    pub tool_calls: Vec<RemoteToolCallView>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct RemoteToolCallView {
    pub tool_id: String,
    pub tool_name: String,
    pub status: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct RemoteToastView {
    pub message: String,
    pub level: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        ChatBlock, ChatFormat, DialogSpec, PanelPlacement, PanelSpec, StatusItemSpec,
        StatusPlacement, TextNode, UiEffect, UiEffectEnvelope, UiEffectSource, UiNode,
    };

    #[test]
    fn tui_message_plugin_ui_effect_round_trip() {
        let effect = UiEffect::OpenDialog {
            dialog: DialogSpec {
                id: "test-dlg".into(),
                title: "Test".into(),
                body: UiNode::Text(TextNode { text: "hi".into() }),
                modal: true,
            },
        };
        let msg = TuiMessage::PluginUiEffect {
            envelope: UiEffectEnvelope {
                session_id: Some("s1".into()),
                source: UiEffectSource::Plugin {
                    plugin_id: "p1".into(),
                },
                invocation_id: Some("inv-1".into()),
                effect,
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("PluginUiEffect"));
        assert!(json.contains("p1"));
        let back: TuiMessage = serde_json::from_str(&json).unwrap();
        match back {
            TuiMessage::PluginUiEffect { envelope } => {
                assert_eq!(
                    envelope.source,
                    UiEffectSource::Plugin {
                        plugin_id: "p1".into()
                    }
                );
                assert_eq!(envelope.invocation_id.as_deref(), Some("inv-1"));
                assert_eq!(envelope.session_id.as_deref(), Some("s1"));
            }
            other => panic!("expected PluginUiEffect, got {:?}", other),
        }
    }

    #[test]
    fn tui_message_plugin_ui_effect_accepts_core_source() {
        let msg = TuiMessage::PluginUiEffect {
            envelope: UiEffectEnvelope {
                session_id: None,
                source: UiEffectSource::Core,
                invocation_id: None,
                effect: UiEffect::EmitChat {
                    block: ChatBlock {
                        format: ChatFormat::Plain,
                        content: "core says hi".into(),
                    },
                },
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("core"));
        let back: TuiMessage = serde_json::from_str(&json).unwrap();
        match back {
            TuiMessage::PluginUiEffect { envelope } => {
                assert_eq!(envelope.source, UiEffectSource::Core);
                assert!(matches!(envelope.effect, UiEffect::EmitChat { .. }));
            }
            other => panic!("expected PluginUiEffect, got {:?}", other),
        }
    }

    #[test]
    fn remote_panel_view_with_body_round_trips() {
        let view = RemotePanelView {
            id: "my-plugin:panel-1".into(),
            title: "My Panel".into(),
            placement: "right".into(),
            source_plugin_id: Some("my-plugin".into()),
            body: Some(UiNode::Text(TextNode {
                text: "hello".into(),
            })),
        };
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("source_plugin_id"));
        assert!(json.contains("hello"));
        let back: RemotePanelView = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source_plugin_id.as_deref(), Some("my-plugin"));
        assert!(matches!(back.body, Some(UiNode::Text(_))));
    }

    #[test]
    fn remote_status_item_view_with_body_round_trips() {
        let view = RemoteStatusItemView {
            id: "my-plugin:status-1".into(),
            label: Some("Build".into()),
            placement: "right".into(),
            source_plugin_id: Some("my-plugin".into()),
            body: Some(UiNode::Empty),
        };
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("Build"));
        let back: RemoteStatusItemView = serde_json::from_str(&json).unwrap();
        assert_eq!(back.label.as_deref(), Some("Build"));
        assert_eq!(back.source_plugin_id.as_deref(), Some("my-plugin"));
    }

    #[test]
    fn remote_panel_view_legacy_without_body_still_deserializes() {
        // Old snapshot JSON without source_plugin_id/body fields must
        // still be accepted by Phase 15+ clients (forward compat).
        let json = r#"{
            "id": "legacy-panel",
            "title": "Legacy",
            "placement": "left"
        }"#;
        let view: RemotePanelView = serde_json::from_str(json).unwrap();
        assert_eq!(view.id, "legacy-panel");
        assert_eq!(view.title, "Legacy");
        assert_eq!(view.placement, "left");
        assert!(view.source_plugin_id.is_none());
        assert!(view.body.is_none());
    }

    #[test]
    fn snapshot_with_plugin_durable_surfaces_serializes() {
        let snapshot = RemoteTuiStateSnapshot {
            protocol_version: REMOTE_TUI_PROTOCOL_VERSION,
            sequence: 42,
            session_id: Some("s1".into()),
            route: "session:s1".into(),
            model: "m".into(),
            agent: "build".into(),
            status: "idle".into(),
            messages: vec![],
            prompt: String::new(),
            dialog: None,
            toasts: vec![],
            git: None,
            plugin_panels: vec![RemotePanelView {
                id: "p1:panel".into(),
                title: "P".into(),
                placement: "right".into(),
                source_plugin_id: Some("p1".into()),
                body: Some(UiNode::Empty),
            }],
            plugin_status_items: vec![RemoteStatusItemView {
                id: "p1:status".into(),
                label: Some("S".into()),
                placement: "right".into(),
                source_plugin_id: Some("p1".into()),
                body: None,
            }],
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("plugin_panels"));
        assert!(json.contains("source_plugin_id"));
        let back: RemoteTuiStateSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.plugin_panels.len(), 1);
        assert_eq!(back.plugin_status_items.len(), 1);
    }

    #[test]
    fn remote_tui_protocol_version_is_three() {
        assert_eq!(REMOTE_TUI_PROTOCOL_VERSION, 3);
    }

    #[test]
    fn panel_spec_supports_used_in_envelope() {
        let _ = PanelSpec {
            id: "x".into(),
            title: "x".into(),
            placement: PanelPlacement::Left,
            body: UiNode::Empty,
        };
        let _ = StatusItemSpec {
            id: "x".into(),
            label: None,
            placement: StatusPlacement::Right,
            body: UiNode::Empty,
        };
    }
}
