//! Project Work Orders M003: Task composer, scheduling sheet, and Task view.
//!
//! Frontend projection/controller for project Task mode. Durable
//! WorkOrder truth stays in daemon/core stores; this module never
//! creates sessions or jobs directly — a Task confirm sends exactly one
//! `CoreRequest::WorkOrderCreate` through the canonical WorkOrder
//! service, and materialization stays daemon-owned (M002).
//!
//! Async discipline mirrors `prompt.rs`: every completion carries the
//! captured [`UiRouteToken`] plus request/generation identity and is
//! dropped at apply time after tab switch/close, workspace rebind,
//! reconnect, or a superseding refresh/reorder. A late create success
//! may have committed daemon state; it is never deleted to "undo".

use crate::core::CoreClient;
use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::protocol::work_order::{SequenceLaneDto, WorkOrderDto, WorkOrderSummaryDto};
use crate::tui::app::state::{
    build_work_order_create, describe_schedule_full, trigger_creation_key, trigger_rotation_key,
    validate_draft, ComposerMode, OneTimeBearer, OneTimeTriggerSecret, PendingTaskCreate,
    ProjectExecutionContext, RouteCheck, TaskModelChoice, TaskScheduleDraft, TaskViewRow,
    TaskViewState, TriggerSetupStatus, UiRouteToken, MAX_TASK_VIEW_ROWS,
};
use crate::tui::app::{App, Dialog, TuiCommand};
use crate::tui::async_cmd::spawn_scoped_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// Prefetched context for opening the scheduling sheet.
#[derive(Debug, Clone)]
pub struct SheetPrefetchData {
    pub lanes: Vec<SequenceLaneDto>,
    pub summary: Option<WorkOrderSummaryDto>,
    pub capabilities_supported: bool,
    pub max_repeat_count: u32,
    pub task_connection: Option<String>,
    pub task_model: Option<String>,
    pub task_pref_revision: u64,
}

// ── Composer mode ────────────────────────────────────────────────────

/// Toggle the prompt composer between Session and Task submission
/// modes. `InputMode` is untouched and the editable prompt text is
/// preserved. Entering Task mode quietly prefetches the daemon-owned
/// Task-model preference so the sheet opens with the right default.
pub(crate) fn toggle_composer_mode(app: &mut App) {
    let mode = app.prompt_state.toggle_composer_mode();
    match mode {
        ComposerMode::Session => {
            app.messages_state
                .toasts
                .info("Composer: Session — Enter sends a normal prompt");
        }
        ComposerMode::Task => {
            app.messages_state
                .toasts
                .info("Composer: Task — Enter opens the scheduling sheet (Tab back to Session)");
            prefetch_task_model(app);
        }
    }
}

/// Best-effort prefetch of the principal-scoped Task-model preference.
/// Quiet on success; failures only surface when the sheet resolves its
/// model default (with a visible fallback notice).
pub(crate) fn prefetch_task_model(app: &mut App) {
    let request_id = app.dialog_state.task_model_pref_request.begin();
    let Some(route) = capture_route(app, request_id) else {
        let _ = app
            .dialog_state
            .task_model_pref_request
            .fail(request_id, "No active project tab".to_string());
        return;
    };
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let scope = task_scope(&route);
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "task_model_prefetch",
        scope.0,
        None,
        scope.1,
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TaskModelPrefetched {
                    request_id,
                    route,
                    connection: None,
                    model: None,
                    revision: 0,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            match core_client
                .request(crate::core::new_request(
                    format!("task-model-prefetch-{request_id}-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ApprovalPreferenceGet,
                ))
                .await
            {
                Ok(CoreResponse::ApprovalPreference { preference }) => {
                    Some(TuiCommand::TaskModelPrefetched {
                        request_id,
                        route,
                        connection: preference.last_task_provider_connection_id,
                        model: preference.last_task_model_id,
                        revision: preference.revision,
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::TaskModelPrefetched {
                        request_id,
                        route,
                        connection: None,
                        model: None,
                        revision: 0,
                        error: Some(format!("{code}: {message}")),
                    })
                }
                Ok(_) => Some(TuiCommand::TaskModelPrefetched {
                    request_id,
                    route,
                    connection: None,
                    model: None,
                    revision: 0,
                    error: Some(
                        "Unexpected response while reading Task model preference".to_string(),
                    ),
                }),
                Err(error) => Some(TuiCommand::TaskModelPrefetched {
                    request_id,
                    route,
                    connection: None,
                    model: None,
                    revision: 0,
                    error: Some(error.to_string()),
                }),
            }
        },
    );
    if task_id.is_none() {
        let _ = app
            .dialog_state
            .task_model_pref_request
            .fail(request_id, "TUI command channel unavailable".to_string());
    }
}

/// Apply a Task-model prefetch completion. Stale completions (newer
/// prefetch, tab switch, reconnect) are dropped silently: the sheet
/// falls back to the current/default model with a visible notice.
pub(crate) fn apply_task_model_prefetched(
    app: &mut App,
    request_id: u64,
    route: UiRouteToken,
    connection: Option<String>,
    model: Option<String>,
    revision: u64,
    error: Option<String>,
) {
    if let Some(error) = error {
        if app
            .dialog_state
            .task_model_pref_request
            .fail(request_id, error)
        {
            // Quiet: the sheet surfaces the fallback notice itself.
        }
        return;
    }
    if !app.dialog_state.task_model_pref_request.finish(request_id) {
        return;
    }
    let Some(check) = current_task_route_check(app) else {
        return;
    };
    if !route.matches(&check) {
        return;
    }
    app.prompt_state.task_model_choice = Some(TaskModelChoice {
        connection,
        model,
        revision,
        from_preference: true,
    });
}

/// Resolve the composer Task model: the last valid Task model when
/// available, else the normal current/default selection with a visible
/// notice. An already-created WorkOrder never silently changes model;
/// this only seeds new drafts.
pub(crate) fn resolve_task_model(app: &mut App) -> (Option<String>, bool, Option<String>) {
    let current = app.agent_state.current_model.clone();
    let known: &[String] = &app.agent_state.models;
    let remembered = app
        .prompt_state
        .task_model_choice
        .clone()
        .and_then(|choice| choice.model);
    match remembered
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
    {
        Some(model) if known.is_empty() || known.iter().any(|m| m == model) => {
            (Some(model.to_string()), true, None)
        }
        Some(model) => (
            Some(current),
            false,
            Some(format!(
                "Remembered Task model '{model}' is unavailable; using current model"
            )),
        ),
        None => (Some(current), false, None),
    }
}

// ── Scheduling sheet ─────────────────────────────────────────────────

/// Enter handler for Task composer mode: open the scheduling sheet
/// instead of submitting a turn. The prompt text stays editable until
/// `WorkOrderCreate` succeeds; slash commands and human-shell input are
/// handled before this branch (see `send_prompt`), so they keep their
/// normal meaning in Task mode.
pub(crate) fn open_task_sheet_for_prompt(app: &mut App, prompt_text: String) {
    if prompt_text.trim().is_empty() {
        return;
    }
    let context = match app.project_execution_context() {
        Ok(context) => context,
        Err(error) => {
            app.messages_state.toasts.error(&error);
            return;
        }
    };
    let project_id = match context.project_id.clone() {
        Some(project_id) => project_id,
        None => {
            app.messages_state
                .toasts
                .error("Task mode needs an active project tab; choose a project first");
            return;
        }
    };
    let request_id = app.dialog_state.task_sheet_request.begin();
    let Some(route) = capture_route(app, request_id) else {
        app.messages_state
            .toasts
            .error("No active project tab; choose a project before scheduling a task");
        let _ = app
            .dialog_state
            .task_sheet_request
            .fail(request_id, "No active project tab".to_string());
        return;
    };
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let scope = task_scope(&route);
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "task_sheet_prefetch",
        scope.0,
        None,
        scope.1,
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TaskSheetPrefetched {
                    request_id,
                    route,
                    project_id,
                    data: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            match fetch_sheet_context(core_client.as_ref(), &project_id).await {
                Ok(data) => Some(TuiCommand::TaskSheetPrefetched {
                    request_id,
                    route,
                    project_id,
                    data: Some(data),
                    error: None,
                }),
                Err(error) => Some(TuiCommand::TaskSheetPrefetched {
                    request_id,
                    route,
                    project_id,
                    data: None,
                    error: Some(error),
                }),
            }
        },
    );
    if task_id.is_none() {
        let _ = app.dialog_state.task_sheet_request.fail(
            request_id,
            "TUI command channel unavailable; task was not scheduled".to_string(),
        );
        app.messages_state
            .toasts
            .error("TUI command channel unavailable; task was not scheduled");
        return;
    }
    app.messages_state
        .toasts
        .info("Loading scheduling options…");
}

async fn fetch_sheet_context(
    core_client: &dyn CoreClient,
    project_id: &str,
) -> Result<SheetPrefetchData, String> {
    let capabilities_supported = match core_client
        .request(crate::core::new_request(
            format!("wo-capabilities-{}", uuid::Uuid::new_v4()),
            CoreRequest::WorkOrderCapabilities,
        ))
        .await
    {
        Ok(CoreResponse::WorkOrderCapabilities { capabilities }) => capabilities.supported,
        Ok(CoreResponse::Error { code, message }) => {
            return Err(format!(
                "WorkOrder capability check failed: {code}: {message}"
            ));
        }
        Ok(_) => return Err("Unexpected capability response".to_string()),
        Err(error) => return Err(error.to_string()),
    };
    if !capabilities_supported {
        return Err(
            "This server has no WorkOrder capability; tasks need a current daemon".to_string(),
        );
    }
    let lanes = match core_client
        .request(crate::core::new_request(
            format!("wo-lanes-{}", uuid::Uuid::new_v4()),
            CoreRequest::WorkOrderLaneList {
                project_id: project_id.to_string(),
                limit: Some(20),
            },
        ))
        .await
    {
        Ok(CoreResponse::WorkOrderLaneList { lanes, .. }) => lanes,
        Ok(CoreResponse::Error { code, message }) => {
            return Err(format!("Lane list failed: {code}: {message}"));
        }
        Ok(_) => return Err("Unexpected lane list response".to_string()),
        Err(error) => return Err(error.to_string()),
    };
    let summary = match core_client
        .request(crate::core::new_request(
            format!("wo-summary-{}", uuid::Uuid::new_v4()),
            CoreRequest::WorkOrderSummary {
                project_id: project_id.to_string(),
            },
        ))
        .await
    {
        Ok(CoreResponse::WorkOrderSummary { summary }) => Some(summary),
        // Summary is a preview hint; its absence never blocks the sheet.
        Ok(_) | Err(_) => None,
    };
    let (task_connection, task_model, task_pref_revision, max_repeat_count) = match core_client
        .request(crate::core::new_request(
            format!("wo-pref-{}", uuid::Uuid::new_v4()),
            CoreRequest::ApprovalPreferenceGet,
        ))
        .await
    {
        Ok(CoreResponse::ApprovalPreference { preference }) => (
            preference.last_task_provider_connection_id,
            preference.last_task_model_id,
            preference.revision,
            100u32,
        ),
        Ok(_) | Err(_) => (None, None, 0, 100u32),
    };
    let max_repeat_count = match core_client
        .request(crate::core::new_request(
            format!("wo-cap2-{}", uuid::Uuid::new_v4()),
            CoreRequest::WorkOrderCapabilities,
        ))
        .await
    {
        Ok(CoreResponse::WorkOrderCapabilities { capabilities }) => {
            capabilities.max_repeat_count.max(1)
        }
        Ok(_) | Err(_) => max_repeat_count,
    };
    Ok(SheetPrefetchData {
        lanes,
        summary,
        capabilities_supported,
        max_repeat_count,
        task_connection,
        task_model,
        task_pref_revision,
    })
}

/// Apply a sheet-prefetch completion: build the draft and open the
/// sheet dialog. The prompt text is read fresh from the widget so edits
/// made while loading are honored; staleness is project/view-scoped.
pub(crate) fn apply_sheet_prefetched(
    app: &mut App,
    request_id: u64,
    route: UiRouteToken,
    project_id: String,
    data: Option<SheetPrefetchData>,
    error: Option<String>,
) {
    if let Some(error) = error {
        if app
            .dialog_state
            .task_sheet_request
            .fail(request_id, error.clone())
        {
            app.messages_state
                .toasts
                .error(&format!("Could not open scheduling sheet: {error}"));
        }
        return;
    }
    if !app.dialog_state.task_sheet_request.finish(request_id) {
        return;
    }
    let Some(check) = current_task_route_check(app) else {
        return;
    };
    if !route.matches(&check) {
        return;
    }
    if app.active_project_id() != Some(project_id.as_str()) {
        return;
    }
    let Some(data) = data else {
        app.messages_state
            .toasts
            .error("Could not open scheduling sheet: empty prefetch");
        return;
    };
    // Cache the daemon preference for model resolution.
    app.prompt_state.task_model_choice = Some(TaskModelChoice {
        connection: data.task_connection.clone(),
        model: data.task_model.clone(),
        revision: data.task_pref_revision,
        from_preference: data.task_model.is_some(),
    });
    let (model, from_pref, fallback_notice) = resolve_task_model(app);
    let policy = app.policy_ui.cached.clone();
    let mut draft = TaskScheduleDraft::default();
    if let Some(first) = data.lanes.first() {
        draft.lane_id = Some(first.lane_id.clone());
        draft.lane_label = first.label.clone();
    }
    // C001: trigger capability hint. Current daemons bundle M005 with
    // WorkOrders; older servers leave the row disabled. No protocol
    // change: `supported` is the existing capability flag.
    draft.external_trigger_capable = data.capabilities_supported;
    draft.model = model;
    draft.model_from_task_preference = from_pref;
    draft.requested_approval = policy
        .as_ref()
        .map(|view| view.effective_mode.as_str().to_string());
    draft.requested_sandbox = policy
        .as_ref()
        .map(|view| view.effective_profile.as_str().to_string());
    if let Some(notice) = fallback_notice {
        app.messages_state.toasts.warning(&notice);
    }
    app.dialog_state.task_schedule_draft = Some(draft.clone());
    open_task_schedule_dialog(app, &draft, data.lanes, data.summary);
}

/// Build the sheet seed from cached UI state and mount the
/// FocusManager-owned scheduling dialog.
fn open_task_schedule_dialog(
    app: &mut App,
    draft: &TaskScheduleDraft,
    lanes: Vec<SequenceLaneDto>,
    summary: Option<WorkOrderSummaryDto>,
) {
    use crate::tui::components::dialogs::task_schedule::{TaskScheduleDialog, TaskScheduleSeed};
    let models: Vec<String> = app.agent_state.models.iter().take(50).cloned().collect();
    let model_index = draft
        .model
        .as_ref()
        .and_then(|model| models.iter().position(|known| known == model))
        .unwrap_or(0);
    let lane_index = draft
        .lane_id
        .as_ref()
        .and_then(|lane_id| lanes.iter().position(|lane| &lane.lane_id == lane_id))
        .unwrap_or(0);
    // C001 queue preview: per-lane order + revision + pinned head from
    // the Task-view cache when available (running/claimed head is a
    // frontend pin hint; durable safety stays CAS-guarded).
    let mut queue_orders: Vec<Vec<String>> = Vec::with_capacity(lanes.len());
    let mut queue_revisions: Vec<u64> = Vec::with_capacity(lanes.len());
    let mut queue_pinned_heads: Vec<Option<String>> = Vec::with_capacity(lanes.len());
    for lane in &lanes {
        queue_revisions.push(lane.revision);
        queue_orders.push(lane.ordered_work_order_ids.clone());
        let pinned = lane
            .ordered_work_order_ids
            .first()
            .filter(|first| {
                app.dialog_state
                    .task_view
                    .rows
                    .iter()
                    .find(|row| &row.work_order.work_order_id == *first)
                    .is_some_and(|row| row.is_running())
            })
            .cloned();
        queue_pinned_heads.push(pinned);
    }
    let seed = TaskScheduleSeed {
        prompt_preview: app.prompt_state.prompt.get_text(),
        lanes: lanes
            .iter()
            .map(|lane| (lane.lane_id.clone(), lane.label.clone()))
            .collect(),
        lane_index,
        models,
        model_index,
        policy_summary: app
            .policy_ui
            .status_line()
            .unwrap_or_else(|| "policy unavailable".to_string()),
        workspace_summary: "auto (isolated worktree for Git mutation)".to_string(),
        queue_summary: match summary.as_ref() {
            Some(summary) => format!(
                "{} active · {} waiting · {} attention",
                summary.active, summary.waiting_occurrences, summary.attention_occurrences
            ),
            None => "queue unavailable".to_string(),
        },
        capabilities_supported: true,
        queue_orders,
        queue_revisions,
        queue_pinned_heads,
        trigger_capable: draft.external_trigger_capable,
    };
    let dialog = TaskScheduleDialog::new(seed, draft);
    app.push_dialog(Dialog::TaskSchedule, Box::new(dialog));
}

