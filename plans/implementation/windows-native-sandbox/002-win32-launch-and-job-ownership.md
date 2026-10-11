# Windows Native Sandbox Milestone 002 — Win32 Launch and Job Ownership

Status: ready for handoff

Repository baseline: `8e9d8b01e5c229715c8e4dea929e050b391e252c`

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#7-milestones`

Long-term requirements:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs: `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`; `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`

Primary class: infrastructure

## 1. Objective

Implement one supervised native Win32 process launch lifecycle with private status, restricted-token creation plumbing and Job Object descendant ownership. Deliver process control but do not yet claim filesystem or network sandboxing.

## 2. Why this milestone is ready / dependency gate

Hard dependency: M001 closed with stable platform launcher contract and no-fallback behavior. Do not start merely because a cross-compile succeeds.

## 3. Current implementation evidence

`managed_process.rs` Windows `terminate_child` currently kills only the direct child; Windows `SandboxRequest::Required` returns unavailable; `src/bin/codegg-sandbox-helper.rs` is Unix-only. `crates/codegg-client/src/windows_process.rs` is limited to liveness detection and not an execution launcher. Windows artifact packaging lists `codegg-sandbox-helper.exe` but that file is currently a stub.

## 4. Invariants that must not regress

- Child never resumes until all required token/job/status setup checks complete.
- One owner retains Job and child handles; no orphaned process/subprocess on cancellation.
- No inherited privileged handles or daemon credentials; no forged `Enforced` status.
- No change to Linux/macOS paths.

## 5. Scope

### In scope

- Narrow Windows `cfg(windows)` platform runner with primary-token/Win32 `CreateProcessAsUserW` (or documented safe equivalent) and `CREATE_SUSPENDED`; Job Object assignment and kill-on-last-handle-close.
- Secure anonymous or access-controlled named status channel with bounded frame, trusted parent/child identity and explicit post-setup handoff.
- Spawn/timeout/reap/kill tree, bounded pipes, argv Unicode support and resource ceilings.

### Explicitly out of scope

- Filesystem containment, firewall, administrative account setup, shell syntax policy, IDE terminal UX, full native sandbox availability.

## 6. Required production changes

### Core/runtime
Add `src/managed_process/windows.rs` or similarly scoped module behind the M001 seam. Use narrow `windows-sys` features for Threading, Security, JobObjects, Foundation, Pipes and process environment. RAII every HANDLE, explicit close inheritance, exit status, stdout/stderr and job lifetimes. Make `ManagedProcessService` the sole process owner; never spawn separately from a tool facade.

### Security
Implement disposable restricted token *plumbing only*; no `Enforced` filesystem claim. Open child suspended, bind Job with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and no breakaway, verify handles/status, then resume; terminal failure closes Job and process handles. Detect unsupported OS features accurately.

### Protocol/storage
Reuse M001-private status framing or version its transport only; no new durable protocol or DB columns. Provenance remains job/attempt-scoped.

### Documentation/guards
Update process-ownership matrix and Windows execution semantics.

## 7. Ordered work packages

### WP-A — Parent/child launch adapter
Create Windows-only spawn with owned process/thread handles, environment block and cwd; prove failure cases do not execute target.

### WP-B — Job supervision
Create and configure Job Object before resume, assign suspended child, disallow breakaway, impose bounded active process/memory/time as configured; ensure nested-job incompatibilities fail explicitly. Probe child tree on timeout, cancellation and parent death.

### WP-C — Private status and output
Implement bounded protected status transport and current version state-machine, split stdout/stderr pipes with precise inheritance, and guaranteed reaping/EOF behavior.

### WP-D — Native verification harness
Add Windows-only Rust fixtures that spawn child/grandchild and assert no survivors after closing job or cancellation. Add focused Windows CI lane as a non-security smoke prerequisite for M004.

## 8. Failure, cancellation, restart, and contention semantics

Cancellation races with `ResumeThread` must never yield running unsupervised code. On failure before resume terminate the suspended child. A stale PID never substitutes for owned HANDLE; on daemon crash close-on-process-death Job handle terminates remaining descendants if no untrusted inherited duplicate. Concurrent runs must have distinct Jobs, paths, cancellation and status channels.

## 9. Compatibility and migration

No changes to Unix helper. Windows installation still bundles helper as needed but may rely on secure parent-side launch per ADR-0016; decide and document exact sibling/binary usage. No privilege elevation or permanent machine state.

## 10. Required tests

- Windows native integration: process identity/exit, Unicode/shell-special args, bounded output, timeout and cancellation, Job chain grandchild, sibling isolation.
- Security negative: inherited handles, failed AssignProcessToJobObject, partial status, forced kill, breakaway attempt, parent death.
- Non-Windows: unit tests/conditional compilation verify Windows path cannot report filesystem or network containment.

## 11. Required verification commands

`cargo check --target x86_64-pc-windows-msvc -p codegg --bin codegg-sandbox-helper` (with target/toolchain)
`cargo test -p codegg --test managed_process` (where target exists; otherwise narrower test selector)
`cargo test -p codegg --lib managed_process`
`python3 scripts/check_execution_ownership.py`
`cargo fmt --all -- --check`
`bash scripts/verify.sh quick`
On Windows: `cargo test -p codegg --test windows_process_launcher -- --nocapture` (new native test target).

## 12. Documentation updates

`architecture/process-tool-execution-ownership.md`, `architecture/security.md`, `architecture/testing.md`; record Windows installation expectations.

## 13. Acceptance criteria

- Real Win32 command and descendants execute through the same managed-process result path; cancellation kills the entire Job-owned tree.
- Failed setup and status never execute target or report enforced.
- No filesystem/network containment advertised.

## 14. Stop conditions

Stop if CI runner restricts Job assignment in a way preventing full descendant ownership; if Job assignment races with resume; if nested Job semantics cannot be qualified; or if source requires a second process service. Document host versions and Windows runner restrictions.

## 15. Closure evidence required

Windows host build metadata, native process/descendant behavioral traces, forced failure output, manual PROCESS/JOB handle audit, Unix regression results, scripts outputs, notes for M003.

## 16. Handoff notes

Do not call test-only resource supervision a filesystem sandbox. A restricted token with no DACL policy is incomplete security. Include actual Windows execution evidence, not only `cargo check`.
