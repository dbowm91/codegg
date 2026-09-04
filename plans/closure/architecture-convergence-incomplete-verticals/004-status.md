# Architecture Convergence M004 — AgentLoop Coordinator Reduction Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/architecture-convergence-incomplete-verticals/004-agent-loop-coordinator-reduction.md`

Source subsystem roadmap:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Repository baseline reviewed: `3c4890035513cd4d74430b6f64523c8be676024e`

Implementation commits or pull requests:

- `1e06bc2` — activate M004 in the planning registry.
- `e0a9595` — reduce AgentLoop to a coordinator over grouped typed service handles and add lifecycle phase tracking.
- `395e1a7` — name the grouped tool-definition cache type and resolve the exact-head Clippy finding.

## 1. Executive finding

M004 production work is complete. `AgentLoop` now owns turn identity,
transient sequencing, workspace/run identity, live controls, and bounded
observation state. Provider, context, tool, recovery, snapshot/checkpoint,
goal, artifact, projection, and configuration handles are constructed behind
`AgentLoopServices`. `TurnLifecycle` gives the ordinary and follow-up paths
the same explicit phase vocabulary without creating a second durable state
machine.

The milestone remains conditionally closed because the exact-head hosted
workspace test run reproduced one unrelated existing shell-runtime timeout
failure, while the M004 coordinator/lifecycle tests and the other 4,315 root
tests passed. Root test-binary linking and the workspace all-target check also
remain unavailable on this host because the repository's existing x86_64/arm64
native-library/linker incompatibility causes silent linking after compilation.
No production compile error or changed-path correctness finding remains; the
remaining condition is operational verification of the unrelated shell test
and the local host toolchain.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| M001-M003 canonical owners are consumed | `AgentLoopServices` stores the canonical context, managed-tool, and Git-facing adapters; no new policy engine was introduced | pass | Existing M001-M003 conditional closures remain authoritative |
| AgentLoop policy ownership is materially reduced | `AgentLoop` direct fields reduced from 68 at the baseline to 32; 38 capability/config handles are grouped in `AgentLoopServices` | pass | The reduction is ownership/boundary based, not a cosmetic rename |
| Major phases use narrow typed boundaries | `TurnPhase`, `TurnLifecycle`, `ContextPlan`, `ToolExecutionOutcome`, and `RecoveryDecision` are used across the lifecycle | pass | Durable run state remains outside the in-memory enum |
| Duplicate outcome/recovery handling is not expanded | Existing `ProviderTurnAdapter`, `ToolBatchExecutor`, `EventProcessor`, and recovery types remain the only phase adapters; both ordinary and follow-up paths use them | pass | No parallel string/boolean recovery policy was added |
| Ordinary turn and tool continuation behavior is retained | Existing `tests/agent_loop_harness.rs` covers smoke, tool continuation, soft-stop retry, follow-up tool calls, failures, permissions, ordering, and retry exhaustion; exact-head hosted tests passed the changed coordinator/lifecycle coverage | pass (scoped) | The overall hosted workspace run had one unrelated shell-runtime failure |
| Cancellation and compaction behavior is retained | Existing compaction and scheduler/run-control tests plus the unchanged cancellation checks remain in place; exact-head hosted tests passed the changed-path coverage | pass (scoped) | The overall hosted workspace run had one unrelated shell-runtime failure |
| Architecture documentation identifies one owner per phase | `architecture/agent.md` now contains the phase diagram, ownership table, and final field grouping | pass | — |
| Storage/protocol compatibility is preserved | No schema, protocol DTO, scheduler authority, or run identity changes | pass | No migration required |

## 3. Production implementation evidence

- Added `src/agent/coordinator.rs` with the `AgentLoopServices` construction
  boundary and the typed `TurnPhase`/`TurnLifecycle` sequencing state.
- Moved the loop's provider, permission, registry, context, recovery, model
  routing, snapshot/checkpoint, persistence, artifact/projection, goal,
  notification, and configuration handles into the service bundle.
- Updated the existing context-runtime, provider-turn, and tool-batch adapters
  to consume those grouped handles without changing their public behavior.
- Wired lifecycle transitions through admission, context preparation, provider
  invocation, tool execution, recovery, and completion for both the main loop
  and queued follow-up execution.
- Retained the public compatibility constructor and all existing setters,
  keeping daemon construction routed through the typed factory.
- Updated `architecture/agent.md` to document the final coordinator/service
  boundary and removed stale field/cache claims.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all
