//! Human finite shell requests run on the daemon-owned workspace host.

use crate::core::daemon::CoreDaemon;
use crate::error::AppError;
use crate::protocol::core::{CoreRequest, CoreResponse, RequestEnvelope};
use codegg_core::project_storage::{BindingStatus, ProjectStorage};
use sha2::Digest;

impl CoreDaemon {
    pub(crate) async fn handle_human_shell_request(
        &self,
        request: RequestEnvelope<CoreRequest>,
        _trusted_client_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        decision: &codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        match request.payload {
            CoreRequest::HumanShellRunCapabilities => Ok(CoreResponse::HumanShellRunCapabilities {
                capabilities: codegg_protocol::human_shell::HumanShellRunCapabilitiesDto {
                    supported: true,
                    protocol_version: codegg_protocol::human_shell::HUMAN_SHELL_PROTOCOL_VERSION,
                    max_command_bytes: codegg_protocol::human_shell::HUMAN_SHELL_MAX_COMMAND_BYTES,
                    max_read_bytes: codegg_protocol::human_shell::HUMAN_SHELL_MAX_READ_BYTES,
                    timeout_secs: 300,
                    max_runs_per_principal: 8,
                },
            }),
            CoreRequest::HumanShellRunCreate { request: dto } => {
                if dto.project_id.is_empty() || dto.workspace_id.is_empty() {
                    return Ok(error(
                        "human_shell_invalid_request",
                        "project and workspace are required",
                    ));
                }
                let pool = self.pool.clone();
                let poolless_owner = pool.is_none()
                    && authority.principal().auth_method()
                        == codegg_core::transport_auth::AuthMethod::LocalOwner;
                if pool.is_none() && !poolless_owner {
                    return Ok(error(
                        "human_shell_policy_unavailable",
                        "execution authority is unavailable",
                    ));
                }
                let workspace_id =
                    match codegg_core::workspace::WorkspaceId::parse(&dto.workspace_id) {
                        Ok(id) => id,
                        Err(_) => {
                            return Ok(error(
                                "human_shell_workspace_unavailable",
                                "workspace is unavailable",
                            ))
                        }
                    };
                let Some(workspace) = self.workspaces.resolve(&workspace_id).await else {
                    return Ok(error(
                        "human_shell_workspace_unavailable",
                        "workspace is unavailable",
                    ));
                };
                if workspace.is_archived() {
                    return Ok(error(
                        "human_shell_workspace_unavailable",
                        "workspace is unavailable",
                    ));
                }
                if let Some(pool) = pool.as_ref() {
                    let storage = ProjectStorage::new(pool.clone());
                    let binding = match storage.workspace_binding(&workspace_id).await {
                        Ok(Some(binding)) if binding.status == BindingStatus::Resolved => binding,
                        _ => {
                            return Ok(error(
                                "human_shell_workspace_unavailable",
                                "workspace has no resolved project binding",
                            ))
                        }
                    };
                    if binding.project_id.as_str() != dto.project_id {
                        return Ok(error(
                            "human_shell_scope_mismatch",
                            "workspace is not bound to the authorized project",
                        ));
                    }
                    if binding.node_id.is_some() {
                        return Ok(error(
                            "human_shell_remote_target_unsupported",
                            "workspace execution on a remote node is not supported",
                        ));
                    }
                    if let Some(session_id) = dto.session_id.as_deref() {
                        let bound = storage.session_binding(session_id).await;
                        if !matches!(bound, Ok(Some(ref row)) if row.status == BindingStatus::Resolved
                            && row.project_id.as_ref().is_some_and(|id| id.as_str() == dto.project_id)
                            && row.workspace_id.as_ref().is_some_and(|id| id == &workspace_id))
                        {
                            return Ok(error(
                                "human_shell_scope_mismatch",
                                "session is not bound to the requested project and workspace",
                            ));
                        }
                    }
                } else if dto.session_id.is_some() {
                    return Ok(error(
                        "human_shell_scope_mismatch",
                        "poolless standalone runs cannot be attached to a durable session",
                    ));
                }
                match crate::shell::policy::evaluate_command(&dto.command) {
                    crate::shell::policy::HumanShellPolicyDecision::Block { reason } => {
                        return Ok(error("human_shell_policy_blocked", &reason));
                    }
                    crate::shell::policy::HumanShellPolicyDecision::Warn { .. }
                        if !dto.confirmed_dangerous =>
                    {
                        return Ok(error(
                            "human_shell_confirmation_required",
                            "this command requires an explicit human confirmation",
                        ));
                    }
                    _ => {}
                }
                let principal = authority.principal_id().as_str().to_owned();
                let profile = match pool {
                    Some(pool) => match codegg_core::approval::RuntimePreferenceStore::new(pool)
                        .get(&principal)
                        .await
                    {
                        Ok(Some(pref)) => pref.effective_sandbox_profile(),
                        Ok(None) => codegg_core::approval::SandboxProfile::default(),
                        Err(pref_error) => {
                            tracing::warn!(%pref_error, "human shell sandbox preference lookup failed");
                            return Ok(error(
                                "human_shell_policy_unavailable",
                                "execution policy is unavailable",
                            ));
                        }
                    },
                    None => codegg_core::approval::SandboxProfile::default(),
                };
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_owned());
                let sandbox = match crate::security::sandbox::sandbox_config_for_profile(
                    profile,
                    &workspace.canonical_root,
                ) {
                    None => None,
                    Some(config) => {
                        let args = vec!["-lc".to_owned(), dto.command.clone()];
                        match config.launch_spec(&shell, &args, Some(&workspace.canonical_root)) {
                            Ok(spec) => Some(spec),
                            Err(sandbox_error) => {
                                return Ok(error(
                                    "human_shell_sandbox_unavailable",
                                    &sandbox_error.to_string(),
                                ))
                            }
                        }
                    }
                };
                let scope = crate::shell::daemon::HumanShellRunScope {
                    principal_id: principal,
                    project_id: dto.project_id.clone(),
                    workspace_id: dto.workspace_id.clone(),
                    session_id: dto.session_id.clone(),
                };
                let origin = dto.origin;
                let command_digest = hex::encode(sha2::Sha256::digest(dto.command.as_bytes()));
                let audit_request = CoreRequest::HumanShellRunCreate {
                    request: dto.clone(),
                };
                let response = self
                    .human_shell_runs
                    .create(
                        scope,
                        dto,
                        workspace.canonical_root.clone(),
                        format!("daemon:{}", self.daemon_id),
                        shell,
                        sandbox,
                    )
                    .await;
                let duplicate = matches!(
                    &response,
                    CoreResponse::HumanShellRunCreated {
                        duplicate: true,
                        ..
                    }
                );
                let admitted = matches!(&response, CoreResponse::HumanShellRunCreated { .. });
                if !duplicate {
                    let context =
                        Self::execution_audit_context(authority, decision, &audit_request);
                    self.audit_emitter()
                        .emit_with(&context, |principal, provenance, chain| {
                            codegg_core::audit_instrumentation::human_shell_run_event(
                                principal,
                                provenance,
                                chain,
                                Some(&command_digest),
                                match origin {
                                    codegg_protocol::human_shell::HumanShellOriginDto::Bang => {
                                        "bang"
                                    }
                                    codegg_protocol::human_shell::HumanShellOriginDto::BangBang => {
                                        "bangbang"
                                    }
                                },
                                if admitted { "admitted" } else { "refused" },
                            )
                        })
                        .await;
                }
                Ok(response)
            }
            CoreRequest::HumanShellRunRead { request: dto } => {
                if let Err(response) = validate_workspace_scope(
                    self,
                    authority,
                    &dto.project_id,
                    &dto.workspace_id,
                    dto.session_id.as_deref(),
                )
                .await
                {
                    return Ok(response);
                }
                let scope = crate::shell::daemon::HumanShellRunScope {
                    principal_id: authority.principal_id().as_str().to_owned(),
                    project_id: dto.project_id.clone(),
                    workspace_id: dto.workspace_id.clone(),
                    session_id: dto.session_id.clone(),
                };
                Ok(self
                    .human_shell_runs
                    .read(&scope, &dto.run_id, dto.from_sequence, dto.max_bytes)
                    .await)
            }
            CoreRequest::HumanShellRunCancel { request: dto } => {
                if let Err(response) = validate_workspace_scope(
                    self,
                    authority,
                    &dto.project_id,
                    &dto.workspace_id,
                    dto.session_id.as_deref(),
                )
                .await
                {
                    return Ok(response);
                }
                let scope = crate::shell::daemon::HumanShellRunScope {
                    principal_id: authority.principal_id().as_str().to_owned(),
                    project_id: dto.project_id,
                    workspace_id: dto.workspace_id,
                    session_id: dto.session_id,
                };
                Ok(self.human_shell_runs.cancel(&scope, &dto.run_id).await)
            }
            _ => Ok(error(
                "human_shell_invalid_request",
                "request is not a human shell operation",
            )),
        }
    }
}

