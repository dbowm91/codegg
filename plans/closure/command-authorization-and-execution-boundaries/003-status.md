# Command Authorization M003 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/command-authorization-and-execution-boundaries/003-equivalent-effects-and-execution-observability.md`

Source subsystem roadmap:

- `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md`

Repository baseline reviewed: `b01ee2f7`; M002 implementation: `b415efad`.

Implementation commits:

- `43562c68` — qualify equivalent filesystem effects and add bounded command outcome audit metadata.
- `0f0296be` — assert output truncation and completed process metadata in the live audit fixture.

## 1. Executive finding

M003 is conditionally closed for the Linux filesystem-only scope qualified on
this host. A single `WorkspaceWrite` launch policy denies an outside-root
write attempted through direct shell text, a nested shell, a generated script,
Python, a compiled native executable, and a planted Git hook. The same backend
continues to permit an allowed workspace write. A live accepted Bash call
records its permission receipt/source/revision, requested profile, bounded
risk and route labels, obtained filesystem/network outcome, process exit code,
output-truncation status, cleanup status, helper identity, and command digest.
The audit stores no command text, argv, script content, output, or secret path.

Strict closure remains conditional on a successful macOS cross-target compile.
The host has no Apple SDK/sysroot: the normal target check failed when the host
compiler rejected Apple flags, and the Clang retry stopped in native C
dependencies at missing `bits/libc-header-start.h`. The installed Windows GNU
target compiled successfully. No native macOS or Windows execution result is
claimed.

Landlock still leaves networking, Unix IPC, and host process visibility
unrestricted. No outbound TCP/UDP or IPC denial is claimed. Those dimensions
remain gated on an enforcing backend and native evidence; they are not
represented as completed capabilities here.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Same outside-root write denied across equivalent execution forms | `tests/sandbox_containment.rs::landlock_denies_equivalent_outside_write_from_shell_python_and_native_binary` | pass | Direct shell, nested shell, generated script, Python, compiled native executable, and planted Git pre-commit hook use the same `WorkspaceWrite` launch policy; all outside markers remain absent. |
| Authorized workspace effect remains available | `tests/sandbox_containment.rs::workspace_write_allows_workspace_and_refuses_everything_else` | pass | Existing positive/negative fixture remains green. |
| Sensitive path access remains denied under the workspace allowance | `tests/sandbox_containment.rs::denied_credential_nested_under_workspace_is_not_read_or_written` | pass | Synthetic credential fixture only. |
| Accepted authorization and actual Linux enforcement reach the live audit record | `tests/identity_m002_live_audit_hooks.rs::bash_constrained_audit_reports_obtained_landlock_and_unrestricted_network` | pass | Reports `enforced:landlock:abi=4`, `network=unrestricted`, receipt source, requested profile, route, helper identity, exit code, truncation, and cleanup status. Oversized output is not retained in the event. |
| FullHost command audit remains structural and redacted | `tests/identity_m002_live_audit_hooks.rs::bash_shell_success_emits_one_structural_event` | pass | Includes digest and bounded metadata only; no command string is present. |
| Permission precedence and route behavior remain intact | `cargo test -p codegg --test approval_router -- --test-threads=1`; `cargo test -p codegg --lib permission` | pass | Approval router 17/17; permission tests 124/124. The stale test expecting syntax-only escalation was aligned with the advisory classifier contract. |
| Linux supported-host filesystem behavior and unsupported fail-closed behavior | `cargo test -p codegg --test sandbox_containment -- --test-threads=1`; M002 `sandbox_policy_wiring` evidence | pass | Containment 14/14 on Linux Landlock ABI 4; constrained calls without obtained filesystem enforcement refuse before launch. |
| Live execution audit ownership and redaction regression suite | `cargo test -p codegg --test identity_m002_live_audit_hooks -- --test-threads=1` | pass | 19/19; includes denial/no-dispatch and typed Git event ownership. The constrained output-truncation assertion also passed in a focused rerun. |
| Static policy, sandbox, authorization, ownership, and audit contracts | `check_policy_surface.py`, `check_sandbox_contract.py`, `check_authorization_matrix.py`, `check_approval_router.py`, `check_sandbox_policy_wiring.py`, `check_execution_ownership.py`, `check_audit_coverage.py` | pass | All listed guards passed. |
| Routine repository verification | `scripts/verify.sh quick` | pass | Includes workspace all-target check. |
| Windows platform compile | `cargo check -p codegg --lib --target x86_64-pc-windows-gnu --locked` | pass | Used installed MinGW compiler; emitted platform-specific warnings, no errors. |
| macOS platform compile | `cargo check -p codegg --lib --target x86_64-apple-darwin --locked`; Clang retry with target flags | conditional | Both stop in native C dependencies because the Apple SDK/sysroot is absent; no Rust diagnostic in changed CodeGG sources was reported. Must be repeated in an Apple-capable environment before strict closure. |
| Network and IPC isolation | No enforcing backend is registered for this tier | unavailable | Landlock reports unrestricted networking; no denial claim or mock-only substitute is accepted. |

