# Windows Native Sandbox Milestone 002 — Win32 Launch and Job Ownership

Status: active

Repository baseline: `739c524d791c9d8d4e81a827fa77e8651e5d1550`

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

Implement one supervised native Win32 process launch lifecycle with Job Object descendant ownership. Deliver process control but do not yet claim filesystem or network sandboxing.

Scope disposition after implementation review: M002 launches only unconstrained
(`SandboxRequest::Disabled`) Windows children. All constrained Windows requests
still fail before process creation, so there is no Windows sandbox setup result
to send over a status channel and no restricted token whose access policy has
been validated. The child inherits the caller's existing token for this
FullHost-only path. M004 owns restricted-token creation and the Windows private
status handoff together with the enforcement backend that consumes them. This
keeps M002 from introducing unused or security-ambiguous token/status plumbing;
it does not relax any constrained-request guarantee.

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
No restricted token or enforcement status is created in M002. Open the
unconstrained child suspended, bind a private Job with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and no breakaway, then resume; terminal
failure closes Job and process handles. Do not label Job ownership as
filesystem/network enforcement. M004 must add restricted-token and protected
status setup as fail-closed resume gates before any constrained Windows target
can run.

### Protocol/storage
M002 has no child status channel because no Windows backend can report
constrained enforcement yet. Preserve M001's private Unix framing unchanged;
M004 owns a Windows-specific private status handoff. No durable protocol or DB
columns are added. Provenance remains job/attempt-scoped.

### Documentation/guards
Update process-ownership matrix and Windows execution semantics.

## 7. Ordered work packages

### WP-A — Parent/child launch adapter
Create Windows-only spawn with owned process/thread handles, environment block and cwd using the caller's current token for explicitly unconstrained runs; prove setup failure does not execute target. Restricted-token launch remains M004 scope.

### WP-B — Job supervision
Create and configure Job Object before resume, assign suspended child, disallow breakaway, impose bounded active process/memory/time as configured; ensure nested-job incompatibilities fail explicitly. Probe child tree on timeout, cancellation and parent death.

### WP-C — Output and handle inheritance
Use Tokio's explicit stdio inheritance list and non-inheritable Job handles; keep stdout/stderr separate and bounded, and guarantee reaping/EOF behavior. Do not add an unused sandbox status channel; M004 must establish a protected private status handoff before constrained Windows execution.

### WP-D — Native verification harness
Add Windows-only Rust fixtures that spawn child/grandchild and assert no survivors after closing job or cancellation. Add focused Windows CI lane as a non-security smoke prerequisite for M004.

## 8. Failure, cancellation, restart, and contention semantics

Cancellation races with `ResumeThread` must never yield running unsupervised code. On failure before resume terminate the suspended child. A stale PID never substitutes for owned HANDLE; on daemon crash close-on-process-death Job handle terminates remaining descendants if no untrusted inherited duplicate. Concurrent runs must have distinct Jobs, paths, cancellation and status channels.

## 9. Compatibility and migration

No changes to Unix helper. Windows installation still bundles helper as needed but may rely on secure parent-side launch per ADR-0016; decide and document exact sibling/binary usage. No privilege elevation or permanent machine state.

## 10. Required tests

- Windows native integration: exit status, Unicode/shell-special args, bounded output, timeout and cancellation, Job chain grandchild, sibling isolation, root-exit cleanup, and constrained-request fail-closed behavior.
- Security review: Job handle is created non-inheritable; Tokio's Windows process adapter supplies only configured stdio handles. Native failure injection for Job assignment, breakaway, parent death, and protected status remains a required M004/M006 qualification item before constrained execution is enabled.
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
