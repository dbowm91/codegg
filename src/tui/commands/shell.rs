//! Shell command handlers for the TUI.
//!
//! Contains handlers for human shell execution, shell event processing,
//! shell output inclusion, rerunning, killing, listing, and showing shell commands.

use super::super as app;
use super::super::task_lifecycle::TuiTaskKind;

pub(crate) fn handle_run_human_shell(app: &mut app::App, command: String, promote_after: bool) {
    use crate::shell::policy::evaluate_command;

    let policy = evaluate_command(&command);
    match policy {
        crate::shell::policy::HumanShellPolicyDecision::Block { reason } => {
            app.messages_state
                .toasts
                .error(&format!("Blocked: {}", reason));
            return;
        }
        crate::shell::policy::HumanShellPolicyDecision::Warn { reason } => {
            let confirm_enabled = crate::config::schema::Config::load()
                .ok()
                .and_then(|c| c.human_shell)
                .map(|h| h.confirm_dangerous())
                .unwrap_or(true);
            if confirm_enabled {
                app.dialog_state.pending_shell_command = Some((command, promote_after));
                let title = "Dangerous Command".to_string();
                let msg = format!("{}\n\nRun this command anyway?", reason);
                app.ui_state.dialog = crate::tui::Dialog::Confirm;
                app.focus_manager.push(Box::new(
                    crate::tui::components::dialogs::confirm::ConfirmDialog::new(title, msg),
                ));
                return;
            } else {
                app.messages_state
                    .toasts
                    .warning(&format!("Warning: {}", reason));
            }
        }
        crate::shell::policy::HumanShellPolicyDecision::Allow => {}
    }

    spawn_human_shell(app, command, promote_after);
}

pub(crate) fn spawn_human_shell(app: &mut app::App, command: String, promote_after: bool) {
    use crate::shell::types::{ShellCapturePolicy, ShellEnvPolicy, ShellOrigin, ShellRequest};

    let id = app.shell_store.alloc_id();
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let capture_policy = if promote_after {
        ShellCapturePolicy::StoreAndPromote
    } else {
        ShellCapturePolicy::StoreEphemeral
    };
    let req = ShellRequest {
        id,
        origin: ShellOrigin::HumanEphemeral,
        command: command.clone(),
        cwd: cwd.clone(),
        timeout: std::time::Duration::from_secs(crate::shell::DEFAULT_TIMEOUT_SECS),
        capture_policy,
        env_policy: ShellEnvPolicy::Inherit,
    };
    app.shell_store.insert_started(&req);

    app.messages_state
        .messages
        .add_shell_cell(id.0, &command, &cwd.to_string_lossy());

    let (tx, mut rx) = tokio::sync::mpsc::channel(128);
    let runtime = crate::shell::ShellRuntime::new();
    let tui_cmd_tx = app.tui_cmd_tx.clone();
    app.task_registry
        .spawn(TuiTaskKind::Shell, "shell_event_forwarding", async move {
            match runtime.spawn(req, tx.clone()).await {
                Ok(_handle) => {
                    if let Some(ref ttx) = tui_cmd_tx {
                        let _ = ttx.try_send(app::TuiCommand::ShellEvent(
                            crate::shell::ShellEvent::Started { id, command, cwd },
                        ));
                    }
                    while let Some(event) = rx.recv().await {
                        if let Some(ref ttx) = tui_cmd_tx {
                            let _ = ttx.try_send(app::TuiCommand::ShellEvent(event));
                        }
                    }
                }
                Err(e) => {
                    if let Some(ref ttx) = tui_cmd_tx {
                        let _ = ttx.try_send(app::TuiCommand::ShellEvent(
                            crate::shell::ShellEvent::FailedToStart { id, error: e },
                        ));
                    }
                }
            }
        });
}

