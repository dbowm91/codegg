# Async Background Jobs Corrective C006 — Model-Facing Tool to Idle Resume Lifecycle Qualification

Status: **implemented** — closed with strict local model-facing lifecycle evidence; exact latest-main hosted qualification is owned by C007.
Planning baseline: `247edec5f37851ca3f67db5943675c896d19e9d9` (`work/async-background-jobs-production-wiring-c004-plans`, 2026-10-15 review).
Active corrective addendum: `plans/subsystems/async-background-jobs-integration-qualification-corrective-addendum.md`.
Historical closures: M001–M004 `plans/closure/async-background-jobs/`; C001–C005 `plans/closure/async-background-jobs-corrective/001-status.md` through `005-status.md`. C005 is **historically closed** and must not be rewritten.
Related design: accepted `plans/adrs/ADR-0031-authenticated-idle-continuation-delegated-turn-admission.md` plus ADR-0001/0007.

## 1. Executive objective and why this successor is necessary

C005 implemented a real, host-authenticated `CoreRequest::ContinuationGrantCreate` family and a daemon-internal, canonical `TurnSubmit` admission. Its production positive test `tests/async_background_c005_idle_resume.rs::prod_resume_admits_exactly_one_owner_turn_through_fanout` calls `record_completed(&daemon, ...)` to create a synthetic `JobTerminalNotice`, then verifies fanout -> actual daemon admission with a stub `TurnRuntime`. The other portions of the delivery and CI watcher were tested separately (M001–M004, C001–C004). **This is strong boundary qualification but does not yet prove that a model-facing `ci_watch`/`bash`/`test` invocation reaches C005's real idle resume through the entire canonical unit**.

The new regression must exercise tool registration and accepted authority -> durable job/watch creation -> originating turn exits -> scheduler/C001 daemon-owned observation -> committed M001 notice -> persisted owner-scoped `BackgroundJobCompleted` and TUI/remote projection -> optional C005 authenticated delegated turn, without the test forging an intermediate completion or manually prompting an agent. This closes a test-gap, **not** a claim that C005 production code is defective.

### Scope and non-goals

- Real production `CoreDaemon` with SQLite, singleton `JobScheduler`, notifier, CI watcher, fanout, continuation store and `attach_idle_wake`. Exercise `DefaultTurnRuntime` + normal ToolBroker where feasible; a deterministic scripted provider at the model transport boundary is allowed and must be identified as such. No live/personal inference, no paid GitHub API.
- Use a loopback GitHub Actions fixture for `ci_watch`; run actual scheduler-backed `bash` and/or `test` background tool to test machine execution under normal user permissions/sandbox. Do not create an alternate scheduler, a direct tool shortcut bypassing ToolBroker authorization, a test-only notification path or a synthetic `CoreRequest::TurnSubmit` after the originating turn ends.
- Preserve `notify_only` as the default. Agent `resume_owner` request alone is inert; user-authorized grant must be created through authenticated `CoreRequest::ContinuationGrantCreate` (after the job/watch identity is available and before terminal). No test `ContinuationService::authorize(NewGrant)` as a proxy for consent.
- Linux headless/loopback + connected frontend transport is the primary deterministic lane; macOS/Windows native and real GitHub API are optional external evidence and must be labeled not run if absent.

## 2. Existing source contract and coverage gap

| Stage | Implemented seam | Current proof / gap |
|---|---|---|
| Agent tool surface | `src/tool/{ci_watch,bash,test,job}.rs`; `src/agent/turn_runtime.rs`; ToolBroker authorization | M002/M004 focused tests independently; no single trajectory into C005 |
| Durable execution | `src/scheduler/{scheduler,ci_watch,ci_watch_idle}.rs`; `JobSubmissionService`; singleton `CoreDaemon` composition | C001 driver and job suites; C005 test directly commits notice |
| Durable completion and live event | `src/scheduler/job_notifications.rs`, `src/core/{daemon_construct,daemon_bootstrap,event_log}.rs`, `CoreEvent::BackgroundJobCompleted` | C004 event, replay, recovery tests; not joined to tool-initiated C005 |
| Authenticated approval + idle admission | `src/core/daemon_continuation.rs`; `CoreRequest::ContinuationGrantCreate` and accepted ADR-0031 | C005 direct terminal fixture -> one real `TurnSubmit`; need production tool trigger |
| Frontend | `src/tui/app/mod.rs`, `crates/codegg-protocol/src/projection/adapters.rs`, event log/replay/projection | C004 TUI toast/projection tests; need end-to-end owning client visibility and nonowner exclusion |

## 3. Ordered work packages

### WP-A — Freeze the single-unit harness contract

Trace `DefaultTurnRuntime` startup, `ToolBroker` invocation/permission receipts, `CiWatchTool::execute_structured`, `BashTool`/`TestTool`, `TurnRunInput` model/agent registry and scheduler submission. Select the **shortest existing harness seam** that genuinely runs a model-facing tool with a validated `ToolExecutionContext` derived from a normal authenticated turn. A scripted/fake provider must produce actual tool-call DTOs consumed by the existing agent loop (no direct tool method call from test unless separately and honestly labeled "broker-tool integration", not "full agent turn"). Do not relax safety checks for tests. Reuse existing `tests/common`, C001/C004/C005 fixtures.

Write red assertions against the C005 baseline or document why earlier tests fail to cover this seam. If a full scripted-provider harness is impossible without modifying substantial production architecture, make the first regression an actual ToolBroker/Tool Program boundary integration, state its narrower coverage explicitly, and separately implement the scripted provider lane before claiming **full** closure. A correct partial test is better than a fake proof.

### WP-B — Exact CI-watch → autonomous continuation trajectory

