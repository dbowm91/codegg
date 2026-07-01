use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiNode {
    Text(TextNode),
    Markdown(MarkdownNode),
    Code(CodeNode),
    Table(TableNode),
    KeyValue(KeyValueNode),
    Progress(ProgressNode),
    Container(ContainerNode),
    Empty,
    Unsupported {
        unknown_kind: String,
        data: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextNode {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarkdownNode {
    pub markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeNode {
    pub language: Option<String>,
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableNode {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyValueNode {
    pub entries: Vec<KeyValueEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyValueEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProgressNode {
    pub label: Option<String>,
    pub current: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContainerNode {
    pub title: Option<String>,
    pub children: Vec<UiNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEffect {
    EmitChat { block: ChatBlock },
    ShowToast { toast: ToastSpec },
    OpenDialog { dialog: DialogSpec },
    CloseDialog { id: String },
    OpenPanel { panel: PanelSpec },
    UpdatePanel { id: String, body: UiNode },
    ClosePanel { id: String },
    AddStatusItem { item: StatusItemSpec },
    UpdateStatusItem { id: String, body: UiNode },
    RemoveStatusItem { id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatBlock {
    pub format: ChatFormat,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChatFormat {
    Plain,
    Markdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToastSpec {
    pub level: ToastLevel,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DialogSpec {
    pub id: String,
    pub title: String,
    pub body: UiNode,
    pub modal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PanelSpec {
    pub id: String,
    pub title: String,
    pub placement: PanelPlacement,
    pub body: UiNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PanelPlacement {
    Left,
    Right,
    Bottom,
    Main,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatusItemSpec {
    pub id: String,
    pub label: Option<String>,
    pub placement: StatusPlacement,
    pub body: UiNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StatusPlacement {
    Left,
    Right,
}

/// Envelope wrapping a [`UiEffect`] with session-scoped metadata for
/// transport through the core event stream or remote TUI protocol.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiEffectEnvelope {
    /// Optional session this effect belongs to.
    pub session_id: Option<String>,
    /// Where the effect originated.
    pub source: UiEffectSource,
    /// Optional invocation that produced this effect.
    pub invocation_id: Option<String>,
    /// The effect payload.
    pub effect: UiEffect,
}

/// Identifies the origin of a [`UiEffect`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEffectSource {
    Plugin { plugin_id: String },
    Core,
    Tui,
}

/// Capability flags that a client advertises for plugin UI rendering.
///
/// Clients that do not support a given surface type should degrade
/// deterministically or omit the surface entirely.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PluginUiCapabilities {
    #[serde(default)]
    pub dialog: bool,
    #[serde(default)]
    pub toast: bool,
    #[serde(default)]
    pub panel: bool,
    #[serde(default)]
    pub status_item: bool,
    #[serde(default)]
    pub table: bool,
    #[serde(default)]
    pub markdown: bool,
    #[serde(default)]
    pub code: bool,
    #[serde(default)]
    pub progress: bool,
}

impl PluginUiCapabilities {
    /// Returns a capabilities set where all surface types are supported.
    /// Use this as the default for clients known to handle all UI effects.
    pub fn all_supported() -> Self {
        Self {
            dialog: true,
            toast: true,
            panel: true,
            status_item: true,
            table: true,
            markdown: true,
            code: true,
            progress: true,
        }
    }

    /// Returns true if the client supports the surface type required by
    /// the given effect. Unknown effects are treated as unsupported.
    pub fn supports_effect(&self, effect: &UiEffect) -> bool {
        match effect {
            UiEffect::EmitChat { .. } | UiEffect::ShowToast { .. } => self.toast,
            UiEffect::OpenDialog { .. } | UiEffect::CloseDialog { .. } => self.dialog,
            UiEffect::OpenPanel { .. }
            | UiEffect::UpdatePanel { .. }
            | UiEffect::ClosePanel { .. } => self.panel,
            UiEffect::AddStatusItem { .. }
            | UiEffect::UpdateStatusItem { .. }
            | UiEffect::RemoveStatusItem { .. } => self.status_item,
        }
    }
}

/// Degrade a [`UiNode`] to plain text lines when the client does not
/// support the specific node type.
pub fn degrade_node_to_text(node: &UiNode) -> Vec<String> {
    match node {
        UiNode::Text(t) => vec![t.text.clone()],
        UiNode::Markdown(m) => vec![m.markdown.clone()],
        UiNode::Code(c) => {
            let mut lines = vec![];
            if let Some(lang) = &c.language {
                lines.push(format!("[{}]", lang));
            }
            lines.extend(c.code.lines().map(|l| l.to_string()));
            lines
        }
        UiNode::Table(t) => {
            let mut lines = vec![];
            lines.push(t.columns.join(" | "));
            lines.push(
                t.columns
                    .iter()
                    .map(|_| "---")
                    .collect::<Vec<_>>()
                    .join(" | "),
            );
            for row in &t.rows {
                lines.push(row.join(" | "));
            }
            lines
        }
        UiNode::KeyValue(kv) => kv
            .entries
            .iter()
            .map(|e| format!("{}: {}", e.key, e.value))
            .collect(),
        UiNode::Progress(p) => {
            let label = p.label.as_deref().unwrap_or("progress");
            match p.total {
                Some(total) => vec![format!("{} {}/{}", label, p.current, total)],
                None => vec![format!("{} {}", label, p.current)],
            }
        }
        UiNode::Container(c) => {
            let mut lines = vec![];
            if let Some(title) = &c.title {
                lines.push(format!("--- {} ---", title));
            }
            for child in &c.children {
                lines.extend(degrade_node_to_text(child));
            }
            lines
        }
        UiNode::Empty => vec![],
        UiNode::Unsupported { unknown_kind, .. } => {
            vec![format!("[unsupported: {}]", unknown_kind)]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_node_table_round_trip() {
        let node = UiNode::Table(TableNode {
            columns: vec!["name".into(), "version".into()],
            rows: vec![
                vec!["foo".into(), "1.0".into()],
                vec!["bar".into(), "2.0".into()],
            ],
        });
        let json = serde_json::to_string(&node).unwrap();
        assert!(json.contains("table"));
        assert!(json.contains("name"));
        let back: UiNode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, node);
    }

    #[test]
    fn ui_effect_open_dialog_round_trip() {
        let effect = UiEffect::OpenDialog {
            dialog: DialogSpec {
                id: "test-dialog".into(),
                title: "Test Dialog".into(),
                body: UiNode::Text(TextNode {
                    text: "hello".into(),
                }),
                modal: true,
            },
        };
        let json = serde_json::to_string(&effect).unwrap();
        assert!(json.contains("open_dialog"));
        assert!(json.contains("test-dialog"));
        let back: UiEffect = serde_json::from_str(&json).unwrap();
        assert_eq!(back, effect);
    }

    #[test]
    fn unsupported_node_round_trip() {
        let node = UiNode::Unsupported {
            unknown_kind: "tree".into(),
            data: serde_json::json!({"nodes": []}),
        };
        let json = serde_json::to_string(&node).unwrap();
        let back: UiNode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, node);
    }

    #[test]
    fn container_node_round_trip() {
        let node = UiNode::Container(ContainerNode {
            title: Some("My Container".into()),
            children: vec![
                UiNode::Text(TextNode {
                    text: "child".into(),
                }),
                UiNode::Empty,
            ],
        });
        let json = serde_json::to_string(&node).unwrap();
        let back: UiNode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, node);
    }

    #[test]
    fn progress_node_round_trip() {
        let node = UiNode::Progress(ProgressNode {
            label: Some("downloading".into()),
            current: 50,
            total: Some(100),
        });
        let json = serde_json::to_string(&node).unwrap();
        let back: UiNode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, node);
    }

    #[test]
    fn key_value_node_round_trip() {
        let node = UiNode::KeyValue(KeyValueNode {
            entries: vec![
                KeyValueEntry {
                    key: "k1".into(),
                    value: "v1".into(),
                },
                KeyValueEntry {
                    key: "k2".into(),
                    value: "v2".into(),
                },
            ],
        });
        let json = serde_json::to_string(&node).unwrap();
        let back: UiNode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, node);
    }

    #[test]
    fn code_node_round_trip() {
        let node = UiNode::Code(CodeNode {
            language: Some("rust".into()),
            code: "fn main() {}".into(),
        });
        let json = serde_json::to_string(&node).unwrap();
        let back: UiNode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, node);
    }

    #[test]
    fn markdown_node_round_trip() {
        let node = UiNode::Markdown(MarkdownNode {
            markdown: "# Hello\n\nWorld".into(),
        });
        let json = serde_json::to_string(&node).unwrap();
        let back: UiNode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, node);
    }

    #[test]
    fn effect_show_toast_round_trip() {
        let effect = UiEffect::ShowToast {
            toast: ToastSpec {
                level: ToastLevel::Warning,
                message: "careful!".into(),
            },
        };
        let json = serde_json::to_string(&effect).unwrap();
        assert!(json.contains("show_toast"));
        let back: UiEffect = serde_json::from_str(&json).unwrap();
        assert_eq!(back, effect);
    }

    #[test]
    fn effect_close_dialog_round_trip() {
        let effect = UiEffect::CloseDialog { id: "dlg-1".into() };
        let json = serde_json::to_string(&effect).unwrap();
        let back: UiEffect = serde_json::from_str(&json).unwrap();
        assert_eq!(back, effect);
    }

    #[test]
    fn panel_placement_serializes_snake_case() {
        let p = PanelPlacement::Bottom;
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, "\"bottom\"");
    }

    #[test]
    fn toast_level_serializes_snake_case() {
        let t = ToastLevel::Error;
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "\"error\"");
    }

    #[test]
    fn ui_effect_envelope_round_trip() {
        let env = UiEffectEnvelope {
            session_id: Some("s1".into()),
            source: UiEffectSource::Plugin {
                plugin_id: "my-plugin".into(),
            },
            invocation_id: Some("inv-1".into()),
            effect: UiEffect::ShowToast {
                toast: ToastSpec {
                    level: ToastLevel::Info,
                    message: "hello".into(),
                },
            },
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("plugin"));
        assert!(json.contains("my-plugin"));
        let back: UiEffectEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn ui_effect_source_plugin_serializes() {
        let src = UiEffectSource::Plugin {
            plugin_id: "p1".into(),
        };
        let json = serde_json::to_string(&src).unwrap();
        assert!(json.contains("plugin"));
        assert!(json.contains("p1"));
        let back: UiEffectSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back, src);
    }

    #[test]
    fn ui_effect_source_core_serializes() {
        let src = UiEffectSource::Core;
        let json = serde_json::to_string(&src).unwrap();
        assert!(json.contains("core"));
        let back: UiEffectSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back, src);
    }

    #[test]
    fn plugin_ui_capabilities_default_all_false() {
        let caps = PluginUiCapabilities::default();
        assert!(!caps.dialog);
        assert!(!caps.toast);
        assert!(!caps.panel);
        assert!(!caps.status_item);
        assert!(!caps.table);
        assert!(!caps.markdown);
        assert!(!caps.code);
        assert!(!caps.progress);
    }

    #[test]
    fn plugin_ui_capabilities_supports_effect() {
        let caps = PluginUiCapabilities {
            dialog: true,
            toast: true,
            ..Default::default()
        };
        assert!(caps.supports_effect(&UiEffect::OpenDialog {
            dialog: DialogSpec {
                id: "d".into(),
                title: "t".into(),
                body: UiNode::Empty,
                modal: true,
            }
        }));
        assert!(caps.supports_effect(&UiEffect::ShowToast {
            toast: ToastSpec {
                level: ToastLevel::Info,
                message: "m".into()
            }
        }));
        assert!(!caps.supports_effect(&UiEffect::OpenPanel {
            panel: PanelSpec {
                id: "p".into(),
                title: "t".into(),
                placement: PanelPlacement::Left,
                body: UiNode::Empty,
            }
        }));
    }

    #[test]
    fn degrade_text_node() {
        let lines = degrade_node_to_text(&UiNode::Text(TextNode {
            text: "hello".into(),
        }));
        assert_eq!(lines, vec!["hello"]);
    }

    #[test]
    fn degrade_code_node_with_language() {
        let lines = degrade_node_to_text(&UiNode::Code(CodeNode {
            language: Some("rust".into()),
            code: "fn main() {}".into(),
        }));
        assert_eq!(lines, vec!["[rust]", "fn main() {}"]);
    }

    #[test]
    fn degrade_table_node() {
        let lines = degrade_node_to_text(&UiNode::Table(TableNode {
            columns: vec!["a".into(), "b".into()],
            rows: vec![vec!["1".into(), "2".into()]],
        }));
        assert_eq!(lines, vec!["a | b", "--- | ---", "1 | 2"]);
    }

    #[test]
    fn degrade_key_value_node() {
        let lines = degrade_node_to_text(&UiNode::KeyValue(KeyValueNode {
            entries: vec![KeyValueEntry {
                key: "k".into(),
                value: "v".into(),
            }],
        }));
        assert_eq!(lines, vec!["k: v"]);
    }

    #[test]
    fn degrade_progress_node_with_total() {
        let lines = degrade_node_to_text(&UiNode::Progress(ProgressNode {
            label: Some("loading".into()),
            current: 50,
            total: Some(100),
        }));
        assert_eq!(lines, vec!["loading 50/100"]);
    }

    #[test]
    fn degrade_empty_node() {
        let lines = degrade_node_to_text(&UiNode::Empty);
        assert!(lines.is_empty());
    }

    #[test]
    fn all_supported_caps_pass_every_effect_type() {
        let caps = PluginUiCapabilities::all_supported();
        assert!(caps.dialog);
        assert!(caps.toast);
        assert!(caps.panel);
        assert!(caps.status_item);
        assert!(caps.table);
        assert!(caps.markdown);
        assert!(caps.code);
        assert!(caps.progress);
        // Every known effect type should be supported.
        assert!(caps.supports_effect(&UiEffect::ShowToast {
            toast: ToastSpec {
                level: ToastLevel::Info,
                message: "x".into(),
            }
        }));
        assert!(caps.supports_effect(&UiEffect::EmitChat {
            block: ChatBlock {
                format: ChatFormat::Plain,
                content: "x".into(),
            }
        }));
        assert!(caps.supports_effect(&UiEffect::OpenDialog {
            dialog: DialogSpec {
                id: "d".into(),
                title: "t".into(),
                body: UiNode::Empty,
                modal: true,
            }
        }));
        assert!(caps.supports_effect(&UiEffect::CloseDialog {
            id: "d".into(),
        }));
        assert!(caps.supports_effect(&UiEffect::OpenPanel {
            panel: PanelSpec {
                id: "p".into(),
                title: "t".into(),
                placement: PanelPlacement::Right,
                body: UiNode::Empty,
            }
        }));
        assert!(caps.supports_effect(&UiEffect::UpdatePanel {
            id: "p".into(),
            body: UiNode::Empty,
        }));
        assert!(caps.supports_effect(&UiEffect::ClosePanel {
            id: "p".into(),
        }));
        assert!(caps.supports_effect(&UiEffect::AddStatusItem {
            item: StatusItemSpec {
                id: "s".into(),
                label: None,
                placement: StatusPlacement::Right,
                body: UiNode::Empty,
            }
        }));
        assert!(caps.supports_effect(&UiEffect::UpdateStatusItem {
            id: "s".into(),
            body: UiNode::Empty,
        }));
        assert!(caps.supports_effect(&UiEffect::RemoveStatusItem {
            id: "s".into(),
        }));
    }
}
