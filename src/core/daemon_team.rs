//! Team Collaboration Corrective M003: `team` request family for `CoreDaemon`.
//!
//! Canonical membership management and LocalOwner principal/device-token
//! provisioning over the existing [`codegg_core::team::TeamStore`] and
//! [`codegg_core::transport_auth::PersonalTokenStore`]. Introduces no new
//! identity store, no password/OIDC flow, and no remote secret-delivery
//! protocol: `TeamTokenCreate` is secret-bearing (denied on the remote
//! WebSocket transport) so issuance stays on the secret-safe local surface.
//!
//! Authorization (gate) runs before this handler:
//! membership operations are `DirectProject + member.manage` (a project
//! Owner manages only their own project); principal/token operations are
//! `Opaque + project.configure` so ordinary team principals fail closed
//! and only LocalOwner broad policy passes. Every arm below re-resolves
//! durable state at request time so revocation between gate and dispatch
//! still fails closed where the store enforces it.
//!
//! Secrecy: token digests are never read out of the store rows in this
//! module (list/create/revoke map only id/prefix/label/timestamps into
//! [`TeamTokenDto`](codegg_protocol::core::TeamTokenDto)). The one-time
//! `cggt_...` plaintext is returned exactly once inside
//! `CoreResponse::TeamTokenCreated` and never enters audit metadata,
//! events, logs, or chat. Audit uses `membership_change` (ids/roles/
//! revisions) and `authentication` (method/transport/kind) only.

use codegg_core::identity::{PrincipalId, ProjectId};
use codegg_core::team::{MembershipState, PrincipalKind, PrincipalStatus, ProjectRole, TeamError};
use codegg_core::transport_auth::TransportAuthError;

use crate::error::AppError;
use crate::protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, TeamCapabilitiesDto, TeamMembershipDto, TeamPrincipalDto,
    TeamTokenDto, TEAM_CAPABILITY, TEAM_MAX_LIST_LIMIT, TEAM_PROTOCOL_VERSION,
};

use super::daemon::CoreDaemon;

macro_rules! ok_or_response {
    ($expr:expr) => {
        match $expr {
            Ok(value) => value,
            Err(response) => return Ok(*response),
        }
    };
}

impl CoreDaemon {
    /// `true` for the bounded team administration family, which dispatches
    /// through the boxed [`Self::handle_team_request`] helper instead of
    /// the giant dispatch match.
    pub(crate) fn is_team_request(request: &CoreRequest) -> bool {
        matches!(
            request,
            CoreRequest::TeamCapabilities
                | CoreRequest::TeamMembershipList { .. }
                | CoreRequest::TeamMembershipAdd { .. }
                | CoreRequest::TeamMembershipUpdate { .. }
                | CoreRequest::TeamMembershipRevoke { .. }
                | CoreRequest::TeamPrincipalList { .. }
                | CoreRequest::TeamPrincipalCreate { .. }
                | CoreRequest::TeamPrincipalStatusSet { .. }
                | CoreRequest::TeamTokenList { .. }
                | CoreRequest::TeamTokenCreate { .. }
                | CoreRequest::TeamTokenRevoke { .. }
        )
    }

    /// `true` for team operations that mutate durable state. They skip the
    /// pre-side-effect audit emit and are recorded post-mutation with
    /// their durable ids and revisions (see `after_team_*` helpers).
    pub(crate) fn is_team_mutation(request: &CoreRequest) -> bool {
        matches!(
            request,
            CoreRequest::TeamMembershipAdd { .. }
                | CoreRequest::TeamMembershipUpdate { .. }
                | CoreRequest::TeamMembershipRevoke { .. }
                | CoreRequest::TeamPrincipalCreate { .. }
                | CoreRequest::TeamPrincipalStatusSet { .. }
                | CoreRequest::TeamTokenCreate { .. }
                | CoreRequest::TeamTokenRevoke { .. }
        )
    }

