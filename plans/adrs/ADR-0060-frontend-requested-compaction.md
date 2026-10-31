# ADR-0060: A Frontend May Ask the Daemon to Compact, and the Daemon Does Not Refuse

Status: accepted — 2026-10-31 by the project maintainer

Date: 2026-10-31

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md`
- `architecture/agent.md`
- `architecture/command.md`
- `architecture/core.md`

Affected subsystem roadmaps:

- `plans/subsystems/tui-compact-command-corrective-roadmap.md` (gates C002)

## Context

### 1. The command used to report an effect it never caused

`/compact` toasted **"Compaction triggered - reducing context"** and sent
nothing. Milestone C001 closed that defect on 2026-10-31: the command now
reports what compaction is and what this TUI has actually observed, and it
sends no request
(`plans/closure/tui-compact-command-corrective/001-status.md`).

### 2. C001 stopped at an authority question that was the owner's

C001's closure §10.1 recorded the question and did not answer it:

> may a frontend ask the daemon to compact now, and does the daemon retain
> the right to refuse?

The same section proposed the variant, its shape, and what the daemon would
do with it, and left the decision open. C001's plan §4 made that a **stop
condition**: "No new protocol variant without the owner's decision."

**The owner's answer, 2026-10-31:**

> ideally no, if a frontend asks for compaction, the daemon should do the
> compaction and not refuse.

So the daemon **does not refuse**. The authority question is now decided, and
this record decides the shape.

### 3. Compaction is a turn-scoped operation, not a transcript rewrite

The daemon's compaction machinery is:

- `compact_if_needed` (`src/agent/context_runtime.rs:865`) — the turn-lifecycle
  entry point. It reads the session's execution policy, takes the pending
  request and the threshold at `src/agent/context_runtime.rs:900`, and runs the
  engine.
- `compact_context` (`src/context/compaction.rs:954`) — the canonical engine,
  owned by `src/context/compaction.rs`. It applies the configured policy
  (`auto`, `prune`, `mode`, `policy`) and produces the replacement history.
- the durable rollover (`src/agent/context_runtime.rs:1136` onward) — the
  transactional checkpoint install, and finally `publish_compaction_tail`,
  which publishes `AppEvent::CompactionTriggered`
  (`src/agent/context_runtime.rs:1664`).

The history it compacts is the **live turn's** history, carried on the turn
(`TurnRunInput.messages_dto`, `src/agent/turn_runtime.rs:99`). The user's
durable transcript is not rewritten by compaction: the compaction is carried
across turns by an installed continuation checkpoint, and the transcript the
user reads stays whole.

This is the fact that decides the shape: **there is no session-wide context
sitting on the daemon waiting to be compacted.** The context is built at turn
start from the frontend's transcript. So "compact now" can only mean "the
context the daemon builds next is a compacted one".

### 4. The catalog misdeclares the command because the daemon had no request

**At the time this decision was taken**, `compact` was declared a frontend
effect with no capability (the `compact` descriptor in
`crates/codegg-core/src/command_catalog.rs`), and the comment said why: "The
daemon has no compaction request, so the form is a frontend effect and the
declared action names it." The declared action was
`FrontendAction::CompactSession` (`crates/codegg-core/src/frontend_action.rs`,
declared in the frontend-action table).

That premise is now false. Once the daemon has a compaction request, the
command's effect is the daemon's, and the declared frontend action — which
the frontend-action guard defines as a **non-mutating local** effect
(`scripts/check_frontend_action_set.py`) — no longer describes it.

## Decision drivers

1. **The daemon owns compaction.** The threshold, the tracker, the policy and
   the checkpoint are the daemon's. A frontend that compacts its own history
   would be a second compaction engine.
2. **A frontend must never narrate an effect it did not cause.** C001 closed
   the false report; this must not reopen it in a new shape.
3. **The daemon does not refuse.** The owner's answer, above. The daemon
   performs the compaction.
4. **One policy.** A forced compaction applies the *configured* policy — the
   same `auto`, `prune`, `mode` and `policy` the automatic path applies. There
   is no separate manual policy, and no second engine.
5. **The daemon keeps its authority over *when* compaction fires.** A request
   is an assertion that a compaction is wanted now; it does not let a
   frontend choose the policy, the model, or the checkpoint.

## Options

### Option A — the daemon performs the compaction on request (accepted)

`compact` becomes a daemon effect. The frontend sends the daemon a compaction
request for its session; the daemon accepts it and performs the compaction
through its own path at its next compaction opportunity for that session.

### Option B — the daemon refuses a frontend's request (rejected)

Rejected by the owner's answer, and by driver 3. It would also make `/compact`
a no-op again in a new shape: a command that is answered with a refusal is a
command the user cannot use.

### Option C — the frontend keeps the command and only reports (rejected)

This is C001's Shape B. It was correct while the question was open, and it is
superseded now that the question is answered: it leaves the user with no way
to ask for a compaction at all.

## Decision

### 1. `compact` is a daemon effect

The catalog declares `compact` as a daemon effect under the
`command.execute` capability, its catalog version is bumped so a client that
was built against the previous classification is refused, and
`FrontendAction::CompactSession` is **removed** from the declared frontend
action set — a frontend action that asks the daemon to change daemon-owned
state is not a frontend effect, and the guard's set is defined as the
non-mutating local effects.

### 2. One new request variant

`CoreRequest::SessionCompact { session_id }` — one request, naming the
session it compacts and nothing else. The daemon resolves the session from
that name; a frontend never names a turn, a model, a policy, or a path.

### 3. The daemon does not refuse

The daemon accepts the request and performs the compaction. The **only**
refusals are factual, and both are about the request being unanswerable
rather than about policy:

- the named session does not exist; and
- the caller named no session at all (the form needs one — the daemon
  resolves the project and the workspace from its own durable binding, but
  the session is the caller's selection).

There is **no** refusal for "compaction is disabled", for "the session is
mid-turn", for "no summarization model is configured", or for "the context is
below the threshold". A mid-turn request is honoured at that turn's next
compaction opportunity. The
configured policy — including `auto: false`, which selects the deterministic
reduction rather than the provider-backed one — is applied as the automatic
path applies it, and a policy that produces no reduction produces no
reduction; that is the policy's answer, not a refusal.

### 4. The daemon performs the compaction through its own path

The request marks the session, and the daemon compacts at its **next
compaction opportunity** for that session:

- if a turn is live, that is the live turn's next compaction opportunity —
  before its next provider call, **if the turn continues**; and
- if the session is idle, that is the start of the next turn.

That is the only place the compaction can happen, for the reason in Context
§3: the context is built when a turn starts.

### 5. The engine honours a forced compaction — through the same engine

The forced compaction runs `compact_context` — the canonical engine — with
the threshold gates bypassed **for that one call** and the configured policy
otherwise untouched. The automatic path is not modified: its threshold
behaviour, its tracker and its checkpoint install are exactly what they are.

### 6. The outcome is the daemon's report, not the frontend's

The daemon reports the compaction it performed through its own
`AppEvent::CompactionTriggered` (`src/agent/context_runtime.rs:1665`), which
the TUI already handles and counts
(`src/tui/runtime/app_events.rs:525`). The frontend's own message reports
only what the daemon answered to the request. A frontend that cannot reach
the daemon says so; it never says the compaction happened.

### 7. The request is bound to the daemon's session runtime

The request is held in the daemon's in-memory session runtime, and a daemon
restart drops it. This is recorded, not hidden: the request is a request to
*this* daemon, and a restarted daemon has no pending request to honour. It is
also why the request is not written to durable session storage — a durable
pending-compaction row would outlive the daemon that has to honour it and
would be honoured by a daemon the user never asked.

## Consequences

### Positive

- `/compact` becomes a command that **does** something, on the TUI and on the
  desktop, and it does it through the daemon's own authority.
- The daemon's answer is the only thing the frontend reports, so the false
  report C001 closed cannot come back in a new shape.
- The desktop's palette routes a daemon-served form to the daemon by itself
  (`apps/desktop/src/commands/CommandPalette.tsx:194`), so the desktop's
  honest "the desktop does not implement it yet" notice for `/compact`
  becomes unnecessary for this command — the desktop runs it.

### Negative

- The compaction happens at the next context build, not inside the request.
  A user who asks while the session is idle sees the request accepted and the
  compaction performed when the next turn starts. The frontend's message says
  exactly that.
- The catalog's version is bumped, so a client built against the previous
  classification is refused with the stale-catalog refusal, and a daemon
  built against the previous classification refuses the new form. This is the
  mechanism the catalog already uses for a reclassification.

### Neutral or deferred

- The compaction count the TUI keeps remains the TUI's **observed total**,
  not a session's. C001's closure §10.2 recorded it; this decision does not
  change it.
- Whether a forced compaction should bypass the configured `auto` flag is
  settled by §3 and §5: it does not bypass it. `auto` selects *how* the
  compaction reduces the history, not *whether* it may.

## Compatibility, security and migration

- **Wire.** `CoreRequest` gains one variant and `CoreResponse` gains its
  answer. The variant is `#[serde(tag = "type", rename_all = "snake_case")]`,
  so it is `{"type": "session_compact", "session_id": "..."}`.
