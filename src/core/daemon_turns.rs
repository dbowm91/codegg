//! M002: `turns` request family for `CoreDaemon`.
//!
//! Turn submit/cancel/steer, agent/model selection writes, permission/question responses, and transport lifecycle.
//! Operates on the same daemon-owned state as the thin dispatcher;
//! introduces no new store, scheduler, state machine, or authority.

use std::sync::Arc;

use crate::error::AppError;
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse, UserPromptQueueSendNowResult};

use super::daemon::CoreDaemon;
use super::daemon_control::{queue_error_response, ControllerScope};
use super::event_log::EventFilter;

fn turn_admission_receipt_response(
    receipt: codegg_core::session_control::TurnAdmissionReceipt,
) -> CoreResponse {
    CoreResponse::TurnAdmissionReceipt {
        receipt: crate::protocol::core::TurnAdmissionReceiptDto {
            submission_id: receipt.submission_id,
            session_id: receipt.session_id,
            turn_id: receipt.turn_id,
            status: receipt.status,
            accepted_at_ms: receipt.accepted_at_ms,
            updated_at_ms: receipt.updated_at_ms,
        },
    }
}

/// Narrow domain error for daemon-side prompt composition. Protocol
/// response construction belongs to the `SessionPromptSubmit` boundary.
#[derive(Debug)]
pub(crate) struct PromptCompositionError {
    code: &'static str,
    message: String,
}

impl PromptCompositionError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn into_core_response(self) -> CoreResponse {
        CoreResponse::Error {
            code: self.code.to_owned(),
            message: self.message,
        }
    }
}

impl CoreDaemon {
    pub(crate) async fn handle_turns_request(
        &self,
        request: CoreRequest,
        request_id: &str,
        inherited_submission_digest: Option<String>,
        trusted_client_id: &str,
        authority: codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        let submission_digest = inherited_submission_digest.unwrap_or_else(|| {
            use sha2::Digest;
            let bytes = serde_json::to_vec(&request).unwrap_or_default();
            format!("{:x}", sha2::Sha256::digest(bytes))
        });
        // Session-control operations dispatch to the dedicated M004
        // handler (same Turns family, separate function to keep this
        // match small). The capability gate has already run; the
        // handler enforces lease eligibility against current team
        // state.
        match &request {
            CoreRequest::SessionControlGet { .. }
            | CoreRequest::SessionControlRequest { .. }
            | CoreRequest::SessionControlTransfer { .. }
            | CoreRequest::SessionControlRelease { .. }
            | CoreRequest::SessionControlTakeover { .. } => {
                return self
                    .handle_control_request(
                        request,
                        request_id,
                        trusted_client_id,
                        authority,
                        authz_decision,
                    )
                    .await;
            }
            _ => {}
        }
        match request {
            CoreRequest::TurnSubmit {
                session_id,
                model,
                agents,
                current_agent_idx,
                mut messages,
                human_shell_promotion_ids,
                plan_mode,
                reasoning,
                ..
            } => {
                if let Some(store) = self.controller_store() {
                    match store.get_turn_receipt(request_id).await {
                        Ok(Some(receipt)) => {
                            if receipt.session_id != session_id
                                || receipt.principal_id != authority.principal_id().as_str()
                                || receipt.request_sha256 != submission_digest
                            {
                                return Ok(CoreResponse::Error {
                                    code: "turn_submission_identity_conflict".to_owned(),
                                    message: "turn submission identity was reused with different content or authority".to_owned(),
                                });
                            }
                            return Ok(turn_admission_receipt_response(receipt));
                        }
                        Ok(None) => {}
                        Err(error) => {
                            return Ok(CoreResponse::Error {
                                code: error.code().to_owned(),
                                message: error.to_string(),
                            })
                        }
                    }
                }
                if current_agent_idx >= agents.len() {
                    crate::bus::global::GlobalEventBus::publish(
                        crate::bus::events::AppEvent::Error {
                            message: format!(
                                "Invalid agent index {} for {} agents",
                                current_agent_idx,
                                agents.len()
                            ),
                        },
                    );
                    return Ok(CoreResponse::Error {
                        code: "invalid_agent_index".to_string(),
                        message: "Invalid agent index".to_string(),
                    });
                }
                // A durable session selection is authoritative for new
                // turns. Never silently route around a selected connection
                // that has entered a non-active lifecycle state.
                let mut selected_connection = None;
                if let Some(selection_service) = self.selection_service.as_ref() {
                    if let Ok(crate::protocol::provider::SessionSelectionDto::Selected {
                        connection,
                        ..
                    }) = selection_service.get(&session_id).await
                    {
                        if connection.state != "active" {
                            return Ok(CoreResponse::Error {
                                code: "connection_state".to_string(),
                                message: format!(
                                    "selected provider connection {} is {}",
                                    connection.id, connection.state
                                ),
                            });
                        }
                        // M010: remember the exact selected revision so the
                        // turn's terminal inference outcome can feed
                        // revision-scoped connection credential qualification.
                        selected_connection = Some(connection);
                    }
                }
                // M004: the durable selection is also the authority for two
                // things the projected DTO cannot express — whether the
                // pinned model still exists in the connection's bounded
                // catalog, and which registry provider the connection-scoped
                // provider kind names. `SessionSelectionDto::Selected`
                // synthesises a zero-capability model row for a model the
                // connection cannot serve, so a turn that trusts it ships a
                // 404 to the user instead of naming the gone model.
                let mut durable_provider: Option<String> = None;
                if let Some(selection_service) = self.selection_service.as_ref() {
                    // A classification that cannot be read is not authority:
                    // it must not change this path's outcome, exactly as the
                    // projection lookup above tolerates a failed read.
                    match selection_service.classify_model(&session_id).await {
                        Ok(crate::core::session_selection::DurableModelSelection::Live { .. }) => {
                            match crate::core::session_selection::resolve_turn_provider(
                                selection_service.connection_store.as_ref(),
                                &model,
                            )
                            .await
                            {
                                Ok(provider) => durable_provider = Some(provider),
                                Err(resolve_error) => {
                                    return Ok(CoreResponse::Error {
                                        code: resolve_error.code().to_string(),
                                        message: resolve_error.message(),
                                    });
                                }
                            }
                        }
                        Ok(crate::core::session_selection::DurableModelSelection::UnknownModel {
                            connection_id,
                            model_id,
                        }) => {
                            // Reuse the selection service's own wire shape.
                            // An unavailable model is surfaced, never
                            // replaced by a different one.
                            let outcome = crate::core::session_selection::SelectionUpdateOutcome::UnknownModel { connection_id, model_id };
                            return Ok(CoreResponse::Error {
                                code: crate::core::session_selection::selection_outcome_code(
                                    &outcome,
                                )
                                .to_string(),
                                message: crate::core::session_selection::selection_outcome_message(
                                    &outcome,
                                ),
                            });
                        }
                        Ok(crate::core::session_selection::DurableModelSelection::ConnectionNotSelectable {
                            connection_id,
                            state,
                        }) => {
                            return Ok(CoreResponse::Error {
                                code: "connection_state".to_string(),
                                message: format!(
                                    "selected provider connection {} is {}",
                                    connection_id, state
                                ),
                            });
                        }
                        // No durable selection, a connection row that is gone,
                        // or a revision that moved past the pin: none of these
                        // can speak for the model, so the client-supplied model
                        // string stays the identity. A *present* connection with
                        // a *missing* model is `UnknownModel` above and never
                        // lands here.
                        Ok(
                            crate::core::session_selection::DurableModelSelection::Unselected
                            | crate::core::session_selection::DurableModelSelection::ConnectionMissing {
                                ..
                            }
                            | crate::core::session_selection::DurableModelSelection::RevisionMoved {
                                ..
                            },
                        ) => {}
                        Err(error) => {
                            tracing::debug!(
                                target: "codegg::core::daemon_turns",
                                error = %error,
                                %session_id,
                                "durable model classification unavailable; falling back to the submitted model"
                            );
                        }
                    }
                }
                // The selected connection's `secret_binding` is this turn's
                // credential locator: the exact store account the connection
                // is bound to. A turn bound to a connection must authenticate
                // with that account, not with whichever record for the same
                // provider happens to sit first in `credentials.json` — that
                // file-order pick silently undid every rotation. A session
                // with no durable selection passes `None`, leaving provider
                // resolution to the (ambiguity-checked) store fallback.
                let credential_locator = match (
                    selected_connection.as_ref(),
                    self.selection_service.as_ref(),
                ) {
                    (Some(selected), Some(selection_service)) => {
                        match codegg_core::identity::ProviderConnectionId::parse(
                            selected.id.as_str(),
                        ) {
                            Ok(connection_id) => {
                                match selection_service.connection_store.get(&connection_id).await {
                                    Ok(Some(connection)) => {
                                        connection.secret_binding.map(|binding| {
                                            crate::provider::TurnCredentialLocator {
                                                provider_ref: binding.provider_ref,
                                                account_ref: binding.account_ref,
                                            }
                                        })
                                    }
                                    Ok(None) => None,
                                    Err(error) => {
                                        tracing::debug!(
                                            target: "codegg::core::daemon_turns",
                                            error = %error,
                                            connection_id = %connection_id,
                                            "selected connection binding unreadable; the turn resolves its credential without connection authority"
                                        );
                                        None
                                    }
                                }
                            }
                            Err(_) => None,
                        }
                    }
                    // No durable model selection. The session was never bound
                    // to a connection, but the provider may still have exactly
                    // one `active` connection, and that connection is the
                    // credential authority for this turn. Without it the turn
                    // falls through to the ambiguity-checked store fallback,
                    // where the live record sits beside the orphan its rotation
                    // replaced and *every* turn for the provider is refused.
                    (_, Some(selection_service)) => {
                        let provider_name =
                            crate::core::session_selection::runtime_model_provider_segment(&model);
                        crate::core::session_selection::unbound_turn_credential_locator(
                            selection_service.connection_store.as_ref(),
                            provider_name,
                        )
                        .await
                    }
                    _ => None,
                };
                // Validate the provider exists before delegating to the turn
                // runtime. This preserves the existing `provider_not_found`
                // response shape from the daemon layer. The turn runtime
                // also validates provider existence internally, so this is
                // intentionally duplicated for backward-compatible error handling.
                let mut registry = crate::provider::ProviderRegistry::new();
                let config = super::load_config_or_default();
                // This pre-check must resolve providers with the *same*
                // authority the turn runtime uses. Built without connection
                // authority it falls through to the credential-store
                // fallback, which refuses any provider a rotation has left an
                // orphan record for — so `registry.get` came back empty and
                // this returned `provider_not_found` for a provider the daemon
                // was perfectly able to serve, aborting the turn before the
                // runtime ever ran.
                let locators = match self.pool.clone() {
                    Some(pool) => {
                        crate::core::session_selection::pool_connection_credential_locators(&pool)
                            .await
                    }
                    None => Vec::new(),
                };
                crate::provider::register_builtin_with_config_connections(
                    &mut registry,
                    &config,
                    &locators,
                    None,
                );
                let semantic_router =
                    match crate::agent::semantic_router::SemanticRouter::from_config(&config) {
                        Ok(router) => router,
                        Err(error) => {
                            return Ok(CoreResponse::Error {
                                code: "invalid_model_router_config".to_string(),
                                message: error.to_string(),
                            });
                        }
                    };
                let is_virtual_model = codegg_core::model_routing::is_virtual_model(&model);
                if is_virtual_model && !semantic_router.has_virtual_model(&model) {
                    return Ok(CoreResponse::Error {
                        code: "semantic_router_not_found".to_string(),
                        message: format!("Semantic model router not found: {}", model),
                    });
                }
                if !is_virtual_model {
                    // The registry check runs against the *resolved*
                    // provider. With a live durable selection that is the
                    // connection's own provider kind — a durable model id is
                    // `<connection-scoped kind>/<model>` (e.g.
                    // `opencode_go/minimax-m3`, stored as `other:opencode_go`),
                    // and neither the storage key nor a bare string split is a
                    // registry id. Only a session with no durable authority
                    // left falls back to the client-supplied segment.
                    let provider_name = match durable_provider.as_deref() {
                        Some(provider) => provider.to_owned(),
                        None => {
                            crate::core::session_selection::runtime_model_provider_segment(&model)
                                .to_owned()
                        }
                    };
                    if registry.get(&provider_name).is_none() {
                        crate::bus::global::GlobalEventBus::publish(
                            crate::bus::events::AppEvent::Error {
                                message: format!(
                                    "Provider '{}' not found. Please check your configuration.",
                                    provider_name
                                ),
                            },
                        );
                        return Ok(CoreResponse::Error {
                            code: "provider_not_found".to_string(),
                            message: format!("Provider not found: {}", provider_name),
                        });
                    }
                }

                let runtime = match self.bind_runtime_for_session(&session_id).await {
                    Ok(rt) => rt,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "session_unbound".to_string(),
                            message: format!(
                                "session {} has no resolvable workspace: {}",
                                session_id, e
                            ),
                        });
                    }
                };
                // Durable delegated runs need the session's canonical
                // repository identity so the scheduler can allocate their
                // managed worktrees. SessionRuntime intentionally stores the
                // workspace locator and project compatibility projection,
                // not repository lineage; resolve the full bound context at
                // the turn boundary instead of silently constructing
                // repository-less AgentTasks.
                let repository_id = match self
                    .resolve_session_context(&session_id, &runtime.directory.to_string_lossy())
                    .await
                {
                    Ok(context) => context.repository_id,
                    Err(error) => return Err(error),
                };

