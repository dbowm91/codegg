use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::core::daemon::CoreDaemon;
use crate::core::event_log::{event_matches_filter, EventFilter};
use crate::error::AppError;
use crate::protocol::core::{CoreEvent, CoreResponse, EventEnvelope};
use crate::protocol::frames::{CoreFrame, ServerCapabilities, ServerHello};
use codegg_protocol::projection::replay::ProjectionSubscriptionId;

use super::projection::{
    bounded_critical_delivery, CriticalDeliveryError, OwnedProjectionSubscription,
    ProjectionConnectionState, ProjectionLifecycleBoundary, ProjectionLifecycleSeam,
};

/// Bind a Unix-domain socket listener to `endpoint`. Returns the bound
/// `UnixListener` plus the absolute path. Used by the singleton lifecycle
/// path so the caller can decide when to bind (after lock acquisition)
/// and can pass a pre-bound listener to [`run_core_socket_with_listener`].
pub fn bind_listener(endpoint: &Path) -> Result<UnixListener, AppError> {
    if let Some(parent) = endpoint.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    UnixListener::bind(endpoint).map_err(|e| {
        AppError::Other(anyhow::anyhow!(
            "failed to bind socket '{}': {}",
            endpoint.display(),
            e
        ))
    })
}

/// Serve a pre-bound listener until `shutdown` is cancelled. This is the
/// lifecycle-aware variant of [`run_core_socket`]; it never deletes paths
/// it does not own and stops accepting new connections cleanly when the
/// token fires.
pub async fn run_core_socket_with_listener(
    daemon: Arc<CoreDaemon>,
    listener: UnixListener,
    endpoint: &Path,
    shutdown: CancellationToken,
) -> Result<(), AppError> {
    run_core_socket_with_listener_and_seam(
        daemon,
        listener,
        endpoint,
        shutdown,
        ProjectionLifecycleSeam::default(),
    )
    .await
}

/// Serve a pre-bound listener with an adapter-local lifecycle seam. The seam
/// is normally a no-op; integration tests use it to pause or fail at the
/// response/receiver/activation boundaries without global mutable hooks.
pub async fn run_core_socket_with_listener_and_seam(
    daemon: Arc<CoreDaemon>,
    listener: UnixListener,
    endpoint: &Path,
    shutdown: CancellationToken,
    lifecycle_seam: ProjectionLifecycleSeam,
) -> Result<(), AppError> {
    tracing::info!("Core daemon listening on {}", endpoint.display());
    let mut clients = JoinSet::new();

    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => {
                tracing::info!("Core daemon accept loop cancelled");
                break;
            }
            Some(result) = clients.join_next() => {
                if let Err(error) = result {
                    tracing::warn!("Core daemon client task terminated abnormally: {}", error);
                }
            }
            accept = listener.accept() => {
                let (stream, _addr) = accept
                    .map_err(|e| AppError::Other(anyhow::anyhow!("accept failed: {}", e)))?;
                let daemon = Arc::clone(&daemon);
                let client_shutdown = shutdown.child_token();
                let client_lifecycle_seam = lifecycle_seam.clone();
                clients.spawn(async move {
                    if let Err(e) = handle_client(
                        daemon,
                        stream,
                        client_shutdown,
                        client_lifecycle_seam,
                    )
                    .await
                    {
                        tracing::error!("Client handler error: {}", e);
                    }
                });
            }
        }
    }
    // Let connection handlers observe shutdown and perform their own cleanup.
    shutdown.cancel();
    while let Some(result) = clients.join_next().await {
        if let Err(error) = result {
            tracing::warn!(
                "Core daemon client cleanup terminated abnormally: {}",
                error
            );
        }
    }
    Ok(())
}

/// Backwards-compatible serve entry point. Binds and serves without
/// lifecycle cancellation. The singleton-lifecycle path uses
/// [`run_core_socket_with_listener`] instead.
pub async fn run_core_socket(daemon: Arc<CoreDaemon>, endpoint: &str) -> Result<(), AppError> {
    let listener = bind_listener(std::path::Path::new(endpoint))?;
    run_core_socket_with_listener(
        daemon,
        listener,
        std::path::Path::new(endpoint),
        CancellationToken::new(),
    )
    .await
}

