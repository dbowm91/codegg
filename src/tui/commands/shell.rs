//! Shell command handlers for the TUI.
//!
//! Contains handlers for human shell execution, shell event processing,
//! shell output inclusion, rerunning, killing, listing, and showing shell commands.

use super::super::task_lifecycle::TuiTaskKind;
use crate::tui as app;
use crate::tui::app::send_tui;
use crate::util::truncate::truncate_prefix;

pub(crate) fn handle_run_human_shell(
    app: &mut app::App,
    command: String,
    promote_after: bool,
    cwd: std::path::PathBuf,
) {
    use crate::shell::policy::evaluate_command;

    if !app.shell_enabled {
        app.messages_state
            .toasts
            .error("Human shell is disabled by configuration");
        return;
    }

    let policy = evaluate_command(&command);
    match policy {
        crate::shell::policy::HumanShellPolicyDecision::Block { reason } => {
            app.messages_state
                .toasts
                .error(&format!("Blocked: {}", reason));
            return;
        }
        crate::shell::policy::HumanShellPolicyDecision::Warn { reason } => {
            let confirm_enabled = app.shell_confirm_dangerous;
            if confirm_enabled {
                app.dialog_state.pending_shell_command = Some((command, promote_after, cwd));
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

    spawn_human_shell(app, command, promote_after, cwd);
}

pub(crate) fn spawn_human_shell(
    app: &mut app::App,
    command: String,
    promote_after: bool,
    cwd: std::path::PathBuf,
) {
    use crate::shell::types::{ShellCapturePolicy, ShellEnvPolicy, ShellOrigin, ShellRequest};

    let promote_after = promote_after && app.shell_auto_promote_bangbang;

    let id = app.shell_store.alloc_id();
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
        timeout: std::time::Duration::from_secs(app.shell_default_timeout_secs),
        capture_policy,
        env_policy: ShellEnvPolicy::Inherit,
    };
    app.shell_store.insert_started(&req);
    capture_shell_binding(app, id.0);

    app.messages_state
        .messages
        .add_shell_cell(id.0, &command, &cwd.to_string_lossy());

    let (tx, mut rx) = tokio::sync::mpsc::channel(128);
    let runtime = crate::shell::ShellRuntime::new();
    let tui_cmd_tx = app.tui_cmd_tx.clone();
    app.task_registry
        .spawn(TuiTaskKind::Shell, "shell_event_forwarding", async move {
            match runtime.spawn(req, tx.clone()).await {
                Ok(handle) => {
                    if let Some(ref ttx) = tui_cmd_tx {
                        let _ = send_tui(
                            ttx,
                            app::TuiCommand::RegisterShellHandle {
                                id: handle.id.0,
                                handle,
                            },
                        );
                    }
                    while let Some(event) = rx.recv().await {
                        if let Some(ref ttx) = tui_cmd_tx {
                            let _ = send_tui(ttx, app::TuiCommand::ShellEvent(event));
                        }
                    }
                }
                Err(e) => {
                    if let Some(ref ttx) = tui_cmd_tx {
                        let _ = send_tui(
                            ttx,
                            app::TuiCommand::ShellEvent(crate::shell::ShellEvent::FailedToStart {
                                id,
                                error: e,
                            }),
                        );
                    }
                }
            }
        });
}

fn current_shell_binding(app: &app::App) -> (Option<String>, Option<String>) {
    let session_id = app.active_session_id().map(str::to_owned).or_else(|| {
        app.session_state
            .session
            .as_ref()
            .map(|session| session.id.clone())
    });
    let workspace_id = app.active_workspace_id().map(str::to_owned).or_else(|| {
        app.session_state
            .session
            .as_ref()
            .and_then(|session| session.workspace_id.clone())
    });
    (session_id, workspace_id)
}

pub(crate) fn capture_shell_binding(app: &mut app::App, id: u64) {
    let binding = current_shell_binding(app);
    app.shell_command_bindings.insert(id, binding);
    let retained: std::collections::HashSet<u64> = app
        .shell_store
        .list_recent(usize::MAX)
        .iter()
        .map(|entry| entry.id.0)
        .collect();
    app.shell_command_bindings
        .retain(|run_id, _| retained.contains(run_id));
}

fn shell_binding_matches_current(app: &app::App, id: u64) -> bool {
    app.shell_command_bindings
        .get(&id)
        .is_some_and(|binding| binding == &current_shell_binding(app))
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
            let preview = entry
                .map(|e| crate::shell::sanitize_ansi(&e.stdout.head_str_lossy(), app.shell_ansi))
                .unwrap_or_default();
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
            let preview = entry
                .map(|e| crate::shell::sanitize_ansi(&e.stderr.head_str_lossy(), app.shell_ansi))
                .unwrap_or_default();
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
            app.shell_handles.remove(&id.0);
            app.shell_store.mark_exited(*id, *status, *elapsed);
            let elapsed_ms = elapsed.as_millis() as u64;
            let exit_code = *status;
            let status_str = "exited".to_string();
            let entry = app.shell_store.get(*id);
            let stdout_preview = entry
                .map(|e| crate::shell::sanitize_ansi(&e.stdout.head_str_lossy(), app.shell_ansi))
                .unwrap_or_default();
            let stderr_preview = entry
                .map(|e| crate::shell::sanitize_ansi(&e.stderr.head_str_lossy(), app.shell_ansi))
                .unwrap_or_default();
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

            // Populate projection metadata from the command-run store
            if let Some(run) = app
                .command_run_store
                .get_run(crate::shell::projection::CommandRunId(id.0))
            {
                let shell_output_config = app.shell_output_config.clone();
                let result = crate::shell::projector::config_command_projection(
                    run,
                    &app.command_run_store,
                    &shell_output_config,
                    crate::shell::projector::ProjectionTarget::ModelContext,
                );
                let omitted_summary = if result.omitted.is_empty() {
                    None
                } else {
                    let total_omitted: usize =
                        result.omitted.iter().map(|o| o.total_retained_bytes).sum();
                    if total_omitted > 0 {
                        Some(crate::shell::projector::format_bytes(total_omitted as u64))
                    } else {
                        None
                    }
                };
                let raw_handle = result.expansion_handles.first().map(|h| h.as_url());
                app.messages_state.messages.update_shell_cell(id.0, |cell| {
                    cell.projection_projector = Some(result.projector);
                    cell.projection_exactness = Some(result.exactness.label().to_string());
                    cell.projection_input_bytes = Some(result.input_bytes);
                    cell.projection_output_bytes = Some(result.output_bytes);
                    cell.projection_omitted = omitted_summary;
                    cell.projection_raw_handle = raw_handle;
                });
            }

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
                    if app.session_state.session.is_none() {
                        app.messages_state.toasts.warning(
                            "Shell output remains private because no session is bound; select a session before promoting it",
                        );
                        return;
                    }
                    if !shell_binding_matches_current(app, id.0) {
                        app.messages_state.toasts.warning(
                            "Shell output stayed private because the active session or workspace changed",
                        );
                        return;
                    }
                    match project_for_model(&include_text) {
                        Ok(include_text) => {
                            if !stage_remote_shell_projection(app, include_text.clone()) {
                                app.messages_state
                                    .messages
                                    .add_user_message(include_text.clone(), Some(false));
                                app.pending_shell_promotions.push((
                                    id.0,
                                    include_text,
                                    uuid::Uuid::new_v4(),
                                ));
                                app.messages_state.toasts.info(
                                    "Redacted shell output staged for the next submitted turn",
                                );
                            }
                        }
                        Err(error) => app.messages_state.toasts.error(&error),
                    }
                }
            }
        }
        crate::shell::ShellEvent::TimedOut { id, elapsed } => {
            app.shell_handles.remove(&id.0);
            app.shell_store.mark_timeout(*id, *elapsed);
            app.messages_state.messages.update_shell_cell(id.0, |cell| {
                cell.status = Some("timed_out".to_string());
                cell.elapsed_ms = Some(elapsed.as_millis() as u64);
            });
        }
        crate::shell::ShellEvent::FailedToStart { id, error } => {
            app.shell_handles.remove(&id.0);
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

    if app.observer.blocks_prompt_submit() {
        app.messages_state
            .toasts
            .warning("Observer sessions cannot promote shell output into model context");
        return;
    }
    if app.session_state.session.is_none() {
        app.messages_state
            .toasts
            .warning("Select a session before promoting shell output; the output remains private");
        return;
    }
    let cmd_id = ShellCommandId(id);
    if let Some(entry) = app.shell_store.get(cmd_id) {
        if !shell_binding_matches_current(app, id) {
            app.messages_state
                .toasts
                .warning("Shell output cannot be promoted into a different session or workspace");
            return;
        }
        if entry.status != crate::shell::types::ShellStatus::Exited {
            app.messages_state
                .toasts
                .error("Shell output can only be promoted after the command exits");
            return;
        }
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
        match project_for_model(&include_text) {
            Ok(include_text) => {
                if stage_remote_shell_projection(app, include_text.clone()) {
                    return;
                }
                app.messages_state
                    .messages
                    .add_user_message(include_text.clone(), Some(false));
                app.pending_shell_promotions
                    .push((id, include_text, uuid::Uuid::new_v4()));
                app.messages_state
                    .toasts
                    .info("Redacted shell output staged for the next submitted turn");
            }
            Err(error) => app.messages_state.toasts.error(&error),
        }
    } else {
        app.messages_state
            .toasts
            .error(&format!("Shell command {} not found", id));
    }
}

