//! Plugin UI effect application and validation.
//!
//! Plugin effects are validated, ownership-checked, and lowered into the
//! existing FocusManager, plugin state, and bounded presentation surfaces.
//! This module does not authorize plugins or create a second UI owner.

use super::{App, Dialog};
use crate::util::truncate::truncate_prefix;
use std::sync::Arc;

impl App {
    pub fn apply_plugin_ui_effect(
        &mut self,
        effect: crate::protocol::ui::UiEffect,
        source_plugin_id: Option<&str>,
    ) -> crate::tui::app::state::PluginUiApplyResult {
        use crate::protocol::ui::UiEffect;
        use crate::tui::app::state::PluginUiApplyResult;

        // Capability gate: reject effects the client does not support.
        // Degrade to a summary toast so the effect is not silently lost.
        if !self.ui_state.plugin_ui_caps.supports_effect(&effect) {
            let summary = Self::degrade_effect_to_summary(&effect);
            if !summary.is_empty() {
                self.messages_state.toasts.info(&summary);
            }
            return PluginUiApplyResult::Unsupported(
                "effect type not supported by client capabilities".to_string(),
            );
        }

        // Cross-plugin spoofing protection: when a plugin source is
        // provided, reject effects whose surface IDs belong to a
        // different plugin.
        if let Some(owner) = source_plugin_id {
            if let Some(violation) = self.validate_plugin_surface_ownership(&effect, owner) {
                return PluginUiApplyResult::Unsupported(violation);
            }
        }

        match &effect {
            UiEffect::ShowToast { toast } => {
                match toast.level {
                    crate::protocol::ui::ToastLevel::Info => {
                        self.messages_state.toasts.info(&toast.message);
                    }
                    crate::protocol::ui::ToastLevel::Success => {
                        self.messages_state.toasts.success(&toast.message);
                    }
                    crate::protocol::ui::ToastLevel::Warning => {
                        self.messages_state.toasts.warning(&toast.message);
                    }
                    crate::protocol::ui::ToastLevel::Error => {
                        self.messages_state.toasts.error(&toast.message);
                    }
                };
                return PluginUiApplyResult::ToastRequested;
            }
            UiEffect::EmitChat { block } => {
                // EmitChat is now rendered visibly in the TUI. Both
                // ChatFormat::Plain and ChatFormat::Markdown are lowered to
                // line-based text so they never execute embedded escape
                // sequences or markdown links. Output is shown via the
                // toast/info-dialog surface: short blocks toast, long
                // blocks open the scrollable info dialog. This output is
                // NOT added to the model-visible chat transcript — it is a
                // user-facing display surface only.
                let lines: Vec<String> = block.content.lines().map(|s| s.to_string()).collect();
                if lines.is_empty() {
                    return PluginUiApplyResult::Ignored;
                }
                self.show_short_or_info(
                    crate::tui::components::dialogs::info::InfoType::Stats,
                    lines,
                );
                return PluginUiApplyResult::ChatApplied;
            }
            _ => {}
        }

        // Capture the close target before the effect is consumed so the
        // matching focus-stack entry can be dropped afterwards.
        let closing_plugin_dialog = matches!(&effect, UiEffect::CloseDialog { .. });

        let result = self.plugin_ui_state.apply_effect(effect);

        // If a plugin dialog was just opened and no first-party modal
        // is active, open it in the FocusManager. Use the dialog ID
        // captured by `apply_effect` (the one this effect just
        // opened) rather than the lexicographically-last entry in
        // `dialogs`, which may be a stale previously-opened dialog.
        if matches!(result, PluginUiApplyResult::Applied) {
            if closing_plugin_dialog {
                // CloseDialog removed the spec; drop the matching
                // focus-stack entry so the ghost dialog stops rendering
                // and consuming keys.
                if self.focus_manager.active_dialog_type()
                    == crate::tui::components::component::DialogType::Plugin
                {
                    self.focus_manager.pop();
                    let active_type = self.focus_manager.active_dialog_type();
                    self.ui_state.dialog =
                        if active_type != crate::tui::components::component::DialogType::None {
                            Dialog::from(active_type)
                        } else {
                            Dialog::None
                        };
                }
            } else {
                let opened_id = self.plugin_ui_state.last_opened_dialog_id.clone();
                if let Some(id) = opened_id {
                    if let Some(spec) = self.plugin_ui_state.get_dialog(&id) {
                        if !matches!(
                            self.ui_state.dialog,
                            Dialog::Permission | Dialog::Question | Dialog::SecurityReview
                        ) {
                            let lines = crate::tui::components::ui_node_renderer::UiNodeRenderer::
                                node_to_lines(&spec.body);
                            if self.focus_manager.active_dialog_type()
                                == crate::tui::components::component::DialogType::Plugin
                            {
                                let _ = self.focus_manager.with_dialog_mut(
                                    crate::tui::components::component::DialogType::Plugin,
                                    |dialog: &mut crate::tui::components::dialogs::plugin::PluginDialog| {
                                        dialog.update_content(lines);
                                        dialog.set_title(spec.title.clone());
                                        dialog.set_theme(&self.ui_state.theme);
                                    },
                                );
                            } else {
                                let dialog =
                                    crate::tui::components::dialogs::plugin::PluginDialog::new(
                                        spec.id.clone(),
                                        spec.title.clone(),
                                        lines,
                                        Arc::clone(&self.ui_state.theme),
                                    );
                                self.push_dialog(Dialog::Plugin, Box::new(dialog));
                            }
                            self.ui_state.dialog = Dialog::Plugin;
                        }
                    }
                }
            }
        }

        result
    }