async fn validate_workspace_scope(
    daemon: &CoreDaemon,
    authority: &codegg_core::transport_auth::RequestAuthorityContext,
    project_id: &str,
    workspace_id: &str,
    session_id: Option<&str>,
) -> Result<std::path::PathBuf, CoreResponse> {
    let pool = daemon.pool.clone();
    if pool.is_none()
        && (authority.principal().auth_method()
            != codegg_core::transport_auth::AuthMethod::LocalOwner
            || session_id.is_some())
    {
        return Err(error(
            "human_shell_policy_unavailable",
            "execution authority is unavailable",
        ));
    }
    let workspace_id = codegg_core::workspace::WorkspaceId::parse(workspace_id).map_err(|_| {
        error(
            "human_shell_workspace_unavailable",
            "workspace is unavailable",
        )
    })?;
    let workspace = daemon
        .workspaces
        .resolve(&workspace_id)
        .await
        .ok_or_else(|| {
            error(
                "human_shell_workspace_unavailable",
                "workspace is unavailable",
            )
        })?;
    if workspace.is_archived() {
        return Err(error(
            "human_shell_workspace_unavailable",
            "workspace is unavailable",
        ));
    }
    let Some(pool) = pool else {
        return Ok(workspace.canonical_root.clone());
    };
    let storage = ProjectStorage::new(pool);
    let binding = storage
        .workspace_binding(&workspace_id)
        .await
        .map_err(|_| {
            error(
                "human_shell_workspace_unavailable",
                "workspace binding is unavailable",
            )
        })?
        .ok_or_else(|| {
            error(
                "human_shell_workspace_unavailable",
                "workspace binding is unavailable",
            )
        })?;
    if binding.status != BindingStatus::Resolved
        || binding.project_id.as_str() != project_id
        || binding.node_id.is_some()
    {
        return Err(error(
            "human_shell_scope_mismatch",
            "workspace is not bound to the requested local project",
        ));
    }
    if let Some(session_id) = session_id {
        let binding = storage.session_binding(session_id).await.map_err(|_| {
            error(
                "human_shell_scope_mismatch",
                "session binding is unavailable",
            )
        })?;
        if !matches!(binding, Some(row) if row.status == BindingStatus::Resolved
            && row.project_id.as_ref().is_some_and(|id| id.as_str() == project_id)
            && row.workspace_id.as_ref().is_some_and(|id| id == &workspace_id))
        {
            return Err(error(
                "human_shell_scope_mismatch",
                "session is not bound to the requested project and workspace",
            ));
        }
    }
    Ok(workspace.canonical_root.clone())
}

fn error(code: &str, message: &str) -> CoreResponse {
    CoreResponse::Error {
        code: code.to_owned(),
        message: message.to_owned(),
    }
}
