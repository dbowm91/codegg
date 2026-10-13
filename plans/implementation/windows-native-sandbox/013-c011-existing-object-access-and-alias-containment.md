# Windows Native Sandbox Corrective C013 — Existing-Object Access and Alias Containment

Status: implemented (negative feasibility; successor deferred pending ADR review)

Repository baseline: `265146ac451a74c1c8b41acdc50df993c38e6b9a` on `work/windows-native-sandbox-foundation` (2026-10-12).

Source corrective: `plans/implementation/windows-native-sandbox/011-m004-native-adversarial-qualification.md`, still **blocked**.

Source failed evidence: `plans/closure/windows-native-sandbox/011-status.md`; source fail-closed mitigation: `plans/implementation/windows-native-sandbox/012-existing-tree-deny-lease-correction.md` and `plans/closure/windows-native-sandbox/012-status.md` (conditionally closed).

Affected milestone: `plans/implementation/windows-native-sandbox/004-filesystem-token-and-acl-enforcement.md`. Historical M004 and C011 failed/blocked closure reports are retained as originally observed.

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#m004-corrective-execution-sequence`.

Long-term requirements:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs: `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md` and `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`. Any new host-wide provisioning, broad ACL mutation, or semantics change requires its own reviewed ADR before implementation.

Primary class: security invariant / native enforcement capability corrective.

## 1. Objective and immediate decision

Replace the C012 "reject any populated deny root" stopgap with a **measured and defensible path** to protecting existing Windows file objects, or demonstrate precisely why the current AppContainer-plus-ACL model cannot deliver that guarantee safely and preserve a fail-closed unsupported disposition.

A successful proof must exercise a genuinely **running** restricted child against existing populated synthetic files, not merely reject the launch. The implementation must not assume that changing a directory DACL changes effective access on a preexisting descendant, or that pathname denial is sufficient when hardlinks refer to the same underlying file.

The result of C013 is (a) a small native causal proof and, **only if qualified**, (b) a bounded test-support enforcement correction with explicit supported-object limitations. C013 does **not** close C011/M004 or enable production Windows `SandboxRequest::Required` by itself.

## 2. Why ready / existing evidence

C008–C010 have strict or scoped closure. C012's same-revision Windows Server 2025 workflow `38223080719` established fail-before-spawn rejection of populated deny roots and working **empty-root** test fixtures, but did not explain or fix the original leak. The elevated hosted runner can run synthetic NTFS DACL and AppContainer tests; an isolated reusable privileged VM remains the gate for full C011 recovery/IPC/real-workspace qualification, not for this bounded corrective.

Original C011 workflows `38217933472` and `38220136385` executed a child which read a synthetic credential **directly, through a junction, and through a hardlink**; the alternate data stream was denied. Corrected C012 tests return `SandboxFailed` before that child launches. This leaves the security boundary unproven for real populated denied directories (e.g. `.ssh`).

Relevant implementation evidence:

- `src/managed_process/windows_acl_lease.rs::add_appcontainer_ace` uses `SetEntriesInAclW` with an inheritable `DENY_ACCESS` on the **directory DACL**; `set_dacl` calls pathname-based `SetNamedSecurityInfoW`. The lease journal captures a root DACL but does not journal any child file's descriptor or control bits.
- `src/managed_process/windows_acl_lease.rs::validate_empty_deny_directory` and `src/managed_process.rs::run_windows_test_restricted` use path-based preflight, then canonicalization, then a separate open/mutation/launch window. They do not keep identities pinned continuously.
- `tests/windows_sandbox_adversarial.rs` now only asserts pre-spawn rejection; it no longer supplies a running-child regression that demonstrates denial on a populated tree.
- Path-keyed root coalescing and directory-level roots do not identify aliases by (volume, file ID). An NTFS hardlink has no independent security descriptor: permissions attach to its underlying file. A path-level "allowed here, denied there" rule for the **same file ID** cannot be implemented by giving its hardlinks different ACLs.

