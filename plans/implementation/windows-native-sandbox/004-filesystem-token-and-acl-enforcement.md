# Windows Native Sandbox Milestone 004 — Filesystem Token and ACL Enforcement

Status: active

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

Ship a native Windows enforcement mechanism that can **prove** both read restriction of confidential data and write restriction to approved roots for `ReadOnly` / `WorkspaceWrite`. Implement token/ACL lifecycle and fail closed if only partial write restriction is attainable.

## 2. Why this milestone is ready / dependency gate

Hard: M001–M003 strict closures. The Win32 launcher exists with suspended process creation and Job ownership; Windows runtime roots and credentials are classified. A non-admin native fixture runner is available.

## 3. Current implementation evidence

`src/security/sandbox/backend.rs` ships no Windows backend; `SandboxConfig::launch_spec` carries requested read/write/denied paths; `SandboxLaunchOutcome::Enforced` conveys backend guarantees/limits. Microsoft `CreateRestrictedToken(WRITE_RESTRICTED)` only applies restricting SIDs to write access, so that mechanism by itself cannot satisfy Codegg's sensitive read restrictions. Windows root user-profile ACLs are not necessarily deny-first.

M004 WP-A feasibility evidence is now available in
`research/windows-native-sandbox/appcontainer-access-proof.md`. A disposable
Windows Server 2025 AppContainer fixture demonstrated the package-SID DACL
intersection on new runner-temp directories: an explicitly granted read and
scratch write succeeded, while a synthetic secret read and outside write were
denied. This validates AppContainer as a candidate mechanism only. It does not
implement or exercise CodeGG's production launcher, existing-path ACL leases,
credential/named-pipe denial, crash recovery, or the required adversarial ACL
VM matrix. Follow-up run `38155073513` also passed exact before-image
restoration for disposable ACL
fixtures after protecting their inherited ACL bases. This proves restoration
for those isolated test roots only; it does not establish crash recovery for
production ACL leases. Run `38156232045`'s AppContainer proof step additionally
passed suspended creation, Job assignment, `IsProcessInJob` verification, and
resume ordering. Run `38158022356` additionally passed a fixture in which an
AppContainer root started a long-lived child, the child remained in the owner
Job after root exit, and `TerminateJobObject` stopped it. The full Windows
workflow passed in 14m 22s, including the existing launcher suite (8 passed),
suspended-target failure injection, and selected shell/policy tests. These
fixtures still do not prove production launcher integration or production
descendant behavior. No Windows sandbox backend is registered and constrained
launches remain unavailable.

Run `38162151380` also passed a Rust-side, test-only AppContainer creation
proof. It queried the token-information buffer size before reading
`TokenAppContainerSid`, matched the child token to the disposable profile SID,
assigned the suspended target to its Job, verified `IsProcessInJob`, and only
then resumed the target. The full Windows workflow passed in 14m 14s, including
all 8 managed-process launcher tests, failure injection, filesystem/ACL
fixtures, and selected shell/policy tests. This still does not connect
AppContainer creation or ACL leases to production `ManagedProcessService`.

Run `38164279173` strengthened that test evidence. The disposable fixture
proved the ordinary runner principal could read the synthetic secret that its
AppContainer child was denied. The Rust test now checks Job membership through
the same assignment/resume helper used by the managed-process path, inside a
callback that runs after assignment and before `ResumeThread`. The full Windows
workflow succeeded in 9m 59s: all 8 launcher tests, failure injection, the
AppContainer token/Job proof, the filesystem/ACL fixture, and selected
shell/policy tests passed. It remains a test-only launch proof; production
AppContainer launch, ACL lease/recovery, credential/IPC denial, and the
adversarial VM matrix are unimplemented.

The evidence-record follow-up `38165081918` passed the same Windows fixture
and selected suite in 11m 17s. Its serial follow-up `38166050591` also passed
in 13m 06s, including the launcher suite, suspended-target failure injection,
Rust AppContainer/Job proof, and selected shell/policy checks. These reruns
confirm the recorded evidence state; they do not change the production
implementation gaps above.

