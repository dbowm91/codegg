# Daemon Authorization and Originating-Principal Attribution

Identity, authorization, and audit M003, plus team-collaboration M001 HTTP
convergence. The daemon enforces project/resource semantic capabilities at
the operation boundary using transport-bound principals, and propagates
immutable originating-principal attribution into sessions, turns, jobs, and
provider selections. Every network REST/SSE compatibility route converges
on the same service — there is no second authorization framework.

`codegg-core::authorization::AuthorizationService` is the single authority
that evaluates capabilities at request time. Handler code asks for semantic
capabilities through `operation_descriptor`; role expansion stays in
`codegg-core::team` and never appears at call sites. HTTP handlers ask
through `src/server/authz.rs` with the same capabilities as their Core
equivalents.

## Decision flow

1. The transport binds an `AuthenticatedPrincipal` per connection
   (`ClientRegistry`, HTTP/WS middleware, Unix socket) — never from a
   request payload field.
2. `CoreDaemon::handle_request_with_client` resolves the
   `RequestAuthorityContext` for the trusted connection id and calls
   `authorize_request` before the dispatch match. Denied requests reply
   with a typed `CoreResponse::Error` and have zero side effect.
3. `authorize_request` maps the request through `operation_descriptor`
   (exhaustive over `CoreRequest`; adding a variant is a compile error
   until classified), resolves the project scope (`resolve_authorization_project`:
   direct `project_id`, owning session row, or job session row), and
   evaluates via `AuthorizationService`.
4. Creation arms capture the success decision context with the durable
   record through `OriginAttributionStore` (`record_origin_with_decision`).

## LocalOwner composition

`LocalOwner` resolves through the same `AuthorizationService::authorize`
API under the broad local policy (`PolicyKind::LocalOwnerBroad`). It is an
explicit policy composition, not a bypass: the decision is still
constructed, still carries a policy marker and decision id, and still
binds attribution. Personal-local startup stays login-free; legacy
in-memory daemons without a pool decide under the same broad policy.

## Privacy

- Project enumeration is protected: `ProjectList` passes the gate as an
  enumeration decision (the principal must still be active) and
  `filter_projects_for_principal` reduces rows to `project.read` grants
  via `visible_projects`. A principal with no grants observes an empty
  list, never an error that confirms absence versus denial.
- Single-project reads deny as not-found (`denial_as_not_found`): the
  `authorization_denied` shape for `ProjectGet` is byte-identical to the
  catalog's genuinely-absent shape, so unauthorized callers cannot infer
  project existence. Presence M001 extends the same shape to
  `PresenceSnapshotGet` and `PresenceHeartbeat`: unauthorized presence
  reads/writes are indistinguishable from absent projects (no
  membership/collaborator/activity signal).
- Structured denials (`authorization_denied`,
  `authorization_scope_required`, `authorization_scope_ambiguous`,
  `authorization_principal_inactive`, `authorization_unavailable`) carry
  the operation and capability names only — no secret material and no
  project-existence signal.

## Scope kinds

- `global` — no project data; any active principal may proceed.
- `direct_project` — `project_id` locator in the DTO.
- `via_session` — `session_id` locator resolved through the session row
  (includes `JobSubmit`/`ScheduleCreate`/`RunRerun` when they carry a
  session).
- `via_job` — `job_id` locator resolved through the job's session.
- `enumeration` — bounded listing; `project_list` is privacy-filtered
  post-read, redacted connection lists are authenticated-only.
- `opaque` — no project linkage in the DTO. Team principals fail closed
  (`authorization_scope_required`); only the local-owner broad policy
  authorizes. Opaque covers filesystem locators (`project_dir`),
  credential-adjacent connection operations, run/job/schedule stores, and
  workspace-scoped mutations. Adding project linkage to these surfaces is
  follow-up work, not a semantic gap: the fail-closed default is the
  required behavior.

## Attribution

`OriginAttribution` is built from the bound principal plus the captured
`AuthorizationDecision` (`from_authority`). One row per scope
(`session`, `turn`, `job`, `provider`) in `origin_attribution`
(introduced by migration v53 at storage layout 53); the first write wins so the
origin is immutable under concurrent writers. Restart-safe: file-DB
close/reopen preserves rows.

Pre-M003 records carry no canonical principal and are attributed
explicitly through `OriginAttribution::legacy_local` — the literal
`legacy-local` provenance marker — never by fabricating a team identity.

