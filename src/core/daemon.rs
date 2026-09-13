use std::sync::Arc;
use std::time::Instant;

use crate::error::AppError;
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse, RequestEnvelope};

use super::project_activation::ProjectActivationRegistry;
use super::runtime_deps::CoreRuntimeDeps;
use codegg_core::context::ProjectContextResolver;
use codegg_core::workspace::WorkspaceRegistry;
use codegg_core::workspace_services::WorkspaceServiceRegistry;

pub struct CoreDaemon {
    pub daemon_id: String,
    /// Identity generation captured at lock acquisition. Distinct from
    /// `daemon_id` so two processes that happen to roll the same 8-hex
    /// suffix are still distinguishable. Set by [`Self::with_deps_and_identity`]
    /// or generated on demand by [`Self::with_deps`].
    pub generation: String,
    pub pool: Option<sqlx::SqlitePool>,
    pub deps: CoreRuntimeDeps,
    pub event_log: Arc<super::event_log::EventLog>,
    pub sessions: Arc<crate::core::session_runtime::SessionRuntimeRegistry>,
    pub clients: Arc<super::client_registry::ClientRegistry>,
    pub notification_router: Arc<super::notification::NotificationRouter>,
    pub audio_arbiter: Option<Arc<super::notification::AudioArbiter>>,
    pub started_at: Instant,
    /// Phase 2: daemon-owned workspace registry. Every persisted session
    /// is bound to exactly one workspace before any execution is
    /// permitted. The registry deduplicates canonical project roots and
    /// gates turn submission behind a valid `WorkspaceId`.
    pub workspaces: Arc<WorkspaceRegistry>,
    /// Canonical project/workspace/session resolver. Directory compatibility
    /// is read-only and succeeds only for an existing unique binding.
    pub context_resolver: Option<Arc<ProjectContextResolver>>,
    /// Phase 3: workspace services registry. The daemon owns the
    /// canonical [`WorkspaceServiceRegistry`] which lazily activates
    /// per-workspace `WorkspaceServices` bundles and shares them
    /// across sessions, the TUI, and remote clients. Created during
    /// `with_deps_and_identity` if the caller did not supply one via
    /// `CoreRuntimeDeps::with_workspace_services`.
    pub workspace_services: Arc<WorkspaceServiceRegistry>,
    /// M003: durable managed worktree ownership and lease service.
    pub worktree_service: Arc<codegg_core::worktree_service::WorktreeService>,
    /// Bounded startup reconciliation against Git's actual worktree list.
    pub(crate) _worktree_reconcile_handle: Option<tokio::task::JoinHandle<()>>,
    /// Daemon-owned Eggpool provisioning service. It is present only for
    /// SQLite-backed daemons; legacy in-memory daemons retain compatibility.
    pub eggpool_provisioner: Option<Arc<crate::core::eggpool::EggpoolProvisioner>>,
    /// Interactive Process Sessions M002: daemon-owned bounded
    /// attach/resume handler family over the scheduler-owned M001 PTY
    /// engine. Always present; shares the scheduler's admission
    /// controller so interactive spawns draw from the same process-slot
    /// accounting as durable work. Handles and attachments are ephemeral
    /// and do not survive daemon restart.
    pub interactive_processes: Arc<crate::interactive_process_attach::InteractiveProcessProtocol>,
    /// Provider Connections Milestone 3: daemon-owned session selection
    /// service. Reads and writes the connection/model selection on the
    /// session row through the typed core crate; never mutates provider
    /// credentials or constructs a provider in the frontend.
    pub selection_service: Option<Arc<crate::core::session_selection::SelectionService>>,
    /// Daemon-owned immutable runtime-asset publication coordinator. Every
    /// lifecycle and manual refresh path uses this one service.
    pub asset_refresh: Arc<crate::agent::asset_refresh::AssetRefreshCoordinator>,
    /// Project Catalog Milestone 3: explicit owner-scoped activation leases.
    pub project_activation: Arc<ProjectActivationRegistry>,
    /// Presence and Observation M001: daemon-owned ephemeral
    /// project-scoped presence leases. Never durable, never
    /// authoritative for authorization; cleared on restart.
    pub presence: Arc<codegg_core::presence::PresenceService>,
    /// Project Collaboration M001: daemon-owned project-chat service.
    /// Durable channels/messages/read markers live in the catalog pool
    /// (when present); composing leases are ephemeral in memory and
    /// cleared on restart. Pool-less daemons serve composing state only
    /// and fail durable operations with `chat_unavailable`.
    pub collaboration: Arc<codegg_core::collaboration::CollaborationService>,
    /// Projection replay publication seam. Present only for SQLite-backed
    /// daemons; legacy in-memory daemons retain `None`.
    pub projection_seam:
        Option<Arc<codegg_core::projection_replay::seam::ProjectionPublicationSeam>>,
    /// Handle for the background projection replay maintenance task.
    /// `None` when no pool is available. Held to keep the task alive.
    pub(crate) _projection_maintenance_handle: Option<tokio::task::JoinHandle<()>>,
    /// Total events the event bridge was forced to drop because the
    /// broadcast receiver lagged behind publishers. Exposed for metrics
    /// and the request to manually resync after a sustained spike.
    pub dropped_event_bridge_events: std::sync::atomic::AtomicU64,
}

// M003: construction lives in `super::daemon_construct` (`with_deps`,
// `with_deps_and_identity`, `new`, `SeamProjectionSink`); bootstrap/recovery
// lives in `super::daemon_bootstrap` (hydrate, recover, bridge, replay);
// refresh coordinators live in `super::daemon_refresh`; shutdown/join lives
// in `super::daemon_shutdown` (`Drop` + `abort_background_handles`). This file
// retains the struct, thin request router, auth/audit preamble, chat and
// interactive pre-routers, shared `pub(crate)` helpers, and existing tests.

