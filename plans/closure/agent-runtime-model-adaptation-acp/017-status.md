# Agent Runtime, Model Adaptation, and ACP Milestone 017 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-runtime-model-adaptation-acp/017-corrective-integration-evidence-and-closure.md`

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md#milestone-017--corrective-integration-evidence-and-closure`

Repository baseline reviewed: `8f86dd2a13fd0418ca850d1a84548e5f78b76a6a`

Implementation commits reviewed:

- `da0f5b8668f523ab099b66fad632b99d36a59528` — ACP turn lifecycle correlation
- `d91ccea1471ff27e22bcb8825218a60944647b59` — specialized runtime finalization and research coordination
- `5a914ca7030ffad3ca3747d9db77cd6400e11818` — prompt/context convergence closure
- `f7f8ce091d90bd858d7ee09a6add8a145be3b5b1` — adapter-driven reasoning safety
- `8a29926e612bd0c956fdf52c1990a405129a8dc4` — descendant admission/cancellation
- `5a5c0fe23a9c681ff3b6c763ed83524a021aa421` — descendant workspace propagation

## 1. Executive finding

Milestone 017 is strictly closed. An independent review traced the M012–M016
production seams from native submission through provider/runtime finalization,
prompt compilation, adapter transforms, descendant admission, tool dispatch,
projection, and ACP completion. Focused mechanism-faithful evidence passed and
no critical, high, or medium finding remains in the corrective addendum scope.

The prescribed broad all-features command is not green and is recorded below
without concealment. It reproduces two unrelated stale all-features assertions
and an independent daemon-socket stack overflow. These are owned by the
existing agent policy/builtin tests and daemon transport test respectively;
the focused M012–M016 production evidence remains independently sufficient for
this closure. Durable AgentRun/worktree/team capabilities remain deferred.

## 2. Requirement-to-evidence matrix

| Corrective requirement | Production evidence | Focused evidence | Result / limitation |
|---|---|---|---|
| ACP pre-ID cancel/close, stale isolation, terminal uniqueness, role-correct replay, stdout purity | `src/acp.rs` lifecycle/correlation state and stdio adapter | `cargo test --test acp_stdio -- --nocapture` (1 passed); ACP unit tests in M012 (6 passed); `projection_transport_real` (58 passed) | Pass; retired `session_projection_transport` target reconciled to current fixture |
| Security local finalization rejects unsupported output | `src/agent/specialized_runtime.rs`, `src/security/`, receipt path | `cargo test --test security_review_runner -- --test-threads=4` (16); `cargo test --test security_review_receipt -- --test-threads=4` (36); specialized runtime (3) | Pass |
| Host-owned bounded research children and typed evidence validation | `src/research/runtime.rs`, `src/research/coordinator.rs`, specialized finalizer | research runtime unit tests (4); specialized runtime (3); M013 production tests | Pass; no live external provider required |
| Complete prompt/context blocks before compilation and cache identity separation | `src/agent/prompt.rs`, `src/context/plan.rs`, `src/agent/loop.rs` | prompt unit tests (22); context plan unit tests (2); `context_plan_convergence` (4); `agent_loop_harness` (40) | Pass |
| UTF-8-safe reasoning, adapter-selected transforms, alias/exclusion behavior, privacy | `src/agent/processor.rs`, `crates/codegg-core/src/model_profile/`, provider serializers | `event_processor` (17); `provider_transcripts` (21); adapter unit tests (5); provider unit tests (4) | Pass |
| Atomic descendant admission, exact release, lineage cancellation | `src/agent/worker.rs`, scheduler/subagent ownership | admission unit tests (2); `subagent` (22); `agent_loop_harness` (40) | Pass |
| Explicit two-workspace native tool ownership and no process-cwd authority | worker/tool execution envelopes and workspace context | cwd/PWD/scheduler/ownership/broker guards; M016 focused tests | Pass |
| Broad verification truthfulness | repository-prescribed verification command | bounded all-features command reproduced failures below | Recorded failed evidence; not represented as green |

## 3. Production-path review and scenarios

| Scenario | Traced path | Disposition |
|---|---|---|
| A — ACP cancellation race | ACP request → native submission/turn correlation → pending cancel/close → exact terminal event | Pass; stale and neighboring session events are rejected |
| B — security local validation | security preparation → ordinary loop → typed local finalizer → receipt/projection/ACP | Pass; malformed/unsupported findings cannot become confirmed findings |
| C — research coordination | classification → bounded host-owned child roles → typed aggregation → synthesis/citation validation | Pass; dedupe, conflict, malformed branch, and minimum-evidence behavior are locally validated |
| D — prompt/cache convergence | root/child typed blocks → compiler fingerprint → context/cache identity → provider request | Pass; plan guidance is single and behavior-affecting inputs are fingerprinted before request creation |
| E — Laguna reasoning safety | provider event processor → UTF-8 bounded private state → resolved adapter serializer | Pass; reasoning remains private and adapter selection is not substring-driven |
| F — descendant contention/isolation | atomic admission → lineage cancellation/release → explicit workspace tool envelope | Pass; concurrent capacity cannot oversubscribe and distinct workspace roots remain explicit |

## 4. Verification executed

