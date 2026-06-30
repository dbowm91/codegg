//! Plugin command handlers.
//!
//! Provides the TUI-side plumbing for running plugin-backed commands and
//! applying plugin UI responses without blocking the render loop. In this
//! phase the actual command execution is a stub; the important part is
//! that the dispatch/apply path is correct and non-blocking.

use crate::protocol::plugin::PluginResponse;
use crate::protocol::ui::UiEffect;
use crate::tui::app::App;
use crate::tui::app::TuiCommand;

/// Start a plugin command. In this phase the command is a stub that
/// posts a `not_implemented` completion. Phase 4 will wire real
/// process/WASM execution here.
pub(crate) fn start_plugin_command(app: &mut App, command: String, args: Vec<String>) {
    let invocation_id = uuid::Uuid::new_v4().to_string();
    let tx = app.tui_cmd_tx.clone();

    crate::tui::async_cmd::spawn_tui_task(tx, "plugin_command_run", async move {
        Some(TuiCommand::PluginCommandFinished {
            invocation_id,
            command,
            response: None,
            stdout: None,
            stderr: None,
            error: Some(format!(
                "Plugin command not yet implemented (phase 4). args={:?}", args
            )),
        })
    });
}

/// Apply a completed plugin command to the TUI state.
///
/// Response application rules (deterministic):
/// 1. If `error` is present, show an error toast and optionally an info
///    dialog with stderr/stdout diagnostics.
/// 2. If `response` is present and `response.ok == true`, apply each
///    `UiEffect` in order.
/// 3. If `response` is present and `response.ok == false`, apply any
///    diagnostic effects but show an error/warning toast.
/// 4. If no structured response exists but `stdout` exists, render stdout
///    as chat/plain text or info dialog depending on length.
/// 5. If only `stderr` exists, render as warning/error diagnostics.
/// 6. If nothing exists, show a concise "plugin command produced no output"
///    warning.
pub(crate) fn apply_plugin_command_finished(
    app: &mut App,
    _invocation_id: String,
    command: String,
    response: Option<Box<PluginResponse>>,
    stdout: Option<String>,
    stderr: Option<String>,
    error: Option<String>,
) {
    // 1. Error path
    if let Some(err) = error {
        app.messages_state.toasts.error(&format!("Plugin '{command}' failed: {err}"));
        let mut extra = Vec::new();
        if let Some(ref out) = stdout {
            if !out.is_empty() {
                extra.push(format!("stdout: {out}"));
            }
        }
        if let Some(ref err_out) = stderr {
            if !err_out.is_empty() {
                extra.push(format!("stderr: {err_out}"));
            }
        }
        if !extra.is_empty() {
            extra.insert(0, format!("Plugin command: {command}"));
            app.show_short_or_info(
                crate::tui::components::dialogs::info::InfoType::Stats,
                extra,
            );
        }
        return;
    }

    // 2. Structured response path
    if let Some(resp) = response {
        if resp.ok {
            // Apply each effect in order
            for effect in resp.effects {
                app.apply_plugin_ui_effect(effect);
            }
        } else {
            // Apply diagnostic effects but show error toast
            for effect in &resp.effects {
                app.apply_plugin_ui_effect(effect.clone());
            }
            let diag_msgs: Vec<String> = resp
                .diagnostics
                .iter()
                .map(|d| format!("[{}] {}", level_label(d.level.clone()), d.message))
                .collect();
            if !diag_msgs.is_empty() {
                let mut lines = vec![format!("Plugin '{command}' returned errors:")];
                lines.extend(diag_msgs);
                app.show_short_or_info(
                    crate::tui::components::dialogs::info::InfoType::Stats,
                    lines,
                );
            } else {
                app.messages_state
                    .toasts
                    .warning(&format!("Plugin '{command}' returned an error response"));
            }
        }
        return;
    }

    // 4. Stdout fallback
    if let Some(out) = stdout {
        if !out.is_empty() {
            let lines: Vec<String> = out.lines().map(|s| s.to_string()).collect();
            app.show_short_or_info(
                crate::tui::components::dialogs::info::InfoType::Stats,
                lines,
            );
            return;
        }
    }

    // 5. Stderr fallback
    if let Some(err_out) = stderr {
        if !err_out.is_empty() {
            let mut lines: Vec<String> = err_out.lines().map(|s| s.to_string()).collect();
            lines.insert(0, format!("Plugin '{command}' stderr:"));
            app.messages_state.toasts.warning(&format!(
                "Plugin '{command}' produced stderr output"
            ));
            app.show_short_or_info(
                crate::tui::components::dialogs::info::InfoType::Stats,
                lines,
            );
            return;
        }
    }

    // 6. No output at all
    app.messages_state
        .toasts
        .warning(&format!("Plugin '{command}' produced no output"));
}

