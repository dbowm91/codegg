# Context Continuity and Multi-Compaction Coherence — Post-Closure Corrective Addendum

Status: closed (C001 closed 2026-10-26; opened from a post-closure review of `origin/main`
`957fcbf00`)

Repository baseline reviewed: `957fcbf00e17bf108429f0d75dcbf760736ae511`
(`origin/main`; the reviewing worktree `feat/auto-20261026-7c94ea67` was already
at this commit, so its rebase was a no-op)

Canonical long-term references:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Related ADRs:

- **ADR-0054 (accepted 2026-10-26)** — the daemon owns the provider-visible
  sequence across a turn boundary. This addendum is that record's
  implementation line. `plans/adrs/ADR-0054-daemon-owned-provider-visible-history-across-turn-boundaries.md`
- ADR-0003 (accepted) — long-horizon work state and context epochs. Bound by it;
  not revisited.
- ADR-0051 (accepted) — durable delegated run transcript and continuation
  authority. Not revisited.

Predecessor work (closed; **not reopened** by this addendum):

- `plans/subsystems/context-continuity-compaction-roadmap.md` — M001–M004 closed
  (`plans/closure/context-continuity-compaction/001-status.md` …
  `004-status.md`)
- `architecture/context-compaction-ownership.md` — the ownership contract this
  addendum restores compliance with

## 1. What this line is

A **corrective** line for one defect: **a compaction does not survive the turn
boundary**, so the turn after a long turn carries the old turn's material
twice, and the legitimately new material is buried between the two copies.

### 1.1 The defect, exactly

The frontend owns its transcript and re-sends all of it every turn.
`App::build_provider_context` (`src/tui/app/mod.rs:3382`) renders the whole
in-memory transcript into provider messages — User/Assistant/Tool rows only,
never a `Message::System` — and `dispatch_turn_submit_request`
(`src/tui/app/mod.rs:3113`) attaches that list to `CoreRequest::TurnSubmit`.
The daemon forwards it unchanged (`src/core/daemon_turns.rs:312`, `:985`;
`src/agent/turn_runtime.rs:1074`) and does **not** re-read durable session
history to build the agent's request.

The daemon's compaction replaces only the daemon's **own** copy of the message
sequence (`src/agent/context_runtime.rs:827` `compact_if_needed` →
`src/context/compaction.rs:950` `compact_context`), and that copy is discarded
at the turn boundary. The TUI transcript is never rewritten; the only reaction
to a compaction is a toast (`src/tui/runtime/app_events.rs:525`).

**Therefore a compaction has no durable effect on what the next turn sends.**
Turn N+1 re-sends turn N's full pre-compaction body and compacts again.

On top of that, `inject_installed_continuation_for_turn`
(`src/agent/context_runtime.rs:773`, called from `src/agent/loop.rs:1330`)
re-injects the installed checkpoint's bounded projection at the start of every
subsequent turn, because `strip_prior_frames`
(`src/context/rollover.rs:653`) removes only `Message::System` rows that are
CodeGG-owned frames (`src/context/compaction.rs:2478`) and the frontend's
transcript contains none.

Turn N+1's request therefore carries turn N's material **twice** — raw from the
frontend transcript, distilled from the checkpoint — with the new user message
between them. The distilled copy restates the goal, current task, constraints,
**decisions**, commands, tests, errors and next steps
(`src/context/rollover.rs:507`); a pending question or a permission verdict is
a decision, so questions and permissions the agent already sent come back
alongside the raw copy of themselves.

### 1.2 The reported symptom, mapped to the defect

| Reported symptom | Mechanism |
|---|---|
| "after one very long turn … another message to the LLM repeats sections of what was already shown" | the raw pre-compaction body is re-sent (the compaction was undone), and the checkpoint projection restates the same material |
| "this also applies for things like questions or permissions the agent has already sent" | those are decisions, and the projection carries decisions (`src/context/rollover.rs:507`) beside the raw decision record |
| "the legitimately new agent messages may get buried under repeated ones" | the projection is appended **after** the new user message on the default profile (`crates/codegg-core/src/model_profile/policy.rs:56`; default profile allows late system messages, `crates/codegg-core/src/model_profile/resolve.rs:160`) — M004 §6.5 required "before the current user turn" |
| "the loop may ultimately fail" | the re-sent body grows without bound, so a long trajectory eventually fails on provider capacity rather than on anything the agent did wrong |

### 1.3 The two contracts this violates

1. **`architecture/context-compaction-ownership.md`**: "durable session history
   is never deleted; only the in-memory/provider-visible sequence is replaced
   after verification." The replacement does not survive the turn boundary, so
   in practice the sequence is replaced and then immediately re-asserted.
2. **M004's own invariants**
   (`plans/implementation/context-continuity-compaction/004-transactional-rollover-and-multi-compaction-qualification.md` §4):
   "compaction state does not become a frontend-owned concern", and §6.5's
   requirement that the continuation projection be injected "before the current
   user turn".

Both are **pre-existing statements of intent that the implementation did not
meet**. This line restores them; it does not invent a new architecture.