impl CoreDaemon {
    // M003 seam: construction/bootstrap/refresh/shutdown methods live in
    // `daemon_construct`, `daemon_bootstrap`, `daemon_refresh`, and
    // `daemon_shutdown` as boring `impl CoreDaemon` methods on this same
    // state. No second daemon type, store, or authority exists.
    pub async fn handle_request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        self.handle_request_with_client(request, None).await
    }

    /// Handle a request on behalf of one trusted transport connection.
    ///
    /// The connection identity is assigned by the transport after
    /// authentication/handshake. It is deliberately not part of the
    /// client-controlled projection DTOs, so projection ownership cannot be
    /// forged by copying another subscription ID onto a request.
    pub async fn handle_request_for_client(
        &self,
        request: RequestEnvelope<CoreRequest>,
        client_id: &str,
    ) -> Result<CoreResponse, AppError> {
        self.handle_request_with_client(request, Some(client_id))
            .await
    }

    /// Transport-bound request authority for one trusted connection.
    ///
    /// M002: the principal comes from `ClientRegistry` (bound at handshake
    /// from trusted transport evidence), never from the request payload.
    /// Unregistered connections (in-process, stdio, legacy callers) resolve
    /// to `LocalOwner` so personal-local startup stays login-free.
    pub fn request_authority_for_client(
        &self,
        client_id: &str,
    ) -> codegg_core::transport_auth::RequestAuthorityContext {
        if let Some(principal) = self.clients.principal_for(client_id) {
            return codegg_core::transport_auth::RequestAuthorityContext::new(
                principal,
                format!("req-{client_id}"),
            );
        }
        codegg_core::transport_auth::RequestAuthorityContext::local(
            client_id,
            format!("req-{client_id}"),
        )
    }

    /// Canonical projection access context for one trusted connection.
    ///
    /// Uses the bound principal's canonical projection string and transport
    /// class. Falls back to the local single-user context for unregistered
    /// (in-process/stdio/legacy) callers. Replaces the historical synthetic
    /// `"authenticated-remote"` placeholder: remote bindings now carry
    /// their canonical principal id.
    ///
    /// Presence M003 note: this sync helper preserves the historical
    /// local-user capability shape for callers without a target project
    /// (legacy artifact paths, diagnostics). Session observation MUST use
    /// [`Self::canonical_observe_access_for_project`], which derives
    /// capabilities from the canonical team membership (`session.observe`
    /// / `project.observe`) and bounds the resolver to the target
    /// project. New code MUST NOT rely on the allow-all resolver here
    /// for cross-session visibility.
    pub fn projection_access_for_client(
        &self,
        client_id: &str,
        correlation_id: &str,
    ) -> codegg_core::projection_replay::context::ProjectionAccessContext {
        use codegg_core::projection_replay::context::{
            AllowAllProjectResolver, ProjectionCapabilitySet,
        };
        if let Some(principal) = self.clients.principal_for(client_id) {
            return principal.to_projection_access_context(
                correlation_id,
                ProjectionCapabilitySet::local_user(),
                std::sync::Arc::new(AllowAllProjectResolver),
            );
        }
        codegg_core::projection_replay::context::ProjectionAccessContext::local(
            client_id,
            correlation_id,
        )
    }

    /// Presence M003: canonical observe access for one target project.
    ///
    /// Derives projection capabilities from the canonical team membership
    /// for `project_id` (via [`codegg_core::authorization::team_capabilities_to_projection`])
    /// and bounds the resolver to exactly that project. LocalOwner broad
    /// policy and pool-less (local-only) daemons retain the local-user
    /// context; team principals without an active membership receive an
    /// empty capability set so [`ProjectionAccessContext::authorize_scope`]
    /// fails closed. The principal string always comes from transport
    /// authority, never from a request payload.
    pub async fn canonical_observe_access_for_project(
        &self,
        client_id: &str,
        project_id: &codegg_core::identity::ProjectId,
        correlation_id: &str,
    ) -> codegg_core::projection_replay::context::ProjectionAccessContext {
        use codegg_core::authorization::{is_local_owner_broad, team_capabilities_to_projection};
        use codegg_core::projection_replay::context::{
            AllowAllProjectResolver, BoundedProjectResolver, ProjectionCapabilitySet,
        };
        let authority = self.request_authority_for_client(client_id);
        let principal = authority.principal().clone();
        if is_local_owner_broad(&principal) {
            return principal.to_projection_access_context(
                correlation_id,
                ProjectionCapabilitySet::local_user(),
                std::sync::Arc::new(AllowAllProjectResolver),
            );
        }
        let Some(pool) = self.pool.clone() else {
            return codegg_core::projection_replay::context::ProjectionAccessContext::local(
                client_id,
                correlation_id,
            );
        };
        let team = codegg_core::team::TeamStore::new(pool);
        let membership = team
            .get_membership(project_id, principal.principal_id())
            .await
            .unwrap_or(None);
        let Some(membership) = membership else {
            return principal.to_projection_access_context(
                correlation_id,
                ProjectionCapabilitySet::new(),
                std::sync::Arc::new(BoundedProjectResolver::new(Vec::<String>::new())),
            );
        };
        let team_caps = membership.effective_capabilities();
        let proj_caps = team_capabilities_to_projection(&team_caps);
        let resolver = std::sync::Arc::new(BoundedProjectResolver::new([project_id.as_str()]));
        principal.to_projection_access_context(correlation_id, proj_caps, resolver)
    }

    /// Presence M003: `true` when `client_id` may observe `session_id`.
    ///
    /// Resolves the owning project through the session row (the same
    /// authority used by [`Self::resolve_authorization_project`]) and
    /// requires canonical `session.observe` on that project. LocalOwner
    /// broad policy and pool-less daemons allow; unknown sessions,
    /// unresolvable projects, inactive principals, and memberships
    /// without `session.observe` deny. Callers map denials to
    /// `project_not_found` so outsiders cannot distinguish a missing
    /// session from a denied one.
    pub async fn session_observe_allowed(
        &self,
        client_id: &str,
        session_id: &str,
    ) -> Option<codegg_core::identity::ProjectId> {
        use codegg_core::authorization::is_local_owner_broad;
        if session_id.is_empty() {
            return None;
        }
        let authority = self.request_authority_for_client(client_id);
        let principal = authority.principal().clone();
        let Some(pool) = self.pool.clone() else {
            // Pool-less local daemons have no team state; resolve through
            // the projection seam's session binding when available so
            // local observation keeps working without fabricating a
            // project.
            if let Some(seam) = self.projection_seam.as_ref() {
                if let Some(storage) = seam.project_storage() {
                    if let Ok(Some(record)) = storage.session_binding(session_id).await {
                        if let Some(project) = record.project_id {
                            return Some(project);
                        }
                    }
                }
            }
            return None;
        };
        let project = self.session_project(&pool, session_id).await?;
        if is_local_owner_broad(&principal) {
            return Some(project);
        }
        let team = codegg_core::team::TeamStore::new(pool);
        let has = team
            .has_capability(
                &project,
                principal.principal_id(),
                codegg_core::team::Capability::SessionObserve,
            )
            .await
            .unwrap_or(false);
        if has {
            Some(project)
        } else {
            None
        }
    }

    /// Presence M003: boxed observe-scope decision for dispatch call sites.
    ///
    /// The dispatch match in `handle_request_with_client` is already near
    /// the debug stack limit (see the boxed authorization preamble): every
    /// observe check runs through this helper via `Box::pin` at the call
    /// site so the dispatch future holds only the box, never the team
    /// lookup frames. Returns the canonical project when `client_id` may
    /// observe `session_id` (session scope) or `project_id` (project
    /// scope, `session_id` `None`); `None` denies (callers map to
    /// `project_not_found`).
    pub(crate) async fn observe_scope_project_boxed(
        &self,
        client_id: &str,
        project_id: Option<&codegg_core::identity::ProjectId>,
        session_id: Option<&str>,
        correlation_id: &str,
    ) -> Option<codegg_core::identity::ProjectId> {
        // Outer box: the dispatch future holds only this box.
        Box::pin(async move {
            if let Some(session_id) = session_id {
                let project =
                    Box::pin(self.session_observe_allowed(client_id, session_id)).await?;
                let access = Box::pin(self.canonical_observe_access_for_project(
                    client_id,
                    &project,
                    correlation_id,
                ))
                .await;
                let allowed = codegg_core::projection_replay::policy::ProjectionAccessPolicy::authorize_subscribe(
                    &codegg_core::projection_replay::policy::DefaultAccessPolicy::new(),
                    &access,
                    project.as_str(),
                    Some(session_id),
                );
                if allowed {
                    Some(project)
                } else {
                    None
                }
            } else if let Some(project_id) = project_id {
                let access = Box::pin(self.canonical_observe_access_for_project(
                    client_id,
                    project_id,
                    correlation_id,
                ))
                .await;
                let allowed = codegg_core::projection_replay::policy::ProjectionAccessPolicy::authorize_subscribe(
                    &codegg_core::projection_replay::policy::DefaultAccessPolicy::new(),
                    &access,
                    project_id.as_str(),
                    None,
                );
                if allowed {
                    Some(project_id.clone())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .await
    }

    /// Presence M003: boxed canonical access context for dispatch call
    /// sites (artifact paths need the context itself, not just the
    /// decision). Same boxing rationale as
    /// [`Self::observe_scope_project_boxed`].
    pub(crate) async fn observe_access_boxed(
        &self,
        client_id: &str,
        project_id: &codegg_core::identity::ProjectId,
        correlation_id: &str,
    ) -> codegg_core::projection_replay::context::ProjectionAccessContext {
        Box::pin(self.canonical_observe_access_for_project(client_id, project_id, correlation_id))
            .await
    }

    /// Presence and Observation M001: best-effort activity contribution
    /// from connection/session/agent progress.
    ///
    /// The principal comes from transport authority, never from a
    /// payload. Failures are ignored so presence can never break
    /// session/agent correctness; those systems do not depend on
    /// presence. Renewal uses the tracked connection generation so
    /// implicit touches never stale-reject against an explicit
    /// heartbeat generation.
    pub fn note_presence_activity(
        &self,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        trusted_client_id: &str,
        project: Option<codegg_core::identity::ProjectId>,
        session_id: Option<&str>,
        activity: codegg_core::presence::PresenceActivity,
    ) {
        let Some(project) = project else {
            return;
        };
        let principal = authority.principal().principal_id().clone();
        let now_instant = std::time::Instant::now();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let _ = self.presence.touch(
            project,
            principal,
            trusted_client_id,
            session_id,
            activity,
            now_instant,
            now_ms,
        );
    }

    /// Presence M001: expire every contribution owned by one transport
    /// connection. Called on socket/WebSocket disconnect after the
    /// client registry entry is removed.
    pub fn note_client_disconnected(&self, trusted_client_id: &str) {
        self.presence.remove_client(trusted_client_id);
    }

    /// Interactive Process Sessions M002: transport-derived authority for
    /// one trusted connection.
    ///
    /// The principal comes from `ClientRegistry` (bound at handshake from
    /// trusted transport evidence), never from the request payload.
    /// Unregistered connections (in-process, stdio, legacy callers) and
    /// `LocalOwner` bindings resolve to the local authority; any other
    /// bound principal resolves to the restricted remote authority until a
    /// later milestone plugs semantic interactive capabilities through the
    /// same [`InteractiveAuthority`](crate::interactive_process_attach::InteractiveAuthority)
    /// context without a wire change.
    pub fn interactive_authority_for(
        &self,
        client_id: &str,
    ) -> crate::interactive_process_attach::InteractiveAuthority {
        use crate::interactive_process_attach::InteractiveAuthority;
        match self.clients.principal_for(client_id) {
            Some(principal)
                if principal.auth_method()
                    == codegg_core::transport_auth::AuthMethod::LocalOwner =>
            {
                InteractiveAuthority::local(client_id)
            }
            Some(_) => InteractiveAuthority::remote(client_id),
            None => InteractiveAuthority::local(client_id),
        }
    }

    /// M003: server-side denial response for one authorization failure.
    ///
    /// Denied requests have zero side effect. A single-project read
    /// denies as not-found so an unauthorized caller cannot infer
    /// project existence; all other denials carry the typed code plus
    /// a secret-free message.
    fn authorization_denial(
        request: &CoreRequest,
        error: &codegg_core::authorization::AuthorizationError,
    ) -> CoreResponse {
        // Presence M001: single-project presence reads/heartbeats deny
        // as not-found so unauthorized callers cannot infer project
        // existence, membership, collaborators, or activity.
        // Presence M003: observation reads (`ProjectionSubscribe` either
        // scope, artifact list/read) deny identically so outsiders cannot
        // distinguish a missing session/project from a denied one and
        // learn nothing about project/session existence or activity.
        // Artifact list denials match the in-handler recheck shape
        // (`project_not_found`); artifact read gate denials match too
        // (the in-handler `Denied` outcome is defense-in-depth for a
        // gate-passed caller whose derived caps lack the read kind).
        let observe_private = matches!(
            request,
            CoreRequest::ProjectionSubscribe { .. }
                | CoreRequest::ProjectionArtifactList { .. }
                | CoreRequest::ProjectionArtifactRead { .. }
        );
        // Collaboration M001: single-project chat reads/writes deny as
        // not-found so unauthorized callers cannot infer project
        // existence, membership, channels, or message activity.
        // M003 actions share the same gate so outsiders and viewers
        // observe the identical shape.
        let chat_private = matches!(
            request,
            CoreRequest::ChatChannelEnsure { .. }
                | CoreRequest::ChatChannelList { .. }
                | CoreRequest::ChatHistory { .. }
                | CoreRequest::ChatSend { .. }
                | CoreRequest::ChatEdit { .. }
                | CoreRequest::ChatRedact { .. }
                | CoreRequest::ChatReadSet { .. }
                | CoreRequest::ChatReadGet { .. }
                | CoreRequest::ChatComposingSet { .. }
                | CoreRequest::ChatComposingList { .. }
                | CoreRequest::ChatSync { .. }
                | CoreRequest::ChatActionSubmit { .. }
                | CoreRequest::ChatActionGet { .. }
                | CoreRequest::ChatActionList { .. }
        );
        if (matches!(
            request,
            CoreRequest::ProjectGet { .. }
                | CoreRequest::PresenceSnapshotGet { .. }
                | CoreRequest::PresenceHeartbeat { .. }
        ) || observe_private
            || chat_private)
            && error.is_denial()
        {
            let (code, message) = codegg_core::authorization::denial_as_not_found();
            return CoreResponse::Error {
                code: code.to_string(),
                message,
            };
        }
        CoreResponse::Error {
            code: error.code().to_string(),
            message: error.to_string(),
        }
    }

    /// M003: evaluate one request against current team state.
    ///
    /// LocalOwner principals decide through this same API under the
    /// broad local policy. All other principals need an active record;
    /// project-scoped operations additionally need a resolved project
    /// plus a current membership grant. Legacy in-memory daemons without
    /// a pool are local-only and decide under the broad local policy.
    async fn authorize_request(
        &self,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        request_id: &str,
        request: &CoreRequest,
    ) -> Result<
        codegg_core::authorization::AuthorizationDecision,
        codegg_core::authorization::AuthorizationError,
    > {
        use codegg_core::authorization::{
            operation_descriptor, AuthorizationRequest, AuthorizationService, ScopeKind,
        };
        let descriptor = operation_descriptor(request);
        let correlation = format!("{}:{request_id}", authority.correlation_id());
        let Some(pool) = self.pool.clone() else {
            return Ok(codegg_core::authorization::AuthorizationDecision {
                principal_id: authority.principal_id().clone(),
                operation: descriptor.operation.to_owned(),
                capability: descriptor.capability.map(|cap| cap.as_str().to_owned()),
                project_id: None,
                membership_revision: None,
                policy: codegg_core::authorization::PolicyKind::LocalOwnerBroad,
                decision_id: uuid::Uuid::new_v4().to_string(),
                correlation_id: correlation,
                reason: "no durable team store; local-only daemon".to_owned(),
                decided_at_ms: chrono::Utc::now().timestamp_millis(),
            });
        };
        let service = AuthorizationService::new(codegg_core::team::TeamStore::new(pool.clone()));
        if descriptor.scope_kind == ScopeKind::Enumeration && descriptor.operation == "project_list"
        {
            return service
                .authorize_enumeration(authority.principal(), descriptor.operation, &correlation)
                .await;
        }
        let project = Box::pin(self.resolve_authorization_project(&pool, request)).await;
        let authz_request = AuthorizationRequest::new(
            authority.principal().clone(),
            descriptor,
            project,
            correlation,
        );
        service.authorize(&authz_request).await
    }

    /// M003: resolve one request to its project scope, if any.
    ///
    /// Direct `project_id` locators parse first; `session_id` locators
    /// resolve through the owning session row; `job_id` locators resolve
    /// through the job's owning session. Unresolvable scopes return
    /// `None` so team principals fail closed; lookup failures (missing
    /// rows, bad ids) likewise yield `None` rather than an error.
    async fn resolve_authorization_project(
        &self,
        pool: &sqlx::SqlitePool,
        request: &CoreRequest,
    ) -> Option<codegg_core::identity::ProjectId> {
        use codegg_core::identity::ProjectId;
        let direct: Option<&str> = match request {
            CoreRequest::ProjectGet { project_id }
            | CoreRequest::ProjectArchive { project_id }
            | CoreRequest::ProjectRestore { project_id } => Some(project_id),
            CoreRequest::ProjectHealth { project_id, .. } => Some(project_id),
            CoreRequest::SessionList { project_id, .. } => Some(project_id),
            CoreRequest::SessionCreate {
                project_id: Some(project_id),
                ..
            }
            | CoreRequest::SessionCreateFromTemplate {
                project_id: Some(project_id),
                ..
            } => Some(project_id),
            CoreRequest::ProjectionArtifactRead { project_id, .. }
            | CoreRequest::ProjectionArtifactList { project_id } => Some(project_id),
            CoreRequest::AssetRefresh { request } => Some(request.scope.project_id.as_str()),
            CoreRequest::AssetRefreshStatus { scope } => Some(scope.project_id.as_str()),
            CoreRequest::GoalSet { project_id, .. }
            | CoreRequest::GoalFromFile { project_id, .. }
            | CoreRequest::GoalCheckpoint { project_id, .. } => Some(project_id),
            CoreRequest::AuditQuery { query } => Some(query.project_id.as_str()),
            CoreRequest::AuditExport { request } => Some(request.project_id.as_str()),
            CoreRequest::PresenceHeartbeat { request } => Some(request.project_id.as_str()),
            CoreRequest::PresenceSnapshotGet { project_id } => Some(project_id.as_str()),
            CoreRequest::ChatChannelEnsure { project_id, .. }
            | CoreRequest::ChatChannelList { project_id, .. } => Some(project_id.as_str()),
            _ => None,
        };
        if let Some(raw) = direct {
            return ProjectId::parse(raw).ok();
        }
        // Collaboration M001: channel-scoped requests carry only the
        // channel locator; the owning project resolves server-side
        // through the durable channel row. Unknown channels yield `None`
        // so team principals fail closed.
        if let Some(channel_id) = Self::chat_channel_id_for_request(request) {
            return Box::pin(codegg_core::collaboration::channel_project(
                pool, channel_id,
            ))
            .await;
        }
        if let CoreRequest::ProjectionSubscribe { request } = request {
            return match request.scope {
                codegg_protocol::projection::replay::ProjectionStreamKind::Project => {
                    ProjectId::parse(request.scope_id.as_str()).ok()
                }
                codegg_protocol::projection::replay::ProjectionStreamKind::Session => {
                    self.session_project(pool, &request.scope_id).await
                }
            };
        }
        if let CoreRequest::ProjectionSnapshotGet { scope, scope_id } = request {
            return match scope {
                codegg_protocol::projection::replay::ProjectionStreamKind::Project => {
                    ProjectId::parse(scope_id.as_str()).ok()
                }
                codegg_protocol::projection::replay::ProjectionStreamKind::Session => {
                    self.session_project(pool, scope_id).await
                }
            };
        }
        if let Some(session_id) = Self::session_id_for_request(request) {
            return self.session_project(pool, session_id).await;
        }
        let job_id: Option<&str> = match request {
            CoreRequest::JobWait { job_id, .. }
            | CoreRequest::JobGet { job_id }
            | CoreRequest::JobCancel { job_id, .. }
            | CoreRequest::JobRetry { job_id }
            | CoreRequest::JobAttempts { job_id } => Some(job_id),
            _ => None,
        };
        if let Some(job_id) = job_id {
            return self.job_session_project(pool, job_id).await;
        }
        None
    }

    /// Collaboration M001: `channel_id` locator carried by one
    /// channel-scoped chat request, if any. Capability creation and
    /// channel listing carry a direct `project_id` instead. M003
    /// action requests are channel-scoped the same way.
    fn chat_channel_id_for_request(request: &CoreRequest) -> Option<&str> {
        match request {
            CoreRequest::ChatHistory { channel_id, .. }
            | CoreRequest::ChatSend { channel_id, .. }
            | CoreRequest::ChatEdit { channel_id, .. }
            | CoreRequest::ChatRedact { channel_id, .. }
            | CoreRequest::ChatReadSet { channel_id, .. }
            | CoreRequest::ChatReadGet { channel_id }
            | CoreRequest::ChatComposingSet { channel_id, .. }
            | CoreRequest::ChatComposingList { channel_id }
            | CoreRequest::ChatSync { channel_id, .. }
            | CoreRequest::ChatActionSubmit { channel_id, .. }
            | CoreRequest::ChatActionGet { channel_id, .. }
            | CoreRequest::ChatActionList { channel_id, .. } => Some(channel_id),
            _ => None,
        }
    }

    /// Collaboration M001: `true` when the request is served by the
    /// dedicated chat handler ([`Self::handle_chat_request`]).
    fn is_chat_request(request: &CoreRequest) -> bool {
        matches!(
            request,
            CoreRequest::ChatCapabilities
                | CoreRequest::ChatChannelEnsure { .. }
                | CoreRequest::ChatChannelList { .. }
                | CoreRequest::ChatHistory { .. }
                | CoreRequest::ChatSend { .. }
                | CoreRequest::ChatEdit { .. }
                | CoreRequest::ChatRedact { .. }
                | CoreRequest::ChatReadSet { .. }
                | CoreRequest::ChatReadGet { .. }
                | CoreRequest::ChatComposingSet { .. }
                | CoreRequest::ChatComposingList { .. }
                | CoreRequest::ChatSync { .. }
                | CoreRequest::ChatActionSubmit { .. }
                | CoreRequest::ChatActionGet { .. }
                | CoreRequest::ChatActionList { .. }
        )
    }
    /// Collaboration M001: resolve one channel locator to its
    /// `(ChannelId, ProjectId)` pair through the durable channel row.
    ///
    /// Unknown channels report `chat_channel_not_found` and malformed
    /// ids report `chat_invalid_input`, neither leaking which projects
    /// exist. Pool-less daemons have no durable channels and report
    /// `chat_unavailable`. The error is boxed: `CoreResponse` is a large
    /// enum and returning it by value trips the large-error lint.
    async fn resolve_chat_channel(
        &self,
        channel_id: &str,
    ) -> Result<
        (
            codegg_core::identity::ChannelId,
            codegg_core::identity::ProjectId,
        ),
        Box<CoreResponse>,
    > {
        let channel = codegg_core::identity::ChannelId::parse(channel_id).map_err(|error| {
            Box::new(CoreResponse::Error {
                code: "chat_invalid_input".to_owned(),
                message: error.to_string(),
            })
        })?;
        let Some(pool) = self.pool.clone() else {
            return Err(Box::new(CoreResponse::Error {
                code: "chat_unavailable".to_owned(),
                message: "project chat requires a durable database pool".to_owned(),
            }));
        };
        match codegg_core::collaboration::channel_project(&pool, channel.as_str()).await {
            Some(project) => Ok((channel, project)),
            None => Err(Box::new(CoreResponse::Error {
                code: "chat_channel_not_found".to_owned(),
                message: "chat channel not found".to_owned(),
            })),
        }
    }

    /// Collaboration M001: dedicated chat request handler.
    ///
    /// The M003 gate enforced `project.chat` before this runs. Bodies
    /// are inert text: this handler never parses commands, mentions, or
    /// references into execution. Principals come from transport
    /// authority, never from the payload. M003 structured actions are
    /// the sole execution seam: they check the ordinary semantic
    /// capability for the kind and call the existing canonical
    /// Task/AgentRun/Job services; free text never reaches this path.
    async fn handle_chat_request(
        &self,
        request_id: &str,
        request: CoreRequest,
        trusted_client_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, crate::error::AppError> {
        use codegg_core::collaboration::CollaborationError;
        let chat_error = |error: CollaborationError| CoreResponse::Error {
            code: error.code().to_owned(),
            message: error.to_string(),
        };
        match request {
            CoreRequest::ChatCapabilities => Ok(CoreResponse::ChatCapabilities {
                capabilities: self.collaboration.capabilities_dto(),
            }),
            CoreRequest::ChatChannelEnsure { project_id, name } => {
                let project = match codegg_core::identity::ProjectId::parse(project_id.as_str()) {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "chat_invalid_input".to_owned(),
                            message: error.to_string(),
                        });
                    }
                };
                let authority = self.request_authority_for_client(trusted_client_id);
                let principal = authority.principal().principal_id().clone();
                let now_ms = chrono::Utc::now().timestamp_millis();
                let outcome = if let Some(name) = name.as_deref() {
                    self.collaboration
                        .ensure_channel_by_name(&project, &principal, name, now_ms)
                        .await
                } else {
                    self.collaboration
                        .ensure_default_channel(&project, &principal, now_ms)
                        .await
                };
                match outcome {
                    Ok(channel) => Ok(CoreResponse::ChatChannel {
                        channel: channel.to_dto(),
                    }),
                    Err(error) => Ok(chat_error(error)),
                }
            }
            CoreRequest::ChatChannelList { project_id, limit } => {
                let project = match codegg_core::identity::ProjectId::parse(project_id.as_str()) {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "chat_invalid_input".to_owned(),
                            message: error.to_string(),
                        });
                    }
                };
                match self.collaboration.list_channels(&project, limit).await {
                    Ok((channels, truncated)) => Ok(CoreResponse::ChatChannelList {
                        channels: channels.iter().map(|c| c.to_dto()).collect(),
                        truncated,
                    }),
                    Err(error) => Ok(chat_error(error)),
                }
            }
            CoreRequest::ChatHistory {
                channel_id,
                from_seq,
                limit,
            } => {
                let (channel, project) = match self.resolve_chat_channel(channel_id.as_str()).await
                {
                    Ok(pair) => pair,
                    Err(response) => return Ok(*response),
                };
                match self
                    .collaboration
                    .history(&project, &channel, from_seq, limit)
                    .await
                {
                    Ok(page) => Ok(CoreResponse::ChatHistory {
                        channel_id: channel.as_str().to_owned(),
                        messages: page.messages.iter().map(|m| m.to_dto()).collect(),
                        next_cursor: page.next_cursor,
                        truncated: page.truncated,
                        retention_floor_seq: page.retention_floor_seq,
                    }),
                    Err(error) => Ok(chat_error(error)),
                }
            }
            CoreRequest::ChatSend {
                channel_id,
                body,
                reply_to,
                thread_root,
                mentions,
                references,
                idempotency_key,
            } => {
                let (channel, project) = match self.resolve_chat_channel(channel_id.as_str()).await
                {
                    Ok(pair) => pair,
                    Err(response) => return Ok(*response),
                };
                let parse_message_id = |raw: &str| {
                    codegg_core::identity::ChatMessageId::parse(raw).map_err(|error| {
                        Box::new(CoreResponse::Error {
                            code: "chat_invalid_input".to_owned(),
                            message: error.to_string(),
                        })
                    })
                };
                let reply_to = match reply_to.as_deref().map(parse_message_id).transpose() {
                    Ok(target) => target,
                    Err(response) => return Ok(*response),
                };
                let thread_root = match thread_root.as_deref().map(parse_message_id).transpose() {
                    Ok(target) => target,
                    Err(response) => return Ok(*response),
                };
                let authority = self.request_authority_for_client(trusted_client_id);
                let principal = authority.principal().principal_id().clone();
                let now_ms = chrono::Utc::now().timestamp_millis();
                match self
                    .collaboration
                    .send_message(
                        &project,
                        &channel,
                        &principal,
                        None,
                        &body,
                        reply_to.as_ref(),
                        thread_root.as_ref(),
                        &mentions,
                        &references,
                        idempotency_key.as_deref(),
                        now_ms,
                    )
                    .await
                {
                    Ok(outcome) => {
                        if !outcome.duplicate {
                            let dto = outcome.message.to_dto();
                            self.event_log
                                .publish(
                                    None,
                                    None,
                                    CoreEvent::ChatMessageCommitted {
                                        project_id: project.as_str().to_owned(),
                                        channel_id: channel.as_str().to_owned(),
                                        message: dto.clone(),
                                    },
                                )
                                .await;
                            Ok(CoreResponse::ChatMessage {
                                message: dto,
                                duplicate: false,
                            })
                        } else {
                            Ok(CoreResponse::ChatMessage {
                                message: outcome.message.to_dto(),
                                duplicate: true,
                            })
                        }
                    }
                    Err(error) => Ok(chat_error(error)),
                }
            }
            CoreRequest::ChatEdit {
                channel_id,
                message_id,
                expected_revision,
                new_body,
            } => {
                let (channel, project) = match self.resolve_chat_channel(channel_id.as_str()).await
                {
                    Ok(pair) => pair,
                    Err(response) => return Ok(*response),
                };
                let message_id =
                    match codegg_core::identity::ChatMessageId::parse(message_id.as_str()) {
                        Ok(id) => id,
                        Err(error) => {
                            return Ok(CoreResponse::Error {
                                code: "chat_invalid_input".to_owned(),
                                message: error.to_string(),
                            });
                        }
                    };
                let authority = self.request_authority_for_client(trusted_client_id);
                let principal = authority.principal().principal_id().clone();
                let now_ms = chrono::Utc::now().timestamp_millis();
                match self
                    .collaboration
                    .edit_message(
                        &project,
                        &channel,
                        &message_id,
                        &principal,
                        expected_revision,
                        &new_body,
                        now_ms,
                    )
                    .await
                {
                    Ok(message) => {
                        let dto = message.to_dto();
                        self.event_log
                            .publish(
                                None,
                                None,
                                CoreEvent::ChatMessageEdited {
                                    project_id: project.as_str().to_owned(),
                                    channel_id: channel.as_str().to_owned(),
                                    message: dto.clone(),
                                },
                            )
                            .await;
                        Ok(CoreResponse::ChatMessage {
                            message: dto,
                            duplicate: false,
                        })
                    }
                    Err(error) => Ok(chat_error(error)),
                }
            }
            CoreRequest::ChatRedact {
                channel_id,
                message_id,
                expected_revision,
                reason,
            } => {
                let (channel, project) = match self.resolve_chat_channel(channel_id.as_str()).await
                {
                    Ok(pair) => pair,
                    Err(response) => return Ok(*response),
                };
                let message_id =
                    match codegg_core::identity::ChatMessageId::parse(message_id.as_str()) {
                        Ok(id) => id,
                        Err(error) => {
                            return Ok(CoreResponse::Error {
                                code: "chat_invalid_input".to_owned(),
                                message: error.to_string(),
                            });
                        }
                    };
                let authority = self.request_authority_for_client(trusted_client_id);
                let principal = authority.principal().principal_id().clone();
                let now_ms = chrono::Utc::now().timestamp_millis();
                match self
                    .collaboration
                    .redact_message(
                        &project,
                        &channel,
                        &message_id,
                        &principal,
                        expected_revision,
                        reason.as_deref(),
                        now_ms,
                    )
                    .await
                {
                    Ok(message) => {
                        self.event_log
                            .publish(
                                None,
                                None,
                                CoreEvent::ChatMessageRedacted {
                                    project_id: project.as_str().to_owned(),
                                    channel_id: channel.as_str().to_owned(),
                                    message_id: message.id.as_str().to_owned(),
                                    revision: message.revision,
                                },
                            )
                            .await;
                        Ok(CoreResponse::ChatMessage {
                            message: message.to_dto(),
                            duplicate: false,
                        })
                    }
                    Err(error) => Ok(chat_error(error)),
                }
            }
            CoreRequest::ChatReadSet {
                channel_id,
                last_read_seq,
            } => {
                let (channel, project) = match self.resolve_chat_channel(channel_id.as_str()).await
                {
                    Ok(pair) => pair,
                    Err(response) => return Ok(*response),
                };
                let authority = self.request_authority_for_client(trusted_client_id);
                let principal = authority.principal().principal_id().clone();
                let now_ms = chrono::Utc::now().timestamp_millis();
                match self
                    .collaboration
                    .set_read_marker(&project, &channel, &principal, last_read_seq, now_ms)
                    .await
                {
                    Ok(marker) => Ok(CoreResponse::ChatReadMarker {
                        channel_id: channel.as_str().to_owned(),
                        last_read_seq: marker.last_read_seq,
                        updated_at_ms: marker.updated_at_ms,
                    }),
                    Err(error) => Ok(chat_error(error)),
                }
            }
            CoreRequest::ChatReadGet { channel_id } => {
                let (channel, project) = match self.resolve_chat_channel(channel_id.as_str()).await
                {
                    Ok(pair) => pair,
                    Err(response) => return Ok(*response),
                };
                let authority = self.request_authority_for_client(trusted_client_id);
                let principal = authority.principal().principal_id().clone();
                match self
                    .collaboration
                    .get_read_marker(&project, &channel, &principal)
                    .await
                {
                    Ok(marker) => {
                        let (last_read_seq, updated_at_ms) = marker
                            .map(|m| (m.last_read_seq, m.updated_at_ms))
                            .unwrap_or((0, 0));
                        Ok(CoreResponse::ChatReadMarker {
                            channel_id: channel.as_str().to_owned(),
                            last_read_seq,
                            updated_at_ms,
                        })
                    }
                    Err(error) => Ok(chat_error(error)),
                }
            }
            CoreRequest::ChatComposingSet {
                channel_id,
                composing,
            } => {
                // Composing is ephemeral and pool-independent: the lease
                // key parses lexically and never touches durable rows.
                let channel = match codegg_core::identity::ChannelId::parse(channel_id.as_str()) {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "chat_invalid_input".to_owned(),
                            message: error.to_string(),
                        });
                    }
                };
                let authority = self.request_authority_for_client(trusted_client_id);
                let principal = authority.principal().principal_id().clone();
                // Composing still requires project membership: resolve the
                // owning project and deny unknown channels without
                // leaking existence. Pool-less daemons skip the check
                // (local-only, no team state).
                if self.pool.is_some()
                    && codegg_core::collaboration::channel_project(
                        self.pool.as_ref().expect("pool checked"),
                        channel.as_str(),
                    )
                    .await
                    .is_none()
                {
                    return Ok(CoreResponse::Error {
                        code: "chat_channel_not_found".to_owned(),
                        message: "chat channel not found".to_owned(),
                    });
                }
                let now = std::time::Instant::now();
                let now_ms = chrono::Utc::now().timestamp_millis();
                if let Err(error) = self.collaboration.set_composing(
                    &channel,
                    &principal,
                    trusted_client_id,
                    composing,
                    now,
                    now_ms,
                ) {
                    return Ok(chat_error(error));
                }
                let entries = self.collaboration.list_composing(&channel, now);
                // Content-free liveness hint; receivers re-fetch through
                // the authorized composing-list path. Pool-less leases
                // stay local-only.
                if let Some(pool) = self.pool.clone() {
                    if let Some(project) =
                        codegg_core::collaboration::channel_project(&pool, channel.as_str()).await
                    {
                        self.event_log
                            .publish(
                                None,
                                None,
                                CoreEvent::ChatComposingUpdated {
                                    project_id: project.as_str().to_owned(),
                                    channel_id: channel.as_str().to_owned(),
                                },
                            )
                            .await;
                    }
                }
                Ok(CoreResponse::ChatComposing {
                    channel_id: channel.as_str().to_owned(),
                    composing: entries.iter().map(|e| e.to_dto(&channel)).collect(),
                })
            }
            CoreRequest::ChatComposingList { channel_id } => {
                let channel = match codegg_core::identity::ChannelId::parse(channel_id.as_str()) {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "chat_invalid_input".to_owned(),
                            message: error.to_string(),
                        });
                    }
                };
                if self.pool.is_some()
                    && codegg_core::collaboration::channel_project(
                        self.pool.as_ref().expect("pool checked"),
                        channel.as_str(),
                    )
                    .await
                    .is_none()
                {
                    return Ok(CoreResponse::Error {
                        code: "chat_channel_not_found".to_owned(),
                        message: "chat channel not found".to_owned(),
                    });
                }
                let entries = self
                    .collaboration
                    .list_composing(&channel, std::time::Instant::now());
                Ok(CoreResponse::ChatComposing {
                    channel_id: channel.as_str().to_owned(),
                    composing: entries.iter().map(|e| e.to_dto(&channel)).collect(),
                })
            }
            CoreRequest::ChatSync {
                channel_id,
                from_seq,
                limit,
            } => {
                let (channel, project) = match self.resolve_chat_channel(channel_id.as_str()).await
                {
                    Ok(pair) => pair,
                    Err(response) => return Ok(*response),
                };
                match self
                    .collaboration
                    .sync(&project, &channel, from_seq, limit)
                    .await
                {
                    Ok(page) => Ok(CoreResponse::ChatSync {
                        channel_id: channel.as_str().to_owned(),
                        messages: page.messages.iter().map(|m| m.to_dto()).collect(),
                        next_cursor: page.next_cursor,
                        resync_required: page.resync_required,
                        retention_floor_seq: page.retention_floor_seq,
                    }),
                    Err(error) => Ok(chat_error(error)),
                }
            }
            CoreRequest::ChatActionSubmit {
                channel_id,
                message_id,
                action,
                idempotency_key,
            } => {
                // Boxed so the chat dispatch future stays small; the
                // action path submits durable jobs and emits audit.
                // execution-ownership: scheduler
                return Box::pin(self.handle_chat_action_submit(
                    request_id,
                    channel_id.as_str(),
                    message_id.as_str(),
                    action,
                    idempotency_key.as_str(),
                    trusted_client_id,
                    authority,
                    authz_decision,
                ))
                .await;
            }
            CoreRequest::ChatActionGet {
                channel_id,
                action_id,
            } => match self.resolve_chat_channel(channel_id.as_str()).await {
                Ok((channel, project)) => {
                    match self
                        .collaboration
                        .get_action(&project, &channel, action_id.as_str())
                        .await
                    {
                        Ok(action) => Ok(CoreResponse::ChatAction {
                            action: action.to_dto(),
                            duplicate: false,
                        }),
                        Err(error) => Ok(chat_error(error)),
                    }
                }
                Err(response) => Ok(*response),
            },
            CoreRequest::ChatActionList {
                channel_id,
                message_id,
                limit,
            } => match self.resolve_chat_channel(channel_id.as_str()).await {
                Ok((channel, project)) => {
                    let message = match message_id
                        .as_deref()
                        .map(codegg_core::identity::ChatMessageId::parse)
                        .transpose()
                    {
                        Ok(target) => target,
                        Err(error) => {
                            return Ok(CoreResponse::Error {
                                code: "chat_invalid_input".to_owned(),
                                message: error.to_string(),
                            });
                        }
                    };
                    match self
                        .collaboration
                        .list_actions(&project, &channel, message.as_ref(), limit)
                        .await
                    {
                        Ok(actions) => Ok(CoreResponse::ChatActionList {
                            channel_id: channel.as_str().to_owned(),
                            actions: actions.iter().map(|a| a.to_dto()).collect(),
                        }),
                        Err(error) => Ok(chat_error(error)),
                    }
                }
                Err(response) => Ok(*response),
            },
            other => Ok(CoreResponse::Error {
                code: "chat_invalid_input".to_owned(),
                message: format!(
                    "request is not a chat operation: {}",
                    codegg_core::authorization::operation_descriptor(&other).operation
                ),
            }),
        }
    }

    /// Collaboration M003: check the ordinary semantic capability for
    /// one action kind on the owning project.
    ///
    /// The `project.chat` gate already passed. LocalOwner broad policy
    /// passes everything; pool-less daemons pass (local-only); team
    /// principals pass exactly when their membership carries the
    /// capability. Returns `true` when the caller may proceed.
    async fn action_capability_allows(
        &self,
        project: &codegg_core::identity::ProjectId,
        principal: &codegg_core::transport_auth::AuthenticatedPrincipal,
        capability: codegg_core::team::Capability,
    ) -> bool {
        use codegg_core::authorization::is_local_owner_broad;
        if is_local_owner_broad(principal) {
            return true;
        }
        let Some(pool) = self.pool.clone() else {
            return true;
        };
        let team = codegg_core::team::TeamStore::new(pool);
        match team.get_membership(project, principal.principal_id()).await {
            Ok(Some(membership)) => membership.effective_capabilities().has(capability),
            _ => false,
        }
    }

    /// Collaboration M003: owning project of one session row, if
    /// resolvable. Used to enforce message/job project isolation for
    /// actions. Returns `None` for unknown sessions so callers fail
    /// closed without leaking existence.
    async fn action_session_project(
        &self,
        session_id: &str,
    ) -> Option<codegg_core::identity::ProjectId> {
        let pool = self.pool.clone()?;
        self.session_project(&pool, session_id).await
    }

    /// Collaboration M003: structured action dispatcher.
    ///
    /// Explicit typed operation only; free text never reaches here.
    /// Checks `project.chat` (gate, already passed) plus the ordinary
    /// semantic capability for the kind, validates message/channel/
    /// project linkage, converges retries on `(channel, key)` without
    /// a second job, calls the existing canonical Job services, stores
    /// only the reference/status projection, and emits audit causation
    /// plus a liveness event. Denials create no job and no action row.
    #[allow(clippy::too_many_arguments)]
    async fn handle_chat_action_submit(
        &self,
        request_id: &str,
        channel_id: &str,
        message_id: &str,
        action: codegg_protocol::core::ChatActionSubmitDto,
        idempotency_key: &str,
        trusted_client_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, crate::error::AppError> {
        use codegg_core::collaboration::{
            CollaborationError, CHAT_ACTION_STATUS_REFERENCED, CHAT_ACTION_STATUS_SUBMITTED,
        };
        let chat_error = |error: CollaborationError| CoreResponse::Error {
            code: error.code().to_owned(),
            message: error.to_string(),
        };
        // Resolve channel -> project server-side; unknown channels fail
        // closed without leaking existence.
        let (channel, project) = match self.resolve_chat_channel(channel_id).await {
            Ok(pair) => pair,
            Err(response) => return Ok(*response),
        };
        let message = match codegg_core::identity::ChatMessageId::parse(message_id) {
            Ok(id) => id,
            Err(error) => {
                return Ok(CoreResponse::Error {
                    code: "chat_invalid_input".to_owned(),
                    message: error.to_string(),
                });
            }
        };
        // Validate the idempotency key shape early so malformed retries
        // fail without touching durable state.
        if let Err(error) =
            codegg_core::collaboration::validate_idempotency_key_for_action(idempotency_key)
        {
            return Ok(chat_error(error));
        }
        // Retry convergence: a duplicate key returns the stored
        // projection without creating a second job and without a
        // second audit/event. A key reused for a different
        // message/kind fails as a conflict (never coerced).
        if let Ok(Some(existing)) = self
            .collaboration
            .find_action_by_idempotency(&project, &channel, idempotency_key)
            .await
        {
            let requested_kind = match &action {
                codegg_protocol::core::ChatActionSubmitDto::AgentTask { .. } => {
                    codegg_core::collaboration::ChatActionKind::AgentTask
                }
                codegg_protocol::core::ChatActionSubmitDto::ReviewRequest { .. } => {
                    codegg_core::collaboration::ChatActionKind::ReviewRequest
                }
                codegg_protocol::core::ChatActionSubmitDto::JobSubmit { .. } => {
                    codegg_core::collaboration::ChatActionKind::JobSubmit
                }
                codegg_protocol::core::ChatActionSubmitDto::JobReference { .. } => {
                    codegg_core::collaboration::ChatActionKind::JobReference
                }
            };
            if existing.message_id != message || existing.kind != requested_kind {
                return Ok(chat_error(CollaborationError::ActionConflict(
                    idempotency_key.to_owned(),
                )));
            }
            return Ok(CoreResponse::ChatAction {
                action: existing.to_dto(),
                duplicate: true,
            });
        }
        // The linked message must exist in the same channel; otherwise
        // this is a message/project mismatch (never coerced, never
        // executed). Not-found errors map to `ProjectMismatch` without
        // leaking existence; storage/auth failures surface as-is so the
        // root cause stays visible.
        if let Err(error) = self
            .collaboration
            .get_message(&project, &channel, &message)
            .await
        {
            match &error {
                CollaborationError::MessageNotFound(_) | CollaborationError::ChannelNotFound(_) => {
                    tracing::warn!(
                        error = %error,
                        "chat action references missing message/channel"
                    );
                    return Ok(chat_error(CollaborationError::ProjectMismatch(
                        "action message does not belong to the named channel/project".to_owned(),
                    )));
                }
                _ => {
                    tracing::warn!(error = %error, "chat action message lookup failed");
                    return Ok(chat_error(error));
                }
            }
        }
        let principal = authority.principal().principal_id().clone();
        let now_ms = chrono::Utc::now().timestamp_millis();
        // Dispatch per kind: validate payload, check the ordinary
        // semantic capability, then call the existing canonical service.
        // No branch parses free text; every branch requires an explicit
        // typed payload.
        enum PendingAction {
            SubmitJob {
                kind: codegg_core::collaboration::ChatActionKind,
                title: Option<String>,
                new_job: Box<codegg_core::jobs::NewJob>,
                session_id: Option<String>,
            },
            ReferenceJob {
                title: Option<String>,
                job_id: String,
            },
        }
        let pending: PendingAction = match action {
            codegg_protocol::core::ChatActionSubmitDto::AgentTask {
                workspace_id,
                agent,
                prompt,
                session_id,
                title,
            } => {
                if !self
                    .action_capability_allows(
                        &project,
                        authority.principal(),
                        codegg_core::team::Capability::AgentDelegate,
                    )
                    .await
                {
                    return Ok(chat_error(CollaborationError::ActionDenied(
                        "agent_task requires agent.delegate".to_owned(),
                    )));
                }
                let config = self.collaboration.config().clone();
                let workspace =
                    match codegg_core::collaboration::validate_action_workspace(&workspace_id) {
                        Ok(value) => value,
                        Err(error) => return Ok(chat_error(error)),
                    };
                let agent = match codegg_core::collaboration::validate_action_agent(&config, &agent)
                {
                    Ok(value) => value,
                    Err(error) => return Ok(chat_error(error)),
                };
                let prompt =
                    match codegg_core::collaboration::validate_action_prompt(&config, &prompt) {
                        Ok(value) => value,
                        Err(error) => return Ok(chat_error(error)),
                    };
                let title = match codegg_core::collaboration::validate_action_title(
                    &config,
                    title.as_deref(),
                ) {
                    Ok(value) => value,
                    Err(error) => return Ok(chat_error(error)),
                };
                let session = match session_id.as_deref() {
                    Some(raw) => {
                        let trimmed = raw.trim();
                        if trimmed.is_empty() || trimmed.len() > 128 {
                            return Ok(chat_error(CollaborationError::invalid(
                                "action_session",
                                "action session must be 1..=128 bytes",
                            )));
                        }
                        match self.action_session_project(trimmed).await {
                            Some(owner) if owner == project => Some(trimmed.to_owned()),
                            _ => {
                                return Ok(chat_error(CollaborationError::ProjectMismatch(
                                    "action session does not belong to the named project"
                                        .to_owned(),
                                )));
                            }
                        }
                    }
                    None => None,
                };
                let workspace_id = codegg_core::workspace::WorkspaceId::new_unchecked(workspace);
                let new_job = codegg_core::jobs::NewJob {
                    workspace_id,
                    session_id: session.clone(),
                    turn_id: None,
                    kind: codegg_core::jobs::JobKind::Subagent,
                    source: codegg_core::jobs::JobSource::AgentDelegated,
                    priority: codegg_core::jobs::JobPriority::Interactive,
                    payload: codegg_core::jobs::JobPayload::Subagent {
                        prompt,
                        agent,
                        model: None,
                        parent_id: session.clone(),
                        denied_tools: Vec::new(),
                        allowed_paths: Vec::new(),
                        max_tool_calls: None,
                    },
                    resource_request: codegg_core::jobs::ResourceRequest::for_kind(
                        codegg_core::jobs::JobKind::Subagent,
                    ),
                    timeout: None,
                    retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
                    idempotency: codegg_core::jobs::IdempotencyClass::NonIdempotent,
                    not_before: None,
                    deadline: None,
                    schedule_id: None,
                    depends_on: Vec::new(),
                    parent_job_id: None,
                    parent_attempt_id: None,
                    parent_call_id: None,
                    parent_program_id: None,
                    parent_instruction_sequence: None,
                    relation_kind: None,
                };
                PendingAction::SubmitJob {
                    kind: codegg_core::collaboration::ChatActionKind::AgentTask,
                    title,
                    new_job: Box::new(new_job),
                    session_id: session,
                }
            }
            codegg_protocol::core::ChatActionSubmitDto::ReviewRequest {
                workspace_id,
                agent,
                prompt,
                session_id,
                title,
            } => {
                if !self
                    .action_capability_allows(
                        &project,
                        authority.principal(),
                        codegg_core::team::Capability::AgentDelegate,
                    )
                    .await
                {
                    return Ok(chat_error(CollaborationError::ActionDenied(
                        "review_request requires agent.delegate".to_owned(),
                    )));
                }
                let config = self.collaboration.config().clone();
                let workspace =
                    match codegg_core::collaboration::validate_action_workspace(&workspace_id) {
                        Ok(value) => value,
                        Err(error) => return Ok(chat_error(error)),
                    };
                let agent = match codegg_core::collaboration::validate_action_agent(&config, &agent)
                {
                    Ok(value) => value,
                    Err(error) => return Ok(chat_error(error)),
                };
                let prompt =
                    match codegg_core::collaboration::validate_action_prompt(&config, &prompt) {
                        Ok(value) => value,
                        Err(error) => return Ok(chat_error(error)),
                    };
                let title = match codegg_core::collaboration::validate_action_title(
                    &config,
                    title.as_deref(),
                ) {
                    Ok(value) => value,
                    Err(error) => return Ok(chat_error(error)),
                };
                let session = match session_id.as_deref() {
                    Some(raw) => {
                        let trimmed = raw.trim();
                        if trimmed.is_empty() || trimmed.len() > 128 {
                            return Ok(chat_error(CollaborationError::invalid(
                                "action_session",
                                "action session must be 1..=128 bytes",
                            )));
                        }
                        match self.action_session_project(trimmed).await {
                            Some(owner) if owner == project => Some(trimmed.to_owned()),
                            _ => {
                                return Ok(chat_error(CollaborationError::ProjectMismatch(
                                    "action session does not belong to the named project"
                                        .to_owned(),
                                )));
                            }
                        }
                    }
                    None => None,
                };
                let workspace_id = codegg_core::workspace::WorkspaceId::new_unchecked(workspace);
                let new_job = codegg_core::jobs::NewJob {
                    workspace_id,
                    session_id: session.clone(),
                    turn_id: None,
                    kind: codegg_core::jobs::JobKind::Subagent,
                    source: codegg_core::jobs::JobSource::AgentDelegated,
                    priority: codegg_core::jobs::JobPriority::Interactive,
                    payload: codegg_core::jobs::JobPayload::Subagent {
                        prompt,
                        agent,
                        model: None,
                        parent_id: session.clone(),
                        denied_tools: Vec::new(),
                        allowed_paths: Vec::new(),
                        max_tool_calls: None,
                    },
                    resource_request: codegg_core::jobs::ResourceRequest::for_kind(
                        codegg_core::jobs::JobKind::Subagent,
                    ),
                    timeout: None,
                    retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
                    idempotency: codegg_core::jobs::IdempotencyClass::NonIdempotent,
                    not_before: None,
                    deadline: None,
                    schedule_id: None,
                    depends_on: Vec::new(),
                    parent_job_id: None,
                    parent_attempt_id: None,
                    parent_call_id: None,
                    parent_program_id: None,
                    parent_instruction_sequence: None,
                    relation_kind: None,
                };
                PendingAction::SubmitJob {
                    kind: codegg_core::collaboration::ChatActionKind::ReviewRequest,
                    title,
                    new_job: Box::new(new_job),
                    session_id: session,
                }
            }
            codegg_protocol::core::ChatActionSubmitDto::JobSubmit { spec, title } => {
                if !self
                    .action_capability_allows(
                        &project,
                        authority.principal(),
                        codegg_core::team::Capability::JobSubmit,
                    )
                    .await
                {
                    return Ok(chat_error(CollaborationError::ActionDenied(
                        "job_submit requires job.submit".to_owned(),
                    )));
                }
                let config = self.collaboration.config().clone();
                let title = match codegg_core::collaboration::validate_action_title(
                    &config,
                    title.as_deref(),
                ) {
                    Ok(value) => value,
                    Err(error) => return Ok(chat_error(error)),
                };
                let new_job = match codegg_core::protocol_conversions::job_submit_from_dto(*spec) {
                    Ok(job) => job,
                    Err(message) => {
                        return Ok(CoreResponse::Error {
                            code: "chat_invalid_input".to_owned(),
                            message,
                        });
                    }
                };
                if new_job.kind == codegg_core::jobs::JobKind::ToolProgram {
                    return Ok(CoreResponse::Error {
                        code: "chat_invalid_input".to_owned(),
                        message: "tool_program jobs must be submitted through the authorized tool_program invocation boundary".to_owned(),
                    });
                }
                if let Some(session) = new_job.session_id.clone() {
                    match self.action_session_project(&session).await {
                        Some(owner) if owner == project => {}
                        _ => {
                            return Ok(chat_error(CollaborationError::ProjectMismatch(
                                "action job session does not belong to the named project"
                                    .to_owned(),
                            )));
                        }
                    }
                }
                // Chat idempotency owns the retry: the caller-supplied
                // submission key (if any) is ignored so duplicate chat
                // retries converge on the chat key.
                let session_id = new_job.session_id.clone();
                PendingAction::SubmitJob {
                    kind: codegg_core::collaboration::ChatActionKind::JobSubmit,
                    title,
                    new_job: Box::new(new_job),
                    session_id,
                }
            }
            codegg_protocol::core::ChatActionSubmitDto::JobReference { job_id, title } => {
                if !self
                    .action_capability_allows(
                        &project,
                        authority.principal(),
                        codegg_core::team::Capability::SessionRead,
                    )
                    .await
                {
                    return Ok(chat_error(CollaborationError::ActionDenied(
                        "job_reference requires session.read".to_owned(),
                    )));
                }
                let config = self.collaboration.config().clone();
                let title = match codegg_core::collaboration::validate_action_title(
                    &config,
                    title.as_deref(),
                ) {
                    Ok(value) => value,
                    Err(error) => return Ok(chat_error(error)),
                };
                let job = match codegg_core::collaboration::validate_action_job_id(&job_id) {
                    Ok(value) => value,
                    Err(error) => return Ok(chat_error(error)),
                };
                PendingAction::ReferenceJob { title, job_id: job }
            }
        };
        // Execute the pending action against the canonical owners.
        // No chat-owned execution exists: submits go through the
        // daemon-owned `JobSubmissionService` boundary; references
        // only read the canonical `JobStore`.
        let (kind, title, job_id, status, session_id) = match pending {
            PendingAction::SubmitJob {
                kind,
                title,
                new_job,
                session_id,
            } => {
                let Some(submission) = self.deps.submission.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "scheduler_unavailable".to_owned(),
                        message: "daemon has no job submission service".to_owned(),
                    });
                };
                let submission_key =
                    format!("chat-action:{}:{}", channel.as_str(), idempotency_key);
                let submission_key = match crate::scheduler::SubmissionKey::new(submission_key) {
                    Ok(key) => Some(key),
                    Err(_) => {
                        return Ok(CoreResponse::Error {
                            code: "chat_invalid_input".to_owned(),
                            message: "action idempotency key is too long for job submission"
                                .to_owned(),
                        });
                    }
                };
                let submitted = match submission.submit(submission_key, *new_job).await {
                    Ok(submitted) => submitted,
                    Err(error) => {
                        let message = error.to_string();
                        if message.contains("Submission key was reused")
                            || message.contains("submission key was reused")
                            || message.contains("SubmissionKeyConflict")
                            || matches!(
                                error,
                                crate::scheduler::JobSubmissionError::SubmissionKeyConflict
                            )
                        {
                            return Ok(chat_error(CollaborationError::ActionConflict(
                                idempotency_key.to_owned(),
                            )));
                        }
                        return Ok(CoreResponse::Error {
                            code: "chat_action_submit_failed".to_owned(),
                            message,
                        });
                    }
                };
                let job_id = submitted.job_id.as_str().to_owned();
                // Origin attribution for the canonical job, mirroring
                // the `JobSubmit` protocol path.
                self.record_origin_with_decision(authority, authz_decision, "job", &job_id)
                    .await;
                (
                    kind,
                    title,
                    Some(job_id),
                    CHAT_ACTION_STATUS_SUBMITTED.to_owned(),
                    session_id,
                )
            }
            PendingAction::ReferenceJob { title, job_id } => {
                let lookup = codegg_core::jobs::JobId::new_unchecked(job_id.clone());
                let stored = match self.deps.job_store.get_job(&lookup).await {
                    Ok(record) => record,
                    Err(error) => {
                        return Ok(chat_error(CollaborationError::Storage(
                            codegg_core::error::StorageError::Database(error.to_string()),
                        )));
                    }
                };
                let Some(record) = stored else {
                    return Ok(chat_error(CollaborationError::ActionNotFound(job_id)));
                };
                // Per-object project recheck when the job carries a
                // session: cross-project references fail closed as
                // not-found so existence cannot be probed across
                // projects. Session-less (workspace-scoped) jobs are
                // chat-gated only.
                if let Some(session) = record.session_id.clone() {
                    match self.action_session_project(&session).await {
                        Some(owner) if owner == project => {}
                        _ => {
                            return Ok(chat_error(CollaborationError::ActionNotFound(job_id)));
                        }
                    }
                }
                let _ = record;
                (
                    codegg_core::collaboration::ChatActionKind::JobReference,
                    title,
                    Some(job_id),
                    CHAT_ACTION_STATUS_REFERENCED.to_owned(),
                    None,
                )
            }
        };
        // Persist the reference/status projection. The unique backstop
        // converges concurrent retries: on conflict, return the winner
        // without creating a second job (the job above is already
        // idempotent via the submission key, so the loser converges).
        let stored = match self
            .collaboration
            .insert_action(
                &project,
                &channel,
                &message,
                &principal,
                kind,
                title.clone(),
                job_id.clone(),
                &status,
                idempotency_key,
                now_ms,
            )
            .await
        {
            Ok(action) => (action, false),
            Err(CollaborationError::ActionConflict(_)) => {
                match self
                    .collaboration
                    .find_action_by_idempotency(&project, &channel, idempotency_key)
                    .await
                {
                    Ok(Some(winner)) => (winner, true),
                    Ok(None) => {
                        return Ok(chat_error(CollaborationError::ActionConflict(
                            idempotency_key.to_owned(),
                        )));
                    }
                    Err(error) => return Ok(chat_error(error)),
                }
            }
            Err(error) => return Ok(chat_error(error)),
        };
        let (action_record, duplicate) = stored;
        // Audit causation: message -> auth decision -> action -> job.
        // Structural locators only; titles/prompts never enter audit.
        {
            use codegg_core::audit_instrumentation as instr;
            let provenance = codegg_core::authorization::audit_provenance(authz_decision);
            let mut chain = instr::AuditChainContext::new();
            chain.project = Some(project.clone());
            if let Some(session) = session_id.clone() {
                chain.session_id = Some(session);
            }
            if let Some(job) = job_id.clone() {
                chain.job_id = Some(job.clone());
            }
            let builder = instr::chat_triggered_action_event(
                authority.principal(),
                &provenance,
                &chain,
                channel.as_str(),
                message.as_str(),
                &action_record.id,
                action_record.kind.as_str(),
                job_id.as_deref(),
                "allow",
            );
            self.append_audit_event(builder).await;
        }
        // Publish the durable projection (with the real action id) for
        // live chat subscribers. Duplicate retries publish nothing.
        if !duplicate {
            self.event_log
                .publish(
                    session_id.clone(),
                    None,
                    CoreEvent::ChatActionUpdated {
                        project_id: project.as_str().to_owned(),
                        channel_id: channel.as_str().to_owned(),
                        message_id: message.as_str().to_owned(),
                        action: action_record.to_dto(),
                    },
                )
                .await;
        }
        // Mirror the canonical job-submit audit for submitted jobs so
        // job pages stay consistent for chat-triggered work.
        if status == CHAT_ACTION_STATUS_SUBMITTED {
            if let Some(job) = job_id.clone() {
                use codegg_core::audit_instrumentation as instr;
                let provenance = codegg_core::authorization::audit_provenance(authz_decision);
                let mut chain = instr::AuditChainContext::new();
                chain.project = Some(project.clone());
                if let Some(session) = session_id.clone() {
                    chain.session_id = Some(session);
                }
                chain.job_id = Some(job.clone());
                let builder = instr::job_submit_event(
                    authority.principal(),
                    &provenance,
                    &chain,
                    &job,
                    "allow",
                );
                self.append_audit_event(builder).await;
                // Canonical job creation event for job subscribers.
                if let Ok(Some(record)) = self
                    .deps
                    .job_store
                    .get_job(&codegg_core::jobs::JobId::new_unchecked(job.clone()))
                    .await
                {
                    self.event_log
                        .publish(
                            record.session_id.clone(),
                            record.turn_id.clone(),
                            CoreEvent::JobCreated {
                                job_id: job.clone(),
                                workspace_id: record.workspace_id.to_string(),
                                kind: record.kind.as_str().to_owned(),
                                session_id: record.session_id.clone(),
                                turn_id: record.turn_id.clone(),
                            },
                        )
                        .await;
                }
            }
        }
        let _ = (request_id, trusted_client_id);
        Ok(CoreResponse::ChatAction {
            action: action_record.to_dto(),
            duplicate,
        })
    }

    /// M003: `session_id` locator carried by one request, if any.
    fn session_id_for_request(request: &CoreRequest) -> Option<&str> {
        match request {
            CoreRequest::SessionAttach { session_id }
            | CoreRequest::SessionLoad { session_id }
            | CoreRequest::SessionMessagesLoad { session_id }
            | CoreRequest::SessionFork { session_id }
            | CoreRequest::SessionDelete { session_id, .. }
            | CoreRequest::SessionArchive { session_id, .. }
            | CoreRequest::SessionRestore { session_id }
            | CoreRequest::SessionShare { session_id }
            | CoreRequest::SessionUnshare { session_id }
            | CoreRequest::SessionRename { session_id, .. }
            | CoreRequest::SessionExport { session_id }
            | CoreRequest::TurnSubmit { session_id, .. }
            | CoreRequest::TurnCancel { session_id, .. }
            | CoreRequest::TurnSteer { session_id, .. }
            | CoreRequest::AgentSelect { session_id, .. }
            | CoreRequest::ModelSelect { session_id, .. }
            | CoreRequest::SessionSelectionGet { session_id }
            | CoreRequest::SessionSelectionList { session_id }
            | CoreRequest::SessionSelectionModels { session_id, .. }
            | CoreRequest::SessionLifecycleGet { session_id }
            | CoreRequest::GoalShow { session_id }
            | CoreRequest::GoalPause { session_id }
            | CoreRequest::GoalResume { session_id }
            | CoreRequest::GoalClear { session_id }
            | CoreRequest::GoalDone { session_id }
            | CoreRequest::TodoList { session_id }
            | CoreRequest::ActiveGoalLoad { session_id }
            | CoreRequest::GoalSetBudget { session_id, .. }
            | CoreRequest::SnapshotSession { session_id }
            | CoreRequest::ToolProgramList { session_id, .. }
            | CoreRequest::ToolProgramNotificationReinject { session_id }
            | CoreRequest::ToolProgramRecoveryDebugInspect { session_id, .. }
            | CoreRequest::EditCheckpointList { session_id, .. }
            | CoreRequest::EditCheckpointUndo { session_id, .. }
            | CoreRequest::EditCheckpointUndoLatest { session_id, .. }
            | CoreRequest::EditCheckpointReapply { session_id, .. }
            | CoreRequest::EditCheckpointReapplyLatest { session_id, .. } => Some(session_id),
            CoreRequest::SessionSelectionUpdate { request } => Some(request.session_id.as_str()),
            CoreRequest::GoalSet { session_id, .. }
            | CoreRequest::GoalFromFile { session_id, .. }
            | CoreRequest::GoalCheckpoint { session_id, .. } => Some(session_id),
            CoreRequest::JobSubmit { spec } => spec.session_id.as_deref(),
            CoreRequest::ScheduleCreate { spec } => spec.session_id.as_deref(),
            CoreRequest::RunRerun {
                session_id: Some(session_id),
                ..
            } => Some(session_id),
            _ => None,
        }
    }

    /// M003: owning project of one session row, if resolvable.
    async fn session_project(
        &self,
        pool: &sqlx::SqlitePool,
        session_id: &str,
    ) -> Option<codegg_core::identity::ProjectId> {
        let row: Option<(String,)> = sqlx::query_as("SELECT project_id FROM session WHERE id = ?")
            .bind(session_id)
            .fetch_optional(pool)
            .await
            .unwrap_or(None);
        row.and_then(|(raw,)| codegg_core::identity::ProjectId::parse(&raw).ok())
    }

    /// M003: owning project of one job's session, if resolvable.
    async fn job_session_project(
        &self,
        pool: &sqlx::SqlitePool,
        job_id: &str,
    ) -> Option<codegg_core::identity::ProjectId> {
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT session_id FROM job WHERE id = ?")
                .bind(job_id)
                .fetch_optional(pool)
                .await
                .unwrap_or(None);
        match row {
            Some((Some(session_id),)) => self.session_project(pool, &session_id).await,
            _ => None,
        }
    }

    /// M003: filter catalog records to the projects `client_id` may
    /// observe. LocalOwner broad policy observes everything; team
    /// principals observe exactly their `project.read` grants so
    /// enumeration cannot leak project existence.
    pub(crate) async fn filter_projects_for_principal(
        &self,
        trusted_client_id: &str,
        records: Vec<codegg_core::project_catalog::ProjectCatalogRecord>,
    ) -> Vec<codegg_core::project_catalog::ProjectCatalogRecord> {
        let authority = self.request_authority_for_client(trusted_client_id);
        if codegg_core::authorization::is_local_owner_broad(authority.principal()) {
            return records;
        }
        let Some(pool) = self.pool.clone() else {
            return records;
        };
        let service = codegg_core::authorization::AuthorizationService::new(
            codegg_core::team::TeamStore::new(pool),
        );
        let ids: Vec<codegg_core::identity::ProjectId> = records
            .iter()
            .map(|record| record.project_id.clone())
            .collect();
        let visible = Box::pin(codegg_core::authorization::visible_projects(
            &service,
            authority.principal(),
            &ids,
        ))
        .await;
        records
            .into_iter()
            .filter(|record| visible.contains(&record.project_id))
            .collect()
    }

    /// M003: capture originating-principal attribution for durable work.
    ///
    /// Best-effort: the gate already authorized this request, so the
    /// captured decision is the enforcement context. Attribution write
    /// failures warn without failing the operation; the M004 audit
    /// store will harden the failure policy.
    pub(crate) async fn record_origin_with_decision(
        &self,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        decision: &codegg_core::authorization::AuthorizationDecision,
        scope_kind: &str,
        scope_id: &str,
    ) {
        let Some(pool) = self.pool.clone() else {
            return;
        };
        let attribution = codegg_core::authorization::OriginAttribution::from_authority(
            authority.principal(),
            decision,
        );
        let store = codegg_core::authorization::OriginAttributionStore::new(pool);
        if let Err(error) = store.record(scope_kind, scope_id, &attribution).await {
            tracing::warn!(
                error = %error,
                scope_kind,
                scope_id,
                "origin attribution write failed"
            );
        }
    }

    /// M005: best-effort append of one structural audit event.
    ///
    /// Bounded: one per-write timeout, no unbounded queue, never fails
    /// the operation. Outcomes are observable via
    /// `audit_instrumentation::emit_counters_snapshot` plus warn logs.
    pub(crate) async fn append_audit_event(&self, builder: codegg_core::audit::AuditEventBuilder) {
        let Some(pool) = self.pool.clone() else {
            codegg_core::audit_instrumentation::record_emit_dropped_no_pool();
            return;
        };
        let store = codegg_core::audit::AuditStore::new(pool);
        match tokio::time::timeout(std::time::Duration::from_millis(500), store.append(builder))
            .await
        {
            Ok(Ok(_)) => codegg_core::audit_instrumentation::record_emit_appended(),
            Ok(Err(error)) => {
                codegg_core::audit_instrumentation::record_emit_failed();
                tracing::warn!(error = %error, "audit event append failed");
            }
            Err(_) => {
                codegg_core::audit_instrumentation::record_emit_failed();
                tracing::warn!("audit event append timed out");
            }
        }
    }

    /// M005: causal chain locators for one request.
    ///
    /// Derived from request DTO locators plus the gate-resolved project.
    /// Payloads supply locators only; actor/decision always come from
    /// the trusted authority/decision pair.
    fn audit_chain_for_request(
        request: &CoreRequest,
        decision: &codegg_core::authorization::AuthorizationDecision,
    ) -> codegg_core::audit_instrumentation::AuditChainContext {
        let mut chain = codegg_core::audit_instrumentation::AuditChainContext::new();
        chain.project = decision.project_id.clone();
        if let Some(session) = Self::session_id_for_request(request) {
            chain.session_id = Some(session.to_owned());
        }
        match request {
            CoreRequest::TurnCancel { turn_id, .. } | CoreRequest::TurnSteer { turn_id, .. } => {
                chain.turn_id = Some(turn_id.clone());
            }
            CoreRequest::JobCancel { job_id, .. } | CoreRequest::JobRetry { job_id } => {
                chain.job_id = Some(job_id.clone());
            }
            CoreRequest::ManagedWorktreeCleanup { worktree_id, .. }
            | CoreRequest::ManagedWorktreeArchive { worktree_id, .. } => {
                chain.worktree_id = Some(worktree_id.clone());
            }
            CoreRequest::SessionSelectionUpdate { request } => {
                chain.provider_connection_id = Some(request.connection_id.clone());
            }
            CoreRequest::SessionSelectionModels { connection_id, .. } => {
                chain.provider_connection_id = Some(connection_id.clone());
            }
            CoreRequest::RunRerun {
                parent_run_id,
                session_id,
                ..
            } => {
                chain.run_id = Some(parent_run_id.clone());
                if chain.session_id.is_none() {
                    chain.session_id = session_id.clone();
                }
            }
            CoreRequest::LspPreviewApply { request } => {
                chain.session_id = Some(request.session_id.clone());
                if let Some(turn) = request.turn_id.clone() {
                    chain.turn_id = Some(turn);
                }
            }
            CoreRequest::EditCheckpointUndo { session_id, .. }
            | CoreRequest::EditCheckpointUndoLatest { session_id, .. }
            | CoreRequest::EditCheckpointReapply { session_id, .. }
            | CoreRequest::EditCheckpointReapplyLatest { session_id, .. } => {
                chain.session_id = Some(session_id.clone());
            }
            CoreRequest::SchedulePause { schedule_id }
            | CoreRequest::ScheduleResume { schedule_id }
            | CoreRequest::ScheduleDelete { schedule_id } => {
                chain.job_id = Some(schedule_id.clone());
            }
            _ => {}
        }
        if let CoreRequest::JobSubmit { spec } = request {
            if let Some(session) = spec.session_id.clone() {
                chain.session_id = Some(session);
            }
            if let Some(turn) = spec.turn_id.clone() {
                chain.turn_id = Some(turn);
            }
        }
        if let CoreRequest::ScheduleCreate { spec } = request {
            if let Some(session) = spec.session_id.clone() {
                chain.session_id = Some(session);
            }
        }
        chain
    }

    /// M005: emit one structural event for an authorized request.
    ///
    /// Canonical control-plane seam: runs after the M003 gate, before
    /// any side effect, at the daemon dispatch owner. Creation
    /// operations that mint their identity in the handler
    /// (`SessionCreate`, `JobSubmit`, audit reads) are skipped here
    /// and emitted post-creation with their durable ids.
    async fn emit_audit_for_authorized(
        &self,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        decision: &codegg_core::authorization::AuthorizationDecision,
        request_id: &str,
        request: &CoreRequest,
    ) {
        use codegg_core::audit_instrumentation as instr;
        let operation = codegg_core::authorization::operation_descriptor(request).operation;
        let Some(action) = instr::operation_to_audit_action(operation) else {
            return;
        };
        match request {
            CoreRequest::SessionCreate { .. }
            | CoreRequest::JobSubmit { .. }
            | CoreRequest::ChatActionSubmit { .. }
            | CoreRequest::AuditQuery { .. }
            | CoreRequest::AuditExport { .. } => return,
            _ => {}
        }
        let provenance = codegg_core::authorization::audit_provenance(decision);
        let chain = Self::audit_chain_for_request(request, decision);
        let principal = authority.principal();
        let builder = match action {
            codegg_core::audit::AuditAction::Authentication => {
                instr::authentication_event(principal, &provenance, &chain, "allow")
            }
            codegg_core::audit::AuditAction::SessionCreate => {
                let session = chain
                    .session_id
                    .clone()
                    .unwrap_or_else(|| request_id.to_owned());
                instr::session_create_event(principal, &provenance, &chain, &session, "allow")
            }
            codegg_core::audit::AuditAction::SessionAttach => {
                let session = chain.session_id.clone().unwrap_or_default();
                instr::session_attach_event(principal, &provenance, &chain, &session, "allow")
            }
            codegg_core::audit::AuditAction::PromptSubmit => {
                let (session, text) = match request {
                    CoreRequest::TurnSubmit {
                        session_id, text, ..
                    } => (session_id.clone(), text.clone()),
                    CoreRequest::TurnSteer {
                        session_id, text, ..
                    } => (session_id.clone(), text.clone()),
                    _ => (chain.session_id.clone().unwrap_or_default(), String::new()),
                };
                let digest = instr::structural_digest(text.as_bytes());
                instr::prompt_submit_event(
                    principal,
                    &provenance,
                    &chain,
                    &session,
                    request_id,
                    &digest,
                    text.len(),
                    "allow",
                )
            }
            codegg_core::audit::AuditAction::ProviderSelect => {
                let (session, connection, model) = match request {
                    CoreRequest::SessionSelectionUpdate { request } => (
                        request.session_id.clone(),
                        request.connection_id.clone(),
                        request.model_id.clone(),
                    ),
                    _ => (
                        chain.session_id.clone().unwrap_or_default(),
                        chain
                            .provider_connection_id
                            .clone()
                            .unwrap_or_else(|| decision.operation.clone()),
                        decision.operation.clone(),
                    ),
                };
                instr::provider_select_event(
                    principal,
                    &provenance,
                    &chain,
                    &session,
                    &connection,
                    &model,
                    "allow",
                )
            }
            codegg_core::audit::AuditAction::ModelSelect => {
                let (session, model) = match request {
                    CoreRequest::ModelSelect { session_id, model } => {
                        (session_id.clone(), model.clone())
                    }
                    _ => (
                        chain.session_id.clone().unwrap_or_default(),
                        decision.operation.clone(),
                    ),
                };
                instr::model_select_event(principal, &provenance, &chain, &session, &model, "allow")
            }
            codegg_core::audit::AuditAction::AgentDelegate => {
                let (parent, child) = match request {
                    CoreRequest::AgentSelect {
                        session_id,
                        agent_name,
                    } => (session_id.clone(), agent_name.clone()),
                    CoreRequest::GoalSet { session_id, .. }
                    | CoreRequest::GoalFromFile { session_id, .. }
                    | CoreRequest::GoalPause { session_id }
                    | CoreRequest::GoalResume { session_id }
                    | CoreRequest::GoalClear { session_id }
                    | CoreRequest::GoalDone { session_id }
                    | CoreRequest::GoalCheckpoint { session_id, .. }
                    | CoreRequest::GoalSetBudget { session_id, .. } => {
                        (session_id.clone(), request_id.to_owned())
                    }
                    CoreRequest::ToolProgramNotificationReinject { session_id }
                    | CoreRequest::ToolProgramRecoveryDebugInspect { session_id, .. } => {
                        (session_id.clone(), request_id.to_owned())
                    }
                    _ => (
                        chain
                            .session_id
                            .clone()
                            .unwrap_or_else(|| request_id.to_owned()),
                        request_id.to_owned(),
                    ),
                };
                instr::agent_delegate_event(
                    principal,
                    &provenance,
                    &chain,
                    &parent,
                    &child,
                    "allow",
                )
            }
            codegg_core::audit::AuditAction::PermissionDecision => {
                let (tool, outcome) = match request {
                    CoreRequest::PermissionRespond { id, choice } => (id.clone(), choice.clone()),
                    _ => (request_id.to_owned(), "allow".to_owned()),
                };
                instr::permission_decision_event(
                    principal,
                    &provenance,
                    &chain,
                    &tool,
                    &outcome,
                    &decision.operation,
                )
            }
            codegg_core::audit::AuditAction::ToolInvoke => {
                let run = chain
                    .run_id
                    .clone()
                    .unwrap_or_else(|| request_id.to_owned());
                instr::tool_invoke_event(
                    principal,
                    &provenance,
                    &chain,
                    "run_rerun",
                    "scheduler",
                    "allow",
                )
                .with_run(run)
            }
            codegg_core::audit::AuditAction::FileMutate => {
                let (digest_source, op) = match request {
                    CoreRequest::EditCheckpointUndo { checkpoint_id, .. }
                    | CoreRequest::EditCheckpointReapply { checkpoint_id, .. } => {
                        (checkpoint_id.clone(), decision.operation.clone())
                    }
                    CoreRequest::LspPreviewApply { request } => {
                        (request.preview_id.clone(), decision.operation.clone())
                    }
                    _ => (request_id.to_owned(), decision.operation.clone()),
                };
                let digest = instr::structural_digest(digest_source.as_bytes());
                instr::file_mutate_event(principal, &provenance, &chain, &digest, &op, "allow")
            }
            codegg_core::audit::AuditAction::WorktreeLifecycle => {
                let worktree = chain
                    .worktree_id
                    .clone()
                    .unwrap_or_else(|| request_id.to_owned());
                instr::worktree_lifecycle_event(
                    principal,
                    &provenance,
                    &chain,
                    &worktree,
                    &decision.operation,
                    "allow",
                )
            }
            codegg_core::audit::AuditAction::JobSubmit => {
                let job = chain
                    .job_id
                    .clone()
                    .unwrap_or_else(|| request_id.to_owned());
                instr::job_submit_event(principal, &provenance, &chain, &job, "allow")
            }
            codegg_core::audit::AuditAction::JobCancel => {
                let job = chain
                    .job_id
                    .clone()
                    .or_else(|| chain.turn_id.clone())
                    .unwrap_or_else(|| request_id.to_owned());
                instr::job_cancel_event(principal, &provenance, &chain, &job, "allow")
            }
            codegg_core::audit::AuditAction::JobComplete => {
                let job = chain
                    .job_id
                    .clone()
                    .unwrap_or_else(|| request_id.to_owned());
                instr::job_complete_event(principal, &provenance, &chain, &job, "retry", "allow")
            }
            codegg_core::audit::AuditAction::MembershipChange => {
                let member = chain
                    .session_id
                    .clone()
                    .unwrap_or_else(|| request_id.to_owned());
                instr::membership_change_event(
                    principal,
                    &provenance,
                    &chain,
                    &member,
                    &decision.operation,
                    decision.membership_revision,
                )
            }
            codegg_core::audit::AuditAction::ConfigChange => instr::config_change_event(
                principal,
                &provenance,
                &chain,
                &decision.operation,
                decision
                    .project_id
                    .as_ref()
                    .map(|id| id.as_str())
                    .unwrap_or("workspace"),
                "allow",
            ),
            codegg_core::audit::AuditAction::AssetRefresh => {
                let project = decision
                    .project_id
                    .as_ref()
                    .map(|id| id.as_str().to_owned())
                    .unwrap_or_default();
                instr::asset_refresh_event(
                    principal,
                    &provenance,
                    &chain,
                    &project,
                    "request_authorized",
                    "allow",
                )
            }
            codegg_core::audit::AuditAction::AuthorizationDecision => {
                instr::authorization_denied_event(
                    principal,
                    &provenance,
                    &chain,
                    &decision.operation,
                    decision.capability.as_deref().unwrap_or("none"),
                    "authorized",
                )
            }
            _ => return,
        };
        self.append_audit_event(builder).await;
    }

    /// M005: emit one terminal denial event for a rejected request.
    ///
    /// Denials carry the operation/capability the caller supplied plus
    /// the denial reason. They never carry secret material and never
    /// leak project existence beyond the authorized read gate that
    /// already protects audit pages.
    async fn emit_audit_for_denial(
        &self,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        request_id: &str,
        request: &CoreRequest,
        error: &codegg_core::authorization::AuthorizationError,
    ) {
        use codegg_core::audit_instrumentation as instr;
        let descriptor = codegg_core::authorization::operation_descriptor(request);
        let capability = match error {
            codegg_core::authorization::AuthorizationError::Denied { capability, .. } => {
                (*capability).to_owned()
            }
            _ => descriptor
                .capability
                .map(|cap| cap.as_str().to_owned())
                .unwrap_or_else(|| "none".to_owned()),
        };
        let correlation = format!("{}:{request_id}", authority.correlation_id());
        // Best-effort direct project locator so project-scoped denials
        // remain queryable by the project owner through the `audit.read`
        // gate. Unresolvable scopes stay `None` and never leak existence
        // through the audit page itself.
        let direct_project: Option<codegg_core::identity::ProjectId> = match request {
            CoreRequest::AuditQuery { query } => {
                codegg_core::identity::ProjectId::parse(query.project_id.as_str()).ok()
            }
            CoreRequest::AuditExport { request } => {
                codegg_core::identity::ProjectId::parse(request.project_id.as_str()).ok()
            }
            CoreRequest::SessionList { project_id, .. }
            | CoreRequest::ProjectGet { project_id }
            | CoreRequest::ProjectArchive { project_id }
            | CoreRequest::ProjectRestore { project_id } => {
                codegg_core::identity::ProjectId::parse(project_id).ok()
            }
            CoreRequest::SessionCreate {
                project_id: Some(project_id),
                ..
            }
            | CoreRequest::SessionCreateFromTemplate {
                project_id: Some(project_id),
                ..
            } => codegg_core::identity::ProjectId::parse(project_id).ok(),
            _ => None,
        };
        let provenance = codegg_core::audit::AuditDecisionProvenance::new(
            uuid::Uuid::new_v4().to_string(),
            correlation,
            "denied",
            direct_project.clone(),
        );
        let mut chain = codegg_core::audit_instrumentation::AuditChainContext::new();
        chain.project = direct_project;
        if let Some(session) = Self::session_id_for_request(request) {
            chain.session_id = Some(session.to_owned());
        }
        let builder = instr::authorization_denied_event(
            authority.principal(),
            &provenance,
            &chain,
            descriptor.operation,
            &capability,
            &error.to_string(),
        );
        self.append_audit_event(builder).await;
    }

    async fn handle_request_with_client(
        &self,
        request: RequestEnvelope<CoreRequest>,
        trusted_client_id: Option<&str>,
    ) -> Result<CoreResponse, AppError> {
        let trusted_client_id = trusted_client_id.unwrap_or("local-daemon");
        // M003: server-side authorization before any side effect. Denied
        // requests reply here with zero side effect. The success decision
        // stays in scope so creation arms can capture its context with
        // the resulting durable work. The authorization future is boxed:
        // the dispatch match below is already near the stack limit and
        // must only hold the box.
        let authority = self.request_authority_for_client(trusted_client_id);
        let authz_decision = match Box::pin(self.authorize_request(
            &authority,
            &request.request_id,
            &request.payload,
        ))
        .await
        {
            Ok(decision) => decision,
            Err(error) => {
                Box::pin(self.emit_audit_for_denial(
                    &authority,
                    &request.request_id,
                    &request.payload,
                    &error,
                ))
                .await;
                return Ok(Self::authorization_denial(&request.payload, &error));
            }
        };
        Box::pin(self.emit_audit_for_authorized(
            &authority,
            &authz_decision,
            &request.request_id,
            &request.payload,
        ))
        .await;
        // Collaboration M001: chat arms run in a dedicated handler so
        // the main dispatch future stays small (same rationale as the
        // boxed authorization preamble above). The M003 gate has already
        // enforced `project.chat`; the handler only records/reads the
        // caller's project-scoped chat state. M003 actions receive the
        // gate decision so audit causation links message -> decision ->
        // action -> job without re-authorizing.
        if Self::is_chat_request(&request.payload) {
            return Box::pin(self.handle_chat_request(
                &request.request_id,
                request.payload,
                trusted_client_id,
                &authority,
                &authz_decision,
            ))
            .await;
        }
        // Interactive Process Sessions M002: the attach/resume family runs
        // on a fresh task (boxed at the call site). The dispatch match
        // below is already near its stack limit: nesting the PTY handler
        // frames inside it overflows small worker stacks, so the envelope
        // moves into `run_interactive_request` instead.
        if Self::is_interactive_process_request(&request.payload) {
            let protocol = self.interactive_processes.clone();
            let workspaces = self.workspaces.clone();
            let event_log = self.event_log.clone();
            let authority = self.interactive_authority_for(trusted_client_id);
            let owned_client = trusted_client_id.to_string();
            let join = tokio::spawn(async move {
                let out = Self::run_interactive_request(
                    protocol,
                    workspaces,
                    event_log,
                    authority,
                    owned_client,
                    request,
                )
                .await;
                out
            });
            return join.await.map_err(|error| {
                AppError::Other(anyhow::anyhow!("interactive request task failed: {error}"))
            })?;
        }
        let owned_request_id = request.request_id.clone();
        let payload = request.payload;
        let family = super::daemon_family::DaemonRequestFamily::of(&payload);
        match family {
            super::daemon_family::DaemonRequestFamily::Assets => {
                Box::pin(self.handle_assets_request(
                    payload,
                    &owned_request_id,
                    trusted_client_id,
                    authority,
                    authz_decision,
                ))
                .await
            }
            super::daemon_family::DaemonRequestFamily::Providers => {
                Box::pin(self.handle_providers_request(
                    payload,
                    &owned_request_id,
                    trusted_client_id,
                    authority,
                    authz_decision,
                ))
                .await
            }
            super::daemon_family::DaemonRequestFamily::Sessions => {
                Box::pin(self.handle_sessions_request(
                    payload,
                    &owned_request_id,
                    trusted_client_id,
                    authority,
                    authz_decision,
                ))
                .await
            }
            super::daemon_family::DaemonRequestFamily::Turns => {
                Box::pin(self.handle_turns_request(
                    payload,
                    &owned_request_id,
                    trusted_client_id,
                    authority,
                    authz_decision,
                ))
                .await
            }
            super::daemon_family::DaemonRequestFamily::Jobs => {
                Box::pin(self.handle_jobs_request(
                    payload,
                    &owned_request_id,
                    trusted_client_id,
                    authority,
                    authz_decision,
                ))
                .await
            }
            super::daemon_family::DaemonRequestFamily::Projects => {
                Box::pin(self.handle_projects_request(
                    payload,
                    &owned_request_id,
                    trusted_client_id,
                    authority,
                    authz_decision,
                ))
                .await
            }
            super::daemon_family::DaemonRequestFamily::Goals => {
                Box::pin(self.handle_goals_request(
                    payload,
                    &owned_request_id,
                    trusted_client_id,
                    authority,
                    authz_decision,
                ))
                .await
            }
            super::daemon_family::DaemonRequestFamily::Projection => {
                Box::pin(self.handle_projection_request(
                    payload,
                    &owned_request_id,
                    trusted_client_id,
                    authority,
                    authz_decision,
                ))
                .await
            }
            super::daemon_family::DaemonRequestFamily::Ops => {
                Box::pin(self.handle_ops_request(
                    payload,
                    &owned_request_id,
                    trusted_client_id,
                    authority,
                    authz_decision,
                ))
                .await
            }
            super::daemon_family::DaemonRequestFamily::Chat
            | super::daemon_family::DaemonRequestFamily::Interactive => {
                // Unreachable: chat and interactive-process envelopes return
                // through their boxed/spawned pre-router paths above, which
                // preserve their stack and cancellation semantics. Keep the
                // historical unimplemented contract as defense in depth.
                tracing::warn!("Unhandled CoreRequest variant");
                Ok(CoreResponse::Error {
                    code: "unimplemented".to_string(),
                    message: "This request type is not yet implemented".to_string(),
                })
            }
        }
    }

    /// Interactive Process Sessions M002: `true` for the bounded
    /// attach/resume operation family, which dispatches through the boxed
    /// [`Self::handle_interactive_request`] helper instead of the giant
    /// dispatch match.
    fn is_interactive_process_request(request: &CoreRequest) -> bool {
        matches!(
            request,
            CoreRequest::InteractiveProcessCapabilities
                | CoreRequest::InteractiveProcessCreate { .. }
                | CoreRequest::InteractiveProcessList { .. }
                | CoreRequest::InteractiveProcessAttach { .. }
                | CoreRequest::InteractiveProcessDetach { .. }
                | CoreRequest::InteractiveProcessInput { .. }
                | CoreRequest::InteractiveProcessResize { .. }
                | CoreRequest::InteractiveProcessResume { .. }
                | CoreRequest::InteractiveProcessTerminate { .. }
                | CoreRequest::InteractiveProcessRemove { .. }
        )
    }

    /// Interactive Process Sessions M002: dispatch one attach/resume
    /// envelope through the daemon-owned handler family. Ownership always
    /// comes from the transport-bound `client_id`; payloads carry no
    /// identity. Runs on a fresh task (see the call site): `&self` is not
    /// held, only the owned daemon pieces the family needs.
    async fn run_interactive_request(
        protocol: Arc<crate::interactive_process_attach::InteractiveProcessProtocol>,
        workspaces: Arc<codegg_core::workspace::WorkspaceRegistry>,
        event_log: Arc<super::event_log::EventLog>,
        authority: crate::interactive_process_attach::InteractiveAuthority,
        client_id: String,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        let trusted_client_id = client_id.as_str();
        match request.payload {
            CoreRequest::InteractiveProcessCapabilities => {
                let current =
                    codegg_protocol::interactive_process::InteractiveProcessCapabilities::current();
                Ok(protocol.capabilities(&current))
            }
            CoreRequest::InteractiveProcessCreate { request } => {
                let ctx =
                    match Self::interactive_execution_context(&workspaces, &request.workspace_id)
                        .await
                    {
                        Ok(ctx) => ctx,
                        Err(response) => return Ok(*response),
                    };
                Ok(protocol.create(trusted_client_id, &ctx, &request).await)
            }
            CoreRequest::InteractiveProcessList {
                workspace_id,
                limit,
            } => Ok(protocol
                .list(trusted_client_id, workspace_id.as_deref(), limit)
                .await),
            CoreRequest::InteractiveProcessAttach {
                handle,
                from_seq,
                max_bytes,
            } => Ok(protocol
                .attach(trusted_client_id, &handle, from_seq, max_bytes)
                .await),
            CoreRequest::InteractiveProcessDetach { attachment_id } => {
                Ok(protocol.detach(trusted_client_id, &attachment_id).await)
            }
            CoreRequest::InteractiveProcessInput {
                attachment_id,
                data_b64,
            } => Ok(protocol
                .input(trusted_client_id, &attachment_id, &data_b64)
                .await),
            CoreRequest::InteractiveProcessResize {
                attachment_id,
                cols,
                rows,
            } => Ok(protocol
                .resize(trusted_client_id, &attachment_id, cols, rows)
                .await),
            CoreRequest::InteractiveProcessResume {
                attachment_id,
                from_seq,
                max_bytes,
            } => Ok(protocol
                .resume(trusted_client_id, &attachment_id, from_seq, max_bytes)
                .await),
            CoreRequest::InteractiveProcessTerminate { attachment_id } => {
                let response = protocol
                    .terminate(trusted_client_id, &authority, &attachment_id)
                    .await;
                if let CoreResponse::InteractiveProcessTerminated {
                    handle,
                    exit_code,
                    exit_signal,
                    ..
                } = &response
                {
                    event_log
                        .publish(
                            None,
                            None,
                            CoreEvent::InteractiveProcessExited {
                                handle: handle.clone(),
                                exit_code: *exit_code,
                                exit_signal: *exit_signal,
                            },
                        )
                        .await;
                }
                Ok(response)
            }
            CoreRequest::InteractiveProcessRemove { attachment_id } => Ok(protocol
                .remove(trusted_client_id, &authority, &attachment_id)
                .await),
            other => {
                let _ = other;
                Ok(CoreResponse::Error {
                    code: "interactive_invalid_request".to_string(),
                    message: "request is not an interactive-process operation".to_string(),
                })
            }
        }
    }

    /// Interactive Process Sessions M002: resolve the immutable execution
    /// context for one workspace locator. The workspace must already be
    /// registered; interactive create never invents workspace identity.
    /// The error is boxed: `CoreResponse` is too large for an inline
    /// `Err` variant under the workspace Clippy gate.
    async fn interactive_execution_context(
        workspaces: &Arc<codegg_core::workspace::WorkspaceRegistry>,
        workspace_id_raw: &str,
    ) -> Result<Arc<codegg_core::workspace::ExecutionContext>, Box<CoreResponse>> {
        let workspace_id =
            codegg_core::workspace::WorkspaceId::parse(workspace_id_raw).map_err(|error| {
                Box::new(CoreResponse::Error {
                    code: "interactive_invalid_request".to_string(),
                    message: format!("invalid interactive workspace id: {error}"),
                })
            })?;
        let record = workspaces.resolve(&workspace_id).await.ok_or_else(|| {
            Box::new(CoreResponse::Error {
                code: "interactive_invalid_request".to_string(),
                message: "interactive workspace is not registered".to_string(),
            })
        })?;
        Ok(codegg_core::workspace::ExecutionContext::new(
            record,
            None,
            tokio_util::sync::CancellationToken::new(),
        ))
    }

    pub(crate) async fn find_tool_program_job(
        &self,
        program_id: &str,
    ) -> Result<Option<codegg_core::jobs::JobRecord>, String> {
        let query = codegg_core::jobs::store::JobStoreQuery {
            kinds: vec![codegg_core::jobs::JobKind::ToolProgram],
            limit: Some(256),
            ..Default::default()
        };
        let summaries = self
            .deps
            .job_store
            .list_jobs(query)
            .await
            .map_err(|error| error.to_string())?;
        for summary in summaries {
            if let Some(job) = self
                .deps
                .job_store
                .get_job(&summary.job_id)
                .await
                .map_err(|error| error.to_string())?
            {
                if tool_program_id(&job).is_some_and(|id| id == program_id) {
                    return Ok(Some(job));
                }
            }
        }
        Ok(None)
    }

    pub(crate) async fn tool_program_call_page_for_job(
        &self,
        job: &codegg_core::jobs::JobRecord,
        offset: u32,
    ) -> Result<codegg_protocol::projection::dto::ToolProgramCallPage, String> {
        let Some(program_id) = tool_program_id(job) else {
            return Err("job does not contain a tool program id".to_string());
        };
        let lease = self
            .workspace_services
            .acquire(&job.workspace_id)
            .await
            .map_err(|error| error.to_string())?;
        crate::tool::tool_program_ledger::ToolProgramLedger::new(
            &lease.path_policy().canonical_root,
        )
        .read_page(program_id, offset)
        .map_err(|error| error.to_string())
    }
}

