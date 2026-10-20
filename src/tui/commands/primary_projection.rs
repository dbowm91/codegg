//! Default `/core` session projection lifecycle.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;
use codegg_protocol::projection::replay::{
    ProjectionSnapshotBundle, ProjectionStreamKind, ProjectionSubscriptionId,
};

pub(crate) struct PrimaryProjectionCompletion {
    pub request_id: u64,
    pub project_id: String,
    pub session_id: String,
    pub tab_id: Option<String>,
    pub subscription_id: Option<ProjectionSubscriptionId>,
    pub cursor: Option<codegg_protocol::projection::replay::ProjectionCursor>,
    pub projection: Option<(
        codegg_protocol::projection::replay::ProjectionStreamDescriptor,
        ProjectionSnapshotBundle,
    )>,
    pub error: Option<String>,
    pub unsupported: bool,
}

pub(crate) fn after_event(
    app: &mut App,
    subscription_id: &ProjectionSubscriptionId,
    outcome: codegg_protocol::projection::controller::ControllerApplyOutcome,
) {
    if matches!(
        outcome,
        codegg_protocol::projection::controller::ControllerApplyOutcome::ResyncRequested { .. }
    ) {
        if let (Some(project_id), Some(session_id)) = (
            app.active_project_id().map(str::to_owned),
            app.active_session_id().map(str::to_owned),
        ) {
            tracing::warn!(%session_id, "primary projection requested resync; rebuilding from an authoritative snapshot");
            start_subscription(app, project_id, session_id);
        }
        return;
    }
    if let Some(ack) = app
        .projection_client
        .controller_mut()
        .try_ack(subscription_id)
    {
        let Some(client) = app.core_client.clone() else {
            return;
        };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let request = crate::core::new_request(
                    uuid::Uuid::new_v4().to_string(),
                    CoreRequest::ProjectionAck { ack },
                );
                if let Err(error) = client.request(request).await {
                    tracing::warn!(%error, "failed to acknowledge primary projection cursor");
                }
            });
        }
    }
}

pub(crate) fn bind_active_session(app: &mut App) {
    let active_session_id = app.active_session_id().map(str::to_owned);
    if app.projection_client.primary_session_id() != active_session_id.as_deref() {
        // Rows are session-scoped. A missing new snapshot during a session
        // switch must not leave the previous session's children visible or
        // keep the composer armed to one of them.
        app.ui_state.subagents.set_rows(Vec::new());
        app.prompt_state.subagent_addressee = None;
    }
    let Some(session_id) = active_session_id else {
        let old = app.projection_client.clear_primary_binding();
        unsubscribe(app, old);
        return;
    };
    let Some(project_id) = app.active_project_id().map(str::to_owned) else {
        let old = app.projection_client.clear_primary_binding();
        unsubscribe(app, old);
        let request_id = app.projection_client.begin_primary_subscription(
            String::new(),
            session_id,
            app.project_tabs
                .active_tab_id()
                .map(|id| id.as_str().to_string()),
        );
        app.projection_client.finish_primary_subscription(
            request_id,
            None,
            None,
            Some("The active session has no project identity".into()),
        );
        return;
    };
    start_subscription(app, project_id, session_id);
}

fn unsubscribe(app: &App, subscription_id: Option<ProjectionSubscriptionId>) {
    let (Some(client), Some(subscription_id)) = (app.core_client.clone(), subscription_id) else {
        return;
    };
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            let request = crate::core::new_request(
                uuid::Uuid::new_v4().to_string(),
                CoreRequest::ProjectionUnsubscribe { subscription_id },
            );
            let _ = client.request(request).await;
        });
    }
}