- **Authorization.** The form is executed under `command.execute`, and the
  inner request is authorized on its own operation and capability — the
  daemon re-authorizes it, and `command.execute` is the authority to *run* a
  command, never a widening of what the command may touch. The request names
  a session and nothing else, so it cannot reach a path, a model or another
  session.
- **Migration.** The catalog version bump is the migration. A client that
  learned `/compact` as a frontend effect asks for the old classification and
  is refused with `stale_catalog`, which tells it to refetch; a client that
  learned the new one against an old daemon is refused the same way. Nothing
  is silently mis-executed in either direction.

## Required verification

- `python3 scripts/check_frontend_action_set.py` — the declared set is still
  closed over the catalog's frontend-effect forms, with `CompactSession`
  gone.
- `python3 scripts/check_command_catalog_ownership.py` — the catalog and the
  TUI's command surface are still the same set, and only the catalog
  executes.
- `cargo test --locked --test core_daemon -- c002` — the effect table's
  row, the family's routing, and the handler.
- `cargo test --locked --test agent_loop -- a_compaction_request_forces` —
  the request reaches the agent loop and forces a compaction the threshold
  would have kept.
- `cargo test --locked -p codegg --lib -- compact_command_tests` — the TUI's
  message reports the daemon's answer and never an effect it did not cause.
  (The TUI's compaction suite is a library test, not a `tui_render` member:
  `--test tui_render -- compact` selects zero tests and proves nothing.)
- `cargo test --locked -p codegg-protocol -- the_compaction_request` — the
  request and its answer round-trip over the wire shape.
