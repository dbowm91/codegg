# Linux Execution Isolation C009 — M003 Alias, Race, and Compatibility Qualification

Status: closed (bounded candidate qualification; M003 production remains blocked)

Repository baseline: `6f764ae76e786ad3e3b1b6e6e25997bc3371ffd0`

Source roadmap: `plans/subsystems/linux-execution-isolation-roadmap.md`

Long-term requirements: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8.

Applicable ADRs: accepted ADR-0046 (Linux mechanism); accepted ADR-0045 (command syntax and execution authority); ADR-0001/0002/0004/0016.

Primary class: capability / security invariant

Corrects: M003 `plans/implementation/linux-execution-isolation/003-protected-workspace-mounts.md` and C006's negative strict-acceptance disposition in `plans/closure/linux-execution-isolation/014-status.md`.

## 1. Objective

Supply the missing native evidence for linked-worktree/common-dir aliases, path replacement races, and ordinary developer-tool compatibility on the opt-in Bubblewrap protected-mount candidate. Keep the Landlock default unchanged and do not register or advertise the candidate as production enforcement unless every M003 acceptance gate passes.

Arbitrary future nested `.git` creation is permitted as workspace content. Trusted Git must remain bound to the daemon-selected worktree, and executable Git routes remain unavailable until separately contained and authorized.

## 2. Why this corrective is ready

- ADR-0046 is accepted.
- M002's typed launch interface and obtained-status boundary are implemented; its remaining macOS cross-compile condition is tracked separately and must be resolved or explicitly classified as non-blocking for this Linux-only gate.
- C001 provides the typed Git execution-class interface.
- C006 provides an opt-in Bubblewrap mount compiler and exact-head hosted proof of its basic managed-process handoff, but not the alias/race/tool compatibility matrix in this plan.

## 3. Current implementation evidence and gaps

`src/security/sandbox/linux_mount_view.rs` discovers existing `.git` entries, linked-worktree pointers and common-dir pointers, pins bind sources through the trusted helper, and fails closed on ambiguous path types, including bare Git repositories as writable roots. `crates/egggit/src/process.rs` pins trusted operations to the selected worktree, disables global/system config and selected executable features, and retains commit identity through a bounded data-only handoff. Trusted object-reading commands reject static alternates and run against a private per-command snapshot of regular loose and packed objects; the snapshot has no `objects/info` and is copied/reflinked so later alternate creation or source-object mutation cannot change the admitted object view.

Hosted runs cover linked worktrees with external common dirs and nested submodules, configuration attacks, setup replacement races, Cargo/Python/test-child behavior, PTY first-hop transport, cancellation/concurrency, and mount startup samples. The exact-hosted gap is the newest trusted-object snapshot and bare-workspace refusal implementation, which must be run after commit. The native PTY probe qualifies the candidate through the existing transport; production `InteractiveProcessService` profile routing is a separate M004 integration concern and is not required to close this M003 candidate qualification.

## 4. Invariants

- No target starts before required mount setup, identity validation, and private status handoff succeed.
- No alias or replacement grants reads/writes outside the selected lease or exposes protected Git/denied content.
- Ambiguous repository layouts and unsupported mount topology fail closed with no target marker.
- Bare Git repositories are unsupported as writable workspaces and fail before target execution.
- Trusted Git reads operate on the selected worktree and cannot execute repository-controlled helpers on the trusted route.
- Trusted object-reading commands cannot follow repository object alternates created after admission or observe source-object writes made after their private view is prepared.
- Obtained status describes only mounted filesystem scope; it does not imply process, IPC, network, seccomp, or resource enforcement.
- Landlock remains the default and all other platform launch behavior remains intact.

## 5. Scope

### In scope

- Native fixture matrix for ordinary repos, linked worktrees, external common dirs, nested repositories/submodules, malformed pointers, unsupported aliases, and bare-root refusal.
- Barrier-controlled rename/replacement tests across lease capture, mount source opening, and the pre-target gate.
- Trusted Git configuration tests for local `core.worktree`, `includeIf`, hooks, fsmonitor, filters, diff/textconv, and helper routes.
- Native compatibility checks for Cargo/rustup/rustc, Python, representative trusted Git reads, LSP/test subprocesses, and PTY use where available.
- Exact Linux host/kernel/Bubblewrap/LSM/obtained-scope evidence and bounded cold/warm startup measurements.
- Reconcile live registry, roadmap, and handoff dependency statements without rewriting historical closure claims.

