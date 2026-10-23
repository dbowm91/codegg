# Event Bus Module

The `bus` module provides inter-component communication via an
event-driven architecture and two synchronous request/response
registries.

## Purpose

Global event publishing and subscribing via a broadcast channel, plus
synchronous permission-request and question-request registries that
pair a pending event with a oneshot response channel.

## Where It Lives

| Path | Role |
|------|------|
| `crates/codegg-core/src/bus/global.rs` | `GlobalEventBus` singleton |
| `crates/codegg-core/src/bus/events.rs` | `AppEvent` enum (61 variants) |
| `crates/codegg-core/src/bus/mod.rs` | `PermissionRegistry`, `QuestionRegistry`, `PermissionDecision`, pending-info types |

## How It Works

### GlobalEventBus

Central event distribution using a tokio broadcast channel
(capacity 4096):

```rust
static GLOBAL_BUS: LazyLock<GlobalEventBus> = LazyLock::new(GlobalEventBus::new);

pub struct GlobalEventBus {
    tx: broadcast::Sender<AppEvent>,
}
```

- `publish()` is synchronous (not async).
- `subscribe()` returns a `broadcast::Receiver<AppEvent>`.
- `subscriber_count()` reports the receiver count for debugging.
- Uses `std::sync::LazyLock` for zero-cost singleton init.

### AppEvent Enum

61 variants across these categories:

| Category | Count | Variants |
|----------|-------|----------|
| Session | 7 | `SessionCreated`, `SessionUpdated`, `SessionArchived`, `SessionForked`, `SessionShared`, `SessionUnshared`, `SessionReverted` |
| Message | 2 | `MessageAdded`, `MessageDeleted` |
| Tool | 3 | `ToolCalled`, `ToolResult`, `ToolCallStarted` |
| MCP | 3 | `McpServerConnected`, `McpServerDisconnected`, `McpToolListChanged` |
| Permission | 2 | `PermissionPending`, `PermissionResponded` |
| Question | 2 | `QuestionPending`, `QuestionAnswered` |
| Streaming | 3 | `TextDelta`, `ReasoningDelta`, `AgentFinished` |
| Subagent | 4 | `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed` |
| AgentRun | 4 | `AgentRunUpdated`, `AgentRunProgress`, `AgentRunTerminal`, `AgentRunControlUpdated` |
| Worktree/Group | 4 | `WorktreeUpdated`, `AgentRunGroupUpdated`, `WorkstreamClaimUpdated`, `AgentRunEvidenceObserved` |
| Convergence | 1 | `ConvergenceUpdated` |
| TestRun | 3 | `TestRunStarted`, `TestRunProgress`, `TestRunCompleted` |
| Job | 1 | `JobStatusObserved` |
| Diff | 2 | `DiffPending`, `DiffResponded` |
| Goal | 4 | `GoalUpdated`, `GoalUsageUpdated`, `GoalBudgetLimited`, `GoalCompleted` |
| Run | 1 | `RunRerunLinked` |
| ProviderAttempt | 3 | `ProviderAttemptStarted`, `ProviderAttemptFailed`, `ProviderAttemptSuperseded` |
| Context/Plan | 2 | `ContextEpochStarted`, `WorkPlanUpdated` |
| Other | 10 | `ConfigChanged`, `AgentChanged`, `ModelChanged`, `CompactionTriggered`, `Error`, `Info`, `TodoUpdated`, `FileChanged`, `ContextUpdated`, `PluginUiEffect` |

Each variant has an `event_type()` method returning a `&'static str`
discriminator for SSE filtering (e.g. `"session:created"`,
`"tool_call:started"`, `"permission:pending"`, `"provider_attempt:failed"`).

`WorkstreamClaimUpdated`, `JobStatusObserved` and
`AgentRunEvidenceObserved` are internal hints; the Core projection adapter
(`src/core/mod.rs`) intentionally maps all three to `None` rather than
exposing them as client events:

- `WorkstreamClaimUpdated` carries a bounded claim hint for same-group agent
  loops; the consumer must filter by its own run id first.
- `JobStatusObserved` (M007) carries a coarse, scheduler-owned job status —
  job id, typed kind and a bounded phase label (`running`, `completed`,
  `failed`, `cancelled`, `timed_out`, `interrupted`). It deliberately omits
  the command line, environment values, logs, cache contents and artifact
  paths, because the event reaches every member of a group.