    /// Team administration request handler.
    ///
    /// Principals come from transport authority, never from the payload
    /// except the explicit target ids of an admin row. Token plaintext is
    /// handled exactly once (create path); no other arm reads secrets.
    pub(crate) async fn handle_team_request(
        &self,
        _request_id: &str,
        request: CoreRequest,
        _trusted_client_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        match request {
            CoreRequest::TeamCapabilities => Ok(CoreResponse::TeamCapabilities {
                capabilities: TeamCapabilitiesDto {
                    supported: true,
                    protocol_version: TEAM_PROTOCOL_VERSION,
                    max_list_limit: TEAM_MAX_LIST_LIMIT,
                },
            }),
            CoreRequest::TeamMembershipList { project_id, limit } => {
                let project = ok_or_response!(parse_project(&project_id));
                let Some(pool) = self.pool.clone() else {
                    return Ok(team_unavailable());
                };
                let team = codegg_core::team::TeamStore::new(pool);
                match team.list_memberships_for_project(&project).await {
                    Ok(rows) => {
                        let bound = bound_limit(limit);
                        let truncated = rows.len() > bound;
                        let memberships = rows
                            .into_iter()
                            .take(bound)
                            .map(membership_to_dto)
                            .collect();
                        Ok(CoreResponse::TeamMembershipList {
                            memberships,
                            truncated,
                        })
                    }
                    Err(error) => Ok(team_error(error)),
                }
            }
            CoreRequest::TeamMembershipAdd {
                project_id,
                principal_id,
                role,
            } => {
                let project = ok_or_response!(parse_project(&project_id));
                let target = ok_or_response!(parse_principal(&principal_id));
                let role = ok_or_response!(parse_role(&role));
                let Some(pool) = self.pool.clone() else {
                    return Ok(team_unavailable());
                };
                let team = codegg_core::team::TeamStore::new(pool);
                match team.create_membership(&project, &target, role).await {
                    Ok(record) => {
                        let revision = record.revision;
                        let dto = membership_to_dto(record);
                        self.after_membership_mutation(
                            authority,
                            authz_decision,
                            &project,
                            &target,
                            dto.role.as_str(),
                            revision,
                        )
                        .await;
                        self.event_log
                            .publish(
                                None,
                                None,
                                CoreEvent::TeamMembershipChanged {
                                    project_id: project.as_str().to_owned(),
                                    principal_id: target.as_str().to_owned(),
                                    revision,
                                },
                            )
                            .await;
                        Ok(CoreResponse::TeamMembership { membership: dto })
                    }
                    Err(error) => Ok(team_error(error)),
                }
            }
            CoreRequest::TeamMembershipUpdate {
                project_id,
                principal_id,
                expected_revision,
                role,
                state,
            } => {
                let project = ok_or_response!(parse_project(&project_id));
                let target = ok_or_response!(parse_principal(&principal_id));
                let role = ok_or_response!(role.as_deref().map(parse_role).transpose());
                let state = ok_or_response!(state.as_deref().map(parse_update_state).transpose());
                let Some(pool) = self.pool.clone() else {
                    return Ok(team_unavailable());
                };
                let team = codegg_core::team::TeamStore::new(pool);
                match team
                    .update_membership(&project, &target, expected_revision, role, state)
                    .await
                {
                    Ok(record) => {
                        let revision = record.revision;
                        let dto = membership_to_dto(record);
                        self.after_membership_mutation(
                            authority,
                            authz_decision,
                            &project,
                            &target,
                            dto.role.as_str(),
                            revision,
                        )
                        .await;
                        self.event_log
                            .publish(
                                None,
                                None,
                                CoreEvent::TeamMembershipChanged {
                                    project_id: project.as_str().to_owned(),
                                    principal_id: target.as_str().to_owned(),
                                    revision,
                                },
                            )
                            .await;
                        Ok(CoreResponse::TeamMembership { membership: dto })
                    }
                    Err(error) => Ok(team_error(error)),
                }
            }
            CoreRequest::TeamMembershipRevoke {
                project_id,
                principal_id,
                expected_revision,
            } => {
                let project = ok_or_response!(parse_project(&project_id));
                let target = ok_or_response!(parse_principal(&principal_id));
                let Some(pool) = self.pool.clone() else {
                    return Ok(team_unavailable());
                };
                let team = codegg_core::team::TeamStore::new(pool);
                match team
                    .revoke_membership(&project, &target, expected_revision)
                    .await
                {
                    Ok(record) => {
                        let revision = record.revision;
                        let dto = membership_to_dto(record);
                        self.after_membership_mutation(
                            authority,
                            authz_decision,
                            &project,
                            &target,
                            dto.role.as_str(),
                            revision,
                        )
                        .await;
                        self.event_log
                            .publish(
                                None,
                                None,
                                CoreEvent::TeamMembershipChanged {
                                    project_id: project.as_str().to_owned(),
                                    principal_id: target.as_str().to_owned(),
                                    revision,
                                },
                            )
                            .await;
                        Ok(CoreResponse::TeamMembership { membership: dto })
                    }
                    Err(error) => Ok(team_error(error)),
                }
            }
            CoreRequest::TeamPrincipalList { limit } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(team_unavailable());
                };
                let team = codegg_core::team::TeamStore::new(pool);
                match team.list_principals().await {
                    Ok(rows) => {
                        let bound = bound_limit(limit);
                        let truncated = rows.len() > bound;
                        let principals =
                            rows.into_iter().take(bound).map(principal_to_dto).collect();
                        Ok(CoreResponse::TeamPrincipalList {
                            principals,
                            truncated,
                        })
                    }
                    Err(error) => Ok(team_error(error)),
                }
            }
            CoreRequest::TeamPrincipalCreate { kind, display_name } => {
                let kind = ok_or_response!(parse_principal_kind(kind.as_deref()));
                let Some(pool) = self.pool.clone() else {
                    return Ok(team_unavailable());
                };
                let team = codegg_core::team::TeamStore::new(pool);
                match team.create_principal(kind, &display_name).await {
                    Ok(record) => {
                        let revision = record.revision;
                        let id = record.id.clone();
                        let dto = principal_to_dto(record);
                        self.after_principal_mutation(
                            authority,
                            authz_decision,
                            &id,
                            dto.kind.as_str(),
                            revision,
                        )
                        .await;
                        self.event_log
                            .publish(
                                None,
                                None,
                                CoreEvent::TeamPrincipalChanged {
                                    principal_id: id.as_str().to_owned(),
                                    revision,
                                },
                            )
                            .await;
                        Ok(CoreResponse::TeamPrincipal { principal: dto })
                    }
                    Err(error) => Ok(team_error(error)),
                }
            }
            CoreRequest::TeamPrincipalStatusSet {
                principal_id,
                status,
                expected_revision,
            } => {
                let target = ok_or_response!(parse_principal(&principal_id));
                let status = ok_or_response!(parse_principal_status(&status));
                let Some(pool) = self.pool.clone() else {
                    return Ok(team_unavailable());
                };
                let team = codegg_core::team::TeamStore::new(pool);
                match team
                    .set_principal_status(&target, expected_revision, status)
                    .await
                {
                    Ok(record) => {
                        let revision = record.revision;
                        let dto = principal_to_dto(record);
                        self.after_principal_mutation(
                            authority,
                            authz_decision,
                            &target,
                            dto.kind.as_str(),
                            revision,
                        )
                        .await;
                        self.event_log
                            .publish(
                                None,
                                None,
                                CoreEvent::TeamPrincipalChanged {
                                    principal_id: target.as_str().to_owned(),
                                    revision,
                                },
                            )
                            .await;
                        Ok(CoreResponse::TeamPrincipal { principal: dto })
                    }
                    Err(error) => Ok(team_error(error)),
                }
            }
            CoreRequest::TeamTokenList { principal_id } => {
                let target = ok_or_response!(parse_principal(&principal_id));
                let Some(pool) = self.pool.clone() else {
                    return Ok(team_unavailable());
                };
                let team = codegg_core::team::TeamStore::new(pool.clone());
                let tokens = codegg_core::transport_auth::PersonalTokenStore::with_team(pool, team);
                match tokens.list_tokens_for_principal(&target).await {
                    Ok(rows) => {
                        let bound = TEAM_MAX_LIST_LIMIT;
                        let truncated = rows.len() > bound;
                        let list: Vec<TeamTokenDto> =
                            rows.into_iter().take(bound).map(token_to_dto).collect();
                        Ok(CoreResponse::TeamTokenList {
                            tokens: list,
                            truncated,
                        })
                    }
                    Err(error) => Ok(token_error(error)),
                }
            }
            CoreRequest::TeamTokenCreate {
                principal_id,
                label,
                expires_at_ms,
                idempotency_key,
            } => {
                let target = ok_or_response!(parse_principal(&principal_id));
                ok_or_response!(validate_expiry(expires_at_ms));
                ok_or_response!(validate_idempotency_key(idempotency_key.as_deref()));
                let Some(pool) = self.pool.clone() else {
                    return Ok(team_unavailable());
                };
                let team = codegg_core::team::TeamStore::new(pool.clone());
                let tokens = codegg_core::transport_auth::PersonalTokenStore::with_team(pool, team);
                match tokens
                    .create_personal_token(&target, &label, expires_at_ms)
                    .await
                {
                    Ok((plaintext, record)) => {
                        let dto = token_to_dto(record);
                        self.after_token_mutation(authority, authz_decision, false)
                            .await;
                        self.event_log
                            .publish(
                                None,
                                None,
                                CoreEvent::TeamTokenChanged {
                                    principal_id: dto.principal_id.clone(),
                                    token_id: dto.token_id.clone(),
                                    revoked: false,
                                },
                            )
                            .await;
                        // The plaintext is returned exactly once here. It
                        // is never logged, audited, or stored: the audit
                        // arm above carries method/transport/kind only and
                        // the event carries ids plus the minted flag.
                        Ok(CoreResponse::TeamTokenCreated {
                            token: dto,
                            plaintext,
                        })
                    }
                    Err(error) => Ok(token_error(error)),
                }
            }
            CoreRequest::TeamTokenRevoke { token_id } => {
                if token_id.trim().is_empty() || token_id.len() > 128 {
                    return Ok(invalid_input("token id must be non-empty and bounded"));
                }
                let Some(pool) = self.pool.clone() else {
                    return Ok(team_unavailable());
                };
                let team = codegg_core::team::TeamStore::new(pool.clone());
                let tokens = codegg_core::transport_auth::PersonalTokenStore::with_team(pool, team);
                match tokens.revoke_personal_token(&token_id).await {
                    Ok(record) => {
                        let dto = token_to_dto(record);
                        self.after_token_mutation(authority, authz_decision, true)
                            .await;
                        self.event_log
                            .publish(
                                None,
                                None,
                                CoreEvent::TeamTokenChanged {
                                    principal_id: dto.principal_id.clone(),
                                    token_id: dto.token_id.clone(),
                                    revoked: true,
                                },
                            )
                            .await;
                        Ok(CoreResponse::TeamToken { token: dto })
                    }
                    Err(error) => Ok(token_error(error)),
                }
            }
            other => {
                tracing::warn!("team handler received non-team request");
                let _ = other;
                Ok(CoreResponse::Error {
                    code: "unimplemented".to_string(),
                    message: "This request type is not yet implemented".to_string(),
                })
            }
        }
    }

    /// Post-mutation evidence for one membership write: structural audit
    /// (`member.principal/role/revision`, never secrets) plus the
    /// structural liveness event published by the caller.
    async fn after_membership_mutation(
        &self,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
        project: &ProjectId,
        member: &PrincipalId,
        role: &str,
        revision: u64,
    ) {
        use codegg_core::audit_instrumentation as instr;
        let provenance = codegg_core::authorization::audit_provenance(authz_decision);
        let mut chain = instr::AuditChainContext::new();
        chain.project = Some(project.clone());
        let builder = instr::membership_change_event(
            authority.principal(),
            &provenance,
            &chain,
            member.as_str(),
            role,
            Some(revision),
        );
        Box::pin(self.append_audit_event(builder)).await;
    }

    /// Post-mutation evidence for one principal write: structural audit
    /// with the affected principal id, kind, and revision only.
    async fn after_principal_mutation(
        &self,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
        member: &PrincipalId,
        kind: &str,
        revision: u64,
    ) {
        use codegg_core::audit_instrumentation as instr;
        let provenance = codegg_core::authorization::audit_provenance(authz_decision);
        let chain = instr::AuditChainContext::new();
        let builder = instr::membership_change_event(
            authority.principal(),
            &provenance,
            &chain,
            member.as_str(),
            kind,
            Some(revision),
        );
        Box::pin(self.append_audit_event(builder)).await;
    }

    /// Post-mutation evidence for one token mint/revoke: structural
    /// authentication event (method/transport/kind only). Token digests
    /// and plaintext never enter audit metadata.
    async fn after_token_mutation(
        &self,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
        revoked: bool,
    ) {
        use codegg_core::audit_instrumentation as instr;
        let provenance = codegg_core::authorization::audit_provenance(authz_decision);
        let chain = instr::AuditChainContext::new();
        let outcome = if revoked { "revoked" } else { "issued" };
        let builder =
            instr::authentication_event(authority.principal(), &provenance, &chain, outcome);
        Box::pin(self.append_audit_event(builder)).await;
    }
}

