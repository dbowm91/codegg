# Async Background Jobs Corrective C006 — Closure Status

Status: **closed**

Source implementation plan: `plans/implementation/async-background-jobs-corrective/006-model-facing-tool-to-idle-resume-lifecycle-qualification.md`

Source subsystem roadmap: `plans/subsystems/async-background-jobs-integration-qualification-corrective-addendum.md`

Repository baseline reviewed: `247edec5f37851ca3f67db5943675c896d19e9d9` (PR #119 branch before this qualification)

Implementation commits:

- `471202027373df507b1da4604cc908f10d682e30` — wake the daemon-owned CI idle driver after watch acceptance; add model-facing CI-watch and background-Bash lifecycle qualification; update scheduler architecture evidence.

## 1. Executive finding

C006 is closed. A normal authenticated `SessionPromptSubmit` now drives a scripted model-facing `ci_watch` call through production tool registration, `DefaultTurnRuntime`, ToolBroker, durable watch admission, the daemon-owned CI watcher, terminal fanout, an authenticated host grant, and exactly one daemon-admitted owner continuation. No test status call, synthetic terminal write, coordinator invocation, or post-origin `TurnSubmit` causes progression. A second model-facing background Bash case receives ordinary permission approval, completes through the production scheduler, publishes durable owner completion, and makes no post-terminal model call under the default notify-only policy.

The new CI lifecycle exposed one production wake gap: a watch accepted while the idle driver slept on an empty queue did not interrupt that sleep. The watch service and its driver now share one `Notify`; accepted/idempotent watches wake the driver, and new watches wake it only after durable creation/subscription succeeds. CI still owns the cadence, bounded poll budget, and all remote observation.

This closure qualifies the model-to-idle lifecycle on the current feature branch. Exact latest-main hosted `CI / verify`, Desktop E2E, and PR #119 integration remain C007 gates.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Full model-facing CI watch to authenticated idle turn | `tests/async_background_c006_tool_to_idle_ci.rs::model_facing_ci_watch_reaches_authenticated_idle_resume_without_injected_terminal` | Pass | Real `CoreDaemon`, `DefaultTurnRuntime`, registered `ci_watch`, ToolBroker and SQLite; scripted provider and GitHub are loopback fixtures. Origin turn ends before terminal is released. |
| Trusted consent binds to actual watch identity | Same C006 CI case | Pass | Host calls authenticated `ContinuationGrantCreate` only after reading the actual watch ID and `ciwatch:<watch_id>` locator. Grant binds the owner session, selected model/connection and one bounded continuation. |
| Durable terminal, live owner event and exactly one continuation | Same C006 CI case | Pass | One durable `BackgroundJobCompleted`, stable notice ID, two owner event subscribers observe the same sequence, one resumed `TurnStarted`, one durable receipt, and one provider request carrying the host-authored summary. |
| Owner isolation and replay | Same C006 CI case; `async_background_c004_live_attention` | Pass | Owner replay contains one terminal and origin plus continuation; a foreign-session filter/replay sees no completion. C004 restart/replay and duplicate-fanout cases also pass. |
| Scheduler-backed background machine job and default zero-inference | `tests/async_background_c006_tool_to_idle_ci.rs::model_facing_background_bash_completes_via_scheduler_without_autonomous_resume` | Pass | Model-facing Bash reaches ordinary `PermissionRespond`, scheduler terminal, durable live/replayed event and `JobGet=completed`; provider fixture confirms no post-terminal request. |
| Watch acceptance wakes empty-queue idle driver | `scheduler::ci_watch_idle::tests::newly_accepted_watch_interrupts_empty_queue_sleep` | Pass | The driver consumes the watch service's shared wake signal. |
| Authorization, cancellation, retry, duplicate and restart regressions | C005 (16), C004 (12), C001 idle driver (13), M004 CI watch (14) focused suites | Pass | Includes revoked/expired/mismatched grants, 50-contender wake race, ambiguous provider start without replay, restart reconciliation, expiry, selector/SHA/repository failure, cancellation, duplicate terminal and bounded backoff coverage. |
| Formatting, ownership and static guards | `cargo fmt --all -- --check`; idle-continuation callsite, scheduler-bypass and execution-ownership guards | Pass | No authority or sandbox policy was relaxed. |
| Workspace quick verification | `bash scripts/verify.sh quick` | Pass | Includes formatting, agent schema, boundaries and guards, plus `cargo check --workspace --all-targets --locked`; completed successfully. |
| Workspace Clippy | `cargo clippy --workspace --all-targets --locked -- -D warnings` | Pass | Completed with no issues. |
| Latest-main hosted qualification | Owned by C007 | Deferred | C006 does not claim the branch is merge-ready. |

## 3. Production implementation evidence

`CiWatchService` now owns a shared `Arc<Notify>` and exposes a crate-scoped clone to `CiWatchIdleDriver`. The driver no longer creates a disconnected notifier. Existing in-memory and persisted idempotent watch resolution notify the driver; new watch creation notifies only after the watch and its completion subscription are committed. The existing idle driver remains the sole polling authority and its bounded sleep remains the backstop.

The model-facing test uses the normal authenticated prompt admission and the registered tool schema delivered to the scripted OpenAI-compatible loopback transport. The production agent loop consumes the returned tool-call DTO. The GitHub loopback response includes hostile instruction-like URL text; the durable summary and resumed provider input do not promote it. Consent is issued through the normal host request gate against the observed job locator. For Bash, a real session permission is approved through `CoreRequest::PermissionRespond`; scheduler acceptance and terminal state are inspected only after production execution.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test --test async_background_c006_tool_to_idle_ci -- --nocapture
cargo test --test async_background_c005_idle_resume \
  --test async_background_c004_live_attention \
  --test ci_watch_c001_idle_driver \
  --test ci_watch_m004_completion
python3 scripts/check_idle_continuation_callsites.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
bash scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
```

### Results

- C006 integration file: **7 passed, 0 failed** (five shared secret-scan cases and both lifecycle trajectories).
- C005 idle-resume: **16 passed, 0 failed**.
- C004 live-attention: **12 passed, 0 failed**.
- C001 idle-driver: **13 passed, 0 failed**.
- M004 CI-watch completion: **14 passed, 0 failed**.
- `verify.sh quick`, Clippy, formatting and all three explicit ownership/callsite guards passed.
- No C006 exact-head hosted CI or Desktop E2E is claimed here; C007 owns those exact integration SHA gates.

## 5. Invariant review

- One `CoreDaemon`, `JobScheduler`, ToolBroker authorization boundary and daemon-owned CI observer remain authoritative.
- The test provider and GitHub service are loopback fixtures; they do not replace production scheduling, storage, grant validation, event fanout or turn admission.
- Model `resume_owner` intent does not create consent. Only the authenticated host grant permits one bounded continuation.
- Default Bash completion remains notify-only and performs no idle inference.
- Owner-scoped durable events and replay remain isolated from the foreign session.
- Watch cancellation and expiry do not cancel the GitHub workflow; bounded retries and stale identity handling remain covered by the existing focused suites.
- No scheduler bypass, ambient process permission, raw remote prompt injection, or blind replay was introduced.

## 6. Failure and recovery review

The C006 test holds terminal delivery until the origin turn is complete, then observes production polling and fanout. Existing C004/C001/M004 suites cover commit-before-hint recovery, restart/reconciliation, already-terminal subscriptions, lease recovery, bounded backoff, duplicate observations, cancellation and expired watches. C005 covers grant revocation/expiry/binding mismatch, ambiguous provider-side start as `needs_attention`, restart fencing and the 50-contender single-admission race. Logical exactly-once admission is tested; exactly-once external provider side effects are not claimed.

## 7. Migration and compatibility review

No schema, protocol, configuration, or storage-layout migration is introduced. The watch service and driver are constructed as one daemon-owned pair; the notify signal is process-local and reconstructs naturally after restart, while the accepted watch/subscription remain durable. Existing protocol and event shapes are unchanged.

## 8. Security review

The CI path uses authenticated session prompt submission and host-resolved grant creation. The Bash path retains its ordinary permission request/response. Fixtures use isolated homes and local endpoints; credentials are test sentinels. Remote workflow strings remain bounded result data and are not inserted into the resumed turn as instructions. The change does not broaden sandbox, tool, or network authority.

## 9. Documentation and operations

- Updated `architecture/scheduler.md` to specify the service-shared wake and post-commit notification point.
- Added `tests/async_background_c006_tool_to_idle_ci.rs` with both complete model-facing trajectories.
- Existing quick verification and ownership guards remain green.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | Exact latest-main hosted `CI / verify`, Desktop E2E, full PR diff, and PR metadata qualification are not part of C006's owned evidence. | C006 is not a merge-readiness or release-readiness claim. | Complete under C007 on the exact integrated PR #119 SHA. |
| Low | Live paid-provider behavior, public GitHub API behavior, Windows enforced sandboxing, and other native OS lanes were not exercised by these local loopback tests. | Those external/platform properties remain unqualified. | No action within C006; preserve as explicit nonclaims for later platform qualification. |

No C006 high or medium finding remains.

## 11. Roadmap disposition

C006 is closed. The new real-tool lifecycle qualification and bounded wake fix satisfy the C006 capability evidence. C007 remains blocked at this closure point only on the independent tool-advisor C001 formal closure/integration and then must execute its latest-main, migration, exact-head hosted, Desktop E2E, review and PR metadata gates.

## 12. Registry updates

- Mark C006 implemented/closed; add this record under recently closed work and remove its dependency-ready row.
- Keep C007 blocked and narrow its blocker to the outstanding tool-advisor C001 closure/integration plus C007-owned integration/hosted qualification.
- The unblock audit finds **no plan newly eligible yet**, because tool-advisor C001 had not been formally closed at this C006 closure point.