- `AgentRunEvidenceObserved` is built from bounded durable run results and
  carries relative changed paths plus validation kind/status only.

Peer loops apply same-group membership and claim-overlap filtering before
user-message delivery.

Events use `Arc<str>` for hot-path fields: `session_id` and `delta` on
`TextDelta`, and `session_id` on `ReasoningDelta` (whose `delta` remains a
`String`).

### PermissionRegistry

**All methods are `fn` (synchronous), NOT `async fn`.**

The registry stores a `DashMap<String, PendingPermission>` keyed by
`perm_id` (format `"{tool_call_id}-{tool_name}"`, e.g.
`"call_abc123-write"`). Each `PendingPermission` carries:

- `session_id: String` — the session that registered the permission.
- `turn_id: Option<String>` — optional turn scoping.
- `tx: oneshot::Sender<PermissionDecision>` — the response channel.
- `created_at: Instant` — for TTL expiry.

**Scoped API** (preferred for new call sites):

```rust
PermissionRegistry::register_with_session(
    session_id, turn_id, perm_id, tx,
);
PermissionRegistry::respond_scoped(session_id, perm_id, choice);
PermissionRegistry::unregister_scoped(session_id, perm_id);
PermissionRegistry::is_registered_scoped(session_id, perm_id);
PermissionRegistry::get_pending_for_session(session_id);
```

**Legacy API** (backward-compatible; uses `session_id = "default"`):

```rust
PermissionRegistry::register(perm_id, tx);
PermissionRegistry::respond(perm_id, choice);
PermissionRegistry::unregister(perm_id);
PermissionRegistry::is_registered(perm_id);
```

`PermissionDecision` is the bus-owned DTO:

```rust
pub enum PermissionDecision {
    AllowOnce,
    AlwaysAllow,
    DenyOnce,
    AlwaysDeny,
}
```

Has `allowed()` and `persist()` helpers plus bidirectional `From`
impls with `PermissionChoice` (the domain type in
`src/permission/mod.rs`).

### QuestionRegistry

Same pattern as `PermissionRegistry` but for interactive questions.
All methods are synchronous. Key is `question_id`; each
`PendingQuestion` carries `session_id` and `turn_id`.

**Scoped API**:

```rust
QuestionRegistry::register_with_session(
    session_id, turn_id, question_id, tx,
);
QuestionRegistry::answer_question_scoped(session_id, question_id, answers);
QuestionRegistry::unregister_scoped(session_id, question_id);
QuestionRegistry::get_pending_for_session(session_id);
```

**Legacy API**:

```rust
QuestionRegistry::register(question_id, tx);
QuestionRegistry::answer_question(question_id, answers);
```

### Cleanup and TTL

The two registries have **different** expiry policies:

| Registry | Age bound | Default |
|----------|-----------|---------|
| `PermissionRegistry` | configurable via `set_pending_ttl(Option<Duration>)` | **unbounded** (`PENDING_TTL_UNBOUNDED`) |
| `QuestionRegistry` | fixed 310 s (5 min 10 s) | fixed |

The permission registry's age bound is **not** a fixed constant. It is
unbounded until a caller arms it, because a fixed 310 s sweep silently
denied any approval the user simply had not answered yet. Production arms
it once per tool batch in `src/agent/tool_batch.rs`
(`AgentLoop::orphan_guard_ttl`): the configured
`permission.approval_timeout_secs` plus a 60 s grace margin, or a 24 h
orphan guard when no timeout is configured. The grace margin keeps the
registry a *leak guard* for a waiter dropped mid-await — it must never
evict a request the approval router is still waiting on.

Evicting an entry drops its `oneshot::Sender`, so a waiter that can never
be woken resolves as a closed-channel deny rather than parking forever.

Cleanup is throttled to run at most once every 30 seconds
(`CLEANUP_THROTTLE_MS`) via an `AtomicU64` monotonic timestamp checked on
each `register_with_session`, `pending_*_ids`, `get_pending_for_session` and
`resolve_pending_owner` call. The `cleanup_now()` method forces an
immediate sweep, ignoring the throttle; it is a no-op while the permission
TTL is unbounded.