rtk cargo fmt --all -- --check
rtk cargo check -p codegg --lib
rtk cargo clippy -p codegg --lib -- -D warnings
rtk cargo test -p codegg coordinator::tests::lifecycle_exposes_typed_phase_transitions --lib
rtk cargo test -p codegg --no-default-features --lib coordinator::tests::lifecycle_exposes_typed_phase_transitions
rtk scripts/verify.sh quick
rtk git diff --check
hosted CI 33888051687: cargo clippy --workspace --all-targets --locked -- -D warnings
hosted CI 33888051687: cargo test --workspace --locked -- --test-threads=1
```

### Results

- Formatting, `cargo check -p codegg --lib`, and `git diff --check` passed.
- Exact-head hosted CI run [33888051687](https://github.com/dbowm91/codegg/actions/runs/33888051687)
  passed all static guards, formatting, and workspace Clippy. Its first
  workspace test attempt ran 4,315 tests successfully and failed only
  `shell::runtime::tests::runtime_timeout_emits_timed_out_event`, outside the
  M004 change surface; the failed-job rerun reproduced that same single test
  failure with 4,315 tests otherwise passing.
- The hosted run explicitly exercised
  `agent::coordinator::tests::lifecycle_exposes_typed_phase_transitions` and
  the existing AgentLoop behavior suites successfully before the unrelated
  failure.
- `scripts/verify.sh quick` passed its generated-agent, core-boundary,
  sandbox-contract, and execution-ownership guards, then stalled during the
  workspace all-target Cargo check and was manually interrupted after no
  compiler/linker process remained visible.
- The package Clippy command and both focused root test invocations produced no
  diagnostic before entering the same silent host link phase; they were
  manually interrupted. The earlier pre-change root test/link behavior is
  documented in M001-M003 closure records as the x86_64/arm64 native-library
  mismatch.
- The focused behavior suites were retained rather than weakened or replaced.
  A corrected host or exact-head CI run must execute them before strict closure.

## 5. Invariant review

- Context accounting and compaction remain delegated to `crate::context`;
  `AgentLoop` only sequences `ContextPlan` application and lifecycle phases.
- Process lifecycle, tool authorization, and broker provenance remain in the
  existing managed process/tool owners; this change does not add a subprocess
  path or shell fallback.
- Git/worktree safety and provenance remain in their M003 owners; no Git policy
  was copied into the coordinator.
- Scheduler admission and durable run completion remain in the existing
  scheduler/run-control/RunStore paths.
- Cancellation, steering, bounded follow-ups, and safe-boundary journaling
  continue to run through the existing live controls and durable bridge.
- `TurnLifecycle` is bounded in-memory sequencing and cannot be used as a
  durable completion authority.

## 6. Failure and recovery review

Provider retry remains in `ProviderTurnAdapter`; typed tool failures continue
through `ToolExecutionOutcome`; progress/stall handling continues through
`RecoveryController`/`AutonomyState`. Follow-up and autonomous goal
continuation still use bounded loops. No detached task was introduced for
provider, tool, or turn lifecycle ownership. Existing checkpoint, notification,
run recovery, and contention behavior was not altered.

## 7. Migration and compatibility review

No storage or protocol migration is required. The public `AgentLoop::new`
constructor and compatibility setters remain available to CLI, exec, test, and
embedded callers. The daemon's `build_agent_loop` factory remains the typed
production construction path. The refactor changes internal field layout only.

## 8. Security review

No authorization boundary was widened. Permission checks, path containment,
managed process policy, Git policy, artifact redaction, and provider/session
context propagation remain in their existing owners. Service grouping does not
make any policy mutable through a new public API. No secret or raw tool output
was added to durable state.

## 9. Documentation and operations

- Updated `architecture/agent.md` with the final lifecycle diagram, ownership
  table, service boundary, and direct-field observations.
- Added `src/agent/coordinator.rs` module documentation and a lifecycle
  transition unit test.
- Ran the existing static guards through `scripts/verify.sh quick`; no new
  guard or verification lane was added.
- Future focused behavior tests should be rerun with the repository's existing
  capped commands on CI or a host with compatible native libraries.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low / operational | Root focused tests and workspace all-target verification cannot complete on this host's silent x86_64/arm64 native-library link path | Strict local runtime evidence is incomplete, while hosted changed-path tests and static guards pass | Rerun the focused `agent_loop_harness`/coordinator tests and `scripts/verify.sh quick` on a corrected host toolchain |
| low / operational | `shell::runtime::tests::runtime_timeout_emits_timed_out_event` failed identically on both attempts of hosted CI run 33888051687 | The failure is outside M004's changed files; the remaining workspace-wide green result is blocked by an existing shell-runtime test | Track or repair that pre-existing shell-runtime test in its owning work; no M004 change is required |
| critical/high/medium | None in the changed coordinator/service boundary | — | None |

## 11. Roadmap disposition

M004 is conditionally closed with explicit operational evidence conditions.
The subsystem roadmap remains active because M005-M008 are future milestones.
No corrective implementation plan or ADR is required: the unresolved item is
host verification, not a production correctness finding.

The dependency audit found that M006's sole hard dependency is now satisfied.
M005 remains independently ready from M003; M007 remains ready from M002's
stable execution/edit contract; and M008 remains independently ready from the
closed session-projection subsystem.

## 12. Registry updates

The closure commit updates the planning control surfaces to:

- mark this implementation plan `implemented`;
- record M004 as conditionally closed in the subsystem roadmap and registry;
- remove M004 from dependency-ready work;
- promote M006 from `blocked` to `ready` because M004 is complete;
- reconcile M007's implementation-plan header with its already-ready registry
  entry; and
- remove M006 from the blocked-work section while retaining only the unrelated
  runtime-safety evidence blocker.

Final disposition: conditionally closed pending the named host-toolchain
focused-test/quick-verification rerun and resolution of the unrelated
shell-runtime test failure by its owning work.
