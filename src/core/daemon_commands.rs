//! M007-D: the `commands` request family for `CoreDaemon`.
//!
//! ADR-0052 (accepted 2026-10-24) moved command ownership into the daemon.
//! This module is the daemon side of that decision:
//!
//! * [`CoreRequest::CommandCatalog`] serves the daemon's own catalog — the
//!   single definition of what commands exist. No frontend compiles a second
//!   list.
//! * [`CoreRequest::CommandExecute`] executes one **daemon-effect** form.
//!   The daemon authorizes it under the capability the catalog entry names
//!   (`command.execute`), audits it as a canonical `Run`, and returns a
//!   bounded structured result.
//!
//! ## What is refused, and why
//!
//! A refusal is typed and produces **no** `Run`:
//!
//! | code | meaning |
//! |---|---|
//! | `unknown_command` | the command identity is not in the catalog |
//! | `unknown_form` | the command has no such argument form |
//! | `not_daemon_effect` | the form is a frontend effect; the frontend owns it |
//! | `stale_catalog` | the request names a catalog version the daemon no longer serves |
//! | `argument_too_long` | the argument exceeds the bound |
//! | `capability_required` | the catalog entry names no `command.execute` capability |
//! | `run_store_unavailable` | the daemon cannot record the run, so it refuses to execute |
//! | `catalog_over_bound` | the daemon's own catalog exceeds its bounds |
//! | `result_over_bound` | the produced result exceeds its bounds |
//! | `command_not_served` | the daemon does not serve this command's effect yet |
//!
//! ## The inner request is authorized on its own terms
//!
//! A command's effect is served by the daemon's existing handler for the
//! equivalent request. That inner request is authorized **again**, on its
//! own operation and capability, rather than inheriting the command's
//! decision: `command.execute` is the authority to *run* a command, never a
//! widening of what the command may touch.

use crate::error::AppError;
use crate::protocol::core::{
    CommandCatalogDto, CommandDescriptorDto, CommandExecuteRequestDto, CommandExecuteResultDto,
    CommandFormDto, CommandKindDto, CommandOutcomeStatusDto, CommandReportDto, CoreRequest,
    CoreResponse,
};

use super::daemon::CoreDaemon;

use codegg_core::command_catalog::{
    self, bounds, catalog_version, find_by_command_id, the_catalog, CommandCapability, CommandKind,
};
use serde_json::json;

use codegg_core::run_store::{
    ActualBackend, BackendRecord, PlannedBackend, RiskRecord, RunCompletion, RunDraft,
    RunInvocation, RunKind, RunOwnership, RunStatus,
};

/// The command identity is not in the catalog.
pub const REFUSAL_UNKNOWN_COMMAND: &str = "unknown_command";
/// The command has no such argument form.
pub const REFUSAL_UNKNOWN_FORM: &str = "unknown_form";
/// The form is a frontend effect; the daemon does not execute it.
pub const REFUSAL_NOT_DAEMON_EFFECT: &str = "not_daemon_effect";
/// The request names a catalog version the daemon no longer serves.
pub const REFUSAL_STALE_CATALOG: &str = "stale_catalog";
/// The argument exceeds its bound.
pub const REFUSAL_ARGUMENT_TOO_LONG: &str = "argument_too_long";
/// The catalog entry names no `command.execute` capability.
pub const REFUSAL_CAPABILITY_REQUIRED: &str = "capability_required";
/// The daemon cannot record the run, so it refuses to execute.
pub const REFUSAL_RUN_STORE_UNAVAILABLE: &str = "run_store_unavailable";
/// The daemon's own catalog exceeds its bounds.
pub const REFUSAL_CATALOG_OVER_BOUND: &str = "catalog_over_bound";
/// The produced result exceeds its bounds.
pub const REFUSAL_RESULT_OVER_BOUND: &str = "result_over_bound";
/// The daemon does not serve this command's effect yet.
pub const REFUSAL_COMMAND_NOT_SERVED: &str = "command_not_served";
/// The command acts on a session and the caller named none.
pub const REFUSAL_NO_SESSION: &str = "no_session";