### Server Route Session Scoping

The session-scoped routes are `/api/permission/{session_id}`,
`/api/permission/{session_id}/submit`, and `/api/question/{session_id}`
(`src/server/http.rs:289-297`). Their handlers call
`get_pending_for_session()` on both registries
(`src/server/routes/permission.rs:74`, `src/server/routes/question.rs:38`),
so pending entries are filtered by owning session.

The legacy unscoped `pending_permission_ids()` / `pending_question_ids()`
methods still exist and are used only to populate `ResyncRequired` WS
envelopes (`src/server/ws.rs:2572-2687`), where no session scope is
available from the socket.

## Key Types & APIs

| Type | File:line | Purpose |
|------|-----------|---------|
| `GlobalEventBus` | `bus/global.rs:7` | Broadcast singleton |
| `AppEvent` | `bus/events.rs:81` | 61-variant event enum |
| `PermissionRegistry` | `bus/mod.rs:88` | Permission request/response |
| `QuestionRegistry` | `bus/mod.rs:348` | Question request/response |
| `PermissionDecision` | `bus/mod.rs:12` | Bus-owned permission DTO |
| `PendingPermission` | `bus/mod.rs:46` | Stored permission with session/turn |
| `PendingPermissionInfo` | `bus/mod.rs:57` | Read-only view of pending permission |
| `PendingQuestion` | `bus/mod.rs:68` | Stored question with session/turn |
| `PendingQuestionInfo` | `bus/mod.rs:79` | Read-only view of pending question |
| `DEFAULT_SESSION_ID` | `bus/mod.rs:40` | Sentinel `"default"` for legacy calls |
| `PENDING_TTL_UNBOUNDED` | `bus/mod.rs:107` | Sentinel: no age-based expiry |
| `CLEANUP_THROTTLE_MS` | `bus/mod.rs:541` | 30 s sweep throttle |

Registry APIs not covered by the scoped/legacy lists above:

- `PermissionRegistry::cancel_session(session_id)` /
  `QuestionRegistry::cancel_session(session_id)` — drop every pending
  sender owned by a deleted session, waking those waiters with a
  closed-channel deny. Returns the number of entries dropped.
- `PermissionRegistry::resolve_pending_owner(perm_id)` /
  `QuestionRegistry::resolve_pending_owner(question_id)` — recover
  `(session_id, Option<turn_id>)` for a **bare** id. The daemon's
  `PermissionRespond` handler accepts both the scoped
  `perm:<session>:<turn>:<simple>` protocol id and the bare simple id the
  local TUI receives verbatim from `AppEvent::PermissionPending`; the bare
  form carries no session, so ownership must be recovered from the
  registry. Returns `None` when the id is unknown *or* when more than one
  session owns it, so an ambiguous reply fails closed rather than
  reaching an arbitrary waiter.
- `PermissionRegistry::pending_ttl()` — the currently configured age bound,
  or `None` when unbounded.

## Configuration Surface

No configuration file. The broadcast channel capacity (4096) and the
cleanup throttle (30 s) are compile-time constants. The question registry's
310 s TTL is also a compile-time constant, but the **permission** registry's
age bound is runtime-configured through `set_pending_ttl` by the agent
loop (see "Cleanup and TTL"); it has no operator-facing config key of its
own.

## Invariants & Gotchas

1. **Registration-before-publish**: When publishing
   `PermissionPending` or `QuestionPending`, register the responder
   channel BEFORE publishing the event. This prevents race conditions
   where the event arrives before the listener is ready.

2. **Synchronous registries**: `register()`, `respond()`,
   `answer_question()` are `fn`, not `async fn`. Never `.await`
   them.

3. **Registry keys lack session_id**: The DashMap key is `perm_id`
   or `question_id`, not a session-scoped composite. The
   `session_id` is stored in the value struct and used by the
   `*_scoped` methods for atomic check-and-remove via
   `DashMap::remove_if`.

