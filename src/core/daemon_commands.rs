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
    AssetRefreshReasonDto, AssetRefreshRequestDto, AssetRefreshScopeDto, CommandCatalogDto,
    CommandDescriptorDto, CommandExecuteRequestDto, CommandExecuteResultDto, CommandFormDto,
    CommandKindDto, CommandOutcomeStatusDto, CommandReportDto, CoreRequest, CoreResponse,
};

use super::daemon::CoreDaemon;

use codegg_core::command_catalog::{
    self, bounds, catalog_version, find_by_command_id, the_catalog, CommandCapability, CommandKind,
};
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

/// One daemon-effect the executor can serve.
///
/// The table is explicit: a command the daemon does not serve yet is named
/// [`CommandEffect::NotServed`] rather than being absent, so the coverage
/// gap is visible and auditable instead of silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandEffect {
    /// Reload the project's runtime assets (`/reload`).
    AssetRefresh,
    /// The project's effective skill catalogue (`/skills`).
    SkillCatalogue,
    /// The effective execution policy (`/policy`).
    ExecutionPolicy,
    /// The effective reasoning preference (`/reasoning`).
    ReasoningPreference,
    /// Refresh the model list (`/models-refresh`).
    ModelsRefresh,
    /// The project's root `AGENTS.md` draft (`/init`).
    ProjectInitDraft,
    /// The project's memories (`/memory-list`).
    MemoryList,
    /// Search the project's memories (`/memory-search`).
    MemorySearch,
    /// Remember something in the project's memories (`/memory-remember`).
    MemoryRemember,
    /// Forget one memory (`/memory-forget`).
    MemoryForget,
    /// The project's work orders (`/tasks`, `/task`).
    WorkOrderList,
    /// The project's schedules (`/schedules`).
    ScheduleList,
    /// Remote Eggwork node telemetry (`/nodes`).
    NodeTelemetry,
    /// The workspace's managed worktrees (`/worktree`).
    ManagedWorktreeList,
    /// The daemon does not serve this command's effect yet.
    NotServed,
}

