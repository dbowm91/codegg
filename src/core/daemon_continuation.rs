//! C005 (ADR-0031, accepted): authenticated idle-continuation grants and
//! the daemon-internal delegated turn admission.
//!
//! Grant *management* is ordinary principal-authorized project mutation:
//! three `CoreRequest` variants through the existing daemon gate
//! (`via_session` + `agent.invoke` for create/revoke, `session.read` for
//! status), with every identity field resolved server-side. The grant
//! handler additionally captures immutable [`OriginAttribution`] under
//! scope `continuation_grant`; a grant row without that row is
//! non-autonomous and can never admit a turn, which is what keeps every
//! legacy C002/C004 row notify-only.
//!
//! Dispatch is NOT a wire operation. [`CoreDaemon::submit_idle_continuation_turn`]
//! is the single daemon-internal delegated admission entry: it revalidates
//! the grant against current team/session/provider state, CAS-claims the
//! `(notice, grant)` intent, then admits through the canonical
//! `TurnSubmit` path with authority derived from the grant's stored origin
//! (reconstructed owner principal, `idle:<grant_id>` client marker as
//! metadata only). Exactly two call sites are permitted — the post-commit
//! fanout wake and the bounded restart-reconciliation twin — enforced by
//! `scripts/check_idle_continuation_callsites.py`.

use std::sync::Arc;

use crate::error::AppError;
use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::protocol::dto::ContinuationGrantDto;
use crate::scheduler::idle_continuation::{
    delegated_client_id, delegation_request_digest, delegation_submission_id, grant_status_to_str,
    ContinuationGrant, ContinuationService, DispatchOutcome, HostGrantInput, IdleContinuationState,
};

use super::daemon::CoreDaemon;

fn continuation_service_of(daemon: &CoreDaemon) -> Option<Arc<ContinuationService>> {
    daemon.continuation_service.clone()
}

fn continuation_unavailable() -> CoreResponse {
    CoreResponse::Error {
        code: "continuation_unavailable".to_string(),
        message: "daemon has no idle-continuation service".to_string(),
    }
}

/// C005 grant-creation input. Bundled so the handler keeps a narrow
/// signature; the struct carries locators and bounds only — there is no
/// identity field to spoof, and every identity the grant stores is still
/// resolved server-side below.
pub(crate) struct GrantCreateInput {
    pub session_id: String,
    pub notice_job_id: String,
    pub allowed_model: Option<String>,
    pub allowed_connection: Option<String>,
    pub max_continuations: Option<u32>,
    pub max_tokens: Option<u64>,
    pub max_tool_calls: Option<u32>,
    pub max_duration_ms: Option<i64>,
    pub ttl_ms: Option<i64>,
}

fn grant_to_dto(grant: &ContinuationGrant) -> ContinuationGrantDto {
    ContinuationGrantDto {
        grant_id: grant.grant_id.clone(),
        notice_job_id: grant.notice_job_id.clone(),
        session_id: grant.session_id.clone(),
        // Legacy rows carry no attestable owner; the empty string marks
        // them non-autonomous at the projection boundary, exactly as the
        // wake treats a missing attribution row.
        owner_principal: grant.owner_principal.clone().unwrap_or_default(),
        workspace_id: grant.workspace_id.clone(),
        allowed_model: grant.allowed_model.clone(),
        allowed_connection: grant.allowed_connection.clone(),
        max_continuations: grant.max_continuations,
        used_continuations: grant.used_continuations,
        max_tokens: grant.max_tokens,
        max_tool_calls: grant.max_tool_calls,
        max_duration_ms: grant.max_duration_ms,
        status: grant_status_to_str(grant.status).to_string(),
        expiry_ms: grant.expiry_ms,
        created_at_ms: grant.created_at,
        updated_at_ms: grant.updated_at,
    }
}