### Focused commands

Passed:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --test acp_stdio -- --nocapture                         # 1
cargo test --features server --test projection_transport_real -- --nocapture # 58
cargo test --test security_review_runner -- --test-threads=4      # 16
cargo test --test security_review_receipt -- --test-threads=4     # 36
cargo test --test agent_loop_harness -- --test-threads=4          # 40
cargo test --test subagent -- --test-threads=4                    # 22
cargo test --test context_plan_convergence -- --test-threads=4    # 4
cargo test --test provider_transcripts -- --test-threads=4        # 21
cargo test --test event_processor -- --test-threads=4             # 17
cargo test -p codegg --lib agent::prompt -- --test-threads=4      # 22
cargo test -p codegg --lib context::plan -- --test-threads=4      # 2
cargo test -p codegg --lib security::runtime -- --test-threads=4  # 2
cargo test -p codegg --lib research::runtime -- --test-threads=4  # 4
cargo test -p codegg --lib specialized_runtime -- --test-threads=4 # 3
cargo test -p codegg --lib agent::worker::admission_tests -- --test-threads=4 # 2
cargo test -p codegg-core model_profile::adapter -- --test-threads=4 # 5
cargo test -p codegg-providers openai_compatible -- --test-threads=4 # 4
scripts/verify.sh quick
```

Static guards all passed:

```text
check_daemon_cwd_usage.py
check_project_agent_pwd_inference.py
check_scheduler_bypass.py
check_execution_ownership.py
check_tool_broker_boundary.py
check_builtin_agents.py
generate_builtin_agents.py --check
check_projection_disclosure.sh
check_projection_publication_seam.sh
check_projection_transport_isolation.py
check_projection_transport_lifecycle.py
check_websocket_bounds.py
```

The plan's `cargo test --test session_projection_transport` command was
attempted and failed because that target no longer exists. It was not counted
as evidence; the current server-enabled `projection_transport_real` fixture
was run instead and passed all 58 tests.

### Broad command

```text
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=14
```

Result: failed after 14m47s. The full-feature `codegg` lib test binary
reproduced:

- `agent::policy::tests::test_frontier_reasoning_policy`: expected 10, got 1;
- `agent::tests::test_builtin_research_agent_registered`: expected `ask`, got
  `deny`;
- `core::transport::daemon_socket::daemon_socket_integration_tests::socket_f0_successful_production_write_is_observed` aborted with stack overflow.

The first two are existing all-features assertion/configuration drift, and the
third is an independent daemon-socket test failure. None is a M012–M016
production-path failure, and none is a corrective addendum finding. The
default-feature quick verification and all required focused production suites
remain green.

## 5. Invariant, failure, compatibility, and security review

- Execution authority remains daemon/scheduler-owned; no second ACP or
  specialized-runtime authority was found.
- ACP cancellation, close, stale generation, replay, and terminal paths are
  bounded and idempotent.
- Security and research provider output is advisory; host parsing and evidence
  validation remain authoritative.
- Prompt/compiler/cache identity is finalized before provider execution;
  protocol chronology and tool pairing remain lossless.
- Private reasoning is valid UTF-8, bounded, and excluded from public
  serialization, projections, ACP, logs, diagnostics, and error bodies.
- Descendant admission reservations are atomic and released on rejection,
  completion, cancellation, timeout, and shutdown; root cancellation is
  lineage-scoped.
- Native tool dispatch consumes explicit workspace/execution context; static
  cwd and PWD guards pass.
- No storage migration or incompatible protocol change was introduced by the
  corrective sequence. Existing legacy paths are bounded compatibility paths.
- Path, permission, tool-broker, projection disclosure, and secret-scan guards
  pass. Durable AgentRun/worktree/team authority remains explicitly deferred.

## 6. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| none in addendum scope | No critical, high, or medium corrective finding remains | None | No corrective follow-up plan required |
| low / unrelated | All-features agent policy and built-in research assertions are stale under the current feature configuration | Broad workspace command is not green | Owning agent-test maintenance; outside M017 |
| low / unrelated | Daemon socket F0 production-write test stack-overflows under the all-features lib test binary | Broad workspace command aborts | Owning daemon transport/DVR investigation; outside M017 |

## 7. Roadmap and registry disposition

M017 is strictly closed. The corrective addendum and agent-runtime subsystem
roadmap are marked closed. The original M011 conditional record remains
historical and is superseded for current disposition by this M012–M017
sequence.

No future registered plan is unblocked by M017. Provider M007 and Tool
Programs M019 remain the independent prerequisites for Development Verification
and Release M006; those rows are intentionally unchanged.

Registry changes made with this record:

- mark the agent-runtime roadmap closed at corrective M017;
- remove M017 from active closure and dependency-ready sections;
- add M017 to recently closed work;
- retain the unrelated Provider M007, Tool Programs M019, and DVR M006 statuses;
- retain the two broad-verification findings as attributed, unregistered
  follow-up ownership rather than reopening this subsystem.

## 8. Final recommendation

Accept strict closure for M017 and the corrective agent-runtime/model-
adaptation/ACP addendum. Do not claim the broad all-features workspace command
is green; route its reproduced failures to their owning workstreams if they
become release-blocking.
