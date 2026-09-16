//! Runtime-policy (approval/sandbox) TUI commands (M007).
//!
//! The TUI is never the source of security truth: every read goes through
//! the daemon-owned `ApprovalPreferenceGet` / `ExecutionPolicyGet`
//! contract and every mutation through `RuntimePolicySet`. The cached
//! view below is built exclusively from daemon DTOs; a failed update
//! leaves the previous effective policy cached and reports the error.
//!
//! FullHost confirmation is bound to the preference revision the user
//! confirmed (CAS `expected_revision`), never a permanent bypass token:
//! cancelling the dialog changes nothing, and a concurrent mode change
//! from another frontend fails the update with `preference_conflict`
//! instead of last-write-wins.
//!
//! Concurrency follows the `start_*/apply_*` pair pattern: every `start_*`
//! bumps the request generation and the matching `apply_*` drops stale
//! completions.

use codegg_core::approval::{ApprovalMode, SandboxProfile};

use crate::policy_surface::{
    format_policy_detail, format_policy_line, format_restore_summary, warning_for,
    EffectivePolicyView,
};
use crate::protocol::core::{
    CoreRequest, CoreResponse, ExecutionPolicySnapshotDto, RuntimePreferenceDto,
};
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// Why a snapshot was requested. Controls completion verbosity: startup
/// announces the restored preference once, selector flows show detail,
/// post-update refreshes stay quiet (the update toast already spoke).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicySnapshotReason {
    StartupRestore,
    SelectorRefresh,
    AfterUpdate,
}

/// A confirmed-but-not-yet-applied policy change awaiting explicit
/// confirmation dialog(s). Bound to the revision the user saw so the
/// daemon CAS rejects concurrent changes from other frontends.
#[derive(Debug, Clone)]
pub struct PendingPolicyConfirm {
    pub approval_mode: Option<String>,
    pub sandbox_profile: Option<String>,
    pub expected_revision: Option<u64>,
    pub confirmations_done: u8,
    pub confirmations_required: u8,
    pub warning_title: String,
    pub warning_body: String,
}

/// Frontend-local cache of the daemon-resolved effective policy. Display
/// only; authority stays daemon-side.
#[derive(Debug, Default)]
pub struct PolicyUiState {
    pub request: crate::tui::app::state::AsyncUiRequestState,
    pub cached: Option<EffectivePolicyView>,
    /// Last-used model preference as daemon state (connection, model).
    /// Displayed alongside the policy; the TUI manifest hint never
    /// overrides it.
    pub cached_model: Option<(Option<String>, Option<String>)>,
    /// Startup restore announced once; later refreshes stay quiet unless
    /// the operator asked (`/policy`) or an update just landed.
    pub restore_announced: bool,
}

impl PolicyUiState {
    /// One-line status-bar rendering of the cached effective state.
    pub fn status_line(&self) -> Option<String> {
        self.cached.as_ref().map(format_policy_line)
    }
}

/// Build the frontend-neutral view from daemon DTOs. Unknown wire names
/// degrade conservatively to the built-in defaults for display; the
/// daemon remains authoritative and the next refresh corrects the cache.
pub(crate) fn view_from_dtos(
    preference: &RuntimePreferenceDto,
    snapshot: &ExecutionPolicySnapshotDto,
) -> EffectivePolicyView {
    let requested_mode = ApprovalMode::parse(preference.approval_mode.as_str()).unwrap_or_default();
    let requested_profile =
        SandboxProfile::parse(preference.sandbox_profile.as_str()).unwrap_or_default();
    let effective_mode = ApprovalMode::parse(snapshot.approval_mode.as_str()).unwrap_or_default();
    let effective_profile =
        SandboxProfile::parse(snapshot.sandbox_profile.as_str()).unwrap_or_default();
    let enforcement_summary = snapshot
        .sandbox_enforcement
        .as_ref()
        .map(|enforcement| enforcement.summary.clone())
        .unwrap_or_default();
    EffectivePolicyView {
        requested_mode,
        effective_mode,
        requested_profile,
        effective_profile,
        enforcement_summary,
        reviewer_available: snapshot.reviewer_available,
        reviewer_detail: snapshot.reviewer_detail.clone(),
        revision: Some(preference.revision),
    }
}