4. **Timeout handling**: The agent loop waits for a response with **no
   deadline by default** — `permission.approval_timeout_secs` is absent
   unless the operator opts in. On timeout the operation defaults to
   deny/empty, recorded as `timeout_deny`, which is deliberately distinct
   from an explicit user deny (`user_choice`) and from an abandoned
   waiter (`channel_closed_deny`). The permission registry's own age
   bound is **unbounded until armed**; the agent loop arms it to the
   configured timeout plus a 60 s grace margin (or a 24 h orphan guard
   when no timeout is configured) so it stays a leak guard for a waiter
   dropped mid-await and never evicts a request the loop is still waiting
   on.

5. **Unregister after response**: Always call `unregister()` after
   receiving a response or after a timeout to prevent memory leaks.

6. **Backward compatibility**: Legacy `register(perm_id, tx)` uses
   `session_id = "default"`. Legacy `respond(perm_id, choice)`
   also matches against `session_id = "default"`. New call sites
   MUST use the `*_with_session` / `*_scoped` variants.

7. **A dropped event is reported, never absorbed**: the event log's
   broadcast is **bounded**. A subscriber that falls behind it loses
   events, and the loss used to be a bare log line — the consumer's
   stream simply continued as if it were complete, so a dropped tool
   completion was indistinguishable from a tool that never finished.
   The socket forwarder now reports the loss to the client with the
   namespaced `event_stream_gap` code (`crates/codegg-protocol`'s
   `EVENT_STREAM_GAP_CODE`), and the ACP adapter surfaces it to the
   consumer as the optional `codegg_event_gap` session update. A
   consumer that cannot see the gap cannot mark the affected span
   unmeasured, so **a stream with a hole must never be reported as
   complete**.

8. **The ring buffer cannot repair a broadcast lag**: the event log's
   ring buffer and the bounded broadcast have the **same capacity** and
   evict the **same** window. The events a lagged subscriber missed are
   exactly the ones the ring has already evicted, so a re-delivery from
   the ring would re-send what the broadcast still holds and **not** the
   events that were lost. `EventLog::ring_floor_seq` reports the
   boundary directly; `EventLog::replay_ring_from_any` serves the
   cases it actually serves (a reconnect resuming from a cursor) and is
   not a repair path.

## Event Flow

### Permission Flow

```
AgentLoop                  PermissionRegistry         GlobalEventBus         TUI/Server
  │ check_tool_permission()                            │                    │
  │──────────────────────►                             │                    │
  │  (if Ask)                                          │                    │
  │◄───── cached decision ─────                        │                    │
  │                                                    │                    │
  │  register_with_session(sid,tid,perm_id,tx)         │                    │
  │──────────────────────►                             │                    │
  │                                                    │                    │
  │                      publish(PermissionPending) ──►│                    │
  │                                                    │───────────────────►│
  │                                                    │                    │
  │                                                    │  respond_scoped() │
  │                      ◄──────────────────────────── │◄───────────────────│
  │◄──────────────────────                             │                    │
  │  choice                                            │                    │
```

### Question Flow

Same pattern: register BEFORE publish, wait on the oneshot, unregister
after completion. The question registry keeps its own fixed TTL.

## Testing

```bash
# Unit tests for bus, events, registries
cargo test -p codegg-core -- bus

# Session-scoped permission/question HTTP routes (server feature).
# The root package's integration tests are families: the target is
# `server_feature`, and the member is selected by name after `--`.
cargo test --features server --test server_feature -- team_collaboration_m001_http_auth
```

## Related Docs

- `architecture/permission.md` — permission domain
- `architecture/tui.md` — TUI event subscription
- `architecture/server.md` — SSE endpoint
- `architecture/agent.md` — agent loop event publishing

## Source verification

Verified 2026-10-23 against `crates/codegg-core/src/bus/{mod,global,events}.rs`,
`src/server/{http,ws}.rs`, `src/server/routes/{permission,question}.rs`,
`src/server/perm_ids.rs`, `src/permission/mod.rs`,
`src/permission/approval.rs` and `src/agent/tool_batch.rs`.

Changes made in this pass:

- **`AppEvent` 60 → 61 variants.** The enum now also carries
  `JobStatusObserved` (M007), which the doc omitted entirely. Added a
  `Job` row to the category table and re-diffed every variant name
  against `crates/codegg-core/src/bus/events.rs:81-485`; the per-row
  counts now sum to 61.
