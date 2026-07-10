//! /test slash command handler for supervised test execution.

use crate::test_runner::custom::validate_custom_command;
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// Parse the raw arguments after `/test` into a scope string and extra args.
pub(crate) fn parse_test_slash_args(raw: &str) -> (String, String) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return ("auto".to_string(), String::new());
    }
    let mut parts = trimmed.splitn(2, ' ');
    let subcmd = parts.next().unwrap_or("auto").to_lowercase();
    let rest = parts.next().unwrap_or("").trim().to_string();

    match subcmd.as_str() {
        "workspace" | "ws" => ("workspace".to_string(), String::new()),
        "changed" | "diff" => ("changed".to_string(), String::new()),
        "package" | "pkg" | "p" => ("package".to_string(), rest),
        "file" | "f" => ("file".to_string(), rest),
        "previous" | "prev" | "last" => ("previous_failures".to_string(), String::new()),
        "custom" | "cmd" => ("custom".to_string(), rest),
        _ => ("auto".to_string(), trimmed.to_string()),
    }
}

/// Build a TestRunRequest from scope and args.
fn build_test_request(
    scope: &str,
    args: &str,
) -> Result<crate::test_runner::TestRunRequest, String> {
    use crate::test_runner::TestScope;
    use std::path::PathBuf;

    let test_scope = match scope {
        "auto" => TestScope::Auto,
        "workspace" => TestScope::Workspace,
        "changed" => TestScope::Changed,
        "package" => {
            if args.trim().is_empty() {
                return Err("package scope requires a package name".into());
            }
            TestScope::Package(args.trim().to_string())
        }
        "file" => {
            if args.trim().is_empty() {
                return Err("file scope requires a file path".into());
            }
            TestScope::File(PathBuf::from(args.trim()))
        }
        "previous_failures" => TestScope::PreviousFailures,
        "custom" => {
            if args.trim().is_empty() {
                return Err("custom scope requires a command".into());
            }
            let cmd = args.trim();
            if validate_custom_command(cmd).is_err() {
                return Err(format!(
                    "custom command rejected by safety validator: '{cmd}'. \
                     Allowed argv prefixes: cargo test, cargo nextest, pytest, \
                     uv run pytest, go test, zig build test, make test, make check, \
                     npm test, pnpm test, yarn test, bun test. \
                     Shell metacharacters, redirection, pipes, command substitution, \
                     and newlines are not allowed."
                ));
            }
            TestScope::CustomCommand(cmd.to_string())
        }
        other => return Err(format!("unknown scope '{other}'")),
    };

    let workdir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    Ok(crate::test_runner::TestRunRequest {
        scope: test_scope,
        workdir,
        timeout_secs: None,
        stall_timeout_secs: None,
        max_report_bytes: None,
        session_id: None,
    })
}

/// Start a supervised test run from the /test slash command.
pub(crate) fn start_test_run(app: &mut App, scope: String, args: String) {
    let tx = app.tui_cmd_tx.clone();
    let request_id = app.dialog_state.test_run_request.begin();
    let run_store = app.run_store.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "test_run",
        async move {
            let request = match build_test_request(&scope, &args) {
                Ok(r) => r,
                Err(e) => {
                    return Some(TuiCommand::TestRunFinished {
                        request_id,
                        report: None,
                        error: Some(e),
                    });
                }
            };

            let report = crate::test_runner::resolve_and_run_test(
                request,
                Some(&crate::test_runner::BusEventSink),
                run_store.as_ref(),
            )
            .await
            .map_err(|e| format!("test runner error: {e}"));

            match report {
                Ok(report) => Some(TuiCommand::TestRunFinished {
                    request_id,
                    report: Some(Box::new(report)),
                    error: None,
                }),
                Err(e) => Some(TuiCommand::TestRunFinished {
                    request_id,
                    report: None,
                    error: Some(e),
                }),
            }
        },
    );
}

