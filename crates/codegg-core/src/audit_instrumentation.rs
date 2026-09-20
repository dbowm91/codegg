//! Audit instrumentation and attribution closure (M005).
//!
//! M004 owns the append-only structural store ([`crate::audit`]). This
//! module owns the M005 executable coverage contract: which Phase-11
//! actions must emit events, which canonical owner emits each action,
//! which trusted actor/scope/decision supplies attribution, which
//! structural metadata keys are allowed, and how correlation and
//! causation link request -> session/turn -> run/task -> job/tool/Git
//! chains.
//!
//! ## Design notes
//!
//! - Instrumentation is emitted by canonical owners, not duplicate
//!   wrappers. The daemon dispatch arms are the canonical control-plane
//!   owners; the daemon maps one operation name through
//!   [`operation_to_audit_action`] to its structural
//!   [`AuditAction`](crate::audit::AuditAction) at the single
//!   post-authorization seam. Execution owners (agent/tool/process/file/
//!   Git/worktree/job) use the typed builders below with stored
//!   attribution; they never invent principals.
//! - Actor/correlation derives from trusted context. Every builder takes
//!   the transport-bound [`AuthenticatedPrincipal`] plus the gate-copied
//!   [`AuditDecisionProvenance`](crate::audit::AuditDecisionProvenance)
//!   from the M003 decision bridge. There are no setters for actor,
//!   decision, policy, or sequence.
//! - Secrets/body retention rules apply uniformly. Builders accept only
//!   structural locators, digests, bounded labels, and outcome enums.
//!   Prompt/file/tool output bodies are never accepted; digests are
//!   SHA-256 hex. The underlying [`AuditEventBuilder`](crate::audit::AuditEventBuilder)
//!   still rejects secret-bearing keys/values before any write.
//! - High-volume paths remain bounded. Daemon emits are best-effort with
//!   a per-write timeout and no unbounded queue; saturation surfaces
//!   typed backpressure through [`crate::audit::AuditWriter`] metrics
//!   plus the process-wide [`emit_counters_snapshot`]. Reads stay
//!   authorized (`audit.read`) at the M003 gate.
//! - Chat remains separate. [`AuditAction::ChatTriggeredAction`](crate::audit::AuditAction)
//!   has a landed builder but no live daemon mapping; collaboration M003
//!   owns structured chat actions. Remote/node sequencing is likewise
//!   out of scope (single-host coordinator sequence).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::audit::{
    sha256_hex, AuditAction, AuditDecisionProvenance, AuditEventBuilder, AuditVisibility,
};
use crate::identity::{AuditEventId, ProjectId};
use crate::transport_auth::AuthenticatedPrincipal;

/// One row of the required Phase-11 event-coverage matrix.
///
/// `owner` names the canonical code owner that must emit the action
/// (never a duplicate wrapper). `actor` names the trusted attribution
/// source. `scope` names the authorization scope kind. `decision`
/// names the representative daemon operation whose M003 decision
/// supplies provenance. `metadata` lists the only structural keys the
/// typed builder for this action may set. `causation` describes how the
/// event links into the request -> session/turn -> run/task ->
/// job/tool/process/Git/worktree chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditCoverageEntry {
    /// Canonical [`AuditAction`](crate::audit::AuditAction) wire name.
    pub action: &'static str,
    /// Canonical code owner, e.g. `daemon:session`.
    pub owner: &'static str,
    /// Trusted actor source, always transport-bound.
    pub actor: &'static str,
    /// Authorization scope kind (`global`, `direct_project`, ...).
    pub scope: &'static str,
    /// Representative daemon operation supplying M003 provenance.
    pub decision: &'static str,
    /// Allowed structural metadata keys for this action.
    pub metadata: &'static [&'static str],
    /// Causation linkage description.
    pub causation: &'static str,
    /// Default visibility for this action.
    pub visibility: AuditVisibility,
    /// `true` when the daemon live-maps this action in this milestone.
    /// Builders still land for unmapped actions so fixtures prove the
    /// chain shape; unmapped rows are explicit gaps, not defects.
    pub live_mapped: bool,
}