/// Confirm handler for the scheduling sheet: validate, then spawn
/// exactly one `WorkOrderCreate` through the canonical WorkOrder
/// service. Never creates a session/job directly from the TUI.
/// Validation failures keep the sheet open with an actionable error
/// and leave the editable prompt untouched.
#[allow(clippy::too_many_arguments)]
pub(crate) fn confirm_task_schedule(
    app: &mut App,
    delay_text: String,
    not_before_text: String,
    repeat_text: String,
    sequential: bool,
    lane_id: Option<String>,
    queue_insert_position: Option<usize>,
    queue_expected_revision: Option<u64>,
    external_trigger: bool,
    gate_join_all: bool,
    model: Option<String>,
) {
    let mut draft = match app.dialog_state.task_schedule_draft.clone() {
        Some(draft) => draft,
        None => {
            app.messages_state
                .toasts
                .warning("Scheduling sheet is not open");
            return;
        }
    };
    draft.delay_text = delay_text;
    draft.not_before_text = not_before_text;
    draft.repeat_text = repeat_text;
    draft.sequential = sequential;
    if sequential {
        draft.lane_id = lane_id;
        draft.queue_insert_position = queue_insert_position;
        draft.queue_expected_revision = queue_expected_revision;
    } else {
        draft.queue_insert_position = None;
        draft.queue_expected_revision = None;
    }
    draft.external_trigger = external_trigger;
    draft.gate_join = if gate_join_all {
        crate::tui::app::state::GateJoin::All
    } else {
        crate::tui::app::state::GateJoin::Any
    };
    if let Some(model) = model {
        draft.model = Some(model);
    }
    let validated = match validate_draft(&draft) {
        Ok(validated) => validated,
        Err(error) => {
            sync_sheet_error(app, &error);
            app.messages_state.toasts.warning(&error);
            return;
        }
    };
    // Re-resolve the model against the current catalog before creation:
    // a remembered model that vanished falls back visibly, and an
    // already-created WorkOrder never silently changes model.
    let known: &[String] = &app.agent_state.models;
    if let Some(model) = draft.model.clone() {
        let trimmed = model.trim();
        if !trimmed.is_empty() && !known.is_empty() && !known.iter().any(|m| m == trimmed) {
            let current = app.agent_state.current_model.clone();
            app.messages_state.toasts.warning(&format!(
                "Task model '{trimmed}' is unavailable; using current model"
            ));
            draft.model = Some(current);
            draft.model_from_task_preference = false;
        }
    }
    let context = match app.project_execution_context() {
        Ok(context) => context,
        Err(error) => {
            sync_sheet_error(app, &error);
            app.messages_state.toasts.error(&error);
            return;
        }
    };
    let project_id = match context.project_id.clone() {
        Some(project_id) => project_id,
        None => {
            let error = "Task mode needs an active project tab".to_string();
            sync_sheet_error(app, &error);
            app.messages_state.toasts.error(&error);
            return;
        }
    };
    let prompt = app.prompt_state.prompt.get_text();
    if prompt.trim().is_empty() {
        let error = "Task prompt is empty".to_string();
        sync_sheet_error(app, &error);
        app.messages_state.toasts.warning(&error);
        return;
    }
    let request_id = app.dialog_state.work_order_create_request.begin();
    let Some(route) = capture_route(app, request_id) else {
        let error = "No active project tab".to_string();
        sync_sheet_error(app, &error);
        app.messages_state.toasts.error(&error);
        let _ = app
            .dialog_state
            .work_order_create_request
            .fail(request_id, error);
        return;
    };
    let lane_preview = draft.lane_label.clone().or(draft.lane_id.clone());
    let schedule_summary = describe_schedule_full(
        &validated,
        lane_preview.as_deref(),
        draft.model.as_deref(),
        draft.external_trigger,
        draft.gate_join,
    );
    let request = build_work_order_create(
        &project_id,
        prompt.trim(),
        None,
        &draft,
        &validated,
        Some(&format!("tui-task-{}", uuid::Uuid::new_v4())),
    );
    let pending = PendingTaskCreate {
        request_id,
        prompt: prompt.clone(),
        project_id: project_id.clone(),
        route: route.clone(),
        context: context.clone(),
        external_trigger: draft.external_trigger,
        trigger_key_prefix: draft
            .external_trigger
            .then(|| format!("tui-task-{request_id}")),
        queue_lane_id: if draft.sequential {
            draft.lane_id.clone()
        } else {
            None
        },
        queue_insert_position: draft.queue_insert_position,
        queue_expected_revision: draft.queue_expected_revision,
    };
    app.prompt_state.pending_task_create = Some(pending.clone());
    app.prompt_state.task_submit_started = true;
    app.prompt_state.prompt.set_waiting(true);
    // The sheet closes on confirm; the prompt text stays until the
    // create succeeds (failure restores it exactly once).
    app.close_dialog();
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let scope = task_scope(&route);
    let prompt_for_history = prompt.trim().to_string();
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "work_order_create",
        scope.0,
        None,
        scope.1,
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::WorkOrderCreated {
                    request_id,
                    route,
                    project_id,
                    prompt: prompt_for_history,
                    work_order: None,
                    duplicate: false,
                    schedule_summary,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            match core_client
                .request(crate::core::new_request(
                    format!("work-order-create-{request_id}-{}", uuid::Uuid::new_v4()),
                    CoreRequest::WorkOrderCreate { request },
                ))
                .await
            {
                Ok(CoreResponse::WorkOrder {
                    work_order,
                    duplicate,
                }) => Some(TuiCommand::WorkOrderCreated {
                    request_id,
                    route,
                    project_id,
                    prompt: prompt_for_history,
                    work_order: Some(work_order),
                    duplicate,
                    schedule_summary,
                    error: None,
                }),
                Ok(CoreResponse::Error { code, message }) => Some(TuiCommand::WorkOrderCreated {
                    request_id,
                    route,
                    project_id,
                    prompt: prompt_for_history,
                    work_order: None,
                    duplicate: false,
                    schedule_summary,
                    error: Some(format!("{code}: {message}")),
                }),
                Ok(_) => Some(TuiCommand::WorkOrderCreated {
                    request_id,
                    route,
                    project_id,
                    prompt: prompt_for_history,
                    work_order: None,
                    duplicate: false,
                    schedule_summary,
                    error: Some("Unexpected response while creating the task".to_string()),
                }),
                Err(error) => Some(TuiCommand::WorkOrderCreated {
                    request_id,
                    route,
                    project_id,
                    prompt: prompt_for_history,
                    work_order: None,
                    duplicate: false,
                    schedule_summary,
                    error: Some(error.to_string()),
                }),
            }
        },
    );
    if task_id.is_none() {
        let _ = app.dialog_state.work_order_create_request.fail(
            request_id,
            "TUI command channel unavailable; task was not created".to_string(),
        );
        apply_task_create_failure(
            app,
            pending,
            "TUI command channel unavailable; task was not created",
        );
    }
}

/// Apply a `WorkOrderCreate` completion with project/view/request
/// guards. Failures restore the editable prompt exactly once (following
/// the session-create continuation pattern); late successes never
/// delete daemon state.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_work_order_created(
    app: &mut App,
    request_id: u64,
    route: UiRouteToken,
    project_id: String,
    prompt: String,
    work_order: Option<WorkOrderDto>,
    duplicate: bool,
    schedule_summary: String,
    error: Option<String>,
) {
    if !app
        .dialog_state
        .work_order_create_request
        .is_current(request_id)
        || app.dialog_state.work_order_create_request.is_cancelled()
    {
        return;
    }
    let pending = app.prompt_state.pending_task_create.clone();
    let Some(check) = current_task_route_check(app) else {
        return;
    };
    // Stale route: the WorkOrder may still have committed daemon-side.
    // Never delete it to "undo"; refresh the projection when next
    // foregrounded and drop the UI completion.
    if !route.matches(&check) || app.active_project_id() != Some(project_id.as_str()) {
        app.prompt_state.pending_task_create = None;
        app.prompt_state.task_submit_started = false;
        app.prompt_state.prompt.set_waiting(false);
        refresh_task_view_if_open(app, &project_id);
        return;
    }
    app.prompt_state.pending_task_create = None;
    app.prompt_state.task_submit_started = false;

    if let Some(error) = error {
        if app
            .dialog_state
            .work_order_create_request
            .fail(request_id, error.clone())
        {
            let pending = pending.unwrap_or(fallback_pending(
                app,
                request_id,
                prompt.clone(),
                project_id.clone(),
                route.clone(),
            ));
            apply_task_create_failure(app, pending, &error);
        }
        return;
    }
    let Some(work_order) = work_order else {
        if app.dialog_state.work_order_create_request.fail(
            request_id,
            "WorkOrderCreate returned no work order".to_string(),
        ) {
            let pending = pending.unwrap_or(fallback_pending(
                app,
                request_id,
                prompt.clone(),
                project_id.clone(),
                route.clone(),
            ));
            apply_task_create_failure(app, pending, "WorkOrderCreate returned no work order");
        }
        return;
    };
    if !app
        .dialog_state
        .work_order_create_request
        .finish(request_id)
    {
        return;
    }
    // Capture the chained intentions before clearing pending: external
    // trigger setup and queue placement both survive the prompt clear.
    let wants_trigger = pending.as_ref().is_some_and(|p| p.external_trigger);
    let queue_lane = pending.as_ref().and_then(|p| p.queue_lane_id.clone());
    let queue_pos = pending.as_ref().and_then(|p| p.queue_insert_position);
    // Success: the prompt was confirmed, so the editor clears. Record
    // history with the same bounded rule as normal sends.
    push_prompt_history(app, &prompt);
    app.prompt_state.prompt.clear();
    app.prompt_state.prompt.set_waiting(false);
    app.prompt_state.pending_send = false;
    let short: String = work_order.work_order_id.chars().take(8).collect();
    let duplicate_note = if duplicate {
        " (converged on existing task)"
    } else {
        ""
    };
    app.messages_state.toasts.info(&format!(
        "Task {short} created: {schedule_summary}{duplicate_note}"
    ));
    // Remember the snapshotted Task model through the canonical
    // settings service (best-effort; warn-only on failure, like the
    // session model-preference path).
    persist_task_model_best_effort(app, work_order.requested_model.clone());
    // C001 §6.2: honor the focused queue insertion position. The
    // WorkOrder was appended by creation; move it to the selected
    // position best-effort via the existing lane CAS contract. Stale
    // revisions refresh with `queue changed; retry` instead of a
    // speculative order.
    if let (Some(lane_id), Some(pos)) = (queue_lane, queue_pos) {
        place_new_work_order_in_lane(app, &project_id, &work_order.work_order_id, &lane_id, pos);
    }
    // A visible Task view for this project refreshes in place.
    refresh_task_view_if_open(app, &project_id);
    // C001 §6.4: chain exactly one trigger create when the draft
    // requested an external gate. The WorkOrder stands regardless;
    // trigger failure surfaces setup-incomplete with a retry path.
    if wants_trigger {
        start_trigger_create(
            app,
            project_id.clone(),
            work_order.work_order_id.clone(),
            None,
            Some(trigger_creation_key(request_id, &work_order.work_order_id)),
        );
    }
}

fn fallback_pending(
    app: &App,
    request_id: u64,
    prompt: String,
    project_id: String,
    route: UiRouteToken,
) -> PendingTaskCreate {
    PendingTaskCreate {
        request_id,
        prompt,
        project_id: project_id.clone(),
        route: route.clone(),
        context: match app.project_execution_context() {
            Ok(context) => context,
            Err(_) => ProjectExecutionContext {
                project_id: Some(project_id.clone()),
                workspace_id: None,
                session_id: None,
                workspace_root: std::path::PathBuf::from("/"),
            },
        },
        external_trigger: false,
        trigger_key_prefix: None,
        queue_lane_id: None,
        queue_insert_position: None,
        queue_expected_revision: None,
    }
}

// ── Focused queue placement (C001 §6.2) ─────────────────────────────

/// Best-effort placement of a newly created WorkOrder at the sheet's
/// focused insertion position. Creation appends; this moves the new
/// member via the existing lane CAS contract. Stale revisions or order
/// mismatches refresh with `queue changed; retry` instead of a
/// speculative order. No fake WorkOrder is ever fabricated.
fn place_new_work_order_in_lane(
    app: &mut App,
    project_id: &str,
    work_order_id: &str,
    lane_id: &str,
    desired_position: usize,
) {
    let request_id = app.dialog_state.work_order_reorder_request.begin();
    let Some(route) = capture_route(app, request_id) else {
        let _ = app
            .dialog_state
            .work_order_reorder_request
            .fail(request_id, "No active project tab".to_string());
        return;
    };
    let generation = app.dialog_state.task_view.generation;
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let scope = task_scope(&route);
    let lane_id = lane_id.to_string();
    let work_order_id = work_order_id.to_string();
    let project_id = project_id.to_string();
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "queue_placement",
        scope.0,
        None,
        scope.1,
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::LaneReordered {
                    request_id,
                    route,
                    generation,
                    lane_id,
                    revision: 0,
                    error: Some("Core unavailable".to_string()),
                });
            };
            // Fetch the fresh lane, insert the new member at the
            // clamped position, then CAS-reorder. Fresh revision avoids
            // using a stale sheet preview; conflicts still refresh.
            let lanes: Vec<SequenceLaneDto> = match core_client
                .request(crate::core::new_request(
                    format!("queue-place-list-{request_id}-{}", uuid::Uuid::new_v4()),
                    CoreRequest::WorkOrderLaneList {
                        project_id: project_id.clone(),
                        limit: Some(20),
                    },
                ))
                .await
            {
                Ok(CoreResponse::WorkOrderLaneList { lanes, .. }) => lanes,
                Ok(CoreResponse::Error { code, message }) => {
                    return Some(TuiCommand::LaneReordered {
                        request_id,
                        route,
                        generation,
                        lane_id,
                        revision: 0,
                        error: Some(format!(
                            "Queue placement lane list failed: {code}: {message}"
                        )),
                    });
                }
                Ok(_) => {
                    return Some(TuiCommand::LaneReordered {
                        request_id,
                        route,
                        generation,
                        lane_id,
                        revision: 0,
                        error: Some("Unexpected lane list response".to_string()),
                    });
                }
                Err(error) => {
                    return Some(TuiCommand::LaneReordered {
                        request_id,
                        route,
                        generation,
                        lane_id,
                        revision: 0,
                        error: Some(error.to_string()),
                    });
                }
            };
            let Some(lane) = lanes.into_iter().find(|lane| lane.lane_id == lane_id) else {
                return Some(TuiCommand::LaneReordered {
                    request_id,
                    route,
                    generation,
                    lane_id,
                    revision: 0,
                    error: Some("Queue lane is no longer listed; task stays at end".to_string()),
                });
            };
            let mut order = lane.ordered_work_order_ids.clone();
            // The new member is already appended by creation; remove it
            // first so insertion is a move, not a duplicate.
            order.retain(|id| id != &work_order_id);
            let pos = desired_position.min(order.len());
            order.insert(pos, work_order_id.clone());
            // If the placement is already the append position, no CAS is
            // needed (avoids a spurious revision bump).
            if lane.ordered_work_order_ids == order {
                return Some(TuiCommand::LaneReordered {
                    request_id,
                    route,
                    generation,
                    lane_id,
                    revision: lane.revision,
                    error: None,
                });
            }
            match core_client
                .request(crate::core::new_request(
                    format!("queue-place-reorder-{request_id}-{}", uuid::Uuid::new_v4()),
                    CoreRequest::WorkOrderLaneReorder {
                        request: crate::protocol::work_order::WorkOrderLaneReorderRequest {
                            lane_id: lane_id.clone(),
                            expected_revision: lane.revision,
                            ordered_work_order_ids: order,
                        },
                    },
                ))
                .await
            {
                Ok(CoreResponse::WorkOrderLane { lane }) => Some(TuiCommand::LaneReordered {
                    request_id,
                    route,
                    generation,
                    lane_id,
                    revision: lane.revision,
                    error: None,
                }),
                Ok(CoreResponse::Error { code, message }) => Some(TuiCommand::LaneReordered {
                    request_id,
                    route,
                    generation,
                    lane_id,
                    revision: lane.revision,
                    error: Some(format!("{code}: {message}")),
                }),
                Ok(_) => Some(TuiCommand::LaneReordered {
                    request_id,
                    route,
                    generation,
                    lane_id,
                    revision: lane.revision,
                    error: Some("Unexpected reorder response".to_string()),
                }),
                Err(error) => Some(TuiCommand::LaneReordered {
                    request_id,
                    route,
                    generation,
                    lane_id,
                    revision: lane.revision,
                    error: Some(error.to_string()),
                }),
            }
        },
    );
    if task_id.is_none() {
        let _ = app
            .dialog_state
            .work_order_reorder_request
            .fail(request_id, "TUI command channel unavailable".to_string());
    }
}

// ── External trigger (C001 §6.3–§6.6, §7) ───────────────────────────