// ── DTO conversion (metadata only; never secrets) ──────────────────────────

fn principal_to_dto(record: codegg_core::team::PrincipalRecord) -> TeamPrincipalDto {
    TeamPrincipalDto {
        principal_id: record.id.as_str().to_owned(),
        kind: record.kind.as_str().to_owned(),
        display_name: record.display_name,
        status: record.status.as_str().to_owned(),
        revision: record.revision,
        created_at_ms: record.created_at,
        updated_at_ms: record.updated_at,
    }
}

fn membership_to_dto(record: codegg_core::team::MembershipRecord) -> TeamMembershipDto {
    TeamMembershipDto {
        project_id: record.project_id.as_str().to_owned(),
        principal_id: record.principal_id.as_str().to_owned(),
        role: record.role.as_str().to_owned(),
        state: record.state.as_str().to_owned(),
        revision: record.revision,
        created_at_ms: record.created_at,
        updated_at_ms: record.updated_at,
    }
}

fn token_to_dto(record: codegg_core::transport_auth::PersonalTokenRecord) -> TeamTokenDto {
    TeamTokenDto {
        token_id: record.token_id,
        principal_id: record.principal_id.as_str().to_owned(),
        token_prefix: record.token_prefix,
        label: record.label,
        created_at_ms: record.created_at,
        expires_at_ms: record.expires_at,
        revoked_at_ms: record.revoked_at,
    }
}

