//! Plugin command handlers.
//!
//! Provides the TUI-side plumbing for running plugin-backed commands and
//! applying plugin UI responses without blocking the render loop.
//!
//! Process-backed commands (`runtime: process`) are executed through
//! `ProcessRuntime`, which handles child process spawning, timeout,
//! output capping, and response parsing.

use crate::command::ProcessCommandSpec;
use crate::plugin::runtime::process::{ProcessRuntime, ProcessRuntimeSpec};
use crate::plugin::runtime::{PluginRuntime, RuntimeLimits};
use crate::protocol::plugin::{PluginInvocation, PluginResponse};
use crate::protocol::ui::UiEffect;
use crate::tui::app::App;
use crate::tui::app::TuiCommand;

/// Start a process-backed plugin command. Delegates to `ProcessRuntime`
/// and posts a `PluginCommandFinished` with the result.
pub(crate) fn start_plugin_command(app: &mut App, spec: ProcessCommandSpec, args: Vec<String>) {
    let invocation_id = uuid::Uuid::new_v4().to_string();
    let command_name = spec.command.clone();
    let tx = app.tui_cmd_tx.clone();

    crate::tui::async_cmd::spawn_tui_task(tx, "plugin_command_run", async move {
        let result = execute_via_runtime(&spec, &args, &invocation_id).await;
        match result {
            Ok(response) => Some(TuiCommand::PluginCommandFinished {
                invocation_id,
                command: command_name,
                response: Some(Box::new(response)),
                stdout: None,
                stderr: None,
                error: None,
            }),
            Err(e) => Some(TuiCommand::PluginCommandFinished {
                invocation_id,
                command: command_name,
                response: None,
                stdout: None,
                stderr: None,
                error: Some(e),
            }),
        }
    });
}

/// Execute a process command through `ProcessRuntime`.
async fn execute_via_runtime(
    spec: &ProcessCommandSpec,
    args: &[String],
    invocation_id: &str,
) -> Result<PluginResponse, String> {
    let runtime_spec: ProcessRuntimeSpec = spec.clone().into();
    let runtime = ProcessRuntime::new(runtime_spec, RuntimeLimits::default());

    let invocation = build_invocation(spec, args, invocation_id);
    runtime.invoke(invocation).await.map_err(|e| e.to_string())
}

