//! M002: `projection` request family for `CoreDaemon`.
//!
//! Projection replay subscribe/resume/ack/snapshot/artifacts plus ephemeral presence leases.
//! Operates on the same daemon-owned state as the thin dispatcher;
//! introduces no new store, scheduler, state machine, or authority.

use crate::error::AppError;
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse};

use super::daemon::CoreDaemon;

use codegg_protocol::projection::replay::{
    ProjectionResyncReason, ProjectionStreamKind, ProjectionSubscriptionRequest,
};

impl CoreDaemon {
    pub(crate) async fn handle_projection_request(
        &self,
        request: CoreRequest,
        request_id: &str,
        trusted_client_id: &str,
        authority: codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        let _ = (request_id, trusted_client_id, &authority, &authz_decision);
        match request {
            CoreRequest::PresenceCapabilities => Ok(CoreResponse::PresenceCapabilities {
                capabilities: self.presence.config().capabilities_dto(),
            }),
            CoreRequest::PresenceHeartbeat { request } => {
                let project = match codegg_core::identity::ProjectId::parse(&request.project_id) {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "presence_invalid_project".into(),
                            message: error.to_string(),
                        });
                    }
                };
                let session = match request.session_id.as_deref() {
                    Some("") => None,
                    Some(raw) => Some(raw),
                    None => None,
                };
                if let Some(raw) = session {
                    if raw.len() > 128 {
                        return Ok(CoreResponse::Error {
                            code: "presence_invalid_session".into(),
                            message: "session locator exceeds the bounded length".into(),
                        });
                    }
                }
                // Principal and client come from transport authority,
                // never from the payload.
                let authority = self.request_authority_for_client(trusted_client_id);
                let principal = authority.principal().principal_id().clone();
                let now_instant = std::time::Instant::now();
                let now_ms = chrono::Utc::now().timestamp_millis();
                match self.presence.heartbeat_dto(
                    project.clone(),
                    principal,
                    trusted_client_id,
                    session,
                    request.activity,
                    request.connection_generation,
                    now_instant,
                    now_ms,
                ) {
                    Ok(expires_at_ms) => {
                        // Bounded liveness hint for authorized
                        // collaborators. Carries only the project id;
                        // receivers re-fetch through the authorized
                        // snapshot path.
                        self.event_log
                            .publish(
                                None,
                                None,
                                CoreEvent::PresenceUpdated {
                                    project_id: project.as_str().to_owned(),
                                },
                            )
                            .await;
                        Ok(CoreResponse::PresenceHeartbeatAck {
                            project_id: project.as_str().to_owned(),
                            expires_at_ms,
                        })
                    }
                    Err(codegg_core::presence::PresenceError::Capacity) => {
                        Ok(CoreResponse::Error {
                            code: "presence_capacity".into(),
                            message: "presence capacity is exhausted".into(),
                        })
                    }
                    Err(codegg_core::presence::PresenceError::StaleGeneration) => {
                        Ok(CoreResponse::Error {
                            code: "presence_stale_generation".into(),
                            message: "stale connection generation cannot resurrect presence".into(),
                        })
                    }
                    Err(codegg_core::presence::PresenceError::InvalidInput { field, message }) => {
                        Ok(CoreResponse::Error {
                            code: "presence_invalid_input".into(),
                            message: format!("invalid presence {field}: {message}"),
                        })
                    }
                }
            }
            CoreRequest::PresenceSnapshotGet { project_id } => {
                let project = match codegg_core::identity::ProjectId::parse(project_id.as_str()) {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "presence_invalid_project".into(),
                            message: error.to_string(),
                        });
                    }
                };
                // Single bounded cleanup before the read; no
                // task-per-lease exists.
                self.presence.evict_expired(std::time::Instant::now());
                let snapshot = self.presence.snapshot(
                    &project,
                    std::time::Instant::now(),
                    chrono::Utc::now().timestamp_millis(),
                );
                Ok(CoreResponse::PresenceSnapshot { snapshot })
            }
            CoreRequest::ProjectionCapabilities => {
                Ok(CoreResponse::ProjectionCapabilitiesResponse {
                    supported: self.projection_seam.is_some(),
                    projection_version: 1,
                    max_events_per_batch: 512,
                    max_event_bytes: 64 * 1024,
                    max_subscriptions_per_client: 32,
                    max_subscriptions_per_daemon: 256,
                    retention_session_max_events: 20_000,
                    retention_project_max_events: 50_000,
                })
            }
            CoreRequest::ProjectionSubscribe { request } => {
                let Some(ref seam) = self.projection_seam else {
                    return Ok(CoreResponse::Error {
                        code: "projection_unavailable".into(),
                        message: "projection replay requires a SQLite-backed daemon".into(),
                    });
                };
                if let Err(e) = request.validate() {
                    return Ok(CoreResponse::Error {
                        code: "invalid_projection_subscribe".into(),
                        message: e.to_string(),
                    });
                }
                // Presence M003: canonical `session.observe` integration.
                // The daemon gate already enforced `project.observe` for
                // this operation; session scope additionally requires
                // `session.observe` on the owning project. Denials use
                // `project_not_found` so outsiders cannot distinguish a
                // missing session from a denied one. The check runs boxed
                // (see `observe_scope_project_boxed`) so the dispatch
                // future holds only the box.
                let canonical_project = if matches!(request.scope, ProjectionStreamKind::Session) {
                    match Box::pin(self.observe_scope_project_boxed(
                        trusted_client_id,
                        None,
                        Some(request.scope_id.as_str()),
                        "observe-subscribe",
                    ))
                    .await
                    {
                        Some(project) => Some(project),
                        None => {
                            let (code, message) = codegg_core::authorization::denial_as_not_found();
                            return Ok(CoreResponse::Error {
                                code: code.to_string(),
                                message,
                            });
                        }
                    }
                } else {
                    // Project scope: re-enforce the canonical context so a
                    // revocation between the gate and dispatch cannot retain
                    // a stale allow. Denials stay privacy-preserving.
                    // Unparseable locators skip the recheck (the stream
                    // layer validates); parseable ones must pass it.
                    match codegg_core::identity::ProjectId::parse(request.scope_id.as_str()) {
                        Ok(project_id) => {
                            match Box::pin(self.observe_scope_project_boxed(
                                trusted_client_id,
                                Some(&project_id),
                                None,
                                "observe-subscribe-project",
                            ))
                            .await
                            {
                                Some(_) => None,
                                None => {
                                    let (code, message) =
                                        codegg_core::authorization::denial_as_not_found();
                                    return Ok(CoreResponse::Error {
                                        code: code.to_string(),
                                        message,
                                    });
                                }
                            }
                        }
                        Err(_) => None,
                    }
                };
                let service = seam.service();
                let client_id = trusted_client_id;

                // Resolve canonical binding for Session scope so the
                // subscription lands on the same stream publications use.
                // Presence M003: when no binding row exists (test-injected
                // session rows, legacy sessions), fall back to the canonical
                // session-table project already authorized above so local
                // observation keeps working without fabricating a stream.
                // A binding that disagrees with the canonical project fails
                // closed as not-found (privacy-preserving).
                let (mut resolved_project, resolved_workspace, _resolved_revision) =
                    if matches!(request.scope, ProjectionStreamKind::Session) {
                        if let Some(storage) = seam.project_storage() {
                            match storage.session_binding(&request.scope_id).await {
                                Ok(Some(record))
                                    if matches!(
                                        record.status,
                                        codegg_core::project_storage::BindingStatus::Resolved
                                    ) =>
                                {
                                    (
                                        record
                                            .project_id
                                            .map(|p| p.as_str().to_string())
                                            .unwrap_or_default(),
                                        record.workspace_id.map(|w| w.as_str().to_string()),
                                        record.revision,
                                    )
                                }
                                _ => (String::new(), None, 1),
                            }
                        } else {
                            (String::new(), None, 1)
                        }
                    } else {
                        (request.scope_id.clone(), None, 1)
                    };
                if matches!(request.scope, ProjectionStreamKind::Session)
                    && resolved_project.is_empty()
                {
                    // Reuse the project already authorized above (no second
                    // team lookup, no extra dispatch frame).
                    if let Some(canonical) = canonical_project.as_ref() {
                        resolved_project = canonical.as_str().to_owned();
                    }
                }
                if matches!(request.scope, ProjectionStreamKind::Session)
                    && !resolved_project.is_empty()
                {
                    if let Some(canonical) = canonical_project.as_ref() {
                        if resolved_project != canonical.as_str() {
                            let (code, message) = codegg_core::authorization::denial_as_not_found();
                            return Ok(CoreResponse::Error {
                                code: code.to_string(),
                                message,
                            });
                        }
                    }
                }

                let sub_id = match request.scope {
                    ProjectionStreamKind::Session => {
                        service
                            .subscribe_session(
                                &request.scope_id,
                                &resolved_project,
                                resolved_workspace.as_deref(),
                                client_id,
                                &request,
                            )
                            .await
                    }
                    ProjectionStreamKind::Project => {
                        service
                            .subscribe_project(&request.scope_id, client_id, &request)
                            .await
                    }
                };

                match sub_id {
                    Ok(sub_id) => {
                        let descriptor_result = match request.scope {
                            ProjectionStreamKind::Session => service
                                .store()
                                .lookup_session_stream(&request.scope_id, &resolved_project)
                                .await
                                .map_err(|e| e.to_string())
                                .and_then(|descriptor| {
                                    descriptor.ok_or_else(|| {
                                        "projection stream descriptor missing after subscribe"
                                            .to_string()
                                    })
                                }),
                            ProjectionStreamKind::Project => service
                                .store()
                                .get_or_create_project_stream(&request.scope_id)
                                .await
                                .map(|(descriptor, _)| descriptor)
                                .map_err(|e| e.to_string()),
                        };
                        let descriptor = match descriptor_result {
                            Ok(descriptor) => descriptor,
                            Err(message) => {
                                if let Err(error) = service.unsubscribe(&sub_id).await {
                                    tracing::warn!(error = %error, subscription_id = %sub_id.0, "failed to clean up projection subscription");
                                }
                                return Ok(CoreResponse::Error {
                                    code: "projection_descriptor_missing".into(),
                                    message,
                                });
                            }
                        };
                        let snapshot =
                            codegg_protocol::projection::replay::ProjectionSnapshotBundle::One {
                                snapshot: Box::new(
                                    self.projection_snapshot_for_session(
                                        &request.scope_id,
                                        &descriptor.project_id,
                                        descriptor.workspace_id.as_deref().unwrap_or(""),
                                    )
                                    .await,
                                ),
                            };
                        let cursor = codegg_protocol::projection::replay::ProjectionCursor {
                            stream_id: descriptor.stream_id.clone(),
                            event_seq: descriptor.high_water_seq,
                            projection_version: descriptor.projection_version,
                        };
                        let retention_floor_seq = descriptor.retention_floor_seq;
                        Ok(CoreResponse::ProjectionSubscribed {
                            subscription_id: sub_id,
                            descriptor,
                            snapshot,
                            cursor: cursor.clone(),
                            retention_floor_seq,
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "projection_subscribe_failed".into(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ProjectionResume {
                cursor,
                include_snapshot_if_resync,
            } => {
                let Some(ref seam) = self.projection_seam else {
                    return Ok(CoreResponse::Error {
                        code: "projection_unavailable".into(),
                        message: "projection replay requires a SQLite-backed daemon".into(),
                    });
                };
                let service = seam.service();
                let descriptor = match service
                    .store()
                    .lookup_stream_by_id(cursor.stream_id.as_str())
                    .await
                {
                    Ok(Some(descriptor)) => descriptor,
                    Ok(None) => {
                        return Ok(CoreResponse::ProjectionResyncRequired {
                            subscription_id: None,
                            reason: ProjectionResyncReason::StreamMismatch,
                            descriptor: None,
                            requested_cursor: Some(cursor),
                            snapshot: None,
                        });
                    }
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "projection_resume_failed".into(),
                            message: e.to_string(),
                        });
                    }
                };
                // Presence M003: recheck canonical authorization on resume.
                // `ProjectionResume` is global at the gate (no project
                // locator), so this handler is the authority boundary.
                // Session streams require `session.observe` on the owning
                // project; project streams require `project.observe`.
                // Denials use `project_not_found` (privacy-preserving) and
                // clean any transient owned subscription so revoked grants
                // cannot retain delivery. The check runs boxed (see
                // `observe_scope_project_boxed`) so the dispatch future
                // holds only the box.
                let resume_allowed = match descriptor.kind {
                    ProjectionStreamKind::Session => {
                        let session_id = descriptor.session_id.clone().unwrap_or_default();
                        Box::pin(self.observe_scope_project_boxed(
                            trusted_client_id,
                            None,
                            Some(session_id.as_str()),
                            "observe-resume",
                        ))
                        .await
                        .is_some()
                    }
                    ProjectionStreamKind::Project => {
                        match codegg_core::identity::ProjectId::parse(
                            descriptor.project_id.as_str(),
                        ) {
                            Ok(project_id) => Box::pin(self.observe_scope_project_boxed(
                                trusted_client_id,
                                Some(&project_id),
                                None,
                                "observe-resume-project",
                            ))
                            .await
                            .is_some(),
                            Err(_) => false,
                        }
                    }
                };
                if !resume_allowed {
                    // Clean transient owned state for this connection so a
                    // revoked grant cannot retain delivery via an existing
                    // subscription id.
                    let owned: Vec<codegg_protocol::projection::replay::ProjectionSubscriptionId> =
                        service
                            .subscriptions()
                            .by_id()
                            .iter()
                            .filter(|entry| {
                                entry.value().stream_id == cursor.stream_id
                                    && entry.value().client_id == trusted_client_id
                            })
                            .map(|entry| entry.key().clone())
                            .collect();
                    for sub in owned {
                        let _ = service.unsubscribe(&sub).await;
                    }
                    let (code, message) = codegg_core::authorization::denial_as_not_found();
                    return Ok(CoreResponse::Error {
                        code: code.to_string(),
                        message,
                    });
                }

                // Reuse only a subscription owned by this trusted connection.
                // A reconnect has no active entry, so establish a fresh
                // daemon-issued subscription for the persisted stream before
                // replay is evaluated.
                let sub_id = service
                    .subscriptions()
                    .by_id()
                    .iter()
                    .find(|entry| {
                        entry.value().stream_id == cursor.stream_id
                            && entry.value().client_id == trusted_client_id
                    })
                    .map(|entry| entry.key().clone());
                if sub_id.is_none()
                    && service.subscriptions().by_id().iter().any(|entry| {
                        entry.value().stream_id == cursor.stream_id
                            && entry.value().client_id != trusted_client_id
                    })
                {
                    return Ok(CoreResponse::Error {
                        code: "projection_resume_not_owned".into(),
                        message: "the projection stream is actively owned by another connection"
                            .into(),
                    });
                }
                let sub_id = if let Some(sub_id) = sub_id {
                    sub_id
                } else {
                    let scope_id = match descriptor.kind {
                        ProjectionStreamKind::Session => descriptor.session_id.clone(),
                        ProjectionStreamKind::Project => Some(descriptor.project_id.clone()),
                    };
                    let Some(scope_id) = scope_id else {
                        return Ok(CoreResponse::ProjectionResyncRequired {
                            subscription_id: None,
                            reason: ProjectionResyncReason::ScopeMismatch,
                            descriptor: Some(descriptor),
                            requested_cursor: Some(cursor),
                            snapshot: None,
                        });
                    };
                    let request = ProjectionSubscriptionRequest {
                        scope: descriptor.kind,
                        scope_id,
                        cursor: Some(cursor.clone()),
                        projection_version: cursor.projection_version,
                    };
                    let result = match descriptor.kind {
                        ProjectionStreamKind::Session => {
                            service
                                .subscribe_session(
                                    descriptor.session_id.as_deref().unwrap_or_default(),
                                    &descriptor.project_id,
                                    descriptor.workspace_id.as_deref(),
                                    trusted_client_id,
                                    &request,
                                )
                                .await
                        }
                        ProjectionStreamKind::Project => {
                            service
                                .subscribe_project(
                                    &descriptor.project_id,
                                    trusted_client_id,
                                    &request,
                                )
                                .await
                        }
                    };
                    match result {
                        Ok(sub_id) => sub_id,
                        Err(e) => {
                            return Ok(CoreResponse::Error {
                                code: "projection_resume_subscribe_failed".into(),
                                message: e.to_string(),
                            });
                        }
                    }
                };

                match service
                    .resume(&sub_id, &cursor, include_snapshot_if_resync)
                    .await
                {
                    Ok(codegg_core::projection_replay::service::ResumeOutcome::Replayed {
                        events,
                        current_high_water,
                        next_cursor,
                    }) => Ok(CoreResponse::ProjectionReplay {
                        subscription_id: Some(sub_id.clone()),
                        batch: codegg_protocol::projection::replay::ProjectionReplayBatch {
                            descriptor,
                            events,
                            snapshot: None,
                            replay_start_seq: cursor.event_seq + 1,
                            replay_end_seq: next_cursor.event_seq,
                            current_high_water,
                            truncation_flag: false,
                            next_cursor: if next_cursor.event_seq < current_high_water {
                                Some(next_cursor)
                            } else {
                                None
                            },
                        },
                    }),
                    Ok(codegg_core::projection_replay::service::ResumeOutcome::Empty {
                        current_high_water,
                        next_cursor,
                    }) => Ok(CoreResponse::ProjectionReplay {
                        subscription_id: Some(sub_id.clone()),
                        batch: codegg_protocol::projection::replay::ProjectionReplayBatch {
                            descriptor,
                            events: vec![],
                            snapshot: None,
                            replay_start_seq: cursor.event_seq + 1,
                            replay_end_seq: next_cursor.event_seq,
                            current_high_water,
                            truncation_flag: false,
                            next_cursor: None,
                        },
                    }),
                    Ok(codegg_core::projection_replay::service::ResumeOutcome::Resync {
                        reason,
                        descriptor,
                        requested_cursor,
                        snapshot,
                    }) => Ok(CoreResponse::ProjectionResyncRequired {
                        subscription_id: Some(sub_id),
                        reason,
                        descriptor,
                        requested_cursor,
                        snapshot,
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "projection_resume_failed".into(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ProjectionAck { ack } => {
                let Some(ref seam) = self.projection_seam else {
                    return Ok(CoreResponse::Error {
                        code: "projection_unavailable".into(),
                        message: "projection replay requires a SQLite-backed daemon".into(),
                    });
                };
                let service = seam.service();
                // Find the subscription that owns this ack
                let sub_id_opt = service
                    .subscriptions()
                    .by_id()
                    .get(&ack.subscription_id)
                    .filter(|entry| entry.value().client_id == trusted_client_id)
                    .map(|e| e.key().clone());

                let Some(sub_id) = sub_id_opt else {
                    return Ok(CoreResponse::Error {
                        code: "subscription_not_found".into(),
                        message: "no active subscription with this ID".into(),
                    });
                };

                match service.ack(&sub_id, &ack.cursor).await {
                    Ok(codegg_core::projection_replay::service::AckResult::Accepted {
                        last_acked_seq,
                        lag_count,
                    }) => Ok(CoreResponse::ProjectionAckAccepted {
                        subscription_id: sub_id,
                        last_acked_seq,
                        lag_count,
                    }),
                    Ok(codegg_core::projection_replay::service::AckResult::Rejected { reason }) => {
                        Ok(CoreResponse::Error {
                            code: "projection_ack_rejected".into(),
                            message: reason,
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "projection_ack_failed".into(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ProjectionUnsubscribe { subscription_id } => {
                let Some(ref seam) = self.projection_seam else {
                    return Ok(CoreResponse::Error {
                        code: "projection_unavailable".into(),
                        message: "projection replay requires a SQLite-backed daemon".into(),
                    });
                };
                let service = seam.service();
                let owned = service
                    .subscriptions()
                    .by_id()
                    .get(&subscription_id)
                    .map(|entry| entry.value().client_id == trusted_client_id)
                    .unwrap_or(false);
                if !owned {
                    return Ok(CoreResponse::Error {
                        code: "projection_subscription_not_owned".into(),
                        message: "projection subscription is not owned by this connection".into(),
                    });
                }
                match service.unsubscribe(&subscription_id).await {
                    Ok(()) => Ok(CoreResponse::ProjectionUnsubscribed { subscription_id }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "projection_unsubscribe_failed".into(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ProjectionSnapshotGet { scope, scope_id } => {
                // This legacy request has no project/session binding context
                // with which to resolve the canonical persisted stream. Do
                // not manufacture a stream ID from the caller's scope ID;
                // callers must use ProjectionSubscribe/ProjectionResume so
                // the replay store remains the identity authority.
                let _ = (scope, scope_id);
                Ok(CoreResponse::Error {
                    code: "projection_snapshot_requires_subscription".into(),
                    message: "projection snapshots require an authoritative subscription".into(),
                })
            }
            // ── Session Projections M3: Artifact Read Protocol ────────────────
            CoreRequest::ProjectionArtifactRead {
                request,
                project_id,
                context_correlation_id: _,
            } => {
                let Some(ref _seam) = self.projection_seam else {
                    return Ok(CoreResponse::Error {
                        code: "projection_unavailable".into(),
                        message: "projection replay requires a SQLite-backed daemon".into(),
                    });
                };

                // Build access context for the calling principal.
                // Presence M003: use the canonical team-derived context for
                // the target project (no synthetic allow-all). The gate
                // already enforced `project.observe`; this recheck binds
                // the artifact policy to the same membership so revocation
                // between gate and dispatch cannot retain reads. The
                // principal string is never taken from the request payload.
                // Boxed (see `observe_access_boxed`) so the dispatch
                // future holds only the box.
                let access_ctx = match codegg_core::identity::ProjectId::parse(project_id.as_str())
                {
                    Ok(project) => std::sync::Arc::new(
                        Box::pin(self.observe_access_boxed(
                            trusted_client_id,
                            &project,
                            "artifact-read",
                        ))
                        .await,
                    ),
                    Err(_) => std::sync::Arc::new(
                        self.projection_access_for_client(trusted_client_id, "artifact-read"),
                    ),
                };
                let policy = std::sync::Arc::new(
                    codegg_core::projection_replay::policy::PolicyRegistry::default(),
                );

                // Authorize the artifact read
                let kind = codegg_core::projection_replay::policy::ArtifactReadKind::RunArtifact;

                if !policy
                    .policy()
                    .authorize_artifact_read(&access_ctx, &project_id, kind)
                {
                    return Ok(CoreResponse::ProjectionArtifactRead {
                        outcome:
                            codegg_protocol::projection::replay::ProjectionArtifactReadOutcome::Denied {
                                reason: "authorization failed".into(),
                            },
                    });
                }

                // Build a disclosure context to access the artifact registry
                let metrics = std::sync::Arc::new(
                    codegg_core::projection_replay::ProjectionReplayMetrics::new(),
                );
                let disclosure = codegg_core::projection_replay::ProjectionDisclosureContext::local(
                    None,
                    Some(project_id.clone()),
                    metrics,
                );

                if let Some(ref registry) = disclosure.artifact_registry {
                    // Convert wire DTO to core type
                    let core_request =
                        codegg_core::projection_replay::artifacts::ArtifactReadRequest {
                            handle_id: request.handle_id.clone(),
                            start: request.start,
                            end: request.end,
                            expected_revision: request.expected_revision,
                        };
                    match registry.read(&core_request, &project_id).await {
                        Ok(response) => {
                            return Ok(CoreResponse::ProjectionArtifactRead {
                                outcome:
                                    codegg_protocol::projection::replay::ProjectionArtifactReadOutcome::Ok(
                                        codegg_protocol::projection::replay::ProjectionArtifactReadResponse {
                                            handle_id: response.handle_id,
                                            revision: response.revision,
                                            start: response.start,
                                            end: response.end,
                                            content_type: format!("{:?}", response.content_type)
                                                .to_lowercase(),
                                            content: response.content,
                                            redacted: response.redacted,
                                            truncated: response.truncated,
                                            note: response.note,
                                        },
                                    ),
                            });
                        }
                        Err(e) => {
                            let outcome = match &e {
                                codegg_core::projection_replay::artifact_registry::ArtifactRegistryError::NotFound => {
                                    codegg_protocol::projection::replay::ProjectionArtifactReadOutcome::NotFound
                                }
                                codegg_core::projection_replay::artifact_registry::ArtifactRegistryError::RevisionMismatch { current, .. } => {
                                    codegg_protocol::projection::replay::ProjectionArtifactReadOutcome::RevisionMismatch {
                                        current_revision: *current,
                                    }
                                }
                                codegg_core::projection_replay::artifact_registry::ArtifactRegistryError::InvalidRequest(msg) => {
                                    codegg_protocol::projection::replay::ProjectionArtifactReadOutcome::InvalidRequest {
                                        reason: msg.clone(),
                                    }
                                }
                                _ => codegg_protocol::projection::replay::ProjectionArtifactReadOutcome::InvalidRequest {
                                    reason: "internal error".into(),
                                },
                            };
                            return Ok(CoreResponse::ProjectionArtifactRead { outcome });
                        }
                    }
                }

                Ok(CoreResponse::ProjectionArtifactRead {
                    outcome:
                        codegg_protocol::projection::replay::ProjectionArtifactReadOutcome::InvalidRequest {
                            reason: "no artifact registry available".into(),
                        },
                })
            }
            CoreRequest::ProjectionArtifactList { project_id } => {
                let Some(ref _seam) = self.projection_seam else {
                    return Ok(CoreResponse::Error {
                        code: "projection_unavailable".into(),
                        message: "projection replay requires a SQLite-backed daemon".into(),
                    });
                };

                // Presence M003: canonical scope check for artifact listing.
                // The gate enforced `project.observe`; re-enforce the
                // team-derived context here so the handle list cannot leak
                // across projects via a stale gate decision. Artifact
                // handles remain project-scoped opaque ids; reads are capped
                // by the protocol's 64 KiB window at the registry layer.
                // Boxed (see `observe_access_boxed`) so the dispatch
                // future holds only the box.
                if let Ok(project) = codegg_core::identity::ProjectId::parse(project_id.as_str()) {
                    let access = Box::pin(self.observe_access_boxed(
                        trusted_client_id,
                        &project,
                        "artifact-list",
                    ))
                    .await;
                    if !access.authorize_scope(project_id.as_str(), None) {
                        let (code, message) = codegg_core::authorization::denial_as_not_found();
                        return Ok(CoreResponse::Error {
                            code: code.to_string(),
                            message,
                        });
                    }
                }

                let metrics = std::sync::Arc::new(
                    codegg_core::projection_replay::ProjectionReplayMetrics::new(),
                );
                let disclosure = codegg_core::projection_replay::ProjectionDisclosureContext::local(
                    None,
                    Some(project_id.clone()),
                    metrics,
                );

                if let Some(ref registry) = disclosure.artifact_registry {
                    if let Ok(handles) = registry.list(&project_id).await {
                        let dto_handles: Vec<
                            codegg_protocol::projection::replay::ProjectionArtifactHandleDto,
                        > = handles
                            .iter()
                            .map(|h| {
                                codegg_protocol::projection::replay::ProjectionArtifactHandleDto {
                                    handle_id: h.handle_id.clone(),
                                    kind: match h.kind {
                                        codegg_core::projection_replay::artifacts::ArtifactKind::RunOutput => {
                                            codegg_protocol::projection::replay::ArtifactHandleKind::RunOutput
                                        }
                                        codegg_core::projection_replay::artifacts::ArtifactKind::ToolOutput => {
                                            codegg_protocol::projection::replay::ArtifactHandleKind::ToolOutput
                                        }
                                        codegg_core::projection_replay::artifacts::ArtifactKind::DiffExcerpt => {
                                            codegg_protocol::projection::replay::ArtifactHandleKind::DiffExcerpt
                                        }
                                        codegg_core::projection_replay::artifacts::ArtifactKind::LogTail => {
                                            codegg_protocol::projection::replay::ArtifactHandleKind::LogTail
                                        }
                                    },
                                    project_id: h.project_id.clone(),
                                    source_record_id: h.source_record_id.clone(),
                                    content_type: format!("{:?}", h.content_type).to_lowercase(),
                                    total_bytes: h.total_bytes,
                                    created_at: h.created_at,
                                    expires_at: h.expires_at,
                                    revision: h.revision,
                                    public_summary: h.public_summary.clone(),
                                }
                            })
                            .collect();
                        return Ok(CoreResponse::ProjectionArtifactList {
                            handles: dto_handles,
                        });
                    }
                }

                Ok(CoreResponse::ProjectionArtifactList { handles: vec![] })
            }
            // ── Tool Programs M8: Inspect Protocol ──────────────────
            _ => {
                tracing::warn!("Unhandled CoreRequest variant");
                Ok(CoreResponse::Error {
                    code: "unimplemented".to_string(),
                    message: "This request type is not yet implemented".to_string(),
                })
            }
        }
    }
}