// ── Request validation ─────────────────────────────────────────────────────

fn bound_limit(limit: Option<usize>) -> usize {
    match limit {
        Some(0) | None => TEAM_MAX_LIST_LIMIT,
        Some(n) => n.clamp(1, TEAM_MAX_LIST_LIMIT),
    }
}

fn parse_project(raw: &str) -> Result<ProjectId, Box<CoreResponse>> {
    ProjectId::parse(raw).map_err(|error| {
        Box::new(CoreResponse::Error {
            code: "team_invalid_input".to_owned(),
            message: error.to_string(),
        })
    })
}

fn parse_principal(raw: &str) -> Result<PrincipalId, Box<CoreResponse>> {
    PrincipalId::parse(raw).map_err(|error| {
        Box::new(CoreResponse::Error {
            code: "team_invalid_input".to_owned(),
            message: error.to_string(),
        })
    })
}

fn parse_role(raw: &str) -> Result<ProjectRole, Box<CoreResponse>> {
    ProjectRole::parse(raw).map_err(|_| {
        Box::new(CoreResponse::Error {
            code: "team_invalid_input".to_owned(),
            message: format!(
                "unknown project role {raw:?}; expected viewer|contributor|maintainer|owner"
            ),
        })
    })
}

/// Update-path membership states. Revocation has its own explicit
/// operation so a generic update can never silently revoke: `revoked`
/// here is rejected with guidance.
fn parse_update_state(raw: &str) -> Result<MembershipState, Box<CoreResponse>> {
    match raw {
        "active" => Ok(MembershipState::Active),
        "suspended" => Ok(MembershipState::Suspended),
        "revoked" => Err(Box::new(CoreResponse::Error {
            code: "team_invalid_input".to_owned(),
            message: "use team_membership_revoke to revoke; update accepts active|suspended"
                .to_owned(),
        })),
        _ => Err(Box::new(CoreResponse::Error {
            code: "team_invalid_input".to_owned(),
            message: format!("unknown membership state {raw:?}; expected active|suspended"),
        })),
    }
}

