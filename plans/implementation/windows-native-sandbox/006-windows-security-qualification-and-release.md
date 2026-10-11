# Windows Native Sandbox Milestone 006 — Windows Security Qualification and Release

Status: blocked (M004 closure required; offline scope additionally M005)

Repository baseline: `8e9d8b01e5c229715c8e4dea929e050b391e252c`

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#7-milestones`

Long-term requirements:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs: `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`; `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`

Primary class: capability

## 1. Objective

Deliver a publishable, honestly scoped native Windows filesystem sandbox with end-to-end acceptance, CI, packaging/operator diagnostics and explicit security support matrix. Gate any offline network claim on separate M005 evidence.

## 2. Why this milestone is ready / dependency gate

Hard M004 strict closure, plus M001–M003. M005 only required for advertising offline network behavior. Do not conflate GitHub Windows compile with native sandbox qualification; use dedicated Windows hosts for tests that need administrator privileges.

## 3. Current implementation evidence

`.github/workflows/ci.yml` is Ubuntu-only; release documents Windows best-effort archive; `tests/sandbox_containment.rs` assumes Unix `SANDBOX_STATUS_FD` and `sh`. Security/permission docs currently describe Landlock/Seatbelt and Windows daemon pipe partially. Without native tests, a Windows build alone provides no containment proof.

## 4. Invariants that must not regress

- No filesystem-enforced claim without actual adversarial read+write deny evidence.
- No network-enforced claim without M005; otherwise clearly `Unrestricted`.
- Shared authorization, audit, scheduler, helper trust, Unix containment and installed binary provenance remain unchanged.
- Host-specific unsupported states remain visible and safe.

## 5. Scope

### In scope

- Windows integration/test matrix for full process/tool/agent/scheduler route and policy profile.
- Windows CI workflow with non-admin native tests; opt-in isolated Windows VM harness for privilege/ACL/firewall scenarios.
- Packaging/release smoke, installed helper provenance, operator docs, audit correctness and startup resource budgets.

### Explicitly out of scope

- Expanding desktop IDE scope, refactoring unrelated IPC/core, allowing Windows preview APIs without qualification, weakening Linux/macOS tests.

## 6. Required production changes

### CI/qualification
Build/test `x86_64-pc-windows-msvc` on a genuine Windows runner; run real constrained `ManagedProcessService` tests, shell/argv, native filesystem containment, async cancellation and side effect denial. Privileged tests run only in an explicitly isolated VM, never opportunistically elevate hosted CI. Existing Unix testing retained; note macOS live availability separately.

### Product/installer
Validate correct `codegg-sandbox-helper.exe` or parent-owned launcher installation; no PATH-based helper search. Release checks for exact binaries and dependency DLLs; Windows support matrix (minimum OS build, filesystem/network guarantees, privilege/setup prerequisites, unsupported targets).

### UI/audit
Display requested vs obtained profile, file restrictions, network state and unavailable reason; prevent incorrect 'sandboxed' badge in fallback/partial state. Reconcile path policy/approval receipts for TUI, daemon, headless, scheduler and child worktrees.

### Documentation
Update operations/tests/troubleshooting and review of changed unsafe Windows code.

## 7. Ordered work packages

### WP-A — Adversarial native suite
Convert Unix contract cases into platform-neutral assertions and Windows target variants. Test Write/ReadOnly/FullHost separately, named secret deny, outside writes, symlink/junction/hardlink/ADS/UNC, handle/pipe access, descendant inherit and shutdown.

### WP-B — CI and fault injection
New Windows native lane with deterministic fixture cleanup plus privileged VM job manual/secure. Inject token/Job/ACL/status/cancel failures; no status-only proof.

### WP-C — Production paths/audit
End-to-end live agent tool through `JobSubmissionService`→`ManagedProcessService`; ensure no bypass via alternate shell, managed argv, PythonScript or verify/test job.

### WP-D — Release gate & docs
Verify packaging/signature/path trust and operator messaging; record startup cost and support matrix; create closure record with strict no-claim rule.

## 8. Failure, cancellation, restart, and contention semantics

On CI cancellation/timeout, Job and sandbox-owned ACLs must be released. Native test fixture must clean isolated accounts/firewall rules after success/failure. Restart must reprobe and recover incomplete owned leases. Multiple projects/worktrees cannot read/write one another; historical/unavailable results stay auditable.

## 9. Compatibility and migration

Unix changes backward compatible. Windows release stays best-effort/experimental until native security evidence; only upgrade to supported when target build and installer smoke pass. New capability status fields must be versioned if observable over CoreClient. No extra host provisioning unless explicitly documented.

## 10. Required tests

- Unit/state machine + CLI/headless permission matrix, native real-process security tests, worktree concurrency and no bypass tests.
- Windows hosted CI: compile, run managed_process, shell, sandbox containment, release packaging tests.
- Dedicated test VM: admin provisioning, ACL rollback, offline networking (only if M005), restart/kill, reparse races.
- Linux/macOS: existing sandbox containment tests and static guards remain green.
- Negative: mismatched helper, forged status, stale capability, no backend and yolo cannot silently FullHost.

## 11. Required verification commands

`cargo fmt --all -- --check`
`python3 scripts/check_sandbox_contract.py`
`python3 scripts/check_execution_ownership.py`
`python3 scripts/check_sandbox_policy_wiring.py`
`cargo test -p codegg --lib security::sandbox`
`cargo test -p codegg --test sandbox_policy_wiring`
On Windows: `cargo test -p codegg --test windows_sandbox_containment -- --nocapture`
On Windows: `cargo test -p codegg --test windows_process_launcher -- --nocapture`
On Windows: `cargo test -p codegg --test windows_shell_policy -- --nocapture`
`bash scripts/verify.sh quick` on supported Unix; hosted Windows workflow and privileged VM evidence attached in closure.

## 12. Documentation updates

`architecture/security.md`, `architecture/testing.md`, `architecture/tool.md`, `architecture/process-tool-execution-ownership.md`, `docs/install.md`, `RELEASING.md`, plan registry and roadmap.

## 13. Acceptance criteria

- Target Windows host runs real constrained agent tool: allowed workspace write; denied outside write, denied sensitive read; ReadOnly denial; Job teardown and audit correct.
- Native Windows CI is green on committed revision; privileged security fixture evidence clearly labeled.
- Installer/distribution prevents missing/mismatched helper from reporting Enforced.
- Unsupported hosts fail closed and report reason, not a fake success.
- Offline label appears only if M005 separately qualified.

## 14. Stop conditions

STOP for omitted negative read-deny proof, CI only compiling rather than executing, any escaped descendant, stale ACL grant, network advertised without M005, or open critical/high security finding. Do not mark conditionally closed as fully supported without recording blockers.

## 15. Closure evidence required

Exact commit/hash, Windows build and token/job version, test command outputs, host/CI URLs, negative read/write/socket traces, installers smoke, lifecycle/fault evidence, Unix regression comparison, remaining severities and registry unblock audit.

## 16. Handoff notes

Do not stamp 'full parity' when Linux/macOS backends also do not isolate network; report obtained guarantees dimension by dimension.