/// Required Phase-11 event-coverage matrix (work package A).
///
/// One row per [`AuditAction::ALL`](crate::audit::AuditAction). The daemon
/// operation mapping ([`operation_to_audit_action`]) emits exactly the
/// `live_mapped` rows; the remaining rows (remote/node/chat plus deferred
/// execution hooks) have landed builders and store-level fixtures but no
/// live single-host emission by design (see plan scope).
pub const REQUIRED_AUDIT_COVERAGE: &[AuditCoverageEntry] = &[
    AuditCoverageEntry {
        action: "authentication",
        owner: "daemon:transport_auth",
        actor: "transport-bound principal",
        scope: "global",
        decision: "initialize",
        metadata: &[
            "auth.method",
            "transport.class",
            "principal.kind",
            "decision.outcome",
        ],
        causation: "root of chain; correlation starts here, no parent",
        visibility: AuditVisibility::ActorOnly,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "authorization_decision",
        owner: "daemon:authorization",
        actor: "transport-bound principal",
        scope: "per-operation",
        decision: "any denied operation",
        metadata: &["operation", "capability", "decision.outcome", "decision.reason"],
        causation: "denial terminal; correlation mirrors denied request",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "membership_change",
        owner: "daemon:team_store",
        actor: "transport-bound principal",
        scope: "direct_project",
        decision: "project_configure",
        metadata: &[
            "member.principal",
            "member.role",
            "membership.revision",
            "decision.outcome",
        ],
        causation: "project-scoped; later decisions bind this revision",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "node_enrollment",
        owner: "future:node-protocol",
        actor: "transport-bound principal",
        scope: "global",
        decision: "node_target",
        metadata: &["node.id_digest", "decision.outcome"],
        causation: "out of scope single-host; builder landed, no live emit",
        visibility: AuditVisibility::Administrators,
        live_mapped: false,
    },
    AuditCoverageEntry {
        action: "session_create",
        owner: "daemon:session",
        actor: "transport-bound principal",
        scope: "direct_project",
        decision: "session_create",
        metadata: &["session.id", "project.id", "decision.outcome"],
        causation: "request correlation -> session; parent is auth chain root",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "session_attach",
        owner: "daemon:session",
        actor: "transport-bound principal",
        scope: "via_session",
        decision: "session_attach",
        metadata: &["session.id", "project.id", "decision.outcome"],
        causation: "session locator resolves project; causation is session create where known",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "prompt_submit",
        owner: "daemon:turn",
        actor: "transport-bound principal",
        scope: "via_session",
        decision: "turn_submit",
        metadata: &[
            "session.id",
            "turn.id",
            "prompt.digest",
            "prompt.bytes",
            "decision.outcome",
        ],
        causation: "session -> turn; turn is causation child of session event",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "provider_select",
        owner: "daemon:provider_selection",
        actor: "transport-bound principal",
        scope: "via_session",
        decision: "session_selection_update",
        metadata: &[
            "session.id",
            "provider.connection_id",
            "model.id",
            "decision.outcome",
        ],
        causation: "session -> selection; causation is session or prompt event",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "model_select",
        owner: "daemon:provider_selection",
        actor: "transport-bound principal",
        scope: "via_session",
        decision: "model_select",
        metadata: &["session.id", "model.id", "decision.outcome"],
        causation: "session -> model; causation is session or prompt event",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "agent_delegate",
        owner: "daemon:agent",
        actor: "transport-bound principal",
        scope: "via_session",
        decision: "agent_select",
        metadata: &[
            "session.id",
            "run.id",
            "agent.parent",
            "agent.child",
            "decision.outcome",
        ],
        causation: "parent run -> child run; causation is prompt or parent delegate event",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "permission_decision",
        owner: "daemon:permission",
        actor: "transport-bound principal",
        scope: "global",
        decision: "permission_respond",
        metadata: &[
            "session.id",
            "permission.tool",
            "decision.outcome",
            "decision.reason",
        ],
        causation: "session/run -> decision; causation is prompt or tool event",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "tool_invoke",
        owner: "daemon:tool_broker",
        actor: "transport-bound principal",
        scope: "via_session",
        decision: "run_rerun",
        metadata: &[
            "session.id",
            "run.id",
            "tool.name",
            "tool.backend",
            "decision.outcome",
        ],
        causation: "run -> tool call; causation is delegate or prior tool event",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "command_execute",
        owner: "daemon:tool_broker",
        actor: "transport-bound principal",
        scope: "via_session",
        decision: "run_rerun",
        metadata: &[
            "session.id",
            "run.id",
            "command.digest",
            "command.family",
            "decision.outcome",
        ],
        causation: "run -> command; store-level chain via builders/fixtures in M005, live tool-broker hook deferred",
        visibility: AuditVisibility::Project,
        live_mapped: false,
    },
    AuditCoverageEntry {
        action: "file_mutate",
        owner: "daemon:file",
        actor: "transport-bound principal",
        scope: "via_session",
        decision: "edit_checkpoint_undo",
        metadata: &[
            "session.id",
            "run.id",
            "file.path_digest",
            "file.op",
            "decision.outcome",
        ],
        causation: "run/tool -> file delta; causation is tool or command event",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "git_operation",
        owner: "daemon:git",
        actor: "transport-bound principal",
        scope: "opaque",
        decision: "worktree_list",
        metadata: &[
            "run.id",
            "job.id",
            "worktree.id",
            "git.op",
            "git.ref_digest",
            "decision.outcome",
        ],
        causation: "run/job/worktree -> git; store-level chain via builders/fixtures in M005, live git-executor hook deferred",
        visibility: AuditVisibility::Project,
        live_mapped: false,
    },
    AuditCoverageEntry {
        action: "worktree_lifecycle",
        owner: "daemon:worktree",
        actor: "transport-bound principal",
        scope: "opaque",
        decision: "managed_worktree_cleanup",
        metadata: &["worktree.id", "run.id", "worktree.op", "decision.outcome"],
        causation: "run -> worktree; causation is delegate or prior worktree event",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "job_submit",
        owner: "daemon:scheduler",
        actor: "transport-bound principal",
        scope: "via_session",
        decision: "job_submit",
        metadata: &["session.id", "run.id", "job.id", "decision.outcome"],
        causation: "session/run -> job; causation is prompt or delegate event",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "job_cancel",
        owner: "daemon:scheduler",
        actor: "transport-bound principal",
        scope: "via_job",
        decision: "job_cancel",
        metadata: &["job.id", "session.id", "decision.outcome"],
        causation: "terminal for job; causation is the submit event",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "job_complete",
        owner: "daemon:scheduler",
        actor: "transport-bound principal",
        scope: "via_job",
        decision: "job_retry",
        metadata: &["job.id", "run.id", "job.outcome", "decision.outcome"],
        causation: "terminal for job; retry maps live as representative, full async completion chain via builders/fixtures",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "remote_execute",
        owner: "future:node-protocol",
        actor: "transport-bound principal",
        scope: "opaque",
        decision: "node_target",
        metadata: &["run.id", "remote.target_digest", "decision.outcome"],
        causation: "out of scope single-host; builder landed, no live emit",
        visibility: AuditVisibility::Project,
        live_mapped: false,
    },
    AuditCoverageEntry {
        action: "chat_triggered_action",
        owner: "daemon:collaboration",
        actor: "transport-bound principal",
        scope: "direct_project",
        decision: "chat_action_submit",
        metadata: &[
            "chat.channel",
            "chat.message",
            "chat.action",
            "chat.action_kind",
            "chat.project",
            "chat.status",
            "job.id",
            "session.id",
            "decision.outcome",
        ],
        causation: "message -> auth decision -> action -> job; causation parent links submit to job where present",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "config_change",
        owner: "daemon:config",
        actor: "transport-bound principal",
        scope: "opaque",
        decision: "workspace_config_reload",
        metadata: &["config.key", "config.scope", "decision.outcome"],
        causation: "workspace-scoped; correlation mirrors requesting session where present",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "asset_refresh",
        owner: "daemon:assets",
        actor: "transport-bound principal",
        scope: "direct_project",
        decision: "asset_refresh",
        metadata: &["project.id", "asset.scope", "asset.reason", "decision.outcome"],
        causation: "project-scoped refresh; causation is session lifecycle where present",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "audit_export",
        owner: "daemon:audit",
        actor: "transport-bound principal",
        scope: "direct_project",
        decision: "audit_export",
        metadata: &["project.id", "audit.limit", "audit.count", "decision.outcome"],
        causation: "project-scoped read; self-describing, never recursive",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "audit_query",
        owner: "daemon:audit",
        actor: "transport-bound principal",
        scope: "direct_project",
        decision: "audit_query",
        metadata: &["project.id", "audit.limit", "audit.count", "decision.outcome"],
        causation: "project-scoped read; self-describing, never recursive",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
    AuditCoverageEntry {
        action: "work_order_lifecycle",
        owner: "daemon:work_order",
        actor: "transport-bound principal",
        scope: "direct_project",
        decision: "work_order_create",
        metadata: &[
            "project.id",
            "work_order.id",
            "work_order.revision",
            "work_order.op",
            "work_order.state",
            "decision.id",
            "decision.outcome",
        ],
        causation: "project -> work order; later occurrence/session/job events link by work_order.id",
        visibility: AuditVisibility::Project,
        live_mapped: true,
    },
];

/// Look up the coverage row for one [`AuditAction`](crate::audit::AuditAction).
pub fn coverage_for_action(action: &AuditAction) -> Option<&'static AuditCoverageEntry> {
    let name = action.clone().as_str();
    REQUIRED_AUDIT_COVERAGE
        .iter()
        .find(|entry| entry.action == name)
}