Windows may automatically propagate inheritable ACEs, but protected/explicit descendant DACLs, inheritance control bits, reparse traversal, link aliases, and access-check order can change the outcome. No single root-cause explanation is accepted without a **captured effective descendant DACL and observed child access check**.

## 3. Architectural/security invariants

1. No `ReadOnly` / `WorkspaceWrite` security claim or backend registration until C011's **entire** security matrix is accepted; keep `windows-sandbox-test-support` test-only and production Required requests unavailable.
2. No host-wide ACL rewrite, taking ownership of arbitrary host data, removing existing grants, implicit elevation, or mutation of actual user/runner credentials. Work exclusively with disposable runner-owned NTFS fixtures and synthetic secrets.
3. Child and descendants are suspended/Job-controlled until **all** required per-object access state and lease prerequisites are established; no direct, junction, hardlink, ADS, handle, or alternate namespace route to denied content may be left reachable under a claimed guarantee.
4. Native ACL protection is object-based (volume and file identity, descriptor/control bits), not merely pathname prefix. Unresolvable conflicts between allowed and denied aliases **fail closed before resume**; do not try to allow two incompatible DACLs for one file object.
5. The before-image and journal semantics of **every mutated object**, including DACL control/inheritance metadata and link identity, must be recovered or conflict-preserved on interruption. Root-only before/after equality is insufficient when the operation can propagate to children.
6. Unknown filesystem types, UNC/device paths, symlinks/reparse points, protected or inaccessible descendants, unsupported ADS/hardlink cases and concurrent mutations produce typed unavailable/error with no success claim. No silent FullHost or `DegradedUncontained` escalation.
7. Windows network status remains unrestricted, and Unix Landlock/Seatbelt and approval/authority semantics are unchanged.

## 4. Scope boundaries

In scope:

- Native hosted forensic access-check/ACL inheritance diagnostic against **existing populated synthetic** files before and after parent ACE changes.
- Handle-based object identity, alias classification and scoped admission/transaction design.
- One bounded implementation strategy whose actual security property is supported by the evidence, plus failure-injection tests and explicit unsupported cases.
- C011 dependency/qualification reconciliation and a clear choice of next technical mechanism if native per-object DACL management is impractical.

Out of scope:

- Broad recursive ACL surgery on a real user profile, silently deleting/replacing hardlinks, global firewall/network M005, production backend activation, MXC adoption, and review/merge of unrelated changes on this branch.
- Treating a running test as safe solely because a parent directory had an inherited ACE, because a static `AccessCheck` predicted denial, or because C012 rejected the request.

## 5. Ordered work packages and mandatory gates

### WP-A — Isolate and explain the C011 failure (hard gate, first)

Construct a **separate forensic fixture/test** in `research/windows-native-sandbox/` or in a `windows-sandbox-test-support` native test module. It must bypass C012's populated-root preflight **only in this isolated test harness**, never the production/service admission path, and run under an ephemeral AppContainer identity with test-owned roots.

For an existing synthetic credential, capture and report for both parent and file:

- normalized Win32 path; volume serial + file ID; hardlink count and aliases; reparse tags when relevant;
- DACL owner, inherited/explicit ACEs in effective order; `SE_DACL_PROTECTED` / `SE_DACL_AUTO_INHERITED` and inheritance flags; SID/capability memberships of the actual child token;
- parent/descendant security descriptor **before and after** the existing `SetNamedSecurityInfoW` call; whether child DACL or control bits changed; result of direct `CreateFileW` read by the restricted child;
- matching results for protected (inheritance-disabled) existing child, inherited child, child created **after** setup, junction, hardlink and ADS cases.

Use synthetic secret bytes only. Baseline reproduction may produce an *expected known failure* in a clearly labelled diagnostic run; a separate security-asserting test must fail if leakage persists. Do not mark the security correction successful based on tests that simply observe it.