/// The daemon-side context a command's effect is built from.
///
/// The daemon owns the effect, so it resolves what it can from its own
/// durable binding and reads the rest from the request's scope: the
/// project, the workspace, and the session the caller named.
pub struct CommandContext<'a> {
    /// The project the command acts in.
    pub project_id: &'a str,
    /// The workspace the command acts in.
    pub workspace_id: &'a str,
    /// The session the command acts on, when the caller named one.
    pub session_id: Option<&'a str>,
    /// The command's argument.
    pub argument: &'a str,
}

/// Build a daemon request from its wire shape.
///
/// The requests are built from JSON rather than from typed constructors so
/// that one table row is one line, and so that a row which names a shape the
/// protocol does not have fails *closed* — the deserialisation fails, the
/// entry yields nothing, and the command is refused with
/// `command_not_served` instead of dispatching a half-built request.
fn json_request(shape: serde_json::Value) -> Option<CoreRequest> {
    match serde_json::from_value(shape.clone()) {
        Ok(r) => Some(r),
        Err(e) => {
            eprintln!(
                "PROBE-SHAPE-FAIL {} :: {}",
                shape.get("type").and_then(|v| v.as_str()).unwrap_or("?"),
                e
            );
            None
        }
    }
}

/// What the daemon's effect table yields for one command form.
#[derive(Debug)]
pub enum CommandEffect {
    /// The request that serves the form.
    ///
    /// The request is boxed: it is far larger than the other variant, and
    /// an unboxed pair would make every match on this enum copy a
    /// `CoreRequest`.
    Ready(Box<CoreRequest>),
    /// The form acts on a session and the caller named none.
    ///
    /// The daemon resolves the project and the workspace from its own durable
    /// binding, but the session is the caller's selection: the daemon cannot
    /// guess which session a user is looking at.
    NeedsSession,
}