pub(crate) fn handle_shell_event(app: &mut app::App, event: crate::shell::ShellEvent) {
    // Mirror the event into the durable command-run store used by the
    // Phase 1 projection pipeline. This must happen for every event
    // variant (Started/Stdout/Stderr/Exited/TimedOut/FailedToStart)
    // so that the bridge has all the bytes it needs when it finalizes
    // the run on a terminal event.
    app.command_run_bridge
        .observe(&mut app.command_run_store, &event);

    match &event {
        crate::shell::ShellEvent::Started { id, .. } => {
            app.messages_state.messages.update_shell_cell(id.0, |cell| {
                cell.status = Some("running".to_string());
            });
        }
        crate::shell::ShellEvent::Stdout { id, bytes } => {
            app.shell_store.append_stdout(*id, bytes);
            let entry = app.shell_store.get(*id);
            let preview = entry.map(|e| e.stdout.head_str_lossy()).unwrap_or_default();
            let preview_lines: Vec<&str> = preview.lines().rev().take(8).collect();
            let stdout_preview: Vec<&str> = preview_lines.into_iter().rev().collect();
            let stdout_preview = stdout_preview.join("\n");
            let truncated = entry.map(|e| e.stdout.omitted_bytes > 0).unwrap_or(false);
            app.messages_state.messages.update_shell_cell(id.0, |cell| {
                cell.stdout_preview = Some(stdout_preview);
                cell.truncated = Some(truncated);
            });
        }
        crate::shell::ShellEvent::Stderr { id, bytes } => {
            app.shell_store.append_stderr(*id, bytes);
            let entry = app.shell_store.get(*id);
            let preview = entry.map(|e| e.stderr.head_str_lossy()).unwrap_or_default();
            let preview_lines: Vec<&str> = preview.lines().rev().take(8).collect();
            let stderr_preview: Vec<&str> = preview_lines.into_iter().rev().collect();
            let stderr_preview = stderr_preview.join("\n");
            app.messages_state.messages.update_shell_cell(id.0, |cell| {
                cell.stderr_preview = Some(stderr_preview);
            });
        }
        crate::shell::ShellEvent::Exited {
            id,
            status,
            elapsed,
        } => {
            // Do not overwrite Killed status from a late exited event
            if let Some(entry) = app.shell_store.get(*id) {
                if entry.status == crate::shell::types::ShellStatus::Killed {
                    return;
                }
            }
            app.shell_store.mark_exited(*id, *status, *elapsed);
            let elapsed_ms = elapsed.as_millis() as u64;
            let exit_code = *status;
            let status_str = "exited".to_string();
            let entry = app.shell_store.get(*id);
            let stdout_preview = entry.map(|e| e.stdout.head_str_lossy()).unwrap_or_default();
            let stderr_preview = entry.map(|e| e.stderr.head_str_lossy()).unwrap_or_default();
            let truncated = entry.map(|e| e.stdout.omitted_bytes > 0).unwrap_or(false);
            let command = entry.map(|e| e.command.clone()).unwrap_or_default();
            let _cwd = entry
                .map(|e| e.cwd.to_string_lossy().to_string())
                .unwrap_or_default();
            app.messages_state.messages.update_shell_cell(id.0, |cell| {
                cell.status = Some(status_str);
                cell.elapsed_ms = Some(elapsed_ms);
                cell.exit_code = exit_code;
                cell.stdout_preview = Some(stdout_preview);
                cell.stderr_preview = Some(stderr_preview);
                cell.truncated = Some(truncated);
            });

            let should_promote = entry
                .map(|e| e.promote_after && !e.promoted)
                .unwrap_or(false);
            if should_promote {
                if let Some(entry) = app.shell_store.get(*id) {
                    let digest = crate::shell::ShellDigest::build(
                        &command,
                        &entry.cwd,
                        entry.status,
                        exit_code,
                        *elapsed,
                        &entry.stdout,
                        &entry.stderr,
                    );
                    let include_text = if digest.has_failures() {
                        format!(
                            "Shell command output (auto-promoted on failure):\n{}",
                            digest.render()
                        )
                    } else {
                        let tail = entry.stderr.tail_str_lossy();
                        if tail.is_empty() {
                            let tail = entry.stdout.tail_str_lossy();
                            format!(
                                "Shell command output (auto-promoted):\n$ {}\n\n{}",
                                command, tail
                            )
                        } else {
                            format!(
                                "Shell command output (auto-promoted):\n$ {}\n\nstderr:\n{}",
                                command, tail
                            )
                        }
                    };
                    app.messages_state
                        .messages
                        .add_user_message(include_text, Some(false));
                    app.shell_store.mark_promoted(*id);
                    app.messages_state
                        .toasts
                        .info("Shell output auto-promoted to context");
                }
            }
        }
        crate::shell::ShellEvent::TimedOut { id, elapsed } => {
            app.shell_store.mark_timeout(*id, *elapsed);
            app.messages_state.messages.update_shell_cell(id.0, |cell| {
                cell.status = Some("timed_out".to_string());
                cell.elapsed_ms = Some(elapsed.as_millis() as u64);
            });
        }
        crate::shell::ShellEvent::FailedToStart { id, error } => {
            app.shell_store.mark_failed_to_start(*id);
            app.messages_state.messages.update_shell_cell(id.0, |cell| {
                cell.status = Some("failed".to_string());
                cell.stderr_preview = Some(format!("Failed to start: {}", error));
            });
        }
    }
}

