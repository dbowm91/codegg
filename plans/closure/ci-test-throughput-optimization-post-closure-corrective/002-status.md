# CI/Test Throughput Post-Closure Corrective C002 Closure — Hosted CI Timing-Flake Stabilization

Source plan: `plans/implementation/ci-test-throughput-optimization-post-closure-corrective/002-hosted-ci-timing-flake-stabilization.md`
Source addendum: `plans/subsystems/ci-test-throughput-optimization-post-closure-corrective-addendum.md`
Implementation commit: `696ec282` (CodeGG `main`)

## Disposition

C002 is closed. The controller lost-event race is fixed with
deterministic regression coverage; the Eggwork helper-readiness
boundary is phase-diagnostic with guaranteed cleanup; the
session-family consolidation is exonerated; hosted stability is
qualified with three consecutive green full runs on the final tree.
C001 is unblocked back to `ready`.

## Original failures

Main run `36215541015` (first push after the C001 live-selector fix):

- attempt 1: `session_family::control_m004_controller_lease::reaper_releases_on_turn_completed_event`;
- attempt 2: `eggwork_remote_execution_live::live_blob_upload_and_workspace_materialization`
  (60-second helper-readiness bound).

Last authoritative green before C002: `36196068239` attempt 3.

## Controller lost-event race: FIXED

Pre-fix, `CoreDaemon::spawn_turn_reaper`
(`src/core/daemon_control.rs`) called `tokio::spawn` first and created
the event-log broadcast receiver *inside* the spawned future. The
caller received no guarantee the task had reached `subscribe()` before
a terminal event was published, so an immediately published
`TurnCompleted`/`TurnFailed` could be broadcast with no receiver and
lost permanently. The integration test's one-second poll could not
repair a lost broadcast. Production lifecycle race, not a flaky
assertion.

Post-fix, the receiver is created synchronously *before*
`tokio::spawn` and moved into the task: when `spawn_turn_reaper`
returns, the terminal-event subscription already exists. The spawned
task creates no new subscription. Exact session/turn filtering,
`Lagged` handling, release-on-match, active-turn clearing, idle
transition, and terminal precedence are unchanged. No sleeps, yields,
poll enlargements, or publication retries were added.

Focused regression (`tests/session_family/control_m004_controller_lease.inc`):

- existing `reaper_releases_on_turn_completed_event` (immediate publish,
  now deterministic);
- new `reaper_releases_on_turn_failed_event` (immediate `TurnFailed`);
- new `reaper_ignores_unrelated_terminal_events` (other-session/other-turn
  terminals do not release; the matching terminal still releases after);
- new `reaper_immediate_publish_is_stable_across_repetitions` (25
  immediate-publish iterations; a lost event is near-certain under the
  pre-fix ordering, every iteration passes post-fix).

Local: `cargo test --test session_family -- --test-threads=1` 96/96;
`cargo nextest run --locked --profile ci -E 'binary(session_family)'`
96/96. Hosted: zero reaper failures across all qualification runs.

## Eggwork readiness flake: BOUNDED WITH EVIDENCE (diagnostic)

No Eggwork startup code was changed (no timeout increase, no
weakened mTLS/Landlock/loopback/authorization, no mock, no retry).
WP3 instrumentation instead:

- Helper (`crates/eggwork-test-node/src/main.rs`) prints test-only
  `PHASE` markers on stdout (`args-parsed`, `tls-ready`,
  `storage-ready`, `server-starting`) before the existing `READY
  port=<N>` line. Stage names only; never paths, identities, or key
  material. The harness parser already tolerated non-`READY` lines.
- Harness (`tests/eggwork_remote_execution_live.rs`) keeps the
  overall 60-second bound and now reports on every failure: helper
  binary path, elapsed time, child alive/exited plus exit status, last
  stdout line, last `PHASE`, and a bounded 2048-char stderr tail. On
  timeout the child is explicitly killed and reaped before failing, so
  a failed qualification cannot leak a helper process. Secret hygiene:
  diagnostics carry paths/stages/tails only; PEM, keys, and bearer
  material never enter these channels.

WP4 characterization: the original 60-second stall did **not**
reproduce in any hosted execution after instrumentation — the M003
qualification run plus all three C002 stability runs below ran the
full live suite (each starts the helper repeatedly) with zero
readiness timeouts. With no reproduced stall there is no phase
evidence justifying a timeout change, so the 60-second failure bound
is retained unchanged. If the stall recurs, the new diagnostics
capture the exact phase and child state by construction. This is the
WP4 no-reproduction outcome: multiple hosted executions bound normal
startup below the retained bound.

## Session-family consolidation: EXONERATED

Focused evidence (`cargo test --test session_family --
--test-threads=1` and the nextest `ci` selector, both 96/96):

- no shared process-global state: zero `static` items across the five
  modules; the only env mutation (`OPENAI_API_KEY` sentinel) is
  `lock_env()`-guarded with restore;
- per-test in-memory SQLite pools (no shared files, no temp-path
  collisions);
- the reaper failure is fully explained by the production
  subscription race above.

The `session_family` consolidation stays intact; no split-back.

## Hosted stability: QUALIFIED

Three consecutive green full hosted `CI / verify` executions on the
same final candidate tree (`696ec282`), live Eggwork included, no
sccache experiment, no retries-until-green, no superseded runs in the
chain:

- run `36748429660` attempt 1: success;
- run `36748429660` attempt 2 (rerun, same tree): success;
- run `36748429660` attempt 3 (rerun, same tree): success.

No controller-reaper failure and no helper-readiness timeout in any
attempt. Test scope consistent (~11.7k tests incl. live suite).
The pre-existing lint-fix run `36740881458` (superseded, unrelated
`eggpool` cancellation-registration flakes — see below) does not
interrupt this chain: it ran on a different tree.

## Local verification

- `cargo test --test session_family` (both runners): 96/96.
- `scripts/verify.sh quick`: passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`:
  clean. `cargo fmt --all -- --check`, `git diff --check`: clean.
- Focused live Eggwork runs locally are Linux-only; hosted runs above
  carry the live signal.

## Unresolved findings

- `eggpool` cancellation-registration flakes
  (`cancellation_compensates_operation_owned_credential` ×2) appeared
  **once** on the unrelated lint-fix run `36740881458` (attempt 1) and
  pass locally. They touch no C002 or M003 code path and did not
  reproduce in the M003 qualification run or any of the three C002
  stability runs. Recorded as a new, currently non-repeating hosted
  observation for future corrective triage — explicitly NOT a C002
  stop-condition trigger (single occurrence, causally unrelated
  boundary, stability chain unaffected). If it repeats, it owns a
  separate corrective, not a C002 reopen.

## C001 handoff

C002 is closed. C001 (closure evidence, cache qualification, doc
reconciliation) is unblocked back to `ready` with the standing
constraint: sccache A/B and unrelated-PR timing measurements must cite
only post-C002 green baselines, never red or diagnostically ambiguous
runs.
