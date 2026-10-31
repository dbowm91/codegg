# TUI Compact Command Corrective — Milestone C002 — The Daemon Compacts on Request

Status: implemented

Repository baseline: `3c340c1ea` — `origin/main` with the M007 line and C001
landed.

Milestone:

- **C002** — the daemon compacts when a frontend asks.

Plan:

- `plans/implementation/tui-compact-command-corrective/002-the-daemon-compacts-on-request.md`

Decision:

- `plans/adrs/ADR-0060-frontend-requested-compaction.md` — accepted
  2026-10-31 by the project maintainer.

Roadmap:

- `plans/subsystems/tui-compact-command-corrective-roadmap.md`

## 1. Executive finding

C001 closed the false report: `/compact` stopped claiming a compaction it
never caused. It stopped there because the question behind the command — may
a frontend ask the daemon to compact, and does the daemon retain the right
to refuse? — was **the owner's**, and C001's plan made a new protocol
variant a stop condition until that question was answered.

The owner answered on 2026-10-31:

> ideally no, if a frontend asks for compaction, the daemon should do the
> compaction and not refuse.

**C002 carries that answer out.** `/compact` is now a **daemon effect**. The
TUI asks the daemon's own catalog command to compact the session the user is
looking at; the daemon accepts the request, performs the compaction through
the canonical engine (`compact_context`), and refuses only what is
factually unanswerable — a session it cannot resolve. The frontend reports
the daemon's answer, and the compaction itself is reported by the daemon's
own `AppEvent::CompactionTriggered`, which the TUI already counts.

Three properties hold on every path:

1. **The frontend never narrates an effect it did not cause.** The toast
   reports the daemon's answer; a refusal is reported as a refusal; a
   transport failure says the request never arrived.
2. **There is one compaction engine.** The forced compaction runs
   `compact_context` with the configured policy. The automatic path's
   threshold behaviour, tracker and checkpoint install are untouched.
3. **The daemon does not refuse.** No refusal exists for "compaction is
   disabled", "the session is mid-turn", or "the context is below the
   threshold" — those are the policy's answers, not refusals.

The one thing the daemon **cannot** do is compact outside a turn, and that is
a fact about the product rather than a policy choice: the context is built
when a turn starts (`TurnRunInput.messages_dto`,
`src/agent/turn_runtime.rs:91`), so a compaction request is honoured at the
daemon's **next compaction opportunity** for the session — the live turn's
next opportunity when one is live (and that turn continues), and otherwise
the start of the next turn. The frontend's message says exactly that.

## 2. Requirement-to-evidence matrix

| # | Requirement | Evidence |
|---|---|---|
| R1 | The frontend asks the daemon's own catalog command | `src/tui/commands/compaction.rs:17` sends `send_command(…, "compact", "", "")`; the catalog form is a daemon effect (`crates/codegg-core/src/command_catalog.rs:586`) and the daemon's effect table serves it (`src/core/daemon_commands.rs:266`) |
| R2 | The daemon accepts and performs the compaction | `src/core/daemon_sessions.rs:1202` records the request against the session's runtime (`src/core/session_runtime.rs:55`) and answers `CoreResponse::SessionCompact`; the loop honours it at `src/agent/context_runtime.rs:905` |
| R3 | The daemon refuses only what is factually unanswerable | The handler's only refusal is the unresolvable session; the daemon test proves an unknown session is refused and a known one is not (`tests/core_daemon/c002_daemon_compaction.inc`) |
| R4 | One engine, one policy | The forced compaction calls `compact_context` (`src/context/compaction.rs:954`) with `force` bypassing only the threshold gate (`:1004`); the automatic path passes `force: false` and is unchanged |
| R5 | The frontend reports the daemon's answer | `src/tui/commands/compaction.rs:81` renders the answer; the seven TUI members in `src/tui/app/compact_command_tests.rs` assert the request, the answer, the running-turn case, the refusal case, the no-session case, the catalog's classification, and that an answer about another session is not this session's compaction |
| R6 | The compaction itself is the daemon's report | `AppEvent::CompactionTriggered` is published by the daemon (`src/agent/context_runtime.rs`, `publish_compaction_tail`) and counted by the TUI (`src/tui/runtime/app_events.rs:525`) — unchanged by this milestone |
| R7 | The catalog's classification is truthful | `compact` is `CommandKind::DaemonEffect` under `CommandCapability::CommandExecute` (`crates/codegg-core/src/command_catalog.rs:586`), the catalog version is 4, and `FrontendAction::CompactSession` is **removed** from the declared frontend action set |
| R8 | The operation is audited under its own action | `session_compact` is a new audit action (`crates/codegg-core/src/audit.rs:144`) mapped to the `session_compact` operation (`crates/codegg-core/src/audit_instrumentation.rs:683`) — not `prompt_submit`, because no prompt was submitted — and the handler writes the row on whichever route the request arrived |