/// Kick off an `ApprovalPreferenceGet` + `ExecutionPolicyGet` round-trip.
/// The completion lands as `TuiCommand::PolicySnapshotLoaded`; stale
/// completions are dropped on apply.
pub(crate) fn start_snapshot_refresh(app: &mut App, reason: PolicySnapshotReason) {
    let Some(core_client) = app.core_client.clone() else {
        if matches!(reason, PolicySnapshotReason::StartupRestore) {
            app.messages_state
                .toasts
                .warning("Runtime policy restore unavailable: no core connection");
        }
        return;
    };
    let request_id = app.policy_ui.request.begin();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "policy_snapshot",
        async move {
            let preference = match core_client
                .request(crate::core::new_request(
                    format!("policy-preference-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ApprovalPreferenceGet,
                ))
                .await
            {
                Ok(CoreResponse::ApprovalPreference { preference }) => Some(preference),
                Ok(CoreResponse::Error { code, message }) => {
                    return Some(TuiCommand::PolicySnapshotLoaded {
                        request_id,
                        reason,
                        preference: None,
                        snapshot: None,
                        error: Some(format!(
                            "Policy preference read failed ({}): {}",
                            code, message
                        )),
                    });
                }
                Ok(_) => {
                    return Some(TuiCommand::PolicySnapshotLoaded {
                        request_id,
                        reason,
                        preference: None,
                        snapshot: None,
                        error: Some(
                            "Policy preference read got an unexpected response".to_string(),
                        ),
                    });
                }
                Err(e) => {
                    return Some(TuiCommand::PolicySnapshotLoaded {
                        request_id,
                        reason,
                        preference: None,
                        snapshot: None,
                        error: Some(format!("Policy preference request failed: {}", e)),
                    });
                }
            };
            match core_client
                .request(crate::core::new_request(
                    format!("policy-effective-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ExecutionPolicyGet { session_id: None },
                ))
                .await
            {
                Ok(CoreResponse::ExecutionPolicy { snapshot }) => {
                    Some(TuiCommand::PolicySnapshotLoaded {
                        request_id,
                        reason,
                        preference,
                        snapshot: Some(snapshot),
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::PolicySnapshotLoaded {
                        request_id,
                        reason,
                        preference: None,
                        snapshot: None,
                        error: Some(format!(
                            "Effective policy read failed ({}): {}",
                            code, message
                        )),
                    })
                }
                Ok(_) => Some(TuiCommand::PolicySnapshotLoaded {
                    request_id,
                    reason,
                    preference: None,
                    snapshot: None,
                    error: Some("Effective policy read got an unexpected response".to_string()),
                }),
                Err(e) => Some(TuiCommand::PolicySnapshotLoaded {
                    request_id,
                    reason,
                    preference: None,
                    snapshot: None,
                    error: Some(format!("Effective policy request failed: {}", e)),
                }),
            }
        },
    );
}

/// Apply a `PolicySnapshotLoaded` completion. Drops stale completions.
pub(crate) fn apply_snapshot_loaded(
    app: &mut App,
    request_id: u64,
    reason: PolicySnapshotReason,
    preference: Option<RuntimePreferenceDto>,
    snapshot: Option<ExecutionPolicySnapshotDto>,
    error: Option<String>,
) {
    if let Some(err) = error {
        if app.policy_ui.request.fail(request_id, err.clone()) {
            app.messages_state
                .toasts
                .error(&format!("Runtime policy: {}", err));
        }
        return;
    }
    let (Some(preference), Some(snapshot)) = (preference, snapshot) else {
        if app.policy_ui.request.fail(
            request_id,
            "policy snapshot missing daemon payload".to_string(),
        ) {
            app.messages_state
                .toasts
                .error("Runtime policy: daemon returned no policy payload");
        }
        return;
    };
    if !app.policy_ui.request.finish(request_id) {
        return;
    }
    let view = view_from_dtos(&preference, &snapshot);
    app.policy_ui.cached_model = Some((
        preference.last_provider_connection_id.clone(),
        preference.last_model_id.clone(),
    ));
    app.policy_ui.cached = Some(view.clone());
    match reason {
        PolicySnapshotReason::StartupRestore => {
            if app.policy_ui.restore_announced {
                return;
            }
            app.policy_ui.restore_announced = true;
            let mut fallbacks = Vec::new();
            if view.requested_mode != view.effective_mode
                || view.requested_profile != view.effective_profile
            {
                fallbacks.push(
                    "stored preference was narrowed by a project or administrator ceiling"
                        .to_string(),
                );
            }
            let summary = format_restore_summary(&view, &fallbacks);
            match &app.policy_ui.cached_model {
                Some((connection, model)) => match (connection, model) {
                    (Some(connection), Some(model)) => {
                        app.messages_state.toasts.info(&format!(
                            "{} Last model preference (daemon state): {}/{}",
                            summary, connection, model
                        ));
                    }
                    _ => {
                        app.messages_state.toasts.info(&format!(
                            "{} No last-used model preference stored.",
                            summary
                        ));
                    }
                },
                None => app.messages_state.toasts.info(&summary),
            }
        }
        PolicySnapshotReason::SelectorRefresh => {
            for line in format_policy_detail(&view) {
                app.messages_state.toasts.info(&line);
            }
        }
        PolicySnapshotReason::AfterUpdate => {}
    }
}

/// Entry point for `/approval [mode]` and `/sandbox [profile]` with an
/// explicit target. With `None` (bare command) the cached state is shown
/// and a refresh is kicked off. With a target the warning matrix decides
/// whether the update applies immediately or needs explicit confirmation.
pub(crate) fn request_policy_change(
    app: &mut App,
    approval_mode: Option<ApprovalMode>,
    sandbox_profile: Option<SandboxProfile>,
) {
    if approval_mode.is_none() && sandbox_profile.is_none() {
        // Bare command: show the cached state, then refresh. Never opens
        // a confirmation dialog for the already-active selection.
        if let Some(line) = app.policy_ui.status_line() {
            app.messages_state
                .toasts
                .info(&format!("Runtime policy: {}", line));
        }
        start_snapshot_refresh(app, PolicySnapshotReason::SelectorRefresh);
        return;
    }
    let target_mode = approval_mode.unwrap_or_else(|| {
        app.policy_ui
            .cached
            .as_ref()
            .map(|view| view.effective_mode)
            .unwrap_or_default()
    });
    let target_profile = sandbox_profile.unwrap_or_else(|| {
        app.policy_ui
            .cached
            .as_ref()
            .map(|view| view.effective_profile)
            .unwrap_or_default()
    });
    let warning = warning_for(target_mode, target_profile);
    let expected_revision = app.policy_ui.cached.as_ref().and_then(|view| view.revision);
    if warning.confirmations_required == 0 {
        start_policy_update(
            app,
            approval_mode.map(|mode| mode.as_str().to_string()),
            sandbox_profile.map(|profile| profile.as_str().to_string()),
            expected_revision,
        );
        return;
    }
    app.dialog_state.pending_policy_confirm = Some(PendingPolicyConfirm {
        approval_mode: approval_mode.map(|mode| mode.as_str().to_string()),
        sandbox_profile: sandbox_profile.map(|profile| profile.as_str().to_string()),
        expected_revision,
        confirmations_done: 0,
        confirmations_required: warning.confirmations_required,
        warning_title: warning.title.clone(),
        warning_body: warning.body.join("\n"),
    });
    open_policy_confirm_dialog(
        app,
        &warning.title,
        &warning.body.join("\n"),
        1,
        warning.confirmations_required,
    );
}

fn open_policy_confirm_dialog(app: &mut App, title: &str, body: &str, step: u8, of: u8) {
    let title = if of > 1 {
        format!("{title} (confirmation {step} of {of})")
    } else {
        title.to_string()
    };
    app.push_dialog(
        crate::tui::app::Dialog::Confirm,
        Box::new(
            crate::tui::components::dialogs::confirm::ConfirmDialog::new(title, body.to_string()),
        ),
    );
}

/// Apply a `ConfirmResult` for a pending policy change. Returns `true`
/// when the message was consumed by the policy flow.
pub(crate) fn apply_policy_confirm_result(app: &mut App, confirmed: bool) -> bool {
    let Some(pending) = app.dialog_state.pending_policy_confirm.take() else {
        return false;
    };
    if !confirmed {
        // Cancelling a mode-change dialog changes nothing.
        app.messages_state.toasts.info("Runtime policy unchanged.");
        return true;
    }
    let done = pending.confirmations_done + 1;
    if done < pending.confirmations_required {
        app.dialog_state.pending_policy_confirm = Some(PendingPolicyConfirm {
            confirmations_done: done,
            ..pending.clone()
        });
        // The strongest combination (Yolo + FullHost) requires a second
        // explicit confirmation after the first.
        open_policy_confirm_dialog(
            app,
            "Confirm again: Yolo with full host access",
            &pending.warning_body,
            done + 1,
            pending.confirmations_required,
        );
        return true;
    }
    start_policy_update(
        app,
        pending.approval_mode,
        pending.sandbox_profile,
        pending.expected_revision,
    );
    true
}

/// Issue a `RuntimePolicySet` for the confirmed values, then refresh the
/// effective snapshot in the same task. The completion lands as
/// `TuiCommand::PolicyUpdateFinished`.
pub(crate) fn start_policy_update(
    app: &mut App,
    approval_mode: Option<String>,
    sandbox_profile: Option<String>,
    expected_revision: Option<u64>,
) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Runtime policy update unavailable: no core connection");
        return;
    };
    let request_id = app.policy_ui.request.begin();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "policy_update",
        async move {
            let preference = match core_client
                .request(crate::core::new_request(
                    format!("policy-set-{}", uuid::Uuid::new_v4()),
                    CoreRequest::RuntimePolicySet {
                        approval_mode,
                        sandbox_profile,
                        expected_revision,
                    },
                ))
                .await
            {
                Ok(CoreResponse::ApprovalPreference { preference }) => preference,
                Ok(CoreResponse::Error { code, message }) => {
                    return Some(TuiCommand::PolicyUpdateFinished {
                        request_id,
                        preference: None,
                        snapshot: None,
                        error: Some(policy_update_error_hint(&code, &message)),
                    });
                }
                Ok(_) => {
                    return Some(TuiCommand::PolicyUpdateFinished {
                        request_id,
                        preference: None,
                        snapshot: None,
                        error: Some("Policy update got an unexpected response".to_string()),
                    });
                }
                Err(e) => {
                    return Some(TuiCommand::PolicyUpdateFinished {
                        request_id,
                        preference: None,
                        snapshot: None,
                        error: Some(format!("Policy update request failed: {}", e)),
                    });
                }
            };
            // The set response carries the new revision; refresh the
            // effective snapshot (enforcement + reviewer availability)
            // so the cache never shows a request as enforcement truth.
            match core_client
                .request(crate::core::new_request(
                    format!("policy-effective-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ExecutionPolicyGet { session_id: None },
                ))
                .await
            {
                Ok(CoreResponse::ExecutionPolicy { snapshot }) => {
                    Some(TuiCommand::PolicyUpdateFinished {
                        request_id,
                        preference: Some(preference),
                        snapshot: Some(snapshot),
                        error: None,
                    })
                }
                // The preference landed; only the refresh failed. Report
                // the preference and let the next snapshot fill in the
                // enforcement detail.
                Ok(_) | Err(_) => Some(TuiCommand::PolicyUpdateFinished {
                    request_id,
                    preference: Some(preference),
                    snapshot: None,
                    error: None,
                }),
            }
        },
    );
}

