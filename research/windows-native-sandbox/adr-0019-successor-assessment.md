# ADR-0019 Research — Windows Staged Workspace Versus Dedicated Identity

Date: 2026-10-12

Codegg baseline: `d00f1274c844bde7055559f240c020ffb2f1ff46` on `work/windows-native-sandbox-foundation`.

Research outcome: **Select a bounded staged-workspace + AppContainer feasibility experiment; preserve production fail-closed behavior.** Do not claim that projection alone isolates the host; rely on observed Windows token/DACL limits and only advertise supported operations after native testing.

## Evidence already in Codegg

- C008 `plans/closure/windows-native-sandbox/008-status.md`: repaired `SECURITY_CAPABILITIES` pointer lifetime; Rust AppContainer token/Job pre-resume and failure cleanup verified on Windows Server 2025.
- C009 `plans/closure/windows-native-sandbox/009-status.md`: test-only AppContainer child integrated with `ManagedProcessService`, bounded stdio, cancellation and validated environment.
- C010 `plans/closure/windows-native-sandbox/010-status.md`: owner-bound, durable **directory ACL** lease/recovery on disposable test roots only.
- C011 `plans/closure/windows-native-sandbox/011-status.md`: hosted Windows running child read synthetic credential by direct, junction and hardlink paths, failing `deny_paths`.
- C012 `plans/closure/windows-native-sandbox/012-status.md`: rejects populated deny roots before test-child spawn; path replacement remains unqualified.
- C013 `research/windows-native-sandbox/c013-existing-object-findings.md` / `plans/closure/windows-native-sandbox/013-status.md`: hardlink alias shares NTFS file ID and security descriptor; root ACL lease changed that object, descriptor control restoration drifted from `0x8004` to `0x8404`, and grant-only fixture denied intended workspace read. Closed negative for existing-tree root leases.

A process that runs as the **same unrestricted user** on a different copied cwd can still access originals: staging is a data-freshness/host-ACL-nonmutation tool, not a security mechanism. This is why the successor retains AppContainer access checks and Job Objects.

## Current repository integration boundaries

- `src/managed_process.rs::run_inner` is the canonical finite-process owner; an AppContainer `Required` test seam is compiled only with `windows-sandbox-test-support`. `SandboxExecutionOutcome::TestOnlyAppContainer` must not be treated as obtained `Enforced`.
- `src/managed_process/windows.rs` owns restricted token launch, handles and Job association. No production Windows filesystem backend is registered.
- `src/managed_process/windows_acl_lease.rs` root ACL transactions do not safely apply to populated host trees; do not reuse them against the source workspace.
- `src/security/sandbox/policy.rs` produces read/write/deny roots, but its Windows path checks are string/path admission rather than a filesystem open-operation boundary.
- `src/scheduler/executors.rs` / workspace/worktree services already own project/worktree permission ceilings and contention. The projection must be workspace-ID-scoped rather than treating an arbitrary physical path as a durable project identity.
- `src/git_service.rs` and `crates/egggit` have Git operations, but they do not currently furnish an approved sandbox-output promotion transaction. Do not use 'git apply' as a silent, privileged shortcut to authorization.

## Microsoft and ecosystem research

### AppContainer
Microsoft's AppContainer overview describes package-SID/capability-SID access as an intersection with normal user/group rights, with low-integrity and OS-provided resource allowances. It also explicitly notes that ordinary AppContainers can access certain system files/registries/COM by default; LPAC tightens some defaults. **Therefore one cannot promise blanket host read denial just because an AppContainer SID is used.** Need actual negative probes on real host paths and pre-opened/inherited handles. Reference: https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer

### Hardlinks
NTFS hardlinks are distinct directory names pointing to one file object and **one security descriptor**. A workspace hardlink and credential link cannot have independently enforced ACLs. Materializing as new regular-file bytes can remove shared identity **only if** the source is accepted, copied into a newly created file, with independent verified file ID, and no hardlinks/junctions carried into the projection. A source hardlink may already point to sensitive content outside the allowed tree; fail closed for `link_count>1` in first phase rather than copy it. Reference: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createhardlinkw

### Copying APIs and metadata
Microsoft `CopyFileExW` explicitly preserves NTFS alternate data streams, extended attributes, and security resource attributes. Do **not** use it as an unexamined 'safe clone'. The first materializer uses handles to read the unnamed primary stream of approved regular files and freshly creates destination objects with explicit package-scoped DACL. Reject nondefault ADS, EFS/sparse/reparse/device types, unexpected metadata, encrypted or offline/cloud placeholders, directories with suspect tags and other unsupported source objects. Reference: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-copyfileexw

