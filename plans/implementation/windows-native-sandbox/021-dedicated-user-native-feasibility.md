# Windows Native Sandbox C021 — Dedicated Windows Principal Native Feasibility

Status: active (C019 shared oracle accepted; disposable GitHub-hosted Windows VM authorized)
Date: 2026-10-12
Primary class: infrastructure / security feasibility
Objective: Independently prove or reject a separately provisioned Windows principal with narrow staging access, useful native developer tools, real denied host reads/writes and controlled process lifecycle.

**Authority:** ADR-0016 (cross-platform launch authority), ADR-0019 (historical experimental record), ADR-0020 (accepted feasibility comparison), `plans/subsystems/windows-native-sandbox-roadmap.md`, and `plans/003-planning-process.md`.

**Baseline:** `b1024ba719c627f8a294cb50170d48ec08ce62f4` on `work/windows-native-sandbox-foundation` (2026-10-12), refresh HEAD before implementation.

**Security invariant for every plan:** Production Windows `SandboxRequest::Required` stays unavailable; no production backend registration, no `SandboxExecutionOutcome::Enforced`, no silent FullHost downgrade, no network confinement assertion without packet/socket evidence. Existing Linux/macOS backends, canonical `ManagedProcessService` ownership, identity/worktree ceilings and approval/audit remain unchanged. Fixture-only behavior is not release behavior. Preserve unrelated branch changes and keep Windows sandbox scope independent from the other diff.

**Closure convention:** report exact code SHA, Windows edition/build/installed KB, host privilege, target tool versions, child token/identity/backend, selected nonzero test count, logs proving *running-child* allowed and prohibited operations, skipped tests and reasons, provenance of isolated VM evidence, and before/after host security descriptors where applicable. A missing native/privileged host is **blocked**, an observed incompatibility or privilege escape is **closed negative**, and a passing compile or preflight is never positive security evidence. Close with `plans/closure/windows-native-sandbox/021-status.md`; update roadmap and registry on actual evidence only.


Long-term references: `plans/000-long-term-specification.md` (sections 4, 17, 27 and 29) and `plans/001-terminology-and-domain-model.md` (section 8).

## Evidence and reference architecture

Codex's Windows sandbox documents isolated offline/online local accounts, distinct elevated setup and unprivileged runner, restricted `WRITE_RESTRICTED` child tokens and identity-scoped network policies. Anthropic `sandbox-runtime` Windows alpha uses `srt-sandbox`, DPAPI/OS credentials, per-session ACEs, and SID-bound WFP egress. Neither pattern automatically enforces Codegg's denied host *reads*. C011/C013 specifically prohibit reintroducing in-place populated host-tree ACL leases. References: https://openai.com/index/building-codex-windows-sandbox/ ; https://github.com/anthropics/sandbox-runtime/blob/main/README.md .

## Experiment boundaries

Administrator provisioning is permitted **only on a disposable isolated Windows test VM after explicit operator action**. Use local account with a distinct SID, no administrator/group privileges, private HKCU/HOME/TEMP and no access grants to the operator's credential caches, original profile or original workspace. Test separate bootstrap/run/teardown commands, all out of the agent tool path. Prefer per-run **new independent staging objects** with narrow DACLs and no original workspace hardlinks/reparse/ADS, manual small fixtures before any C015 materializer. No persistent account or firewall in a normal Codegg install. Do not implement production profile/user onboarding, public UI or separate scheduler.

## Ordered work packages

1. **Design setup protocol:** account SID/name ownership token, ephemeral random credential managed only by the test host, no text secret in logs/repo, failure-injected idempotent create/verify/revoke. Record exact privilege and Windows edition requirements. Prevent adoption or deletion of colliding preexisting accounts. Separate setup command from ordinary agent execution.
2. **Runner under principal:** test `CreateProcessWithLogonW` or equivalent documented logon bootstrap, then obtain/restrict *that principal's* primary token and launch suspended; verify TokenUser, privileges, restricted SIDs and integrity, Job assignment before `ResumeThread`. Compare ordinary sandbox user, `WRITE_RESTRICTED` token (write-only implications) and fully restricting SID token (extra read ACL requirements). Never assume restricted SID implies host read-deny.
3. **Toolchain and scope:** new private writable projection/scratch and minimal known runtime executables/DLLs, no original user caches. Independently run Git init/status, PowerShell/cmd, Python and Cargo bounded commands. Record tool availability by installation type and intentionally unsupported commands, including MSVC paths if exercised.
4. **C019 actual-host denial:** test distinct original host secret/workspace/sibling objects accessible to the *operator*, direct and hardlink/junction/ADS paths, IPC, preopened/inherited handles, file creation/move/delete outside stage, and child/grandchild. A dedicated account is inadequate if `Authenticated Users`/`Everyone` access still exposes declared secrets. Explicitly measure this and fail positive read-isolation gate if bypassed.
5. **Operation and recovery:** two simultaneous workspaces, cancellation and Job closed-before-teardown, orphan recovery, missing credential, stale SID/reuse, interrupted provisioning, ACL descriptor hash before/after, selective deletion of only test-owned resources, no agent admin escalation. Network capability separately **unclaimed**; optional WFP/firewall scoping only in isolated diagnostic, cannot be labeled offline absent full socket/DNS/loopback/bypass coverage.
6. **Disposition:** compare exact read and write enforcement vs C020, consumed dependencies and install burden. Close positive, negative or blocked after one bounded investigation; don't build production promotion/installer.

## Strict acceptance / negative stops

Positive feasibility = actual verified different principal + Job, no source host-object ACL alteration, Git + native shell working, all forbidden direct/alias original-host read/write probes denied, no ambient operator handle/profile/IPC leak, rollback and crash handling proven. Python/Cargo accepted individually only if observed. Positive is **not** production, network confinement or per-worktree output promotion.

If ordinary Windows ACLs expose a protected host object to the separate identity, either prove a narrowly scoped **non-mutating** restriction or close candidate negative for required read denial. Do not respond with recursively mutating original host DACLs or silently loosening Codegg's read guarantees. If no eligible admin VM or consent, record blocked, not fail/pass. Never ship plaintext account passwords or permit arbitrary agent-supplied helper arguments to select elevated setup.

## Verification / documents

Test-only `windows_dedicated_identity_feasibility` and C019 shared matrix on an isolated Windows VM; exact selected-test counts, token dumps (redacted), child output, network status unclaimed, security descriptor and host resource before/after state. Cross-host `cargo fmt --all -- --check`, sandbox/execution-owner guards, `git diff --check`, `bash scripts/verify.sh quick`. Record Windows edition/build, account creation/removal, group membership, failed setup/rollback, disposal and per-command logs. No public API, storage, installer, dependency/MSRV changes.

## Closure

Write `plans/closure/windows-native-sandbox/021-status.md`. Strict positive or closed-negative enables factual C022 comparison, not production execution. If chosen in a later ADR, split M005's account-foundation portion into an M004 prerequisite; leave firewall/offline networking separately gated after filesystem admission.