## 3. What was delivered

### 3.1 The protocol

One request variant and one answer
(`crates/codegg-protocol/src/core.rs:2726` and `:1387`):

```rust
SessionCompact { session_id: String },
```

```rust
SessionCompact { session_id: String, turn_active: bool },
```

The request names a session and nothing else: no turn, no model, no policy,
no path. The answer names the session it compacts and whether that session
had a live turn when the request was accepted — the fact the frontend needs
to say *when* the compaction will happen.

### 3.2 The daemon

- **The effect table** (`src/core/daemon_commands.rs:266`): the `compact` row
  builds `{"type": "session_compact", "session_id": …}` from the **scope's**
  session — the caller's selection — and a call that names none is
  `NeedsSession`.
- **The family** (`src/core/daemon_family.rs:170`): the request routes to the
  sessions family, so the daemon's own authorization seam sees it.
- **The handler** (`src/core/daemon_sessions.rs:1202`): resolves the session
  through the daemon's durable binding, records the request on that session's
  runtime, and answers. The runtime's flag is an
  `Arc<AtomicBool>` (`src/core/session_runtime.rs:55`), so a request that
  arrives **mid-turn** reaches the running loop.
- **The turn** (`src/core/daemon_turns.rs:992`): the turn's input carries the
  session's pending request, so a request that arrived while the session was
  idle is honoured by the next turn.
- **The authorization** (`crates/codegg-core/src/authorization/policy.rs`):
  `session_compact` is `via_session` under `agent.invoke` — the same class as
  a steer — and the daemon resolves the session's project through the
  session id (`src/core/daemon.rs`, `session_id_for_request`).

### 3.3 The engine

`ContextCompactionRequest` gained `force: bool`
(`src/context/compaction.rs:954`). When it is set, the engine's own
threshold gate does not short-circuit the call (`:1004`) — and **nothing
else changes**: `auto` still selects the provider-backed reduction or the
deterministic one, `prune` still prunes, and the configured `mode` and
`policy` still decide the shape. The automatic path passes `force: false`
and is byte-for-byte the path it was.

### 3.4 The TUI

`src/tui/commands/compaction.rs` is a new module holding the command's
three pieces:

- `handle_compact_session` (`:17`) — the ask. It refuses locally only when
  there is no session to compact or no core connection to ask;
- `apply_compact_session_finished` (`:62`) — the answer, applied on the UI
  thread when the daemon has answered rather than when the request was sent;
- `compact_answer_message` (`:81`) — the renderer, pure, so the truth it
  states is testable without a runtime.

The palette's builtin arm reaches the handler
(`src/tui/app/mod.rs:6047`), and the completion is a first-class command
(`src/tui/app/commands.rs:297`, `src/tui/runtime/command_dispatch.rs:840`).