/// Daemon operations with a live structural mapping in this milestone.
///
/// Each tuple is `(operation, audit_action)`. Operations absent from
/// both this list and [`UNINSTRUMENTED_OPERATIONS`] fail the coverage
/// guard when they carry a mutating capability.
pub const INSTRUMENTED_OPERATIONS: &[(&str, &str)] = &[
    ("initialize", "authentication"),
    ("session_create", "session_create"),
    ("session_create_from_template", "session_create"),
    ("session_attach", "session_attach"),
    ("session_delete", "session_attach"),
    ("session_archive", "session_attach"),
    ("session_restore", "session_attach"),
    ("session_rename", "session_attach"),
    ("session_import_data", "session_create"),
    ("session_fork", "session_create"),
    ("turn_submit", "prompt_submit"),
    ("turn_steer", "prompt_submit"),
    ("turn_cancel", "job_cancel"),
    ("agent_select", "agent_delegate"),
    ("model_select", "model_select"),
    ("session_selection_update", "provider_select"),
    ("tool_program_notification_reinject", "agent_delegate"),
    ("tool_program_recovery_debug_inspect", "agent_delegate"),
    ("permission_respond", "permission_decision"),
    ("run_rerun", "tool_invoke"),
    ("goal_set", "agent_delegate"),
    ("goal_from_file", "agent_delegate"),
    ("goal_pause", "agent_delegate"),
    ("goal_resume", "agent_delegate"),
    ("goal_clear", "agent_delegate"),
    ("goal_done", "agent_delegate"),
    ("goal_checkpoint", "agent_delegate"),
    ("goal_set_budget", "agent_delegate"),
    ("edit_checkpoint_undo", "file_mutate"),
    ("edit_checkpoint_undo_latest", "file_mutate"),
    ("edit_checkpoint_reapply", "file_mutate"),
    ("edit_checkpoint_reapply_latest", "file_mutate"),
    ("lsp_preview_apply", "file_mutate"),
    ("managed_worktree_cleanup", "worktree_lifecycle"),
    ("managed_worktree_archive", "worktree_lifecycle"),
    ("job_submit", "job_submit"),
    ("job_cancel", "job_cancel"),
    ("job_retry", "job_complete"),
    ("job_recovery_report", "audit_query"),
    ("schedule_create", "job_submit"),
    ("schedule_pause", "job_cancel"),
    ("schedule_resume", "job_submit"),
    ("schedule_delete", "job_cancel"),
    ("workspace_archive", "config_change"),
    ("workspace_config_reload", "config_change"),
    ("asset_refresh", "asset_refresh"),
    ("audit_export", "audit_export"),
    ("audit_query", "audit_query"),
    ("session_share", "membership_change"),
    ("session_unshare", "membership_change"),
    ("project_archive", "config_change"),
    ("project_restore", "config_change"),
    ("eggpool_connection_create", "provider_select"),
    ("provider_connection_create", "provider_select"),
    ("connection_rotate_begin", "provider_select"),
    ("connection_rotate_secret_stage", "provider_select"),
    ("connection_rotate_cancel", "provider_select"),
    ("connection_refresh_begin", "provider_select"),
    ("connection_enable", "provider_select"),
    ("connection_disable", "provider_select"),
    ("connection_delete", "provider_select"),
    ("connection_restore", "provider_select"),
    ("connection_purge", "provider_select"),
    ("provider_connection_use", "provider_select"),
    ("chat_action_submit", "chat_triggered_action"),
    // Team collaboration post-closure M003: chat policy administration
    // is membership/collaboration administration (`member.manage`), not
    // ordinary message content. Reads stay uninstrumented below; the two
    // policy-set mutations emit `membership_change` through the generic
    // pre-side-effect seam with structural locators only.
    ("chat_project_policy_set", "membership_change"),
    ("chat_channel_policy_set", "membership_change"),
    // Team collaboration corrective M003: membership/principal lifecycle
    // emits `membership_change` with member ids/roles/revisions only;
    // device-token mint/revoke emits `authentication` with method/
    // transport/kind only. Token digests and plaintext never enter
    // audit metadata (rejected by the builder's secret-key guard).
    ("team_membership_add", "membership_change"),
    ("team_membership_update", "membership_change"),
    ("team_membership_revoke", "membership_change"),
    ("team_principal_create", "membership_change"),
    ("team_principal_status_set", "membership_change"),
    ("team_token_create", "authentication"),
    ("team_token_revoke", "authentication"),
    // Team collaboration corrective M004: control transitions emit
    // `membership_change` with session/turn/controller ids and revision
    // only (post-mutation in the handler, like team mutations). The
    // inert request path audits the same way; the get path is a
    // bounded read and stays uninstrumented.
    ("session_control_request", "membership_change"),
    ("session_control_transfer", "membership_change"),
    ("session_control_release", "membership_change"),
    ("session_control_takeover", "membership_change"),
    ("work_order_create", "work_order_lifecycle"),
    ("work_order_batch_create", "work_order_lifecycle"),
    ("work_order_update", "work_order_lifecycle"),
    ("work_order_cancel", "work_order_lifecycle"),
    ("work_order_pause", "work_order_lifecycle"),
    ("work_order_resume", "work_order_lifecycle"),
    ("work_order_lane_create", "work_order_lifecycle"),
    ("work_order_lane_reorder", "work_order_lifecycle"),
    ("work_order_lane_attach", "work_order_lifecycle"),
    // Work Orders M005: trigger mint/revoke are already audited
    // post-mutation via `after_trigger_mutation` as `work_order_lifecycle`
    // with trigger/work-order ids and state only (secret never enters
    // audit). The table entry makes the guard truthful; the generic
    // pre-side-effect emit is skipped via `is_work_order_mutation`.
    ("work_order_trigger_create", "work_order_lifecycle"),
    ("work_order_trigger_revoke", "work_order_lifecycle"),
];

/// Explicitly uninstrumented operations.
///
/// Every entry is one of:
///
/// - read-only, global-infrastructure, or privacy-filtered listing whose
///   per-request volume would violate the bounded-emission invariant;
/// - caller-scoped principal preference/infrastructure with no
///   project-keyed data (analogous to `memory_*`);
/// - chat-domain message/channel/composing/read/sync content: only
///   structured chat actions emit `chat_triggered_action` (see
///   `architecture/audit.md` "Live in M003"); ordinary chat content
///   carries no audit action by design, like `memory_*`;
/// - presence heartbeat/snapshot: high-volume own-contribution plus
///   bounded `project.observe` reads;
/// - interactive-process execution surface (`Global`, per-process
///   attachment registry): deferred like the explicit
///   `command_execute`/`git_operation` gaps in `architecture/audit.md`
///   (builders/fixtures landed, no live single-host emission).
///
/// The coverage guard pins this list so a new privileged operation
/// cannot hide here. Do not add a shared-state mutation (membership,
/// credential, bearer-mint, config) to this list merely to pass the
/// guard; instrument it instead.
pub const UNINSTRUMENTED_OPERATIONS: &[&str] = &[
    "asset_refresh_capabilities",
    "asset_refresh_status",
    "audit_capabilities",
    "connection_list_detail",
    "eggpool_connection_cancel",
    "eggpool_connection_status",
    "job_attempts",
    "job_get",
    "job_list",
    "job_wait",
    "managed_worktree_get",
    "managed_worktree_list",
    "memory_forget",
    "memory_list",
    "memory_remember",
    "memory_search",
    "models_refresh",
    "notification_speak",
    "notification_stop",
    "project_catalog_capabilities",
    "project_get",
    "project_health",
    "project_list",
    "project_register",
    "projection_ack",
    "projection_artifact_list",
    "projection_artifact_read",
    "projection_capabilities",
    "projection_resume",
    "projection_snapshot_get",
    "projection_subscribe",
    "projection_unsubscribe",
    "provider_connection_list",
    "provider_connection_models",
    "connection_get",
    "connection_refresh_cancel",
    "connection_refresh_status",
    "connection_rotate_status",
    "question_respond",
    "resume",
    "run_artifact_read",
    "run_get",
    "run_list",
    "scheduler_snapshot",
    "schedule_get",
    "schedule_list",
    "session_export",
    "session_lifecycle_get",
    "session_list",
    "session_load",
    "session_message_counts",
    "session_messages_load",
    "session_selection_get",
    "session_selection_list",
    "session_selection_models",
    "snapshot_daemon",
    "snapshot_models",
    "snapshot_session",
    "snapshot_workspace",
    "subscribe",
    "task_delete",
    "task_list",
    "task_schedule",
    "tool_program_call_page",
    "tool_program_inspect",
    "tool_program_list",
    "edit_checkpoint_get",
    "edit_checkpoint_list",
    "goal_show",
    "active_goal_load",
    "todo_list",
    "workspace_list",
    "workspace_register",
    "workspace_services_snapshot",
    "workspace_snapshot_request",
    "worktree_list",
    "chat_action_get",
    "chat_action_list",
    // Team collaboration corrective M003: read-only team listings are
    // explicitly uninstrumented (bounded, privacy-filtered reads; the
    // mutations above carry the audit trail). `team_capabilities` is a
    // global version probe.
    "team_capabilities",
    "team_membership_list",
    "team_principal_list",
    "team_token_list",
    // Team collaboration corrective M004: the control get path is a
    // bounded session-scoped read (lease plus inert requests); the
    // mutations above carry the audit trail.
    "session_control_get",
    "work_order_capabilities",
    "work_order_list",
    "work_order_get",
    "work_order_lane_get",
    "work_order_lane_list",
    "work_order_occurrence_get",
    "work_order_occurrence_list",
    "work_order_summary",
    // Work Orders M005: trigger metadata reads are bounded listings
    // like `work_order_list`; mint/revoke above carry the audit trail.
    "work_order_trigger_list",
    "work_order_trigger_get",
    // Execution Reliability M003: principal-scoped daemon-owned
    // preferences. Transport scope is `Global` with no semantic
    // capability; the daemon binds the principal server-side and the
    // payload carries no project-keyed data. Reads and own-preference
    // writes are caller-scoped infrastructure like `memory_*`, not
    // shared-state mutations.
    "approval_preference_get",
    "approval_mode_set",
    "sandbox_profile_set",
    "runtime_policy_set",
    "task_model_preference_set",
    "execution_policy_get",
    // Provider setup catalog: secret-free daemon-owned projection for
    // selection surfaces (no secrets, no authorization grant), like
    // `provider_connection_list`.
    "provider_setup_list",
    // Project Work Orders M004: enumeration-style dashboard listing;
    // the daemon returns only `project.read` rows, like `project_list`.
    "workspace_dashboard",
    // Presence: `presence_capabilities` is a global version probe;
    // `presence_heartbeat` is a high-volume own-contribution write;
    // `presence_snapshot_get` is a bounded `project.observe` read.
    "presence_capabilities",
    "presence_heartbeat",
    "presence_snapshot_get",
    // Project Collaboration chat domain: ordinary message/channel/
    // composing/read/sync content plus policy reads. Only structured
    // chat actions emit `chat_triggered_action` (see
    // `architecture/audit.md` "Live in M003"); ordinary content has no
    // audit action by design. Policy-set mutations above carry the
    // administration trail as `membership_change`.
    "chat_capabilities",
    "chat_channel_ensure",
    "chat_channel_list",
    "chat_history",
    "chat_send",
    "chat_edit",
    "chat_redact",
    "chat_read_set",
    "chat_read_get",
    "chat_composing_set",
    "chat_composing_list",
    "chat_sync",
    "chat_policy_get",
    "chat_policy_list",
    // Interactive Process Sessions: `Global` execution surface with
    // per-process attachment-registry authority (trusted `client_id`,
    // never payload fields). Deferred like the explicit
    // `command_execute`/`git_operation` gaps: builders/fixtures prove
    // the chain shape, but no live single-host emission by design.
    "interactive_process_capabilities",
    "interactive_process_create",
    "interactive_process_list",
    "interactive_process_attach",
    "interactive_process_detach",
    "interactive_process_input",
    "interactive_process_resize",
    "interactive_process_resume",
    "interactive_process_terminate",
    "interactive_process_remove",
];

