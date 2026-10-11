//! M002: `turns` request family for `CoreDaemon`.
//!
//! Turn submit/cancel/steer, agent/model selection writes, permission/question responses, and transport lifecycle.
//! Operates on the same daemon-owned state as the thin dispatcher;
//! introduces no new store, scheduler, state machine, or authority.

use std::sync::Arc;

use crate::error::AppError;
use crate::protocol::core::{CoreRequest, CoreResponse};

use super::daemon::CoreDaemon;
use super::event_log::EventFilter;

/// Narrow domain error for daemon-side prompt composition. Protocol
/// response construction belongs to the `SessionPromptSubmit` boundary.
#[derive(Debug)]
struct PromptCompositionError {
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

#[derive(Debug, Clone)]
struct StagedHumanShellPromotion {
    outcome: crate::protocol::human_shell::HumanShellPromotionStageOutcomeDto,
    promotion: crate::protocol::human_shell::HumanShellPromotionDto,
}

fn human_shell_store_error(error: codegg_core::session::HumanShellPromotionError) -> CoreResponse {
    let code = match error {
        codegg_core::session::HumanShellPromotionError::InvalidInput => {
            "human_shell_promotion_invalid"
        }
        codegg_core::session::HumanShellPromotionError::IdentityConflict => {
            "human_shell_promotion_identity_conflict"
        }
        codegg_core::session::HumanShellPromotionError::Capacity => {
            "human_shell_promotion_capacity"
        }
        codegg_core::session::HumanShellPromotionError::Unavailable => {
            "human_shell_promotion_unavailable"
        }
        codegg_core::session::HumanShellPromotionError::StaleBinding => {
            "human_shell_promotion_stale_binding"
        }
        codegg_core::session::HumanShellPromotionError::CorruptState
        | codegg_core::session::HumanShellPromotionError::Storage(_) => {
            "human_shell_promotion_storage_error"
        }
    };
    CoreResponse::Error {
        code: code.to_owned(),
        message: error.to_string(),
    }
}

fn redact_human_shell_projection(text: String) -> Result<String, CoreResponse> {
    use crate::shell::projection::RedactionState;
    use crate::shell::projector::{
        apply_redaction_hook, ProjectionKind, ProjectionResult, ProjectionTarget,
    };

    if text.is_empty() || text.len() > codegg_protocol::human_shell::HUMAN_SHELL_PROMOTION_MAX_BYTES
    {
        return Err(CoreResponse::Error {
            code: "human_shell_promotion_invalid".into(),
            message: "Shell projection is empty or exceeds the 32 KiB limit".into(),
        });
    }
    let mut projection = ProjectionResult::empty("human-shell-promotion", ProjectionKind::Raw);
    projection.text = crate::shell::sanitize_ansi(&text, crate::config::schema::AnsiMode::Strip);
    projection.output_bytes = projection.text.len();
    apply_redaction_hook(&mut projection, ProjectionTarget::ModelContext);
    if !matches!(
        projection.redaction,
        RedactionState::Applied { .. } | RedactionState::AppliedNoMatches
    ) {
        return Err(CoreResponse::Error {
            code: "human_shell_redaction_unavailable".into(),
            message: "Shell projection could not be safely redacted".into(),
        });
    }
    if projection.text.is_empty()
        || projection.text.len() > codegg_protocol::human_shell::HUMAN_SHELL_PROMOTION_MAX_BYTES
    {
        return Err(CoreResponse::Error {
            code: "human_shell_promotion_invalid".into(),
            message: "Redacted shell projection is empty or exceeds the 32 KiB limit".into(),
        });
    }
    Ok(projection.text)
}

fn promotion_to_dto(
    promotion: codegg_core::session::HumanShellPromotion,
) -> crate::protocol::human_shell::HumanShellPromotionDto {
    use crate::protocol::human_shell::HumanShellPromotionStateDto as State;
    crate::protocol::human_shell::HumanShellPromotionDto {
        promotion_id: promotion.promotion_id,
        source_command_id: promotion.source_command_id,
        projection: promotion.projection,
        projection_bytes: promotion.projection_bytes,
        source_exit_code: promotion.source_exit_code,
        source_truncated: promotion.source_truncated,
        created_at_ms: promotion.created_at_ms,
        expires_at_ms: promotion.expires_at_ms,
        state: match promotion.state {
            codegg_core::session::HumanShellPromotionState::Pending => State::Pending,
            codegg_core::session::HumanShellPromotionState::Reserved => State::Reserved,
            codegg_core::session::HumanShellPromotionState::Consumed => State::Consumed,
            codegg_core::session::HumanShellPromotionState::Expired => State::Expired,
        },
        consumed_turn_id: promotion.consumed_turn_id,
    }
}

fn shell_prompt_submit_response(
    accepted: bool,
    staged: StagedHumanShellPromotion,
    error_code: Option<String>,
    error_message: Option<String>,
) -> CoreResponse {
    CoreResponse::ShellPromptSubmitResult {
        accepted,
        stage_outcome: staged.outcome,
        promotion: staged.promotion,
        error_code,
        error_message,
    }
}

fn append_shell_promotions(
    messages: &mut Vec<crate::protocol::dto::ProviderMessage>,
    promotions: &[codegg_core::session::HumanShellPromotion],
) {
    use crate::protocol::dto::{ContentPart, ProviderMessage};
    for promotion in promotions {
        let Some(projection) = promotion.projection.as_ref() else {
            continue;
        };
        let text = format!(
            "[User-approved human-shell output from command {}]\n{}",
            promotion.source_command_id, projection
        );
        if let Some(ProviderMessage::User { content }) = messages.last_mut() {
            content.push(ContentPart::Text { text });
        } else {
            messages.push(ProviderMessage::User {
                content: vec![ContentPart::Text { text }],
            });
        }
    }
}

impl CoreDaemon {
    pub(crate) async fn handle_turns_request(
        &self,
        request: CoreRequest,
        request_id: &str,
        trusted_client_id: &str,
        authority: codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
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
                plan_mode,
                reasoning,
                shell_promotion_ids,
                ..
            } => {
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
                // Validate the provider exists before delegating to the turn
                // runtime. This preserves the existing `provider_not_found`
                // response shape from the daemon layer. The turn runtime
                // also validates provider existence internally, so this is
                // intentionally duplicated for backward-compatible error handling.
                let mut registry = crate::provider::ProviderRegistry::new();
                let config = super::load_config_or_default();
                crate::provider::register_builtin_with_config(&mut registry, &config);
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

                let promotion_records = if shell_promotion_ids.is_empty() {
                    None
                } else {
                    let principal = authority.principal();
                    if !matches!(
                        principal.kind(),
                        codegg_core::team::PrincipalKind::Human
                            | codegg_core::team::PrincipalKind::LocalOwner
                    ) {
                        self.rollback_unstarted_turn(&session_id, &turn_id).await;
                        return Ok(CoreResponse::Error {
                            code: "human_shell_promotion_unauthorized".into(),
                            message: "Only a human session owner can submit staged shell output"
                                .into(),
                        });
                    }
                    let Some(pool) = self.pool.clone() else {
                        self.rollback_unstarted_turn(&session_id, &turn_id).await;
                        return Ok(CoreResponse::Error {
                            code: "human_shell_promotion_unavailable".into(),
                            message: "Durable session storage is unavailable".into(),
                        });
                    };
                    let store = codegg_core::session::HumanShellPromotionStore::new(pool);
                    let recovered = store
                        .recover_orphaned_for_session(&session_id, Some(&turn_id))
                        .await;
                    if let Err(error) = recovered {
                        self.rollback_unstarted_turn(&session_id, &turn_id).await;
                        return Ok(human_shell_store_error(error));
                    }
                    match store
                        .reserve_for_turn(
                            &session_id,
                            &runtime.project_id,
                            &runtime.workspace_id.to_string(),
                            principal.principal_id().as_str(),
                            &shell_promotion_ids,
                            &turn_id,
                            chrono::Utc::now().timestamp_millis(),
                        )
                        .await
                    {
                        Ok(records) => {
                            append_shell_promotions(&mut messages, &records);
                            Some(store)
                        }
                        Err(error) => {
                            self.rollback_unstarted_turn(&session_id, &turn_id).await;
                            return Ok(human_shell_store_error(error));
                        }
                    }
                };

                // M004 (ADR-0007): atomic controller acquisition with
                // accepting the turn. The durable row is written before
                // any execution starts; a store conflict rolls back the
                // in-memory turn so a failed submission leaves no lease.
                // LocalOwner broad policy still records a lease (solo
                // behavior unchanged: the submitter controls its turn)
                // so projection/presence stay coherent.
                let now_ms = chrono::Utc::now().timestamp_millis();
                match self
                    .acquire_turn_controller(
                        &session_id,
                        &turn_id,
                        &authority,
                        trusted_client_id,
                        now_ms,
                    )
                    .await
                {
                    Ok((controller, revision)) => {
                        self.mirror_controller_to_runtime(
                            &session_id,
                            &turn_id,
                            controller
                                .as_deref()
                                .unwrap_or(authority.principal_id().as_str()),
                            Some(trusted_client_id),
                            revision,
                        )
                        .await;
                    }
                    Err(error) => {
                        if let Some(store) = promotion_records.as_ref() {
                            if let Err(release_error) =
                                store.release_for_turn(&shell_promotion_ids, &turn_id).await
                            {
                                tracing::error!(%release_error, "failed to release shell promotions after controller rejection");
                            }
                        }
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
                            code: "turn_already_active".to_string(),
                            message: format!("turn submission raced active control: {error}"),
                        });
                    }
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
                    repository_id: None,
                    asset_snapshot,
                    asset_pin,
                    approval_mode,
                    sandbox_profile,
                    credential_observer,
                };
                let turn_output = match self.deps.turn_runtime.run_turn(turn_input).await {
                    Ok(output) => {
                        if let Some(store) = promotion_records.as_ref() {
                            if let Err(error) =
                                store.consume_for_turn(&shell_promotion_ids, &turn_id).await
                            {
                                tracing::error!(%error, %session_id, %turn_id, "accepted turn could not finalize staged shell promotions");
                                // The runtime already accepted the turn. Do not report
                                // rejection to the client or invite a duplicate submit;
                                // the durable reservation is recovered if this process
                                // later loses the active turn.
                            }
                        }
                        output
                    }
                    Err(error) => {
                        if let Some(store) = promotion_records.as_ref() {
                            if let Err(release_error) =
                                store.release_for_turn(&shell_promotion_ids, &turn_id).await
                            {
                                tracing::error!(%release_error, "failed to release shell promotions after turn startup failure");
                            }
                        }
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

                Ok(CoreResponse::Ack)
            }
            CoreRequest::SessionPromptSubmit {
                session_id,
                text,
                plan_mode,
                mut shell_promotion_ids,
                shell_promotion,
            } => {
                let staged = if let Some(promotion) = shell_promotion {
                    match self
                        .stage_human_shell_promotion(&session_id, promotion, &authority, request_id)
                        .await
                    {
                        Ok(staged) => {
                            if !shell_promotion_ids.contains(&staged.promotion.promotion_id) {
                                shell_promotion_ids.push(staged.promotion.promotion_id.clone());
                            }
                            Some(staged)
                        }
                        Err(error) => return Ok(error),
                    }
                } else {
                    None
                };
                // M004 desktop session slice: resolve the composed
                // turn daemon-side, then submit through the identical
                // `TurnSubmit` path above (single recursion, no new
                // runtime, scheduler, or authority).
                let (model, agents, messages) = match self
                    .resolve_prompt_submit_composition(&session_id, &text)
                    .await
                {
                    Ok(composed) => composed,
                    Err(error) => {
                        if let Some(staged) = staged {
                            return Ok(shell_prompt_submit_response(
                                false,
                                staged,
                                Some(error.code.to_owned()),
                                Some(error.message),
                            ));
                        }
                        return Ok(error.into_core_response());
                    }
                };
                let response = Box::pin(self.handle_turns_request(
                    CoreRequest::TurnSubmit {
                        session_id,
                        text,
                        plan_mode,
                        model,
                        agents,
                        current_agent_idx: 0,
                        messages,
                        reasoning: None,
                        shell_promotion_ids,
                    },
                    request_id,
                    trusted_client_id,
                    authority,
                    authz_decision,
                ))
                .await?;
                if let Some(staged) = staged {
                    let accepted = matches!(response, CoreResponse::Ack);
                    let (code, message) = match response {
                        CoreResponse::Error { code, message } => (Some(code), Some(message)),
                        _ => (None, None),
                    };
                    return Ok(shell_prompt_submit_response(
                        accepted, staged, code, message,
                    ));
                }
                return Ok(response);
            }
            CoreRequest::ShellPromotionCapabilities => {
                return Ok(CoreResponse::ShellPromotionCapabilities {
                    capabilities:
                        crate::protocol::human_shell::HumanShellPromotionCapabilitiesDto::current(),
                });
            }
            CoreRequest::ShellPromotionStage {
                session_id,
                promotion,
            } => {
                return match self
                    .stage_human_shell_promotion(&session_id, promotion, &authority, request_id)
                    .await
                {
                    Ok(staged) => Ok(CoreResponse::ShellPromotionStaged {
                        outcome: staged.outcome,
                        promotion: staged.promotion,
                    }),
                    Err(response) => Ok(response),
                };
            }
            CoreRequest::ShellPromotionList { session_id } => {
                return self
                    .list_human_shell_promotions(&session_id, &authority)
                    .await;
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
                let (session_id, turn_id, simple_perm_id) = match id.strip_prefix("perm:").and_then(
                    |rest| {
                        let mut parts = rest.splitn(3, ':');
                        let sid = parts.next()?.to_string();
                        let turn = parts.next()?.to_string();
                        let pid = parts.next()?.to_string();
                        Some((sid, turn, pid))
                    },
                ) {
                    Some(parsed) => parsed,
                    None => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_permission_id".to_string(),
                            message: format!(
                                "Permission ID '{}' is not in perm:<session_id>:<turn_id>:<perm_id> format",
                                id
                            ),
                        });
                    }
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
                    None => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_question_id".to_string(),
                            message: format!(
                                "Question ID '{}' is not in question:<session_id>:<turn_id>:<question_id> format",
                                id
                            ),
                        });
                    }
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
                crate::provider::register_builtin_with_config(&mut registry, &config);
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
                                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                    tracing::debug!(turn_id = %turn_id, "turn steering receiver already closed");
                                    Ok(CoreResponse::Error {
                                        code: "steer_unavailable".to_string(),
                                        message: "Turn steering receiver is unavailable"
                                            .to_string(),
                                    })
                                }
                                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                                    Ok(CoreResponse::Error {
                                        code: "steer_queue_full".to_string(),
                                        message: "Turn steering queue is full".to_string(),
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
                crate::provider::register_builtin_with_config(&mut registry, &config);
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

    async fn rollback_unstarted_turn(&self, session_id: &str, turn_id: &str) {
        if let Some(runtime) = self.sessions.get(session_id) {
            let mut active = runtime.active_turn.write().await;
            if active
                .as_ref()
                .is_some_and(|handle| handle.turn_id == turn_id)
            {
                *active = None;
            }
            drop(active);
            let mut status = runtime.status.write().await;
            *status = crate::core::session_runtime::RuntimeSessionStatus::Idle;
        }
    }

    async fn stage_human_shell_promotion(
        &self,
        session_id: &str,
        mut stage: crate::protocol::human_shell::HumanShellPromotionStageDto,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        _request_id: &str,
    ) -> Result<StagedHumanShellPromotion, CoreResponse> {
        stage.projection = redact_human_shell_projection(stage.projection)?;
        let principal = authority.principal();
        if !matches!(
            principal.kind(),
            codegg_core::team::PrincipalKind::Human | codegg_core::team::PrincipalKind::LocalOwner
        ) {
            return Err(CoreResponse::Error {
                code: "human_shell_promotion_unauthorized".into(),
                message: "Only a human session owner can stage shell output".into(),
            });
        }
        let runtime = self
            .bind_runtime_for_session(session_id)
            .await
            .map_err(|error| CoreResponse::Error {
                code: "session_unbound".into(),
                message: format!("session has no resolvable workspace: {error}"),
            })?;
        let Some(pool) = self.pool.clone() else {
            return Err(CoreResponse::Error {
                code: "human_shell_promotion_unavailable".into(),
                message: "Durable session storage is unavailable".into(),
            });
        };
        let input = codegg_core::session::HumanShellPromotionInput {
            promotion_id: stage.promotion_id,
            session_id: session_id.to_owned(),
            project_id: runtime.project_id.clone(),
            workspace_id: runtime.workspace_id.to_string(),
            principal_id: principal.principal_id().as_str().to_owned(),
            source_command_id: stage.source_command_id,
            projection: stage.projection,
            source_exit_code: stage.source_exit_code,
            source_truncated: stage.source_truncated,
        };
        let store = codegg_core::session::HumanShellPromotionStore::new(pool);
        let (outcome, record) = store
            .stage(input, chrono::Utc::now().timestamp_millis())
            .await
            .map_err(human_shell_store_error)?;
        Ok(StagedHumanShellPromotion {
            outcome: match outcome {
                codegg_core::session::HumanShellPromotionIdempotency::Staged => {
                    crate::protocol::human_shell::HumanShellPromotionStageOutcomeDto::Staged
                }
                codegg_core::session::HumanShellPromotionIdempotency::Duplicate => {
                    crate::protocol::human_shell::HumanShellPromotionStageOutcomeDto::Duplicate
                }
            },
            promotion: promotion_to_dto(record),
        })
    }

    async fn list_human_shell_promotions(
        &self,
        session_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
    ) -> Result<CoreResponse, AppError> {
        let principal = authority.principal();
        if !matches!(
            principal.kind(),
            codegg_core::team::PrincipalKind::Human | codegg_core::team::PrincipalKind::LocalOwner
        ) {
            return Ok(CoreResponse::Error {
                code: "human_shell_promotion_unauthorized".into(),
                message: "Only a human session owner can query staged shell output".into(),
            });
        }
        let runtime = match self.bind_runtime_for_session(session_id).await {
            Ok(runtime) => runtime,
            Err(error) => {
                return Ok(CoreResponse::Error {
                    code: "session_unbound".into(),
                    message: format!("session has no resolvable workspace: {error}"),
                });
            }
        };
        let Some(pool) = self.pool.clone() else {
            return Ok(CoreResponse::Error {
                code: "human_shell_promotion_unavailable".into(),
                message: "Durable session storage is unavailable".into(),
            });
        };
        let active_turn = runtime
            .active_turn
            .read()
            .await
            .as_ref()
            .map(|handle| handle.turn_id.clone());
        let store = codegg_core::session::HumanShellPromotionStore::new(pool);
        store
            .recover_orphaned_for_session(session_id, active_turn.as_deref())
            .await
            .map_err(|error| AppError::Other(anyhow::anyhow!(error.to_string())))?;
        let promotions = store
            .list(
                session_id,
                &runtime.project_id,
                runtime.workspace_id.as_str(),
                principal.principal_id().as_str(),
                chrono::Utc::now().timestamp_millis(),
            )
            .await
            .map_err(|error| AppError::Other(anyhow::anyhow!(error.to_string())))?
            .into_iter()
            .map(promotion_to_dto)
            .collect();
        Ok(CoreResponse::ShellPromotionList { promotions })
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

    async fn resolve_prompt_submit_composition(
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

    use super::append_shell_promotions;
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

    #[test]
    fn accepted_turn_context_includes_each_staged_projection_once() {
        let record = codegg_core::session::HumanShellPromotion {
            promotion_id: "p1".into(),
            session_id: "s1".into(),
            project_id: "project1".into(),
            workspace_id: "workspace1".into(),
            principal_id: "human1".into(),
            source_command_id: "9".into(),
            projection: Some("redacted evidence marker".into()),
            projection_bytes: 23,
            source_exit_code: Some(0),
            source_truncated: false,
            created_at_ms: 1,
            expires_at_ms: 100,
            state: codegg_core::session::HumanShellPromotionState::Pending,
            consumed_turn_id: None,
        };
        let mut messages = Vec::new();
        append_shell_promotions(&mut messages, &[record]);
        let rendered = serde_json::to_string(&messages).expect("provider DTO serialization");
        assert_eq!(rendered.matches("redacted evidence marker").count(), 1);
        assert_eq!(
            rendered.matches("User-approved human-shell output").count(),
            1
        );
    }

    /// Records that the turn actually reached the injected runtime, so a
    /// test can tell "accepted" from "rejected early" without a provider
    /// call ever leaving the process.
    struct RecordingTurnRuntime {
        called: AtomicBool,
        messages: std::sync::Mutex<Option<Vec<codegg_protocol::dto::ProviderMessage>>>,
    }

    #[async_trait::async_trait]
    impl crate::agent::turn_runtime::TurnRuntime for RecordingTurnRuntime {
        async fn run_turn(
            &self,
            input: crate::agent::turn_runtime::TurnRunInput,
        ) -> Result<crate::agent::turn_runtime::TurnRunOutput, crate::error::AppError> {
            self.called.store(true, Ordering::SeqCst);
            *self.messages.lock().expect("runner messages lock") = Some(input.messages_dto);
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
            messages: std::sync::Mutex::new(None),
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
                model: model.to_string(),
                agents: vec![crate::protocol_conversions::agent_to_dto(agent).expect("agent dto")],
                current_agent_idx: 0,
                messages: vec![],
                reasoning: None,
                shell_promotion_ids: Vec::new(),
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

    #[tokio::test(flavor = "current_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn accepted_turn_injects_staged_shell_projection_once_and_consumes_it() {
        let _env_guard = crate::auth::test_support::lock_env();
        let previous = std::env::var("OPENCODE_GO_API_KEY").ok();
        std::env::set_var("OPENCODE_GO_API_KEY", "test-key-not-used");

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

        let promotion_id = "accepted-shell-promotion";
        let staged = daemon
            .handle_request(crate::core::new_request(
                "stage-before-turn".into(),
                CoreRequest::ShellPromotionStage {
                    session_id: session_id.clone(),
                    promotion: codegg_protocol::human_shell::HumanShellPromotionStageDto {
                        promotion_id: promotion_id.into(),
                        source_command_id: "17".into(),
                        projection:
                            "captured evidence Authorization: Bearer staged-secret-1234567890"
                                .into(),
                        source_exit_code: Some(1),
                        source_truncated: false,
                    },
                },
            ))
            .await
            .expect("stage projection");
        assert!(matches!(staged, CoreResponse::ShellPromotionStaged { .. }));

        let mut request = turn_request(&session_id, "other:opencode_go/minimax-m3");
        if let CoreRequest::TurnSubmit {
            shell_promotion_ids,
            ..
        } = &mut request.payload
        {
            shell_promotion_ids.push(promotion_id.into());
        } else {
            unreachable!("turn_request returns TurnSubmit");
        }
        let response = daemon.handle_request(request).await.expect("accepted turn");
        assert!(matches!(response, CoreResponse::Ack), "{response:?}");
        assert!(runtime.called.load(Ordering::SeqCst));
        let messages = runtime
            .messages
            .lock()
            .expect("runner messages lock")
            .clone()
            .expect("captured provider-facing context");
        let rendered = serde_json::to_string(&messages).expect("provider DTO serialization");
        assert_eq!(rendered.matches("captured evidence").count(), 1);
        assert_eq!(
            rendered
                .matches("[User-approved human-shell output")
                .count(),
            1
        );
        assert!(rendered.contains("[REDACTED:bearer-token]"));
        assert!(!rendered.contains("staged-secret-1234567890"));

        let (state, projection_bytes): (String, i64) = sqlx::query_as(
            "SELECT state, projection_bytes FROM human_shell_promotion WHERE promotion_id = ?",
        )
        .bind(promotion_id)
        .fetch_one(&pool)
        .await
        .expect("promotion lifecycle row");
        assert_eq!(state, "consumed");
        assert_eq!(projection_bytes, 0);

        if let Some(value) = previous {
            std::env::set_var("OPENCODE_GO_API_KEY", value);
        } else {
            std::env::remove_var("OPENCODE_GO_API_KEY");
        }
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
                matches!(&response, CoreResponse::Ack),
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
            matches!(&response, CoreResponse::Ack),
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
