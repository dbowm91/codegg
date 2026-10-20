# C002 evidence 02 — Group reservation, principal authority and tenancy

Read-only source audit of `crates/codegg-core` for C002 (durable, generation-fenced
"checkout writer permit"). Scope: `agent_run_group.rs`, `workstream_claim.rs`,
`workspace_services.rs`, `agent_run.rs` (identity/ownership only),
`authorization.rs` + `authorization/`. Cross-references into
`identity.rs`, `team.rs`, `transport_auth.rs`, `workspace.rs`,
`worktree_service.rs`, `work_order/`, `session/schema.rs` are cited where a
permit would have to reach them; no file outside the deliverable was modified.

Every claim carries a `file:line` citation. `UNPROVEN` marks what could not be
established from the source.

---

## 1. What is an `AgentRunGroup`?

A group is a **durable, bounded fan-out scope over a set of agent runs**. It is
not a filesystem, checkout, or resource scope.

**Record.** `AgentRunGroupRecord` (`crates/codegg-core/src/agent_run_group.rs:105-118`):

| field | type | line |
|---|---|---|
| `group_id` | `AgentRunGroupId` | `agent_run_group.rs:106` |
| `root_run_id` | `AgentRunId` | `agent_run_group.rs:107` |
| `owner_run_id` | `AgentRunId` | `agent_run_group.rs:108` |
| `owner` | `AgentRunGroupOwner` | `agent_run_group.rs:109` |
| `member_run_ids` | `Vec<AgentRunId>` | `agent_run_group.rs:110` |
| `join_policy` | `RunJoinPolicy` | `agent_run_group.rs:111` |
| `cancel_remaining_on_satisfaction` | `bool` | `agent_run_group.rs:112` |
| `status` | `RunGroupStatus` | `agent_run_group.rs:113` |
| `created_at` / `completed_at` | `i64` / `Option<i64>` | `agent_run_group.rs:114-115` |
| `winner_run_id` | `Option<AgentRunId>` | `agent_run_group.rs:116` |
| `idempotency_key` | `String` | `agent_run_group.rs:117` |

**Typed ids.** `AgentRunGroupId` (`crates/codegg-core/src/identity.rs:253-257`,
"Stable identity for a bounded group of related agent runs") and `AgentRunId`
(`crates/codegg-core/src/identity.rs:243-247`).

**Owner enum.** `AgentRunGroupOwner` (`agent_run_group.rs:26-31`):
`Turn { session_id: String, turn_id: String }` | `Run { run_id: AgentRunId }`.
The doc comment (`agent_run_group.rs:24-25`) states the split: a turn owns the
top-level fan-out it accepted; a run owns only its direct descendants.

**Join policy.** `RunJoinPolicy` = `All | AnySuccessful | FirstCompleted |
Detached` (`agent_run_group.rs:33-40`), with `as_str`/`parse`
(`agent_run_group.rs:42-63`).

**Status.** `RunGroupStatus` = `Pending | Running | Completed | Failed |
Cancelled` (`agent_run_group.rs:65-73`); `is_terminal` at
`agent_run_group.rs:86-88`.

**Bound.** `MAX_GROUP_MEMBERS: usize = 16` (`agent_run_group.rs:20`).

**Creation.** `AgentRunGroupService::create` (`agent_run_group.rs:648-722`):
validates via `validate_new` (`agent_run_group.rs:231-247` — non-empty members,
≤16 members, no duplicates, idempotency key 1..512 bytes), then authorizes the
owner and every member:
- Run-owned group: the owner run must exist, share `root_run_id`, and be
  non-terminal (`agent_run_group.rs:653-668`).
- Members of a Run-owned group must be direct children — same `root_run_id`,
  `parent_run_id == owner.run_id`, `depth == owner.depth + 1`
  (`agent_run_group.rs:682-692`).
- Members of a Turn-owned group must originate in the owning
  session/turn and be top-level (`parent_run_id.is_none()`)
  (`agent_run_group.rs:693-716`).
Then `self.groups.create_or_get(input)` (`agent_run_group.rs:720`) and
`recompute_group` (`agent_run_group.rs:721`).

**Persistence.** Two tables:
- `agent_run_group` (`crates/codegg-core/src/session/schema.rs:2176-2188`,
  migration v41): `group_id` PK, `root_run_id`, `owner_run_id`, `join_policy`
  (CHECK in `all|any_successful|first_completed|detached`), `cancel_remaining`,
  `status` (CHECK in `pending|running|completed|failed|cancelled`),
  `created_at`, `completed_at`, `winner_run_id`, `idempotency_key` **UNIQUE**
  (`schema.rs:2186`), `notification_claimed` (`schema.rs:2187`).
  `owner_kind` / `owner_session_id` / `owner_turn_id` were added by migration
  v43 (`schema.rs:2221-2229`).
- `agent_run_group_member` (`schema.rs:2191-2199`): `PRIMARY KEY (group_id,
  ordinal)`, `UNIQUE (group_id, run_id)`, FK to `agent_run_group` (CASCADE) and
  to `agent_run` (CASCADE), with `ordinal` bounded `0 <= ordinal < 16`.

The SQLite store persists with `INSERT OR IGNORE` on the idempotency key and
rolls back to re-read the winner on conflict
(`agent_run_group.rs:451-499`; the insert statement is `agent_run_group.rs:465`).
A replay with the same key but a different shape returns
`AgentRunGroupError::IdempotencyConflict` (`agent_run_group.rs:440-448`,
error text at `agent_run_group.rs:178-179`).

**Lookup.**
- Store trait `AgentRunGroupStore` (`agent_run_group.rs:189-203`):
  `create_or_get`, `get`, `update`, `list_by_owner`, `list_by_member`.
- `load_sqlite_group` (`agent_run_group.rs:373-420`) reads the group row plus
  ordered members.
- Service reads: `status` (`agent_run_group.rs:724-731`), `groups_for_member`
  (`agent_run_group.rs:737-742`), `wait` (`agent_run_group.rs:744-767`),
  `cancel` (`agent_run_group.rs:769-795`), `member_changed`
  (`agent_run_group.rs:800-810`), `recompute` (`agent_run_group.rs:829-839`),
  `refresh` (`agent_run_group.rs:841-851`), `groups_for_run`
  (`agent_run_group.rs:856-870`).

**Service shape.** `AgentRunGroupService` holds `Arc<dyn AgentRunStore>`,
`Arc<dyn AgentRunGroupStore>`, a `Notify`, and a `broadcast::Sender` for
exactly-once terminal notifications (`agent_run_group.rs:609-614`); three
constructors: `in_memory` (`:617-625`), `with_store` (`:627-638`),
`with_pool` (`:640-642`).