### Explicitly out of scope

- Enabling Bubblewrap by default or registering a generally selectable production profile.
- Process/IPC/seccomp enforcement (M004), offline networking (M005), mediated egress (M006), or cgroup quotas (M007).
- Restoring contained Git mutations/remotes (C002/C003), broad Git parity, or forbidding nested repository creation.
- Changing accepted ADRs, canonical requirements, persistent policy, or public protocol.

## 6. Required production changes

Change mount or trusted-Git implementation only when an adversarial fixture demonstrates a real failure. Any fix must preserve descriptor pinning, parent authority ceilings, fail-closed refusal, and the existing Landlock default. Do not broaden host mounts to make compatibility tests pass; add narrow grants only when their security scope is explicit and tested.

## 7. Ordered work packages

### A — Reconcile live planning state

Update the active M003 dependency/status summaries to distinguish accepted ADR-0046, the implemented M002 interface, M002's outstanding macOS compile condition, and this corrective's actual native gates. Preserve old closure records and their reviewed-baseline findings.

### B — Qualify repository aliases and Git trust boundary

Build ordinary, linked, external-common-dir, nested/submodule, malformed-pointer, and replacement fixtures. Prove allowed trusted reads stay on the selected worktree and executable Git features remain terminally gated. Refuse unsupported or ambiguous layouts.

### C — Exercise replacement races and failure handoff

Use synchronization barriers to replace/rename leased roots, `.git` pointers, gitdirs, common dirs, denied paths, and ancestors at each pre-target boundary. Assert pinned identity or setup failure and assert no target marker on failure.

### D — Qualify developer-tool compatibility

Run the supported commands under the actual native mount candidate. Record required narrow runtime/cache grants, successful expected writes, denied outside accesses, startup latency, and any unsupported command with a concrete refusal. Include PTY and LSP/test child paths where the host's installed tools permit them.

### E — Exact-head hosted native run and disposition

Run the expanded Linux fixture suite on a hosted runner with Bubblewrap 0.9.0 or newer, recording kernel, userns/LSM policy, helper identity, actual obtained scope, and all test outcomes. Keep platform compile evidence separate from native Linux enforcement evidence.

## 8. Failure, cancellation, restart, and contention semantics

Every required setup or identity failure aborts before target execution. Cancellation and timeout must tear down Bubblewrap and descendants, close private descriptors, remove owned scratch, and leave host worktree/Git metadata unchanged. Concurrent launches may not share status writers, scratch roots, writable leases, or metadata authority.

## 9. Compatibility and migration

No persistent configuration or default changes. Unsupported layouts remain explicit refusals. A passing result qualifies only the tested Linux candidate and exact source revision; macOS and Windows compile/runtime claims remain independently scoped.

## 10. Required tests

- Unit: `.git` directory/file parsing, linked gitdir/common-dir resolution, malformed and oversized pointers, alias normalization, unsupported mount topology, and trusted Git config precedence.
- Native security: protected metadata, denied secrets, outside symlinks/hardlinks, common-dir writes, replacement races, no-target setup failure, and actual mount visibility.
- Compatibility: `cargo check`, a bounded `cargo test` fixture, `rustc`, Python, trusted `git status`/`diff`, one installed LSP or test child, and PTY input/output through the existing first-hop transport.
- Lifecycle/contention: timeout/cancel cleanup and two concurrent isolated launches.
- Cross-platform: existing Windows compile and a provisioned macOS compile, without claiming those platforms enforce the Linux mount view.

## 11. Required verification commands

At minimum run the focused Linux mount-view and managed-process tests, `cargo test -p egggit --locked`, `cargo test -p codegg --test sandbox_containment -- --test-threads=1`, `cargo test -p codegg --test sandbox_landlock -- --test-threads=1`, `python3 scripts/check_sandbox_contract.py`, `python3 scripts/check_sandbox_policy_wiring.py`, `python3 scripts/check_execution_ownership.py`, `python3 scripts/check_git_forbidden_patterns.py`, and `scripts/verify.sh quick`. Record the exact native hosted command and result; do not infer native enforcement from compilation or mocks.

## 12. Documentation updates

Update the Linux roadmap and registry with actual status and dependency evidence. Add any supported runtime grant and compatibility limitation to `architecture/security.md` and the sandbox skill. Preserve historical C006 closure findings; append a corrective disposition rather than rewriting them.

