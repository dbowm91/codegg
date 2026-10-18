# Linux Trusted Git Cost Corrective Roadmap

Status: active planning — C001/C003 are bounded test-only handoffs; C002/C004/C005 gated; **no new production Git backend admitted**

Parent: `plans/subsystems/linux-execution-isolation-roadmap.md`. Corrects C010 no-admission (`plans/closure/linux-execution-isolation/018-status.md`) and C012's closed-negative no-writer/live-mount finding (`020-status.md`) **without rewriting them**. M004 closed only on its qualified host (`004-status.md`); M005 offline-network remains internal candidate. Linux C002 contained local Git and C003 mediated remote Git remain separate authoritative acceptance paths.

Normative authority: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8; `plans/002-long-term-roadmap.md`; `plans/003-planning-process.md`. Accepted ADR-0004, ADR-0016, ADR-0034, ADR-0035. Proposed **ADR-0036**, not accepted, gates any replacement of immutable trusted Git with live-object reads or production private promotion.

## 1. Purpose and ownership

Reduce trusted Git read setup from ODB-sized snapshots to work proportional to the requested operation, while retaining daemon-owned authorization and actual OS-enforced containment. `egggit::process` owns Git argv/environment construction; `GitExecutionService` and `GitMutationExecutor` own typed results and mutations; `ManagedProcessService`/trusted Linux helper own launch receipts and child trees; Codegg-owned worktree leases coordinate only cooperating processes. No second daemon, shell parser as authority, or unreviewed direct spawn path.

## 2. Current evidence and defects

Current source review is post-rebase commit `0b36c71f`, reconciled from pre-rebase evidence commit `004d770b8cd23ca33fb724dae95f97de563ba60d` (2026-10-18). At that reviewed source baseline, `GitEnvPolicy::{apply,apply_sync}` prepares a private ODB snapshot for object-reading commands. Cache is 4 entries/2 GiB with inotify/fingerprint invalidation and global mutex during miss construction. C012 verified CoW integrity on Btrfs but fragmented cold p95 1,109.391 ms exceeds C010's 1,000 ms limit. C012 **rejected** a live view claiming snapshot equivalence because external writers are not excluded. M004 process/IPC/PTY/LSP tests passed on a hosted Linux configuration; M005 offline candidate has native denial tests but no production owner/policy admission. Historical M001 Git mutation failures and C002/C003 remain open. Some full CI Git object-view/alias tests have failed intermittently; do not relabel them transient.

## 3. Security invariants and non-goals

A confined Git worker cannot read host credentials or private daemon data, signal host processes, use host IPC, talk to host/external network in an offline-required tier, write protected gitdir/common-dir outside an expressly brokered operation, or elevate via hook/filter/config/extension. Parent/child ceilings and typed `DestructiveRepositoryChange` remain. An untrusted live ODB read is **not** an immutable snapshot; external-write consistency is not falsely guaranteed. No default policy change, no universal persistent object mirror, no Git daemon replacement, no mandatory gix dependency, no host-global lock, no remote-credential/SSH feature in this corrective, no rewriting C010/C012 closures or superseding ADR-0035.

## 4. Target decision boundary

Proposed ADR-0036 separates **live contained data reads** (untrusted, possibly inconsistent) from **immutable generation reads** (current snapshot guarantees). Contained workers read existing objects read-only and use operation-private writable index/new-objects as needed. A narrowly authorized publisher validates effects and uses Git-compatible locking/ref CAS for output. Native gix can implement selected reads only if isolation, configuration trust, dependency and parity tests justify it. No direct unsafe in-daemon parsing of arbitrary repo data is automatically approved.

## 5. Dependency graph

