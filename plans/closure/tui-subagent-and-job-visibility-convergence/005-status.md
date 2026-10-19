# C007 — Send-to-subagent: closure record

**Milestone:** C007 of `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md`
**Plan:** `plans/implementation/tui-subagent-and-job-visibility-convergence/005-send-to-subagent.md`
**ADR:** `plans/adrs/ADR-0041-subagent-run-addressing-and-human-interaction-authority.md` — accepted 2026-10-18, **Option A**
**Closed:** 2026-10-19
**Working tree:** uncommitted, on `feat/auto-20261018-2b07c50d` at `e8a6b5ca2`

---

## 1. What was delivered

ADR-0041 Option A, as planned: a `/subagent-send` command arms the composer to address
one **running** subagent, and the next submission issues exactly one
`CoreRequest::TurnSteer` for the **controlling parent turn**, carrying the subagent's
display name as an explicit addressee inside the parent's context.

The milestone's four contract claims, each stated as something observable:

| Claim | Where it is enforced |
|---|---|
| An addressed send issues exactly one `TurnSteer` for the parent turn | `commands/subagent_steer.rs::start_subagent_reply`; `tests/user_prompt_queue_box.rs::an_addressed_reply_steers_the_parent_turn_and_submits_nothing` |
| An armed composer can **never** issue `TurnSubmit` | `app/prompt_turn.rs::send_prompt` — the branch returns unconditionally |
| A stale turn id reports `turn_id_mismatch` | `steer_answer` → `send_now_outcome_message` |
| The accepted sentence names the **parent**, never the subagent | `SteerConsequence::ComposerOnly::accepted`; `src/tui/commands/subagent_steer.rs::tests::subagent_steer_the_accepted_sentence_names_the_parent_not_the_subagent` |

## 2. Files changed

Production code:

| File | Change |
|---|---|
| `src/tui/app/state/subagent_addressee.rs` | **New.** `SubagentAddressee` + `reconcile_addressee`. |
| `src/tui/commands/subagent_steer.rs` | **New.** Arm/disarm, `/subagent-send` dispatch, the `TurnSteer` send, the applier. |
| `src/tui/app/prompt_turn.rs` | The addressed branch in `send_prompt`. |
| `src/tui/app/mod.rs` | `App::apply_subagent_rows` funnel, `B::SubagentSend` arm, `PromptState` fields, `TurnStarted` clears the addressee. |
| `src/tui/app/state/prompt.rs` | `subagent_addressee`, `subagent_steer_request`. |
| `src/tui/app/state/mod.rs` | Module + re-exports. |
| `src/tui/app/render.rs` | Badge, placeholder, both `set_rows` sites routed through the funnel. |
| `src/tui/app/commands.rs`, `src/tui/runtime/command_dispatch.rs` | `TuiCommand::SubagentSteerFinished` + dispatch arm. |
| `src/tui/command.rs` | `BuiltinSlashAction::SubagentSend` + registry entry. |
| `src/tui/commands/queue.rs` | `SteerConsequence`; `send_now_outcome_message` takes it. |

Tests: `tests/tui_main_buffer.rs` (+8), `tests/user_prompt_queue_box.rs` (+6),
`src/tui/app/state/subagent_addressee.rs` (+11 unit),
`src/tui/commands/subagent_steer.rs` (+9 unit). **34 total.**

Docs: `architecture/tui.md`, `architecture/command.md`, `docs/tui.md`.

## 3. The design decisions that were not in the plan

Three of these are findings, not preferences. Each would have shipped a defect.

### 3.1 The parent model has no name pool — `Sally` alone is unresolvable

The plan assumed `TurnSteer` "carries the subagent's `display_name` as an explicit
addressee". Carrying the name satisfies ADR-0041's letter and fails its purpose: the
controlling agent receives the text as a steer, and **the pool that mints display names
lives in `src/agent/subagent_names/`, which the TUI deliberately does not have and the
parent model never sees**. Nothing resolves `Sally` back to a run on the model side.

So `SubagentAddressee::addressed_text` reproduces the agent type and the task
description alongside the name:

```
[addressed to subagent Sally (explore): find the retry path]
try the second retry first
```

The parent can act on that without a registry lookup. The description is bounded
(`MAX_ADDRESSED_DESCRIPTION_CHARS = 160`) because it is free text that would otherwise
be able to crowd the reply out of the payload.

### 3.2 A steer is never stored, so a refused reply has no durable home

Confirmed against the source rather than assumed: `TurnSteer` pushes into the loop's
steering channel (`daemon_turns.rs:1283`), the loop drains it into `pending_steer`
(`agent/loop.rs:1075-1079`), and it is injected into the **in-flight provider
transcript**. There is no row in the message store.

Two consequences, both implemented:

- **A refusal restores the user's text.** `TuiCommand::SubagentSteerFinished` carries
  `body`, and the applier puts it back into an *empty* composer. Restoring into a
  non-empty one would destroy whatever the user has started typing instead.
- **The TUI writes no transcript message for an accepted reply.** One would render as
  durable on screen and vanish on reload or export — the divergence the projection
  rules exist to prevent. The toast is the record.