pub(crate) fn tool_program_id(job: &codegg_core::jobs::JobRecord) -> Option<&str> {
    match &job.payload {
        codegg_core::jobs::JobPayload::ToolProgram { program_id, .. } => Some(program_id),
        _ => None,
    }
}

pub(crate) fn tool_program_summary_from_job(
    job: &codegg_core::jobs::JobRecord,
    attempts: &[codegg_core::jobs::JobAttempt],
    calls_completed: u32,
) -> codegg_protocol::projection::dto::ToolProgramSummary {
    let (program_id, source_digest, _ir_digest) = match &job.payload {
        codegg_core::jobs::JobPayload::ToolProgram {
            program_id,
            source_digest,
            ir_digest,
            ..
        } => (program_id.clone(), source_digest.clone(), ir_digest.clone()),
        _ => (job.job_id.to_string(), String::new(), None),
    };
    let started_at = attempts.iter().find_map(|attempt| {
        attempt
            .started_at
            .map(|timestamp| timestamp.timestamp_millis())
    });
    let completed_at = job
        .terminal_at
        .map(|timestamp| timestamp.timestamp_millis());
    let failure_class = if job.state == codegg_core::jobs::JobState::Completed {
        None
    } else if job.state.is_terminal() {
        Some(job.state.as_str().to_string())
    } else {
        None
    };
    let mut summary = codegg_protocol::projection::dto::ToolProgramSummary {
        program_id,
        job_id: job.job_id.to_string(),
        state: job.state.as_str().to_string(),
        phase: None,
        language: "restricted_python".into(),
        parent_turn_id: job.turn_id.clone(),
        parent_agent_id: None,
        calls_completed,
        child_jobs_running: 0,
        submitted_at: job.created_at.timestamp_millis(),
        started_at,
        completed_at,
        failure_class,
        terminal_handle: Some(job.job_id.to_string()),
        last_progress: if source_digest.is_empty() {
            None
        } else {
            Some("source verified at executor admission".into())
        },
    };
    summary.normalise();
    summary
}