**Gate A acceptance:** a reproducible, object-level reason for the direct read leak or an explicit measured uncertainty with the precise discriminating next test. Do not modify production admission to "fix" an unproven cause. Limit to one concise root-cause report and at most one targeted second diagnosis; if still unresolved, stop and record blocked evidence instead of repeating speculative flag changes.

### WP-B — Evaluate enforceable policies (hard decision gate)

Compare these strategies on the identical test files/token, with a matrix of guarantees, costs and side effects:

1. Keep AppContainer's independent access check with **grant-only** access to the intended read/write roots and *no ACL mutation of denied trees*, using an actual effective-token read test and rejecting preexisting grants/aliases that violate the ceiling.
2. A **bounded explicit per-file/object deny lease** on disposable test-owned objects, with handle-bound security calls, a complete object-by-object journal, protected DACL controls and cleanup; hardlinks deduplicate by file ID. Do not assume an inheritable root ACE suffices.
3. If the first two cannot safely meet non-empty existing-tree requirements without racing arbitrary writers, a **staged workspace projection / isolated identity** that does not grant access to host denied data, or keep the entire configuration unavailable and specify the next ADR decision.

**Hardlink rule:** since all hardlinks share the file's security descriptor, a workspace path and denied credential path resolving to the same file ID must either both be effectively denied or the request must be rejected; if this destroys a required workspace guarantee, fail closed. Do not "fix" by changing only one hardlink path's DACL.

**Gate B acceptance:** choose exactly one bounded test implementation supported by observed evidence, or document a negative feasibility verdict with a specific future architecture decision. No more than one approved candidate enters implementation in this plan.

### WP-C — Test-only object admission and journal correction

Only after WP-A/B gates pass, implement the selected minimal strategy inside the existing Windows test-support adapter. In any per-object lease approach:

- validate real NTFS volume and opened directory/file handles using `FILE_FLAG_OPEN_REPARSE_POINT` and `GetFileInformationByHandle(Ex)`; record file ID, link count, descriptor and root ownership; reject junction/reparse/path aliases that cannot be closed or pinned;
- pin the exact object through validation, mutation and launch or provide a demonstrated equally strong revalidation/locking mechanism; use handle-bound `GetSecurityInfo`/`SetSecurityInfo` where appropriate; path-only `validate_empty_deny_directory` is insufficient for trusted admission;
- journal **all potentially affected files**, not just root DACL bytes. If OS inheritance propagation mutates descendants, enumerate and record those descendants before mutation and prove coverage or reject the tree. Respect protected/explicit DACLs and third-party edits; no resetting unrelated ACEs;
- handle hardlinks by file ID and refuse conflicts with allowed paths/outside roots. Preserve preexisting security descriptors/control bits exactly or quarantine drift. Never grant extra rights from parent or token defaults;
- bind any scan to a bounded object count, depth, bytes, elapsed time and allowed volume(s). Exceeding limits is typed unavailable, not best-effort partial enforcement.

**Gate C acceptance:** on a populated *disposable* denied tree where the chosen strategy is supported, a **running** restricted child must be denied direct, junction, hardlink, ADS and equivalent file-open access while allowed read/write behavior remains valid. If a fixture is intentionally unsupported, the target must not start and no ACL bytes/control bits may change. Keep production feature disabled.

### WP-D — Races, rollback and concrete security regressions

Extend `tests/windows_sandbox_adversarial.rs`, `tests/windows_acl_lease.rs`, `tests/windows_restricted_managed_process.rs` and native Windows workflow as necessary. Use separate fixtures so expected failure and actually supported policy aren't conflated.

Required:

- concurrent directory rename/replacement **between** identity check and ACL mutation; populate an empty denied root after preflight; switch a descendant to a junction/reparse point; introduce/repoint a hardlink/alias; rename an allowed or denied ancestor;
- failure injection after each journal write, per-object ACL mutation, Job assignment, and before resume; test cancellation, timeout, normal finish, simulated parent crash and subsequent `recover_all`;
- simultaneous worktrees with overlapping/nested roots, hardlink pointing to denied object from allowed workspace, descriptor drift, protected DACL, unknown filesystem, unreadable files and deliberately excessive tree;
- verify permitted accesses and **denied file opens on the actual AppContainer token/descendant**, not just status JSON. Verify no child marker when unsupported and exact per-object before/after security descriptor metadata when any lease was applied;
- verify `RunOnly/ReadOnly` and `WorkspaceWrite` semantics through existing policy requests as feasible. In particular, no false `Enforced` result for the test seam.

**Gate D acceptance:** deterministic native Windows run at one pinned commit demonstrates successful read-denial *or a deliberate fail-closed unsupported disposition* for each case, exact lease restoration/conflict retention, and no escaped/unsupervised descendant. Any demonstrated credential read during a claimed supported path fails the gate and preserves C011/M004 blocked.

### WP-E — Evidence, stop/go and C011 linkage

Document measured root cause, mechanism choice, supported/unsupported matrix, host build and principal, access-check traces, file-ID/DACL evidence, fixture timestamps, workflow run IDs, before/after descriptors and CI results. Record a bounded **C013** closure at `plans/closure/windows-native-sandbox/013-status.md`:

- **Closed (test-only)** if WP-A–D accepted, including a genuinely running child denied access to supported populated objects;
- **Closed-negative / deferred** if a documented impossible/unsafe strategy decision is reached and unsupported cases fail closed, with an ADR proposal scoped to the blocker;
- **Blocked** if a real leak, restoration failure or race remains uncontained.

C011 stays **blocked** until its independent privilege/IPC/descendant/restart/real-workspace matrix and explicit security review are accepted. M004 stays blocked; M005/M006 stay blocked. Do not alter historical `004-status.md`, `011-status.md` or `012-status.md` to replace failing evidence.

## 6. Windows-specific technical references and cautions

- Microsoft `SetEntriesInAcl` permits new deny ACEs and orders them on the **ACL being modified**; it does not prove a particular descendant's effective access: https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setentriesinaclw .
- Microsoft documents automatic propagation of inheritable ACEs *subject to inheritance/security descriptor semantics*: https://learn.microsoft.com/en-us/windows/win32/secauthz/automatic-propagation-of-inheritable-aces .
- Microsoft documents that hardlinks share the **same file security descriptor**, so per-path hardlink-specific ACLs are impossible: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createhardlinkw .
- Win32 reparse point opening and handle identity must be checked on actual handles, not canonical string representations: https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew .
- AppContainer access is a token/ACL intersection with possible capability/package grants. The only conclusive boundary evidence is actual restricted-child behavior for the advertised operation.

## 7. Verification commands and exact evidence policy

Required on Windows Server 2025 x86_64 MSVC, on the same SHA:

`cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_sandbox_adversarial -- --nocapture --test-threads=1`
`cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_acl_lease -- --nocapture --test-threads=1`
`cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_restricted_managed_process -- --nocapture --test-threads=1`
`cargo test --locked -p codegg --features windows-sandbox-test-support --lib windows_acl_lease -- --nocapture --test-threads=1`

Add a specifically named C013 forensic/positive native test target or filter; record its **exact** command and nonzero selected test count in the closure. Run existing process-launcher, failure-injection and token/Job tests on the same SHA. Use fixture-owned synthetic credentials only. The hosted Windows runner's observed Administrator/high-integrity token is sufficient for the diagnostic and disposable-tree work; it does **not** certify arbitrary user/daemon IPC rights or VM recovery.

Cross-host before closure:

`cargo fmt --all -- --check`
`git diff --check`
`python3 scripts/check_sandbox_contract.py`
`python3 scripts/check_execution_ownership.py`
`bash scripts/verify.sh quick`

Test command availability must be checked before claiming a run. If Windows-native test results are unavailable, retain `ready/blocked` status; cross-compilation and green Unix quick checks are not substitute evidence.

