# Windows Native Sandbox Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/windows-native-sandbox/002-win32-launch-and-job-ownership.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#7-milestones`

Repository baseline reviewed: `739c524d791c9d8d4e81a827fa77e8651e5d1550`

Closure revision: `6c933866d5f8dcb329f70e364672fd941260997c` (native tests)

Windows workflow: `https://github.com/dbowm91/codegg/actions/runs/38105765923`

## 1. Executive finding

M002 is strictly closed for its bounded process-supervision scope. Windows
`SandboxRequest::Disabled` launches run through the canonical managed-process
service. The child is created suspended, assigned to a dedicated Job Object
with kill-on-close and no breakaway, and resumed only after assignment succeeds.
Timeout, cancellation, output bounds, root exit, pipe EOF, and sibling ownership
have native behavioral coverage. An injected Job-assignment failure proves the
suspended target never runs.

This is not filesystem or network sandboxing. Constrained Windows requests
continue to fail before spawn; this path inherits the caller's token. M004 owns
restricted-token creation, its protected status handoff, and the enforcement
consumer. M003 is now dependency-ready for Windows command construction and
policy-input compatibility work.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Windows launch is owned by `ManagedProcessService` | `src/managed_process/windows.rs`; `src/managed_process.rs` | pass | Only explicitly unconstrained Windows requests reach the native launcher. |
| Child cannot run before Job ownership is established | `failed_job_assignment_never_runs_the_suspended_target`; Windows native workflow run `38105765923` | pass | Failure injection terminates/cleans the suspended child and confirms the target marker is absent. |
| Job owns descendants and does not leak between siblings | `tests/windows_process_launcher.rs`; native workflow run `38105765923` | pass | Sibling isolation, grandchild cancellation, and root-exit cleanup passed. |
| Timeout and cancellation terminate the process tree | Windows native integration suite | pass | Native timeout and descendant-cancellation tests passed. |
| Output, argv, and constrained-request behavior remain bounded and truthful | Windows native integration suite | pass | Unicode argv, output bound, and constrained request rejected before launch passed. |
| Job handle is not inherited and close owns the tree | `src/managed_process/windows.rs` manual HANDLE audit | pass | Job is created with null security attributes; only configured standard I/O handles are inherited by Tokio's process adapter. Job uses kill-on-close and no breakaway. |
| Windows execution does not claim filesystem/network enforcement | managed-process policy and architecture documentation | pass | `CleanupDiagnostics.job_object_established` records supervision only; constrained requests remain unavailable. |
| Unix behavior and repository guards remain intact | ARM64 focused tests and `scripts/verify.sh quick` | pass | Results below. |

## 3. Native Windows evidence

Hosted workflow run `38105765923` passed on revision
`6c933866d5f8dcb329f70e364672fd941260997c`:

- OS: Microsoft Windows Server 2025 Datacenter, version `10.0.26100`, build `26100`.
- Rust: `rustc 1.89.0 (29483883e 2025-08-04)`, target `x86_64-pc-windows-msvc`.
- `cargo test --locked -p codegg --test windows_process_launcher -- --nocapture --test-threads=1`: 8 passed, 0 failed.
- `cargo test --locked -p codegg --lib managed_process::windows::tests::failed_job_assignment_never_runs_the_suspended_target -- --nocapture`: 1 passed, 0 failed.

The integration tests cover the Rust child fixture, Unicode argv, bounded
output, fail-closed constrained requests, root-exit/pipe-EOF ordering, timeout,
descendant cancellation, and sibling isolation. The failure-injection test
forces Job assignment failure before resume and verifies the target does not
execute.

The initial workflow attempts exposed two real compatibility issues and drove
the fixes before this passing run: sanitized Windows environments needed
`SystemRoot`/`WINDIR` and temporary-directory variables for native PowerShell
startup, and tree fixtures needed deterministic Rust child processes instead
of shell `Start-Process` behavior. A subsequent failure-injection compile also
found a Unix-only transport socket test module lacking a `cfg(unix)` gate; that
gate was added before the passing run. These failures are retained in workflow
history, not treated as successful evidence.

## 4. Unix and repository verification

On Rust 1.89 ARM64 macOS:

- `cargo test -p codegg --lib managed_process`: 16 passed.
- `cargo test -p codegg --test managed_process_descendants`: 5 passed.
- `bash scripts/verify.sh quick`: passed at the final implementation revision.
- `cargo fmt --all -- --check`, `python3 scripts/check_execution_ownership.py`,
  and `git diff --check`: passed.

The default x86_64 Homebrew Rust 1.98 link attempt failed in native dependency
linking for the host deployment target; the same focused tests passed using the
repository Rust 1.89 ARM64 toolchain. Local Windows cross-compilation could not
be performed because the host lacks the MSVC native C toolchain. The hosted
Windows run supplies the required native evidence.

## 5. Invariant and security review

- The child is created suspended and is not resumed until Job assignment and
  required supervision setup have succeeded.
- Job and process/thread handles are RAII-owned. Failure before resume closes
  the Job and terminates the suspended child; timeout/cancellation terminate
  the Job-owned tree.
- Each run gets a distinct Job. Breakaway is not enabled. Root exit closes the
  Job before waiting for inherited pipe EOF, preventing an unowned descendant
  from keeping captured output open indefinitely.
- Tokio receives only configured stdio inheritance. The Job handle is
  non-inheritable; no daemon credentials or privileged handles are passed.
- Windows `SystemRoot`, `WINDIR`, `TEMP`, and `TMP` are retained in the
  sanitized environment so normal Windows process startup works.
- Only `SandboxRequest::Disabled` is accepted. Required/constrained requests
  still fail before child creation. The child's caller token is not restricted.
- Job ownership is process supervision, not filesystem or network containment.
  No `Enforced` filesystem/network result is produced by this work.

No Unix helper or Unix status framing changed. No database schema, public
protocol, or durable token handle was added.

## 6. Documentation and operations

Updated `architecture/process-tool-execution-ownership.md`,
`architecture/security.md`, `architecture/testing.md`, and the Windows native
sandbox plan to describe the supervised-only scope, environment requirements,
and native test lane. The workflow records host OS/build and Rust metadata on
each run.

## 7. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| informational | Windows constrained filesystem and network enforcement are not implemented. | Required/constrained Windows requests remain unavailable. | M004 must prove restricted-token/ACL read/write behavior before enabling constrained launches. |
| informational | Parent-death, breakaway, and protected-status qualification exceed M002's bounded native smoke scope. | No protected status channel or constrained token exists in M002. | M004/M006 must qualify the applicable failure and containment cases before any enforcement claim. |

No unresolved high or medium finding remains in M002 scope.

## 8. Roadmap and dependency disposition

M002 is strictly closed. M003's hard dependency is satisfied, so M003 moves
from blocked to ready. M004 remains blocked on M003; M005 remains blocked on
M004 and a provisioned Windows identity/network test host; M006 remains blocked
on M004 (and on M005 only for offline-network claims). M007 remains an optional,
independent evidence-only plan. No other registered plan declares M002 as a
dependency.

Windows support remains constrained/unavailable pending M004 and M006 evidence.
