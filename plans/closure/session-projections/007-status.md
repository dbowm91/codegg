# Session Projections Milestone 007 — Closure Status

Status: conditionally closed — Milestone 008 final transport lifecycle and replay evidence polish required

Source implementation plan:

- `plans/implementation/session-projections/007-corrective-transport-lifecycle-and-evidence-closure.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md`

Repository baseline reviewed: `dbbaabdde51db09f0c5beb704234ce1d94d01c9a`

Implementation commit:

- `9887c2d581a3d01280485523161695d08469c34f` — corrective transport lifecycle implementation, adapter seams, focused regression tests, and lifecycle static guard.

Original closure record commit:

- `922333b5787944d033c004cbc184a9de06778a88`

Corrective handoff:

- `plans/implementation/session-projections/008-final-transport-lifecycle-and-replay-evidence-polish.md`

## 1. Closure decision

The principal M007 production changes remain accepted. M007 fixed the detached Unix raw-forwarder lifecycle, added connection-local lifecycle fault seams, introduced epoch-safe `/tui` raw routing with final-writer stale rejection, added real blocked-response ordering tests, expanded foreign-operation coverage, added reconnect fixtures, and reconciled the prior transport test count and protocol-version expectation.

Strict closure was invalidated by post-closure source and test inspection. M008 must close three remaining polish findings before the session-projections subsystem returns to strict closed status:

1. `/core` and `/tui` abort sibling connection tasks but do not await the aborted handles before connection cleanup completes;
2. production-adapter staged-subscription failure coverage does not yet exercise the complete approved queue/cancellation/serialization/disconnect matrix;
3. reconnect tests verify replay range metadata and a subsequent live subscription ID, but do not directly prove exact envelope identity, monotonic sequence continuity, and absence of duplication.

M008 does not reopen projection storage, replay authority, sequence semantics, reducer behavior, disclosure policy, artifact bounds, protocol DTO meaning, or the version-5 wire contract.

## 2. Accepted M007 outcomes

### 2.1 Unix connection lifecycle

Accepted-client handlers are owned by the daemon socket serving `JoinSet`. Each Unix client owns a cancellation token and retains its raw-forwarder handle. Peer EOF, writer failure, cancellation, and listener shutdown cancel and await the raw forwarder before the connection handler returns.

### 2.2 Epoch-safe TUI raw routing

Raw `/tui` outbound messages carry a monotonically increasing route generation. `SessionInfo` route changes advance the generation, and the final writer boundary drops messages whose generation is stale. Identical normalized routes preserve their generation, and projection-primary mode continues to suppress raw session mutations.

### 2.3 Atomic response-before-live behavior

Unix, `/core`, and `/tui` have deterministic connection-local lifecycle gates. Real transport tests pause subscription establishment after receiver installation, publish while the canonical response is blocked, prove no live event escapes, then prove the canonical response precedes buffered live delivery.

### 2.4 Foreign operations and reconnect foundations

Real transport fixtures cover fail-closed foreign acknowledgement, resume, unsubscribe, artifact operations, and TUI subscription status where supported. Owner connections remain live after rejected foreign operations.

Reconnect fixtures use a fresh connection identity, preserve stream identity, receive replay metadata for the missing committed range, install a new subscription, and receive a subsequent live event.

### 2.5 Verification and evidence corrections

M007 corrected the remote protocol-version assertion to version 5 and recorded the real transport integration executable count as 18. Focused protocol, core, transport, replay, disclosure, TUI, and static-guard commands were recorded as passing. Workspace-wide clippy continued to report only the pre-existing EggLSP `question_mark` findings.

## 3. Post-closure findings

### 3.1 WebSocket sibling tasks are aborted but not joined

`upgrade_core_ws` and `upgrade_tui` retain send, receive, and raw-event task handles. When one task exits, the handlers call `abort()` on the siblings, then proceed to projection cleanup without awaiting the aborted handles.

Impact:

- task-owned writer/channel/state references may remain alive until the runtime schedules and drops the aborted future;
- the closure claim that `/core` and `/tui` deterministically cancel and join all connection-scoped tasks is too broad;
- repeated connection churn lacks a direct task/drop-baseline proof.

Required correction:

- cancel the connection token first;
- abort remaining sibling tasks where necessary;
- await every retained task handle;
- treat expected cancellation joins separately from abnormal panic/error termination;
- prove repeated teardown returns task/drop probes to baseline and does not perturb another connection.

### 3.2 Adapter-level critical failure matrix is incomplete

The lifecycle seam supports boundaries after daemon subscription creation, after receiver installation, before control enqueue, after enqueue/before writer receipt, during writer write, and before activation. Real adapter tests exercise blocked response ordering and selected writer/receiver failures, while helper tests retain queue timeout, serialization, and cancellation coverage.

The approved M007 plan required queue saturation/timeout, writer close, cancellation, serialization-equivalent failure, disconnect during installation, pre-activation failure, and duplicate rollback to be demonstrated against installed staged daemon subscriptions where applicable.

Required correction:

- complete the production-adapter matrix for `/core`, `/tui`, and Unix;
- assert no successful response or live event;
- assert connection ownership and daemon subscription count return to baseline;
- assert the receiver cannot be retaken and forwarder/task probes terminate;
- assert cleanup remains idempotent and unrelated clients remain live.

### 3.3 Reconnect assertions do not prove exact gap-free handoff

The reconnect tests assert two replay events and range metadata `(1, 2)`, then assert that a later live event uses the new subscription ID. The live helper returns only the subscription ID, not the full envelope sequence or event identity.

Impact:

- a duplicate replay event arriving on the live path could satisfy the current final assertion;
- the tests do not directly prove that the first live sequence is `replay_end_seq + 1`;
- exact event identity, ordering, and bounded post-handoff quietness are not asserted.

Required correction:

- inspect complete replay and live envelopes;
- assert exact sequence vectors and unique event identities;
- assert stable stream identity and changed connection/subscription identities;
- assert the first live sequence is the next sequence after replay;
- assert no duplicate replay or live event follows during a bounded quiet period;
- add at least one replay-response pause race and one disconnect-during-replay cleanup fixture.

## 4. Requirement disposition

| Requirement | M007 disposition | M008 requirement |
|---|---|---|
| Owned/joined Unix raw forwarding | accepted | retain regression coverage |
| Epoch-safe TUI raw routing | accepted | retain regression coverage |
| Response-before-live race proof | accepted | retain and integrate with final failure matrix |
| Foreign operations fail closed | accepted | retain owner-state and cursor side-effect checks |
| `/core` and `/tui` connection tasks joined | incomplete | cancel, abort where needed, and await every retained task |
| Complete staged adapter failure matrix | incomplete | add queue/cancellation/serialization/disconnect/pre-activation fixtures |
| Exact replay-to-live continuity | incomplete | assert full envelopes, sequences, identities, and no duplicates |
| Truthful strict closure | blocked | write M008 closure only after exact evidence passes |

## 5. Verification record retained from M007

Recorded focused commands included:

```text
cargo fmt -- --check
CARGO_BUILD_JOBS=1 cargo check --workspace --all-features
CARGO_BUILD_JOBS=1 cargo clippy -p codegg-protocol --all-targets -- -D warnings
cargo test -p codegg-protocol --all-features -- --nocapture
cargo test -p codegg-core --all-features -- --nocapture
cargo test -p codegg --lib core::transport::projection --all-features -- --nocapture
cargo test -p codegg --lib server::ws --all-features -- --nocapture
cargo test -p codegg --lib core::transport::daemon_socket --all-features -- --nocapture
cargo test --test projection_transport_real --features server -- --test-threads=1
cargo test --test projection_replay_daemon_protocol -- --nocapture
cargo test --test projection_replay_subscription -- --nocapture
cargo test --test projection_replay_resume -- --nocapture
cargo test --test projection_replay_restart_recovery -- --nocapture
cargo test --test projection_replay_transport_isolation -- --nocapture
cargo test --test projection_disclosure_invariants -- --nocapture
cargo test --test projection_artifact_handles -- --nocapture
cargo test --test tui -- --nocapture
cargo test --test tui_render -- --nocapture
cargo test --test tui_project_routing -- --nocapture
cargo test --test tui_project_tabs -- --nocapture
cargo test --test single_daemon_lifecycle -- --nocapture
python3 scripts/check_projection_transport_isolation.py
python3 scripts/check_projection_transport_lifecycle.py
python3 scripts/check_websocket_bounds.py
bash scripts/check-core-boundary.sh
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_scheduler_bypass.py
bash scripts/check_projection_disclosure.sh
git diff --check
```

M008 must rerun the relevant matrix after its production and test changes. The M007 result counts remain historical evidence, not the final M008 counts.

## 6. Unresolved findings

| Severity | Finding | Required action |
|---|---|---|
| medium | `/core` and `/tui` connection task cleanup is abort-only for sibling tasks | implement cancel/abort-and-await or structured joined teardown |
| medium | adapter-level staged failure coverage is incomplete relative to the approved plan | complete production-adapter queue/cancellation/serialization/disconnect/pre-activation tests |
| low-to-medium | reconnect tests do not assert full envelope sequence continuity and absence of duplication | strengthen Unix, `/core`, and `/tui` replay-to-live fixtures |
| repository baseline | workspace-wide clippy reports pre-existing EggLSP `question_mark` findings | track separately; M008 must not introduce new warnings |

## 7. Roadmap disposition

M007 is conditionally closed. The frontend-neutral session-projections roadmap is active for one final dependency-ready milestone:

- `plans/implementation/session-projections/008-final-transport-lifecycle-and-replay-evidence-polish.md`

M008 is a final correctness/evidence polish pass. It must not expand into storage, replay authority, protocol, frontend product, or team-collaboration work.

## 8. Final closure rule

The subsystem returns to strict closed status only when M008 records:

- joined `/core` and `/tui` connection-task teardown;
- complete staged adapter failure evidence or narrowly justified impossible cases;
- exact replay and live envelope sequences with no gaps or duplicates;
- passing focused transport/replay/disclosure/TUI/static-guard results;
- exact implementation and closure commits;
- no unresolved high or medium M008 finding.
