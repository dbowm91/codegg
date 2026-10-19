# Session Module Architecture

## Purpose

The session module handles SQLite-backed persistence for AI coding
conversations: creation, retrieval, search, fork, import/export,
analytics, checkpoints, and event journaling. It also provides
provider-connection lifecycle selection, legacy model-string resolution,
and a derived TUI session state reconstructed from typed events.

## Where It Lives

All source is in `crates/codegg-core/src/session/`:

```
session/
├── mod.rs               # Public re-exports, constants, JSON helpers
├── schema.rs            # Database migrations (see migrate() chain), all in sqlx
├── store.rs             # SessionStore, TodoStore, MessageStore,
│                        # PartStore, PermissionStore, UsageStore,
│                        # EventStore
├── models.rs            # Session, CreateSession, UpdateSession,
│                        # SessionAnalytics, UsageRecord, TodoItem,
│                        # PermissionEntry, LegacyResolution
├── message.rs           # Message, MessageData, Part, PartInfo,
│                        # PartData, ToolStatus
├── checkpoint.rs        # CheckpointStore, Checkpoint, WorkingFile,
│                        # SHA-256 checksum utilities
├── import.rs            # SessionImport types, validate_import_size,
│                        # redact_for_export
├── row.rs               # SessionRow, MessageRow, PartRow, TodoRow,
│                        # PermissionRow — sqlx FromRow conversions
├── status.rs            # SessionStatus enum, SessionState struct
├── continuation.rs      # Durable continuation checkpoints (M001+M004):
│                        # typed payload/envelope, lineage, digest,
│                        # ContinuationCheckpointStore + prepare_with_id
├── events.rs            # 20 typed SessionEvent variants, EventMeta,
│                        # ToolRisk, ToolCallStatus, PlanItemStatus,
│                        # ContextCompacted lineage fields
├── state.rs             # TuiSessionState — derived from events
├── selection_catalog.rs # Read-only model/health catalog helpers
└── legacy_resolution.rs # Legacy provider/model string resolver
```

## How It Works

### Database Schema

The schema is managed by `schema.rs`, whose sequential `migrate()` chain
applies every wired `migrate_vN` migration in order (current highest wiring
is checked against `storage::STORAGE_LAYOUT_VERSION` by
`scripts/check_project_catalog_invariants.py`), each wrapped in an explicit `BEGIN IMMEDIATE`
transaction. The `migration_version` table tracks the current version.
On startup, `migrate()` checks the version and runs all unapplied
migrations in order.

Migrations create and evolve **76 CREATE TABLE statements** across
these table groups:

**Core session tables (v1):**
`project`, `session`, `message`, `part`, `todo`, `permission`,
`session_share`

**Supporting tables (v3–v15):**
`cached_models` (v3), `task` (v9), `checkpoints` (v10),
`snapshot` (v13), `usage` (v15)

**Goal and event tables (v16–v21):**
`goal` (v16), `session_events` (v17), `research_run` (v18),
`user_preferences` (v19), `core_event_log` (v20),
`notification_history` (v21)

**Workspace and identity (v22, v25):**
`workspace` (v22), `logical_project`, `repository`,
`project_repository`, `workspace_project_binding`,
`session_project_binding`, `identity_diagnostic` (v25)

**Provider connections (v24, v26, v27, v31):**
`provider_connections` (v24), `provider_provisioning`,
`provider_connection_health`, `provider_connection_models` (v26),
session selection columns (v27), lifecycle/reference/tombstone/audit
tables (v31)

**Durable jobs (v23):**
`job`, `job_attempt`, `job_dependency`, `schedule`,
`schedule_occurrence`

**Agent run domain (v37–v45):**
`agent_task` + `agent_run` (v37, with `depth` in v42),
`agent_run_mailbox` + `agent_run_journal` (v38),
`managed_worktree` + `worktree_lease` (v39),
`agent_run_result` (v40),
`agent_run_group` + `agent_run_group_member` (v41, owner attribution in
v43). v44 adds `agent_task.request_fingerprint`; v45 adds `goal.revision`.

**Mutation attribution and recovery (v46–v50):**
`edit_checkpoint` (v46), `edit_restore_operation` (v47), lookup indexes
(v48, v50), `agent_convergence` + `agent_convergence_cycle` (v49)

**Team, audit, and collaboration (v51–v56):**
`principal` + `project_membership` (v51), `personal_auth_token` (v52),
`origin_attribution` (v53, scope-rebuilt in v61),
`audit_event` + `audit_body` (v54), chat schema statements (v55–v56)

**Work Orders (v60–v63):**
`work_order` + occurrence/batch ordering tables (v60),
`runtime_preferences` Task-composer model scope (v62),
`task_trigger` + `task_trigger_receipt` (v63)

**Chat policy and session control (v64–v65):**
`chat_project_policy` / `chat_channel_policy` and their override tables
(v64), the turn-scoped controller lease (v65)

