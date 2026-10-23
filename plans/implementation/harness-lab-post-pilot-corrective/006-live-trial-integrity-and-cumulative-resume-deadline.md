# Harness Lab Post-Pilot C006 — Live-Trial Integrity and the Cumulative Resume Deadline

Status: ready for handoff
Repository baseline: `plans/harness-lab-post-pilot-correctives@ff651895b` (2026-10-22; inspect latest at handoff).
Source roadmap: `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md#5-milestones`
Corrects C004 `plans/closure/harness-lab-post-pilot-corrective/004-status.md` §5 conditions 4 and 5; preserves the closed C001–C003 records and the preserved comparison receipt `plans/closure/harness-lab/evidence/autonomy-comparison-20261022.json`.
Long-term requirements: `plans/000-long-term-specification.md` §§23–24, 26, 28–29; `plans/001-terminology-and-domain-model.md`.
Primary class: invariant.

## 1. Objective

Make a live qualification's **measurement** trustworthy in the two ways it is
currently not.

**First: a dropped daemon event must never be silent.** The daemon's socket
forwarder drops events when its subscriber lags and only writes a log line; the
ACP adapter has no way to know a gap happened, so it can report a tool call it
never saw complete. The harness already refuses to score such a trial as a
candidate failure — it calls the trial unmeasured and re-runs it — but the
**cause** was never diagnosed, the **rate** has never been measured, and the
re-run is not bounded, not counted in the receipt, and not charged visibly
against the admitted budget.

**Second: a qualification's wallclock budget must be cumulative.** Today the
cumulative deadline exists in exactly one command. `optimize` persists an
absolute `deadline_unix` in its immutable inputs and restores it on a resume.
The two commands that actually *perform* a qualification — `qualify` and
`evaluate` — enforce `--max-wallclock` against a process-local `started` that
is re-created on every invocation. Resuming a qualification therefore restarts
the wallclock budget at its full declared length, so the total wallclock one
candidate qualification may consume is unbounded in the number of resumes.

Neither of these is a scoring problem. The score contract did its job; it was
fed an unmeasured trial and it said so. These are the two ways a *measurement*
can be wrong before it reaches the scorer.

## 2. Evidence and readiness

### 2.1 The event-loss mechanism (code-verified)

- The daemon's event log is a **bounded** broadcast: `EventLog::new(capacity)`
  creates `broadcast::channel(capacity)` and keeps a ring buffer of the same
  capacity (`src/core/event_log.rs:85-96`).
- The socket transport's raw event forwarder subscribes to that broadcast. Its
  lag arm is `src/core/transport/daemon_socket.rs:1255-1257`:

  ```rust
  Err(broadcast::error::RecvError::Lagged(n)) => {
      tracing::warn!("Event forwarder lagged, {} events dropped", n);
  }
  ```

  The dropped events are **not** re-delivered, and nothing tells the consumer.
  The loop continues as if the stream were complete.
- The ACP adapter subscribes through that path (`src/acp.rs:467`,
  `*events = connected.subscribe();`) and turns two projection events into two
  ACP notifications per tool call (`src/acp.rs:779-780`):
  `ProjectionEvent::ToolStarted` → `sessionUpdate: "tool_call"`, and
  `ProjectionEvent::ToolCompleted` → `sessionUpdate: "tool_call_update"`.
- `src/acp.rs` contains **no** `Lagged` handling at all. `drain_event_floor`
  (`src/acp.rs:421-427`) only reads what has already arrived. So the ACP
  adapter cannot distinguish "the tool never finished" from "the completion
  notification was dropped", and a consumer that pairs `tool_call` with
  `tool_call_update` can be left with an unpaired start.

The loss is therefore conditional on the consumer falling behind the bounded
broadcast — that is a property of the mechanism, not a measured rate.

### 2.2 What is already fixed, and what is not

Two harness-side halves of this defect were already fixed in C003 and stay
fixed:

- the harness counted tool notifications **before** draining the ACP stream, so
  the same turn could report a different number of tool calls from run to run
  (`plans/closure/harness-lab-post-pilot-corrective/003-evidence.md` §6, defect
  2);
- an `infrastructure_error` — "the harness, the daemon or the transport failed
  before the trial could produce an observation" — was scored as a **candidate
  failure**. It is now an **unmeasured** trial: it is excluded from the index
  and the pair is re-run, with the measured row the one that counts
  (`dev/harness_lab/improve.py:795-817`).

What remains, and what this milestone owns:

1. the product-side drop is still silent — the consumer is never told;
2. the live **rate** of the flake has never been measured. One
   `infrastructure_error` was observed in C003's live run; its cause was never
   diagnosed, and the C004 sustained scenario (24 trials) reported no rate.
   **An unmeasured rate is not a rate of zero.**
3. the re-run is not bounded and not visible. Within one invocation a pair is
   attempted at most once, but on every **resume** an unmeasured pair is
   re-attempted with no aggregate cap, the receipt does not say how many
   attempts a pair needed, and each attempt spends a provider request against
   the admitted budget without the operator seeing that extra spend.

### 2.3 The missing cumulative deadline (code-verified)

- `optimize` is the **only** command with a persisted cumulative deadline. It
  writes `inputs["deadline_unix"] = time.time() + max_wallclock` on a fresh
  run and, on a resume, **restores** the stored value and refuses a checkpoint
  that has none (`dev/harness_lab/improve.py:1464-1476`). It then threads
  `remaining = deadline_unix - time.time()` into each candidate.
- `qualify_candidate` starts its own clock at `started = time.monotonic()`
  (`dev/harness_lab/improve.py:1060`) and bounds the build, the compile
  correction, the gates and the ACP trials with
  `max_wallclock - (time.monotonic() - started)`
  (`dev/harness_lab/improve.py:1134,1141,1150,1153,1165,1184`). On
  `--resume` that clock is **new**: `Checkpoint.resume` restores `state`,
  `base_revision`, `suite_digest` and `attempt_id` and **no** elapsed-time
  accounting (`dev/harness_lab/improve.py:184-226`).
- `evaluate` is the same shape: `started = time.monotonic()`
  (`dev/harness_lab/__main__.py:551`) and
  `remaining = args.max_wallclock - (time.monotonic() - started)`
  (`dev/harness_lab/__main__.py:576`). On `--resume-candidate` the already
  measured pairs are reloaded (`:554-570`) and the pairs that were *not*
  measured are re-attempted — but the wallclock the interrupted invocation
  already consumed is **not** restored.

So `qualify --resume` and `evaluate --resume-candidate` each hand the resumed
qualification a **fresh full-length** wallclock budget. The declared budget is
not the budget that was actually consumed, and the total a single candidate
qualification may consume grows with the number of resumes.

### 2.4 Why prior verification missed both

The harness's verification is **offline and deterministic**: the Python suite
runs the controller against fixtures and a fake provider, and the Rust suite
runs the product against local fixtures. Neither exercises a consumer that
falls behind a busy bounded broadcast, and neither resumes a qualification
*mid-wallclock* — the interruption proofs kill a process between trials, at a
point where the remaining budget was still ample. A budget that is only ever
resumed with budget to spare cannot fail to be cumulative.

## 3. Invariants and non-goals

- **A dropped event must be observable.** No consumer may be told "this stream
  is complete" when the daemon dropped part of it.
- **A gap must not be silently absorbed.** When the ACP stream has a gap, the
  affected tool call is unmeasured. It must never be reported as completed.
- **An unmeasured trial is never zero, and never a pass.** It stays unmeasured,
  it stays excluded from the score, and it stays visible in the receipt.
- **A declared budget is the budget that was consumed.** The cumulative
  wallclock of a qualification is measured from the start of the *first*
  attempt, across every resume, and is bounded by the declared budget.
