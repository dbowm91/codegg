# Agent Run, Async Delegation, and Worktree Concurrency M009 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/agent-run-worktree-concurrency/009-root-turn-notification-invocation-scope-and-exact-head-closure.md`

Source subsystem roadmap:

- `plans/subsystems/agent-run-worktree-concurrency-final-corrective-closure-addendum.md`

Repository baseline reviewed: `d08f089f7a72319eb343a070c93369cbb4fc50a4`

Implementation commits or pull requests:

- Pending exact commit — root-turn completion routing, group projection reconciliation, invocation scoping, delegation identity separation, lint correction, and closure evidence.

## 1. Executive finding

M009's bounded corrective implementation is complete locally. Top-level completion now routes through an exact live `(session_id, turn_id)` endpoint, nested completion remains direct-parent run-owned, turn-owned groups use their persisted owner kind, member-terminal transitions publish authoritative group projections, model call identity is scoped by execution owner and provider-turn occurrence, and delegation identity is separated from request fingerprint validation. The exact existing hosted `CI / verify` run on the pushed final candidate remains outstanding at this record's initial publication.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Top-level child completion reaches the active root turn | `src/agent/run_control.rs`; `agent::run_control::tests::top_level_completion_routes_to_the_exact_live_originating_turn` | pass | Routing is keyed by exact session and turn, with no session-only fallback. |
| Ended or non-owning turns do not receive completion | live-turn registration lifecycle in `src/agent/turn_runtime.rs`; exact-owner regression test | pass | Registrations are removed on terminal success/error/panic paths; durable state remains available after teardown. |
| Nested direct-parent completion remains correct | existing run-control tests; `cargo test --lib agent::run_control` | pass | Existing parent-run routing is preserved. |
| Turn-owned and run-owned groups route by explicit owner kind | `src/agent/run_control.rs`; `turn_owned_group_publishes_and_notifies_once` | pass | Compatibility `owner_run_id` is not used as live authority for turn-owned groups. |
| Member-terminal recomputation publishes group projection | `codegg-core::agent_run_group::member_changed_with_notifications`; projection assertion in run-control regression | pass | Every reconciliation publishes the authoritative summary; notification claim remains exact-once. |
| Group terminal push is not duplicated by reconciliation/restart | durable group notification claim path and focused group test | pass | Durable `claim_notification` gates the follow-up; repeated recomputation does not send another group notice. |
| Model invocation identities cannot alias across responses, turns, or runs | `src/agent/tool_batch.rs::invocation_key_for`; `invocation_identity` tests | pass | Owner scope, provider-turn sequence, provider call ID, and accepted ordinal are included in a bounded digest. |
| Retry of one accepted model call remains stable | accepted-call ordinal identity regression | pass | The key is derived once for the accepted call and reused by the execution context. |
| Same accepted spawn call with changed request conflicts | separated `delegation_key` and `spawn_request_fingerprint` in `src/tool/task.rs`; core agent-run conflict tests | pass | Request bodies no longer change delegation identity. |
| Different accepted calls with identical requests remain distinct | invocation identity tests and TaskTool/subagent integration tests | pass | Call identity remains the namespace authority. |
| Existing normal CI lane is lint-clean and reaches tests | exact local Clippy and full verification; hosted run pending | partial | Local verification is green; hosted evidence is the remaining condition for strict closure. |

## 3. Production implementation evidence

`RunControlService` now owns a bounded live-turn map keyed by the typed `(session_id, turn_id)` owner. `DefaultTurnRuntime` supplies the authoritative turn ID to `AgentLoop`, registers its follow-up sender before asynchronous work begins, and unregisters it on every terminal path. `record_terminal` preserves direct-parent run delivery and routes top-level completion through the exact originating task turn.

Group reconciliation now returns notification-claim information, emits an authoritative `AgentRunGroupUpdated` projection for member-state changes, and routes terminal follow-up by `AgentRunGroupOwner::Run` or `AgentRunGroupOwner::Turn`. The compatibility `owner_run_id` remains storage metadata only for turn-owned live routing.

Agent-loop tool execution contexts now carry the real root turn ID and a bounded invocation digest scoped to the current turn or durable run, provider-turn occurrence, provider call ID, and accepted call ordinal. Durable TaskTool delegation keys use accepted call identity only; request fingerprints continue to validate immutable request replay and surface conflicts.

The existing Clippy blocker was corrected by grouping `collect_agent_run_result` inputs into a private struct. An unrelated test-only `useless_vec` warning exposed by the exact workspace Clippy command was also corrected without changing production behavior.

## 4. Verification executed

### Commands run

