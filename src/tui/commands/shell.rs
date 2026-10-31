//! Shell command handlers for the TUI: execution, events, inclusion, rerun, kill, list, show.

use super::super::task_lifecycle::TuiTaskKind;
use crate::protocol::projection::dto::ShellRunProjection;
use crate::tui as app;
use crate::tui::app::send_tui;
use crate::util::truncate::truncate_prefix;
use std::collections::VecDeque;

pub(crate) fn handle_run_human_shell(
    app: &mut app::App,
    command: String,
    promote_after: bool,
    cwd: std::path::PathBuf,
    confirmed_dangerous: bool,
) {
    use crate::shell::policy::evaluate_command;
    let mut confirmed_dangerous = confirmed_dangerous;

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
                confirmed_dangerous = true;
            }
        }
        crate::shell::policy::HumanShellPolicyDecision::Allow => {}
    }

    spawn_human_shell(app, command, promote_after, cwd, confirmed_dangerous);
}

pub(crate) fn spawn_human_shell(
    app: &mut app::App,
    command: String,
    promote_after: bool,
    _cwd: std::path::PathBuf,
    confirmed_dangerous: bool,
) {
    let promote_after = promote_after && app.shell_auto_promote_bangbang;

    let id = app.shell_store.alloc_id();
    app.messages_state
        .messages
        .add_shell_cell(id.0, &command, "daemon workspace");
    app.messages_state.messages.update_shell_cell(id.0, |cell| {
        cell.status = Some("submitting".to_string());
    });

    let core_client = app.core_client.clone();
    let project_id = app.active_project_id().map(str::to_owned);
    let workspace_id = app.active_workspace_id().map(str::to_owned);
    let session_id = app.active_session_id().map(str::to_owned);
    let timeout_secs = app.shell_default_timeout_secs.clamp(
        1,
        crate::protocol::human_shell::HUMAN_SHELL_MAX_TIMEOUT_SECS,
    );
    let submission_id = uuid::Uuid::new_v4().to_string();
    let tui_cmd_tx = app.tui_cmd_tx.clone();
    app.task_registry
        .spawn(TuiTaskKind::Shell, "shell_event_forwarding", async move {
            macro_rules! emit {
                ($event:expr) => {
                    if let Some(ref sender) = tui_cmd_tx {
                        let _ = send_tui(sender, app::TuiCommand::ShellEvent($event));
                    }
                };
            }
            let (Some(client), Some(project_id), Some(workspace_id)) =
                (core_client, project_id, workspace_id)
            else {
                emit!(crate::shell::ShellEvent::FailedToStart {
                    id,
                    error: "human shell requires a selected daemon workspace and project".into(),
                });
                return;
            };
            let origin = if promote_after {
                crate::protocol::human_shell::HumanShellOriginDto::BangBang
            } else {
                crate::protocol::human_shell::HumanShellOriginDto::Bang
            };
            let capabilities = client.request(crate::core::new_request(
                format!("human-shell-capabilities-{submission_id}"),
                crate::protocol::core::CoreRequest::HumanShellRunCapabilities,
            )).await;
            if !matches!(capabilities,
                Ok(crate::protocol::core::CoreResponse::HumanShellRunCapabilities { capabilities })
                    if capabilities.supported && capabilities.protocol_version == crate::protocol::human_shell::HUMAN_SHELL_PROTOCOL_VERSION)
            {
                emit!(crate::shell::ShellEvent::FailedToStart {
                    id, error: "daemon does not support the human shell protocol".into(),
                });
                return;
            }
            let create = crate::core::new_request(
                format!("human-shell-{submission_id}"),
                crate::protocol::core::CoreRequest::HumanShellRunCreate {
                    request: crate::protocol::human_shell::HumanShellRunCreateRequestDto {
                        protocol_version: crate::protocol::human_shell::HUMAN_SHELL_PROTOCOL_VERSION,
                        submission_id: submission_id.clone(),
                        project_id: project_id.clone(),
                        workspace_id: workspace_id.clone(),
                        session_id: session_id.clone(),
                        origin,
                        confirmed_dangerous,
                        command: command.clone(),
                        timeout_secs,
                    },
                },
            );
            let mut response = client.request(create).await;
            if response.is_err() {
                let retry = crate::core::new_request(
                    format!("human-shell-{submission_id}"),
                    crate::protocol::core::CoreRequest::HumanShellRunCreate {
                        request: crate::protocol::human_shell::HumanShellRunCreateRequestDto {
                            protocol_version: crate::protocol::human_shell::HUMAN_SHELL_PROTOCOL_VERSION,
                            submission_id: submission_id.clone(), project_id: project_id.clone(),
                            workspace_id: workspace_id.clone(), session_id: session_id.clone(),
                            origin, confirmed_dangerous, command: command.clone(), timeout_secs,
                        },
                    },
                );
                response = client.request(retry).await;
            }
            let run = match response {
                Ok(crate::protocol::core::CoreResponse::HumanShellRunCreated { run, .. }) => run,
                Ok(crate::protocol::core::CoreResponse::Error { code, message }) => {
                    emit!(crate::shell::ShellEvent::FailedToStart { id, error: format!("{code}: {message}") });
                    return;
                }
                Ok(other) => {
                    emit!(crate::shell::ShellEvent::FailedToStart { id, error: format!("unexpected human shell response: {other:?}") });
                    return;
                }
                Err(error) => {
                    emit!(crate::shell::ShellEvent::FailedToStart { id, error: format!("human shell request failed: {error}") });
                    return;
                }
            };
            let binding = crate::shell::daemon::RemoteShellRunBinding {
                run_id: run.run_id.clone(), project_id: run.project_id.clone(),
                workspace_id: run.workspace_id.clone(), session_id: run.session_id.clone(),
            };
            if let Some(ref sender) = tui_cmd_tx {
                let _ = send_tui(sender, app::TuiCommand::RemoteShellRunAccepted {
                    local_id: id.0, command: command.clone(), cwd: std::path::PathBuf::from(&run.cwd),
                    promote_after,
                    shell_binding: (session_id.clone(), Some(workspace_id.clone())),
                    binding,
                });
            }
            let run_started_at = tokio::time::Instant::now();
            let run_deadline = run_started_at + std::time::Duration::from_secs(timeout_secs);
            emit!(crate::shell::ShellEvent::Started {
                id, command: command.clone(), cwd: std::path::PathBuf::from(&run.cwd),
            });
            let mut cursor = 0u64;
            loop {
                let req = crate::core::new_request(
                    format!("human-shell-read-{}-{}", run.run_id, cursor),
                    crate::protocol::core::CoreRequest::HumanShellRunRead {
                        request: crate::protocol::human_shell::HumanShellRunReadDto {
                            run_id: run.run_id.clone(), project_id: project_id.clone(),
                            workspace_id: workspace_id.clone(), session_id: session_id.clone(),
                            from_sequence: cursor, max_bytes: crate::protocol::human_shell::HUMAN_SHELL_MAX_READ_BYTES,
                        },
                    },
                );
                let read_response = tokio::time::timeout_at(run_deadline, client.request(req)).await;
                let read = match read_response {
                    Err(_) => {
                        emit!(crate::shell::ShellEvent::TimedOut {
                            id,
                            elapsed: std::time::Duration::from_secs(timeout_secs),
                        });
                        break;
                    }
                    Ok(Ok(crate::protocol::core::CoreResponse::HumanShellRunOutput { run, chunks, next_sequence, .. })) => {
                        for chunk in chunks {
                            let event = if chunk.stderr {
                                crate::shell::ShellEvent::Stderr { id, bytes: chunk.bytes }
                            } else { crate::shell::ShellEvent::Stdout { id, bytes: chunk.bytes } };
                            emit!(event);
                        }
                        cursor = next_sequence;
                        Some(run)
                    }
                    Ok(Ok(crate::protocol::core::CoreResponse::Error { code, message })) => {
                        emit!(crate::shell::ShellEvent::FailedToStart {
                            id,
                            error: format!("human shell read failed ({code}): {message}"),
                        });
                        break;
                    }
                    Ok(Ok(other)) => {
                        emit!(crate::shell::ShellEvent::FailedToStart {
                            id,
                            error: format!("unexpected human shell read response: {other:?}"),
                        });
                        break;
                    }
                    Ok(Err(_)) => {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                        continue;
                    }
                };
                let Some(run) = read else { break; };
                let elapsed = run_started_at.elapsed();
                let terminal = match run.status {
                    crate::protocol::human_shell::HumanShellRunStatusDto::Running => false,
                    crate::protocol::human_shell::HumanShellRunStatusDto::Exited => {
                        emit!(crate::shell::ShellEvent::Exited { id, status: run.exit_code, elapsed }); true
                    }
                    crate::protocol::human_shell::HumanShellRunStatusDto::TimedOut => {
                        emit!(crate::shell::ShellEvent::TimedOut { id, elapsed }); true
                    }
                    crate::protocol::human_shell::HumanShellRunStatusDto::Cancelled => {
                        emit!(crate::shell::ShellEvent::Exited { id, status: None, elapsed }); true
                    }
                    crate::protocol::human_shell::HumanShellRunStatusDto::Failed
                    | crate::protocol::human_shell::HumanShellRunStatusDto::Lost => {
                        emit!(crate::shell::ShellEvent::FailedToStart { id, error: format!("human shell ended with status {:?}", run.status) }); true
                    }
                };
                if terminal { break; }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
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

#[cfg(test)]
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

/// Drop the bindings whose local entry is gone. A binding outlives its
/// run: it is the only join to the daemon's run id.
fn prune_remote_shell_bindings(app: &mut app::App) {
    let stale: Vec<u64> = app
        .remote_shell_runs
        .keys()
        .copied()
        .filter(|local_id| {
            app.shell_store
                .get(crate::shell::types::ShellCommandId(*local_id))
                .is_none()
        })
        .collect();
    for local_id in stale {
        app.remote_shell_runs.remove(&local_id);
    }
}

pub(crate) fn handle_shell_event(app: &mut app::App, event: crate::shell::ShellEvent) {
    // Mirror every event into the durable command-run store, so the bridge has
    // the bytes it needs when it finalizes the run on a terminal event.
    app.command_run_bridge
        .observe(&mut app.command_run_store, &event);

    match &event {
        crate::shell::ShellEvent::Started { id, cwd, .. } => {
            app.shell_store.update_cwd(*id, cwd.clone());
            app.messages_state.messages.update_shell_cell(id.0, |cell| {
                cell.status = Some("running".to_string());
                cell.cwd = Some(cwd.to_string_lossy().to_string());
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
            // Never overwrite Killed from a late exited event
            if let Some(entry) = app.shell_store.get(*id) {
                if entry.status == crate::shell::types::ShellStatus::Killed {
                    return;
                }
            }
            app.shell_handles.remove(&id.0);
            prune_remote_shell_bindings(app);
            let cancelled = status.is_none();
            if cancelled {
                app.shell_store.mark_killed(*id, *elapsed);
            } else {
                app.shell_store.mark_exited(*id, *status, *elapsed);
            }
            let elapsed_ms = elapsed.as_millis() as u64;
            let exit_code = *status;
            let status_str = if cancelled { "killed" } else { "exited" }.to_string();
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

            // Projection metadata from the command-run store
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
                            if !stage_remote_shell_projection(app, include_text.clone())
                                && !stage_local_shell_projection(app, id.0, include_text.clone())
                            {
                                app.prompt_state.prompt.set_text(include_text);
                                app.messages_state.toasts.warning(
                                    "Redacted shell evidence is an unsubmitted composer draft because no session owner is connected",
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
            prune_remote_shell_bindings(app);
            app.shell_store.mark_timeout(*id, *elapsed);
            app.messages_state.messages.update_shell_cell(id.0, |cell| {
                cell.status = Some("timed_out".to_string());
                cell.elapsed_ms = Some(elapsed.as_millis() as u64);
            });
        }
        crate::shell::ShellEvent::FailedToStart { id, error } => {
            app.shell_handles.remove(&id.0);
            prune_remote_shell_bindings(app);
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
                if !stage_local_shell_projection(app, id, include_text.clone()) {
                    app.prompt_state.prompt.set_text(include_text);
                    app.messages_state.toasts.warning(
                        "Redacted shell evidence is an unsubmitted composer draft because no session owner is connected",
                    );
                }
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
    if app.observer.blocks_prompt_submit() {
        app.messages_state
            .toasts
            .warning("Observer sessions cannot submit shell output to a model");
        return;
    }
    // M007-I (work package E): the evidence is the **daemon's** own run
    // bytes. The frontend names the run and the question; it never reads
    // the run's bytes and never assembles the evidence. The daemon reads
    // its own bytes and redacts the assembled evidence.
    if app.core_client.is_some() {
        let Some(binding) = app.remote_shell_runs.get(&id).cloned() else {
            app.messages_state
                .toasts
                .error(&format!("Shell command {} not found", id));
            return;
        };
        if app.session_state.session.is_some() && !shell_binding_matches_current(app, id) {
            app.messages_state
                .toasts
                .warning("Shell output cannot be submitted from a different session or workspace");
            return;
        }
        submit_shell_ask(app, id, binding.run_id, question);
        return;
    }
    // The standalone path: with no daemon attached, the TUI owns the run
    // bytes itself and keeps its own redaction.
    use crate::shell::types::ShellCommandId;
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
                submit_shell_ask_standalone(app, id, include_text);
            }
            Err(error) => app.messages_state.toasts.error(&error),
        }
    } else {
        app.messages_state
            .toasts
            .error(&format!("Shell command {} not found", id));
    }
}

/// Submit the ask, as one request.
///
/// M007-I (work packages D and E): the frontend sends the run id and the
/// question. The daemon reads its own run bytes, redacts the assembled
/// evidence, stages it and admits the turn **together**. There is no second
/// request, and therefore no `pending` row that survives a failed one.
fn submit_shell_ask(app: &mut app::App, id: u64, run_id: String, question: String) {
    use crate::protocol::core::{CoreRequest, CoreResponse};

    let Some(client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Cannot submit shell question: no daemon is connected");
        return;
    };
    let Some(session_id) = app.session_state.session.as_ref().map(|s| s.id.clone()) else {
        app.messages_state
            .toasts
            .warning("Select a session before asking about shell output");
        return;
    };
    let Some(tx) = app.tui_cmd_tx.clone() else {
        app.messages_state
            .toasts
            .error("Cannot submit shell question: TUI command channel is unavailable");
        return;
    };
    let promotion_id = uuid::Uuid::new_v4();
    let request = crate::core::new_request(
        uuid::Uuid::new_v4().to_string(),
        CoreRequest::SessionPromptSubmitWithHumanShellPromotions {
            session_id: session_id.clone(),
            text: "Answer the question contained in the attached human-shell evidence.".into(),
            plan_mode: app.agent_state.plan_mode,
            human_shell_promotions: vec![
                crate::protocol::human_shell::HumanShellPromotionStageDto {
                    promotion_id: promotion_id.to_string(),
                    source_id: run_id,
                    question,
                },
            ],
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
                    promotion_id: Some(promotion_id),
                    text: String::new(),
                    error,
                },
            );
        });
}

/// The standalone path's submit: with no daemon attached, the TUI sends the
/// evidence it assembled itself.
fn submit_shell_ask_standalone(app: &mut app::App, id: u64, text: String) {
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
        let promotion_id = uuid::Uuid::new_v4();
        let workspace_id = app.active_workspace_id().map(str::to_owned).or_else(|| {
            app.session_state
                .session
                .as_ref()
                .and_then(|session| session.workspace_id.clone())
        });
        let stage_request = crate::core::new_request(
            uuid::Uuid::new_v4().to_string(),
            CoreRequest::HumanShellPromotionStage {
                session_id: session_id.clone(),
                promotion_id: promotion_id.to_string(),
                source_id: id.to_string(),
                projection: text.clone(),
            },
        );
        let request = crate::core::new_request(
            uuid::Uuid::new_v4().to_string(),
            CoreRequest::SessionPromptSubmit {
                session_id: session_id.clone(),
                text: "Answer the question contained in the attached human-shell evidence.".into(),
                plan_mode: app.agent_state.plan_mode,
                human_shell_promotion_ids: vec![promotion_id.to_string()],
            },
        );
        app.task_registry
            .spawn(TuiTaskKind::Command, "shell_ask_submit", async move {
                let stage_response = match client.request(stage_request).await {
                    Ok(response) => Ok(response),
                    Err(first_error) => {
                        let retry = crate::core::new_request(
                            uuid::Uuid::new_v4().to_string(),
                            CoreRequest::HumanShellPromotionStage {
                                session_id: session_id.clone(),
                                promotion_id: promotion_id.to_string(),
                                source_id: id.to_string(),
                                projection: submitted_text.clone(),
                            },
                        );
                        client.request(retry).await.map_err(|retry_error| {
                            format!(
                                "could not confirm durable staging (the connected daemon may not support it): {first_error}; retry: {retry_error}"
                            )
                        })
                    }
                };
                let stage_error = match stage_response {
                    Ok(CoreResponse::HumanShellPromotionStaged {
                        promotion,
                        duplicate,
                    }) if promotion.status == "pending" => {
                        let _ = send_tui(
                            &tx,
                            app::TuiCommand::ShellPromotionStaged {
                                result: app::ShellPromotionStageResult {
                                    source_id: id,
                                    promotion_id,
                                    session_id: session_id.clone(),
                                    workspace_id: workspace_id.clone(),
                                    projection: promotion.projection,
                                    duplicate,
                                    status: Some("pending".into()),
                                    error: None,
                                },
                            },
                        );
                        None
                    }
                    Ok(CoreResponse::HumanShellPromotionStaged { promotion, .. }) => {
                        Some(format!("promotion is already {}", promotion.status))
                    }
                    Ok(CoreResponse::Error { code, message }) => {
                        Some(human_shell_stage_error(&code, &message))
                    }
                    Ok(_) => Some("unexpected response while staging shell question".to_string()),
                    Err(error) => Some(error),
                };
                let error = if let Some(error) = stage_error {
                    Some(error)
                } else {
                    match client.request(request).await {
                        Ok(CoreResponse::Ack) => None,
                        Ok(CoreResponse::Error { code, message }) => {
                            Some(format!("{code}: {message}"))
                        }
                        Ok(_) => Some("unexpected response to shell question".to_string()),
                        Err(error) => Some(error.to_string()),
                    }
                };
                let _ = send_tui(
                    &tx,
                    app::TuiCommand::ShellAskSubmitted {
                        id,
                        promotion_id: Some(promotion_id),
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
    promotion_id: Option<uuid::Uuid>,
    text: String,
    error: Option<String>,
) {
    if let Some(error) = error {
        if promotion_id.is_none() && app.prompt_state.prompt.get_text().is_empty() {
            app.prompt_state.prompt.set_text(text);
        }
        app.messages_state
            .toasts
            .error(&format!("Shell question was not accepted: {error}"));
        return;
    }
    app.shell_store
        .mark_promoted(crate::shell::types::ShellCommandId(id));
    if let Some(promotion_id) = promotion_id {
        app.pending_shell_promotions
            .retain(|pending| pending.id != promotion_id);
    }
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
    // budget, so a secret straddling the output boundary cannot escape.
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

/// Send a prepared projection to the daemon session owner and wait for
/// the typed response before showing it as staged.
fn stage_local_shell_projection(app: &mut app::App, source_id: u64, projection: String) -> bool {
    let Some(client) = app.core_client.clone() else {
        return false;
    };
    let Some(session_id) = app.active_session_id().map(str::to_owned).or_else(|| {
        app.session_state
            .session
            .as_ref()
            .map(|session| session.id.clone())
    }) else {
        return false;
    };
    if app.observer.blocks_prompt_submit() {
        app.messages_state
            .toasts
            .warning("Observer sessions cannot stage shell output for model context");
        return true;
    }
    let Some(tx) = app.tui_cmd_tx.clone() else {
        app.messages_state
            .toasts
            .error("Cannot stage shell output: TUI command channel is unavailable");
        return true;
    };
    let workspace_id = app.active_workspace_id().map(str::to_owned).or_else(|| {
        app.session_state
            .session
            .as_ref()
            .and_then(|session| session.workspace_id.clone())
    });
    let promotion_id = uuid::Uuid::new_v4();
    let request = crate::core::new_request(
        uuid::Uuid::new_v4().to_string(),
        crate::protocol::core::CoreRequest::HumanShellPromotionStage {
            session_id: session_id.clone(),
            promotion_id: promotion_id.to_string(),
            source_id: source_id.to_string(),
            projection: projection.clone(),
        },
    );
    app.task_registry
        .spawn(TuiTaskKind::Command, "stage_shell_promotion", async move {
            let response = match client.request(request).await {
                Ok(response) => response,
                Err(first_error) => {
                    // A lost response is ambiguous: retry the same durable identity.
                    let retry = crate::core::new_request(
                        uuid::Uuid::new_v4().to_string(),
                        crate::protocol::core::CoreRequest::HumanShellPromotionStage {
                            session_id: session_id.clone(),
                            promotion_id: promotion_id.to_string(),
                            source_id: source_id.to_string(),
                            projection: projection.clone(),
                        },
                    );
                    match client.request(retry).await {
                        Ok(response) => response,
                        Err(retry_error) => {
                            let _ = send_tui(
                                &tx,
                                app::TuiCommand::ShellPromotionStaged {
                                    result: app::ShellPromotionStageResult {
                                        source_id,
                                        promotion_id,
                                        session_id,
                                        workspace_id,
                                        projection,
                                        duplicate: false,
                                        status: None,
                                        error: Some(format!(
                                            "could not confirm durable staging (the connected daemon may not support it): {first_error}; retry: {retry_error}"
                                        )),
                                    },
                                },
                            );
                            return;
                        }
                    }
                }
            };
            let command = match response {
                crate::protocol::core::CoreResponse::HumanShellPromotionStaged {
                    promotion,
                    duplicate,
                } => app::TuiCommand::ShellPromotionStaged {
                    result: app::ShellPromotionStageResult {
                        source_id,
                        promotion_id,
                        session_id,
                        workspace_id,
                        projection: promotion.projection,
                        duplicate,
                        status: Some(promotion.status),
                        error: None,
                    },
                },
                crate::protocol::core::CoreResponse::Error { code, message } => {
                    app::TuiCommand::ShellPromotionStaged {
                        result: app::ShellPromotionStageResult {
                            source_id,
                            promotion_id,
                            session_id,
                            workspace_id,
                            projection,
                            duplicate: false,
                            status: None,
                            error: Some(human_shell_stage_error(&code, &message)),
                        },
                    }
                }
                _ => app::TuiCommand::ShellPromotionStaged {
                    result: app::ShellPromotionStageResult {
                        source_id,
                        promotion_id,
                        session_id,
                        workspace_id,
                        projection,
                        duplicate: false,
                        status: None,
                        error: Some("unexpected response to shell promotion staging".into()),
                    },
                },
            };
            let _ = send_tui(&tx, command);
        });
    true
}

fn human_shell_stage_error(code: &str, message: &str) -> String {
    if matches!(
        code,
        "invalid_request" | "unknown_request" | "unsupported_capability"
    ) {
        format!("connected daemon does not support durable human-shell staging ({code})")
    } else {
        format!("{code}: {message}")
    }
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
                    confirmed_dangerous: false,
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
    let still_running = app
        .shell_store
        .get(crate::shell::types::ShellCommandId(id))
        .is_some_and(|entry| entry.status == crate::shell::types::ShellStatus::Running);
    let binding = app.remote_shell_runs.get(&id).cloned();
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
    } else if let Some(binding) = binding.filter(|_| still_running) {
        let Some(client) = app.core_client.clone() else {
            app.messages_state
                .toasts
                .error("Daemon connection is unavailable; shell cancellation was not sent");
            return;
        };
        let Some(tui_cmd_tx) = app.tui_cmd_tx.clone() else {
            return;
        };
        app.task_registry
            .spawn(TuiTaskKind::Shell, "cancel_remote_shell_run", async move {
                let request = crate::core::new_request(
                    format!("human-shell-cancel-{}", binding.run_id),
                    crate::protocol::core::CoreRequest::HumanShellRunCancel {
                        request: crate::protocol::human_shell::HumanShellRunCancelDto {
                            run_id: binding.run_id,
                            project_id: binding.project_id,
                            workspace_id: binding.workspace_id,
                            session_id: binding.session_id,
                        },
                    },
                );
                let message = match client.request(request).await {
                    Ok(crate::protocol::core::CoreResponse::HumanShellRunCancelled { .. }) => {
                        "Cancellation requested".to_owned()
                    }
                    Ok(crate::protocol::core::CoreResponse::Error { code, message }) => {
                        format!("{code}: {message}")
                    }
                    Ok(other) => format!("Unexpected cancellation response: {other:?}"),
                    Err(error) => format!("Cancellation request failed: {error}"),
                };
                let _ = send_tui(
                    &tui_cmd_tx,
                    app::TuiCommand::RemoteShellRunCancelResult { message },
                );
            });
    } else {
        app.messages_state
            .toasts
            .error(&format!("No running shell command with id {}", id));
    }
}

pub(crate) fn handle_shell_list(app: &mut app::App) {
    let promoted = promoted_run_ids(app);
    let rendered = app
        .projection_client
        .active_snapshot()
        .map(|(_, snapshot)| shell_run_rows(&snapshot.shell_runs, &promoted));
    let Some(lines) = rendered else {
        app.messages_state
            .toasts
            .warning("The daemon's shell command list is unavailable");
        return;
    };
    if lines.is_empty() {
        app.messages_state
            .toasts
            .info("No shell commands in history");
        return;
    }
    if lines.len() > 5 {
        app.open_info_dialog(
            crate::tui::components::dialogs::info::InfoType::ShellShow,
            lines,
        );
    } else {
        app.messages_state.toasts.info(&lines.join("\n"));
    }
}

/// The daemon's run ids this frontend promoted: its own annotation,
/// matched to the published rows by run id.
fn promoted_run_ids(app: &app::App) -> std::collections::HashSet<String> {
    app.remote_shell_runs
        .iter()
        .filter(|(local_id, _)| {
            app.shell_store
                .get(crate::shell::types::ShellCommandId(**local_id))
                .is_some_and(|entry| entry.promoted)
        })
        .map(|(_, binding)| binding.run_id.clone())
        .collect()
}

/// Render the daemon's published runs as this list always rendered them:
/// the most recent first, at most ten rows, the status spelled as this list
/// spells it (`done`, `timeout`, `killed`), plus this frontend's marker.
fn shell_run_rows(
    runs: &VecDeque<ShellRunProjection>,
    promoted: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut ordered: Vec<&ShellRunProjection> = runs.iter().collect();
    ordered.sort_by_key(|run| std::cmp::Reverse(run.started_at_ms.unwrap_or(i64::MIN)));
    ordered
        .into_iter()
        .take(10)
        .map(|run| shell_run_row(run, promoted))
        .collect()
}

fn shell_run_row(run: &ShellRunProjection, promoted: &std::collections::HashSet<String>) -> String {
    let elapsed = run
        .started_at_ms
        .zip(run.completed_at_ms)
        .map(|(started, completed)| (completed.saturating_sub(started) as f64) / 1000.0);
    let status = match run.status.as_str() {
        "running" => format!("running {}", tenths(elapsed)),
        "exited" => match run.exit_code {
            Some(code) => match elapsed {
                Some(seconds) => format!("done exit={} {:.1}s", code, seconds),
                None => format!("done exit={}", code),
            },
            None => "done".to_string(),
        },
        "timed_out" => match elapsed {
            Some(seconds) => format!("timeout {:.0}s", seconds),
            None => "timeout".to_string(),
        },
        "cancelled" => match elapsed {
            Some(seconds) => format!("killed {:.1}s", seconds),
            None => "killed".to_string(),
        },
        "failed" => "failed".to_string(),
        "lost" => "lost".to_string(),
        other => other.to_string(),
    };
    let promoted_str = if promoted.contains(&run.run_id) {
        " [promoted]"
    } else {
        ""
    };
    format!(
        "[{}] {}{} $ {}",
        run.run_id, status, promoted_str, run.command
    )
}

fn tenths(elapsed: Option<f64>) -> String {
    elapsed
        .map(|seconds| format!("{:.1}s", seconds))
        .unwrap_or_else(|| "0.0s".to_string())
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

    // Projection metadata from the durable run store
    let cmd_run_id = crate::shell::projection::CommandRunId(id);
    if let Some(run) = app.command_run_store.get_run(cmd_run_id) {
        lines.push(String::new());
        lines.push("── projection ──".to_string());

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

        if !result.expansion_handles.is_empty() {
            lines.push(format!("Handles: {}", result.expansion_handles.len()));
            for handle in &result.expansion_handles {
                lines.push(format!("  {}", handle.as_url()));
            }
        }

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

    // The command is in the legacy store: a friendly message
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

    // Try expansion from the durable run store
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
            // In the legacy store, not the durable one: the bridge has not finalized it
            if app.command_run_store.get_run(cmd_id).is_none() {
                app.messages_state.toasts.warning(&format!(
                    "Command {} has no durable output yet (still running or not captured)",
                    id
                ));
            } else {
                // Run exists, stream name invalid
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

#[cfg(test)]
mod shell_list_projection_tests {
    use super::handle_shell_list;
    use crate::protocol::projection::dto::ShellRunProjection;
    use crate::protocol::projection::replay::{
        ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind,
        ProjectionSubscriptionId,
    };
    use crate::protocol::projection::snapshot::SessionProjectionSnapshot;
    use crate::tui::app::App;

    fn published_run(run_id: &str, command: &str, started_at_ms: i64) -> ShellRunProjection {
        ShellRunProjection {
            run_id: run_id.to_string(),
            command: command.to_string(),
            cwd: "/tmp".to_string(),
            origin: "bang".to_string(),
            status: "exited".to_string(),
            started_at_ms: Some(started_at_ms),
            completed_at_ms: Some(started_at_ms + 1_200),
            exit_code: Some(0),
        }
    }

    fn app_with_published_runs(runs: Vec<ShellRunProjection>) -> App {
        let mut app = App::new_for_testing("/tmp".into());
        let mut snapshot = SessionProjectionSnapshot::empty("sess", "proj", "ws");
        for run in runs {
            snapshot.upsert_shell_run(run);
        }
        app.projection_client.controller_mut().install_subscription(
            ProjectionSubscriptionId::new("sub-1"),
            ProjectionStreamDescriptor {
                stream_id: ProjectionStreamId::new("stream-1").expect("a valid stream id"),
                kind: ProjectionStreamKind::Session,
                project_id: "proj".into(),
                workspace_id: Some("ws".into()),
                session_id: Some("sess".into()),
                projection_version: 1,
                retention_floor_seq: 0,
                high_water_seq: 0,
                latest_checkpoint_seq: None,
            },
            snapshot,
        );
        app
    }

    fn toast_text(app: &App) -> String {
        app.messages_state
            .toasts
            .iter()
            .map(|toast| toast.message.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn shell_list_renders_the_published_runs_in_the_daemons_order() {
        let mut app = app_with_published_runs(vec![
            published_run("run-new", "echo newer", 2_000),
            published_run("run-old", "echo older", 1_000),
        ]);

        handle_shell_list(&mut app);

        let text = toast_text(&app);
        let newer = text.find("echo newer").expect("the newer run is listed");
        let older = text.find("echo older").expect("the older run is listed");
        assert!(newer < older, "the daemon's order is kept: {text}");
        let row = text
            .lines()
            .find(|line| line.contains("echo newer"))
            .expect("a row for the newer run");
        assert!(row.contains("[run-new]"), "the row names the run: {row}");
        assert!(row.contains("done exit=0 1.2s"), "the row spells it: {row}");
    }

    #[test]
    fn shell_list_says_so_when_the_daemon_published_nothing() {
        let mut app = app_with_published_runs(vec![]);

        handle_shell_list(&mut app);

        let text = toast_text(&app);
        assert!(
            text.contains("No shell commands in history"),
            "an empty published list reads as empty: {text}"
        );
    }

    #[test]
    fn shell_list_says_so_when_no_daemon_is_attached() {
        let mut app = App::new_for_testing("/tmp".into());

        handle_shell_list(&mut app);

        let text = toast_text(&app);
        assert!(
            text.contains("daemon's shell command list is unavailable"),
            "an unattached projection is named, not faked: {text}"
        );
    }

    fn local_entry(app: &mut App, local_id: u64, command: &str) {
        let request = crate::shell::types::ShellRequest {
            id: crate::shell::types::ShellCommandId(local_id),
            origin: crate::shell::types::ShellOrigin::HumanPromoted,
            command: command.to_string(),
            cwd: std::env::temp_dir(),
            timeout: std::time::Duration::from_secs(300),
            capture_policy: crate::shell::types::ShellCapturePolicy::StoreAndPromote,
            env_policy: crate::shell::types::ShellEnvPolicy::Inherit,
        };
        app.shell_store.insert_started(&request);
    }

    fn bind_run(app: &mut App, local_id: u64, run_id: &str) {
        app.remote_shell_runs.insert(
            local_id,
            crate::shell::daemon::RemoteShellRunBinding {
                run_id: run_id.to_string(),
                project_id: "proj".to_string(),
                workspace_id: "ws".to_string(),
                session_id: Some("sess".to_string()),
            },
        );
    }

    #[test]
    fn shell_list_marks_the_runs_this_frontend_promoted() {
        let mut app = app_with_published_runs(vec![
            published_run("run-promoted", "echo promoted", 2_000),
            published_run("run-plain", "echo plain", 1_000),
        ]);
        local_entry(&mut app, 1, "echo promoted");
        app.shell_store
            .mark_promoted(crate::shell::types::ShellCommandId(1));
        bind_run(&mut app, 1, "run-promoted");
        local_entry(&mut app, 2, "echo plain");
        bind_run(&mut app, 2, "run-plain");

        handle_shell_list(&mut app);

        let text = toast_text(&app);
        let promoted = text
            .lines()
            .find(|line| line.contains("echo promoted"))
            .expect("a row for the promoted run");
        assert!(
            promoted.contains("[promoted]"),
            "the frontend's own marker is overlaid: {promoted}"
        );
        let plain = text
            .lines()
            .find(|line| line.contains("echo plain"))
            .expect("a row for the plain run");
        assert!(
            !plain.contains("[promoted]"),
            "an unpromoted run carries no marker: {plain}"
        );
    }

    #[test]
    fn shell_list_leaves_the_marker_off_when_the_run_cannot_be_matched() {
        let mut app = app_with_published_runs(vec![published_run("run-1", "echo one", 2_000)]);
        local_entry(&mut app, 1, "echo one");
        app.shell_store
            .mark_promoted(crate::shell::types::ShellCommandId(1));

        handle_shell_list(&mut app);

        let text = toast_text(&app);
        assert!(
            !text.contains("[promoted]"),
            "a run with no daemon binding is not claimed: {text}"
        );
    }
}