---

## 2. Does a group own or reference a checkout/worktree today?

**No. There is no worktree, checkout, path, or repository field anywhere on a
group.**

- `AgentRunGroupRecord` has no such field (`agent_run_group.rs:105-118`).
- `NewAgentRunGroup` has no such field (`agent_run_group.rs:141-150`).
- The `agent_run_group` table has no such column (`schema.rs:2176-2188` and
  the v43 additions at `schema.rs:2221-2229`).
- A regex search of `agent_run_group.rs` for
  `worktree|Worktree|reservation|Reservation|checkout|Checkout|worktree_id|path|Path`
  returns **no matches** (searched across the whole file).
- A regex search of `crates/codegg-core/src/` for `reservation|Reservation`
  returns only unrelated hits: tool-program crash-boundary reservations
  (`crates/codegg-core/src/tool_program/interpreter.rs:709`, `:1341`, `:1436`,
  `:1587`), and `crates/codegg-core/src/jobs/mod.rs:1879`, which explicitly
  says a count "is not a reservation or guarantee".

**The only worktree link is run-level, not group-level.**
`AgentRunRecord.worktree_id: Option<WorktreeId>`
(`crates/codegg-core/src/agent_run.rs:185`; column `agent_run.worktree_id TEXT`
at `schema.rs:2004`), attached through
`AgentRunStore::attach_worktree(&self, id: &AgentRunId, worktree_id: WorktreeId)`
(`agent_run.rs:518-522`). A group's *member runs* may therefore each carry a
worktree id, and different members may carry different ones — the group never
names a single checkout.

**There is no "group reservation" concept.** The nearest existing thing is a
*per-worktree* reservation **state**, `ManagedWorktreeState::Reserved`
(`crates/codegg-core/src/worktree_service.rs:33-42`), in the enum
`Reserved | Preparing | Ready | InUse | Releasing | Archived | Orphaned |
Removed`. It carries no group id, no principal id, and no permit token; it is a
per-worktree lifecycle state, not a group reservation.

**The absence is already named in-tree as a gap.**
`crates/codegg-core/src/work_order/coordinator.rs:298-305`:
> "The scheduler's old global resource key was neither checkout scoped nor
> acquired by every AgentTurn writer. Calling this serialized would therefore
> promise a guarantee it cannot enforce."

with the diagnostic string at `coordinator.rs:304`:
> "shared_mutation_unavailable: this WorkOrder agent can mutate its checkout,
> but the daemon has no checkout-scoped writer reservation covering every
> AgentTurn; select auto_isolated or resolve the writer-guard capability first"

A repo-wide search for `writer-guard|writer_guard|WriterGuard` across
`crates/codegg-core/src/` and `src/` returns **only** that one diagnostic line
(`coordinator.rs:304`). There is no `WriterGuard` type, no capability, and no
registry.

---

## 3. What is a `WorkstreamClaim`? Lifetime, atomicity, owner, authority

**Definition.** `WorkstreamClaim` (`crates/codegg-core/src/workstream_claim.rs:108-121`):
`claim_id: WorkstreamClaimId`, `project_id: ProjectId`, `workspace_id:
WorkspaceId`, `group_id: AgentRunGroupId`, `target: ClaimTarget`, `state:
ClaimState`, `owner: ClaimActor`, `version: u64`, `lease_generation: u64`,
`lease_expires_at: Option<i64>`, `created_at`, `updated_at`.

- `ClaimActor` = `Principal(PrincipalId) | Run(AgentRunId)`
  (`workstream_claim.rs:28-31`).
- `ClaimTarget` = `Task(AgentTaskId) | Path(String) | Interface(String)`
  (`workstream_claim.rs:51-55`); paths are normalized to a *relative* intent
  by `normalize_relative_intent` (`workstream_claim.rs:1183-…`,
  called at `workstream_claim.rs:69`).
- `ClaimState` = `Offered | Claimed | Released | Blocked | Completed`
  (`workstream_claim.rs:76-82`).

**Authority: advisory only — confirmed from code.**
The module header is explicit (`workstream_claim.rs:1-4`):
> "Claims coordinate declared responsibilities only. They are never consulted
> by filesystem, Git, sandbox, or scheduler authorization."

ADR-0044 states the same rule (`plans/adrs/ADR-0044-restricted-agent-shared-checkout-writer-contract.md:27`):
> "A task/path claim, process-resource key, repository lock or Git lease is not
> write authority."

and (`plans/adrs/ADR-0044-restricted-agent-shared-checkout-writer-contract.md:47`):
> "Claims are advisory; no path-glob locking as a safety substitute."

Nothing in the filesystem, Git, sandbox, or scheduler path in `codegg-core`
reads `workstream_claim`. The claim's only guards are `Unauthorized`
(`workstream_claim.rs:213-214`) and `OwnedByAnother`
(`workstream_claim.rs:219-220`); a successful claim grants no capability.

**Owner.** A claim's owner is a `ClaimActor` — either a `PrincipalId` or an
`AgentRunId` — and the *scope* is the group plus the `(project_id,
workspace_id)` pair. `validate_group_actor` (`workstream_claim.rs:988-1064`)
is the ownership gate:
- The group's project/workspace scope is derived from its member runs'
  `agent_task.project_id` / `agent_task.workspace_id`; zero scoped members ⇒
  `NotFound` (`workstream_claim.rs:995-1016`).
- A `Run` actor must be the group owner or a member, and its run must be
  non-terminal (`workstream_claim.rs:1018-1037`).
- A `Principal` actor is authorized only when the group is turn-owned
  (`owner_kind == "turn"`) **and** the owning session's
  `origin_attribution.origin_principal` equals that principal
  (`workstream_claim.rs:1038-1061`). A run-owned group can never be claimed
  by a principal.

**Lifetime.** Leased, not permanent: `lease_expires_at: Option<i64>`
(`workstream_claim.rs:118`), bounded by `MAX_CLAIM_LEASE_MS: i64 = 24h`
(`workstream_claim.rs:24`) and validated by `validate_lease`
(`workstream_claim.rs:1117-1125` — deadline must be in the future and within
24h). An expired lease makes any further transition fail with `StaleRevision`
(`workstream_claim.rs:762-767`). Capacity bounds: `MAX_CLAIMS_PER_GROUP = 128`
(`workstream_claim.rs:22`), `MAX_CLAIM_EVENTS_PER_GROUP = 4096`
(`workstream_claim.rs:23`), `MAX_CLAIM_TARGET_BYTES = 256`
(`workstream_claim.rs:19`), `MAX_CLAIM_IDEMPOTENCY_BYTES = 128`
(`workstream_claim.rs:20`).