/// Apply a single plugin UI effect directly (without going through a
/// command response). This is the same as `App::apply_plugin_ui_effect`
/// but callable from the command dispatch path.
pub(crate) fn apply_plugin_ui_effect(app: &mut App, effect: UiEffect) {
    app.apply_plugin_ui_effect(effect);
}

fn level_label(level: crate::protocol::plugin::PluginDiagnosticLevel) -> &'static str {
    use crate::protocol::plugin::PluginDiagnosticLevel;
    match level {
        PluginDiagnosticLevel::Debug => "debug",
        PluginDiagnosticLevel::Info => "info",
        PluginDiagnosticLevel::Warning => "warn",
        PluginDiagnosticLevel::Error => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::plugin::{PluginDiagnostic, PluginDiagnosticLevel};
    use crate::protocol::ui::{DialogSpec, ToastLevel, ToastSpec, UiNode};
    use crate::tui::app::App;
    use crate::tui::app::state::PluginUiApplyResult;

    fn make_test_app() -> App {
        App::new_for_testing("/tmp".into())
    }

    fn text_node(s: &str) -> UiNode {
        use crate::protocol::ui::TextNode;
        UiNode::Text(TextNode { text: s.into() })
    }

    #[test]
    fn apply_plugin_ui_effect_delegates_to_app() {
        let mut app = make_test_app();
        let result = app.apply_plugin_ui_effect(UiEffect::ShowToast {
            toast: ToastSpec {
                level: ToastLevel::Info,
                message: "direct effect".into(),
            },
        });
        assert_eq!(result, PluginUiApplyResult::ToastRequested);
    }

    #[test]
    fn apply_plugin_ui_effect_dialog_opens() {
        let mut app = make_test_app();
        let result = app.apply_plugin_ui_effect(UiEffect::OpenDialog {
            dialog: DialogSpec {
                id: "test-dlg".into(),
                title: "Test".into(),
                body: text_node("hello"),
                modal: true,
            },
        });
        assert_eq!(result, PluginUiApplyResult::Applied);
        assert!(app.plugin_ui_state.get_dialog("test-dlg").is_some());
    }

    #[test]
    fn success_response_applies_effects() {
        let mut app = make_test_app();
        let resp = Box::new(PluginResponse {
            ok: true,
            effects: vec![
                UiEffect::ShowToast {
                    toast: ToastSpec {
                        level: ToastLevel::Success,
                        message: "done".into(),
                    },
                },
                UiEffect::OpenDialog {
                    dialog: DialogSpec {
                        id: "my-plugin:result".into(),
                        title: "Result".into(),
                        body: text_node("all good"),
                        modal: false,
                    },
                },
            ],
            data: serde_json::Value::Null,
            diagnostics: Vec::new(),
        });
        apply_plugin_command_finished(
            &mut app,
            "inv-1".into(),
            "test-cmd".into(),
            Some(resp),
            None,
            None,
            None,
        );
        // Toast was emitted
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(toasts.iter().any(|t| t == "done"));
        // Dialog was stored
        assert!(app.plugin_ui_state.get_dialog("my-plugin:result").is_some());
    }

    #[test]
    fn error_response_shows_error_toast() {
        let mut app = make_test_app();
        let resp = Box::new(PluginResponse {
            ok: false,
            effects: Vec::new(),
            data: serde_json::Value::Null,
            diagnostics: Vec::new(),
        });
        apply_plugin_command_finished(
            &mut app,
            "inv-2".into(),
            "failing-cmd".into(),
            Some(resp),
            None,
            None,
            None,
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.iter().any(|t| t.contains("error response")),
            "should show error warning toast, got: {toasts:?}"
        );
    }

    #[test]
    fn error_response_with_diagnostics_shows_info() {
        let mut app = make_test_app();
        let resp = Box::new(PluginResponse {
            ok: false,
            effects: Vec::new(),
            data: serde_json::Value::Null,
            diagnostics: vec![
                PluginDiagnostic {
                    level: PluginDiagnosticLevel::Error,
                    message: "bad input".into(),
                },
                PluginDiagnostic {
                    level: PluginDiagnosticLevel::Warning,
                    message: "deprecated flag".into(),
                },
            ],
        });
        apply_plugin_command_finished(
            &mut app,
            "inv-3".into(),
            "diag-cmd".into(),
            Some(resp),
            None,
            None,
            None,
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.iter().any(|t| t.contains("diag-cmd")),
            "should show diagnostic info, got: {toasts:?}"
        );
    }

    #[test]
    fn error_completion_shows_error_toast() {
        let mut app = make_test_app();
        apply_plugin_command_finished(
            &mut app,
            "inv-4".into(),
            "crash-cmd".into(),
            None,
            None,
            None,
            Some("process crashed".into()),
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.iter().any(|t| t.contains("crashed")),
            "should show error toast, got: {toasts:?}"
        );
    }

    #[test]
    fn error_with_stdout_shows_info_dialog() {
        let mut app = make_test_app();
        apply_plugin_command_finished(
            &mut app,
            "inv-5".into(),
            "fail-cmd".into(),
            None,
            Some("line1\nline2\nline3\nline4".into()),
            None,
            Some("something went wrong".into()),
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.iter().any(|t| t.contains("fail-cmd")),
            "should show error toast, got: {toasts:?}"
        );
    }

    #[test]
    fn stdout_fallback_short_toasts() {
        let mut app = make_test_app();
        apply_plugin_command_finished(
            &mut app,
            "inv-6".into(),
            "out-cmd".into(),
            None,
            Some("short output".into()),
            None,
            None,
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.iter().any(|t| t.contains("short output")),
            "short stdout should toast, got: {toasts:?}"
        );
    }

    #[test]
    fn stdout_fallback_long_opens_info() {
        let mut app = make_test_app();
        let long_output = (0..10).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        apply_plugin_command_finished(
            &mut app,
            "inv-7".into(),
            "long-cmd".into(),
            None,
            Some(long_output),
            None,
            None,
        );
        // Long output (>3 lines) opens info dialog, not just toasts
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            !toasts.iter().any(|t| t.contains("line 0")),
            "long stdout should not be in toast, got: {toasts:?}"
        );
    }

    #[test]
    fn stderr_fallback_shows_warning() {
        let mut app = make_test_app();
        apply_plugin_command_finished(
            &mut app,
            "inv-8".into(),
            "err-cmd".into(),
            None,
            None,
            Some("some stderr".into()),
            None,
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.iter().any(|t| t.contains("err-cmd")),
            "stderr should show warning, got: {toasts:?}"
        );
    }

    #[test]
    fn empty_completion_shows_warning() {
        let mut app = make_test_app();
        apply_plugin_command_finished(
            &mut app,
            "inv-9".into(),
            "empty-cmd".into(),
            None,
            None,
            None,
            None,
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.iter().any(|t| t.contains("no output")),
            "empty completion should warn, got: {toasts:?}"
        );
    }

    #[test]
    fn multiple_effects_apply_in_order() {
        let mut app = make_test_app();
        let resp = Box::new(PluginResponse {
            ok: true,
            effects: vec![
                UiEffect::OpenDialog {
                    dialog: DialogSpec {
                        id: "first".into(),
                        title: "First".into(),
                        body: text_node("one"),
                        modal: false,
                    },
                },
                UiEffect::OpenDialog {
                    dialog: DialogSpec {
                        id: "second".into(),
                        title: "Second".into(),
                        body: text_node("two"),
                        modal: false,
                    },
                },
                UiEffect::ShowToast {
                    toast: ToastSpec {
                        level: ToastLevel::Info,
                        message: "both opened".into(),
                    },
                },
            ],
            data: serde_json::Value::Null,
            diagnostics: Vec::new(),
        });
        apply_plugin_command_finished(
            &mut app,
            "inv-10".into(),
            "multi".into(),
            Some(resp),
            None,
            None,
            None,
        );
        assert!(app.plugin_ui_state.get_dialog("first").is_some());
        assert!(app.plugin_ui_state.get_dialog("second").is_some());
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(toasts.iter().any(|t| t == "both opened"));
    }
}