pub(crate) fn handle_shell_ask(app: &mut app::App, id: u64, question: String) {
    use crate::shell::types::ShellCommandId;

    if app.observer.blocks_prompt_submit() {
        app.messages_state
            .toasts
            .warning("Observer sessions cannot submit shell output to a model");
        return;
    }
    let cmd_id = ShellCommandId(id);
    if let Some(entry) = app.shell_store.get(cmd_id) {
        if app.session_state.session.is_some() && !shell_binding_matches_current(app, id) {
            app.messages_state
                .toasts
                .warning("Shell output cannot be submitted from a different session or workspace");
            return;
        }
        if entry.status != crate::shell::types::ShellStatus::Exited {
            app.messages_state
                .toasts
                .error("Shell output can only be promoted after the command exits");
            return;
        }
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
        match project_for_model(&include_text) {
            Ok(include_text) => {
                submit_shell_ask(app, id, include_text);
            }
            Err(error) => app.messages_state.toasts.error(&error),
        }
    } else {
        app.messages_state
            .toasts
            .error(&format!("Shell command {} not found", id));
    }
}

fn submit_shell_ask(app: &mut app::App, id: u64, text: String) {
    use crate::protocol::core::{CoreRequest, CoreResponse};

    if let Some(client) = app.core_client.clone() {
        let Some(session_id) = app.session_state.session.as_ref().map(|s| s.id.clone()) else {
            app.prompt_state.prompt.set_text(text);
            app.messages_state
                .toasts
                .warning("Select a session before asking about shell output; evidence remains in the composer");
            return;
        };
        let Some(tx) = app.tui_cmd_tx.clone() else {
            app.prompt_state.prompt.set_text(text);
            app.messages_state
                .toasts
                .error("Cannot submit shell question: TUI command channel is unavailable");
            return;
        };
        let submitted_text = text.clone();
        let request = crate::core::new_request(
            uuid::Uuid::new_v4().to_string(),
            CoreRequest::SessionPromptSubmit {
                session_id,
                text,
                plan_mode: app.agent_state.plan_mode,
            },
        );
        app.task_registry
            .spawn(TuiTaskKind::Command, "shell_ask_submit", async move {
                let error = match client.request(request).await {
                    Ok(CoreResponse::Ack) => None,
                    Ok(CoreResponse::Error { code, message }) => Some(format!("{code}: {message}")),
                    Ok(_) => Some("unexpected response to shell question".to_string()),
                    Err(error) => Some(error.to_string()),
                };
                let _ = send_tui(
                    &tx,
                    app::TuiCommand::ShellAskSubmitted {
                        id,
                        text: submitted_text,
                        error,
                    },
                );
            });
        return;
    }

    if let Some(tx) = app.remote_send_tx.as_ref() {
        match tx.try_send(crate::protocol::tui::TuiMessage::Input { text }) {
            Ok(()) => {
                let _ = id;
                app.messages_state
                    .toasts
                    .info("Redacted shell question sent; remote session acceptance is pending");
            }
            Err(error) => {
                app.messages_state.toasts.error(&format!(
                    "Shell question was not submitted; retry when the remote connection is ready: {error}"
                ));
            }
        }
        return;
    }

    app.prompt_state.prompt.set_text(text);
    app.messages_state
        .toasts
        .error("Cannot submit shell question without a session connection; evidence remains in the composer");
}