## 8. Acceptance criteria / stop conditions

C013 test-only **positive closure** requires all of the following:

1. The original populated-tree direct read cause is explained by actual existing-file DACL/control/token evidence (not inferred from root ACL) and the remedy is backed by actual file-open denial in a running AppContainer child.
2. Junction/reparse, hardlink and ADS paths either demonstrably cannot read the synthetic denied object when launched under a supported contract **or cause pre-resume refusal** when conflicting/unsupported; the original workspace read/write positive cases remain valid.
3. Race simulation prevents mutation/launch under replaced identities and no concurrent actor can convert a checked-safe state into a silently less restrictive launched state within the claimed support envelope.
4. No unowned ACLs are changed, per-object descriptor/control before-images are restored or retained as explicit conflict on all injected failure paths, and Job/descendant teardown precedes release.
5. Neither production Windows backend registration nor security status changes to `Enforced`, and missing or partial security setup never silently falls back to FullHost.
6. Test and documentation evidence is pinned to exact commits, Windows version, token identity and observable results; no scope shrink is presented as successful denial on an existing tree.

**Immediate stop** if hardlink aliases demand contradictory file permissions, if preventing directory mutations requires host-global locking/elevation, if descendant ACL inheritance cannot be recovered safely, if a running child reads a denied credential in a purported supported case, or if any test causes non-test host ACL modification. Keep the current C012 fail-closed preflight and document a new narrowly scoped design/ADR decision rather than inventing a passing security claim.

## 9. Storage, concurrency, compatibility and downstream

Follow C010's journal ownership, daemon-root storage, bounded recovery and owner/file-ID checks; extend to per-object state only after WP-B gate and with explicit journal version migration/recovery. Never assume C010's root-only record can restore a propagated descendant. Run startup recovery or refuse admission if conflicts; no user action should delete journal bytes blindly. Distinguish safe restoration of owned ACEs from overwriting an administrator's later policy.

Keep `ManagedProcessService` as the only runtime owner, retain `ApprovalRouter` and subagent ceilings, preserve Linux/macOS behavior, do not alter session/network policy, and keep Windows `Required` unavailable in production. C013 only unblocks C011's **existing-object/root-cause** slice; it cannot independently unblock M004, M005 or M006. A full native Windows security matrix remains separately required.

## 10. Handoff checklist

- C011 existing-file evidence preserved; C012 conditional limitation remains visible.
- WP-A diagnostic proves or precisely isolates parent-vs-file effective ACL and access-check behavior.
- WP-B selects one defensible mechanism and records reasons alternatives were rejected.
- WP-C implements bounded test-only policy or issues a clear negative feasibility report; no unsound "best effort" broad ACL traversal.
- WP-D actual token/child, ancestor/junction/hardlink, races, recovery and Job controls verified.
- WP-E C013 closure and registry/roadmap update based on obtained native evidence, not narrative status.
- No unrelated branch diff review or rebase/merge work in this plan.

## 11. Final disposition (2026-10-12)

C013 is complete as a **negative feasibility/defer result**, not as a populated-tree enforcement capability. Exact-SHA hosted evidence is recorded in `plans/closure/windows-native-sandbox/013-status.md` and `research/windows-native-sandbox/c013-existing-object-findings.md` (commit `6028e03f`, workflow `38296752862`). Root-level ACL leasing changed the hardlink-backed credential object and left descendant DACL control-bit drift after restoration; the actual child also could not read the intended allowed workspace file in the forensic grant-only fixture. No bounded, race-safe per-object journal strategy was qualified. The normal test-support admission path rejects populated lease roots before mutation or child creation.

The future staged-projection/isolated-identity architecture question is scoped as proposed ADR-0019. It is not accepted and authorizes no implementation. C011, M004, M005 and M006 remain blocked. Production Windows `SandboxRequest::Required` remains unavailable, and no Windows filesystem `Enforced` claim is made. No plan is unblocked by this disposition.
