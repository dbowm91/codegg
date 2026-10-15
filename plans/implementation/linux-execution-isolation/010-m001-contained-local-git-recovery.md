# Linux Execution Isolation M001 Corrective C002 — Contained Local Git Workflow Recovery

Status: blocked (hard: C001; M002 launch foundation, M003 protected mounts, M004 process/IPC and M005 offline network fully verified on native Linux; accepted ADR-0033 still requires native backend qualification before production use)
Primary class: capability
Target closure: `plans/closure/linux-execution-isolation/010-status.md`
Repository baseline: Linux branch `c7c18181862d67c9e2a649399bbda9665869edd8` (M001 partial implementation), as compared with `main` on 2026-10-13. Source: `plans/subsystems/linux-execution-isolation-roadmap.md` and accepted ADR-0032; ADR-0033 was proposed at this baseline and accepted separately on 2026-10-14. Corrects M001 `plans/implementation/linux-execution-isolation/001-trusted-git-execution-boundary.md` and historical blocked `plans/closure/linux-execution-isolation/001-status.md` without revising its evidence. Long-term: `plans/000-long-term-specification.md` §§4,17,27–29 and `plans/001-terminology-and-domain-model.md` §8. New corrective closure must be independent and preserve original historical M001 findings.

## 1. Objective

Restore legitimate *local* Git mutation and recovery operations that may invoke repository-controlled executable components without granting those components greater privilege than the agent. Require a genuinely verified isolated child route, narrowly leased Git metadata writes through a trusted service, no ambient network/credentials, and native adversarial acceptance. Do not mix network Git transport/SSH/credentials into this corrective: C003 owns those.

## 2. Why blocked; evidence

Git M001 strict constructor refuses `add`, `checkout`, `restore`, `merge`, `rebase`, `switch`, `cherry-pick`, mutating `stash` and more. Such operations can invoke clean/smudge/process filters, hooks or merge drivers from `.gitattributes`, local config and worktree context. Landlock-only restricts filesystem but does **not** establish offline network, PID/IPC/socket or special Git metadata integrity. Native sandbox M002/M003/M004/M005 and their **obtained** evidence are hard requirements before general executable Git semantics become available.

## 3. Invariants

- Repository-controlled executable code never executes in privileged daemon/root scope; launch every executable Git helper and its descendants within an enforced profile not broader than originating agent/worktree.
- Protected gitdir/common-dir cannot be rewritten by arbitrary child Bash. Git index/refs/staging writes required by legitimate operations occur through an explicitly authorized narrow Git service/operation lease, not a blanket bind of all `.git` or agent-controlled hooks/config.
- No ambient host/SSH agent/DBUS/Docker socket/proc, credential files, private home or outbound network; local Git executable filters/hooks have no network by default. If legitimate project workflow requires more, require independent user approval and a later authorized mediation path, never auto-upgrade.
- Parent/child agent scope, original Git operation identity, argv/path validation, worktree lease, audit and rollback/checkpoint are preserved. `git -c` overrides and environment that expand authority still blocked at the trusted execution boundary, even if the user can write ordinary source.
- An unavailable/partial required sandbox stops before executing any repo-supplied program; do not assume bubblewrap/namespace probe equals obtained isolation.

## 4. Required production work

A. Create a typed `ContainedGitOperation` dispatch behind the existing `GitMutationExecutor`/`GitExecutionService` and `ManagedProcessService`, consuming C001's capabilities and M002's launcher facts. One trusted scheduler-owned process lifecycle, not a subprocess created by untrusted Git strings. Classify each Git operation's needs (write worktree, stage refs/index, invoke external local driver, require network). Preserve exact typed argv and operation metadata; flag unsupported unsafe routes explicitly.

