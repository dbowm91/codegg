# CI/Test Throughput Post-Closure Corrective C002 — Hosted CI Timing-Flake Stabilization

Status: closed

Closure record: `plans/closure/ci-test-throughput-optimization-post-closure-corrective/002-status.md`

Repository baseline reviewed: `9c88b9b80852022b39daa80a6b9fb7797fcaa824`

Source corrective addendum:

- `plans/subsystems/ci-test-throughput-optimization-post-closure-corrective-addendum.md`

Blocks:

- C001 closure evidence, cache qualification, and documentation reconciliation.

Primary class: correctness / development infrastructure.

## 1. Objective

Restore a trustworthy green hosted-CI baseline before C001 performs unrelated-PR timing or compiler-cache measurements.

The first main push after the C001 live-selector fix, run `36215541015`, failed twice on two different timing-sensitive paths:

- attempt 1: `session_family::control_m004_controller_lease::reaper_releases_on_turn_completed_event`;
- attempt 2: `eggwork_remote_execution_live::live_blob_upload_and_workspace_materialization`.

Post-failure inspection shows that the controller-reaper failure exposes a real production lost-event race, while the Eggwork failure is a previously observed hosted helper-startup flake whose current timeout path is not sufficiently diagnostic.

C002 must correct and qualify those two boundaries without weakening assertions, deleting coverage, broadening CI concurrency, or using arbitrary timeout inflation as the primary fix.

## 2. Current evidence

### 2.1 Controller reaper has a subscription-readiness race

Current `CoreDaemon::spawn_turn_reaper` in `src/core/daemon_control.rs` calls `tokio::spawn` first and creates the event-log broadcast receiver inside the spawned future:

```text
pub fn spawn_turn_reaper(...) {
    ...
    tokio::spawn(async move {
        let mut rx = event_log.subscribe();
        ...
    });
}
```

The caller receives no guarantee that the spawned task has reached `subscribe()` before a terminal event is published.

The failing integration test does exactly this sequence:

1. call `spawn_turn_reaper(...)`;
2. immediately publish `TurnCompleted`;
3. poll the controller store for approximately one second.

If the publish wins the scheduling race, the broadcast event has no receiver and is permanently lost. The one-second poll cannot repair that correctness failure.

This is a production lifecycle race, not merely a flaky assertion.

### 2.2 Eggwork helper readiness failure is under-diagnosed

`LiveNode::start`:

1. creates test TLS material and roots;
2. asserts/builds the prebuilt helper;
3. spawns `codegg-eggwork-test-node`;
4. captures stdout/stderr;
5. waits up to 60 seconds for `READY port=<N>`.

The helper itself prints `READY` only after `NodeServer::start(...).await` completes.

On timeout, the harness currently uses:

```text
tokio::time::timeout(...).await.expect("helper must become ready")
```

so the timeout panic omits the captured stderr, last stdout line, child status, and startup phase. Those details are only reported when stdout closes before readiness.

The same `live_blob_upload_and_workspace_materialization` readiness timeout was already observed during M004 closure. The M002 fixture prebuild itself normally completes in a few seconds, so the unresolved delay is after helper spawn and before the readiness line.

### 2.3 Hosted evidence to preserve

- PR run `36213918571` for the selector fix was green, but the changed detector path itself is live-relevant, so it is not C001's unrelated-PR fast-path baseline.
- Main run `36215541015`, attempt 1: failed `reaper_releases_on_turn_completed_event`.
- Same run, attempt 2: passed the reaper test and later failed `live_blob_upload_and_workspace_materialization` after the 60-second helper-readiness bound.
- The predecessor main/live baseline `36196068239` attempt 3 remains the last authoritative green full run.

## 3. Invariants and non-goals

C002 MUST preserve:

- one ordinary `CI / verify` job;
- `CARGO_BUILD_JOBS=4`;
- Nextest `ci` at 4 slots;
- whole-binary exclusivity for the existing heavy binaries unless C002 evidence specifically requires stricter isolation;
- M002 live relevance policy: relevant PRs and main qualify live Eggwork; unrelated PRs may omit it;
- M003/M004 consolidated integration targets;
- all existing controller-lease semantics: terminal events release only the matching turn lease, terminal wins over later transfer/takeover, disconnect behavior remains unchanged;
- the real Eggwork `NodeServer` + `LocalProcessRunner` + TLS/mTLS path;
- all assertions and test coverage;
- fail-closed behavior for actual helper startup errors.

C002 does NOT authorize:

- starting the C001 sccache experiment;
- collecting C001 performance conclusions from red or diagnostically ambiguous runs;
- deleting or skipping the flaky tests;
- converting the live Eggwork test to a mock;
- retrying a failed test automatically until green;
- raising global Nextest timeouts or reducing test parallelism repository-wide;
- arbitrary large sleeps or timeout increases without phase evidence;
- changing scheduler/session-controller product semantics beyond repairing the lost-event race;
- a second CI lane or matrix.