                // Session-open and manual refreshes converge here as the
                // final correctness gate: the turn captures the currently
                // published immutable generation before runtime assembly.
                let asset_refresh = match self
                    .refresh_runtime_assets(
                        &runtime,
                        &session_id,
                        crate::agent::asset_refresh::RefreshReason::SessionLifecycle,
                    )
                    .await
                {
                    Ok(report) => report,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "turn_asset_refresh_failed".to_string(),
                            message: error.to_string(),
                        });
                    }
                };
                if let Some(error) = Self::refresh_report_error(&asset_refresh) {
                    return Ok(CoreResponse::Error {
                        code: "turn_asset_refresh_failed".to_string(),
                        message: error.to_string(),
                    });
                }
                let asset_scope = crate::agent::asset_refresh::AssetScope::new(
                    &runtime.project_id,
                    runtime.workspace_id.as_str(),
                );
                let asset_snapshot =
                    self.asset_refresh
                        .snapshot(&asset_scope)
                        .await
                        .map(|published| {
                            (
                                published.snapshot.clone(),
                                std::sync::Arc::new(std::sync::Mutex::new(
                                    published.runtime_asset_pin(),
                                )),
                            )
                        });
                let (asset_snapshot, asset_pin) = asset_snapshot
                    .map(|(snapshot, pin)| (Some(snapshot), Some(pin)))
                    .unwrap_or((None, None));

                let turn_id = {
                    let mut active = runtime.active_turn.write().await;
                    if active.is_some() {
                        return Ok(CoreResponse::Error {
                            code: "turn_already_active".to_string(),
                            message: "A turn is already active for this session".to_string(),
                        });
                    }
                    let turn_id = format!("turn-{}", uuid::Uuid::new_v4());
                    *active = Some(crate::core::session_runtime::TurnHandle {
                        turn_id: turn_id.clone(),
                        cancel_tx: tokio::sync::watch::channel(false).0,
                        steer_tx: None,
                        started_at: chrono::Utc::now(),
                        asset_pin: asset_pin.clone(),
                        controller_principal: None,
                        controller_client: None,
                        controller_revision: 0,
                    });
                    turn_id
                };

                // M004 (ADR-0007): atomic controller acquisition with
                // accepting the turn. The durable row is written before
                // any execution starts; a store conflict rolls back the
                // in-memory turn so a failed submission leaves no lease.
                // LocalOwner broad policy still records a lease (solo
                // behavior unchanged: the submitter controls its turn)
                // so projection/presence stay coherent.
                let now_ms = chrono::Utc::now().timestamp_millis();
                let (admitted_promotions, admission_receipt) = match self
                    .acquire_turn_controller_with_promotions(
                        super::daemon_control::TurnControllerAdmission {
                            session_id: &session_id,
                            submission_id: request_id,
                            request_sha256: &submission_digest,
                            turn_id: &turn_id,
                            authority: &authority,
                            trusted_client_id,
                            project_id: &runtime.project_id,
                            workspace_id: runtime.workspace_id.as_str(),
                            promotion_ids: &human_shell_promotion_ids,
                            now_ms,
                        },
                    )
                    .await
                {
                    Ok(admission) if admission.duplicate => {
                        if let Some(runtime) = self.sessions.get(&session_id) {
                            let mut active = runtime.active_turn.write().await;
                            if active
                                .as_ref()
                                .is_some_and(|handle| handle.turn_id == turn_id)
                            {
                                *active = None;
                            }
                            drop(active);
                            *runtime.status.write().await =
                                crate::core::session_runtime::RuntimeSessionStatus::Idle;
                        }
                        return Ok(turn_admission_receipt_response(admission.receipt));
                    }
                    Ok(admission) => {
                        self.mirror_controller_to_runtime(
                            &session_id,
                            &turn_id,
                            admission
                                .controller_principal
                                .as_deref()
                                .unwrap_or(authority.principal_id().as_str()),
                            Some(trusted_client_id),
                            admission.revision,
                        )
                        .await;
                        (admission.promotions, admission.receipt)
                    }
                    Err(error) => {
                        // Roll back: failed submissions must not leave a
                        // controller lease or a stuck active turn behind.
                        if let Some(runtime) = self.sessions.get(&session_id) {
                            let mut active = runtime.active_turn.write().await;
                            if let Some(handle) = active.as_ref() {
                                if handle.turn_id == turn_id {
                                    *active = None;
                                }
                            }
                            drop(active);
                            let mut status = runtime.status.write().await;
                            *status = crate::core::session_runtime::RuntimeSessionStatus::Idle;
                        }
                        return Ok(CoreResponse::Error {
                            code: match &error {
                                codegg_core::session_control::SessionTurnAdmissionError::Controller(_) => {
                                    "turn_already_active"
                                }
                                codegg_core::session_control::SessionTurnAdmissionError::Promotion(promotion) => {
                                    promotion.code()
                                }
                            }
                            .to_string(),
                            message: format!("turn submission could not claim session state: {error}"),
                        });
                    }
                };
                for promotion in &admitted_promotions {
                    self.record_human_shell_promotion_stage(
                        &authority,
                        &authz_decision,
                        promotion,
                        "consumed",
                    )
                    .await;
                }
                for promotion in admitted_promotions {
                    messages.push(crate::protocol::dto::ProviderMessage::User {
                        content: vec![crate::protocol::dto::ContentPart::Text {
                            text: promotion.projection,
                        }],
                    });
                }
                // The terminal transition wins over later transfer: the
                // reaper releases the lease on the first TurnCompleted /
                // TurnFailed for this exact turn id.
                self.spawn_turn_reaper(session_id.clone(), turn_id.clone());

                {
                    let mut status = runtime.status.write().await;
                    *status = crate::core::session_runtime::RuntimeSessionStatus::Running;
                }

                // Emit TurnStarted immediately so subscribers (and the bridge
                // fallback) see a coherent turn identity from the first event.
                self.event_log
                    .publish(
                        Some(session_id.clone()),
                        Some(turn_id.clone()),
                        crate::protocol::core::CoreEvent::TurnStarted {
                            session_id: session_id.clone(),
                            turn_id: turn_id.clone(),
                        },
                    )
                    .await;

                // M003: capture originating-principal attribution for the turn.
                self.record_origin_with_decision(
                    &authority,
                    &authz_decision,
                    "turn",
                    turn_id.as_str(),
                )
                .await;

                // Presence M001: meaningful agent activity. Best-effort
                // only; turn correctness never depends on presence.
                self.note_presence_activity(
                    &authority,
                    trusted_client_id,
                    authz_decision.project_id.clone(),
                    Some(session_id.as_str()),
                    codegg_core::presence::PresenceActivity::AgentRunning,
                );

                // Build an immutable execution context from the bound
                // runtime's workspace identity. The context flows through
                // every daemon-owned execution path inside the turn.
                let workspace_record = codegg_core::workspace::WorkspaceRecord {
                    id: runtime.workspace_id.clone(),
                    canonical_root: runtime.workspace_root.clone(),
                    display_name: runtime
                        .workspace_root
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| runtime.workspace_root.to_string_lossy().into_owned()),
                    created_at: chrono::Utc::now(),
                    last_opened_at: chrono::Utc::now(),
                    archived_at: None,
                };
                let execution = codegg_core::workspace::ExecutionContext::new(
                    Arc::new(workspace_record),
                    Some(session_id.clone()),
                    Default::default(),
                );
                // Pin the resolved root to the log: a turn whose tools
                // all fail with "not a directory" is otherwise
                // indistinguishable from a model looping on bad paths.
                tracing::info!(
                    session_id = %session_id,
                    turn_id = %turn_id,
                    workspace_root = %execution.workspace_root.display(),
                    "turn execution context bound",
                );

                // Delegate to the injected turn runtime which handles tool
                // registry, agent loop construction, and background spawning.
                let plugin_service = match crate::plugin::create_default_plugin_service().await {
                    Some(service) => match service
                        .for_workspace(runtime.workspace_id.to_string())
                        .await
                    {
                        Ok(mut contextual) => {
                            // Bind plugin invocation context to the
                            // authoritative workspace root rather than the
                            // daemon process CWD.
                            contextual.set_workspace_root(execution.workspace_root.clone());
                            Some(Arc::new(contextual))
                        }
                        Err(error) => {
                            tracing::error!(%error, "failed to resolve plugin activation for workspace");
                            None
                        }
                    },
                    None => None,
                };
                // Retain the daemon-owned workspace service for the detached
                // turn. This keeps its canonical lock table alive until the
                // loop exits, so checkpoint capture and native mutation share
                // one inter-session repository authority.
                let workspace_service_lease = self
                    .workspace_services
                    .acquire(&runtime.workspace_id)
                    .await
                    .map_err(|error| AppError::Other(anyhow::anyhow!(error.to_string())))?;
                // M003: resolve the persisted principal approval/sandbox
                // preference for this turn. Restart reloads the last
                // preference; explicit turn overrides (none in this path)
                // would win, then project ceiling, then persisted, then
                // Interactive/WorkspaceWrite defaults. The snapshot is
                // captured by the loop at the batch boundary so concurrent
                // mode changes apply on the next turn, never retroactively.
                let (approval_mode, sandbox_profile) = match self.pool.clone() {
                    None => (None, None),
                    Some(pool) => {
                        let store = codegg_core::approval::RuntimePreferenceStore::new(pool);
                        let principal = authority.principal_id().as_str().to_owned();
                        match store.get(&principal).await {
                            Ok(Some(pref)) => (
                                Some(pref.effective_approval_mode()),
                                Some(pref.effective_sandbox_profile()),
                            ),
                            Ok(None) => (None, None),
                            Err(error) => {
                                tracing::warn!(
                                    error = %error,
                                    "approval preference read failed; using defaults"
                                );
                                (None, None)
                            }
                        }
                    }
                };
                let credential_observer = selected_connection.as_ref().and_then(|connection| {
                    // M010: bind this turn's terminal inference outcome to the
                    // exact selected connection revision. A turn that is not
                    // bound to a durable connection produces no verdict.
                    self.pool.as_ref().map(|pool| {
                        std::sync::Arc::new(
                            crate::core::provider_qualification::ProviderConnectionCredentialReporter::new(
                                pool.clone(),
                                std::sync::Arc::from(connection.id.as_str()),
                                connection.revision,
                            ),
                        ) as crate::agent::provider_qualification::SharedProviderCredentialObserver
                    })
                });
                let turn_input = crate::agent::turn_runtime::TurnRunInput {
                    session_id: session_id.clone(),
                    origin_principal: Some(authority.principal_id().clone()),
                    agents_dto: agents,
                    current_agent_idx,
                    model,
                    messages_dto: messages,
                    plan_mode,
                    reasoning_override: reasoning,
                    config,
                    pool: self.pool.clone(),
                    subagent_pool: self.deps.legacy_agent.subagent_pool.clone(),
                    memory_store: self.deps.memory_store.clone(),
                    event_log: Arc::clone(&self.event_log),
                    turn_id: turn_id.clone(),
                    lsp_service: self.deps.lsp_service.clone(),
                    document_service: Some(self.documents.clone()),
                    lsp_context_input: None,
                    plugin_service,
                    execution,
                    submission: self.deps.submission.clone(),
                    workspace_service_lease: Some(workspace_service_lease),
                    agent_run_store: self.deps.agent_run_store.clone(),
                    convergence_store: self.deps.convergence_store.clone(),
                    run_control: self.deps.run_control.clone(),
                    run_group_service: self.deps.run_group_service.clone(),
                    project_id: codegg_core::identity::ProjectId::parse(&runtime.project_id).ok(),
                    repository_id,
                    asset_snapshot,
                    asset_pin,
                    approval_mode,
                    sandbox_profile,
                    credential_observer,
                    // C001 daemon singletons: every turn shares one notifier
                    // + one watcher; closing a turn leaves watchers live.
                    shared_job_notification_service: self.deps.job_notification_service.clone(),
                    shared_ci_watch_service: self.deps.ci_watch_service.clone(),
                    credential_locator,
                };
                let turn_output = match self.deps.turn_runtime.run_turn(turn_input).await {
                    Ok(output) => output,
                    Err(error) => {
                        if let Some(store) = self.controller_store() {
                            if let Err(receipt_error) = store
                                .finish_turn_receipt(
                                    &turn_id,
                                    "failed",
                                    chrono::Utc::now().timestamp_millis(),
                                )
                                .await
                            {
                                tracing::warn!(error = %receipt_error, turn_id = %turn_id, "failed to close rejected runtime admission receipt");
                            }
                        }
                        self.event_log
                            .publish(
                                Some(session_id.clone()),
                                Some(turn_id.clone()),
                                CoreEvent::TurnFailed {
                                    session_id: session_id.clone(),
                                    turn_id: Some(turn_id.clone()),
                                    message: error.to_string(),
                                },
                            )
                            .await;
                        self.release_turn_controller(&session_id, &turn_id).await;
                        return Err(error);
                    }
                };

                // Update the TurnHandle with the runtime's cancel/steer channels.
                {
                    let mut active = runtime.active_turn.write().await;
                    if let Some(handle) = active.as_mut() {
                        handle.cancel_tx = turn_output.cancel_tx;
                        handle.steer_tx = Some(turn_output.steer_tx);
                    }
                }

                Ok(turn_admission_receipt_response(admission_receipt))
            }
            CoreRequest::SessionPromptSubmit {
                session_id,
                text,
                plan_mode,
                human_shell_promotion_ids,
            } => {
                if let Some(store) = self.controller_store() {
                    match store.get_turn_receipt(request_id).await {
                        Ok(Some(receipt)) => {
                            if receipt.session_id != session_id
                                || receipt.principal_id != authority.principal_id().as_str()
                                || receipt.request_sha256 != submission_digest
                            {
                                return Ok(CoreResponse::Error {
                                    code: "turn_submission_identity_conflict".to_owned(),
                                    message: "turn submission identity was reused with different content or authority".to_owned(),
                                });
                            }
                            return Ok(turn_admission_receipt_response(receipt));
                        }
                        Ok(None) => {}
                        Err(error) => {
                            return Ok(CoreResponse::Error {
                                code: error.code().to_owned(),
                                message: error.to_string(),
                            })
                        }
                    }
                }
                // M004 desktop session slice: resolve the composed
                // turn daemon-side, then submit through the identical
                // `TurnSubmit` path above (single recursion, no new
                // runtime, scheduler, or authority).
                let (model, agents, messages) = match self
                    .resolve_prompt_submit_composition(&session_id, &text)
                    .await
                {
                    Ok(composed) => composed,
                    Err(error) => return Ok(error.into_core_response()),
                };
                return Box::pin(self.handle_turns_request(
                    CoreRequest::TurnSubmit {
                        session_id,
                        text,
                        plan_mode,
                        model,
                        agents,
                        current_agent_idx: 0,
                        messages,
                        reasoning: None,
                        human_shell_promotion_ids,
                    },
                    request_id,
                    Some(submission_digest),
                    trusted_client_id,
                    authority,
                    authz_decision,
                ))
                .await;
            }
            CoreRequest::HumanShellPromotionStage {
                session_id,
                promotion_id,
                source_id,
                projection,
            } => {
                return Ok(self
                    .stage_human_shell_promotion(
                        &session_id,
                        &promotion_id,
                        &source_id,
                        &projection,
                        &authority,
                        &authz_decision,
                    )
                    .await);
            }
            CoreRequest::HumanShellPromotionsGet { session_id } => {
                return Ok(self
                    .get_human_shell_promotions(&session_id, &authority)
                    .await);
            }
            CoreRequest::UserPromptQueueList { session_id } => {
                return Ok(self.list_user_prompt_queue(&session_id).await);
            }
            // M002b (ADR-0037): every queue mutation goes through one handler
            // that runs the capability gate's follow-up — the durable queue
            // owner — before any write. Routing them to a single function keeps
            // that from being re-implemented per operation.
            CoreRequest::UserPromptQueueAdd {
                session_id,
                entry_id,
                text,
                run_at_ms,
            } => {
                return Ok(self
                    .handle_user_prompt_queue_mutation(
                        session_id,
                        QueueMutation::Add {
                            entry_id: &entry_id,
                            text: &text,
                            run_at_ms,
                        },
                        &authority,
                        &authz_decision,
                    )
                    .await);
            }
            CoreRequest::UserPromptQueueEdit {
                session_id,
                entry_id,
                text,
                run_at_ms,
            } => {
                return Ok(self
                    .handle_user_prompt_queue_mutation(
                        session_id,
                        QueueMutation::Edit {
                            entry_id: &entry_id,
                            text: &text,
                            run_at_ms,
                        },
                        &authority,
                        &authz_decision,
                    )
                    .await);
            }
            CoreRequest::UserPromptQueueDelete {
                session_id,
                entry_id,
            } => {
                return Ok(self
                    .handle_user_prompt_queue_mutation(
                        session_id,
                        QueueMutation::Delete {
                            entry_id: &entry_id,
                        },
                        &authority,
                        &authz_decision,
                    )
                    .await);
            }
            CoreRequest::UserPromptQueueReorder {
                session_id,
                entry_ids,
            } => {
                return Ok(self
                    .handle_user_prompt_queue_mutation(
                        session_id,
                        QueueMutation::Reorder {
                            entry_ids: &entry_ids,
                        },
                        &authority,
                        &authz_decision,
                    )
                    .await);
            }
            CoreRequest::UserPromptQueueAdopt { session_id } => {
                return Ok(self
                    .handle_user_prompt_queue_mutation(
                        session_id,
                        QueueMutation::Adopt,
                        &authority,
                        &authz_decision,
                    )
                    .await);
            }
            CoreRequest::UserPromptQueueTransfer {
                session_id,
                recipient_principal,
                expected_revision,
            } => {
                return Ok(self
                    .handle_user_prompt_queue_mutation(
                        session_id,
                        QueueMutation::Transfer {
                            recipient: &recipient_principal,
                            expected_revision,
                        },
                        &authority,
                        &authz_decision,
                    )
                    .await);
            }
            // M006 (ADR-0036): deliberately NOT a `QueueMutation`. It reaches the
            // running turn, and ADR-0037 excludes steering from the durable owner
            // binding — so gating it here on `ControllerScope::QueueMutation`
            // would refuse the legitimate case of a caller steering its own turn
            // in a session it does not own. The turn controller is the only gate
            // that applies; the queue-side ownership check happens inside the
            // settlement write, where it is answering a different question
            // ("may this principal change this queue?").
            CoreRequest::UserPromptQueueSendNow {
                session_id,
                entry_id,
            } => {
                return Ok(self
                    .handle_user_prompt_queue_send_now(
                        &session_id,
                        &entry_id,
                        &authority,
                        &authz_decision,
                    )
                    .await);
            }
            CoreRequest::PermissionRespond { id, choice } => {
                let parsed = match choice.as_str() {
                    "allow" => crate::bus::PermissionDecision::AllowOnce,
                    "always_allow" => crate::bus::PermissionDecision::AlwaysAllow,
                    "deny" => crate::bus::PermissionDecision::DenyOnce,
                    "always_deny" => crate::bus::PermissionDecision::AlwaysDeny,
                    _ => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_permission_choice".to_string(),
                            message: format!("Invalid permission choice: {}", choice),
                        });
                    }
                };
                // Extract session_id, turn_id, and simple perm_id from protocol ID:
                // perm:{session_id}:{turn_id}:{perm_id}. Reject malformed IDs
                // explicitly rather than silently using empty defaults
                // (which could route a response to the wrong session).
                // The owning session/turn is resolved before the
                // controller check so opaque IDs cannot bypass it.
                //
                // A bare simple id is also accepted. The local TUI receives
                // `AppEvent::PermissionPending` straight off the bus, where
                // `perm_id` is the unprefixed `{tool_call_id}-{tool}` form, and
                // echoes it back verbatim. Previously that reply was rejected
                // with `invalid_permission_id`, the responder's oneshot was
                // never completed, and the human wait — unbounded by default —
                // blocked the tool forever with no dialog and no error.
                // Ownership is recovered from the registry (which fails closed
                // when the id is unknown or ambiguous) so the controller check
                // below still runs against the true owner.
                let (session_id, turn_id, simple_perm_id) =
                    match id.strip_prefix("perm:").and_then(|rest| {
                        let mut parts = rest.splitn(3, ':');
                        let sid = parts.next()?.to_string();
                        let turn = parts.next()?.to_string();
                        let pid = parts.next()?.to_string();
                        Some((sid, turn, pid))
                    }) {
                        Some((sid, turn, pid)) => (sid, turn, pid),
                        None => match crate::bus::PermissionRegistry::resolve_pending_owner(&id) {
                            Some((sid, turn)) => (sid, turn.unwrap_or_default(), id.clone()),
                            None => {
                                return Ok(CoreResponse::Error {
                                    code: "invalid_permission_id".to_string(),
                                    message: format!(
                                        "Permission ID '{}' matches no pending request; expected \
                                     perm:<session_id>:<turn_id>:<perm_id> or a pending simple id",
                                        id
                                    ),
                                });
                            }
                        },
                    };
                // M004: permission responses are in-flight human control.
                // The capability gate for this Global operation is `none`,
                // so the handler enforces the owning-session mutation
                // authority plus the controller lease here. Failures use
                // the no-pending shape when the caller lacks session
                // authority (no existence oracle) and the typed
                // controller code when an authorized member is not the
                // controller.
                if let Some(denial) = self
                    .check_control_response(&session_id, &turn_id, &authority, &authz_decision)
                    .await
                {
                    return Ok(denial);
                }
                let sent = crate::bus::PermissionRegistry::respond_scoped(
                    &session_id,
                    &simple_perm_id,
                    parsed,
                );
                if sent {
                    // Remove from session runtime's pending set
                    if let Some(runtime) = self.sessions.get(&session_id) {
                        runtime.pending_permissions.remove(&id);
                    }
                    // Emit PermissionResponded event
                    crate::bus::global::GlobalEventBus::publish(
                        crate::bus::events::AppEvent::PermissionResponded {
                            session_id,
                            tool: String::new(),
                            allowed: parsed.allowed(),
                        },
                    );
                    Ok(CoreResponse::Ack)
                } else {
                    Ok(CoreResponse::Error {
                        code: "permission_response_failed".to_string(),
                        message: "No pending permission request found".to_string(),
                    })
                }
            }
            CoreRequest::QuestionRespond { id, answers } => {
                // Extract session_id, turn_id, and simple question_id from
                // protocol ID: question:{session_id}:{turn_id}:{question_id}.
                // Reject malformed IDs explicitly.
                //
                // As with permissions, a bare simple id is accepted and its
                // owner resolved from the registry, so the local TUI's
                // verbatim echo of `AppEvent::QuestionPending` is not dropped
                // on the floor (which left the questioner waiting forever).
                let (session_id, turn_id, simple_question_id) = match id
                    .strip_prefix("question:")
                    .and_then(|rest| {
                        let mut parts = rest.splitn(3, ':');
                        let sid = parts.next()?.to_string();
                        let turn = parts.next()?.to_string();
                        let qid = parts.next()?.to_string();
                        Some((sid, turn, qid))
                    }) {
                    Some(parsed) => parsed,
                    None => match crate::bus::QuestionRegistry::resolve_pending_owner(&id) {
                        Some((sid, turn)) => (sid, turn.unwrap_or_default(), id.clone()),
                        None => {
                            return Ok(CoreResponse::Error {
                                code: "invalid_question_id".to_string(),
                                message: format!(
                                    "Question ID '{}' matches no pending request; expected \
                                     question:<session_id>:<turn_id>:<question_id> or a pending simple id",
                                    id
                                ),
                            });
                        }
                    },
                };
                // M004: same in-flight control predicate as permissions.
                if let Some(denial) = self
                    .check_control_response(&session_id, &turn_id, &authority, &authz_decision)
                    .await
                {
                    return Ok(denial);
                }
                let sent = crate::bus::QuestionRegistry::answer_question_scoped(
                    &session_id,
                    &simple_question_id,
                    answers.to_string(),
                );
                if sent {
                    // Remove from session runtime's pending set
                    if let Some(runtime) = self.sessions.get(&session_id) {
                        runtime.pending_questions.remove(&id);
                    }
                    // Emit QuestionAnswered event
                    crate::bus::global::GlobalEventBus::publish(
                        crate::bus::events::AppEvent::QuestionAnswered {
                            session_id,
                            answers: answers.to_string(),
                        },
                    );
                    Ok(CoreResponse::Ack)
                } else {
                    Ok(CoreResponse::Error {
                        code: "question_response_failed".to_string(),
                        message: "No pending question found".to_string(),
                    })
                }
            }
            CoreRequest::ModelsRefresh => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let config = super::load_config_or_default();
                let mut registry = crate::provider::ProviderRegistry::new();
                // A live connection is the authority for its own provider, so
                // the picker sees the same provider set a turn would use. Without
                // it a rotated provider vanishes from `/models` and the TUI
                // renders a permanently empty list.
                let locators =
                    crate::core::session_selection::pool_connection_credential_locators(&pool)
                        .await;
                crate::provider::register_builtin_with_config_connections(
                    &mut registry,
                    &config,
                    &locators,
                    None,
                );
                let discovery = crate::provider::discovery::ModelDiscoveryService::new(
                    std::path::PathBuf::new(),
                )
                .with_pool(pool);
                let models = discovery.refresh(&registry).await;
                let model_ids: Vec<String> = models
                    .iter()
                    .map(|m| format!("{}/{}", m.provider, m.id))
                    .collect();
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "models": model_ids }),
                })
            }
            CoreRequest::Subscribe { session_id } => {
                let current_seq = self.event_log.current_seq();
                Ok(CoreResponse::Json {
                    data: serde_json::json!({
                        "current_seq": current_seq,
                        "session_id": session_id,
                    }),
                })
            }
            CoreRequest::Resume {
                session_id,
                from_event_seq,
            } => {
                let filter = EventFilter {
                    session_id: session_id.clone(),
                    client_id: None,
                    include_global: true,
                };

                let current_seq = self.event_log.current_seq();

                // `ResyncRequired` means "the requested sequence is too old
                // to replay from available event storage" -- not "there are
                // no new events". A client that is already caught up
                // (from_event_seq >= current_seq) gets an empty `Events`
                // response so the resume handshake always completes for
                // in-sync clients.
                if !self.event_log.covers_from(from_event_seq).await {
                    return Ok(CoreResponse::ResyncRequired {
                        from_event_seq,
                        current_seq,
                        session_id,
                    });
                }

                // Already caught up (or covered by the ring/DB): return
                // an empty events vector. A future `from_event_seq` (above
                // `current_seq`) also returns empty here rather than
                // erroring; clients treat that as a no-op resume.
                if from_event_seq >= current_seq {
                    return Ok(CoreResponse::Events {
                        events: Vec::new(),
                        current_seq,
                    });
                }

                let events = self.event_log.replay_from(from_event_seq, &filter).await;
                Ok(CoreResponse::Events {
                    events,
                    current_seq,
                })
            }
            CoreRequest::TurnCancel {
                session_id,
                turn_id,
            } => {
                let Some(runtime) = self.sessions.get(&session_id) else {
                    return Ok(CoreResponse::Error {
                        code: "session_not_found".to_string(),
                        message: format!("No runtime for session: {}", session_id),
                    });
                };
                // M004: controller authorization runs after the
                // capability gate. Only the submitting principal (any
                // of its devices) may cancel the active turn; an
                // eligible Maintainer/Owner recovers via explicit
                // takeover, never by direct cancel.
                if let Some(denial) = self
                    .check_turn_controller(&session_id, &turn_id, &authority, &authz_decision)
                    .await
                {
                    return Ok(denial);
                }
                let active = runtime.active_turn.read().await;
                match active.as_ref() {
                    Some(handle) if handle.turn_id == turn_id => {
                        if handle.cancel_tx.send(true).is_err() {
                            tracing::debug!(turn_id = %turn_id, "turn cancellation receiver already closed");
                        }
                        Ok(CoreResponse::Ack)
                    }
                    Some(handle) => Ok(CoreResponse::Error {
                        code: "turn_id_mismatch".to_string(),
                        message: format!(
                            "Requested turn_id '{}' does not match active turn_id '{}'",
                            turn_id, handle.turn_id
                        ),
                    }),
                    None => Ok(CoreResponse::Error {
                        code: "no_active_turn".to_string(),
                        message: "No active turn to cancel".to_string(),
                    }),
                }
            }
            CoreRequest::TurnSteer {
                session_id,
                turn_id,
                text,
            } => {
                let Some(runtime) = self.sessions.get(&session_id) else {
                    return Ok(CoreResponse::Error {
                        code: "session_not_found".to_string(),
                        message: format!("No runtime for session: {}", session_id),
                    });
                };
                // M004: same controller predicate as cancel. Observers
                // and non-controller Contributors cannot steer another
                // human's active turn.
                if let Some(denial) = self
                    .check_turn_controller(&session_id, &turn_id, &authority, &authz_decision)
                    .await
                {
                    return Ok(denial);
                }
                let active = runtime.active_turn.read().await;
                match active.as_ref() {
                    Some(handle) if handle.turn_id == turn_id => {
                        if let Some(ref steer_tx) = handle.steer_tx {
                            match steer_tx.try_send(text) {
                                Ok(()) => Ok(CoreResponse::Ack),
                                Err(ref error) => {
                                    // M006: the code comes from the shared
                                    // mapping so direct steering and send-now
                                    // cannot drift apart; only the prose here.
                                    let code = steer_channel_code(error);
                                    tracing::debug!(turn_id = %turn_id, code = code, "turn steering refused");
                                    Ok(CoreResponse::Error {
                                        code: code.to_string(),
                                        message: match code {
                                            "steer_queue_full" => {
                                                "Turn steering queue is full".to_string()
                                            }
                                            _ => {
                                                "Turn steering receiver is unavailable".to_string()
                                            }
                                        },
                                    })
                                }
                            }
                        } else {
                            Ok(CoreResponse::Error {
                                code: "steer_not_supported".to_string(),
                                message: "Turn does not support steering".to_string(),
                            })
                        }
                    }
                    Some(handle) => Ok(CoreResponse::Error {
                        code: "turn_id_mismatch".to_string(),
                        message: format!(
                            "Requested turn_id '{}' does not match active turn_id '{}'",
                            turn_id, handle.turn_id
                        ),
                    }),
                    None => Ok(CoreResponse::Error {
                        code: "no_active_turn".to_string(),
                        message: "No active turn to steer".to_string(),
                    }),
                }
            }
            CoreRequest::AgentSelect {
                session_id,
                agent_name,
            } => {
                let runtime = match self.bind_runtime_for_session(&session_id).await {
                    Ok(rt) => rt,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "session_unbound".to_string(),
                            message: format!(
                                "session {} has no resolvable workspace: {}",
                                session_id, e
                            ),
                        });
                    }
                };
                {
                    let mut selected = runtime.selected_agent.write().await;
                    *selected = Some(agent_name.clone());
                }
                crate::bus::global::GlobalEventBus::publish(
                    crate::bus::events::AppEvent::SessionUpdated {
                        id: session_id.clone(),
                    },
                );
                Ok(CoreResponse::Ack)
            }
            CoreRequest::ModelSelect { session_id, model } => {
                // M004 convergence: `ModelSelect` is a compatibility
                // adapter over the durable `SelectionService`. The
                // runtime cache is only updated after durable success;
                // failures leave both durable state and the cache
                // untouched (no silent fallback, no runtime-only
                // authority).
                let Some(service) = self.selection_service.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "session_selection_unavailable".to_string(),
                        message: "Model selection requires a daemon SQLite catalog".to_string(),
                    });
                };
                let (connection_id, model_id) =
                    match crate::core::session_selection::resolve_model_select_target(
                        service.connection_store.as_ref(),
                        &model,
                    )
                    .await
                    {
                        Ok(target) => target,
                        Err(resolve_error) => {
                            return Ok(CoreResponse::Error {
                                code: resolve_error.code().to_string(),
                                message: resolve_error.message(),
                            });
                        }
                    };
                // Resolve the current connection/catalog revision first
                // and perform a CAS update so a concurrent catalog bump
                // surfaces a stale diagnostic instead of overwriting.
                let (expected_connection_revision, expected_catalog_revision) =
                    match service.connection_store.get(&connection_id).await {
                        Ok(Some(connection)) => {
                            let catalog = service
                                .models(&session_id, &connection_id)
                                .await
                                .map(|(revision, _)| revision)
                                .unwrap_or(None);
                            (Some(connection.revision), catalog)
                        }
                        Ok(None) => (None, None),
                        Err(error) => {
                            return Ok(CoreResponse::Error {
                                code: "connection_store_error".to_string(),
                                message: error.to_string(),
                            });
                        }
                    };
                match service
                    .update(
                        &session_id,
                        &connection_id,
                        &model_id,
                        expected_connection_revision,
                        expected_catalog_revision,
                    )
                    .await
                {
                    Ok(crate::core::session_selection::SelectionUpdateOutcome::Updated(
                        selection,
                    )) => {
                        // Durable success: project the canonical
                        // `provider/model` string into the runtime cache.
                        // Best-effort only; the durable row is canonical
                        // and `SnapshotSession` lazily reconciles on read.
                        let canonical =
                            crate::core::session_selection::durable_selected_runtime_model(
                                &selection,
                            )
                            .unwrap_or_else(|| model.clone());
                        if let Some(runtime) = self.sessions.get(&session_id) {
                            let mut selected = runtime.selected_model.write().await;
                            *selected = Some(canonical);
                        }
                        // Remember the principal's last-used preference.
                        // A preference write failure never rolls back the
                        // explicit selection; it is reported via tracing
                        // and the selection still acknowledges.
                        if let Some(pool) = self.pool.clone() {
                            let principal_id = authority.principal_id().as_str().to_owned();
                            let preference_store =
                                codegg_core::approval::RuntimePreferenceStore::new(pool);
                            if let Err(error) = preference_store
                                .set_model_preference(
                                    &principal_id,
                                    Some(connection_id.as_str()),
                                    Some(&model_id),
                                    None,
                                )
                                .await
                            {
                                tracing::warn!(
                                    error = %error,
                                    session_id = %session_id,
                                    "model selection persisted but last-used preference was not saved"
                                );
                            }
                        }
                        crate::bus::global::GlobalEventBus::publish(
                            crate::bus::events::AppEvent::SessionUpdated {
                                id: session_id.clone(),
                            },
                        );
                        Ok(CoreResponse::Ack)
                    }
                    Ok(other) => Ok(CoreResponse::Error {
                        code: crate::core::session_selection::selection_outcome_code(&other)
                            .to_string(),
                        message: crate::core::session_selection::selection_outcome_message(&other),
                    }),
                    Err(error) => Ok(CoreResponse::Error {
                        code: crate::core::session_selection::selection_error_code(&error)
                            .to_string(),
                        message: crate::core::session_selection::selection_error_message(&error),
                    }),
                }
            }
            CoreRequest::SnapshotSession { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool.clone());
                let msg_store = crate::session::MessageStore::new(pool);

                let session = match store.get(&session_id).await {
                    Ok(Some(s)) => s,
                    Ok(None) => {
                        return Ok(CoreResponse::Error {
                            code: "session_not_found".to_string(),
                            message: format!("Session not found: {}", session_id),
                        })
                    }
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "session_load_failed".to_string(),
                            message: e.to_string(),
                        })
                    }
                };

                let messages = match msg_store.list(&session_id).await {
                    Ok(messages) => messages,
                    Err(error) => {
                        tracing::warn!(error = %error, session_id = %session_id, "message replay query failed");
                        Vec::new()
                    }
                };

                // M004: the runtime `selected_model` cache is a
                // projection of the durable selection row. When the
                // durable row is `Selected`, its canonical
                // `provider/model` string wins over any stale cache and
                // the cache is repaired in place (restart
                // reconstruction). Unselected/legacy states keep the
                // existing cache (e.g. a turn-level override).
                let durable_canonical = if let Some(service) = self.selection_service.as_ref() {
                    match service.get(&session_id).await {
                        Ok(selection) => {
                            crate::core::session_selection::durable_selected_runtime_model(
                                &selection,
                            )
                        }
                        Err(error) => {
                            tracing::debug!(
                                session_id = %session_id,
                                error = %error,
                                "durable selection read failed during snapshot; using runtime cache"
                            );
                            None
                        }
                    }
                } else {
                    None
                };
                if let Some(canonical) = durable_canonical.clone() {
                    if let Some(runtime) = self.sessions.get(&session_id) {
                        let cached = runtime.selected_model.read().await.clone();
                        if cached.as_deref() != Some(canonical.as_str()) {
                            let mut selected = runtime.selected_model.write().await;
                            *selected = Some(canonical.clone());
                        }
                    }
                }
                let (
                    status,
                    selected_model,
                    selected_agent,
                    pending_permissions,
                    pending_questions,
                    input_tokens,
                    output_tokens,
                    active_subagents,
                    controller_principal,
                    controller_revision,
                ) = if let Some(runtime) = self.sessions.get(&session_id) {
                    let status = format!("{:?}", *runtime.status.read().await);
                    let cached = runtime.selected_model.read().await.clone();
                    // Durable `Selected` always wins over the cache.
                    let model = durable_canonical.or(cached);
                    let agent = runtime.selected_agent.read().await.clone();
                    let pending_permissions: Vec<String> = runtime
                        .pending_permissions
                        .iter()
                        .map(|r| r.key().clone())
                        .collect();
                    let pending_questions: Vec<String> = runtime
                        .pending_questions
                        .iter()
                        .map(|r| r.key().clone())
                        .collect();
                    let input_tokens = *runtime.last_input_tokens.read().await;
                    let output_tokens = *runtime.last_output_tokens.read().await;
                    let active_subagents = runtime
                        .active_subagent_count
                        .load(std::sync::atomic::Ordering::Relaxed);
                    // M004: safe controller projection (principal id plus
                    // coarse revision only; never credentials or device
                    // secrets). Prefers the durable lease; falls back to
                    // the in-memory handle for pool-less daemons.
                    let (controller_principal, controller_revision) =
                        if let Some(store) = self.controller_store() {
                            match store.get(&session_id).await {
                                Ok(Some(record)) => (
                                    Some(record.controller_principal.as_str().to_owned()),
                                    Some(record.revision),
                                ),
                                _ => (None, None),
                            }
                        } else {
                            let active = runtime.active_turn.read().await;
                            match active.as_ref() {
                                Some(handle) => (
                                    handle.controller_principal.clone(),
                                    Some(handle.controller_revision),
                                ),
                                None => (None, None),
                            }
                        };
                    (
                        status,
                        model,
                        agent,
                        pending_permissions,
                        pending_questions,
                        input_tokens,
                        output_tokens,
                        active_subagents,
                        controller_principal,
                        controller_revision,
                    )
                } else {
                    (
                        "idle".to_string(),
                        None,
                        None,
                        Vec::new(),
                        Vec::new(),
                        None,
                        None,
                        0,
                        None,
                        None,
                    )
                };

                let event_seq = self.event_log.current_seq();

                Ok(CoreResponse::SnapshotSession {
                    event_seq,
                    session: crate::protocol_conversions::session_to_dto(session).unwrap_or_else(
                        |e| {
                            tracing::error!(error = %e, "session_to_dto conversion failed");
                            Default::default()
                        },
                    ),
                    messages: crate::protocol_conversions::messages_to_dtos(messages)
                        .unwrap_or_else(|e| {
                            tracing::error!(error = %e, "messages_to_dtos conversion failed");
                            Default::default()
                        }),
                    status,
                    selected_model,
                    selected_agent,
                    pending_permissions,
                    pending_questions,
                    input_tokens,
                    output_tokens,
                    active_subagents,
                    controller_principal,
                    controller_revision,
                })
            }
            CoreRequest::SnapshotModels => {
                let config = super::load_config_or_default();
                let mut registry = crate::provider::ProviderRegistry::new();
                let locators = match self.pool.clone() {
                    Some(pool) => {
                        crate::core::session_selection::pool_connection_credential_locators(&pool)
                            .await
                    }
                    None => Vec::new(),
                };
                crate::provider::register_builtin_with_config_connections(
                    &mut registry,
                    &config,
                    &locators,
                    None,
                );
                let model_ids: Vec<String> = if let Some(pool) = self.pool.clone() {
                    let discovery = crate::provider::discovery::ModelDiscoveryService::new(
                        std::path::PathBuf::new(),
                    )
                    .with_pool(pool);
                    // Seed from the persisted cache, then hit the network only
                    // when the cache is missing or stale. This used to call
                    // `refresh()` unconditionally on a fresh service, so every
                    // `SnapshotModels` request re-queried all providers and
                    // returned `[]` whenever any of them was slow — the TUI
                    // picker then rendered permanently empty. Mirrors the
                    // standalone path in `src/main.rs`.
                    discovery.initialize().await;
                    if discovery.needs_refresh().await {
                        let models = discovery.refresh(&registry).await;
                        models
                            .iter()
                            .map(|m| format!("{}/{}", m.provider, m.id))
                            .collect()
                    } else {
                        discovery.get_model_ids().await
                    }
                } else {
                    let mut ids = Vec::new();
                    for provider in registry.list() {
                        if let Ok(models) = provider.models().await {
                            for m in models {
                                ids.push(format!("{}/{}", provider.id(), m.id));
                            }
                        }
                    }
                    ids
                };
                Ok(CoreResponse::ModelsSnapshot {
                    current_model: None,
                    models: model_ids,
                })
            }
            _ => {
                tracing::warn!("Unhandled CoreRequest variant");
                Ok(CoreResponse::Error {
                    code: "unimplemented".to_string(),
                    message: "This request type is not yet implemented".to_string(),
                })
            }
        }
    }

    /// M004 desktop session slice: daemon-side composition for
    /// `SessionPromptSubmit`.
    ///
    /// Resolves model/agents/messages the narrow desktop client must
    /// not assert itself:
    ///
    /// - model comes only from the durable session selection
    ///   (`Selected`); `Unselected`/`LegacyUnresolved`/lookup failure
    ///   fail closed so the daemon never invents provider identity;
    /// - agents come from daemon-owned configuration resolved against
    ///   the session-bound workspace root (same root the ACP
    ///   `session/prompt` path uses; never a renderer path);
    /// - messages carry the single user prompt (ACP precedent); the
    ///   desktop owns no history store to replay.
    ///
    /// At-most-once across reconnect derives from the shared
    /// `TurnSubmit` body: a still-active turn rejects the duplicate
    /// with `turn_already_active` instead of spawning a second turn.
    pub(crate) const SESSION_PROMPT_MAX_TEXT_CHARS: usize = 200_000;

    async fn stage_human_shell_promotion(
        &self,
        session_id: &str,
        promotion_id: &str,
        source_id: &str,
        projection: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
    ) -> CoreResponse {
        let Some(pool) = self.pool.clone() else {
            return promotion_error_response(
                "human_shell_promotion_unavailable",
                "durable session storage is required for promotion staging",
            );
        };
        if projection.len() > codegg_core::session::HUMAN_SHELL_PROMOTION_MAX_BYTES {
            return promotion_error_response(
                "human_shell_promotion_invalid",
                "projection exceeds the 32 KiB storage bound",
            );
        }
        let runtime = match self.bind_runtime_for_session(session_id).await {
            Ok(runtime) => runtime,
            Err(_) => {
                return promotion_error_response(
                    "human_shell_promotion_stale_binding",
                    "session has no current workspace binding",
                )
            }
        };
        let sanitized =
            crate::shell::sanitize_ansi(projection, crate::config::schema::AnsiMode::Strip);
        let redacted = crate::shell::redactor::Redactor::default().redact(&sanitized);
        if redacted.text.trim().is_empty()
            || redacted.text.len() > codegg_core::session::HUMAN_SHELL_PROMOTION_MAX_BYTES
        {
            return promotion_error_response(
                "human_shell_promotion_invalid",
                "redacted projection is empty or exceeds the 32 KiB storage bound",
            );
        }
        let input = codegg_core::session::NewHumanShellPromotion {
            id: promotion_id.to_owned(),
            source_id: source_id.to_owned(),
            session_id: session_id.to_owned(),
            principal_id: authority.principal_id().as_str().to_owned(),
            project_id: runtime.project_id.clone(),
            workspace_id: runtime.workspace_id.as_str().to_owned(),
            projection: redacted.text,
            created_at_ms: chrono::Utc::now().timestamp_millis(),
        };
        match codegg_core::session::HumanShellPromotionStore::new(pool)
            .stage(input)
            .await
        {
            Ok(codegg_core::session::StageHumanShellPromotionOutcome::Staged(record)) => {
                self.record_human_shell_promotion_stage(
                    authority,
                    authz_decision,
                    &record,
                    "staged",
                )
                .await;
                CoreResponse::HumanShellPromotionStaged {
                    promotion: promotion_record_to_dto(record),
                    duplicate: false,
                }
            }
            Ok(codegg_core::session::StageHumanShellPromotionOutcome::Duplicate(record)) => {
                CoreResponse::HumanShellPromotionStaged {
                    promotion: promotion_record_to_dto(record),
                    duplicate: true,
                }
            }
            Err(error) => promotion_error_response(error.code(), &error.to_string()),
        }
    }

    async fn record_human_shell_promotion_stage(
        &self,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        decision: &codegg_core::authorization::AuthorizationDecision,
        record: &codegg_core::session::HumanShellPromotionRecord,
        outcome: &str,
    ) {
        let provenance = codegg_core::authorization::audit_provenance(decision);
        let mut chain = codegg_core::audit_instrumentation::AuditChainContext::new();
        chain.project = decision.project_id.clone();
        chain.session_id = Some(record.session_id.clone());
        let builder = codegg_core::audit::AuditEventBuilder::new(
            codegg_core::audit::AuditAction::PromptSubmit,
            authority.principal(),
            &provenance,
        )
        .with_visibility(codegg_core::audit::AuditVisibility::Project)
        .with_metadata("human_shell_promotion.id", &record.id)
        .with_metadata("human_shell_promotion.outcome", outcome)
        .with_metadata(
            "human_shell_promotion.projection_bytes",
            record.projection_bytes.to_string().as_str(),
        )
        .with_metadata("decision.id", provenance.decision_id())
        .with_metadata("decision.outcome", "allow");
        let builder = codegg_core::audit_instrumentation::apply_chain(
            builder,
            &chain,
            chain.project.as_ref(),
        );
        self.append_audit_event(builder).await;
    }

    async fn get_human_shell_promotions(
        &self,
        session_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
    ) -> CoreResponse {
        let Some(pool) = self.pool.clone() else {
            return promotion_error_response(
                "human_shell_promotion_unavailable",
                "durable session storage is required for promotion recovery",
            );
        };
        let runtime = match self.bind_runtime_for_session(session_id).await {
            Ok(runtime) => runtime,
            Err(_) => {
                return promotion_error_response(
                    "human_shell_promotion_stale_binding",
                    "session has no current workspace binding",
                )
            }
        };
        match codegg_core::session::HumanShellPromotionStore::new(pool)
            .list_pending(
                session_id,
                authority.principal_id().as_str(),
                &runtime.project_id,
                runtime.workspace_id.as_str(),
                chrono::Utc::now().timestamp_millis(),
            )
            .await
        {
            Ok(records) => CoreResponse::HumanShellPromotions {
                promotions: records.into_iter().map(promotion_record_to_dto).collect(),
            },
            Err(error) => promotion_error_response(error.code(), &error.to_string()),
        }
    }

    /// M002a (ADR-0035): read the durable user prompt queue projection.
    ///
    /// Read-only. Authority is enforced by the `operation_descriptor` seam
    /// (`ScopeKind::ViaSession` + `Capability::SessionRead`) before this
    /// handler runs — there is no hand-rolled check here and deliberately no
    /// `is_controller_bypass` path: a read needs session read authority, not
    /// an identity bypass.
    ///
    /// The queue always reads empty until M002b lands, because nothing can
    /// enqueue an entry yet. That is expected, not a defect.
    async fn list_user_prompt_queue(&self, session_id: &str) -> CoreResponse {
        let Some(pool) = self.pool.clone() else {
            return promotion_error_response(
                "user_prompt_queue_unavailable",
                "durable session storage is required for the prompt queue",
            );
        };
        match codegg_core::session::UserPromptQueueStore::new(pool)
            .list_live(session_id, chrono::Utc::now().timestamp_millis())
            .await
        {
            Ok(entries) => CoreResponse::UserPromptQueue {
                session_id: session_id.to_owned(),
                entries: entries
                    .into_iter()
                    .map(user_prompt_queue_entry_to_dto)
                    .collect(),
            },
            // `UserPromptQueueError`'s Display never carries entry text: the
            // typed variants are about identity, bounds, and availability.
            Err(error) => promotion_error_response(error.code(), &error.to_string()),
        }
    }
}