This is also why `start_subagent_reply` clears the composer itself, *after* its
pre-flight, rather than having `send_prompt` clear it first.

### 3.3 `send_now_outcome_message` could not be reused verbatim

The plan said to reuse the vocabulary and not write a second thinner one. Reuse is
right; verbatim reuse would have been wrong in two places:

- The accepted sentence — *"Handed to the turn that is running now, and marked delivered
  to it"* — describes a **queue write** that never happens for a bare steer.
- Every refusal ending — *"It stays queued"* — promises a resend that nothing will
  perform, because nothing durable holds the text.

Rather than fork the function, it now takes a `SteerConsequence`:

```rust
pub enum SteerConsequence { DurableQueue, ComposerOnly }
```

`DurableQueue` is M006's send-now and renders exactly the sentences it rendered before.
`ComposerOnly` is the addressed reply. One code table, one place to keep in step, and
the difference that actually matters is a parameter rather than a divergent copy.

**M006's user-visible sentences changed as a result** (the queue-tail rewording), and
its three tests in `tests/user_prompt_queue_box.rs` were updated to pass the new
argument. They were **not** weakened: the fallback test still asserts the code is named
and the text is said not to have gone out.

## 4. Where the refusal vocabulary groups instead of splitting

The plan's WP-3 acceptance asked for a distinct sentence per code, listing
`session_control_not_controller` and `session_control_conflict` separately. Those two
**deliberately share one sentence**, exactly as they already did in M006's vocabulary.
They differ only in *why* another principal holds the turn — a stale revision against
not-ours — and both resolve to the same user-visible fact and the same non-actionable
outcome. Splitting them would be a distinction without a difference.

The test now asserts distinctness **grouped by remedy**, and asserts that codes within a
group render the *same* sentence, so the grouping is pinned rather than incidental. This
is a **recorded deviation from the plan's literal wording**, and it is a narrowing of a
documentation requirement rather than of the behaviour it protects.

## 5. Non-vacuity — five injected defects, all caught

Every guard was proven against a real injected defect before being cited. All files
were restored and re-verified green after each.

| # | Injected defect | Caught by | Failure named the defect |
|---|---|---|---|
| 1 | Deleted the addressed branch from `send_prompt` | `an_addressed_reply_steers_the_parent_turn_and_submits_nothing` | `an addressed reply must never fall through to TurnSubmit: ["submit:try the second retry first"]` |
| 2 | `reconcile_addressee` clears but returns `false` | 3 unit tests in `subagent_addressee` | `a_run_that_finishes_but_is_still_in_the_tree_also_clears_the_addressee`; `the match is on task_id, not on the row merely existing` |
| 3 | Accepted sentence → `"Sent to {name}."` | `subagent_steer_the_accepted_sentence_names_the_parent_not_the_subagent` | `the parent must be named as the recipient: "Sent to Sally."` |
| 4 | Refusal no longer restores the composer | `a_refused_steer_keeps_the_users_words_in_the_composer` | `a refused reply must survive in the composer  left: ""  right: "never mind, do the other one"` |
| 5 | `TurnSteer` names the subagent's `task_id` instead of the parent turn | `an_addressed_reply_steers_the_parent_turn_and_submits_nothing` | `the steer must name the parent session and turn, not the subagent's task_id: "steer:session-queue-1:task-1:[addressed to subagent Sally (explore): …]"` |

Two notes on how these were read, because both initially looked like weak evidence:

- **Injection 1 first failed as a *timeout*.** True, but indirect — it read the same as
  a hang. The test was changed to settle on *either* request before asserting, and the
  harness was put in the socket-daemon frontend shape so an unarmed submit actually
  dispatches. Re-run, the test then failed naming `submit:…` directly. The first result
  was not cited as evidence.
- **Injection 2 was caught by the unit tests, not the integration test.** The
  integration test asserts *state* (addressee cleared, badge back to `[LLM]`), which
  injection 2 preserved — it only broke the *return value*. The unit tests assert the
  contract and caught it. Recorded rather than smoothed over: the two suites cover
  different halves, and neither alone would have been sufficient evidence.

## 6. Verification actually run

All local. **No CI receipt exists for this work and no runtime observation was made —
the TUI was never launched.** Every command below succeeded on this tree.

```bash
cargo fmt --check                                              # pass
cargo clippy --workspace --all-targets --locked -- -D warnings # pass, no warnings
scripts/verify.sh quick                                        # "Quick verification passed."
cargo test --lib subagent                       # 118 passed (was 98; +20)
cargo test --test tui_main_buffer               #  56 passed (was 48;  +8)
cargo test --test user_prompt_queue_box          #  70 passed (was 64;  +6)
cargo test --test tui_side_panels                #  31 passed (unchanged)
cargo test --test tui_sessions_pending_sections  #  11 passed (unchanged)