## 13. Acceptance criteria

- Every supported repository topology passes native allowed/denied assertions; unsupported or ambiguous topology fails before the target marker.
- Replacement attacks cannot redirect a mount source or trusted Git read after authorization; object reads use a private command-scoped view with no repository alternates.
- Trusted local Git metadata cannot execute hooks, fsmonitor, filters, diff drivers, or helpers on the trusted route, and cannot redirect reads outside the selected worktree.
- The documented compatibility subset passes on the exact hosted source head with no broad host-root mount.
- No default change or production backend promotion occurs unless M003's full closure definition and every required native gate are satisfied.

## 14. Stop conditions

Stop and record a negative result if a linked/common-dir alias cannot be pinned safely, any path replacement reaches the target with changed authority, compatibility requires a broad host grant, or required native host facilities are unavailable. Do not weaken the acceptance boundary to turn a failed fixture green.

## 15. Closure evidence required

Include a requirement-to-test matrix, exact hosted workflow/run and SHA, host/kernel/Bubblewrap/LSM/userns facts, actual mount scope, per-tool outcomes and timings, race outcomes, cross-platform compile evidence, unresolved findings, and an explicit M003/downstream unblock audit.

## 16. Handoff notes

The candidate is currently internal and opt-in through `SandboxConfig::with_linux_protected_mount_view`; do not route production tools to it or claim enhanced network/process/resource enforcement. Historical C006 and M002 closures are immutable evidence, not substitutes for this corrective's missing tests.

## 17. Implementation progress — 2026-10-15

### Exact-head hosted native evidence