- **No forged measurement.** The cumulative deadline is derived from recorded
  evidence (the checkpoint's own event log), not from a value the caller
  asserts. A checkpoint that cannot account for its elapsed time is a refusal,
  not a fresh budget.
- **The score contract is not in scope.** It did the right thing with the input
  it was given. Changing the contract, the criterion set, or the acceptance
  rule is out of scope.
- **The frozen suite, the frozen digests and the frozen taxonomy manifest are
  not in scope.** No change to a case, a prompt, an expected terminal or a
  manifest byte may be made to make a measurement succeed.
- **No live provider call without an operator-approved route and a C001-verified
  spend authority.** The live measurement in §4.2 is manual and separately
  labelled, exactly as C004's was.
- **No candidate code is merged or pushed** (plan §9 of the C004 plan).

## 4. Work packages

### 4.1 Make the daemon's event loss observable

*Intent:* a bounded-broadcast lag must reach the consumer as a fact, not as
silence.

*Changes:* the forwarder's lag arm must stop being a bare `tracing::warn!`.
The implementer chooses the seam **from the reproduction**, and records the
choice:

- **(a) re-delivery** — the forwarder catches the subscriber up from the event
  log's existing ring buffer, so a lagged subscriber resumes at the next
  undelivered event rather than at "now". This requires no protocol change;
- **(b) an explicit gap** — the client is told that N events were dropped, via
  a namespaced optional notification or an existing resync/error surface, so
  the consumer can mark the affected span unmeasured;
- **(c) neither** — only if the reproduction shows the loss is unreachable from
  any consumer, in which case the finding is recorded as unreachable and the
  consumer-side defence below carries the milestone.

Whichever seam is chosen, the following must hold: the ACP adapter can
distinguish "the tool never finished" from "the completion notification was
dropped"; an unpaired `tool_call` is never reported as a completed tool call;
and the event ordering guarantee for delivered events is unchanged.

*Acceptance evidence:* a Rust test that drives the event bus past its capacity
with a deliberately slow consumer and proves (i) the drop is reported to the
consumer, (ii) no tool call is reported completed without its completion, and
(iii) the delivered events remain ordered. The test must **fail** against the
current code — a test that passes before the change proves nothing.

### 4.2 Measure the flake rate

*Intent:* replace "one observed `infrastructure_error`" with a rate, or with an
explicit statement that the rate could not be established.

*Changes:* two measurements, reported separately:

- a **deterministic** one, from §4.1's reproduction, that states the load
  condition under which the loss occurs (how far behind the consumer must fall)
  — this is a property of the mechanism and is reproducible without a provider;
- a **live** one, under the sustained scenario, that reports how many trials
  ended in an unmeasured terminal and why, per trial.

Where the live rate cannot be established — a small, cheap route may produce
too few trials to estimate a rate — the closure records **"not established"**
with the denominator. It does not record zero.

*Acceptance evidence:* a recorded artifact naming the observation condition,
the denominator, and the per-trial outcomes.

### 4.3 Bound and disclose the harness's re-run

*Intent:* the unmeasured-trial re-run must be bounded, budget-aware and
visible, so a flaky route cannot quietly spend the budget and quietly inflate
the work.

*Changes:* the harness gains a declared cap on measurement-failure re-runs
per pair, per invocation and across resumes. The cap is enforced against the
**provider-request** budget, not only against a count. The receipt reports, per
pair: the number of attempts, the number of measurement failures, and the
terminal of the measured row. A pair that exhausts the re-run cap is recorded
as **unmeasured and unresolved** — never as a pass, never as a zero, and never
as a silently dropped trial. A retried trial must not add a second row to the
**denominator**; the ledger keeps the unmeasured row for auditability, and the
score continues to use the measured row only.

*Acceptance evidence:* a test that a pair which fails with an
`infrastructure_error` more times than the cap is reported as unresolved, and
that the extra attempts are counted against the request ceiling.

### 4.4 Make the qualification's wallclock budget cumulative

*Intent:* one candidate qualification consumes at most the budget it declares,
however many times it is resumed.

*Changes:* the cumulative deadline moves out of a process-local `started` and
into the checkpoint's own append-only event log, so that a resume **restores**
the consumed wallclock instead of restarting it. Concretely:

- the `qualify` and `evaluate` commands must record their consumed wallclock
  into the artifact they already persist (the candidate checkpoint's event log
  and the ledger's trial rows), and must restore it on a resume;
- the persisted deadline must be derived from **recorded** evidence — the
  checkpoint's own accumulated elapsed time — and not from a value the caller
  supplies;
- a checkpoint that carries no elapsed-time accounting must be a **refusal**
  with a named reason, not a silent fresh budget. This mirrors the existing
  `deadline_unix` refusal in `optimize`
  (`dev/harness_lab/improve.py:1472-1473`), which must keep working;
- the `optimize` command's `deadline_unix` remains authoritative for the outer
  run and must not be replaced by a second, weaker mechanism.

The clock used for the cumulative accounting must be monotonic. The existing
`deadline_unix` is a unix-time deadline; if it is kept, the change must record
how it behaves across a wall-clock adjustment, and the inner accounting must
not depend on wall-clock time.

*Acceptance evidence:* a test that qualifies a candidate, interrupts it, resumes
it, and proves the resumed invocation's **total** consumed wallclock is bounded
by the declared budget — including the case where the interruption happens
after the budget is nearly spent. The test must fail against the current code.

### 4.5 Give the sustained scenario a cumulative bound

*Intent:* the sustained scenario's only cumulative bound today is its
provider-request ceiling; a scenario whose route is slow and free could run for
a long time without crossing it.

*Changes:* the scenario gains an optional cumulative wallclock cap, measured
from the **start of the scenario** — not from each trial. The cap is optional
so that an existing receipt's meaning does not change, and the receipt records
the cap when it is set.

*Acceptance evidence:* a test that a scenario exceeding its cumulative cap
stops with a named reason and records the trials it did not attempt.

## 5. Failure, restart and contention semantics

- An interrupted qualification is still recorded as `interrupted_unknown` and
  is never silently replayed. The cumulative deadline accounting must not
  change that: an interrupted attempt's elapsed time is **counted** against the
  budget, and its outcome remains unknown.
- A resumed qualification that cannot restore its cumulative accounting is
  **refused** with a named reason. It does not fall back to a fresh budget.
- A measurement-failure re-run that is itself interrupted is recorded as
  unmeasured and interrupted; the re-run cap is not consumed by an attempt that
  produced no observation, and it is not replenished by the resume.
- Two qualifications of the same candidate must not share a deadline. Each
  candidate's cumulative accounting is its own.
- A live measurement whose lane daemon fails to start, whose socket path does
  not fit the platform's `sun_path` limit, or whose budget admission is refused
  is recorded as `refused` with the reason and is counted in no direction.
- Concurrent runs against the same lane root are refused, exactly as the
  comparison runner refuses them today.

## 6. Required tests and verification

- `python3 -m unittest discover -s dev/harness_lab/tests -v` — the new
  re-run bound, the cumulative-deadline accounting, and no regression in the
  276 existing tests.
- The §4.4 test must **fail** against the current code. A test that passes
  before the change is not evidence; the closure record must show the failing
  run.
- The §4.1 Rust test must **fail** against the current code, for the same
  reason.
- `cargo test -p codegg-core` and `cargo test -p codegg` for the touched
  product surfaces; `cargo clippy --workspace --all-targets -- -D warnings`;
  `cargo fmt --all -- --check`.
- `scripts/verify.sh quick` passes.
- The projection-transport guards
  (`scripts/check_projection_transport_isolation.py`,
  `scripts/check_projection_transport_lifecycle.py`,
  `scripts/check_projection_disclosure.sh`,
  `scripts/check_projection_publication_seam.sh`) and
  `scripts/check_websocket_bounds.py` stay green — the ACP adapter's event
  consumption is a projection transport surface.
- The live measurement is manual and separately labelled: its receipt names the
  route, the binary's SHA-256, the admitted budget, the denominator and the
  per-trial outcomes. It is not part of CI.

## 7. Acceptance criteria

1. A bounded-broadcast lag is reported to the consumer, and an unpaired tool
   call is never reported as completed. Proven by a test that fails against the
   current code.
2. The live flake rate is recorded with its denominator, or recorded as **not
   established** with the reason. It is never recorded as zero.
3. Measurement-failure re-runs are bounded, are charged against the
   provider-request ceiling, and are reported per pair in the receipt. A pair
   that exhausts the cap is unresolved, not a pass.
4. A qualification resumed after an interruption consumes, in total, no more
   than its declared wallclock budget — proven by a test that fails against the
   current code.
5. A checkpoint with no elapsed-time accounting is refused, not silently given
   a fresh budget.
6. `optimize`'s persisted `deadline_unix` still refuses a checkpoint without
   one, and no second, weaker deadline mechanism has been introduced.
7. The frozen suite digests, the taxonomy manifest bytes and the score contract
   are unchanged, and the closure record says so.

## 8. Compatibility and migration

The product-side change alters what a consumer observes when the daemon drops
events. If the chosen seam is an ACP-visible notification, it is an **optional,
namespaced** addition under the addendum's protocol rule: the ACP v1 wire shape
gains no required field, and an existing consumer that ignores it keeps working
— but such a consumer is then exposed to the same silent gap, and the closure
record must say which consumers exist.

The harness-side change alters the meaning of `--max-wallclock` for `qualify`
and `evaluate`: it becomes a **cumulative** budget across resumes rather than a
per-invocation one. An existing invocation that is not resumed is unaffected. A
checkpoint written by the current code carries no elapsed-time accounting and
will therefore be **refused** on resume — this is a deliberate, recorded
behaviour change, and the closure record must name it.

The ledger gains no new required row shape. If the re-run accounting is
recorded in the trial rows, an old row that lacks it must be read as "unknown",
never as "zero attempts".

## 9. Documentation and static guards

- `architecture/acp.md` documents the gap-reporting seam and what a consumer
  must do with it.
- `architecture/bus.md` documents the bounded-broadcast lag behaviour and the
  delivery guarantee that now holds.
- `architecture/session.md` or the harness's own README documents the
  cumulative-deadline semantics for `qualify` and `evaluate`.
- `dev/harness_lab/README.md` documents the re-run bound and the cumulative
  budget, and states that an unmeasured trial is never zero.
- `plans/closure/harness-lab-post-pilot-corrective/004-status.md` is updated
  with the outcome and the unblock audit.
- `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md` §8 and
  `plans/registry.md` are updated in the same commit.

## 10. Handoff and closure evidence

The closure record `plans/closure/harness-lab-post-pilot-corrective/006-status.md`
must carry: the requirement-to-evidence matrix; the exact SHA; the tests run
with their results, **including the failing runs against the unfixed code**;
the chosen gap-reporting seam and why it was chosen; the measured or
explicitly-unestablished flake rate with its denominator; the re-run bound and
how it is charged; the cumulative-deadline mechanism and the refusal path; the
per-trial live outcomes; and an audit of the registry's blocked work for
anything this unblocks.

**Stop conditions.** Stop and report rather than improvise if: the event loss
cannot be reproduced and its rate cannot be established — record "not
established" with the attempts made, and do not report zero; the only available
seam would change the ACP v1 wire shape in a way that breaks a required field;
the cumulative accounting cannot be derived from recorded evidence and would
have to be asserted by the caller; the fix would require changing a frozen
suite, a frozen digest, the score contract or the taxonomy manifest; or the
approved route's budget cannot be admitted.
