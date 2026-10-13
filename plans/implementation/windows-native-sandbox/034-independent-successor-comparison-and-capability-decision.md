# Windows Native Sandbox C034 — Independent Successor Comparison and Capability Decision

Status: **blocked** — depends on written C032 and C033 dispositions (positive, negative or blocked)
Primary class: security invariant / architecture
Baseline: `work/windows-native-sandbox-foundation` at `c7e2dae404d5b58f6bd927aa8d99b4d518c4f23d`; refresh evidence SHA at execution.
Roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0022-bounded-successor-feasibility`.
Authority: ADR-0022 feasibility, ADR-0004/0016/0019/0020; ADR-0021 rejection intact.
Canonical: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8; `plans/003-planning-process.md`.

## 1. Objective

Compare independent C032 two-stage unprivileged runner and C033 MXC on the same threat/compatibility criteria, document a **no-admission**, blocked, or *future production architecture proposal* disposition. Do not select a winner based on a green workflow when the actual security probe was unavailable. A candidate may be strict-positive in bounded synthetic testing but still remain unqualified for release.

## 2. Hard readiness and authority

C019/C021/C024–C029 remain immutable background evidence. C032 and C033 must each have an exact-SHA closure with explicit outcome; a supported host being absent still counts as `blocked`, not positive. If neither is strict positive, C034 may close **no-admission** without running independent positive security review. If proposing a future route, require a separate independent Windows security reviewer to examine actual child logs, token/capability enforcement, negative probes, trust boundaries and host-state integrity, and document unresolved high findings. Production launch is *never* authorized by C034 itself; propose a **new separately accepted** ADR and implementation/requalification plans afterward.

## 3. Ordered work packages

A. Freeze identity of C032 and C033 revisions, Windows edition/build/UBR, controlled VM source and account privilege, SDK/runner provenance. Build a common result table of baseline reads, restricted same-file broad-read denial, stage read/write, host writes, Git+shell, Python script, native helper, Cargo, direct/alias/hardlink/ADS/UNC, IPC/handles, descendants, restart, concurrency, networking, user-host metadata integrity, and performance. Mark **pass**, **fail**, **not_run**, **blocked** separately.

B. Independently review threat coverage. Trace original user credentials, token normal/restricting SID intersections, system and user DLLs, PATH/library lookup, HKCU/registry, process-spawn surrogate paths, handles, pipes, devices, network IPv4/IPv6/loopback and untrusted output promotion. Distinguish positive private-DACL isolation from true strict read policy. Unsupported user-host read, inherited handle or toolchain coverage remains a no-admission blocker even if other operations pass.

C. Evaluate backend policy reporting: `filesystem_read`, `filesystem_write`, `credential_read`, `network`, `process_tree` and tool-compatibility observed guarantees. If only write confinement works, document an optional separate **future** security-profile/API decision without implementing, registering or silently broadening existing `ReadOnly` or `WorkspaceWrite`. Missing requested guarantees continue to return unavailable/fail-closed.

D. Assess lifecycle and operations for a future backend: consent, helper signing/hash, account creation/rotation/credential handling, offline WFP if ever claimed, per-worktree concurrency, Job ownership, session-restart/reboot/force kill, deny-trace redaction, telemetry, packaging, Windows Home/11/Server matrices and cleanup rollback. Do not treat any C032/C033 bounded `finally` teardown as production recovery qualification.

E. Record reviewed recommendation: (1) strict-positive **candidate proposal only** with new independent production ADR needed; (2) no admissible candidate; or (3) insufficient operational host/evidence. Create `research/windows-native-sandbox/c034-independent-comparison.md` and `plans/closure/windows-native-sandbox/034-status.md`; update registry and roadmap without rewriting C020/C021/C024–C029 closure evidence.

## 4. Security, compatibility, migration

This is documentation/review only. No binary, runtime, schema, public protocol, installer, user security mode, sandbox backend registration, setup service, account, firewall rule, root ACL or host workspace write may change. All candidates preserve production Windows `SandboxRequest::Required` unavailable and no filesystem/network `Enforced`. If a future ADR admits a qualified mechanism, its separate production plan must bind it to the canonical `ManagedProcessService`, approvals, immutable policy snapshot and independent C011/M004 requalification.

## 5. Verification and acceptance

Validate C032 and C033 closure documents against actual workflow/run IDs and SHA, selected nonzero native tests and machine-readable reports. Verify provenance and no test outcome was relabeled. Run `git diff --check`, `python3 scripts/check_sandbox_contract.py`, `python3 scripts/check_execution_ownership.py`, `bash scripts/verify.sh quick` for planning changes when available and disclose any not run. An independent reviewer must explicitly sign off on a positive recommendation and high findings must be closed before recommending production planning.

**Acceptance:** complete, candid comparison with exact evidence; true distinction of write vs read vs network vs process containment; explicit no-admission/blocked/proposal decision; no launch-authority policy or historical status regression. **Stop:** missing C032/C033 disposition, contradictory file IDs/evidence, actual secret leak, unreviewed positive claim, unavailable security host represented as pass, or any requested production release in this plan.