**Human-shell staging and admission receipts (v71–v72):**
`human_shell_promotion` stores only bounded, already-redacted projections,
scoped to the authenticated principal and exact session/project/workspace.
Pending payloads expire after 24 hours; old consumed/expired records are
removed lazily on stage/recovery reads after an additional 24-hour
idempotency-retention window. The store caps each session/principal at 32
pending and 256 retained records. Turn admission claims
selected IDs in the same transaction as the controller lease. The table
never stores raw shell streams.

`turn_admission_receipt` records the stable request ID, authenticated principal,
session, request digest, accepted turn ID, and lifecycle status. The receipt,
controller lease, and promotion consumption commit in one transaction. A retry
with the same request ID and content returns the same receipt. Terminal
receipts are retained for 24 hours for idempotency and capped at 256 per
session/principal; still-accepted receipts are retained until terminal or
restart recovery. Recovery marks any prior accepted receipt `interrupted`,
which prevents replay after an unknown provider outcome.

**User prompt queue (v80):**
`user_prompt_queue_entry` is the durable owner for queued user prompts
(ADR-0035). It is session-scoped with `ON DELETE CASCADE`. Entries carry a
daemon-maintained dense `ordinal` under `UNIQUE (session_id, ordinal)` — a
schema-enforced total order, not a convention, which is why reorder renumbers
inside one transaction using a collision-free strategy.
`idx_user_prompt_queue_entry_session(session_id, status, ordinal)` is the
covering index the read path uses.

**Prompt queue due times (v82, ADR-0039):**
`run_at_ms INTEGER` is the nullable optional due time M007 adds. It is additive
and in place — `ALTER TABLE … ADD COLUMN`, no rebuild — which matters because
v80's `status` `CHECK` cannot be altered at all. That is also why there is no
`CHECK` here and no "scheduled" status: a nullable column with no sentinel admits
every value a writer can produce, so there is no invalid state to constrain.

`NULL` means **no due time**, not a sentinel. Every pre-v82 row reads back as an
ordinary immediate entry. A backfill of `0` would have been indistinguishable
from a real due time in the delivery predicate (`0 <= now` is true, so the row is
immediately due) while rendering every existing prompt as scheduled.

`idx_user_prompt_queue_entry_due(run_at_ms, session_id)` is **partial** over
`status = 'pending' AND run_at_ms IS NOT NULL`. It exists solely for the
cross-session advance sweep, which is the one query the v80 index cannot answer.
Partial so a queue that never schedules anything pays nothing for it: its rows
are exactly the scheduled pending entries, already bounded per session by
`USER_PROMPT_QUEUE_MAX_PENDING`.

The eligibility rule lives in one predicate,
`(run_at_ms IS NULL OR run_at_ms <= ?)`, used by `claim_next_for_delivery` and
`sessions_with_due_entries` alike. Ordering stays `ORDER BY ordinal ASC`, so
reorder permutes only entries that are *already* eligible and a scheduled entry
cannot be pulled forward by position — a not-yet-due entry is invisible to the
ordering rather than a head-of-line block. The comparison is `<=`, so an entry is
eligible at the exact due millisecond.

**Prompt queue ownership (v81, ADR-0037):**
`user_prompt_queue_owner` is the **durable, session-scoped** authority for who
may mutate a session's queue. It is a second identity concept sitting beside the
turn controller lease, not a replacement for it — see `architecture/core.md`
"Two Ownership Notions" for which question each answers. v81 is additive and
carries **only** this table; v80 is closed and was not edited.

```sql
CREATE TABLE user_prompt_queue_owner (
    session_id TEXT PRIMARY KEY,
    owner_principal TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    adopted_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
);
```

`revision` is the transfer fence, mirroring
`session_turn_controller.revision`: `user_prompt_queue_transfer` carries
`expected_revision`, and two concurrent transfers both reading the same revision
converge on exactly one winner. The loser is refused by name
(`user_prompt_queue_owner_revision_conflict`) rather than overwriting.
`ON DELETE CASCADE` removes the authorization with the session, so a deleted
session cannot leave a dangling mutator behind.

Ownership is **three-valued** — `Absent` (no owner row and no entries), `Owned(p)`
(an owner row names `p`), `Unowned` (entries but no owner row). Only `Owned(p)`
authorizes a mutation; `Unowned` fails closed on every operation including
`add`, and only explicit `UserPromptQueueAdopt` repairs it. This is why the
first `add` creates the owner row and the entry in **one transaction**: a crash
leaves neither or both, never an owner with no queue or a queue with no owner.

Bounds are enforced **twice**: a typed refusal at the store boundary
(`USER_PROMPT_QUEUE_MAX_ENTRY_BYTES` 64 KiB, `USER_PROMPT_QUEUE_MAX_PENDING`
64, `USER_PROMPT_QUEUE_MAX_RETAINED_PER_SESSION` 256) and a `CHECK`
constraint in the schema. A bounded resource refuses rather than truncates —
shortening a prompt would silently change what the user wrote. An over-bound
**edit** is refused for the same reason, and an expired entry is never
resurrected by an edit.