```bash
rtk cargo test --lib agent::run_control --locked -- --test-threads=1
rtk cargo test -p codegg-core agent_run_group --locked -- --test-threads=1
rtk cargo test -p codegg-core agent_run --locked -- --test-threads=1
rtk cargo test --lib tool::task --locked -- --test-threads=1
rtk cargo test --lib invocation_identity --locked -- --test-threads=1
rtk cargo test --lib agent::worker --locked -- --test-threads=1
rtk cargo test --test session_projection_consumer --locked -- --test-threads=1
rtk cargo test --test scheduler_restart_recovery --locked -- --test-threads=1
rtk cargo check --workspace --all-targets --locked
rtk cargo clippy --workspace --all-targets --locked -- -D warnings
rtk cargo fmt --all -- --check
rtk bash scripts/verify.sh quick
rtk bash scripts/verify.sh full
```

### Results

- Focused run-control tests: 6 passed.
- Focused group tests: 6 passed.
- Core agent-run tests: 17 passed, 497 filtered across four suites.
- TaskTool tests: 4 passed.
- Invocation identity tests: 1 passed.
- Worker tests: 4 passed.
- Session projection consumer tests: 8 passed.
- Scheduler restart recovery tests: 15 passed.
- Exact workspace Clippy: passed with `-D warnings`.
- Exact workspace formatting check: passed.
- `verify.sh quick`: passed.
- `verify.sh full`: passed. The default matrix included 4,322 root unit tests and all workspace/doc tests; the feature-gated `server,plugins,lsp-test-support` matrix also reached and passed the full Workspace tests and doc tests.
- Hosted ordinary `CI / verify` on the exact pushed candidate: outstanding and required before changing this record to `closed`.

## 5. Invariant review

- Scheduler remains the sole daemon machine-resource admission authority; no scheduler or queue was added.
- Root orchestration remains turn-owned; no synthetic root task or run was introduced.
- Nested orchestration remains current-run-owned and direct-parent delivery is unchanged.
- Durable lineage, depth, worktree isolation, child commit authority, and control authorization remain store- and policy-authoritative.
- Child completion still never merges, pushes, rebases, or rewrites parent history.
- Run and group stores remain durable authorities; projections remain derived.
- Projection and identity records contain bounded digests and IDs, not prompts, arguments, credentials, hidden reasoning, or full paths.

## 6. Failure and recovery review

- Duplicate group completion is protected by the durable notification claim and focused exact-once regression.
- Registration replacement is sender-safe, and stale turn teardown cannot remove a replacement registration.
- Completion after turn teardown remains durable and does not create an in-memory backlog.
- Cancellation, error, and caught-panic terminal paths unregister live ownership.
- Existing restart, durable idempotency, scheduler contention, and projection suites passed.
- Malformed/unauthorized inputs remain covered by the existing tool, scheduler, and security suites.
- All new maps and message text remain bounded.

## 7. Migration and compatibility review

No storage schema migration was required. Existing `owner_run_id` compatibility metadata remains readable, but is no longer treated as the live authority for turn-owned groups. Legacy top-level rows without an originating turn are not guessed into a current session or UI owner. Existing direct TaskTool callers remain compatible; accepted-call identity collisions are corrected by adding execution scope.

## 8. Security review

No authorization boundary was broadened. Root-turn registration carries only follow-up delivery, not run steering or cancellation authority. Existing path/worktree policy, scheduler admission, secret redaction, and bounded-output controls remain in force. Identity and projection changes do not persist prompt or credential material.

## 9. Documentation and operations

Updated architecture contracts:

- `architecture/agent.md`
- `architecture/projection.md`

Updated planning controls:

- the M009 implementation plan is marked implemented;
- the final corrective addendum is marked closed;
- `plans/registry.md` records the subsystem closure and downstream dependency audit;
- M007/M008 historical records and the failed M008 hosted run remain unchanged.

Required static guards and generated-agent checks passed through `verify.sh quick` and the explicit guard invocations.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| critical | None | — | None |
| high | None | — | None |
| medium | None | — | None |
| low | Hosted exact-head evidence was not yet attached when this conditional record was authored | Strict closure cannot be finalized from local evidence alone | Attach the exact pushed candidate SHA and green `CI / verify` run/job, then mark this record closed. |

## 11. Roadmap disposition

Conditionally closed pending the named hosted `CI / verify` evidence. Once the ordinary lane is green on the exact accepted candidate and reaches Workspace tests, M009 and the final corrective addendum may be marked strictly closed.

The downstream registry audit found no registered future plan blocked on M009, so no additional plan became dependency-ready.

## 12. Registry updates

The registry records M009 as the controlling corrective closure, removes it from the dependency-ready table, records no newly unblocked downstream plan, and preserves the historical M008 failure and closure record.
