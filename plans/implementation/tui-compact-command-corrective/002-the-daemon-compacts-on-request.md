# TUI Compact Command Corrective — Milestone C002 — The Daemon Compacts on Request

Status: ready for handoff

Repository baseline: `3c340c1ea` — `origin/main` with the M007 line and C001
landed.

Source roadmap:

- `plans/subsystems/tui-compact-command-corrective-roadmap.md`

Decision record:

- `plans/adrs/ADR-0060-frontend-requested-compaction.md` — accepted
  2026-10-31.

Planning basis:

- **The owner's answer**, 2026-10-31: *"ideally no, if a frontend asks for
  compaction, the daemon should do the compaction and not refuse."* C001's
  closure §10.1 recorded that question and stopped at it; this milestone
  carries the answer out.
- `plans/closure/tui-compact-command-corrective/001-status.md` §10.1 — the
  recorded question, the proposed variant, and the daemon's proposed right to
  refuse.
- `plans/adrs/ADR-0060-frontend-requested-compaction.md` §3 — compaction is a
  **turn-scoped** operation: the engine compacts the live turn's history, and
  the context is built when a turn starts. This is why "compact now" means
  "the context the daemon builds next is a compacted one", and why the
  request is honoured at the daemon's next compaction opportunity for the
  session.
- `plans/000-long-term-specification.md` §4.4 — a frontend renders
  frontend-neutral snapshots and events; its own toast is presentation and
  must not narrate an effect the daemon did not perform.

Long-term requirements:

- The daemon owns compaction: the threshold, the tracker, the policy and the
  checkpoint. **No second compaction engine.**
- A frontend that asks for a compaction reports **the daemon's answer**, never
  an effect it did not cause.
- The daemon does not refuse a compaction request (ADR-0060 §3).

## 1. Objective

`/compact` must make the daemon compact, and the TUI must report what the
daemon answered.

C001 stopped at an authority question. The owner has now answered it, so this
milestone lands the **request** half of the command: the frontend asks, the
daemon performs, and the frontend reports the daemon's answer.

## 2. What the milestone must do

### 2.1 The floor — no false report, in the new shape

The TUI's message must never claim a compaction that the daemon did not
perform. Concretely:

- the request is sent **before** any message claims an effect;
- the message reports the daemon's answer, including a refusal as a refusal;
- and the compaction itself is reported by the daemon's own
  `AppEvent::CompactionTriggered`, not by the frontend.

### 2.2 The daemon performs the compaction

- The request names a session and nothing else.
- The daemon accepts it and performs the compaction through
  `compact_context`, the canonical engine, with the configured policy.
- The compaction happens at the daemon's next compaction opportunity for that
  session: inside the turn if a turn is live, at the start of the next turn
  if the session is idle.

### 2.3 What must not happen

- **No second compaction engine.** The forced compaction runs
  `compact_context`; the automatic path's threshold behaviour, tracker and
  checkpoint install are untouched.
- **No separate manual policy.** A forced compaction applies the configured
  `auto` / `prune` / `mode` / `policy` exactly as the automatic path applies
  it.
- **No refusal the owner did not ask for.** The daemon refuses only what is
  factually unanswerable: an unknown session, or no session named.
- **No change to the durable transcript.** Compaction compacts the model's
  context; it does not rewrite the user's history.

## 3. Out of scope — explicit non-goals

- **The compaction count's scope.** `session_state.compaction_count` remains
  the TUI's observed total, not a session's (C001 closure §10.2). Making it
  session-scoped changes what the `/context` panel renders and is a separate
  milestone.
- **A compaction the frontend performs itself.** Nothing in this milestone
  reduces a history in the frontend.
- **The durable transcript's compaction.** No message row is rewritten,
  deleted or re-summarized.
- **The desktop's own surface.** The desktop's palette routes a daemon-served
  form to the daemon by itself, so the desktop gains `/compact` as a
  consequence — this milestone does not add a desktop-specific compaction
  surface.
- **The automatic compaction policy.** The threshold, the tracker and the
  automatic trigger are untouched.

## 4. Work packages

### WP-A — the protocol

`crates/codegg-protocol/src/core.rs`:

- `CoreRequest::SessionCompact { session_id: String }` — one request, naming
  the session it compacts and nothing else.
- `CoreResponse::SessionCompact { session_id: String, outcome: ... }` — the
  daemon's answer to the request.

The answer carries the session it names, so a frontend cannot report a
compaction against a session it did not ask for.

### WP-B — the catalog and the frontend action set

`crates/codegg-core/src/command_catalog.rs`:

- `compact` becomes `CommandKind::DaemonEffect` with
  `capability: Some(CommandCapability::CommandExecute)`.
- The catalog version is **bumped** (3 → 4), so a client built against the
  previous classification is refused with the stale-catalog refusal rather
  than silently mis-executing.

`crates/codegg-core/src/frontend_action.rs`:

- `FrontendAction::CompactSession` is **removed** — the enum variant, its
  wire name, its entry in `ALL`, its declaration, and its form→action
  mapping. A frontend action is a non-mutating **local** effect; asking the
  daemon to change daemon-owned state is not one, and the guard's set is
  defined over the catalog's frontend-effect forms.

### WP-C — the daemon

- `src/core/daemon_commands.rs`: the effect table gains the row
  `("compact", "")`, which builds the inner request
  `{"type": "session_compact", "session_id": …}`. A call that names no
  session is `CommandEffect::NeedsSession` — the daemon resolves the project
  and the workspace from its own durable binding, but the session is the
  caller's selection.
- `src/core/daemon_family.rs`: `SessionCompact` routes to the sessions
  family.
- `src/core/daemon_sessions.rs`: the handler resolves the session from the
  request, records the request against the session's runtime, and answers
  with the daemon's outcome.

### WP-D — the request reaches the engine

- `src/core/session_runtime.rs`: `SessionRuntime` carries the pending
  compaction request.
- `src/core/daemon_turns.rs`: the turn's input carries it, so the live turn
  and the next turn read the same request.
- `src/agent/turn_runtime.rs` → `src/agent/agent_loop_factory.rs` →
  `src/agent/coordinator.rs`: the request is threaded into the loop's
  services.
- `src/agent/context_runtime.rs`: `compact_if_needed` honours the request —
  it bypasses the threshold gate for that one call, and the request is spent
  when the daemon **attempts** the compaction, not when it reads it.

### WP-E — the engine honours a forced compaction

`src/context/compaction.rs`:

- `ContextCompactionRequest` carries `force: bool`. When it is set, the
  engine's own threshold gate (`src/context/compaction.rs:998`) does not
  short-circuit the call. The policy is otherwise untouched: `auto` still
  selects the provider-backed reduction or the deterministic one, `prune`
  still prunes, and the configured `mode` and `policy` still decide the
  shape.
- The automatic path passes `force: false`; nothing about it changes.

### WP-F — the TUI

- `src/tui/commands/tasks.rs`: `handle_compact_session` sends the daemon's
  command and reports the daemon's answer. It no longer reports that
  compaction "is automatic" — that is no longer the whole truth.
- `src/tui/app/mod.rs`: the `/compact` arm reaches that handler.
- `src/tui/app/commands.rs` / `src/tui/runtime/command_dispatch.rs`: the
  asynchronous answer comes back as its own command, so the toast is raised
  when the daemon has answered rather than when the request was sent.

## 5. Required tests

- **The protocol.** The variant round-trips over the wire shape, and the
  answer names the session it names.
- **The catalog.** `compact` is a daemon effect with the
  `command.execute` capability, the catalog version moved, and no frontend
  action remains for it.
- **The effect table.** The `compact` row builds the inner request, a call
  with no session is `NeedsSession`, and `serves_form` reports the form as
  daemon-served.
- **The handler.** A request for a known session is accepted and the
  session's runtime carries it; a request for an unknown session is refused.
- **The engine.** A forced compaction runs the canonical engine and reduces
  the history, where the automatic path would not have; and the automatic
  path is unchanged.
- **The TUI.** The message reports the daemon's answer; a refusal is
  reported as a refusal; and no message claims a compaction the daemon did
  not perform.

## 6. Required verification commands

```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets --locked
rustup run 1.99.0 cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --locked --test core_daemon -- compact
cargo test --locked --test agent_loop -- compact
cargo test --locked --test tui_render -- compact
cargo test --locked -p codegg --lib -- compact
python3 scripts/check_frontend_action_set.py
python3 scripts/check_command_catalog_ownership.py
python3 scripts/check_adr_integrity.py
scripts/verify.sh quick
```

## 7. Acceptance criteria

1. A frontend's compaction request reaches the daemon, and the daemon
   performs the compaction through `compact_context`.
2. The daemon refuses only what is factually unanswerable: an unknown
   session, or no session named.
3. The frontend's message reports the daemon's answer, and never claims a
   compaction the daemon did not perform.
4. The automatic compaction path is unchanged.
5. No second compaction engine and no separate manual policy exist.
6. The catalog version is bumped, and the frontend action set no longer
   carries `CompactSession`.
7. Every surface that reaches `/compact` — the TUI's palette entry and the
   daemon's catalog form — reaches the daemon.

## 8. Closure evidence required

A closure record at `plans/closure/tui-compact-command-corrective/002-status.md`
following the eleven-section shape of closures 020, 021 and 022, carrying:

- the requirement-to-evidence matrix;
- the anchors for every production change;
- the real test counts;
- the hosted CI receipts;
- and the unresolved findings, including the ones this milestone does not
  close — the count's scope (C001 §10.2), the request's bound to the
  daemon's runtime (ADR-0060 §7), and the fact that the compaction happens
  at the next context build rather than inside the request.
