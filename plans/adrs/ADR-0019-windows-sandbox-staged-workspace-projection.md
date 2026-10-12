# ADR-0019: Windows Sandbox Existing-Data Isolation Strategy

Status: accepted (bounded staged-projection **feasibility direction only**; production mechanism not accepted)

Date: 2026-10-12

Decision owners: project maintainers

Related specification sections:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`

Affected subsystem roadmap:
- `plans/subsystems/windows-native-sandbox-roadmap.md`

Related ADRs:
- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`
- `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`

Research record: `research/windows-native-sandbox/adr-0019-successor-assessment.md`. Decision baseline: `d00f1274c844bde7055559f240c020ffb2f1ff46`.

## Context

C011 native Windows workflows `38217933472` and `38220136385` demonstrated a restricted child reading synthetic credentials through the direct path, a junction and a hardlink. C012 blocked populated deny roots before spawn but had an unqualified path-replacement race. C013 `plans/closure/windows-native-sandbox/013-status.md` then showed that applying the root directory ACL lease to an existing tree affects a file shared by a workspace hardlink and credential path. It also found that root-only DACL restoration left descendant security descriptor control bits changed. The grant-only fixture could not read an intended workspace file. The root-level lease approach is rejected for populated host trees.

Microsoft documents that an NTFS hardlink shares its target's security descriptor across names, and that AppContainer enforces access using both normal user/group and package/capability SID permissions, subject to compatible host grants. AppContainer is a stronger starting OS boundary than copying files by itself. OpenAI's May 2026 Windows engineering report documents substantial AppContainer compatibility limitations for arbitrary coding tools and the operational complexity of alternative dedicated-user approaches. These are **hypotheses to test on Codegg**, not proof that Codegg's selected environment works.

## Decision drivers

- Protect actual user-owned workspaces and secret files without modifying their NTFS DACLs, SACLs or integrity labels.
- Preserve real `ReadOnly`/`WorkspaceWrite` semantics, explicit approval and per-worktree ceilings without accepting fake `Enforced` status.
- Keep `ManagedProcessService` as canonical process owner and AppContainer token/Job verification before resume.
- Limit user-host executable, DLL, registry, device and IPC access to independently qualified dependencies.
- Account for links, alternate data streams, hardlink aliases, inherited handles and changes between snapshot and promotion.
- Ensure cancellation, restart, freshness, bounded resource use, and explicit output authorization.
- Avoid an expensive unvalidated design by gating work on early native compatibility evidence.

## Options considered

### A. Continue root-level or recursive ACL mutation on original host trees
Rejected for production. C013 shows identity/alias conflicts and descriptor restoration drift; recursive mutation adds unbounded TOCTOU and recovery surface. Keep historical C010/C012 research fixtures test-only. A later revisitation requires new evidence/ADR.

### B. Copy a workspace to a staging directory **without** a native security principal
Rejected as a security boundary. A same-user unrestricted child can still open original host paths regardless of its cwd. It may serve as a pure workflow convenience only and must never satisfy Windows `SandboxProfile`.

### C. Stage an independent workspace projection and run its tools under Codegg's existing AppContainer + Job boundary
**Selected only as a bounded experiment**, sequenced through C014–C018. The trusted daemon-side materializer copies approved bytes to independently allocated files inside a per-run root, with no host hardlinks, reparse points, ADS, ACL/security descriptor or broad inherited grants copied. A distinct AppContainer SID obtains explicit permissions to the projection and a narrowly verified runtime allowlist only. Outputs remain staged until separately authorized, conflict-checked promotion. Do not assume AppContainer can execute Git/Cargo/Python with this narrow policy: C014 must demonstrate a viable toolchain envelope or stop.

### D. Provision a separate low-privilege sandbox Windows user
A contingency, **not selected** by this ADR. It may improve compatibility, and dedicated offline/online identities can support firewall scoping, but setup requires explicit administrative permission, credential lifecycle, ACL grants and a different principal. See `plans/implementation/windows-native-sandbox/005-dedicated-identity-and-offline-network.md`. If C014's AppContainer experiment fails, a **new ADR amendment/decision** compares this option against retaining Windows constrained execution unavailable before committing provisioning work.

### E. Continue fail-closed unavailability
**Retained as the product/runtime default.** Production `SandboxRequest::Required` stays unavailable on Windows until C011/M004 strict security acceptance; no automated escalation to FullHost.

## Decision