/// Map one daemon operation name to its structural audit action.
///
/// Returns `None` for explicitly uninstrumented reads/infrastructure.
/// Unknown operations return `None` so callers emit nothing rather
/// than fabricating attribution; the coverage guard (not this
/// function) is responsible for rejecting unclassified privileged
/// operations.
pub fn operation_to_audit_action(operation: &str) -> Option<AuditAction> {
    let action = INSTRUMENTED_OPERATIONS
        .iter()
        .find(|(op, _)| *op == operation)
        .map(|(_, action)| *action)?;
    AuditAction::parse_known(action).ok()
}

/// Causal chain locators for one structural event.
///
/// All fields are bounded structural locators (never bodies). The
/// daemon derives these from request DTO locators; execution owners
/// derive them from stored run/job/session rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditChainContext {
    /// Override for the decision-derived correlation id, if any.
    pub correlation_id: Option<String>,
    /// Parent event id for causation (`None` for chain roots).
    pub causation_parent: Option<String>,
    /// Deterministic idempotency key for retries, if any.
    pub event_id: Option<AuditEventId>,
    pub project: Option<ProjectId>,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub run_id: Option<String>,
    pub job_id: Option<String>,
    pub worktree_id: Option<String>,
    pub provider_connection_id: Option<String>,
}

impl AuditChainContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Derive a child context from a stored parent event.
    ///
    /// The child preserves the parent correlation id and points its
    /// causation parent at the parent event id, so
    /// prompt -> run -> tool/job -> Git/worktree chains stay
    /// reconstructable with one ordered query.
    pub fn child_of(
        parent_correlation: &str,
        parent_event_id: &crate::identity::AuditEventId,
    ) -> Self {
        Self {
            correlation_id: Some(parent_correlation.to_owned()),
            causation_parent: Some(parent_event_id.as_str().to_owned()),
            event_id: None,
            project: None,
            session_id: None,
            turn_id: None,
            run_id: None,
            job_id: None,
            worktree_id: None,
            provider_connection_id: None,
        }
    }
}

/// Deterministic idempotency key for retried state transitions.
///
/// Replayed submissions reuse the same event id so the store returns
/// the stored event instead of assigning a second sequence number.
/// The preimage carries only structural linkage (never bodies or
/// secrets); the digest is hex so it satisfies the identity lexical
/// contract.
pub fn deterministic_event_id(
    decision_id: &str,
    action: &AuditAction,
    correlation_id: &str,
    scope_id: &str,
) -> AuditEventId {
    let preimage = format!(
        "{decision_id}|{}|{correlation_id}|{scope_id}",
        action.clone().as_str()
    );
    let digest = sha256_hex(preimage.as_bytes());
    let short = digest.get(..32).unwrap_or(&digest);
    AuditEventId::parse(short).expect("hex digest satisfies identity contract")
}

/// Apply one causal chain to an audit builder: correlation, causation
/// parent, event id, project (falling back to the provenance project),
/// and session/turn/run/job/worktree/provider locators.
///
/// Daemon seams use this when they emit an action whose typed builder
/// does not cover their locator shape (e.g. lane-scoped work-order
/// events under the shared `work_order_lifecycle` action).
pub fn apply_chain(
    mut builder: AuditEventBuilder,
    chain: &AuditChainContext,
    provenance_project: Option<&ProjectId>,
) -> AuditEventBuilder {
    if let Some(correlation) = chain.correlation_id.as_deref() {
        if !correlation.is_empty() {
            builder = builder.with_correlation(correlation.to_owned());
        }
    }
    if let Some(parent) = chain.causation_parent.as_deref() {
        if !parent.is_empty() {
            builder = builder.with_causation_parent(parent.to_owned());
        }
    }
    if let Some(event_id) = chain.event_id.clone() {
        builder = builder.with_event_id(event_id);
    }
    if let Some(project) = chain
        .project
        .clone()
        .or_else(|| provenance_project.cloned())
    {
        builder = builder.with_project(project);
    }
    if let Some(session) = chain.session_id.as_deref() {
        builder = builder.with_session(session.to_owned());
    }
    if let Some(turn) = chain.turn_id.as_deref() {
        builder = builder.with_turn(turn.to_owned());
    }
    if let Some(run) = chain.run_id.as_deref() {
        builder = builder.with_run(run.to_owned());
    }
    if let Some(job) = chain.job_id.as_deref() {
        builder = builder.with_job(job.to_owned());
    }
    if let Some(worktree) = chain.worktree_id.as_deref() {
        builder = builder.with_worktree(worktree.to_owned());
    }
    if let Some(connection) = chain.provider_connection_id.as_deref() {
        builder = builder.with_provider_connection(connection.to_owned());
    }
    builder
}

fn truncate_label(value: &str, max: usize) -> String {
    let mut out = value.to_owned();
    if out.len() > max {
        out.truncate(max);
    }
    out
}

/// Structural authentication event (no token/secret material).
pub fn authentication_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::Authentication, principal, provenance)
        .with_visibility(AuditVisibility::ActorOnly)
        .with_metadata("auth.method", principal.auth_method().as_str().to_owned())
        .with_metadata(
            "transport.class",
            principal.transport_class().as_str().to_owned(),
        )
        .with_metadata("principal.kind", principal.kind().as_str().to_owned())
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Authorization denial terminal event.
pub fn authorization_denied_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    operation: &str,
    capability: &str,
    reason: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::AuthorizationDecision, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("operation", truncate_label(operation, 128))
        .with_metadata("capability", truncate_label(capability, 128))
        .with_metadata("decision.outcome", "denied".to_owned())
        .with_metadata("decision.reason", truncate_label(reason, 128));
    apply_chain(builder, chain, provenance.project())
}