/// Creatable principal kinds. Node and local-owner kinds are never
/// minted through team administration.
fn parse_principal_kind(raw: Option<&str>) -> Result<PrincipalKind, Box<CoreResponse>> {
    match raw.unwrap_or("human") {
        "human" => Ok(PrincipalKind::Human),
        "service_account" => Ok(PrincipalKind::ServiceAccount),
        _ => Err(Box::new(CoreResponse::Error {
            code: "team_invalid_input".to_owned(),
            message: "unknown principal kind; expected human|service_account".to_owned(),
        })),
    }
}

fn parse_principal_status(raw: &str) -> Result<PrincipalStatus, Box<CoreResponse>> {
    match raw {
        "active" => Ok(PrincipalStatus::Active),
        "disabled" => Ok(PrincipalStatus::Disabled),
        _ => Err(Box::new(CoreResponse::Error {
            code: "team_invalid_input".to_owned(),
            message: format!("unknown principal status {raw:?}; expected active|disabled"),
        })),
    }
}

fn validate_expiry(expires_at_ms: Option<i64>) -> Result<(), Box<CoreResponse>> {
    if let Some(expires) = expires_at_ms {
        let now = chrono::Utc::now().timestamp_millis();
        if expires <= now {
            return Err(Box::new(CoreResponse::Error {
                code: "team_invalid_input".to_owned(),
                message: "token expiry must be in the future".to_owned(),
            }));
        }
    }
    Ok(())
}