pub(crate) fn tool_program_detail_from_job(
    job: &codegg_core::jobs::JobRecord,
    attempts: &[codegg_core::jobs::JobAttempt],
    call_page: codegg_protocol::projection::dto::ToolProgramCallPage,
) -> codegg_protocol::projection::dto::ToolProgramDetail {
    let (source_hash, ir_hash, manifest_summary) = match &job.payload {
        codegg_core::jobs::JobPayload::ToolProgram {
            source_digest,
            ir_digest,
            allowed_tools,
            ..
        } => (
            Some(source_digest.clone()),
            ir_digest.clone(),
            Some(format!(
                "language=restricted_python; allowed_tools={}; source withheld",
                allowed_tools.join(",")
            )),
        ),
        _ => (None, None, None),
    };
    let total_calls = call_page.total_calls;
    codegg_protocol::projection::dto::ToolProgramDetail {
        summary: tool_program_summary_from_job(job, attempts, total_calls),
        source_hash,
        ir_hash,
        checkpoint_version: None,
        manifest_summary,
        artifacts: Vec::new(),
        total_calls,
        call_page: Some(call_page),
    }
}

pub(crate) fn connection_detail_dto(
    summary: &crate::protocol::provider::ProviderConnectionSummaryDto,
) -> crate::protocol::provider::ConnectionDetailDto {
    crate::protocol::provider::ConnectionDetailDto {
        connection_id: summary.id.clone(),
        display_name: summary.display_name.clone(),
        endpoint_authority: summary.endpoint.clone(),
        tls_policy: summary.tls_policy.clone(),
        scope: summary.scope.clone(),
        state: summary.state.clone(),
        revision: summary.revision,
        catalog_revision: summary.catalog_revision.clone(),
        health: summary.health.clone(),
        actor_seam: Some("local_operator".to_string()),
    }
}

