# User Prompt Queue M001 — Turn contention refusal correctness — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/user-prompt-queue/001-turn-contention-refusal-correctness.md`

Source subsystem roadmap:

- `plans/subsystems/user-prompt-queue-roadmap.md` §7 Milestone 1

Repository baseline: `18c3dca92099fe2de09f875731530348fb76b68f`
(`feat(tui): panel chrome, keymap adoption, and derived theme selection (M002a-M003)`)

Primary class: capability

Applicable ADRs:

- `plans/adrs/ADR-0036-mid-turn-prompt-steering-delivery-contract.md` — **still
  `proposed`**. M001 borrowed only its refusal-taxonomy boundary (authorization
  refusal vs. channel/resource condition) as naming vocabulary. No part of
  ADR-0036 was implemented, and its acceptance was never required.
- `plans/adrs/ADR-0007-shared-session-control-ownership.md` (accepted) —
  establishes that active-turn control is a controller-authority concept, which
  is why `turn_already_active` is not a generic failure.

## 1. Executive finding

M001 is closed. A turn submit refused because a turn is already active no longer
destroys the state of the turn that is actually running, and the user's composed
text is no longer destroyed by any refusal.

The substantive defect was state corruption, not a bad message. `turn_already_active`
was routed through `fail_unstarted_turn`, which nulled `active_turn_id` and
`working_since`, stopped the thinking indicator, finalized the activity stream of
a turn that was still producing events, and set `SessionStatus::Error` on a
session that was actively working. The UI asserted the opposite of the truth at
exactly the moment a user was waiting. This matters beyond the message: the
queue milestone that follows has to consult turn state to decide between queueing
and steering, and this path invalidated that state.

Two further live defects were found and fixed while implementing the plan's
acceptance criteria, both recorded below because §15 requires it.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Typed, total refusal classification | `src/tui/app/turn_submit.rs` — `TurnSubmitOutcome`, `classify` | pass |
| Classification at one point only | `turn_submit::submit` called from the single submit task; the completion arm receives data | pass |
| Unknown code is not contention, not success | `an_unknown_code_is_a_rejection_not_contention` | pass |
| Contention preserves active-turn identity/age/status/spinner | `contention_leaves_the_running_turn_exactly_as_it_found_it` | pass |
| Contention releases the composer and reports | same test, plus `tests/tui_usability_fixes.rs::contention_keeps_the_running_turn_and_gives_the_prompt_back` | pass |
| Contention with no active turn invents nothing | `contention_with_no_active_turn_preserves_nothing_it_does_not_have` | pass |
| Composed text restored on a recoverable refusal | `a_refusal_restores_the_prompt_and_stashes_the_newer_draft` | pass |
| Newer draft preserved in the bounded stash | same test | pass |
| Retry does not duplicate the transcript entry | `resubmitting_a_restored_prompt_does_not_duplicate_its_transcript_entry` | pass |
| Stale completion changes nothing | `a_superseded_completion_changes_nothing` | pass |
| Cancellation changes nothing | `a_completion_arriving_after_cancel_changes_nothing` | pass |
| Transport failure is not reported as contention | `a_lost_connection_is_reported_as_a_connection_problem`, `a_transport_failure_is_not_reported_as_contention` | pass |
| Unexpected response reported, not swallowed, not success | `an_unexpected_response_is_reported_without_offering_a_resubmit`, `an_uninterpreted_response_is_reported_and_does_not_offer_a_resubmit` | pass |
| Bounded retyping, never truncated | `restoring_a_long_prompt_is_bounded_never_truncated` | pass |
| Observer submit blocking unregressed | `an_observer_is_still_refused_before_any_submission_is_attempted` | pass |
| Daemon idempotency key unchanged | `turn_submission_request_id` and its derivation untouched; `turn_submission_retries_reuse_identity_only_for_identical_payload` unchanged | pass |
| Working strip row count unchanged | no render change; `tui_render` (111 tests) unchanged and passing | pass |
| Turn failures stay latched | `report_turn_error` semantics untouched; only *which* class reaches it changed | pass |

## 3. Production implementation evidence

**`src/tui/app/turn_submit.rs` (new).** `TurnSubmitOutcome` is `Accepted`,
`Contended`, `Rejected`, `TransportFailure`, or `Unexpected`. `classify` is total
over `Result<CoreResponse, AppError>`. Contention is reached only through the
exact-code allowlist `CONTENTION_CODES` (`turn_already_active`); it is never a
default and never a resemblance match.