The old `TuiCommand::CompactSession` unit variant — which **nothing in the
tree sent** (C001's closure §10.1 recorded that history) — is gone, replaced
by the completion variant that carries the daemon's answer.

### 3.5 The catalog and the frontend action set

`compact` is now a daemon effect under `command.execute`
(`crates/codegg-core/src/command_catalog.rs:586`), the catalog version moved
from 3 to 4, and `FrontendAction::CompactSession` is **removed** — the
variant, its wire name, its entry in `ALL`, its declaration, and its
form→action mapping.

The reason is the guard's own definition: a frontend action is a
**non-mutating local** effect (`scripts/check_frontend_action_set.py`), and
asking the daemon to change daemon-owned state is not one.

### 3.6 The audit

A compaction request is its **own** audit action, `session_compact`
(`crates/codegg-core/src/audit.rs:144`,
`crates/codegg-core/src/audit_instrumentation.rs:683`). It is deliberately
**not** `prompt_submit`: no prompt was submitted, and labelling the request
as one would put a false statement in the operator's log — the same class of
defect this line exists to close. The audited event is the **request**; the
compaction it causes is reported by the daemon's own
`AppEvent::CompactionTriggered`, not by an audit row. The metadata is bounded
to two keys, `session.id` and `decision.outcome`.

The row is written by the **handler**, not by the generic control-plane seam,
because the catalog's `compact` command reaches the handler without passing
that seam. The seam therefore skips this request, exactly as it skips the
creation operations that mint their identity in the handler. One request
leaves one row, on the direct route and on the command route alike — the
route a frontend actually takes.

### 3.7 What did **not** move

- The automatic compaction path: its threshold, its tracker, its policy and
  its checkpoint install are untouched.
- The durable transcript: compaction reduces the model's context; it does not
  rewrite the user's history.
- The compaction count's scope: `session_state.compaction_count` remains the
  TUI's **observed total**, not a session's (C001 closure §10.2). The count
  the TUI reports is the count it keeps.
- The desktop's own surface: its palette routes a daemon-served form to the
  daemon by itself (`apps/desktop/src/commands/CommandPalette.tsx:194`), so
  the desktop gains `/compact` as a **consequence** — no desktop-specific
  compaction surface was added.

## 4. Verification executed

### 4.1 The plan's commands (§6 of the plan)

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets --locked` | exit 0 |
| `rustup run 1.99.0 cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 (CI's toolchain) |
| `cargo test --locked -p codegg-core` | **991 passed, 0 failed** (the crate's library target; its auxiliary targets add 4, 7, 11 and 1) |
| `cargo test --locked --test core_daemon` | **536 passed, 0 failed** |
| `cargo test --locked --test agent_loop -- compact` | **74 passed, 0 failed** |
| `cargo test --locked --test tui_render` | **1110 passed, 0 failed** |
| `cargo test --locked -p codegg --lib -- compact` | **67 passed, 0 failed** |
| `cargo test --locked -p codegg-protocol` | **333 passed, 0 failed** |
| `cargo test --locked --test tui_render -- compact` | **0 tests selected** — that family has no compaction-named member; the TUI's compaction members live in the library target, and the family's 1110 members all pass (§4.2) |
| `python3 scripts/check_frontend_action_set.py` | `frontend-action set: clean` |
| `python3 scripts/check_command_catalog_ownership.py` | `command catalog ownership: clean` |
| `python3 scripts/check_authorization_matrix.py` | `All authorization matrix invariants verified.` |
| `python3 scripts/check_adr_integrity.py` | `ADR integrity fixtures passed; active ADR IDs are unique and local paths resolve` |
| `python3 scripts/check_audit_coverage.py` | `All audit coverage invariants verified.` |
| `python3 scripts/check_audit_invariants.py` | `All audit foundation invariants verified.` |
| `python3 scripts/check_test_assertion_quality.py` | `check_test_assertion_quality: all checks pass` |
| `python3 scripts/check_test_duplication.py` | `check_test_duplication: all checks pass` |
| `python3 scripts/check_comment_budget.py` | `comment budget guard passed` (after the sanctioned `--bank`) |
| `XDG_CACHE_HOME="$(mktemp -d)" cargo nextest run --workspace --locked --profile ci --no-fail-fast` | **16758 tests run, 16758 passed, 0 failed, 10 skipped** — the same sweep CI runs |
| `scripts/verify.sh quick` | see §4.4 |

**One failure was observed and explained.** In the first full `core_daemon`
run, `identity_m003_daemon_authorization::user_prompt_queue_advance_attempts_a_due_entry_on_an_idle_session`
failed. It passed in isolation and in the family's re-run, and the re-run
reported **535 passed, 0 failed**. It is recorded here rather than smoothed
away: the family ran under a machine load above 20, and the member is
time-sensitive. The three other failures in that run were **real** and are
fixed: the catalog's form counts moved by one form (`compact` left the
frontend effects), and `tests/core_daemon/command_catalog_m007d.inc` now
records 125 daemon-effect forms and 42 frontend-effect forms.

### 4.2 The new tests

| Member | What it proves |
|---|---|
| `src/tui/app/compact_command_tests.rs::the_compact_command_asks_the_daemon_to_compact_the_session` | the request that reaches the daemon is a `CommandExecute` for the catalog's own `compact`, and it names the session |
| `…::the_compact_command_reports_the_daemons_answer` | the toast says "Compaction requested", never "Compaction triggered", names when the daemon compacts, and reports the count the TUI keeps |
| `…::the_compact_command_names_the_running_turn_when_the_daemon_says_so` | a live turn is named as such |
| `…::a_refused_compaction_request_is_reported_as_a_refusal` | a refusal reads as a refusal, never as an accepted request |
| `…::the_compact_command_says_so_when_no_session_is_attached` | the command says why it did nothing instead of asking |
| `…::the_compact_form_is_a_daemon_effect_the_daemon_serves` | the catalog classifies the form as a daemon effect under `command.execute`, no frontend action serves it, and the TUI's route reaches the daemon |
| `tests/core_daemon/c002_daemon_compaction.inc::the_daemon_accepts_a_compaction_request_and_records_it` | the daemon accepts the request for a resolvable session and records it on that session's runtime |
| `…::the_daemon_refuses_a_compaction_request_for_an_unknown_session` | the only refusal is factual, and it is named |
| `…::the_compact_command_reaches_the_daemons_compaction_handler` | the catalog's own command reaches the same handler — the frontend reaches the daemon **through** the catalog |
| `…::the_compaction_request_is_audited_under_its_own_action` | the request leaves one `session_compact` row on the route a frontend takes, naming the session and the principal |
| `src/core/daemon_commands.rs::the_compact_row_asks_for_the_callers_session` | the row names the caller's session, and a call that names none is `NeedsSession` |
| `src/context/compaction.rs::a_forced_compaction_reduces_a_history_the_threshold_would_keep` | the forced call compacts where the automatic call does not, and the automatic call is unchanged |
| `src/tui/app/compact_command_tests.rs::an_answer_about_another_session_is_not_reported_as_this_sessions_compaction` | the daemon's answer names the session it compacts, and an answer about another session is refused rather than reported as this session's compaction |
| `tests/agent_loop/agent_loop_harness.inc::a_compaction_request_forces_a_compaction_the_threshold_would_keep` | the request reaches the **agent loop**: with the flag bound, a six-message history the threshold would keep is compacted; without it, the same history is untouched; and the flag is spent at the attempt |
| `crates/codegg-protocol/src/core.rs::the_compaction_request_and_answer_round_trip` | the request and its answer round-trip over the wire shape the daemon's effect table builds |

### 4.3 The guards that bite this change

- `scripts/check_frontend_action_set.py` — the declared set is still closed
  over the catalog's frontend-effect forms, with `CompactSession` gone.
- `scripts/check_command_catalog_ownership.py` — the catalog and the TUI's
  command surface are still the same set, and only the catalog executes.
- `scripts/check_authorization_matrix.py` — the new operation is classified,
  and its mapping names a known action.
- `scripts/check_audit_coverage.py` — the new action has a coverage row and
  a live operation mapping.
- `scripts/check_comment_budget.py` — the corpus moved by the delta and the
  manifest was banked by the sanctioned mechanism
  (`python3 scripts/check_comment_budget.py --bank`); the manifest was **not**
  hand-edited.
- `scripts/check_adr_integrity.py` — ADR-0060's pattern, uniqueness and
  path references resolve.

### 4.4 The hosted receipts

See §4.4 of this record's final commit: the hosted CI receipts for the
merge candidate are recorded there.

### 4.5 The daemon's automatic compaction still fires

The forced compaction is a **new** call shape, not a change to the existing
one: `compact_if_needed` reads the request, and with no request the gate is
the threshold check it always was. The engine's own test proves the
automatic path's outcome is unchanged for the same history.

## 5. Invariant review

| Invariant | Holds because |
|---|---|
| The daemon owns compaction | the request names a session; the policy, the model, the budget and the checkpoint are the daemon's. The frontend cannot select any of them |
| One compaction engine | the forced call runs `compact_context`; the automatic call runs the same function with `force: false` |
| The frontend reports what happened | the toast is rendered from the daemon's answer; the compaction itself is counted from `AppEvent::CompactionTriggered` |
| No silent refusal | the daemon's only refusal is the unresolvable session, and it is reported with the daemon's reason |
| The catalog is the single definition | the TUI asks for the catalog's `compact`; the desktop's palette routes the same form to the same command |
| The action set is finite and closed | `FrontendAction::CompactSession` is removed rather than repurposed, so the declared set holds only local effects |
| The durable transcript is intact | compaction reduces the model's context; it never rewrites the user's messages |
| The request cannot widen authority | the request names one session and no path; the daemon resolves that session's own binding, and the operation is `via_session` under `agent.invoke` |

## 6. Failure and recovery

| Failure | Behaviour |
|---|---|
| The daemon is unreachable | the toast says the request did not reach the daemon, and names the transport error. Nothing claims a compaction |
| The daemon refuses (unknown session) | the toast says the daemon refused, with the daemon's code and message |
| The daemon's answer is not understood | the toast says so. It never falls back to a claim of its own |
| The command's effect fails before running | the toast reports the failure summary, distinguished from a refusal by the outcome status |
| The daemon restarts after the request | the request is bound to the daemon's in-memory session runtime and is dropped by the restart (ADR-0060 §7). The session is unchanged, and the user may ask again |
| The turn is cancelled before the compaction opportunity | the request is taken at the opportunity, so a cancellation **before** it leaves the request pending for the next turn |

## 7. Migration and compatibility

- **Wire.** `CoreRequest` and `CoreResponse` gain one variant each. The
  request is `{"type": "session_compact", "session_id": "…"}`.
- **Catalog.** The version moves from 3 to 4. A client built against the
  previous classification asks for the old one and is refused with the
  stale-catalog refusal, which tells it to refetch; a client built against
  the new one against an old daemon is refused the same way. **Nothing is
  silently mis-executed in either direction** — the catalog version is the
  migration, and it is the mechanism ADR-0052 established for a
  reclassification.
- **Audit.** The action list grows from 30 to 31. A reader that meets an
  unknown action still degrades to `Unknown` rather than failing a page.

## 8. Security

- The request names a session and nothing else, so it cannot reach a path, a
  model, a turn or another session.
- The operation is `via_session` under `agent.invoke` — the same authority
  class as a steer — and the daemon resolves the session's project from the
  **session's own durable binding**, never from a caller-supplied path.
- The audit metadata is bounded to two keys; no path, file name, model or turn
  identity enters it.
- The compaction runs the daemon's own engine with the daemon's own policy.
  A frontend cannot widen the policy, choose the summarization model, or
  force a budget.

## 9. Documentation and operations

| Artefact | Change |
|---|---|
| `plans/adrs/ADR-0060-frontend-requested-compaction.md` | **new** — the owner's decision, its drivers, the options, and the consequences |
| `plans/implementation/tui-compact-command-corrective/002-the-daemon-compacts-on-request.md` | **new** — the plan this milestone executed |
| `plans/closure/tui-compact-command-corrective/002-status.md` | **new** — this record |
| `plans/subsystems/tui-compact-command-corrective-roadmap.md` | C002 row; the closure boundary; the discharge of the "no new protocol variant without the owner's decision" rule |
| `plans/registry.md` | the subsystem row; the C002 dependency-ready row; the source-verification entry |
| `architecture/audit.md` | the action list (31) and a dated re-verification of the taxonomy and the operation classification |
| `docs/comment-budget.toml` | banked by the sanctioned mechanism; the corpus moved by the delta |

**Operations.** `/compact` now asks the daemon to compact. A user who wants
the compaction to happen **now** should send a message after it: the
compaction is performed at the next context build, which is the start of
that turn. This is the product fact recorded in ADR-0060 §4 and in this
record's §1.

## 10. Unresolved findings

### 10.1 The compaction happens at the next context build, not inside the request (medium, **recorded**)

A request that arrives while the session is idle is honoured at the **start
of the next turn**, not immediately. The reason is structural: the context
is built when a turn starts (`TurnRunInput.messages_dto`), so there is no
context to compact before one exists.

A request that arrives **mid-turn** is honoured at that turn's next
compaction opportunity — which exists only if the turn continues. A turn
whose provider call ends it has no further opportunity, and the compaction
falls to the next turn. The frontend's message names both cases rather than
predicting that the live turn continues: it says the compaction happens "at
the running turn's next compaction opportunity, or at the start of the next
turn if that turn ends first". The daemon's own `CompactionTriggered`
reports the compaction when it happens.

Compacting *outside* a turn would mean rebuilding the turn's context outside
a turn — a second compaction path, which ADR-0060 §2 forbids.

### 10.2 The request is bound to the daemon's in-memory runtime (low, **recorded**)

The pending request lives on the daemon's session runtime. A daemon restart
drops it. A durable pending-compaction row would outlive the daemon that has
to honour it and would be honoured by a daemon the user never asked, so the
in-memory binding is the honest one (ADR-0060 §7).

### 10.3 The count the TUI keeps is the TUI's observed total, not a session's (medium, **recorded**)

Unchanged from C001's closure §10.2. The message the command reports names
the count without claiming a session scope for it.

### 10.4 The daemon's `CompactionTriggered` does not name the requester (low, **recorded**)

The event carries the session, the tokens before and the tokens after — it
does not say who asked. The audit trail carries the request separately
(`session_compact`), which is the record of the requester.

### 10.5 The request is taken even when the attempt cannot complete (low, **recorded**)

Taking the request and spending it are one atomic step
(`src/agent/context_runtime.rs:905`), so exactly one compaction opportunity can
honour it and a request that arrives later is a new one that survives. The
cost of that atomicity is that a request is **taken** even when the attempt it
was taken for cannot finish: a plugin's `SessionCompacting` hook that blocks
the compaction, or an engine that reports `InsufficientCapacity` or
`InvalidHistoryOrBudget`, spends the request without a compaction.

This is recorded rather than repaired. Re-arming the flag on a blocked attempt
would make one request span several attempts, and the flag is a boolean rather
than a counter, so a second request arriving in that window would be merged
into the first. One request, one attempt, is the cleaner rule — and the
attempt that failed was still an attempt, so the daemon's "does not refuse"
decision is not what is in question here.

### 10.6 The desktop's own surface was not exercised by a hosted test (low, **recorded**)

The desktop gains `/compact` by consequence: its palette routes a
daemon-served form to the daemon
(`apps/desktop/src/commands/CommandPalette.tsx:194`). No desktop code
changed, and the desktop's own test suite has no `/compact` case to update
— its previous "the desktop does not implement it yet" notice was reachable
only for a frontend effect, and `/compact` is no longer one.

## 11. Roadmap disposition

The **TUI compact command corrective** line is **closed**, with both
milestones closed on 2026-10-31:

- **C001** — the false report is closed, and the question it recorded is
  answered.
- **C002** — the owner's answer is implemented: the daemon compacts on a
  frontend's request and does not refuse.

The line's own rule — *"No new protocol variant without the owner's
decision"* — is discharged by ADR-0060, and the variant it added is the one
C001's closure proposed. The roadmap's §5 non-goals all hold: the daemon's
compaction policy is unchanged, the TUI's rendering of
`AppEvent::CompactionTriggered` is unchanged, and the protocol variant
arrived with the owner's decision attached.