```text
C001 threat model + test-only denial harness ───┬──> C002 contained Git read pilot
                                                │      [ADR-0036 acceptance for promotion;
                                                │       M004 native, M005 obtained offline]
C003 Git CLI vs gix feasibility (parallel) ──────┤
                                                └──> C004 private-index/ODB promotion pilot
                                                       [ADR-0036 acceptance; M005 gate]
C002 + C004 + C003 findings + Linux C002/M005 ─────> C005 parity/admission handoff
                                                         │
                                                         ├──> Linux C002 local Git closure
                                                         ├──> M003 revised admission review
                                                         └──> M008 rollout (plus other existing gates)
Linux C003 remote transport retains its independent M006/credential gate.
```

A plan may use an **interface** contract or a test-only harness before a hard production dependency closes; this is not authority to dispatch a new backend. C001 is conditionally closed for test-only host-authority evidence; its closure leaves consistency and metadata-expansion limits explicit. C003 is closed with a negative gix decision for status. C002 and C004 may proceed as test-only pilots against those limits; production activation requires ADR-0036 acceptance, full obtained M004/M005 guarantees and independent admission. C004 promotion additionally waits on broker safety tests. Existing Linux C002 remains the authoritative positive local-mutation exit; this corrective supplies prerequisites and must not claim to close it automatically.

## 6. Milestones and handoffs

| Milestone | Class | Plan | Status | Gate / exit |
|---|---|---|---|---|
| C001 | invariant | `plans/archive/implementation/linux-trusted-git-cost-corrective/001-live-odb-trust-model-and-negative-probes.md` | **conditionally closed, test-only** | Closure `plans/closure/linux-trusted-git-cost-corrective/001-status.md`; hosted alias/diff probes establish the tested authority boundary; generation consistency and external metadata remain unresolved |
| C002 | capability | `plans/implementation/linux-trusted-git-cost-corrective/002-contained-live-odb-read-pilot.md` | **in progress, test-only; production blocked** | Linux M004 run `39135657937` passed on `515023dc`; full suite included C002 seven-operation parity and C004 interrupted-index recovery, PTY/LSP, protected-mount/Git, and restricted-userns refusal. Repeat end-to-end run `39139527534` on `9c75a1f7` passed 30 cache-drop-advised/100 warm `git log -1` samples: contained-live p95 82.628/72.868 ms cold-advised and 17.641/32.019 ms warm; immutable-snapshot p95 1,060.944/3,695.070 ms on ext2/ext3. Exact-head run `39147231320` on `950873ca` reproduced the cost shape: live p95 49.848/26.599 ms cold-advised and 16.517/27.609 ms warm; snapshot p95 1,160.407/1,816.656 ms cold-advised. Btrfs CoW run `39139475216` passed source-mutation isolation and measured private-view preparation p95 0.411 ms packed / 1,867.715 ms fragmented, so fragmented CoW still exceeds the 1,000 ms comparison. These remain single-operation exploratory results; DONTNEED is advisory, and resource/contention/lifecycle coverage and backend admission remain open. Full CI runs `39139511878` and `39141276739` stopped in audit fixtures without explicit workspace roots; `39142997301` reached 7,963/14,470 then stopped in LSP fixtures missing roots/files; `39145202171` passed audit/LSP fixtures but reached 9,380/14,470 with 9,377 passing before three Bash tests in `tool_execution` failed for missing workspace roots. Audit modules pass 19/19 and 4/4, LSP 164/164, and `tool_execution` 61/61 locally after binding fixtures to explicit temp roots; `verify.sh quick` passes. Exact-head CI for `950873ca` is running. C010 no-admission and C012's negative conclusion remain unchanged. |
| C003 | infrastructure | `plans/archive/implementation/linux-trusted-git-cost-corrective/003-gix-versus-cli-benchmark.md` | **closed (negative gix decision)** | Closure `plans/closure/linux-trusted-git-cost-corrective/003-status.md`; gix deferred after packed/fragmented status comparison; production snapshot route unchanged |
| C004 | invariant | `plans/implementation/linux-trusted-git-cost-corrective/004-private-index-odb-promotion-prototype.md` | **active, test-only staging/promotion prototype; production publication blocked** | Full Linux M004 workflow `39115964190` passed on `7a5c79c9` with private staging and protected source gitdir/ODB identity checks; prior run `39113624242` passed index-conflict rejection, object import, and locked index replacement. Exact-head hosted run `39132604511` on `5d190312` passed the interruption model; run `39147222328` on `950873ca` completed successfully, including cross-process C004 recovery, PTY owner/routing and descendant cleanup, constrained LSP duplex/procfs, protected-mount/Git regressions, and restricted-userns refusal. This remains process-death simulation, not production daemon recovery. Path-race, arbitrary-writer, cancellation and multi-resource proofs remain unimplemented; production promotion requires ADR-0036 and broker safety proof |
| C005 | capability | `plans/implementation/linux-trusted-git-cost-corrective/005-local-git-recovery-and-admission.md` | **blocked on C002/C004, Linux M005 and ADR acceptance** | Positive local Git parity, independent security review and C010/M003/C002 handoffs; no implicit release admission |

