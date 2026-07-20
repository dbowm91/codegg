//! Command dispatch: routes TuiCommand variants to handler functions.

use super::super::app::state::session::GitSidebarInfo;
use super::super::app::{App, TuiCommand};

#[allow(unused_imports)]
use super::super::commands::agents::{apply_asset_refresh_finished, start_refresh_assets};
#[allow(unused_imports)]
use super::super::commands::diagnostics::{apply_doctor_result, start_run_doctor};
#[allow(unused_imports)]
use super::super::commands::goals::{
    apply_goal_operation_finished, apply_session_state_refreshed, handle_goal_budget,
    handle_goal_from_file, handle_goal_set, handle_goal_simple, start_goal_checkpoint,
    start_goal_show, start_refresh_session_state,
};
#[allow(unused_imports)]
use super::super::commands::memory::{
    apply_memory_result, start_memory_forget, start_memory_remember, start_memory_search,
    start_memory_summary,
};
#[allow(unused_imports)]
use super::super::commands::plugins::{
    apply_plugin_command_finished, apply_plugin_ui_effect, start_plugin_command,
};
#[allow(unused_imports)]
use super::super::commands::project_catalog::{
    apply_project_catalog_refreshed, start_refresh_project_catalog,
};
#[allow(unused_imports)]
use super::super::commands::provider_connections::start_connection_lifecycle;
#[allow(unused_imports)]
use super::super::commands::research::{
    apply_research_run_loaded, apply_research_runs_loaded, apply_research_section_loaded,
    start_research_list_runs, start_research_load_run, start_research_load_section,
};
#[allow(unused_imports)]
use super::super::commands::security::{
    handle_security_review_finished, handle_security_review_run,
};
#[allow(unused_imports)]
use super::super::commands::sessions::{
    apply_export_session_finished, apply_session_messages_loaded, apply_session_mutation_finished,
    apply_sessions_reloaded, apply_share_session_finished, apply_template_session_created,
    apply_tree_dialog_loaded, apply_unshare_session_finished, start_archive_session,
    start_bulk_archive, start_bulk_delete, start_bulk_export, start_create_from_template,
    start_delete_session, start_export_session, start_fork_session, start_load_session_messages,
    start_open_tree_dialog, start_reload_sessions, start_rename_session, start_share_session,
    start_undo_delete, start_unshare_session,
};
#[allow(unused_imports)]
use super::super::commands::shell::{
    handle_run_human_shell, handle_shell_ask, handle_shell_event, handle_shell_expand,
    handle_shell_include, handle_shell_kill, handle_shell_list, handle_shell_rerun,
    handle_shell_show,
};
#[allow(unused_imports)]
use super::super::commands::tasks::{
    apply_notification_sent, apply_subagent_spawn_finished, apply_task_operation_finished,
    apply_tasks_listed, apply_worktree_listed, handle_compact_session,
    handle_file_diff_stats_ready, handle_open_diff_dialog, handle_spawn_subagent,
    start_delete_task, start_list_tasks, start_send_notification, start_task_schedule,
    start_worktree_list,
};
#[allow(unused_imports)]
use super::super::commands::test::{apply_test_run_finished, start_test_run};

use crate::protocol::core::CoreRequest;
use crate::tui::components::toast::Toast;