**Atomicity.** Both mutating entry points open an explicit **immediate**
write transaction: `claim` runs `BEGIN IMMEDIATE` at `workstream_claim.rs:523`
with `COMMIT`/`ROLLBACK` at `:530-539`; `transition` runs `BEGIN IMMEDIATE` at
`workstream_claim.rs:710` with `COMMIT`/`ROLLBACK` at `:717-727`. The doc
comment on `claim` states the reason (`workstream_claim.rs:497-499`):
> "The group/member checks and target arbitration run under SQLite's immediate
> writer lock so simultaneous contenders see one winner."

**Durability / tenancy columns.** `workstream_claim` table
(`schema.rs:3294-3312`): `project_id TEXT NOT NULL`, `workspace_id TEXT NOT
NULL`, FK `group_id → agent_run_group(group_id) ON DELETE CASCADE`
(`schema.rs:3310`), `UNIQUE (group_id, idempotency_key)` (`schema.rs:3311`),
`version INTEGER NOT NULL CHECK (version >= 1)` (`schema.rs:3304`),
`lease_generation INTEGER NOT NULL CHECK (lease_generation >= 1)`
(`schema.rs:3305`), and a partial unique index on the active target
(`schema.rs:3317-3320`).

---

## 4. The principal concept, and the authorization check API

**Typed principal id.** `PrincipalId` (`crates/codegg-core/src/identity.rs:238-242`),
"Stable identity for a human, service, or other authorization principal".

**Kinds.** `PrincipalKind` = `Human | ServiceAccount | Node | LocalOwner`
(`crates/codegg-core/src/team.rs:66-71`). **There is no `AgentRun` principal
kind** — an agent run is *not* a principal.

**Durable principal record.** `PrincipalRecord`
(`team.rs:131-139`): `id: PrincipalId`, `kind: PrincipalKind`,
`display_name: String`, `status: PrincipalStatus`, `revision: u64`,
`created_at`, `updated_at`. `PrincipalStatus` = `Active | Disabled`
(`team.rs:105-108`). The well-known local owner is
`LOCAL_OWNER_PRINCIPAL_ID: &str = "local-owner"` (`team.rs:58`).

**Authenticated actor binding.** `AuthenticatedPrincipal`
(`crates/codegg-core/src/transport_auth.rs:153-161`) — all fields private, "no
setters, so the binding is immutable after construction"; holds
`principal_id`, `kind`, `auth_method`, `transport_class`, `client_id`,
`authenticated_at_ms`. Accessors: `principal_id()` (`transport_auth.rs:259-261`),
`kind()` (`:263-265`), `auth_method()` (`:267-269`). Constructors:
`local_owner` (`:182-192`), `personal_token` (`:195-203`),
`bootstrap_global_bearer` (`:207-217`), `internal_test` (`:220-228`),
`reconstructed` (from a durable attribution row, `:243-257`).

**Durable first-write-wins attribution.** `OriginAttribution`
(`crates/codegg-core/src/authorization/attribution.rs:22-32`):
`origin_principal`, `origin_kind`, `auth_method`, `transport_class`, `policy`,
`membership_revision`, `decision_id`, `correlation_id`, `created_at_ms`.
"Built by the daemon boundary from the transport-bound principal plus the
captured `AuthorizationDecision`" (`attribution.rs:14-20`).
`OriginAttributionStore::record` writes with
`ON CONFLICT(scope_kind, scope_id) DO NOTHING` (`attribution.rs:116-136`) —
first write wins, origin is immutable. Valid scope kinds
(`attribution.rs:165-178`): `session`, `turn`, `run`, `job`, **`worktree`**,
`provider`, `work_order`, `continuation_grant`. Scope ids must be non-empty,
≤128 bytes, and **not path-like** (`attribution.rs:184-193`).

### The authorization check API a permit acquisition would call

| API | Signature | Location |
|---|---|---|
| `AuthorizationService::authorize` | `async fn authorize(&self, request: &AuthorizationRequest) -> Result<AuthorizationDecision, AuthorizationError>` | `crates/codegg-core/src/authorization.rs:281-337` |
| `AuthorizationService::authorize_enumeration` | `async fn authorize_enumeration(&self, principal: &AuthenticatedPrincipal, operation: &'static str, correlation_id: &str) -> Result<AuthorizationDecision, AuthorizationError>` | `authorization.rs:345-…` |
| `AuthorizationService::new` / `team` | `new(team: TeamStore)` / `team(&self) -> &TeamStore` | `authorization.rs:265-271` |
| `AuthorizationRequest::new` | `new(principal: AuthenticatedPrincipal, descriptor: OperationDescriptor, project: Option<ProjectId>, correlation_id: impl Into<String>)` | `authorization.rs:123-137` |
| `AuthorizationRequest::principal_id` | `-> &PrincipalId` | `authorization.rs:143-145` |
| `operation_descriptor` | `fn operation_descriptor(request: &codegg_protocol::core::CoreRequest) -> OperationDescriptor` | `crates/codegg-core/src/authorization/policy.rs:101` |
| `OperationDescriptor` | `{ operation: &'static str, scope_kind: ScopeKind, capability: Option<Capability> }` | `policy.rs:49-57` |
| `ScopeKind` | `Global | DirectProject | ViaSession | ViaJob | Enumeration | Opaque` | `policy.rs:10-29` |
| `TeamStore::has_capability` | `async fn has_capability(&self, project_id: &ProjectId, principal_id: &PrincipalId, capability: Capability) -> Result<bool, TeamError>` | `crates/codegg-core/src/team.rs:929-939` |
| `TeamStore::effective_capabilities` | `async fn effective_capabilities(&self, project_id: &ProjectId, principal_id: &PrincipalId) -> Result<CapabilitySet, TeamError>` | `team.rs:907-925` |
| `TeamStore::get_membership` | `async fn get_membership(&self, project_id: &ProjectId, principal_id: &PrincipalId) -> Result<Option<MembershipRecord>, TeamError>` | `team.rs:762` |
| `TeamStore::get_principal` | `async fn get_principal(&self, id: &PrincipalId) -> Result<Option<PrincipalRecord>, TeamError>` | `team.rs:655` |
| `TeamStore::list_memberships_for_principal` | `async fn list_memberships_for_principal(&self, principal_id: &PrincipalId) -> ...` | `team.rs:798` |
| `is_local_owner_broad` | `pub fn is_local_owner_broad(principal: &AuthenticatedPrincipal) -> bool` | `authorization.rs:249` |

