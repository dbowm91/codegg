//! Team Collaboration Corrective M004: shared-session controller lease (ADR-0007).
//!
//! Turn-scoped controller helpers for `CoreDaemon`. The lease narrows
//! existing project/session capability: every in-flight control path
//! (`TurnSteer`, `TurnCancel`, permission/question responses) runs its
//! normal capability gate first and then this controller predicate.
//! Observer/chat access never satisfies the predicate; disconnect never
//! transfers control; transfer/takeover is explicit, revisioned (CAS),
//! and audited. Revoked/suspended principals cannot exercise a stale
//! lease because every check re-resolves current team state.
//!
//! The durable record lives in
//! [`codegg_core::session_control::SessionControllerStore`] (migration
//! v65). The in-memory [`crate::core::session_runtime::TurnHandle`]
//! carries the same principal/client/revision so pool-less local
//! daemons and restart reconciliation share one predicate shape.

use codegg_core::identity::{PrincipalId, ProjectId};
use codegg_core::team::{Capability, MembershipState, PrincipalStatus, ProjectRole};

use crate::error::AppError;
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse};

use super::daemon::CoreDaemon;

impl CoreDaemon {
    /// `true` for session-control operations that mutate durable state.
    /// They skip the pre-side-effect audit emit and are recorded
    /// post-mutation with their durable revision (see
    /// `handle_control_request`).
    pub(crate) fn is_control_mutation(request: &CoreRequest) -> bool {
        matches!(
            request,
            CoreRequest::SessionControlRequest { .. }
                | CoreRequest::SessionControlTransfer { .. }
                | CoreRequest::SessionControlRelease { .. }
                | CoreRequest::SessionControlTakeover { .. }
        )
    }

    /// Durable controller store, when a pool is present.
    pub(crate) fn controller_store(
        &self,
    ) -> Option<codegg_core::session_control::SessionControllerStore> {
        self.pool
            .clone()
            .map(codegg_core::session_control::SessionControllerStore::new)
    }

    /// `true` for the personal-local broad policy: solo behavior is
    /// unchanged because the submitter automatically controls its turn.
    /// Team principals always take the lease path below.
    pub(crate) fn is_controller_bypass(
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
    ) -> bool {
        codegg_core::authorization::is_local_owner_broad(authority.principal())
    }

    /// Current controller lease for one session, if any.
    ///
    /// Pool-less daemons read the in-memory turn handle; durable
    /// daemons read the store (the handle mirrors it after every
    /// mutation). Returns `(controller_principal, revision, turn_id)`.
    pub(crate) async fn controller_lease_for(
        &self,
        session_id: &str,
    ) -> Option<(String, u64, String)> {
        if let Some(store) = self.controller_store() {
            if let Ok(Some(record)) = store.get(session_id).await {
                return Some((
                    record.controller_principal.as_str().to_owned(),
                    record.revision,
                    record.turn_id,
                ));
            }
            return None;
        }
        let runtime = self.sessions.get(session_id)?;
        let active = runtime.active_turn.read().await;
        let handle = active.as_ref()?;
        handle.controller_principal.clone().map(|principal| {
            (
                principal,
                handle.controller_revision,
                handle.turn_id.clone(),
            )
        })
    }

    /// Enforce the controller predicate for an in-flight mutation.
    ///
    /// Runs after the capability gate. `turn_id` is the caller-supplied
    /// turn locator (steer/cancel) or the pending item's owning turn
    /// (permission/question). Fails closed: unknown sessions, missing
    /// leases, turn mismatches, non-controller callers, and revoked or
    /// suspended callers all deny with typed codes and zero side
    /// effect. LocalOwner broad policy passes (solo behavior).
    ///
    /// Returns `None` when the caller satisfies the predicate and
    /// `Some(denial)` otherwise. The denial travels as an `Ok`
    /// `CoreResponse::Error` (never a large `Err` variant) following
    /// the daemon's typed-denial convention.
    pub(crate) async fn check_turn_controller(
        &self,
        session_id: &str,
        turn_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
    ) -> Option<CoreResponse> {
        // `Some` arms below construct the typed denial; `None` passes.
        let not_controller = || {
            Some(CoreResponse::Error {
                code: "session_control_not_controller".to_string(),
                message: "session turn is controlled by another principal".to_string(),
            })
        };
        if Self::is_controller_bypass(authority) {
            return None;
        }
        let caller = authority.principal_id().as_str().to_owned();
        // Revocation applies at request time: the caller must still be
        // an active principal with an active membership granting
        // `agent.invoke` on the owning project. The gate already
        // checked this at dispatch, but the lease must not outlive a
        // revocation that landed between gate and handler.
        if let Some(pool) = self.pool.clone() {
            if !self
                .caller_holds_invoke(&pool, authz_decision.project_id.as_ref(), authority)
                .await
            {
                return not_controller();
            }
        }
        let Some((controller, _revision, lease_turn)) = self.controller_lease_for(session_id).await
        else {
            return not_controller();
        };
        if lease_turn != turn_id {
            return Some(CoreResponse::Error {
                code: "session_control_conflict".to_string(),
                message: "turn is no longer active for this session".to_string(),
            });
        }
        if controller != caller {
            return not_controller();
        }
        // A revoked/suspended lease holder cannot exercise a stale
        // lease even if the row remains: re-resolve the controller's
        // current standing and fail closed.
        if let Some(pool) = self.pool.clone() {
            if !self
                .principal_holds_invoke(&pool, authz_decision.project_id.as_ref(), &controller)
                .await
            {
                return Some(CoreResponse::Error {
                    code: "session_control_not_controller".to_string(),
                    message: "session controller is no longer eligible".to_string(),
                });
            }
        }
        None
    }