## 2. Why the existing verification did not catch it

The existing continuity suites assert the **daemon-internal** invariant — "the
compacted list contains exactly one continuation frame" — and model a daemon
that rewrites its own history:

- `tests/agent_loop/context_continuity_m004.inc:443` carries the comment
  "Replace history only after verification (transactional seam)" and then does
  `messages = result.messages.clone()`, carrying the compacted list into the
  next epoch. That is exactly the assumption the shipped topology violates.
- `src/context/compaction.rs:3881`
  (`m002_repeated_compaction_does_not_stack_frames`) compiles the list five
  times and asserts one frame each time — but its input carries no installed
  checkpoint projection and no frontend transcript.
- `src/context/compaction.rs:3850`
  (`m002_frame_replacement_keeps_one_current_frame`) is the same shape.

**No test drives two `AgentLoop::run` turns on one session with a compaction
between them**, and `inject_installed_continuation_for_turn` has **zero** test
call sites: it is `pub(super)` (`src/agent/context_runtime.rs:773`), so it is
not reachable from `tests/` at all, and no in-crate unit test exists. The
broken seam is the TUI→daemon boundary, and no test spans it.

This is the line's central lesson: **a compaction engine can be exhaustively
tested at the unit level and still be ineffective, because its effect does not
cross the boundary the next turn is built from.**

## 3. Why this is a corrective and not a feature

There is no new user-facing capability here. The product already claims, in
`architecture/context-compaction-ownership.md`, that compaction replaces the
provider-visible sequence. It does not. The line closes that gap.

The **decision** the line needed — who owns the provider-visible sequence
across a turn boundary — was taken by the maintainer on 2026-10-26 and is
recorded in **ADR-0054**. This line implements that decision; it does not
reopen it.

## 4. Milestone decomposition

| Milestone | Scope | Dependency | Status |
|---|---|---|---|
| C001 | the provider-visible sequence survives the turn boundary: the daemon drops the superseded prefix it has already compacted, the continuation projection is placed before the current user turn on every profile, and the "current user message" read is immune to daemon-injected rows | ADR-0054 (accepted) | **blocked** — `plans/implementation/context-continuity-compaction-post-closure-corrective/001-provider-visible-sequence-survives-the-turn-boundary.md`; closure `plans/closure/context-continuity-compaction-post-closure-corrective/001-status.md` |

C001 is one milestone because it is one ownership boundary: the daemon's
provider-visible sequence. Splitting the prefix-drop from the projection
placement would leave the defect half-fixed and the ordering wrong in the
interim.

## 5. Closure boundary

This line's closure evidence is C001's closure record at
`plans/closure/context-continuity-compaction-post-closure-corrective/001-status.md`,
which records C001 as **closed** on 2026-10-26. The line has no other
milestone: the scoped gate C001's closure required also needed the
pre-existing base-branch failures recorded in that closure record §3 repaired,
and those repairs are recorded there rather than in a second milestone.

## 6. What this line must not do

- **No change to the `TurnSubmit` contract.**
  `crates/codegg-client/src/compose.rs:124` requires a non-empty
  provider-message list; that contract is used by every frontend and is not in
  scope.
- **No truncation, rewrite or emptying of the frontend's display transcript.**
  The user's scrollback is their record. The correction is daemon-side.
- **No second history service, no vector store, no new durable store.** The new
  durable state is two additive, bounded fields on the existing continuation
  checkpoint payload
  (`crates/codegg-core/src/session/continuation.rs:319`), whose 128 KiB bound
  (`continuation.rs:51`) is not to be raised.
- **No reopening of the closed `context-continuity-compaction` M001–M004
  milestones**, their closure records, or the closed `long-horizon-work-execution`
  line.
- **No change to compaction policy, thresholds, the tracker, or the semantic
  enrichment contract.** The compaction engine works; the problem is that its
  result does not survive the turn boundary.
- **No bounding of the wire payload in this milestone.** The daemon still
  receives the superseded prefix before it can drop it. That transport cost is
  recorded as a known cost in ADR-0054, not silently absorbed here.
- **No new CI lane, benchmark gate, or live-provider test.** Verification stays
  on the existing `scripts/verify.sh quick` surface and the existing
  deterministic continuity suites.

## 7. Relationship to the closed predecessor line

`plans/subsystems/context-continuity-compaction-roadmap.md` and its M001–M004
closure records remain **closed and unchanged**. This addendum does not
amend them and does not claim their work was wrong. What it records is that
M004's delivered behaviour — "loading the latest installed continuation
checkpoint for subsequent turns/restart" — is implemented, but is **incoherent
with the frontend-supplied history** that actually crosses the wire, and with
M004 §6.5's placement requirement it did not meet for one class of model
profile. That incoherence is the defect; M004's own durable contracts (the
checkpoint store, the transactional ordering, the one-frame invariant) are the
machinery this line reuses.

The M004 closure records are therefore **inputs** to this line, not subjects of
it. The plan references them explicitly, as a corrective pass must.
