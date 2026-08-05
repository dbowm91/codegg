# Runtime Safety, Resource Control, and Footprint Milestone 003 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/runtime-safety-resource-footprint/003-typed-argv-and-shell-routing-convergence.md`

Source subsystem roadmap: `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`

Repository baseline reviewed: `bc3efd8`

Implementation commits:

- `bc3efd8` — implement typed argv and shell-routing convergence

## 1. Executive finding

M003 is implemented and strictly closed. Native command planning now carries a
typed `NativeCommand { executable, argv }` through routing and into the M002
managed-process adapter. The previous native `split_whitespace()` reparsing and
the Git parser-error-to-shell fallback are removed. Raw shell remains a
distinct route selected by the shell-shape classifier or an explicit shell job.

The existing durable job schema already had separate typed argv and shell
payloads, so no storage or protocol migration was required. No authority,
scheduler, shell-parser framework, or user-facing command feature was added.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Typed executable and argv remain separate | `NativeCommand`; `ExecutionBackend::ManagedArgv`; `RoutingDecision::RouteToManagedProcess`; Bash adapter calls `full_argv()` only at the M002 boundary | pass |
| Quoted, whitespace, escaped, empty, and special arguments survive | `parse_shell_words`; shell-shape tests for quoted spaces, escaped spaces, empty arguments, literal shell characters, and quoted newline/tab | pass |
| Native execution does not shell-expand arguments | Native dispatch constructs `ManagedProcessRequest` from typed argv; no `sh -c` in native/managed routes | pass |
| No lossy native reparsing | `scripts/check_execution_ownership.py --self-test` and regular guard; governed source paths contain no argv whitespace reconstruction | pass |
| Explicit shell remains distinct | `ExecutionBackend::RawShell` and `RoutingDecision::RouteToShell`; 139 adversarial routing tests pass | pass |
| Native/parser failure cannot silently select shell | Git parser failure now yields typed managed argv or `Reject`; active dispatch errors are terminal and never retries through shell; 20 execution-ownership tests pass | pass |
| Risk/approval stays attached to one plan | `CommandPlan` retains intent risk and permission requests through `resolve_routing`; route is not recomputed from display text | pass |
| Cwd, environment, timeout, cancellation, sandbox, and provenance reach M002 | Existing `ManagedProcessRequest` adapter remains the only finite-process boundary; managed-process suite remains green | pass |
| Interior NUL is rejected before spawn | `ManagedProcessError::InvalidArgument`; focused managed-process test | pass |
| Legacy shell payload compatibility | `JobPayload::Shell { command, argv: Option<_> }` preserves old records as `sh -c`; `durable_jobs_phase4`: 42 passed | pass |

## 3. Production implementation evidence

- `src/command_intent/plan.rs` introduces `NativeCommand`, with separate
  executable/argv fields, complete-argv conversion only at execution, and
  display rendering that is not reused as execution input.
- Search, file-read, build, lint, and format backends require typed parsed argv;
  missing argv is rejected instead of reconstructed with whitespace splitting.
- Git parser errors are conservative: representable typed argv uses the
  managed native route; malformed or missing argv is rejected and requires an
  explicit shell route.
- `src/command_routing.rs` carries `NativeCommand` objects rather than command
  strings for native and managed routes.
- `src/tool/bash.rs` passes the typed command directly to the M002 adapter and
  retains scheduler-owned managed argv and shell payloads.
- `src/managed_process.rs` rejects interior NULs before spawn, preserving the
  canonical platform-boundary validation.

## 4. Verification executed

All commands below were run locally on the Darwin workspace. Results are shown
exactly as observed:

```text
cargo fmt --all                                      pass
cargo check -p codegg --all-targets                  pass
cargo test -p codegg --lib shell_shape -- --test-threads=1  41 passed
cargo test -p codegg --lib command_intent::plan -- --test-threads=1  70 passed
cargo test -p codegg --lib command_routing -- --test-threads=1  17 passed
cargo test -p codegg --lib managed_process::tests -- --test-threads=1  11 passed
cargo test -p codegg --test command_routing_adversarial -- --test-threads=1  139 passed
cargo test -p codegg --test command_routing_execution_ownership -- --test-threads=1  20 passed
cargo test -p codegg --test git_closure_matrix unknown_subcommand_falls_back_to_managed_argv -- --test-threads=1  1 passed
cargo test -p codegg --test durable_jobs_phase4 -- --test-threads=1  42 passed
python3 scripts/check_execution_ownership.py --self-test  pass
python3 scripts/check_execution_ownership.py             pass
scripts/verify.sh quick                                  pass
git diff --check                                        pass
```

The normal hosted verification trigger was not available for this direct
feature-branch workspace, so no hosted run is claimed. Per the M003 plan this
is operational evidence, not an implementation blocker. Supported-Linux
Landlock evidence remains an M001/C001 and final M008 condition; it is outside
M003's typed argv contract and did not block this closure.

## 5. Invariant review

- Native arguments are never interpreted by a shell and are not rebuilt from a
  display string.
- Empty arguments and whitespace/special-character content are retained by the
  shell-shape parser and typed route.
- Explicit shell remains the only route for pipelines, redirects, expansion,
  and other complex shell syntax.
- Active-route admission failures return errors; they do not execute the same
  command a second time through raw shell.
- Existing scheduler ownership and M002 process supervision remain unchanged.
- The ownership guard rejects a negative fixture that assigns
  `command.split_whitespace()` to process argv while allowing unrelated text
  parsing.

## 6. Failure and recovery review

Malformed or absent native argv is rejected with a routing error. A managed
process spawn failure, timeout, cancellation, output limit, sandbox failure,
and descendant cleanup behavior remain classified by M002. Legacy shell jobs
continue to use the durable shell executor and are not reinterpreted as native
argv. No retry or fallback path was added.

## 7. Migration and compatibility review

No database migration was needed. `JobPayload::ManagedArgv` already stores a
complete argv vector, while `JobPayload::Shell` retains the legacy command
string and optional shell invocation argv. Existing explicit shell commands,
Git managed fallbacks, scheduler job payloads, and public output annotations
remain compatible. Diagnostic command rendering now quotes values that would
otherwise be ambiguous, without changing the executed argv.

## 8. Security review

The change does not broaden command authority or weaken approval/risk checks.
Shell metacharacters inside a typed native argument remain literal. Raw shell
continues through the existing high-risk policy and M002 supervision. Interior
NUL values are rejected before spawn. No critical, high, or medium M003
security finding remains.

## 9. Documentation and operations

Updated:

- `architecture/command_planner.md`
- `architecture/command_routing.md`
- `scripts/check_execution_ownership.py`
- M003 plan, subsystem roadmap, and registry lifecycle state

The static guard is now part of the existing execution-ownership check and its
self-test covers both unbounded output and lossy argv reconstruction.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| none | No unresolved M003 correctness, authority, compatibility, or security finding | none | none |

Hosted verification and supported-Linux Landlock execution are recorded as
operational conditions owned by the broader runtime-safety roadmap, not as
M003 defects.

## 11. Roadmap disposition

M003 is closed. M007's other hard dependencies (M002, M005, and M006) have
accepted production dispositions, and M004 is closed as its soft measurement
input. M007 is therefore dependency-ready. M008 remains blocked on M001–M007
and the supported-Linux Landlock result.

## 12. Registry updates

- Marked M003's implementation plan and closure record `closed`.
- Removed M003 from active closure work and recorded it under recently closed
  runtime-safety milestones.
- Moved M007 from `blocked` to `ready` because M003 was its final hard
  dependency; retained its soft M004 measurement dependency as satisfied.
- Left M008 blocked with its precise M001–M007 and supported-Linux evidence
  requirements.
- Audited the runtime-safety roadmap dependency graph and registry blocked-work
  section; no other registered plan became ready, and no corrective plan is
  required.