`AuthorizationService::authorize` semantics (`authorization.rs:281-337`):
LocalOwner gets the broad local policy (`:286-293`); every other principal must
exist and be `Active` (`:294-301`); a capability-bearing operation **must**
carry a project scope, else `MissingScope` (`:310-314`); it must have a
membership granting the capability, else `Denied` (`:315-330`); on success the
decision carries `membership.revision` (`:331-337`).

`AuthorizationRequest` deliberately has **no** constructor accepting a
caller-supplied principal, role, or capability
(`authorization.rs:105-112`), so a spoofed payload principal cannot influence
the decision.

---

## 5. Resolving `ProjectId`, `RepositoryId`, `WorkspaceId`, `WorktreeId`, `AgentRunId`

### Which are durable typed ids

| Id | Where defined | Durable? | Note |
|---|---|---|---|
| `ProjectId` | `crates/codegg-core/src/identity.rs:217-222` | yes | "independent of paths and repository/workspace locators" (`identity.rs:218-219`) |
| `RepositoryId` | `identity.rs:223-227` | yes | |
| `WorktreeId` | `identity.rs:228-232` | yes | |
| `AgentRunId` | `identity.rs:243-247` | yes | |
| `AgentRunGroupId` | `identity.rs:253-257` | yes | |
| `WorkstreamClaimId` | `identity.rs:283-287` | yes | |
| `WorkspaceId` | `crates/codegg-core/src/workspace.rs:39` | yes | UUIDv4 minted by the registry, "intentionally not derived from the canonical path" (`workspace.rs:30-39`) |

All are opaque newtypes with `parse` / `new` / `as_str`; `typed_identity!` is
defined at `identity.rs:105-…` and applied at `identity.rs:217-337`.

### Which are only path-derived

- `WorkspaceRecord.canonical_root: PathBuf` (`workspace.rs:108`) — the path is
  the *lookup* key for a workspace, not its identity.
- `WorktreeRecord.repository_root: PathBuf` and `WorktreeRecord.path: PathBuf`
  (`crates/codegg-core/src/worktree_service.rs:124-125`).
- `WorkspacePathPolicy.canonical_root` / `allowed_read_roots` /
  `allowed_write_roots` (`crates/codegg-core/src/workspace_services.rs:93-97`).
- `WorkspaceServices.artifact_root` (`workspace_services.rs:277`).

### Resolution paths

- **`WorkspaceId`**: `WorkspaceRegistry::get_or_register(&self, root: &Path) -> Result<Arc<WorkspaceRecord>, WorkspaceError>`
  (`workspace.rs:480-541`) canonicalizes the root and mints a UUIDv4 id
  (`workspace.rs:526-533`), with a registration lock so two concurrent
  first-time registrations cannot mint two ids (`workspace.rs:497-512`).
  Read-back: `resolve(&self, id: &WorkspaceId)` (`workspace.rs:543-554`),
  `resolve_root(&self, root: &Path)` (`workspace.rs:556-571`).
- **`ProjectId` / `RepositoryId` / `WorktreeId` / `NodeId` together**:
  `ProjectBinding { project_id, repository_id: Option<RepositoryId>,
  workspace_id, worktree_id: Option<WorktreeId>, node_id: Option<NodeId> }`
  (`identity.rs:352-361`) — "A path-independent project binding for one
  workspace registration"; builders `with_repository` (`identity.rs:374-377`),
  `with_worktree` (`:379-382`), `with_node` (`:384-387`).
  `ProjectRepositoryBinding { project_id, repository_id }`
  (`identity.rs:342-345`).
  The **durable record that already bundles all four** is `WorktreeRecord`
  (`worktree_service.rs:118-135`): `worktree_id`, `project_id`,
  `repository_id`, `workspace_id`, `node_id`, `repository_root`, `path`,
  `branch`, `base_commit`, `managed`, `state`, `health`, `lease_generation`,
  `owner_run_id`, timestamps. This is the natural tenancy key bundle for a
  worktree-scoped permit.
- **`AgentRunId`**: `AgentRunStore::get_run(&self, id: &AgentRunId) -> Result<Option<AgentRunRecord>, AgentRunStoreError>`
  (`crates/codegg-core/src/agent_run.rs:495`);
  `AgentRunRecord` carries `workspace_id` and `worktree_id`
  (`agent_run.rs:184-185`).
- **The canonical resolved-identity bundle for a delegated run**:
  `DelegatedAgentExecutionContext` (`agent_run.rs:215-228`) —
  `current_run_id`, `root_run_id`, `parent_run_id`, `depth`, `session_id`,
  `turn_id`, `project_id`, `repository_id`, `workspace_id`, `worktree_id`,
  `shared_scratch`; built by `from_records(task, run)`
  (`agent_run.rs:231-…`). Its doc comment (`agent_run.rs:210-213`) says it
  "contains stable identities and bounded authority inputs, never prompts or
  credentials" and that reconstructing it "prevents a scheduler payload from
  becoming a second provenance authority".

---

## 6. The "effective authority narrowing" model

**The narrowing primitive.** `narrow_authority(parent: &CapabilitySet, child: &CapabilitySet) -> CapabilitySet`
(`crates/codegg-core/src/authorization/authority.rs:21-26`) — an **intersection**
of the parent set with the child set. It can only narrow.

Its two siblings:
- `child_escalates(parent, child) -> bool` (`authority.rs:29-31`).
- `authorize_child_delegation(parent, child) -> Result<CapabilitySet, AuthorizationError>`
  (`authority.rs:35-46`) — fails closed with
  `Denied { operation: "child_delegation", capability: "narrowed_parent_authority" }`
  (`authority.rs:39-44`).

**`CapabilitySet`** (`crates/codegg-core/src/team.rs:380-435`) is explicitly
"data, never authority" (`team.rs:375-378`): a sorted, deduplicated
`Vec<Capability>` with `has` (`:415-417`), `iter` (`:419-421`),
`to_sorted_keys` (`:432-434`).

**`Capability`** (`team.rs:248-291`), 21 values, `ALL` at `team.rs:295-317`.
The write-relevant ones: `FileModify` = `"file.modify"` (`team.rs:267-268`),
`GitWrite` = `"git.write"` (`team.rs:277-278`),
`WorktreeCreate` = `"worktree.create"` (`team.rs:279-280`),
`WorktreeRemove` = `"worktree.remove"` (`team.rs:281-282`),
`CommandExecute` (`team.rs:269-270`).