/// Idempotency keys are bounded opaque client tokens. The daemon
/// validates shape only: each explicit `TeamTokenCreate` mints one
/// credential, so after an ambiguous transport failure callers must
/// reconcile through `TeamTokenList` and require explicit user
/// confirmation before re-issuing rather than silently retrying.
fn validate_idempotency_key(raw: Option<&str>) -> Result<(), Box<CoreResponse>> {
    if let Some(key) = raw {
        if key.trim().is_empty() || key.len() > 128 || key.chars().any(char::is_control) {
            return Err(Box::new(CoreResponse::Error {
                code: "team_invalid_input".to_owned(),
                message: "idempotency key must be non-empty, bounded, and control-free".to_owned(),
            }));
        }
    }
    Ok(())
}

// ── Error mapping (secret-free, privacy-safe) ──────────────────────────────

fn invalid_input(message: impl Into<String>) -> CoreResponse {
    CoreResponse::Error {
        code: "team_invalid_input".to_owned(),
        message: message.into(),
    }
}

fn team_unavailable() -> CoreResponse {
    CoreResponse::Error {
        code: "team_unavailable".to_owned(),
        message: "team administration requires a durable database pool".to_owned(),
    }
}

fn team_error(error: TeamError) -> CoreResponse {
    let (code, message) = match &error {
        TeamError::Invalid(message) => ("team_invalid_input", message.clone()),
        TeamError::UnknownRole(_) | TeamError::UnknownCapability(_) => {
            ("team_invalid_input", error.to_string())
        }
        TeamError::Identity(_) => ("team_invalid_input", error.to_string()),
        TeamError::PrincipalNotFound(_) => ("team_principal_not_found", error.to_string()),
        TeamError::MembershipNotFound { .. } => ("team_membership_not_found", error.to_string()),
        TeamError::PrincipalConflict(_) => ("team_principal_conflict", error.to_string()),
        TeamError::MembershipConflict { .. } => ("team_membership_conflict", error.to_string()),
        TeamError::RevisionConflict { .. } | TeamError::PrincipalRevisionConflict { .. } => {
            ("team_revision_conflict", error.to_string())
        }
        TeamError::Storage(_) => ("team_unavailable", error.to_string()),
    };
    CoreResponse::Error {
        code: code.to_owned(),
        message,
    }
}

fn token_error(error: TransportAuthError) -> CoreResponse {
    let (code, message) = match &error {
        TransportAuthError::Invalid(message) => ("team_invalid_input", message.clone()),
        TransportAuthError::UnknownToken => (
            "team_token_not_found",
            "personal token not found".to_owned(),
        ),
        TransportAuthError::Revoked => (
            "team_token_not_found",
            "personal token not found".to_owned(),
        ),
        TransportAuthError::Expired => (
            "team_token_not_found",
            "personal token not found".to_owned(),
        ),
        TransportAuthError::VerificationFailed => (
            "team_token_not_found",
            "personal token not found".to_owned(),
        ),
        TransportAuthError::PrincipalNotActive => (
            "team_principal_not_found",
            "token principal is not active".to_owned(),
        ),
        TransportAuthError::Team(_) | TransportAuthError::Storage(_) => {
            return match error {
                TransportAuthError::Team(team) => team_error(team),
                TransportAuthError::Storage(_) => team_unavailable(),
                _ => team_unavailable(),
            };
        }
    };
    CoreResponse::Error {
        code: code.to_owned(),
        message,
    }
}

/// Protocol capability advertisement for the team surface.
pub fn team_capabilities_dto() -> TeamCapabilitiesDto {
    TeamCapabilitiesDto {
        supported: true,
        protocol_version: TEAM_PROTOCOL_VERSION,
        max_list_limit: TEAM_MAX_LIST_LIMIT,
    }
}

/// `true` when the capability string names this surface (used by TUI
/// negotiation copy).
pub fn team_capability_name() -> &'static str {
    TEAM_CAPABILITY
}