Code inspection also confirms that the current process-service seam is not
yet an AppContainer launch path: `prepare_launch_argv` rejects every required
sandbox request on non-Unix, and `ManagedProcessService` still launches through
`tokio::process::Command`. `src/managed_process/windows.rs` now owns reusable
AppContainer profile, `STARTUPINFOEXW` attribute-list, and suspended
`CreateProcessW` helpers; the Rust token/Job proof exercises those helpers.
This is launch groundwork only. Production work must connect the helpers to the
process-service path, preserve inherited output handling, and establish ACLs
before resume while verifying token and Job facts. Because approved roots may
be existing directories, ACE cleanup also needs a crash-recoverable ownership
record; an in-memory guard alone cannot satisfy WP-C after daemon/helper loss.

## 4. Invariants that must not regress

- No backend registered available until read AND write requirements can be demonstrated on target Windows version.
- Ordinary OS user permissions still bound the sandbox; no grants beyond parent principal.
- ReadOnly forbids persistent filesystem writes, including public temp, except explicitly qualified nonpersistent devices/outputs.
- WorkspaceWrite can modify only workspace/approved scratch, not secrets or sibling worktrees.
- Permission/hard-deny and Job ownership unchanged.

## 5. Scope

### In scope

- Evaluate and implement restricted token with mandatory restricting SIDs, carefully scoped NTFS DACLs, and if needed an alternative low-privilege/dedicated principal or AppContainer to satisfy read restrictions.
- Create sandbox-specific temporary locations and ACL leases with deterministic cleanup, no clobbering existing ACEs or inheritable grants.
- Implement Windows backend probe/apply-via-launch with observed enforcement and precise limits.

### Explicitly out of scope

- Windows Firewall/network containment; long-lived daemon local-account management unless technically required and approved as a separate ADR; arbitrary Windows filesystem-wide re-ACL operations; claims based solely on generated policies.

## 6. Required production changes

### Backend/Win32
Implement `src/security/sandbox/windows.rs` or similar with safe handle wrappers around `CreateRestrictedToken`/`CreateProcessAsUserW`, `GetTokenInformation` and NTFS security descriptors. If a full restricting-SID token makes common signed/system binary loads impossible, document candidate strategy and *prove* an alternative before claiming enforced. `WRITE_RESTRICTED` alone may be a usable **write-only partial mode**, not a `SandboxProfile` implementation.

### Filesystem/ACL ownership
Calculate approved roots and minimal system runtime read set. Establish a sandbox principal whose DACL grants are limited to allowed operations; verify existing ACEs, integrity rules, privileges and inherited group permissions. Record exact ACEs added and remove only owned ACEs. Deny read of Codegg credential store, SSH/AWS/key material and other deny roots; prefer absence of read permission over global arbitrary deny ACE mutations.

### Privilege/control-plane separation
Any privileged install/provision action requires out-of-band explicit operator consent and separate lifecycle; no model command elevates itself. Restricted children must not communicate with privileged Codegg named pipes or inherit handles that bypass token checks.

### Provenance/observability
Only report `filesystem=Enforced` after setup plus runtime behavioral probe matching both requested rights. Network remains `Unrestricted` if no enforcement. Include OS build, backend identity, handle/token/job facts and limits in status.

## 7. Ordered work packages

### WP-A — Threat model + Windows access-check proof
Executable fixtures examine restricted token, target file DACL, read/write access decisions. Record whether non-admin per-run SID strategy can satisfy denied reads; do not assume.

### WP-B — Restrict and launch
Compile request into principal/token + ACL rights; launch suspended with Job, fail before resume on token/DACL/privilege errors. Return obtained-enforcement evidence only when complete.

### WP-C — ACL lease and recovery
Make changes minimal/idempotent, atomic where possible, uniquely owned and audited. Roll back after normal completion, cancellation, process crash and interrupted setup. Prove preexisting ACLs unchanged.