/// Membership/role change event. `member_principal` is the affected
/// principal id; no secret material is accepted.
pub fn membership_change_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    member_principal: &str,
    member_role: &str,
    membership_revision: Option<u64>,
) -> AuditEventBuilder {
    let mut builder = AuditEventBuilder::new(AuditAction::MembershipChange, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("member.principal", truncate_label(member_principal, 128))
        .with_metadata("member.role", truncate_label(member_role, 64))
        .with_metadata("decision.outcome", "allow".to_owned());
    if let Some(revision) = membership_revision {
        builder = builder.with_metadata("membership.revision", revision.to_string());
    }
    apply_chain(builder, chain, provenance.project())
}

/// Session creation event.
pub fn session_create_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    session_id: &str,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::SessionCreate, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("session.id", truncate_label(session_id, 128))
        .with_metadata(
            "project.id",
            chain
                .project
                .as_ref()
                .map(|id| id.as_str().to_owned())
                .or_else(|| provenance.project().map(|id| id.as_str().to_owned()))
                .unwrap_or_default(),
        )
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Session attach event.
pub fn session_attach_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    session_id: &str,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::SessionAttach, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("session.id", truncate_label(session_id, 128))
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Prompt submission structural event. The prompt body is never
/// stored; only its SHA-256 digest and byte length are recorded.
pub fn prompt_submit_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    session_id: &str,
    turn_id: &str,
    prompt_digest_hex: &str,
    prompt_bytes: usize,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::PromptSubmit, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("session.id", truncate_label(session_id, 128))
        .with_metadata("turn.id", truncate_label(turn_id, 128))
        .with_metadata("prompt.digest", truncate_label(prompt_digest_hex, 128))
        .with_metadata("prompt.bytes", prompt_bytes.to_string())
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Provider/model selection event. Connection ids and model labels
/// only; credentials are never accepted.
pub fn provider_select_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    session_id: &str,
    connection_id: &str,
    model_id: &str,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::ProviderSelect, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("session.id", truncate_label(session_id, 128))
        .with_metadata("provider.connection_id", truncate_label(connection_id, 128))
        .with_metadata("model.id", truncate_label(model_id, 128))
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Model selection event.
pub fn model_select_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    session_id: &str,
    model_id: &str,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::ModelSelect, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("session.id", truncate_label(session_id, 128))
        .with_metadata("model.id", truncate_label(model_id, 128))
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Agent delegation event (parent run -> child run).
pub fn agent_delegate_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    parent_run: &str,
    child_run: &str,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::AgentDelegate, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("agent.parent", truncate_label(parent_run, 128))
        .with_metadata("agent.child", truncate_label(child_run, 128))
        .with_metadata("run.id", truncate_label(child_run, 128))
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Permission allow/deny event.
pub fn permission_decision_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    tool: &str,
    decision_outcome: &str,
    reason: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::PermissionDecision, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("permission.tool", truncate_label(tool, 128))
        .with_metadata("decision.outcome", truncate_label(decision_outcome, 64))
        .with_metadata("decision.reason", truncate_label(reason, 128));
    apply_chain(builder, chain, provenance.project())
}

/// Tool invocation structural event.
pub fn tool_invoke_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    tool_name: &str,
    backend: &str,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::ToolInvoke, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("tool.name", truncate_label(tool_name, 128))
        .with_metadata("tool.backend", truncate_label(backend, 64))
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Command execution structural event. The argv is never stored;
/// only its digest plus the intent family are recorded.
pub fn command_execute_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    command_digest_hex: &str,
    family: &str,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::CommandExecute, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("command.digest", truncate_label(command_digest_hex, 128))
        .with_metadata("command.family", truncate_label(family, 64))
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// File mutation structural event. Paths are digested; content is
/// never accepted.
pub fn file_mutate_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    path_digest_hex: &str,
    op: &str,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::FileMutate, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("file.path_digest", truncate_label(path_digest_hex, 128))
        .with_metadata("file.op", truncate_label(op, 64))
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Git operation structural event.
pub fn git_operation_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    git_op: &str,
    ref_digest_hex: &str,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::GitOperation, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("git.op", truncate_label(git_op, 64))
        .with_metadata("git.ref_digest", truncate_label(ref_digest_hex, 128))
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Worktree lifecycle structural event.
pub fn worktree_lifecycle_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    worktree_id: &str,
    op: &str,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::WorktreeLifecycle, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("worktree.id", truncate_label(worktree_id, 128))
        .with_metadata("worktree.op", truncate_label(op, 64))
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Job submission structural event.
pub fn job_submit_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    job_id: &str,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::JobSubmit, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("job.id", truncate_label(job_id, 128))
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Job cancellation terminal event.
pub fn job_cancel_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    job_id: &str,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::JobCancel, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("job.id", truncate_label(job_id, 128))
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Job completion terminal event (success/failure/interrupted).
pub fn job_complete_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    job_id: &str,
    job_outcome: &str,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::JobComplete, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("job.id", truncate_label(job_id, 128))
        .with_metadata("job.outcome", truncate_label(job_outcome, 64))
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Structured chat-action structural event (M003).
///
/// Links message -> auth decision -> action -> job with structural
/// locators only. Titles/prompts never enter metadata; job payloads
/// stay in the canonical job store. `job_id` is `None` only for
/// reference actions that failed open (never emitted); submit actions
/// always carry the canonical job id.
#[allow(clippy::too_many_arguments)]
pub fn chat_triggered_action_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    channel_id: &str,
    message_id: &str,
    action_id: &str,
    action_kind: &str,
    job_id: Option<&str>,
    outcome: &str,
) -> AuditEventBuilder {
    let mut builder =
        AuditEventBuilder::new(AuditAction::ChatTriggeredAction, principal, provenance)
            .with_visibility(AuditVisibility::Project)
            .with_metadata("chat.channel", truncate_label(channel_id, 128))
            .with_metadata("chat.message", truncate_label(message_id, 128))
            .with_metadata("chat.action", truncate_label(action_id, 128))
            .with_metadata("chat.action_kind", truncate_label(action_kind, 64))
            .with_metadata("decision.outcome", truncate_label(outcome, 64));
    if let Some(job) = job_id {
        builder = builder.with_metadata("job.id", truncate_label(job, 128));
    }
    apply_chain(builder, chain, provenance.project())
}

/// Work-order lifecycle structural event. Identity, revision, operation,
/// and state only: prompt bodies, secrets, and reasoning are never
/// recorded. Creation operations that mint their identity in the handler
/// emit post-creation with their durable ids (see the daemon seam).
#[allow(clippy::too_many_arguments)]
pub fn work_order_lifecycle_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    work_order_id: &str,
    revision: u64,
    operation: &str,
    state: &str,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::WorkOrderLifecycle, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("work_order.id", truncate_label(work_order_id, 128))
        .with_metadata("work_order.revision", revision.to_string())
        .with_metadata("work_order.op", truncate_label(operation, 64))
        .with_metadata("work_order.state", truncate_label(state, 64))
        .with_metadata("decision.id", truncate_label(provenance.decision_id(), 128))
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Configuration change structural event. Keys only; values are never
/// accepted.
pub fn config_change_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    config_key: &str,
    scope: &str,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::ConfigChange, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("config.key", truncate_label(config_key, 128))
        .with_metadata("config.scope", truncate_label(scope, 64))
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Asset refresh structural event.
pub fn asset_refresh_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    scope: &str,
    reason: &str,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::AssetRefresh, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("asset.scope", truncate_label(scope, 64))
        .with_metadata("asset.reason", truncate_label(reason, 64))
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Audit export self-describing event.
pub fn audit_export_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    limit: u32,
    count: usize,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::AuditExport, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("audit.limit", limit.to_string())
        .with_metadata("audit.count", count.to_string())
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// Audit query self-describing event.
pub fn audit_query_event(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    limit: u32,
    count: usize,
    outcome: &str,
) -> AuditEventBuilder {
    let builder = AuditEventBuilder::new(AuditAction::AuditQuery, principal, provenance)
        .with_visibility(AuditVisibility::Project)
        .with_metadata("audit.limit", limit.to_string())
        .with_metadata("audit.count", count.to_string())
        .with_metadata("decision.outcome", truncate_label(outcome, 64));
    apply_chain(builder, chain, provenance.project())
}