/// One M002b queue mutation, after the capability gate has run.
///
/// The capability gate is necessary but not sufficient: `AgentInvoke` says the
/// caller may act on this project, not that this caller owns this session's
/// queue. The durable owner row answers the second question, and the store
/// re-reads it inside the mutation's own transaction, so authorization and
/// mutation are never separable by a crash.
#[derive(Debug)]
enum QueueMutation<'a> {
    Add {
        entry_id: &'a str,
        text: &'a str,
        /// M007 (ADR-0039): the requested due time, or `None` for an immediate
        /// entry. Carried through to the store, which owns the expiry check —
        /// the daemon does not second-guess it with a duplicate rule.
        run_at_ms: Option<i64>,
    },
    Edit {
        entry_id: &'a str,
        text: &'a str,
        /// M007: `None` clears an existing schedule.
        run_at_ms: Option<i64>,
    },
    Delete {
        entry_id: &'a str,
    },
    Reorder {
        entry_ids: &'a [String],
    },
    Adopt,
    Transfer {
        recipient: &'a str,
        expected_revision: u64,
    },
}

impl QueueMutation<'_> {
    /// Stable operation label for the audit record. It mirrors the protocol
    /// variant name, not the descriptor name, so one label is unambiguous.
    const fn operation(&self) -> &'static str {
        match self {
            Self::Add { .. } => "add",
            Self::Edit { .. } => "edit",
            Self::Delete { .. } => "delete",
            Self::Reorder { .. } => "reorder",
            Self::Adopt => "adopt",
            Self::Transfer { .. } => "transfer",
        }
    }

    /// The entry this mutation targets, when it targets exactly one.
    const fn entry_id(&self) -> Option<&str> {
        match self {
            Self::Add { entry_id, .. }
            | Self::Edit { entry_id, .. }
            | Self::Delete { entry_id } => Some(entry_id),
            Self::Reorder { .. } | Self::Adopt | Self::Transfer { .. } => None,
        }
    }
}

