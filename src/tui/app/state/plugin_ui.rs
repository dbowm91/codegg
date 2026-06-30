use std::collections::BTreeMap;

use crate::protocol::ui::{DialogSpec, PanelSpec, StatusItemSpec, UiEffect};

#[derive(Debug, Clone, Default)]
pub struct PluginUiState {
    pub dialogs: BTreeMap<String, DialogSpec>,
    pub panels: BTreeMap<String, PanelSpec>,
    pub status_items: BTreeMap<String, StatusItemSpec>,
    pub last_effect_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginUiApplyResult {
    Applied,
    ChatRequested,
    ToastRequested,
    Ignored,
    Error(String),
}

impl PluginUiState {
    pub fn apply_effect(&mut self, effect: UiEffect) -> PluginUiApplyResult {
        match effect {
            UiEffect::EmitChat { .. } => PluginUiApplyResult::ChatRequested,
            UiEffect::ShowToast { .. } => PluginUiApplyResult::ToastRequested,
            UiEffect::OpenDialog { dialog } => {
                self.dialogs.insert(dialog.id.clone(), dialog);
                PluginUiApplyResult::Applied
            }
            UiEffect::CloseDialog { id } => {
                if self.dialogs.remove(&id).is_some() {
                    PluginUiApplyResult::Applied
                } else {
                    PluginUiApplyResult::Ignored
                }
            }
            UiEffect::OpenPanel { panel } => {
                self.panels.insert(panel.id.clone(), panel);
                PluginUiApplyResult::Applied
            }
            UiEffect::UpdatePanel { id, body } => {
                if let Some(panel) = self.panels.get_mut(&id) {
                    panel.body = body;
                    PluginUiApplyResult::Applied
                } else {
                    PluginUiApplyResult::Ignored
                }
            }
            UiEffect::ClosePanel { id } => {
                if self.panels.remove(&id).is_some() {
                    PluginUiApplyResult::Applied
                } else {
                    PluginUiApplyResult::Ignored
                }
            }
            UiEffect::AddStatusItem { item } => {
                self.status_items.insert(item.id.clone(), item);
                PluginUiApplyResult::Applied
            }
            UiEffect::UpdateStatusItem { id, body } => {
                if let Some(item) = self.status_items.get_mut(&id) {
                    item.body = body;
                    PluginUiApplyResult::Applied
                } else {
                    PluginUiApplyResult::Ignored
                }
            }
            UiEffect::RemoveStatusItem { id } => {
                if self.status_items.remove(&id).is_some() {
                    PluginUiApplyResult::Applied
                } else {
                    PluginUiApplyResult::Ignored
                }
            }
        }
    }

    pub fn clear_plugin(&mut self, plugin_id: &str) {
        let prefix = format!("{}:", plugin_id);
        self.dialogs.retain(|k, _| !k.starts_with(&prefix));
        self.panels.retain(|k, _| !k.starts_with(&prefix));
        self.status_items.retain(|k, _| !k.starts_with(&prefix));
    }