### WP-D — Security adversarial matrix
Run actual child and grandchild attempts against outside files, protected secrets, sibling worktree, public writable dirs, junctions/reparse/UNC/ADS/hardlinks, privileged IPC and inherited handles. Denied attempts are essential evidence.

### WP-E — Backend registration and capability probe
Register Windows backend only when the real host can establish required read/write restrictions for supported profile. Otherwise return `Unavailable`; a write-only backend may report narrower telemetry but must not masquerade as complete.

## 8. Failure, cancellation, restart, and contention semantics

Prepare identity/ACL lease before resume; complete cleanup on any failure. Abort start if security setup fails or target breaks Job membership. Maintain handle lifetime until exit, then restore owned ACLs; cancellation/timeout kills all descendants before cleanup. Recovery enumerates only Codegg-owned recorded ACL/identity leases with identity verification. Concurrent worktrees have distinct restricting SIDs/owned access rules. Never edit unrelated host permissions to heal stale state.

## 9. Compatibility and migration

Existing Unix unchanged. The user-selected FullHost profile intentionally bypasses Codegg filesystem restrictions but still respects OS authority. If a Windows profile cannot meet read+write criteria, preserve unavailability and require explicit authority escalation. No new DB schema unless necessary to recover ACL leases; if durable lease state is required, version and test schema and store it in daemon-owned protected storage.

## 10. Required tests

- Windows non-admin hosted test: token check, read/write inside/outside, denied credentials, public temp, second workspace, Job descendant propagation.
- Privileged isolated VM: ACL install/revert, power-interruption/kill, malformed ACL, existing deny ACE, reparse race, name collision, hardlinks/ADS and daemon pipe access.
- Static: Windows registration never unconditional; token incomplete never `Enforced`.
- Cross-host regression: Landlock/Seatbelt observed tests, no altered Unix policy contract.

## 11. Required verification commands

`cargo test -p codegg --lib security::sandbox`
`python3 scripts/check_sandbox_contract.py`
`python3 scripts/check_execution_ownership.py`
`cargo fmt --all -- --check`
On Windows: `cargo test -p codegg --test windows_sandbox_containment -- --nocapture` (new target)
On provisioned VM: `cargo test -p codegg --test windows_acl_recovery -- --nocapture` (new target)
`bash scripts/verify.sh quick` on Unix compatible host.

## 12. Documentation updates

`architecture/security.md` (explicit version/profile/limits), `architecture/permission.md`, `architecture/testing.md`, installation/provisioning instructions if required; registry and closure.

## 13. Acceptance criteria

- Under ReadOnly, child cannot persistently write any tested host path.
- Under WorkspaceWrite, target/grandchild writes only authorized roots and cannot read denied secret paths.
- Failed, partial or unavailable backend does not start target under constrained authority.
- ACL rollback verified against exact before/after security descriptors, and backend capability truth supported by live observations.

## 14. Stop conditions

STOP if Windows token/ACL mechanism cannot enforce denied reads and workspace restrictions simultaneously, if a common inheritable group ACE defeats restrictions, if helper/Job allows an unsupervised child, or if cleanup requires modifying uncontrolled host ACLs. Produce measured findings and a new ADR/corrective rather than publishing a false security guarantee.

## 15. Closure evidence required

Observed child filesystem probe matrix with Windows build, token SID/access check and ACL evidence, cleanup/repair traces, credential/named-pipe denial, failure-injection results, supported host matrix, Unix regressions, named gaps and exact unblocking decision for M006.

The available qualification environment currently provides only a hosted
Windows runner. The privileged isolated-VM ACL recovery and adversarial matrix
has not been run, so M004 remains active and dependent milestones remain
blocked.

## 16. Handoff notes

Relevant references: https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken and https://learn.microsoft.com/en-us/windows/win32/secauthz/restricted-tokens. Using `WRITE_RESTRICTED` as the whole backend is specifically prohibited.