**Role expansion is the single central authority.**
`ProjectRole::capabilities(self) -> CapabilitySet` (`team.rs:185-…`), with the
"expansion is strictly monotonic" rule stated at `team.rs:180-184`.

**Approval/sandbox narrowing (a different axis).**
`ApprovalSnapshot::narrow_for_child` (`crates/codegg-core/src/approval.rs:502`),
and `narrow_approval` / `narrow_sandbox`
(`crates/codegg-core/src/work_order/coordinator.rs:355`, `:378`), with the
invariant "narrow but never widen" asserted at `coordinator.rs:666-672`.
`FailureClass::AuthorityNarrowed` (`crates/codegg-core/src/tool_program/interpreter.rs:175`,
displayed at `:210`) is a *failure classification*, not a permission holder.

**Where a narrowed *per-run* permission set would live — it does not.**
`AgentRunRecord.authority_digest: String` (`agent_run.rs:193`) is the only
authority field on a run; it is a bounded digest (≤256 bytes,
`agent_run.rs:651`), copied in and out of SQLite (`agent_run.rs:1408`,
`:1609`) and never interpreted inside `codegg-core`. A repo-wide grep of
`crates/codegg-core/src/` for `allowed_tools|tool_allowlist|tool_permission|permission_set|PermissionSnapshot`
returns exactly one hit, `crates/codegg-core/src/jobs/mod.rs:1139`
(`allowed_tools: Vec<String>`). That field is not a per-run ceiling: it is a
member of the `JobPayload::ToolProgram` variant's "Frozen Tool Broker
manifest" (`crates/codegg-core/src/jobs/mod.rs:1012` for the enum,
`jobs/mod.rs:1136-1139` for the field) — a *job-payload* manifest, scoped to
one tool program, not a persisted per-agent-run authority set. The same
payload variant carries `authority_grant_json: Option<String>`
(`crates/codegg-core/src/jobs/mod.rs:1140-1145`): "Pre-computed authority grant
serialized as JSON. Built at submission time from the real permission/path-
policy decision. The executor must deserialize and verify this grant rather
than fabricating a replacement." It is an opaque JSON string in a job payload,
with no codegg-core API to read, narrow, or compare it — and it is not
addressable by a `WorktreeId`.

**Could a permit acquisition consult it?**
- **Yes** for team authority: `TeamStore::effective_capabilities(project_id,
  principal_id)` (`team.rs:907-925`) and `has_capability` (`team.rs:929-939`)
  are re-readable and fail closed for unknown/disabled principals and
  non-active memberships.
- **Yes** for the admission-time snapshot: `OriginAttribution.membership_revision`
  (`attribution.rs:28`) and `policy` (`attribution.rs:27`) record the exact
  authority the work was admitted under, and `OriginAttributionStore::get`
  (`attribution.rs:143-161`) reads it back. This is the "consume the captured
  decision instead of re-resolving authority" pattern the module documents
  (`attribution.rs:17-20`).
- **No** for a run-scoped tool ceiling: `codegg-core` persists no such set.
  `UNPROVEN`: what the `authority_digest` bytes actually encode, and where
  they are computed — no digest-computation code exists in `codegg-core`; the
  field is only populated and copied there (`agent_run.rs:193`, `:303`,
  `:651`, `:1408`, `:1609`). The computation is outside this crate's scope.
- **Constraint for C002**: any consultation must go through
  `narrow_authority`/`authorize_child_delegation` (`authority.rs:21-46`) so it
  can only narrow. A permit that *widens* a capability set is not expressible
  with these primitives.

---

## 7. Is there a durable "ownership transfer" with non-duplicability?

**No "identity-bound non-duplicable final-effect owner transfer" exists in
`codegg-core`.** Three related mechanisms exist; none is a final-effect
transfer.

### 7a. Worktree lease — identity-bound, non-duplicable acquisition (closest match)

`WorktreeRecord` carries `lease_generation: u64`
(`crates/codegg-core/src/worktree_service.rs:131`) and
`owner_run_id: Option<AgentRunId>` (`worktree_service.rs:132`);
`WorktreeLease` = `{ worktree_id, owner_run_id, generation, acquired_at,
renewed_at, released_at }` (`worktree_service.rs:138-145`).

The SQLite acquisition (`worktree_service.rs:618-660`):
- reads `lease_generation, owner_run_id, state` (`:628`);
- refuses if `owner_run_id IS NOT NULL` (`:629-631`) or if the state is not
  `Ready | Reserved` (`:632-641`);
- computes `generation = lease_generation + 1` (`:642`);
- **the arbiter is a conditional UPDATE**:
  `UPDATE managed_worktree SET lease_generation = ?, owner_run_id = ?, state =
  'in_use', updated_at = ? WHERE worktree_id = ? AND owner_run_id IS NULL`
  (`worktree_service.rs:644`), and it fails unless exactly one row changed
  (`:645-647`);
- inserts the lease row (`:648`) and commits (`:649-651`).

`renew_lease` is generation-fenced: mismatched generation ⇒
`WorktreeServiceError::StaleGeneration` (`worktree_service.rs:961-966`).
`release_lease` requires owner match *and* generation match
(`worktree_service.rs:719-721`) and clears the owner with a
`WHERE ... AND owner_run_id = ? AND lease_generation = ?` predicate
(`worktree_service.rs:723`).

At the database level, `worktree_lease` has
`CREATE UNIQUE INDEX idx_worktree_active_lease ON worktree_lease(worktree_id)
WHERE released_at IS NULL` (`crates/codegg-core/src/session/schema.rs:2132`) —
one active lease per worktree, enforced by SQLite.

**This is a single-winner, generation-fenced *acquisition*. It is not a
transfer**: there is no path that hands an existing `owner_run_id` to a
different `AgentRunId` while preserving the effect, and no `owner_run_id`
change is ever performed by an update that keeps the same owner. `release_lease`
sets `owner_run_id = NULL` (`worktree_service.rs:723`), reopening the worktree
rather than transferring it.

### 7b. Workstream claim handoff — two-phase, not final-effect

- `ClaimTransition::Handoff` (`crates/codegg-core/src/workstream_claim.rs:192`)
  with `new_owner: Option<ClaimActor>` on `ClaimCas`
  (`workstream_claim.rs:203`).
- `transition_target` (`workstream_claim.rs:1092-1108`): on `Handoff` the
  claim moves to `ClaimState::Offered` with the **new** owner and
  `bump_generation = true`; a handoff to the current owner is
  `InvalidTransition` (`:1099-1101`).
