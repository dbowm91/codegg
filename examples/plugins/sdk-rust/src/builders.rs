pub use codegg_protocol::plugin::{
    PluginCapabilityInvocation, PluginContext, PluginDiagnostic, PluginDiagnosticLevel,
    PluginInvocation, PluginResponse,
};
pub use codegg_protocol::ui::{
    ChatBlock, ChatFormat, CodeNode, ContainerNode, DialogSpec, KeyValueEntry, KeyValueNode,
    MarkdownNode, PanelPlacement, PanelSpec, ProgressNode, StatusItemSpec, StatusPlacement,
    TableNode, TextNode, ToastLevel, ToastSpec, UiEffect, UiNode,
};

pub fn text_node(text: impl Into<String>) -> UiNode {
    UiNode::Text(TextNode { text: text.into() })
}

pub fn markdown_node(md: impl Into<String>) -> UiNode {
    UiNode::Markdown(MarkdownNode {
        markdown: md.into(),
    })
}

pub fn code_node(language: Option<&str>, code: impl Into<String>) -> UiNode {
    UiNode::Code(CodeNode {
        language: language.map(|s| s.to_string()),
        code: code.into(),
    })
}

pub fn table_node(columns: Vec<String>, rows: Vec<Vec<String>>) -> UiNode {
    UiNode::Table(TableNode { columns, rows })
}

pub fn key_value_node(entries: Vec<(String, String)>) -> UiNode {
    UiNode::KeyValue(KeyValueNode {
        entries: entries
            .into_iter()
            .map(|(key, value)| KeyValueEntry { key, value })
            .collect(),
    })
}

pub fn progress_node(label: Option<&str>, current: u64, total: Option<u64>) -> UiNode {
    UiNode::Progress(ProgressNode {
        label: label.map(|s| s.to_string()),
        current,
        total,
    })
}

pub fn container_node(title: Option<&str>, children: Vec<UiNode>) -> UiNode {
    UiNode::Container(ContainerNode {
        title: title.map(|s| s.to_string()),
        children,
    })
}

pub fn diagnostic(level: PluginDiagnosticLevel, message: impl Into<String>) -> PluginDiagnostic {
    PluginDiagnostic {
        level,
        message: message.into(),
    }
}

pub fn ok_response(effects: Vec<UiEffect>, data: serde_json::Value) -> PluginResponse {
    PluginResponse {
        ok: true,
        effects,
        data,
        diagnostics: vec![],
    }
}

pub fn error_response(message: impl Into<String>) -> PluginResponse {
    PluginResponse {
        ok: false,
        effects: vec![],
        data: serde_json::Value::Null,
        diagnostics: vec![PluginDiagnostic {
            level: PluginDiagnosticLevel::Error,
            message: message.into(),
        }],
    }
}

pub fn response_chat(content: impl Into<String>, format: ChatFormat) -> PluginResponse {
    PluginResponse {
        ok: true,
        effects: vec![UiEffect::EmitChat {
            block: ChatBlock {
                format,
                content: content.into(),
            },
        }],
        data: serde_json::Value::Null,
        diagnostics: vec![],
    }
}

pub fn response_chat_markdown(md: impl Into<String>) -> PluginResponse {
    response_chat(md, ChatFormat::Markdown)
}

pub fn response_dialog(
    id: impl Into<String>,
    title: impl Into<String>,
    body: UiNode,
    modal: bool,
) -> PluginResponse {
    PluginResponse {
        ok: true,
        effects: vec![UiEffect::OpenDialog {
            dialog: DialogSpec {
                id: id.into(),
                title: title.into(),
                body,
                modal,
            },
        }],
        data: serde_json::Value::Null,
        diagnostics: vec![],
    }
}

pub fn response_panel(
    id: impl Into<String>,
    title: impl Into<String>,
    placement: PanelPlacement,
    body: UiNode,
) -> PluginResponse {
    PluginResponse {
        ok: true,
        effects: vec![UiEffect::OpenPanel {
            panel: PanelSpec {
                id: id.into(),
                title: title.into(),
                placement,
                body,
            },
        }],
        data: serde_json::Value::Null,
        diagnostics: vec![],
    }
}

pub fn response_status(
    id: impl Into<String>,
    placement: StatusPlacement,
    body: UiNode,
) -> PluginResponse {
    PluginResponse {
        ok: true,
        effects: vec![UiEffect::AddStatusItem {
            item: StatusItemSpec {
                id: id.into(),
                label: None,
                placement,
                body,
            },
        }],
        data: serde_json::Value::Null,
        diagnostics: vec![],
    }
}