## 3. Production implementation evidence

- Expanded the existing Linux containment fixture rather than adding a new
  always-on security harness. Each helper result must report obtained
  enforcement, and every prohibited outside marker remains absent.
- Extended the existing `command_execute` structural event. Raw Bash emits
  actual `ManagedProcessResult` filesystem enforcement, explicit unrestricted
  network, exit code, output-truncation status, cleanup status, and fixed helper
  identity when that helper ran. Timeout paths identify that no completed
  enforcement result was returned.
- Permission receipt ID/source/revision, requested profile, route, advisory
  risk, and command digest remain bounded metadata. Typed Git keeps its own
  `git_operation` owner, avoiding duplicate command records.
- Corrected one stale policy assertion that expected shell substitution
  syntax alone to trigger escalation. The test now verifies terminal payload
  composition while preserving the M001 advisory classification contract.
- Updated `architecture/audit.md`, `architecture/security.md`,
  `architecture/permission.md`, and the `permission` skill. Added the host and
  effect qualification record at
  `docs/validation/command-authorization-execution-boundaries-m003.md`.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib security
cargo test -p codegg --lib permission
cargo test -p codegg --test sandbox_containment -- --test-threads=1
cargo test -p codegg --test approval_router -- --test-threads=1
cargo test -p codegg --test identity_m002_live_audit_hooks -- --test-threads=1
cargo test -p codegg --test identity_m002_live_audit_hooks bash_constrained_audit_reports_obtained_landlock_and_unrestricted_network -- --exact --test-threads=1
python3 scripts/check_policy_surface.py
python3 scripts/check_sandbox_contract.py
python3 scripts/check_authorization_matrix.py
python3 scripts/check_approval_router.py
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_audit_coverage.py
cargo check -p codegg --lib --target x86_64-pc-windows-gnu --locked
cargo check -p codegg --lib --target x86_64-apple-darwin --locked
CC_x86_64_apple_darwin=clang CFLAGS_x86_64_apple_darwin="-target x86_64-apple-darwin" cargo check -p codegg --lib --target x86_64-apple-darwin --locked
scripts/verify.sh quick
git diff --check
```

### Results

- Security-focused library tests: 484 passed.
- Permission-focused library tests: 124 passed after updating the stale
  syntax-only expectation.
- Linux containment: 14 passed; approval router: 17 passed; live audit hooks:
  19 passed. The exact constrained audit test was rerun after asserting output
  truncation metadata and passed.
- All listed static guards passed. `scripts/verify.sh quick` passed, including
  workspace/all-target checking. `git diff --check` passed.
- Host: `Linux deadpool 6.8.0-146-generic`, `x86_64`, Rust `1.99.0`, Landlock
  ABI 4. Verification was local; no hosted CI result is claimed.

## 5. Invariant and limitation review

- Permission syntax/risk classification does not create kernel authority.
  Explicit policy and accepted capabilities remain the authorization source.
- The Linux effect test demonstrates filesystem denial for the specific tested
  child operations only. It does not infer network, IPC, process, pre-opened
  descriptor, or all-syscall guarantees.
- Audit values are bounded labels and digests. Output content is excluded;
  output and cleanup are summarized as status fields. Route provenance does
  not claim to capture every nested process or syscall.
- macOS Seatbelt behavior was not exercised here. The macOS compile is the
  single named condition preventing strict closure; the failure is an
  unavailable host SDK/sysroot, not a Rust source diagnostic.

## 6. Downstream audit

No later implementation plan is registered in
`plans/implementation/command-authorization-and-execution-boundaries/` beyond
M003 and the already closed M001 corrective plans. No plan was unblocked by
this conditional closure. The separate Linux execution-isolation roadmap
remains independently gated for network/process containment and contained Git
workflows.
