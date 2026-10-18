# User Prompt Queue Milestone 001 — Turn contention refusal correctness

Status: implemented

Closure record: `plans/closure/user-prompt-queue/001-status.md`

Repository baseline: `18c3dca92099fe2de09f875731530348fb76b68f`
(`feat(tui): panel chrome, keymap adoption, and derived theme selection (M002a-M003)`)

Source roadmap: `plans/subsystems/user-prompt-queue-roadmap.md` §7 Milestone 1,
§4 Current state

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`

Applicable ADRs:

- `plans/adrs/ADR-0036-mid-turn-prompt-steering-delivery-contract.md` (**proposed**
  — used only for its refusal-taxonomy boundary; this milestone does not implement
  steering and does not depend on ADR-0036 acceptance)
- `plans/adrs/ADR-0007-shared-session-control-ownership.md` (accepted — establishes
  that active-turn control is a controller-authority concept, which is why a
  contention refusal is not a generic failure)

Primary class: capability

## 1. Objective

A turn submit refused because a turn is already active must leave the frontend's
turn state truthful and the user's composed text intact, and must be reported
distinguishably from a submit that failed for any other reason.

One bounded outcome: correct frontend handling of turn-contention refusal.

## 2. Why this milestone is ready

It has no hard dependency on any other milestone in the roadmap and no
dependency on the storage design. It does not require ADR-0035 acceptance
because it introduces no queue. It is written first because the queue must not be
built on a submit path that reports "no active turn" while a turn is running, and
because the text-loss defect it fixes is live today, independent of whether the
queue is ever approved.

The one input it takes from an unaccepted ADR is a *taxonomy* — the distinction
between an authorization refusal and a channel/resource condition — which is
already implemented in the daemon's steer handler and is recorded here so that
the classification vocabulary is chosen once rather than twice. If that boundary
is later rejected, work package A still stands; only the naming changes.

## 3. Current implementation evidence

**The composer is cleared before the transport decision.** The submit path adds
the user's message to the transcript and clears the composer optimistically,
before dispatching, because the daemon may refuse. Any refusal must therefore
undo that optimistic state.

**The refusal arm reports every failure identically.** The submit-completion arm
formats the daemon's error code and message and passes it to
`fail_unstarted_turn`. That function clears `working_since`, clears
`active_turn_id`, sets `SessionStatus::Error`, stops the thinking indicator,
finalizes streaming, and releases the composer. It is correct for a submit that
never started a turn. It is **wrong for `turn_already_active`**, where a turn is
running on the daemon: the frontend discards the identity and age of a turn that
is still in flight, stops its spinner, and reports an error state for a session
that is actively working. The failure text still names the daemon's code, so the
user learns *that* they were refused but the UI simultaneously asserts the
opposite of the truth about session state.

**Nothing restores the composed text on this path.** The daemon's refusal reaches
the completion arm as a formatted string with no access to the prompt, so the
text cannot be restored even if the arm wanted to.

**A correct restore mechanism already exists and is unused here.** Session-create
failure applies a restore that stashes any newer draft, puts the failed prompt
back into the composer, and marks the prompt so a retry does not duplicate its
transcript entry. The turn-submit path does not use it. The reuse is the
cheapest correct fix and inherits a proven shape, including its bounded stash and
its no-duplicate-transcript rule.

**The in-flight slot is a single option with an ad-hoc currency check.** One
pending submission is tracked. The submit path spawns a raw task that hand-rolls
an identity comparison at the completion site. The TUI already has a shared
request-state machine used by several dialogs that provides exactly this with
generation-based staleness, a cancel operation, and a transient error slot — and
the submit path does not use it.

**Two refusals are already distinguished upstream.** The daemon returns
`turn_already_active` both for an in-memory active-turn collision and for a
controller-lease conflict, and it returns distinct codes for other conditions
including a missing session, a turn-id mismatch, and no active turn. The
frontend currently collapses all of these into one formatted string.

## 4. Invariants that must not regress

- **Turn failures are latched.** A reported turn failure stays visible until a
  turn starts. This milestone must not make errors transient, must not clear
  `last_error` on a timer, and must not stop the latched row-0 rendering.
- **Row 0 is reserved unconditionally.** Nothing here may add a row to the
  working strip or gate row 0 on anything. The contention case must reuse the
  existing status surface, not extend it.
- **Transport selection follows what the app holds, not app mode.** Submit
  dispatch distinguishes the attach transport from the socket-daemon transport by
  which channel it owns. This milestone must not route or classify on app mode.
- **No submit may spawn a second turn.** The frontend must not retry a refused
  submit on its own initiative. Restoring text for the user to resubmit is the
  only permitted recovery.
- **A retry must not duplicate the transcript entry.** A restored prompt that is
  resubmitted must produce one user message, not two.
- **The request identity remains the daemon's idempotency key.** The identity
  scheme must not change; only the local staleness bookkeeping is replaced.
- **Prompt text must not be logged verbatim** by any new diagnostic.
- **Observer read-only behavior is unchanged.** Observers never submit turns.

## 5. Scope

### In

- A typed classification of turn-submit refusal outcomes, sufficient to separate
  contention from other failures.
- Frontend response per class: correct turn-state handling, correct release of the
  composer, correct reporting.
- Preservation of active-turn identity, age, and spinner when a turn is in fact
  still running.
- Restoration of the composed text into the composer for recoverable refusals,
  reusing the existing draft-restore mechanism, with no transcript duplication on
  retry.
- Replacing the submit path's ad-hoc raw spawn and hand-rolled currency check
  with the shared task-spawn helper and the shared request-state machine.
- A stale-completion test proving a superseded submission cannot mutate current
  state.
- Documentation updates for the corrected refusal behavior.

### Out

- Any queue: storage, protocol, dialog, status indicator, or ordering.
- Any steering path. `TurnSteer` remains unissued by the frontend.
- Any change to daemon turn admission, the controller lease, or receipts.
- Any change to the working strip's layout, row count, or reservation.
- Any new user-facing keybinding or dialog.
- Desktop changes.
- Retry-on-behalf-of-the-user, backoff, or automatic resubmission.
- Recovering text already lost before this milestone.

## 6. Required production changes

### Core / daemon

None. The daemon's behavior is correct and its error codes are sufficient. This
milestone is entirely frontend.

### Storage

None. No migration. The storage layout version stays at its current value.

### Protocol

None. No new variant and no change to any existing request or response shape.

### Runtime / frontend

1. **A refusal classification type.** A small enum over submit outcomes —
   contention, recoverable rejection, transport failure, and unexpected response —
   derived from the daemon's error code and the transport result. It must be total:
   an unrecognized code maps to an explicit unexpected outcome rather than
   defaulting into contention or into a generic success.

2. **Classified response in the submit completion arm.** Each class gets its own
   handling. Contention must not clear active-turn state. Recoverable rejection
   restores the composer. Transport failure must be distinguishable from a daemon
   refusal, because "the socket is gone" and "a turn is already running" imply
   different next actions for the user.

3. **Split the unstarted-turn release.** The current single helper conflates "no
   turn was ever started" with "a turn was refused because one is already
   running". Separate the optimistic-teardown path from the contention path. The
   contention path releases the composer and reports, but leaves active-turn
   identity, working age, status, and the thinking indicator intact.

4. **Restore the composed text on recoverable outcomes.** Reuse the existing
   draft-restore shape: stash any newer draft within its existing bound, put the
   refused prompt back, and mark it so resubmission does not duplicate the
   transcript entry. The completion arm needs access to the submitted prompt,
   which means carrying it — or its identity — in the completion payload.

5. **Replace the ad-hoc submit spawn.** Use the shared task-spawn helper instead of
   a bare spawn, and replace the hand-rolled identity comparison with the shared
   request-state machine so staleness, cancellation, and the transient error slot
   are handled by the existing tested type rather than by local arithmetic.

6. **Bounded retyping.** A restored prompt must respect the existing per-prompt
   bound and must not be silently truncated in a way that changes its meaning.

### Security

No new authority. Observer blocking is unchanged and must be verified not to have
regressed, since the submit path's observer gate sits near the code being changed.

### Documentation

The TUI architecture doc and the `tui` skill gain the corrected refusal behavior
and the contention distinction. `docs/tui.md` gains a short user-facing note that
a prompt typed during an active turn is retained and can be resubmitted.

## 7. Ordered work packages

### Work package A — Classify the refusal

Intent: give the completion arm a total, typed view of what happened.

Required changes: introduce the refusal classification and construct it at the
single point where the daemon response or transport error is interpreted. Every
currently possible outcome must map to exactly one class, and an unrecognized
daemon code must map to an explicit unexpected class.

Acceptance evidence: a table-driven test over every daemon error code the submit
path can observe, asserting the classification of each, plus a case proving an
unknown code does not classify as contention.

### Work package B — Preserve active-turn state on contention

Intent: stop the frontend from asserting that no turn is running when one is.

Required changes: separate the optimistic teardown from the contention response as
described in §6.3. Contention releases the composer and reports, and leaves
active-turn identity, working age, session status, and the thinking indicator as
they were.

Acceptance evidence: a test that begins with an active turn, injects a contention
refusal, and asserts that the turn identity and working age are unchanged, the
thinking indicator is still active, and the session status still reflects an
in-flight turn — while the composer is released and the refusal is reported.

### Work package C — Restore the composed text

Intent: stop losing the user's prompt.

Required changes: carry the submitted prompt through to the completion arm, and
apply the existing draft-restore shape for recoverable outcomes. Contention is
recoverable: the user can resubmit once the turn ends. Confirm and state which
outcomes are recoverable and which are not; a transport failure whose socket is
gone is recoverable by restoring text, while an unexpected response shape may not
be.

Acceptance evidence: a test asserting that after a contention refusal the composer
holds the original prompt, that a newer draft typed in the interim was preserved
in the bounded stash, and that resubmitting the restored prompt produces exactly
one user transcript entry.

### Work package D — Replace the ad-hoc spawn and currency check

Intent: use the shared, tested async conventions.

Required changes: adopt the shared task-spawn helper and the shared request-state
machine for the submit path. Preserve the existing request-identity derivation,
because it is the daemon's idempotency key.

Acceptance evidence: a stale-completion test in the shape of the existing
convention — a superseded submission's completion must not clear the current
submission, must not release the composer, and must not mutate turn state.

### Work package E — Documentation

Intent: make the corrected behavior discoverable.

Required changes: update the TUI architecture doc, the `tui` skill, and the
user-facing TUI guide as described in §6 Documentation.

Acceptance evidence: doc-surface review confirming the change is described once in
the authoritative architecture doc and referenced, not duplicated, elsewhere.

## 8. Failure, cancellation, restart, contention semantics

- **Contention.** Not a turn failure in the frontend's sense. The turn continues;
  the frontend keeps showing it. The submit is refused and the text is restored.
- **Recoverable rejection.** Text restored, composer released, refusal reported
  through the normal latched turn-failure path.
- **Transport failure.** Distinct from a daemon refusal. Text restored where
  recovery is possible; the report must not claim a turn is already running when
  the actual cause is a lost connection.
- **Unexpected response shape.** Reported explicitly, never silently treated as
  success and never treated as contention.
- **Cancellation.** The shared request-state machine's cancel operation must
  invalidate an in-flight submit so a late completion cannot act on state the
  user has moved past.
- **Restart.** Nothing durable changes. This milestone adds no storage, so a
  restart loses nothing that this milestone stored.
- **Contention between frontends.** Unchanged: the daemon's controller lease
  remains the single writer.

## 9. Compatibility and migration

No storage migration and no layout-version change. No protocol change. Frontends
that predate this milestone are unaffected. The only observable behavior changes
are that a contention refusal no longer clears active-turn state, and that the
composed prompt is retained.

The daemon error codes this milestone classifies are already in use and are not
modified.

## 10. Required tests

- Table-driven classification over every observable daemon error code, including
  an unknown code.
- Contention refusal with an active turn in flight: turn identity, working age,
  session status, and thinking indicator preserved; composer released; refusal
  reported.
- Contention refusal with no active turn: normal release path, no false
  "preserved turn" state.
- Text restoration: composer content, newer-draft preservation within the bound,
  and exactly one transcript entry on resubmission.
- Retry suppression: the restored prompt does not duplicate its user message.
- Stale completion: a superseded submission's completion changes nothing.
- Cancellation: a completion arriving after cancel changes nothing.
- Transport failure is not reported as contention.
- Unexpected response is reported, not swallowed and not treated as success.
- Bounded retyping: an oversized restored prompt is bounded rather than silently
  truncated.
- Observer submit blocking is unregressed.
- The existing turn-submission tests continue to pass unchanged unless they
  asserted the behavior this milestone corrects, in which case the change must be
  called out in the closure record with the reason.

## 11. Required verification commands

```bash
scripts/verify.sh quick
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p codegg --lib tui::async_request
cargo test -p codegg --lib tui::app::prompt_turn
cargo test -p codegg --lib tui::runtime::command_dispatch
cargo test -p codegg --test tui_render
cargo test -p codegg --test tui_completion_trajectory
```

`verify.sh quick` is required because this changes the TUI submit path and the
turn-authority guards it sits beside. Hosted CI is not required for closure but a
run should be recorded if one is obtained; label it truthfully either way.

## 12. Documentation updates

- `architecture/tui.md` — the refusal taxonomy, the contention distinction, and
  the fact that a composed prompt is retained on a recoverable refusal.
- `.opencode/skills/tui/SKILL.md` — the same, in operator guidance form.
- `docs/tui.md` — one user-facing sentence about typing during an active turn.

If any count or line reference in those documents is used as evidence, re-verify
it against the tree at closure rather than copying a number forward.

## 13. Acceptance criteria

Externally observable, and each independently checkable:

1. Typing a prompt and submitting it while a turn is active reports the refusal
   and leaves the prompt in the composer.
2. After that refusal, the UI continues to show the running turn — spinner,
   elapsed time, and active-turn identity — rather than an error or idle state.
3. Resubmitting the restored prompt after the turn ends produces exactly one user
   message in the transcript.
4. A refusal caused by something other than contention does not claim a turn is
   already running.
5. A lost connection is reported as a connection problem, not as contention.
6. A completion for a superseded or cancelled submission changes nothing.
7. The working strip still reserves the same number of rows and shows no new row.

## 14. Stop conditions

Stop and report rather than improvise if any of these occur:

- The daemon cannot distinguish contention from another refusal well enough to
  classify reliably. Report the ambiguity instead of guessing from message text.
- Preserving the composed text would require changing the request-identity scheme,
  because that identity is the daemon's idempotency key.
- The fix appears to require any daemon-side, storage-side, or protocol-side
  change. That is a scope expansion into M002 or beyond.
- Any queue, steering, or dialog work is found to be necessary to fix the text
  loss. Report it as evidence for M002 rather than absorbing it here.
- Restoring text would duplicate a transcript entry for which no suppression
  mechanism exists.
- The shared request-state machine cannot represent the submit path's needs
  without modification that would affect other dialogs. Report it rather than
  forking a private copy.

## 15. Closure evidence required

- Each acceptance criterion in §13 demonstrated by a named passing test, not by
  inspection.
- The classification table test, including the unknown-code case.
- The stale-completion test, matching the existing convention's shape.
- `scripts/verify.sh quick`, `cargo fmt`, and clippy with warnings denied, all
  passing, with output recorded.
- The targeted test targets in §11 passing.
- A list of every pre-existing test whose assertions were changed, each with the
  reason it encoded the behavior this milestone corrects.
- Documentation updates landed and reviewed for single-source-of-truth
  compliance.
- An unblock audit of the roadmap: M002's hard dependency on M001 is discharged by
  this closure, so M002 becomes eligible to plan once ADR-0035 is accepted. That
  audit must be recorded in the closure record, and the registry updated in the
  same commit.

A closure that reports only compilation, or that skips the stale-completion test,
is not acceptable.

## 16. Handoff notes

Three things this plan found that a reader should not have to rediscover:

**The restore mechanism already exists.** Session-create failure already
implements exactly the right shape — stash the newer draft within its bound, put
the refused prompt back, mark it so retry does not duplicate the transcript
entry. Reuse it. Writing a second restore path would duplicate a proven
mechanism and would need its own no-duplicate proof.

**The bug is state corruption, not just a bad message.** It is tempting to treat
this milestone as "make the error message nicer". The substantive defect is that
the frontend discards the identity and elapsed age of a turn that is still
running. A cosmetic fix would leave the queue — which must later consult turn
state to decide between queueing and steering — reading a state this path
invalidates.

**Do not add a retry.** The temptation after "the text is back now" is to offer
resubmission on the user's behalf, or to retry when the turn ends. That would be a
queue, and it would be an unauthorized one. The correct end state here is the
user's prompt sitting in their composer, which is strictly better than the
composer's contents being destroyed.

## 17. Implementation outcome

Implemented and closed. See
`plans/closure/user-prompt-queue/001-status.md` for the requirement-to-evidence
matrix, the verification actually run, the two additional defects found and
fixed, the deliberate deviation from §6.1 on unknown daemon codes, and the M002
unblock audit.

Two corrections to this plan are recorded there and should not be re-cited from
here: §11's `cargo test -p codegg --lib tui::async_request` and
`--lib tui::runtime::command_dispatch` filters match module paths, not test
names, and select zero tests; and §6.1's "unrecognized code maps to an explicit
unexpected outcome" is implemented as a recoverable rejection, because
`Rejected` is the only class that keeps the user's text.