pub(crate) fn start_subscription(app: &mut App, project_id: String, session_id: String) {
    let tab_id = app
        .project_tabs
        .active_tab_id()
        .map(|id| id.as_str().to_string());
    let previous = app.projection_client.primary_subscription_id().cloned();
    // Starting a new authoritative fetch invalidates rows derived from the
    // snapshot being replaced. Keep the loading state visible until the new
    // snapshot installs rather than painting potentially stale children.
    app.ui_state.subagents.set_rows(Vec::new());
    app.prompt_state.subagent_addressee = None;
    if let Some(previous) = previous.as_ref() {
        app.projection_client.controller_mut().unsubscribe(previous);
    }
    let request_id = app.projection_client.begin_primary_subscription(
        project_id.clone(),
        session_id.clone(),
        tab_id.clone(),
    );
    let Some(client) = app.core_client.clone() else {
        app.projection_client.finish_primary_subscription(
            request_id,
            None,
            None,
            Some("Core unavailable — check daemon status with /doctor".into()),
        );
        return;
    };
    let Some(tx) = app.tui_cmd_tx.clone() else {
        app.projection_client.finish_primary_subscription(
            request_id,
            None,
            None,
            Some("TUI event loop unavailable".into()),
        );
        return;
    };
    if tokio::runtime::Handle::try_current().is_err() {
        app.projection_client.finish_primary_subscription(
            request_id,
            None,
            None,
            Some("Projection subscription requires the TUI runtime".into()),
        );
        return;
    }
    spawn_registered_tui_task(
        Some(tx),
        &mut app.task_registry,
        TuiTaskKind::Command,
        "primary_projection_subscribe",
        async move {
            if let Some(subscription_id) = previous {
                let request = crate::core::new_request(
                    uuid::Uuid::new_v4().to_string(),
                    CoreRequest::ProjectionUnsubscribe { subscription_id },
                );
                let _ = client.request(request).await;
            }
            let capabilities = crate::core::new_request(
                uuid::Uuid::new_v4().to_string(),
                CoreRequest::ProjectionCapabilities,
            );
            let supported = matches!(
                client.request(capabilities).await,
                Ok(CoreResponse::ProjectionCapabilitiesResponse {
                    supported: true,
                    ..
                })
            );
            if !supported {
                return Some(TuiCommand::PrimaryProjectionSubscribed {
                    request_id,
                    project_id,
                    session_id,
                    tab_id,
                    subscription_id: None,
                    cursor: None,
                    projection: None,
                    error: Some("Daemon does not support session projections".into()),
                    unsupported: true,
                });
            }
            let request = crate::core::new_request(
                uuid::Uuid::new_v4().to_string(),
                CoreRequest::ProjectionSubscribe {
                    request: codegg_protocol::projection::replay::ProjectionSubscriptionRequest {
                        scope: ProjectionStreamKind::Session,
                        scope_id: session_id.clone(),
                        cursor: None,
                        projection_version: 1,
                    },
                },
            );
            let (subscription_id, cursor, projection, error) = match client.request(request).await {
                Ok(CoreResponse::ProjectionSubscribed {
                    subscription_id,
                    cursor,
                    descriptor,
                    snapshot,
                    ..
                }) => (
                    Some(subscription_id),
                    Some(cursor),
                    Some((descriptor, snapshot)),
                    None,
                ),
                Ok(CoreResponse::Error { code, message }) => {
                    (None, None, None, Some(format!("{code}: {message}")))
                }
                Ok(other) => (
                    None,
                    None,
                    None,
                    Some(format!("Unexpected response: {other:?}")),
                ),
                Err(error) => (
                    None,
                    None,
                    None,
                    Some(format!("Projection subscribe failed: {error}")),
                ),
            };
            Some(TuiCommand::PrimaryProjectionSubscribed {
                request_id,
                project_id,
                session_id,
                tab_id,
                subscription_id,
                cursor,
                projection,
                error,
                unsupported: false,
            })
        },
    );
}