Implementation documents are bounded independently; their future closures go to `plans/closure/linux-trusted-git-cost-corrective/00N-status.md`. No closure documents are created during planning.

## 7. Cross-cutting concerns

Policy/storage/protocol: retain snapshot path, Landlock defaults and existing typed DTOs; version new public or persisted consistency/obtained rights before enabling. Private ODB/index scratch must be owned, quota-limited, cancellation/restart cleaned and scoped to job/worktree. Coordination uses repo identity, real gitdir/common-dir, Git locks and expected-value ref transactions; no claim that `WorkspaceLockTable` prevents external edits.

Security: configured hooks and other executable Git configs may evolve by Git version. Test actual supported installed Git versions and do not rely solely on `core.hooksPath=/dev/null`; escaping via Git aliases, config includeIf, replace refs, alternates, partial clone/promisor remotes, external gitdir and object-pack indexes must be considered. On a host lacking required namespace/ABI/network guarantees, fail before exec. Never report M005 offline enforcement based on a candidate flag alone.

Performance: benchmark paired identical operations on packed and >=4,000-loose-object/~100-MiB stores, ext4 and optional CoW host, actual native Git baseline, old snapshot route, proposed contained CLI, and optional gix. Sample >=100 warm and >=30 cold per shape where meaningful, with p50/p95, CPU/RSS, bytes copied, setup time, cache invalidation, and multi-agent contention. Preserve C010 thresholds as comparators; a new latency threshold/change requires explicit admission decision, not quietly changing prior results.

Operator/compatibility: unsupported live route returns typed unavailable, never FullHost. Preserve Windows/macOS semantics and ordinary human-shell authority. Remote Git stays C003/M006. No SBC startup/build benchmark.

## 8. Verification and closure

Required: focused unit/property/adversarial tests, positive and negative native Linux jobs with exact kernel/ABI/bwrap/LSM/userns facts and no-target marker on failure, corrupted/volatile pack and alternate fixtures, concurrent external git-gc/repack/index/ref writes, access-denial markers for secrets/host IPC and TCP/UDP/DNS, child cleanup, worktree affinity, and trusted mutation publication integrity. Run `cargo fmt --all -- --check`, `python3 scripts/check_adr_integrity.py`, `python3 scripts/check_execution_ownership.py`, `python3 scripts/check_sandbox_contract.py`, `python3 scripts/check_git_forbidden_patterns.py`, `scripts/verify.sh quick`, focused tests and exact-head hosted CI. Failures and skips recorded separately, no blanket release claims.

## 9. Decisions, stop conditions, completion

C001 may prove only candidate behavior; ADR-0036 remains proposed until independently ratified. Reject no-writer-lease-as-snapshot, unsandboxed live-ODB fallback, arbitrary broker host Git, wholesale gitdir write grants, unsafe hardlinks, claims that a read-only mount is immutable, and unmeasured snapshot-free performance. If none of the pilots can reproduce required security or normal Git correctness, retain snapshots and record negative closure. C005 cannot strictly close without independent decision/host admission; downstream M003/C002/C003/M008 statuses remain unchanged until their own closures.