With a real daemon and normal originating turn, create a `ci_watch` using exact run ID or immutable SHA through the **normal accepted tool path**; loopback GitHub REST fixture transitions queued -> in_progress -> completed. Capture the **actual** returned watch ID and `ciwatch:<watch_id>` job locator, not a predicted string. Authenticate a host `ContinuationGrantCreate` for that exact locator and owning session with `resume_owner` requested, bound selection/model/connection and bounded max tokens/tools/time/TTL. Let the originating turn finish; disable all test status/observe/manual completion calls thereafter.

Wait on a subscribed **session-scoped** `EventLog`/transport client, assert one durable `BackgroundJobCompleted` notice ID and one authorised real `TurnStarted` plus `TurnAdmissionReceipt`, exactly one stub provider-boundary entry, originating principal/workspace unchanged, no raw remote CI strings promoted to instructions. Replay/reconnect and a second connected *owner* client must see identical durable state, not duplicate turns; an unauthorized client must see none. The CI driver must be the sole remote poll source and hold no process admission permit while waiting.

### WP-C — Background machine job + default notify-only

Start a short background build/test via actual `bash`/`test` tool with scheduler-mediated submission and normal sandbox/command authorization (use a harmless deterministic command or test fixture). End the originating turn while work is pending. Without a status poll or injected completion, scheduler terminal causes one notice and one live event. With `notify_only` (or missing host grant), zero post-terminal `TurnStarted` and zero provider calls; later `job` status/output returns correct real terminal and bounded output. Repeat with a grant only if the job's execution path supports `resume_owner` and the server can issue trusted consent **without** bypassing the tool configuration; otherwise document exact constraint, keep default notify lane as qualification and leave a narrowly scoped follow-up.

### WP-D — Faults, safety and lifecycle

Exercise: incorrect run SHA or ambiguous check selector, GitHub HTTP 429/503 with bounded Retry-After/backoff, watch expiry/cancel (must **never cancel GitHub workflow**), duplicate terminal observation, restart after job/store commit before wake/event, revoked/expired/wrong-principal/wrong-workspace grant, archived/active session, missing provider/selection, 50 contender wake race, reconnect/two owner clients/foreign principal. Assert exactly-once *logical* turn admission (not exactly-once network side-effect), unambiguous `needs_attention` after uncertain provider start and **no blind replay**; all failures retain durable actionable owner notice. No hidden provider tokens or shell privilege escalation.

### WP-E — Documentation, focused suites and closure

New test candidates: `tests/async_background_c006_tool_to_idle_ci.rs` and `tests/async_background_c006_tool_to_job.rs`, or one bounded file with clearly named cases. Reuse C004/C005 helpers rather than copying independent mock schedulers; keep test time bounded and capable of running on ordinary SBC/CI hardware.

Update `architecture/{agent,jobs,scheduler,projection,authorization}.md` only where proof exposed changed semantics; update `docs/tools.md` for discoverability if needed. Close in `plans/closure/async-background-jobs-corrective/006-status.md` with exact code commits, matrix and real-vs-fake boundary distinction; update registry and integration addendum; do not overwrite C005 closure. Any true contract defect gets a bounded corrective within C006 and regression. Unexpected fundamental authority decision requires a separate ADR; do not invent one merely for testing.

## 4. Strict acceptance gates

1. **No synthetic completion:** Positive CI trajectory from a normal **model-facing tool invocation** (scripted provider acceptable) -> persisted watch -> background observation -> terminal -> production fanout -> real daemon delegated `TurnSubmit`; positive machine-job/default trajectory likewise begins at an authorized tool and ends by scheduler. Test never calls `record_completed`, `record_terminal`, `observe_once`, `advance_due_watches`, `pending_for_session` to *cause* progression, `maybe_dispatch_idle_continuation`, or `CoreRequest::TurnSubmit` after the origin ends. Inspecting records non-mutatively after the fact is allowed.
2. **Trusted consent is real:** Host approval via the authenticated `CoreRequest` gate using returned job identity, one bound grant, one resumed turn. Model-only `resume_owner` with no grant and default notify-only perform zero autonomous model calls.
3. **Ownership/visibility:** One persisted owner-scoped `BackgroundJobCompleted` with stable ID, live owning TUI/transport subscriber evidence, one allowed `TurnStarted`, nonowner isolation, reconnect/restart replay. No client needs another agent turn or `ci_watch.status`.
4. **Negative proof:** cancellation/revocation/expiry, active turn, drifted model/connection, failed selection, missing sandbox, GitHub failure/timeout, and transport disconnect all fail closed with bounded durable notice; no duplicate provider entry on re-wake.
5. **Compatibility:** M001–M004 + C001–C005 suites, TUI presentation/projection, authorization/admission/audit/sandbox/scheduler/broker guards remain green. Commands: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `scripts/verify.sh quick`, focused `cargo test --test <new_c006_test>`, `cargo test --test async_background_c005_idle_resume --test async_background_c004_live_attention --test ci_watch_c001_idle_driver --test ci_watch_m004_completion`, plus command/scheduler bypass and `scripts/check_idle_continuation_callsites.py`.
6. **Evidence naming:** Separate local focused verification from exact-commit hosted `CI / verify` / Desktop E2E, which C007 owns on the integration candidate. C006 closure cannot call the full feature release-ready without C007's CI and mainline gates. No live provider/network/native OS assertion unless those lanes ran.

**Stop:** If a full real-tool trajectory cannot be shown, do **not** rewrite "broker-only" proof into "model-facing full E2E"; report partial, exact missing seam, negative safety observations, and necessary follow-up. Never weaken approval or sandbox authority to pass the test.
