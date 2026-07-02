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

/// Limits applied when validating a [`UiEffect`] (and any embedded
/// [`UiNode`] payloads). Plugins can otherwise be abused as an output
/// channel; these caps keep wire payloads, in-memory state, and remote
/// snapshot bodies bounded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiLimits {
    /// Maximum number of effects accepted in a single response.
    pub max_effects_per_response: usize,
    /// Maximum serialized JSON byte size for a single effect payload.
    pub max_effect_bytes: usize,
    /// Maximum nesting depth for [`UiNode`] trees.
    pub max_node_depth: usize,
    /// Maximum number of rows in a [`UiNode::Table`].
    pub max_table_rows: usize,
    /// Maximum number of columns in a [`UiNode::Table`].
    pub max_table_columns: usize,
    /// Maximum length (in chars) of any string field inside a [`UiNode`].
    pub max_string_len: usize,
    /// Maximum number of durable panels per plugin.
    pub max_panels_per_plugin: usize,
    /// Maximum number of durable status items per plugin.
    pub max_status_items_per_plugin: usize,
    /// Maximum number of open plugin dialogs globally.
    pub max_open_dialogs_global: usize,
    /// Maximum size of a single surface body when included in a
    /// `RemotePanelView` / `RemoteStatusItemView` snapshot.
    pub max_snapshot_body_bytes: usize,
}

impl Default for UiLimits {
    fn default() -> Self {
        Self::balanced()
    }
}

impl UiLimits {
    /// Convenience: validate a single effect against this `UiLimits`
    /// instance. Equivalent to `validate_ui_effect(effect, self)`.
    pub fn validate_effect(&self, effect: &UiEffect) -> Result<(), UiValidationError> {
        validate_ui_effect(effect, self)
    }

    /// Convenience: validate a batch of effects against this
    /// `UiLimits` instance. Equivalent to
    /// `validate_ui_effects(effects, self)`.
    pub fn validate_effects(&self, effects: &[UiEffect]) -> Result<(), UiValidationError> {
        validate_ui_effects(effects, self)
    }

    /// Conservative defaults suitable for embedded/remote TUI clients.
    /// Tight enough to prevent a misbehaving plugin from destabilising
    /// the client, permissive enough to host real-world plugin UI.
    pub fn balanced() -> Self {
        Self {
            max_effects_per_response: 64,
            max_effect_bytes: 256 * 1024,
            max_node_depth: 16,
            max_table_rows: 512,
            max_table_columns: 32,
            max_string_len: 16 * 1024,
            max_panels_per_plugin: 32,
            max_status_items_per_plugin: 64,
            max_open_dialogs_global: 8,
            max_snapshot_body_bytes: 16 * 1024,
        }
    }

    /// Very tight caps suitable for automation / log-only clients.
    pub fn text_only() -> Self {
        Self {
            max_effects_per_response: 16,
            max_effect_bytes: 32 * 1024,
            max_node_depth: 4,
            max_table_rows: 32,
            max_table_columns: 8,
            max_string_len: 2 * 1024,
            max_panels_per_plugin: 0,
            max_status_items_per_plugin: 0,
            max_open_dialogs_global: 0,
            max_snapshot_body_bytes: 0,
        }
    }
}

/// Errors returned by [`validate_ui_node`] / [`validate_ui_effect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiValidationError {
    /// Effect would exceed a per-response effect count cap.
    TooManyEffects { limit: usize },
    /// Effect (or its serialized form) exceeds the per-effect byte cap.
    EffectTooLarge { limit: usize, approx: usize },
    /// A string field exceeds `max_string_len`.
    StringTooLong { limit: usize, len: usize },
    /// A [`UiNode`] exceeds `max_node_depth`.
    TooDeep { limit: usize },
    /// A [`UiNode::Table`] exceeds the row or column cap.
    TableTooLarge {
        rows: usize,
        cols: usize,
        row_limit: usize,
        col_limit: usize,
    },
}

impl std::fmt::Display for UiValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyEffects { limit } => {
                write!(f, "too many effects (limit {})", limit)
            }
            Self::EffectTooLarge { limit, approx } => write!(
                f,
                "effect payload too large (~{} bytes, limit {})",
                approx, limit
            ),
            Self::StringTooLong { limit, len } => {
                write!(f, "string field too long ({} chars, limit {})", len, limit)
            }
            Self::TooDeep { limit } => {
                write!(f, "ui node tree too deep (limit {})", limit)
            }
            Self::TableTooLarge {
                rows,
                cols,
                row_limit,
                col_limit,
            } => write!(
                f,
                "table too large ({}x{}, limits {}x{})",
                rows, cols, row_limit, col_limit
            ),
        }
    }
}