impl CoreDaemon {
    /// Execute one authorized queue mutation, then audit it.
    ///
    /// Order is deliberate: **authorize, mutate, audit.** A refusal returns
    /// before any write, so an unauthorized mutation leaves no partial state.
    async fn handle_user_prompt_queue_mutation(
        &self,
        session_id: String,
        mutation: QueueMutation<'_>,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
    ) -> CoreResponse {
        let Some(pool) = self.pool.clone() else {
            return promotion_error_response(
                "user_prompt_queue_unavailable",
                "durable session storage is required for the prompt queue",
            );
        };
        let store = codegg_core::session::UserPromptQueueStore::new(pool.clone());
        let caller = authority.principal_id().as_str();
        let now_ms = chrono::Utc::now().timestamp_millis();

        // Adoption answers a different question from the other five: it repairs
        // a queue that has no owner, so the queue-owner term is absent by
        // definition. Every other mutation requires ownership.
        let scope = match mutation {
            QueueMutation::Adopt => ControllerScope::QueueAdoption {
                session_id: &session_id,
            },
            _ => ControllerScope::QueueMutation {
                session_id: &session_id,
            },
        };
        if let Some(denial) = self
            .check_controller(scope, authority, authz_decision)
            .await
        {
            return denial;
        }

        // Resolved before the write so a refusal can name the entry it refused
        // to change, without ever naming the owner to a non-owner.
        let operation = mutation.operation();
        let entry_id = mutation.entry_id().map(str::to_owned);

        let result = match mutation {
            QueueMutation::Add {
                entry_id,
                text,
                run_at_ms,
            } => {
                let stored = store
                    .add(caller, &session_id, entry_id, text, run_at_ms, now_ms)
                    .await;
                let bytes = stored.as_ref().ok().map(|entry| entry.text_bytes);
                match self
                    .queue_projection(&store, &session_id, now_ms, stored.err())
                    .await
                {
                    Ok(response) => Ok((response, bytes)),
                    Err(error) => Err(error),
                }
            }
            QueueMutation::Edit {
                entry_id,
                text,
                run_at_ms,
            } => {
                let stored = store
                    .edit(caller, &session_id, entry_id, text, run_at_ms, now_ms)
                    .await;
                let bytes = stored.as_ref().ok().map(|entry| entry.text_bytes);
                match self
                    .queue_projection(&store, &session_id, now_ms, stored.err())
                    .await
                {
                    Ok(response) => Ok((response, bytes)),
                    Err(error) => Err(error),
                }
            }
            QueueMutation::Delete { entry_id } => {
                let stored = store.delete(caller, &session_id, entry_id, now_ms).await;
                self.queue_projection(&store, &session_id, now_ms, stored.err())
                    .await
                    .map(|response| (response, None))
            }
            QueueMutation::Reorder { entry_ids } => {
                let stored = store.reorder(caller, &session_id, entry_ids, now_ms).await;
                self.queue_projection(&store, &session_id, now_ms, stored.err())
                    .await
                    .map(|response| (response, None))
            }
            QueueMutation::Adopt => match store.adopt(caller, &session_id, now_ms).await {
                Ok(record) => Ok((
                    CoreResponse::UserPromptQueueOwner {
                        session_id: record.session_id,
                        owner_principal: record.owner_principal,
                        revision: record.revision,
                    },
                    None,
                )),
                Err(error) => Err(error),
            },
            QueueMutation::Transfer {
                recipient,
                expected_revision,
            } => {
                let Ok(recipient) = codegg_core::identity::PrincipalId::parse(recipient) else {
                    return promotion_error_response(
                        "user_prompt_queue_invalid",
                        "transfer recipient is not a valid principal",
                    );
                };
                // Mirrors `session_control_transfer`: the recipient must already
                // be eligible for in-flight control, so a transfer never mints
                // authority the recipient did not already hold.
                if !self
                    .principal_holds_invoke(
                        &pool,
                        authz_decision.project_id.as_ref(),
                        recipient.as_str(),
                    )
                    .await
                {
                    return promotion_error_response(
                        "user_prompt_queue_invalid",
                        "transfer recipient is not eligible to own this queue",
                    );
                }
                match store
                    .transfer(
                        caller,
                        recipient.as_str(),
                        expected_revision,
                        &session_id,
                        now_ms,
                    )
                    .await
                {
                    Ok(record) => Ok((
                        CoreResponse::UserPromptQueueOwner {
                            session_id: record.session_id,
                            owner_principal: record.owner_principal,
                            revision: record.revision,
                        },
                        None,
                    )),
                    Err(error) => Err(error),
                }
            }
        };

        match result {
            Ok((response, bytes)) => {
                self.audit_queue_mutation(
                    authority,
                    authz_decision,
                    &session_id,
                    operation,
                    entry_id.as_deref(),
                    bytes,
                    "applied",
                )
                .await;
                // M007 (ADR-0039): tell the advance task the schedule may have
                // moved. It is parked with no timer when nothing is scheduled,
                // so without this a newly scheduled entry would have nothing to
                // wake it.
                //
                // Deliberately unconditional over add and edit rather than
                // "only when `run_at_ms` is `Some`": clearing the last schedule
                // must also wake the task so it can go back to parking, and
                // tracking the prior value here would mean reading the row the
                // store has already replaced. The cost is one indexed `MIN()`
                // per queue mutation.
                if matches!(operation, "add" | "edit") {
                    self.queue_schedule_changed.notify_one();
                }
                response
            }
            Err(error) => {
                // The store refused. `authorize_request` already audited a
                // controller-level denial upstream; this records the store-level
                // refusal so a rejected mutation is not invisible.
                self.audit_queue_mutation(
                    authority,
                    authz_decision,
                    &session_id,
                    operation,
                    entry_id.as_deref(),
                    None,
                    "refused",
                )
                .await;
                queue_error_response(&error)
            }
        }
    }

