# Windows Native Sandbox Corrective C011 — M004 Native Security Requalification and Closure

Status: blocked (high-severity existing-file read leak; full privileged matrix outstanding after C012)

Repository baseline: `17ef5e09b46fa364a520c8dc8793ab866020674f`, branch `work/windows-native-sandbox-foundation`, 2026-10-11.

Corrects original implementation plan: `plans/implementation/windows-native-sandbox/004-filesystem-token-and-acl-enforcement.md`.

Original blocked closure evidence: `plans/closure/windows-native-sandbox/004-status.md` (immutable historical finding).

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#m004-corrective-execution-sequence`.

Long-term requirements:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs: `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`; `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`. No new architectural decision is authorized.

Primary class: capability

## 1. Objective

Prove the full requested Windows `ReadOnly`/`WorkspaceWrite` security semantics from real child/grandchild behavior on realistic existing directories, test crash/ACL/credential/IPC denial, then decide whether to register a native backend and strictly close the original blocked M004. Where privileged VM evidence is unavailable, record a blocked result and do not proceed to M005/M006.

## 2. Why this milestone is ready / dependency gate

Hard: C008–C010 closed on hosted native evidence. GitHub's hosted Windows Server 2025 runner passed the disposable privilege probe (`Administrator role=True`, `High integrity token=True`; UAC is enabled), satisfying the operational gate for synthetic fixtures. C010's owner-SID correction passed same-revision Windows workflow `38215171043`. C011 uses fresh runner-owned workspaces and synthetic credentials only; it never reads repository or runner secrets. M004's historical closure stays blocked until the adversarial matrix passes and receives additive review.

## 3. Current implementation evidence and failed predecessor coverage

- M004 closure identifies high-severity unverified AppContainer Rust launch, missing production backend/ACL leases and unavailable privileged VM. C008–C010 are the named corrective owners.
- Historical disposable script proved synthetic read-deny and outside-write denial but **did not** establish daemon IPC protection, runtime credential exclusion, existing host path ACL lease, hardlink/ADS/reparse race or crash restoration.
- M005 depends on strict M004 closure and optional network testing; M006 requires native file security qualification.

This is a corrective of the **blocked M004**, not a claim that `004-status.md` closed or that Windows filesystem sandboxing works. Its security regression tests must capture the evidence that the previous pass failed to obtain.

## 4. Invariants that must not regress

- Advertised filesystem guarantees equal independently observed restricted-child behavior under actual Codegg policy and executable route.
- ReadOnly cannot write persistent host/workspace/temp except explicitly documented, nonpersistent necessary I/O; WorkspaceWrite writes only approved roots and cannot read denied credentials.
- Subagents and sibling projects cannot expand root/authority. FullHost remains explicit, no Yolo fallback.
- Network stays `Unrestricted` absent M005, and Job supervision is never conflated with access control.

## 5. Scope

### In scope

- Add canonical production `SandboxRequest::Required` Windows backend registration only when C008–C010 and exact host probe meet observed guarantee conditions; test backend descriptor and status truth.
- Native real-world security suite on isolated admin VM and non-admin Windows CI; explicit compatibility matrix by OS build, toolchain, filesystem, profile, installed binary and required operator preparation.
- Recover and restore actual ACLs, test daemon named-pipe and sensitive credential denial, breakaway/descendants, resource limits, symlink/junction/ADS/hardlinks and cancellation.
- Write corrected M004 closure record **additively** using C011 and earlier evidence, preserving original blocked `004-status.md` as historical snapshot (e.g. M004-requalification addendum under plans/closure).

### Explicitly out of scope

- Network firewall M005, unrelated desktop/IDE, forcing MXC adoption, weakening unix sandbox, administrative changes to non-test hosts, declaring broad Windows parity based only on hosted fixtures.

## 6. Required production changes

### Production security admission
Register native Windows backend in a Windows-specific launch registry/probe only after it can demonstrate requested read and write guarantees. If backend registry remains apply/exec-only for Unix, use ADR-0016 platform-launch adapter; do not stuff Windows process creation into a deceptive `apply(spec)` callback. Install helper only via trusted sibling path and verify any status channel and executable identity. Set obtained `Enforced` only after successful authenticated setup.

### Adversarial qualification
Run full `ManagedProcessService`+Bash/Git/test/Python/subagent route on isolated Windows VM: allowed and denied reads/writes, protected credentials (including actual Codegg config and SSH/test secrets), sibling worktrees, daemon named pipes, preexisting user/group ACL inheritance, hardlinks, ADS, junction/reparse races, process descendants, cancellation, crash/restart and ACL cleanup.

### Evidence/release decisions
Record every failure, OS/version/privilege, launch flags/token facts, original/updated security descriptor and exact test result. If host VM is unavailable, **leave blocked**, but report passing hosted checks separately. Do not change M005/M006 to ready without accepted M004 requalification.

### Cross-platform docs
Only claim Windows filesystem containment on verified supported hosts; report network unrestricted and unsupported setup distinct from FullHost.

## 7. Ordered work packages

### WP-A — M004 requirements trace
Map each line of original M004 to C008/C009/C010 evidence; enumerate remaining unsupported claims. Freeze threat matrix and supported host versions before running VM security tests.

### WP-B — Native adversarial VM matrix
Exercise preexisting dirs and real executable descendants, daemon IPC, controlled secret fixture and host ACLs; verify read/write denied through actual system calls.

### WP-C — Recovery/negative security
Kill process/daemon during ACL transitions and resumed descendants; ensure Job teardown and exact/owner-safe ACL rollback; inject tampered token, status and missing helper.

### WP-D — Backend/guard/closure
Enable backend only for hosts passing capability probe, run Unix regressions and native CI, document support/limits and write additive accepted M004 requalification closure or a new blocked record with precise failures.

