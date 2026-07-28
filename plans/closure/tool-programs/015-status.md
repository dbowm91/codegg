# Tool Programs Milestone 015 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-programs/015-final-production-path-and-independent-closure.md`

Source subsystem roadmaps:

- `plans/subsystems/tool-programs-roadmap.md`
- `plans/subsystems/tool-programs-correctness-closure-addendum.md`

Repository baseline reviewed:

- `247ef5015d79bdd834bffca15c76ebb2426beb40`

Independent review evidence:

- `230f435fa03fb7464607f0b4cf9e4be239621701`
- `plans/closure/tool-programs/015-independent-review.md`

Implementation commits:

- `e22ceb06` through `27bbb834` — accepted-decision authority and one
  canonical runtime contract snapshot.
- `2d5ab5a3` through `8415d81b` — monotonic replay and durable child
  reattachment.
- `af8a3c5b` through `351edb85` — canonical call, child, result, and
  large-output artifacts.
- `3bfa10e1` through `de432a8c` — fail-closed durable notifications.
- `280365de` through `143a8b59` — complete descendant traversal and resource
  reconciliation.
- `ffcac3d3` through `aec7284c` — real daemon failpoints, recovery fixtures,
  and native harness routing.
- `247ef50` — ordinary public-protocol authority rejection and durable
  append-before-mark parent-session injection.

## 1. Executive finding

M015 is closed. Native Tool Programs now use one accepted-decision authority
chain, one frozen Broker contract identity, monotonic durable recovery,
canonical artifacts and results, fail-closed parent notification delivery,
complete descendant reconciliation, and real two-process daemon crash
evidence. The independent reviewer approved the exact implementation head
with no unresolved high or medium findings.

This closes the native-only Tool Programs correctness workstream. Hosted
execution and programmable-palette expansion remain deferred product work and
are not implied by this closure.

## 2. Requirement-to-evidence matrix

| Criteria | Evidence | Result | Notes |
|---|---|---|---|
| C-01–C-05 authority | `tool_program_m015_authority_contract`; public forged-`JobSubmit` process regression; persisted grant verification | pass | Missing, denied, stale, revoked, mismatched, or client-fabricated authority cannot create an ordinary executable Tool Program |
| C-06–C-10 contracts | canonical snapshot helper; injected Broker catalog; authority/contract suite | pass | Submission, admission, and nested calls verify the same full snapshot and reject drift |
| C-11–C-20 replay/child | `tool_program_m015_recovery`; daemon call and child failpoints | pass | Completed calls merge monotonically; pending child and original deadline survive restart |
| C-21–C-28 results/artifacts | `tool_program_m015_artifact_pipeline`; corruption failpoints | pass | Handles and digests originate in canonical stores; corruption and persistence failure fail closed |
| C-29–C-36 notifications | `tool_program_m015_notification_recovery`; idempotent session-event tests | pass | Durable creation, claim, append, mark, and acknowledgement are ordered and retry-safe |
| C-37–C-42 descendants/resources | `tool_program_m015_descendant_convergence`; daemon cancel/restart fixture | pass | Traversal crosses terminal intermediates, bounds cycles, and restores processes and scheduler capacity |
| C-43–C-48 process evidence | `tool_program_m015_daemon_failpoints`; native harness | pass | Debug process-owner fixture submits over public stdio protocol, activates failpoints, kills process A, and recovers in process B without shared memory |
| C-49–C-52 governance | focused/broader tests, format/check, static guards, independent review commit, synchronized planning docs | pass | Exact implementation and review commits are recorded; no high/medium finding remains |

## 3. Production implementation evidence

- The model-facing submission path requires an immutable accepted decision;
  executable fallback authority was removed.
- Generic daemon `JobSubmit` rejects Tool Program payloads in ordinary
  operation. The cross-process recovery capability is explicitly
  process-owner enabled in debug builds and is disabled in release builds.
- Submission freezes the active runtime Broker catalog and persists its
  canonical full-snapshot digest. Executor admission and every nested call
  verify that identity.
- Checkpoints and completed-call journals merge by sequence without erasing
  newer completions. Active child waits and absolute deadlines are durable.
- Calls, children, results, and oversized output use canonical artifact,
  RunStore, and result-store identities with integrity verification.
- SQLite notification creation and transitions propagate errors. Parent
  injection first appends a stable, collision-checked session event, then
  records injection, then acknowledges delivery.
- Descendant discovery traverses terminal intermediate nodes and reconciles
  process groups, permits, leases, counters, and capacity.
- `core-stdio` startup hydrates workspace state and performs generation
  recovery before resumed work is admitted.

