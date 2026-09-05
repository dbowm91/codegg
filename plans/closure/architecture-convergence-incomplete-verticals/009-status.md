# Architecture Convergence M009 — Closure Status

Status: conditionally closed

Source implementation plan: `plans/implementation/architecture-convergence-incomplete-verticals/009-strict-closure-evidence-and-guard-triage.md`

Source subsystem roadmap: `plans/subsystems/architecture-convergence-strict-closure-corrective-addendum.md`

Historical source roadmap: `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Repository baseline reviewed: `7635a13` — current implementation/evidence candidate

Implementation commits:

- `50df9c3` — activate M009 in the planning registry.
- `7635a13` — resolve the two daemon-cwd findings, reconcile the accepted v49 catalog layout, and correct the tool-broker guard's long-test false positive.

## 1. Executive finding

M009 is implemented and the three M008 guard findings are resolved on the
current tree. The two Python `current_dir` findings were corrected without
changing the Python execution contract: the AST-only scanner now uses a
neutral temporary directory, and the interpreter-prefix probe now uses the
explicit workspace root. The project-catalog guard and current architecture
documentation now agree with the accepted `STORAGE_LAYOUT_VERSION = 49`.
The ReviewTool report was test-only; the broker guard now tracks test scopes
by balanced braces, so it no longer mistakes a long `#[cfg(test)]` body for a
production call. No production tool call bypass was found.

The aggregate workstream is conditionally closed, not strictly closed. The
current host still cannot execute root CodeGG test binaries: focused compaction,
managed-process, shell-timeout, Git, and command-intent attempts reach the
test build and then encounter or enter the existing x86_64/arm64 macOS native
linker failure/stall. Compile-time, leaf-crate, static-guard, and quick
verification evidence passes. The remaining condition is operational and
must be rerun on a compatible host or exact-head CI; no current production
correctness defect was demonstrated.

Before M009, the source roadmap/registry incorrectly claimed the aggregate
subsystem was `closed` while M001-M004 and M006 each retained an explicit
`conditionally closed` record. M009 corrects the aggregate status to
`conditionally closed`, preserves those historical records, and records the
precise remaining evidence condition here.

## 2. Requirement-to-evidence matrix

| Milestone | Historical condition | Current evidence | Disposition |
|---|---|---|---|
| M001 | `cargo test -p codegg --test compaction` could not execute on the mixed-architecture host | Current focused command was rerun and failed at link time with x86_64 `lzma` symbols unavailable from arm64 `/opt/local/lib/liblzma.dylib`; `cargo check -p codegg --all-targets` passed | Remains a named low operational condition; no compaction defect found |
| M002 | Root managed-process/runtime evidence and strict feature-heavy Clippy were incomplete | `cargo check -p codegg --all-targets`, quick verification, execution-ownership, sandbox, and broker/path guards pass; the no-default-features managed-process test reached root test compilation and entered the same silent host link phase | Remains a named low operational condition; canonical process ownership is intact |
| M003 | Root focused Git runtime evidence and strict all-features Clippy were blocked by host/unrelated verification issues | `eggit` 75 tests, `codegg-git` 358 tests, core worktree 12 tests, Git forbidden-pattern guard, and all-target compile pass; root policy-drift test reached the silent link phase | Leaf/domain evidence resolves the implementation condition; root runtime rerun remains operationally incomplete |
| M004 | Hosted coordinator coverage passed but the unrelated shell timeout test failed; local root runtime evidence was unavailable | Current exact shell timeout test was attempted on current head and entered the silent root test link phase; current source still routes shell execution through `ManagedProcessService` | Historical shell failure is not reclassified as resolved without a runtime result; remains an independently owned low operational condition |
| M006 | Focused command-intent runtime evidence could not link and hosted workspace completion was not available at closure time | Current all-target compile and quick verification pass; the focused command-intent no-run attempt reached root test compilation and entered the same silent link phase | Remains a named low operational condition; command pipeline source/static contract is intact |