`ToolExecutionContext` carries `origin_principal` (canonical
`PrincipalId` string), `origin_auth_method`, and `origin_decision_id`
via `apply_origin`; `principal_identity` remains the compatibility
projection. Agent-loop stamping of tool receipts from turn attribution is
M005 instrumentation input; the fields, contract, and decision linkage
are owned here.

## Narrowing

Effective agent/tool authority can only narrow. `narrow_authority`
intersects parent and child capability sets;
`authorize_child_delegation` fails closed on escalation
(`child_escalates`). Every turn re-enters the daemon gate with
`agent.invoke` on the session's project, so a delegated child cannot
exceed the session grant structurally; the contract-level negative
(parent `Contributor` vs child `member.manage`) is pinned by test.

## Provider scope

`authorize_provider_use` enforces connection scope at selection/use
time: personal connections are owner-only, project connections require
the capability on the scoped project, deployment connections require an
`Owner` grant somewhere (or the local-owner broad policy).

## Projection adapter

`team_capabilities_to_projection` maps team grants onto
`ProjectionCapabilitySet` conservatively (observe/read only, never
`AdminBypass`); `bounded_resolver_for_principal` builds the
`BoundedProjectResolver` from `project.read` grants so
`authorize_scope` enforces the same memberships as the daemon boundary.

### Session observation (presence M003)

Session-scope `ProjectionSubscribe` requires canonical
`session.observe` on the owning project (resolved through the session
row), in addition to the gate's `project.observe`. The projection
access context is derived from the same membership
(`CoreDaemon::canonical_observe_access_for_project`: team expansion +
resolver bounded to the target project), so no synthetic allow-all
authority remains on the observation path. `ProjectionResume` is
global at the gate and rechecks per stream kind on every resume;
artifact reads/lists re-enforce the team-derived context. Session and
project subscribe plus artifact list/read gate denials use
`project_not_found`, indistinguishable from absent (same convention
as `ProjectGet` and presence reads).

### Project chat (collaboration M001), access policy (team-collaboration M002), actions (M003)

`project.chat` remains the role baseline/compatibility capability
(`Contributor` and above allow, `Viewer` denies), but it is no longer
the only gate. Team-collaboration M002 (ADR-0006) adds a revisioned
daemon-owned overlay after active membership is established:
project principal overrides, per-channel mode
(`inherit_project` | `restricted`), and channel principal overrides
(`codegg-core::collaboration::policy::effective_chat_access`).
Channel-scoped requests carry only the channel locator; the daemon
resolves the owning project server-side through the durable channel
row, so unknown or foreign channels fail closed for team principals.
Channel listing filters rows through the same resolver, so restricted
or denied channels are not enumerable or distinguishable. All
project-scoped chat denials use `project_not_found` at the gate and
`chat_channel_not_found` in the handler, both indistinguishable from
absent. Policy administration (`chat_policy_get/list`,
`chat_project_policy_set`, `chat_channel_policy_set`) requires
`member.manage` (Owner only) with optimistic revision checks; stale
writes fail with `chat_policy_conflict` and change nothing. M003
`chat_action_submit/get/list` share the chat-access gate; the submit
path additionally checks the ordinary semantic capability for the
kind (`agent.delegate` for agent tasks/reviews, `job.submit` for job
submits, `session.read` for job references) before creating anything,
so a chat grant alone never authorizes execution. See
`architecture/collaboration.md`.

### Project work orders (M001)

Every `work_order_*` operation except `work_order_capabilities` is
project-scoped from its first version. Creation
(`work_order_create`, `work_order_batch_create`) requires
`session.create` — the authority sufficient to create the future
session; reads (`work_order_get/list`, `work_order_lane_get/list`,
`work_order_occurrence_get/list`, `work_order_summary`) use
`session.read`; cancel/update/pause/resume/lane
create/reorder/attach use `session.create`. ID-only requests
(`work_order_get/update/cancel/pause/resume`,
`work_order_lane_get/reorder/attach`, `work_order_occurrence_get`,
`work_order_occurrence_list` via its work order) resolve the owning
project server-side through the durable work-order/occurrence/lane
row, so no broad local-owner-only opaque operation is the primary
team-facing surface. All work-order denials use `project_not_found`,
indistinguishable from absent — unauthorized callers cannot enumerate
waiting work or infer project existence from an opaque id. Creator
origin attribution is captured immutably on creation
(`OriginAttributionStore` scope `work_order`, first-write-wins). See
`architecture/work_orders.md`.