/// Start one project-authorized `WorkOrderTriggerCreate` bound to the
/// WorkOrder's single external gate (`trigger_ref: None` binds
/// unambiguously; the draft always mints one `external` gate). The
/// idempotency key is bounded and deterministic so retry converges.
pub(crate) fn start_trigger_create(
    app: &mut App,
    project_id: String,
    work_order_id: String,
    trigger_ref: Option<String>,
    idempotency_key: Option<String>,
) {
    let request_id = app.dialog_state.trigger_create_request.begin();
    let Some(route) = capture_route(app, request_id) else {
        let _ = app
            .dialog_state
            .trigger_create_request
            .fail(request_id, "No active project tab".to_string());
        app.messages_state
            .toasts
            .error("Trigger setup needs an active project tab");
        return;
    };
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let scope = task_scope(&route);
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "trigger_create",
        scope.0,
        None,
        scope.1,
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::WorkOrderTriggerCreated {
                    request_id,
                    route,
                    project_id,
                    work_order_id,
                    trigger: None,
                    secret: None,
                    duplicate: false,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let request = crate::protocol::work_order::TaskTriggerCreateRequest {
                project_id: project_id.clone(),
                work_order_id: work_order_id.clone(),
                trigger_ref,
                expires_at_ms: None,
                max_fires: None,
                idempotency_key,
            };
            match core_client
                .request(crate::core::new_request(
                    format!("trigger-create-{request_id}-{}", uuid::Uuid::new_v4()),
                    CoreRequest::WorkOrderTriggerCreate { request },
                ))
                .await
            {
                Ok(CoreResponse::WorkOrderTrigger {
                    trigger,
                    secret,
                    duplicate,
                }) => Some(TuiCommand::WorkOrderTriggerCreated {
                    request_id,
                    route,
                    project_id,
                    work_order_id,
                    trigger: Some(trigger),
                    secret,
                    duplicate,
                    error: None,
                }),
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::WorkOrderTriggerCreated {
                        request_id,
                        route,
                        project_id,
                        work_order_id,
                        trigger: None,
                        secret: None,
                        duplicate: false,
                        error: Some(format!("{code}: {message}")),
                    })
                }
                Ok(_) => Some(TuiCommand::WorkOrderTriggerCreated {
                    request_id,
                    route,
                    project_id,
                    work_order_id,
                    trigger: None,
                    secret: None,
                    duplicate: false,
                    error: Some("Unexpected response while creating the trigger".to_string()),
                }),
                Err(error) => Some(TuiCommand::WorkOrderTriggerCreated {
                    request_id,
                    route,
                    project_id,
                    work_order_id,
                    trigger: None,
                    secret: None,
                    duplicate: false,
                    error: Some(error.to_string()),
                }),
            }
        },
    );
    if task_id.is_none() {
        let _ = app
            .dialog_state
            .trigger_create_request
            .fail(request_id, "TUI command channel unavailable".to_string());
        app.messages_state
            .toasts
            .error("TUI command channel unavailable; trigger was not created");
    }
}

/// Apply a trigger-create completion. Success shows the one-time bearer
/// exactly once (route-scoped); stale routes drop foreground display
/// without logging the secret and leave active metadata for rotation.
/// Failures retain the WorkOrder and surface setup-incomplete with a
/// retry path — never a delete, never a secret-read.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_trigger_created(
    app: &mut App,
    request_id: u64,
    route: UiRouteToken,
    project_id: String,
    work_order_id: String,
    trigger: Option<crate::protocol::work_order::TaskTriggerMetadataDto>,
    secret: Option<String>,
    duplicate: bool,
    error: Option<String>,
) {
    if let Some(error) = error {
        if app
            .dialog_state
            .trigger_create_request
            .fail(request_id, error.clone())
        {
            // C001 case C/D: transient/authorization error without commit
            // keeps the WorkOrder intact and offers idempotent retry.
            // Ambiguous timeouts should reconcile first; this path is for
            // explicit errors, so record setup-incomplete directly.
            app.dialog_state
                .trigger_setup_error
                .insert(work_order_id.clone(), error.clone());
            let Some(check) = current_task_route_check(app) else {
                return;
            };
            if route.matches(&check) && app.active_project_id() == Some(project_id.as_str()) {
                let short: String = work_order_id.chars().take(8).collect();
                app.messages_state.toasts.warning(&format!(
                    "Task {short}: trigger setup incomplete ({error}); press t in the Task view to retry"
                ));
                sync_task_view_dialog(app);
            } else {
                refresh_task_view_if_open(app, &project_id);
            }
        }
        return;
    }
    if !app.dialog_state.trigger_create_request.finish(request_id) {
        return;
    }
    let Some(trigger) = trigger else {
        app.dialog_state.trigger_setup_error.insert(
            work_order_id.clone(),
            "Trigger create returned no metadata".to_string(),
        );
        return;
    };
    // Cache metadata (never the secret) for Task-view status.
    app.dialog_state
        .trigger_metadata
        .entry(work_order_id.clone())
        .or_default()
        .retain(|existing| existing.trigger_id != trigger.trigger_id);
    app.dialog_state
        .trigger_metadata
        .entry(work_order_id.clone())
        .or_default()
        .push(trigger.clone());
    app.dialog_state.trigger_setup_error.remove(&work_order_id);
    let Some(check) = current_task_route_check(app) else {
        return;
    };
    // Stale route (case A/B): the trigger may have committed. Never log
    // the secret to make it recoverable; drop foreground display and
    // leave active metadata + explicit rotate.
    if !route.matches(&check) || app.active_project_id() != Some(project_id.as_str()) {
        refresh_task_view_if_open(app, &project_id);
        return;
    }
    if duplicate || secret.as_deref().map(str::trim).unwrap_or("").is_empty() {
        // Converged retry or lost one-time response: plaintext cannot be
        // reconstructed. Show active metadata and require explicit rotate
        // for a new bearer (never auto-revoke another bearer's credential).
        let short: String = work_order_id.chars().take(8).collect();
        app.messages_state.toasts.info(&format!(
            "Task {short}: trigger {} is active (secret shown only at creation); press T to rotate for a new bearer",
            short_trigger_id(&trigger.trigger_id)
        ));
        sync_task_view_dialog(app);
        refresh_task_view_if_open(app, &project_id);
        return;
    }
    let bearer = match OneTimeBearer::new(secret.unwrap_or_default()) {
        Ok(bearer) => bearer,
        Err(error) => {
            app.dialog_state
                .trigger_setup_error
                .insert(work_order_id.clone(), error.clone());
            app.messages_state.toasts.warning(&format!(
                "Trigger created but the one-time response was unusable ({error}); press T to rotate"
            ));
            sync_task_view_dialog(app);
            return;
        }
    };
    // Mount the transient secret-safe surface exactly once.
    let secret_state = OneTimeTriggerSecret {
        project_id: project_id.clone(),
        work_order_id: work_order_id.clone(),
        trigger_id: trigger.trigger_id.clone(),
        bearer,
        route: route.clone(),
    };
    app.dialog_state.trigger_secret = Some(secret_state);
    if let Some(secret_ref) = app.dialog_state.trigger_secret.as_ref() {
        use crate::tui::components::dialogs::trigger_secret::TriggerSecretDialog;
        let dialog = TriggerSecretDialog::from_secret(secret_ref);
        app.push_dialog(Dialog::TriggerSecret, Box::new(dialog));
    }
    sync_task_view_dialog(app);
}

/// Close the one-time secret dialog and forget the bearer. Metadata
/// stays listable; the secret is never re-readable.
pub(crate) fn close_trigger_secret(app: &mut App) {
    app.dialog_state.trigger_secret = None;
    // Pop only the secret dialog; leave the Task view underneath.
    app.focus_manager
        .pop_dialog(crate::tui::components::component::DialogType::TriggerSecret);
    let active = app.focus_manager.active_dialog_type();
    app.ui_state.dialog = crate::tui::app::Dialog::from(active);
}

/// Drop the transient bearer on scope loss (project/tab switch,
/// reconnect, logout/authority loss, shutdown). Never logs the secret;
/// later metadata shows configuration and offers rotation.
pub(crate) fn clear_trigger_secret_on_scope_loss(app: &mut App) {
    if app.dialog_state.trigger_secret.is_some() {
        app.dialog_state.trigger_secret = None;
        app.focus_manager
            .pop_dialog(crate::tui::components::component::DialogType::TriggerSecret);
        let active = app.focus_manager.active_dialog_type();
        app.ui_state.dialog = crate::tui::app::Dialog::from(active);
    }
    app.dialog_state.trigger_create_request.cancel();
}

fn short_trigger_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// Selected Task-view row's WorkOrder identity, if any.
fn selected_trigger_target(app: &App) -> Option<(String, String)> {
    let row = app.dialog_state.task_view.selected_row()?;
    let project_id = app
        .dialog_state
        .task_view
        .project_id
        .clone()
        .or_else(|| app.active_project_id().map(str::to_string))?;
    Some((project_id, row.work_order.work_order_id.clone()))
}

/// C001 §7 + §6.6 case D: list metadata first, then decide. If an active
/// trigger exists but the bearer is unavailable, require explicit rotate.
/// If none exists, retry creation with the original key. Never blindly
/// mint a second credential.
pub(crate) fn setup_trigger_for_selected(app: &mut App) {
    let Some((project_id, work_order_id)) = selected_trigger_target(app) else {
        app.messages_state.toasts.info("No task selected");
        return;
    };
    // If we already know an active trigger exists, don't create again —
    // point at rotation instead (prevents accidental second credential).
    if let Some(triggers) = app.dialog_state.trigger_metadata.get(&work_order_id) {
        if triggers.iter().any(|t| t.status == "active") {
            app.messages_state.toasts.info(
                "An active trigger already exists (secret shown only at creation); press T to rotate for a new bearer",
            );
            return;
        }
    }
    // Mark setup pending and reconcile via metadata first (ambiguous
    // timeout safe). The list completion drives creation only when no
    // active trigger exists.
    app.dialog_state
        .trigger_setup_pending
        .insert(work_order_id.clone());
    refresh_trigger_for_selected(app);
    let _ = project_id;
}

/// Fetch trigger metadata for the selected row (never secrets).
pub(crate) fn refresh_trigger_for_selected(app: &mut App) {
    let Some((project_id, work_order_id)) = selected_trigger_target(app) else {
        app.messages_state.toasts.info("No task selected");
        return;
    };
    let request_id = app.dialog_state.trigger_manage_request.begin();
    let Some(route) = capture_route(app, request_id) else {
        let _ = app
            .dialog_state
            .trigger_manage_request
            .fail(request_id, "No active project tab".to_string());
        return;
    };
    let generation = app.dialog_state.task_view.generation;
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let scope = task_scope(&route);
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "trigger_list",
        scope.0,
        None,
        scope.1,
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::WorkOrderTriggersListed {
                    request_id,
                    route,
                    generation,
                    project_id,
                    work_order_id,
                    triggers: Vec::new(),
                    truncated: false,
                    error: Some("Core unavailable".to_string()),
                });
            };
            match core_client
                .request(crate::core::new_request(
                    format!("trigger-list-{request_id}-{}", uuid::Uuid::new_v4()),
                    CoreRequest::WorkOrderTriggerList {
                        request: crate::protocol::work_order::TaskTriggerListRequest {
                            project_id: project_id.clone(),
                            work_order_id: Some(work_order_id.clone()),
                            limit: Some(20),
                        },
                    },
                ))
                .await
            {
                Ok(CoreResponse::WorkOrderTriggerList {
                    triggers,
                    truncated,
                }) => Some(TuiCommand::WorkOrderTriggersListed {
                    request_id,
                    route,
                    generation,
                    project_id,
                    work_order_id,
                    triggers,
                    truncated,
                    error: None,
                }),
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::WorkOrderTriggersListed {
                        request_id,
                        route,
                        generation,
                        project_id,
                        work_order_id,
                        triggers: Vec::new(),
                        truncated: false,
                        error: Some(format!("{code}: {message}")),
                    })
                }
                Ok(_) => Some(TuiCommand::WorkOrderTriggersListed {
                    request_id,
                    route,
                    generation,
                    project_id,
                    work_order_id,
                    triggers: Vec::new(),
                    truncated: false,
                    error: Some("Unexpected trigger list response".to_string()),
                }),
                Err(error) => Some(TuiCommand::WorkOrderTriggersListed {
                    request_id,
                    route,
                    generation,
                    project_id,
                    work_order_id,
                    triggers: Vec::new(),
                    truncated: false,
                    error: Some(error.to_string()),
                }),
            }
        },
    );
    if task_id.is_none() {
        let _ = app
            .dialog_state
            .trigger_manage_request
            .fail(request_id, "TUI command channel unavailable".to_string());
    }
}

/// Apply a trigger-list completion: cache metadata (never secrets),
/// clear or set setup-incomplete, and refresh the Task-view labels.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_triggers_listed(
    app: &mut App,
    request_id: u64,
    route: UiRouteToken,
    generation: u64,
    project_id: String,
    work_order_id: String,
    triggers: Vec<crate::protocol::work_order::TaskTriggerMetadataDto>,
    _truncated: bool,
    error: Option<String>,
) {
    if let Some(error) = error {
        if app
            .dialog_state
            .trigger_manage_request
            .fail(request_id, error.clone())
        {
            app.messages_state.toasts.warning(&error);
        }
        return;
    }
    if !app.dialog_state.trigger_manage_request.finish(request_id) {
        return;
    }
    if app.dialog_state.task_view.generation != generation {
        return;
    }
    let Some(check) = current_task_route_check(app) else {
        return;
    };
    if !route.matches(&check) {
        return;
    }
    if app.dialog_state.task_view.project_id.as_deref() != Some(project_id.as_str()) {
        // Still cache for later foregrounding.
    }
    if triggers.is_empty() {
        app.dialog_state.trigger_metadata.remove(&work_order_id);
    } else {
        app.dialog_state
            .trigger_metadata
            .insert(work_order_id.clone(), triggers.clone());
        app.dialog_state.trigger_setup_error.remove(&work_order_id);
    }
    sync_task_view_dialog(app);
    // Pending setup reconciliation (case D): create only when no active
    // trigger exists; otherwise require explicit rotate.
    if app
        .dialog_state
        .trigger_setup_pending
        .remove(&work_order_id)
    {
        if triggers.iter().any(|t| t.status == "active") {
            app.messages_state.toasts.info(
                "An active trigger already exists (secret shown only at creation); press T to rotate for a new bearer",
            );
            return;
        }
        // No durable trigger: retry creation with the stable key so
        // concurrent setups converge to one credential.
        let stable_key = format!(
            "tui-setup-{}",
            work_order_id.chars().take(100).collect::<String>()
        );
        start_trigger_create(
            app,
            project_id.clone(),
            work_order_id.clone(),
            None,
            Some(stable_key),
        );
    }
}

/// Revoke the active trigger for the selected row (explicit user
/// mutation; idempotent/monotonic, fail-closed on authority loss).
pub(crate) fn revoke_trigger_for_selected(app: &mut App) {
    let Some((project_id, work_order_id)) = selected_trigger_target(app) else {
        app.messages_state.toasts.info("No task selected");
        return;
    };
    let Some(trigger_id) = app
        .dialog_state
        .trigger_metadata
        .get(&work_order_id)
        .and_then(|triggers| triggers.iter().find(|t| t.status == "active"))
        .map(|t| t.trigger_id.clone())
    else {
        app.messages_state.toasts.info(
            "No active trigger to revoke (refresh with e to reconcile, or set one up with t)",
        );
        return;
    };
    let request_id = app.dialog_state.trigger_manage_request.begin();
    let Some(route) = capture_route(app, request_id) else {
        let _ = app
            .dialog_state
            .trigger_manage_request
            .fail(request_id, "No active project tab".to_string());
        return;
    };
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let scope = task_scope(&route);
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "trigger_revoke",
        scope.0,
        None,
        scope.1,
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::WorkOrderTriggerRevoked {
                    request_id,
                    route,
                    project_id,
                    work_order_id,
                    trigger_id,
                    trigger: None,
                    error: Some("Core unavailable".to_string()),
                });
            };
            match core_client
                .request(crate::core::new_request(
                    format!("trigger-revoke-{request_id}-{}", uuid::Uuid::new_v4()),
                    CoreRequest::WorkOrderTriggerRevoke {
                        trigger_id: trigger_id.clone(),
                    },
                ))
                .await
            {
                Ok(CoreResponse::WorkOrderTrigger { trigger, .. }) => {
                    Some(TuiCommand::WorkOrderTriggerRevoked {
                        request_id,
                        route,
                        project_id,
                        work_order_id,
                        trigger_id,
                        trigger: Some(trigger),
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::WorkOrderTriggerRevoked {
                        request_id,
                        route,
                        project_id,
                        work_order_id,
                        trigger_id,
                        trigger: None,
                        error: Some(format!("{code}: {message}")),
                    })
                }
                Ok(_) => Some(TuiCommand::WorkOrderTriggerRevoked {
                    request_id,
                    route,
                    project_id,
                    work_order_id,
                    trigger_id,
                    trigger: None,
                    error: Some("Unexpected revoke response".to_string()),
                }),
                Err(error) => Some(TuiCommand::WorkOrderTriggerRevoked {
                    request_id,
                    route,
                    project_id,
                    work_order_id,
                    trigger_id,
                    trigger: None,
                    error: Some(error.to_string()),
                }),
            }
        },
    );
    if task_id.is_none() {
        let _ = app
            .dialog_state
            .trigger_manage_request
            .fail(request_id, "TUI command channel unavailable".to_string());
    }
}

/// Apply a revoke completion: update cached metadata and Task-view
/// labels. Never touches a bearer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_trigger_revoked(
    app: &mut App,
    request_id: u64,
    route: UiRouteToken,
    project_id: String,
    work_order_id: String,
    trigger_id: String,
    trigger: Option<crate::protocol::work_order::TaskTriggerMetadataDto>,
    error: Option<String>,
) {
    if let Some(error) = error {
        if app
            .dialog_state
            .trigger_manage_request
            .fail(request_id, error.clone())
        {
            app.messages_state.toasts.warning(&error);
        }
        return;
    }
    if !app.dialog_state.trigger_manage_request.finish(request_id) {
        return;
    }
    let Some(check) = current_task_route_check(app) else {
        return;
    };
    if !route.matches(&check) {
        return;
    }
    if let Some(updated) = trigger {
        app.dialog_state
            .trigger_metadata
            .entry(work_order_id.clone())
            .or_default()
            .retain(|existing| existing.trigger_id != trigger_id);
        app.dialog_state
            .trigger_metadata
            .entry(work_order_id.clone())
            .or_default()
            .push(updated);
    } else {
        if let Some(list) = app.dialog_state.trigger_metadata.get_mut(&work_order_id) {
            list.retain(|existing| existing.trigger_id != trigger_id);
        }
    }
    let _ = project_id;
    app.messages_state.toasts.info("Trigger revoked");
    sync_task_view_dialog(app);
    refresh_task_view_if_open(
        app,
        &app.dialog_state
            .task_view
            .project_id
            .clone()
            .unwrap_or_default(),
    );
}

