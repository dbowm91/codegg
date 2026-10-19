# Linux Trusted Git Cost Corrective Roadmap

Status: active planning — C001/C003 are bounded test-only handoffs; C002/C004/C005 gated; **no new production Git backend admitted**

Parent: `plans/subsystems/linux-execution-isolation-roadmap.md`. Corrects C010 no-admission (`plans/closure/linux-execution-isolation/018-status.md`) and C012's closed-negative no-writer/live-mount finding (`020-status.md`) **without rewriting them**. M004 closed only on its qualified host (`004-status.md`); M005 offline-network remains internal candidate. Linux C002 contained local Git and C003 mediated remote Git remain separate authoritative acceptance paths.

Normative authority: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8; `plans/002-long-term-roadmap.md`; `plans/003-planning-process.md`. Accepted ADR-0004, ADR-0016, ADR-0034, ADR-0035. Proposed **ADR-0036**, not accepted, gates any replacement of immutable trusted Git with live-object reads or production private promotion.

## 1. Purpose and ownership

Reduce trusted Git read setup from ODB-sized snapshots to work proportional to the requested operation, while retaining daemon-owned authorization and actual OS-enforced containment. `egggit::process` owns Git argv/environment construction; `GitExecutionService` and `GitMutationExecutor` own typed results and mutations; `ManagedProcessService`/trusted Linux helper own launch receipts and child trees; Codegg-owned worktree leases coordinate only cooperating processes. No second daemon, shell parser as authority, or unreviewed direct spawn path.

## 2. Current evidence and defects

Current source review began at post-rebase commit `0b36c71f`, reconciled from pre-rebase evidence commit `004d770b8cd23ca33fb724dae95f97de563ba60d` (2026-10-18). At that reviewed source baseline, `GitEnvPolicy::{apply,apply_sync}` prepares a private ODB snapshot for object-reading commands. Cache is 4 entries/2 GiB with inotify/fingerprint invalidation and global mutex during miss construction. C012 verified CoW integrity on Btrfs but fragmented cold p95 1,109.391 ms exceeds C010's 1,000 ms limit. C012 **rejected** a live view claiming snapshot equivalence because external writers are not excluded. M004 process/IPC/PTY/LSP tests passed on a hosted Linux configuration. **Update (2026-10-19):** M005 now has a revisioned per-principal network-intent preference, `/network` command, immutable snapshot propagation, and finite/Test/Python/PTY/LSP owner wiring. The exact-source hosted owner run is pending; local native execution is unsupported because this host rejects Landlock ABI 6 Signal/AbstractUnixSocket scope. This is implementation progress, not M005 closure/admission. Historical M001 Git mutation failures and C002/C003 remain open. Some full CI Git object-view/alias tests have failed intermittently; do not relabel them transient.

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
| C002 | capability | `plans/implementation/linux-trusted-git-cost-corrective/002-contained-live-odb-read-pilot.md` | **in progress, test-only; production blocked** | Exact-source cost run `39204689018` on `61cf831b` passed both `git log -1` and `git status --short`, with 30 cold-advised/100 warm samples and 30 fan-out 1/4/8 contention batches per operation and fixture. On Git 2.55.0, runner-reported `ext2/ext3`, packed (~105 MiB, one pack) and fragmented (~105 MiB, 4,099 loose objects), native/snapshot/contained-live cold p95 was 73.675/1,610.576/216.953 and 2.137/6,273.641/112.378 ms for log; 77.784/2,327.353/276.292 and 18.898/6,849.921/97.696 ms for status. Contained-live fan-out-8 p95/max was 60.913/68.068 and 100.414/114.549 ms for log; 82.143/87.937 and 115.685/123.294 ms for status. DONTNEED is advisory. Only contained-live RSS was sampled; CPU resolution is 10 ms, and native/snapshot resource cost, setup/copy I/O/retention, distinct-agent contention and arbitrary-writer consistency remain open. C004 owner integration `39200873597` passed. M005 production-owner admission and proposed ADR-0036 remain gates; C010 no-admission and C012 negative conclusion remain unchanged.
| C003 | infrastructure | `plans/archive/implementation/linux-trusted-git-cost-corrective/003-gix-versus-cli-benchmark.md` | **closed (negative gix decision)** | Closure `plans/closure/linux-trusted-git-cost-corrective/003-status.md`; gix deferred after packed/fragmented status comparison; production snapshot route unchanged |
| C004 | invariant | `plans/implementation/linux-trusted-git-cost-corrective/004-private-index-odb-promotion-prototype.md` | **active, test-only staging/promotion prototype; production publication blocked** | Exact-source M004 run `39200873597` on `0fa4e1e5` passed the full hosted matrix. Process/IPC/offline passed 15/15, including private-index staging through feature-gated `GitExecutionService`, typed live-read parity, cancellation/source preservation, and refusal of untyped mutations before path access. PTY live-owner/routing, constrained LSP duplex/procfs, protected-mount/Git (20/20), and restricted-userns refusal checks passed. This proves service-owned contained staging execution only; the method has no ordinary dispatch caller and owns neither the promotion journal nor production index/ref publication. Production broker cancellation/recovery, arbitrary-writer exclusion, concurrent path-swap publication safety, and multi-resource promotion remain open. Production promotion requires accepted ADR-0036 and broker safety proof.
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