impl std::error::Error for UiValidationError {}

/// Validate a single [`UiNode`] against the given [`UiLimits`]. Walks the
/// full tree depth-first, checking depth, table size, and string length.
pub fn validate_ui_node(
    node: &UiNode,
    limits: &UiLimits,
    depth: usize,
) -> Result<(), UiValidationError> {
    if depth > limits.max_node_depth {
        return Err(UiValidationError::TooDeep {
            limit: limits.max_node_depth,
        });
    }
    let check_str = |s: &str| -> Result<(), UiValidationError> {
        if s.chars().count() > limits.max_string_len {
            Err(UiValidationError::StringTooLong {
                limit: limits.max_string_len,
                len: s.chars().count(),
            })
        } else {
            Ok(())
        }
    };
    match node {
        UiNode::Text(t) => check_str(&t.text)?,
        UiNode::Markdown(m) => check_str(&m.markdown)?,
        UiNode::Code(c) => {
            check_str(&c.code)?;
            if let Some(lang) = &c.language {
                check_str(lang)?;
            }
        }
        UiNode::Table(t) => {
            for col in &t.columns {
                check_str(col)?;
            }
            if t.columns.len() > limits.max_table_columns {
                return Err(UiValidationError::TableTooLarge {
                    rows: t.rows.len(),
                    cols: t.columns.len(),
                    row_limit: limits.max_table_rows,
                    col_limit: limits.max_table_columns,
                });
            }
            if t.rows.len() > limits.max_table_rows {
                return Err(UiValidationError::TableTooLarge {
                    rows: t.rows.len(),
                    cols: t.columns.len(),
                    row_limit: limits.max_table_rows,
                    col_limit: limits.max_table_columns,
                });
            }
            for row in &t.rows {
                for cell in row {
                    check_str(cell)?;
                }
            }
        }
        UiNode::KeyValue(kv) => {
            for entry in &kv.entries {
                check_str(&entry.key)?;
                check_str(&entry.value)?;
            }
        }
        UiNode::Progress(_) => {}
        UiNode::Container(c) => {
            if let Some(title) = &c.title {
                check_str(title)?;
            }
            for child in &c.children {
                validate_ui_node(child, limits, depth + 1)?;
            }
        }
        UiNode::Empty => {}
        UiNode::Unsupported { unknown_kind, .. } => check_str(unknown_kind)?,
    }
    Ok(())
}

/// Extract the [`UiNode`] payload of an effect, if any.
fn effect_payload_node(effect: &UiEffect) -> Option<&UiNode> {
    match effect {
        UiEffect::OpenDialog { dialog } => Some(&dialog.body),
        UiEffect::OpenPanel { panel } => Some(&panel.body),
        UiEffect::UpdatePanel { body, .. } => Some(body),
        UiEffect::AddStatusItem { item } => Some(&item.body),
        UiEffect::UpdateStatusItem { body, .. } => Some(body),
        UiEffect::EmitChat { .. }
        | UiEffect::ShowToast { .. }
        | UiEffect::CloseDialog { .. }
        | UiEffect::ClosePanel { .. }
        | UiEffect::RemoveStatusItem { .. } => None,
    }
}

