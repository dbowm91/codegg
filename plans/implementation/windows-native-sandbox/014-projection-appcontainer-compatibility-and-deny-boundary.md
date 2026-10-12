# Windows Native Sandbox Corrective C014 — Projection/AppContainer Runtime Feasibility and OS Denial Proof

Status: active

Repository baseline: `d00f1274c844bde7055559f240c020ffb2f1ff46` (`work/windows-native-sandbox-foundation`), 2026-10-12.

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0019-projection-successor-milestones`

Accepted decision (experiment only): `plans/adrs/ADR-0019-windows-sandbox-staged-workspace-projection.md`.
Research basis: `research/windows-native-sandbox/adr-0019-successor-assessment.md`.
Inherited blocked security gate: `plans/implementation/windows-native-sandbox/011-m004-native-adversarial-qualification.md`; `plans/closure/windows-native-sandbox/011-status.md`; C013 `plans/closure/windows-native-sandbox/013-status.md`.

Long-term requirements:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Other applicable ADRs: `ADR-0004` approval/profile independence; `ADR-0016` platform-owned launches and truthful obtained guarantees.

Primary class: invariant / security feasibility.

## 1. Objective

Establish or reject the **minimal viable** staged-projection security envelope before building a production-shaped materializer. Use the current C009 restricted launcher/Job as a gated test harness with newly created disposable projection roots. Observe real allowed tool actions and denied original-host accesses under the AppContainer token; decide what toolchains can be supported without broad credential grants.

## 2. Readiness and hard gates

Hard predecessors C008–C010 are closed infrastructure, C013 is closed-negative and ADR-0019 selects the bounded experiment. Windows Server 2025 build 26100 has a real hosted runner. **No C015 work** starts before a strict positive C014 evidence gate. No privileged isolated VM is needed to test synthetic host-owned roots, though C018 still requires it.

## 3. Repository evidence and security problem

Existing `src/managed_process/windows.rs` creates an AppContainer child suspended and checks token/Job before resume; `src/managed_process.rs::run_windows_test_restricted` presently uses test-only ACL leases. C013 showed root leases cannot protect populated host trees. AppContainer documentation lists default grants to some OS files/registries/COM; its name is not a proof of zero host access. OpenAI's published Windows engineering report found AppContainer poorly matched to arbitrary developer tooling. C014 must test this rather than assume away the limitation.

## 4. Invariants and boundaries

- Windows production `SandboxRequest::Required` remains **unavailable**; experiment/test-only runs must not return `SandboxExecutionOutcome::Enforced`, update public capability or select FullHost.
- Never mutate real source workspace ACLs, SACLs, integrity labels, original hardlinks, user profile permissions or daemon named-pipe DACLs. No elevation or new account without a separate accepted design.
- `ManagedProcessService` owns restricted process creation, token/Job before resume, all descendants, stdout/stderr and status; authenticated daemon retains authorization and workspace ownership.
- AppContainer package SID, runtime permissions and file projection are *obtained* access facts only when the native tests prove them. No network isolation claimed without separate actual tests.
- Inputs are untrusted when copied into the projection; outputs are untrusted after the child writes them. Neither is a direct authority to modify the host worktree.
- All file paths and resource ownership must be bound to a stable workspace/worktree/job identity; stale paths, unsupported metadata, security conflict or partial setup fail closed.

## 5. Production-code boundaries and intended modules

Implement a self-contained `tests/windows_projection_feasibility.rs` (or named test-only module) that creates fresh standalone projection files, applies AppContainer grants to **only** disposable projection root and a narrowly justified executable/DLL set, then uses the existing suspended-child/Job test seam. Avoid `WindowsAclLease::Deny` and any ACL writes to real source fixture roots. Stage native tool scripts/inputs manually for the proof; no new production caller. Audit restricted inherited handle-list and environment filtering, AppContainer profile-local TEMP/TMP, and effective token/capabilities. Preserve unsupported/error semantics.

No unrelated UI, editor, human shell, distributed daemon, branch-wide merge or protocol refactor is permitted in this plan. Add public protocol/DB fields only after recording an explicit compatibility decision; tests and private types are preferred at this stage.

## 6. Ordered work packages

### WP-A — Minimum policy and negative-root model
Define exact allowed projection/scratch executable roots and the requested filesystem read/write mask, with a separate native OS root list. No blanket `USERPROFILE`, `LOCALAPPDATA`, `ProgramData` or user cache grant. Query actual child token SID/capability set, LPAC availability, Job association and effective ACL on projected fixture.

### WP-B — Native allowed toolchain smoke
Run at least PowerShell or cmd with a staged script, Git reading a staged synthetic repository and representative Rust/Cargo and Python invocations where available. Capture exact unsupported features and required DLL/registry dependencies. If full set impossible, explicitly nominate and justify a limited initial command subset; never automatically grant the original user profile to make Cargo pass.

### WP-C — Actual denied host access probes
Provide separate **unprojected** synthetic host workspace/credential fixtures; restricted process attempts direct read/write, junction/reparse and hardlink routes, ADS, original workspace path, daemon named pipe, inherited parent handles and an ungranted sibling projection. Assert observed denial or explicitly fail/no support. Do not read non-synthetic user secrets. Include child/grandchild and cancellation check.

### WP-D — Acceptance/disposition
Capture p50/p95 warm/cold launch and filesystem/registry access profile. Decide specific supported subset and required read roots. If AppContainer default host reads cannot be constrained to truthful contract or basic tooling is unusable, close negative and propose dedicated identity or narrower product mode via ADR review. C015 must not be unblocked.

## 7. Acceptance criteria

- A real restricted child reads/writes a newly staged file and exits successfully under a verified token and Job; tests fail if it reads an unprojected host credential or writes an original workspace fixture.
- Tested shell and Git (minimum) work using **no broad user-profile read grants**; any claimed additional Cargo/Python runtime is proven, not inferred.
- Host-readable system paths and any unblocked registry/IPC are enumerated as limits. No asserted blanket read-deny or network property.
- Production Windows `Required` remains unavailable and test seam returns `TestOnlyAppContainer` only.
- Either an accepted strict positive subset unblocks C015 or an explicit negative feasibility report stops the projection line.

## 8. Failure, cancellation, recovery, concurrency and stop conditions

Reject if required tools access original user caches with sensitive material, inherited handles remain accessible, another workspace/path is reachable contrary to request, default AppContainer permissions undermine claimed secret boundary, or tool compatibility is untenable without broad host grants. Fail before child resume when setup incomplete; cancellation kills Job. Tests may deliberately observe a default OS accessible path but cannot label it denied. No ACL mutation of original fixtures. If C014 fails, **stop C015–C018** and revisit ADR-0019/isolated identity; avoid endless environment tweaks.

## 9. Verification requirements

Native Windows same SHA:

`cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_projection_feasibility -- --nocapture --test-threads=1` (new test)
`cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_restricted_managed_process -- --nocapture --test-threads=1`
`cargo test --locked -p codegg --lib managed_process::windows::tests::appcontainer_ -- --nocapture`
Run current focused Windows process-launcher workflow plus exact new target on Windows Server 2025 MSVC.

Cross-host: `cargo fmt --all -- --check`; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_execution_ownership.py`; `bash scripts/verify.sh quick`.

