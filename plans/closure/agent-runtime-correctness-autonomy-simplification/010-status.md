# Agent Runtime Correctness, Autonomy, and Simplification M010 — Closure Status

Status: conditionally closed
Source implementation plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/010-recovery-state-strict-closure-corrective-pass.md`
Source subsystem roadmap: `plans/subsystems/agent-runtime-correctness-autonomy-simplification-corrective-closure-addendum.md`
Repository baseline reviewed: `cbdc0150`
Implementation commits: `ea4136ff` — recovery strict-closure correction; `cbdc0150` — stable-Clippy constructor annotation

## 1. Executive finding

The M010 corrective implementation is complete in the pushed tree. Dead bootstrap,
narration, and duplicate recovery branches were deleted; primary and follow-up
loops now consume one bounded continuation allowance; and recovery accepts a
typed `ToolExecutionOutcome`, with rendered text isolated to an explicitly
legacy fallback constructor.

Strict closure is conditional only because GitHub did not publish a `CI / verify`
run for the exact final candidate. The existing workflow has no dispatch trigger,
and PR #74 reports no checks for the branch after the push. This is an external
evidence limitation, not an identified production defect.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Delete synthetic bootstrap implementation | `src/agent/loop.rs` contains no `bootstrap_allowed`, `call_bootstrap_`, or bootstrap execution block | pass |
| Remove dead retry/narration branches | No `if false` recovery branches remain in `src/agent`; disabled branch tests were removed | pass |
| One continuation authority | Both loop paths call `AutonomyState::continuation_allowed()` exactly once in their post-tool soft-stop path | pass |
| Remove repository-specific continuation bypass | `is_repo_task_prompt` and the unbudgeted continuation message were deleted | pass |
| Preserve textual repair bounds | Existing adapter repair path remains the only textual repair path and uses `adapter_repair_allowed()` | pass |
| Typed recovery input | `observe_tool_result(&ToolExecutionOutcome, ...)` is the recovery boundary; success preserves typed success despite misleading display text | pass |
| Preserve M009 authority/workspace corrections | `principal_ref` remains bound to the grant issuer; M009 files remain in the tree | pass |
| Focused verification | Recovery, loop, and harness tests passed locally | pass |
| Quick verification | `scripts/verify.sh quick` passed through its workspace check after the final lint correction; direct locked workspace check passed | pass |
| Hosted final verification | No run exists for `cbdc0150`; dispatch is unsupported and PR checks are empty | condition |

## 3. Production implementation evidence

- Removed 326 lines of unreachable synthetic bootstrap and duplicate recovery code
  from `src/agent/loop.rs`.
- Removed `bootstrap_used` state from `AutonomyState`.
- Unified primary/follow-up post-tool continuation checks around the same bounded
  state transition.
- Added `ToolExecutionOutcome::success` and `ToolExecutionOutcome::legacy`; only
  the latter classifies rendered text.
- Updated `architecture/agent.md` to describe the actual transition contract.
- Added the minimal `clippy::too_many_arguments` allowance required by current
  stable Clippy for the pre-existing constructor; behavior is unchanged.

## 4. Verification executed

- `cargo test -p codegg --lib agent::progress_recovery -- --nocapture` — passed.
- `cargo test -p codegg --lib agent::r#loop::tests -- --nocapture` — passed.
- `cargo test --test agent_loop_harness -- --test-threads=1` — passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.
- `cargo check --workspace --all-targets --locked` — passed.
- `scripts/verify.sh quick` — passed after the final lint correction.
- `git diff --check` — passed.
- Hosted predecessor run `31517770763` passed for `7ae157e9`, not this final
  candidate. The push-triggered runs for planning-only predecessors failed on
  the pre-existing constructor Clippy lint; `cbdc0150` fixes that lint locally.

## 5. Invariant review

No authority, workspace, permission, textual-repair, lifecycle, storage, or
protocol invariant was intentionally changed. Denied execution remains distinct
from success and cannot restore the base palette. Recovery state remains
turn-local and non-durable.

## 6. Failure and recovery review

Provider transport retry remains in `stream_with_retry`. Autonomous recovery has
one post-tool continuation allowance per loop state, and a second generic
repository heuristic no longer exists. Cancellation and steering checks remain
outside the deleted branches.

## 7. Migration and compatibility review

No storage or protocol migration is required. Structured providers retain their
canonical path; explicitly configured textual adapters retain their existing
single repair allowance. Legacy rendered-only status classification is isolated
in `ToolExecutionOutcome::legacy`.

## 8. Security review

The broker principal correction from M009 remains intact. Dead synthetic
bootstrap execution was removed, and recovery does not broaden denied tool
authority or fabricate permission metadata.

## 9. Documentation and operations

Updated `architecture/agent.md`, the M010 implementation status, and the
corrective addendum. No CI lane, matrix, scheduled audit, release automation, or
new static guard was added.

## 10. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| low/operational | Exact final hosted `CI / verify` evidence is unavailable because the workflow has no dispatch trigger and PR #74 reports no checks for the pushed branch | Named condition for conditional closure; rerun on the exact SHA when GitHub exposes a normal PR check |

No critical, high, or medium production finding remains in M010 scope.

## 11. Roadmap disposition

M010 is implemented and conditionally closed. M009 remains historical
predecessor integration evidence and is reconciled by this corrective record;
its closure text is not rewritten. The corrective addendum remains
`conditionally closed` until the exact hosted run is available.

## 12. Registry updates

- M010 moved from dependency-ready to recently closed as `conditionally closed`.
- The agent-runtime corrective workstream is recorded as conditionally closed.
- Blocked-work audit: `plans/registry.md` contains no registered plan blocked by
  M010, so no future plan was eligible for promotion to `ready`.
- No additional corrective pass is registered; the only remaining item is the
  explicitly named external hosted-verification condition.
