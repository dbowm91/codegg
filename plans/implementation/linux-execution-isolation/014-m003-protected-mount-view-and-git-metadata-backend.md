# Linux Execution Isolation C006 — M003 Protected Mount View and Git Metadata Integration

Status: **blocked** — C004 ADR identity reconciliation, explicit acceptance of canonical Linux ADR, qualifying C005 native launcher evidence and M002 internal contract
Class: capability / security invariant
Closure target: `plans/closure/linux-execution-isolation/014-status.md`.
Baseline: main @ `ec43e495c3619966ac33d0acccf7904812dd37bc` (2026-10-13 EDT). Long-term: `plans/000-long-term-specification.md` §§4/17/27/29, `plans/001-terminology-and-domain-model.md` §8, and `plans/003-planning-process.md`. Execution authority: ADR-0001/0004/0016; command classification: currently accepted `plans/adrs/ADR-0028-command-syntax-and-execution-authority.md` (canonical ID to be reconciled under C004). Existing Landlock ABI >=3 filesystem-only support must remain intact. This is a handoff plan, not a claim of implementation or test execution.

## 1. Objective

Make M003's protected writable-workspace view implementable on the *actual* trusted Linux launch path, without circularly waiting for Git C002 (which depends on M003). Preserve Landlock-only defaults until a separately approved opt-in feature is qualified. The new concrete namespace backend must truthfully enforce read roots, writable leased workspace/scratch and protected descendants such as Git executable-control metadata and synthetic credentials.

Parent authority: `plans/implementation/linux-execution-isolation/003-protected-workspace-mounts.md`. This corrective strengthens its launcher/backend and Git transactional interface; it is not an alternative M003 that can be closed by mocks.

## 2. Required design and exact boundaries

- Compile mount policy from immutable `LinuxLaunchRequest` and canonical worktree lease into a **capability-checked mount graph**: narrow read-only runtime/toolchain binds; writable workspace and private scratch; readonly/hidden protected descendants; minimal `/dev`, *never broad host `/proc`, `/run` or `/sys` read-through* while claiming confidential isolation.
- Resolve the actual workspace via dirfd/canonical path, validate symlink/hardlink/mount aliases and nested existing or future paths. Include linked worktree `.git` pointer, gitdir/common-dir, external linked worktree metadata and submodule cases. A writable parent does not negate child protection, even if Landlock alone cannot express it.
- Agent Bash and other untrusted children must not change Git hooks/config/filters/fsmonitor or agent executable control files; Codegg-owned typed Git service may later mutate index/refs through separate **lease-scoped non-executable metadata transaction**. C001 trusted Git policy is closed; Git C002 executable local operations remain blocked until mounted/process/offline restrictions are genuinely obtained.
- Preserve parent privilege ceiling; `ReadOnly` cannot gain writes, `WorkspaceWrite` does not silently gain host reads, and `FullHost` remains separately explicit. Network isolation, seccomp, PID/IPC and cgroups are separate obtained dimensions; never mark them enforced from a successful mount alone.
- Launch adapter must own helper executable identity, private status channel, setup/exec ordering, namespace PID lifetime, stdout/stderr/PTY, cancellation and exact obtained enforcement. No fallback to unrestricted Bash or trusted Git when required mount setup fails.

## 3. Implementation sequence

A. After C005 qualifying evidence and canonical ADR acceptance, implement mount-manifest compiler with typed allowed read/write/protected paths and denial on unsupported aliases, malformed spec, symlink race and nested path ambiguities. Keep the legacy Landlock path untouched.

B. Integrate Bubblewrap child setup under `ManagedProcessService` as a distinct selected backend. Keep private status obtained only after actual mount graph assembly, descriptor hygiene and privilege checks. Reject helper substitution, setuid and fake success.

C. Construct readonly nested overlays for protected existing and future metadata; pin resolved inode identity or refuse race-prone mounts. Test linked worktrees, Git common-dir out of workspace, `.git` file redirects, hardlink alias, credential symlinks and concurrent workspaces.

D. Define but **do not auto-activate** a small typed Git metadata write lease for C002 (index/refs transitions only under Codegg owner); pre-verify that a malformed/malicious `.git/config` cannot inject code into a higher-authority trusted operation. Keep filters/hooks executability inside actually confined children only after M004/M005.

E. Validate ordinary shell, `cargo fmt/check/test`, rustup, Python, `git status`, LSP/test-runner invocation compatibility without broad read/root binds or workspace escape. Measure setup latency; document supported Linux kernels/LSMs and no-userns refusal.

## 4. Verification and security matrix

Native positive/negative fixtures: allowed source write, denied nested secret read/write (existing and future path), external workspace read denied, hooks/config/filters cannot be changed by arbitrary child, real linked worktree config readonly, hardlink/symlink/rename race no bypass, `/proc/*/root` alias, sibling session scratches, helper/FD substitution, private mount propagation and timeout/cancellation cleanup. Use synthetic secrets/markers only; do not run destructive host operations.

Run original `sandbox_containment`/`sandbox_landlock`, launch contract/managed-process, `git_execution_origin_matrix`, policy static guards and `scripts/verify.sh quick`, plus M003's new Linux-only adversarial target with exact test host/kernel/namespace/ABI metadata.

## 5. Acceptance and stop

Accept only if real target writes inside its authorized lease while existing/new protected Git/secret descendants and outside roots remain inaccessible despite alternate paths; obtained status lists precise filesystem read/write/protected scope and no fictional network/process enforcement. No target marker on any required backend failure. Legacy Landlock and macOS/Windows builds remain unchanged.

**Stop** for any unprotectable nested descendant, successful path/FD/link escape, host-root read fallback, unsupervised namespace process or requirement to allow all `.git` writes. Produce a blocker/corrective rather than a false M003 close. Only completion of this plan's *qualified native mount backend* can discharge the corresponding parent M003 gate; C006 is not itself permission to activate default hardened mode.