- `transition_locked` re-validates the new owner through `validate_group_actor`
  (`workstream_claim.rs:769-778`) and CAS-guards the write on
  `(version, lease_generation)` (`workstream_claim.rs:780-799`).
- The new owner must still call `AcceptOffer` to actually hold it
  (`workstream_claim.rs:1109-1111`).

So the claim handoff is **offer → accept**, not a final-effect transfer: after
the handoff the claim is `Offered`, not `Claimed`, and the previous owner has
already lost it.

`request_handoff` (`workstream_claim.rs:378-404`) is explicitly only a
*request*: "Record a sibling's bounded handoff request without changing
ownership. Only the current claim owner can reassign it through `Handoff`."
(`workstream_claim.rs:376-377`).

### 7c. Scratch attempt lease — non-duplicable by primary key, but a *use* lease

`agent_scratch_attempt_lease` table with
`PRIMARY KEY (scratch_id, job_id, attempt_id)`
(`crates/codegg-core/src/session/schema.rs:3534-3543`).
`acquire_scratch_attempt_leases` inserts with `INSERT OR IGNORE`
(`crates/codegg-core/src/agent_run.rs:2045`) — non-duplicable by primary key —
and first verifies the grant's session, `owner_run_id`, and non-revoked state
(`agent_run.rs:2026-2040`). `SharedScratchResource.owner_run_id:
Option<AgentRunId>` (`agent_run.rs:260`). This grants *use* of a resource; it
does not move ownership.

### 7d. First-write-wins origin (immutable, not a transfer)

`OriginAttributionStore::record` uses
`ON CONFLICT(scope_kind, scope_id) DO NOTHING`
(`crates/codegg-core/src/authorization/attribution.rs:120`) and the doc states
"origin is immutable" (`attribution.rs:87-89`; `attribution.rs:17-20`).
It binds a principal to a scope exactly once; it never moves.

**Conclusion for Q7:** the reusable primitive is 7a's **conditional-UPDATE
single-winner acquisition** (`worktree_service.rs:644`) plus 7b's
**owner-validated, generation-bumped CAS** (`workstream_claim.rs:769-799`).
The "final-effect, single-shot, non-duplicable owner transfer" semantics that
C002's plan names **do not exist** and must be built.

---

## 8. What does `workspace_services.rs` own?

**Purpose.** Module header (`crates/codegg-core/src/workspace_services.rs:1-20`):
"Daemon-owned workspace service bundles, leases, registry, and policy." Phase 3
of the single-daemon multi-project roadmap turns each workspace into a
daemon-owned service domain. The module is "UI-, server-, and plugin-free"
(`workspace_services.rs:17-19`).

**The bundle.** `WorkspaceServices` (`workspace_services.rs:271-282`):

| member | type | storage backend |
|---|---|---|
| `workspace` | `Arc<WorkspaceRecord>` | durable, via `WorkspaceRegistry` (workspace.rs) |
| `config_snapshot` | `Arc<WorkspaceConfigSnapshot>` | **in-memory** |
| `run_store` | `Arc<dyn RunStore>` | filesystem (`FsRunStore`) in production |
| `path_policy` | `Arc<WorkspacePathPolicy>` | **in-memory** |
| `locks` | `Arc<WorkspaceLockTable>` | **in-memory** |
| `artifact_root` | `PathBuf` | filesystem (`<root>/.codegg/runs`, `:294`) |
| `activated_at`, `last_used_at`, `active_leases` | `DateTime<Utc>`, `AtomicI64`, `AtomicUsize` | in-memory |
| `shutdown` | `CancellationToken` | in-memory |

**Service list and their backends:**

1. `WorkspacePathPolicy` (`workspace_services.rs:91-98`) — canonical root plus
   approved read/write roots. Built by `for_workspace` (`:102-109`) or
   `with_allowed_roots` (`:112-135`). Content-addressed identity:
   `authority_fingerprint() -> String` (`:140-165`), a SHA-256 over the
   *trusted policy roots only*, "never from a session, model argument, or
   caller supplied identifier" (`:137-139`). **In-memory.**
2. `WorkspaceConfigSnapshot` (`workspace_services.rs:173-178`) — `revision:
   u64`, `loaded_at`, `source_files`, `diagnostics`. The production factory
   seeds `revision: 0` (`:421-426`); reloading produces a new snapshot with an
   incremented revision (`:168-171`). **In-memory.**
3. `WorkspaceLockTable` (`workspace_services.rs:204-207`) —
   `DashMap<PathBuf, Arc<AsyncMutex<()>>>` keyed by *canonical repository
   root*; `acquire_repository(&self, repo_root: &Path) -> WorkspaceRepositoryGuard`
   (`:219-234`), `tracked_repositories()` (`:237-239`),
   `forget_repository()` (`:243-245`). **In-memory only — a process-local
   mutex, not a durable lock, and it serializes *writers* without recording
   *who* is writing.** It has no principal, no generation, and no owner.
4. `RunStore` (`workspace_services.rs:274`, trait at
   `crates/codegg-core/src/run_store.rs:1054`) — production binding is
   `FsRunStore::new(run_root)` (`workspace_services.rs:418`) rooted at
   `<workspace>/.codegg/runs` (`:406`). **Filesystem.**
5. `WorkspaceServiceRegistry` (`workspace_services.rs:465-471`) —
   `DashMap<WorkspaceId, Arc<WorkspaceServices>>` + per-workspace
   `activation_locks` + factory + policy. `acquire(&self, workspace_id) ->
   Result<WorkspaceServicesLease, WorkspaceServiceError>` (`:495-505`),
   `activate` (`:511-…`) with a single-flight per-workspace lock (`:520-532`),
   idle eviction against `max_active_workspaces` (`:549-550`).
   **In-memory.**
6. `WorkspaceServicesLease` (`workspace_services.rs:347-381`) — RAII handle
   (`services()`, `workspace_id()`, `run_store()`, `config_snapshot()`,
   `path_policy()`, `locks()`, `artifact_root()`); `Drop` decrements the lease
   counter (`:383-389`).
7. `WorkspaceServicesFactory` trait (`workspace_services.rs:395-397`) and
   `ProductionWorkspaceServicesFactory` (`:402-435`).
8. `WorkspaceServicePolicy` (`workspace_services.rs:59-76`) — default
   `max_active_workspaces: 16`, `idle_evict_after: 1800s` (`:69-76`).

**Errors / reports.** `WorkspaceServiceError` (`workspace_services.rs:40-55`);
`EvictionReport` (`:439-443`); `ShutdownReport` (`:447-451`);
`ReloadResult` (`:455-460`); `WorkspaceServiceSnapshot` (`:258-266`).