    /// `true` when `principal` is currently eligible for in-flight
    /// control on `project`: active principal record plus an active
    /// membership granting `agent.invoke`. LocalOwner broad policy is
    /// handled by the caller (bypass); this helper answers for team
    /// principals only. Unknown projects fail closed.
    pub(crate) async fn principal_holds_invoke(
        &self,
        pool: &sqlx::SqlitePool,
        project: Option<&ProjectId>,
        principal_raw: &str,
    ) -> bool {
        let Ok(principal) = PrincipalId::parse(principal_raw) else {
            return false;
        };
        let team = codegg_core::team::TeamStore::new(pool.clone());
        let principal_record = match team.get_principal(&principal).await {
            Ok(Some(record)) => record,
            _ => return false,
        };
        if principal_record.status != PrincipalStatus::Active {
            return false;
        }
        let Some(project) = project else {
            return false;
        };
        let membership = match team.get_membership(project, &principal).await {
            Ok(membership) => membership,
            Err(_) => return false,
        };
        let Some(membership) = membership else {
            return false;
        };
        if membership.state != MembershipState::Active {
            return false;
        }
        membership.has_capability(Capability::AgentInvoke)
    }

    /// `true` when the request caller still holds `agent.invoke` on the
    /// gate-resolved project at handler time.
    async fn caller_holds_invoke(
        &self,
        pool: &sqlx::SqlitePool,
        project: Option<&ProjectId>,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
    ) -> bool {
        self.principal_holds_invoke(pool, project, authority.principal_id().as_str())
            .await
    }

    /// `true` when the caller holds Maintainer/Owner-equivalent policy
    /// on `project` (explicit role check, not just the gate
    /// capability, so the audit record names the authorizing role).
    pub(crate) async fn caller_is_maintainer_or_owner(
        &self,
        pool: &sqlx::SqlitePool,
        project: &ProjectId,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
    ) -> bool {
        let team = codegg_core::team::TeamStore::new(pool.clone());
        let membership = match team.get_membership(project, authority.principal_id()).await {
            Ok(membership) => membership,
            Err(_) => return false,
        };
        let Some(membership) = membership else {
            return false;
        };
        if membership.state != MembershipState::Active {
            return false;
        }
        matches!(
            membership.role,
            ProjectRole::Maintainer | ProjectRole::Owner
        )
    }

    /// Acquire the controller lease atomically with accepting a turn.
    ///
    /// Writes the durable row first (when pooled), then mirrors the
    /// principal/client/revision into the in-memory handle while still
    /// holding the turn lock semantics of the caller. On store
    /// conflict the caller must roll back the in-memory turn (the
    /// store changed nothing for the loser).
    pub(crate) async fn acquire_turn_controller(
        &self,
        session_id: &str,
        turn_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        trusted_client_id: &str,
        now_ms: i64,
    ) -> Result<(Option<String>, u64), AppError> {
        let principal = authority.principal_id().clone();
        if let Some(store) = self.controller_store() {
            let record = store
                .acquire(
                    session_id,
                    turn_id,
                    &principal,
                    Some(trusted_client_id),
                    now_ms,
                )
                .await
                .map_err(|e| AppError::Other(anyhow::anyhow!(e.to_string())))?;
            self.publish_control_changed(
                session_id,
                Some(turn_id),
                Some(record.controller_principal.as_str()),
                record.revision,
                "acquired",
            )
            .await;
            return Ok((
                Some(record.controller_principal.as_str().to_owned()),
                record.revision,
            ));
        }
        Ok((Some(principal.as_str().to_owned()), 1))
    }