Expiry is **lazy and deterministic**: the read path marks due entries
`expired` and returns the survivors in one transaction, so a crash mid-sweep
leaves either all or none of them expired, and repeated reads are idempotent
(an already-`expired` row matches no query). `USER_PROMPT_QUEUE_TTL_MS` is 7
days, longer than the 24-hour promotion TTL, because a queue is an intentional
backlog rather than a staging buffer. Mutation paths sweep expiry inside their
own transaction too, so an edit or reorder cannot act on a row the read
projection would already have hidden.

Two schema decisions are deliberate and inherited by later milestones. The
status `CHECK` enumerates all five roadmap values
(`pending`/`delivering`/`delivered`/`failed`/`expired`) because SQLite cannot
`ALTER` a `CHECK` — narrowing it after M002a would have forced a
create-copy-drop-rename rebuild of a durable user table. **M003 gave all five
their producer**, so the constraint no longer admits an unreachable value. And
`run_at_ms` is **absent** from v80: nothing writes it. M007 adds it via
`ALTER TABLE … ADD COLUMN`, which SQLite performs in place with no rebuild, in
**v82** — v81 was spent on the queue-owner binding, and M003 took **no**
migration number at all (see `plans/adrs/ADR-0038-queue-delivery-execution-authority.md`,
Option B). A durable column with no producer would be a claim without a hook.

**Queue delivery (M003, ADR-0038 — no migration):**
`delivering` is **not** a terminal state, so it must behave like a live entry in
exactly three places it did not have to before M002b: the read projection, the
backlog bound, and the expiry sweep. Those three were separate
`status = 'pending'` literals while `delivering` had no producer, which was
correct only because no row could ever reach it. They are now one constant
(`LIVE_STATUS_PREDICATE`), because they are one fact — split again, and a
`delivering` row becomes invisible, uncounted, and unexpired at once, which is
the exact defect M003 exists to close.

The **operable** set is deliberately narrower than the **projected** set. A
`delivering` entry appears in the read projection but is not editable, deletable,
or reorderable, and returns `user_prompt_queue_entry_delivering` rather than
`user_prompt_queue_expired`: mapping it to `Expired` was accurate while every
non-`pending` row was terminal, and is now a lie a client may render to a user.

A client therefore sends a permutation of the **operable** ids only, and the
daemon owns where anything else lands. See **Queue renumbering (C001)** below for
what "anywhere else" means.

**Queue write transactions (C002 — no migration):**
every queue path that **reads before it writes** opens its transaction with
`begin_immediate`, which issues `BEGIN IMMEDIATE`. `SqlitePool::begin()` issues a
**deferred** `BEGIN`, which takes no lock: the first `SELECT` then holds a shared
read lock and the first `UPDATE` has to *upgrade* it, and SQLite refuses that
upgrade with `SQLITE_BUSY` — code 6, `database is deadlocked` — **without calling
the busy handler**, because waiting could deadlock. `busy_timeout` therefore never
applies, and two callers that each read before either wrote could *both* fail
rather than one winning and the other waiting its turn. Measured at 204 of 256
concurrent claim rounds before C002; zero in 192 after.

`BEGIN IMMEDIATE` takes the write lock before any read, and that acquisition *does*
honour `busy_timeout`. It goes through `sqlx`'s `TransactionManager` rather than
raw SQL, so sqlx's own transaction-depth counter stays correct — a raw `BEGIN`
leaves it at zero, which silently turns the queued rollback into a no-op.

Two paths keep a deferred transaction on purpose. `ownership` writes nothing, so
there is no read lock to upgrade. `list_live` does write (the lazy expiry sweep),
but its **first** statement is that sweep, so it already takes the write lock at
that statement; taking it earlier would serialize every queue read in the daemon.
`scripts/check_queue_write_transaction_shape.py` enforces both the shape and the
allowlist, and fails if a deferred site stops matching its stated reason.

An uncommitted `ImmediateWrite` queues a `ROLLBACK` on its connection when it
drops, exactly as `sqlx::Transaction` does. It deliberately does **not** close the
connection to force the rollback: that is destructive for a shared-cache
in-memory database, where dropping the last connection destroys the database.

Delivery transitions are `pending` → `delivering` on claim, then exactly one of
`delivered` (a turn was admitted), `failed` (a refusal, terminal, with a reason
code), or back to `pending` (never reached admission — **not** a failure, and
the distinction is what stops a transient error from silently consuming a user's
prompt). Only `failed` is explicitly retryable; `delivered` has had its turn and
`expired` outlived its window.

**Queue renumbering (C001 — invariant, no migration):**
`renumber_dense` displaces **every row in the session, status-blind** by
`USER_PROMPT_QUEUE_RENUMBER_BAND_OFFSET`, then writes dense `0..n-1` for the
**whole live set**. Before C001 it wrote the client's *operable* ids only, so a
`delivering` row was displaced and never written back: it sorted last, and a
released delivery returned to `pending` at the back of the queue carrying a
band-offset ordinal. `claim_next_for_delivery` orders by `ordinal ASC`, so that
was a **delivery-order** defect, not a rendering one.