/// SHA-256 hex digest helper for structural handles (prompts, paths,
/// argv, refs). Bodies themselves are never stored.
pub fn structural_digest(bytes: &[u8]) -> String {
    sha256_hex(bytes)
}

// --- Process-wide best-effort emission diagnostics ---------------------

static EMIT_APPENDED: AtomicU64 = AtomicU64::new(0);
static EMIT_FAILED: AtomicU64 = AtomicU64::new(0);
static EMIT_DROPPED_NO_POOL: AtomicU64 = AtomicU64::new(0);

/// Record one successful best-effort daemon emission.
pub fn record_emit_appended() {
    EMIT_APPENDED.fetch_add(1, Ordering::Relaxed);
}

/// Record one failed best-effort daemon emission (bounded failure,
/// never silent success).
pub fn record_emit_failed() {
    EMIT_FAILED.fetch_add(1, Ordering::Relaxed);
}

/// Record one skipped emission when no durable pool is available
/// (legacy in-memory daemons).
pub fn record_emit_dropped_no_pool() {
    EMIT_DROPPED_NO_POOL.fetch_add(1, Ordering::Relaxed);
}

/// Process-wide best-effort emission counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditEmitCounters {
    /// Successful daemon audit appends.
    pub appended: u64,
    /// Failed daemon audit appends (bounded, warned).
    pub failed: u64,
    /// Skipped emissions without a durable pool.
    pub dropped_no_pool: u64,
}

/// Snapshot the process-wide emission counters.
pub fn emit_counters_snapshot() -> AuditEmitCounters {
    AuditEmitCounters {
        appended: EMIT_APPENDED.load(Ordering::Relaxed),
        failed: EMIT_FAILED.load(Ordering::Relaxed),
        dropped_no_pool: EMIT_DROPPED_NO_POOL.load(Ordering::Relaxed),
    }
}

// --- Trusted execution audit seam (M001) ---------------------------------

/// Bounded per-write timeout shared by the daemon seam and every injected
/// execution owner. One policy, no separate queue/store/schema.
pub const EXECUTION_AUDIT_EMIT_TIMEOUT: Duration = Duration::from_millis(500);

/// Trusted execution-audit context for canonical execution owners.
///
/// Carries only what typed builders need: the cloned immutable
/// transport-bound [`AuthenticatedPrincipal`], the gate-copied
/// [`AuditDecisionProvenance`], and the [`AuditChainContext`] locators.
/// It carries no command text, argv, terminal input, Git URLs, file
/// bodies, or tool output.
///
/// Construction is daemon-owned: the admission/turn/job boundary copies
/// the already-admitted principal/decision/chain into this value and
/// threads it through [`BrokerInvocationContext`](`crate::jobs`)/
/// tool-execution and scheduler/Git composition. Execution layers must
/// never synthesize it from `principal_ref`, tool input, model output,
/// or a grant string. Legacy/local callers that genuinely lack team
/// attribution must use [`Self::legacy_local`], which records the
/// explicit `legacy-local`/`LocalOwner` provenance rather than an
/// invented human identity.
///
/// The struct deliberately has no `serde` impls: request DTOs, tool
/// inputs, and model outputs are JSON and therefore cannot supply this
/// value over the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedExecutionAuditContext {
    principal: AuthenticatedPrincipal,
    provenance: AuditDecisionProvenance,
    chain: AuditChainContext,
}

impl TrustedExecutionAuditContext {
    /// Copy trusted daemon-owned state into an immutable context.
    ///
    /// `principal` is the transport-bound principal; `provenance` is the
    /// gate-copied decision linkage; `chain` carries the session/turn/
    /// run/job correlation for this execution.
    pub fn new(
        principal: &AuthenticatedPrincipal,
        provenance: &AuditDecisionProvenance,
        chain: AuditChainContext,
    ) -> Self {
        Self {
            principal: principal.clone(),
            provenance: provenance.clone(),
            chain,
        }
    }

    /// Explicit legacy provenance for pre-attribution local execution.
    ///
    /// Uses the literal `"legacy-local"` decision marker with a
    /// `LocalOwner` principal and `local_owner_broad` policy, mirroring
    /// `OriginAttribution::legacy_local`. Never fabricates a team
    /// identity.
    pub fn legacy_local(correlation_id: impl Into<String>) -> Self {
        let correlation = correlation_id.into();
        let principal = AuthenticatedPrincipal::local_owner("legacy-local");
        let provenance = AuditDecisionProvenance::new(
            "legacy-local",
            correlation.clone(),
            "local_owner_broad",
            None,
        );
        let chain = AuditChainContext {
            correlation_id: Some(correlation),
            ..AuditChainContext::default()
        };
        Self {
            principal,
            provenance,
            chain,
        }
    }

    /// Transport-bound actor for typed builders. Cloned, never rewritten.
    pub fn principal(&self) -> &AuthenticatedPrincipal {
        &self.principal
    }

    /// Gate-copied decision linkage for typed builders.
    pub fn provenance(&self) -> &AuditDecisionProvenance {
        &self.provenance
    }

    /// Correlation/causation locators for typed builders.
    pub fn chain(&self) -> &AuditChainContext {
        &self.chain
    }

    /// Start one structural builder bound to this trusted context.
    ///
    /// Applies the stored chain (correlation, causation parent, event
    /// id, project/session/turn/run/job/worktree/provider locators) so
    /// execution owners cannot drop correlation silently.
    pub fn builder(&self, action: AuditAction) -> AuditEventBuilder {
        let base = AuditEventBuilder::new(action, &self.principal, &self.provenance);
        apply_chain(base, &self.chain, self.provenance.project())
    }

    /// Derive a child context that preserves correlation and points
    /// causation at `parent_event_id`, keeping the same actor/provenance.
    pub fn child_with_parent(&self, parent_event_id: &AuditEventId) -> Self {
        let mut chain = self.chain.clone();
        if chain.correlation_id.as_ref().is_none_or(|c| c.is_empty()) {
            chain.correlation_id = Some(self.provenance.correlation_id().to_owned());
        }
        chain.causation_parent = Some(parent_event_id.as_str().to_owned());
        Self {
            principal: self.principal.clone(),
            provenance: self.provenance.clone(),
            chain,
        }
    }
}

/// One cloneable bounded audit-emission service.
///
/// Constructed by the daemon/coordinator from the same pool/store policy
/// as `CoreDaemon::append_audit_event`. It owns the bounded append
/// timeout, the existing emit counters/warn behavior, no separate
/// queue/store/schema, and no authorization decisions of its own.
/// Execution owners (ToolBroker, scheduler, Git) receive a clone via
/// injection and emit only through this seam.
#[derive(Debug, Clone)]
pub struct ExecutionAuditEmitter {
    pool: Option<sqlx::SqlitePool>,
    timeout: Duration,
}

impl ExecutionAuditEmitter {
    /// Build an emitter over `pool`. `None` preserves the legacy
    /// in-memory-daemon behavior: emits are dropped with a counter.
    pub fn new(pool: Option<sqlx::SqlitePool>) -> Self {
        Self {
            pool,
            timeout: EXECUTION_AUDIT_EMIT_TIMEOUT,
        }
    }