    /// Read back the resulting projection for a mutation that already succeeded.
    ///
    /// `store_error` short-circuits the read when the mutation itself was
    /// refused, so a refusal never costs an extra query and never reports a
    /// projection that did not result from this call.
    async fn queue_projection(
        &self,
        store: &codegg_core::session::UserPromptQueueStore,
        session_id: &str,
        now_ms: i64,
        store_error: Option<codegg_core::session::UserPromptQueueError>,
    ) -> Result<CoreResponse, codegg_core::session::UserPromptQueueError> {
        if let Some(error) = store_error {
            return Err(error);
        }
        let entries = store.list_live(session_id, now_ms).await?;
        Ok(CoreResponse::UserPromptQueue {
            session_id: session_id.to_owned(),
            entries: entries
                .into_iter()
                .map(user_prompt_queue_entry_to_dto)
                .collect(),
        })
    }

    /// M006 (ADR-0036): hand one queued prompt to the turn that is running now.
    ///
    /// **Not a `QueueMutation`** — see the dispatch arm for why the durable owner
    /// binding does not gate this. The order below *is* the safety argument, and
    /// each step exists because the one after it cannot undo it:
    ///
    /// 1. **Pre-flight** the entry, so a prompt that could never be settled is
    ///    never steered into a turn in the first place. This is what stops a
    ///    `delivering` row from being handed to a turn when it is already
    ///    running as its own.
    /// 2. **Steer** the active turn, through the same controller predicate
    ///    `TurnSteer` uses and against the `turn_id` the daemon resolves itself,
    ///    so a client cannot steer a turn that has already ended.
    /// 3. **Settle** the row in the store, which re-checks queue ownership
    ///    inside its transaction and is guarded on `status = 'pending'`.
    ///
    /// Step 3 can still fail after step 2 succeeded: the turn holds the text and
    /// the row did not move. That is reported as `AcceptedNotSettled`, never as
    /// an error — an error here would tell the user their prompt was rejected
    /// and invite them to resend a prompt that has already run, which is
    /// exactly the duplication ADR-0036 forbids.
    async fn handle_user_prompt_queue_send_now(
        &self,
        session_id: &str,
        entry_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
    ) -> CoreResponse {
        let Some(pool) = self.pool.clone() else {
            return self
                .send_now_unavailable(authority, authz_decision, session_id, entry_id)
                .await;
        };
        let store = codegg_core::session::UserPromptQueueStore::new(pool.clone());
        let caller = authority.principal_id().as_str();
        let now_ms = chrono::Utc::now().timestamp_millis();

        let preflight = match store.get_live(entry_id, now_ms).await {
            Ok(record) if record.session_id == session_id => record,
            // `get_live` reads by identity alone, so a row in another session is
            // indistinguishable from a row that never existed. Reported as the
            // latter on purpose: telling a caller that an id exists in a session
            // it cannot see would leak that session's queue.
            Ok(_) | Err(codegg_core::session::UserPromptQueueError::NotFound) => {
                return self
                    .finish_send_now(
                        authority,
                        authz_decision,
                        &store,
                        session_id,
                        entry_id,
                        now_ms,
                        UserPromptQueueSendNowResult::EntryNotFound,
                    )
                    .await
            }
            Err(error) => {
                let code = error.code().to_owned();
                return self
                    .finish_send_now(
                        authority,
                        authz_decision,
                        &store,
                        session_id,
                        entry_id,
                        now_ms,
                        UserPromptQueueSendNowResult::Refused { code },
                    )
                    .await;
            }
        };

        // A `delivering` row is already running as its own turn, and any other
        // non-pending status is terminal. Both refuse before the steer, so the
        // only prompt this path can hand over is one still waiting its turn.
        if preflight.status != codegg_core::session::UserPromptQueueStatus::Pending {
            let code = match preflight.status {
                codegg_core::session::UserPromptQueueStatus::Delivering => {
                    "user_prompt_queue_entry_delivering"
                }
                _ => "user_prompt_queue_expired",
            };
            return self
                .finish_send_now(
                    authority,
                    authz_decision,
                    &store,
                    session_id,
                    entry_id,
                    now_ms,
                    UserPromptQueueSendNowResult::Refused {
                        code: code.to_owned(),
                    },
                )
                .await;
        }

        let turn_id = match self
            .steer_running_turn(session_id, preflight.text, authority, authz_decision)
            .await
        {
            Ok(turn_id) => turn_id,
            Err(code) => {
                return self
                    .finish_send_now(
                        authority,
                        authz_decision,
                        &store,
                        session_id,
                        entry_id,
                        now_ms,
                        UserPromptQueueSendNowResult::Refused { code },
                    )
                    .await
            }
        };

        // The turn has the prompt. From here the settlement cannot be allowed to
        // fail into silence, which is why this is a reported outcome rather than
        // a discarded error.
        match store
            .mark_steer_delivered(caller, session_id, entry_id, &turn_id, now_ms)
            .await
        {
            Ok(()) => {
                self.finish_send_now(
                    authority,
                    authz_decision,
                    &store,
                    session_id,
                    entry_id,
                    now_ms,
                    UserPromptQueueSendNowResult::Accepted {
                        turn_id: turn_id.clone(),
                    },
                )
                .await
            }
            Err(error) => {
                self.finish_send_now(
                    authority,
                    authz_decision,
                    &store,
                    session_id,
                    entry_id,
                    now_ms,
                    UserPromptQueueSendNowResult::AcceptedNotSettled {
                        turn_id,
                        code: error.code().to_owned(),
                    },
                )
                .await
            }
        }
    }