The displacement stays exactly as it is, and must: terminal rows are not live but
still hold ordinals under a session-wide `UNIQUE (session_id, ordinal)`, so
excluding them from the displacement would reintroduce the collision the offset
exists to prevent. The write set is folded by `merge_operable_into_live`, which
holds each non-operable live row at **the rank it already had** — never pinned
to the head and never swept to the tail, because pinning a released entry to an
end is the defect being repaired. Concretely, live `[p1, d1, p2]` reordered to
`[p2, p1]` becomes `[p2, d1, p1]`.

That rule is what makes `release_delivery` and
`resolve_stranded_delivery(Retry)` order-neutral: the entry kept a dense ordinal
while stranded, so restoring `pending` moves nothing — neither the entry nor the
rows around it. `retry_failed` is the one return path the merge does **not**
reach, because it revives a row that was already **terminal** during the
reorder and therefore absent from the live set; it has no production caller, and
where a revived `failed` entry *should* land is an open maintainer decision
(C001 closure §6.1).

**The steer settlement (M006 — `mark_steer_delivered`):** a queued prompt can now
reach a turn that was **already running**, without ever passing through a claim,
so `mark_steer_delivered` is the second and last path into `delivered`. It is
guarded on `status = 'pending'` for the same at-most-once reason every other
transition here is: a row already `delivering` is running as its **own** turn, and
settling it as steered too would record one prompt delivered twice.

Its authority check is deliberately **settlement** authority, not steering
authority. ADR-0037 excludes steering from the durable owner binding, so the
*handover* is gated by the turn controller; but this write changes the queue, so
`authorize_owned_queue` runs inside the transaction — otherwise a caller who
steered someone else's turn could mark their entry delivered. Non-winning cases
return the **same typed vocabulary as `delete`** (`NotFound`, `Delivering`,
`Expired`, `NotQueueOwner`, `QueueUnowned`) rather than a second one, so a caller
reports one reason set.

One shared detail worth naming: `validate_turn_id` now backs both
`mark_delivered` and `mark_steer_delivered`. A second copy of that rule could
drift into accepting what the first refuses.

At-most-once is **inherited from `turn_admission_receipt`**, not reimplemented:
the submission id is a deterministic hash of the entry id alone, so a retry
after a restart re-derives the same receipt and receives `Duplicate`. A stranded
`delivering` row — a daemon killed between claim and receipt — is resolved at
startup **from the receipt**, never from the entry, because the receipt is the
only evidence of whether a turn actually started. That sweep is bounded
(`USER_PROMPT_QUEUE_MAX_RECONCILE` 256) and runs once per daemon start; it is a
repair pass, not a poller.

Entry text is user-authored prompt content with the same sensitivity as a
submitted turn: it never appears in a `tracing` call, an audit record, or a
`CoreResponse::Error` message. Diagnostics reference entry identity only. Audit
records carry entry identity, the actor's own owner identity, and a byte count —
never prompt text and never a digest of it.

**Tool Programs (v33–v35):**
`tool_program`, `tool_program_call` (v33),
`tool_program_notification` (v34), nullable lineage columns on `job`
(v35)

**Projections (v32):**
`projection_stream`, `projection_event`, `projection_checkpoint`

**Discovery (v28–v29):**
`project_locator`, `project_health`,
`legacy_catalog_association_marker` (v28), `discovery_root`,
`discovery_scan`, `discovery_observation` (v29)

**Runtime assets (v30):**
`runtime_asset_refresh`

**Continuation checkpoints (v57):**
`continuation_checkpoint` — durable context-epoch candidates with
`prepared | installed | aborted` lifecycle, explicit
`previous_installed_id` lineage, per-session sequence, SHA-256 payload
digest, and bounded 128 KiB payload envelope. Only `installed` rows are
resume authority. Distinct from the historical `checkpoints` table (a
different full-session snapshot contract) and from the user-facing goal
Markdown journal.

**Runtime preferences (v58):**
`runtime_preferences` — daemon-owned principal-scoped approval/sandbox/
model preference (see `architecture/execution_reliability.md` or
`approval.rs`).

**WorkPlans (v59):**
`work_plan` + `work_item` — durable revisioned long-horizon work state
(see `architecture/work_plan.md`). One active plan per session; items carry
bounded dependencies/acceptance/evidence JSON with CAS revisions. Additive
with no backfill; legacy sessions simply have no active plan.

**Repository bindings and job targets (v66–v68):**
Eggwork fixed-target job/attempt columns (v66),
`job_attempt.source_subject_json` (v67),
`work_plan_eggplan_binding` / `work_plan_eggplan_item_binding` /
`work_order_eggplan_binding` (v68, append/replace only)

### Column additions to session table

The `session` table gains columns across multiple migrations:
v12 adds `time_deleted`, v22 adds `workspace_id`, v27 adds
`provider_connection_id`, `provider_connection_revision`,
`model_catalog_revision`, `selected_model_id`, `agent`, `model`.

### Session Columns constant

The `SESSION_COLUMNS` constant (`session/mod.rs:48`) lists all 27
session columns including the v27 provider connection selection fields.
Qualified queries use `SESSION_COLUMNS_QUALIFIED` (`session/mod.rs:55`).