/// Apply a `PolicyUpdateFinished` completion. Failed updates leave the
/// previous effective policy cached and report the error; the cache is
/// only replaced by daemon-confirmed state.
pub(crate) fn apply_policy_updated(
    app: &mut App,
    request_id: u64,
    preference: Option<RuntimePreferenceDto>,
    snapshot: Option<ExecutionPolicySnapshotDto>,
    error: Option<String>,
) {
    if let Some(err) = error {
        if app.policy_ui.request.fail(request_id, err.clone()) {
            app.messages_state
                .toasts
                .error(&format!("Runtime policy: {}", err));
        }
        return;
    }
    let Some(preference) = preference else {
        if app.policy_ui.request.fail(
            request_id,
            "policy update missing daemon payload".to_string(),
        ) {
            app.messages_state
                .toasts
                .error("Runtime policy: daemon returned no policy payload");
        }
        return;
    };
    if !app.policy_ui.request.finish(request_id) {
        return;
    }
    // Prefer the refreshed effective snapshot; fall back to rendering the
    // confirmed preference with unknown enforcement (marked as such).
    let view = match snapshot {
        Some(ref snapshot) => view_from_dtos(&preference, snapshot),
        None => EffectivePolicyView {
            requested_mode: ApprovalMode::parse(preference.approval_mode.as_str())
                .unwrap_or_default(),
            effective_mode: ApprovalMode::parse(preference.approval_mode.as_str())
                .unwrap_or_default(),
            requested_profile: SandboxProfile::parse(preference.sandbox_profile.as_str())
                .unwrap_or_default(),
            effective_profile: SandboxProfile::parse(preference.sandbox_profile.as_str())
                .unwrap_or_default(),
            enforcement_summary: String::new(),
            reviewer_available: false,
            reviewer_detail: "effective snapshot refresh pending".to_string(),
            revision: Some(preference.revision),
        },
    };
    app.policy_ui.cached_model = Some((
        preference.last_provider_connection_id.clone(),
        preference.last_model_id.clone(),
    ));
    app.policy_ui.cached = Some(view.clone());
    let line = format_policy_line(&view);
    if view.is_degraded() {
        app.messages_state.toasts.warning(&format!(
            "Runtime policy updated: {}. Effective state differs from the request — see /policy.",
            line
        ));
    } else {
        app.messages_state
            .toasts
            .info(&format!("Runtime policy updated: {}", line));
    }
    for detail in format_policy_detail(&view) {
        app.messages_state.toasts.info(&detail);
    }
}

fn policy_update_error_hint(code: &str, message: &str) -> String {
    if code == "preference_conflict" {
        format!(
            "Policy update rejected ({code}): {message}. Another frontend changed the policy; run /policy to reload and retry."
        )
    } else {
        format!("Policy update failed ({}): {}", code, message)
    }
}