    /// Audit the attempt and attach the post-attempt projection.
    ///
    /// Every outcome routes through here so no path can return without leaving
    /// a record, and every outcome carries the projection — including the
    /// refusals, because a refusal may have been caused by a row another
    /// frontend already removed, and a box left showing it would be showing
    /// something the daemon no longer believes.
    #[allow(clippy::too_many_arguments)]
    async fn finish_send_now(
        &self,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
        store: &codegg_core::session::UserPromptQueueStore,
        session_id: &str,
        entry_id: &str,
        now_ms: i64,
        outcome: UserPromptQueueSendNowResult,
    ) -> CoreResponse {
        let audit_outcome = match &outcome {
            UserPromptQueueSendNowResult::Accepted { .. } => "applied",
            UserPromptQueueSendNowResult::AcceptedNotSettled { .. } => "accepted_unsettled",
            UserPromptQueueSendNowResult::Refused { .. }
            | UserPromptQueueSendNowResult::EntryNotFound => "refused",
        };
        self.audit_queue_mutation(
            authority,
            authz_decision,
            session_id,
            "send_now",
            Some(entry_id),
            // Deliberately absent: the audit records that a prompt was handed to
            // a turn, never what the prompt said. The byte count was recorded by
            // the `add` that stored it.
            None,
            audit_outcome,
        )
        .await;

        let entries = store
            .list_live(session_id, now_ms)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(user_prompt_queue_entry_to_dto)
            .collect();
        CoreResponse::UserPromptQueueSendNowOutcome {
            session_id: session_id.to_owned(),
            entry_id: entry_id.to_owned(),
            outcome,
            entries,
        }
    }

    /// Send-now with no durable store at all.
    ///
    /// The entry's text lives only in the queue, so with no queue there is
    /// nothing to steer and nothing honest to report as accepted. Audited like
    /// every other outcome — a refused attempt that left no record would make
    /// this path the one hole in the trail.
    async fn send_now_unavailable(
        &self,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
        session_id: &str,
        entry_id: &str,
    ) -> CoreResponse {
        self.audit_queue_mutation(
            authority,
            authz_decision,
            session_id,
            "send_now",
            Some(entry_id),
            None,
            "refused",
        )
        .await;
        CoreResponse::UserPromptQueueSendNowOutcome {
            session_id: session_id.to_owned(),
            entry_id: entry_id.to_owned(),
            outcome: UserPromptQueueSendNowResult::Refused {
                code: "user_prompt_queue_unavailable".to_owned(),
            },
            entries: Vec::new(),
        }
    }

    /// M006: put one prompt into the session's active turn.
    ///
    /// Resolves `turn_id` from the daemon's **own** runtime handle rather than
    /// from the caller, then applies the same controller predicate `TurnSteer`
    /// applies. Returns the turn that accepted, or a stable code naming the one
    /// condition that stopped it.
    ///
    /// The handle is read once, under a short lock, and the `(turn_id, sender)`
    /// pair is cloned out before any database work — holding a read lock across
    /// the controller check would serialise every turn's steering behind one
    /// storage round trip. A sender cloned from a handle whose turn has since
    /// ended is not a hazard: its receiver is dropped with the turn, so
    /// `try_send` reports `Closed` and the prompt stays queued rather than
    /// reaching a turn that did not ask for it.
    async fn steer_running_turn(
        &self,
        session_id: &str,
        text: String,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
    ) -> Result<String, String> {
        let Some(runtime) = self.sessions.get(session_id) else {
            return Err("session_not_found".to_owned());
        };
        let resolved = {
            let active = runtime.active_turn.read().await;
            active.as_ref().map(|handle| {
                (
                    handle.turn_id.clone(),
                    handle
                        .steer_tx
                        .clone()
                        .ok_or_else(|| "steer_not_supported".to_owned()),
                )
            })
        };
        let Some((turn_id, sender)) = resolved else {
            return Err("no_active_turn".to_owned());
        };
        let sender = sender?;

        if let Some(denial) = self
            .check_turn_controller(session_id, &turn_id, authority, authz_decision)
            .await
        {
            return Err(controller_denial_code(&denial));
        }

        match sender.try_send(text) {
            Ok(()) => Ok(turn_id),
            Err(error) => Err(steer_channel_code(&error).to_owned()),
        }
    }

    /// Audit one queue mutation with entry and owner identity, never text.
    ///
    /// Uses the `prompt_submit` action ADR-0037 selected, with queue-shaped
    /// metadata. The owner is recorded only on an applied action by its own
    /// actor; a refusal never carries an owner identity, because telling a
    /// refused caller who owns the queue would itself be the leak.
    async fn audit_queue_mutation(
        &self,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
        session_id: &str,
        operation: &'static str,
        entry_id: Option<&str>,
        entry_bytes: Option<usize>,
        outcome: &str,
    ) {
        use codegg_core::audit_instrumentation as instr;
        let provenance = codegg_core::authorization::audit_provenance(authz_decision);
        let mut chain = instr::AuditChainContext::new();
        chain.project = authz_decision.project_id.clone();
        chain.session_id = Some(session_id.to_owned());
        // Only the actor's own identity, never a third party's owner row.
        let owner = (outcome == "applied").then(|| authority.principal_id().as_str());
        let builder = instr::user_prompt_queue_mutation_event(
            authority.principal(),
            &provenance,
            &chain,
            session_id,
            codegg_core::audit_instrumentation::UserPromptQueueAudit {
                operation,
                entry_id,
                owner_principal: owner,
                entry_bytes,
                outcome,
            },
        );
        self.append_audit_event(builder).await;
    }