**What the module does *not* have:** the registry key is `WorkspaceId` only
(`workspace_services.rs:467`) — there is no `ProjectId`, no `PrincipalId`, and
no `AgentRunId` dimension anywhere in the module. There is no durable
writer-permit, no generation, no owner identity, and no project/principal
scoping. `acquire` grants *service access*, not write authority.

---

## 9. Existing state machines with explicit states and a monotonic revision

### 9a. `workstream_claim` — the closest existing model for a permit store

- Explicit states: `ClaimState` = `Offered | Claimed | Released | Blocked |
  Completed` (`crates/codegg-core/src/workstream_claim.rs:76-82`), with
  `as_str` (`:85-93`) and a fail-closed `parse` returning
  `CorruptState` on unknown input (`:95-104`).
- Monotonic counters: `version: u64` (`workstream_claim.rs:116`) and
  `lease_generation: u64` (`workstream_claim.rs:117`).
- Legal-transition table: `transition_target`
  (`workstream_claim.rs:1066-1115`) — every arm is guarded by the current
  state, and anything else is `InvalidTransition` (`:1112`).
- CAS: `transition_locked` issues
  `UPDATE workstream_claim SET state=?, owner_kind=?, owner_id=?,
  version=version+1, lease_generation=lease_generation+?, lease_expires_at=?,
  updated_at=? WHERE claim_id=? AND version=? AND lease_generation=?`
  (`workstream_claim.rs:780-794`) and rejects unless exactly one row changed
  (`:797-799`). The pre-checks are `OwnedByAnother` (`:754-756`),
  `StaleRevision` on counter mismatch (`:757-761`), and `StaleRevision` on an
  expired lease (`:762-767`).
- Schema-level enforcement: `state TEXT NOT NULL CHECK (state IN
  ('offered','claimed','released','blocked','completed'))` (`schema.rs:3301`),
  `version INTEGER NOT NULL CHECK (version >= 1)` (`schema.rs:3304`),
  `lease_generation INTEGER NOT NULL CHECK (lease_generation >= 1)`
  (`schema.rs:3305`).
- Event log: `workstream_claim_event` table (`schema.rs:3338-3350`) with
  `action` CHECK over `claimed|renewed|released|blocked|unblocked|completed|
  handed_off|offer_accepted|handoff_requested|expired|dependency_added`
  (`schema.rs:3344`) and the `version` + `lease_generation` of the transition
  recorded on the event (`schema.rs:3345-3346`).

### 9b. `managed_worktree` — explicit states + generation (no version)

`ManagedWorktreeState` = `Reserved | Preparing | Ready | InUse | Releasing |
Archived | Orphaned | Removed`
(`crates/codegg-core/src/worktree_service.rs:33-42`), with `as_str` (`:45-56`)
and `parse_state` (`:58-72`). The counter is `lease_generation: u64`
(`worktree_service.rs:131`), bumped on every acquisition
(`worktree_service.rs:642`, `:644`). State transitions go through
`transition(&mut record, target)` (called at `worktree_service.rs:931`, `:938`,
`:989`, `:1008`, `:1041`).

### 9c. `work_order` — explicit states; revision is *not* CAS-guarded on state

`WorkOrderState` = `Active | Paused | Completed | Cancelled | Archived`
(`crates/codegg-core/src/work_order/model.rs:82-88`) with `as_str` (`:91-99`),
`parse` (`:101-110`), `is_terminal` (`:112-114`), and a free-standing legality
predicate `can_transition_work_order(from, to)`
(`crates/codegg-core/src/work_order/model.rs:120-…`).

The state update is
`UPDATE work_order SET revision = revision + 1, state = ?, updated_at = ?,
cancelled_at = ? WHERE id = ? AND project_id = ?`
(`crates/codegg-core/src/work_order/store.rs:1131-1134`) — note there is
**no `AND revision = ?`**; the revision is incremented but the transition is
not revision-guarded. The revision-guarded sibling is the **sequence lane**:
`UPDATE sequence_lane SET revision = revision + 1, updated_at = ? WHERE id = ?
AND project_id = ? AND revision = ?`
(`crates/codegg-core/src/work_order/store.rs:1442-1445`), with
`rows_affected() == 0` ⇒ `WorkOrderError::RevisionConflict`
(`work_order/store.rs:1453-1464`).

`SequenceLaneId` documents the pattern in-line
(`crates/codegg-core/src/identity.rs:320-322`): "A lane owns the stable order
of its waiting work orders. Reorder is CAS-guarded by the lane revision, never
last-writer-wins."

### 9d. `team` membership — states + revision

`MembershipState` = `Active | Suspended | Revoked`
(`crates/codegg-core/src/team.rs:446-450`); `MembershipRecord` carries
`state` and `revision: u64` (`team.rs:485-486`); conflict errors
`TeamError::RevisionConflict` (`team.rs:535-541`) and
`TeamError::PrincipalRevisionConflict` (`team.rs:542-547`);
`PrincipalRecord.revision: u64` (`team.rs:136`).

### 9e. `agent_run_group` — states but **no revision** (last-writer-wins)

`RunGroupStatus` (`agent_run_group.rs:65-73`) with `is_terminal`
(`agent_run_group.rs:86-88`), but the `agent_run_group` table has **no
revision column** (`schema.rs:2176-2188`), and the store's `update` writes the
whole row and checks `rows_affected() == 1`
(`agent_run_group.rs:598`). A group is therefore **not** a revision-CAS state
machine; concurrent status updates are last-writer-wins.

### 9f. `workspace_services` — revision without a state machine

