# Harness Lab Post-Pilot Corrective Milestone 006 — Closure Status

Status: conditionally closed
Source implementation plan: `plans/implementation/harness-lab-post-pilot-corrective/006-live-trial-integrity-and-cumulative-resume-deadline.md`
Source subsystem roadmap: `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md`
Repository baseline reviewed: `1ee4e3aa2`
Implementation commits: see §3

## 1. Executive finding

**The product half landed and is verified. The measurement half did not run.**

The daemon's event loss is now a **fact the consumer can see**, and the
harness now treats a trial whose observation had a hole in it as
**unmeasured** rather than as a candidate failure. The harness's
measurement-failure re-runs are bounded and disclosed, the qualification's
wallclock budget is **cumulative** across resumes and derived from recorded
evidence, and the sustained scenario has an optional cumulative wallclock
cap.

The **flake rate itself was not measured**: §4.2 asks for a rate with its
denominator, and the live denominator — the number of live trials in which the
event stream actually lost events — was not available in this window. The
rate is therefore recorded as **not established**, with the attempts made, and
never as zero.

## 2. Requirement-to-evidence matrix

| Plan requirement | Status | Evidence |
|---|---|---|
| §4.1 The forwarder's lag arm stops being a bare `tracing::warn!` | met | `src/core/transport/daemon_socket.rs`: the lag arm now sends an `event_stream_gap` frame to the client |
| §4.1 The seam is chosen from the reproduction and recorded | met | §3.1; the re-delivery option was **ruled out** by measurement, not by preference (§3.2) |
| §4.1 The ACP adapter can distinguish "the tool never finished" from "the completion notification was dropped" | met | `src/acp.rs`: the adapter subscribes loss-aware and emits the optional `codegg_event_gap` session update; the turn's result carries `codeggEventGap` |
| §4.1 An unpaired `tool_call` is never reported as a completed tool call | met | The adapter reports the gap; the harness treats a trial with a gap as unmeasured and never as completed |
| §4.1 The event ordering guarantee for delivered events is unchanged | met | `the_delivered_events_stay_ordered_across_a_gap` |
| §4.1 A failing-first Rust test | met | Both new tests fail against the unfixed forwarder and pass against the fixed one — §4.2 |
| §4.2 The flake rate, with its denominator | **not established** | §10 |
| §4.3 A declared cap on measurement-failure re-runs, per pair, across resumes | met | `dev/harness_lab/__main__.py`: `--max-measurement-retries`; `ledger.measurement_failures_by_pair` |
| §4.3 The cap is enforced against the provider-request budget | met | Every attempt is admitted through the spend guard, which refuses **before** the ceiling is crossed |
| §4.3 The receipt reports, per pair, the attempts, the measurement failures and the terminal of the measured row | met | The evaluate command's trial entries carry `attempts`, `measurement_failures`/`measurement_failures_before` and `measured_terminal` |
| §4.3 A pair that exhausts the cap is unmeasured and unresolved — never a pass, never a zero, never a silently dropped trial | met | The evaluate command's `unresolved` list; `result["ok"] = False` |
| §4.4 The cumulative wallclock is recorded in the artifact the command already persists | met | `Checkpoint._save` records `elapsed_seconds` and `trials_covered` in the checkpoint's own event log; the ledger's trial rows carry the trial durations |
| §4.4 The cumulative deadline is derived from recorded evidence | met | `improve.restored_elapsed_seconds` and `ledger.resumed_candidate_elapsed_seconds` |
| §4.4 A resumed qualification that cannot restore its cumulative accounting is refused | met | `Checkpoint.resume` raises with a named reason |
| §4.5 The sustained scenario's cumulative wallclock is bounded | met | `sustained.run_sustained_scenario(scenario_wallclock=…)`; `--scenario-wallclock` |
| §6 The ACP wire shape gains no required field | held | The gap is an **optional** session-update kind and an **optional** result field; an existing consumer that ignores both keeps working |
| §7.1 A failing-first test | met | §4.2 |
| §7.2 A test that the drop is reported, that no tool call is reported completed without its completion, and that delivered events stay ordered | met | §4.2 |
| §7.3 A test that the harness classifies a gapped trial as unmeasured rather than failed | met | `dev/harness_lab/tests/test_measurement_budget.py::UnmeasuredTrialTests` |
| §7.4 A test that the re-run bound is enforced and disclosed | met | `MeasurementFailureTests`, `CheckpointAccountingTests` |
| §7.5 A test that the cumulative deadline is enforced across a resume | met | `CheckpointAccountingTests` |
| §8 No frozen suite, digest, score contract or taxonomy manifest was changed | held | No manifest, score contract or taxonomy byte changed in this milestone |

## 3. Production implementation evidence

### 3.1 The seam — an explicit gap (option b)

The daemon's event log is a **bounded** broadcast. A connection that falls
behind it loses events, and until this milestone the loss was a bare log line.
The consumer's stream simply continued as if it were complete, so a dropped
tool completion was indistinguishable from a tool that never finished.