impl CoreDaemon {
    /// C005 grant creation: approve one bounded idle continuation.
    ///
    /// The gate already enforced `agent.invoke` on the owning session's
    /// project. Everything identity is re-resolved here server-side from
    /// the transport authority, the gate decision, and the owning session
    /// row; payload identity fields do not exist on this request. The
    /// stored origin attribution row is what later makes the grant
    /// wake-eligible — attribution failure revokes the grant rather than
    /// leaving an approved row without attestable origin.
    pub(crate) async fn handle_continuation_grant_create(
        &self,
        input: GrantCreateInput,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        let GrantCreateInput {
            session_id,
            notice_job_id,
            allowed_model,
            allowed_connection,
            max_continuations,
            max_tokens,
            max_tool_calls,
            max_duration_ms,
            ttl_ms,
        } = input;
        let Some(service) = continuation_service_of(self) else {
            return Ok(continuation_unavailable());
        };
        let Some(pool) = self.pool.clone() else {
            return Ok(CoreResponse::Error {
                code: "continuation_unavailable".to_string(),
                message: "idle-continuation grants require durable daemon storage".to_string(),
            });
        };
        if notice_job_id.is_empty() {
            return Ok(CoreResponse::Error {
                code: "invalid_continuation_grant".to_string(),
                message: "grant requires a background job locator".to_string(),
            });
        }
        // Owning session row, server-side: unknown, deleted, or archived
        // sessions cannot anchor a grant. `SessionStore::get` already
        // excludes deleted rows.
        let store = codegg_core::session::SessionStore::new(pool.clone());
        let session = match store.get(&session_id).await {
            Ok(Some(session)) => session,
            Ok(None) => {
                return Ok(CoreResponse::Error {
                    code: "invalid_continuation_grant".to_string(),
                    message: format!("session not found: {session_id}"),
                });
            }
            Err(error) => {
                return Ok(CoreResponse::Error {
                    code: "continuation_grant_failed".to_string(),
                    message: format!("session lookup failed: {error}"),
                });
            }
        };
        if session.time_archived.is_some() {
            return Ok(CoreResponse::Error {
                code: "invalid_continuation_grant".to_string(),
                message: format!("session is archived: {session_id}"),
            });
        }
        let workspace_id = match self
            .resolve_session_context(&session_id, &session.directory)
            .await
        {
            Ok(context) => Some(context.workspace_id.to_string()),
            Err(error) => {
                return Ok(CoreResponse::Error {
                    code: "invalid_continuation_grant".to_string(),
                    message: format!("session has no resolvable workspace: {error}"),
                });
            }
        };
        // Lineage only: the turn active at grant time, if any. The wake
        // revalidates idleness; this never licenses a concurrent turn.
        let origin_turn_id = self.sessions.get(&session_id).and_then(|runtime| {
            runtime
                .active_turn
                .try_read()
                .ok()
                .and_then(|active| active.as_ref().map(|handle| handle.turn_id.clone()))
        });
        for bound in [&allowed_model, &allowed_connection] {
            if bound
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Ok(CoreResponse::Error {
                    code: "invalid_continuation_grant".to_string(),
                    message: "grant model/connection bindings must not be empty".to_string(),
                });
            }
        }
        let owner_principal = authority.principal_id().as_str().to_string();
        let grant = match service
            .authorize_host_grant(HostGrantInput {
                notice_job_id,
                session_id: session_id.clone(),
                owner_principal,
                workspace_id,
                origin_turn_id,
                allowed_model,
                allowed_connection,
                max_continuations,
                max_tokens,
                max_tool_calls,
                max_duration_ms,
                ttl_ms,
            })
            .await
        {
            Ok(grant) => grant,
            Err(error) => {
                return Ok(CoreResponse::Error {
                    code: "continuation_grant_failed".to_string(),
                    message: error.to_string(),
                });
            }
        };
        // Immutable server-resolved origin: the wake-eligibility root.
        // First-write-wins; a failed write revokes the grant (fail-closed)
        // so no approved row ever lacks attestable origin.
        let attribution = codegg_core::authorization::OriginAttribution::from_authority(
            authority.principal(),
            authz_decision,
        );
        let attribution_store = codegg_core::authorization::OriginAttributionStore::new(pool);
        if attribution_store
            .record("continuation_grant", &grant.grant_id, &attribution)
            .await
            .is_err()
        {
            let _ = service.revoke(&grant.grant_id).await;
            return Ok(CoreResponse::Error {
                code: "continuation_grant_failed".to_string(),
                message: "grant origin capture failed; grant revoked".to_string(),
            });
        }
        // Post-creation structural audit with the durable grant id (the
        // pre-side-effect emit is skipped for this minting operation, same
        // pattern as `JobSubmit`).
        {
            let provenance = codegg_core::authorization::audit_provenance(authz_decision);
            let mut chain = codegg_core::audit_instrumentation::AuditChainContext::new();
            chain.project = authz_decision.project_id.clone();
            chain.session_id = Some(session_id.clone());
            let builder = codegg_core::audit_instrumentation::agent_delegate_event(
                authority.principal(),
                &provenance,
                &chain,
                &session_id,
                &grant.grant_id,
                "allow",
            );
            self.append_audit_event(builder).await;
        }
        Ok(CoreResponse::ContinuationGrantCreated {
            grant_id: grant.grant_id,
        })
    }

    /// C005 grant revocation. Scoped to the owning session: a foreign
    /// session reads as not-found (privacy-safe, no existence oracle).
    /// Idempotent: revoking an already terminal grant succeeds.
    pub(crate) async fn handle_continuation_grant_revoke(
        &self,
        session_id: String,
        grant_id: String,
    ) -> Result<CoreResponse, AppError> {
        let Some(service) = continuation_service_of(self) else {
            return Ok(continuation_unavailable());
        };
        let grant = match service.get_grant(&grant_id).await {
            Ok(Some(grant)) if grant.session_id == session_id => grant,
            _ => {
                return Ok(CoreResponse::Error {
                    code: "continuation_grant_not_found".to_string(),
                    message: "grant not found".to_string(),
                });
            }
        };
        let _ = grant;
        match service.revoke(&grant_id).await {
            Ok(_) => Ok(CoreResponse::ContinuationGrantRevoked { grant_id }),
            Err(error) => Ok(CoreResponse::Error {
                code: "continuation_grant_failed".to_string(),
                message: error.to_string(),
            }),
        }
    }

    /// C005 grant inspection. Unknown or foreign-session grants read as
    /// absent (privacy-safe, same shape as the job surfaces). DB-fresh so
    /// a revocation committed elsewhere is visible immediately.
    pub(crate) async fn handle_continuation_grant_status(
        &self,
        session_id: String,
        grant_id: String,
    ) -> Result<CoreResponse, AppError> {
        let Some(service) = continuation_service_of(self) else {
            return Ok(continuation_unavailable());
        };
        let grant = match service.get_grant_fresh(&grant_id).await {
            Ok(Some(grant)) if grant.session_id == session_id => Some(grant_to_dto(&grant)),
            _ => None,
        };
        Ok(CoreResponse::ContinuationGrantStatus { grant })
    }

    /// C005 fanout wake: evaluate exactly one delegated dispatch for a
    /// committed terminal notice.
    ///
    /// Permitted caller 1 of [`Self::submit_idle_continuation_turn`]
    /// (the other is the restart twin
    /// [`Self::reconcile_idle_continuation_wakes`]). A missing, unusable,
    /// or non-autonomous grant leaves the durable notice + live hint
    /// standing as notify-only attention: this path never mints
    /// authority, it only revalidates it.
    pub async fn maybe_dispatch_idle_continuation(
        &self,
        notice: &crate::scheduler::job_notifications::JobTerminalNotice,
    ) -> DispatchOutcome {
        let Some(service) = self.continuation_service.clone() else {
            return DispatchOutcome::Suppressed;
        };
        if self.pool.is_none() {
            return DispatchOutcome::Suppressed;
        }
        let grant = match service.grant_for_job(&notice.job_id).await {
            Ok(Some(grant)) => grant,
            // No usable grant: a consumed/revoked grant with a dispatched
            // intent still reports replay (the first dispatch consumed the
            // budget); anything else stays notify-only.
            _ => match service.intents_for_notice(&notice.notice_id).await {
                Ok(intents) => {
                    let mut terminal = false;
                    for intent in &intents {
                        match intent.state {
                            IdleContinuationState::NeedsAttention => {
                                return DispatchOutcome::NeedsAttention;
                            }
                            IdleContinuationState::Suppressed | IdleContinuationState::Expired => {}
                            _ => terminal = true,
                        }
                    }
                    if terminal {
                        return DispatchOutcome::AlreadyDispatched;
                    }
                    return DispatchOutcome::Suppressed;
                }
                Err(_) => return DispatchOutcome::Suppressed,
            },
        };
        Box::pin(self.submit_idle_continuation_turn(notice, &grant)).await
    }

    /// C005 restart twin: bounded re-evaluation of recent terminals that
    /// may have missed their wake (crash between commit and fanout).
    ///
    /// Permitted caller 2 of [`Self::submit_idle_continuation_turn`].
    /// Notices with an existing intent row are skipped without dispatch;
    /// every attempt is CAS-gated, so a concurrent fanout wake wins at
    /// most once. Bounded like the hint reconciliation (512 rows); no
    /// unbounded poller, no per-job task.
    pub async fn reconcile_idle_continuation_wakes(&self) -> usize {
        let Some(service) = self.continuation_service.clone() else {
            return 0;
        };
        if self.pool.is_none() {
            return 0;
        }
        let Some(notifier) = self.deps.job_notification_service.clone() else {
            return 0;
        };
        let notices = match notifier.durable_notices_for_hint_recovery().await {
            Ok(notices) => notices,
            Err(error) => {
                tracing::warn!(error = %error, "idle-continuation wake reconciliation failed to list notices");
                return 0;
            }
        };
        let mut dispatched = 0usize;
        for notice in &notices {
            if !matches!(notice.status.as_str(), "completed" | "failed" | "timed_out") {
                continue;
            }
            let grant = match service.grant_for_job(&notice.job_id).await {
                Ok(Some(grant)) => grant,
                _ => continue,
            };
            match service.intents_for_notice(&notice.notice_id).await {
                Ok(intents) if !intents.is_empty() => continue,
                Err(_) => continue,
                _ => {}
            }
            if Box::pin(self.submit_idle_continuation_turn(notice, &grant)).await
                == DispatchOutcome::Dispatched
            {
                dispatched = dispatched.saturating_add(1);
            }
        }
        dispatched
    }

    /// C005 delegated admission: admit exactly one owner turn for a
    /// committed terminal through the canonical `TurnSubmit` path.
    ///
    /// Daemon-internal: no `CoreRequest` variant, no transport route, no
    /// bearer. Callers are restricted to
    /// [`Self::maybe_dispatch_idle_continuation`] and
    /// [`Self::reconcile_idle_continuation_wakes`] (see
    /// `scripts/check_idle_continuation_callsites.py`). Every check below
    /// revalidates against durable state at wake time; any mismatch fails
    /// closed to [`DispatchOutcome::Suppressed`] with the notice retained,
    /// and ambiguity after admission becomes `NeedsAttention`, never a
    /// blind replay.
    pub async fn submit_idle_continuation_turn(
        &self,
        notice: &crate::scheduler::job_notifications::JobTerminalNotice,
        grant: &ContinuationGrant,
    ) -> DispatchOutcome {
        use codegg_core::authorization::{
            AuthorizationRequest, AuthorizationService, Capability, OperationDescriptor, ScopeKind,
        };

        let Some(pool) = self.pool.clone() else {
            return DispatchOutcome::Suppressed;
        };
        let Some(service) = self.continuation_service.clone() else {
            return DispatchOutcome::Suppressed;
        };
        // Only resumable terminal statuses may continue. Cancel/expiry/
        // suppression/attention never resume from status text.
        if !matches!(notice.status.as_str(), "completed" | "failed" | "timed_out") {
            return DispatchOutcome::Suppressed;
        }
        // The grant is bound to this notice's exact owner lineage.
        if grant.session_id != notice.session_id {
            return DispatchOutcome::Suppressed;
        }
        // Replay short-circuit: an already-admitted intent reports its
        // disposition without requiring a still-usable grant (the first
        // dispatch consumes the grant budget). This keeps exact-once
        // replay safe after `bump_grant_use` marks the grant consumed.
        // Suppressed/expired-only histories fall through: a later wake
        // with a usable grant may still proceed.
        match service.intents_for_notice(&notice.notice_id).await {
            Ok(intents) => {
                let mut terminal = false;
                for intent in &intents {
                    match intent.state {
                        IdleContinuationState::NeedsAttention => {
                            return DispatchOutcome::NeedsAttention;
                        }
                        IdleContinuationState::Suppressed | IdleContinuationState::Expired => {}
                        _ => terminal = true,
                    }
                }
                if terminal {
                    return DispatchOutcome::AlreadyDispatched;
                }
            }
            Err(_) => return DispatchOutcome::Suppressed,
        }
        let now = chrono::Utc::now().timestamp_millis();
        let usable = grant.status
            == crate::scheduler::idle_continuation::ContinuationGrantStatus::Approved
            && now <= grant.expiry_ms
            && grant.used_continuations < grant.max_continuations;
        if !usable {
            return DispatchOutcome::Suppressed;
        }
        // Attestable origin is the wake-eligibility root: reconstruct the
        // owner principal from the stored row (lossless restoration of
        // already-admitted durable attribution, never synthesis from a
        // payload). No row — every legacy grant — stays notify-only.
        let attribution_store =
            codegg_core::authorization::OriginAttributionStore::new(pool.clone());
        let attribution = match attribution_store
            .get("continuation_grant", &grant.grant_id)
            .await
        {
            Ok(Some(attribution)) => attribution,
            _ => return DispatchOutcome::Suppressed,
        };
        let Some(stored_owner) = grant.owner_principal.as_deref() else {
            return DispatchOutcome::Suppressed;
        };
        if attribution.origin_principal.as_str() != stored_owner {
            tracing::warn!(
                grant_id = %grant.grant_id,
                "idle-continuation grant store divergence: attribution owner differs from grant row; failing closed"
            );
            return DispatchOutcome::Suppressed;
        }
        let delegated_client = delegated_client_id(&grant.grant_id);
        let owner = codegg_core::transport_auth::AuthenticatedPrincipal::reconstructed(
            attribution.origin_principal.clone(),
            attribution.origin_kind,
            attribution.auth_method,
            attribution.transport_class,
            delegated_client.clone(),
        );
        // Owning session row, server-side: unknown, deleted, or archived
        // sessions fail closed before any admission work.
        let session_store = codegg_core::session::SessionStore::new(pool.clone());
        let session = match session_store.get(&notice.session_id).await {
            Ok(Some(session)) if session.time_archived.is_none() => session,
            _ => return DispatchOutcome::Suppressed,
        };
        // Workspace lineage re-resolved, not trusted from the row.
        let workspace_id = match self
            .resolve_session_context(&notice.session_id, &session.directory)
            .await
        {
            Ok(context) => context.workspace_id.to_string(),
            Err(_) => return DispatchOutcome::Suppressed,
        };
        if let Some(expected) = grant.workspace_id.as_deref() {
            if workspace_id != expected {
                return DispatchOutcome::Suppressed;
            }
        }
        // Revalidate the owner against CURRENT team state through the
        // canonical service: revoked/suspended principals, removed
        // memberships, and capability narrowing all fail here even when
        // the grant-time decision allowed. The decision binds the current
        // membership revision for the admission below.
        let project = self.session_project(&pool, &notice.session_id).await;
        let correlation = format!("idle-continuation:{}:{}", grant.grant_id, notice.notice_id);
        let decision = {
            let team_service =
                AuthorizationService::new(codegg_core::team::TeamStore::new(pool.clone()));
            let authz_request = AuthorizationRequest::new(
                owner.clone(),
                OperationDescriptor::new(
                    "idle_continuation_dispatch",
                    ScopeKind::ViaSession,
                    Some(Capability::AgentInvoke),
                ),
                project,
                correlation.clone(),
            );
            match team_service.authorize(&authz_request).await {
                Ok(decision) => decision,
                Err(_) => return DispatchOutcome::Suppressed,
            }
        };
        // Owner idleness is validated UNDER the intent claim below, not
        // here: a pre-claim check races a concurrent winner (its admitted
        // turn would flip losers to suppression and hide the dispatch).
        // Claim losers observe the intent row and report replay; only the
        // claim holder evaluates busyness, parking its own intent.
        //
        // Host-authored terminal context only: kind/status/summary refs,
        // never remote CI text as instructions.
        let summary_text = format!(
            "Background {} {}: {}",
            notice.kind, notice.status, notice.summary
        );
        // Daemon-side composition through the same helper ordinary
        // session-composed submits use; an unselected/unresolvable model
        // fails closed here.
        let (composed_model, agents, messages) = match self
            .resolve_prompt_submit_composition(&notice.session_id, &summary_text)
            .await
        {
            Ok(composed) => composed,
            Err(_) => return DispatchOutcome::Suppressed,
        };
        // Grant bindings pin the wake to the approved model/connection.
        if let Some(expected) = grant.allowed_model.as_deref() {
            if composed_model != expected {
                return DispatchOutcome::Suppressed;
            }
        }
        if let Some(selection_service) = self.selection_service.as_ref() {
            match selection_service.get(&notice.session_id).await {
                Ok(crate::protocol::provider::SessionSelectionDto::Selected {
                    connection, ..
                }) => {
                    if connection.state != "active" {
                        return DispatchOutcome::Suppressed;
                    }
                    if let Some(expected) = grant.allowed_connection.as_deref() {
                        if connection.id != expected {
                            return DispatchOutcome::Suppressed;
                        }
                    }
                }
                Ok(_) => {
                    // No durable selection, yet composition resolved a
                    // model: a binding to a specific connection cannot be
                    // honored without one — fail closed only in that case.
                    if grant.allowed_connection.is_some() {
                        return DispatchOutcome::Suppressed;
                    }
                }
                Err(_) => return DispatchOutcome::Suppressed,
            }
        } else if grant.allowed_connection.is_some() {
            return DispatchOutcome::Suppressed;
        }
        // Exactly-once claim: first-write-wins on (notice, grant), then
        // Pending→Claimed CAS. A replay or a second daemon loses here.
        let first_write = match service
            .insert_intent_pending(&notice.notice_id, &grant.grant_id, &notice.session_id)
            .await
        {
            Ok(first_write) => first_write,
            Err(_) => return DispatchOutcome::Suppressed,
        };
        if !first_write {
            return match service.get_intent(&notice.notice_id, &grant.grant_id).await {
                Ok(Some(intent)) => match intent.state {
                    IdleContinuationState::NeedsAttention => DispatchOutcome::NeedsAttention,
                    IdleContinuationState::Suppressed | IdleContinuationState::Expired => {
                        DispatchOutcome::Suppressed
                    }
                    _ => DispatchOutcome::AlreadyDispatched,
                },
                _ => DispatchOutcome::Suppressed,
            };
        }
        if service
            .transition_intent(
                &notice.notice_id,
                &grant.grant_id,
                IdleContinuationState::Pending,
                IdleContinuationState::Claimed,
            )
            .await
            .map_or(true, |claimed| !claimed)
        {
            return DispatchOutcome::AlreadyDispatched;
        }
        // Re-validate idleness under the claim: a concurrent winner may
        // have admitted its turn between our pre-check and the CAS. The
        // claim holder parks its own intent as suppressed (a later wake
        // may retry); concurrent losers observe the intent row and report
        // replay instead of suppression.
        if let Some(runtime) = self.sessions.get(&notice.session_id) {
            let busy = runtime.active_turn.read().await.as_ref().is_some();
            if busy {
                let _ = service
                    .mark_intent(
                        &notice.notice_id,
                        &grant.grant_id,
                        IdleContinuationState::Suppressed,
                    )
                    .await;
                return DispatchOutcome::Suppressed;
            }
        }
        // Admit through the canonical TurnSubmit path with authority
        // derived from the revalidated grant origin. The deterministic
        // submission id + digest make a retried wake idempotent at the
        // durable receipt: same content replays to the same receipt,
        // different content or authority is a conflict, never a replay.
        let submission_id = delegation_submission_id(&notice.notice_id, &grant.grant_id);
        let digest = delegation_request_digest(
            &notice.notice_id,
            &grant.grant_id,
            &notice.session_id,
            owner.principal_id().as_str(),
            &summary_text,
        );
        let delegated_authority =
            codegg_core::transport_auth::RequestAuthorityContext::new(owner.clone(), correlation);
        // The generic pre-side-effect audit emit does not run for internal
        // recursion, so the delegated turn carries its own explicit record,
        // built here from the revalidated decision (clone) before the move.
        let dispatch_audit = {
            let provenance = codegg_core::authorization::audit_provenance(&decision);
            let mut chain = codegg_core::audit_instrumentation::AuditChainContext::new();
            chain.project = decision.project_id.clone();
            chain.session_id = Some(notice.session_id.clone());
            chain.job_id = Some(notice.job_id.clone());
            (owner, provenance, chain)
        };
        let admission = Box::pin(self.handle_turns_request(
            CoreRequest::TurnSubmit {
                session_id: notice.session_id.clone(),
                text: summary_text,
                plan_mode: false,
                model: composed_model,
                agents,
                current_agent_idx: 0,
                messages,
                reasoning: None,
                human_shell_promotion_ids: Vec::new(),
                max_model_tokens: None,
            },
            &submission_id,
            Some(digest),
            &delegated_client,
            delegated_authority,
            decision,
        ))
        .await;
        match admission {
            Ok(CoreResponse::TurnAdmissionReceipt { receipt }) => {
                let _ = service
                    .set_intent_turn(&notice.notice_id, &grant.grant_id, &receipt.turn_id)
                    .await;
                let _ = service
                    .transition_intent(
                        &notice.notice_id,
                        &grant.grant_id,
                        IdleContinuationState::Claimed,
                        IdleContinuationState::TurnAccepted,
                    )
                    .await;
                let _ = service.bump_grant_use(&grant.grant_id).await;
                let _ = service
                    .mark_intent(
                        &notice.notice_id,
                        &grant.grant_id,
                        IdleContinuationState::Completed,
                    )
                    .await;
                // Delegated-turn record: owner + grant + notice bound at
                // admission, auditable as delegated (never a transport
                // submission).
                {
                    let (audit_owner, provenance, mut chain) = dispatch_audit;
                    chain.turn_id = Some(receipt.turn_id.clone());
                    let builder = codegg_core::audit_instrumentation::agent_delegate_event(
                        &audit_owner,
                        &provenance,
                        &chain,
                        &notice.session_id,
                        &receipt.turn_id,
                        "allow",
                    );
                    self.append_audit_event(builder).await;
                }
                DispatchOutcome::Dispatched
            }
            Ok(_) => {
                let _ = service
                    .mark_intent(
                        &notice.notice_id,
                        &grant.grant_id,
                        IdleContinuationState::NeedsAttention,
                    )
                    .await;
                DispatchOutcome::NeedsAttention
            }
            Err(_) => {
                let _ = service
                    .mark_intent(
                        &notice.notice_id,
                        &grant.grant_id,
                        IdleContinuationState::NeedsAttention,
                    )
                    .await;
                DispatchOutcome::NeedsAttention
            }
        }
    }
}