### Project work-order triggers (M005)

Trigger *management* is ordinary principal-authorized project
mutation/reads: `work_order_trigger_create/revoke` require
`session.create`, `work_order_trigger_list/get` require
`session.read`, trigger locators resolve server-side through the
durable row, and denials keep the `project_not_found` shape.
Trigger *firing* is deliberately not a Core operation — no
`WorkOrderTriggerFire` request variant exists — so a trigger bearer
can never authorize general Core APIs. The bearer verifies against
the stored verifier inside the narrow HTTP POST route only and never
enters principal resolution. See `architecture/work_orders.md`.

### HTTP compatibility convergence (team-collaboration M001)

`src/server/authz.rs` is the single server-side adapter. It accepts the
transport-bound `AuthenticatedPrincipal` plus canonical operation/scope
information and delegates to the same `AuthorizationService` and canonical
scope resolvers — never a duplicated role expansion:

- Project/session/workspace/file routes use the same capabilities as their
  Core equivalents; project enumeration filters by `visible_projects`
  before building rows.
- File routes require explicit canonical project/workspace context and
  `file.read` / `file.modify`; ambiguous or scope-free team requests fail
  closed.
- Permission/question lists require `session.read` on the owning session;
  responses require `session.create` via `authorize_control_response`,
  which M004 will narrow to the controller lease without another bypass.
- Config/provider/tool/MCP and global SSE are LocalOwner-only
  compatibility (no safe project scope); legacy `/ws` is LocalOwner-only
  with no projection authority. Team denials are privacy-safe 404s
  (`project_not_found` or equivalent) with zero side effects.
- The executable route-disposition matrix
  (`route_disposition_table()`) classifies every authenticated route;
  `scripts/check_http_route_disposition.py` enforces coverage so new
  routes cannot be mounted without a disposition.

## Failure, restart, contention

- Denied requests have zero side effect (gate precedes dispatch).
- Revocation races fail new authorization: decisions bind the membership
  revision observed at decision time, and `TeamStore` revision checks
  reject stale re-grants (`RevisionConflict`).
- In-flight work keeps its immutable origin; continuation/cancellation
  consume the captured policy/revision.
- Restart rebuilds no broader grants: membership and attribution rows
  persist; `ensure_local_owner` reconverges idempotently.
- Attribution writes are best-effort (warn, never fail the operation);
  M004 hardens the audit failure policy.

## Operation-to-capability matrix