    /// Override the bounded per-write timeout (tests only).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Bounded timeout for one append.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Whether a durable pool backs this emitter.
    pub fn has_pool(&self) -> bool {
        self.pool.is_some()
    }

    /// Best-effort append of one already-built structural event.
    ///
    /// Same policy as the daemon seam: one bounded timeout, counters
    /// plus warn on failure, never fails the owning operation.
    pub async fn emit(&self, builder: AuditEventBuilder) {
        let Some(pool) = self.pool.clone() else {
            record_emit_dropped_no_pool();
            return;
        };
        let store = crate::audit::AuditStore::new(pool);
        match tokio::time::timeout(self.timeout, store.append(builder)).await {
            Ok(Ok(_)) => record_emit_appended(),
            Ok(Err(error)) => {
                record_emit_failed();
                tracing::warn!(error = %error, "audit event append failed");
            }
            Err(_) => {
                record_emit_failed();
                tracing::warn!("audit event append timed out");
            }
        }
    }

    /// Build via `build` from a trusted context and emit with one policy.
    ///
    /// `build` receives the context's principal/provenance/chain so the
    /// typed instrumentation builders bind attribution losslessly.
    pub async fn emit_with(
        &self,
        ctx: &TrustedExecutionAuditContext,
        build: impl FnOnce(
            &AuthenticatedPrincipal,
            &AuditDecisionProvenance,
            &AuditChainContext,
        ) -> AuditEventBuilder,
    ) {
        let builder = build(ctx.principal(), ctx.provenance(), ctx.chain());
        self.emit(builder).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::ProjectId;

    fn local_context(correlation: &str) -> (AuthenticatedPrincipal, AuditDecisionProvenance) {
        let principal = AuthenticatedPrincipal::local_owner("client-test");
        let provenance =
            AuditDecisionProvenance::new("decision-test", correlation, "local_owner_broad", None);
        (principal, provenance)
    }

    #[test]
    fn coverage_matrix_covers_every_known_action() {
        for known in AuditAction::ALL {
            let action = AuditAction::parse_known(known).expect("known action");
            let entry = coverage_for_action(&action);
            assert!(entry.is_some(), "missing coverage for {known}");
            let entry = entry.expect("checked");
            assert_eq!(entry.action, known);
            assert!(!entry.owner.is_empty());
            assert!(!entry.actor.is_empty());
            assert!(!entry.scope.is_empty());
            assert!(!entry.decision.is_empty());
            assert!(!entry.metadata.is_empty());
            assert!(!entry.causation.is_empty());
        }
        assert_eq!(REQUIRED_AUDIT_COVERAGE.len(), AuditAction::ALL.len());
    }

    #[test]
    fn operation_mapping_covers_representative_privileged_operations() {
        for operation in [
            "session_create",
            "turn_submit",
            "agent_select",
            "model_select",
            "session_selection_update",
            "permission_respond",
            "run_rerun",
            "edit_checkpoint_undo",
            "lsp_preview_apply",
            "managed_worktree_cleanup",
            "job_submit",
            "job_cancel",
            "job_retry",
            "workspace_config_reload",
            "asset_refresh",
            "audit_export",
            "audit_query",
            // Team-collaboration post-closure M003: canonical
            // descriptor drift must not silently drop audit mappings.
            "provider_connection_create",
            "work_order_trigger_create",
            "work_order_trigger_revoke",
            "chat_project_policy_set",
            "chat_channel_policy_set",
        ] {
            assert!(
                operation_to_audit_action(operation).is_some(),
                "privileged operation unmapped: {operation}"
            );
        }
        assert!(operation_to_audit_action("session_load").is_none());
        assert!(operation_to_audit_action("unknown_future_op").is_none());
    }

    #[test]
    fn operation_matrix_covers_canonical_policy_descriptor_set() {
        // Regression for the M003 stale-guard failure: the descriptor
        // table lives in `authorization/policy.rs`, and the audit tables
        // must classify every canonical operation. The Python guard pins
        // the same invariant statically; this test pins it at the type
        // level so a future move fails here instead of silently passing.
        let matrix = crate::authorization::operation_capability_matrix();
        assert!(
            matrix.len() >= 130,
            "canonical operation set too small: {}",
            matrix.len()
        );
        let ops: std::collections::HashSet<&str> =
            matrix.iter().map(|(op, _, _)| op.as_str()).collect();
        for required in [
            "workspace_register",
            "workspace_list",
            "project_register",
            "provider_connection_create",
            "work_order_trigger_create",
            "work_order_trigger_revoke",
            "chat_project_policy_set",
            "workspace_dashboard",
        ] {
            assert!(ops.contains(required), "canonical set missing {required}");
        }
        let instrumented: std::collections::HashSet<&str> =
            INSTRUMENTED_OPERATIONS.iter().map(|(op, _)| *op).collect();
        let uninstrumented: std::collections::HashSet<&str> =
            UNINSTRUMENTED_OPERATIONS.iter().copied().collect();
        for (op, _, _) in &matrix {
            if *op == "provider_connection_use" {
                continue;
            }
            assert!(
                instrumented.contains(op.as_str()) || uninstrumented.contains(op.as_str()),
                "unclassified canonical operation: {op}"
            );
        }
    }

    #[test]
    fn deterministic_event_id_is_stable_and_lexical() {
        let action = AuditAction::JobSubmit;
        let first = deterministic_event_id("decision-1", &action, "corr-1", "job-1");
        let second = deterministic_event_id("decision-1", &action, "corr-1", "job-1");
        assert_eq!(first, second);
        let different = deterministic_event_id("decision-1", &action, "corr-1", "job-2");
        assert_ne!(first, different);
    }

    #[test]
    fn child_context_preserves_correlation_and_parent() {
        let parent_id = AuditEventId::new();
        let child = AuditChainContext::child_of("corr-1", &parent_id);
        assert_eq!(child.correlation_id.as_deref(), Some("corr-1"));
        assert_eq!(child.causation_parent.as_deref(), Some(parent_id.as_str()));
    }

    #[test]
    fn builders_reject_secret_bearing_labels_at_build_time() {
        let (principal, provenance) = local_context("corr-secret");
        let pool_block = |builder: AuditEventBuilder| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("rt");
            rt.block_on(async {
                let pool = sqlx::SqlitePool::connect("sqlite::memory:")
                    .await
                    .expect("pool");
                crate::session::schema::migrate(&pool)
                    .await
                    .expect("migrate");
                let store = crate::audit::AuditStore::new(pool);
                store.append(builder).await
            })
        };
        let chain = AuditChainContext::new();
        let evil = tool_invoke_event(
            &principal,
            &provenance,
            &chain,
            "ghp_eviltool",
            "native",
            "allow",
        );
        let result = pool_block(evil);
        assert!(result.is_err(), "secret-bearing tool name must fail");
    }