pub(crate) fn handle_shell_include(
    app: &mut app::App,
    id: u64,
    mode: String,
    _question: Option<String>,
) {
    use crate::shell::types::{ShellCommandId, ShellPromotionMode};

    let cmd_id = ShellCommandId(id);
    if let Some(entry) = app.shell_store.get(cmd_id) {
        let command = entry.command.clone();
        let cwd = entry.cwd.clone();
        let exit_code = entry.exit_code;
        let elapsed = entry.elapsed.unwrap_or_default();
        let stdout = &entry.stdout;
        let stderr = &entry.stderr;

        let promotion = ShellPromotionMode::parse(&mode);
        let include_text = match promotion {
            ShellPromotionMode::Tail { lines } => {
                let stderr_text = stderr.head_str_lossy();
                let all_lines: Vec<&str> = stderr_text.lines().collect();
                let tail: Vec<&str> = all_lines.iter().rev().take(lines).rev().copied().collect();
                format!(
                    "Shell output (tail {} lines) for `{}`:\n{}",
                    lines,
                    command,
                    tail.join("\n")
                )
            }
            ShellPromotionMode::StdoutOnly => {
                let digest = crate::shell::ShellDigest::build(
                    &command,
                    &cwd,
                    entry.status,
                    exit_code,
                    elapsed,
                    stdout,
                    stderr,
                );
                if digest.has_failures() {
                    format!(
                        "Shell output (stdout + failures) for `{}`:\n{}",
                        command,
                        digest.render()
                    )
                } else {
                    format!(
                        "Shell output (stdout) for `{}`:\n{}",
                        command,
                        stdout.head_str_lossy()
                    )
                }
            }
            ShellPromotionMode::StderrOnly => {
                format!(
                    "Shell output (stderr) for `{}`:\n{}",
                    command,
                    stderr.head_str_lossy()
                )
            }
            ShellPromotionMode::Summary => {
                let digest = crate::shell::ShellDigest::build(
                    &command,
                    &cwd,
                    entry.status,
                    exit_code,
                    elapsed,
                    stdout,
                    stderr,
                );
                format!(
                    "Shell output (summary) for `{}`:\n{}",
                    command,
                    digest.render()
                )
            }
            ShellPromotionMode::FailureDigest => {
                let digest = crate::shell::ShellDigest::build(
                    &command,
                    &cwd,
                    entry.status,
                    exit_code,
                    elapsed,
                    stdout,
                    stderr,
                );
                if digest.has_failures() {
                    format!(
                        "Shell output (failure digest) for `{}`:\n{}",
                        command,
                        digest.render()
                    )
                } else {
                    format!(
                        "Shell output for `{}`:\nstdout:\n{}\nstderr:\n{}",
                        command,
                        stdout.head_str_lossy(),
                        stderr.head_str_lossy()
                    )
                }
            }
            ShellPromotionMode::Full => {
                let digest = crate::shell::ShellDigest::build(
                    &command,
                    &cwd,
                    entry.status,
                    exit_code,
                    elapsed,
                    stdout,
                    stderr,
                );
                if digest.has_failures() {
                    format!("Shell output for `{}`:\n{}", command, digest.render())
                } else {
                    format!(
                        "Shell output for `{}`:\nstdout:\n{}\nstderr:\n{}",
                        command,
                        stdout.head_str_lossy(),
                        stderr.head_str_lossy()
                    )
                }
            }
        };
        app.shell_store.mark_promoted(cmd_id);
        app.messages_state
            .messages
            .add_user_message(include_text, Some(false));
        app.messages_state
            .toasts
            .info("Shell output included in context");
    } else {
        app.messages_state
            .toasts
            .error(&format!("Shell command {} not found", id));
    }
}

