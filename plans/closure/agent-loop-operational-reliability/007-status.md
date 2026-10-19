# Agent Loop Operational Reliability M007 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-loop-operational-reliability/007-sustained-autonomy-trajectory-qualification.md`

Source subsystem roadmap: `plans/subsystems/agent-loop-operational-reliability-roadmap.md`

Repository baseline reviewed: `f37840ef5fca36d44b35daec5da6b26cd049a3ee`

Implementation and qualification commit: `c409e1e012d9e599abe67e975c28f4fac9dd66de`

Verified implementation head: `c409e1e012d9e599abe67e975c28f4fac9dd66de`

Decision: ADR-0040 Option B accepted 2026-10-18.

## 1. Executive finding

M001–M006 implementation and closure records are present. The composed production-wiring evidence covers source projection, glob/discovery, evidence-gated recovery, provider schema fallback, and durable child admission/failure visibility. Repeated unknown outcomes reach the ordinary hard turn limit without a false `stagnated` claim.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Model-visible source trajectory has usable bounded detail | `large_source_read_reaches_the_next_model_request_as_a_contiguous_page` | pass | Verifies 1,000-line input yields contiguous >20-line page and continuation without another read call. |
| Unknown repeated outcomes remain budget-bounded | `repeated_unknown_outcomes_remain_bounded_by_the_hard_turn_limit`; progress-recovery tests | pass | Exact three-request hard cap. |
| Provider compatibility retry and child admission failure visibility | fake-gateway harness; AgentRunStore contention/redaction; task regressions | pass | Existing owners and test targets reused. |
| Ordinary broad checks | full default-feature workspace nextest, quick, clippy | pass | 14,614 tests across 288 binaries; quick and clippy passed. |
| Feature-configured full verification | `scripts/verify.sh full` | partial | Quick and clippy passed; feature-configured nextest stopped on the TUI fixture recorded below. |

## 3. Production implementation evidence

No new scheduler, store, test framework, live-provider lane, or unbounded execution path was introduced. The deterministic suite composes focused production-path fixtures across the existing AgentLoop harness, SQLite AgentRunStore, task tool, and provider adapters. The historical causal-receipt fixture reconstructs its pre-M003 schema instead of mutating frozen receipt data.

## 4. Verification executed

```bash
rtk cargo fmt --all
rtk cargo nextest run -p codegg --test agent_loop_harness --locked --profile ci
rtk cargo nextest run --workspace --locked --profile ci --no-fail-fast
rtk scripts/verify.sh quick
rtk cargo clippy --workspace --all-targets --locked -- -D warnings
rtk scripts/verify.sh full
rtk cargo nextest run -p codegg --locked --profile ci -E 'test(socket_daemon_mode_never_takes_the_remote_input_path)'
```

Results at the implementation head: formatting passed; harness 61/61 passed; workspace default suite 14,614 passed across 288 binaries; quick and clippy passed. The full-script attempt at that head ran before the final test-only harness addition; it passed quick and clippy, then reported 5,128 passed and one failed before fail-fast cancellation.

## 5. Invariant review

Output, tool queries, SQLite failure text, and provider retries remain bounded. Unknown progress is not represented as stagnation. Tool authorization, run identity, child ownership, and request retry ceilings remain controlled by existing owners.

## 6. Failure and recovery review

The suite covers concurrent idempotent admission, bounded redacted failure persistence, missing and paged context behavior, provider schema downgrade, denial, cancellation, and restart/recovery paths through existing focused tests. No new manual model smoke was run; it remains optional and is not claimed as evidence.

## 7. Migration and compatibility review

No database migration or public protocol change. Existing configuration, tool contracts, and provider model selection remain compatible. The provider retry only removes unsupported deferral metadata.

## 8. Security review

No authorization ceiling was widened. Search is permission-filtered, source paging retains session checks, failure text is redacted, and the provider downgrade preserves the authorized tool set.

## 9. Documentation and operations

Updated agent, context, permission, provider, and tool architecture docs plus `docs/tools.md`. Quick guards and workspace clippy pass.

## 10. Follow-up verification and findings

The previously reported `socket_daemon_mode_never_takes_the_remote_input_path` fixture failure is resolved. Its test now installs a recording `CoreClient`, submits through the socket-daemon path, and asserts the accepted `TurnSubmit` request. The focused regression passes.

The same verification follow-up supplied explicit canonical workspace roots to the background-shell and command-routing integration fixtures. Their binaries pass (12/12 and 21/21). A feature-enabled test build also required the server status mapper to classify `MissingWorkspaceRoot` as `400 Bad Request`; the focused feature build then passed both previously timing-out LSP scenarios.

`scripts/verify.sh full` has not completed cleanly in this environment. With default concurrency, two unrelated LSP security-context cases exceeded the 120-second nextest timeout; both passed in isolation and passed in a rerun with two concurrent tests. That rerun then stopped at `live_derived_workspace_reuse_under_required_isolation`: the local Eggwork node did not advertise `isolation.landlock.workspace-rw.v1`, so the environment cannot qualify that required-isolation trajectory. Quick guards and clippy pass. This is recorded as an environment-limited verification result, not as a passing full suite.

No unresolved TUI or agent-loop implementation finding remains.

## 11. Roadmap disposition

M007 is closed. M001–M006 were closed first, which unblocked M007; M007 then completed the dependency audit. No future plan in this line remains eligible or blocked. The TUI test fixture correction is included as a verification follow-up; no successor plan is required.

## 12. Registry updates

All seven plans and the subsystem roadmap are closed. ADR-0040 Option B is accepted. No new plan was unblocked beyond M007.
