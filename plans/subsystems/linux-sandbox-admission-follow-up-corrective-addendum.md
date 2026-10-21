# Linux Sandbox Admission — Post-Review Corrective Addendum

Status: active planning; C001–C003 **ready for bounded handoff**; no production promotion authorized
Planning baseline: `planning/linux-sandbox-next-admission-and-adr-cleanup` at `d251b11cd7fa5b89ca008e45d80a28a4ac3aea9f` (2026-10-20).
Primary owner: Linux execution-isolation roadmap. This is a corrective **coordination addendum**, not a competing implementation of existing Linux M003/M004/M005 or trusted-Git C005.

Canonical sources: `plans/000-long-term-specification.md` §§4, 17, 27, 29; `plans/001-terminology-and-domain-model.md` §8; `plans/003-planning-process.md` §§4–10; accepted ADR-0004/0016/0042/0043; **proposed (unaccepted) ADR-0044**. Refer to `plans/subsystems/linux-execution-isolation-roadmap.md` and `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md`.

## 1. Purpose and ownership

Restore a truthful, reviewable Linux admission sequence after a long-running branch accumulated production candidates, qualified but not admitted Linux components, and unresolved Git consistency/performance decisions. Do **not** reopen M004 or C007/C014/C023: preserve their exact native evidence and explicitly prove only remaining gaps. Separate infrastructure/probes, operator decisions, and release admission. Keep the Landlock-only legacy path available and accurately labeled.

`ManagedProcessService`/scheduler own process authority, receipts, lease and cleanup; `GitExecutionService` / `GitMutationExecutor` own typed Git operation authority; `GitIndexPublisher` owns the already-closed private publication vertical. This addendum owns no new executor, proxy, persistent setting or alternative ADR.

## 2. Findings that justify a corrective pass

1. **Review/integration evidence drift.** The active Linux branch had diverged from `main` by 295 commits ahead/90 behind at review, includes substantial non-Linux change overlap, and its final `d251b11c` source does not have a branch-head all-green full test record. An authoritative quiescent `nextest` run recorded **14,877 tests, 14,874 passed, three timed out, seven skipped**; an earlier sweep failed fourteen Git-routing/security-review tests sharing repository-cwd assumptions, and the root cause remains unestablished. Native hosted Linux run `39356728579` and macOS compile `39356728506` passed at source `862ddd14`. CI evidence is not release admission. The branch also contains `plans/.mutate-probe`: determine if it is intentional before disposition.
2. **M005 remaining effective-route gap.** C014 closed the discovered `JobPayload::{Shell,ManagedArgv}` intent/ceiling loss and its native owner proof; C023 closed per-launch obtained-network evidence (including audit and C007 receipt). The **Tool Program child-job** route remains unqualified, and no machine-readable reviewed supported-host admission policy exists. `OfflineRequired` is opt-in, not a default upgrade; the private network namespace is tested but M005 remains active.
3. **Protected-view/contained Git gate.** M003/C010 retains **no production host admission**: C012's immutable-snapshot cold fragmented p95 of 1,109.391 ms misses the 1,000 ms budget and a process-local no-writer lock cannot exclude external Git writers. The contained-live C002 pilot improves latency but is test-only. C007 has since delivered a production private index publisher, CAS lock/journal and startup reconciliation; do not describe it as absent. C005 remains blocked on typed consistency, hostile metadata mediation, live-read consumer admissibility, explicit ADR-0044 decision, complete parity/security evidence and independent M003/C010 admission. Remote Git C003 also needs M006 transport.
4. **Stale milestone descriptions.** The M005 source plan/roadmap retain historical sentences asserting the obtained network fact is missing; `plans/closure/linux-execution-isolation/023-status.md` is the later authoritative closure. Update current-state labels by links/addenda, never alter historical evidence retroactively.

## 3. Invariants and exclusions

- An authenticated principal and immutable parent/child capability ceilings, not tool name, language, shell syntax, job storage or environment, grant execution authority.
- Required filesystem, namespace, network, process/IPC and syscall boundaries must be **obtained before target**; unavailable is a typed fail-closed outcome, never FullHost/legacy reroute. `NetworkEnforcement::Enforced` is sourced only from an observed launch, not a host probe, permission profile or offline request.
- Preserve legacy Landlock-only/default unrestricted behavior; never claim generic host confidentiality, offline networking, cgroup quotas or full Linux distribution coverage from Landlock or a feature probe.
- A host's feature discovery is not native attack-denial evidence or accepted **host policy**. Production host admission must be independently recorded with kernel/LSM/toolchain versions and negative cases.
- Preserve M004 and C007/C014/C023 closures without gratuitous rewrites. C010's no-admission and C012's no-writer rejection remain authoritative. Proposed ADR-0044 grants no production authority.
- A mutable/live Git read, a captured but potentially stale generation, and a truly immutable generation must not be interchangeable. Do not allow untrusted hooks/filters/include files or Git helpers to elevate to daemon authority. Keep `DestructiveRepositoryChange` and credential/transport grants explicit.
- No broad root helper, global sysctl/firewall change, second executor, new default, remote Git reopening, or unnecessary cgroup/proxy work in this corrective.
- Existing non-Linux targets and clean per-attempt scratch/cancellation semantics must not regress.

