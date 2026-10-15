# Linux Execution Isolation C009 — M003 Alias, Race, and Compatibility Qualification

Status: active

Repository baseline: `6f764ae76e786ad3e3b1b6e6e25997bc3371ffd0`

Source roadmap: `plans/subsystems/linux-execution-isolation-roadmap.md`

Long-term requirements: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8.

Applicable ADRs: accepted ADR-0031 (Linux mechanism); accepted ADR-0032 (command syntax and execution authority); ADR-0001/0002/0004/0016.

Primary class: capability / security invariant

Corrects: M003 `plans/implementation/linux-execution-isolation/003-protected-workspace-mounts.md` and C006's negative strict-acceptance disposition in `plans/closure/linux-execution-isolation/014-status.md`.

## 1. Objective

Supply the missing native evidence for linked-worktree/common-dir aliases, path replacement races, and ordinary developer-tool compatibility on the opt-in Bubblewrap protected-mount candidate. Keep the Landlock default unchanged and do not register or advertise the candidate as production enforcement unless every M003 acceptance gate passes.

Arbitrary future nested `.git` creation is permitted as workspace content. Trusted Git must remain bound to the daemon-selected worktree, and executable Git routes remain unavailable until separately contained and authorized.

## 2. Why this corrective is ready

- ADR-0031 is accepted.
- M002's typed launch interface and obtained-status boundary are implemented; its remaining macOS cross-compile condition is tracked separately and must be resolved or explicitly classified as non-blocking for this Linux-only gate.
- C001 provides the typed Git execution-class interface.
- C006 provides an opt-in Bubblewrap mount compiler and exact-head hosted proof of its basic managed-process handoff, but not the alias/race/tool compatibility matrix in this plan.

## 3. Current implementation evidence and gaps

`src/security/sandbox/linux_mount_view.rs` discovers existing `.git` entries, linked-worktree pointers and common-dir pointers, pins bind sources through the trusted helper, and fails closed on several ambiguous path types. `crates/egggit/src/process.rs` pins trusted operations to the selected worktree, disables global/system config and selected executable features, and retains commit identity through a bounded data-only handoff.

Current native evidence is one hosted C006 real-child handoff. Missing evidence includes linked worktrees with external common dirs, configuration include attacks, deterministic identity/path replacement during setup, Cargo/rustup and compiler behavior, LSP/PTY compatibility, concurrency/cancellation cleanup, and cold/warm setup measurements.

## 4. Invariants

- No target starts before required mount setup, identity validation, and private status handoff succeed.
- No alias or replacement grants reads/writes outside the selected lease or exposes protected Git/denied content.
- Ambiguous repository layouts and unsupported mount topology fail closed with no target marker.
- Trusted Git reads operate on the selected worktree and cannot execute repository-controlled helpers on the trusted route.
- Obtained status describes only mounted filesystem scope; it does not imply process, IPC, network, seccomp, or resource enforcement.
- Landlock remains the default and all other platform launch behavior remains intact.

## 5. Scope

### In scope

- Native fixture matrix for ordinary repos, linked worktrees, external common dirs, nested repositories/submodules, malformed pointers, and unsupported aliases.
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

Update the active M003 dependency/status summaries to distinguish accepted ADR-0031, the implemented M002 interface, M002's outstanding macOS compile condition, and this corrective's actual native gates. Preserve old closure records and their reviewed-baseline findings.

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
- Compatibility: `cargo check`, a bounded `cargo test` fixture, `rustc`, Python, trusted `git status`/`diff`, one installed LSP or test child, and PTY input/output.
- Lifecycle/contention: timeout/cancel cleanup and two concurrent isolated launches.
- Cross-platform: existing Windows compile and a provisioned macOS compile, without claiming those platforms enforce the Linux mount view.

## 11. Required verification commands

At minimum run the focused Linux mount-view and managed-process tests, `cargo test -p egggit --locked`, `cargo test -p codegg --test sandbox_containment -- --test-threads=1`, `cargo test -p codegg --test sandbox_landlock -- --test-threads=1`, `python3 scripts/check_sandbox_contract.py`, `python3 scripts/check_sandbox_policy_wiring.py`, `python3 scripts/check_execution_ownership.py`, `python3 scripts/check_git_forbidden_patterns.py`, and `scripts/verify.sh quick`. Record the exact native hosted command and result; do not infer native enforcement from compilation or mocks.

## 12. Documentation updates

Update the Linux roadmap and registry with actual status and dependency evidence. Add any supported runtime grant and compatibility limitation to `architecture/security.md` and the sandbox skill. Preserve historical C006 closure findings; append a corrective disposition rather than rewriting them.

## 13. Acceptance criteria

- Every supported repository topology passes native allowed/denied assertions; unsupported or ambiguous topology fails before the target marker.
- Replacement attacks cannot redirect a mount source or trusted Git operation after authorization.
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
- The linked-worktree native fixture now also runs `python3` to write a file inside the selected workspace; the local opt-in native test passed with that addition. Exact-head hosted evidence for the Python compatibility extension is pending.

### Remaining acceptance work

- Expand native layouts: nested repositories/submodules, malformed and unsupported pointers, and other supported alias topologies.
- Expand deterministic replacement coverage across `.git` pointer, gitdir, common-dir, denied path, and ancestor setup barriers; current tests cover mount-source pinning and workspace replacement before target start, not every requested race boundary.
- Extend trusted Git adversarial config coverage beyond the current `includeIf`/`core.worktree`/fsmonitor fixture to hooks, filters, diff/textconv, and helper routes, and prove those routes stay gated.
- Qualify installed rustup/toolchain behavior, Python, LSP or test children, PTY, bounded `cargo test`, startup timings, cancellation/timeout cleanup, and concurrent isolated launches where supported.
- M003 remains blocked; C009 remains active. No backend registration, production routing, or default change follows from this partial hosted pass.