pub(crate) fn apply_shell_ask_submitted(
    app: &mut app::App,
    id: u64,
    text: String,
    error: Option<String>,
) {
    if let Some(error) = error {
        if app.prompt_state.prompt.get_text().is_empty() {
            app.prompt_state.prompt.set_text(text);
        }
        app.messages_state
            .toasts
            .error(&format!("Shell question was not accepted: {error}"));
        return;
    }
    app.shell_store
        .mark_promoted(crate::shell::types::ShellCommandId(id));
    if app.prompt_state.prompt.get_text() == text {
        app.prompt_state.prompt.clear();
    }
    app.messages_state
        .toasts
        .info("Redacted shell output and question accepted for the session");
}

/// Apply the canonical model-target redaction hook to every human-shell
/// promotion mode. Human promotion fails closed when the hook cannot produce
/// an applied state, regardless of the general shell-output redaction setting.
fn project_for_model(text: &str) -> Result<String, String> {
    use crate::shell::projection::RedactionState;
    use crate::shell::projector::{
        apply_redaction_hook, ProjectionKind, ProjectionResult, ProjectionTarget,
    };

    const MAX_PROMOTION_BYTES: usize = 32 * 1024;
    const TRUNCATION_MARKER: &str = "\n[human-shell promotion truncated]";
    const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;
    if text.len() > MAX_SOURCE_BYTES {
        return Err(
            "Shell output was not promoted because its source exceeded the safe projection limit"
                .to_string(),
        );
    }
    let sanitized = crate::shell::sanitize_ansi(text, crate::config::schema::AnsiMode::Strip);
    if sanitized.len() > MAX_SOURCE_BYTES {
        return Err(
            "Shell output was not promoted because its source exceeded the safe projection limit"
                .to_string(),
        );
    }
    let mut projection = ProjectionResult::empty("human-shell-promotion", ProjectionKind::Raw);
    // Redact the complete bounded source before applying the smaller model
    // budget. This prevents a secret that straddles the output boundary from
    // escaping as a truncated, unmatched prefix.
    projection.text = sanitized;
    projection.output_bytes = projection.text.len();
    apply_redaction_hook(&mut projection, ProjectionTarget::ModelContext);
    if !matches!(
        projection.redaction,
        RedactionState::Applied { .. } | RedactionState::AppliedNoMatches
    ) {
        return Err("Shell output was not promoted because model redaction failed".to_string());
    }
    if projection.text.len() > MAX_PROMOTION_BYTES {
        let mut end = MAX_PROMOTION_BYTES - TRUNCATION_MARKER.len();
        while !projection.text.is_char_boundary(end) {
            end -= 1;
        }
        projection.text.truncate(end);
        projection.text.push_str(TRUNCATION_MARKER);
    }
    Ok(projection.text)
}