## 4. Corrective milestone decomposition

| Package | Class / status | Plan | Hard dependency | Exit |
|---|---|---|---|---|
| **C001** | infrastructure; **ready** | `plans/implementation/linux-sandbox-admission-follow-up-corrective/001-branch-ci-evidence-stabilization.md` | Current branch and `main` readable | Clean reproducible integration candidate + non-vacuous CI/authoritative failure report; no merge/default promotion |
| **C002** | invariant; **ready** | `plans/implementation/linux-sandbox-admission-follow-up-corrective/002-m005-delegated-owner-and-host-admission.md` | Closed M004/C014/C023 interfaces | Tool Program/delegation owner's actual intent + obtained boundary qualified; host-support matrix machine-readable, but rollout separate |
| **C003** | invariant; **ready for non-promoting interface/review only** | `plans/implementation/linux-sandbox-admission-follow-up-corrective/003-git-live-read-consistency-and-metadata-review.md` | Closed C007 publisher; existing C002 test pilot; proposed ADR-0044 | Typed Git consistency/consumer fence, hostile metadata negatives, ADR decision packet; no production live route |

**Integration dependency:** C001 is operational/soft for isolated C002/C003 implementation but **hard for any independent production admission**. C002 and C003 can proceed in separate worktrees. C003 implementation of a production live-read route is **hard-blocked by separate maintainer acceptance of ADR-0044**, and positive Git C005/parent Linux C002 closure also requires its own entire parity/admission matrix. C002 cannot silently close parent M005: parent closure requires explicit supported-host and all-owner evidence and review. No newly invented C004 duplicates Git C005 or M003/C010; they remain the downstream admission owners.

Dependency sketch:

```text
existing M004 + C014 + C023 ------> C002 (M005 owner/host closure evidence) ----+
                                                                              |
C001 (integration + CI integrity) --------------------------------------------+--> independent M005 / M003-C010 decisions
                                                                              |                   |
existing C007 + test-only Git pilot --> C003 (typed evidence + ADR packet) ----+----> ADR-0044 owner decision
                                                                                                    |
                                                                                          existing Git C005
                                                                                                    |
                                                                                            Linux C002 / M008
```

## 5. Handoff/verification discipline

Each agent receives **one** C00x plan, inspects the actual head and freezes source/runner identity before editing. A completed code change without native hostile-negative positives and anti-vacuity controls is not security closure. Require exact host kernel, Landlock ABI, bubblewrap identity, AppArmor/SELinux/userns condition, target executable identity, allowed control and denied hostile effect, and obtained outcome. A zero-match filtered test, skipped native test or host probe alone is **not** passing security evidence.

Focused suites and static guards are in the individual plans. Integration review additionally runs `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked`, `scripts/verify.sh quick`, the relevant native Linux workflows (M004/M005/C006) and hosted macOS target compile. Use `cargo nextest run --workspace --locked --profile ci --no-fail-fast` for branch-wide evidence; report all failures/timeouts/skips, separately from focused native pass results. Keep timeout masking/retries from becoming security proof.

For each package, write `plans/closure/linux-sandbox-admission-follow-up-corrective/00x-status.md` with exact commit and artifact/test URLs, pass/fail/skips, requirement-to-evidence matrix, security/rollback and migration effects, unresolved severity, and whether the closure is closed/conditional/blocked. Update `plans/registry.md` and both parent roadmaps with **brief links and truthful admission status**. Do not reword earlier no-admission or qualified-closure records as if they never existed.

## 6. Release/admission boundary

The review packet may recommend narrowly supported opt-in Linux tiers, but no plan in this addendum accepts ADR-0044, admits M003/C010, enables general production Bubblewrap, changes `LegacyUnrestricted` defaults, or certifies remote Git/proxy/cgroup limits. M005 may independently close only after C002 evidence plus parent-gate review; Git C005 may proceed only under a separately accepted ADR-0044 with its own parity matrix. A release must have a maintainer-signed, exact-revision supported-host/admission/rollback record under the existing M003/C010/M008 authorities.

**Stop/blocked outcome is correct** if authority leaks, race safety is unproved, runner security evidence is vacuous, or required host features cannot be demonstrated.