### Session Lifecycle

1. **Creation**: `SessionStore::create()` generates a UUID, creates
   the project row if missing, inserts the session with slug.
2. **Canonical binding**: `create_with_binding()` atomically creates
   a session with its `session_project_binding` row in one transaction.
3. **Fork**: Copies messages/parts/todos with new IDs, preserves
   parent_id and workspace binding.
   **Rename** (`update` with a new title, including daemon auto-title)
   recomputes `slug` from the new title in the same statement; a
   title-less update leaves the slug untouched. The slug is a derived
   non-unique search key, never an identity.
4. **Revert**: Truncates messages after a pivot point, saves removed
   messages/parts as JSON in `session.revert`.
5. **Soft delete**: Sets `time_deleted`; queries filter `IS NULL`.
6. **Archive**: Sets `time_archived`; `list()` excludes archived;
   `list_all()` includes them.
7. **Share**: Generates a 7-day share URL (configurable via
   `CODEGG_SHARE_DURATION_DAYS`) with a random token.

### Active-turn controller lease (team-collaboration M004)

Writers serialize turns through the per-session `active_turn` lock,
but the lock alone names no human: the turn-scoped controller lease
(ADR-0007, `codegg-core::session_control`, migration v65 tables
`session_turn_controller` + `session_control_request`) names the
principal that submitted the turn plus originating client, revision,
and transfer/takeover provenance. Successful `TurnSubmit` acquires
the lease atomically with accepting the turn (store conflict rolls
the in-memory turn back, so failed submissions leave no lease); the
per-turn reaper and restart recovery release it on the first terminal
event for the exact turn id (terminal wins over later transfer).
Steer/cancel and permission/question responses require the
controller principal in addition to existing capabilities;
same-principal second devices pass, observers/chat grants never do,
and ambiguous provenance (no lease, legacy attribution, revoked
controller) fails closed until an explicit Maintainer/Owner
takeover. `SnapshotSession` and `SessionSnapshot` project the
controller principal plus coarse revision only — never credentials
or device secrets. See `architecture/authorization.md` and
`architecture/collaboration.md`.

### Event System

`EventStore` (`session/store.rs:2687`) persists typed `SessionEvent`
variants into the `session_events` table. Events are:
- `GoalSet`, `PlanUpdated`, `PlanItemUpdated`
- `AgentMessage`, `UserMessage`
- `ToolCallStarted`, `ToolCallFinished`
- `ToolProgramNotification` (with semantic equality for crash recovery)
- `PermissionRequested`, `PermissionResolved`
- `FileChanged`, `TestRunStarted`, `TestRunFinished`
- `ContextCompacted`, `ModelRouted`
- `SubagentStarted`, `SubagentFinished`
- `FindingRaised`, `CheckpointCreated`, `SessionExported`

`EventStore::append_idempotent()` provides exactly-once semantics.
ToolProgramNotification and ContextCompacted events use
`semantic_equals()` which ignores `meta.created_at` (stamped on crash
recovery) while requiring all identity and content fields to match.
`EventStore::append_in_tx()` shares the same serialization and
collision semantics inside a caller-owned transaction so continuation
installation commits checkpoint state and its commit-marker event
atomically.

`ContextCompacted` carries additive optional continuation lineage
(`checkpoint_id`, `checkpoint_digest`, `epoch_sequence`,
`previous_checkpoint_id`, `continuity_degraded_reason`) with
`#[serde(default)]`. Pre-M001 stored JSON without those fields remains
deserializable. The continuation payload itself is never embedded in
the event; the event is structural evidence and the atomic install
commit marker.

### TUI Session State

`TuiSessionState` (`session/state.rs:107`) is a fully derived
in-memory representation reconstructed from events via
`from_events()`. It tracks: goal, plan, active/recent tool calls
(capped at 50), changed files, test state, context state, model state,
findings, and subagent summaries.

`TestState` transitions to `Stale` when a `FileChanged` event arrives
after a `Passed` state (but not after `Failed`).

### Provider Connection Lifecycle

`SessionLifecycleGet` provides the current state, health timestamp,
selected model, and bounded removed-model list. Disable, tombstone,
missing credentials, stale health, and removed models are surfaced as
lifecycle diagnostics and never trigger fallback to another connection.

### Legacy Resolution

`legacy_resolution.rs` resolves a legacy `provider/model` string
against the durable connection catalog. It is deterministic,
non-fallbacking, and read-only. Outcomes:
- `Unset` — empty string
- `Resolved` — single active match
- `UnresolvedLegacyProvider` — no match
- `AmbiguousLegacyProvider` — multiple matches
- `DisabledLegacyConnection` — match is disabled/tombstoned/error
- `MissingCredentialLegacyConnection` — match lacks credential

## Key Types & APIs

### Session (`session/models.rs:6`)