The prior M001-M004/M006 conditions were not silently erased by broad green
commands. The leaf and compile results above exercise the same ownership
surfaces where possible, but they do not substitute for root runtime binaries
that could not link on this host.

## 3. Production implementation evidence

- `src/python_script/analyze.rs` no longer infers a daemon workspace from
  process-global cwd. The stdin-only AST scanner runs from
  `std::env::temp_dir()` and has no workspace filesystem dependency.
- `src/python_script/executor.rs` supplies the already-resolved
  `workspace_root` to the bounded interpreter-prefix probe. Landlock path
  discovery therefore retains explicit workspace authority.
- `scripts/check_tool_broker_boundary.py` now recognizes complete test scopes
  with brace depth instead of a fixed 20-line lookback. This documents and
  enforces the existing test-only exception without adding a production
  allowlist entry.
- `scripts/check_project_catalog_invariants.py` now checks the accepted
  repository terminal layout version, 49. The current `storage/mod.rs`
  migration sequence and `architecture/storage.md` establish v49; M009 did
  not change schema or migration code.
- Current architecture documents that stated stale versions were updated to
  49: `architecture/overview.md`, `codegg_core.md`, `core.md`,
  `tool_programs.md`, and `workspace_services.md`.

## 4. Verification executed

Successful current-head commands:

```text
rtk cargo fmt --all
rtk cargo fmt --all -- --check
rtk env CARGO_BUILD_JOBS=1 cargo check -p codegg --all-targets
rtk cargo test -p eggcontext                         # 18 passed
rtk cargo test -p egggit                             # 75 passed
rtk cargo test -p codegg-git                         # 358 passed
rtk env CARGO_BUILD_JOBS=1 cargo test -p codegg-core worktree --lib # 12 passed
rtk env CARGO_BUILD_JOBS=1 cargo clippy -p codegg --lib -- -D warnings
rtk scripts/verify.sh quick
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_git_forbidden_patterns.py
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_project_catalog_invariants.py
rtk python3 scripts/check_tool_broker_boundary.py
rtk git diff --check
```

`scripts/verify.sh quick` passed generated-agent freshness, core-boundary,
sandbox, execution-ownership, formatting, and locked workspace all-target
checking. The catalog guard reports 7/7 checks passed. The tool-broker guard
reports no direct calls outside the broker, and the cwd guard reports no
protected `current_dir` uses.

Current-head focused runtime attempts that could not execute:

```text
rtk env CARGO_BUILD_JOBS=1 cargo test -p codegg --test compaction
rtk env CARGO_BUILD_JOBS=1 cargo test -p codegg --no-default-features --lib managed_process -- --nocapture
rtk env CARGO_BUILD_JOBS=1 cargo test -p codegg --lib shell::runtime::tests::runtime_timeout_emits_timed_out_event -- --exact --nocapture
rtk env CARGO_BUILD_JOBS=1 cargo test -p codegg git_mutations::policy_drift_tests --lib
rtk env CARGO_BUILD_JOBS=1 cargo test -p codegg --lib command_intent --no-run
rtk env CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features -- -D warnings
```

The compaction attempt produced the concrete linker error: arm64
`/opt/local/lib/liblzma.dylib` was ignored for the x86_64 target, leaving
`_lzma_code`, `_lzma_end`, and `_lzma_stream_decoder` unresolved. The other
root attempts compiled or entered the same silent link phase and were
manually stopped after no active compiler/linker process remained. No test
failure or changed-path compiler diagnostic was emitted. Strict all-feature
Clippy likewise compiled the workspace and entered that host-specific phase.
These are local results; no hosted CI result was invented for this candidate.

## 5. Invariant review

- Daemon execution and durable-state authority remain unchanged.
- Scheduler admission remains separate from managed process execution and
  ToolBroker execution.
- The Python corrections preserve typed argv, explicit cwd, bounded capture,
  cancellation, sandbox policy, and fail-closed capability checks.
- The ReviewTool path remains a normal registered tool. Production
  model/agent execution is broker-owned; the only reported direct call is in
  `#[cfg(test)]` coverage of the tool's own structured adapter.