Three decisions are load-bearing and are recorded in the module's own doc
comment:

- **Unknown daemon codes classify as `Rejected`, not `Unexpected`.** `Rejected`
  is the only class that restores the prompt. The daemon grows refusal codes;
  routing an unrecognised one to a class that refuses to restore would lose the
  user's words the first time a new reason appeared. This is a **deliberate
  deviation from the plan's §6.1 wording**, which said an unrecognized code
  should map to an explicit unexpected outcome. `Unexpected` is reserved for a
  response *shape* the frontend cannot interpret — there, the submit may already
  have been accepted, so no resubmit may be offered.
- **Contention does not latch.** Row 0 gives `last_error` absolute precedence
  (`render_working_strip`), so calling `report_turn_error` would replace the
  live turn's spinner and elapsed time with a stale `✗` line — the same state
  corruption in a different costume. `fail_contended_turn` reports a warning
  toast instead, which is transient by design and is retained in `/logs`.
- **`Unexpected` does not restore the prompt.**

**`App::apply_turn_submission_finished`.** The single completion handler.
`finish(generation)` is checked *first* and a `false` return drops the
completion having mutated nothing. The daemon `request_id` is then checked
separately and a mismatch also returns `false` without clearing the retained
record, so an identical retry keeps its idempotency key.

**`App::turn_submit_request: AsyncUiRequestState`.** The local staleness guard,
deliberately distinct from the daemon's `request_id` idempotency key.
`App::cancel_turn_submit` invalidates an in-flight submit on tab switch,
reconnect, and project change, and returns the submitted prompt so reconnect can
hand it back.