- **The "internal hints" paragraph named only two of the three events the
  Core adapter drops.** `src/core/mod.rs` maps `WorkstreamClaimUpdated`,
  `JobStatusObserved` *and* `AgentRunEvidenceObserved` to `None`. Added
  `JobStatusObserved` and documented its bounded-phase-label contract and
  its deliberate omission of command line / env / logs / artifact paths.
- **"Cleanup and TTL" was materially wrong.** It claimed *both* registries
  auto-expire after 310 s. In fact `PermissionRegistry`'s bound is
  runtime-configured and **unbounded by default**
  (`PENDING_TTL_UNBOUNDED`, `bus/mod.rs:107`); only `QuestionRegistry` has
  the fixed 310 s TTL (`bus/mod.rs:512`). Rewrote the section as a
  two-row table, documented `set_pending_ttl` / `pending_ttl` and the
  agent loop's `orphan_guard_ttl` (timeout + 60 s grace, else a 24 h
  orphan guard, `src/agent/tool_batch.rs`), and corrected the "Configuration
  Surface" claim that the TTL is a compile-time constant.
- **Invariant 4 said the age sweep is "disabled by default
  (`set_pending_ttl(None)`)".** The registry *starts* unbounded; the agent
  loop arms it. Restated, and added the `channel_closed_deny` distinction
  alongside `timeout_deny`.
- **`QuestionRegistry` line ref `bus/mod.rs:252` → `bus/mod.rs:348`**; added
  `PENDING_TTL_UNBOUNDED` (`bus/mod.rs:107`) and `CLEANUP_THROTTLE_MS`
  (`bus/mod.rs:541`) to the type table.
- **Added three undocumented registry APIs**: `cancel_session`,
  `resolve_pending_owner` (fails closed on an ambiguous id) and
  `pending_ttl`.
- **Fixed the Testing command.** `cargo test --test
  team_collaboration_m001_http_auth --features server` targets a
  nonexistent executable — the root package's integration tests are
  families, so the target is `server_feature` and the member is selected
  after `--`.

Verified accurate and left unchanged: the broadcast capacity (4096) and
`LazyLock` singleton (`bus/global.rs`), `publish`/`subscribe`/
`subscriber_count` being synchronous, the `Arc<str>` hot-path fields on
`TextDelta` / `ReasoningDelta`, `PermissionDecision`'s `allowed()` /
`persist()` helpers and its bidirectional `From` impls with
`PermissionChoice` (`src/permission/mod.rs:326,337`), the `perm_id` format
`"{tool_call_id}-{tool_name}"` (`src/agent/tool_batch.rs:942`), the
session-scoped HTTP routes (`src/server/http.rs:288-299`) and their
`get_pending_for_session()` handlers, the unscoped
`pending_permission_ids()` / `pending_question_ids()` use in
`ResyncRequired` envelopes (`src/server/ws.rs`), the registry-key/session
split with `DashMap::remove_if`, and the 30 s cleanup throttle.

### Earlier passes

Verified 2026-10-06 against `crates/codegg-core/src/bus/{mod,global,events}.rs`
and `src/server/{http,ws}.rs`, `src/server/routes/{permission,question}.rs`.
Corrected `AppEvent` 53 → 58 variants; the "Other" row claimed 8 while
listing 10 and omitted `ProviderAttempt*` (3) and `ContextEpochStarted` /
`WorkPlanUpdated` (2) — added a `ProviderAttempt` and a `Context/Plan`
row and set `Other` to 10, then diffed all 58 names and every per-row
count against the enum. Corrected `AppEvent` line ref `events.rs:61` →
`events.rs:81`. Replaced the stale "Server Route Limitation" section: the
permission/question routes are now session-scoped and use
`get_pending_for_session()`. Corrected the `event_type()` example
`"tool:delta"` → `"tool_call:started"` and noted `ReasoningDelta.delta`
stays `String`. Removed the Testing command `cargo test --test server`,
which targets a nonexistent binary: the permission/question HTTP routes
are covered by `tests/server_feature/team_collaboration_m001_http_auth.inc` (gated on
the `server` feature), and there is no `tests/server.rs`.

M004 adds the internal `AgentRunEvidenceObserved` hint, bringing the enum
to 60 variants. It carries bounded relative paths and typed validation
statuses from durable run results and is intentionally omitted from the Core
client projection adapter.