- Commit `ccbaa9bdca23e6afe365358a09296342647c2fda` was tested by hosted run [`38671588961`](https://github.com/dbowm91/codegg/actions/runs/38671588961) (`Linux C006 protected mount qualification`), job `116215708578`; conclusion: success.
- Runner: GitHub-hosted Ubuntu 24.04.5, kernel `6.17.0-1022-azure`, x86_64, Bubblewrap `0.9.0` (`0.9.0-1ubuntu0.3`), `/usr/bin/bwrap` mode 0755 owned by root. Runner identity was UID/GID 1001; AppArmor profile was `unconfined`. `kernel.apparmor_restrict_unprivileged_userns` was initially `1`, then changed to `0` on this ephemeral hosted runner for the native qualification. This is test-host setup evidence, not a production host requirement.
- Exact hosted command: `cargo test --locked --lib protected_mount_view -- --nocapture`, with `CODEGG_RUN_C006_BWRAP_NATIVE=1` and `CODEGG_RUN_C009_BWRAP_NATIVE=1`.
- Result: 2 passed, 0 failed. The C009 test exercised a linked worktree whose common Git directory is external to the selected worktree, verified trusted reads and denied writes to common Git config/hooks, permitted an ordinary worktree write, and completed dependency-free `cargo check --offline` inside the protected mount. The existing C006 real managed-process handoff also passed.
- The workflow proves a real native protected-mount launch and the tested fixture's obtained visibility/denial behavior. It does not record `uname`/Bubblewrap facts for a broader host matrix or claim process, IPC, network, seccomp, or resource isolation.
- Commit `6cdfee6662bdef3fedbd9e2f58a09ff160da6626` was tested by hosted run [`38672792308`](https://github.com/dbowm91/codegg/actions/runs/38672792308), job `116219548811`; conclusion: success. It passed the two protected-mount native tests and all 9 tests selected by the serial trusted-Git config filter; the full local egggit suite passed 96 tests.
- The trusted-Git integration regression covers repository-configured external diff and textconv helpers; ordinary diff output remained available and the helper marker stayed absent. Hosted evidence is run `38672792308`.
- Commit `9dddcc5d7c69dd3117dc8eafd009881719dec63d` was tested by hosted run [`38674058393`](https://github.com/dbowm91/codegg/actions/runs/38674058393), job `116223507871`; conclusion: success. It again passed 2/2 protected-mount tests and 9/9 trusted-Git config tests.
- The linked-worktree native fixture runs `python3` to write a file inside the selected workspace; the assertion passed locally and on the exact hosted head. This qualifies one normal Python write path, not general Python package installation or subprocess compatibility.
- Commit `9ba0ba7dfbf9526cbceab867ed5421ffec42c102` was tested by hosted run [`38675043602`](https://github.com/dbowm91/codegg/actions/runs/38675043602), job `116226619752`; conclusion: success. It passed 2/2 native protected-mount tests and 10/10 trusted-Git config tests.
- A direct trust-boundary regression creates a nested repository after the outer repository has been selected, gives its local config an outside `core.worktree` and executable fsmonitor, then runs trusted outer `git status`. Local and exact-head hosted results pass: Git reports the nested repository as ordinary untracked content, does not reveal the outside marker, and does not execute the nested fsmonitor.
- The final workflow-filter/docs head `d3e6cb8c76d8647747f9a627bd6148b9ab1e5291` was also tested by hosted run [`38675775638`](https://github.com/dbowm91/codegg/actions/runs/38675775638), job `116228913415`; conclusion: success. It passed the same 2/2 native and 10/10 trusted-Git tests, including the nested-repository regression.
- Commit `b3e77a64a9ccbe2b49908eeaaefd9b2bf227dd5a` passed hosted run [`38680772418`](https://github.com/dbowm91/codegg/actions/runs/38680772418), job `116244575585`; conclusion: success. The runner was GitHub-hosted Ubuntu 24.04, kernel `6.17.0-1022-azure`, Bubblewrap `0.9.0`, UID/GID 1001, AppArmor profile `unconfined`; the unprivileged-userns restriction changed from `1` to `0` on the ephemeral runner. The native workflow passed 2/2 protected-mount tests, now including the nested-submodule/linked-worktree/common-dir fixture, and 12/12 trusted-Git tests, now including clean-filter and executable-alias refusal. The exact tested revision does not include the subsequent malformed-pointer and replacement-race additions below.

### Additional local evidence — hosted qualification pending

- The native linked-worktree fixture now also creates and checks out a real nested submodule. The hosted Bubblewrap child read the submodule worktree and ran trusted `git status` inside it while writes to the outer common config, common hook, and submodule gitdir config were denied. The local fixture removes test-created hardlinks from the Git metadata tree; production still refuses hardlinked protected metadata.
- The native managed-process handoff now creates malformed and oversized nested `.git` pointer fixtures. Both setup attempts fail before their target marker is created. This addition is local-only pending hosted qualification.
- Trusted-Git tests now exercise a repository clean filter and a shell executable alias. The trusted route returns the typed `BackendUnavailable` capability result before spawn; neither helper marker appears and the filter attempt creates no index. Both cases passed hosted run `38680772418` on `b3e77a64`.
- Local verification passed: `cargo test -p egggit --locked -- --test-threads=1` (99 passed), `cargo test -p codegg --test sandbox_containment -- --test-threads=1` (15 passed), `cargo test -p codegg --test sandbox_landlock -- --test-threads=1` (3 passed), the sandbox contract/policy wiring, execution ownership, and forbidden Git-pattern guards, plus `scripts/verify.sh quick`.
- An earlier local native run passed 5 `protected_mount_` tests, including both real Bubblewrap handoffs, malformed and oversized nested pointers, linked-worktree `.git` pointer/gitdir/common-dir replacement, workspace replacement, and denied-path alias replacement. Subsequent expanded local and hosted results are recorded below.
- Post-lease replacement of the linked-worktree `.git` pointer, gitdir, and common dir each fails setup before target execution. An earlier version's denied-path mask appeared to hide a pre-launch symlink into a separately granted root, but a new post-setup barrier test showed a host rename could detach that mask and let the child read the replacement target (exit code 91). The current compiler therefore refuses deny paths that resolve beneath any writable grant; a pre-launch deny alias into another writable grant also refuses before the target marker. Do not rely on descendant deny masks under writable roots.
- The linked-worktree compatibility fixture now runs both offline `cargo check` and a bounded offline `cargo test`; it sets Cargo's target linker to the runner's canonical `cc` path because `/usr/bin/cc` resolves through an unmounted `/etc/alternatives` symlink in the mount view. Both commands pass in the native Bubblewrap child. The fixture also invokes installed `rustup`; rustup fails with `no /proc/self/exe available`, so the test accepts only that explicit limitation (or a successful rustup run). `/proc` is not added to the filesystem-only view because doing so changes the process-isolation boundary owned by M004. This is a concrete rustup compatibility limit, not a claim of rustup support.
- Hosted run [`38681857998`](https://github.com/dbowm91/codegg/actions/runs/38681857998), job `116247953738`, passed on exact commit `426dc4959e5d212370af83a822b2eb9b67dfc68e`. Its `protected_mount_view` test filter covers the real-child fixtures, including malformed/oversized nested-pointer refusal, but not the separately named post-lease replacement tests; those race additions remain local pending a widened workflow filter.
- Commit `55031eb26c5c7ba71e01243a4f26c37014fa71f1` passed hosted run [`38718065312`](https://github.com/dbowm91/codegg/actions/runs/38718065312), job `116365519461`. On GitHub-hosted Ubuntu 24.04.5, kernel `6.17.0-1022-azure`, Bubblewrap 0.9.0, UID/GID 1001, AppArmor unconfined (userns restriction changed from 1 to 0 on the ephemeral runner), the existing `protected_mount_view` filter passed 3/3 native tests. This includes the linked-worktree/submodule fixture, offline Cargo check/test, and the rustup `/proc/self/exe` limitation check. Trusted-Git qualification passed 12/12 tests. The filtered invocation still omits the separately named post-lease replacement tests and the new concurrent-launch test; they remain local pending the workflow-filter expansion.
- Commit `872d0bd846090c9481300e8136cdbce48aead034` passed hosted run [`38719045615`](https://github.com/dbowm91/codegg/actions/runs/38719045615), job `116368759428`, with the expanded `protected_mount_` filter: 6/6 native tests and 12/12 trusted-Git tests passed. The six native tests include linked-worktree/submodule/common-dir protection, malformed and oversized nested pointers, workspace/metadata/denied-alias replacement, and concurrent isolated launches. Runner facts: GitHub-hosted Ubuntu 24.04.5, kernel `6.17.0-1022-azure`, Bubblewrap 0.9.0, UID/GID 1001, AppArmor unconfined; `kernel.apparmor_restrict_unprivileged_userns` changed from 1 to 0 on the ephemeral runner. The real-child fixture also includes offline Cargo check/test, Python workspace write, and rustup's explicit `/proc/self/exe` limitation. Native cancellation cleanup was added after this head and remains pending hosted qualification.
- Commit `60433598817fb0ba2792d905f24acdc4a8c066b8` passed hosted run [`38719972886`](https://github.com/dbowm91/codegg/actions/runs/38719972886), job `116371833906`: 6/6 native protected-mount tests and 12/12 trusted-Git tests. The real-child test now includes both timeout and cancellation cleanup, and completed with no delayed marker after either termination. Runner was Ubuntu 24.04.5, kernel `6.17.0-1022-azure`, Bubblewrap 0.9.0, AppArmor unconfined, UID/GID 1001; the ephemeral runner's userns restriction changed from 1 to 0.
- Commit `add35993a2479d9d3e1cf1bdff99db865c75e259` passed hosted run [`38721017009`](https://github.com/dbowm91/codegg/actions/runs/38721017009), job `116375292315`: 7/7 protected-mount tests and 12/12 trusted-Git tests. The cold and warm minimal launch samples were 40.37 ms and 22.47 ms on GitHub-hosted Ubuntu 24.04.5, kernel `6.17.0-1022-azure`, Bubblewrap 0.9.0, UID/GID 1001, AppArmor unconfined; userns restriction changed from 1 to 0 on the ephemeral runner. These paired observations do not define an SLO.
- Commit `30833b8a211a0949d3a404d5be43c2692ccec0e0` passed hosted run [`38722098349`](https://github.com/dbowm91/codegg/actions/runs/38722098349), job `116378893704`: 8/8 native protected-mount tests and 12/12 trusted-Git tests. It includes the parent-ancestor replacement refusal. Runner facts remain Ubuntu 24.04.5, Bubblewrap 0.9.0, UID/GID 1001, AppArmor unconfined, with userns restriction changed from 1 to 0. The newly discovered post-setup denied-path mask race and its fail-closed correction are subsequent work and require exact-head hosted evidence.
- Latest local native rerun after adding the Cargo test/rustup probe passed all 5 `protected_mount_` tests, including the real linked-worktree/submodule handoff and bounded Cargo test.
- A separate local native concurrency test runs two Bubblewrap launches simultaneously against distinct workspaces and confirms each writes only its own marker. It passed locally and exact-head hosted run `38719045615`.
- The real-child fixture cancels a running Bubblewrap child and verifies its delayed marker is absent; local and exact-head hosted run `38719972886` passed.
- A post-lease ancestor replacement test renames the leased parent and redirects its original path to a different workspace; launch fails on canonical identity change and leaves no target marker. It passed locally and awaits hosted evidence.
- A post-setup barrier test showed that a deny mask nested under a writable grant can be detached by a host-side rename and that a symlink can then expose a separately granted file. The protected-mount compiler now refuses all deny paths beneath writable grants; direct nested-deny and deny-alias cases assert `SandboxFailed` before the target marker. The real-child fixture no longer claims nested deny support.
- Hosted run [`38724319195`](https://github.com/dbowm91/codegg/actions/runs/38724319195), job `116386292207`, passed on exact commit `9b12ddf84f09d8447d9f4e4d937e5e28ae3db7da`: 9/9 native `protected_mount_` tests and 12/12 trusted-Git tests. This qualifies the deny-under-writable-grant refusal, workspace and ancestor replacement refusal, linked-worktree mount view, cancellation/timeout cleanup, concurrent launches, and bounded launch timing on the hosted Ubuntu 24.04.5/Bubblewrap 0.9.0 runner.
- A local trusted-Git regression reproduced `git cat-file` reading an object from an external repository through the selected repository's `.git/objects/info/alternates`. The current implementation refuses pre-existing alternates found in either the worktree Git directory or linked-worktree common directory with typed `BackendUnavailable`; 2 targeted tests and the full egggit suite (101 tests) pass locally. This is only a preflight snapshot: writable repository metadata can change after the check, so it does not satisfy the race-safe trusted-Git acceptance criterion and must not be described as complete containment.
- Cross-platform compile checks passed locally: `cargo check -p egggit --tests --target x86_64-pc-windows-gnu --locked` (three existing test-only unused warnings, no errors) and `cargo check -p egggit --tests --target x86_64-apple-darwin --locked` (no errors).
- Hosted run [`38725723076`](https://github.com/dbowm91/codegg/actions/runs/38725723076), job `116390963426`, passed on exact commit `496b8e403124bef97dbe28900bd008863dd2e8af`: 9/9 native protected-mount tests and 14/14 trusted-Git tests, including both static object-alternate refusals. Runner facts: GitHub-hosted Ubuntu 24.04.5, kernel `6.17.0-1022-azure`, Bubblewrap `0.9.0-1ubuntu0.3`, UID/GID 1001, AppArmor `unconfined`; the ephemeral runner's `kernel.apparmor_restrict_unprivileged_userns` changed from 1 to 0.
- Exact-head macOS compile workflow [`38725723102`](https://github.com/dbowm91/codegg/actions/runs/38725723102) passed `cargo check -p codegg --lib --target x86_64-apple-darwin --locked` on `496b8e403124bef97dbe28900bd008863dd2e8af`. Local Windows GNU and macOS test-target checks for `egggit` also passed; the Windows target emitted three existing test-only unused warnings.
- The PTY first-hop probe runs Bubblewrap through `InteractiveProcessService` without a broad host-root bind. It confirms the child retains a controlling terminal on stdin/stdout/stderr and can write to the selected workspace. Hosted run [`38727487239`](https://github.com/dbowm91/codegg/actions/runs/38727487239), job `116396842810`, passed it on exact commit `508bc89f8ef2c6cde540fdc038b294d187e739fe`. This tests mount-view compatibility at the PTY transport; the service still has no sandbox descriptor or profile-routing contract, so service-level sandbox integration remains open.
- Hosted run [`38727487239`](https://github.com/dbowm91/codegg/actions/runs/38727487239), job `116396842810`, passed on exact commit `508bc89f8ef2c6cde540fdc038b294d187e739fe`: 10/10 native protected-mount tests and 14/14 trusted-Git tests. Runner: GitHub-hosted Ubuntu 24.04.5, kernel `6.17.0-1022-azure`, Bubblewrap `0.9.0-1ubuntu0.3`, UID/GID 1001, AppArmor `unconfined`; `kernel.apparmor_restrict_unprivileged_userns` changed from 1 to 0 for the ephemeral runner.
- Exact-head macOS compile workflow [`38727487230`](https://github.com/dbowm91/codegg/actions/runs/38727487230) passed on `508bc89f8ef2c6cde540fdc038b294d187e739fe`. Current local bundle: all 10 `protected_mount_` tests pass with C006/C009 native flags enabled, including the PTY first-hop probe; egggit (101 tests), `scripts/verify.sh quick`, formatting, ADR integrity, forbidden-Git, sandbox contract/policy, execution ownership, and diff checks pass on the working tree.

### Implementation progress — 2026-10-15 (trusted object-view corrective)

- Trusted Git read admission now prepares a private per-command object view for the exact object-reading allowlist. The view excludes `objects/info`; Git receives its path through `GIT_OBJECT_DIRECTORY`, and `GIT_OPTIONAL_LOCKS=0` remains enabled. Static alternates still return typed `BackendUnavailable[RepositoryAlias]` for compatibility. Mutation and executable Git routes do not use this view.
- Unix setup traverses the object directory using descriptor-relative, no-follow opens, validates fanout/pack names and regular-file types, and bounds entries. Linux uses `FICLONE` when the filesystem supports it, falling back to a private byte copy; other platforms use private copies. The temporary view remains alive through command completion/cancellation.
- Added regression tests prove an alternate created after admission is ignored for ordinary and linked-worktree common-dir layouts, and that mutating a source loose object after admission cannot change the already-created private view. Added a Linux native protected-mount test proving a bare repository is refused before target execution.
- Local verification: `cargo test -p egggit --locked -- --test-threads=1` passed 105 tests; the focused private-view route matrix passes 1/1. Windows GNU and macOS egggit test-target checks pass; Windows retains three existing test-only unused warnings. Native protected-mount tests pass 11/11, sandbox containment 15/15, Landlock 3/3, and the final `scripts/verify.sh quick` passes. Exact-head hosted verification is pending after commit.
- The plan's compatibility interpretation is resolved: C009 qualifies PTY compatibility at the existing first-hop transport boundary. It does not add production service routing, which belongs to M004 and is not required for the opt-in M003 candidate qualification.

### Acceptance disposition — closed 2026-10-15

- Supported repository layouts in this corrective are covered: ordinary repositories, linked worktrees with external common dirs, nested repositories/submodules as workspace content, malformed and oversized pointers, and bare-root refusal. Unsupported layouts fail closed.
- Replacement coverage includes leased `.git` pointer/gitdir/common-dir/workspace-root/ancestor changes, denied aliases, and the post-setup deny-mask race. Denies beneath writable grants are rejected before target start.
- The typed Git matrix covers the trusted object-reading routes, configuration include and `core.worktree`, fsmonitor, hooks, diff/textconv, filters, executable aliases, late object alternates, and source-object mutation. The exact trusted read allowlist uses the private object view; object-writing Git mutations and remote routes remain owned by C002/C003 and are not promoted by C009.
- Exact-head hosted qualification passed on commit `2921a9c6a77588669801cebb24b80c1d1c8f1a4f`: Linux run `38741704670` passed 11/11 native protected-mount tests and 18/18 trusted-Git tests. Runner evidence: GitHub-hosted Ubuntu 24.04.5, kernel `6.17.0-1022-azure`, Bubblewrap `0.9.0-1ubuntu0.3`, UID/GID 1001, AppArmor profile `unconfined`; user-namespace restriction changed from 1 to 0 on that ephemeral runner. Measured minimal mount launch was 30.46 ms cold and 19.83 ms warm.
- Exact-head macOS compile passed in run `38741704581`. Local Windows GNU and macOS egggit test-target checks pass. Local verification also passed: egggit 105/105, focused object-route matrix 1/1, protected-mount 11/11, sandbox containment 15/15, Landlock 3/3, ADR integrity, policy/ownership guards, and `scripts/verify.sh quick`.
- C009 qualifies PTY behavior at the existing first-hop transport boundary; production `InteractiveProcessService` profile routing belongs to M004. Rustup's `/proc/self/exe` requirement and refusal of nested deny under writable grants remain explicit compatibility limitations.
- Trusted object-view setup cost measured locally at 2.95 seconds for `git status` over a fragmented 105 MiB / 3,945-file object store on ext4 without reflink support. Packed-repository cost is not measured. This is a recorded medium compatibility finding and blocks production Git routing until representative performance is qualified. C002 contained object-writing mutations remain independently blocked.
- C009 is closed as bounded candidate qualification. M003 remains blocked: no backend registration, production routing, or default change follows from this closure; Landlock remains canonical.