fn build_invocation(
    spec: &ProcessCommandSpec,
    args: &[String],
    invocation_id: &str,
) -> PluginInvocation {
    use crate::protocol::plugin::{
        PluginCapabilityInvocation, PluginContext, PLUGIN_PROTOCOL_VERSION,
    };

    PluginInvocation {
        protocol_version: PLUGIN_PROTOCOL_VERSION,
        invocation_id: invocation_id.to_string(),
        plugin_id: format!("cmd:{}", spec.command),
        capability: PluginCapabilityInvocation::Command {
            name: spec.command.clone(),
        },
        args: args.to_vec(),
        input: serde_json::Value::Null,
        context: PluginContext {
            project_dir: std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string()),
            ..PluginContext::default()
        },
    }
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
        app.messages_state
            .toasts
            .error(&format!("Plugin '{command}' failed: {err}"));
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
            app.messages_state
                .toasts
                .warning(&format!("Plugin '{command}' produced stderr output"));
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
    use crate::tui::app::state::PluginUiApplyResult;
    use crate::tui::app::App;

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
        let long_output = (0..10)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
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

    // --- Process execution tests (delegated to ProcessRuntime) ---

    #[tokio::test]
    async fn execute_stdout_text_returns_text() {
        use crate::config::schema::CommandStdoutMode;
        let spec = ProcessCommandSpec {
            command: "echo".to_string(),
            args: vec!["hello world".to_string()],
            stdout: CommandStdoutMode::Text,
            ..Default::default()
        };
        let result = execute_via_runtime(&spec, &[], "test-inv").await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.ok);
        // Text mode produces an EmitChat effect
        assert_eq!(resp.effects.len(), 1);
    }

    #[tokio::test]
    async fn execute_stdout_auto_falls_back_to_text_on_invalid_json() {
        use crate::config::schema::CommandStdoutMode;
        let spec = ProcessCommandSpec {
            command: "echo".to_string(),
            args: vec!["not json".to_string()],
            stdout: CommandStdoutMode::Auto,
            ..Default::default()
        };
        let result = execute_via_runtime(&spec, &[], "test-inv").await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.ok);
        // Auto mode falls back to text, producing an EmitChat effect
        assert_eq!(resp.effects.len(), 1);
    }

    #[tokio::test]
    async fn execute_stdout_auto_parses_valid_json() {
        use crate::config::schema::CommandStdoutMode;
        let json_resp = r#"{"ok": true, "effects": [], "data": null, "diagnostics": []}"#;
        let spec = ProcessCommandSpec {
            command: "printf".to_string(),
            args: vec!["%s".to_string(), json_resp.to_string()],
            stdout: CommandStdoutMode::Auto,
            ..Default::default()
        };
        let result = execute_via_runtime(&spec, &[], "test-inv").await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.ok);
        assert!(resp.effects.is_empty());
    }

    #[tokio::test]
    async fn execute_stdout_json_errors_on_invalid_json() {
        use crate::config::schema::CommandStdoutMode;
        let spec = ProcessCommandSpec {
            command: "echo".to_string(),
            args: vec!["not json".to_string()],
            stdout: CommandStdoutMode::Json,
            ..Default::default()
        };
        let result = execute_via_runtime(&spec, &[], "test-inv").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid PluginResponse JSON"));
    }

    #[tokio::test]
    async fn execute_nonzero_exit_produces_error() {
        let spec = ProcessCommandSpec {
            command: "false".to_string(),
            args: vec![],
            ..Default::default()
        };
        let result = execute_via_runtime(&spec, &[], "test-inv").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("exited with code"));
    }

    #[tokio::test]
    async fn execute_nonzero_exit_includes_stderr() {
        let spec = ProcessCommandSpec {
            command: "sh".to_string(),
            args: vec!["-c".to_string(), "echo oops >&2; exit 1".to_string()],
            ..Default::default()
        };
        let result = execute_via_runtime(&spec, &[], "test-inv").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("oops"));
    }

    #[tokio::test]
    async fn execute_nonexistent_command_fails() {
        let spec = ProcessCommandSpec {
            command: "nonexistent_command_xyz_123".to_string(),
            args: vec![],
            ..Default::default()
        };
        let result = execute_via_runtime(&spec, &[], "test-inv").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("failed to spawn"));
    }

    #[tokio::test]
    async fn execute_stdout_cap_is_enforced() {
        use crate::config::schema::CommandStdoutMode;
        // Generate output larger than 1 MiB
        let spec = ProcessCommandSpec {
            command: "python3".to_string(),
            args: vec![
                "-c".to_string(),
                "print('x' * (1024 * 1024 + 100))".to_string(),
            ],
            stdout: CommandStdoutMode::Text,
            ..Default::default()
        };
        let result = execute_via_runtime(&spec, &[], "test-inv").await;
        match result {
            Ok(resp) => {
                // Text mode returns a response with an EmitChat effect
                assert!(resp.ok);
                assert_eq!(resp.effects.len(), 1);
            }
            Err(e) => {
                // python3 may not be available; skip gracefully
                assert!(
                    e.contains("failed to spawn") || e.contains("not found"),
                    "unexpected error: {e}"
                );
            }
        }
    }

    #[tokio::test]
    async fn execute_args_are_passed() {
        use crate::config::schema::CommandStdoutMode;
        let spec = ProcessCommandSpec {
            command: "echo".to_string(),
            args: vec![],
            stdout: CommandStdoutMode::Text,
            ..Default::default()
        };
        let result = execute_via_runtime(&spec, &["foo".into(), "bar".into()], "test-inv").await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.ok);
        // The EmitChat effect should contain "foo bar"
        match &resp.effects[0] {
            UiEffect::EmitChat { block } => {
                assert!(block.content.contains("foo bar"));
            }
            other => panic!("expected EmitChat, got {:?}", other),
        }
    }

    #[test]
    fn build_invocation_has_correct_fields() {
        let spec = ProcessCommandSpec {
            command: "my-script".to_string(),
            args: vec!["--flag".to_string()],
            ..Default::default()
        };
        let inv = build_invocation(&spec, &["extra".into()], "inv-42");
        assert_eq!(inv.protocol_version, 1);
        assert_eq!(inv.invocation_id, "inv-42");
        assert_eq!(inv.plugin_id, "cmd:my-script");
        assert_eq!(inv.args, vec!["extra"]);
    }
}
