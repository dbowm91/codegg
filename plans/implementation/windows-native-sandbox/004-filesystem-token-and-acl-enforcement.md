# Windows Native Sandbox Milestone 004 — Filesystem Token and ACL Enforcement

Status: blocked (M003 closure required)

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

## 16. Handoff notes

Relevant references: https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken and https://learn.microsoft.com/en-us/windows/win32/secauthz/restricted-tokens. Using `WRITE_RESTRICTED` as the whole backend is specifically prohibited.