/// Explicit rotation (C001 §7): revoke the old trigger (idempotent)
/// then create a replacement bound to the same external gate, showing
/// only the new bearer. The human must choose it; lost responses never
/// auto-revoke.
pub(crate) fn rotate_trigger_for_selected(app: &mut App) {
    let Some((project_id, work_order_id)) = selected_trigger_target(app) else {
        app.messages_state.toasts.info("No task selected");
        return;
    };
    let active = app
        .dialog_state
        .trigger_metadata
        .get(&work_order_id)
        .and_then(|triggers| triggers.iter().find(|t| t.status == "active"))
        .cloned();
    // No active trigger: setup covers it (create/retry, not rotate).
    let Some(active) = active else {
        setup_trigger_for_selected(app);
        return;
    };
    // Rotation reports via WorkOrderTriggerCreated, so it owns a
    // trigger_create_request id (not manage). Revoke + create happen in
    // one task; only the new bearer is ever displayed.
    let request_id = app.dialog_state.trigger_create_request.begin();
    let Some(route) = capture_route(app, request_id) else {
        let _ = app
            .dialog_state
            .trigger_create_request
            .fail(request_id, "No active project tab".to_string());
        return;
    };
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let scope = task_scope(&route);
    let rotation_key = trigger_rotation_key(request_id, &work_order_id);
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "trigger_rotate",
        scope.0,
        None,
        scope.1,
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::WorkOrderTriggerCreated {
                    request_id,
                    route,
                    project_id,
                    work_order_id,
                    trigger: None,
                    secret: None,
                    duplicate: false,
                    error: Some("Core unavailable".to_string()),
                });
            };
            // Step 1: revoke old (monotonic, idempotent).
            let revoke_result: Result<(), String> = match core_client
                .request(crate::core::new_request(
                    format!(
                        "trigger-rotate-revoke-{request_id}-{}",
                        uuid::Uuid::new_v4()
                    ),
                    CoreRequest::WorkOrderTriggerRevoke {
                        trigger_id: active.trigger_id.clone(),
                    },
                ))
                .await
            {
                Ok(CoreResponse::WorkOrderTrigger { .. }) => Ok(()),
                Ok(CoreResponse::Error { code, message }) => Err(format!("{code}: {message}")),
                Ok(_) => Err("Unexpected revoke response".to_string()),
                Err(error) => Err(error.to_string()),
            };
            if let Err(error) = revoke_result {
                return Some(TuiCommand::WorkOrderTriggerCreated {
                    request_id,
                    route,
                    project_id,
                    work_order_id,
                    trigger: None,
                    secret: None,
                    duplicate: false,
                    error: Some(format!("Rotation revoke failed: {error}")),
                });
            }
            // Step 2: create replacement with a fresh rotation key.
            let request = crate::protocol::work_order::TaskTriggerCreateRequest {
                project_id: project_id.clone(),
                work_order_id: work_order_id.clone(),
                trigger_ref: None,
                expires_at_ms: None,
                max_fires: None,
                idempotency_key: Some(rotation_key),
            };
            match core_client
                .request(crate::core::new_request(
                    format!(
                        "trigger-rotate-create-{request_id}-{}",
                        uuid::Uuid::new_v4()
                    ),
                    CoreRequest::WorkOrderTriggerCreate { request },
                ))
                .await
            {
                Ok(CoreResponse::WorkOrderTrigger {
                    trigger,
                    secret,
                    duplicate,
                }) => Some(TuiCommand::WorkOrderTriggerCreated {
                    request_id,
                    route,
                    project_id,
                    work_order_id,
                    trigger: Some(trigger),
                    secret,
                    duplicate,
                    error: None,
                }),
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::WorkOrderTriggerCreated {
                        request_id,
                        route,
                        project_id,
                        work_order_id,
                        trigger: None,
                        secret: None,
                        duplicate: false,
                        error: Some(format!("{code}: {message}")),
                    })
                }
                Ok(_) => Some(TuiCommand::WorkOrderTriggerCreated {
                    request_id,
                    route,
                    project_id,
                    work_order_id,
                    trigger: None,
                    secret: None,
                    duplicate: false,
                    error: Some("Unexpected rotation create response".to_string()),
                }),
                Err(error) => Some(TuiCommand::WorkOrderTriggerCreated {
                    request_id,
                    route,
                    project_id,
                    work_order_id,
                    trigger: None,
                    secret: None,
                    duplicate: false,
                    error: Some(error.to_string()),
                }),
            }
        },
    );
    if task_id.is_none() {
        let _ = app
            .dialog_state
            .trigger_create_request
            .fail(request_id, "TUI command channel unavailable".to_string());
        app.messages_state
            .toasts
            .error("TUI command channel unavailable; rotation was not started");
    }
}

/// Compute Task-view trigger labels from cached metadata + gates +
/// setup errors (metadata only, never secrets).
pub(crate) fn trigger_labels_for_view(app: &App) -> std::collections::HashMap<String, String> {
    let mut labels = std::collections::HashMap::new();
    for row in &app.dialog_state.task_view.rows {
        let wo_id = row.work_order.work_order_id.clone();
        let has_external_gate = row
            .work_order
            .gates
            .iter()
            .any(|gate| gate.kind == "external_trigger");
        if let Some(triggers) = app.dialog_state.trigger_metadata.get(&wo_id) {
            if let Some(active) = triggers.iter().find(|t| t.status == "active") {
                let short: String = active.trigger_id.chars().take(8).collect();
                let max = active
                    .max_fires
                    .map(|m| format!("/{m}"))
                    .unwrap_or_default();
                labels.insert(
                    wo_id.clone(),
                    format!(
                        "trigger {} · {} · {}{} fires",
                        TriggerSetupStatus::Active.label(),
                        short,
                        active.fire_count,
                        max
                    ),
                );
                continue;
            }
            if let Some(first) = triggers.first() {
                let status = TriggerSetupStatus::from_metadata_status(&first.status);
                let short: String = first.trigger_id.chars().take(8).collect();
                labels.insert(
                    wo_id.clone(),
                    format!(
                        "trigger {} · {short} — press t to set up a new bearer",
                        status.label()
                    ),
                );
                continue;
            }
        }
        if let Some(error) = app.dialog_state.trigger_setup_error.get(&wo_id) {
            let hint: String = error.chars().take(60).collect();
            labels.insert(
                wo_id.clone(),
                format!("trigger setup incomplete ({hint}) — press t to retry"),
            );
            continue;
        }
        if has_external_gate {
            labels.insert(
                wo_id.clone(),
                "trigger not configured — press t to set up".to_string(),
            );
        }
    }
    labels
}

/// Refresh the Task view only when it is the open dialog for
/// `project_id`. Never foregrounds the view on a stale completion.
fn refresh_task_view_if_open(app: &mut App, project_id: &str) {
    if app.ui_state.dialog != Dialog::TaskView {
        return;
    }
    if app.dialog_state.task_view.project_id.as_deref() != Some(project_id) {
        return;
    }
    refresh_task_view(app);
}

fn apply_task_create_failure(app: &mut App, pending: PendingTaskCreate, error: &str) {
    app.prompt_state.pending_task_create = None;
    app.prompt_state.task_submit_started = false;
    // The prompt was never cleared or submitted to a transcript, so
    // "restore exactly once" keeps the user's text: stash any newer
    // typing, then put the confirmed text back.
    let current_draft = app.prompt_state.prompt.get_text();
    if !current_draft.trim().is_empty() && current_draft != pending.prompt {
        if app.prompt_state.stashed_prompts.len() >= 100 {
            app.prompt_state.stashed_prompts.remove(0);
        }
        app.prompt_state.stashed_prompts.push(current_draft);
    }
    app.prompt_state.prompt.set_text(pending.prompt.clone());
    app.prompt_state.prompt.set_waiting(false);
    app.prompt_state.pending_send = false;
    app.session_state.session_status = crate::tui::app::SessionStatus::Error;
    app.messages_state
        .toasts
        .error(&format!("Task creation failed: {error}"));
}

fn push_prompt_history(app: &mut App, text: &str) {
    use crate::tui::app::HistoryEntry;
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return;
    }
    if let Some(pos) = app
        .session_state
        .history
        .iter()
        .position(|e| e.text == trimmed)
    {
        app.session_state.history[pos].touch();
    } else {
        if app.session_state.history.len() >= 1000 {
            app.session_state.history.pop_front();
        }
        app.session_state
            .history
            .push_back(HistoryEntry::new(trimmed));
    }
    let mut sorted: Vec<HistoryEntry> = app.session_state.history.iter().cloned().collect();
    sorted.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    app.session_state.history = sorted.into();
    app.session_state.history_pos = None;
}

/// Persist the snapshotted Task model through the canonical
/// daemon-owned settings service. Best-effort: failures warn only and
/// never touch the already-created WorkOrder or the ordinary session
/// preference.
fn persist_task_model_best_effort(app: &mut App, model: Option<String>) {
    let Some(model) = model
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
    else {
        return;
    };
    let request_id = app.dialog_state.task_model_pref_request.begin();
    let expected_revision = app
        .prompt_state
        .task_model_choice
        .as_ref()
        .map(|choice| choice.revision);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let route = capture_route(app, request_id);
    let scope_tab = route
        .as_ref()
        .and_then(|route| route.tab_id.clone())
        .map(|tab| tab.as_str().to_string());
    let scope_epoch = route.as_ref().map(|route| route.active_view_epoch);
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "task_model_persist",
        scope_tab,
        None,
        scope_epoch,
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TaskModelPrefSaved {
                    request_id,
                    error: Some("Core unavailable; Task model was not remembered".to_string()),
                });
            };
            match core_client
                .request(crate::core::new_request(
                    format!("task-model-save-{request_id}-{}", uuid::Uuid::new_v4()),
                    CoreRequest::TaskModelPreferenceSet {
                        connection_id: None,
                        model_id: Some(model),
                        expected_revision,
                    },
                ))
                .await
            {
                Ok(CoreResponse::ApprovalPreference { preference }) => {
                    let _ = preference;
                    Some(TuiCommand::TaskModelPrefSaved {
                        request_id,
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => Some(TuiCommand::TaskModelPrefSaved {
                    request_id,
                    error: Some(format!("{code}: {message}")),
                }),
                Ok(_) => Some(TuiCommand::TaskModelPrefSaved {
                    request_id,
                    error: Some("Unexpected response while saving Task model".to_string()),
                }),
                Err(error) => Some(TuiCommand::TaskModelPrefSaved {
                    request_id,
                    error: Some(error.to_string()),
                }),
            }
        },
    );
    if task_id.is_none() {
        let _ = app
            .dialog_state
            .task_model_pref_request
            .fail(request_id, "TUI command channel unavailable".to_string());
    }
}

/// Apply the best-effort Task-model persist. Warn-only; never touches
/// the prompt, the transcript, or the created WorkOrder.
pub(crate) fn apply_task_model_pref_saved(app: &mut App, request_id: u64, error: Option<String>) {
    if let Some(error) = error {
        if app
            .dialog_state
            .task_model_pref_request
            .fail(request_id, error.clone())
        {
            app.messages_state.toasts.warning(&format!(
                "Task created, but the Task model was not remembered: {error}"
            ));
        }
        return;
    }
    let _ = app.dialog_state.task_model_pref_request.finish(request_id);
}

/// Forward sheet navigation to the mounted sheet component.
pub(crate) fn move_sheet_selection(app: &mut App, delta: isize) {
    use crate::tui::components::dialogs::task_schedule::TaskScheduleDialog;
    if let Some(dialog) = app.focus_manager.dialog_mut_any::<TaskScheduleDialog>() {
        dialog.move_focus(delta);
    }
}

fn sync_sheet_error(app: &mut App, error: &str) {
    use crate::tui::components::dialogs::task_schedule::TaskScheduleDialog;
    let error = error.to_string();
    if let Some(dialog) = app.focus_manager.dialog_mut_any::<TaskScheduleDialog>() {
        dialog.set_error(error);
    }
}

// ── Project Task view ────────────────────────────────────────────────

/// Open (or foreground) the project Task view and refresh its bounded
/// projection. One `WorkOrderList` + one `WorkOrderSummary` + one
/// `WorkOrderLaneList`; occurrence detail stays lazy per selected row.
pub(crate) fn open_task_view(app: &mut App) {
    let context = match app.project_execution_context() {
        Ok(context) => context,
        Err(error) => {
            app.messages_state.toasts.error(&error);
            return;
        }
    };
    let Some(project_id) = context.project_id.clone() else {
        app.messages_state
            .toasts
            .error("Task view needs an active project tab; choose a project first");
        return;
    };
    ensure_task_view_dialog(app, &project_id);
    start_task_view_refresh(app, project_id);
}

/// Refresh the visible Task view projection (no-op when the view is
/// not bound to a project).
pub(crate) fn refresh_task_view(app: &mut App) {
    let Some(project_id) = app
        .dialog_state
        .task_view
        .project_id
        .clone()
        .or_else(|| app.active_project_id().map(str::to_string))
    else {
        return;
    };
    ensure_task_view_dialog(app, &project_id);
    start_task_view_refresh(app, project_id);
}

fn ensure_task_view_dialog(app: &mut App, project_id: &str) {
    use crate::tui::components::dialogs::task_view::TaskViewDialog;
    app.dialog_state.task_view.project_id = Some(project_id.to_string());
    if app.ui_state.dialog != Dialog::TaskView {
        let dialog = TaskViewDialog::new(project_id.to_string());
        app.push_dialog(Dialog::TaskView, Box::new(dialog));
    }
    sync_task_view_dialog(app);
}

fn start_task_view_refresh(app: &mut App, project_id: String) {
    let request_id = app.dialog_state.work_order_list_request.begin();
    let Some(route) = capture_route(app, request_id) else {
        let _ = app
            .dialog_state
            .work_order_list_request
            .fail(request_id, "No active project tab".to_string());
        app.dialog_state.task_view.loading = false;
        return;
    };
    let generation = app.dialog_state.task_view.begin_refresh(&project_id);
    sync_task_view_dialog(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let scope = task_scope(&route);
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "task_view_refresh",
        scope.0,
        None,
        scope.1,
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TaskViewRefreshed {
                    request_id,
                    route,
                    generation,
                    project_id,
                    work_orders: Vec::new(),
                    summary: None,
                    lanes: Vec::new(),
                    capabilities_supported: false,
                    truncated: false,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            match fetch_task_view(core_client.as_ref(), &project_id).await {
                Ok((work_orders, summary, lanes, capabilities_supported, truncated)) => {
                    Some(TuiCommand::TaskViewRefreshed {
                        request_id,
                        route,
                        generation,
                        project_id,
                        work_orders,
                        summary,
                        lanes,
                        capabilities_supported,
                        truncated,
                        error: None,
                    })
                }
                Err(error) => Some(TuiCommand::TaskViewRefreshed {
                    request_id,
                    route,
                    generation,
                    project_id,
                    work_orders: Vec::new(),
                    summary: None,
                    lanes: Vec::new(),
                    capabilities_supported: false,
                    truncated: false,
                    error: Some(error),
                }),
            }
        },
    );
    if task_id.is_none() {
        let _ = app
            .dialog_state
            .work_order_list_request
            .fail(request_id, "TUI command channel unavailable".to_string());
        app.dialog_state.task_view.loading = false;
        app.dialog_state.task_view.error = Some("TUI command channel unavailable".to_string());
        sync_task_view_dialog(app);
    }
}

async fn fetch_task_view(
    core_client: &dyn CoreClient,
    project_id: &str,
) -> Result<
    (
        Vec<WorkOrderDto>,
        Option<WorkOrderSummaryDto>,
        Vec<SequenceLaneDto>,
        bool,
        bool,
    ),
    String,
> {
    let capabilities_supported = match core_client
        .request(crate::core::new_request(
            format!("wo-view-cap-{}", uuid::Uuid::new_v4()),
            CoreRequest::WorkOrderCapabilities,
        ))
        .await
    {
        Ok(CoreResponse::WorkOrderCapabilities { capabilities }) => capabilities.supported,
        Ok(CoreResponse::Error { code, message }) => {
            return Err(format!(
                "WorkOrder capability check failed: {code}: {message}"
            ));
        }
        Ok(_) => return Err("Unexpected capability response".to_string()),
        Err(error) => return Err(error.to_string()),
    };
    if !capabilities_supported {
        return Err(
            "This server has no WorkOrder capability; the Task view needs a current daemon"
                .to_string(),
        );
    }
    let (work_orders, truncated) = match core_client
        .request(crate::core::new_request(
            format!("wo-view-list-{}", uuid::Uuid::new_v4()),
            CoreRequest::WorkOrderList {
                project_id: project_id.to_string(),
                state_filter: None,
                cursor: None,
                limit: Some(MAX_TASK_VIEW_ROWS as u32),
            },
        ))
        .await
    {
        Ok(CoreResponse::WorkOrderList {
            work_orders,
            truncated,
            ..
        }) => (work_orders, truncated),
        Ok(CoreResponse::Error { code, message }) => {
            return Err(format!("Task list failed: {code}: {message}"));
        }
        Ok(_) => return Err("Unexpected task list response".to_string()),
        Err(error) => return Err(error.to_string()),
    };
    let summary = match core_client
        .request(crate::core::new_request(
            format!("wo-view-summary-{}", uuid::Uuid::new_v4()),
            CoreRequest::WorkOrderSummary {
                project_id: project_id.to_string(),
            },
        ))
        .await
    {
        Ok(CoreResponse::WorkOrderSummary { summary }) => Some(summary),
        Ok(_) | Err(_) => None,
    };
    let lanes = match core_client
        .request(crate::core::new_request(
            format!("wo-view-lanes-{}", uuid::Uuid::new_v4()),
            CoreRequest::WorkOrderLaneList {
                project_id: project_id.to_string(),
                limit: Some(20),
            },
        ))
        .await
    {
        Ok(CoreResponse::WorkOrderLaneList { lanes, .. }) => lanes,
        Ok(_) | Err(_) => Vec::new(),
    };
    Ok((work_orders, summary, lanes, true, truncated))
}