1. Accept **only** the staged-projection/AppContainer feasibility direction as the next research and implementation-planning line, with C014 as the sole dependency-ready plan. This does not authorize a production backend, a public protocol change, or general user workspace access.
2. C014 must independently prove (a) a running AppContainer child can perform realistic *minimal* work in a projection and (b) real denied host accesses fail (direct, junction/hardlink/ADS, daemon IPC, inherited handles, known sensitive paths); record the capabilities and read grants actually required. An AppContainer normally has some common OS access. Do not claim blanket host read denial absent measured guarantees.
3. Only following a **strict positive C014 closure** may C015 snapshot materialization, C016 constrained invocation, C017 output reconciliation and C018 adversarial qualification proceed in order. Any impossible host-read boundary, broad unsafe credential/root grant, or unacceptable runtime incompatibility yields a negative stop and reviewed architectural alternative.
4. The snapshot is **not** a live filesystem overlay and its content is not automatically trusted. The materializer operates under explicit original-workspace authorization, rejects sensitive roots, paths and unsupported aliases, and produces an immutable source manifest; the sandbox child receives separate, minimal credentials/environment and no privileged host handles.
5. Staged files must be **new objects** with independent file IDs and a new controlled security descriptor; do not use `CopyFileExW` without independently preventing its documented retention of ADS and security resource attributes. Do not carry NTFS hardlinks or junctions into the projection. Fail closed for unsupported metadata, file count, size or elapsed-time bounds.
6. No sandbox mutation directly writes the real worktree. The daemon presents candidate outputs and applies only explicitly authorized, bounded, identity/freshness-checked changes through the canonical workspace/approval route. No silent overwriting dirty files, broad deletion, traversal or git metadata mutation.
7. The selected AppContainer must be supervised by the existing `ManagedProcessService`. Setup and security evidence precede `ResumeThread`; Job closure precedes cleanup. Network and filesystem obtained-enforcement dimensions remain independently reported. No security-status upgrade until the separate C018/C011 qualification and an explicit production-admission decision.
8. Unix and existing Windows FullHost semantics are unchanged. Never use a projection runner as a fallback for unqualified production constrained requests.
9. The original C011/M004 security gates, native Windows hosted and privileged isolated-VM matrix and release criteria remain intact. No downstream M005/M006 unblocking until M004 is strictly requalified.
10. The additional, unrelated worktree/branch diff is outside this ADR and these implementation plans.

## Experimental execution model and trust boundaries

`Authorized host workspace (trusted daemon reads)` → bounded, handle-verified independent-file materializer → `projection root` and `source manifest` → narrow AppContainer grant + verified runtime environment → suspended restricted process + Job → `staged result inventory` → authorization/freshness/conflict-checked promotion → host workspace.

Trust boundaries:
- Materialization is privileged relative to the restricted child. The materializer may only read within the source workspace's authorized paths and must exclude the request's `deny_paths`/credential data; the child never uses it as a general read proxy.
- The projection root is untrusted after a child runs. A reviewable output manifest cannot contain executable traversal directives, absolute paths, reparse points, ADS, hardlinks to host files or symlink shortcuts.
- Promotion is a separate operation owned by the authorized daemon/workspace service; agent intent and tool stdout are never an authorization receipt.

## Consequences and limitations

### Positive
- Avoids mutating ACLs of populated host workspace descendants.
- Creates independent file objects, breaking original workspace hardlink *identity sharing* for accepted regular-file inputs.
- Provides explicit staging, output-approval and cleanup boundaries.

### Negative
- Larger IO, storage, setup latency and cache duplication than in-place sandboxing; freshness and dev-tool compatibility may limit utility.
- Untrusted bytes or secrets already stored in an authorized workspace cannot be recognized perfectly by filenames. An explicit deny-by-path policy is necessary but not a complete content-based confidentiality guarantee.
- Tools requiring live in-place host edits, arbitrary user profile caches, nonportable junctions, COM/registry features or global project state may be unsupported and should be surfaced as such.

### Deferred
- Dedicated user/network firewall architecture, LPAC adoption, MXC, virtualized filesystem projections, production platform integration, and independent security review.

## Compatibility, state, performance and recovery

No durable identity or public API migration is authorized by accepting this **experimental** direction. Any per-run state must be bounded, versioned, owned by the execution-node workspace, private to the daemon where needed, recovered idempotently after crash and never reused for another authority ceiling. No global ACL or account mutation. Preserve the existing read/write profile and approval vocabulary. Track stage bytes/files/depth, p50/p95 startup and toolchain availability on actual native Windows hosts.

## Verification and stop conditions

- C014: exact-SHA native Windows AppContainer running-child access matrix and minimal Git/Cargo/Python/shell compatibility with no host secret grant. **Hard stop** if a required host read grant exposes credentials or target cannot execute except with excessive capabilities.
- C015: handle-bound regular-file copy, unique file IDs, reject all unsupported reparse/ADS/hardlink cases, bounded snapshot + manifest, read-only originals unchanged.
- C016: restricted process uses staged cwd and only permitted staged/runtime roots; real denied reads/writes and descendants, cancellation, status and no host handles.
- C017: dry-run/review of output manifests, authorized promotion, three-way or hash/identity conflict checks, interrupted atomicity/rollback.
- C018: full adversarial Windows and privileged VM qualification, honest obtained guarantees and additive C011/M004 closure if independently accepted.
- Any remaining high/medium security finding keeps production blocked. Record negative experiments as completed **negative**, not pretend they are successful capabilities.

## Supporting research

- `research/windows-native-sandbox/adr-0019-successor-assessment.md`
- Microsoft AppContainer access model: https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer
- Microsoft hardlink security descriptor: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createhardlinkw
- Microsoft CopyFileExW metadata preservation: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-copyfileexw
- OpenAI Windows sandbox engineering comparison: https://openai.com/index/building-codex-windows-sandbox/

## Supersession

Supersedes the **undecided direction** of the prior proposed ADR-0019. It does not supersede ADR-0016, ADR-0004, the C011/M004 failure records or their strict evidence requirements. This is an experiment-direction decision; an additional, explicitly reviewed production-admission decision is mandatory.