/// Validate a single [`UiEffect`] against the given [`UiLimits`]. Checks
/// the effect payload size, any embedded node tree, and string fields.
pub fn validate_ui_effect(effect: &UiEffect, limits: &UiLimits) -> Result<(), UiValidationError> {
    let approx = serde_json::to_vec(effect)
        .map(|v| v.len())
        .unwrap_or(usize::MAX);
    if approx > limits.max_effect_bytes {
        return Err(UiValidationError::EffectTooLarge {
            limit: limits.max_effect_bytes,
            approx,
        });
    }
    if let Some(node) = effect_payload_node(effect) {
        validate_ui_node(node, limits, 1)?;
    }
    match effect {
        UiEffect::EmitChat { block } => {
            if block.content.chars().count() > limits.max_string_len {
                return Err(UiValidationError::StringTooLong {
                    limit: limits.max_string_len,
                    len: block.content.chars().count(),
                });
            }
        }
        UiEffect::ShowToast { toast } => {
            if toast.message.chars().count() > limits.max_string_len {
                return Err(UiValidationError::StringTooLong {
                    limit: limits.max_string_len,
                    len: toast.message.chars().count(),
                });
            }
        }
        UiEffect::OpenDialog { dialog } => {
            if dialog.id.chars().count() > limits.max_string_len
                || dialog.title.chars().count() > limits.max_string_len
            {
                return Err(UiValidationError::StringTooLong {
                    limit: limits.max_string_len,
                    len: dialog.id.chars().count().max(dialog.title.chars().count()),
                });
            }
        }
        UiEffect::OpenPanel { panel } => {
            if panel.id.chars().count() > limits.max_string_len
                || panel.title.chars().count() > limits.max_string_len
            {
                return Err(UiValidationError::StringTooLong {
                    limit: limits.max_string_len,
                    len: panel.id.chars().count().max(panel.title.chars().count()),
                });
            }
        }
        UiEffect::AddStatusItem { item } => {
            let label_len = item
                .label
                .as_deref()
                .map(|s| s.chars().count())
                .unwrap_or(0);
            if item.id.chars().count() > limits.max_string_len || label_len > limits.max_string_len
            {
                return Err(UiValidationError::StringTooLong {
                    limit: limits.max_string_len,
                    len: item.id.chars().count().max(label_len),
                });
            }
        }
        _ => {}
    }
    Ok(())
}

/// Validate a batch of effects: enforces per-response effect count and
/// validates each effect individually.
pub fn validate_ui_effects(
    effects: &[UiEffect],
    limits: &UiLimits,
) -> Result<(), UiValidationError> {
    if effects.len() > limits.max_effects_per_response {
        return Err(UiValidationError::TooManyEffects {
            limit: limits.max_effects_per_response,
        });
    }
    for effect in effects {
        validate_ui_effect(effect, limits)?;
    }
    Ok(())
}

/// Degrade a [`UiEffect`] to a form the client can render.
///
/// Returns the unchanged effect when the client supports all relevant
/// surfaces, a degraded effect when partial support is available (e.g.
/// a table stripped to key/value rows), or `None` when the client
/// cannot render the effect at all (caller should drop or log it).
pub fn degrade_effect(effect: &UiEffect, caps: &PluginUiCapabilities) -> Option<UiEffect> {
    if caps.supports_effect(effect) {
        return Some(effect.clone());
    }
    match effect {
        UiEffect::OpenDialog { .. } | UiEffect::CloseDialog { .. } => {
            if caps.toast {
                Some(UiEffect::ShowToast {
                    toast: effect_summary_toast(effect),
                })
            } else {
                None
            }
        }
        UiEffect::OpenPanel { .. } | UiEffect::UpdatePanel { .. } | UiEffect::ClosePanel { .. } => {
            if caps.toast {
                Some(UiEffect::ShowToast {
                    toast: effect_summary_toast(effect),
                })
            } else {
                None
            }
        }
        UiEffect::AddStatusItem { .. }
        | UiEffect::UpdateStatusItem { .. }
        | UiEffect::RemoveStatusItem { .. } => None,
        UiEffect::ShowToast { .. } | UiEffect::EmitChat { .. } => {
            if caps.toast {
                Some(effect.clone())
            } else {
                None
            }
        }
    }
}

/// Produce a short textual summary of an effect, suitable for log
/// output, toast degradation, or chat-style fallback rendering.
pub fn effect_summary(effect: &UiEffect) -> Option<String> {
    match effect {
        UiEffect::OpenDialog { dialog } => {
            let body_preview = degrade_node_to_text(&dialog.body).join(" ");
            Some(format!("[dialog] {}: {}", dialog.title, body_preview))
        }
        UiEffect::OpenPanel { panel } => {
            let body_preview = degrade_node_to_text(&panel.body).join(" ");
            Some(format!("[panel] {}: {}", panel.title, body_preview))
        }
        UiEffect::AddStatusItem { item } => {
            let body_preview = degrade_node_to_text(&item.body).join(" ");
            if body_preview.is_empty() {
                None
            } else {
                let label = item.label.as_deref().unwrap_or("status");
                Some(format!("[status] {}: {}", label, body_preview))
            }
        }
        UiEffect::EmitChat { block } => Some(format!("[chat] {}", block.content)),
        UiEffect::ShowToast { toast } => Some(toast.message.clone()),
        UiEffect::CloseDialog { .. }
        | UiEffect::UpdatePanel { .. }
        | UiEffect::ClosePanel { .. }
        | UiEffect::UpdateStatusItem { .. }
        | UiEffect::RemoveStatusItem { .. } => None,
    }
}