    #[test]
    fn structural_digest_is_hex_and_stable() {
        let first = structural_digest(b"prompt body");
        let second = structural_digest(b"prompt body");
        assert_eq!(first, second);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn coverage_entries_have_valid_metadata_keys() {
        for entry in REQUIRED_AUDIT_COVERAGE {
            for key in entry.metadata {
                assert!(!key.is_empty(), "empty key for {}", entry.action);
                let mut chars = key.chars();
                match chars.next() {
                    Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
                    _ => panic!("bad key start {key} for {}", entry.action),
                }
                assert!(
                    key.chars().all(|c| c.is_ascii_lowercase()
                        || c.is_ascii_digit()
                        || c == '_'
                        || c == '.'
                        || c == '-'),
                    "bad key {key} for {}",
                    entry.action
                );
            }
        }
    }

    #[test]
    fn project_locator_flows_into_builder_chain() {
        let (principal, provenance) = local_context("corr-project");
        let project =
            ProjectId::parse("01J0000000000000000000000").unwrap_or_else(|_| ProjectId::new());
        let chain = AuditChainContext {
            project: Some(project),
            session_id: Some("session-1".to_owned()),
            ..AuditChainContext::default()
        };
        let builder = session_create_event(&principal, &provenance, &chain, "session-1", "allow");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("rt");
        let stored = rt.block_on(async {
            let pool = sqlx::SqlitePool::connect("sqlite::memory:")
                .await
                .expect("pool");
            crate::session::schema::migrate(&pool)
                .await
                .expect("migrate");
            let store = crate::audit::AuditStore::new(pool);
            store.append(builder).await.expect("append")
        });
        assert_eq!(stored.session_id.as_deref(), Some("session-1"));
        assert!(stored.project_id.is_some());
    }

    // --- M001 trusted execution audit seam ---------------------------

    #[test]
    fn trusted_context_preserves_principal_provenance_chain() {
        let (principal, provenance) = local_context("corr-m001");
        let chain = AuditChainContext {
            correlation_id: Some("corr-m001".to_owned()),
            session_id: Some("session-m001".to_owned()),
            turn_id: Some("turn-m001".to_owned()),
            job_id: Some("job-m001".to_owned()),
            ..AuditChainContext::default()
        };
        let ctx = TrustedExecutionAuditContext::new(&principal, &provenance, chain.clone());
        assert_eq!(ctx.principal(), &principal);
        assert_eq!(ctx.provenance(), &provenance);
        assert_eq!(ctx.chain(), &chain);
        // Builder binds the same attribution losslessly.
        let builder = ctx.builder(AuditAction::ToolInvoke);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("rt");
        let stored = rt.block_on(async {
            let pool = sqlx::SqlitePool::connect("sqlite::memory:")
                .await
                .expect("pool");
            crate::session::schema::migrate(&pool)
                .await
                .expect("migrate");
            let store = crate::audit::AuditStore::new(pool);
            store.append(builder).await.expect("append")
        });
        assert_eq!(
            stored.actor_principal.as_str(),
            principal.principal_id().as_str()
        );
        assert_eq!(stored.decision_id, "decision-test");
        assert_eq!(stored.correlation_id, "corr-m001");
        assert_eq!(stored.session_id.as_deref(), Some("session-m001"));
        assert_eq!(stored.turn_id.as_deref(), Some("turn-m001"));
        assert_eq!(stored.job_id.as_deref(), Some("job-m001"));
    }

    #[test]
    fn trusted_context_child_preserves_correlation() {
        let (principal, provenance) = local_context("corr-child");
        let chain = AuditChainContext {
            correlation_id: Some("corr-child".to_owned()),
            session_id: Some("session-child".to_owned()),
            ..AuditChainContext::default()
        };
        let ctx = TrustedExecutionAuditContext::new(&principal, &provenance, chain);
        let parent = AuditEventId::new();
        let child = ctx.child_with_parent(&parent);
        assert_eq!(child.principal(), &principal);
        assert_eq!(child.provenance(), &provenance);
        assert_eq!(
            child.chain().causation_parent.as_deref(),
            Some(parent.as_str())
        );
        assert_eq!(child.chain().correlation_id.as_deref(), Some("corr-child"));
        assert_eq!(child.chain().session_id.as_deref(), Some("session-child"));
    }

    #[test]
    fn trusted_context_legacy_local_uses_explicit_provenance() {
        let ctx = TrustedExecutionAuditContext::legacy_local("corr-legacy-m001");
        // Explicit local-owner binding, never an invented team identity.
        assert_eq!(ctx.principal().principal_id().as_str(), "local-owner");
        assert_eq!(ctx.provenance().decision_id(), "legacy-local");
        assert_eq!(ctx.provenance().policy(), "local_owner_broad");
        assert_eq!(ctx.provenance().correlation_id(), "corr-legacy-m001");
        assert_eq!(
            ctx.chain().correlation_id.as_deref(),
            Some("corr-legacy-m001")
        );
    }

    #[test]
    fn trusted_context_carries_no_secret_or_body_fields() {
        // Compile-time shape: only principal/provenance/chain. Debug
        // rendering must not leak payload-shaped fields.
        let ctx = TrustedExecutionAuditContext::legacy_local("corr-shape");
        let rendered = format!("{ctx:?}");
        for forbidden in [
            "command", "argv", "secret", "token", "password", "body", "output", "prompt",
        ] {
            assert!(
                !rendered.to_lowercase().contains(forbidden),
                "trusted context debug must not carry {forbidden}"
            );
        }
    }

    #[test]
    fn emitter_without_pool_drops_with_counter() {
        let before = emit_counters_snapshot();
        let emitter = ExecutionAuditEmitter::new(None);
        assert!(!emitter.has_pool());
        assert_eq!(emitter.timeout(), EXECUTION_AUDIT_EMIT_TIMEOUT);
        let ctx = TrustedExecutionAuditContext::legacy_local("corr-drop");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("rt");
        rt.block_on(async {
            emitter
                .emit_with(&ctx, |principal, provenance, chain| {
                    session_create_event(principal, provenance, chain, "session-drop", "allow")
                })
                .await;
        });
        let after = emit_counters_snapshot();
        assert_eq!(after.dropped_no_pool, before.dropped_no_pool + 1);
        assert_eq!(after.appended, before.appended);
    }

    #[test]
    fn emitter_appends_with_shared_policy_and_counters() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("rt");
        rt.block_on(async {
            let pool = sqlx::SqlitePool::connect("sqlite::memory:")
                .await
                .expect("pool");
            crate::session::schema::migrate(&pool)
                .await
                .expect("migrate");
            let before = emit_counters_snapshot();
            let emitter = ExecutionAuditEmitter::new(Some(pool.clone()));
            assert!(emitter.has_pool());
            let ctx = TrustedExecutionAuditContext::legacy_local("corr-emit");
            emitter
                .emit_with(&ctx, |principal, provenance, chain| {
                    session_create_event(principal, provenance, chain, "session-emit", "allow")
                })
                .await;
            let after = emit_counters_snapshot();
            assert_eq!(after.appended, before.appended + 1);
            // One policy, one store: the event is queryable once.
            let store = crate::audit::AuditStore::new(pool);
            let page = store
                .query(&crate::audit::AuditQueryFilter {
                    project_id: None,
                    action: Some(AuditAction::SessionCreate.as_str().to_owned()),
                    principal: None,
                    from_seq: None,
                    limit: 100,
                })
                .await
                .expect("query");
            assert!(
                page.events.iter().any(|e| e.correlation_id == "corr-emit"),
                "emitted event must be stored once"
            );
        });
    }

    #[test]
    fn emitter_timeout_surfaces_failure_counter() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("rt");
        rt.block_on(async {
            let pool = sqlx::SqlitePool::connect("sqlite::memory:")
                .await
                .expect("pool");
            crate::session::schema::migrate(&pool)
                .await
                .expect("migrate");
            // Drop the pool so the write fails fast; the emitter must
            // record failure rather than silent success.
            drop(pool);
            let closed = sqlx::SqlitePool::connect("sqlite::memory:")
                .await
                .expect("pool");
            closed.close().await;
            let before = emit_counters_snapshot();
            let emitter = ExecutionAuditEmitter::new(Some(closed))
                .with_timeout(std::time::Duration::from_millis(50));
            let ctx = TrustedExecutionAuditContext::legacy_local("corr-fail");
            emitter
                .emit_with(&ctx, |principal, provenance, chain| {
                    session_create_event(principal, provenance, chain, "session-fail", "allow")
                })
                .await;
            let after = emit_counters_snapshot();
            assert_eq!(after.failed, before.failed + 1);
            assert_eq!(after.appended, before.appended);
        });
    }
}