- Storage layout documentation and guard expectations now follow the
  repository's canonical v49 migration authority; no version was changed to
  make the guard green.
- Git environment/redaction, typed argv, run identity, rerun linkage, LSP
  checked mutation, projection, and child-authority boundaries were not
  widened or redesigned.

## 6. Failure and recovery review

The code changes do not alter durable state machines, protocol framing,
scheduler recovery, process cancellation, or retry behavior. The explicit
workspace-root probe continues to use the existing managed process service;
the neutral AST scanner is bounded and receives source only through stdin.
The root linker condition prevents runtime confirmation but does not create a
new runtime recovery path or conceal a production error.

## 7. Migration and compatibility review

No storage migration, protocol change, durable identity change, or user-visible
compatibility change was made. `STORAGE_LAYOUT_VERSION` was not changed by
M009; only the stale invariant expectation and architecture references were
reconciled with the already-accepted v49 schema. Historical run/session,
projection, and compatibility transport data remain readable.

## 8. Security review

The pass remains fail-closed. No guard was suppressed for a production call,
no authority was widened, and no shell substitution replaced typed argv. The
AST scanner does not gain workspace access; the Landlock discovery probe uses
the owning explicit workspace root. ReviewTool's direct test call does not
authorize or execute production work, while production broker validation,
authority grants, output bounds, provenance, and audit behavior remain
unchanged. No raw secret or authenticated remote was added to source,
durable state, or closure evidence.

## 9. Documentation and operations

The active implementation plan is now `implemented`. The corrective addendum
and source roadmap now identify M009 as the final conditional closure pass.
The registry keeps the unrelated runtime-safety C002 supported-Linux Landlock
condition blocked and independent. No new CI lane, scanner, coverage gate,
benchmark, release automation, or verification framework was introduced.

The remaining operational action is to rerun the named root focused suites,
the shell timeout test, and strict all-feature Clippy on an x86_64-compatible
macOS toolchain or exact-head hosted CI, retaining the current static guards.

## 10. Unresolved findings by severity and owner

| Severity | Finding | Owner/disposition |
|---|---|---|
| critical/high/medium | None found in the M009 changes or M001-M008 ownership boundaries | Closed |
| low / operational | Root CodeGG test binaries and strict all-feature Clippy cannot complete on this host's mixed x86_64/arm64 native-link environment | Development verification/toolchain owner; exact evidence remains a condition of strict closure |
| low / operational | Current-head shell timeout test has no runtime result because the root test binary cannot link; the historical hosted failure remains visible | Shell/runtime verification owner; no production defect demonstrated and no separate corrective plan registered |

No new corrective implementation plan is registered. M009 found no material
out-of-scope production defect, migration need, protocol change, or boundary
redesign that would justify absorbing or inventing a follow-up plan.

## 11. Roadmap disposition

M009 is conditionally closed. The architecture-convergence workstream is
substantially complete and its three M008 guard findings are resolved, but
strict aggregate closure remains conditional on compatible-host root runtime
evidence. The historical M001-M004/M006 conditional records remain immutable;
their operational conditions are consolidated here rather than rewritten.

## 12. Registry updates and downstream unblock audit

The registry and dependency text in the architecture-convergence plans were
audited. The only registered blocked work is runtime-safety C002's supported-
Linux Landlock fixture evidence, which is independent of M009 and remains
blocked. No registered future plan lists M009, or an M009-resolved condition,
as a hard or interface dependency, so no future plan became dependency-ready
and no unrelated status was promoted. No new follow-up plan was required.

The final planning changes:

- mark the implementation plan `implemented`;
- mark the corrective addendum, source roadmap, and registry aggregate
  `conditionally closed` with this record as the controlling evidence;
- remove M009 from dependency-ready work;
- add M009 to recently completed control points; and
- preserve the independent runtime-safety C002 blocker.

Final recommendation: `conditionally closed` pending the named compatible-host
root runtime and strict-Clippy evidence.