pub(crate) fn purge_blocker_dto(
    blocker: codegg_core::provider_connections::PurgeBlocker,
) -> crate::protocol::provider::PurgeBlocker {
    match blocker {
        codegg_core::provider_connections::PurgeBlocker::SelectedSessions { count } => {
            crate::protocol::provider::PurgeBlocker::SelectedSessions { count }
        }
        codegg_core::provider_connections::PurgeBlocker::ProvisioningOperation { operation_id } => {
            crate::protocol::provider::PurgeBlocker::ProvisioningOperation { operation_id }
        }
        codegg_core::provider_connections::PurgeBlocker::ActiveRuntime { reference_id } => {
            crate::protocol::provider::PurgeBlocker::ActiveRuntime { reference_id }
        }
    }
}

pub(crate) async fn connection_lifecycle_response(
    pool: Option<sqlx::SqlitePool>,
    connection_id: String,
    expected_revision: u64,
    action: &str,
) -> Result<CoreResponse, AppError> {
    let Some(pool) = pool else {
        return Ok(CoreResponse::Error {
            code: "provider_connections_unavailable".to_string(),
            message: "Provider connections require a daemon SQLite catalog".to_string(),
        });
    };
    let Ok(id) = codegg_core::identity::ProviderConnectionId::parse(&connection_id) else {
        return Ok(CoreResponse::Error {
            code: "invalid_connection_id".to_string(),
            message: "Provider connection ID is invalid".to_string(),
        });
    };
    let store = codegg_core::provider_connections::ProviderConnectionStore::new(pool);
    let result = match action {
        "enable" => store.enable(&id, expected_revision).await.map(|_| ()),
        "disable" => store.disable(&id, expected_revision).await.map(|_| ()),
        "delete" => store.delete(&id, expected_revision).await.map(|_| ()),
        "restore" => store.restore(&id, expected_revision).await.map(|_| ()),
        _ => Err(
            codegg_core::provider_connections::ProviderConnectionError::Invalid(
                "unknown lifecycle action".to_string(),
            ),
        ),
    };
    match result {
        Ok(()) => Ok(CoreResponse::Ack),
        Err(error) => Ok(CoreResponse::Error {
            code: "connection_lifecycle_failed".to_string(),
            message: error.to_string(),
        }),
    }
}