## 8. Failure, cancellation, restart, and contention semantics

If setup returns any partial enforcement, do not resume. Cancellation closes Job and cleans leases after descendants exit. Power loss/restart reconciles only owned records and blocks conflict. Concurrent worktrees/roots remain isolated under separate principal/ACL authority. FullHost and automatic approvals do not broaden a constrained request; persisted 'enforced' status is never reused without host re-probe.

## 9. Compatibility and migration

Guard legacy Windows builds/unsupported file systems with explicit unavailability; do not enable backend for FAT/exFAT, untrusted remote shares or paths without known NTFS DACL semantics unless independently qualified. Version any private status channel; no durable public protocol expansion without ADR. Keep Windows Server 2025 evidence distinct from client Windows 11 and document missing native coverage. Unix helper unchanged.

## 10. Required tests

- Hosted native suite C008–C010 across same revision, plus windows sandbox containment end-to-end.
- Hosted synthetic-root adversarial test `windows_sandbox_adversarial`: direct, junction, hardlink, and ADS credential-read denial; approved workspace read/write; token/Job verification; and exact ACL restoration. This is a bounded C011 slice, not the full matrix or real-user-data qualification.
- Isolated privileged VM: inherited ACL, file read/write adversarial fixtures, real sensitive-path denial, ADS/hardlink/junction/UNC, named-pipe access, descendant process tokens and Job, failure injection, recovery after forced stop.
- Optional Windows 11 smoke for product support matrix, or accurately narrow minimum supported build.
- Unix: Landlock/Seatbelt previously covered paths, workspace quick/static guards, no scheduler bypass.
- Verify without inferring network isolation; M005 separate.

## 11. Required verification commands

Native Windows hosted:

`cargo test --locked -p codegg --test windows_sandbox_containment -- --nocapture` (new target)
`cargo test --locked -p codegg --test windows_restricted_managed_process -- --nocapture`

Provisioned isolated Windows VM:

`cargo test --locked -p codegg --test windows_acl_recovery -- --nocapture` (new target)
`cargo test --locked -p codegg --test windows_sandbox_adversarial -- --nocapture --test-threads=1` (new target)

Unix compatible:

`python3 scripts/check_sandbox_contract.py`
`python3 scripts/check_execution_ownership.py`
`cargo fmt --all -- --check`
`bash scripts/verify.sh quick`

The closure must record exact versions, run IDs, skipped coverage and failures. Do not mark tests passed if the hosted runner has no rights to execute them. New test filenames are prescribed as targets to create, not files assumed to exist today.

## 12. Documentation updates

`architecture/security.md`, `architecture/testing.md`, `architecture/tool.md`, `architecture/process-tool-execution-ownership.md`, Windows security/operator runbook and support/limits; roadmap and registry; additive closure status.

## 13. Acceptance criteria

- On a qualified actual Windows process, ReadOnly and WorkspaceWrite rights match original M004 guarantees, including protected read deny and out-of-root write deny.
- Job descendant and sensitive IPC boundary hold; ACLs exactly or owner-safely restore after cancellation/crash/restart.
- Complete negative matrix and capability statuses recorded; no absent privileged VM test represented as pass.
- Original M004 becomes strictly closed only after independently reviewed **new additive** closure evidence; M005/M006 may then be audited for unblocking.

## 14. Stop conditions

Stop for unrun privileged VM evidence, leaking access to credentials or daemon pipe, broken ACL restoration, unsupported host falsely reporting enforced, symlink/hardlink escape, or high/medium unresolved security finding. Do not manufacture a conditional 'supported' disposition; keep M004 blocked and file narrowly scoped corrective as needed.

## 15. Closure evidence required

Traceability table from all M004 requirements and corrective C008–C010 closures, exact native VM/CI run IDs and host build, positive/negative process traces, static guards/Unix results, ACL rollback, all unresolved severities, signed-off support matrix, M005/M006 unblock audit and additive M004 requalification closure decision.

Do not edit the historical `plans/closure/windows-native-sandbox/004-status.md` to turn its failures into successes. Create `plans/closure/windows-native-sandbox/011-status.md` and make a later M004 requalification/closure decision from aggregated evidence.

## 16. Handoff notes

The GitHub hosted Windows runner meets the disposable privileged environment gate: the workflow observed an Administrator role and high-integrity token (UAC remains enabled). Use the fresh runner-owned workspace and synthetic credential/IPC fixtures for C011; do not access repository or runner secrets. Do not rewrite historical `plans/closure/windows-native-sandbox/004-status.md`. An additive M004 closure may use e.g. `plans/closure/windows-native-sandbox/004-requalification-status.md` to preserve original evidence chronology.

## C013 existing-object and alias corrective handoff (2026-10-12)

This C011 plan remains **blocked** by the genuine existing-file read leak; C012's populated-tree preflight is only a conditional test-only fail-closed mitigation. The next dependency-ready corrective is `plans/implementation/windows-native-sandbox/013-c011-existing-object-access-and-alias-containment.md`. C013 requires an actual file-object DACL/token/alias causal proof, a bounded implementation strategy if feasible, native running-child denial evidence, handle-based admission/race tests and journal-safe rollback. Windows Server 2025 disposable hosted fixtures suffice to begin. A safe **negative** feasibility disposition must keep populated roots unsupported rather than invent recursive ACL assurance. C013 alone never marks C011 or M004 closed and must not enable the production backend.

The C011 full privileged IPC, descendant, restart, realistic-workspace and independent security review gate remains intact. Preserve original `plans/closure/windows-native-sandbox/011-status.md` and `plans/closure/windows-native-sandbox/012-status.md` as historical findings. No unrelated branch diff work is included.