pub(crate) fn apply_subscribed(app: &mut App, completion: PrimaryProjectionCompletion) {
    let PrimaryProjectionCompletion {
        request_id,
        project_id,
        session_id,
        tab_id,
        subscription_id,
        cursor: _cursor,
        projection,
        error,
        unsupported,
    } = completion;
    let is_current = app.projection_client.primary_request_matches(
        request_id,
        &project_id,
        &session_id,
        tab_id.as_deref(),
    ) && app.active_session_id() == Some(session_id.as_str())
        && app.active_project_id() == Some(project_id.as_str())
        && app.project_tabs.active_tab_id().map(|id| id.as_str()) == tab_id.as_deref();
    if !is_current {
        if let (Some(client), Some(subscription_id)) = (app.core_client.clone(), subscription_id) {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let request = crate::core::new_request(
                        uuid::Uuid::new_v4().to_string(),
                        CoreRequest::ProjectionUnsubscribe { subscription_id },
                    );
                    let _ = client.request(request).await;
                });
            }
        }
        return;
    }
    if unsupported {
        let reason = error.unwrap_or_else(|| "Projection unavailable".into());
        app.projection_client.finish_primary_subscription(
            request_id,
            None,
            None,
            Some(reason.clone()),
        );
        tracing::warn!(%reason, "default session projection is unavailable");
        return;
    }
    let Some((descriptor, bundle)) = projection else {
        let reason = error.unwrap_or_else(|| "Projection subscribe returned no snapshot".into());
        app.projection_client.finish_primary_subscription(
            request_id,
            None,
            None,
            Some(reason.clone()),
        );
        tracing::warn!(%reason, "default session projection subscribe failed");
        return;
    };
    if descriptor.session_id.as_deref() != Some(session_id.as_str())
        || descriptor.project_id != project_id
        || !matches!(descriptor.kind, ProjectionStreamKind::Session)
    {
        let reason = "Projection descriptor does not match the active session".to_string();
        app.projection_client.finish_primary_subscription(
            request_id,
            None,
            None,
            Some(reason.clone()),
        );
        if let (Some(client), Some(subscription_id)) = (app.core_client.clone(), subscription_id) {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let request = crate::core::new_request(
                        uuid::Uuid::new_v4().to_string(),
                        CoreRequest::ProjectionUnsubscribe { subscription_id },
                    );
                    let _ = client.request(request).await;
                });
            }
        }
        tracing::warn!(%session_id, %project_id, "rejected mismatched default projection descriptor");
        return;
    }
    let Some(workspace_id) = descriptor.workspace_id.clone() else {
        app.projection_client.finish_primary_subscription(
            request_id,
            None,
            None,
            Some("Projection descriptor lacks workspace identity".into()),
        );
        unsubscribe(app, subscription_id);
        return;
    };
    let Some(subscription_id) = subscription_id else {
        app.projection_client.finish_primary_subscription(
            request_id,
            None,
            None,
            Some("Projection subscription id is missing".into()),
        );
        return;
    };
    let partial_job_state = match &bundle {
        ProjectionSnapshotBundle::One { snapshot } => snapshot
            .diagnostics
            .iter()
            .find(|diagnostic| {
                matches!(
                    diagnostic.code.as_str(),
                    "job_snapshot_unavailable" | "job_snapshot_truncated"
                )
            })
            .map(|diagnostic| diagnostic.message.clone()),
        _ => None,
    };
    let initial = codegg_protocol::projection::snapshot::SessionProjectionSnapshot::empty(
        &session_id,
        &project_id,
        &workspace_id,
    );
    // `on_reconnect` demotes the controller to Unsupported until a fresh
    // capability round-trip completes. The daemon response above is that
    // round-trip; restore local negotiation before installing its snapshot.
    app.projection_client.controller_mut().negotiate(Some(
        &codegg_protocol::projection::caps::ProjectionCapabilities::default(),
    ));
    match app.projection_client.controller_mut().install_replay(
        subscription_id.clone(),
        descriptor.clone(),
        Vec::new(),
        Some(bundle),
        initial,
    ) {
        codegg_protocol::projection::controller::ControllerSubscribeOutcome::Failed { reason } => {
            app.projection_client.finish_primary_subscription(
                request_id,
                None,
                None,
                Some(format!("{reason:?}")),
            );
            unsubscribe(app, Some(subscription_id));
        }
        _ => {
            let stream_id = descriptor.stream_id.clone();
            app.projection_client.finish_primary_subscription(
                request_id,
                Some(subscription_id.clone()),
                Some(stream_id.clone()),
                partial_job_state,
            );
            let (pending, overflowed) = app
                .projection_client
                .take_pending_envelopes(&subscription_id, &stream_id);
            let tab_id = tab_id.as_deref().unwrap_or("default");
            for envelope in pending {
                let _ = app.apply_projection_envelope_on_stream(
                    &subscription_id,
                    &stream_id,
                    tab_id,
                    envelope,
                );
            }
            if overflowed {
                tracing::warn!(%session_id, "primary projection pre-install buffer overflowed; rebuilding snapshot");
                start_subscription(app, project_id, session_id);
            }
        }
    }
}
