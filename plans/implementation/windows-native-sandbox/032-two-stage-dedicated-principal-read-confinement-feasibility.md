# Windows Native Sandbox C032 — Two-stage Dedicated-principal Strict Read-confinement Feasibility

Status: **ready for test-only implementation**; operationally gated by disposable administrator-consented Windows fixture for native execution
Primary class: security/infrastructure
Baseline: `work/windows-native-sandbox-foundation` at `c7e2dae404d5b58f6bd927aa8d99b4d518c4f23d` (refresh HEAD before edits).
Roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0022-bounded-successor-feasibility`.
Authority: accepted feasibility-only ADR-0022; ADR-0004, ADR-0016, ADR-0020; ADR-0021 historical no-admission unaffected.
Canonical: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8; `plans/003-planning-process.md`.
Predecessors: C019 actual-child security oracle, C021 first-hop success, C024/C027/C028 negative host-side token-launch evidence; C029 remains closed without admission.

## 1. Objective and exact hypothesis

Prove or falsify that an *unprivileged* trusted runner started as a disposable dedicated account by `CreateProcessWithLogonW` can call `OpenProcessToken(GetCurrentProcess())`, mint a genuine read-restricting token from **its own** primary token, and launch an actually usable child via `CreateProcessAsUserW`. Compare the child's read of an **unchanged broad-group-readable synthetic original file** with the ordinary account's positive read, while allowing bounded native Git + PowerShell/cmd operations in an independently created stage.

This is a new authority topology, not C027 retries. Separate process startup feasibility, effective security restrictions and useful developer-tool compatibility as three *independent* gates. Stronger claims require a separate production ADR, independent review and subsequent plans.

## 2. Scope, owners and exclusions

Allowed: test-only `research/windows-native-sandbox/two-hop-harness/` native helper (small isolated Rust 1.89-compatible crate or C helper with explicit ABI), `scripts/windows_two_hop_feasibility.ps1` disposable-fixture driver, focused native workflow named distinctly from existing launcher CI, and common C019-style probe/report fixtures. No changes to released `ManagedProcessService` launch routing, `SandboxRequest::Required`, backend registry, approvals, daemon IPC, installers, source ACLs, or real accounts. Never add a privileged SCM broker or guest on this plan. Temporary elevated test provisioning is operator-consented and limited to a disposable VM/synthetic account and objects.

A normal GitHub-hosted Windows runner may contribute *diagnostic* evidence if account creation is supported. Do not claim ordinary customer support, independently verified security admission or reboot/recovery from a hosted green job. Use a separately controlled, consented disposable VM for positive security conclusion and adversarial replay.

## 3. Current evidence and reconciliation

`scripts/windows_dedicated_user_feasibility.ps1` (C021) already proves first-hop account creation, `CreateProcessWithLogonW`, suspended-then-Job-owned PowerShell child, Git/Python/Cargo on Windows Server 2025, and *private-DACL* fixture denials. C024's second test establishes a **normal account can read broad-group files**, but attempts to take that account's restricted token from the original process failed to produce working children; C027 `CreateProcessAsUserW` returned Win32 1314, C028 `CreateProcessWithTokenW` children did not reach entry. Codex and Anthropic implement a two-stage intermediary under sandbox account identity. Microsoft's `CreateProcessAsUserW` provides a restricted-*caller*-token exception to `SE_ASSIGNPRIMARYTOKEN_NAME`, but not blanket privilege/compatibility success.

## 4. Security invariants

- First-hop process must run as *new* standard sandbox SID, not original user/admin. Inner child uses a **restricted form of this caller's own primary token**, not a foreign token. Query actual child `TokenUser`, `TokenType`, `TokenGroups`, `TokenRestrictedSids`, privileges, elevation and integrity; report errors and missing information explicitly.
- Read-confinement experiment MUST NOT set `WRITE_RESTRICTED`; disabling writes is a different test. A unique restrictive SID must grant only the *new* stage and narrowly observed necessary runtime objects. Do not add `Everyone`/`Users`/`Authenticated Users`/user-profile grants to the restriction list as a shortcut. No host DACL/SACL/MIC/owner/control/ADS/links mutation.
- First-hop runner image must be trusted, identified by immutable absolute path + hash and inaccessible to the sandbox account for modification. No arbitrary agent command, privileged tool, host handles, credential-bearing environment, broad PATHEXT/PATH executable search or shared mutable runner manifest.
- Controller owns one bounded Job lifetime for runner and descendants (or documented, verified nested ownership). Spawn suspended; verify token, handle inheritance, Job membership, no breakaway and Job-handle exclusion before resume. Stage/status/IPC cannot become command injection or elevated authority.
- Production Windows constrained mode stays unavailable, even if a test-only probe passes.

## 5. Ordered work packages

**A — Freeze threat model and synthetic fixture.** Enumerate broad-read `Users`/`Authenticated Users`/`Everyone` object, private-DACL negative control, stage root, fresh SID-granted DLL/tool/runtime prerequisites, junction/hardlink/ADS/case/extended/UNC variants, sibling file, operator-only named pipe, secret handle. Snapshot file IDs, link count, security descriptor hashes/control bits, MIC, hashes. Build a required first-hop read **positive** on the unchanged broad-readable same file and a private-file denial. If the positive read fails, flag fixture-invalid rather than success.

**B — Build two-hop topology and startup gate.** Test driver creates random owned disposable standard account; invokes immutable first-hop helper via `CreateProcessWithLogonW` with suspended startup and guaranteed owned Job before resume. Runner opens its own primary token, calls `CreateRestrictedToken` with a non-`WRITE_RESTRICTED` limiting SID, then `CreateProcessAsUserW` using fixed absolute helper executable, mutable UTF-16 command line, explicit narrow unicode environment, cwd/profile/session/desktop handling and restricted startup handles. Capture API, privilege presence (including `SE_INCREASE_QUOTA_NAME`), last Win32 error immediately, actual suspend count, process entry marker/exit and bounded timeout. Do **not** fall back to a host-side token launch, administrator-enabled right or `CreateProcessWithLogonW` of the final **unrestricted** tool and call it qualified. If the minimal restricted child cannot reach user entry, close blocked/negative with localized reason. No repeated parameter roulette.

**C — Strict read boundary gate.** Child must reach entry and provide actual token proof. Repeat the normal dedicated-account baseline and restricted-child attempt against the **same unchanged, broad-readable file** with actual `CreateFileW` read/read-data, not mere metadata or parent-side `AccessCheck`. Restricted child MUST deny while fresh stage read/write and minimum PowerShell/cmd + Git init/status succeed. Copy system/tool binaries only into an independently created synthetic stage when necessary, never import security descriptors or links from original objects or modify system ACLs. Record executable/DLL/registry/device dependencies and explicit exceptions. An allowed forbidden read, inability to run minimum developer tools or host grants beyond policy closes negative.

**D — Escape, cleanup and reproducibility gate.** Exercise C019-style direct, sibling, junction, hardlink, ADS, case, extended-path, create/write/move/delete, IPC/pipe, inherited secret handle and descendant probes. Distinguish unsupported UNC/test cases as `not_run`; include Job-breakaway, handle duplication/surrogate-process and runner manifest substitution attempts on synthetic fixtures. Forced failure after stage creation and before resume, timeouts, cancellation, child crash and abrupt runner exit must leave no live descendants. Observe Job ownership before deleting only own stage/account; never delete a foreign reused name/SID. Record startup p50/p95 on bounded repetitions if time permits but do not relax gates.

**E — Exact evidence and handoff.** Machine-readable report with test SHA, OS edition/build/UBR/KB, runner/child token groups and restricting SIDs, first-hop/second-hop API error/entry timing, same-file before/after IDs/DACL hashes, per-operation expected/observed and `pass/fail/not_run/unavailable`, shell/Git results, root/account cleanup, Job descendants and workflow permalink. Separate green *evidence collection* from positive security disposition. Add closure `plans/closure/windows-native-sandbox/032-status.md`, roadmap/registry status and independent review referral.

## 6. Verification and qualification

Native driver: `pwsh -NoProfile -File scripts/windows_two_hop_feasibility.ps1` in disposable administrator-consented Windows VM (if created). Focused Windows tests: invoke any new harness directly and run `cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_native_probe_contract -- --nocapture --test-threads=1`, plus Windows launcher integration suites if their common helpers changed. A new workflow must be **explicitly test-only**, separate from production launcher CI, record metadata and actually run nonzero cases. Compare hosted Server 2025 and consented Windows 11 where available; do not generalize from one image.

Cross-host: `git diff --check`; `cargo fmt --all -- --check` (if Rust changed); `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_execution_ownership.py`; `bash scripts/verify.sh quick`. Do not report a check as passed if not run.

## 7. Concurrency, restart and security limits

The feasibility pass tests one owned account/session at a time; true multi-worktree concurrency, reboot recovery, credential rotation/provisioning interruption, installed launcher signing, and network/WFP are *future* qualification gates, not implied by `finally` teardown. Document these as required for any proposal in C034. Treat privileges, HKCU/profile, loader, desktop/session, inherited handles, Win32 object namespaces and surrogate spawn as attack surfaces. No general product launch or persisted credential storage is introduced.

## 8. Acceptance and stop conditions

Strict-positive **bounded feasibility** only if: normal dedicated SID **reads** broad-group original; first-hop runner executes as that SID; inner child has true restricting SID token and joins the owned Job before resume; child **denies** same original read while stage read/write and Git + one native shell work; alias/IPC/handle/descendant probes are meaningful and correct; source metadata unchanged; cleanup passes. A pending/failed startup or a private-only denial cannot count. Network remains unknown.

Fail/close negative if true child reads forbidden file, runs with wrong SID, cannot execute minimum tools without unsafe grants, needs unsanctioned elevation, or lifecycle escapes. Close *blocked* for ineligible test host/operator consent. A positive result **does not** authorize production backend, C025/C026 rewriting, C011/M004–M006 re-entry, or `Enforced` status. No successor implementation except C034's comparison until a new explicit ADR.