## 4. Verification executed

Local implementation verification:

```text
cargo fmt --all -- --check                                      passed
cargo check -p codegg-core                                      passed
cargo check -p codegg --all-features                            passed
cargo test -p codegg --test tool_program_m015_authority_contract  5 passed
cargo test -p codegg --test tool_program_m015_recovery             5 passed
cargo test -p codegg --test tool_program_m015_artifact_pipeline     4 passed
cargo test -p codegg --test tool_program_m015_notification_recovery 9 passed
cargo test -p codegg --test tool_program_m015_descendant_convergence 8 passed
cargo test -p codegg --test tool_program_m015_daemon_failpoints      8 passed
cargo test -p codegg --lib tool_program                            39 passed
cargo test -p codegg-core tool_program                            156 passed
cargo test -p codegg-core event_store_idempotency_tests             2 passed
python3 scripts/e2e/tool_program_harness.py --mode native --scenario all
                                                                   1 passed, 0 skipped
bash scripts/check-core-boundary.sh                                passed
python3 scripts/check_scheduler_bypass.py                          passed
python3 scripts/check_execution_ownership.py                       passed
python3 scripts/check_daemon_cwd_usage.py                           passed
python3 scripts/check_tool_broker_boundary.py                       passed
```

The independent reviewer separately reran the six M015 integration suites:
39 tests passed. CI status was not available in this local closure.

The repo-wide all-target Clippy invocation was attempted and stopped on three
pre-existing `clippy::question_mark` findings in `crates/egglsp/src/edit.rs`.
Those files are outside M015 and the required M015 compilation, test, harness,
and guard evidence passed.

## 5. Invariant review

- Programmable calls remain read-only or scheduler-owned child operations.
- No shell, patch, Git mutation, destructive, approval-sensitive, commit,
  push, or subagent capability was added to the program palette.
- The scheduler remains the sole durable admission and child-work authority.
- The Tool Broker remains the sole nested tool execution boundary.
- Workspace, session, principal, decision, policy, and contract identities
  are immutable and verified through execution.
- Production remains explicitly `native_only`.

## 6. Failure and recovery review

The process harness proves recovery after job persistence, completed-call
persistence, result commit, and active-child checkpoint windows. It also
proves deadline retention, corruption rejection, exact-once call/child
behavior, recursive cancellation, and returned scheduler capacity.

Notification persistence and query errors are typed failures. Independent
service instances contend through SQLite compare-and-set state. A crash after
session append retries the same stable event identity rather than creating a
second parent event.

## 7. Migration and compatibility review

M015 adds no storage-layout version. It corrects the existing v35 lineage
insert/read sites and retains compatibility with existing durable job,
checkpoint, notification, result, and artifact records. Invalid or incomplete
legacy authority/contract material is deliberately non-executable and fails
closed.

The generic public protocol remains available for non-Tool-Program jobs.
Ordinary clients must use the authorized model-facing Tool Program boundary.

## 8. Security review

Authority cannot be synthesized from correlation strings or arbitrary public
payload JSON. Decision outcome, identity, scope, revisions, validity, effect,
and contract snapshot are verified before execution. Contract drift,
malformed/corrupt persistence, path-policy mismatch, missing artifacts, and
notification persistence failure all reject rather than widen authority or
report false success.

The independent reviewer found no remaining high or medium security or
correctness issue.

## 9. Documentation and operations

The implementation plan, subsystem roadmap, correctness addendum, native
harness, closure record, and planning registry now identify the exact
implementation and independent-review commits. Existing Tool Broker,
scheduler, and execution-ownership guards cover the changed boundaries.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Repo-wide Clippy currently reports three unrelated `question_mark` findings in `crates/egglsp/src/edit.rs` | No M015 production-path impact; required M015 evidence is green | Resolve in the owning LSP maintenance work |

No critical, high, or medium finding remains.

## 11. Roadmap disposition

Milestone 015 and the strict native-only Tool Programs subsystem are closed.
M011–M014 remain historical conditional records; their remaining strict
closure ownership is satisfied by M015.

The registry and roadmap dependency audit found no registered future
implementation plan with M015 as a remaining hard or interface blocker.
Therefore no future plan is newly unblocked. Hosted execution and palette
expansion remain intentionally unregistered deferred product work.

## 12. Registry updates

- Mark the Tool Programs subsystem and M015 `closed`.
- Remove M015 from dependency-ready and active-closure work.
- Add M015 to recently closed work with implementation head `247ef50` and
  independent review commit `230f435`.
- Record that no registered downstream plan was unblocked.