/// The command's effect table.
///
/// Each row names the daemon request that serves the form, built from the
/// scope and the argument. A form with no row is refused with
/// `command_not_served`: the daemon does not guess what a command means.
///
/// The rows are built from JSON rather than from typed constructors so that
/// one row is one line, and so that a row which names a shape the protocol
/// does not have fails *closed* — the deserialisation fails, the entry
/// yields nothing, and the command is refused with `command_not_served`
/// instead of dispatching a half-built request. The closedness is proved by
/// the `every_row_builds` test, so a wrong shape fails a test rather than
/// quietly becoming a refusal.
pub fn build_inner_request(
    command_id: &str,
    form: &str,
    ctx: &CommandContext<'_>,
) -> Option<CommandEffect> {
    let project_id = ctx.project_id;
    let workspace_id = ctx.workspace_id;
    let session = ctx.session_id;
    let argument = ctx.argument;
    let trimmed = argument.trim_start();
    let first = trimmed.split_whitespace().next().unwrap_or_default();
    let rest = trimmed[first.len()..].trim_start();
    let shape = match (command_id, form) {
        // ── assets and the project ──
        ("reload", "") => json!({
            "type": "asset_refresh",
            "request": {
                "scope": { "project_id": project_id, "workspace_id": workspace_id },
                "reason": "reload",
                "session_id": session,
            }
        }),
        ("project_init", "") => json!({
            "type": "project_init_draft_get",
            "project_id": project_id,
            "workspace_id": workspace_id,
        }),

        // ── the effective policy and the preferences ──
        ("policy", "") => json!({
            "type": "execution_policy_get",
            "session_id": session,
        }),
        ("approval", "") => json!({ "type": "approval_preference_get" }),
        ("reasoning", "") => json!({ "type": "reasoning_preference_get" }),
        ("network", "") => json!({
            "type": "network_intent_set",
            "network_intent": first,
            "expected_revision": null,
        }),

        // ── the daemon's own catalogues ──
        ("skills", "") => json!({
            "type": "skill_catalogue_list",
            "request": { "project_id": project_id, "max_entries": null },
        }),
        ("models_refresh", "") => json!({ "type": "models_refresh" }),
        ("nodes", "") => json!({ "type": "node_telemetry", "node_id": null }),
        ("worktree", "") => json!({
            "type": "managed_worktree_list",
            "workspace_id": workspace_id,
            "repository_id": null,
            "run_id": null,
            "include_removed": false,
        }),
        ("tool_contracts", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "tool_program_list",
                "session_id": session_id,
                "state_filter": null,
            })
        }

        // ── sessions ──
        ("share", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({ "type": "session_share", "session_id": session_id })
        }
        ("unshare", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({ "type": "session_unshare", "session_id": session_id })
        }
        ("export", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({ "type": "session_export", "session_id": session_id })
        }
        ("status", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({ "type": "session_lifecycle_get", "session_id": session_id })
        }

        // ── goals, tasks and schedules ──
        ("tasks", "") | ("task", "") => json!({
            "type": "work_order_list",
            "project_id": project_id,
            "state_filter": null,
            "cursor": null,
            "limit": 20,
        }),
        ("schedules", "") => json!({
            "type": "schedule_list",
            "workspace_id": workspace_id,
            "include_archived": false,
        }),
        ("goal", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({ "type": "goal_show", "session_id": session_id })
        }
        ("state", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({ "type": "active_goal_load", "session_id": session_id })
        }

        // ── memory ──
        ("memory", "") | ("memory_list", "") => json!({
            "type": "memory_list",
            "namespace": if argument.is_empty() { "user/preferences".to_string() } else { argument.to_string() },
        }),
        ("memory_search", "") => json!({ "type": "memory_search", "query": argument }),
        ("memory_remember", "") => json!({
            "type": "memory_remember",
            "text": argument,
            "namespace": Some("user/preferences".to_string()),
        }),
        ("memory_forget", "") => json!({ "type": "memory_forget", "id": argument }),

        // ── collaboration ──
        ("collaborators", "") => json!({
            "type": "team_principal_list",
            "limit": null,
        }),
        ("team", "") => json!({
            "type": "team_membership_list",
            "project_id": project_id,
            "limit": null,
        }),
        ("control", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({ "type": "session_control_get", "session_id": session_id })
        }
        ("observe", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "projection_subscribe",
                "request": {
                    "scope": "session",
                    "scope_id": session_id,
                    "cursor": null,
                    "projection_version": 1,
                }
            })
        }

        // ── the editor's checkpoints ──
        ("edit_undo", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "edit_checkpoint_undo_latest",
                "workspace_id": workspace_id,
                "session_id": session_id,
            })
        }
        ("edit_reapply", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "edit_checkpoint_reapply_latest",
                "workspace_id": workspace_id,
                "session_id": session_id,
            })
        }
        ("edit_checkpoints", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "edit_checkpoint_list",
                "workspace_id": workspace_id,
                "session_id": session_id,
                "limit": 20,
            })
        }

        // ── the human shell ──
        ("shell_list", "") => json!({ "type": "human_shell_run_capabilities" }),
        ("shell_show", "") | ("shell_expand", "") => json!({
            "type": "human_shell_run_read",
            "request": {
                "run_id": first,
                "project_id": project_id,
                "workspace_id": workspace_id,
                "session_id": session,
                "from_sequence": 0,
                "max_bytes": 65536,
            }
        }),
        ("shell_kill", "") => json!({
            "type": "human_shell_run_cancel",
            "request": {
                "run_id": first,
                "project_id": project_id,
                "workspace_id": workspace_id,
                "session_id": session,
            }
        }),

        // ── the interactive terminal ──
        ("terminal_list", "") | ("terminal_show", "") => json!({
            "type": "interactive_process_list",
            "workspace_id": workspace_id,
            "limit": 20,
        }),
        ("terminal_send", "") => json!({
            "type": "interactive_process_input",
            "attachment_id": first,
            "data_b64": rest,
        }),

        // ── the language server ──
        ("lsp_status", "") | ("lsp_errors", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "lsp_diagnostics_get",
                "request": { "project_id": project_id, "session_id": session_id },
            })
        }

        _ => return None,
    };
    json_request(shape).map(|request| CommandEffect::Ready(Box::new(request)))
}

/// Whether the daemon serves this command's effect.
///
/// The check is independent of the caller's scope: it asks whether the
/// effect table has a row for the form at all. A row whose effect needs a
/// session still counts as served — the daemon refuses *that* call, with
/// `no_session`, rather than pretending the form is unknown.
pub fn serves_form(command_id: &str, form: &str) -> bool {
    let ctx = CommandContext {
        project_id: "probe",
        workspace_id: "probe",
        session_id: Some("probe"),
        argument: "",
    };
    matches!(
        build_inner_request(command_id, form, &ctx),
        Some(CommandEffect::Ready(_))
    )
}