/// Apply a Task view refresh with generation + route guards. Selection
/// is preserved by work-order id when the row survives; otherwise it
/// clamps into the new flat order.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_task_view_refreshed(
    app: &mut App,
    request_id: u64,
    route: UiRouteToken,
    generation: u64,
    project_id: String,
    work_orders: Vec<WorkOrderDto>,
    summary: Option<WorkOrderSummaryDto>,
    lanes: Vec<SequenceLaneDto>,
    capabilities_supported: bool,
    truncated: bool,
    error: Option<String>,
) {
    if let Some(error) = error {
        if app
            .dialog_state
            .work_order_list_request
            .fail(request_id, error.clone())
        {
            // Only surface the error when this refresh is still the
            // live view for its project.
            if app.dialog_state.task_view.generation == generation
                && app.dialog_state.task_view.project_id.as_deref() == Some(project_id.as_str())
            {
                app.dialog_state.task_view.loading = false;
                app.dialog_state.task_view.error = Some(error.clone());
                sync_task_view_dialog(app);
                if app.ui_state.dialog == Dialog::TaskView {
                    app.messages_state.toasts.warning(&error);
                }
            }
        }
        return;
    }
    if !app.dialog_state.work_order_list_request.finish(request_id) {
        return;
    }
    if app.dialog_state.task_view.generation != generation {
        return;
    }
    let Some(check) = current_task_route_check(app) else {
        return;
    };
    if !route.matches(&check) {
        return;
    }
    if app.dialog_state.task_view.project_id.as_deref() != Some(project_id.as_str()) {
        return;
    }
    let previous_selected_id = app
        .dialog_state
        .task_view
        .selected_row()
        .map(|row| row.work_order.work_order_id.clone());
    // Preserve lazily-fetched occurrence detail across refreshes for
    // rows that survive.
    let mut previous_occurrences = std::collections::HashMap::new();
    for row in &app.dialog_state.task_view.rows {
        if let Some(occurrence) = row.occurrence.clone() {
            previous_occurrences.insert(row.work_order.work_order_id.clone(), occurrence);
        }
    }
    let mut rows: Vec<TaskViewRow> = work_orders
        .into_iter()
        .take(MAX_TASK_VIEW_ROWS)
        .map(|work_order| {
            let occurrence = previous_occurrences
                .remove(&work_order.work_order_id)
                .filter(|occurrence| occurrence.work_order_id == work_order.work_order_id);
            TaskViewRow {
                work_order,
                occurrence,
            }
        })
        .collect();
    let _ = &mut rows;
    app.dialog_state.task_view.rows = rows;
    app.dialog_state.task_view.summary = summary;
    app.dialog_state.task_view.lanes = lanes;
    app.dialog_state.task_view.capabilities_supported = capabilities_supported;
    app.dialog_state.task_view.loading = false;
    app.dialog_state.task_view.error = None;
    app.dialog_state.task_view.notice = if truncated {
        Some(format!(
            "Showing the first {MAX_TASK_VIEW_ROWS} tasks; refine or cancel older work to see more"
        ))
    } else {
        None
    };
    // Restore selection by id, else clamp.
    if let Some(selected_id) = previous_selected_id {
        if let Some(pos) = app
            .dialog_state
            .task_view
            .rows
            .iter()
            .position(|row| row.work_order.work_order_id == selected_id)
        {
            app.dialog_state.task_view.selected = pos;
        } else {
            clamp_task_view_selection(app);
        }
    } else {
        clamp_task_view_selection(app);
    }
    sync_task_view_dialog(app);
}

fn clamp_task_view_selection(app: &mut App) {
    let order = app.dialog_state.task_view.flat_order();
    if order.is_empty() {
        app.dialog_state.task_view.selected = 0;
        return;
    }
    if !order.contains(&app.dialog_state.task_view.selected) {
        app.dialog_state.task_view.selected = order[0];
    }
}

/// Move the Task-view selection over the flat section order.
pub(crate) fn move_task_view_selection(app: &mut App, delta: isize) {
    app.dialog_state.task_view.move_selection(delta);
    app.dialog_state.task_view.scroll_offset = app
        .dialog_state
        .task_view
        .scroll_offset
        .max(app.dialog_state.task_view.selected.saturating_sub(20));
    sync_task_view_dialog(app);
}

fn sync_task_view_dialog(app: &mut App) {
    use crate::tui::components::dialogs::task_view::TaskViewDialog;
    let labels = trigger_labels_for_view(app);
    let snapshot = crate::tui::components::dialogs::task_view::TaskViewSnapshot::from_state(
        &app.dialog_state.task_view,
    )
    .with_trigger_labels(labels);
    if let Some(dialog) = app.focus_manager.dialog_mut_any::<TaskViewDialog>() {
        dialog.set_snapshot(snapshot);
    }
}

// ── Lazy occurrence detail ───────────────────────────────────────────

/// Fetch occurrence detail for the selected row only (never N+1).
pub(crate) fn fetch_selected_task_detail(app: &mut App) {
    let Some(row) = app.dialog_state.task_view.selected_row().cloned() else {
        app.messages_state.toasts.info("No task selected");
        return;
    };
    let work_order_id = row.work_order.work_order_id.clone();
    let generation = app.dialog_state.task_view.generation;
    let request_id = app.dialog_state.work_order_occurrence_request.begin();
    let Some(route) = capture_route(app, request_id) else {
        let _ = app
            .dialog_state
            .work_order_occurrence_request
            .fail(request_id, "No active project tab".to_string());
        return;
    };
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let scope = task_scope(&route);
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "task_occurrence_detail",
        scope.0,
        None,
        scope.1,
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TaskOccurrenceLoaded {
                    request_id,
                    route,
                    generation,
                    work_order_id,
                    occurrence: None,
                    error: Some("Core unavailable".to_string()),
                });
            };
            match core_client
                .request(crate::core::new_request(
                    format!("wo-occ-{}", uuid::Uuid::new_v4()),
                    CoreRequest::WorkOrderOccurrenceList {
                        work_order_id: work_order_id.clone(),
                        limit: Some(10),
                    },
                ))
                .await
            {
                Ok(CoreResponse::WorkOrderOccurrenceList { occurrences, .. }) => {
                    let latest = occurrences.into_iter().max_by_key(|o| o.occurrence_index);
                    Some(TuiCommand::TaskOccurrenceLoaded {
                        request_id,
                        route,
                        generation,
                        work_order_id,
                        occurrence: latest,
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::TaskOccurrenceLoaded {
                        request_id,
                        route,
                        generation,
                        work_order_id,
                        occurrence: None,
                        error: Some(format!("{code}: {message}")),
                    })
                }
                Ok(_) => Some(TuiCommand::TaskOccurrenceLoaded {
                    request_id,
                    route,
                    generation,
                    work_order_id,
                    occurrence: None,
                    error: Some("Unexpected occurrence response".to_string()),
                }),
                Err(error) => Some(TuiCommand::TaskOccurrenceLoaded {
                    request_id,
                    route,
                    generation,
                    work_order_id,
                    occurrence: None,
                    error: Some(error.to_string()),
                }),
            }
        },
    );
    if task_id.is_none() {
        let _ = app
            .dialog_state
            .work_order_occurrence_request
            .fail(request_id, "TUI command channel unavailable".to_string());
    }
}

/// Apply lazy occurrence detail with generation + route guards.
pub(crate) fn apply_occurrence_loaded(
    app: &mut App,
    request_id: u64,
    route: UiRouteToken,
    generation: u64,
    work_order_id: String,
    occurrence: Option<crate::protocol::work_order::WorkOrderOccurrenceDto>,
    error: Option<String>,
) {
    if let Some(error) = error {
        if app
            .dialog_state
            .work_order_occurrence_request
            .fail(request_id, error.clone())
        {
            app.messages_state.toasts.warning(&error);
        }
        return;
    }
    if !app
        .dialog_state
        .work_order_occurrence_request
        .finish(request_id)
    {
        return;
    }
    if app.dialog_state.task_view.generation != generation {
        return;
    }
    let Some(check) = current_task_route_check(app) else {
        return;
    };
    if !route.matches(&check) {
        return;
    }
    if let Some(row) = app
        .dialog_state
        .task_view
        .rows
        .iter_mut()
        .find(|row| row.work_order.work_order_id == work_order_id)
    {
        row.occurrence = occurrence;
    }
    sync_task_view_dialog(app);
}

// ── Reorder (CAS) ────────────────────────────────────────────────────

/// Reorder the selected waiting task within its lane by `delta`
/// positions (Shift+J/K convention). Only waiting/unclaimed lane
/// members are movable; the pinned running/claimed predecessor never
/// moves. Every move carries the expected lane revision; conflicts
/// refresh the lane with "queue changed; retry".
pub(crate) fn start_lane_reorder(app: &mut App, delta: isize) {
    let Some(row) = app.dialog_state.task_view.selected_row().cloned() else {
        app.messages_state.toasts.info("No task selected");
        return;
    };
    if row.is_running() {
        app.messages_state
            .toasts
            .warning("Running tasks are pinned; only waiting tasks can be reordered");
        return;
    }
    if row.is_terminal() {
        app.messages_state
            .toasts
            .warning("Terminal tasks cannot be reordered");
        return;
    }
    let work_order_id = row.work_order.work_order_id.clone();
    let Some(lane) = app
        .dialog_state
        .task_view
        .lanes
        .iter()
        .find(|lane| lane.ordered_work_order_ids.contains(&work_order_id))
        .cloned()
    else {
        app.messages_state
            .toasts
            .warning("Selected task is not in a sequence lane; enable sequential ordering first");
        return;
    };
    // Pinned head: the first member when it has a running/claimed
    // occurrence cached; otherwise no pin.
    let pinned_head: Option<String> = lane
        .ordered_work_order_ids
        .first()
        .filter(|first| {
            app.dialog_state
                .task_view
                .rows
                .iter()
                .find(|row| &row.work_order.work_order_id == *first)
                .is_some_and(|row| row.is_running())
        })
        .cloned();
    let Some(next_order) = TaskViewState::moved_lane_order(
        &lane.ordered_work_order_ids,
        pinned_head.as_deref(),
        &work_order_id,
        delta,
    ) else {
        app.messages_state
            .toasts
            .info("Task cannot move there (lane head is pinned or at the edge)");
        return;
    };
    let generation = app.dialog_state.task_view.generation;
    let request_id = app.dialog_state.work_order_reorder_request.begin();
    let Some(route) = capture_route(app, request_id) else {
        let _ = app
            .dialog_state
            .work_order_reorder_request
            .fail(request_id, "No active project tab".to_string());
        return;
    };
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let scope = task_scope(&route);
    let lane_id = lane.lane_id.clone();
    let expected_revision = lane.revision;
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "lane_reorder",
        scope.0,
        None,
        scope.1,
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::LaneReordered {
                    request_id,
                    route,
                    generation,
                    lane_id,
                    revision: expected_revision,
                    error: Some("Core unavailable".to_string()),
                });
            };
            match core_client
                .request(crate::core::new_request(
                    format!("lane-reorder-{request_id}-{}", uuid::Uuid::new_v4()),
                    CoreRequest::WorkOrderLaneReorder {
                        request: crate::protocol::work_order::WorkOrderLaneReorderRequest {
                            lane_id: lane_id.clone(),
                            expected_revision,
                            ordered_work_order_ids: next_order,
                        },
                    },
                ))
                .await
            {
                Ok(CoreResponse::WorkOrderLane { lane }) => Some(TuiCommand::LaneReordered {
                    request_id,
                    route,
                    generation,
                    lane_id,
                    revision: lane.revision,
                    error: None,
                }),
                Ok(CoreResponse::Error { code, message }) => Some(TuiCommand::LaneReordered {
                    request_id,
                    route,
                    generation,
                    lane_id,
                    revision: expected_revision,
                    error: Some(format!("{code}: {message}")),
                }),
                Ok(_) => Some(TuiCommand::LaneReordered {
                    request_id,
                    route,
                    generation,
                    lane_id,
                    revision: expected_revision,
                    error: Some("Unexpected reorder response".to_string()),
                }),
                Err(error) => Some(TuiCommand::LaneReordered {
                    request_id,
                    route,
                    generation,
                    lane_id,
                    revision: expected_revision,
                    error: Some(error.to_string()),
                }),
            }
        },
    );
    if task_id.is_none() {
        let _ = app
            .dialog_state
            .work_order_reorder_request
            .fail(request_id, "TUI command channel unavailable".to_string());
        app.messages_state
            .toasts
            .error("TUI command channel unavailable; order unchanged");
    }
}

/// Apply a CAS reorder completion. Revision conflicts refresh the lane
/// and display "queue changed; retry" — local speculative order is
/// never applied. Moving a task never edits Job dependencies (lane
/// order only).
pub(crate) fn apply_lane_reordered(
    app: &mut App,
    request_id: u64,
    route: UiRouteToken,
    generation: u64,
    lane_id: String,
    revision: u64,
    error: Option<String>,
) {
    if let Some(error) = error {
        if app
            .dialog_state
            .work_order_reorder_request
            .fail(request_id, error.clone())
            && app.dialog_state.task_view.generation == generation
        {
            if error.contains("work_order_revision_conflict") {
                app.messages_state.toasts.warning("Queue changed; retry");
            } else {
                app.messages_state.toasts.warning(&error);
            }
            // Conflict or failure: refresh so the view shows the
            // canonical lane order, not speculative state.
            refresh_task_view(app);
        }
        return;
    }
    if !app
        .dialog_state
        .work_order_reorder_request
        .finish(request_id)
    {
        return;
    }
    if app.dialog_state.task_view.generation != generation {
        return;
    }
    let Some(check) = current_task_route_check(app) else {
        return;
    };
    if !route.matches(&check) {
        return;
    }
    if let Some(lane) = app
        .dialog_state
        .task_view
        .lanes
        .iter_mut()
        .find(|lane| lane.lane_id == lane_id)
    {
        lane.revision = revision;
    }
    app.messages_state.toasts.info("Task order updated");
    refresh_task_view(app);
}

// ── Lifecycle (cancel / resume) ──────────────────────────────────────

/// Cancel the selected waiting task via `WorkOrderCancel`. Running
/// tasks delegate to existing session/job control surfaces instead.
pub(crate) fn cancel_selected_task(app: &mut App) {
    mutate_selected_task(app, "cancel");
}

/// Retry the selected paused task via `WorkOrderResume`.
pub(crate) fn resume_selected_task(app: &mut App) {
    mutate_selected_task(app, "resume");
}

fn mutate_selected_task(app: &mut App, op: &str) {
    let Some(row) = app.dialog_state.task_view.selected_row().cloned() else {
        app.messages_state.toasts.info("No task selected");
        return;
    };
    if row.is_running() {
        app.messages_state.toasts.warning(
            "Running tasks use session controls (stop/cancel/steer); Task view only cancels waiting work",
        );
        return;
    }
    if op == "resume" && row.work_order.state != "paused" {
        app.messages_state
            .toasts
            .warning("Only paused tasks can be resumed");
        return;
    }
    if op == "cancel" && row.is_terminal() {
        app.messages_state.toasts.info("Task is already terminal");
        return;
    }
    let generation = app.dialog_state.task_view.generation;
    let request_id = app.dialog_state.work_order_mutation_request.begin();
    let Some(route) = capture_route(app, request_id) else {
        let _ = app
            .dialog_state
            .work_order_mutation_request
            .fail(request_id, "No active project tab".to_string());
        return;
    };
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let scope = task_scope(&route);
    let work_order_id = row.work_order.work_order_id.clone();
    let op = op.to_string();
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "task_mutation",
        scope.0,
        None,
        scope.1,
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TaskMutationFinished {
                    request_id,
                    route,
                    generation,
                    op,
                    work_order_id,
                    error: Some("Core unavailable".to_string()),
                });
            };
            let request = match op.as_str() {
                "cancel" => CoreRequest::WorkOrderCancel {
                    work_order_id: work_order_id.clone(),
                },
                _ => CoreRequest::WorkOrderResume {
                    work_order_id: work_order_id.clone(),
                },
            };
            match core_client
                .request(crate::core::new_request(
                    format!("task-mutation-{request_id}-{}", uuid::Uuid::new_v4()),
                    request,
                ))
                .await
            {
                Ok(CoreResponse::WorkOrder { .. }) => Some(TuiCommand::TaskMutationFinished {
                    request_id,
                    route,
                    generation,
                    op,
                    work_order_id,
                    error: None,
                }),
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::TaskMutationFinished {
                        request_id,
                        route,
                        generation,
                        op,
                        work_order_id,
                        error: Some(format!("{code}: {message}")),
                    })
                }
                Ok(_) => Some(TuiCommand::TaskMutationFinished {
                    request_id,
                    route,
                    generation,
                    op,
                    work_order_id,
                    error: Some("Unexpected mutation response".to_string()),
                }),
                Err(error) => Some(TuiCommand::TaskMutationFinished {
                    request_id,
                    route,
                    generation,
                    op,
                    work_order_id,
                    error: Some(error.to_string()),
                }),
            }
        },
    );
    if task_id.is_none() {
        let _ = app
            .dialog_state
            .work_order_mutation_request
            .fail(request_id, "TUI command channel unavailable".to_string());
        app.messages_state
            .toasts
            .error("TUI command channel unavailable");
    }
}

