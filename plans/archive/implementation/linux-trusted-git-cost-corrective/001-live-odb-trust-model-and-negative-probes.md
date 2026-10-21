# Linux Trusted Git Cost Corrective C001 — Live-ODB Threat Model and Native Negative Proof

Status: implemented — **conditionally closed for test-only host-authority evidence; no production backend/policy promotion**
Primary class: invariant
Baseline reviewed: `planning/linux-sandbox-next-admission-and-adr-cleanup` at `0b36c71f (post-rebase review baseline; the pre-rebase evidence commit was 004d770b8cd23ca33fb724dae95f97de563ba60d)` (2026-10-18).
Source: `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md`, parent `plans/subsystems/linux-execution-isolation-roadmap.md`.
Normative: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8; accepted ADR-0004/0016/0034/0035; **proposed, not accepted ADR-0036**.
Corrects C010/C012 evaluation questions; does not reopen `plans/closure/linux-execution-isolation/{018,020}-status.md`.
Target future closure: `plans/closure/linux-trusted-git-cost-corrective/001-status.md`.

## Objective / decision to prove

Determine whether reading the host's *live, read-only-mounted* Git object store in a fully contained child can prevent **host authority escalation** despite not offering a consistent immutable object generation. The previous C012 no-writer-lease idea was rejected correctly; **do not claim it works**. Produce a threat/consistency contract and native adversarial evidence sufficient for a proposed ADR-0036 ratification decision, not an enabled read backend.

## Current source evidence / defect

`crates/egggit/src/process.rs` `GitEnvPolicy::{apply,apply_sync}` calls `prepare_private_object_view`; on cold read it fingerprints and snapshots ODB under the process-global `OBJECT_VIEW_CACHE` mutex. C012 Btrfs 44,671-object CoW test confirmed independent snapshots but cold fragmented p95 1,109.391 ms exceeded C010 threshold. C012 rejected live mounts because external Git/writers escape `WorkspaceLockTable`. M004 process/IPC/seccomp is closed with exact-hosted tests; M005 offline network remains candidate-only. Git command suppression includes `core.hooksPath` but evidence for Git-version-dependent configured `hook.*.command`, includeIf, environment and alternate ODB behavior must be assembled, not assumed.

## Invariants and non-goals

- No target executes before demanded filesystem, process/IPC/FD/seccomp and offline-network limits are obtained; test fixture skips must be explicit. No FullHost/host-Git fallback on sandbox failure.
- A read-only mount constrains sandbox *writes*, **not concurrent host mutation**; no frozen-generation/atomic-multi-object claim.
- Git config, hook, filter, fsmonitor, external diff, pager, remote helper, credential helper or workspace-controlled command cannot access host secrets, peer agent session state, daemon socket, SSH/GPG/Docker/DBus paths, or network while `OfflineRequired` is actually applied.
- Read-only proposed scope does not authorize arbitrary Git writes, pack replacement, ref/index changes, configuration rewrites, or operator credential handoff.
- This pass neither changes `GitEnvPolicy` production behavior nor replaces C012's immutable snapshot, adds no default feature or public DTO, and does not draft a production C002 Git-mutation closure.

## Ordered work packages

1. Inventory trust transitions at `GitExecutionService`, `egggit::process`, Git mutation service, `ManagedProcessService`, `linux_mount_view` and helper. Produce a matrix of Git-native execution surfaces: traditional/configured hooks, `core.fsmonitor`, `filter.*` clean/smudge/process, merge driver, `diff.*.command`, textconv, pager/editor, `core.sshCommand`, credential, `GIT_SSH`, `GIT_EXEC_PATH`, `url.*.insteadOf`, config `include`/`includeIf`, aliases, external gitdir/common-dir, replace refs/grafts/shallow/alternates and promisor/lazy-fetch settings. Mark version-dependent behavior as version-dependent.
2. Define an explicit `LiveReadMayChange` candidate result/receipt description (naming non-binding). Distinguish safety property **no host-authority promotion** from correctness property **coherent object generation**, and specify which operations must retry/error when the latter is absent. No `SandboxLaunchOutcome::Enforced` upgrade without actual child setup.
3. Add a dedicated **test-only** contained Git invocation fixture using existing M004 helper and an internal M005 offline-network test configuration, no production selector. Resolve trusted Git executable independent of untrusted PATH; bind worktree, actual gitdir/common-dir and ODB paths, inhibit external alternates and parent discovery or explicitly prove containment of all reachable paths. Preserve existing production snapshot path.
4. Native positive fixtures: `git status`, `log`, `diff`, `show`, linked worktree, nested repo, packed/fragmented ODB while no external mutation. Native negative fixtures: synthetic credential/home/host-proc/socket/Unix abstract/socket FD/network markers; config-hook/filter/driver attempts; hostile alternate/replace refs; symlink/bind alias and late mutation; fail on any leaked marker or uncontained spawn.
5. Race fixtures: external `git gc`/`repack`, pack/index deletion or replacement, loose-object writes and process-crash during child read. Valid results: clean operation or typed inconsistent/unavailable; *never* host authority escalation or a falsely asserted immutable generation. Test repeatability with two concurrent children and independent host writers. Preserve exact command/exit/status provenance without logging secret content.
6. Write the threat matrix in `research/linux-trusted-git-cost-corrective/` and record a precise acceptance recommendation for proposed ADR-0036, including negative outcomes. Do not accept ADR automatically.

## Expected changes, ownership and compatibility

Test-only fixtures and supporting internal test helper may be added near `tests/linux_m004_process_ipc.rs` / `tests/linux_bubblewrap_first_hop.rs`. No new production subprocess owner, no broad integration rewrite, no non-Linux default, no persistent store/protocol/schema changes. Runtime cost baseline only; no optimization claim. All temporary artifacts and test namespaces are private and cleaned on timeout/panic/cancellation.

## Verification

`cargo fmt --all -- --check`; `cargo test --locked -p egggit trusted_git -- --test-threads=1`; `cargo test --locked -p codegg --test sandbox_landlock -- --test-threads=1`; focused new Linux-only native tests on eligible host; `python3 scripts/check_execution_ownership.py`; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_git_forbidden_patterns.py`; `python3 scripts/check_adr_integrity.py`; `scripts/verify.sh quick`. Run exact-revision native positive suite, negative no-userns/refused setup, CI compilation on macOS/Windows where touched. Record kernel, ABI, bwrap version/hash/path, LSM, userns and actual enforced M004/M005 dimensions; no skip counted as proof.

## Closure evidence and stop conditions

Future `001-status.md` must contain exact commits, command/test results, threat-to-test matrix, host facts, disabled-feature refusal, concurrency/cancellation outcomes, explicit immutable-generation limitation and unresolved findings. **Stop** if any Git-controlled code reaches host secrets/network/IPC; if production trusted Git loses immutable semantics without an accepted ADR; if the fixture smuggles host Git execution; if live objects need broad host-root mounts. C001 closure only makes C002 *test-only* eligible; ADR-0036 adoption, M005 production and Linux C002/C003 remain blocked.