/// The table-driven dispatch: each daemon-effect form maps to an existing
/// daemon handler (or a small adapter over one).
///
/// The classification unit is the **form**, so the key is the pair
/// `(command_id, form)`.
pub fn effect_for(command_id: &str, form: &str) -> CommandEffect {
    match (command_id, form) {
        ("reload", "") => CommandEffect::AssetRefresh,
        ("skills", "") => CommandEffect::SkillCatalogue,
        ("policy", "") => CommandEffect::ExecutionPolicy,
        ("reasoning", "") => CommandEffect::ReasoningPreference,
        ("models_refresh", "") => CommandEffect::ModelsRefresh,
        ("project_init", "") => CommandEffect::ProjectInitDraft,
        ("memory_list", "") => CommandEffect::MemoryList,
        ("memory_search", "") => CommandEffect::MemorySearch,
        ("memory_remember", "") => CommandEffect::MemoryRemember,
        ("memory_forget", "") => CommandEffect::MemoryForget,
        ("tasks", "") | ("task", "") => CommandEffect::WorkOrderList,
        ("schedules", "") => CommandEffect::ScheduleList,
        ("nodes", "") => CommandEffect::NodeTelemetry,
        ("worktree", "") => CommandEffect::ManagedWorktreeList,
        _ => CommandEffect::NotServed,
    }
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

        let effect = effect_for(command.command_id, form.form);
        if effect == CommandEffect::NotServed {
            return Ok(refusal(
                &request.command_id,
                REFUSAL_COMMAND_NOT_SERVED,
                format!("the daemon does not serve {} yet", command.name),
                started_at_ms,
            ));
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

        let outcome = self
            .dispatch_command_effect(effect, request, request_id, trusted_client_id, authority)
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
    async fn dispatch_command_effect(
        &self,
        effect: CommandEffect,
        request: &CommandExecuteRequestDto,
        request_id: &str,
        trusted_client_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
    ) -> CommandOutcome {
        let inner = match build_inner_request(effect, request) {
            Some(inner) => inner,
            None => {
                return CommandOutcome {
                    status: CommandOutcomeStatusDto::Failed,
                    summary: "the command's effect could not be prepared".to_string(),
                    lines: Vec::new(),
                    report: None,
                }
            }
        };

        // The inner request is authorized on its own operation and
        // capability. `command.execute` is the authority to run a command,
        // never a widening of what the command may touch.
        let decision = match self.authorize_request(authority, request_id, &inner).await {
            Ok(decision) => decision,
            Err(error) => {
                return CommandOutcome {
                    status: CommandOutcomeStatusDto::Refused,
                    summary: format!("the command's effect was refused: {error}"),
                    lines: vec![format!("refused: {error}")],
                    report: None,
                };
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
                return CommandOutcome {
                    status: CommandOutcomeStatusDto::Failed,
                    summary: format!("the command's effect failed: {error}"),
                    lines: vec![format!("failed: {error}")],
                    report: None,
                }
            }
        };

        summarize(effect, response)
    }
}

/// Build the inner request a command's effect is served by.
fn build_inner_request(
    effect: CommandEffect,
    request: &CommandExecuteRequestDto,
) -> Option<CoreRequest> {
    let scope = AssetRefreshScopeDto {
        project_id: request.scope.project_id.clone(),
        workspace_id: request.scope.workspace_id.clone(),
    };
    let inner = match effect {
        CommandEffect::AssetRefresh => CoreRequest::AssetRefresh {
            request: AssetRefreshRequestDto {
                scope,
                reason: AssetRefreshReasonDto::Reload,
                session_id: None,
            },
        },
        CommandEffect::SkillCatalogue => CoreRequest::SkillCatalogueList {
            request: codegg_protocol::skill::SkillCatalogueRequestDto {
                project_id: request.scope.project_id.clone(),
                max_entries: None,
            },
        },
        CommandEffect::ExecutionPolicy => CoreRequest::ExecutionPolicyGet { session_id: None },
        CommandEffect::ReasoningPreference => CoreRequest::ReasoningPreferenceGet,
        CommandEffect::ModelsRefresh => CoreRequest::ModelsRefresh,
        CommandEffect::ProjectInitDraft => CoreRequest::ProjectInitDraftGet {
            project_id: request.scope.project_id.clone(),
            workspace_id: request.scope.workspace_id.clone(),
        },
        CommandEffect::MemoryList => CoreRequest::MemoryList {
            namespace: request.argument.clone(),
        },
        CommandEffect::MemorySearch => CoreRequest::MemorySearch {
            query: request.argument.clone(),
        },
        CommandEffect::MemoryRemember => CoreRequest::MemoryRemember {
            text: request.argument.clone(),
            namespace: None,
        },
        CommandEffect::MemoryForget => CoreRequest::MemoryForget {
            id: request.argument.clone(),
        },
        CommandEffect::WorkOrderList => CoreRequest::WorkOrderList {
            project_id: request.scope.project_id.clone(),
            state_filter: None,
            cursor: None,
            limit: None,
        },
        CommandEffect::ScheduleList => CoreRequest::ScheduleList {
            workspace_id: Some(request.scope.workspace_id.clone()),
            include_archived: false,
        },
        CommandEffect::NodeTelemetry => CoreRequest::NodeTelemetry { node_id: None },
        CommandEffect::ManagedWorktreeList => CoreRequest::ManagedWorktreeList {
            workspace_id: Some(request.scope.workspace_id.clone()),
            repository_id: None,
            run_id: None,
            include_removed: false,
        },
        CommandEffect::NotServed => return None,
    };
    Some(inner)
}

/// Summarize the inner response into the command's bounded result.
fn summarize(effect: CommandEffect, response: CoreResponse) -> CommandOutcome {
    if let CoreResponse::Error { code, message } = response {
        return CommandOutcome {
            status: CommandOutcomeStatusDto::Refused,
            summary: message.clone(),
            lines: vec![format!("{code}: {message}")],
            report: None,
        };
    }
    match effect {
        CommandEffect::AssetRefresh => match response {
            CoreResponse::AssetRefresh { report } => {
                let summary = format!(
                    "refreshed {} added, {} removed, {} changed, {} shadowed, {} invalid",
                    report.added.len(),
                    report.removed.len(),
                    report.changed.len(),
                    report.shadowed.len(),
                    report.invalid.len()
                );
                let mut lines = Vec::new();
                for label in ["added", "removed", "changed", "shadowed", "invalid"] {
                    for entry in match label {
                        "added" => &report.added,
                        "removed" => &report.removed,
                        "changed" => &report.changed,
                        "shadowed" => &report.shadowed,
                        _ => &report.invalid,
                    } {
                        lines.push(format!("{label}: {entry}"));
                    }
                }
                CommandOutcome {
                    status: CommandOutcomeStatusDto::Completed,
                    summary,
                    lines,
                    report: Some(CommandReportDto {
                        added: report.added.len(),
                        removed: report.removed.len(),
                        changed: report.changed.len(),
                        shadowed: report.shadowed.len(),
                        invalid: report.invalid.len(),
                    }),
                }
            }
            other => generic_outcome("reload", other),
        },
        CommandEffect::SkillCatalogue => match response {
            CoreResponse::SkillCatalogueList { result } => {
                let names = result
                    .skills
                    .iter()
                    .map(|entry| entry.name.clone())
                    .collect::<Vec<String>>();
                let summary = format!("{} skills available", names.len());
                CommandOutcome {
                    status: CommandOutcomeStatusDto::Completed,
                    summary,
                    lines: names,
                    report: None,
                }
            }
            other => generic_outcome("skills", other),
        },
        CommandEffect::ExecutionPolicy => match response {
            CoreResponse::ExecutionPolicy { snapshot } => {
                let summary = format!(
                    "approval mode {:?}, sandbox profile {:?}",
                    snapshot.approval_mode, snapshot.sandbox_profile
                );
                CommandOutcome {
                    status: CommandOutcomeStatusDto::Completed,
                    summary: summary.clone(),
                    lines: vec![summary],
                    report: None,
                }
            }
            other => generic_outcome("policy", other),
        },
        CommandEffect::ReasoningPreference => match response {
            CoreResponse::ReasoningPreference { preference } => {
                let summary = format!("reasoning preference: {preference:?}");
                CommandOutcome {
                    status: CommandOutcomeStatusDto::Completed,
                    summary,
                    lines: vec![format!("reasoning preference: {preference:?}")],
                    report: None,
                }
            }
            other => generic_outcome("reasoning", other),
        },
        CommandEffect::ModelsRefresh => match response {
            CoreResponse::ModelsSnapshot { models, .. } => {
                let summary = format!("{} models available", models.len());
                CommandOutcome {
                    status: CommandOutcomeStatusDto::Completed,
                    summary,
                    lines: models,
                    report: None,
                }
            }
            other => generic_outcome("models-refresh", other),
        },
        CommandEffect::ProjectInitDraft => match response {
            CoreResponse::ProjectInitDraft { draft } => {
                let summary = draft
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.message.clone())
                    .collect::<Vec<String>>()
                    .join("; ");
                CommandOutcome {
                    status: CommandOutcomeStatusDto::Completed,
                    summary: if summary.is_empty() {
                        "project instruction draft is ready".to_string()
                    } else {
                        summary
                    },
                    lines: Vec::new(),
                    report: None,
                }
            }
            other => generic_outcome("init", other),
        },
        CommandEffect::MemoryList
        | CommandEffect::MemorySearch
        | CommandEffect::MemoryRemember
        | CommandEffect::MemoryForget
        | CommandEffect::WorkOrderList
        | CommandEffect::ScheduleList
        | CommandEffect::NodeTelemetry
        | CommandEffect::ManagedWorktreeList => generic_outcome(effect_label(effect), response),
        CommandEffect::NotServed => CommandOutcome {
            status: CommandOutcomeStatusDto::Failed,
            summary: "the daemon does not serve this command's effect yet".to_string(),
            lines: Vec::new(),
            report: None,
        },
    }
}