```rust
pub struct Session {
    pub id: String,
    pub project_id: String,          // legacy string projection
    pub workspace_id: Option<String>,
    pub parent_id: Option<String>,
    pub slug: String,
    pub directory: String,           // filesystem locator, not a project ID
    pub title: String,
    pub version: String,
    pub share_url: Option<String>,
    pub summary_additions: Option<i64>,
    pub summary_deletions: Option<i64>,
    pub summary_files: Option<i64>,
    pub summary_diffs: Option<serde_json::Value>,
    pub revert: Option<serde_json::Value>,
    pub permission: Option<serde_json::Value>,
    pub tags: Vec<String>,
    // Provider Connections Milestone 3 fields:
    pub provider_connection_id: Option<String>,
    pub provider_connection_revision: Option<u64>,
    pub model_catalog_revision: Option<String>,
    pub selected_model_id: Option<String>,
    pub agent: Option<String>,        // legacy, not authoritative
    pub model: Option<String>,        // legacy "provider/model"
    pub time_created: i64,
    pub time_updated: i64,
    pub time_compacting: Option<i64>,
    pub time_archived: Option<i64>,
    pub time_deleted: Option<i64>,
}
```

### SessionStore (`session/store.rs:56`)

| Method | Line | Description |
|--------|------|-------------|
| `create(CreateSession)` | 76 | Create with auto UUID |
| `create_with_id(id, input)` | 83 | Stable ID for imports |
| `create_with_binding(input, pid, wid, src)` | 179 | Atomic session + binding |
| `create_from_template(template, pid, dir)` | 318 | From config template |
| `get(id)` | 341 | Get by ID (excludes soft-deleted) |
| `list(pid, limit)` | 353 | Active sessions, paginated |
| `list_with_offset(pid, limit, offset)` | 386 | Offset-based pagination |
| `list_all(pid, limit)` | 452 | Includes archived, excludes deleted |
| `list_all_with_offset(pid, limit, offset)` | 482 | Offset-based for all |
| `list_by_canonical_project(pid, limit)` | 360 | Via session_project_binding |
| `list_all_sessions(limit)` | 463 | Cross-project (import tooling) |
| `list_deleted(pid)` | 1073 | Soft-deleted sessions |
| `search(pid, query)` | 505 | LIKE title/slug/directory |
| `search_all(pid, query)` | 527 | Also searches message content |
| `find_by_tag(pid, tag)` | 550 | JSON tag matching |
| `all_tags(pid)` | 570 | Tags with counts |
| `session_count(pid)` | 405 | Active session count |
| `message_count(sid)` | 416 | Messages in session |
| `message_counts(sids)` | 425 | Batch message counts |
| `export_session(sid)` | 593 | Full JSON export |
| `import_session(data, new_pid)` | 674 | Import with ID remapping |
| `import_session_with_binding(data, pid, wid, src)` | 684 | Import + binding |
| `update(id, UpdateSession)` | 955 | Partial update (COALESCE) |
| `delete(id)` | 1037 | Soft delete |
| `soft_delete(id)` | 1042 | Set time_deleted |
| `restore(id)` | 1058 | Clear time_deleted |
| `set_tags(id, tags)` | 1085 | Replace tags |
| `fork(id)` | 1114 | Copy with new IDs |
| `archive(id)` | 1341 | Set time_archived |
| `unarchive(id)` | 1357 | Clear time_archived |
| `share_session(sid)` | 1654 | Generate share URL (7-day) |
| `unshare_session(sid)` | 1726 | Remove share |
| `revert_to_message(sid, mid)` | 1401 | Truncate + save revert |
| `unrevert_session(sid)` | 1757 | Restore from revert state |
| `generate_summary(provider, sid)` | 1552 | LLM summary |
| `generate_title(provider, sid)` | 1588 | LLM title |
| `get_analytics(pid)` | 1889 | Aggregate statistics |
| `children(id)` | 1388 | Child sessions |
| `set_share_url(id, url)` | 1372 | Direct share URL set |

### TodoStore (`session/store.rs:1922`)

Methods: `new`, `list`, `set`, `add`, `update`, `remove`, `clear`

### MessageStore (`session/store.rs:2127`)

Methods: `new`, `create`, `create_with_id`, `append_batch`, `get`, `list`,
`count`, `update`, `delete`

`append_batch` is the live-turn transcript write: one transaction, strictly
increasing `time_created` after the session's current maximum so the canonical
`ORDER BY time_created ASC, id ASC` read order reproduces write order, and
`Ok(None)` for a `session_id` with no `session` row (subagent transcripts have
no durable session, and `message.session_id` is a foreign key). See
`architecture/agent.md` for who calls it.

`fork(id)` copies the newest 5000 messages, not the oldest: a fork branches
from where the session is now, so truncating from the old end would drop the
turn it was branched from. Only parts belonging to a copied message come
along, since `part.message_id` is a foreign key.

### PartStore (`session/store.rs:2347`)

Methods: `new`, `create`, `get`, `list_by_message`, `list_by_session`,
`update`, `delete`

### PermissionStore (`session/store.rs:2465`)

Methods: `new`, `get`, `upsert`, `delete`