    pub fn get_dialog(&self, id: &str) -> Option<&DialogSpec> {
        self.dialogs.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ui::{
        ChatBlock, ChatFormat, PanelPlacement, StatusPlacement, TextNode, ToastLevel, ToastSpec,
        UiNode,
    };

    fn text_node(s: &str) -> UiNode {
        UiNode::Text(TextNode { text: s.into() })
    }

    fn make_dialog(id: &str) -> DialogSpec {
        DialogSpec {
            id: id.into(),
            title: format!("Title {id}"),
            body: text_node("test"),
            modal: true,
        }
    }

    fn make_panel(id: &str) -> PanelSpec {
        PanelSpec {
            id: id.into(),
            title: format!("Panel {id}"),
            placement: PanelPlacement::Right,
            body: text_node("test"),
        }
    }

    fn make_status_item(id: &str) -> StatusItemSpec {
        StatusItemSpec {
            id: id.into(),
            label: Some(format!("Label {id}")),
            placement: StatusPlacement::Right,
            body: text_node("test"),
        }
    }

    #[test]
    fn dialog_open_update_close() {
        let mut state = PluginUiState::default();

        let r = state.apply_effect(UiEffect::OpenDialog {
            dialog: make_dialog("dlg-1"),
        });
        assert_eq!(r, PluginUiApplyResult::Applied);
        assert!(state.dialogs.contains_key("dlg-1"));
        assert_eq!(state.get_dialog("dlg-1").unwrap().title, "Title dlg-1");

        let r = state.apply_effect(UiEffect::OpenDialog {
            dialog: DialogSpec {
                id: "dlg-1".into(),
                title: "Updated".into(),
                body: text_node("updated"),
                modal: false,
            },
        });
        assert_eq!(r, PluginUiApplyResult::Applied);
        assert_eq!(state.get_dialog("dlg-1").unwrap().title, "Updated");

        let r = state.apply_effect(UiEffect::CloseDialog { id: "dlg-1".into() });
        assert_eq!(r, PluginUiApplyResult::Applied);
        assert!(state.get_dialog("dlg-1").is_none());
    }

    #[test]
    fn panel_open_update_close() {
        let mut state = PluginUiState::default();

        let r = state.apply_effect(UiEffect::OpenPanel {
            panel: make_panel("p-1"),
        });
        assert_eq!(r, PluginUiApplyResult::Applied);
        assert!(state.panels.contains_key("p-1"));
        assert_eq!(state.panels["p-1"].title, "Panel p-1");

        let r = state.apply_effect(UiEffect::UpdatePanel {
            id: "p-1".into(),
            body: text_node("new body"),
        });
        assert_eq!(r, PluginUiApplyResult::Applied);
        assert_eq!(state.panels["p-1"].body, text_node("new body"));

        let r = state.apply_effect(UiEffect::ClosePanel { id: "p-1".into() });
        assert_eq!(r, PluginUiApplyResult::Applied);
        assert!(!state.panels.contains_key("p-1"));
    }

    #[test]
    fn status_item_add_update_remove() {
        let mut state = PluginUiState::default();

        let r = state.apply_effect(UiEffect::AddStatusItem {
            item: make_status_item("s-1"),
        });
        assert_eq!(r, PluginUiApplyResult::Applied);
        assert!(state.status_items.contains_key("s-1"));

        let r = state.apply_effect(UiEffect::UpdateStatusItem {
            id: "s-1".into(),
            body: text_node("updated status"),
        });
        assert_eq!(r, PluginUiApplyResult::Applied);
        assert_eq!(state.status_items["s-1"].body, text_node("updated status"));

        let r = state.apply_effect(UiEffect::RemoveStatusItem { id: "s-1".into() });
        assert_eq!(r, PluginUiApplyResult::Applied);
        assert!(!state.status_items.contains_key("s-1"));
    }

    #[test]
    fn show_toast_returns_toast_requested() {
        let mut state = PluginUiState::default();
        let r = state.apply_effect(UiEffect::ShowToast {
            toast: ToastSpec {
                level: ToastLevel::Info,
                message: "hi".into(),
            },
        });
        assert_eq!(r, PluginUiApplyResult::ToastRequested);
    }

    #[test]
    fn emit_chat_returns_chat_requested() {
        let mut state = PluginUiState::default();
        let r = state.apply_effect(UiEffect::EmitChat {
            block: ChatBlock {
                format: ChatFormat::Plain,
                content: "hello".into(),
            },
        });
        assert_eq!(r, PluginUiApplyResult::ChatRequested);
    }

    #[test]
    fn duplicate_dialog_id_overwrites() {
        let mut state = PluginUiState::default();
        state.apply_effect(UiEffect::OpenDialog {
            dialog: make_dialog("d1"),
        });
        state.apply_effect(UiEffect::OpenDialog {
            dialog: DialogSpec {
                id: "d1".into(),
                title: "Overwritten".into(),
                body: UiNode::Empty,
                modal: false,
            },
        });
        assert_eq!(state.dialogs.len(), 1);
        assert_eq!(state.get_dialog("d1").unwrap().title, "Overwritten");
    }

    #[test]
    fn duplicate_panel_id_overwrites() {
        let mut state = PluginUiState::default();
        state.apply_effect(UiEffect::OpenPanel {
            panel: make_panel("p1"),
        });
        state.apply_effect(UiEffect::OpenPanel {
            panel: PanelSpec {
                id: "p1".into(),
                title: "Overwritten".into(),
                placement: PanelPlacement::Left,
                body: UiNode::Empty,
            },
        });
        assert_eq!(state.panels.len(), 1);
        assert_eq!(state.panels["p1"].title, "Overwritten");
    }

    #[test]
    fn duplicate_status_item_id_overwrites() {
        let mut state = PluginUiState::default();
        state.apply_effect(UiEffect::AddStatusItem {
            item: make_status_item("s1"),
        });
        state.apply_effect(UiEffect::AddStatusItem {
            item: StatusItemSpec {
                id: "s1".into(),
                label: Some("Overwritten".into()),
                placement: StatusPlacement::Left,
                body: UiNode::Empty,
            },
        });
        assert_eq!(state.status_items.len(), 1);
        assert_eq!(
            state.status_items["s1"].label.as_deref(),
            Some("Overwritten")
        );
    }

    #[test]
    fn clear_plugin_removes_all_surfaces() {
        let mut state = PluginUiState::default();
        state
            .dialogs
            .insert("my-plugin:dlg".into(), make_dialog("my-plugin:dlg"));
        state
            .panels
            .insert("my-plugin:panel".into(), make_panel("my-plugin:panel"));
        state.status_items.insert(
            "my-plugin:status".into(),
            make_status_item("my-plugin:status"),
        );
        state
            .dialogs
            .insert("other:dlg".into(), make_dialog("other:dlg"));

        state.clear_plugin("my-plugin");

        assert!(!state.dialogs.contains_key("my-plugin:dlg"));
        assert!(!state.panels.contains_key("my-plugin:panel"));
        assert!(!state.status_items.contains_key("my-plugin:status"));
        assert!(state.dialogs.contains_key("other:dlg"));
    }

    #[test]
    fn close_dialog_nonexistent_returns_ignored() {
        let mut state = PluginUiState::default();
        let r = state.apply_effect(UiEffect::CloseDialog { id: "nope".into() });
        assert_eq!(r, PluginUiApplyResult::Ignored);
    }

    #[test]
    fn close_panel_nonexistent_returns_ignored() {
        let mut state = PluginUiState::default();
        let r = state.apply_effect(UiEffect::ClosePanel { id: "nope".into() });
        assert_eq!(r, PluginUiApplyResult::Ignored);
    }

    #[test]
    fn remove_status_item_nonexistent_returns_ignored() {
        let mut state = PluginUiState::default();
        let r = state.apply_effect(UiEffect::RemoveStatusItem { id: "nope".into() });
        assert_eq!(r, PluginUiApplyResult::Ignored);
    }

    #[test]
    fn update_panel_nonexistent_returns_ignored() {
        let mut state = PluginUiState::default();
        let r = state.apply_effect(UiEffect::UpdatePanel {
            id: "nope".into(),
            body: text_node("x"),
        });
        assert_eq!(r, PluginUiApplyResult::Ignored);
    }

    #[test]
    fn update_status_item_nonexistent_returns_ignored() {
        let mut state = PluginUiState::default();
        let r = state.apply_effect(UiEffect::UpdateStatusItem {
            id: "nope".into(),
            body: text_node("x"),
        });
        assert_eq!(r, PluginUiApplyResult::Ignored);
    }

    #[test]
    fn default_state_is_empty() {
        let state = PluginUiState::default();
        assert!(state.dialogs.is_empty());
        assert!(state.panels.is_empty());
        assert!(state.status_items.is_empty());
        assert!(state.last_effect_error.is_none());
    }
}