async fn handle_client(
    daemon: Arc<CoreDaemon>,
    stream: tokio::net::UnixStream,
    shutdown: CancellationToken,
    lifecycle_seam: ProjectionLifecycleSeam,
) -> Result<(), AppError> {
    let (read_half, write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let writer = Arc::new(tokio::sync::Mutex::new(write_half));

    // Negotiated per-connection client identity and per-connection filter set.
    // `client_id` is stable for the lifetime of the connection; `filters` is
    // mutated by the read loop on `Subscribe` frames and read by the live
    // event forwarder.
    let client_id = format!("client-{}", uuid::Uuid::new_v4());
    let filters: Arc<RwLock<Vec<EventFilter>>> = Arc::new(RwLock::new(Vec::new()));
    let projection_state = Arc::new(Mutex::new(
        ProjectionConnectionState::new_with_lifecycle_seam(client_id.clone(), lifecycle_seam),
    ));
    let connection_cancel = shutdown.child_token();

    let event_rx = daemon.event_log.subscribe();
    let writer_for_forwarder = Arc::clone(&writer);
    let filters_for_forwarder = Arc::clone(&filters);
    let raw_cancellation = connection_cancel.clone();
    let raw_forwarder = tokio::spawn(async move {
        forward_events(
            event_rx,
            writer_for_forwarder,
            filters_for_forwarder,
            raw_cancellation.clone(),
        )
        .await;
        // A writer failure (or a closed event log) must wake the reader loop
        // so it follows the same cleanup path as EOF and listener shutdown.
        raw_cancellation.cancel();
    });

    let mut line = String::new();
    loop {
        line.clear();
        let read_result = tokio::select! {
            _ = connection_cancel.cancelled() => break,
            result = reader.read_line(&mut line) => result,
        };
        match read_result {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(frame) = serde_json::from_str::<CoreFrame>(trimmed) {
                    match frame {
                        CoreFrame::Request(envelope) => {
                            let request_id = envelope.request_id.clone();
                            let projection_request = matches!(
                                &envelope.payload,
                                crate::protocol::core::CoreRequest::ProjectionSubscribe { .. }
                                    | crate::protocol::core::CoreRequest::ProjectionResume { .. }
                                    | crate::protocol::core::CoreRequest::ProjectionAck { .. }
                                    | crate::protocol::core::CoreRequest::ProjectionUnsubscribe { .. }
                                    | crate::protocol::core::CoreRequest::ProjectionSnapshotGet { .. }
                                    | crate::protocol::core::CoreRequest::ProjectionArtifactRead { .. }
                                    | crate::protocol::core::CoreRequest::ProjectionArtifactList { .. }
                            );
                            let projection_allowed = projection_state.lock().await.mode()
                                == super::projection::ProjectionConnectionMode::ProjectionPrimary;
                            let artifact_scope_owned = match &envelope.payload {
                                crate::protocol::core::CoreRequest::ProjectionArtifactRead {
                                    project_id,
                                    ..
                                }
                                | crate::protocol::core::CoreRequest::ProjectionArtifactList {
                                    project_id,
                                } => Some(projection_state.lock().await.owns_project(project_id)),
                                _ => None,
                            };
                            let mut artifact_read_started = if artifact_scope_owned == Some(true) {
                                projection_state.lock().await.try_begin_artifact_read()
                            } else {
                                false
                            };
                            let mut response = if projection_request && !projection_allowed {
                                if artifact_read_started {
                                    projection_state.lock().await.end_artifact_read();
                                    artifact_read_started = false;
                                }
                                crate::protocol::core::CoreResponse::Error {
                                    code: "projection_capabilities_required".into(),
                                    message: "send a projection-capable ClientHello before projection operations".into(),
                                }
                            } else if artifact_scope_owned == Some(false) {
                                crate::protocol::core::CoreResponse::Error {
                                    code: "projection_scope_not_owned".into(),
                                    message:
                                        "projection artifact scope is not owned by this connection"
                                            .into(),
                                }
                            } else if artifact_scope_owned == Some(true) && !artifact_read_started {
                                crate::protocol::core::CoreResponse::Error {
                                    code: "projection_artifact_read_limit".into(),
                                    message: "projection artifact read limit exceeded".into(),
                                }
                            } else {
                                match daemon.handle_request_for_client(envelope, &client_id).await {
                                    Ok(resp) => resp,
                                    Err(e) => crate::protocol::core::CoreResponse::Error {
                                        code: "handler_error".to_string(),
                                        message: e.to_string(),
                                    },
                                }
                            };
                            if artifact_read_started {
                                projection_state.lock().await.end_artifact_read();
                            }
                            // Install the daemon-issued receiver before the
                            // response is released. This closes the replay /
                            // live handoff window and makes the receiver the
                            // only source of projection-private events.
                            let lifecycle_seam = projection_state.lock().await.lifecycle_seam();
                            let response_subscription_id = match &response {
                                CoreResponse::ProjectionSubscribed {
                                    subscription_id, ..
                                }
                                | CoreResponse::ProjectionReplay {
                                    subscription_id: Some(subscription_id),
                                    ..
                                } => Some(subscription_id.clone()),
                                _ => None,
                            };
                            let mut setup_failed = false;
                            if let Some(subscription_id) = &response_subscription_id {
                                let cancellation = projection_state.lock().await.cancellation();
                                if lifecycle_seam
                                    .checkpoint(
                                        ProjectionLifecycleBoundary::AfterDaemonSubscriptionCreation,
                                        &cancellation,
                                    )
                                    .await
                                    .is_err()
                                {
                                    cleanup_projection_subscription(
                                        &daemon,
                                        &projection_state,
                                        subscription_id,
                                        &client_id,
                                    )
                                    .await;
                                    setup_failed = true;
                                }
                            }
                            let install_result = if setup_failed {
                                None
                            } else {
                                match &response {
                                    CoreResponse::ProjectionSubscribed {
                                        subscription_id,
                                        descriptor,
                                        cursor,
                                        retention_floor_seq,
                                        ..
                                    } => Some(
                                        install_projection_receiver(
                                            &daemon,
                                            &writer,
                                            &projection_state,
                                            subscription_id,
                                            descriptor,
                                            cursor,
                                            *retention_floor_seq,
                                            &client_id,
                                        )
                                        .await,
                                    ),
                                    CoreResponse::ProjectionReplay {
                                        subscription_id: Some(subscription_id),
                                        batch,
                                    } => {
                                        let cursor = batch.next_cursor.clone().unwrap_or_else(|| {
                                            crate::protocol::projection::replay::ProjectionCursor {
                                                stream_id: batch.descriptor.stream_id.clone(),
                                                event_seq: batch.current_high_water,
                                                projection_version: batch.descriptor.projection_version,
                                            }
                                        });
                                        Some(
                                            install_projection_receiver(
                                                &daemon,
                                                &writer,
                                                &projection_state,
                                                subscription_id,
                                                &batch.descriptor,
                                                &cursor,
                                                batch.descriptor.retention_floor_seq,
                                                &client_id,
                                            )
                                            .await,
                                        )
                                    }
                                    _ => None,
                                }
                            };
                            if install_result == Some(false) {
                                response = CoreResponse::Error {
                                    code: "projection_receiver_install_failed".into(),
                                    message: "projection live receiver could not be installed"
                                        .into(),
                                };
                            } else if setup_failed {
                                response = CoreResponse::Error {
                                    code: "projection_subscription_setup_failed".into(),
                                    message: "projection subscription setup was cancelled".into(),
                                };
                            } else if install_result == Some(true) && {
                                let cancellation = projection_state.lock().await.cancellation();
                                lifecycle_seam
                                    .checkpoint(
                                        ProjectionLifecycleBoundary::AfterReceiverInstallation,
                                        &cancellation,
                                    )
                                    .await
                                    .is_err()
                            } {
                                if let Some(subscription_id) = &response_subscription_id {
                                    cleanup_projection_subscription(
                                        &daemon,
                                        &projection_state,
                                        subscription_id,
                                        &client_id,
                                    )
                                    .await;
                                }
                                response = CoreResponse::Error {
                                    code: "projection_receiver_setup_failed".into(),
                                    message: "projection receiver setup was cancelled".into(),
                                };
                            }
                            let frame = CoreFrame::Response {
                                request_id,
                                response: Box::new(response),
                            };
                            let cancellation = projection_state.lock().await.cancellation();
                            let delivery = if response_subscription_id.is_some() {
                                staged_socket_critical_delivery(
                                    &writer,
                                    &frame,
                                    &cancellation,
                                    &lifecycle_seam,
                                )
                                .await
                            } else {
                                bounded_critical_delivery(
                                    &cancellation,
                                    send_frame(&writer, &frame),
                                )
                                .await
                            };
                            if let Err(error) = delivery {
                                if let Some(subscription_id) = &response_subscription_id {
                                    cleanup_projection_subscription(
                                        &daemon,
                                        &projection_state,
                                        subscription_id,
                                        &client_id,
                                    )
                                    .await;
                                }
                                tracing::warn!(
                                    "critical Unix-socket response delivery failed: {}",
                                    error
                                );
                                break;
                            }
                            if let Some(subscription_id) = response_subscription_id {
                                if lifecycle_seam
                                    .checkpoint(
                                        ProjectionLifecycleBoundary::BeforeActivation,
                                        &cancellation,
                                    )
                                    .await
                                    .is_err()
                                {
                                    cleanup_projection_subscription(
                                        &daemon,
                                        &projection_state,
                                        &subscription_id,
                                        &client_id,
                                    )
                                    .await;
                                    tracing::warn!(
                                        "projection activation cancelled before Unix-socket response commit"
                                    );
                                    break;
                                }
                                let activation = projection_state
                                    .lock()
                                    .await
                                    .activate_after_delivery(&subscription_id);
                                if let Err(error) = activation {
                                    cleanup_projection_subscription(
                                        &daemon,
                                        &projection_state,
                                        &subscription_id,
                                        &client_id,
                                    )
                                    .await;
                                    tracing::warn!(
                                        "projection activation failed after critical response delivery: {:?}",
                                        error
                                    );
                                    break;
                                }
                            }
                        }
                        CoreFrame::Subscribe {
                            client_id: _sub_client_id,
                            session_id,
                            from_event_seq,
                            ..
                        } => {
                            // Build the new filter from this Subscribe frame.
                            //
                            // A session_id produces a session-scoped filter
                            // (events for that session, plus sessionless
                            // events when `include_global: true`). The absence
                            // of session_id yields a global-only filter
                            // (`include_global: false`); it does NOT match
                            // every session. An all-sessions subscription
                            // would require a distinct protocol field.
                            let new_filter = if let Some(sid) = session_id.clone() {
                                EventFilter {
                                    session_id: Some(sid),
                                    client_id: None,
                                    include_global: true,
                                }
                            } else {
                                EventFilter {
                                    session_id: None,
                                    client_id: None,
                                    include_global: false,
                                }
                            };
                            // Append to the connection's filter set. The wire
                            // format currently only advertises one filter per
                            // Subscribe frame, but the connection state holds
                            // a `Vec` so future wire extensions (e.g. a
                            // session_id list) can add to it without changing
                            // the forwarder logic.
                            {
                                let mut guard = filters.write().await;
                                guard.push(new_filter.clone());
                            }
                            if let Some(ref sid) = new_filter.session_id {
                                daemon.clients.attach_session(&client_id, sid);
                            }
                            let from = from_event_seq.unwrap_or(0);
                            let events = daemon.event_log.replay_from(from, &new_filter).await;
                            let mut w = writer.lock().await;
                            for event in events {
                                let frame = CoreFrame::Event(event);
                                if let Ok(json) = serde_json::to_string(&frame) {
                                    let _ = w.write_all(json.as_bytes()).await;
                                    let _ = w.write_all(b"\n").await;
                                }
                            }
                            let _ = w.flush().await;
                            // `_sub_client_id` is a wire field kept for
                            // compatibility; the daemon-issued id in
                            // `client_id` (the closure variable) is the one
                            // it actually trusts.
                        }
                        CoreFrame::ClientHello(hello) => {
                            let projection_supported = hello.capabilities.session_projection;
                            projection_state.lock().await.set_mode(
                                if projection_supported {
                                    super::projection::ProjectionConnectionMode::ProjectionPrimary
                                } else {
                                    super::projection::ProjectionConnectionMode::RawCompatibility
                                },
                                projection_supported.then_some(1),
                            );
                            if !projection_supported {
                                let subscription_ids =
                                    cleanup_projection_state(&projection_state).await;
                                for subscription_id in subscription_ids {
                                    let _ = daemon
                                        .handle_request_for_client(
                                            crate::core::new_request(
                                                format!(
                                                    "projection-downgrade-{}",
                                                    uuid::Uuid::new_v4()
                                                ),
                                                crate::protocol::core::CoreRequest::ProjectionUnsubscribe {
                                                    subscription_id,
                                                },
                                            ),
                                            &client_id,
                                        )
                                        .await;
                                }
                            }
                            tracing::info!(
                                "Client connected: {} (kind: {:?}, id={})",
                                hello.client_name,
                                hello.client_kind,
                                client_id
                            );
                            // Register the negotiated id with the actual
                            // client name from the hello. Registration is
                            // deferred until after ClientHello so the name
                            // is correct (previously this was a hardcoded
                            // "websocket" placeholder).
                            daemon.clients.register(
                                client_id.clone(),
                                hello.client_name.clone(),
                                Some(hello.capabilities.clone()),
                            );
                            let server_hello = CoreFrame::ServerHello(ServerHello {
                                daemon_id: daemon.daemon_id.clone(),
                                protocol_version: crate::protocol::core::PROTOCOL_VERSION,
                                server_capabilities: ServerCapabilities {
                                    event_replay: true,
                                    session_management: true,
                                    permission_routing: true,
                                    workspace_registration: true,
                                    workspace_snapshots: true,
                                    durable_jobs: true,
                                    durable_schedules: true,
                                    identity_aware_context: true,
                                    project_catalog: true,
                                    session_projection: true,
                                },
                                client_id: client_id.clone(),
                            });
                            if let Err(error) = send_frame(&writer, &server_hello).await {
                                tracing::warn!(
                                    "critical Unix-socket ServerHello delivery failed: {}",
                                    error
                                );
                                break;
                            }
                        }
                        CoreFrame::Ping => {
                            let cancellation = projection_state.lock().await.cancellation();
                            if let Err(error) = bounded_critical_delivery(
                                &cancellation,
                                send_frame(&writer, &CoreFrame::Pong),
                            )
                            .await
                            {
                                tracing::warn!("Unix-socket Pong delivery failed: {}", error);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(_) => break,
        }
    }

    connection_cancel.cancel();
    // Join the raw task before doing any connection-owned daemon I/O. The
    // cancellation is idempotent, so this is the same safe path for EOF,
    // listener shutdown, and a writer failure reported by the forwarder.
    if let Err(error) = raw_forwarder.await {
        tracing::warn!(
            client_id = %client_id,
            "Unix raw event forwarder terminated abnormally: {}",
            error
        );
    }

    daemon.clients.unregister(&client_id);

    let subscription_ids = cleanup_projection_state(&projection_state).await;
    for subscription_id in subscription_ids {
        let _ = daemon
            .handle_request_for_client(
                crate::core::new_request(
                    format!("projection-disconnect-{}", uuid::Uuid::new_v4()),
                    crate::protocol::core::CoreRequest::ProjectionUnsubscribe { subscription_id },
                ),
                &client_id,
            )
            .await;
    }

    Ok(())
}

async fn install_projection_receiver(
    daemon: &Arc<CoreDaemon>,
    writer: &Arc<tokio::sync::Mutex<tokio::net::unix::OwnedWriteHalf>>,
    projection_state: &Arc<Mutex<ProjectionConnectionState>>,
    subscription_id: &ProjectionSubscriptionId,
    descriptor: &codegg_protocol::projection::replay::ProjectionStreamDescriptor,
    cursor: &crate::protocol::projection::replay::ProjectionCursor,
    retention_floor_seq: u64,
    client_id: &str,
) -> bool {
    if projection_state.lock().await.owns(subscription_id) {
        return true;
    }
    let Some(seam) = daemon.projection_seam.as_ref() else {
        let _ = daemon
            .handle_request_for_client(
                crate::core::new_request(
                    format!("projection-unsubscribe-{}", uuid::Uuid::new_v4()),
                    crate::protocol::core::CoreRequest::ProjectionUnsubscribe {
                        subscription_id: subscription_id.clone(),
                    },
                ),
                client_id,
            )
            .await;
        return false;
    };
    let Some(rx) = seam
        .service()
        .take_subscription_receiver(subscription_id)
        .await
    else {
        let _ = daemon
            .handle_request_for_client(
                crate::core::new_request(
                    format!("projection-unsubscribe-{}", uuid::Uuid::new_v4()),
                    crate::protocol::core::CoreRequest::ProjectionUnsubscribe {
                        subscription_id: subscription_id.clone(),
                    },
                ),
                client_id,
            )
            .await;
        return false;
    };
    let mut projection = projection_state.lock().await;
    let owned = OwnedProjectionSubscription::new(
        subscription_id.clone(),
        descriptor.clone(),
        cursor.clone(),
        retention_floor_seq,
        projection.reconnect_generation(),
    );
    let ready = owned.ready.clone();
    let cancellation = owned.cancellation.clone();
    if projection.insert_subscription(owned).is_err() {
        drop(projection);
        let _ = daemon
            .handle_request_for_client(
                crate::core::new_request(
                    format!("projection-unsubscribe-{}", uuid::Uuid::new_v4()),
                    crate::protocol::core::CoreRequest::ProjectionUnsubscribe {
                        subscription_id: subscription_id.clone(),
                    },
                ),
                client_id,
            )
            .await;
        return false;
    }
    let sub_id = subscription_id.clone();
    let stream_id = descriptor.stream_id.clone();
    let writer = Arc::clone(writer);
    let handle = tokio::spawn(async move {
        projection_forwarder(sub_id, stream_id, rx, writer, ready, cancellation).await;
    });
    if let Some(subscription) = projection.subscription_mut(subscription_id) {
        subscription.forwarder = Some(handle);
    }
    true
}

async fn cleanup_projection_subscription(
    daemon: &Arc<CoreDaemon>,
    projection_state: &Arc<Mutex<ProjectionConnectionState>>,
    subscription_id: &ProjectionSubscriptionId,
    client_id: &str,
) {
    stop_projection_subscription(projection_state, subscription_id).await;
    let _ = daemon
        .handle_request_for_client(
            crate::core::new_request(
                format!("projection-critical-delivery-{}", uuid::Uuid::new_v4()),
                crate::protocol::core::CoreRequest::ProjectionUnsubscribe {
                    subscription_id: subscription_id.clone(),
                },
            ),
            client_id,
        )
        .await;
}

/// Cancel and join one projection forwarder without holding the connection
/// state lock across the join. Removing it first also makes repeated cleanup
/// calls harmless.
async fn stop_projection_subscription(
    projection_state: &Arc<Mutex<ProjectionConnectionState>>,
    subscription_id: &ProjectionSubscriptionId,
) -> bool {
    let Some(mut subscription) = ({
        let mut state = projection_state.lock().await;
        state.remove_subscription(subscription_id)
    }) else {
        return false;
    };

    subscription.cancel();
    if let Some(forwarder) = subscription.forwarder.take() {
        forwarder.abort();
        let _ = forwarder.await;
    }
    true
}

/// Drain all projection subscriptions owned by a connection. State is
/// removed while locked, then cancellation and joins happen after the lock is
/// released. This is safe to call more than once.
async fn cleanup_projection_state(
    projection_state: &Arc<Mutex<ProjectionConnectionState>>,
) -> Vec<ProjectionSubscriptionId> {
    let (cancellation, mut subscriptions) = {
        let mut state = projection_state.lock().await;
        let cancellation = state.cancellation();
        let ids: Vec<_> = state
            .subscriptions()
            .map(|subscription| subscription.subscription_id.clone())
            .collect();
        let subscriptions: Vec<_> = ids
            .iter()
            .filter_map(|subscription_id| {
                state
                    .remove_subscription(subscription_id)
                    .map(|subscription| (subscription_id.clone(), subscription))
            })
            .collect();
        (cancellation, subscriptions)
    };

    cancellation.cancel();
    let ids = subscriptions
        .iter()
        .map(|(subscription_id, _)| subscription_id.clone())
        .collect();
    for (_, subscription) in &mut subscriptions {
        subscription.cancel();
        if let Some(forwarder) = subscription.forwarder.take() {
            forwarder.abort();
            let _ = forwarder.await;
        }
    }
    ids
}

/// Forward projection events from a subscription receiver to the client writer.
/// Wraps each `ProjectionEnvelope` in a `CoreEvent::ProjectionStreamEvent` and
/// sends it as a regular `CoreFrame::Event` to the client.
async fn projection_forwarder(
    sub_id: ProjectionSubscriptionId,
    stream_id: codegg_protocol::projection::replay::ProjectionStreamId,
    mut rx: tokio::sync::mpsc::Receiver<codegg_protocol::projection::event::ProjectionEnvelope>,
    writer: Arc<tokio::sync::Mutex<tokio::net::unix::OwnedWriteHalf>>,
    ready: Arc<tokio::sync::Notify>,
    cancellation: tokio_util::sync::CancellationToken,
) {
    tokio::select! {
        _ = cancellation.cancelled() => return,
        _ = ready.notified() => {}
    }
    loop {
        let envelope = tokio::select! {
            _ = cancellation.cancelled() => break,
            envelope = rx.recv() => envelope,
        };
        let Some(envelope) = envelope else { break };
        let core_event = CoreEvent::ProjectionStreamEvent {
            subscription_id: sub_id.clone(),
            stream_id: stream_id.clone(),
            envelope,
        };
        let frame = CoreFrame::Event(EventEnvelope {
            protocol_version: crate::protocol::core::PROTOCOL_VERSION,
            event_seq: 0, // projection events don't use core event_seq
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            session_id: None,
            turn_id: None,
            payload: core_event,
        });
        if bounded_critical_delivery(&cancellation, send_frame(&writer, &frame))
            .await
            .is_err()
        {
            break;
        }
    }
}

async fn forward_events(
    mut event_rx: broadcast::Receiver<EventEnvelope<CoreEvent>>,
    writer: Arc<tokio::sync::Mutex<tokio::net::unix::OwnedWriteHalf>>,
    filters: Arc<RwLock<Vec<EventFilter>>>,
    cancellation: CancellationToken,
) {
    loop {
        let receive = tokio::select! {
            _ = cancellation.cancelled() => break,
            receive = event_rx.recv() => receive,
        };
        match receive {
            Ok(event) => {
                if matches!(event.payload, CoreEvent::ProjectionStreamEvent { .. }) {
                    continue;
                }
                // Snapshot the filter list under the read lock, then drop the
                // lock before serializing/writing the frame. This keeps the
                // write path from blocking on Subscribe frame processing.
                let matches = {
                    let guard = filters.read().await;
                    if guard.is_empty() {
                        // No subscription yet: live events do not flow.
                        // Clients must send a Subscribe frame after the
                        // ClientHello/ServerHello handshake to opt in.
                        false
                    } else {
                        guard.iter().any(|f| event_matches_filter(f, &event))
                    }
                };
                if !matches {
                    continue;
                }
                let frame = CoreFrame::Event(event);
                let send_result = tokio::select! {
                    _ = cancellation.cancelled() => break,
                    result = send_frame(&writer, &frame) => result,
                };
                if send_result.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("Event forwarder lagged, {} events dropped", n);
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn send_frame(
    writer: &Arc<tokio::sync::Mutex<tokio::net::unix::OwnedWriteHalf>>,
    frame: &CoreFrame,
) -> Result<(), CriticalDeliveryError> {
    let json = serde_json::to_string(frame).map_err(|_| CriticalDeliveryError::Serialization)?;
    let mut w = writer.lock().await;
    w.write_all(json.as_bytes())
        .await
        .map_err(|_| CriticalDeliveryError::WriterClosed)?;
    w.write_all(b"\n")
        .await
        .map_err(|_| CriticalDeliveryError::WriterClosed)?;
    w.flush()
        .await
        .map_err(|_| CriticalDeliveryError::WriterClosed)
}

async fn staged_socket_critical_delivery(
    writer: &Arc<tokio::sync::Mutex<tokio::net::unix::OwnedWriteHalf>>,
    frame: &CoreFrame,
    cancellation: &CancellationToken,
    lifecycle_seam: &ProjectionLifecycleSeam,
) -> Result<(), CriticalDeliveryError> {
    lifecycle_seam
        .checkpoint(
            ProjectionLifecycleBoundary::BeforeControlEnqueue,
            cancellation,
        )
        .await?;
    // The Unix transport has no intermediate bounded queue, so this marks
    // the point at which the response is committed to the direct writer path.
    lifecycle_seam
        .checkpoint(
            ProjectionLifecycleBoundary::AfterControlEnqueueBeforeWriterReceipt,
            cancellation,
        )
        .await?;
    lifecycle_seam
        .checkpoint(ProjectionLifecycleBoundary::DuringWriterWrite, cancellation)
        .await?;
    bounded_critical_delivery(cancellation, send_frame(writer, frame)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::core::{CoreEvent, EventEnvelope, PROTOCOL_VERSION};
    use std::time::Duration;

    fn envelope(seq: u64, session_id: Option<&str>) -> EventEnvelope<CoreEvent> {
        EventEnvelope {
            protocol_version: PROTOCOL_VERSION,
            event_seq: seq,
            timestamp_ms: 0,
            session_id: session_id.map(str::to_string),
            turn_id: None,
            payload: CoreEvent::Error {
                code: format!("e{}", seq),
                message: "m".into(),
            },
        }
    }

    #[test]
    fn filter_session_matches_event_for_session() {
        let ev = envelope(1, Some("s1"));
        let filter = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: true,
        };
        assert!(event_matches_filter(&filter, &ev));
    }

    #[test]
    fn filter_session_rejects_other_session() {
        let ev = envelope(1, Some("s2"));
        let filter = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: true,
        };
        assert!(!event_matches_filter(&filter, &ev));
    }

    #[test]
    fn global_filter_rejects_session_event() {
        // session_id=None must NOT match session-scoped events, regardless
        // of include_global. This is the core Pass 1 invariant: a global
        // subscription is global-only, not all-sessions.
        let ev = envelope(1, Some("s1"));
        let filter = EventFilter {
            session_id: None,
            client_id: None,
            include_global: true,
        };
        assert!(
            !event_matches_filter(&filter, &ev),
            "global filter must not match session events"
        );

        let filter_no_global = EventFilter {
            session_id: None,
            client_id: None,
            include_global: false,
        };
        assert!(!event_matches_filter(&filter_no_global, &ev));
    }

    #[test]
    fn global_filter_matches_global_event() {
        let ev = envelope(1, None);
        let filter_with = EventFilter {
            session_id: None,
            client_id: None,
            include_global: true,
        };
        assert!(event_matches_filter(&filter_with, &ev));

        let filter_without = EventFilter {
            session_id: None,
            client_id: None,
            include_global: false,
        };
        assert!(event_matches_filter(&filter_without, &ev));
    }

    #[test]
    fn session_filter_can_include_global_event_if_configured() {
        // A session-specific filter with include_global=true must also
        // match sessionless/global events so session subscribers still
        // see updates that affect them (e.g. session list changes).
        let ev_global = envelope(1, None);
        let filter = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: true,
        };
        assert!(event_matches_filter(&filter, &ev_global));
    }

    #[test]
    fn session_filter_without_include_global_rejects_global_event() {
        let ev_global = envelope(1, None);
        let filter = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: false,
        };
        assert!(!event_matches_filter(&filter, &ev_global));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn send_frame_reports_closed_unix_writer() {
        let (server, client) = tokio::net::UnixStream::pair().expect("UnixStream pair");
        let (_read_half, write_half) = server.into_split();
        let writer = Arc::new(Mutex::new(write_half));
        drop(client);

        let result = send_frame(&writer, &CoreFrame::Ping).await;
        assert_eq!(result, Err(CriticalDeliveryError::WriterClosed));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn raw_forwarder_cancellation_releases_receiver_and_shared_state() {
        let (event_tx, event_rx) = broadcast::channel(4);
        let (writer_stream, _peer_stream) = tokio::net::UnixStream::pair().unwrap();
        let (_, writer_half) = writer_stream.into_split();
        let writer = Arc::new(Mutex::new(writer_half));
        let filters = Arc::new(RwLock::new(Vec::new()));
        let cancellation = CancellationToken::new();

        let handle = tokio::spawn(forward_events(
            event_rx,
            Arc::clone(&writer),
            Arc::clone(&filters),
            cancellation.clone(),
        ));
        assert_eq!(event_tx.receiver_count(), 1);

        cancellation.cancel();
        tokio::time::timeout(Duration::from_millis(100), handle)
            .await
            .expect("raw forwarder should join after cancellation")
            .expect("raw forwarder should not panic");

        assert_eq!(event_tx.receiver_count(), 0);
        assert_eq!(Arc::strong_count(&writer), 1);
        assert_eq!(Arc::strong_count(&filters), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn raw_forwarder_writer_failure_terminates_without_a_retained_receiver() {
        let (event_tx, event_rx) = broadcast::channel(4);
        let (writer_stream, peer_stream) = tokio::net::UnixStream::pair().unwrap();
        let (_, writer_half) = writer_stream.into_split();
        let writer = Arc::new(Mutex::new(writer_half));
        let filters = Arc::new(RwLock::new(vec![EventFilter {
            session_id: None,
            client_id: None,
            include_global: true,
        }]));
        let cancellation = CancellationToken::new();

        let handle = tokio::spawn(forward_events(
            event_rx,
            Arc::clone(&writer),
            Arc::clone(&filters),
            cancellation,
        ));
        drop(peer_stream);
        event_tx
            .send(envelope(1, None))
            .expect("forwarder should still own the receiver");

        tokio::time::timeout(Duration::from_millis(100), handle)
            .await
            .expect("writer failure should terminate the raw forwarder")
            .expect("raw forwarder should not panic");
        assert_eq!(event_tx.receiver_count(), 0);
        assert_eq!(Arc::strong_count(&writer), 1);
        assert_eq!(Arc::strong_count(&filters), 1);
    }

    #[tokio::test]
    async fn event_log_replay_respects_filter() {
        // Construct an EventLog directly and verify replay uses the same
        // filter semantics the forwarder relies on. This protects against
        // divergent logic between live forwarding and replay.
        let log = crate::core::event_log::EventLog::new(64);
        log.publish(
            Some("s1".into()),
            None,
            CoreEvent::Error {
                code: "a".into(),
                message: "m".into(),
            },
        )
        .await;
        log.publish(
            Some("s2".into()),
            None,
            CoreEvent::Error {
                code: "b".into(),
                message: "m".into(),
            },
        )
        .await;
        log.publish(
            None,
            None,
            CoreEvent::Error {
                code: "c".into(),
                message: "m".into(),
            },
        )
        .await;

        // Session-scoped filter (include_global: true to also pull global events).
        let s1_filter = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: true,
        };
        let s1_events = log.replay_from(0, &s1_filter).await;
        // s1 events + the global event.
        assert_eq!(s1_events.len(), 2);
        for env in &s1_events {
            let sid = env.session_id.as_deref();
            assert!(sid == Some("s1") || sid.is_none());
        }

        // Session-scoped filter without include_global -> s1 only.
        let s1_strict = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: false,
        };
        let s1_strict_events = log.replay_from(0, &s1_strict).await;
        assert_eq!(s1_strict_events.len(), 1);
        assert_eq!(s1_strict_events[0].session_id.as_deref(), Some("s1"));

        // Global filter: session_id: None, regardless of include_global, must
        // NOT return session events. Only the sessionless one.
        let global_filter = EventFilter {
            session_id: None,
            client_id: None,
            include_global: true,
        };
        let global_events = log.replay_from(0, &global_filter).await;
        assert_eq!(global_events.len(), 1);
        assert!(global_events[0].session_id.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn critical_socket_frame_is_readable_after_successful_write() {
        let (writer_stream, reader_stream) = tokio::net::UnixStream::pair().unwrap();
        let (_, writer_half) = writer_stream.into_split();
        let writer = Arc::new(tokio::sync::Mutex::new(writer_half));
        let frame = CoreFrame::Pong;
        send_frame(&writer, &frame).await.unwrap();

        let (reader_half, _) = reader_stream.into_split();
        let mut reader = BufReader::new(reader_half);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<CoreFrame>(line.trim()).unwrap(),
            CoreFrame::Pong
        ));
    }
}

#[cfg(test)]
#[path = "daemon_socket_integration_tests.rs"]
mod daemon_socket_integration_tests;
