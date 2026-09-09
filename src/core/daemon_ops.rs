//! M002: `ops` request family for `CoreDaemon`.
//!
//! Audit query/export, memory, and notification routing over daemon-owned stores.
//! Operates on the same daemon-owned state as the thin dispatcher;
//! introduces no new store, scheduler, state machine, or authority.

use crate::error::AppError;
use crate::protocol::core::{CoreRequest, CoreResponse};
use chrono::Utc;

use super::daemon::CoreDaemon;

impl CoreDaemon {
    pub(crate) async fn handle_ops_request(
        &self,
        request: CoreRequest,
        request_id: &str,
        trusted_client_id: &str,
        authority: codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        let _ = (request_id, trusted_client_id, &authority, &authz_decision);
        match request {
            CoreRequest::MemoryList { namespace } => {
                let Some(memory_store) = self.deps.memory_store.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_memory_store".to_string(),
                        message: "Core client missing memory store".to_string(),
                    });
                };
                let memories = memory_store.list(&namespace);
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "memories": memories }),
                })
            }
            CoreRequest::MemorySearch { query } => {
                let Some(memory_store) = self.deps.memory_store.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_memory_store".to_string(),
                        message: "Core client missing memory store".to_string(),
                    });
                };
                let memories = memory_store.search(&query);
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "memories": memories }),
                })
            }
            CoreRequest::MemoryRemember { text, namespace } => {
                let Some(memory_store) = self.deps.memory_store.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_memory_store".to_string(),
                        message: "Core client missing memory store".to_string(),
                    });
                };
                let ns = namespace.unwrap_or_else(|| "user/preferences".to_string());
                let memory = crate::memory::Memory::new(ns, text);
                memory_store.add(memory.clone());
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "memory": memory }),
                })
            }
            CoreRequest::MemoryForget { id } => {
                let Some(memory_store) = self.deps.memory_store.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_memory_store".to_string(),
                        message: "Core client missing memory store".to_string(),
                    });
                };
                let deleted = memory_store.delete(&id).is_some();
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "deleted": deleted }),
                })
            }
            CoreRequest::AuditCapabilities => Ok(CoreResponse::AuditCapabilities {
                capabilities: codegg_core::audit::audit_capabilities_dto(),
            }),
            // ── Presence and Observation M001 ──
            //
            // Presence is ephemeral and never authorizes. The M003 gate
            // above already enforced `project.observe`; these arms only
            // record/read the caller's own liveness projection.
            CoreRequest::AuditQuery { query } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".into(),
                        message: "audit query requires a durable database pool".into(),
                    });
                };
                let project = match codegg_core::identity::ProjectId::parse(&query.project_id) {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "audit_invalid_project".into(),
                            message: error.to_string(),
                        });
                    }
                };
                let action_filter = query
                    .action_filter
                    .clone()
                    .filter(|value| !value.is_empty());
                if let Some(action) = action_filter.as_deref() {
                    let unknown = codegg_core::audit::AuditAction::parse_lenient(action)
                        == codegg_core::audit::AuditAction::Unknown
                        && !action.eq_ignore_ascii_case("unknown");
                    if unknown {
                        return Ok(CoreResponse::AuditPage {
                            events: Vec::new(),
                            next_cursor: None,
                            truncated: false,
                        });
                    }
                }
                let principal_filter =
                    match query.principal_filter.clone().filter(|v| !v.is_empty()) {
                        Some(raw) => match codegg_core::identity::PrincipalId::parse(&raw) {
                            Ok(id) => Some(id),
                            Err(_) => {
                                return Ok(CoreResponse::AuditPage {
                                    events: Vec::new(),
                                    next_cursor: None,
                                    truncated: false,
                                });
                            }
                        },
                        None => None,
                    };
                // M004: the M003 gate above already enforced `audit.read`
                // for this project; the store read is coordinator-internal.
                let mut filter = codegg_core::audit::AuditQueryFilter::new(Some(project));
                if let Some(action) = action_filter {
                    filter = filter.with_action(action);
                }
                if let Some(principal) = principal_filter {
                    filter = filter.with_principal(principal);
                }
                if let Some(from_seq) = query.from_seq {
                    filter = filter.with_from_seq(from_seq);
                }
                if let Some(limit) = query.limit {
                    filter = filter.with_limit(limit);
                }
                let store = codegg_core::audit::AuditStore::new(pool);
                match store.query(&filter).await {
                    Ok(page) => {
                        // M005: self-describing audit-read event with the
                        // returned count. Emitted post-read so the envelope
                        // just returned never contains its own event.
                        {
                            let provenance =
                                codegg_core::authorization::audit_provenance(&authz_decision);
                            let mut chain =
                                codegg_core::audit_instrumentation::AuditChainContext::new();
                            chain.project = authz_decision.project_id.clone();
                            let limit = query
                                .limit
                                .unwrap_or(codegg_core::audit::DEFAULT_QUERY_LIMIT);
                            let builder = codegg_core::audit_instrumentation::audit_query_event(
                                authority.principal(),
                                &provenance,
                                &chain,
                                limit,
                                page.events.len(),
                                "allow",
                            );
                            self.append_audit_event(builder).await;
                        }
                        Ok(CoreResponse::AuditPage {
                            events: page.events.iter().map(|event| event.to_dto()).collect(),
                            next_cursor: page.next_cursor,
                            truncated: page.truncated,
                        })
                    }
                    Err(error) => Ok(CoreResponse::Error {
                        code: error.code().to_owned(),
                        message: error.to_string(),
                    }),
                }
            }
            CoreRequest::AuditExport { request } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".into(),
                        message: "audit export requires a durable database pool".into(),
                    });
                };
                let project = match codegg_core::identity::ProjectId::parse(&request.project_id) {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "audit_invalid_project".into(),
                            message: error.to_string(),
                        });
                    }
                };
                let action_filter = request.action_filter.clone().filter(|v| !v.is_empty());
                if let Some(action) = action_filter.as_deref() {
                    let unknown = codegg_core::audit::AuditAction::parse_lenient(action)
                        == codegg_core::audit::AuditAction::Unknown
                        && !action.eq_ignore_ascii_case("unknown");
                    if unknown {
                        let digest = codegg_core::audit::export_digest(&[]);
                        return Ok(CoreResponse::AuditExport {
                            events: Vec::new(),
                            digest,
                            count: 0,
                        });
                    }
                }
                let principal_filter =
                    match request.principal_filter.clone().filter(|v| !v.is_empty()) {
                        Some(raw) => match codegg_core::identity::PrincipalId::parse(&raw) {
                            Ok(id) => Some(id),
                            Err(_) => {
                                let digest = codegg_core::audit::export_digest(&[]);
                                return Ok(CoreResponse::AuditExport {
                                    events: Vec::new(),
                                    digest,
                                    count: 0,
                                });
                            }
                        },
                        None => None,
                    };
                // M004: the M003 gate above already enforced `audit.read`
                // for this project; the store read is coordinator-internal.
                let mut filter = codegg_core::audit::AuditQueryFilter::new(Some(project));
                if let Some(action) = action_filter {
                    filter = filter.with_action(action);
                }
                if let Some(principal) = principal_filter {
                    filter = filter.with_principal(principal);
                }
                if let Some(from_seq) = request.from_seq {
                    filter = filter.with_from_seq(from_seq);
                }
                if let Some(limit) = request.limit {
                    filter = filter.with_limit(limit);
                }
                let store = codegg_core::audit::AuditStore::new(pool);
                match store.export(&filter).await {
                    Ok(export) => {
                        // M005: self-describing export event with digest
                        // count. Post-read so the envelope never contains
                        // its own event.
                        {
                            let provenance =
                                codegg_core::authorization::audit_provenance(&authz_decision);
                            let mut chain =
                                codegg_core::audit_instrumentation::AuditChainContext::new();
                            chain.project = authz_decision.project_id.clone();
                            let limit = request
                                .limit
                                .unwrap_or(codegg_core::audit::MAX_EXPORT_EVENTS);
                            let builder = codegg_core::audit_instrumentation::audit_export_event(
                                authority.principal(),
                                &provenance,
                                &chain,
                                limit,
                                export.count,
                                "allow",
                            );
                            self.append_audit_event(builder).await;
                        }
                        Ok(CoreResponse::AuditExport {
                            events: export.events.iter().map(|event| event.to_dto()).collect(),
                            digest: export.digest,
                            count: export.count,
                        })
                    }
                    Err(error) => Ok(CoreResponse::Error {
                        code: error.code().to_owned(),
                        message: error.to_string(),
                    }),
                }
            }
            CoreRequest::NotificationSpeak {
                text,
                kind,
                priority,
                session_id,
            } => {
                use super::notification::*;
                let kind = match kind.as_deref() {
                    Some("turn_completed") => NotificationKind::TurnCompleted,
                    Some("turn_failed") => NotificationKind::TurnFailed,
                    Some("awaiting_input") => NotificationKind::AwaitingInput,
                    Some("permission_required") => NotificationKind::PermissionRequired,
                    Some("question_required") => NotificationKind::QuestionRequired,
                    Some("subagent_completed") => NotificationKind::SubagentCompleted,
                    Some("subagent_failed") => NotificationKind::SubagentFailed,
                    Some("error") => NotificationKind::Error,
                    _ => NotificationKind::AwaitingInput,
                };
                let priority = match priority.as_deref() {
                    Some("urgent") => NotificationPriority::Urgent,
                    Some("high") => NotificationPriority::High,
                    Some("low") => NotificationPriority::Low,
                    _ => NotificationPriority::Normal,
                };
                let event = NotificationEvent {
                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                    session_id,
                    turn_id: None,
                    kind,
                    priority,
                    message: text,
                    dedupe_key: None,
                    created_at: Utc::now(),
                };
                self.notification_router.emit(event.clone()).await;
                if let Some(ref pool) = self.pool {
                    self.notification_router
                        .persist_notification(pool, &event)
                        .await;
                }
                Ok(CoreResponse::Ack)
            }
            CoreRequest::NotificationStop => {
                if let Some(ref arbiter) = self.audio_arbiter {
                    arbiter.request_interrupt();
                }
                Ok(CoreResponse::Ack)
            }
            // ── Session Projections M2: Replay Protocol ──────────────────────
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