/// Apply a cancel/resume completion with generation + route guards,
/// then refresh the bounded projection.
pub(crate) fn apply_task_mutation_finished(
    app: &mut App,
    request_id: u64,
    route: UiRouteToken,
    generation: u64,
    op: String,
    work_order_id: String,
    error: Option<String>,
) {
    if let Some(error) = error {
        if app
            .dialog_state
            .work_order_mutation_request
            .fail(request_id, error.clone())
        {
            app.messages_state.toasts.warning(&error);
        }
        return;
    }
    if !app
        .dialog_state
        .work_order_mutation_request
        .finish(request_id)
    {
        return;
    }
    if app.dialog_state.task_view.generation != generation {
        return;
    }
    let Some(check) = current_task_route_check(app) else {
        return;
    };
    if !route.matches(&check) {
        return;
    }
    let short: String = work_order_id.chars().take(8).collect();
    app.messages_state.toasts.info(&format!(
        "Task {short} {}",
        if op == "cancel" {
            "cancelled"
        } else {
            "resumed"
        }
    ));
    refresh_task_view(app);
}

// ── Open materialized session ────────────────────────────────────────

/// Open the selected row: a materialized running/recent WorkOrder
/// focuses its canonical session through project-tab/session-loading
/// machinery; a future WorkOrder fetches task detail instead of
/// opening a fake session.
pub(crate) fn open_selected_task(app: &mut App) {
    let Some(row) = app.dialog_state.task_view.selected_row().cloned() else {
        app.messages_state.toasts.info("No task selected");
        return;
    };
    let Some(session_id) = row.materialized_session_id().map(str::to_string) else {
        // Future work: show detail/edit actions, never a fake session.
        fetch_selected_task_detail(app);
        app.messages_state.toasts.info(
            "Future task: showing detail (no session exists until the WorkOrder materializes)",
        );
        return;
    };
    // Project-correct routing: the task's project must own the target
    // tab. Cross-project opens switch to that project's tab first.
    if app.active_project_id() != Some(row.work_order.project_id.as_str()) {
        let target = app
            .project_tabs
            .find_by_project(&row.work_order.project_id)
            .map(|tab| tab.tab_id.clone());
        match target {
            Some(tab_id) => {
                crate::tui::commands::project_picker::switch_active_tab(app, &tab_id);
            }
            None => {
                app.messages_state
                    .toasts
                    .warning("Task belongs to a project with no open tab; open its project first");
                return;
            }
        }
    }
    let generation = app.dialog_state.task_view.generation;
    let project_id = row.work_order.project_id.clone();
    let request_id = app.dialog_state.session_messages_request.begin();
    let Some(route) = capture_route(app, request_id) else {
        let _ = app
            .dialog_state
            .session_messages_request
            .fail(request_id, "No active project tab".to_string());
        return;
    };
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let scope = task_scope(&route);
    let session_for_lookup = session_id.clone();
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "task_session_focus",
        scope.0,
        None,
        scope.1,
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TaskSessionFocus {
                    request_id,
                    route,
                    generation,
                    project_id,
                    session_id: session_for_lookup,
                    sessions: Vec::new(),
                    error: Some("Core unavailable".to_string()),
                });
            };
            match core_client
                .request(crate::core::new_request(
                    format!("task-session-list-{request_id}-{}", uuid::Uuid::new_v4()),
                    CoreRequest::SessionList {
                        project_id: project_id.clone(),
                        show_archived: false,
                        limit: 256,
                    },
                ))
                .await
            {
                Ok(CoreResponse::SessionList { sessions, .. }) => {
                    Some(TuiCommand::TaskSessionFocus {
                        request_id,
                        route,
                        generation,
                        project_id,
                        session_id: session_for_lookup,
                        sessions,
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => Some(TuiCommand::TaskSessionFocus {
                    request_id,
                    route,
                    generation,
                    project_id,
                    session_id: session_for_lookup,
                    sessions: Vec::new(),
                    error: Some(format!("{code}: {message}")),
                }),
                Ok(_) => Some(TuiCommand::TaskSessionFocus {
                    request_id,
                    route,
                    generation,
                    project_id,
                    session_id: session_for_lookup,
                    sessions: Vec::new(),
                    error: Some("Unexpected session list response".to_string()),
                }),
                Err(error) => Some(TuiCommand::TaskSessionFocus {
                    request_id,
                    route,
                    generation,
                    project_id,
                    session_id: session_for_lookup,
                    sessions: Vec::new(),
                    error: Some(error.to_string()),
                }),
            }
        },
    );
    if task_id.is_none() {
        let _ = app
            .dialog_state
            .session_messages_request
            .fail(request_id, "TUI command channel unavailable".to_string());
    }
}

/// Apply a session-focus lookup: bind the canonical session through
/// the existing session machinery with stale-completion guards.
pub(crate) fn apply_task_session_focus(
    app: &mut App,
    request_id: u64,
    route: UiRouteToken,
    generation: u64,
    project_id: String,
    session_id: String,
    sessions: Vec<crate::protocol::dto::Session>,
    error: Option<String>,
) {
    if let Some(error) = error {
        if app
            .dialog_state
            .session_messages_request
            .fail(request_id, error.clone())
        {
            app.messages_state.toasts.warning(&error);
        }
        return;
    }
    if !app.dialog_state.session_messages_request.finish(request_id) {
        return;
    }
    if app.dialog_state.task_view.generation != generation {
        return;
    }
    let Some(check) = current_task_route_check(app) else {
        return;
    };
    // Entering a running WorkOrder session uses canonical
    // project/session routing and stale-completion guards: the tab,
    // project, workspace, view epoch, and reconnect epoch must all
    // still match the captured route.
    if !route.matches(&check) || app.active_project_id() != Some(project_id.as_str()) {
        return;
    }
    let Some(dto) = sessions.into_iter().find(|s| s.id == session_id) else {
        app.messages_state
            .toasts
            .warning("Task session is no longer listed; refreshing tasks");
        refresh_task_view(app);
        return;
    };
    let session = match crate::protocol_conversions::dto_to_session(dto) {
        Ok(session) => session,
        Err(error) => {
            app.messages_state
                .toasts
                .warning(&format!("Could not open task session: {error}"));
            return;
        }
    };
    // Canonical focus path (same machinery as the session dialog):
    // cancel per-session background work, bind the session, refresh
    // git state, close the Task view, and navigate to the session.
    use crate::tui::task_lifecycle::TuiTaskKind;
    app.task_registry.cancel_kind(TuiTaskKind::Research);
    app.task_registry.cancel_kind(TuiTaskKind::Memory);
    app.task_registry.cancel_kind(TuiTaskKind::GitStatus);
    app.set_session(session);
    crate::tui::commands::git_sidebar::start_refresh_git_sidebar(app);
    app.close_dialog();
    app.ui_state
        .routes
        .navigate_to(crate::tui::route::Route::Session(session_id));
}

// ── /tasks migration ─────────────────────────────────────────────────

/// `/tasks` entry: open the WorkOrder Task view when the capability is
/// available; otherwise fall back to the low-level schedule list with
/// an explicit compatibility diagnostic. The low-level `Schedule*`
/// protocol is preserved; `/schedules` keeps direct diagnostic access.
pub(crate) fn start_tasks_command(app: &mut App) {
    let context = match app.project_execution_context() {
        Ok(context) => context,
        Err(_) => {
            // No project scope (or legacy session-only state): keep the
            // legacy schedule surface rather than failing.
            super::tasks::start_list_tasks(app);
            return;
        }
    };
    if context.project_id.is_none() {
        super::tasks::start_list_tasks(app);
        return;
    }
    open_task_view(app);
}

// ── Route/scope helpers ──────────────────────────────────────────────

fn capture_route(app: &App, request_id: u64) -> Option<UiRouteToken> {
    // `project_execution_context` validates explicit project scope
    // (never process cwd); the token carries the same scope plus tab,
    // session, view-epoch, and reconnect identity for stale guards.
    let context = app.project_execution_context().ok()?;
    let tab_id = app.active_tab_id()?;
    Some(UiRouteToken::new(
        Some(tab_id),
        context
            .project_id
            .clone()
            .or_else(|| app.active_project_id().map(str::to_string)),
        context
            .workspace_id
            .clone()
            .or_else(|| app.active_workspace_id().map(str::to_string)),
        context
            .session_id
            .clone()
            .or_else(|| app.active_session_id().map(str::to_string)),
        app.view_switch.active_view_epoch,
        app.routing_registry.reconnect_epoch,
        request_id,
    ))
}

fn current_task_route_check(app: &App) -> Option<RouteCheck> {
    let context = app.project_execution_context().ok()?;
    let tab_id = app.active_tab_id();
    Some(app.routing_registry.check_for(
        tab_id.as_ref(),
        context.project_id.as_deref(),
        context.workspace_id.as_deref(),
        context.session_id.as_deref(),
        app.view_switch.active_view_epoch,
    ))
}