pub(crate) fn handle_shell_ask(app: &mut app::App, id: u64, question: String) {
    use crate::shell::types::ShellCommandId;

    let cmd_id = ShellCommandId(id);
    if let Some(entry) = app.shell_store.get(cmd_id) {
        let command = entry.command.clone();
        let cwd = entry.cwd.clone();
        let exit_code = entry.exit_code;
        let elapsed = entry.elapsed.unwrap_or_default();
        let digest = crate::shell::ShellDigest::build(
            &command,
            &cwd,
            entry.status,
            exit_code,
            elapsed,
            &entry.stdout,
            &entry.stderr,
        );
        let include_text = format!(
            "Using the attached shell output, answer: {}\n\n{}",
            question,
            digest.render()
        );
        app.shell_store.mark_promoted(cmd_id);
        app.messages_state
            .messages
            .add_user_message(include_text, Some(false));
        app.messages_state
            .toasts
            .info("Shell output and question included in context");
    } else {
        app.messages_state
            .toasts
            .error(&format!("Shell command {} not found", id));
    }
}

pub(crate) fn handle_shell_rerun(app: &mut app::App, id: u64) {
    use crate::shell::types::ShellCommandId;

    let cmd_id = ShellCommandId(id);
    if let Some(entry) = app.shell_store.get(cmd_id) {
        let command = entry.command.clone();
        let promote_after = entry.promote_after;
        if let Some(ref tx) = app.tui_cmd_tx {
            let _ = tx.try_send(app::TuiCommand::RunHumanShell {
                command,
                promote_after,
            });
        }
    } else {
        app.messages_state
            .toasts
            .error(&format!("Shell command {} not found", id));
    }
}

pub(crate) fn handle_shell_kill(app: &mut app::App, id: u64) {
    if let Some(handle) = app.shell_handles.remove(&id) {
        handle.kill();
        let cmd_id = crate::shell::types::ShellCommandId(id);
        let elapsed = app
            .shell_store
            .get(cmd_id)
            .map(|e| e.started_at.elapsed().unwrap_or(std::time::Duration::ZERO))
            .unwrap_or(std::time::Duration::ZERO);
        app.shell_store.mark_killed(cmd_id, elapsed);
        app.messages_state
            .toasts
            .info(&format!("Killed shell command {}", id));
    } else {
        app.messages_state
            .toasts
            .error(&format!("No running shell command with id {}", id));
    }
}

pub(crate) fn handle_shell_list(app: &mut app::App) {
    let recent = app.shell_store.list_recent(10);
    if recent.is_empty() {
        app.messages_state
            .toasts
            .info("No shell commands in history");
        return;
    }
    let lines: Vec<String> = recent
        .iter()
        .map(|e| {
            let status_str = match e.status {
                crate::shell::types::ShellStatus::Running => {
                    let elapsed_str = e
                        .elapsed
                        .map(|d| format!("{:.1}s", d.as_secs_f64()))
                        .unwrap_or_else(|| "0.0s".to_string());
                    format!("running {}", elapsed_str)
                }
                crate::shell::types::ShellStatus::Exited => match e.exit_code {
                    Some(code) => {
                        let elapsed_str = e
                            .elapsed
                            .map(|d| format!("{:.1}s", d.as_secs_f64()))
                            .unwrap_or_default();
                        if elapsed_str.is_empty() {
                            format!("done exit={}", code)
                        } else {
                            format!("done exit={} {}", code, elapsed_str)
                        }
                    }
                    None => "done".to_string(),
                },
                crate::shell::types::ShellStatus::TimedOut => {
                    let elapsed_str = e
                        .elapsed
                        .map(|d| format!("{:.0}s", d.as_secs_f64()))
                        .unwrap_or_default();
                    if elapsed_str.is_empty() {
                        "timeout".to_string()
                    } else {
                        format!("timeout {}", elapsed_str)
                    }
                }
                crate::shell::types::ShellStatus::FailedToStart => "failed".to_string(),
                crate::shell::types::ShellStatus::Killed => {
                    let elapsed_str = e
                        .elapsed
                        .map(|d| format!("{:.1}s", d.as_secs_f64()))
                        .unwrap_or_default();
                    if elapsed_str.is_empty() {
                        "killed".to_string()
                    } else {
                        format!("killed {}", elapsed_str)
                    }
                }
            };
            let promoted_str = if e.promoted { " [promoted]" } else { "" };
            format!(
                "[{}] {}{} $ {}",
                e.id.0, status_str, promoted_str, e.command
            )
        })
        .collect();
    if lines.len() > 5 {
        app.open_info_dialog(
            crate::tui::components::dialogs::info::InfoType::ShellShow,
            lines,
        );
    } else {
        app.messages_state.toasts.info(&lines.join("\n"));
    }
}

