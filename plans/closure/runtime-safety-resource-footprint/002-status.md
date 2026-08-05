# Runtime Safety, Resource Control, and Footprint Milestone 002 — Closure Status

Status: conditionally closed

Source implementation plan: `plans/implementation/runtime-safety-resource-footprint/002-canonical-bounded-process-execution.md`

Source subsystem roadmap: `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`

Repository baseline reviewed: `4d540ce315c9ef2a1c07544cd42df0efc43708e1`

Implementation commits: `6e5fbfd` — centralize bounded process execution and process-tree cleanup

## 1. Executive finding

The M002 production implementation is complete. `ManagedProcessService` is
the canonical finite-process boundary for managed argv, raw shell, native
dispatch, managed Git fallback, Python, and scheduler execution. It owns typed
launch inputs, sanitized environment, stdin, sandbox-helper launch/outcome,
concurrent bounded collection, timeout/cancellation, Unix session cleanup,
direct-child reap, provenance, and typed failure distinctions.

Strict closure is conditional because this Darwin workspace cannot provide the
supported-Linux Landlock enforcement evidence required by M001, and the
repository's hosted `verify` result is not available until an accepted remote
revision is evaluated by the hosted workflow. No unresolved critical, high,
or implementation-related medium correctness finding remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| One typed finite-process request/result boundary | `src/managed_process.rs`; Bash, Python, and scheduler adapters | pass |
| Explicit executable/argv, cwd, env, stdin, timeout, cancellation, sandbox, provenance | `ManagedProcessRequest`, `EnvironmentPolicy`, `StdinPolicy`, `SandboxRequest`, `ProcessProvenance` | pass |
| Independent bounded stdout/stderr collection | `read_bounded`, head/tail `BoundedOutput`, dual-stream unit test | pass |
| Concurrent pipe draining without unbounded retention | Two spawned collectors; large-output and dual-stream tests | pass |
| Explicit overflow behavior | Default `ContinueDrain`; opt-in `Terminate`; `OutputLimitExceeded` test | pass |
| Timeout and cancellation distinction | Managed-process unit tests; scheduler cancellation suite; descendant integration suite | pass |
| Descendant cleanup and direct-child reap | Unix `setsid`, verified PGID signaling, grace/escalation cleanup; 5 descendant tests | pass |
| Sandbox request/outcome preserved | M001 `SandboxLaunchSpec` consumed by canonical service; enforced marker mapped to typed outcome | pass on supported helper path; Linux runtime evidence pending |
| Governed finite callers migrated | Bash raw/native/Git fallback, Python, ManagedArgvExecutor | pass |
| Lifecycle-specific exemptions inventoried | TestRunner, LSP, interactive shell/notification, git compatibility, plugin/runtime and bootstrap paths documented in ownership manifest | pass |
| Guard rejects reintroduced unbounded collection | `check_execution_ownership.py --self-test`; regular guard | pass |

## 3. Production implementation evidence

- `src/managed_process.rs` now separates executable from argv, supports null or
  byte stdin, applies independent stream caps, continues draining by default,
  optionally terminates on overflow, and returns per-stream truncation data.
- Sandbox requests are launched through the M001 private helper from the
  canonical service. Reserved helper markers are consumed into a typed
  `SandboxExecutionOutcome`; setup failures are distinct `SandboxFailed`
  errors.
- Unix cleanup validates the child process group while the leader is alive,
  retains the known group identity through leader exit, sends SIGTERM, waits a
  bounded grace interval, escalates to SIGKILL, and reaps the direct child.
- `src/tool/bash.rs` no longer uses direct `.output()` for raw shell, native,
  or managed Git fallback execution.
- `src/python_script/executor.rs` routes user Python through the canonical
  service and wires the scheduler cancellation token into it. Its interpreter
  prefix probe is a separate 64 KiB-capped setup probe.
- `src/scheduler/executors.rs` maps output-limit termination to failed job
  status while preserving timeout and cancellation status.

## 4. Verification executed

Local commands and results:

```text
cargo fmt --all                                  pass
cargo check -p codegg --all-targets              pass
cargo clippy -p codegg --all-targets -- -D warnings  pass
cargo test -p codegg managed_process --lib -- --test-threads=1  11 passed
cargo test -p codegg python_script --lib -- --test-threads=1    197 passed
cargo test -p codegg bash --lib -- --test-threads=1             96 passed
cargo test --test managed_process_descendants -- --test-threads=1 5 passed
cargo test --test command_routing_execution_ownership -- --test-threads=1 20 passed
cargo test --test scheduler_cancellation -- --test-threads=1 10 passed
cargo test --test sandbox_landlock -- --test-threads=1          1 passed (Darwin unavailable path)
python3 scripts/check_execution_ownership.py --self-test       pass
python3 scripts/check_execution_ownership.py                  pass
scripts/verify.sh quick                                        pass
```

The hosted `verify` result is intentionally not claimed here: routine CI is
PR-gated in this repository and no hosted result exists for this revision yet.
Supported-Linux Landlock fixture execution is likewise not claimed on Darwin;
the existing non-Linux sandbox test passed and M001 retains the exact Linux
command and evidence requirement.

## 5. Invariant review

- Scheduler and daemon ownership remains unchanged; migrated callers still
  enter through `JobSubmissionService` where required.
- No process-global cwd mutation was introduced. The canonical request carries
  an explicit path, with existing standalone Bash fallback preserved.
- Timeout, cancellation, and output-limit termination are distinct result
  states. Spawn, wait/read, and sandbox failures are typed errors.
- Output retention is strictly bounded per stream; total byte and line counts
  are saturating diagnostics rather than retained buffers.
- Negative-PID signaling is attempted only after the child session was
  successfully established, and PGID mismatch refuses signaling.
- No new scheduler, protocol, storage, supervisor dependency, or CI lane was
  added.

## 6. Failure and recovery review

The descendant suite covers cancellation, timeout, stubborn descendants,
process-group identity, and bounded output. During an independent second-pass
review, timeout cleanup exposed a leader-exit/PGID race; the implementation was
corrected to force-kill the retained process group after the grace interval.
The full descendant suite passed after that correction. No collector task leak
was observed in the focused termination tests.

## 7. Migration and compatibility review

No database or daemon protocol migration was needed. Existing human-readable
stdout/stderr and exit-code rendering remain available; truncation is now
explicit in managed Bash output. Python policy/snapshot behavior and scheduler
authority remain intact. Non-Unix builds retain direct-child cleanup only and
do not claim descendant cleanup without a platform-specific implementation.

## 8. Security review

The M001 child-only sandbox boundary is retained. Environment policies clear
the inherited environment and restore reviewed variables only. Process-group
cleanup refuses a PGID mismatch and cannot intentionally target the daemon's
group. The ownership guard has a negative self-test proving a temporary
`.output()` fixture is rejected. No critical, high, or medium security finding
was identified in the implementation or second-pass review.

## 9. Documentation and operations

Updated `architecture/jobs.md`, `docs/execution-ownership.md`, and the
machine-readable ownership manifest. The operational follow-up is to run the
existing `cargo test --test sandbox_landlock -- --test-threads=1` on a
Landlock-capable Linux host and record kernel, ABI, and fixture results, then
obtain the hosted `verify` result for strict closure.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Supported-Linux sandbox evidence is unavailable on Darwin; hosted verification is not yet recorded | Strict closure cannot be claimed, although focused production behavior passes locally | Run the existing Linux fixture and hosted verify on an accepted remote revision; then ratify this record as closed |

## 11. Roadmap disposition

M002 is conditionally closed: production implementation, focused tests,
static guards, and documentation are complete. M003 is not unblocked because
the hard dependency is not strictly closed while M001's supported-Linux
evidence remains outstanding. M004 and M005 remain ready and unchanged. M006,
M007, and M008 remain blocked by their independently listed dependencies.

## 12. Registry updates

- M002 implementation plan is marked `implemented`.
- M002 is removed from blocked work and recorded as conditionally closed with
  this closure record.
- The blocked-work audit searched every plan dependency on M002. No plan met
  all hard-dependency conditions for promotion: M003 still depends on strict
  M002/M001 closure, M007 also depends on M003/M005/M006, and M008 depends on
  M001–M007.
- No downstream status was silently changed; M004 and M005 remain the only
  dependency-ready runtime-safety plans.