/// Registry scope for WorkOrder background tasks: abort on tab switch
/// (defense in depth; route guards remain authoritative at apply).
fn task_scope(route: &UiRouteToken) -> (Option<String>, Option<u64>) {
    let tab = route
        .tab_id
        .clone()
        .map(|tab_id| tab_id.as_str().to_string());
    (tab, Some(route.active_view_epoch))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::CoreClient;
    use crate::protocol::core::{CoreEvent, EventEnvelope, RequestEnvelope};
    use crate::protocol::work_order::{
        SequenceLaneDto, WorkOrderCapabilitiesDto, WorkOrderDto, WorkOrderGateDto,
        WorkOrderOccurrenceDto, WorkOrderSummaryDto,
    };
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    struct FakeWorkOrderClient;

    fn capabilities() -> WorkOrderCapabilitiesDto {
        WorkOrderCapabilitiesDto {
            supported: true,
            protocol_version: 1,
            max_title_chars: 200,
            max_prompt_bytes: 20000,
            max_batch_items: 10,
            max_list_limit: 100,
            max_repeat_count: 100,
            max_lanes_per_project: 10,
            max_lane_members: 50,
        }
    }

    pub(crate) fn work_order_dto(id: &str, state: &str) -> WorkOrderDto {
        WorkOrderDto {
            work_order_id: id.to_string(),
            revision: 1,
            project_id: "project-1".to_string(),
            creator_principal: "local-owner".to_string(),
            parent_session_id: None,
            parent_turn_id: None,
            parent_work_order_id: None,
            title: Some(format!("Task {id}")),
            prompt: format!("prompt for {id}"),
            requested_model: Some("conn-a/model-1".to_string()),
            requested_approval: None,
            requested_sandbox: None,
            workspace_policy: None,
            gates: vec![WorkOrderGateDto {
                kind: "immediate".to_string(),
                delay_secs: None,
                not_before_ms: None,
                lane_id: None,
                trigger_ref: None,
            }],
            gate_join: None,
            repeat_count: 1,
            sequence_lane_id: None,
            state: state.to_string(),
            created_at_ms: 1,
            updated_at_ms: 2,
            cancelled_at_ms: None,
        }
    }

    fn occurrence_dto(
        work_order_id: &str,
        state: &str,
        session_id: Option<&str>,
    ) -> WorkOrderOccurrenceDto {
        WorkOrderOccurrenceDto {
            occurrence_id: format!("occ-{work_order_id}"),
            work_order_id: work_order_id.to_string(),
            project_id: "project-1".to_string(),
            occurrence_index: 0,
            state: state.to_string(),
            gate_latches: Vec::new(),
            not_before_ms: None,
            next_check_at_ms: None,
            session_id: session_id.map(str::to_string),
            job_id: None,
            workspace_id: None,
            worktree_id: None,
            attention_code: if state == "needs_attention" {
                Some("model_unavailable".to_string())
            } else {
                None
            },
            diagnostic: None,
            created_at_ms: 1,
            updated_at_ms: 2,
            claimed_at_ms: None,
            started_at_ms: None,
            terminal_at_ms: None,
        }
    }

    #[async_trait]
    impl CoreClient for FakeWorkOrderClient {
        async fn request(
            &self,
            request: RequestEnvelope<CoreRequest>,
        ) -> Result<CoreResponse, crate::error::AppError> {
            Ok(match request.payload {
                CoreRequest::WorkOrderCapabilities => CoreResponse::WorkOrderCapabilities {
                    capabilities: capabilities(),
                },
                CoreRequest::WorkOrderLaneList { .. } => CoreResponse::WorkOrderLaneList {
                    lanes: Vec::new(),
                    truncated: false,
                },
                CoreRequest::WorkOrderSummary { project_id } => CoreResponse::WorkOrderSummary {
                    summary: WorkOrderSummaryDto {
                        project_id,
                        active: 0,
                        paused: 0,
                        completed: 0,
                        cancelled: 0,
                        archived: 0,
                        waiting_occurrences: 0,
                        attention_occurrences: 0,
                        lane_count: 0,
                    },
                },
                CoreRequest::ApprovalPreferenceGet => CoreResponse::ApprovalPreference {
                    preference: crate::protocol::core::RuntimePreferenceDto {
                        principal_id: "local-owner".to_string(),
                        approval_mode: crate::protocol::core::ApprovalModeDto::Interactive,
                        sandbox_profile: crate::protocol::core::SandboxProfileDto::WorkspaceWrite,
                        revision: 3,
                        updated_at_ms: 0,
                        last_provider_connection_id: None,
                        last_model_id: None,
                        last_task_provider_connection_id: Some("conn-a".to_string()),
                        last_task_model_id: Some("gone-model".to_string()),
                    },
                },
                CoreRequest::WorkOrderCreate { request } => CoreResponse::WorkOrder {
                    work_order: WorkOrderDto {
                        requested_model: request.requested_model.clone(),
                        ..work_order_dto("wo-created", "active")
                    },
                    duplicate: false,
                },
                CoreRequest::WorkOrderList { .. } => CoreResponse::WorkOrderList {
                    work_orders: vec![work_order_dto("wo-a", "active")],
                    next_cursor: None,
                    truncated: false,
                },
                CoreRequest::WorkOrderOccurrenceList { work_order_id, .. } => {
                    CoreResponse::WorkOrderOccurrenceList {
                        occurrences: vec![occurrence_dto(&work_order_id, "waiting", None)],
                        truncated: false,
                    }
                }
                CoreRequest::SessionList { project_id, .. } => {
                    let session = crate::protocol::dto::Session {
                        id: "sess-1".to_string(),
                        project_id,
                        title: "task session".to_string(),
                        directory: "/tmp".to_string(),
                        ..crate::protocol::dto::Session::default()
                    };
                    CoreResponse::SessionList {
                        sessions: vec![session],
                    }
                }
                CoreRequest::TaskModelPreferenceSet { .. } => CoreResponse::ApprovalPreference {
                    preference: crate::protocol::core::RuntimePreferenceDto {
                        principal_id: "local-owner".to_string(),
                        approval_mode: crate::protocol::core::ApprovalModeDto::Interactive,
                        sandbox_profile: crate::protocol::core::SandboxProfileDto::WorkspaceWrite,
                        revision: 4,
                        updated_at_ms: 0,
                        last_provider_connection_id: None,
                        last_model_id: None,
                        last_task_provider_connection_id: None,
                        last_task_model_id: None,
                    },
                },
                CoreRequest::WorkOrderCancel { work_order_id } => CoreResponse::WorkOrder {
                    work_order: work_order_dto(&work_order_id, "cancelled"),
                    duplicate: false,
                },
                CoreRequest::WorkOrderResume { work_order_id } => CoreResponse::WorkOrder {
                    work_order: work_order_dto(&work_order_id, "active"),
                    duplicate: false,
                },
                CoreRequest::WorkOrderLaneReorder { request } => CoreResponse::WorkOrderLane {
                    lane: SequenceLaneDto {
                        lane_id: request.lane_id.clone(),
                        project_id: "project-1".to_string(),
                        revision: request.expected_revision + 1,
                        label: None,
                        failure_policy: "hold_lane".to_string(),
                        ordered_work_order_ids: request.ordered_work_order_ids.clone(),
                        created_at_ms: 0,
                        updated_at_ms: 1,
                    },
                },
                _ => CoreResponse::Error {
                    code: "unsupported_in_test".to_string(),
                    message: "fake client does not implement this request".to_string(),
                },
            })
        }

        fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
            let (_tx, rx) = mpsc::channel(1);
            rx
        }
    }

    pub(crate) fn test_app_with_project() -> App {
        let mut app = App::new_for_testing("/tmp".to_string());
        if let Some(tab) = app.project_tabs.active_mut() {
            tab.project_id = Some("project-1".to_string());
            tab.workspace_id = Some("workspace-1".to_string());
        }
        app.set_core_client(Arc::new(FakeWorkOrderClient));
        let (tx, _rx) = mpsc::channel(32);
        app.tui_cmd_tx = Some(tx);
        app
    }

    pub(crate) fn test_route(app: &App, request_id: u64) -> UiRouteToken {
        capture_route(app, request_id).expect("fixture has a project tab")
    }

    pub(crate) fn prime_view_with_lane(app: &mut App) {
        app.dialog_state.task_view.project_id = Some("project-1".to_string());
        app.dialog_state.task_view.rows = vec![
            TaskViewRow {
                work_order: work_order_dto("wo-a", "active"),
                occurrence: Some(occurrence_dto("wo-a", "waiting", None)),
            },
            TaskViewRow {
                work_order: work_order_dto("wo-b", "active"),
                occurrence: Some(occurrence_dto("wo-b", "waiting", None)),
            },
        ];
        app.dialog_state.task_view.lanes = vec![SequenceLaneDto {
            lane_id: "lane-1".to_string(),
            project_id: "project-1".to_string(),
            revision: 7,
            label: Some("main".to_string()),
            failure_policy: "hold_lane".to_string(),
            ordered_work_order_ids: vec!["wo-a".to_string(), "wo-b".to_string()],
            created_at_ms: 0,
            updated_at_ms: 1,
        }];
        app.dialog_state.task_view.selected = 0;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn composer_toggle_preserves_input_mode_and_prompt() {
        let mut app = test_app_with_project();
        app.prompt_state.prompt.set_text("draft prompt".to_string());
        assert_eq!(app.prompt_state.composer_mode, ComposerMode::Session);

        toggle_composer_mode(&mut app);
        assert_eq!(app.prompt_state.composer_mode, ComposerMode::Task);
        assert_eq!(
            app.ui_state.input_mode,
            crate::tui::input::InputMode::Insert
        );
        assert_eq!(app.prompt_state.prompt.get_text(), "draft prompt");

        toggle_composer_mode(&mut app);
        assert_eq!(app.prompt_state.composer_mode, ComposerMode::Session);
        assert_eq!(app.prompt_state.prompt.get_text(), "draft prompt");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn composer_toggle_preserves_normal_mode_and_selection_state() {
        use crate::tui::input::InputMode;
        let mut app = test_app_with_project();
        app.ui_state.input_mode = InputMode::Normal;
        app.prompt_state.prompt.set_text("normal draft".to_string());
        let model_before = app.agent_state.current_model.clone();
        let tab_before = app.project_tabs.active().map(|tab| tab.tab_id.clone());
        let session_before = app.session_state.session.clone();

        toggle_composer_mode(&mut app);
        assert_eq!(app.prompt_state.composer_mode, ComposerMode::Task);
        // InputMode untouched; prompt/model/project/session preserved.
        assert_eq!(app.ui_state.input_mode, InputMode::Normal);
        assert_eq!(app.prompt_state.prompt.get_text(), "normal draft");
        assert_eq!(app.agent_state.current_model, model_before);
        assert_eq!(
            app.project_tabs.active().map(|tab| tab.tab_id.clone()),
            tab_before
        );
        assert_eq!(
            app.session_state.session.as_ref().map(|s| s.id.clone()),
            session_before.as_ref().map(|s| s.id.clone())
        );

        toggle_composer_mode(&mut app);
        assert_eq!(app.prompt_state.composer_mode, ComposerMode::Session);
        assert_eq!(app.ui_state.input_mode, InputMode::Normal);
        assert_eq!(app.prompt_state.prompt.get_text(), "normal draft");
    }

    #[test]
    fn queue_concurrent_reorder_admits_one_cas_winner() {
        // Two reorder intents from the same base order: the first CAS
        // wins, the loser refreshes with `queue changed; retry` and never
        // applies speculative order.
        use crate::tui::app::state::TaskViewState;
        let base = vec!["wo-a".to_string(), "wo-b".to_string(), "wo-c".to_string()];
        let first = TaskViewState::moved_lane_order(&base, None, "wo-a", 1).unwrap();
        let second = TaskViewState::moved_lane_order(&base, None, "wo-c", -1).unwrap();
        assert_ne!(first, second);
        // Winner's order is a valid permutation; loser recomputes from the
        // winner's canonical order instead of applying its stale intent.
        let loser_retry = TaskViewState::moved_lane_order(&first, None, "wo-c", -1).unwrap();
        assert_ne!(loser_retry, second);
        assert_eq!(loser_retry.len(), 3);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scheduling_placement_creates_no_session_or_job() {
        let mut app = test_app_with_project();
        let (tx, mut rx) = mpsc::channel(8);
        app.tui_cmd_tx = Some(tx);
        // Sequential sheet with a queue insertion position (not at end).
        let draft = TaskScheduleDraft {
            sequential: true,
            lane_id: Some("lane-1".to_string()),
            queue_insert_position: Some(0),
            queue_expected_revision: Some(7),
            ..TaskScheduleDraft::default()
        };
        app.dialog_state.task_schedule_draft = Some(draft);
        app.prompt_state.prompt.set_text("queued task".to_string());

        confirm_task_schedule(
            &mut app,
            "0".to_string(),
            String::new(),
            "1".to_string(),
            true,
            Some("lane-1".to_string()),
            Some(0),
            Some(7),
            false,
            true,
            Some("opencode_zen/big-pickle".to_string()),
        );

        // Only WorkOrderCreate ran; no session/job was fabricated and no
        // Job dependency row was edited.
        let completion = rx.recv().await.expect("create completion");
        assert!(matches!(completion, TuiCommand::WorkOrderCreated { .. }));
        assert!(app.session_state.session.is_none());
        // Prompt stays until the create succeeds (failure restores it).
        // Success path tested separately; here we only assert no fake
        // session/job side effect from placement itself.
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn keybinding_audit_has_no_tab_collision() {
        use crate::tui::input::{default_bindings, vim_bindings, InputAction};
        use crossterm::event::{KeyCode, KeyModifiers};
        for bindings in [default_bindings(), vim_bindings()] {
            // C001: bare Tab owns the composer cycle at root prompt focus.
            assert_eq!(
                bindings.get(&(KeyModifiers::NONE, KeyCode::Tab)),
                Some(&InputAction::ToggleComposerMode)
            );
            assert_eq!(
                bindings.get(&(KeyModifiers::SHIFT, KeyCode::Tab)),
                Some(&InputAction::TogglePermissionMode)
            );
            // Ctrl+G remains a backward-compatible alias.
            assert_eq!(
                bindings.get(&(KeyModifiers::CONTROL, KeyCode::Char('g'))),
                Some(&InputAction::ToggleComposerMode)
            );
            // SwitchAgent migrates to Ctrl+A: portable (0x01) and never
            // normalized to Tab (0x09). Ctrl+I/H/M/J are deliberately
            // avoided (terminal-normalized to Tab/Backspace/Enter).
            assert_eq!(
                bindings.get(&(KeyModifiers::CONTROL, KeyCode::Char('a'))),
                Some(&InputAction::SwitchAgent)
            );
            // Exactly one bare-Tab owner after normalization.
            let tab_owners = bindings
                .iter()
                .filter(|((mods, code), _)| *mods == KeyModifiers::NONE && *code == KeyCode::Tab)
                .count();
            assert_eq!(tab_owners, 1);
        }
        // Help documents the composer toggle on Tab in both editing modes,
        // and keeps SwitchAgent discoverable on its migrated binding.
        use crate::tui::input::{default_help_entries, HelpMode};
        for mode in [HelpMode::Insert, HelpMode::Normal] {
            let entries = default_help_entries();
            assert!(
                entries.iter().any(|entry| entry.mode == mode
                    && entry.key == "Tab"
                    && entry.action.contains("composer")),
                "missing Tab composer help for {mode:?}"
            );
            assert!(
                entries.iter().any(|entry| entry.mode == mode
                    && entry.key == "Ctrl+A"
                    && entry.action.contains("Switch agent")),
                "missing migrated SwitchAgent help for {mode:?}"
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn task_mode_enter_opens_sheet_without_touching_transcript() {
        let mut app = test_app_with_project();
        app.prompt_state.composer_mode = ComposerMode::Task;
        app.prompt_state
            .prompt
            .set_text("schedule this work".to_string());
        let messages_before = app.messages_state.messages.messages.len();

        app.process_msg(crate::tui::app::TuiMsg::SubmitPrompt);

        // Sheet prefetch started; the prompt stays editable and nothing
        // was submitted to the session transcript.
        assert!(app.dialog_state.task_sheet_request.is_loading());
        assert_eq!(app.prompt_state.prompt.get_text(), "schedule this work");
        assert_eq!(app.messages_state.messages.messages.len(), messages_before);
        assert!(app.session_state.session.is_none());
    }

    #[test]
    fn remembered_task_model_falls_back_visibly_when_unknown() {
        let mut app = test_app_with_project();
        // Fixture models do not include `gone-model`.
        app.prompt_state.task_model_choice = Some(TaskModelChoice {
            connection: Some("conn-a".to_string()),
            model: Some("gone-model".to_string()),
            revision: 3,
            from_preference: true,
        });
        let (model, from_pref, notice) = resolve_task_model(&mut app);
        assert_eq!(model.as_deref(), Some("opencode_zen/big-pickle"));
        assert!(!from_pref);
        assert!(notice.is_some());
    }

    #[test]
    fn create_failure_restores_prompt_exactly_once() {
        let mut app = test_app_with_project();
        let request_id = app.dialog_state.work_order_create_request.begin();
        let route = test_route(&app, request_id);
        let pending = PendingTaskCreate {
            request_id,
            prompt: "task prompt".to_string(),
            project_id: "project-1".to_string(),
            route: route.clone(),
            context: app.project_execution_context().unwrap(),
            external_trigger: false,
            trigger_key_prefix: None,
            queue_lane_id: None,
            queue_insert_position: None,
            queue_expected_revision: None,
        };
        app.prompt_state.pending_task_create = Some(pending);
        app.prompt_state.prompt.set_text("task prompt".to_string());

        apply_work_order_created(
            &mut app,
            request_id,
            route,
            "project-1".to_string(),
            "task prompt".to_string(),
            None,
            false,
            "Run now".to_string(),
            Some("work_order_invalid: bad gates".to_string()),
        );

        assert_eq!(app.prompt_state.prompt.get_text(), "task prompt");
        assert!(!app.prompt_state.pending_send);
        assert!(app.prompt_state.pending_task_create.is_none());
        assert!(app
            .messages_state
            .toasts
            .iter()
            .any(|toast| toast.message.contains("Task creation failed")));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn create_success_clears_prompt_records_history_and_remembers_model() {
        let mut app = test_app_with_project();
        let (tx, mut rx) = mpsc::channel(8);
        app.tui_cmd_tx = Some(tx);
        let request_id = app.dialog_state.work_order_create_request.begin();
        let route = test_route(&app, request_id);
        let pending = PendingTaskCreate {
            request_id,
            prompt: "task prompt".to_string(),
            project_id: "project-1".to_string(),
            route: route.clone(),
            context: app.project_execution_context().unwrap(),
            external_trigger: false,
            trigger_key_prefix: None,
            queue_lane_id: None,
            queue_insert_position: None,
            queue_expected_revision: None,
        };
        app.prompt_state.pending_task_create = Some(pending);

        apply_work_order_created(
            &mut app,
            request_id,
            route,
            "project-1".to_string(),
            "task prompt".to_string(),
            Some(work_order_dto("wo-created", "active")),
            false,
            "Run now · model conn-a/model-1".to_string(),
            None,
        );

        assert_eq!(app.prompt_state.prompt.get_text(), "");
        assert!(app
            .session_state
            .history
            .iter()
            .any(|entry| entry.text == "task prompt"));
        assert!(app
            .messages_state
            .toasts
            .iter()
            .any(|toast| toast.message.contains("Task wo-creat created")));
        // Best-effort Task-model persist was spawned (not the ordinary
        // session preference path).
        let saved = rx.recv().await.expect("task model persist completion");
        assert!(matches!(
            saved,
            TuiCommand::TaskModelPrefSaved { error: None, .. }
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stale_create_completion_is_dropped_without_undo() {
        let mut app = test_app_with_project();
        // Capture at a nonzero view epoch so the epoch guard is armed
        // (epoch-0 tokens predate view tracking and match openly).
        app.view_switch.bump_epoch();
        let request_id = app.dialog_state.work_order_create_request.begin();
        let route = test_route(&app, request_id);
        app.prompt_state.pending_task_create = Some(PendingTaskCreate {
            request_id,
            prompt: "task prompt".to_string(),
            project_id: "project-1".to_string(),
            route: route.clone(),
            context: app.project_execution_context().unwrap(),
            external_trigger: false,
            trigger_key_prefix: None,
            queue_lane_id: None,
            queue_insert_position: None,
            queue_expected_revision: None,
        });
        app.prompt_state.prompt.set_text("newer typing".to_string());
        app.view_switch.bump_epoch();

        apply_work_order_created(
            &mut app,
            request_id,
            route,
            "project-1".to_string(),
            "task prompt".to_string(),
            Some(work_order_dto("wo-created", "active")),
            false,
            "Run now".to_string(),
            None,
        );

        // Stale: no prompt wipe, no transcript/history write, no undo of
        // daemon state (the WorkOrder stands; view refreshes when open —
        // it is not open here).
        assert_eq!(app.prompt_state.prompt.get_text(), "newer typing");
        assert!(app
            .session_state
            .history
            .iter()
            .all(|entry| entry.text != "task prompt"));
        assert!(app.prompt_state.pending_task_create.is_none());
    }

    #[test]
    fn stale_view_refresh_is_dropped() {
        let mut app = test_app_with_project();
        prime_view_with_lane(&mut app);
        let request_id = app.dialog_state.work_order_list_request.begin();
        let route = test_route(&app, request_id);
        let rows_before = app.dialog_state.task_view.rows.len();

        // A superseding refresh bumps the generation first.
        let _new_generation = app.dialog_state.task_view.begin_refresh("project-1");
        apply_task_view_refreshed(
            &mut app,
            request_id,
            route,
            0,
            "project-1".to_string(),
            vec![work_order_dto("wo-new", "active")],
            None,
            Vec::new(),
            true,
            false,
            None,
        );

        assert_eq!(app.dialog_state.task_view.rows.len(), rows_before);
        assert!(app
            .dialog_state
            .task_view
            .rows
            .iter()
            .all(|row| row.work_order.work_order_id != "wo-new"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reorder_conflict_refreshes_with_retry_notice() {
        let mut app = test_app_with_project();
        let (tx, _rx) = mpsc::channel(8);
        app.tui_cmd_tx = Some(tx);
        prime_view_with_lane(&mut app);
        app.dialog_state.task_view.selected = 1;
        let request_id = app.dialog_state.work_order_reorder_request.begin();
        let route = test_route(&app, request_id);
        let generation = app.dialog_state.task_view.generation;

        apply_lane_reordered(
            &mut app,
            request_id,
            route,
            generation,
            "lane-1".to_string(),
            7,
            Some("work_order_revision_conflict: stale lane revision".to_string()),
        );

        assert!(app
            .messages_state
            .toasts
            .iter()
            .any(|toast| toast.message.contains("Queue changed; retry")));
        // Conflict triggers a canonical refresh, never speculative order.
        assert!(app.dialog_state.work_order_list_request.is_loading());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn running_task_opens_canonical_session() {
        let mut app = test_app_with_project();
        prime_view_with_lane(&mut app);
        app.dialog_state.task_view.rows[0].occurrence =
            Some(occurrence_dto("wo-a", "running", Some("sess-1")));
        let request_id = app.dialog_state.session_messages_request.begin();
        let route = test_route(&app, request_id);
        let generation = app.dialog_state.task_view.generation;
        let session = crate::protocol::dto::Session {
            id: "sess-1".to_string(),
            project_id: "project-1".to_string(),
            title: "task session".to_string(),
            directory: "/tmp".to_string(),
            ..crate::protocol::dto::Session::default()
        };

        apply_task_session_focus(
            &mut app,
            request_id,
            route,
            generation,
            "project-1".to_string(),
            "sess-1".to_string(),
            vec![session],
            None,
        );

        assert_eq!(
            app.session_state.session.as_ref().map(|s| s.id.as_str()),
            Some("sess-1")
        );
        assert!(matches!(
            app.ui_state.routes.current(),
            crate::tui::route::Route::Session(id) if id == "sess-1"
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn future_row_open_fetches_detail_not_a_session() {
        let mut app = test_app_with_project();
        let (tx, _rx) = mpsc::channel(8);
        app.tui_cmd_tx = Some(tx);
        prime_view_with_lane(&mut app);
        app.dialog_state.task_view.selected = 0;

        open_selected_task(&mut app);

        assert!(app.session_state.session.is_none());
        assert!(app.dialog_state.work_order_occurrence_request.is_loading());
    }

    #[test]
    fn tab_close_cancels_ui_only_never_daemon_work() {
        let mut app = test_app_with_project();
        let request_id = app.dialog_state.work_order_create_request.begin();
        let route = test_route(&app, request_id);
        let pending = PendingTaskCreate {
            request_id,
            prompt: "task prompt".to_string(),
            project_id: "project-1".to_string(),
            route,
            context: app.project_execution_context().unwrap(),
            external_trigger: false,
            trigger_key_prefix: None,
            queue_lane_id: None,
            queue_insert_position: None,
            queue_expected_revision: None,
        };
        app.prompt_state.pending_task_create = Some(pending.clone());

        // Tab close path: UI continuation is dropped; no cancel request
        // exists to send (daemon WorkOrders are never deleted to undo).
        let dropped = app.prompt_state.cancel_task_submit();
        assert_eq!(dropped, Some(pending));
        assert!(app.prompt_state.pending_task_create.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tasks_command_opens_view_not_schedule_list() {
        let mut app = test_app_with_project();
        start_tasks_command(&mut app);
        assert_eq!(app.ui_state.dialog, Dialog::TaskView);
        assert!(app.dialog_state.work_order_list_request.is_loading());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn full_create_loop_through_fake_daemon() {
        let mut app = test_app_with_project();
        let (tx, mut rx) = mpsc::channel(8);
        app.tui_cmd_tx = Some(tx);
        // Seed an open sheet draft (as apply_sheet_prefetched would).
        app.dialog_state.task_schedule_draft = Some(TaskScheduleDraft::default());
        app.prompt_state
            .prompt
            .set_text("full loop task".to_string());

        confirm_task_schedule(
            &mut app,
            "0".to_string(),
            String::new(),
            "1".to_string(),
            false,
            None,
            None,
            None,
            false,
            true,
            Some("opencode_zen/big-pickle".to_string()),
        );

        // The create continuation ran against the fake daemon.
        let completion = rx.recv().await.expect("create completion");
        match completion {
            TuiCommand::WorkOrderCreated {
                request_id,
                route,
                project_id,
                prompt,
                work_order,
                duplicate,
                schedule_summary,
                error,
            } => {
                assert!(error.is_none());
                assert!(!duplicate);
                apply_work_order_created(
                    &mut app,
                    request_id,
                    route,
                    project_id,
                    prompt,
                    work_order,
                    duplicate,
                    schedule_summary,
                    None,
                );
            }
            other => panic!("expected WorkOrderCreated, got {other:?}"),
        }
        assert_eq!(app.prompt_state.prompt.get_text(), "");
        assert!(app
            .messages_state
            .toasts
            .iter()
            .any(|toast| toast.message.contains("Run now")));
    }
}

#[cfg(test)]
mod isolation_tests {
    use super::tests::{prime_view_with_lane, test_app_with_project, test_route, work_order_dto};
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn create_completion_for_other_project_is_dropped() {
        let mut app = test_app_with_project();
        app.view_switch.bump_epoch();
        let request_id = app.dialog_state.work_order_create_request.begin();
        let route = test_route(&app, request_id);
        app.prompt_state.pending_task_create = Some(PendingTaskCreate {
            request_id,
            prompt: "task prompt".to_string(),
            project_id: "project-1".to_string(),
            route: route.clone(),
            context: app.project_execution_context().unwrap(),
            external_trigger: false,
            trigger_key_prefix: None,
            queue_lane_id: None,
            queue_insert_position: None,
            queue_expected_revision: None,
        });
        app.prompt_state.prompt.set_text("task prompt".to_string());
        // Switch to another project's tab before the completion lands.
        let mut other = crate::tui::app::state::project_tabs::ProjectTabState::empty(
            crate::tui::app::state::ProjectTabId::new(),
            "other".to_string(),
        );
        other.project_id = Some("project-2".to_string());
        other.workspace_id = Some("workspace-2".to_string());
        other.workspace_root = Some(std::path::PathBuf::from("/tmp"));
        let other_id = other.tab_id.clone();
        app.project_tabs.add_tab(other);
        assert!(app.switch_active_tab(&other_id));

        apply_work_order_created(
            &mut app,
            request_id,
            route,
            "project-1".to_string(),
            "task prompt".to_string(),
            Some(work_order_dto("wo-created", "active")),
            false,
            "Run now".to_string(),
            None,
        );

        // Dropped: prompt intact, no history write, no cross-project toast.
        assert_eq!(app.prompt_state.prompt.get_text(), "task prompt");
        assert!(app
            .session_state
            .history
            .iter()
            .all(|entry| entry.text != "task prompt"));
        assert!(app.prompt_state.pending_task_create.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn view_refresh_for_other_project_is_dropped() {
        let mut app = test_app_with_project();
        prime_view_with_lane(&mut app);
        let request_id = app.dialog_state.work_order_list_request.begin();
        let route = test_route(&app, request_id);
        let generation = app.dialog_state.task_view.generation;
        // Rebind the active tab to another project before completion.
        if let Some(tab) = app.project_tabs.active_mut() {
            tab.project_id = Some("project-2".to_string());
        }

        apply_task_view_refreshed(
            &mut app,
            request_id,
            route,
            generation,
            "project-1".to_string(),
            vec![work_order_dto("wo-intruder", "active")],
            None,
            Vec::new(),
            true,
            false,
            None,
        );

        assert!(app
            .dialog_state
            .task_view
            .rows
            .iter()
            .all(|row| row.work_order.work_order_id != "wo-intruder"));
    }

    fn trigger_metadata(
        status: &str,
        id: &str,
    ) -> crate::protocol::work_order::TaskTriggerMetadataDto {
        trigger_metadata_for("wo-created", status, id)
    }

    fn trigger_metadata_for(
        work_order_id: &str,
        status: &str,
        id: &str,
    ) -> crate::protocol::work_order::TaskTriggerMetadataDto {
        crate::protocol::work_order::TaskTriggerMetadataDto {
            trigger_id: id.to_string(),
            project_id: "project-1".to_string(),
            work_order_id: work_order_id.to_string(),
            trigger_ref: "external".to_string(),
            status: status.to_string(),
            created_by: "local-owner".to_string(),
            created_at_ms: 1,
            expires_at_ms: None,
            max_fires: None,
            fire_count: 0,
            last_fired_at_ms: None,
            revision: 1,
        }
    }

    #[test]
    fn work_order_failure_performs_zero_trigger_create() {
        // Validation: a failed WorkOrder never reaches trigger creation.
        // The pending chain carries no trigger intent after failure and
        // the prompt is restored exactly once.
        let mut app = test_app_with_project();
        let request_id = app.dialog_state.work_order_create_request.begin();
        let route = test_route(&app, request_id);
        app.prompt_state.pending_task_create = Some(PendingTaskCreate {
            request_id,
            prompt: "gated task".to_string(),
            project_id: "project-1".to_string(),
            route: route.clone(),
            context: app.project_execution_context().unwrap(),
            external_trigger: true,
            trigger_key_prefix: Some("tui-task-1".to_string()),
            queue_lane_id: None,
            queue_insert_position: None,
            queue_expected_revision: None,
        });
        app.prompt_state.prompt.set_text("gated task".to_string());
        apply_work_order_created(
            &mut app,
            request_id,
            route,
            "project-1".to_string(),
            "gated task".to_string(),
            None,
            false,
            "Run now".to_string(),
            Some("work_order_invalid: bad gates".to_string()),
        );
        assert_eq!(app.prompt_state.prompt.get_text(), "gated task");
        assert!(!app.dialog_state.trigger_create_request.is_loading());
        assert!(app.dialog_state.trigger_secret.is_none());
        assert!(!app.dialog_state.trigger_metadata.contains_key("wo-created"));
    }

    #[test]
    fn trigger_success_shows_secret_once_and_caches_metadata() {
        let mut app = test_app_with_project();
        let request_id = app.dialog_state.trigger_create_request.begin();
        let route = test_route(&app, request_id);
        apply_trigger_created(
            &mut app,
            request_id,
            route,
            "project-1".to_string(),
            "wo-created".to_string(),
            Some(trigger_metadata("active", "trigger-1")),
            Some("cggtr_trigger-1.secret-abc".to_string()),
            false,
            None,
        );
        // Secret surface mounted exactly once; metadata cached without secret.
        assert!(app.dialog_state.trigger_secret.is_some());
        assert_eq!(
            app.dialog_state.trigger_secret.as_ref().unwrap().trigger_id,
            "trigger-1"
        );
        assert_eq!(app.ui_state.dialog, Dialog::TriggerSecret);
        let cached = app.dialog_state.trigger_metadata.get("wo-created").unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].trigger_id, "trigger-1");
        // Bearer never in debug projections.
        let secret_debug = format!("{:?}", app.dialog_state.trigger_secret.as_ref().unwrap());
        assert!(!secret_debug.contains("secret-abc"));
        // Closing forgets the bearer; metadata stays for rotation.
        close_trigger_secret(&mut app);
        assert!(app.dialog_state.trigger_secret.is_none());
        assert!(app.dialog_state.trigger_metadata.contains_key("wo-created"));
    }

    #[test]
    fn trigger_duplicate_converges_without_new_secret() {
        let mut app = test_app_with_project();
        let request_id = app.dialog_state.trigger_create_request.begin();
        let route = test_route(&app, request_id);
        apply_trigger_created(
            &mut app,
            request_id,
            route,
            "project-1".to_string(),
            "wo-created".to_string(),
            Some(trigger_metadata("active", "trigger-1")),
            None,
            true,
            None,
        );
        // Converged retry: no secret surface, active metadata + rotate hint.
        assert!(app.dialog_state.trigger_secret.is_none());
        assert!(app.dialog_state.trigger_metadata.contains_key("wo-created"));
        assert!(app
            .messages_state
            .toasts
            .iter()
            .any(|toast| toast.message.contains("rotate")));
    }

    #[test]
    fn trigger_failure_leaves_work_order_and_marks_setup_incomplete() {
        let mut app = test_app_with_project();
        prime_view_with_lane(&mut app);
        let request_id = app.dialog_state.trigger_create_request.begin();
        let route = test_route(&app, request_id);
        apply_trigger_created(
            &mut app,
            request_id,
            route,
            "project-1".to_string(),
            "wo-a".to_string(),
            None,
            None,
            false,
            Some("trigger_denied: forbidden".to_string()),
        );
        // WorkOrder rows intact; setup-incomplete recorded with retry hint.
        assert_eq!(app.dialog_state.task_view.rows.len(), 2);
        assert!(app
            .dialog_state
            .trigger_setup_error
            .get("wo-a")
            .unwrap()
            .contains("trigger_denied"));
        assert!(app
            .messages_state
            .toasts
            .iter()
            .any(|toast| toast.message.contains("setup incomplete")));
        let labels = trigger_labels_for_view(&app);
        assert!(labels.get("wo-a").unwrap().contains("setup incomplete"));
    }

    #[test]
    fn stale_trigger_success_drops_display_without_logging_secret() {
        let mut app = test_app_with_project();
        app.view_switch.bump_epoch();
        let request_id = app.dialog_state.trigger_create_request.begin();
        let route = test_route(&app, request_id);
        app.view_switch.bump_epoch();
        apply_trigger_created(
            &mut app,
            request_id,
            route,
            "project-1".to_string(),
            "wo-created".to_string(),
            Some(trigger_metadata("active", "trigger-1")),
            Some("cggtr_trigger-1.secret-abc".to_string()),
            false,
            None,
        );
        // Stale route: no foreground bearer, but active metadata cached
        // for later rotation. The secret is dropped, never logged.
        assert!(app.dialog_state.trigger_secret.is_none());
        assert!(app.dialog_state.trigger_metadata.contains_key("wo-created"));
        assert_ne!(app.ui_state.dialog, Dialog::TriggerSecret);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn trigger_setup_reconciles_before_creating() {
        let mut app = test_app_with_project();
        prime_view_with_lane(&mut app);
        // Known active trigger: setup refuses to mint a second credential.
        app.dialog_state.trigger_metadata.insert(
            "wo-a".to_string(),
            vec![trigger_metadata("active", "trigger-1")],
        );
        setup_trigger_for_selected(&mut app);
        assert!(!app.dialog_state.trigger_create_request.is_loading());
        assert!(app.dialog_state.trigger_setup_pending.is_empty());
        // Unknown state: setup pends and lists first (no immediate create).
        app.dialog_state.trigger_metadata.remove("wo-a");
        setup_trigger_for_selected(&mut app);
        assert!(app.dialog_state.trigger_setup_pending.contains("wo-a"));
        // List with no trigger drives creation with a stable key.
        let request_id = app.dialog_state.trigger_manage_request.begin();
        let route = test_route(&app, request_id);
        let generation = app.dialog_state.task_view.generation;
        // Finish the pending list probe started by setup (its request is
        // already in flight against the fake client in production; here
        // we simulate the empty-list completion directly).
        let _ = app.dialog_state.trigger_manage_request.finish(request_id);
        app.dialog_state
            .trigger_setup_pending
            .insert("wo-a".to_string());
        apply_triggers_listed(
            &mut app,
            request_id,
            route,
            generation,
            "project-1".to_string(),
            "wo-a".to_string(),
            Vec::new(),
            false,
            None,
        );
        assert!(app.dialog_state.trigger_create_request.is_loading());
    }

    #[test]
    fn trigger_bearer_never_in_view_debug_or_history() {
        let mut app = test_app_with_project();
        prime_view_with_lane(&mut app);
        let request_id = app.dialog_state.trigger_create_request.begin();
        let route = test_route(&app, request_id);
        apply_trigger_created(
            &mut app,
            request_id,
            route,
            "project-1".to_string(),
            "wo-created".to_string(),
            Some(trigger_metadata("active", "trigger-9")),
            Some("cggtr_trigger-9.top-secret-bearer".to_string()),
            false,
            None,
        );
        let view_debug = format!("{:?}", app.dialog_state.task_view);
        assert!(!view_debug.contains("top-secret-bearer"));
        assert!(!view_debug.contains("cggtr_"));
        let history_has_secret = app
            .session_state
            .history
            .iter()
            .any(|entry| entry.text.contains("cggtr_"));
        assert!(!history_has_secret);
        let transcript_has_secret = app
            .messages_state
            .messages
            .messages
            .iter()
            .any(|msg| format!("{msg:?}").contains("cggtr_"));
        assert!(!transcript_has_secret);
        // Scope loss forgets the bearer.
        clear_trigger_secret_on_scope_loss(&mut app);
        assert!(app.dialog_state.trigger_secret.is_none());
    }

    #[test]
    fn trigger_labels_cover_status_matrix() {
        let mut app = test_app_with_project();
        prime_view_with_lane(&mut app);
        // Active trigger label.
        app.dialog_state.trigger_metadata.insert(
            "wo-a".to_string(),
            vec![trigger_metadata_for("wo-a", "active", "trigger-1")],
        );
        // Revoked label.
        app.dialog_state.trigger_metadata.insert(
            "wo-b".to_string(),
            vec![trigger_metadata_for("wo-b", "revoked", "trigger-2")],
        );
        let labels = trigger_labels_for_view(&app);
        assert!(labels.get("wo-a").unwrap().contains("active"));
        assert!(!labels.get("wo-a").unwrap().contains("secret"));
        assert!(labels.get("wo-b").unwrap().contains("revoked"));
    }

    #[test]
    fn unauthorized_trigger_setup_fails_closed() {
        // Viewer/unauthorized caller: daemon denies, TUI records
        // setup-incomplete without a bearer and without revealing whether
        // an opaque trigger exists beyond the privacy-safe error shape.
        let mut app = test_app_with_project();
        prime_view_with_lane(&mut app);
        let request_id = app.dialog_state.trigger_create_request.begin();
        let route = test_route(&app, request_id);
        apply_trigger_created(
            &mut app,
            request_id,
            route,
            "project-1".to_string(),
            "wo-a".to_string(),
            None,
            None,
            false,
            Some("forbidden: missing project.schedule authority".to_string()),
        );
        assert!(app.dialog_state.trigger_secret.is_none());
        assert!(app
            .dialog_state
            .trigger_setup_error
            .get("wo-a")
            .unwrap()
            .contains("forbidden"));
        // No bearer in toasts or labels.
        for toast in app.messages_state.toasts.iter() {
            assert!(!toast.message.contains("cggtr_"));
        }
        let labels = trigger_labels_for_view(&app);
        assert!(!labels
            .get("wo-a")
            .unwrap_or(&String::new())
            .contains("cggtr_"));
    }

    #[test]
    fn stale_trigger_list_and_revoke_are_dropped() {
        let mut app = test_app_with_project();
        prime_view_with_lane(&mut app);
        // Stale list generation never clobbers fresh metadata.
        app.dialog_state.trigger_metadata.insert(
            "wo-a".to_string(),
            vec![trigger_metadata_for("wo-a", "active", "trigger-fresh")],
        );
        let request_id = app.dialog_state.trigger_manage_request.begin();
        let route = test_route(&app, request_id);
        let stale_generation = app.dialog_state.task_view.generation.wrapping_add(10);
        // Bump generation so the completion is superseded.
        let _new = app.dialog_state.task_view.begin_refresh("project-1");
        apply_triggers_listed(
            &mut app,
            request_id,
            route,
            stale_generation,
            "project-1".to_string(),
            "wo-a".to_string(),
            Vec::new(),
            false,
            None,
        );
        // Fresh metadata survives; stale empty list did not clear it.
        assert!(app
            .dialog_state
            .trigger_metadata
            .get("wo-a")
            .is_some_and(|list| list.iter().any(|t| t.trigger_id == "trigger-fresh")));
        // Stale revoke error never surfaces a bearer.
        let revoke_id = app.dialog_state.trigger_manage_request.begin();
        let revoke_route = test_route(&app, revoke_id);
        apply_trigger_revoked(
            &mut app,
            revoke_id,
            revoke_route,
            "project-1".to_string(),
            "wo-a".to_string(),
            "trigger-fresh".to_string(),
            None,
            Some("forbidden: stale scope".to_string()),
        );
        assert!(app.dialog_state.trigger_secret.is_none());
    }

    #[test]
    fn project_tab_switch_never_displays_bearer_in_wrong_scope() {
        let mut app = test_app_with_project();
        let request_id = app.dialog_state.trigger_create_request.begin();
        let route = test_route(&app, request_id);
        // Switch project before the trigger response arrives.
        if let Some(tab) = app.project_tabs.active_mut() {
            tab.project_id = Some("project-2".to_string());
        }
        apply_trigger_created(
            &mut app,
            request_id,
            route,
            "project-1".to_string(),
            "wo-created".to_string(),
            Some(trigger_metadata("active", "trigger-1")),
            Some("cggtr_trigger-1.secret-abc".to_string()),
            false,
            None,
        );
        // Wrong scope: no bearer display, metadata cached for the owning
        // project only.
        assert!(app.dialog_state.trigger_secret.is_none());
        assert_ne!(app.ui_state.dialog, Dialog::TriggerSecret);
    }
}