B. Model Git metadata as a separate trusted write-authority resource. M003's protected mount view must continue to hide/readonly external gitdir and executable config/hook paths to arbitrary shell. To mutate index/refs, choose an isolated broker/transactional staging route whose write scope can be proven non-executable and bound to lease and validated gitdir identity; test .git pointer, linked worktree and shared common-dir. Do not use "temporarily writable entire .git" as an easy workaround without proof that config/hook execution cannot escape isolation or contaminate daemon follow-up.

C. Within verified offline/process-isolated child, run operations requiring local filters/merge drivers/hooks only with the least effective capabilities and no higher-authority credentials. Distinguish user-intended hook execution from safe daemon maintenance. If a project filter cannot operate without network, surface typed network capability requirement and leave that variant unimplemented until separately mediated, without breaking safe local operations.

D. Preserve worktree leases, staged/unstaged state, conflict/recovery semantics, rollback/checkpoint attribution and cancellation; handle partial index writes, concurrent operations and crash recovery without trusting gitdir text as an authority locator.

E. Update `src/tool/git.rs`, `src/git_mutations*.rs`, `src/git_recovery.rs`, `src/git_service.rs`, `crates/egggit/src/process.rs`, relevant architecture/validation docs and static guard. No rewrite of Git parser or duplicate network policy stack.

## 5. Work packages

1. Freeze all 9/12 M001 failure cases and classify local vs network.
2. Implement the trusted metadata write transaction and contained child launch, binding requested/obtained enforcement to exact operation and lease.
3. Restore local add, switch/checkout, restore, merge/rebase, mutating stash and conflict recovery in small groups; qualify filters, hooks and executable drivers with synthetic marker/secret fixtures.
4. Exercise linked worktree, Git common-dir, symlink/hardlink, concurrent agents, staged index rollback, kill/reap and restart.
5. Run tests/guards and supported Linux native proof. Update M001 historical closure via **link to corrective** only; do not rewrite original blocked finding.

## 6. Tests and verification

Positive: ordinary staging/add/commit/checkouts, branch switch, merge conflicts and resolves, rebase, stash and restore, with representative benign local filters or hooks under intended authority. Negative: hostile filter, merge driver, fsmonitor, hook and custom Git config trying to read synthetic home secret/outside root, reach host loopback/UDP, trigger cross-session IPC or write protected gitdir; no unintended marker. Reproduce same effects using Bash-script bypass; compare actual authority not invocation spelling. Test full timeout/cancel/partial Git write and concurrent worktrees.

Commands: `cargo fmt --all -- --check`; `cargo test -p egggit`; `cargo test --test git_mutations_integration -- --test-threads=1`; `cargo test -p codegg --test sandbox_containment -- --test-threads=1`; `cargo test -p codegg --test sandbox_landlock -- --test-threads=1`; `python3 scripts/check_git_forbidden_patterns.py`; `python3 scripts/check_execution_ownership.py`; `scripts/verify.sh quick` plus M003–M005 new Linux native tests. Require exact kernel, userns, proc/socket, effective network and filesystem evidence and no skipped required security fixtures.

## 7. Failure and migration

Reject before target if a required dimension unavailable; no automatic FullHost or alternate Git subprocess. If user explicitly requests a higher-authority human operation, it is a *separately admitted action* with new provenance and no automatic agent promotion. Roll back partial Git metadata changes safely without discarding unrelated concurrent state. No persisted schema change by default; add versioned receipt/outcome fields if required.

## 8. Acceptance

All legitimate **local** mutation integration tests pass when run under an actually obtained isolated child or demonstrably non-executable trusted route. Repository-controlled filters/hooks run only inside the containment envelope and cannot reach host resources or higher-authority credentials; daemon Git metadata transaction remains trustworthy and scoped. No invisible command rephrasing or subprocess fallback obtains more authority. Native proof and failure matrix in C002 closure.

## 9. Stop

Stop if general .git write access is required to make the workflow pass, the agent can modify repo executable settings that a trusted higher-authority call will execute, a host socket/FD/network path survives, or no supported native host can prove enforced isolation. Keep operation typed-unavailable rather than claiming closure.
