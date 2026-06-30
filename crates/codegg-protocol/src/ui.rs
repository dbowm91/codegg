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
    EmitChat {
        block: ChatBlock,
    },
    ShowToast {
        toast: ToastSpec,
    },
    OpenDialog {
        dialog: DialogSpec,
    },
    CloseDialog {
        id: String,
    },
    OpenPanel {
        panel: PanelSpec,
    },
    UpdatePanel {
        id: String,
        body: UiNode,
    },
    ClosePanel {
        id: String,
    },
    AddStatusItem {
        item: StatusItemSpec,
    },
    UpdateStatusItem {
        id: String,
        body: UiNode,
    },
    RemoveStatusItem {
        id: String,
    },
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
                KeyValueEntry { key: "k1".into(), value: "v1".into() },
                KeyValueEntry { key: "k2".into(), value: "v2".into() },
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
        let effect = UiEffect::CloseDialog {
            id: "dlg-1".into(),
        };
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
}