`WorkspaceConfigSnapshot.revision: u64` (`workspace_services.rs:174`) is an
immutable-snapshot version ("Reloading produces a new snapshot with an
incremented `revision`; existing attempts continue to see the snapshot they
were admitted under", `workspace_services.rs:168-171`). It is in-memory and
carries no explicit state enum.

---

## 10. The tenancy boundary, and the check to reuse

### The project/principal boundary

`AuthorizationService::authorize` (`crates/codegg-core/src/authorization.rs:281-337`)
is the canonical cross-principal / cross-project gate:
- LocalOwner → broad local policy (`:286-293`).
- Non-local-owner principal must exist and be `Active` (`:294-301`).
- A capability-bearing operation must carry a `ProjectId` scope, else
  `AuthorizationError::MissingScope` (`:310-314`).
- The `(project, principal)` membership must exist and grant the capability,
  else `AuthorizationError::Denied` (`:315-330`).
- On success the decision carries `membership.revision` (`:331-337`).

The membership itself is keyed `(project_id, principal_id)`
(`crates/codegg-core/src/team.rs:481-489`) and
`MembershipRecord::effective_capabilities` returns the empty set for
non-`Active` memberships so authorization fails closed
(`team.rs:497-502`); `TeamStore::effective_capabilities` additionally
neutralizes disabled principals and unknown memberships
(`team.rs:907-925`).

**So: a permit acquisition that wants to deny a cross-principal or
cross-project acquisition calls `AuthorizationService::authorize` with an
`OperationDescriptor` naming the required capability (`file.modify` and/or
`git.write` at `team.rs:267-268`, `:277-278`) and the `ProjectId` of the
target checkout.**

### The run→group boundary

`validate_group_actor` (`crates/codegg-core/src/workstream_claim.rs:988-1064`):
- The group's project/workspace scope is *derived* from its member runs'
  `agent_task.project_id` / `agent_task.workspace_id`
  (`workstream_claim.rs:995-1016`) — a group with no member in the requested
  `(project, workspace)` is `NotFound`, which is the cross-project denial.
- A `Run` actor must be the group owner or a member, and non-terminal
  (`workstream_claim.rs:1018-1037`).
- A `Principal` actor is authorized only for a turn-owned group whose session
  has a matching `origin_attribution.origin_principal`
  (`workstream_claim.rs:1038-1061`).

### The group-service boundary

`AgentRunGroupService::authorize` (`crates/codegg-core/src/agent_run_group.rs:1051-1076`):
the actor must be the owning run, or — for a turn-owned group — the exact
owning `session_id` **and** `turn_id` with no run id
(`agent_run_group.rs:1061-1071`); otherwise `Unauthorized`
(`agent_run_group.rs:1075`).

### The worktree boundary

`WorktreeRecord` already carries `project_id`, `repository_id`, and
`workspace_id` (`crates/codegg-core/src/worktree_service.rs:120-122`), so a
worktree is inherently project-scoped: a permit keyed on `WorktreeId` inherits
the worktree row's tenancy. The `origin_attribution` scope kind **`worktree`**
already exists (`crates/codegg-core/src/authorization/attribution.rs:170`), so
there is already a durable, first-write-wins place to record the principal that
owns a given worktree scope.

### The three reusable checks, in the order a permit would need them

1. **Principal → project capability**: `AuthorizationService::authorize`
   (`authorization.rs:281-337`), optionally intersected with the run's
   admission-time authority via `OriginAttributionStore::get(scope_kind,
   scope_id)` (`attribution.rs:143-161`).
2. **Run → group membership**: `validate_group_actor`
   (`workstream_claim.rs:988-1064`) or `AgentRunGroupService::authorize`
   (`agent_run_group.rs:1051-1076`).
3. **Single-winner durable acquisition**: the conditional-UPDATE arbiter of
   `SqliteWorktreeStore::acquire_lease` (`worktree_service.rs:644`) with a
   `BEGIN IMMEDIATE` transaction and `rows_affected() == 1` enforcement
   (`worktree_service.rs:623-647`), and the `BEGIN IMMEDIATE` discipline
   already codified in `workstream_claim.rs:523` and `:710`.

**None of these three is currently wired to a checkout write path.** The
`work_order` coordinator already fails closed for shared mutation because the
guard does not exist (`crates/codegg-core/src/work_order/coordinator.rs:298-305`).

---

## Summary of reusable primitives

- **`AgentRunGroupId` / `AgentRunGroupRecord`** — a durable, bounded
  fan-out scope with an idempotency key, but **no worktree link and no
  revision** (`agent_run_group.rs:105-118`; `schema.rs:2176-2188`).
- **`WorkstreamClaim` + `ClaimCas`** — the best existing template for a
  permit row: explicit `ClaimState` + `version` + `lease_generation`
  (`workstream_claim.rs:76-82`, `:108-121`, `:196-205`), `BEGIN IMMEDIATE`
  transactions (`:523`, `:710`), and a revision-CAS UPDATE that rejects unless
  exactly one row changed (`:780-799`).
- **`validate_group_actor`** — the existing cross-project / cross-principal
  group-membership gate, with the project/workspace scope derived from member
  runs' tasks (`workstream_claim.rs:988-1064`).
- **`AuthorizationService::authorize` + `TeamStore::has_capability` /
  `effective_capabilities`** — the principal→project capability check
  (`authorization.rs:281-337`; `team.rs:929-939`, `:907-925`).
- **`AuthenticatedPrincipal` + `OriginAttribution` + `OriginAttributionStore`** —
  immutable, first-write-wins principal binding with a **`worktree`** scope
  kind already supported (`transport_auth.rs:153-161`;
  `authorization/attribution.rs:22-32`, `:107-140`, `:165-178`).
- **`narrow_authority` / `authorize_child_delegation` / `CapabilitySet`** —
  intersection-only narrowing; a permit can only narrow, never widen
  (`authorization/authority.rs:21-46`; `team.rs:380-435`).
- **`WorktreeRecord` as tenancy key bundle** — already carries
  `worktree_id + project_id + repository_id + workspace_id` plus
  `lease_generation` and `owner_run_id` (`worktree_service.rs:118-135`).
- **Conditional-UPDATE single-winner acquisition** — the `owner_run_id IS
  NULL` predicate with `rows_affected() == 1` enforcement
  (`worktree_service.rs:644-647`), plus the DB-level partial unique index
  `idx_worktree_active_lease` (`schema.rs:2132`).
- **`WorktreeService` state machine** — explicit
  `ManagedWorktreeState` with a `Reserved` state
  (`worktree_service.rs:33-42`) and a generation counter (`:131`, `:642`).
- **`origin_attribution` first-write-wins insert** — `ON CONFLICT DO NOTHING`
  (`authorization/attribution.rs:120`).
- **`SequenceLaneId` revision-CAS update** — the reference "never
  last-writer-wins" write (`work_order/store.rs:1442-1445`;
  `identity.rs:320-322`).
- **`WorkspaceServiceRegistry` single-flight activation** — per-`WorkspaceId`
  mutex + double-check, but **in-memory and workspace-scoped only** — no
  project, principal, or generation dimension (`workspace_services.rs:511-…`).
- **`WorkspacePathPolicy::authority_fingerprint`** — content-addressed
  identity of a trusted root set, derived only from policy roots
  (`workspace_services.rs:140-165`).
- **`ProjectBinding` / `DelegatedAgentExecutionContext`** — the existing typed
  bundles that already carry project+repository+workspace+worktree+run
  together (`identity.rs:352-361`; `agent_run.rs:215-228`).