    pub(crate) async fn resolve_prompt_submit_composition(
        &self,
        session_id: &str,
        text: &str,
    ) -> Result<
        (
            String,
            Vec<crate::protocol::dto::Agent>,
            Vec<crate::protocol::dto::ProviderMessage>,
        ),
        PromptCompositionError,
    > {
        if text.trim().is_empty() {
            return Err(PromptCompositionError::new(
                "prompt_text_empty",
                "Prompt text must not be empty",
            ));
        }
        if text.chars().count() > Self::SESSION_PROMPT_MAX_TEXT_CHARS {
            return Err(PromptCompositionError::new(
                "prompt_text_too_long",
                format!(
                    "Prompt text exceeds {} characters",
                    Self::SESSION_PROMPT_MAX_TEXT_CHARS
                ),
            ));
        }
        let selection_service = self.selection_service.as_ref().ok_or_else(|| {
            PromptCompositionError::new(
                "model_unselected",
                format!(
                "No model selected for session {session_id}; choose one via SessionSelectionUpdate"
                ),
            )
        })?;
        let selection = selection_service.get(session_id).await.map_err(|error| {
            PromptCompositionError::new(
                "selection_lookup_failed",
                format!("Session selection lookup failed: {error}"),
            )
        })?;
        let model = match &selection {
            crate::protocol::provider::SessionSelectionDto::Selected { .. } => {
                crate::core::session_selection::durable_selected_runtime_model(&selection)
            }
            crate::protocol::provider::SessionSelectionDto::LegacyUnresolved { reason, .. } => {
                return Err(PromptCompositionError::new(
                    "model_unresolved",
                    format!(
                        "Session {session_id} carries a legacy model reference that cannot be resolved: {reason}"
                    ),
                ));
            }
            crate::protocol::provider::SessionSelectionDto::Unselected {} => None,
        };
        let Some(model) = model else {
            return Err(PromptCompositionError::new(
                "model_unselected",
                format!(
                    "No model selected for session {session_id}; choose one via SessionSelectionUpdate"
                ),
            ));
        };
        // Bind the session to learn its authoritative workspace root
        // for agent resolution. Unknown sessions fail here with
        // `session_unbound` before any composition occurs.
        let runtime = self
            .bind_runtime_for_session(session_id)
            .await
            .map_err(|error| {
                PromptCompositionError::new(
                    "session_unbound",
                    format!("session {session_id} has no resolvable workspace: {error}"),
                )
            })?;
        let config = super::load_config_or_default();
        let agents = crate::agent::resolve_agents_with_context(
            &config,
            Some(runtime.workspace_root.as_path()),
        )
        .map_err(|error| {
            PromptCompositionError::new(
                "agents_unresolvable",
                format!("Agent configuration cannot be resolved: {error}"),
            )
        })?;
        if agents.is_empty() {
            return Err(PromptCompositionError::new(
                "agents_unresolvable",
                "No agents available from daemon configuration",
            ));
        }
        let agents = crate::protocol_conversions::agents_to_dtos(agents).map_err(|error| {
            PromptCompositionError::new(
                "agents_invalid",
                format!("Resolved agents cannot be submitted: {error}"),
            )
        })?;
        let messages = vec![crate::protocol::dto::ProviderMessage::User {
            content: vec![crate::protocol::dto::ContentPart::Text {
                text: text.to_owned(),
            }],
        }];
        Ok((model, agents, messages))
    }
}

fn promotion_error_response(code: &str, message: &str) -> CoreResponse {
    CoreResponse::Error {
        code: code.to_owned(),
        message: message.to_owned(),
    }
}

/// The three conditions a steering channel can present, as stable codes.
///
/// Shared by `TurnSteer` and M006's send-now path so the two cannot disagree
/// about what a full or closed channel means — both push into the same channel
/// on the same turn, and a divergence here would show as one user getting an
/// inexplicable refusal where the other gets a clear one.
fn steer_channel_code(error: &tokio::sync::mpsc::error::TrySendError<String>) -> &'static str {
    match error {
        tokio::sync::mpsc::error::TrySendError::Closed(_) => "steer_unavailable",
        tokio::sync::mpsc::error::TrySendError::Full(_) => "steer_queue_full",
    }
}

/// Reduce a controller refusal to its stable code.
///
/// A denial is a `CoreResponse::Error` already carrying the code a client
/// depends on. Send-now reports the code rather than the whole response so its
/// frontend owns the wording, and so it cannot inherit prose that interpolates
/// ids a queue box has no reason to parse. A denial of any other shape cannot
/// occur — the controller check returns exactly this — but it degrades to a
/// named refusal rather than to an empty string a client would present as a
/// blank reason.
fn controller_denial_code(denial: &CoreResponse) -> String {
    match denial {
        CoreResponse::Error { code, .. } => code.clone(),
        _ => "session_control_not_controller".to_owned(),
    }
}

fn promotion_record_to_dto(
    record: codegg_core::session::HumanShellPromotionRecord,
) -> crate::protocol::core::HumanShellPromotionDto {
    crate::protocol::core::HumanShellPromotionDto {
        id: record.id,
        source_id: record.source_id,
        session_id: record.session_id,
        projection: record.projection,
        projection_bytes: record.projection_bytes,
        status: record.status.as_str().to_owned(),
        created_at_ms: record.created_at_ms,
        expires_at_ms: record.expires_at_ms,
        consumed_turn_id: record.consumed_turn_id,
    }
}

/// M003: project one live queue entry.
///
/// **The status is the entry's own, not a constant.** This mapper used to hardcode
/// `"pending"` on the reasoning that every row in `list_live` was pending by
/// construction — true until M003 gave `delivering` a producer. Once an entry
/// starts delivering it still appears in the projection (so a client can see an
/// entry it must not edit), and a hardcoded `pending` would tell the client it
/// may edit it.
pub(crate) fn user_prompt_queue_entry_to_dto(
    entry: codegg_core::session::user_prompt_queue::UserPromptQueueLiveEntry,
) -> crate::protocol::core::UserPromptQueueEntryDto {
    crate::protocol::core::UserPromptQueueEntryDto {
        id: entry.id,
        ordinal: entry.ordinal,
        text: entry.text,
        text_bytes: entry.text_bytes,
        status: entry.status.as_str().to_owned(),
        created_at_ms: entry.created_at_ms,
        expires_at_ms: entry.expires_at_ms,
        run_at_ms: entry.run_at_ms,
    }
}