## 4. Work packages

### WP1 — Make turn-reaper subscription readiness synchronous

Repair `CoreDaemon::spawn_turn_reaper` so its return establishes this postcondition:

> the terminal-event receiver for the supplied session/turn already exists before the caller can publish a following terminal event.

The expected minimal shape is to create the broadcast receiver synchronously before `tokio::spawn`, then move that receiver into the spawned future. Equivalent designs are acceptable only if they provide the same explicit readiness guarantee.

Do not fix this by yielding, sleeping, enlarging the integration-test poll, or retrying publication.

Preserve:

- exact session/turn filtering;
- `Lagged` handling;
- release-on-turn-match semantics;
- active-turn clearing;
- transition to `RuntimeSessionStatus::Idle`;
- terminal event precedence.

Document the new method-level readiness contract.

### WP2 — Add deterministic controller-reaper regression coverage

Add focused coverage that would fail under the pre-C002 ordering.

Required cases:

1. spawn the reaper and immediately publish matching `TurnCompleted` with no readiness sleep/yield;
2. same for matching `TurnFailed`;
3. unrelated session/turn terminal events do not release the lease;
4. terminal release remains turn-match/CAS safe against a later controller transition where existing tests already cover that invariant.

The primary regression must assert the logical state transition, not merely wait for a long timeout.

A short bounded wait may remain only as a deadlock/failure bound after the synchronous subscription guarantee exists.

Where practical, repeat the immediate-publish scenario enough times locally to expose scheduling-order regressions without turning the test into a benchmark.

### WP3 — Make Eggwork helper startup failures phase-diagnostic

Before changing the 60-second startup bound, improve the fixture so every readiness failure reports actionable state.

At minimum, timeout diagnostics must include:

- helper binary path;
- elapsed startup time;
- whether the child is still alive or exited;
- exit status if available;
- last bounded stdout diagnostic line(s);
- bounded stderr tail;
- the last known startup phase.

Use a bounded diagnostic protocol. Acceptable implementations include test-only phase markers from `crates/eggwork-test-node` or an equivalent harness-side state model. Do not print certificates, private keys, bearer tokens, or other secret material.

Useful phases should distinguish at least:

- process spawned / arguments accepted;
- TLS/identity material parsed;
- storage/root initialization;
- runner/Landlock setup;
- `NodeServer::start` entered;
- listening/ready.

The existing parser already tolerates non-`READY` stdout lines; if phase markers are used, keep them structured and test-only.

On timeout, explicitly terminate/reap the child before panicking so a failed qualification cannot leak a helper process.

### WP4 — Characterize the Eggwork readiness stall under hosted load

Use the diagnostics from WP3 to determine where the 60-second stall occurs.

Required evidence:

- focused local live-target runs;
- at least one hosted relevant/main run with phase timing visible;
- if the failure reproduces, exact last phase and stderr tail;
- if it does not reproduce, startup-duration distribution from multiple hosted executions/reruns sufficient to bound normal startup.

Then choose the smallest evidence-supported remedy.

Permitted outcomes:

A. **Startup correctness bug found:** fix that bug and retain the existing 60-second failure bound if normal startup remains comfortably below it.

B. **Runner-load latency proven:** increase only the fixture-readiness failure bound to a documented bounded value with observed headroom. Do not change Nextest's global heavy-test timeout solely for this condition.

C. **Resource contention inside the live target proven:** make a narrowly scoped resource/isolation adjustment that preserves one routine job and does not reduce coverage.

D. **External/upstream Eggwork defect identified:** stop and register the exact upstream blocker rather than masking it locally.

A timeout increase without WP3/WP4 evidence is not acceptable closure.

### WP5 — Re-qualify the consolidated session family

Because the newly observed reaper failure occurred after M004 consolidation, verify that consolidation itself did not introduce shared-module/global-state coupling.

Run at minimum:

```bash
cargo test --test session_family -- --test-threads=1
cargo nextest run --workspace --locked --profile ci -E 'binary(session_family)'
```

Inspect the five consolidated modules for shared process-global state, static initialization, temp-path collisions, or helper-module tests that now execute differently from their pre-consolidation binaries.

If the reaper failure is fully explained by the production subscription race and no consolidation-specific coupling exists, record that negative finding and leave `session_family` intact.

Do not split the family back into separate binaries without concrete evidence.

### WP6 — Hosted stability qualification

After WP1-WP5, run the full current main/live configuration with no sccache experiment.

Required closure evidence:

- at least **three consecutive green full hosted `CI / verify` executions** on the same final candidate tree/configuration, using reruns or no-op/measurement commits only where repository convention permits;
- no controller-reaper failure;
- no Eggwork helper-readiness timeout;
- live Eggwork actually included;
- test count/scope consistent with the current baseline;
- record Clippy, build, execution, and total wall times for diagnostics only.

