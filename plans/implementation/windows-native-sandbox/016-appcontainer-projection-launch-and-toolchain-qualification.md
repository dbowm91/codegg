# Windows Native Sandbox Corrective C016 — AppContainer Projection Launch and Toolchain Qualification

Status: blocked (C015 strict positive closure)

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

Primary class: capability feasibility / execution.

## 1. Objective

Run approved staged inputs through the existing suspended AppContainer + Job process lifecycle with the **projection as cwd**, no direct user-workspace read/write grants, correct finite process supervision, and actual observed toolchain/credential boundary under unchanged production admission.

## 2. Readiness and hard gates

Hard: C014 usable/narrow runtime subset and C015 independent snapshot/manifest accepted. No new backend registration. Early independent testing allowed on disposable synthetic fixtures only.

## 3. Repository evidence and security problem

`src/managed_process.rs::run_windows_test_restricted` currently uses disposable root ACL leases; production `Required` remains unregistered. C008/C009 already supply token/Job, stdio, status, cancellation, package-local TEMP and restricted environment. C013 rejects root leases on actual host trees, so C016 must avoid those leases entirely and only grant the fresh projection and explicitly qualified noncredential-bearing runtime resources.

## 4. Invariants and boundaries

- Windows production `SandboxRequest::Required` remains **unavailable**; experiment/test-only runs must not return `SandboxExecutionOutcome::Enforced`, update public capability or select FullHost.
- Never mutate real source workspace ACLs, SACLs, integrity labels, original hardlinks, user profile permissions or daemon named-pipe DACLs. No elevation or new account without a separate accepted design.
- `ManagedProcessService` owns restricted process creation, token/Job before resume, all descendants, stdout/stderr and status; authenticated daemon retains authorization and workspace ownership.
- AppContainer package SID, runtime permissions and file projection are *obtained* access facts only when the native tests prove them. No network isolation claimed without separate actual tests.
- Inputs are untrusted when copied into the projection; outputs are untrusted after the child writes them. Neither is a direct authority to modify the host worktree.
- All file paths and resource ownership must be bound to a stable workspace/worktree/job identity; stale paths, unsupported metadata, security conflict or partial setup fail closed.

## 5. Production-code boundaries and intended modules

Keep private Windows `ProjectionExecutionRequest`, per-run SID + projection ACL, environment whitelist, immutable `ProjectionManifest` provenance and verified stage identity. Use `ManagedProcessService` alone; never launch an unsupervised tool from a projection helper. Windows test-support gated runtime path may carry a precise `ProjectionExperiment` outcome, but must not report real production filesystem `Enforced`.

No unrelated UI, editor, human shell, distributed daemon, branch-wide merge or protocol refactor is permitted in this plan. Add public protocol/DB fields only after recording an explicit compatibility decision; tests and private types are preferred at this stage.

## 6. Ordered work packages

### WP-A — Authorization binding
Take snapshot metadata and immutable authority from daemon request/worktree lease, not caller-supplied arbitrary host path. Validate target executable/args/cwd and required AppContainer-readable DLL/registry/executable roots against C014 evidence. Do not pass `%USERPROFILE%` or broad caches to child. No host-original writable roots or secret-bearing environment variables; sanitize `PATH`, `HOME`, `LOCALAPPDATA`, `TEMP`, `TMP`, toolchain cache and executable search.

### WP-B — Suspend/grant/verify/resume
Create distinct AppContainer per run, prepare projection DACL and package-local scratch, perform SID/token/Job/handle-list and allowed-target path checks before resume. Validate read-only vs workspace-write stage rights and deny original host fixture. No inherited workspace directory handles, arbitrary parent handles, daemon pipe tokens or staged host path aliases. Child descendants inherit restrictions; own them in Job.

### WP-C — Toolchain compatibility
Run precisely C014-qualified shell/Git and optional cargo/rustc/python samples against synthetic staged projects. Capture dynamic DLL/config/cache accesses; unsupported features must report typed explanation and never trigger blanket grants. Handle stdout/stderr, timeouts, cancellation, exit codes, bounded output, concurrent run roots. Measure p50/p95 launch and staging overhead.

### WP-D — Output inventory and teardown
Freeze staged changes after Job descendants stop, produce bounded manifest vs original stage snapshot, validate no escaped paths, deny link/ADS/device nodes. No promotion here. Dispose AppContainer profile and projection on success, failure or recovered stale run only after authorized output inventory/review is durably staged for C017.

## 7. Acceptance criteria

- A *running* restricted tool reads and writes staged material, while the same process and grandchild cannot read an unprojected credential/original host file, write outside the stage, or connect to privileged daemon IPC.
- ReadOnly and WorkspaceWrite stage permissions are actually denied/allowed as requested; network reporting remains truthful (unrestricted unless independently tested).
- No staged tool inherits unauthorized host handle, environment secret or breakaway path.
- Windows production `Required` remains unimplemented/unavailable, and test-only results cannot be confused with enforced filesystem containment.

## 8. Failure, cancellation, recovery, concurrency and stop conditions

Any missing token/Job/handle/DACL prerequisite denies before first target instruction. Cancellation/timeouts kill full Job before staged inventory or cleanup. A target creating a junction/hardlink to an original host path fails or is rejected in output inventory; *originals remain OS-inaccessible regardless of link*. If a toolchain requires dangerous blanket host-grants, stop this milestone and reopen ADR-0019 alternatives. No automatic fallback to non-AppContainer shell.

## 9. Verification requirements

Native Windows: `cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_projection_execution -- --nocapture --test-threads=1` (new); repeat `windows_projection_feasibility` and `windows_projection_snapshot`. Negative matrix direct original file, junction/hardlink/ADS, sensitive env, IPC, descendants and orphan cleanup. Guard `check_execution_ownership.py` and `check_sandbox_contract.py`; fmt and `verify.sh quick` on Unix. Record exact native build and runtime dependency matrix.

All closure records must include exact SHA, runner/OS build, Rust version, workflow job link, **selected nonzero test counts**, actual child stdout/denied operation results, skipped native/privileged coverage and known regressions. A preflight refusal, compilation-only test, model statement or parent-generated status frame never substitutes for an actual running restricted-child security assertion.

## 10. Documentation and migration

Document per-run staging lifecycle, original vs stage path semantics and limitations (not an in-place worktree), environment/toolchain support, retained private outputs, error taxonomy and no obtained production security guarantee. No release toggle or user setting. Projection receipts remain private.

## 11. Closure record and handoff disposition

C016 positive closure advances C017; no user-visible 'sandboxed Windows' until C018 + strict C011/M004 approval. Preserve or explicitly clean test-owned bundles and record all error states.

Write `plans/closure/windows-native-sandbox/016-status.md` with strict positive or negative evidence, separate from historical `004-status.md`, `011-status.md`, `012-status.md`, `013-status.md`. Update the registry and roadmap only on verified results. A negative feasibility result is a valid *closed-negative* plan, not a reason to mark an unsupported production feature ready.