/// Handle the TestRunFinished completion: display the report in the UI.
pub(crate) fn apply_test_run_finished(
    app: &mut App,
    request_id: u64,
    report: Option<Box<crate::test_runner::TestReport>>,
    error: Option<String>,
) {
    if !app.dialog_state.test_run_request.finish(request_id) {
        return;
    }
    if let Some(err) = error {
        app.messages_state.toasts.error(&err);
        return;
    }
    if let Some(report) = report {
        let output = crate::test_runner::format_test_report(&report);
        let lines: Vec<String> = output.lines().map(|s| s.to_string()).collect();
        if lines.len() <= 3 {
            app.messages_state.toasts.info(&output);
        } else {
            app.open_info_dialog(
                crate::tui::components::dialogs::info::InfoType::DoctorReport,
                lines,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_test_no_args_maps_to_auto() {
        let (scope, args) = parse_test_slash_args("");
        assert_eq!(scope, "auto");
        assert_eq!(args, "");
    }

    #[test]
    fn slash_test_workspace_maps_to_workspace() {
        let (scope, args) = parse_test_slash_args("workspace");
        assert_eq!(scope, "workspace");
        assert_eq!(args, "");
    }

    #[test]
    fn slash_test_changed_maps_to_changed() {
        let (scope, args) = parse_test_slash_args("changed");
        assert_eq!(scope, "changed");
        assert_eq!(args, "");
    }

    #[test]
    fn slash_test_package_requires_name() {
        let (scope, args) = parse_test_slash_args("package codegg-core");
        assert_eq!(scope, "package");
        assert_eq!(args, "codegg-core");
    }

    #[test]
    fn slash_test_file_requires_path() {
        let (scope, args) = parse_test_slash_args("file tests/foo.rs");
        assert_eq!(scope, "file");
        assert_eq!(args, "tests/foo.rs");
    }

    #[test]
    fn slash_test_previous_maps_to_previous_failures() {
        let (scope, args) = parse_test_slash_args("previous");
        assert_eq!(scope, "previous_failures");
        assert_eq!(args, "");
    }

    #[test]
    fn slash_test_custom_uses_same_validation_as_tool() {
        let (scope, args) = parse_test_slash_args("custom cargo test --lib");
        assert_eq!(scope, "custom");
        assert_eq!(args, "cargo test --lib");
    }

    #[test]
    fn slash_test_aliases() {
        let (scope, _) = parse_test_slash_args("ws");
        assert_eq!(scope, "workspace");
        let (scope, _) = parse_test_slash_args("diff");
        assert_eq!(scope, "changed");
        let (scope, _) = parse_test_slash_args("prev");
        assert_eq!(scope, "previous_failures");
        let (scope, _) = parse_test_slash_args("pkg my-crate");
        assert_eq!(scope, "package");
        let (scope, _) = parse_test_slash_args("f src/main.rs");
        assert_eq!(scope, "file");
    }

    #[test]
    fn slash_test_command_is_registered() {
        let registry = crate::tui::command::CommandRegistry::new();
        let cmd = registry.find_by_name_or_alias("/test");
        assert!(cmd.is_some(), "/test command not found in registry");
        let cmd = cmd.unwrap();
        assert_eq!(cmd.name, "/test");
    }

    #[test]
    fn tui_test_custom_rejects_semicolon_suffix() {
        // Bypass regression: TUI /test custom must use the strict validator.
        assert!(build_test_request("custom", "cargo test; rm -rf /").is_err());
    }

    #[test]
    fn tui_test_custom_rejects_newline_suffix() {
        assert!(build_test_request("custom", "cargo test\nrm -rf /").is_err());
    }

    #[test]
    fn tui_test_custom_rejects_pipe_suffix() {
        assert!(build_test_request("custom", "pytest | tee /tmp/out").is_err());
    }

    #[test]
    fn tui_test_custom_rejects_command_substitution() {
        assert!(build_test_request("custom", "pytest $(curl evil)").is_err());
        assert!(build_test_request("custom", "cargo test `curl evil`").is_err());
    }

    #[test]
    fn tui_test_custom_rejects_prefix_collision() {
        assert!(build_test_request("custom", "pytestevil").is_err());
        assert!(build_test_request("custom", "cargo testify").is_err());
    }

    #[test]
    fn tui_test_custom_accepts_normal_pytest_args() {
        let req = build_test_request("custom", "pytest -q tests/test_foo.py").unwrap();
        assert!(matches!(
            req.scope,
            crate::test_runner::TestScope::CustomCommand(_)
        ));
    }

    #[test]
    fn tui_test_custom_accepts_normal_cargo_test_args() {
        let req = build_test_request("custom", "cargo test --lib -p codegg-core").unwrap();
        assert!(matches!(
            req.scope,
            crate::test_runner::TestScope::CustomCommand(_)
        ));
    }
}