    /// Render a structured `UiValidationError` as a short user-facing
    /// description for toasts and tests. This avoids leaking raw serde
    /// field paths while keeping the human-readable cause.
    fn validation_error_to_string(err: &crate::protocol::ui::UiValidationError) -> String {
        use crate::protocol::ui::UiValidationError;
        match err {
            UiValidationError::TooManyEffects { limit } => {
                format!("too many effects (limit {})", limit)
            }
            UiValidationError::EffectTooLarge { limit, approx } => {
                format!(
                    "effect payload too large (~{} bytes, limit {})",
                    approx, limit
                )
            }
            UiValidationError::StringTooLong { limit, len } => {
                format!("string field too long ({} chars, limit {})", len, limit)
            }
            UiValidationError::TooDeep { limit } => {
                format!("node depth exceeds limit {}", limit)
            }
            UiValidationError::TableTooLarge {
                rows,
                cols,
                row_limit,
                col_limit,
            } => format!(
                "table {}x{} exceeds limits {}x{}",
                rows, cols, row_limit, col_limit
            ),
        }
    }

    /// Produce a short summary string for an unsupported [`UiEffect`],
    /// used to degrade gracefully when the client cannot render the
    /// full effect. Returns an empty string for effects that should
    /// be silently omitted (e.g. status items).
    fn degrade_effect_to_summary(effect: &crate::protocol::ui::UiEffect) -> String {
        use crate::protocol::ui::UiEffect;
        match effect {
            UiEffect::OpenDialog { dialog } => {
                let lines = crate::tui::components::ui_node_renderer::UiNodeRenderer::node_to_lines(
                    &dialog.body,
                );
                let preview = lines.join(" ");
                let preview = if preview.len() > 120 {
                    format!("{}...", truncate_prefix(&preview, 120))
                } else {
                    preview
                };
                format!("[dialog] {}: {}", dialog.title, preview)
            }
            UiEffect::OpenPanel { panel } => {
                let lines = crate::tui::components::ui_node_renderer::UiNodeRenderer::node_to_lines(
                    &panel.body,
                );
                let preview = lines.join(" ");
                let preview = if preview.len() > 120 {
                    format!("{}...", truncate_prefix(&preview, 120))
                } else {
                    preview
                };
                format!("[panel] {}: {}", panel.title, preview)
            }
            UiEffect::AddStatusItem { item } => {
                // Status items are omitted unless they carry text content.
                let lines = crate::tui::components::ui_node_renderer::UiNodeRenderer::node_to_lines(
                    &item.body,
                );
                let preview = lines.join(" ");
                if preview.is_empty() {
                    return String::new();
                }
                let label = item.label.as_deref().unwrap_or("status");
                format!("[status] {}: {}", label, preview)
            }
            UiEffect::EmitChat { block } => {
                let preview = if block.content.len() > 120 {
                    format!("{}...", truncate_prefix(&block.content, 120))
                } else {
                    block.content.clone()
                };
                format!("[chat] {}", preview)
            }
            UiEffect::ShowToast { toast } => toast.message.clone(),
            UiEffect::CloseDialog { .. }
            | UiEffect::UpdatePanel { .. }
            | UiEffect::ClosePanel { .. }
            | UiEffect::UpdateStatusItem { .. }
            | UiEffect::RemoveStatusItem { .. } => String::new(),
        }
    }