/// The WebSocket TUI protocol accepts only a prompt string, not a separate
/// staged-context DTO. Put approved shell evidence in the composer so the
/// user's next remote input actually carries it; never represent the outbound
/// queue write as owner acknowledgement or mark the run promoted here.
fn stage_remote_shell_projection(app: &mut app::App, text: String) -> bool {
    let remote_tui = matches!(
        app.ui_state.mode,
        crate::tui::app::state::AppMode::RemoteCore { .. }
    ) && app.remote_send_tx.is_some();
    if !remote_tui {
        return false;
    }
    if app.prompt_state.prompt.get_text().is_empty() {
        app.prompt_state.prompt.set_text(text);
        app.messages_state.toasts.info(
            "Redacted shell evidence is in the composer; submit it to send it to the remote session",
        );
    } else {
        app.messages_state.toasts.warning(
            "Remote session cannot stage shell context separately; existing composer text was kept",
        );
    }
    true
}

pub(crate) fn handle_shell_rerun(app: &mut app::App, id: u64) {
    use crate::shell::types::ShellCommandId;

    let cmd_id = ShellCommandId(id);
    if let Some(entry) = app.shell_store.get(cmd_id) {
        let command = entry.command.clone();
        let promote_after = entry.promote_after;
        let cwd = match app.project_execution_context() {
            Ok(context) => context.workspace_root,
            Err(error) => {
                app.messages_state.toasts.error(&error);
                return;
            }
        };
        if let Some(ref tx) = app.tui_cmd_tx {
            let _ = send_tui(
                tx,
                app::TuiCommand::RunHumanShell {
                    command,
                    promote_after,
                    cwd,
                },
            );
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

    // Projection metadata from the durable command-run store
    let cmd_run_id = crate::shell::projection::CommandRunId(id);
    if let Some(run) = app.command_run_store.get_run(cmd_run_id) {
        lines.push(String::new());
        lines.push("── projection ──".to_string());

        // Raw retention info
        let stdout_total = run.stdout.total_bytes;
        let stdout_retained = run.stdout.retained_bytes;
        let stderr_total = run.stderr.total_bytes;
        let stderr_retained = run.stderr.retained_bytes;
        lines.push(format!(
            "Raw:      stdout {} / stderr {} (retained)",
            crate::shell::projector::format_bytes(stdout_retained),
            crate::shell::projector::format_bytes(stderr_retained)
        ));
        if stdout_total != stdout_retained || stderr_total != stderr_retained {
            lines.push(format!(
                "Observed: stdout {} / stderr {} (total)",
                crate::shell::projector::format_bytes(stdout_total),
                crate::shell::projector::format_bytes(stderr_total)
            ));
        }
        if run.is_partial() {
            lines.push("Partial:  yes (output exceeded retention cap)".to_string());
        }
        lines.push(format!("Exit:     {}", run.exit.label()));

        // Projection result
        let shell_output_config = app.shell_output_config.clone();
        let result = crate::shell::projector::config_command_projection(
            run,
            &app.command_run_store,
            &shell_output_config,
            crate::shell::projector::ProjectionTarget::TuiDetail,
        );
        lines.push(format!("Projector: {}", result.projector));
        lines.push(format!("Exact:    {}", result.exactness.label()));
        lines.push(format!(
            "Output:   {}",
            crate::shell::projector::format_bytes(result.output_bytes as u64)
        ));

        // Omitted ranges
        if !result.omitted.is_empty() {
            lines.push(format!("Omitted:  {} range(s)", result.omitted.len()));
            for (i, omitted) in result.omitted.iter().enumerate() {
                lines.push(format!(
                    "  [{}] {} {}..{} ({} bytes total)",
                    i,
                    omitted.stream.as_str(),
                    omitted.start_byte,
                    omitted.end_byte,
                    omitted.total_retained_bytes
                ));
            }
        }

        // Expansion handles
        if !result.expansion_handles.is_empty() {
            lines.push(format!("Handles: {}", result.expansion_handles.len()));
            for handle in &result.expansion_handles {
                lines.push(format!("  {}", handle.as_url()));
            }
        }

        // Warnings
        if !result.warnings.is_empty() {
            lines.push("Warnings:".to_string());
            for w in &result.warnings {
                lines.push(format!("  - {}", w));
            }
        }
    }

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

    show_shell_detail(
        app,
        id,
        lines,
        "i include  |  a ask  |  r rerun  |  k kill  |  e expand  |  j/k scroll  |  Esc close",
    );
}

fn show_shell_detail(app: &mut app::App, id: u64, lines: Vec<String>, footer: &str) {
    let info_type = crate::tui::components::dialogs::info::InfoType::ShellShow;
    if let Some(dialog) = app
        .focus_manager
        .dialog_mut_any::<crate::tui::components::dialogs::info::InfoDialog>()
    {
        dialog.set_info_type(info_type);
        dialog.set_content(lines);
        dialog.set_theme(&app.ui_state.theme);
        dialog.set_custom_footer(footer.to_string());
    } else {
        let mut dialog = crate::tui::components::dialogs::info::InfoDialog::new(
            std::sync::Arc::clone(&app.ui_state.theme),
            info_type,
            lines,
        );
        dialog.set_custom_footer(footer.to_string());
        app.focus_manager.push(Box::new(dialog));
    }
    app.dialog_state.shell_detail_id = Some(id);
    app.ui_state.dialog = crate::tui::Dialog::ShellShow;
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

pub(crate) fn handle_shell_expand(
    app: &mut app::App,
    id: u64,
    stream: String,
    range: Option<String>,
) {
    use crate::shell::projection::CommandRunId;

    let cmd_id = CommandRunId(id);

    // Check if the command exists in the legacy store for a friendly message
    let has_legacy = app
        .shell_store
        .get(crate::shell::types::ShellCommandId(id))
        .is_some();
    if !has_legacy {
        app.messages_state
            .toasts
            .warning(&format!("No shell command with id {}", id));
        return;
    }

    // Parse optional byte range (e.g. "0..4096")
    let byte_range: Option<std::ops::Range<usize>> = range.as_deref().and_then(|r| {
        if let Some((start_s, end_s)) = r.split_once("..") {
            let start = start_s.trim().parse::<usize>().ok()?;
            let end = end_s.trim().parse::<usize>().ok()?;
            Some(start..end)
        } else {
            None
        }
    });

    // Try expansion from the durable command-run store
    match app
        .command_run_store
        .expand_stream(cmd_id, &stream, byte_range)
    {
        Some(expansion) => {
            let mut lines: Vec<String> = Vec::new();
            lines.push("── Shell Output Expansion ──".to_string());
            lines.push(format!("Command:  {}", id));
            lines.push(format!("Stream:   {}", expansion.stream.as_str()));
            lines.push(format!(
                "Range:    {}",
                match &expansion.byte_range {
                    Some(r) => format!("{}..{}", r.start, r.end),
                    None => "full".to_string(),
                }
            ));
            lines.push(format!("Exact:    {}", expansion.exactness.label()));
            lines.push(format!(
                "Returned: {} of {} bytes",
                expansion.returned_bytes, expansion.total_stream_bytes
            ));
            if !expansion.warnings.is_empty() {
                lines.push(String::new());
                lines.push("Warnings:".to_string());
                for w in &expansion.warnings {
                    lines.push(format!("  - {}", w));
                }
            }
            lines.push(String::new());
            lines.push("── expanded output ──".to_string());
            lines.push(String::new());
            // Show output, truncating if very large
            let display = if expansion.text.len() > 8192 {
                let truncated_len = 8192;
                let mut truncated = truncate_prefix(&expansion.text, truncated_len).to_string();
                truncated.push_str(&format!(
                    "\n\n... (truncated, {} of {} bytes shown)",
                    truncated.len(),
                    expansion.text.len()
                ));
                truncated
            } else {
                expansion.text.clone()
            };
            for line in display.lines() {
                lines.push(format!("  {}", line));
            }

            show_shell_detail(app, id, lines, "j/k scroll  |  / search  |  Esc close");
        }
        None => {
            // Command exists in legacy store but not in durable store
            // This can happen if the bridge hasn't finalized yet
            if app.command_run_store.get_run(cmd_id).is_none() {
                app.messages_state.toasts.warning(&format!(
                    "Command {} has no durable output yet (still running or not captured)",
                    id
                ));
            } else {
                // Run exists but stream name is invalid
                app.messages_state.toasts.warning(&format!(
                    "Invalid stream '{}' for command {}. Use stdout, stderr, or combined.",
                    stream, id
                ));
            }
        }
    }
}

#[cfg(test)]
mod promotion_tests {
    use super::project_for_model;

    #[test]
    fn human_promotion_redacts_credentials_before_context_insertion() {
        let projected = project_for_model(
            "command output: Authorization: Bearer abcdefghijklmnopqrstuvwxyz0123456789",
        )
        .unwrap();
        assert!(projected.contains("[REDACTED:bearer-token]"));
        assert!(!projected.contains("abcdefghijklmnopqrstuvwxyz0123456789"));
    }

    #[test]
    fn human_promotion_is_bounded() {
        let projected = project_for_model(&"x".repeat(64 * 1024)).unwrap();
        assert!(projected.len() <= 32 * 1024);
        assert!(projected.ends_with("[human-shell promotion truncated]"));
    }

    #[test]
    fn human_promotion_redacts_secret_straddling_model_limit() {
        let secret = "abcdefghijklmnopqrstuvwxyz0123456789";
        let text = format!(
            "{} Authorization: Bearer {} trailing",
            "x".repeat(32 * 1024 - 40),
            secret
        );
        let projected = project_for_model(&text).unwrap();
        assert!(!projected.contains(secret));
    }
}