fn effect_summary_toast(effect: &UiEffect) -> ToastSpec {
    let message = effect_summary(effect).unwrap_or_else(|| {
        format!(
            "[{}] (degraded)",
            match effect {
                UiEffect::OpenDialog { .. } | UiEffect::CloseDialog { .. } => "dialog",
                UiEffect::OpenPanel { .. }
                | UiEffect::UpdatePanel { .. }
                | UiEffect::ClosePanel { .. } => "panel",
                UiEffect::AddStatusItem { .. }
                | UiEffect::UpdateStatusItem { .. }
                | UiEffect::RemoveStatusItem { .. } => "status",
                UiEffect::ShowToast { .. } | UiEffect::EmitChat { .. } => "info",
            }
        )
    });
    ToastSpec {
        level: ToastLevel::Info,
        message,
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
        assert!(caps.supports_effect(&UiEffect::CloseDialog { id: "d".into() }));
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
        assert!(caps.supports_effect(&UiEffect::ClosePanel { id: "p".into() }));
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
        assert!(caps.supports_effect(&UiEffect::RemoveStatusItem { id: "s".into() }));
    }

    #[test]
    fn ui_limits_default_is_balanced() {
        let limits = UiLimits::default();
        let balanced = UiLimits::balanced();
        assert_eq!(limits, balanced);
        assert!(balanced.max_effects_per_response >= 16);
        assert!(balanced.max_open_dialogs_global >= 1);
    }

    #[test]
    fn ui_limits_text_only_is_strict() {
        let strict = UiLimits::text_only();
        assert_eq!(strict.max_panels_per_plugin, 0);
        assert_eq!(strict.max_status_items_per_plugin, 0);
        assert_eq!(strict.max_open_dialogs_global, 0);
        assert_eq!(strict.max_snapshot_body_bytes, 0);
    }

    #[test]
    fn validate_ui_node_accepts_normal_node() {
        let node = UiNode::Container(ContainerNode {
            title: Some("ok".into()),
            children: vec![UiNode::Text(TextNode { text: "hi".into() })],
        });
        assert!(validate_ui_node(&node, &UiLimits::balanced(), 1).is_ok());
    }

    #[test]
    fn validate_ui_node_rejects_too_deep_tree() {
        let mut node = UiNode::Text(TextNode {
            text: "leaf".into(),
        });
        for _ in 0..32 {
            node = UiNode::Container(ContainerNode {
                title: None,
                children: vec![node],
            });
        }
        let limits = UiLimits::balanced();
        let err = validate_ui_node(&node, &limits, 1).unwrap_err();
        assert!(matches!(err, UiValidationError::TooDeep { .. }));
    }

    #[test]
    fn validate_ui_node_rejects_oversize_table() {
        let big_columns: Vec<String> = (0..100).map(|i| format!("c{}", i)).collect();
        let node = UiNode::Table(TableNode {
            columns: big_columns,
            rows: vec![],
        });
        let limits = UiLimits::balanced();
        let err = validate_ui_node(&node, &limits, 1).unwrap_err();
        assert!(matches!(err, UiValidationError::TableTooLarge { .. }));
    }

    #[test]
    fn validate_ui_node_rejects_oversize_string() {
        let big_text = "x".repeat(UiLimits::balanced().max_string_len + 8);
        let node = UiNode::Text(TextNode { text: big_text });
        let err = validate_ui_node(&node, &UiLimits::balanced(), 1).unwrap_err();
        assert!(matches!(err, UiValidationError::StringTooLong { .. }));
    }

    #[test]
    fn validate_ui_effect_accepts_normal_toast() {
        let effect = UiEffect::ShowToast {
            toast: ToastSpec {
                level: ToastLevel::Info,
                message: "ok".into(),
            },
        };
        assert!(validate_ui_effect(&effect, &UiLimits::balanced()).is_ok());
    }

    #[test]
    fn validate_ui_effect_rejects_oversize_payload() {
        let limits = UiLimits {
            max_effect_bytes: 64,
            ..UiLimits::balanced()
        };
        let effect = UiEffect::ShowToast {
            toast: ToastSpec {
                level: ToastLevel::Info,
                message: "this message is longer than 64 bytes by quite a margin".into(),
            },
        };
        let err = validate_ui_effect(&effect, &limits).unwrap_err();
        assert!(matches!(err, UiValidationError::EffectTooLarge { .. }));
    }

    #[test]
    fn validate_ui_effects_rejects_too_many() {
        let limits = UiLimits {
            max_effects_per_response: 2,
            ..UiLimits::balanced()
        };
        let effects = vec![
            UiEffect::CloseDialog { id: "a".into() },
            UiEffect::CloseDialog { id: "b".into() },
            UiEffect::CloseDialog { id: "c".into() },
        ];
        let err = validate_ui_effects(&effects, &limits).unwrap_err();
        assert!(matches!(
            err,
            UiValidationError::TooManyEffects { limit: 2 }
        ));
    }

    #[test]
    fn degrade_effect_returns_same_when_supported() {
        let caps = PluginUiCapabilities::all_supported();
        let effect = UiEffect::ShowToast {
            toast: ToastSpec {
                level: ToastLevel::Info,
                message: "hi".into(),
            },
        };
        let degraded = degrade_effect(&effect, &caps);
        assert_eq!(degraded, Some(effect));
    }

    #[test]
    fn degrade_effect_downgrades_dialog_to_toast_when_toast_supported() {
        let caps = PluginUiCapabilities {
            dialog: false,
            toast: true,
            ..Default::default()
        };
        let effect = UiEffect::OpenDialog {
            dialog: DialogSpec {
                id: "x".into(),
                title: "t".into(),
                body: UiNode::Text(TextNode {
                    text: "body".into(),
                }),
                modal: true,
            },
        };
        let degraded = degrade_effect(&effect, &caps);
        assert!(matches!(degraded, Some(UiEffect::ShowToast { .. })));
    }

    #[test]
    fn degrade_effect_returns_none_when_no_support() {
        let caps = PluginUiCapabilities::default();
        let effect = UiEffect::OpenPanel {
            panel: PanelSpec {
                id: "p".into(),
                title: "t".into(),
                placement: PanelPlacement::Right,
                body: UiNode::Empty,
            },
        };
        assert_eq!(degrade_effect(&effect, &caps), None);
    }

    #[test]
    fn degrade_effect_drops_status_item_when_unsupported() {
        let caps = PluginUiCapabilities {
            status_item: false,
            toast: true,
            ..Default::default()
        };
        let effect = UiEffect::AddStatusItem {
            item: StatusItemSpec {
                id: "s".into(),
                label: None,
                placement: StatusPlacement::Right,
                body: UiNode::Empty,
            },
        };
        assert_eq!(degrade_effect(&effect, &caps), None);
    }

    #[test]
    fn effect_summary_extracts_text() {
        let effect = UiEffect::OpenDialog {
            dialog: DialogSpec {
                id: "x".into(),
                title: "My Title".into(),
                body: UiNode::Text(TextNode {
                    text: "Body".into(),
                }),
                modal: true,
            },
        };
        let summary = effect_summary(&effect).unwrap();
        assert!(summary.contains("My Title"));
        assert!(summary.contains("Body"));
    }

    #[test]
    fn effect_summary_returns_none_for_close_variants() {
        assert_eq!(
            effect_summary(&UiEffect::CloseDialog { id: "x".into() }),
            None
        );
        assert_eq!(
            effect_summary(&UiEffect::ClosePanel { id: "x".into() }),
            None
        );
        assert_eq!(
            effect_summary(&UiEffect::RemoveStatusItem { id: "x".into() }),
            None
        );
    }

    #[test]
    fn effect_summary_for_status_item_without_text_is_none() {
        let effect = UiEffect::AddStatusItem {
            item: StatusItemSpec {
                id: "s".into(),
                label: Some("build".into()),
                placement: StatusPlacement::Right,
                body: UiNode::Empty,
            },
        };
        assert_eq!(effect_summary(&effect), None);
    }
}