/// The outcome of one command execution, before it becomes a DTO.
#[derive(Debug, Clone)]
pub struct CommandOutcome {
    /// How the execution ended.
    pub status: CommandOutcomeStatusDto,
    /// A one-line summary.
    pub summary: String,
    /// The structured output lines.
    pub lines: Vec<String>,
    /// The structured report, when the command produces one.
    pub report: Option<CommandReportDto>,
}

impl CoreDaemon {
    /// M007-D: the `commands` request family.
    pub(crate) async fn handle_command_request(
        &self,
        request: CoreRequest,
        request_id: &str,
        trusted_client_id: &str,
        authority: codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        match request {
            CoreRequest::CommandCatalog { scope } => Ok(CoreResponse::CommandCatalog {
                catalog: self.command_catalog_dto(&scope)?,
            }),
            CoreRequest::CommandExecute { request } => {
                let result = self
                    .execute_command(
                        &request,
                        request_id,
                        trusted_client_id,
                        &authority,
                        &authz_decision,
                    )
                    .await?;
                Ok(CoreResponse::CommandExecute { result })
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

    /// Build the catalog DTO.
    ///
    /// An over-bound catalog is **refused**, never truncated: the daemon
    /// will not hand a frontend a silently shortened command list.
    fn command_catalog_dto(
        &self,
        _scope: &crate::protocol::core::CommandCatalogScopeDto,
    ) -> Result<CommandCatalogDto, AppError> {
        let catalog = the_catalog();
        if let Err(error) = command_catalog::check_catalog_bounds(catalog) {
            return Err(AppError::Other(anyhow::anyhow!(
                "{REFUSAL_CATALOG_OVER_BOUND}: {error}"
            )));
        }
        let commands = catalog
            .commands
            .iter()
            .map(|command| CommandDescriptorDto {
                command_id: command.command_id.to_string(),
                name: command.name.to_string(),
                aliases: command
                    .aliases
                    .iter()
                    .map(|alias| (*alias).to_string())
                    .collect(),
                description: command.description.to_string(),
                domain: command.domain.to_string(),
                kind: command_kind_dto(command.kind),
                capability: command
                    .capability
                    .map(|capability| capability.as_str().to_string()),
                forms: command
                    .forms
                    .iter()
                    .map(|form| CommandFormDto {
                        form: form.form.to_string(),
                        kind: command_kind_dto(form.kind),
                        argument_hint: form.argument_hint.to_string(),
                        argument_required: form.argument_required,
                        // The daemon serves exactly the forms its effect
                        // table has a row for. A frontend-effect form has no
                        // row: the frontend owns it.
                        daemon_served: serves_form(command.command_id, form.form),
                    })
                    .collect(),
            })
            .collect();
        Ok(CommandCatalogDto {
            catalog_version: catalog.version,
            commands,
        })
    }

    /// Execute one daemon-effect command form.
    async fn execute_command(
        &self,
        request: &CommandExecuteRequestDto,
        request_id: &str,
        trusted_client_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        _authz_decision: &codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CommandExecuteResultDto, AppError> {
        let started_at_ms = chrono::Utc::now().timestamp_millis();

        // The catalog version is checked first: a stale request is refused
        // with the current version so the frontend refetches rather than
        // guessing which command it is running.
        if request.catalog_version != catalog_version() {
            return Ok(refusal(
                &request.command_id,
                REFUSAL_STALE_CATALOG,
                format!(
                    "catalog version {} is current; refetch and re-issue",
                    catalog_version()
                ),
                started_at_ms,
            ));
        }

        let Some(command) = find_by_command_id(&request.command_id) else {
            return Ok(refusal(
                &request.command_id,
                REFUSAL_UNKNOWN_COMMAND,
                format!("no command named {}", request.command_id),
                started_at_ms,
            ));
        };

        let Some(form) = command.forms.iter().find(|form| form.form == request.form) else {
            return Ok(refusal(
                &request.command_id,
                REFUSAL_UNKNOWN_FORM,
                format!("{} has no form named {}", command.name, request.form),
                started_at_ms,
            ));
        };

        // A frontend effect never reaches the daemon: the frontend owns it,
        // and this refusal produces no run.
        if form.kind != CommandKind::DaemonEffect {
            return Ok(refusal(
                &request.command_id,
                REFUSAL_NOT_DAEMON_EFFECT,
                format!("the form named {} is a frontend effect", request.form),
                started_at_ms,
            ));
        }

        if request.argument.len() > bounds::MAX_ARGUMENT_BYTES {
            return Ok(refusal(
                &request.command_id,
                REFUSAL_ARGUMENT_TOO_LONG,
                format!(
                    "argument is {} bytes, over the {} byte bound",
                    request.argument.len(),
                    bounds::MAX_ARGUMENT_BYTES
                ),
                started_at_ms,
            ));
        }

        // The capability is a property of the catalog entry, never a
        // client-supplied widening. The request DTO carries no capability
        // field at all, so there is nothing for a client to widen.
        match command.capability {
            Some(CommandCapability::CommandExecute) => {}
            _ => {
                return Ok(refusal(
                    &request.command_id,
                    REFUSAL_CAPABILITY_REQUIRED,
                    format!("{} names no command.execute capability", command.name),
                    started_at_ms,
                ));
            }
        }

        // The effect table is the daemon's own map from a command form to the
        // request that serves it. A form with no row is refused: the daemon
        // does not guess what a command means.
        let ctx = CommandContext {
            project_id: &request.scope.project_id,
            workspace_id: &request.scope.workspace_id,
            session_id: request.scope.session_id.as_deref(),
            argument: &request.argument,
        };
        match build_inner_request(command.command_id, form.form, &ctx) {
            None => {
                return Ok(refusal(
                    &request.command_id,
                    REFUSAL_COMMAND_NOT_SERVED,
                    format!("the daemon does not serve {} yet", command.name),
                    started_at_ms,
                ))
            }
            Some(CommandEffect::NeedsSession) => {
                return Ok(refusal(
                    &request.command_id,
                    REFUSAL_NO_SESSION,
                    format!(
                        "{} acts on a session and the request named none",
                        command.name
                    ),
                    started_at_ms,
                ))
            }
            Some(CommandEffect::Ready(_)) => {}
        }

        let Some(store) = self.run_store.as_ref() else {
            // Fail closed: without a run store the execution would be
            // unaudited, and an unaudited daemon effect is not permitted.
            return Ok(refusal(
                &request.command_id,
                REFUSAL_RUN_STORE_UNAVAILABLE,
                "the daemon cannot record the run for this command".to_string(),
                started_at_ms,
            ));
        };

        let workspace_root = self.command_workspace_root(&request.scope).await;
        let draft = RunDraft {
            kind: RunKind::Command,
            invocation: RunInvocation {
                command: format!("/{}", command.command_id),
                argv: Some(vec![
                    command.command_id.to_string(),
                    form.form.to_string(),
                    request.argument.clone(),
                ]),
                script_hash: None,
            },
            session_id: None,
            parent_run_id: None,
            workspace_root: workspace_root.clone(),
            cwd: workspace_root,
            backend: BackendRecord {
                family: "command".to_string(),
                detail: Some(command.command_id.to_string()),
            },
            risk: RiskRecord {
                level: "low".to_string(),
                has_subprocess: false,
                has_git_mutation: false,
                has_destructive_mutation: false,
            },
            planned_backend: Some(PlannedBackend::NativeTool),
            actual_backend: Some(ActualBackend::NativeTool),
            ownership: RunOwnership::Caller,
            asset_provenance: None,
        };

        let handle = match store.begin_run(draft).await {
            Ok(handle) => handle,
            Err(error) => {
                return Ok(refusal(
                    &request.command_id,
                    REFUSAL_RUN_STORE_UNAVAILABLE,
                    format!("the run could not be opened: {error}"),
                    started_at_ms,
                ));
            }
        };
        let run_id = handle.run_id.as_str().to_string();

        let (outcome, inner) = self
            .dispatch_command_effect(
                command.command_id,
                form.form,
                &ctx,
                request_id,
                trusted_client_id,
                authority,
            )
            .await;

        let completed_at_ms = chrono::Utc::now().timestamp_millis();
        let run_status = match outcome.status {
            CommandOutcomeStatusDto::Completed => RunStatus::Complete,
            CommandOutcomeStatusDto::Refused => RunStatus::Failed,
            CommandOutcomeStatusDto::Failed => RunStatus::Failed,
            CommandOutcomeStatusDto::Cancelled => RunStatus::Cancelled,
        };
        let completion = RunCompletion {
            status: run_status,
            completed_at: chrono::Utc::now(),
            permissions: Vec::new(),
            sandbox: None,
            projection: None,
            changes: Vec::new(),
            rerun: None,
            actual_backend: Some(ActualBackend::NativeTool),
            fallback: None,
            remote_resources: None,
        };
        if let Err(error) = store.complete_run(handle, completion).await {
            tracing::warn!(
                run_id = %run_id,
                error = %error,
                "M007-D: the command run could not be closed"
            );
        }

        if outcome.lines.len() > bounds::MAX_RESULT_LINES {
            return Ok(refusal(
                &request.command_id,
                REFUSAL_RESULT_OVER_BOUND,
                format!(
                    "the result carries {} lines, over the {} line bound",
                    outcome.lines.len(),
                    bounds::MAX_RESULT_LINES
                ),
                started_at_ms,
            ));
        }
        let serialized = serde_json::to_string(&outcome.lines).unwrap_or_default();
        if serialized.len() > bounds::MAX_RESULT_BYTES {
            return Ok(refusal(
                &request.command_id,
                REFUSAL_RESULT_OVER_BOUND,
                format!(
                    "the result serializes to {} bytes, over the {} byte bound",
                    serialized.len(),
                    bounds::MAX_RESULT_BYTES
                ),
                started_at_ms,
            ));
        }

        Ok(CommandExecuteResultDto {
            run_id,
            command_id: request.command_id.clone(),
            status: outcome.status,
            summary: outcome.summary,
            lines: outcome.lines,
            report: outcome.report,
            started_at_ms,
            completed_at_ms,
            // The inner response travels verbatim: the daemon owns the
            // effect and the authoritative data, and the frontend keeps the
            // formatting by rendering the response it already knows how to
            // render.
            response: inner.and_then(|response| serde_json::to_value(response).ok()),
        })
    }

    /// Resolve the workspace root the command ran in.
    ///
    /// The daemon resolves the root from its own durable binding. When it
    /// cannot, the run records an empty root rather than guessing: the
    /// daemon never consults the process working directory.
    async fn command_workspace_root(
        &self,
        scope: &crate::protocol::core::CommandCatalogScopeDto,
    ) -> std::path::PathBuf {
        let Some(resolver) = self.context_resolver.as_ref() else {
            return std::path::PathBuf::new();
        };
        let Ok(project_id) = codegg_core::identity::ProjectId::parse(&scope.project_id) else {
            return std::path::PathBuf::new();
        };
        let Ok(workspace_id) = codegg_core::workspace::WorkspaceId::parse(&scope.workspace_id)
        else {
            return std::path::PathBuf::new();
        };
        match resolver
            .resolve(codegg_core::context::ProjectContextRequest::new(
                project_id,
                workspace_id,
            ))
            .await
        {
            Ok(context) => context.workspace_root,
            Err(_) => std::path::PathBuf::new(),
        }
    }

    /// Serve one command effect by running the daemon's own handler for the
    /// equivalent request.
    #[allow(clippy::too_many_arguments)]
    async fn dispatch_command_effect(
        &self,
        command_id: &str,
        form: &str,
        ctx: &CommandContext<'_>,
        request_id: &str,
        trusted_client_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
    ) -> (CommandOutcome, Option<CoreResponse>) {
        let inner = match build_inner_request(command_id, form, ctx) {
            Some(CommandEffect::Ready(inner)) => *inner,
            _ => {
                return (
                    CommandOutcome {
                        status: CommandOutcomeStatusDto::Failed,
                        summary: "the command's effect could not be prepared".to_string(),
                        lines: Vec::new(),
                        report: None,
                    },
                    None,
                )
            }
        };

        // The inner request is authorized on its own operation and
        // capability. `command.execute` is the authority to run a command,
        // never a widening of what the command may touch.
        let decision = match self.authorize_request(authority, request_id, &inner).await {
            Ok(decision) => decision,
            Err(error) => {
                return (
                    CommandOutcome {
                        status: CommandOutcomeStatusDto::Refused,
                        summary: format!("the command's effect was refused: {error}"),
                        lines: vec![format!("refused: {error}")],
                        report: None,
                    },
                    None,
                )
            }
        };

        let family = super::daemon_family::DaemonRequestFamily::of(&inner);
        let response = match self
            .dispatch_family_request(
                family,
                inner,
                request_id,
                trusted_client_id,
                authority.clone(),
                decision,
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                return (
                    CommandOutcome {
                        status: CommandOutcomeStatusDto::Failed,
                        summary: format!("the command's effect failed: {error}"),
                        lines: vec![format!("failed: {error}")],
                        report: None,
                    },
                    None,
                )
            }
        };

        let outcome = summarize(&response);
        (outcome, Some(response))
    }
}

/// Summarize the inner response into the command's bounded result.
///
/// The renderer is variant-agnostic on purpose: it reads the response's
/// wire shape, so one function covers every command family and a new
/// family needs no new renderer. A frontend that wants richer text reads
/// the inner response itself and renders it with the presentation code it
/// already had.
fn summarize(response: &CoreResponse) -> CommandOutcome {
    if let CoreResponse::Error { code, message } = response {
        return CommandOutcome {
            status: CommandOutcomeStatusDto::Refused,
            summary: message.clone(),
            lines: vec![format!("{code}: {message}")],
            report: None,
        };
    }
    let value = serde_json::to_value(response).ok();
    let kind = value
        .as_ref()
        .and_then(|value| value.get("type"))
        .and_then(|value| value.as_str())
        .unwrap_or("result")
        .to_string();
    let lines = render_lines(value.as_ref(), &kind);
    CommandOutcome {
        status: CommandOutcomeStatusDto::Completed,
        summary: format!("{kind} completed"),
        lines,
        report: None,
    }
}

/// The bounded lines a response renders to.
///
/// A list of records renders as one line per record's naming field; every
/// other shape renders as its kind. The renderer never renders a payload
/// that is not already bounded by the protocol.
fn render_lines(value: Option<&serde_json::Value>, kind: &str) -> Vec<String> {
    let Some(object) = value.and_then(|value| value.as_object()) else {
        return vec![kind.to_string()];
    };
    for field in ["items", "sessions", "entries", "skills", "records", "rows"] {
        let Some(serde_json::Value::Array(items)) = object.get(field) else {
            continue;
        };
        if items.is_empty() {
            return vec![format!("{kind}: 0")];
        }
        return items
            .iter()
            .take(64)
            .map(|item| {
                item.get("name")
                    .or_else(|| item.get("title"))
                    .or_else(|| item.get("id"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("entry")
                    .to_string()
            })
            .collect();
    }
    vec![kind.to_string()]
}

/// Build a typed refusal. A refusal produces **no** run.
fn refusal(
    command_id: &str,
    code: &str,
    reason: String,
    started_at_ms: i64,
) -> CommandExecuteResultDto {
    CommandExecuteResultDto {
        run_id: String::new(),
        command_id: command_id.to_string(),
        status: CommandOutcomeStatusDto::Refused,
        summary: reason.clone(),
        lines: vec![format!("{code}: {reason}")],
        report: None,
        started_at_ms,
        completed_at_ms: started_at_ms,
        response: None,
    }
}

/// Map the domain kind onto the wire kind.
fn command_kind_dto(kind: CommandKind) -> CommandKindDto {
    match kind {
        CommandKind::DaemonEffect => CommandKindDto::DaemonEffect,
        CommandKind::FrontendEffect => CommandKindDto::FrontendEffect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::command_catalog::{the_catalog, CommandKind};

    /// Every row of the effect table.
    ///
    /// The list is the table's key set. The `the_effect_table_is_closed` test
    /// proves it agrees with the match arms in both directions, so a row
    /// cannot be added to one and forgotten in the other.
    const EFFECT_TABLE: &[(&str, &str)] = &[
        ("reload", ""),
        ("project_init", ""),
        ("policy", ""),
        ("approval", ""),
        ("reasoning", ""),
        ("network", ""),
        ("skills", ""),
        ("models_refresh", ""),
        ("nodes", ""),
        ("worktree", ""),
        ("tool_contracts", ""),
        ("share", ""),
        ("unshare", ""),
        ("export", ""),
        ("status", ""),
        ("tasks", ""),
        ("task", ""),
        ("schedules", ""),
        ("goal", ""),
        ("state", ""),
        ("memory", ""),
        ("memory_list", ""),
        ("memory_search", ""),
        ("memory_remember", ""),
        ("memory_forget", ""),
        ("collaborators", ""),
        ("team", ""),
        ("control", ""),
        ("observe", ""),
        ("edit_undo", ""),
        ("edit_reapply", ""),
        ("edit_checkpoints", ""),
        ("shell_list", ""),
        ("shell_show", ""),
        ("shell_expand", ""),
        ("shell_kill", ""),
        ("terminal_list", ""),
        ("terminal_show", ""),
        ("terminal_send", ""),
        ("lsp_status", ""),
        ("lsp_errors", ""),
    ];

    fn daemon_effect_forms() -> Vec<(&'static str, &'static str)> {
        the_catalog()
            .commands
            .iter()
            .flat_map(|command| {
                command
                    .forms
                    .iter()
                    .filter(|form| form.kind == CommandKind::DaemonEffect)
                    .map(move |form| (command.command_id, form.form))
            })
            .collect()
    }

    /// Every row of the effect table names a real daemon-effect form, and
    /// every daemon-effect form the table serves is listed.
    ///
    /// The two directions matter: a row naming a form the catalog does not
    /// have is dead, and a listed row whose match arm is missing is a
    /// command the daemon claims to serve and then refuses.
    #[test]
    fn the_effect_table_is_closed() {
        let daemon_forms = daemon_effect_forms();
        for (command_id, form) in EFFECT_TABLE {
            assert!(
                daemon_forms.contains(&(*command_id, *form)),
                "{command_id}/{form} is not a daemon-effect form in the catalog"
            );
            assert!(
                serves_form(command_id, form),
                "{command_id}/{form} is listed but the daemon does not build a request for it"
            );
        }
        let served = daemon_forms
            .iter()
            .filter(|(command_id, form)| serves_form(command_id, form))
            .count();
        assert_eq!(
            served,
            EFFECT_TABLE.len(),
            "the served daemon-effect forms and the listed rows must agree"
        );
    }

    /// Every row builds a request the protocol actually has.
    ///
    /// The table's rows are built from JSON so that a wrong shape fails
    /// *closed*. This test is what keeps that from becoming a silent
    /// refusal: a row whose shape the protocol does not have fails here
    /// rather than turning into `command_not_served` in production.
    #[test]
    fn every_row_builds() {
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "argument",
        };
        for (command_id, form) in EFFECT_TABLE {
            let effect = build_inner_request(command_id, form, &ctx)
                .unwrap_or_else(|| panic!("{command_id}/{form} must have a row"));
            assert!(
                matches!(effect, CommandEffect::Ready(_)),
                "{command_id}/{form} must build a request when a session is named"
            );
        }
    }

    /// A session-scoped row refuses when the caller named no session.
    ///
    /// The refusal is distinct from `command_not_served`: the daemon serves
    /// the form, it just cannot guess which session the caller meant.
    #[test]
    fn a_session_scoped_row_refuses_without_a_session() {
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: None,
            argument: "",
        };
        let effect = build_inner_request("share", "", &ctx).expect("share has a row");
        assert!(
            matches!(effect, CommandEffect::NeedsSession),
            "share acts on a session and must refuse without one"
        );
        // A row that does not act on a session still builds.
        let effect = build_inner_request("skills", "", &ctx).expect("skills has a row");
        assert!(
            matches!(effect, CommandEffect::Ready(_)),
            "skills does not act on a session and must build without one"
        );
    }

    /// A form with no row is not served, and the daemon says so.
    #[test]
    fn a_form_with_no_row_is_not_served() {
        assert!(!serves_form("not_a_command", ""));
        assert!(!serves_form("skills", "not_a_form"));
    }
}