### RuntimePreferenceStore (`codegg-core/src/approval.rs`, table `runtime_preferences`, v58 + v62)

Daemon-owned principal-scoped durable preference for approval mode,
sandbox profile, last-used provider/model identity (M003 foundation,
M004 convergence), and the separately-scoped last Task-composer
provider/model identity (project Work Orders M003, `migrate_v62`).
Methods: `get`, `set_approval_mode`, `set_sandbox_profile`,
`set_model_preference`, `set_task_model_preference`.
Revision-gated CAS (stale writes get `Conflict`, never
last-write-wins), bounded lengths, secret-free, additive/empty on
upgrade. The Task scope never touches the ordinary session preference
(and vice versa); policy writes preserve both. Restart reloads the
last preference; explicit turn overrides win, then project ceiling,
then persisted, then `Interactive`/`WorkspaceWrite` defaults.
Frontend manifests stay display hints, never authority. Protocol:
`ApprovalPreferenceGet`, `ApprovalModeSet`, `SandboxProfileSet`,
`TaskModelPreferenceSet`, `ExecutionPolicyGet` -> `ApprovalPreference`
/ `ExecutionPolicy` (principal derived server-side).

### Selection/preference convergence (M004)

Durable session selection (`provider_connection_id`,
`provider_connection_revision`, `model_catalog_revision`,
`selected_model_id` + `SelectionService::update_selection`) is the only
model authority. `CoreRequest::ModelSelect` is a compatibility adapter
over it (`session_selection::resolve_model_select_target` + CAS update);
the runtime `selected_model` cache is only projected after durable
success and `SnapshotSession` repairs it from the durable row. The
principal last-used preference (`last_provider_connection_id` /
`last_model_id`, outcome `PreferenceApplicationOutcome`) is a
convenience default applied only to otherwise unselected sessions at
create/open (`apply_last_used_preference` + best-effort
`apply_model_preference_best_effort`); explicit bindings always win,
and unavailable/unknown/stale preferences leave the session unselected
with a bounded diagnostic (never silent fallback). Guard:
`scripts/check_model_select_convergence.py`. TUI manifest
`selected_model_id` is a display hint reconciled by
`restore::reconcile_tab_model_with_daemon` (daemon wins).

### UsageStore (`session/store.rs:2530`)

Methods: `new`, `insert`, `get_session_usage`, `get_all_usage`,
`get_session_cost_summary`

### EventStore (`session/store.rs:2687`)

Methods: `new`, `pool`, `append`, `append_idempotent`,
`list_for_session`, `has_event`, `confirm_existing`

### CheckpointStore (`session/checkpoint.rs:44`)

Methods: `new`, `save`, `load`, `load_latest`, `list`, `delete`,
`delete_all`, `has_checkpoint`

Historical full-session snapshots only. Not a continuation store and
never repurposed for context epochs.

### ContinuationCheckpointStore (`session/continuation.rs`)

Durable context-epoch foundation (M001). Methods: `prepare`,
`get`, `latest_installed`, `list_for_session`,
`mark_aborted`, `install_with_compaction_event`, `delete_candidate`.

Lifecycle: `Prepared -> Installed | Aborted`; re-abort is idempotent;
install retry with semantically identical content converges; a stale
candidate whose parent no longer equals the latest installed checkpoint
is rejected; an `Installed` checkpoint can never become `Aborted`.
`latest_installed()` never returns `Prepared`/`Aborted` and fails
closed on digest or schema mismatch. Payloads are bounded to 128 KiB,
digests are SHA-256 over the canonical envelope JSON, and diagnostics
carry IDs/digests/sizes only — never payload bodies.
`ContinuationCheckpoint::build_compacted_event()` is the small
application-facing helper so later AgentLoop integration does not
serialize SQL or event rows manually.

### Data Models

**MessageData** (`session/message.rs:13`):
```rust
pub struct MessageData {
    pub id: String,
    pub session_id: String,
    pub message_id: String,     // renamed "messageID"
    pub parts: Vec<PartInfo>,
}
```

**PartData** (`session/message.rs:38`):
`Text`, `Reasoning`, `ToolCall`, `Image`, `File`

**ToolStatus** (`session/message.rs:63`):
`Pending` (default), `Running`, `Completed`, `Error`

**SessionAnalytics** (`session/models.rs:160`):
```rust
pub struct SessionAnalytics {
    pub total_sessions: u64,
    pub total_messages: u64,
    pub total_tool_calls: u64,
    pub avg_session_duration_ms: u64,
}
```

**UsageRecord** (`session/models.rs:194`):
`id`, `session_id`, `provider`, `model`, `input_tokens`,
`output_tokens`, `cached_tokens`, `cost_usd`, `timestamp`

## Configuration Surface

- `CODEGG_SHARE_DURATION_DAYS` env var overrides the 7-day share
  expiry (parsed at `session/store.rs:1629`)
- Import size limits (`session/import.rs:69-71`):
  `MAX_IMPORT_MESSAGES = 100_000`, `MAX_IMPORT_PARTS = 500_000`,
  `MAX_TOTAL_IMPORT_BYTES = 500 MB`