### In-place native Windows coding agents
OpenAI's 2026 engineering report documents how AppContainer's strict capabilities conflict with arbitrary developer tools (shells, Git, Python/package managers). Its later approach uses separate, optionally online/offline Windows local principals and restricted tokens plus an elevated setup/command runner. That approach is **not automatically available** to Codegg: it entails account credential management, administrative provisioning, host ACL changes, and runner privilege considerations, and may be appropriate only if the projection/AppContainer experiment fails. Reference: https://openai.com/index/building-codex-windows-sandbox/

### Filesystem identity, junctions, time-of-check races
Win32 `CreateFileW(FILE_FLAG_OPEN_REPARSE_POINT)` and handle inspection can establish the identity of a selected opened file/reparse point; canonical strings alone cannot guarantee a later path refers to the same object. Holds/locks and revalidation must cover source materialization and output promotion; reject unsupported replacement/alias states. Reference: https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew

## Strategy comparison

| Dimension | Root ACL leases | Staged copy alone | Staged copy + AppContainer | Dedicated local user |
|---|---|---|---|---|
| Mutates original workspace descriptor | yes (unsafe, C013) | no | no | potentially yes (grant setup) |
| Underlying OS read restriction | attempted, failed C011 | no | AppContainer package/capability DACL check; must verify all paths | ordinary identity ACL restriction; must verify |
| Existing hardlink aliases | shared object causes conflict | copying can break identity; source secrets risk | copying + link rejection/identity check; must verify | shared object still matters if host paths granted |
| Supports native toolchain | original host paths available but security failed | yes, but unconfined | uncertain; hard C014 feasibility gate | more likely but higher privilege/setup |
| Handles live workspace changes | naturally, but dangerous ACL changes | snapshot stale | requires rebase/conflict handling | in-place, but concurrent host edits still matter |
| Administrator setup requirement | may | none for copy itself | not necessarily on hosted fixture; must prove | yes for account/firewall setup |
| Network guarantee | none | none | none claimed; verify separately | only with independently qualified firewall/identity |
| Time-to-safe experimental evidence | rejected | not secure | moderate, early native negative/positive gates | substantial provisioning work |

## Recommended bounded execution sequence

1. **C014** feasibility and AppContainer/LPAC runtime access audit. On real Windows native runner, project a trivial new-file source root into an AppContainer-exclusive fixture without modifying real workspace ACL; require successful shell/Git and at least targeted Cargo/Python smoke *or an explicit narrow supported subset*, while probing forbidden actual host file/IPC access. A strict nonzero compatibility baseline is required before proceeding.
2. **C015** implement independent-file, manifest-driven, bounded and race-checked projection materializer. Reject source link count >1, reparse points, ADS, excluded/sensitive paths and unsupported object states. No `CopyFileExW` to silently inherit ADS.
3. **C016** run native tools within the projection using existing `ManagedProcessService`/AppContainer/Job, with no general original-workspace rights or ambient credential env/handles. Reject unsupported shell features with typed errors.
4. **C017** inventory staged outputs and gate promotion through the original workspace's authoritative per-worktree lease and explicit approval, with source snapshot hash/file-ID/version checks, traversal/alias protection and staged rollback.
5. **C018** native adversarial qualification, privileged isolated VM and independent C011 security re-review; production backend still requires a separate admission decision and M004 strict requalification.

## Negative-stop triggers

- The real child can read a protected original host credential, host workspace file not projected, daemon named pipe or parent handle in a case claimed isolated.
- Native required toolchains need a broad allow over the real user profile or credential-bearing caches. Unsupported toolchains are preferable to silently widening root grants.
- Materializer cannot exclude unexpected link/stream/metadata or produces files retaining original host object identity.
- Promotion can overwrite a modified host file, follow an output junction/ADS, or run without workspace authority.
- Crash/restart leaves a privileged grant, accessible projection, uncontrolled descendant or partial host mutation.
- A green forensic/compile check is mislabeled as a production security acceptance.

## Outcome and conditions

Adopt ADR-0019 as a **bounded research direction**, not as production sandbox approval. C014 is ready; C015–C018 remain hard dependency-blocked. The older C010/C012 fixtures and C013 negative finding are retained. If C014 fails, require a new documented choice (separately provisioned identity vs narrower support vs continued unavailability) rather than a chain of speculative retries.

The broader unrelated branch diff will be handled independently and is intentionally not part of this assessment.