    /// Release the lease when a turn reaches a terminal state.
    ///
    /// Deletes only when `turn_id` matches, so a stale transfer
    /// racing completion changes nothing (terminal wins). Clears the
    /// in-memory handle when it still names the same turn and parks
    /// the runtime status at `Idle` so the next submit can proceed.
    /// Used by restart recovery as well as the per-turn reaper.
    pub async fn release_turn_controller(&self, session_id: &str, turn_id: &str) {
        if let Some(store) = self.controller_store() {
            match store.release(session_id, turn_id).await {
                Ok(true) => {
                    self.publish_control_changed(session_id, Some(turn_id), None, 0, "released")
                        .await;
                }
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        session_id = %session_id,
                        turn_id = %turn_id,
                        "session control release failed"
                    );
                }
            }
        }
        if let Some(runtime) = self.sessions.get(session_id) {
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
    }

    /// Spawn the per-turn reaper that releases the lease (and parks the
    /// runtime) on the first terminal event for `(session_id, turn_id)`.
    ///
    /// The turn runtime publishes `TurnCompleted`/`TurnFailed` directly
    /// to the event log with the captured turn id; the reaper waits
    /// for exactly that envelope and then runs
    /// [`Self::release_turn_controller`]. Terminal wins over later
    /// transfer/takeover because the store release deletes only on
    /// turn match and later writers observe `NotFound`.
    pub fn spawn_turn_reaper(&self, session_id: String, turn_id: String) {
        let daemon_sessions = self.sessions.clone();
        let pool = self.pool.clone();
        let event_log = self.event_log.clone();
        tokio::spawn(async move {
            let mut rx = event_log.subscribe();
            loop {
                let envelope = match rx.recv().await {
                    Ok(envelope) => envelope,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                let terminal = match &envelope.payload {
                    crate::protocol::core::CoreEvent::TurnCompleted {
                        session_id: sid,
                        turn_id: tid,
                        ..
                    } => sid == &session_id && tid == &turn_id,
                    crate::protocol::core::CoreEvent::TurnFailed {
                        session_id: sid,
                        turn_id: Some(tid),
                        ..
                    } => sid == &session_id && tid == &turn_id,
                    _ => false,
                };
                if !terminal {
                    continue;
                }
                if let Some(store) = pool
                    .clone()
                    .map(codegg_core::session_control::SessionControllerStore::new)
                {
                    let _ = store.release(&session_id, &turn_id).await;
                }
                if let Some(runtime) = daemon_sessions.get(&session_id) {
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
                break;
            }
        });
    }

    /// Publish one structural control event (ids/revision/action only).
    pub(crate) async fn publish_control_changed(
        &self,
        session_id: &str,
        turn_id: Option<&str>,
        controller: Option<&str>,
        revision: u64,
        action: &str,
    ) {
        self.event_log
            .publish(
                Some(session_id.to_owned()),
                turn_id.map(str::to_owned),
                CoreEvent::SessionControlChanged {
                    session_id: session_id.to_owned(),
                    turn_id: turn_id.map(str::to_owned),
                    controller_principal: controller.map(str::to_owned),
                    revision,
                    action: action.to_owned(),
                },
            )
            .await;
    }

    /// Post-mutation audit for one control transition: structural
    /// locators (session/turn/controller/revision/action) only, never
    /// credentials, device secrets, or message bodies.
    pub(crate) async fn audit_control_transition(
        &self,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
        session_id: &str,
        turn_id: Option<&str>,
        controller: Option<&str>,
        revision: u64,
        action: &str,
    ) {
        use codegg_core::audit_instrumentation as instr;
        let provenance = codegg_core::authorization::audit_provenance(authz_decision);
        let mut chain = instr::AuditChainContext::new();
        chain.project = authz_decision.project_id.clone();
        chain.session_id = Some(session_id.to_owned());
        if let Some(turn) = turn_id {
            chain.turn_id = Some(turn.to_owned());
        }
        let member = format!(
            "session-control:{}:{}",
            session_id,
            controller.unwrap_or("released")
        );
        let builder = instr::membership_change_event(
            authority.principal(),
            &provenance,
            &chain,
            &member,
            action,
            Some(revision),
        );
        Box::pin(self.append_audit_event(builder)).await;
    }

    /// Mirror one durable lease into the in-memory turn handle (when
    /// the turn is still active under the same id).
    pub(crate) async fn mirror_controller_to_runtime(
        &self,
        session_id: &str,
        turn_id: &str,
        controller: &str,
        client: Option<&str>,
        revision: u64,
    ) {
        let Some(runtime) = self.sessions.get(session_id) else {
            return;
        };
        let mut active = runtime.active_turn.write().await;
        if let Some(handle) = active.as_mut() {
            if handle.turn_id == turn_id {
                handle.controller_principal = Some(controller.to_owned());
                handle.controller_client = client.map(str::to_owned);
                handle.controller_revision = revision;
            }
        }
    }

    /// Enforce the in-flight control predicate for a permission or
    /// question response.
    ///
    /// The `Global` capability gate for these operations is `none`, so
    /// this handler enforces both the owning-session mutation
    /// authority (`session.create`, matching the REST compatibility
    /// hook) and the controller lease. Callers without session
    /// authority observe the no-pending shape (no existence oracle);
    /// authorized non-controller members observe the typed controller
    /// code. LocalOwner broad policy passes (solo behavior).
    ///
    /// Returns `None` on success and `Some(denial)` otherwise (same
    /// `Ok`-denial convention as [`Self::check_turn_controller`]).
    pub(crate) async fn check_control_response(
        &self,
        session_id: &str,
        turn_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
    ) -> Option<CoreResponse> {
        // `Some` arms below construct the denial; `None` passes.
        let no_pending = || {
            Some(CoreResponse::Error {
                code: "permission_response_failed".to_string(),
                message: "No pending permission request found".to_string(),
            })
        };
        if Self::is_controller_bypass(authority) {
            return None;
        }
        let pool = self.pool.clone()?;
        // Resolve the owning project server-side; unknown sessions fail
        // closed with the no-pending shape.
        let project = self.session_project(&pool, session_id).await;
        let Some(project) = project else {
            return no_pending();
        };
        // Mutation authority on the owning session first (never
        // widened by the lease). Outsiders observe the no-pending
        // shape so pending IDs never become a project oracle.
        if !self
            .principal_holds_session_create(&pool, &project, authority.principal_id().as_str())
            .await
        {
            return no_pending();
        }
        // Controller lease second (narrows, never widens). Authorized
        // non-controller members observe the typed code.
        let mut decision = authz_decision.clone();
        decision.project_id = Some(project);
        match self
            .check_turn_controller(session_id, turn_id, authority, &decision)
            .await
        {
            None => None,
            Some(CoreResponse::Error { code, message }) if code == "session_control_conflict" => {
                Some(CoreResponse::Error { code, message })
            }
            Some(CoreResponse::Error { .. }) => Some(CoreResponse::Error {
                code: "session_control_not_controller".to_string(),
                message: "session turn is controlled by another principal".to_string(),
            }),
            Some(other) => Some(other),
        }
    }

    /// `true` when `principal` holds `session.create` on `project`
    /// with an active principal record and membership.
    async fn principal_holds_session_create(
        &self,
        pool: &sqlx::SqlitePool,
        project: &ProjectId,
        principal_raw: &str,
    ) -> bool {
        use codegg_core::team::Capability;
        let Ok(principal) = PrincipalId::parse(principal_raw) else {
            return false;
        };
        let team = codegg_core::team::TeamStore::new(pool.clone());
        let principal_record = match team.get_principal(&principal).await {
            Ok(Some(record)) => record,
            _ => return false,
        };
        if principal_record.status != PrincipalStatus::Active {
            return false;
        }
        let membership = match team.get_membership(project, &principal).await {
            Ok(membership) => membership,
            Err(_) => return false,
        };
        let Some(membership) = membership else {
            return false;
        };
        if membership.state != MembershipState::Active {
            return false;
        }
        membership.has_capability(Capability::SessionCreate)
    }

    /// Typed `CoreResponse::Error` for one domain failure.
    pub(crate) fn control_error(
        error: codegg_core::session_control::SessionControlError,
    ) -> CoreResponse {
        CoreResponse::Error {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }

    /// Handle the five `SessionControl*` operations. The capability
    /// gate has already run; transfer/takeover/request eligibility and
    /// the controller predicate are enforced here against current team
    /// state so revocation between gate and dispatch still fails
    /// closed.
    pub(crate) async fn handle_control_request(
        &self,
        request: CoreRequest,
        _request_id: &str,
        trusted_client_id: &str,
        authority: codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        match request {
            CoreRequest::SessionControlGet { session_id } => {
                let (controller, requests, truncated) = self.control_snapshot(&session_id).await;
                Ok(CoreResponse::SessionControl {
                    controller,
                    requests,
                    truncated,
                })
            }
            CoreRequest::SessionControlRequest {
                session_id,
                message,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "session_control_unavailable".to_string(),
                        message: "session control requires a durable database pool".to_string(),
                    });
                };
                if Self::is_controller_bypass(&authority) {
                    return Ok(CoreResponse::Error {
                        code: "session_control_invalid_input".to_string(),
                        message: "local-owner sessions need no control request".to_string(),
                    });
                }
                let store = codegg_core::session_control::SessionControllerStore::new(pool);
                let lease_turn = store
                    .get(&session_id)
                    .await
                    .map_err(|e| AppError::Other(anyhow::anyhow!(e.to_string())))?
                    .map(|record| record.turn_id);
                let now_ms = chrono::Utc::now().timestamp_millis();
                let request_record = store
                    .request_control(
                        &session_id,
                        lease_turn.as_deref(),
                        authority.principal_id(),
                        message.as_deref(),
                        now_ms,
                    )
                    .await
                    .map_err(|e| AppError::Other(anyhow::anyhow!(e.to_string())))?;
                let request_id = request_record.request_id.clone();
                let requester = request_record.requester_principal.as_str().to_owned();
                self.event_log
                    .publish(
                        Some(session_id.clone()),
                        lease_turn.clone(),
                        CoreEvent::SessionControlRequested {
                            session_id: session_id.clone(),
                            request_id: request_id.clone(),
                            requester_principal: requester,
                        },
                    )
                    .await;
                self.audit_control_transition(
                    &authority,
                    &authz_decision,
                    &session_id,
                    lease_turn.as_deref(),
                    Some(authority.principal_id().as_str()),
                    0,
                    "requested",
                )
                .await;
                let (controller, requests, truncated) = self.control_snapshot(&session_id).await;
                Ok(CoreResponse::SessionControl {
                    controller,
                    requests,
                    truncated,
                })
            }
            CoreRequest::SessionControlTransfer {
                session_id,
                recipient_principal,
                expected_revision,
                reason,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "session_control_unavailable".to_string(),
                        message: "session control requires a durable database pool".to_string(),
                    });
                };
                if Self::is_controller_bypass(&authority) {
                    return Ok(CoreResponse::Error {
                        code: "session_control_invalid_input".to_string(),
                        message: "local-owner sessions need no control transfer".to_string(),
                    });
                }
                let recipient = match PrincipalId::parse(&recipient_principal) {
                    Ok(principal) => principal,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "session_control_invalid_input".to_string(),
                            message: error.to_string(),
                        });
                    }
                };
                let store = codegg_core::session_control::SessionControllerStore::new(pool.clone());
                let current = match store.get(&session_id).await {
                    Ok(Some(record)) => record,
                    Ok(None) => {
                        return Ok(CoreResponse::Error {
                            code: "session_control_not_found".to_string(),
                            message: "no active controller for this session".to_string(),
                        });
                    }
                    Err(error) => return Ok(Self::control_error(error)),
                };
                if current.controller_principal != *authority.principal_id() {
                    return Ok(CoreResponse::Error {
                        code: "session_control_not_controller".to_string(),
                        message: "only the current controller may transfer control".to_string(),
                    });
                }
                let Some(project) = authz_decision.project_id.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "session_control_not_found".to_string(),
                        message: "no active controller for this session".to_string(),
                    });
                };
                if !self
                    .principal_holds_invoke(&pool, Some(&project), recipient.as_str())
                    .await
                {
                    return Ok(CoreResponse::Error {
                        code: "session_control_invalid_input".to_string(),
                        message: "transfer recipient is not eligible for in-flight control"
                            .to_string(),
                    });
                }
                let now_ms = chrono::Utc::now().timestamp_millis();
                match store
                    .transfer(
                        &session_id,
                        &current.turn_id,
                        expected_revision,
                        &current.controller_principal,
                        &recipient,
                        authority.principal_id(),
                        reason.as_deref(),
                        now_ms,
                    )
                    .await
                {
                    Ok(record) => {
                        self.mirror_controller_to_runtime(
                            &session_id,
                            &record.turn_id,
                            record.controller_principal.as_str(),
                            Some(trusted_client_id),
                            record.revision,
                        )
                        .await;
                        self.publish_control_changed(
                            &session_id,
                            Some(&record.turn_id),
                            Some(record.controller_principal.as_str()),
                            record.revision,
                            "transferred",
                        )
                        .await;
                        self.audit_control_transition(
                            &authority,
                            &authz_decision,
                            &session_id,
                            Some(&record.turn_id),
                            Some(record.controller_principal.as_str()),
                            record.revision,
                            "transferred",
                        )
                        .await;
                        Ok(CoreResponse::SessionControlUpdated {
                            controller: Some(record.to_dto()),
                        })
                    }
                    Err(error) => Ok(Self::control_error(error)),
                }
            }
            CoreRequest::SessionControlRelease {
                session_id,
                expected_revision,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "session_control_unavailable".to_string(),
                        message: "session control requires a durable database pool".to_string(),
                    });
                };
                if Self::is_controller_bypass(&authority) {
                    return Ok(CoreResponse::Error {
                        code: "session_control_invalid_input".to_string(),
                        message: "local-owner sessions need no control release".to_string(),
                    });
                }
                let store = codegg_core::session_control::SessionControllerStore::new(pool);
                let current = match store.get(&session_id).await {
                    Ok(Some(record)) => record,
                    Ok(None) => {
                        return Ok(CoreResponse::Error {
                            code: "session_control_not_found".to_string(),
                            message: "no active controller for this session".to_string(),
                        });
                    }
                    Err(error) => return Ok(Self::control_error(error)),
                };
                if current.controller_principal != *authority.principal_id() {
                    return Ok(CoreResponse::Error {
                        code: "session_control_not_controller".to_string(),
                        message: "only the current controller may release control".to_string(),
                    });
                }
                if current.revision != expected_revision {
                    return Ok(CoreResponse::Error {
                        code: "session_control_conflict".to_string(),
                        message: format!(
                            "stale session control revision: expected {expected_revision}, current {}",
                            current.revision
                        ),
                    });
                }
                let turn_id = current.turn_id.clone();
                match store.release(&session_id, &turn_id).await {
                    Ok(true) => {
                        self.publish_control_changed(
                            &session_id,
                            Some(&turn_id),
                            None,
                            current.revision,
                            "released",
                        )
                        .await;
                        self.audit_control_transition(
                            &authority,
                            &authz_decision,
                            &session_id,
                            Some(&turn_id),
                            None,
                            current.revision,
                            "released",
                        )
                        .await;
                        Ok(CoreResponse::SessionControlUpdated { controller: None })
                    }
                    Ok(false) => Ok(CoreResponse::Error {
                        code: "session_control_not_found".to_string(),
                        message: "no active controller for this session".to_string(),
                    }),
                    Err(error) => Ok(Self::control_error(error)),
                }
            }
            CoreRequest::SessionControlTakeover {
                session_id,
                expected_revision,
                reason,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "session_control_unavailable".to_string(),
                        message: "session control requires a durable database pool".to_string(),
                    });
                };
                if Self::is_controller_bypass(&authority) {
                    return Ok(CoreResponse::Error {
                        code: "session_control_invalid_input".to_string(),
                        message: "local-owner sessions need no control takeover".to_string(),
                    });
                }
                let Some(project) = authz_decision.project_id.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "session_control_not_found".to_string(),
                        message: "no active controller for this session".to_string(),
                    });
                };
                if !self
                    .caller_is_maintainer_or_owner(&pool, &project, &authority)
                    .await
                {
                    return Ok(CoreResponse::Error {
                        code: "session_control_not_controller".to_string(),
                        message: "forced takeover requires maintainer or owner".to_string(),
                    });
                }
                if !self
                    .principal_holds_invoke(
                        &pool,
                        Some(&project),
                        authority.principal_id().as_str(),
                    )
                    .await
                {
                    return Ok(CoreResponse::Error {
                        code: "session_control_not_controller".to_string(),
                        message: "takeover principal is not eligible for in-flight control"
                            .to_string(),
                    });
                }
                let store = codegg_core::session_control::SessionControllerStore::new(pool);
                let current = match store.get(&session_id).await {
                    Ok(Some(record)) => record,
                    Ok(None) => {
                        // Recovery takeover: an active turn with
                        // ambiguous provenance (no lease row, e.g.
                        // pre-lease upgrade) fails closed for ordinary
                        // control but an eligible Maintainer/Owner may
                        // establish a fresh lease with a bounded
                        // reason. `expected_revision` must be 0 for a
                        // fresh lease so retries converge explicitly.
                        if expected_revision != 0 {
                            return Ok(CoreResponse::Error {
                                code: "session_control_conflict".to_string(),
                                message: "no active controller for this session".to_string(),
                            });
                        }
                        let Some(turn_id) = self.active_turn_id_for_session(&session_id).await
                        else {
                            return Ok(CoreResponse::Error {
                                code: "session_control_not_found".to_string(),
                                message: "no active controller for this session".to_string(),
                            });
                        };
                        let validated = match codegg_core::session_control::validate_reason(&reason)
                        {
                            Ok(reason) => reason,
                            Err(error) => return Ok(Self::control_error(error)),
                        };
                        let now_ms = chrono::Utc::now().timestamp_millis();
                        match store
                            .acquire(
                                &session_id,
                                &turn_id,
                                authority.principal_id(),
                                Some(trusted_client_id),
                                now_ms,
                            )
                            .await
                        {
                            Ok(record) => {
                                self.mirror_controller_to_runtime(
                                    &session_id,
                                    &record.turn_id,
                                    record.controller_principal.as_str(),
                                    Some(trusted_client_id),
                                    record.revision,
                                )
                                .await;
                                self.publish_control_changed(
                                    &session_id,
                                    Some(&record.turn_id),
                                    Some(record.controller_principal.as_str()),
                                    record.revision,
                                    "takeover",
                                )
                                .await;
                                self.audit_control_transition(
                                    &authority,
                                    &authz_decision,
                                    &session_id,
                                    Some(&record.turn_id),
                                    Some(record.controller_principal.as_str()),
                                    record.revision,
                                    &format!("takeover:{validated}"),
                                )
                                .await;
                                return Ok(CoreResponse::SessionControlUpdated {
                                    controller: Some(record.to_dto()),
                                });
                            }
                            Err(error) => return Ok(Self::control_error(error)),
                        }
                    }
                    Err(error) => return Ok(Self::control_error(error)),
                };
                let now_ms = chrono::Utc::now().timestamp_millis();
                match store
                    .takeover(
                        &session_id,
                        &current.turn_id,
                        expected_revision,
                        authority.principal_id(),
                        authority.principal_id(),
                        &reason,
                        now_ms,
                    )
                    .await
                {
                    Ok(record) => {
                        self.mirror_controller_to_runtime(
                            &session_id,
                            &record.turn_id,
                            record.controller_principal.as_str(),
                            Some(trusted_client_id),
                            record.revision,
                        )
                        .await;
                        self.publish_control_changed(
                            &session_id,
                            Some(&record.turn_id),
                            Some(record.controller_principal.as_str()),
                            record.revision,
                            "takeover",
                        )
                        .await;
                        self.audit_control_transition(
                            &authority,
                            &authz_decision,
                            &session_id,
                            Some(&record.turn_id),
                            Some(record.controller_principal.as_str()),
                            record.revision,
                            "takeover",
                        )
                        .await;
                        Ok(CoreResponse::SessionControlUpdated {
                            controller: Some(record.to_dto()),
                        })
                    }
                    Err(error) => Ok(Self::control_error(error)),
                }
            }
            other => {
                tracing::warn!("control handler received non-control request");
                let _ = other;
                Ok(CoreResponse::Error {
                    code: "unimplemented".to_string(),
                    message: "This request type is not yet implemented".to_string(),
                })
            }
        }
    }

    /// Current lease plus bounded inert requests for one session.
    async fn control_snapshot(
        &self,
        session_id: &str,
    ) -> (
        Option<codegg_protocol::core::SessionControllerDto>,
        Vec<codegg_protocol::core::SessionControlRequestDto>,
        bool,
    ) {
        // Pool-less daemons project the in-memory handle only.
        let Some(store) = self.controller_store() else {
            if let Some(runtime) = self.sessions.get(session_id) {
                let active = runtime.active_turn.read().await;
                if let Some(handle) = active.as_ref() {
                    if let Some(principal) = handle.controller_principal.clone() {
                        let dto = codegg_protocol::core::SessionControllerDto {
                            session_id: session_id.to_owned(),
                            turn_id: handle.turn_id.clone(),
                            controller_principal: principal,
                            origin_client: handle.controller_client.clone(),
                            revision: handle.controller_revision,
                            created_at_ms: handle.started_at.timestamp_millis(),
                            updated_at_ms: handle.started_at.timestamp_millis(),
                            last_action: "acquired".to_owned(),
                            last_actor: None,
                            last_reason: None,
                        };
                        return (Some(dto), Vec::new(), false);
                    }
                }
            }
            return (None, Vec::new(), false);
        };
        let controller = store
            .get(session_id)
            .await
            .unwrap_or(None)
            .map(|record| record.to_dto());
        let (requests, truncated) = store
            .list_requests(session_id, None)
            .await
            .map(|(rows, truncated)| {
                (
                    rows.into_iter().map(|row| row.to_dto()).collect(),
                    truncated,
                )
            })
            .unwrap_or_default();
        (controller, requests, truncated)
    }

    /// Resolve the active turn id for one session: the in-memory
    /// handle first, then the latest `turn_started` without a terminal
    /// event in the durable log. Returns `None` when no turn is
    /// active (idle sessions have no controller).
    pub(crate) async fn active_turn_id_for_session(&self, session_id: &str) -> Option<String> {
        if let Some(runtime) = self.sessions.get(session_id) {
            let active = runtime.active_turn.read().await;
            if let Some(handle) = active.as_ref() {
                return Some(handle.turn_id.clone());
            }
        }
        let pool = self.pool.clone()?;
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT e1.turn_id FROM core_event_log e1 \
              WHERE e1.event_type = 'turn_started' AND e1.session_id = ? \
              AND e1.turn_id IS NOT NULL \
              AND NOT EXISTS ( \
                  SELECT 1 FROM core_event_log e2 \
                  WHERE e2.session_id = e1.session_id AND e2.turn_id = e1.turn_id \
                  AND (e2.event_type = 'turn_completed' OR e2.event_type = 'turn_failed') \
              ) ORDER BY e1.rowid DESC LIMIT 1",
        )
        .bind(session_id)
        .fetch_optional(&pool)
        .await
        .unwrap_or(None);
        row.map(|(turn_id,)| turn_id)
    }

    /// Restart/upgrade reconciliation for the controller domain.
    ///
    /// Runs inside [`CoreDaemon::recover_state`] after interrupted
    /// turns are marked failed:
    /// - leases whose turn now has a terminal event are released
    ///   (stale rows never survive their turn);
    /// - orphan leases with no `turn_started` are released;
    /// - active turns with no lease derive controller identity only
    ///   from trustworthy stored origin attribution
    ///   (`origin_attribution` scope `turn`, non-legacy, still
    ///   eligible); otherwise control fails closed and an explicit
    ///   Maintainer/Owner takeover is required.
    pub async fn reconcile_session_controllers(&self, interrupted: &[(String, String)]) {
        let Some(store) = self.controller_store() else {
            return;
        };
        // Terminal transitions win: release every interrupted turn's
        // lease (the caller already emitted TurnFailed for these).
        for (session_id, turn_id) in interrupted {
            self.release_turn_controller(session_id, turn_id).await;
        }
        // Drop stale leases whose turn has since reached terminal, and
        // orphan leases with no turn_started at all.
        let active = match store.list_active(1000).await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(error = %error, "session control reconciliation list failed");
                return;
            }
        };
        let Some(pool) = self.pool.clone() else {
            return;
        };
        for record in active {
            let terminal: Option<(i64,)> = sqlx::query_as(
                "SELECT 1 FROM core_event_log WHERE session_id = ? AND turn_id = ? \
                  AND (event_type = 'turn_completed' OR event_type = 'turn_failed') LIMIT 1",
            )
            .bind(&record.session_id)
            .bind(&record.turn_id)
            .fetch_optional(&pool)
            .await
            .unwrap_or(None);
            if terminal.is_some() {
                self.release_turn_controller(&record.session_id, &record.turn_id)
                    .await;
                continue;
            }
            let started: Option<(i64,)> = sqlx::query_as(
                "SELECT 1 FROM core_event_log WHERE session_id = ? AND turn_id = ? \
                  AND event_type = 'turn_started' LIMIT 1",
            )
            .bind(&record.session_id)
            .bind(&record.turn_id)
            .fetch_optional(&pool)
            .await
            .unwrap_or(None);
            if started.is_none() {
                self.release_turn_controller(&record.session_id, &record.turn_id)
                    .await;
            }
        }
        // Upgrade derivation: active turns without a lease gain one
        // only from trustworthy origin attribution.
        let unleased: Vec<(String, String)> = sqlx::query_as(
            "SELECT DISTINCT e1.session_id, e1.turn_id FROM core_event_log e1 \
              WHERE e1.event_type = 'turn_started' AND e1.turn_id IS NOT NULL \
              AND NOT EXISTS ( \
                  SELECT 1 FROM core_event_log e2 \
                  WHERE e2.session_id = e1.session_id AND e2.turn_id = e1.turn_id \
                  AND (e2.event_type = 'turn_completed' OR e2.event_type = 'turn_failed') \
              )",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        for (session_id, turn_id) in unleased {
            if store.get(&session_id).await.unwrap_or(None).is_some() {
                continue;
            }
            let attribution_store =
                codegg_core::authorization::OriginAttributionStore::new(pool.clone());
            let attribution = attribution_store
                .get("turn", &turn_id)
                .await
                .unwrap_or(None);
            let Some(attribution) = attribution else {
                continue;
            };
            if attribution.is_legacy() {
                continue;
            }
            let Some(project) = self.session_project(&pool, &session_id).await else {
                continue;
            };
            if !self
                .principal_holds_invoke(
                    &pool,
                    Some(&project),
                    attribution.origin_principal.as_str(),
                )
                .await
            {
                continue;
            }
            let now_ms = chrono::Utc::now().timestamp_millis();
            if let Ok(record) = store
                .acquire(
                    &session_id,
                    &turn_id,
                    &attribution.origin_principal,
                    None,
                    now_ms,
                )
                .await
            {
                self.publish_control_changed(
                    &session_id,
                    Some(&turn_id),
                    Some(record.controller_principal.as_str()),
                    record.revision,
                    "acquired",
                )
                .await;
            }
        }
    }
}