The seam is an **explicit gap**, reported over an existing frame surface:

- `crates/codegg-protocol/src/frames.rs` — `EVENT_STREAM_GAP_CODE`, the
  message builder and the message reader. The code is namespaced; the
  dropped count is carried in the message body so no wire shape gains a
  field.
- `src/core/transport/daemon_socket.rs` — the forwarder's lag arm sends
  `CoreFrame::Error { request_id: None, code: "event_stream_gap", … }`.
  `CoreFrame::Error` already existed, so **no consumer's wire shape changed**:
  a consumer that ignores error frames keeps working unchanged, and one that
  reads the code learns that its stream is incomplete.
- `crates/codegg-client/src/local.rs` — the reader recognises the gap frame
  and publishes the dropped count on a dedicated channel. The loss-aware
  `subscribe_events` now carries both conditions: `ClientEvent::Gap` (the
  daemon dropped events) and `ClientEvent::Lagged` (this consumer fell behind
  the shared broadcast).
- `crates/codegg-client/src/driver.rs` — the projection driver answers a
  `Gap` exactly as it answers a `Lagged`: an authoritative resync. The two
  are the same fact seen from the two ends.
- `src/acp.rs` — the ACP adapter subscribes loss-aware, emits the optional
  `codegg_event_gap` session update, records the gap on the running turn, and
  carries the count in the turn's result as `codeggEventGap`.

### 3.2 The option that was ruled out — re-delivery

`EventLog::replay_ring_from_any` was written to re-deliver the missed events
from the ring buffer and is **not** used for that. The measurement: the ring
and the broadcast have the same capacity and the ring is written first, so
both evict the **same** window. A subscriber that lagged by `n` missed the
`n` oldest undelivered events, and the ring has already evicted them. A
re-delivery from the ring would re-send the events the broadcast still holds
and **not** the ones that were lost — a duplicate, not a repair.

The method is kept because it serves the cases it actually serves (a
reconnect that resumes from a cursor, a test that needs the events a log
published), and its doc comment states the boundary so the next implementer
does not repeat the attempt. `EventLog::ring_floor_seq` reports the boundary
directly.

### 3.3 The harness — `dev/harness_lab/`

- `improve.Checkpoint` accumulates the qualification's consumed wallclock and
  records it, with the trial count it covers, in the checkpoint's own
  append-only event log. `Checkpoint.resume` restores it and **refuses** a
  checkpoint that carries no elapsed-time accounting.
- `improve.restored_elapsed_seconds` derives the consumed figure from the
  checkpoint's record plus the measured durations of the trials that ran
  after the checkpoint's last transition. The qualification's local clock is
  shifted back by that figure, so every remaining-budget computation is
  cumulative.
- `ledger.measurement_failures_by_pair` and `ledger.attempts_by_pair` count
  the pair's history from the ledger's own rows, so the count survives a
  resume.
- `__main__.py`'s `evaluate` gains `--max-measurement-retries` (default 2),
  enforces the cap before the trial, and records an exhausted pair in the
  result's `unresolved` list with `result["ok"] = False`.
- `__main__.py`'s `evaluate --resume-candidate` restores the consumed
  wallclock from the ledger's trial rows.
- `sustained.run_sustained_scenario` gains an optional cumulative
  wallclock cap that stops the scenario before it crosses the limit and caps
  the per-trial timeout by what is left.
- `autonomy_run` classifies a trial whose stream reported a gap as
  `infrastructure_error` — **unmeasured** — and `_success_rate` excludes
  unmeasured trials from its denominator rather than counting them as
  failures.

## 4. Verification executed

| Command | Result |
|---|---|
| `cargo test -p codegg --lib core::transport::daemon_socket::tests::` | 15 tests, OK — local |
| the same tests against the **unfixed** forwarder | **2 failed** — the failing-first evidence, §4.1 |
| `python3 -m unittest discover -s dev/harness_lab/tests` | 316 tests, OK (1 skipped) — local |
| `bash scripts/verify.sh quick` | see §4.2 — local |
| `cargo fmt --all` | clean — local |

### 4.1 The failing-first evidence

With the forwarder's lag arm reverted to the pre-fix `tracing::warn!`:

```
test core::transport::daemon_socket::tests::a_lagged_forwarder_reports_the_gap_to_the_client ... FAILED
test core::transport::daemon_socket::tests::the_delivered_events_stay_ordered_across_a_gap ... FAILED

---- a_lagged_forwarder_reports_the_gap_to_the_client stdout ----
panicked at src/core/transport/daemon_socket.rs:1558:41:
a lagged forwarder must report the gap to the client; before the change the events were dropped silently

---- the_delivered_events_stay_ordered_across_a_gap stdout ----
panicked at src/core/transport/daemon_socket.rs:1627:9:
the forwarder must report the lag it observed

test result: FAILED. 13 passed; 2 failed; 0 ignored; 0 measured; 7274 filtered out
```

With the fix in place both pass. The other 13 tests in the module were
unaffected by the revert, so the two failures are attributable to the lag arm
and to nothing else.