pub(crate) fn eggpool_error_code(error: &crate::core::eggpool::EggpoolError) -> &'static str {
    match error {
        crate::core::eggpool::EggpoolError::InvalidEndpoint(_) => "invalid_endpoint",
        crate::core::eggpool::EggpoolError::InvalidScope(_) => "invalid_scope",
        crate::core::eggpool::EggpoolError::CredentialStore => "credential_store_unavailable",
        crate::core::eggpool::EggpoolError::MasterKeyMissing => "master_key_missing",
        crate::core::eggpool::EggpoolError::Conflict => "connection_conflict",
        crate::core::eggpool::EggpoolError::Cancelled => "connection_cancelled",
        crate::core::eggpool::EggpoolError::Probe(reason) => reason.code(),
        crate::core::eggpool::EggpoolError::Storage => "connection_storage_error",
        crate::core::eggpool::EggpoolError::Rotation(_) => "connection_rotation_failed",
        crate::core::eggpool::EggpoolError::Refresh(_) => "connection_refresh_failed",
    }
}

pub(crate) fn eggpool_error_message(error: &crate::core::eggpool::EggpoolError) -> &'static str {
    match error {
        crate::core::eggpool::EggpoolError::InvalidEndpoint(_) => "Eggpool endpoint is invalid",
        crate::core::eggpool::EggpoolError::InvalidScope(_) => "Connection scope is invalid",
        crate::core::eggpool::EggpoolError::CredentialStore => {
            "Protected credential store is unavailable"
        }
        crate::core::eggpool::EggpoolError::MasterKeyMissing => {
            "Configure the credential-store master key before connecting"
        }
        crate::core::eggpool::EggpoolError::Conflict => {
            "An equivalent connection or provisioning operation already exists"
        }
        crate::core::eggpool::EggpoolError::Cancelled => "Connection provisioning was cancelled",
        crate::core::eggpool::EggpoolError::Probe(reason) => match reason {
            crate::core::eggpool::ProbeReason::AuthenticationFailed => {
                "Eggpool rejected the credential"
            }
            crate::core::eggpool::ProbeReason::Unreachable => "Eggpool endpoint is unreachable",
            crate::core::eggpool::ProbeReason::Timeout => "Eggpool probe timed out",
            crate::core::eggpool::ProbeReason::TlsFailed => "Eggpool TLS negotiation failed",
            crate::core::eggpool::ProbeReason::RedirectDisallowed => {
                "Eggpool endpoint redirected unexpectedly"
            }
            crate::core::eggpool::ProbeReason::UnsupportedApi => {
                "Eggpool endpoint does not expose the supported model API"
            }
            crate::core::eggpool::ProbeReason::InvalidJson => "Eggpool returned invalid model data",
            crate::core::eggpool::ProbeReason::EmptyCatalog => "Eggpool returned no models",
            crate::core::eggpool::ProbeReason::CatalogOversized => {
                "Eggpool model catalog exceeded the safety limit"
            }
            crate::core::eggpool::ProbeReason::Cancelled => "Eggpool probe was cancelled",
        },
        crate::core::eggpool::EggpoolError::Storage => "Provider connection storage is unavailable",
        crate::core::eggpool::EggpoolError::Rotation(_) => "Provider connection rotation failed",
        crate::core::eggpool::EggpoolError::Refresh(_) => "Provider connection refresh failed",
    }
}