**`PromptState::restore_prompt_with_stash` + `PROMPT_STASH_BOUND`.** One restore
shape. The implementation previously existed as a literal `100` repeated in
three places (`apply_prompt_session_failure`, `invalidate_pending_session_submit`,
and the refusal path's own need), which made the bound three promises rather
than one. All three now route through this method. The bound applies to how many
displaced drafts are kept, never to the restored text: truncating would change
what the user wrote.

**`spawn_tui_task` replaces the bare `tokio::spawn`.** The submission produces two
completions (the submit result and, for staged shell evidence, the evidence
disposition), so both are sent from inside the task and the returned `Option` is
`None`. `spawn_tui_task` still owns the spawn, the elapsed-time trace, and the
no-command-sender case. Send order is unchanged, which keeps the existing
`shell_promotions_staged` ordering assertion valid.

## 4. Additional defects found and fixed

Both are recorded because §15 requires every behaviour change to be named.

### 4.1 `send_prompt` overwrote the failure release with `Working`

`dispatch_turn_submit_request` released the unstarted turn correctly on its
no-core-client and no-session-bound paths, and then `send_prompt` stamped
`SessionStatus::Working` a few lines later — overwriting the `Error` the release
had just set. The net effect on the socket-daemon frontend was precisely the
stall the release exists to prevent, and it is why those two paths had never
actually fixed anything.

`dispatch_turn_submit_request` now returns `bool`, and `send_prompt` skips the
optimistic `Working` stamp when the dispatch failed. Proven by
`a_socket_daemon_frontend_reports_a_missing_connection_without_losing_the_prompt`.

The adjacent `else` arm in `send_prompt` (socket daemon, no session, no pending
session-create route) cleared the composer, set `Error`, and returned without
restoring the prompt. It now calls `restore_prompt_with_stash` too.

### 4.2 A stale completion could still null a live turn

The pre-existing arm guarded only the `pending_turn_submission` clear with a
request-id comparison and then called `fail_unstarted_turn`
**unconditionally**. A late completion from a superseded submission could
therefore null the identity of a turn that was still running. Fixed by the
generation guard described above; proven by
`a_superseded_completion_changes_nothing`.

### 4.3 Adjacent gap observed and deliberately NOT taken

`dispatch_turn_submit_request`'s `agents_to_dtos` failure still returns without
releasing the turn. It has the same shape as 4.1 (the composer stays cleared and
`pending_send` stays set). It was **not** changed: it is not a refusal path, it
is not in the plan's scope, and taking it would add untested behaviour to a
closure pass bounded to one concern. Recorded here as evidence for a follow-up
rather than absorbed silently.

## 5. Verification executed

```text
cargo fmt                                                        # applied, clean
CARGO_BUILD_JOBS=2 cargo test -p codegg --lib turn_submit       # 23 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -p codegg --test tui_render               # 111 passed
CARGO_BUILD_JOBS=2 cargo test -p codegg --test tui_completion_trajectory # 22 passed
CARGO_BUILD_JOBS=2 cargo test -p codegg --test tui_usability_fixes      # 26 passed
CARGO_BUILD_JOBS=2 cargo clippy -p codegg --all-targets -- -D warnings   # exit 0, no warnings
scripts/verify.sh quick                                        # Quick verification passed
```

`scripts/verify.sh quick` is the required canonical gate and it passed,
including the agent-schema, core-boundary, sandbox, execution-ownership,
TUI-authority, http-route-disposition, audit-coverage, scheduler-bypass,
config-merge-coverage, no-hardcoded-models, provider wire/catalog/resilience +
OpenAI endpoint, and Eggwork target-routing guards, plus
`cargo check --workspace --all-targets --locked`.

Hosted CI was **not** run. No CI run was obtained for this change, and none is
claimed.

### 5.1 Two §11 commands match nothing and must not be cited as evidence

`cargo test -p codegg --lib tui::async_request` and
`cargo test -p codegg --lib tui::runtime::command_dispatch` each report
`0 passed; 0 failed; 6483 filtered out`. These filter strings are **module
paths, not test names**, and there are no tests at those paths. They were run
and they selected nothing. The behaviour they were meant to cover is covered
instead by `--lib turn_submit` (which subsumes `AsyncUiRequestState` staleness
through `a_superseded_completion_changes_nothing` and
`a_completion_arriving_after_cancel_changes_nothing`) and by the full lib and
workspace suites. The plan's §11 command list should be corrected rather than
re-cited.

### 5.2 One pre-existing environmental failure, unrelated to M001

`core::daemon::tests::turn_submit_rejects_when_active` fails with
`InvalidRootPath("/tmp")`. This was verified **against the clean baseline** by
stashing all M001 changes and re-running the same command: it fails identically
with no M001 code present. It is a macOS `/tmp` symlink interaction in test
workspace registration, not a regression. `turn_submit_uses_injected_runtime`,
its sibling in the same file, passes.

## 6. Invariant review

- **Turn failures stay latched.** `report_turn_error` is unchanged; only which
  classification class reaches it changed. Contention is deliberately excluded
  and is justified above.
- **Row 0 stays reserved and unchanged.** No render code was touched.
  `working_strip_rows` and the row-0 precedence order are as they were.
- **Transport selection still follows what the app holds.**
  `is_ws_attach_transport` and the `remote_send_tx` discriminator are untouched.
- **No submit may spawn a second turn.** The frontend never resubmits. The new
  code has no retry, no backoff, and no timer. Restoring text for the user to
  resubmit is the only recovery introduced.
- **The request identity scheme is unchanged.** `turn_submission_request_id`
  still derives the daemon key from session id plus payload digest; the
  generation counter is a separate, purely local mechanism.
- **No prompt text is logged.** The only new diagnostic is
  `tracing::warn!("core facade turn.submit failed: {}", reason)` on an already
  user-facing message, which contains the daemon's code and message, never the
  prompt.
- **Observer behavior unchanged** and covered by a test.

## 7. Failure, cancellation, restart, and contention semantics

- **Contention** — not a turn failure. The turn continues and stays visible; the
  submit is refused, the composer is released, the text is restored, a warning
  toast names the refusal.
- **Recoverable rejection** — teardown plus a latched turn failure plus restored
  text.
- **Transport failure** — reported as a connection problem, never as contention;
  text restored.
- **Unexpected response shape** — reported explicitly, never treated as success
  and never as contention; no resubmit offered.
- **Cancellation** — `cancel_turn_submit` invalidates the generation; a
  completion arriving after it changes nothing.
- **Restart** — nothing durable changed. No storage, no migration, no layout
  version change, so a restart loses nothing this milestone stored.

## 8. Migration and compatibility review

No storage migration and no `STORAGE_LAYOUT_VERSION` change (it stays at its
current value). No protocol change: `TuiCommand` is in-process only (it derives
`Debug, Clone` and appears nowhere in `src/server/` or
`crates/codegg-protocol/`; the wire type is `TuiMessage`), so changing its
`TurnSubmissionFinished` payload has no wire effect. No daemon change: the
existing error codes are classified, not modified. Frontends predating this
milestone are unaffected. The two observable behaviour changes are that a
contention refusal no longer clears active-turn state, and that the composed
prompt is retained on a recoverable refusal.

## 9. Security review

No new authority, capability, or credential surface. The submit path's observer
gate is unchanged and is covered by a regression test. The daemon remains the
single writer of turn admission; nothing here bypasses the controller lease or
the receipt idempotency key. Restoring a prompt does not grant submission
authority: the user must still press Enter, and daemon-side admission still
refuses a duplicate. Prompt text is not written to any log.

## 10. Pre-existing tests whose assertions changed

| Test | Change | Reason |
|---|---|---|
| `tests/tui_usability_fixes.rs::a_refused_turn_names_its_reason_instead_of_spinning_forever` | added a second argument to `fail_unstarted_turn`; added one assertion for the restored prompt | Signature change plus the new restore contract. The existing assertions are unchanged and still pass. |
| `src/tui/mod.rs` shell-promotion test (`TurnSubmissionFinished` arm) | `error.is_none()` became `!outcome.is_failure()` | The command now carries a typed outcome instead of `Option<String>`. Strictly stronger: it asserts the outcome is not merely non-failing but accepted. |
| `src/tui/app/mod.rs::turn_submission_retries_reuse_identity_only_for_identical_payload` | added the new `prompt` field to the struct literal | Struct gained a field. Assertions unchanged. |

No other test changed. Nothing was deleted or weakened.

## 11. Documentation

- `architecture/tui.md` — the refusal taxonomy table, the three deliberate
  decisions, the shared restore shape, and the request-id-versus-generation
  distinction. This is the single source of truth for the corrected behaviour.
- `.opencode/skills/tui/SKILL.md` — the same contract as operator guidance,
  including the two rules a future contributor is most likely to break
  (contention is not an unstarted turn; unknown codes are rejections).
- `docs/tui.md` — one user-facing section, "Typing while the agent is already
  working", stating the observable behaviour without duplicating internals.

Reviewed for single-source-of-truth compliance: `architecture/tui.md` carries
the full contract; the skill references it and restates only the rules needed to
act; `docs/tui.md` describes behaviour with no internal names.

## 12. Unresolved findings

1. **`agents_to_dtos` failure does not release the turn** (§4.3). Recorded as
   evidence for a follow-up. It does not block M002.
2. **§11's verification command list is partly wrong** (§5.1). Two filters
   select zero tests. The closure record cites what actually ran.
3. **`turn_already_active` cannot distinguish an in-memory collision from a
   controller-lease conflict.** Both preserve active-turn state, so M001 does not
   need to tell them apart, and the plan's stop condition ("the daemon cannot
   distinguish contention well enough to classify reliably") was **not**
   triggered: contention is reliably classifiable, just not sub-classifiable. No
   work was improvised to guess which it was.

## 13. Roadmap disposition and unblock audit

M001 is **closed**. Per §15, the unblock audit for M002:

- M002's hard dependency on M001 is **discharged** by this closure.
- M002 remains **not plannable yet**, for a different reason: it is gated on
  **ADR-0035 acceptance**, which is a maintainer action and is still
  `proposed`. No part of M002's implementation is authorized by this closure.
- M006 and M007 remain gated on ADR-0036 acceptance, still `proposed`. M001 did
  not implement any of ADR-0036.
- M003–M005 and M008 retain their roadmap-sequence dependencies unchanged.
- The `tui-panel-surface-convergence` M004 shared-chrome helper remains a soft /
  interface gate for the queue's own M004, unchanged by this work.

No blocked work is newly unblocked by this closure beyond M001's own completion.

## 14. Registry updates

Performed in the same change set as this record:

- The M001 implementation plan moves `ready for handoff` → `implemented`.
- The subsystem roadmap's M001 milestone moves to closed in §7 and §4's
  "current state" is updated to reflect the corrected path.
- `plans/registry.md`: the "Dependency-ready implementation plans" row moves to
  closed, the roadmap row's M001 status is updated, and the execution-order note
  records M001 closed with M002 still gated on ADR-0035.