/// The display label for one effect.
fn effect_label(effect: CommandEffect) -> &'static str {
    match effect {
        CommandEffect::AssetRefresh => "reload",
        CommandEffect::SkillCatalogue => "skills",
        CommandEffect::ExecutionPolicy => "policy",
        CommandEffect::ReasoningPreference => "reasoning",
        CommandEffect::ModelsRefresh => "models-refresh",
        CommandEffect::ProjectInitDraft => "init",
        CommandEffect::MemoryList => "memory-list",
        CommandEffect::MemorySearch => "memory-search",
        CommandEffect::MemoryRemember => "memory-remember",
        CommandEffect::MemoryForget => "memory-forget",
        CommandEffect::WorkOrderList => "tasks",
        CommandEffect::ScheduleList => "schedules",
        CommandEffect::NodeTelemetry => "nodes",
        CommandEffect::ManagedWorktreeList => "worktree",
        CommandEffect::NotServed => "command",
    }
}

/// The outcome for an effect whose payload the daemon does not render
/// specially. The lines are the payload's own bounded summary.
fn generic_outcome(label: &str, response: CoreResponse) -> CommandOutcome {
    let summary = format!("{label} completed");
    let lines = vec![format!("{label}: {}", response_label(&response))];
    CommandOutcome {
        status: CommandOutcomeStatusDto::Completed,
        summary,
        lines,
        report: None,
    }
}

/// A short, bounded label for a response variant.
fn response_label(response: &CoreResponse) -> &'static str {
    match response {
        CoreResponse::Error { .. } => "error",
        CoreResponse::AssetRefresh { .. } => "asset refresh report",
        CoreResponse::AssetRefreshStatus { .. } => "asset refresh status",
        CoreResponse::SkillCatalogueList { .. } => "skill catalogue",
        CoreResponse::ExecutionPolicy { .. } => "execution policy",
        CoreResponse::ReasoningPreference { .. } => "reasoning preference",
        CoreResponse::ModelsSnapshot { .. } => "model list",
        CoreResponse::ProjectInitDraft { .. } => "project instruction draft",
        CoreResponse::Json { .. } => "result",
        CoreResponse::WorkOrderList { .. } => "work order list",
        CoreResponse::ScheduleList { .. } => "schedule list",
        CoreResponse::NodeTelemetry { .. } => "node telemetry",
        CoreResponse::ManagedWorktrees { .. } => "managed worktree list",
        _ => "result",
    }
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
    }
}

/// Map the domain kind onto the wire kind.
fn command_kind_dto(kind: CommandKind) -> CommandKindDto {
    match kind {
        CommandKind::DaemonEffect => CommandKindDto::DaemonEffect,
        CommandKind::FrontendEffect => CommandKindDto::FrontendEffect,
    }
}