A superseded/cancelled run does not count. A failed run resets the consecutive-green count and must be characterized before continuing.

This is a stability qualification, not a new permanent CI gate.

### WP7 — Closure and handoff back to C001

Create:

- `plans/closure/ci-test-throughput-optimization-post-closure-corrective/002-status.md`.

The closure record must classify separately:

- controller lost-event race: fixed / blocked;
- Eggwork readiness flake: fixed / bounded with evidence / upstream-blocked;
- session-family consolidation: exonerated / corrective required;
- hosted stability: qualified / not qualified.

If C002 closes:

- mark C002 closed;
- unblock C001 back to `ready`;
- C001 resumes with planning/docs reconciliation, unrelated-PR fast-path measurement, and the sccache A/B.

If C002 cannot close, C001 remains blocked and must not begin performance experiments.

## 5. Required verification

Focused controller semantics:

```bash
cargo test --test session_family control_m004_controller_lease -- --test-threads=1
cargo nextest run --workspace --locked --profile ci -E 'binary(session_family)'
```

Focused live Eggwork:

```bash
./scripts/prebuild-eggwork-fixtures.sh
CODEGG_EGGWORK_TEST_NODE="$PWD/crates/eggwork-test-node/target/debug/codegg-eggwork-test-node" \
CODEGG_EGGWORK_FIXTURES_PREBUILT=1 \
cargo test --test eggwork_remote_execution_live -- --test-threads=1
```

Broad:

```bash
git diff --check
cargo fmt --all -- --check
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked --profile ci
```

Hosted:

- three consecutive green full `CI / verify` executions with live Eggwork included.

## 6. Failure/cancellation/restart semantics

Controller reaper:

- terminal publication immediately after `spawn_turn_reaper` returns must not be lost;
- a lagged broadcast receiver may skip stale unrelated events but must continue waiting for the matching terminal event;
- closed event log terminates the reaper without inventing a release;
- release remains conditional on matching `turn_id`;
- restart reconciliation remains the authority for interrupted persisted turns and is not replaced by this task-local reaper.

Eggwork helper:

- startup timeout remains a failure bound, not a retry loop;
- timeout cleanup must kill/reap the helper;
- an exited helper reports its status and bounded diagnostics;
- readiness must still mean the real server has started and has a bound port;
- no fallback to an in-process fake or unrestricted local execution.

## 7. Security constraints

- Helper diagnostics must never include PEM/private-key contents or credentials.
- Preserve mTLS client-auth, fingerprint principal mapping, exact operation allowlist, Landlock helper discovery, and loopback-only bind.
- Do not weaken sandbox/network policy to make the fixture start faster.
- No public-network CI.
- No new workflow write permissions or secrets.

## 8. Documentation effects

Update only as supported by final evidence:

- method comments for the synchronous reaper-readiness contract;
- `architecture/testing.md` if the live-helper timeout/resource policy changes;
- the C002 closure record;
- corrective addendum and registry status.

Do not perform C001's broader stale-timing documentation reconciliation inside C002 except where a changed runtime/test-resource contract requires it.

## 9. Acceptance criteria

C002 is complete only when:

- `spawn_turn_reaper` establishes subscriber readiness before returning;
- immediate terminal publication has deterministic regression coverage;
- no lease-release assertion has been weakened;
- Eggwork readiness timeout failures report bounded phase/stdout/stderr/child-state diagnostics;
- any readiness timeout/resource change is justified by hosted phase evidence;
- helper processes are cleaned up on timeout;
- `session_family` is either exonerated by focused evidence or a concrete consolidation defect is corrected;
- focused and broad local verification pass;
- three consecutive full hosted main/live-equivalent runs pass on the final configuration;
- no sccache experiment has begun;
- C001 is unblocked only after the above evidence exists.

## 10. Stop conditions

Stop and report rather than widen scope if:

- the Eggwork stall is inside an upstream component and cannot be corrected without an upstream release;
- fixing the helper requires weakening mTLS, Landlock, loopback binding, or operation authorization;
- the session reaper requires a larger lifecycle redesign than synchronous subscription readiness;
- a different repeated hosted flake emerges and is not causally related to these two boundaries;
- three-green qualification cannot be reached without hiding or retrying failures;
- the proposed fix requires global serialization or large timeout inflation that materially regresses CI throughput.

## 11. Closure evidence required

The C002 closure record must include:

- implementation commits;
- exact hosted run IDs/attempts for the original failures;
- code-level explanation of the pre-fix reaper race and post-fix readiness guarantee;
- focused reaper regression results;
- Eggwork startup phase evidence and timeout disposition;
- proof timeout failure cleans up the helper and preserves secret hygiene;
- session-family coupling audit result;
- focused/broad local command results;
- the three consecutive final hosted green runs;
- unresolved findings by severity;
- explicit C001 handoff state.
