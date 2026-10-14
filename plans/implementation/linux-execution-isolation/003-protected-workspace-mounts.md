# Linux Execution Isolation M003 — Protected Writable Workspace Mount View

Status: blocked (hard: M002 interface closed and ADR-0031 accepted for backend; C001 Git class is interface, M001 strict closure not required)
Primary class: capability
Repository baseline: `main` at `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03` (2026-10-13). Source roadmap: `plans/subsystems/linux-execution-isolation-roadmap.md`. Long-term requirements: `plans/000-long-term-specification.md` §§4,17,27,29 and `plans/001-terminology-and-domain-model.md` §8. Applicable accepted ADRs: ADR-0001, ADR-0002, ADR-0004, ADR-0016, ADR-0031. Accepted ADR-0032 defines representation-independent authority; C001 typed Git interface is required for integration but M001 Git closure is not. Production backend promotion still requires native qualification and rollout gates. Target closure: `plans/closure/linux-execution-isolation/NNN-status.md` (use milestone number). No code or tests in this plan are represented as already implemented.

## 1. Objective

Deliver an **opt-in, Linux namespace-backed filesystem view** that keeps a leased workspace writable for code edits/build outputs while preventing read/write access to explicitly denied secrets and preventing writes to Git and agent executable-control metadata. Do not ask Landlock to grant a writable ancestor and deny a descendant it cannot express; use a mount graph and prove effective read/write behavior.

## 2. Current evidence and dependencies

Landlock `validate_landlock_write_exclusions` correctly refuses explicit denials under a recursively writable root. Curated runtime reads still include `/proc`, `/sys`, `/var/run`; hardening must not mount those host trees wholesale. Worktrees may store `.git` as a file pointing outside the worktree and share a common gitdir. M001 separates trusted Git execution. M002 establishes an approved trusted helper/namespace adapter; no default mode switch is allowed.

## 3. Invariants

Actual readable mounts never exceed authorized filesystem read roots; hidden denied descendants cannot be accessed by alternate alias, link or inherited descriptor. Workspace mutations stay inside the canonical leased tree plus owned scratch; protected Git hooks, config, attributes controlling external execution, agent settings and secret paths are not writable via the untrusted child. No write access to worktree common directory is inadvertently granted by the sandbox view. Landlock ABI>=3 and normal cwd/toolchain execution still work under the legacy backend. The trusted Git service operates on the host with M001's safety ceiling; no policy weakening for its convenience. ReadOnly never becomes writable.

## 4. Required production changes

A. Build a canonical path/handle-based mount manifest outside child-controlled workspace: system/runtime read roots, narrow Cargo/Rustup/other tool reads, scoped workspace, private scratch, safe device nodes, protected children, optional trusted Git staging route. Resolve Linux symlinks, `.git` pointer and worktree common-dir with `git rev-parse --path-format=absolute --git-dir --git-common-dir` via trusted non-executable Git or an equivalently safe parser. Never trust `.git` string contents to select host mount destinations unchecked.

B. Construct a private mount namespace with explicit read-only runtime mounts, writable lease and private scratch; use bind mounts/overlay ordering to mask or readonly-bind protected descendants even when nested under writable repo. No broad host-root read-only bind unless all sensitive paths are subsequently proven inaccessible and the read guarantee is still truthful. Hide or create a private proc view only when M004 process guarantees are installed; do not advertise process isolation here.

C. Protect existing and **future-created** denied secret locations and links, hardlink aliases, transitive symlinks, renames and paths replaced between manifest validation and mount application. Protect the selected trusted repository's executable metadata and resolved gitdir/common-dir. Do not require the mount graph to forbid arbitrary future nested `.git` creation: nested repositories are untrusted workspace content. The trusted Git process boundary must pin the daemon-selected worktree, prevent repository config from redirecting reads outside it, and refuse any operation that can invoke repository-controlled code until that operation has a qualifying contained route. Cover malicious `includeIf` and worktree configuration. Refuse ambiguous or unprotectable cases rather than writing a misleading `Enforced` status. Treat OCI overlay/overlayfs behavior, writable parent/readonly-child semantics, and mount namespace lifetime as actual test subjects.

D. Allow agent edits to ordinary tracked content while keeping the Git index/refs/lifecycle operational through Codegg-owned trusted typed Git operations. If a full `.git` read-only projection blocks necessary Git operations from shell, return an actionable explicit denial and a safe routed alternative; do not grant unrestricted Git metadata writes. Define allowed protected exceptions separately and test their authority, not as broad `.git` opt-out.