Native missing/skipped tests = blocked, not pass. Add a Win32 actual access/Job/IPC matrix and measured policy root list to closure.

All closure records must include exact SHA, runner/OS build, Rust version, workflow job link, **selected nonzero test counts**, actual child stdout/denied operation results, skipped native/privileged coverage and known regressions. A preflight refusal, compilation-only test, model statement or parent-generated status frame never substitutes for an actual running restricted-child security assertion.

## 10. Documentation and migration

Document tested Windows version/toolchains, compatibility ceiling, AppContainer/LPAC result, host read exceptions, network status, latency, and negative tests. No persisted staging state or schema is necessary for C014. Revise ADR-0019 with a separate decision only if the experiment fails or changes architectural authority.

## 11. Closure record and handoff disposition

C014 closes positive only with **both** runnable tool subset and actual denied original-host access, no broad host grant, strict same-SHA native evidence. A compile-only or fail-before-spawn test is insufficient. Gate C015 explicitly; M004/C011 remain blocked.

Write `plans/closure/windows-native-sandbox/014-status.md` with strict positive or negative evidence, separate from historical `004-status.md`, `011-status.md`, `012-status.md`, `013-status.md`. Update the registry and roadmap only on verified results. A negative feasibility result is a valid *closed-negative* plan, not a reason to mark an unsupported production feature ready.
