# Linux Execution Isolation C006 — M003 Protected Mount View and Git Metadata Integration

Status: **closed negative for strict M003 acceptance** — the bounded opt-in protected-mount candidate and its first native qualification slice are implemented; M003 production acceptance remains blocked by the uncompleted native compatibility/threat matrix. The 2026-10-14 trust-boundary review determines arbitrary nested `.git` creation need not be denied when trusted Git is pinned to the selected worktree and executable routes stay gated; see `research/linux-execution-isolation/m003-future-git-metadata-trust-boundary.md`. Closure: `plans/closure/linux-execution-isolation/014-status.md`.
Class: capability / security invariant
Closure target: `plans/closure/linux-execution-isolation/014-status.md`.
Baseline: main @ `ec43e495c3619966ac33d0acccf7904812dd37bc` (2026-10-13 EDT). Long-term: `plans/000-long-term-specification.md` §§4/17/27/29, `plans/001-terminology-and-domain-model.md` §8, and `plans/003-planning-process.md`. Execution authority: ADR-0001/0004/0016; command classification: accepted `plans/adrs/ADR-0045-command-syntax-and-execution-authority.md` (canonical identity reconciled under C004). Linux mechanism: accepted ADR-0043; this plan implements its protected-mount slice without enabling production defaults. Existing Landlock ABI >=3 filesystem-only support must remain intact. This is a handoff plan, not a claim of completed implementation or test execution.

## Implementation progress (2026-10-14)

The internal opt-in routes through `ManagedProcessService` and the trusted
one-shot helper, compiles a fresh-root mount graph, pins bind sources with
`openat2` and Bubblewrap fd binds, snapshots grant identities, protects
existing Git metadata read-only, and hides future root `.git` paths and denied
descendants. Bubblewrap's `--json-status-fd` reports its sandbox child before
setup finishes; the managed-process owner validates that record and releases
Bubblewrap's `--block-fd` barrier. Bubblewrap consumes the acknowledgment only
after mount setup and privilege drop, before it starts the requested target.
The private status stream is drained and checked for Bubblewrap's terminal
target-exec result. Framed helper errors remain distinct from Bubblewrap JSON
status. Existing configuration stays on Landlock.

Hosted native qualification is running on GitHub's Ubuntu 24.04 runner with
Bubblewrap 0.9.0 (kernel `6.17.0-1022-azure`, UID 1001). The runner starts with
`kernel.apparmor_restrict_unprivileged_userns=1`; the dedicated ephemeral job
records this and temporarily sets it to `0` so unprivileged user namespaces
can be qualified. The job's AppArmor profile is `unconfined`; Bubblewrap is
mode 0755 and not setuid. Run `38529346419` reached Bubblewrap but failed during
UID-map setup under the original host policy. Run `38530810345` passed helper
build and namespace setup, then exposed that `/etc/gai.conf` is a directory on
this runner. Run `38531874263` showed that the compiler also created file mount
destinations as directories, so Bubblewrap still failed at that path. The
compiler now binds allowlisted `/etc` entries only when they are regular files
and creates only parent directories for file mounts. Run `38532957219` passed
mount setup but the target's ELF loader could not resolve `/lib64`; the
canonical runtime grants had omitted the host's `/lib64 -> /usr/lib64`
compatibility alias. The compiler now recreates standard `/bin`, `/sbin`,
`/lib`, and `/lib64` aliases only when their resolved sources are inside an
already-granted read-only runtime root. Exact-head run `38534083804` passed the
native `ManagedProcessService` handoff on Ubuntu 24.04: 1/1 test passed in
0.51 seconds after compilation. The child wrote an allowed workspace file,
read but could not change root Git config/hooks, could not read/write existing
or future denied secrets or an outside symlink target, saw no host `/proc` or
`/sys`, ran Python, and completed fail-closed identity and timeout probes. The
run `38535156777` passed the child-created hardlink probe: the protected Git
config could not be linked into the writable workspace. Run `38536302274`
passed after the obtained-scope reporter was corrected to list only regular
`/etc` files actually mounted. Earlier attempts exposed and corrected a Git
fixture assumption about the hooks directory, inaccessible global deny-path
probing, status-fd inheritance, status/ack handling, mount destination type,
and standard runtime aliases.

C006 closes with a **negative disposition for strict M003 acceptance**. The
native pass qualifies only the bounded root-repository mount handoff. The
compiler discovers existing nested Git metadata at launch and masks an absent
workspace-root `.git`, but it cannot protect `.git` created later under a new
writable nested directory. A static Bubblewrap mount graph cannot intercept
that future path. Linked worktrees/common-dir aliases, rename races, toolchain,
LSP/PTY compatibility, startup measurements, and the typed Git metadata
transaction contract also remain unqualified. The Landlock default remains
canonical, and no production route is registered or enabled. M003/C002 remain
blocked until an accepted corrective establishes protection for future nested
metadata and the remaining native matrix passes.
The compiler protects an absent workspace-root `.git` path and discovers
existing nested submodule metadata at launch. It does not establish protection
for a new nested repository created later inside a newly created writable
directory; the native threat matrix must resolve that topology before this
plan can close.

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