E. Extend Linux capability/status and documentation, including precise `read-only mounted`, `hidden`, `writable` distinctions. Do not claim credentials protected merely from path string listing; test actual open/read/write.

## 5. Ordered work packages

1. Fixture matrix: ordinary repo, linked worktree, worktree with external common-dir, nested .git, bare repo rejection, symlinked workspace root, nested secret, nonexisting future path, hardlinked forbidden file, submodule and adversarial rename race.
2. Mount policy compiler and trusted helper: produce isolated view with expected mount ownership and no host file descriptor bridge.
3. Enforce protected subpaths and future-path denial; prefer in-namespace secure view construction and handle pinning instead of preflight-only string checks.
4. Validate retained shell, Python, git status/diff via safe routes, cargo metadata/check/test, rustc, common LSP/test tools; measure cold/warm setup and duplicate mounts.
5. Add tests/guards and closure evidence, capture real command result and mount topology on supported Linux, document opt-in selection.

## 6. Failure, cancellation, restart, contention

If any mount is missing, unexpected, writable when prohibited, or changes identity during assembly, fail before target. Ensure mount namespace dies with child tree and no host-visible persistent mount/overlay or leaked temp state remains. Two concurrent sessions must not access one another's scratch, Git metadata or workspace grant. Cleanup retains original files and correct leases even after timeout, cancellation and crash. No unowned host mount propagation; make propagation private in the child before manipulating views.

## 7. Compatibility and migration

Initially opt-in only; legacy Landlock `WorkspaceWrite` remains unchanged and honestly weaker. No default network change. Protected metadata may break direct `git commit` within shell; route safe operations via M001 owner and explain the limitation. No schema change unless operator persists opt-in mode, in which case an explicit reviewed migration is required.

## 8. Tests / verification

Unit: canonical manifest, descendant protection, worktree path and alias handling, grant <= authorization. Native adversarial: `openat/openat2`, `O_TRUNC`, rename/hardlink, symlink swapping, executable file in hooks/config, nested secrets created after launch, `/proc/*/root` bypass, mount visibility, sibling scratch, denied host path, allowed shell and compiler writes. Test no-userns host: required tier refuses without executing target. Test both non-root user and constrained distro where applicable. Maintain existing Landlock tests and macOS/Windows compile.

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib security::sandbox
cargo test -p codegg --test sandbox_containment -- --test-threads=1
cargo test -p codegg --test sandbox_landlock -- --test-threads=1
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
scripts/verify.sh quick
```
Add narrow Linux-only mount adversarial integration target; document its exact invocation and host feature matrix in closure.

## 9. Acceptance criteria

A hostile workspace child can write an ordinary leased source/build file and its own scratch but cannot alter/read explicitly denied nested credentials, protected selected-repository gitdir/common-dir executable metadata, or sibling sessions. A future-created denied path remains inaccessible; alternate link and mount aliases do not bypass protection. Arbitrary nested `.git` creation remains permitted as workspace content, but a resulting nested repository cannot use trusted Git configuration to execute code or redirect operations outside the selected worktree. Operations requiring filters, hooks, merge drivers, external helpers, or transport remain blocked unless they have their own contained route. Required enhanced launch fails closed when mount/userns privileges are absent. Scope and backend are reflected by actual obtained evidence, not merely intended manifest.

The trust-boundary reasoning and implementation evidence are tracked in `research/linux-execution-isolation/m003-future-git-metadata-trust-boundary.md`; this correction does not waive the remaining C006 native compatibility and alias/race gates.

## 10. Stop conditions

Stop if nested protected paths cannot be guaranteed under writable ancestor, if worktree alias identity is ambiguous, if the proposed helper requires ambient root/setuid authority, or if toolchain compatibility would require granting host-root reads/secrets. Record a minimal negative reproducer and hand back for decision instead of weakening protections.

## 11. Git dependency clarification

Do not wait for M001 C002 contained-Git local recovery before implementing the protected mount view: that recovery **requires this mount view** and would create a dependency cycle. C001's typed Git execution-class interface is sufficient input to M003. Protect executable Git metadata for *arbitrary child*; later C002 owns narrowly scoped typed Git metadata writes and native-proven Git operations. No subcommand string blocklist may substitute for the mount/lease boundary.