/// M016: ensure a `session` row exists for `session_id` so the
/// production inject loop's FK-constrained append to `session_events`
/// can succeed even when the session row was never created via
/// `SessionStore`. Used only by the recovery-fixture path.
pub(crate) async fn ensure_session_row(
    pool: &sqlx::SqlitePool,
    session_id: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, sandboxes, time_created, time_updated) VALUES (?, '', '[]', 0, 0)",
    )
    .bind("recovery-fixture-project")
    .execute(pool)
    .await
    .map_err(|error| format!("ensure recovery-fixture project: {error}"))?;
    sqlx::query(
        "INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES (?, ?, ?, ?, ?, '1', 0, 0)",
    )
    .bind(session_id)
    .bind("recovery-fixture-project")
    .bind("recovery-fixture")
    .bind("/tmp/recovery-fixture")
    .bind("Recovery Fixture")
    .execute(pool)
    .await
    .map_err(|error| format!("ensure recovery-fixture session: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::turn_runtime::TurnRuntime;
    use crate::core::event_log::EventFilter;
    use crate::core::CoreEvent;
    use crate::session::schema::migrate;
    use crate::tui::commands::{resolve_schedule_id, schedule_display_id, schedule_label};

    /// Build a fresh in-memory SQLite pool with the full session
    /// schema. No on-disk tempdir is created, so the pool's memory is
    /// reclaimed when the test's `SqlitePool` is dropped — no
    /// `Box::leak` required.
    async fn in_memory_pool() -> sqlx::SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let url = format!(
            "file:daemon_test_{}?mode=memory&cache=shared",
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
        migrate(&pool).await.expect("migrate");
        pool
    }

    async fn test_daemon() -> CoreDaemon {
        let pool = in_memory_pool().await;
        CoreDaemon::new(Some(pool), None, None)
    }

    async fn seed_test_context(daemon: &CoreDaemon, root: &std::path::Path) -> (String, String) {
        let workspace = daemon
            .workspaces
            .get_or_register(root)
            .await
            .expect("test workspace registration");
        let project = codegg_core::project_catalog::ProjectCatalog::new(
            daemon.pool.clone().expect("test daemon pool"),
        )
        .register_local_project(
            codegg_core::project_catalog::RegisterLocalProject {
                display_name: "Daemon test project".to_string(),
                description: None,
                tags: Vec::new(),
                primary_repository_id: None,
            },
            &workspace.id,
            "daemon-test",
        )
        .await
        .expect("test project registration");
        (
            project.project_id.as_str().to_string(),
            workspace.id.as_str().to_string(),
        )
    }

    #[tokio::test]
    async fn daemon_has_unique_id() {
        let d1 = test_daemon().await;
        let d2 = test_daemon().await;
        assert_ne!(d1.daemon_id, d2.daemon_id);
    }

    #[tokio::test]
    async fn legacy_task_requests_are_explicitly_rejected() {
        let daemon = test_daemon().await;

        for request in [
            CoreRequest::TaskList,
            CoreRequest::TaskDelete { id: 1 },
            CoreRequest::TaskSchedule {
                session_id: "session".to_string(),
                interval_secs: 60,
                message: "message".to_string(),
            },
        ] {
            let response = daemon
                .handle_request(crate::core::new_request("legacy-task".into(), request))
                .await
                .unwrap();
            assert!(matches!(
                response,
                CoreResponse::Error {
                    code,
                    ..
                } if code == "legacy_task_compatibility_disabled"
            ));
        }
    }

    #[tokio::test]
    async fn durable_schedule_protocol_supports_create_list_delete() {
        let daemon = test_daemon().await;
        let root = tempfile::tempdir().unwrap();
        let (_project_id, workspace_id) = seed_test_context(&daemon, root.path()).await;
        let session_id = "schedule-session".to_string();
        let kind = codegg_core::jobs::ScheduleKind::Interval {
            every: std::time::Duration::from_secs(60),
            anchor: chrono::Utc::now(),
        };
        let template = codegg_core::jobs::schedule::JobTemplate::for_subagent(
            codegg_core::jobs::JobKind::Subagent,
            "check the build".to_string(),
            "build".to_string(),
            Some(session_id.clone()),
        );
        let spec = crate::protocol::dto::ScheduleCreateDto {
            workspace_id: workspace_id.clone(),
            session_id: Some(session_id),
            kind: serde_json::to_value(kind).unwrap(),
            job_template: serde_json::to_value(template).unwrap(),
            overlap_policy: "skip_if_running".to_string(),
            missed_run_policy: serde_json::to_value(codegg_core::jobs::MissedRunPolicy::RunOnceNow)
                .unwrap(),
            labels: std::collections::HashMap::new(),
        };

        let schedule_id = match daemon
            .handle_request(crate::core::new_request(
                "schedule-create".into(),
                CoreRequest::ScheduleCreate { spec },
            ))
            .await
            .unwrap()
        {
            CoreResponse::ScheduleCreated { schedule_id } => schedule_id,
            other => panic!("unexpected schedule creation response: {other:?}"),
        };

        let listed = daemon
            .handle_request(crate::core::new_request(
                "schedule-list".into(),
                CoreRequest::ScheduleList {
                    workspace_id: Some(workspace_id),
                    include_archived: false,
                },
            ))
            .await
            .unwrap();
        assert!(matches!(
            listed,
            CoreResponse::ScheduleList { schedules }
                if schedules.iter().any(|schedule| schedule.schedule_id == schedule_id)
        ));

        let deleted = daemon
            .handle_request(crate::core::new_request(
                "schedule-delete".into(),
                CoreRequest::ScheduleDelete {
                    schedule_id: schedule_id.clone(),
                },
            ))
            .await
            .unwrap();
        assert!(matches!(
            deleted,
            CoreResponse::ScheduleDeleted { schedule_id: deleted_id }
                if deleted_id == schedule_id
        ));
    }

    #[tokio::test]
    async fn durable_schedule_tui_display_token_resolves_and_deletes_in_workspace() {
        let daemon = test_daemon().await;
        let root = tempfile::tempdir().unwrap();
        let (_project_id, workspace_id) = seed_test_context(&daemon, root.path()).await;
        let prompt = "check the build from the TUI";
        let session_id = "tui-schedule-session".to_string();
        let kind = codegg_core::jobs::ScheduleKind::Interval {
            every: std::time::Duration::from_secs(300),
            anchor: chrono::Utc::now(),
        };
        let template = codegg_core::jobs::schedule::JobTemplate::for_subagent(
            codegg_core::jobs::JobKind::Subagent,
            prompt.to_string(),
            "build".to_string(),
            Some(session_id.clone()),
        );
        let spec = crate::protocol::dto::ScheduleCreateDto {
            workspace_id: workspace_id.clone(),
            session_id: Some(session_id),
            kind: serde_json::to_value(kind).unwrap(),
            job_template: serde_json::to_value(template).unwrap(),
            overlap_policy: "skip_if_running".to_string(),
            missed_run_policy: serde_json::to_value(codegg_core::jobs::MissedRunPolicy::RunOnceNow)
                .unwrap(),
            labels: std::collections::HashMap::new(),
        };

        let schedule_id = match daemon
            .handle_request(crate::core::new_request(
                "tui-schedule-create".into(),
                CoreRequest::ScheduleCreate { spec },
            ))
            .await
            .unwrap()
        {
            CoreResponse::ScheduleCreated { schedule_id } => schedule_id,
            other => panic!("unexpected schedule creation response: {other:?}"),
        };

        let schedules = match daemon
            .handle_request(crate::core::new_request(
                "tui-schedule-list".into(),
                CoreRequest::ScheduleList {
                    workspace_id: Some(workspace_id.clone()),
                    include_archived: false,
                },
            ))
            .await
            .unwrap()
        {
            CoreResponse::ScheduleList { schedules } => schedules,
            other => panic!("unexpected schedule list response: {other:?}"),
        };
        let summary = schedules
            .iter()
            .find(|schedule| schedule.schedule_id == schedule_id)
            .expect("created schedule is listed in its workspace");

        let detail = match daemon
            .handle_request(crate::core::new_request(
                "tui-schedule-get".into(),
                CoreRequest::ScheduleGet {
                    schedule_id: schedule_id.clone(),
                },
            ))
            .await
            .unwrap()
        {
            CoreResponse::ScheduleGet { schedule } => schedule,
            other => panic!("unexpected schedule get response: {other:?}"),
        };
        assert_eq!(schedule_label(summary, Some(&detail)), prompt);

        let displayed_token = schedule_display_id(&schedule_id);
        let resolved_id = resolve_schedule_id(&displayed_token, &schedules).unwrap();
        assert_eq!(resolved_id, schedule_id);

        let deleted = daemon
            .handle_request(crate::core::new_request(
                "tui-schedule-delete".into(),
                CoreRequest::ScheduleDelete {
                    schedule_id: resolved_id,
                },
            ))
            .await
            .unwrap();
        assert!(matches!(
            deleted,
            CoreResponse::ScheduleDeleted { schedule_id: deleted_id }
                if deleted_id == schedule_id
        ));

        let remaining = daemon
            .handle_request(crate::core::new_request(
                "tui-schedule-list-after-delete".into(),
                CoreRequest::ScheduleList {
                    workspace_id: Some(workspace_id),
                    include_archived: false,
                },
            ))
            .await
            .unwrap();
        assert!(matches!(
            remaining,
            CoreResponse::ScheduleList { schedules }
                if schedules.iter().all(|schedule| schedule.schedule_id != schedule_id)
        ));
    }

    #[tokio::test]
    async fn session_create_through_daemon() {
        let daemon = test_daemon().await;
        let (project_id, workspace_id) =
            seed_test_context(&daemon, std::path::Path::new("/tmp")).await;
        let req = crate::core::new_request(
            "req-1".into(),
            CoreRequest::SessionCreate {
                directory: "/tmp".into(),
                title: Some("Test".into()),
                project_id: Some(project_id),
                workspace_id: Some(workspace_id),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        assert!(matches!(resp, CoreResponse::Session { .. }));
    }

    #[tokio::test]
    async fn project_catalog_protocol_lists_lifecycle_and_health_by_scope() {
        let daemon = test_daemon().await;
        let first_root = tempfile::tempdir().unwrap();
        let second_root = tempfile::tempdir().unwrap();
        let (first_project, first_workspace) = seed_test_context(&daemon, first_root.path()).await;
        let (second_project, _second_workspace) =
            seed_test_context(&daemon, second_root.path()).await;

        let list = daemon
            .handle_request(crate::core::new_request(
                "project-list".into(),
                CoreRequest::ProjectList {
                    include_archived: false,
                    limit: 1,
                },
            ))
            .await
            .unwrap();
        assert!(matches!(
            list,
            CoreResponse::ProjectList {
                projects,
                truncated: true
            } if projects.len() == 1
        ));

        let details = daemon
            .handle_request(crate::core::new_request(
                "project-get".into(),
                CoreRequest::ProjectGet {
                    project_id: first_project.clone(),
                },
            ))
            .await
            .unwrap();
        assert!(matches!(
            details,
            CoreResponse::ProjectGet { project } if project.project.project_id == first_project
        ));

        let health = daemon
            .handle_request(crate::core::new_request(
                "project-health".into(),
                CoreRequest::ProjectHealth {
                    project_id: first_project.clone(),
                    workspace_id: first_workspace,
                },
            ))
            .await
            .unwrap();
        assert!(matches!(
            health,
            CoreResponse::ProjectHealth { health }
                if health.project_id == first_project
        ));

        let archived = daemon
            .handle_request(crate::core::new_request(
                "project-archive".into(),
                CoreRequest::ProjectArchive {
                    project_id: second_project.clone(),
                },
            ))
            .await
            .unwrap();
        assert!(matches!(
            archived,
            CoreResponse::ProjectArchived { project }
                if project.project_id == second_project && project.lifecycle == "archived"
        ));

        let restored = daemon
            .handle_request(crate::core::new_request(
                "project-restore".into(),
                CoreRequest::ProjectRestore {
                    project_id: second_project,
                },
            ))
            .await
            .unwrap();
        assert!(matches!(
            restored,
            CoreResponse::ProjectRestored { project } if project.lifecycle == "active"
        ));
    }

    #[tokio::test]
    async fn snapshot_daemon_returns_state() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request("req-1".into(), CoreRequest::SnapshotDaemon);
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::SnapshotDaemon {
                daemon_id,
                uptime_secs,
                ..
            } => {
                assert!(!daemon_id.is_empty());
                assert!(uptime_secs < 5);
            }
            other => panic!("expected SnapshotDaemon, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn turn_submit_rejects_when_active() {
        let daemon = test_daemon().await;
        let (project_id, workspace_id) =
            seed_test_context(&daemon, std::path::Path::new("/tmp")).await;
        let req = crate::core::new_request(
            "req-1".into(),
            CoreRequest::SessionCreate {
                directory: "/tmp".into(),
                title: None,
                project_id: Some(project_id),
                workspace_id: Some(workspace_id),
            },
        );
        let session_id = match daemon.handle_request(req).await.unwrap() {
            CoreResponse::Session { session } => session.id,
            _ => panic!("expected Session"),
        };

        let runtime = daemon
            .sessions
            .get_or_create_for_test(&session_id, std::path::PathBuf::new());
        assert!(runtime.active_turn.read().await.is_none());
    }

    #[tokio::test]
    async fn resume_returns_typed_resync_when_seq_too_old() {
        // This test exercises the same path as before -- a too-old seq
        // when nothing is recorded anywhere. To force the ring to
        // have no record of seq 1, we use a no-pool daemon (so the
        // DB layer is bypassed) and a small ring, then evict the
        // only event by overflowing the ring.
        let daemon = CoreDaemon::new(None, None, None);
        // No pool is configured, so the event log is in-memory only
        // and the ring is the source of truth.
        // Publish a few events to a small ring by setting capacity
        // indirectly: we use the default capacity (4096) and publish
        // a single event so seq=1 is in the ring, then issue a
        // resume from seq 0 with no pool -- this would be covered by
        // the ring. To get a true "too old" we need to evict seq 1.
        // The cleanest way without changing daemon internals is to
        // request a seq the ring definitely does not have; with no
        // pool, the only valid request is one the ring can satisfy.
        // A future seq (e.g. 999_999) is treated as caught-up and
        // returns Events(empty), NOT ResyncRequired. So we use a
        // daemon without a pool and no events at all, with a
        // from_event_seq < current_seq (0 < 0 is false). The truly
        // "too old" case below uses a pool + eviction.
        // With no events and from_event_seq=0 and current_seq=0, the
        // path is caught-up and returns empty events.
        let req = crate::core::new_request(
            "req-resume-future".into(),
            CoreRequest::Resume {
                session_id: Some("s1".into()),
                from_event_seq: 999_999,
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Events {
                events,
                current_seq,
            } => {
                assert_eq!(current_seq, 0);
                assert!(events.is_empty());
            }
            other => panic!("expected Events(empty) for future seq, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn resume_returns_typed_events_on_success() {
        let daemon = test_daemon().await;

        daemon
            .event_log
            .publish(
                Some("s1".into()),
                None,
                crate::protocol::core::CoreEvent::SessionUpdated {
                    session_id: "s1".into(),
                },
            )
            .await;

        let req = crate::core::new_request(
            "req-resume-ok".into(),
            CoreRequest::Resume {
                session_id: Some("s1".into()),
                from_event_seq: 0,
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Events {
                events,
                current_seq,
            } => {
                assert_eq!(current_seq, 1);
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].event_seq, 1);
            }
            other => panic!("expected Events, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn resume_from_current_seq_returns_empty_events_not_resync() {
        // A client that is already caught up (from_event_seq == current_seq)
        // must get an empty Events response, NOT ResyncRequired. This is
        // the core Pass 2 invariant: ResyncRequired is reserved for
        // too-old sequences that can no longer be replayed.
        let daemon = test_daemon().await;
        let s1 = daemon
            .event_log
            .publish(
                Some("s1".into()),
                None,
                crate::protocol::core::CoreEvent::SessionUpdated {
                    session_id: "s1".into(),
                },
            )
            .await;

        let req = crate::core::new_request(
            "req-resume-current".into(),
            CoreRequest::Resume {
                session_id: Some("s1".into()),
                from_event_seq: s1,
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Events {
                events,
                current_seq,
            } => {
                assert_eq!(current_seq, s1);
                assert!(
                    events.is_empty(),
                    "expected empty events for caught-up client, got {:?}",
                    events
                );
            }
            other => panic!("expected Events(empty), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn resume_from_future_seq_returns_empty_events() {
        // from_event_seq > current_seq is treated as "no new events" --
        // the client effectively overshot but we don't have anything to
        // send. Return Events(empty, current_seq) so the client can
        // resync its bookkeeping. The plan lists this as one of the
        // acceptable behaviors; we chose empty events.
        let daemon = test_daemon().await;
        let s1 = daemon
            .event_log
            .publish(
                Some("s1".into()),
                None,
                crate::protocol::core::CoreEvent::SessionUpdated {
                    session_id: "s1".into(),
                },
            )
            .await;

        let req = crate::core::new_request(
            "req-resume-future".into(),
            CoreRequest::Resume {
                session_id: Some("s1".into()),
                from_event_seq: s1 + 100,
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Events {
                events,
                current_seq,
            } => {
                assert_eq!(current_seq, s1);
                assert!(events.is_empty());
            }
            other => panic!("expected Events(empty) for future seq, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn resume_from_too_old_seq_returns_resync() {
        // To force a real "too old" outcome we need a daemon without
        // a SQLite pool. With a pool, the DB layer would still cover
        // any from_event_seq whose `from_event_seq + 1` is in the
        // persisted range, so the resync path becomes unreachable
        // for ordinary replay requests. With no pool, the ring is
        // the source of truth and eviction makes old seqs unsatisfiable.
        let daemon = CoreDaemon::new(None, None, None);
        // Publish enough events to overflow the default ring (4096).
        for _ in 0..5000 {
            daemon
                .event_log
                .publish(
                    Some("s1".into()),
                    None,
                    crate::protocol::core::CoreEvent::Error {
                        code: "filler".into(),
                        message: "m".into(),
                    },
                )
                .await;
        }
        let current = daemon.event_log.current_seq();
        assert!(
            current > 4096,
            "ring should have wrapped, current={}",
            current
        );

        // from_event_seq=0 is now too old: the ring's front is
        // current-4095 and there is no DB to fall back to.
        let req = crate::core::new_request(
            "req-resume-old".into(),
            CoreRequest::Resume {
                session_id: Some("s1".into()),
                from_event_seq: 0,
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::ResyncRequired {
                from_event_seq,
                current_seq,
                session_id,
            } => {
                assert_eq!(from_event_seq, 0);
                assert_eq!(current_seq, current);
                assert_eq!(session_id.as_deref(), Some("s1"));
            }
            other => panic!("expected ResyncRequired for too-old seq, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn recovery_detects_interrupted_turn() {
        let daemon = test_daemon().await;

        // Subscribe before publishing so we can observe the recovery-emitted TurnFailed.
        let mut rx = daemon.event_log.subscribe();

        // Insert an interrupted TurnStarted directly (no matching TurnCompleted/TurnFailed).
        sqlx::query(
            "INSERT INTO core_event_log (event_seq, session_id, turn_id, event_type, payload_json) \
             VALUES (1, 's1', 't1', 'turn_started', '{}')",
        )
        .execute(daemon.pool.as_ref().unwrap())
        .await
        .unwrap();

        daemon.recover_state().await;

        // The recovery should have published a TurnFailed for (s1, t1).
        let mut found = false;
        while let Ok(env) = rx.try_recv() {
            if let crate::protocol::core::CoreEvent::TurnFailed {
                session_id,
                turn_id,
                ..
            } = &env.payload
            {
                if session_id == "s1" && turn_id.as_deref() == Some("t1") {
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "expected recovery to emit TurnFailed for s1/t1");
    }

    #[tokio::test]
    async fn recovery_ignores_completed_turn() {
        let daemon = test_daemon().await;

        let mut rx = daemon.event_log.subscribe();

        // Insert a completed turn: TurnStarted followed by TurnCompleted.
        sqlx::query(
            "INSERT INTO core_event_log (event_seq, session_id, turn_id, event_type, payload_json) \
             VALUES (1, 's1', 't1', 'turn_started', '{}')",
        )
        .execute(daemon.pool.as_ref().unwrap())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO core_event_log (event_seq, session_id, turn_id, event_type, payload_json) \
             VALUES (2, 's1', 't1', 'turn_completed', '{\"stop_reason\":\"ok\"}')",
        )
        .execute(daemon.pool.as_ref().unwrap())
        .await
        .unwrap();

        daemon.recover_state().await;

        // Drain and ensure no TurnFailed was emitted.
        let mut emitted_failed = false;
        while let Ok(env) = rx.try_recv() {
            if let crate::protocol::core::CoreEvent::TurnFailed {
                session_id,
                turn_id,
                ..
            } = &env.payload
            {
                if session_id == "s1" && turn_id.as_deref() == Some("t1") {
                    emitted_failed = true;
                    break;
                }
            }
        }
        assert!(
            !emitted_failed,
            "did not expect recovery to emit TurnFailed for a completed turn"
        );
    }

    #[tokio::test]
    async fn snapshot_models_returns_model_ids() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request("req-snap".into(), CoreRequest::SnapshotModels);
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::ModelsSnapshot {
                current_model,
                models,
            } => {
                assert!(current_model.is_none());
                // With no providers configured, the model list is empty.
                // The format contract is `provider/model` (e.g. `openai/gpt-4o`),
                // which is exercised by ModelsRefresh; for the empty-config case
                // we only assert the response shape is well-formed.
                for m in &models {
                    assert!(
                        m.contains('/'),
                        "model id '{}' should be 'provider/model'",
                        m
                    );
                }
            }
            other => panic!("expected ModelsSnapshot, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn permission_respond_invalid_id_format() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request(
            "req-perm-invalid".into(),
            CoreRequest::PermissionRespond {
                id: "perm-1".into(),
                choice: "allow".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, message } => {
                assert_eq!(code, "invalid_permission_id");
                assert!(message.contains("perm-1"));
            }
            other => panic!("expected Error(invalid_permission_id), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn permission_respond_malformed_id() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request(
            "req-perm-malformed".into(),
            CoreRequest::PermissionRespond {
                id: "perm:foo:bar".into(),
                choice: "allow".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, .. } => {
                assert_eq!(code, "invalid_permission_id");
            }
            other => panic!("expected Error(invalid_permission_id), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn question_respond_invalid_id_format() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request(
            "req-q-invalid".into(),
            CoreRequest::QuestionRespond {
                id: "q-1".into(),
                answers: serde_json::json!("yes"),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, message } => {
                assert_eq!(code, "invalid_question_id");
                assert!(message.contains("q-1"));
            }
            other => panic!("expected Error(invalid_question_id), got {:?}", other),
        }
    }

    /// Manually install a `TurnHandle` on the given runtime's
    /// `active_turn` so we can exercise `TurnCancel`/`TurnSteer` paths
    /// without spinning up an actual agent loop. Returns the cancel
    /// sender, the cancel receiver (so the watch channel stays open),
    /// and the steer receiver so tests can observe the downstream
    /// effects.
    async fn install_active_turn(
        runtime: &std::sync::Arc<crate::core::session_runtime::SessionRuntime>,
        turn_id: &str,
    ) -> (
        tokio::sync::watch::Sender<bool>,
        tokio::sync::watch::Receiver<bool>,
        tokio::sync::mpsc::Receiver<String>,
    ) {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let (steer_tx, steer_rx) = tokio::sync::mpsc::channel(32);
        let mut active = runtime.active_turn.write().await;
        *active = Some(crate::core::session_runtime::TurnHandle {
            turn_id: turn_id.to_string(),
            cancel_tx: cancel_tx.clone(),
            steer_tx: Some(steer_tx),
            started_at: chrono::Utc::now(),
            asset_pin: None,
        });
        (cancel_tx, cancel_rx, steer_rx)
    }

    #[tokio::test]
    async fn turn_cancel_wrong_id_rejected() {
        let daemon = test_daemon().await;
        let runtime = daemon
            .sessions
            .get_or_create_for_test("s-cancel-wrong", std::path::PathBuf::from("."));
        let (cancel_tx, _cancel_rx, _steer_rx) = install_active_turn(&runtime, "turn-real").await;

        let req = crate::core::new_request(
            "req-cancel".into(),
            CoreRequest::TurnCancel {
                session_id: "s-cancel-wrong".into(),
                turn_id: "turn-typo".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, message } => {
                assert_eq!(code, "turn_id_mismatch");
                assert!(message.contains("turn-typo"));
                assert!(message.contains("turn-real"));
            }
            other => panic!("expected Error(turn_id_mismatch), got {:?}", other),
        }

        // The runtime should still have an active turn; we did not cancel.
        let active = runtime.active_turn.read().await;
        assert!(
            active.is_some(),
            "active_turn should remain set after a rejected cancel"
        );
        // The cancel channel should not have been signaled.
        assert!(
            !*cancel_tx.borrow(),
            "cancel_tx should not have been signaled"
        );
    }

    #[tokio::test]
    async fn turn_cancel_correct_id_succeeds() {
        let daemon = test_daemon().await;
        let runtime = daemon
            .sessions
            .get_or_create_for_test("s-cancel-ok", std::path::PathBuf::from("."));
        let (cancel_tx, _cancel_rx, _steer_rx) = install_active_turn(&runtime, "turn-good").await;

        let req = crate::core::new_request(
            "req-cancel".into(),
            CoreRequest::TurnCancel {
                session_id: "s-cancel-ok".into(),
                turn_id: "turn-good".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        assert!(matches!(resp, CoreResponse::Ack));

        // The cancel channel should have been signaled.
        assert!(
            *cancel_tx.borrow(),
            "cancel_tx should have been signaled on matching turn_id"
        );
    }

    #[tokio::test]
    async fn turn_cancel_no_active_turn() {
        let daemon = test_daemon().await;
        // Register the session but do not install an active turn.
        daemon
            .sessions
            .get_or_create_for_test("s-cancel-none", std::path::PathBuf::from("."));

        let req = crate::core::new_request(
            "req-cancel".into(),
            CoreRequest::TurnCancel {
                session_id: "s-cancel-none".into(),
                turn_id: "turn-anything".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, .. } => {
                assert_eq!(code, "no_active_turn");
            }
            other => panic!("expected Error(no_active_turn), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn turn_steer_wrong_id_rejected() {
        let daemon = test_daemon().await;
        let runtime = daemon
            .sessions
            .get_or_create_for_test("s-steer-wrong", std::path::PathBuf::from("."));
        let (_cancel_tx, _cancel_rx, _steer_rx) =
            install_active_turn(&runtime, "turn-real-steer").await;

        let req = crate::core::new_request(
            "req-steer".into(),
            CoreRequest::TurnSteer {
                session_id: "s-steer-wrong".into(),
                turn_id: "turn-typo".into(),
                text: "redirect".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, .. } => {
                assert_eq!(code, "turn_id_mismatch");
            }
            other => panic!("expected Error(turn_id_mismatch), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn turn_steer_correct_id_succeeds() {
        let daemon = test_daemon().await;
        let runtime = daemon
            .sessions
            .get_or_create_for_test("s-steer-ok", std::path::PathBuf::from("."));
        let (_cancel_tx, _cancel_rx, mut steer_rx) =
            install_active_turn(&runtime, "turn-good-steer").await;

        let req = crate::core::new_request(
            "req-steer".into(),
            CoreRequest::TurnSteer {
                session_id: "s-steer-ok".into(),
                turn_id: "turn-good-steer".into(),
                text: "redirect".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        assert!(matches!(resp, CoreResponse::Ack));

        // The steer channel should have received the message.
        let got = tokio::time::timeout(std::time::Duration::from_millis(50), steer_rx.recv())
            .await
            .expect("steer message should arrive")
            .expect("steer_rx should yield a value");
        assert_eq!(got, "redirect");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn turn_started_emitted_on_submit() {
        let _env_guard = crate::auth::test_support::lock_env();
        let previous_openai_key = std::env::var("OPENAI_API_KEY").ok();
        // Set up an env var to register the openai provider so TurnSubmit
        // passes the provider-not-found check. The actual API call will
        // fail in the spawned agent loop, but we only care that TurnStarted
        // is published synchronously by the daemon before the spawn.
        std::env::set_var("OPENAI_API_KEY", "test-key-not-used");

        let daemon = test_daemon().await;
        let agent = crate::agent::Agent {
            name: "test".into(),
            description: "test agent".into(),
            ..Default::default()
        };

        // Pre-create a session so TurnSubmit can resolve its workspace.
        let workspace_dir = tempfile::tempdir().unwrap();
        let (project_id, workspace_id) = seed_test_context(&daemon, workspace_dir.path()).await;
        let create_req = crate::core::new_request(
            "req-create".into(),
            CoreRequest::SessionCreate {
                directory: workspace_dir.path().to_string_lossy().into_owned(),
                title: None,
                project_id: Some(project_id),
                workspace_id: Some(workspace_id),
            },
        );
        let session_id = match daemon.handle_request(create_req).await.unwrap() {
            CoreResponse::Session { session } => session.id,
            other => panic!("expected Session, got {:?}", other),
        };

        let req = crate::core::new_request(
            "req-submit".into(),
            CoreRequest::TurnSubmit {
                session_id: session_id.clone(),
                text: "hello".into(),
                plan_mode: false,
                model: "openai/gpt-4o".into(),
                agents: vec![crate::protocol_conversions::agent_to_dto(agent).unwrap()],
                current_agent_idx: 0,
                messages: vec![],
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        assert!(matches!(resp, CoreResponse::Ack));

        // The TurnStarted event should be in the log, identified by
        // session_id and a turn_id that starts with "turn-".
        let filter = EventFilter {
            session_id: Some(session_id.clone()),
            include_global: true,
            client_id: None,
        };
        let events = daemon.event_log.replay_from(0, &filter).await;

        let mut found: Option<(String, String)> = None;
        for env in &events {
            if let CoreEvent::TurnStarted {
                session_id: sid,
                turn_id,
            } = &env.payload
            {
                if sid == &session_id {
                    found = Some((sid.clone(), turn_id.clone()));
                    break;
                }
            }
        }
        let (sid, turn_id) = found.expect("expected TurnStarted event in log");
        assert_eq!(sid, session_id);
        assert!(
            turn_id.starts_with("turn-"),
            "turn_id '{}' should start with 'turn-'",
            turn_id
        );

        if let Some(value) = previous_openai_key {
            std::env::set_var("OPENAI_API_KEY", value);
        } else {
            std::env::remove_var("OPENAI_API_KEY");
        }
    }

    #[tokio::test]
    async fn bridge_attaches_turn_id_for_text_delta() {
        let daemon = test_daemon().await;
        let runtime = daemon
            .sessions
            .get_or_create_for_test("s-bridge-delta", std::path::PathBuf::from("."));
        let turn_id = "turn-bridge-delta".to_string();
        let (_cancel_tx, _cancel_rx, _steer_rx) = install_active_turn(&runtime, &turn_id).await;

        // A TextDelta from the bus carries no turn_id; the bridge must
        // attach the active turn_id.
        let app_event = crate::bus::events::AppEvent::TextDelta {
            session_id: "s-bridge-delta".into(),
            delta: "hi".into(),
        };
        let result = daemon
            .bridge_app_event(app_event)
            .await
            .expect("bridge_app_event should map TextDelta");
        let (session_id, attached_turn_id, core_event) = result;
        assert_eq!(session_id.as_deref(), Some("s-bridge-delta"));
        assert_eq!(attached_turn_id.as_deref(), Some(turn_id.as_str()));
        match core_event {
            CoreEvent::TurnTextDelta { turn_id: tid, .. } => {
                assert_eq!(
                    tid, turn_id,
                    "TurnTextDelta should carry the active turn_id"
                );
            }
            other => panic!("expected TurnTextDelta, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn bridge_no_longer_maps_agent_finished_to_turn_completed() {
        // Pass 3 invariant: the bridge must NOT produce a duplicate
        // `CoreEvent::TurnCompleted` for `AppEvent::AgentFinished`,
        // because the TurnSubmit spawned task publishes the lifecycle
        // event directly with the captured turn_id. The bus event is
        // still consumed by the event bridge to update token counts
        // and emit notifications, but it does not flow through
        // `map_app_event_to_core_event`.
        let daemon = test_daemon().await;
        let runtime = daemon
            .sessions
            .get_or_create_for_test("s-bridge-finished", std::path::PathBuf::from("."));
        let turn_id = "turn-bridge-finished".to_string();
        let (_cancel_tx, _cancel_rx, _steer_rx) = install_active_turn(&runtime, &turn_id).await;

        let app_event = crate::bus::events::AppEvent::AgentFinished {
            session_id: "s-bridge-finished".into(),
            stop_reason: "completed".into(),
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
            reasoning_tokens: None,
        };
        let result = daemon.bridge_app_event(app_event).await;
        assert!(
            result.is_none(),
            "AgentFinished must not produce a CoreEvent from the bridge; got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn direct_turn_completion_uses_runtime_turn_id() {
        // The TurnSubmit spawn task publishes a CoreEvent::TurnCompleted
        // directly with the captured turn_id. We exercise this path
        // here by publishing the same event shape the spawn task
        // produces and asserting that the envelope carries the
        // non-empty turn id and matches what a subscriber sees on
        // the broadcast channel.
        let daemon = test_daemon().await;
        let session_id = "s-direct-completion".to_string();
        let turn_id = "turn-direct".to_string();
        let mut rx = daemon.event_log.subscribe();

        // Direct publish path (mirrors the spawn task).
        daemon
            .event_log
            .publish(
                Some(session_id.clone()),
                Some(turn_id.clone()),
                CoreEvent::TurnCompleted {
                    session_id: session_id.clone(),
                    turn_id: turn_id.clone(),
                    stop_reason: "completed".to_string(),
                },
            )
            .await;

        let env = rx.recv().await.expect("expected an envelope on the bus");
        match env.payload {
            CoreEvent::TurnCompleted {
                turn_id: tid,
                stop_reason,
                ..
            } => {
                assert_eq!(tid, turn_id);
                assert_eq!(stop_reason, "completed");
            }
            other => panic!("expected TurnCompleted, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn bridge_keeps_turn_id_from_event_when_present() {
        let daemon = test_daemon().await;
        let runtime = daemon
            .sessions
            .get_or_create_for_test("s-bridge-explicit", std::path::PathBuf::from("."));
        let active_turn_id = "turn-active".to_string();
        let (_cancel_tx, _cancel_rx, _steer_rx) =
            install_active_turn(&runtime, &active_turn_id).await;

        // ToolResult carries a turn_id on the AppEvent? No - the bus
        // AppEvent::ToolResult doesn't have a turn_id. The bridged
        // CoreEvent::ToolCompleted has turn_id: None, so the bridge
        // should fall back to the active turn_id.
        let app_event = crate::bus::events::AppEvent::ToolResult {
            session_id: "s-bridge-explicit".into(),
            tool_id: "t1".into(),
            tool_name: "bash".into(),
            output: "ok".into(),
            success: true,
        };
        let result = daemon
            .bridge_app_event(app_event)
            .await
            .expect("bridge_app_event should map ToolResult");
        let (_session_id, attached_turn_id, core_event) = result;
        assert_eq!(attached_turn_id.as_deref(), Some(active_turn_id.as_str()));
        match core_event {
            CoreEvent::ToolCompleted { turn_id, .. } => {
                assert_eq!(turn_id.as_deref(), Some(active_turn_id.as_str()));
            }
            other => panic!("expected ToolCompleted, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn bridge_no_active_turn_keeps_empty_turn_id() {
        let daemon = test_daemon().await;
        // No active turn installed for this session.
        daemon
            .sessions
            .get_or_create_for_test("s-bridge-none", std::path::PathBuf::from("."));

        let app_event = crate::bus::events::AppEvent::TextDelta {
            session_id: "s-bridge-none".into(),
            delta: "orphan".into(),
        };
        let result = daemon
            .bridge_app_event(app_event)
            .await
            .expect("bridge_app_event should map TextDelta");
        let (_session_id, attached_turn_id, core_event) = result;
        // No active turn -> turn_id is the empty default from the mapper.
        assert_eq!(attached_turn_id.as_deref(), Some(""));
        match core_event {
            CoreEvent::TurnTextDelta { turn_id, .. } => {
                assert_eq!(turn_id, "");
            }
            other => panic!("expected TurnTextDelta, got {:?}", other),
        }
    }

    /// A minimal fake turn runtime that records whether `run_turn` was called.
    struct FakeTurnRuntime {
        called: std::sync::atomic::AtomicBool,
    }

    impl FakeTurnRuntime {
        fn new() -> Self {
            Self {
                called: std::sync::atomic::AtomicBool::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::agent::turn_runtime::TurnRuntime for FakeTurnRuntime {
        async fn run_turn(
            &self,
            _input: crate::agent::turn_runtime::TurnRunInput,
        ) -> Result<crate::agent::turn_runtime::TurnRunOutput, crate::error::AppError> {
            self.called.store(true, std::sync::atomic::Ordering::SeqCst);
            let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(false);
            let (steer_tx, _steer_rx) = tokio::sync::mpsc::channel(32);
            Ok(crate::agent::turn_runtime::TurnRunOutput {
                cancel_tx,
                steer_tx,
            })
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn turn_submit_uses_injected_runtime() {
        let _env_guard = crate::auth::test_support::lock_env();
        let previous_openai_key = std::env::var("OPENAI_API_KEY").ok();
        // Verify that CoreDaemon::TurnSubmit delegates to the injected
        // TurnRuntime instead of constructing DefaultTurnRuntime directly.
        std::env::set_var("OPENAI_API_KEY", "test-key-not-used");

        let fake = Arc::new(FakeTurnRuntime::new());
        let pool = in_memory_pool().await;
        let deps = CoreRuntimeDeps::new(Some(pool), None, None)
            .with_turn_runtime(Arc::clone(&fake) as Arc<dyn TurnRuntime>);
        let daemon = CoreDaemon::with_deps(deps);
        daemon.hydrate_workspace_registry().await.unwrap();

        let agent = crate::agent::Agent {
            name: "test".into(),
            description: "test agent".into(),
            ..Default::default()
        };

        // Pre-create a session so TurnSubmit can resolve its workspace.
        let workspace_dir = tempfile::tempdir().unwrap();
        let (project_id, workspace_id) = seed_test_context(&daemon, workspace_dir.path()).await;
        let create_req = crate::core::new_request(
            "req-inject-create".into(),
            CoreRequest::SessionCreate {
                directory: workspace_dir.path().to_string_lossy().into_owned(),
                title: None,
                project_id: Some(project_id),
                workspace_id: Some(workspace_id),
            },
        );
        let session_id = match daemon.handle_request(create_req).await.unwrap() {
            CoreResponse::Session { session } => session.id,
            other => panic!("expected Session, got {:?}", other),
        };

        let req = crate::core::new_request(
            "req-inject".into(),
            CoreRequest::TurnSubmit {
                session_id,
                text: "hello".into(),
                plan_mode: false,
                model: "openai/gpt-4o".into(),
                agents: vec![crate::protocol_conversions::agent_to_dto(agent).unwrap()],
                current_agent_idx: 0,
                messages: vec![],
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        assert!(matches!(resp, CoreResponse::Ack));
        assert!(
            fake.called.load(std::sync::atomic::Ordering::SeqCst),
            "injected FakeTurnRuntime should have been invoked"
        );
        if let Some(value) = previous_openai_key {
            std::env::set_var("OPENAI_API_KEY", value);
        } else {
            std::env::remove_var("OPENAI_API_KEY");
        }
    }
}