## Invariants & Gotchas

1. **ID generation**: `uuid::Uuid::new_v4()` for all entities.
   Timestamps are Unix milliseconds (`Utc::now().timestamp_millis()`).
2. **Foreign keys**: `ON DELETE CASCADE` for session-related tables.
3. **Soft delete**: Queries filter `time_deleted IS NULL`. The `delete()`
   method is an alias for `soft_delete()`, not a hard delete.
4. **Partial updates**: `COALESCE(?, field)` pattern allows selective
   updates; passing `None` leaves the field unchanged.
5. **Fork copies with redaction**: Forked messages/parts are run through
   `redact_for_export()` before insertion — sensitive tool_call data
   is stripped.
6. **Revert state**: Removed messages/parts are stored as JSON in
   `session.revert` for potential restoration via `unrevert_session()`.
7. **Canonical binding required**: Executable sessions need a resolved
   project/workspace binding via `session_project_binding`. Legacy
   directory strings become executable only when they resolve to one
   existing active binding.
8. **ToolProgramNotification semantic equality**: `meta.created_at` is
   ignored during conflict reconciliation because crash recovery stamps
   a fresh timestamp. All other fields must match exactly. The same
   rule applies to `ContextCompacted` continuation commit markers,
   whose stable event ID is `continuation-checkpoint:<checkpoint_id>`.
9. **Test state staleness**: `TestState::Passed` transitions to `Stale`
   on any `FileChanged` event. `TestState::Failed` does not go stale.
10. **Message/Part ordering**: All queries use `ORDER BY time_created
    ASC, id ASC` for deterministic ordering.
11. **Slug generation**: Lowercase, alphanumeric only, spaces → hyphens,
    fallback "untitled".
12. **Redact tool names**: `bash`, `write`, `read`, `edit`, `replace`,
    `multiedit`, `terminal`, `git`, `webfetch`, `apply_patch` — input
    and output are redacted on export. Specific keys (`command`, `path`,
    `content`, etc.) are individually redacted within the input object.
    Note: the code uses `terminal`, not `tail` as some docs claim.

## Testing

Narrowest test targets:
```bash
cargo test -p codegg-core --lib session          # session unit tests
cargo test -p codegg-core --lib session::message  # message model tests
cargo test -p codegg-core --lib session::events   # event tests
cargo test -p codegg-core --lib session::state    # TUI state tests
cargo test --test session_family                     # 5-session consolidated integration binary (M004)
cargo test -p codegg-core --lib session::legacy_resolution
cargo test -p codegg-core --lib session::store::event_store_idempotency_tests
cargo test -p codegg-core --lib session::continuation
cargo test -p codegg-core --test continuation_checkpoint
```

M008 qualification (`tests/reliability_qualification_m008.rs`) proves stale
catalog/disabled-connection never silently reroute, explicit selection wins,
and preference/model/sandbox state restores through restart with
re-resolved enforcement.

## Related Docs

- [storage.md](storage.md) — Database initialization, connection
  pooling, `DaemonPaths`
- [workspace_services.md](workspace_services.md) — Workspace-local
  run store, catalog migration
- [core.md](core.md) — `ExecutionContext`, workspace binding semantics
- `crates/codegg-core/src/identity.rs` — Typed project/session
  bindings
- `crates/codegg-core/src/workspace.rs` — Workspace registry

## Source verification

Verified 2026-10-06 against `crates/codegg-core/src/session/*.rs` and
`crates/codegg-core/src/session/schema.rs`. Corrected the `CREATE TABLE`
count 71 → 73 and added the missing table groups (agent run domain v37–v45,
mutation attribution/recovery v46–v50, team/audit/collaboration v51–v56, Work
Orders v60–v63, chat policy/session control v64–v65, repository bindings
v66–v68). Corrected every `file.rs:line` ref: `SESSION_COLUMNS` `:39` → `:48`
and `SESSION_COLUMNS_QUALIFIED` `:46` → `:55` (27 columns confirmed by
counting the literal), `TuiSessionState` `:112` → `:107`,
`SessionStore` `:48` → `:56`, the five per-store `impl` blocks (TodoStore
1918→1922, MessageStore 2123→2127, PartStore 2259→2263, PermissionStore
2377→2381, UsageStore 2442→2446), `EventStore` 2599→2603 (both prose and
heading), `CheckpointStore` `:48` → `:44`, `SessionAnalytics` 159→160,
`UsageRecord` 193→194, and the import limits `:68-70` → `:69-71`. Rebuilt
the 39-row `SessionStore` method/line table from the actual `pub async fn`
declarations (most were off by 1-8). Added the omitted `new`/`pool`
constructors to the store method lists and added the missing `ToolProgram
Notification` ordering detail. Verified accurate: 20 `SessionEvent`
variants in `events.rs`, `MAX_RECENT_TOOL_CALLS = 50`, the
`continuation-checkpoint:` event ID prefix, the redacted tool-name list,
`generate_slug`'s `untitled` fallback, and the `ORDER BY time_created ASC,
id ASC` ordering.