### 4.2 `scripts/verify.sh quick`

See the commit message for the recorded result.

## 5. Invariant review

| Invariant | Status |
|---|---|
| No ACP v1 field became required | held — the gap is an optional session-update kind and an optional result field |
| No consumer's wire shape changed | held — the gap rides the existing `CoreFrame::Error` variant |
| The event ordering guarantee for delivered events is unchanged | held — asserted by a test |
| A missing measurement is never read as a zero | held — the flake rate is `null`, the control metric is `None`, an unmeasured duration contributes nothing |
| A measurement failure is never scored as a candidate failure | held — unmeasured trials are excluded from the denominator and the pair is re-run |
| The re-run is bounded and disclosed | held — the cap is declared, enforced and reported |
| The wallclock budget is cumulative and derived from recorded evidence | held — the checkpoint's own record plus the ledger's measured durations |
| A checkpoint without elapsed-time accounting is a refusal | held — named error, no fresh budget |
| No frozen suite, digest, score contract or taxonomy manifest changed | held |
| No candidate code is merged or pushed | held |

## 6. Failure and recovery review

- A gap does not fail the turn. The turn's own outcome is still reported, and
  the gap is reported **alongside** it: the consumer decides what an
  incomplete observation means, and the adapter's job is to say that it is
  incomplete.
- A gap during a turn is recorded on the running turn and carried in the
  turn's result.
- The harness treats a gapped trial as unmeasured and re-runs the pair under
  the §4.3 cap. A pair that exhausts the cap is unresolved — never a pass.
- The projection drivers resync on a gap exactly as they do on a local lag.

## 7. Migration and compatibility review

- An old client that ignores `CoreFrame::Error` behaves exactly as before.
- An old consumer of the ACP wire sees the same session updates plus, at
  most, one it does not recognise; no required field changed.
- The harness's control metric now excludes never-exercised controls and
  reports `None` rather than `0.0` when nothing was exercised. A comparison
  whose controls were never exercised is **undecidable** rather than accepted
  or rejected.

## 8. Security review

- The gap frame carries a count and two sequence numbers. It carries no
  credential, no prompt and no raw body.
- The namespaced code cannot collide with another error the daemon raises.
- The harness writes no secret to any receipt.

## 9. Documentation and operations

- `architecture/acp.md` documents the optional `codegg_event_gap` session
  update and the `codeggEventGap` result field, and states that an
  unmeasured observation is not a completed one.
- `architecture/bus.md` documents the bounded broadcast's loss report and the
  ring/broadcast eviction boundary.
- `dev/harness_lab/README.md` documents the harness's unmeasured-trial
  classification, the re-run bound and the cumulative budgets.

## 10. Unresolved findings

| Severity | Finding |
|---|---|
| **high** | **The flake rate was not established.** §4.2 asks for the rate at which a live trial's event stream loses events, with its denominator. The denominator is the number of live trials whose stream actually lost events, and that count was not available: no live comparison ran in this window, and the rate is therefore **not established** — **not zero**. The attempts made: the deterministic reproduction (the bus capacity and the lag threshold are known and asserted by the new tests), the product-side reporting that would make the next live run measurable, and the harness-side classification that turns a gapped trial into an unmeasured trial rather than a candidate failure. What remains unproven: the rate itself, and therefore whether the C003 flake was this loss. The next live comparison that observes a `codegg_event_gap` notification would establish both at once. |
| **medium** | **The daemon-side gap is reported but the client's own broadcast lag is only reported to the adapter.** The ACP adapter reports both, but a consumer that uses the raw `subscribe()` — not the loss-aware `subscribe_events()` — still loses events silently. The projection drivers already used the loss-aware path. Any future consumer of the raw path must be migrated, and the plan did not require that migration. |
| **medium** | **The cumulative accounting has a boundary.** The checkpoint's elapsed time covers every phase up to its last recorded transition. A hard kill *during* the trial phase leaves the completed trials' durations out of the restored figure; the ledger holds them, and a resume adds back the ones beyond the checkpoint's `trials_covered` count. A kill during the trials therefore still counts, but only through the ledger — the checkpoint alone would understate it. |
| **low** | The re-run cap defaults to 2 and is per pair. A suite where most pairs fail on measurement would exhaust the cap on many pairs and leave many unresolved; the result says `ok: false` and names them, so it fails safe. |
| **low** | The `sustained` command has no resume path, so its cumulative cap is per invocation. The cap is recorded in the receipt as `scenario_wallclock_seconds`. |

## 11. Roadmap disposition

- The addendum's F008 ("live-trial integrity and cumulative resume deadline")
  is **partly** closed: the integrity mechanism, the re-run bound and the
  cumulative deadlines are implemented and tested; the flake rate is not
  established.
- C004 stays **conditionally closed**. This milestone does not close it: the
  control's live confirmation is still absent (milestone 005 records why).

## 12. Registry updates

- `plans/registry.md`: this milestone's row moves from `ready` to
  `conditionally closed`.
- Nothing else is unblocked. No other registered plan listed C006 as a
  dependency.