    /// Check that surface IDs in the effect belong to the claimed plugin.
    /// Returns `Some(reason)` if a spoofing attempt is detected.
    fn validate_plugin_surface_ownership(
        &self,
        effect: &crate::protocol::ui::UiEffect,
        claimed_owner: &str,
    ) -> Option<String> {
        use crate::protocol::ui::UiEffect;
        let prefix = format!("{}:", claimed_owner);
        let foreign_id = match effect {
            UiEffect::OpenDialog { dialog } if !dialog.id.starts_with(&prefix) => {
                Some(dialog.id.clone())
            }
            UiEffect::CloseDialog { id } if !id.starts_with(&prefix) => Some(id.clone()),
            UiEffect::OpenPanel { panel } if !panel.id.starts_with(&prefix) => {
                Some(panel.id.clone())
            }
            UiEffect::UpdatePanel { id, .. } if !id.starts_with(&prefix) => Some(id.clone()),
            UiEffect::ClosePanel { id } if !id.starts_with(&prefix) => Some(id.clone()),
            UiEffect::AddStatusItem { item } if !item.id.starts_with(&prefix) => {
                Some(item.id.clone())
            }
            UiEffect::UpdateStatusItem { id, .. } if !id.starts_with(&prefix) => Some(id.clone()),
            UiEffect::RemoveStatusItem { id } if !id.starts_with(&prefix) => Some(id.clone()),
            _ => None,
        };
        foreign_id.map(|id| {
            format!(
                "plugin '{}' attempted to use surface id '{}' belonging to another plugin",
                claimed_owner, id
            )
        })
    }

    /// Apply a plugin-UI envelope (with typed source, session, and
    /// invocation) to the TUI. This is the canonical entry point for
    /// all plugin UI effects, regardless of transport (local TUI
    /// command channel or remote WebSocket).
    ///
    /// The envelope-derived source is used for ownership checks
    /// automatically. For Plugin sources the prefix check uses
    /// `envelope.source.plugin_id`; for Core/Tui sources no plugin
    /// ownership is enforced.
    ///
    /// The effect payload is validated against
    /// [`crate::protocol::ui::UiLimits::balanced()`] before
    /// dispatch. Effects that exceed limits are rejected with a
    /// structured result.
    pub fn apply_plugin_ui_envelope(
        &mut self,
        envelope: crate::protocol::ui::UiEffectEnvelope,
    ) -> crate::tui::app::state::PluginUiApplyResult {
        use crate::protocol::ui::{UiEffectSource, UiLimits};

        let limits = UiLimits::balanced();
        let plugin_id_owned;
        let plugin_id_opt: Option<&str> = match &envelope.source {
            UiEffectSource::Plugin { plugin_id } => {
                plugin_id_owned = plugin_id.clone();
                Some(plugin_id_owned.as_str())
            }
            UiEffectSource::Core | UiEffectSource::Tui => None,
        };

        // Session guard: drop envelopes targeted at a different
        // session than the one currently focused.
        if let Some(sid) = envelope.session_id.as_deref() {
            let current_session = self
                .session_state
                .session
                .as_ref()
                .map(|s| s.id.as_str())
                .unwrap_or_default();
            if !sid.is_empty() && sid != current_session {
                return crate::tui::app::state::PluginUiApplyResult::Unsupported(format!(
                    "envelope session_id '{}' does not match active session '{}'",
                    sid, current_session
                ));
            }
        }

        // Validate the effect payload against the limits.
        if let Err(err) = limits.validate_effect(&envelope.effect) {
            return crate::tui::app::state::PluginUiApplyResult::Unsupported(format!(
                "ui effect rejected by limits: {}",
                Self::validation_error_to_string(&err)
            ));
        }

        let crate::protocol::ui::UiEffectEnvelope { effect, .. } = envelope;
        self.apply_plugin_ui_effect(effect, plugin_id_opt)
    }

    /// Validate a batch of effects against the balanced limits and
    /// return the offending error if any one fails. Bounded for
    /// multi-frontend transport — callers (event bridge, lifecycle
    /// hooks) iterate plugin response vectors and need a deterministic
    /// per-batch validation point.
    pub fn validate_plugin_ui_effects(
        &self,
        effects: &[crate::protocol::ui::UiEffect],
    ) -> Result<(), String> {
        use crate::protocol::ui::UiLimits;
        UiLimits::balanced()
            .validate_effects(effects)
            .map_err(|err| Self::validation_error_to_string(&err))
    }
}