Source of truth is `operation_descriptor` in
`crates/codegg-core/src/authorization/policy.rs`, re-exported by the
canonical `codegg_core::authorization` facade
(`scripts/check_authorization_matrix.py` enforces coverage). Current
rendering (188 native operations; M004 adds `audit_capabilities`,
`audit_export`, `audit_query` — see `architecture/audit.md` for the
audit store contract; collaboration M001 adds fourteen `chat_*`
operations below, all `project.chat`; M003 adds three `chat_action_*`
rows on the same gate; work orders M001 adds seventeen `work_order_*`
rows below, all project-scoped on `session.create`/`session.read`;
M005 adds four `work_order_trigger_*` management rows on the same
gates (firing is not a Core operation):

| Operation | Scope | Capability |
|---|---|---|
| `active_goal_load` | via_session | `session.read` |
| `agent_select` | via_session | `agent.invoke` |
| `asset_refresh` | direct_project | `project.configure` |
| `asset_refresh_capabilities` | global | `none` |
| `asset_refresh_status` | direct_project | `project.read` |
| `audit_capabilities` | global | `none` |
| `audit_export` | direct_project | `audit.read` |
| `audit_query` | direct_project | `audit.read` |
| `chat_capabilities` | global | `none` |
| `chat_action_get` | direct_project | `project.chat` |
| `chat_action_list` | direct_project | `project.chat` |
| `chat_action_submit` | direct_project | `project.chat` |
| `chat_channel_ensure` | direct_project | `project.chat` |
| `chat_channel_list` | direct_project | `project.chat` |
| `chat_composing_list` | direct_project | `project.chat` |
| `chat_composing_set` | direct_project | `project.chat` |
| `chat_edit` | direct_project | `project.chat` |
| `chat_history` | direct_project | `project.chat` |
| `chat_read_get` | direct_project | `project.chat` |
| `chat_read_set` | direct_project | `project.chat` |
| `chat_redact` | direct_project | `project.chat` |
| `chat_send` | direct_project | `project.chat` |
| `chat_sync` | direct_project | `project.chat` |
| `connection_delete` | opaque | `project.configure` |
| `connection_disable` | opaque | `project.configure` |
| `connection_enable` | opaque | `project.configure` |
| `connection_get` | opaque | `project.read` |
| `connection_list_detail` | enumeration | `none` |
| `connection_purge` | opaque | `project.configure` |
| `connection_refresh_begin` | opaque | `project.configure` |
| `connection_refresh_cancel` | opaque | `project.read` |
| `connection_refresh_status` | opaque | `project.read` |
| `connection_restore` | opaque | `project.configure` |
| `connection_rotate_begin` | opaque | `project.configure` |
| `connection_rotate_cancel` | opaque | `project.configure` |
| `connection_rotate_secret_stage` | opaque | `project.configure` |
| `connection_rotate_status` | opaque | `project.read` |
| `edit_checkpoint_get` | opaque | `file.read` |
| `edit_checkpoint_list` | via_session | `file.read` |
| `edit_checkpoint_reapply` | via_session | `file.modify` |
| `edit_checkpoint_reapply_latest` | via_session | `file.modify` |
| `edit_checkpoint_undo` | via_session | `file.modify` |
| `edit_checkpoint_undo_latest` | via_session | `file.modify` |
| `eggpool_connection_cancel` | opaque | `project.read` |
| `eggpool_connection_create` | opaque | `project.configure` |
| `eggpool_connection_status` | opaque | `project.read` |
| `goal_checkpoint` | via_session | `session.create` |
| `goal_clear` | via_session | `session.create` |
| `goal_done` | via_session | `session.create` |
| `goal_from_file` | via_session | `agent.invoke` |
| `goal_pause` | via_session | `session.create` |
| `goal_resume` | via_session | `session.create` |
| `goal_set` | via_session | `agent.invoke` |
| `goal_set_budget` | via_session | `session.create` |
| `goal_show` | via_session | `session.read` |
| `initialize` | global | `none` |
| `job_attempts` | via_job | `session.read` |
| `job_cancel` | via_job | `job.cancel` |
| `job_get` | via_job | `session.read` |
| `job_list` | opaque | `session.read` |
| `job_recovery_report` | opaque | `audit.read` |
| `job_retry` | via_job | `job.cancel` |
| `job_submit` | via_session | `job.submit` |
| `job_wait` | via_job | `session.read` |
| `lsp_preview_apply` | opaque | `file.modify` |
| `managed_worktree_archive` | opaque | `worktree.remove` |
| `managed_worktree_cleanup` | opaque | `worktree.remove` |
| `managed_worktree_get` | opaque | `git.read` |
| `managed_worktree_list` | opaque | `git.read` |
| `memory_forget` | global | `none` |
| `memory_list` | global | `none` |
| `memory_remember` | global | `none` |
| `memory_search` | global | `none` |
| `model_select` | via_session | `agent.invoke` |
| `models_refresh` | global | `none` |
| `notification_speak` | global | `none` |
| `notification_stop` | global | `none` |
| `permission_respond` | global | `none` |
| `project_archive` | direct_project | `project.configure` |
| `project_catalog_capabilities` | global | `none` |
| `project_get` | direct_project | `project.read` |
| `project_health` | direct_project | `project.read` |
| `project_list` | enumeration | `project.read` |
| `project_register` | global | `none` |
| `project_restore` | direct_project | `project.configure` |
| `projection_ack` | global | `none` |
| `projection_artifact_list` | direct_project | `project.observe` |
| `projection_artifact_read` | direct_project | `project.observe` |
| `projection_capabilities` | global | `none` |
| `projection_resume` | global | `none` |
| `projection_snapshot_get` | opaque | `project.observe` |
| `projection_subscribe` | opaque | `project.observe` |
| `projection_unsubscribe` | global | `none` |
| `provider_connection_list` | enumeration | `none` |
| `provider_connection_models` | opaque | `project.read` |
| `question_respond` | global | `none` |
| `resume` | global | `none` |
| `run_artifact_read` | opaque | `session.read` |
| `run_get` | opaque | `session.read` |
| `run_list` | opaque | `session.read` |
| `run_rerun` | via_session | `agent.invoke` |
| `schedule_create` | via_session | `job.submit` |
| `schedule_delete` | opaque | `job.cancel` |
| `schedule_get` | opaque | `session.read` |
| `schedule_list` | opaque | `session.read` |
| `schedule_pause` | opaque | `job.cancel` |
| `schedule_resume` | opaque | `job.cancel` |
| `scheduler_snapshot` | opaque | `session.read` |
| `session_archive` | via_session | `session.create` |
| `session_attach` | via_session | `session.read` |
| `session_create` | direct_project | `session.create` |
| `session_create_from_template` | direct_project | `session.create` |
| `session_delete` | via_session | `session.create` |
| `session_export` | via_session | `session.read` |
| `session_fork` | via_session | `session.read` |
| `session_import_data` | opaque | `session.create` |
| `session_lifecycle_get` | via_session | `session.read` |
| `session_list` | direct_project | `session.read` |
| `session_load` | via_session | `session.read` |
| `session_message_counts` | opaque | `session.read` |
| `session_messages_load` | via_session | `session.read` |
| `session_rename` | via_session | `session.create` |
| `session_restore` | via_session | `session.create` |
| `session_selection_get` | via_session | `session.read` |
| `session_selection_list` | via_session | `session.read` |
| `session_selection_models` | via_session | `session.read` |
| `session_selection_update` | via_session | `agent.invoke` |
| `session_share` | via_session | `project.configure` |
| `session_unshare` | via_session | `project.configure` |
| `presence_capabilities` | global | `none` |
| `presence_heartbeat` | direct_project | `project.observe` |
| `presence_snapshot_get` | direct_project | `project.observe` |
| `snapshot_daemon` | global | `none` |
| `snapshot_models` | global | `none` |
| `snapshot_session` | via_session | `session.read` |
| `snapshot_workspace` | opaque | `project.read` |
| `subscribe` | global | `none` |
| `task_delete` | global | `none` |
| `task_list` | global | `none` |
| `task_schedule` | global | `none` |
| `todo_list` | via_session | `session.read` |
| `tool_program_call_page` | opaque | `session.read` |
| `tool_program_inspect` | opaque | `session.read` |
| `tool_program_list` | via_session | `session.read` |
| `tool_program_notification_reinject` | via_session | `agent.delegate` |
| `tool_program_recovery_debug_inspect` | via_session | `agent.delegate` |
| `turn_cancel` | via_session | `agent.invoke` |
| `turn_steer` | via_session | `agent.invoke` |
| `turn_submit` | via_session | `agent.invoke` |
| `work_order_batch_create` | direct_project | `session.create` |
| `work_order_cancel` | direct_project | `session.create` |
| `work_order_capabilities` | global | `none` |
| `work_order_create` | direct_project | `session.create` |
| `work_order_get` | direct_project | `session.read` |
| `work_order_lane_attach` | direct_project | `session.create` |
| `work_order_lane_create` | direct_project | `session.create` |
| `work_order_lane_get` | direct_project | `session.read` |
| `work_order_lane_list` | direct_project | `session.read` |
| `work_order_lane_reorder` | direct_project | `session.create` |
| `work_order_list` | direct_project | `session.read` |
| `work_order_occurrence_get` | direct_project | `session.read` |
| `work_order_occurrence_list` | direct_project | `session.read` |
| `work_order_pause` | direct_project | `session.create` |
| `work_order_resume` | direct_project | `session.create` |
| `work_order_summary` | direct_project | `session.read` |
| `work_order_trigger_create` | direct_project | `session.create` |
| `work_order_trigger_get` | direct_project | `session.read` |
| `work_order_trigger_list` | direct_project | `session.read` |
| `work_order_trigger_revoke` | direct_project | `session.create` |
| `work_order_update` | direct_project | `session.create` |
| `workspace_archive` | opaque | `project.configure` |
| `workspace_config_reload` | opaque | `project.configure` |
| `workspace_list` | global | `none` |
| `workspace_register` | global | `none` |
| `workspace_services_snapshot` | global | `none` |
| `workspace_snapshot_request` | opaque | `project.read` |
| `worktree_list` | opaque | `git.read` |

## Verification

```bash
cargo test -p codegg-core --lib authorization
cargo test --test identity_m003_daemon_authorization
python3 scripts/check_authorization_matrix.py --verbose
python3 scripts/check_project_catalog_invariants.py --verbose
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
scripts/verify.sh quick
```