pub(crate) fn handle_shell_show(app: &mut app::App, id: u64) {
    let entry = match app.shell_store.get(crate::shell::types::ShellCommandId(id)) {
        Some(e) => e.clone(),
        None => {
            app.messages_state
                .toasts
                .warning(&format!("No shell command with id {}", id));
            return;
        }
    };

    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("ID:       {}", entry.id.0));
    lines.push(format!("Command:  {}", entry.command));
    lines.push(format!("CWD:      {}", entry.cwd.display()));
    lines.push(format!(
        "Started:  {}",
        format_system_time(entry.started_at)
    ));
    if let Some(finished) = entry.finished_at {
        lines.push(format!("Finished: {}", format_system_time(finished)));
    }
    if let Some(ref elapsed) = entry.elapsed {
        lines.push(format!("Elapsed:  {:.1}s", elapsed.as_secs_f64()));
    }
    lines.push(format!("Status:   {}", format_shell_status(&entry.status)));
    if let Some(code) = entry.exit_code {
        lines.push(format!("Exit:     {}", code));
    }
    lines.push(format!(
        "Promoted: {}",
        if entry.promoted { "yes" } else { "no" }
    ));
    lines.push(format!("Capture:  {:?}", entry.capture_policy));

    let stdout = entry.stdout.head_str_lossy();
    let stderr = entry.stderr.head_str_lossy();
    let stdout_omitted = entry.stdout.omitted_bytes;
    let stderr_omitted = entry.stderr.omitted_bytes;

    if !stdout.is_empty() {
        lines.push(String::new());
        lines.push("── stdout ──".to_string());
        for line in stdout.lines() {
            lines.push(format!("  {}", line));
        }
        if stdout_omitted > 0 {
            lines.push(format!(
                "... ({} bytes omitted from head+tail buffer)",
                stdout_omitted
            ));
        }
    }
    if !stderr.is_empty() {
        lines.push(String::new());
        lines.push("── stderr ──".to_string());
        for line in stderr.lines() {
            lines.push(format!("  {}", line));
        }
        if stderr_omitted > 0 {
            lines.push(format!(
                "... ({} bytes omitted from head+tail buffer)",
                stderr_omitted
            ));
        }
    }
    if stdout.is_empty() && stderr.is_empty() {
        lines.push(String::new());
        lines.push("(no output captured)".to_string());
    }

    let info_type = crate::tui::components::dialogs::info::InfoType::ShellShow;
    let shell_footer =
        "i include  |  a ask  |  r rerun  |  k kill  |  j/k scroll  |  Esc close".to_string();
    if app.dialog_state.shell_detail_dialog.is_none() {
        let mut dialog = crate::tui::components::dialogs::info::InfoDialog::new(
            std::sync::Arc::clone(&app.ui_state.theme),
            info_type,
            lines,
        );
        dialog.set_custom_footer(shell_footer);
        app.dialog_state.shell_detail_dialog = Some(dialog);
    } else if let Some(ref mut dialog) = app.dialog_state.shell_detail_dialog {
        dialog.set_info_type(info_type);
        dialog.set_content(lines);
        dialog.set_theme(&app.ui_state.theme);
        dialog.set_custom_footer(shell_footer);
    }
    if let Some(ref dialog) = app.dialog_state.shell_detail_dialog {
        app.dialog_state.shell_detail_id = Some(id);
        app.focus_manager.push(Box::new(dialog.clone()));
        app.ui_state.dialog = crate::tui::Dialog::ShellShow;
    }
}

pub(crate) fn format_system_time(t: std::time::SystemTime) -> String {
    use std::time::UNIX_EPOCH;
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}", secs)
}

pub(crate) fn format_shell_status(status: &crate::shell::types::ShellStatus) -> &'static str {
    match status {
        crate::shell::types::ShellStatus::Running => "running",
        crate::shell::types::ShellStatus::Exited => "exited",
        crate::shell::types::ShellStatus::TimedOut => "timed out",
        crate::shell::types::ShellStatus::FailedToStart => "failed to start",
        crate::shell::types::ShellStatus::Killed => "killed",
    }
}