#[cfg(test)]
mod tests {
    //! Turn-path provider resolution and durable-model validation.
    //!
    //! These are in-crate tests because both entry points
    //! (`classify_durable_model_selection`, `resolve_turn_provider`) are
    //! `pub` crate-module seams that a `tests/` integration binary cannot
    //! observe through the daemon's private plumbing.

    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use codegg_core::identity::{PrincipalId, ProviderConnectionId};
    use codegg_core::provider_connections::{
        Endpoint, NewProviderConnection, ProviderConnectionStore, ProviderKind, ProviderScope,
        SecretBindingLocator, SecretRef, TlsPolicy,
    };
    use codegg_core::session::{SessionStore, UpdateSession};
    use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope};

    use crate::core::daemon::CoreDaemon;
    use crate::core::runtime_deps::CoreRuntimeDeps;
    use crate::core::session_selection::{
        classify_durable_model_selection, resolve_turn_provider, DurableModelSelection,
    };

    /// Records that the turn actually reached the injected runtime, so a
    /// test can tell "accepted" from "rejected early" without a provider
    /// call ever leaving the process.
    struct RecordingTurnRuntime {
        called: AtomicBool,
        repository_id: tokio::sync::Mutex<Option<codegg_core::identity::RepositoryId>>,
    }

    #[async_trait::async_trait]
    impl crate::agent::turn_runtime::TurnRuntime for RecordingTurnRuntime {
        async fn run_turn(
            &self,
            input: crate::agent::turn_runtime::TurnRunInput,
        ) -> Result<crate::agent::turn_runtime::TurnRunOutput, crate::error::AppError> {
            self.called.store(true, Ordering::SeqCst);
            *self.repository_id.lock().await = input.repository_id;
            let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(false);
            let (steer_tx, _steer_rx) = tokio::sync::mpsc::channel(32);
            Ok(crate::agent::turn_runtime::TurnRunOutput {
                cancel_tx,
                steer_tx,
            })
        }
    }

    async fn pool() -> sqlx::SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let url = format!(
            "file:turns_test_{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4().simple()
        );
        let opts = SqliteConnectOptions::from_str(&url)
            .expect("valid sqlite options")
            .create_if_missing(true)
            .busy_timeout(std::time::Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .expect("connect in-memory sqlite");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        pool
    }

    async fn daemon(pool: sqlx::SqlitePool) -> (CoreDaemon, Arc<RecordingTurnRuntime>) {
        let runtime = Arc::new(RecordingTurnRuntime {
            called: AtomicBool::new(false),
            repository_id: tokio::sync::Mutex::new(None),
        });
        let deps = CoreRuntimeDeps::new(Some(pool), None, None)
            .with_turn_runtime(
                Arc::clone(&runtime) as Arc<dyn crate::agent::turn_runtime::TurnRuntime>
            );
        let daemon = CoreDaemon::with_deps(deps);
        daemon.hydrate_workspace_registry().await.expect("hydrate");
        (daemon, runtime)
    }

    /// Create a session bound to a real workspace root, the same way the
    /// existing daemon turn tests do, and return its id.
    async fn create_session(daemon: &CoreDaemon) -> String {
        let workspace_dir = tempfile::tempdir().expect("workspace temp dir");
        let workspace = daemon
            .workspaces
            .get_or_register(workspace_dir.path())
            .await
            .expect("register workspace");
        let project = codegg_core::project_catalog::ProjectCatalog::new(
            daemon.pool.clone().expect("daemon pool"),
        )
        .register_local_project(
            codegg_core::project_catalog::RegisterLocalProject {
                display_name: "Turns test project".to_string(),
                description: None,
                tags: Vec::new(),
                primary_repository_id: None,
            },
            &workspace.id,
            "turns-test",
        )
        .await
        .expect("register project");
        // Keep the temp dir alive for the lifetime of the session row.
        let path = workspace_dir.path().to_path_buf();
        std::mem::forget(workspace_dir);
        let request = crate::core::new_request(
            "req-create".into(),
            CoreRequest::SessionCreate {
                directory: path.to_string_lossy().into_owned(),
                title: None,
                project_id: Some(project.project_id.as_str().to_string()),
                workspace_id: Some(workspace.id.as_str().to_string()),
            },
        );
        match daemon
            .handle_request(request)
            .await
            .expect("session create")
        {
            CoreResponse::Session { session } => session.id,
            other => panic!("expected Session, got {other:?}"),
        }
    }

    async fn create_repository_session(daemon: &CoreDaemon) -> (String, tempfile::TempDir) {
        use std::process::Command;

        let workspace_dir = tempfile::tempdir().expect("workspace temp dir");
        // execution-ownership: test_only
        Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(workspace_dir.path())
            .status()
            .expect("git init");
        // execution-ownership: test_only
        Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/example/turns-test.git",
            ])
            .current_dir(workspace_dir.path())
            .status()
            .expect("git remote add");
        let workspace = daemon
            .workspaces
            .get_or_register(workspace_dir.path())
            .await
            .expect("register repository workspace");
        let project = codegg_core::project_catalog::ProjectCatalog::new(
            daemon.pool.clone().expect("daemon pool"),
        )
        .register_local_project(
            codegg_core::project_catalog::RegisterLocalProject {
                display_name: "Turns repository test project".to_string(),
                description: None,
                tags: Vec::new(),
                primary_repository_id: None,
            },
            &workspace.id,
            "turns-test",
        )
        .await
        .expect("register repository project");
        let path = workspace_dir.path().to_path_buf();
        let session_id = match daemon
            .handle_request(crate::core::new_request(
                "req-create-repository-session".into(),
                CoreRequest::SessionCreate {
                    directory: path.to_string_lossy().into_owned(),
                    title: None,
                    project_id: Some(project.project_id.as_str().to_string()),
                    workspace_id: Some(workspace.id.as_str().to_string()),
                },
            ))
            .await
            .expect("create repository session")
        {
            CoreResponse::Session { session } => session.id,
            other => panic!("expected Session, got {other:?}"),
        };
        (session_id, workspace_dir)
    }

    async fn seed_connection(pool: &sqlx::SqlitePool, kind: ProviderKind) -> ProviderConnectionId {
        let store = ProviderConnectionStore::new(pool.clone());
        let account = "turns-account";
        store
            .create(NewProviderConnection {
                provider_kind: kind,
                display_name: "Turns test connection".to_string(),
                endpoint: Endpoint::new("https://example.test/v1", TlsPolicy::Required)
                    .expect("endpoint"),
                tls_policy: TlsPolicy::Required,
                scope: ProviderScope::personal(
                    PrincipalId::parse("turns-user").expect("principal"),
                ),
                secret_binding: Some(
                    SecretBindingLocator::new(SecretRef::new(), "turns-test", account)
                        .expect("secret binding"),
                ),
            })
            .await
            .expect("create connection")
            .id
    }

    /// Persist the bounded catalog rows a successful probe would write, at
    /// the connection's current revision.
    async fn seed_catalog(
        pool: &sqlx::SqlitePool,
        connection_id: &ProviderConnectionId,
        models: &[&str],
    ) {
        for model_id in models {
            sqlx::query(
                "INSERT INTO provider_connection_models \
                 (connection_id, revision, model_id, model_name, context_window, \
                  max_output_tokens, supports_tools, supports_vision) \
                 VALUES (?, 1, ?, ?, 128000, 16384, 1, 1)",
            )
            .bind(connection_id.as_str())
            .bind(model_id)
            .bind(*model_id)
            .execute(pool)
            .await
            .expect("seed model row");
        }
    }

    /// Write a durable selection straight onto the session row. This is how
    /// a *stale* selection — one whose model left the catalog — exists in
    /// production: `update_selection` refuses to write one.
    async fn write_durable_selection(
        pool: &sqlx::SqlitePool,
        session_id: &str,
        connection_id: &ProviderConnectionId,
        revision: u64,
        model_id: &str,
    ) {
        SessionStore::new(pool.clone())
            .update(
                session_id,
                UpdateSession {
                    provider_connection_id: Some(Some(connection_id.as_str().to_string())),
                    provider_connection_revision: Some(Some(revision)),
                    model_catalog_revision: Some(Some("cat-1".to_string())),
                    selected_model_id: Some(Some(model_id.to_string())),
                    ..UpdateSession::default()
                },
            )
            .await
            .expect("write durable selection");
    }

    fn turn_request(session_id: &str, model: &str) -> RequestEnvelope<CoreRequest> {
        let agent = crate::agent::Agent {
            git_read_only: false,
            name: "test".into(),
            description: "test agent".into(),
            ..Default::default()
        };
        crate::core::new_request(
            "req-submit".into(),
            CoreRequest::TurnSubmit {
                session_id: session_id.to_string(),
                text: "hello".into(),
                plan_mode: false,
                human_shell_promotion_ids: vec![],
                model: model.to_string(),
                agents: vec![crate::protocol_conversions::agent_to_dto(agent).expect("agent dto")],
                current_agent_idx: 0,
                messages: vec![],
                reasoning: None,
            },
        )
    }

    fn error_of(response: &CoreResponse) -> (&str, &str) {
        match response {
            CoreResponse::Error { code, message } => (code.as_str(), message.as_str()),
            other => panic!("expected CoreResponse::Error, got {other:?}"),
        }
    }

    /// Drive one concrete model string for a session whose durable selection
    /// is pinned to an `other:opencode_go` connection, and report the
    /// daemon's answer. Each call owns its own daemon and session: a session
    /// admits one turn at a time, so two submits would collide on the turn
    /// controller.
    async fn submit_with_scoped_opencode_go(model: &str) -> (CoreResponse, bool) {
        let pool = pool().await;
        let (daemon, runtime) = daemon(pool.clone()).await;
        let session_id = create_session(&daemon).await;
        let connection_id =
            seed_connection(&pool, ProviderKind::Other("opencode_go".to_string())).await;
        seed_catalog(&pool, &connection_id, &["minimax-m3"]).await;
        let connection = ProviderConnectionStore::new(pool.clone())
            .get(&connection_id)
            .await
            .expect("get connection")
            .expect("connection row");
        write_durable_selection(
            &pool,
            &session_id,
            &connection_id,
            connection.revision,
            "minimax-m3",
        )
        .await;
        let response = daemon
            .handle_request(turn_request(&session_id, model))
            .await
            .expect("turn submit");
        (response, runtime.called.load(Ordering::SeqCst))
    }

    /// `opencode_go` is stored as `other:opencode_go`, the storage-key form
    /// of `ProviderKind::Other`. A durable model id therefore carries a
    /// connection-scoped kind, and the daemon must resolve it to the
    /// registry provider `opencode_go` instead of rejecting a perfectly
    /// valid model as `provider_not_found`.
    #[tokio::test(flavor = "current_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn connection_scoped_provider_kind_resolves_to_the_registry_provider() {
        let _env_guard = crate::auth::test_support::lock_env();
        let previous = std::env::var("OPENCODE_GO_API_KEY").ok();
        std::env::set_var("OPENCODE_GO_API_KEY", "test-key-not-used");

        let pool = pool().await;
        let connection_store = ProviderConnectionStore::new(pool.clone());
        seed_connection(&pool, ProviderKind::Other("opencode_go".to_string())).await;
        assert_eq!(
            resolve_turn_provider(&connection_store, "opencode_go/minimax-m3")
                .await
                .expect("durable model resolves"),
            "opencode_go",
        );
        assert_eq!(
            resolve_turn_provider(&connection_store, "other:opencode_go/minimax-m3")
                .await
                .expect("storage-key form resolves"),
            "opencode_go",
        );

        // The durable projection spells the provider with the bare registry
        // id, but a connection-scoped kind also legitimately arrives in its
        // storage-key form. Neither may read as `provider_not_found`, and a
        // naive `split('/').next()` gets the storage-key form wrong because
        // `other:opencode_go` is not a registry id.
        for model in ["opencode_go/minimax-m3", "other:opencode_go/minimax-m3"] {
            let (response, called) = submit_with_scoped_opencode_go(model).await;
            assert!(
                matches!(
                    &response,
                    CoreResponse::TurnAdmissionReceipt { receipt }
                        if receipt.status == "accepted"
                ),
                "a connection-scoped kind ({model}) must not be reported as \
                 provider_not_found: {response:?}",
            );
            assert!(
                called,
                "the turn must reach the runtime for {model} once the provider resolves",
            );
        }

        if let Some(value) = previous {
            std::env::set_var("OPENCODE_GO_API_KEY", value);
        } else {
            std::env::remove_var("OPENCODE_GO_API_KEY");
        }
    }

    #[tokio::test(flavor = "current_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn repository_binding_reaches_turn_runtime_for_isolated_delegation() {
        let _env_guard = crate::auth::test_support::lock_env();
        let previous = std::env::var("OPENCODE_GO_API_KEY").ok();
        std::env::set_var("OPENCODE_GO_API_KEY", "test-key-not-used");

        let pool = pool().await;
        let (daemon, runtime) = daemon(pool.clone()).await;
        let (session_id, _workspace_dir) = create_repository_session(&daemon).await;
        let session = SessionStore::new(pool.clone())
            .get(&session_id)
            .await
            .expect("load repository session")
            .expect("repository session exists");
        let workspace_id = codegg_core::identity::WorkspaceId::parse(
            session
                .workspace_id
                .as_deref()
                .expect("session workspace binding"),
        )
        .expect("workspace id parses");
        let binding = codegg_core::project_storage::ProjectStorage::new(pool.clone())
            .workspace_binding(&workspace_id)
            .await
            .expect("load workspace binding")
            .expect("workspace binding exists");
        assert_eq!(
            binding.status,
            codegg_core::project_storage::BindingStatus::Resolved
        );
        let expected_repository = binding
            .repository_id
            .clone()
            .expect("repository lineage was resolved");

        let connection_id =
            seed_connection(&pool, ProviderKind::Other("opencode_go".to_string())).await;
        seed_catalog(&pool, &connection_id, &["minimax-m3"]).await;
        let connection = ProviderConnectionStore::new(pool.clone())
            .get(&connection_id)
            .await
            .expect("get connection")
            .expect("connection row");
        write_durable_selection(
            &pool,
            &session_id,
            &connection_id,
            connection.revision,
            "minimax-m3",
        )
        .await;

        let response = daemon
            .handle_request(turn_request(&session_id, "opencode_go/minimax-m3"))
            .await
            .expect("submit repository-backed turn");
        assert!(
            matches!(
                response,
                CoreResponse::TurnAdmissionReceipt { ref receipt } if receipt.status == "accepted"
            ),
            "repository-backed turn should be accepted: {response:?}"
        );
        assert!(runtime.called.load(Ordering::SeqCst));
        assert_eq!(
            *runtime.repository_id.lock().await,
            Some(expected_repository),
            "the resolved workspace repository identity must reach TaskTool"
        );

        if let Some(value) = previous {
            std::env::set_var("OPENCODE_GO_API_KEY", value);
        } else {
            std::env::remove_var("OPENCODE_GO_API_KEY");
        }
    }

    /// A provider name that matches no connection is its own diagnostic.
    /// `provider_not_found` means "no registered implementation", which is
    /// a different user-facing failure, so the two must not collapse.
    #[tokio::test(flavor = "current_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn unknown_provider_gets_its_own_code_not_provider_not_found() {
        let _env_guard = crate::auth::test_support::lock_env();
        let pool = pool().await;
        let (daemon, runtime) = daemon(pool.clone()).await;
        let session_id = create_session(&daemon).await;
        let connection_id = seed_connection(&pool, ProviderKind::OpenAi).await;
        seed_catalog(&pool, &connection_id, &["gpt-4o"]).await;
        let connection = ProviderConnectionStore::new(pool.clone())
            .get(&connection_id)
            .await
            .expect("get connection")
            .expect("connection row");
        write_durable_selection(
            &pool,
            &session_id,
            &connection_id,
            connection.revision,
            "gpt-4o",
        )
        .await;

        let response = daemon
            .handle_request(turn_request(&session_id, "no_such_provider/some-model"))
            .await
            .expect("turn submit");
        let (code, message) = error_of(&response);
        assert_eq!(
            code, "unknown_provider",
            "an unresolvable provider must not be reported as provider_not_found ({message})",
        );
        assert!(
            !runtime.called.load(Ordering::SeqCst),
            "an unresolvable provider must be rejected before the turn runs",
        );
    }

    /// A durable selection whose model left the catalog must surface as
    /// `unknown_model`. Previously `get_selection` synthesised a fully
    /// populated `Selected` DTO for it (`context_window: 0`), the turn
    /// accepted it, and the provider 404'd.
    #[tokio::test(flavor = "current_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn stale_durable_model_is_rejected_as_unknown_model() {
        let _env_guard = crate::auth::test_support::lock_env();
        let previous = std::env::var("OPENAI_API_KEY").ok();
        std::env::set_var("OPENAI_API_KEY", "test-key-not-used");

        let pool = pool().await;
        let (daemon, runtime) = daemon(pool.clone()).await;
        let session_id = create_session(&daemon).await;
        let connection_id = seed_connection(&pool, ProviderKind::OpenAi).await;
        seed_catalog(&pool, &connection_id, &["gpt-4o"]).await;
        let connection = ProviderConnectionStore::new(pool.clone())
            .get(&connection_id)
            .await
            .expect("get connection")
            .expect("connection row");
        write_durable_selection(
            &pool,
            &session_id,
            &connection_id,
            connection.revision,
            "retired-model",
        )
        .await;

        // The projection still reports a synthesised zero-capability model
        // row; the turn path must not trust it.
        let selection = daemon
            .selection_service
            .as_ref()
            .expect("selection service")
            .get(&session_id)
            .await
            .expect("selection lookup");
        if let codegg_protocol::provider::SessionSelectionDto::Selected { model, .. } = &selection {
            assert_eq!(
                model.context_window, 0,
                "precondition: the stale selection projects a synthesised row",
            );
        } else {
            panic!("expected a synthesised Selected DTO, got {selection:?}");
        }

        let response = daemon
            .handle_request(turn_request(&session_id, "openai/retired-model"))
            .await
            .expect("turn submit");
        let (code, message) = error_of(&response);
        assert_eq!(
            code, "unknown_model",
            "a model absent from the connection catalog must surface, got: {message}",
        );
        assert!(
            message.contains("retired-model") && message.contains(connection_id.as_str()),
            "the diagnostic must name the gone model and its connection: {message}",
        );
        assert!(
            !runtime.called.load(Ordering::SeqCst),
            "an unavailable model must never be replaced by another one",
        );

        if let Some(value) = previous {
            std::env::set_var("OPENAI_API_KEY", value);
        } else {
            std::env::remove_var("OPENAI_API_KEY");
        }
    }

    /// The counterpart guard: a durable selection whose model *is* in the
    /// catalog is unchanged. This is what stops the previous fix from
    /// becoming an over-rejection.
    #[tokio::test(flavor = "current_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn live_durable_selection_is_unchanged() {
        let _env_guard = crate::auth::test_support::lock_env();
        let previous = std::env::var("OPENAI_API_KEY").ok();
        std::env::set_var("OPENAI_API_KEY", "test-key-not-used");

        let pool = pool().await;
        let (daemon, runtime) = daemon(pool.clone()).await;
        let session_id = create_session(&daemon).await;
        let connection_id = seed_connection(&pool, ProviderKind::OpenAi).await;
        seed_catalog(&pool, &connection_id, &["gpt-4o"]).await;
        let connection = ProviderConnectionStore::new(pool.clone())
            .get(&connection_id)
            .await
            .expect("get connection")
            .expect("connection row");
        write_durable_selection(
            &pool,
            &session_id,
            &connection_id,
            connection.revision,
            "gpt-4o",
        )
        .await;

        let classified = classify_durable_model_selection(
            daemon
                .selection_service
                .as_ref()
                .expect("service")
                .session_store
                .as_ref(),
            daemon
                .selection_service
                .as_ref()
                .expect("service")
                .connection_store
                .as_ref(),
            &session_id,
        )
        .await
        .expect("classify");
        assert_eq!(
            classified,
            DurableModelSelection::Live {
                connection_id: connection_id.as_str().to_string(),
                model_id: "gpt-4o".to_string(),
            },
        );

        let response = daemon
            .handle_request(turn_request(&session_id, "openai/gpt-4o"))
            .await
            .expect("turn submit");
        assert!(
            matches!(
                &response,
                CoreResponse::TurnAdmissionReceipt { receipt }
                    if receipt.status == "accepted"
            ),
            "a live durable selection must be accepted: {response:?}",
        );
        assert!(
            runtime.called.load(Ordering::SeqCst),
            "the turn must run for a live selection",
        );

        if let Some(value) = previous {
            std::env::set_var("OPENAI_API_KEY", value);
        } else {
            std::env::remove_var("OPENAI_API_KEY");
        }
    }

    /// A *missing connection* is not a missing model. The two failures need
    /// different diagnostics, so the classification must keep them apart.
    #[tokio::test(flavor = "current_thread")]
    async fn missing_connection_is_not_reported_as_an_unknown_model() {
        let pool = pool().await;
        let (daemon, _runtime) = daemon(pool.clone()).await;
        let session_id = create_session(&daemon).await;
        let connection_id = seed_connection(&pool, ProviderKind::OpenAi).await;
        write_durable_selection(&pool, &session_id, &connection_id, 1, "gpt-4o").await;

        sqlx::query("DELETE FROM provider_connections WHERE id = ?")
            .bind(connection_id.as_str())
            .execute(&pool)
            .await
            .expect("delete connection");

        let classified = classify_durable_model_selection(
            daemon
                .selection_service
                .as_ref()
                .expect("service")
                .session_store
                .as_ref(),
            daemon
                .selection_service
                .as_ref()
                .expect("service")
                .connection_store
                .as_ref(),
            &session_id,
        )
        .await
        .expect("classify");
        assert_eq!(
            classified,
            DurableModelSelection::ConnectionMissing {
                connection_id: connection_id.as_str().to_string(),
            },
        );
    }
}