python3 scripts/check_tui_transport_authority.py           # pass
python3 scripts/check_tui_subagent_name_pool_isolation.py # pass
python3 scripts/check_execution_ownership.py              # pass
python3 scripts/check_scheduler_bypass.py                  # pass
(cd apps/desktop && node scripts/check-bindings.mjs)       # "desktop bridge DTO fields match"
```

`verify.sh quick` is a strict superset of the CI `verify` job, so the guards CI runs are
green; CI omits the client/desktop boundaries and the provider guards, which `quick`
additionally covers.

Two suites moving and three not is itself evidence the change was additive: only
`codegg`'s own unit tests and the two TUI integration targets changed count, and no
projection, adapter, or row suite moved at all — consistent with this milestone adding
no projection field.

## 7. Invariants

- **I5 — addressing cancels nothing.** Arming issues no request; the daemon-side
  dispatch is untouched. Pinned by `addressing_a_subagent_issues_no_request_at_all`.
- **I1 / C005 pool isolation — green.** `check_tui_subagent_name_pool_isolation.py`
  passes. The name travels **on the row**, copied into the addressee at arm time. Nothing
  resolves it at send time. The new type is named `SubagentAddressee`; the guard's word
  list bans `SubagentName` and `subagent_names`, which this does not contain.
- **ADR-0036 unchanged.** No relaxation of the steering guarantee, no new failure mode,
  no retry. Every refusal is terminal and reported.
- **No new authority.** The reply rides the parent turn's existing controller predicate.
  Nothing previously refused became reachable.
- **`InputTarget` did not grow a variant.** Still `{Build, Chat}`. The addressee is
  composer data.
- **No new key.** I3 and I6 untouched.

## 8. Desktop-bridge parity decision

**No parity work was required, and none was performed.** C007 adds no field to any
projection or bridge DTO: `SubagentAddressee` is TUI-local composer state in
`src/tui/app/state/`, and `CoreRequest::TurnSteer` already existed on the wire before
this milestone. The desktop frontend reads the same command registry, so `/subagent-send`
is available to it without a second implementation.

`check-bindings.mjs` was run and passed. It is reported as a **regression check, not as
evidence of parity**: it asserts only the fields it enumerates and cannot detect a Rust
struct gaining an unlisted field.

## 9. Findings, recorded not fixed

- **Pre-existing, unrelated:** `architecture/command.md` claimed `BuiltinSlashAction` had
  156 variants; the tree had **157** at `HEAD`, before this milestone. C007 made it 158.
  Because C007's own count change made the number worse, it was corrected to the true
  value (158) and the surrounding line references refreshed. The earlier 156-vs-157 drift
  was **not** introduced here and is not C007's to own.
- **Pre-existing, unrelated:** `tests/user_prompt_queue_box.rs` carries a doc comment for
  `settle_prefix_until` that is separated from its subject by the unrelated
  `commit_compose` function. Left alone; the new helper was inserted above the block
  rather than entangling itself with it.
- **Accepted design limit:** `session_control_not_controller` and
  `session_control_conflict` share a sentence (§4).
- **Inherited:** the pre-existing full-suite `cargo test --lib` failures documented in the
  C002/C005 closure records were **not** re-run in this turn and are unchanged. The five
  focused suites above were run and pass.
- **C003 and C006** remain implemented-but-unclosed in the same tree. Outside this ask.

## 10. Status reconciliation

| Document | Change |
|---|---|
| `plans/registry.md` | Subsystem row → *C001, C002, C004, C005 and C007 closed*; C007 added to *Recently closed* and removed from *Dependency-ready implementation plans*; the row's C007 progress narrative rewritten to the delivered behaviour. |
| `…convergence-corrective-addendum.md` | §6 dependency graph, §7 (C007 heading and body), §11 status table all updated to *closed*. |
| `architecture/command.md` | `/subagent-send` row added; built-in count 165 → 166 (the `command_docs_count_matches_registry` guard enforces this). |
| `architecture/tui.md`, `docs/tui.md` | Behaviour documented. |

One audit note so a later reader is not misled: in this registry a closed milestone
often has **two** rows — its original row retained in *Dependency-ready
implementation plans* with the status cell flipped to `**closed**`, plus a row in
*Recently closed*. C001, C002 and C005 are in that state. Every status label agrees;
only the organisation is duplicated, and it is this registry's existing pattern across
other subsystems too (the Helix `M001` row does the same). C007 was moved to *Recently
closed* rather than left in place, which is the cleaner of the two conventions.

`ADR-0041` was **not** edited — accepted ADRs are immutable. Its line *"A subagent tab
remains read-only until a C007 implementation of Option A lands"* was a **precondition**,
and C007 satisfying it is the expected outcome rather than a contradiction. Nothing in
the implementation contradicts any ADR clause.

---

## 11. Handoff

C007 is closed. Every milestone in the corrective addendum is now either closed or
implemented-but-unclosed:

- **Closed:** C001, C002, C004, C005, C007.
- **Implemented, unclosed:** C003, C006 — both need a closure record, and C003/C006 have
  the same D-8 gap C004 had (no implementation plan).
- **No milestone is planned-and-unstarted.** The addendum's own list is exhausted.

The natural next step is not another milestone but closing C003 and C006, which would
take the addendum to fully closed. That is a separate decision and is not assumed here.