pub(crate) async fn dispatch_tui_command(app: &mut App, cmd: TuiCommand) {
    match cmd {
        TuiCommand::RefreshAssets => {
            start_refresh_assets(app);
        }
        TuiCommand::AssetRefreshFinished { report, error } => {
            apply_asset_refresh_finished(app, report, error);
        }
        TuiCommand::DeleteSession { session_id } => {
            start_delete_session(app, session_id);
        }
        TuiCommand::ArchiveSession {
            session_id,
            unarchive,
        } => {
            start_archive_session(app, session_id, unarchive);
        }
        TuiCommand::ForkSession { session_id } => {
            start_fork_session(app, session_id);
        }
        TuiCommand::ShareSession { session_id } => {
            start_share_session(app, session_id);
        }
        TuiCommand::UnshareSession { session_id } => {
            start_unshare_session(app, session_id);
        }
        TuiCommand::ExportSession { session_id } => {
            start_export_session(app, session_id);
        }
        TuiCommand::RenameSession {
            session_id,
            new_title,
        } => {
            start_rename_session(app, session_id, new_title);
        }
        TuiCommand::BulkDelete { session_ids } => {
            start_bulk_delete(app, session_ids);
        }
        TuiCommand::BulkArchive {
            session_ids,
            unarchive,
        } => {
            start_bulk_archive(app, session_ids, unarchive);
        }
        TuiCommand::BulkExport { session_ids } => {
            start_bulk_export(app, session_ids);
        }
        TuiCommand::ReloadSessions => {
            start_reload_sessions(app);
        }
        TuiCommand::RefreshProjectCatalog => {
            start_refresh_project_catalog(app);
        }
        TuiCommand::ProjectCatalogRefreshed {
            request_id,
            supported,
            entries,
            truncated,
            error,
        } => {
            apply_project_catalog_refreshed(app, request_id, supported, entries, truncated, error);
        }
        TuiCommand::OpenTreeDialog => {
            start_open_tree_dialog(app);
        }
        TuiCommand::PreviewImport { source } => {
            super::super::commands::import::start_preview_import(app, source);
        }
        TuiCommand::ConfirmImport { source } => {
            super::super::commands::import::start_confirm_import(app, source);
        }
        TuiCommand::CreateFromTemplate { key, template } => {
            start_create_from_template(app, key, template);
        }
        TuiCommand::LoadSessionMessages { session_id } => {
            start_load_session_messages(app, session_id);
        }
        TuiCommand::RefreshSessionState { session_id } => {
            start_refresh_session_state(app, session_id);
        }
        TuiCommand::SpawnSubagent { agent_name, prompt } => {
            handle_spawn_subagent(app, agent_name, prompt);
        }
        TuiCommand::UndoDelete { session_id } => {
            start_undo_delete(app, session_id);
        }
        TuiCommand::ListTasks => {
            start_list_tasks(app);
        }
        TuiCommand::UpdateModels(models) => {
            app.set_models(models);
            app.messages_state
                .toasts
                .add(Toast::success("Models list updated"));
        }
        TuiCommand::DeleteTask { id } => {
            start_delete_task(app, id);
        }
        TuiCommand::TaskSchedule {
            interval_secs,
            message,
        } => {
            start_task_schedule(app, interval_secs, message);
        }
        TuiCommand::WorktreeList => {
            start_worktree_list(app);
        }
        TuiCommand::MemorySummary => {
            start_memory_summary(app);
        }
        TuiCommand::MemorySearch { query } => {
            start_memory_search(app, query);
        }
        TuiCommand::MemoryRemember { text } => {
            start_memory_remember(app, text);
        }
        TuiCommand::MemoryForget { id } => {
            start_memory_forget(app, id);
        }
        TuiCommand::CompactSession => {
            handle_compact_session(app);
        }
        TuiCommand::OpenDiffDialog {
            old_content,
            new_content,
            title,
        } => {
            handle_open_diff_dialog(app, old_content, new_content, title);
        }
        TuiCommand::SendNotification {
            notification_type,
            body,
        } => {
            start_send_notification(app, notification_type, body);
        }
        TuiCommand::GoalSet {
            session_id,
            project_id,
            objective,
        } => {
            handle_goal_set(app, session_id, project_id, objective);
        }
        TuiCommand::GoalFromFile {
            session_id,
            project_id,
            path,
        } => {
            handle_goal_from_file(app, session_id, project_id, path);
        }
        TuiCommand::GoalShow { session_id } => {
            start_goal_show(app, session_id);
        }
        TuiCommand::GoalPause { session_id } => {
            handle_goal_simple(app, CoreRequest::GoalPause { session_id }, "pause");
        }
        TuiCommand::GoalResume { session_id } => {
            handle_goal_simple(app, CoreRequest::GoalResume { session_id }, "resume");
        }
        TuiCommand::GoalClear { session_id } => {
            handle_goal_simple(app, CoreRequest::GoalClear { session_id }, "clear");
        }
        TuiCommand::GoalDone { session_id } => {
            handle_goal_simple(app, CoreRequest::GoalDone { session_id }, "done");
        }
        TuiCommand::GoalCheckpoint {
            session_id,
            project_id,
        } => {
            start_goal_checkpoint(app, session_id, project_id);
        }
        TuiCommand::GoalBudget {
            session_id,
            subcommand,
        } => {
            handle_goal_budget(app, session_id, subcommand);
        }
        TuiCommand::ResearchListRuns => {
            start_research_list_runs(app);
        }
        TuiCommand::ResearchLoadRun { run_id } => {
            start_research_load_run(app, run_id);
        }
        TuiCommand::ResearchLoadSection { run_id, section } => {
            start_research_load_section(app, run_id, section);
        }
        TuiCommand::OpenRunDetailLoaded { mut dialog } => {
            dialog.set_theme(&app.ui_state.theme);
            app.dialog_state.run_detail_dialog = Some(dialog);
            if let Some(ref mut dlg) = app.dialog_state.run_detail_dialog {
                dlg.set_theme(&app.ui_state.theme);
                app.focus_manager.push(Box::new(dlg.clone()));
            }
            app.ui_state.dialog = crate::tui::Dialog::RunDetail;
        }
        TuiCommand::OpenRunDetailError { error } => {
            app.messages_state.toasts.error(&error);
        }
        TuiCommand::RunDoctor => {
            start_run_doctor(app);
        }
        TuiCommand::SecurityReviewRun {
            id,
            root,
            args,
            lsp_tool,
        } => {
            handle_security_review_run(app, id, root, args, lsp_tool);
        }
        TuiCommand::SecurityReviewFinished { id, receipt, error } => {
            handle_security_review_finished(app, id, receipt, error);
        }
        TuiCommand::SubagentSpawnFinished {
            agent_name,
            task_id,
            prompt,
            error,
        } => {
            apply_subagent_spawn_finished(app, agent_name, task_id, prompt, error);
        }
        TuiCommand::GitSidebarRefreshFinished {
            generation,
            root,
            branch,
            dirty,
            staged_count,
            unstaged_count,
            untracked_count,
            conflicted_count,
            ahead,
            behind,
            error,
            operation_state_label,
            available_actions,
            conflicted_paths,
        } => {
            let info = GitSidebarInfo {
                root,
                branch,
                dirty,
                staged_count,
                unstaged_count,
                untracked_count,
                conflicted_count,
                ahead,
                behind,
                operation_state_label,
                available_actions,
                conflicted_paths,
            };
            super::super::commands::git_sidebar::apply_git_sidebar_refresh(
                app, generation, error, info,
            );
        }
        TuiCommand::SessionsReloaded {
            request_id,
            sessions,
            message_counts,
            error,
        } => {
            apply_sessions_reloaded(app, request_id, sessions, message_counts, error);
        }
        TuiCommand::SessionMessagesLoaded {
            request_id,
            session_id,
            messages,
            error,
        } => {
            apply_session_messages_loaded(app, request_id, session_id, messages, error);
        }
        TuiCommand::TreeDialogLoaded {
            current_session_id,
            nodes,
            error,
        } => {
            apply_tree_dialog_loaded(app, current_session_id, nodes, error);
        }
        TuiCommand::ImportPreviewLoaded {
            request_id,
            session,
            msg_count,
            error,
        } => {
            super::super::commands::import::apply_import_preview_loaded(
                app, request_id, session, msg_count, error,
            );
        }
        TuiCommand::ImportConfirmed {
            request_id,
            session,
            error,
        } => {
            super::super::commands::import::apply_import_confirmed(app, request_id, session, error);
        }
        TuiCommand::ResearchRunsLoaded {
            request_id,
            runs,
            error,
        } => {
            apply_research_runs_loaded(app, request_id, runs, error);
        }
        TuiCommand::ResearchRunLoaded {
            request_id,
            run_id,
            bundle,
            error,
        } => {
            apply_research_run_loaded(app, request_id, run_id, bundle, error);
        }
        TuiCommand::ResearchSectionLoaded {
            request_id,
            section,
            content,
            error,
        } => {
            apply_research_section_loaded(app, request_id, section, content, error);
        }
        TuiCommand::MemoryResult {
            toast_message,
            is_error,
        } => {
            apply_memory_result(app, toast_message, is_error);
        }
        TuiCommand::DoctorResult { summary, is_error } => {
            apply_doctor_result(app, summary, is_error);
        }
        TuiCommand::ShareSessionFinished {
            session_id,
            session,
            error,
        } => {
            apply_share_session_finished(app, session_id, session, error);
        }
        TuiCommand::UnshareSessionFinished {
            session_id,
            session,
            error,
        } => {
            apply_unshare_session_finished(app, session_id, session, error);
        }
        TuiCommand::ExportSessionFinished {
            session_id,
            json,
            error,
        } => {
            apply_export_session_finished(app, session_id, json, error);
        }
        TuiCommand::SessionMutationFinished {
            request_id,
            op,
            affected_ids,
            message,
            reload_after,
            error,
        } => {
            apply_session_mutation_finished(
                app,
                request_id,
                op,
                affected_ids,
                message,
                reload_after,
                error,
            );
        }
        TuiCommand::GoalOperationFinished {
            session_id,
            op,
            response,
            error,
        } => {
            apply_goal_operation_finished(app, session_id, op, response, error);
        }
        TuiCommand::SessionStateRefreshed {
            todos,
            active_goal,
            error,
        } => {
            apply_session_state_refreshed(app, todos, active_goal, error);
        }
        TuiCommand::EggpoolConnectionFinished {
            operation_id,
            result,
        } => {
            let is_current = app
                .dialog_state
                .connect_dialog
                .as_ref()
                .and_then(|dialog| dialog.operation_id.as_deref())
                == Some(operation_id.as_str());
            if !is_current {
                return;
            }
            match result {
                Ok(result) => {
                    app.messages_state.toasts.success(&format!(
                        "Eggpool connected on {} ({} models)",
                        result.connection.endpoint,
                        result.models.len()
                    ));
                    app.dialog_state.connect_dialog = None;
                    app.close_dialog();
                }
                Err(error) => {
                    app.messages_state
                        .toasts
                        .error(&format!("Eggpool connection failed: {error}"));
                    if let Some(dialog) = app.dialog_state.connect_dialog.as_mut() {
                        dialog.operation_id = None;
                        dialog.clear_secret();
                        dialog.set_error(error);
                    }
                }
            }
        }
        TuiCommand::ConnectionRotationFinished {
            operation_id,
            result,
        } => {
            let is_current = app
                .dialog_state
                .connect_dialog
                .as_ref()
                .and_then(|dialog| dialog.operation_id.as_deref())
                == Some(operation_id.as_str());
            if !is_current {
                return;
            }
            match result {
                Ok(result) => {
                    app.messages_state
                        .toasts
                        .success(&format!("Provider credential rotation {}", result.state));
                    app.dialog_state.connect_dialog = None;
                    app.close_dialog();
                    if let Some(tx) = app.tui_cmd_tx.clone() {
                        let _ = tx.try_send(TuiCommand::SessionSelectionRefresh);
                    }
                }
                Err(error) => {
                    app.messages_state
                        .toasts
                        .error(&format!("Provider credential rotation failed: {error}"));
                    if let Some(dialog) = app.dialog_state.connect_dialog.as_mut() {
                        dialog.operation_id = None;
                        dialog.clear_secret();
                    }
                }
            }
        }
        TuiCommand::SessionSelectionRefresh => {
            // The dialog was opened (or the user requested a refresh).
            // Drive a fresh selection fetch through the daemon.
            if app.dialog_state.connection_selection_dialog.is_some() {
                let session_id = app
                    .dialog_state
                    .connection_selection_dialog
                    .as_ref()
                    .map(|d| d.session_id.clone())
                    .unwrap_or_default();
                if let Some(tx) = app.tui_cmd_tx.clone() {
                    let _ = tx.try_send(TuiCommand::SessionSelectionLoad { session_id });
                }
            }
        }
        TuiCommand::SessionSelectionLoad { session_id } => {
            crate::tui::commands::session_selection::start_selection_refresh(app, session_id);
        }
        TuiCommand::ConnectionLifecycle {
            action,
            connection_id,
            expected_revision,
        } => {
            start_connection_lifecycle(app, action, connection_id, expected_revision);
        }
        TuiCommand::ConnectionLifecycleFinished {
            action,
            connection_id,
            message,
            error,
        } => {
            if let Some(error) = error {
                app.messages_state.toasts.error(&format!(
                    "Provider connection {connection_id} {action:?} failed: {error}"
                ));
            } else if let Some(message) = message {
                app.messages_state.toasts.info(&message);
            }
            if app.dialog_state.connection_selection_dialog.is_some() {
                let session_id = app
                    .dialog_state
                    .connection_selection_dialog
                    .as_ref()
                    .map(|dialog| dialog.session_id.clone())
                    .unwrap_or_default();
                if let Some(tx) = app.tui_cmd_tx.clone() {
                    let _ = tx.try_send(TuiCommand::SessionSelectionLoad { session_id });
                }
            }
        }
        TuiCommand::SessionSelectionLoaded {
            session_id,
            selection,
            connections,
            models,
            focused_connection_id,
            error,
        } => {
            let Some(dialog) = app.dialog_state.connection_selection_dialog.as_mut() else {
                return;
            };
            if dialog.session_id != session_id {
                return;
            }
            dialog.finish_loading();
            if let Some(err) = error {
                dialog.set_error(err);
                return;
            }
            dialog.set_connections(connections);
            dialog.set_models(models);
            if let Some(focused) = focused_connection_id {
                if let Some(idx) = dialog.connections.iter().position(|c| c.id == focused) {
                    dialog.connection_idx = idx;
                }
            }
            if let Some(sel) = selection {
                dialog.set_selection(sel);
            }
        }
        TuiCommand::TasksListed {
            request_id,
            tasks,
            error,
        } => {
            apply_tasks_listed(app, request_id, tasks, error);
        }
        TuiCommand::TaskOperationFinished {
            request_id,
            op,
            task_id,
            error,
        } => {
            apply_task_operation_finished(app, request_id, op, task_id, error);
        }
        TuiCommand::WorktreeListed {
            request_id,
            worktrees,
            error,
        } => {
            apply_worktree_listed(app, request_id, worktrees, error);
        }
        TuiCommand::TemplateSessionCreated {
            request_id,
            session,
            agent,
            model,
            template_name,
            error,
        } => {
            apply_template_session_created(
                app,
                request_id,
                session,
                agent,
                model,
                template_name,
                error,
            );
        }
        TuiCommand::NotificationSent { error } => {
            apply_notification_sent(app, error);
        }
        TuiCommand::RunHumanShell {
            command,
            promote_after,
        } => {
            handle_run_human_shell(app, command, promote_after);
        }
        TuiCommand::TestRun { scope, args } => {
            start_test_run(app, scope, args);
        }
        TuiCommand::TestRunFinished {
            request_id,
            report,
            summary,
            error,
        } => {
            apply_test_run_finished(app, request_id, report, summary, error);
        }
        TuiCommand::ShellEvent(event) => {
            handle_shell_event(app, event);
        }
        TuiCommand::ShellInclude { id, mode, question } => {
            handle_shell_include(app, id, mode, question);
        }
        TuiCommand::ShellRerun { id } => {
            handle_shell_rerun(app, id);
        }
        TuiCommand::ShellKill { id } => {
            handle_shell_kill(app, id);
        }
        TuiCommand::ShellList => {
            handle_shell_list(app);
        }
        TuiCommand::ShellShow { id } => {
            handle_shell_show(app, id);
        }
        TuiCommand::ShellAsk { id, question } => {
            handle_shell_ask(app, id, question);
        }
        TuiCommand::ShellExpand { id, stream, range } => {
            handle_shell_expand(app, id, stream, range);
        }
        TuiCommand::FileDiffStatsReady {
            path,
            generation,
            result,
        } => {
            handle_file_diff_stats_ready(app, path, generation, result);
        }
        TuiCommand::TuiStats => {
            let mut summary = app.ui_state.diagnostics.summary();
            let task_summary = app.task_registry.summary();
            summary.push('\n');
            summary.push_str(&task_summary);
            if !app.shell_handles.is_empty() {
                summary.push_str(&format!("\nShell handles: {}", app.shell_handles.len()));
            }

            summary.push_str("\n\nBackground Activity:");
            let mut has_activity = false;

            if app.dialog_state.session_reload_request.is_loading() {
                summary.push_str("\n  Session reload: loading");
                has_activity = true;
            }
            if app.dialog_state.import_request.is_loading() {
                summary.push_str("\n  Import preview: loading");
                has_activity = true;
            }
            if app.dialog_state.research_request.is_loading() {
                summary.push_str("\n  Research browser: loading");
                has_activity = true;
            }
            if app.dialog_state.session_messages_request.is_loading() {
                summary.push_str("\n  Session messages: loading");
                has_activity = true;
            }
            if app.dialog_state.session_mutation_request.is_loading() {
                summary.push_str("\n  Session mutation: loading");
                has_activity = true;
            }
            if app.dialog_state.task_list_request.is_loading() {
                summary.push_str("\n  Task list: loading");
                has_activity = true;
            }
            if app.dialog_state.worktree_list_request.is_loading() {
                summary.push_str("\n  Worktree list: loading");
                has_activity = true;
            }
            if app.dialog_state.template_create_request.is_loading() {
                summary.push_str("\n  Template create: loading");
                has_activity = true;
            }
            if app.security_review_running.is_some() {
                summary.push_str("\n  Security review: running");
                has_activity = true;
            }
            if !app.shell_handles.is_empty() {
                summary.push_str(&format!(
                    "\n  Shell commands: {} running",
                    app.shell_handles.len()
                ));
                has_activity = true;
            }

            let pending_diffs = app
                .session_state
                .changed_files
                .iter()
                .filter(|f| {
                    matches!(
                        f.diff_state,
                        crate::tui::app::state::session::DiffStatsState::Pending { .. }
                    )
                })
                .count();
            if pending_diffs > 0 {
                summary.push_str(&format!("\n  Pending diffs: {pending_diffs}"));
                has_activity = true;
            }

            if !has_activity {
                summary.push_str("\n  (none)");
            }

            let lines: Vec<String> = summary.lines().map(|s| s.to_string()).collect();
            app.open_info_dialog(
                crate::tui::components::dialogs::info::InfoType::Stats,
                lines,
            );
        }
        TuiCommand::PluginCommandRun {
            spec,
            args,
            session_id,
            model,
        } => {
            start_plugin_command(app, spec, args, session_id, model);
        }
        TuiCommand::PluginCommandFinished {
            invocation_id,
            command,
            response,
            stdout,
            stderr,
            error,
        } => {
            apply_plugin_command_finished(
                app,
                invocation_id,
                command,
                response,
                stdout,
                stderr,
                error,
            );
        }
        TuiCommand::PluginUiEffect { effect } => {
            apply_plugin_ui_effect(app, effect);
        }
        TuiCommand::PluginList => {
            crate::tui::commands::plugin_management::show_plugins(app);
        }
        TuiCommand::PluginInfo { selector } => {
            crate::tui::commands::plugin_management::show_plugin_info(app, &selector);
        }
        TuiCommand::PluginEnable { selector } => {
            crate::tui::commands::plugin_management::enable_plugin(app, &selector);
        }
        TuiCommand::PluginDisable { selector } => {
            crate::tui::commands::plugin_management::disable_plugin(app, &selector);
        }
        TuiCommand::PluginDoctor { selector } => {
            crate::tui::commands::plugin_management::doctor_plugin(app, selector.as_deref());
        }
        TuiCommand::PluginRemove { selector } => {
            crate::tui::commands::plugin_management::remove_plugin(app, &selector);
        }
        TuiCommand::PluginInstall { path } => {
            crate::tui::commands::plugin_management::install_plugin(app, &path);
        }
        TuiCommand::PluginListFinished { lines, error } => {
            crate::tui::commands::plugin_management::apply_plugin_list_finished(app, lines, error);
        }
        TuiCommand::PluginInfoFinished {
            plugin_id,
            lines,
            error,
        } => {
            crate::tui::commands::plugin_management::apply_plugin_info_finished(
                app, plugin_id, lines, error,
            );
        }
        TuiCommand::PluginEnableFinished { plugin_id, error } => {
            crate::tui::commands::plugin_management::apply_plugin_enable_finished(
                app, plugin_id, error,
            );
        }
        TuiCommand::PluginDisableFinished { plugin_id, error } => {
            crate::tui::commands::plugin_management::apply_plugin_disable_finished(
                app, plugin_id, error,
            );
        }
        TuiCommand::PluginDoctorFinished { lines, error } => {
            crate::tui::commands::plugin_management::apply_plugin_doctor_finished(
                app, lines, error,
            );
        }
        TuiCommand::PluginRemoveFinished {
            plugin_id,
            removed_files,
            install_path,
            warning,
            error,
        } => {
            crate::tui::commands::plugin_management::apply_plugin_remove_finished(
                app,
                plugin_id,
                removed_files,
                install_path,
                warning,
                error,
            );
        }
        TuiCommand::PluginInstallFinished {
            source,
            lines,
            error,
        } => {
            crate::tui::commands::plugin_management::apply_plugin_install_finished(
                app, source, lines, error,
            );
        }
    }
}
