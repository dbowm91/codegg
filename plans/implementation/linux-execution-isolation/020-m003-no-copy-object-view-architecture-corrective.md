# Linux Execution Isolation — M003 No-Copy Git Object View Corrective

Status: active; design and implementation work has not yet changed C010 admission

Repository baseline: `f7935ad7` (reconciled with current `main`)

Source roadmap: `plans/subsystems/linux-execution-isolation-roadmap.md`
Related decisions: accepted ADR-0035 (Linux isolation), ADR-0034 (command authorization); C010 no-admission closure `plans/closure/linux-execution-isolation/018-status.md`; C011 warm-cache corrective `plans/closure/linux-execution-isolation/019-status.md`.

## 1. Objective

Replace or materially optimize the cold-miss path in the daemon-owned private Git object-view architecture. Preserve the current trusted Git guarantees while removing the multi-second fragmented-store preparation cost that keeps M003 unadmitted. This work is an architecture corrective; it cannot itself admit a host or enable object-writing or remote Git routes.

## 2. Current evidence and design constraint

C011 added a bounded daemon-lifetime cache of immutable private snapshots. Warm preparation is sub-millisecond for the measured packed and fragmented repositories, but cold-advised misses measured 3.689 s median / 4.482 s p95 / 7.637 s max for the fragmented store. C010's no-admission decision remains in force.

An explicit environment redirect is not enough to safely avoid copying: a local probe confirmed that Git recursively follows the selected object store's own `objects/info/alternates` when that object store is supplied through `GIT_ALTERNATE_OBJECT_DIRECTORIES`. Any no-copy design must prevent a repository-controlled late alternate from influencing the child at the process boundary.

The first process-boundary candidate to prototype is a helper-owned private mount namespace that presents the daemon-selected repository and object directory read-only and masks only its `objects/info/alternates` entry with an empty read-only file. Bubblewrap 0.9.0 on the local environment supports `--ro-bind-data`; that is an option-availability fact, not proof of a correct mount view. The prototype must pin the real gitdir/common-dir, refuse an existing alternate before launch, and establish whether source-object mutation can be excluded by the selected M003 profile or still requires a snapshot.

The candidate must also preserve selected repository identity across nested `.git` creation and linked-worktree/common-dir layouts; reject pre-existing unsupported alternates; avoid source hard links; retain trusted argv/config overrides; and define what concurrent source-object mutation means for each supported launch profile.

## 3. Work packages

1. Review the threat boundary for trusted Git under FullHost and constrained profiles. Establish whether M003's protected metadata/object mounts can be a prerequisite for a shared read-only object view, or whether independent snapshot immutability remains required.
2. Compare no-copy process-boundary candidates, including a trusted mount namespace that masks `objects/info/alternates`, filesystem-native copy-on-write snapshots where supported, and bounded selective snapshots. Reject environment-only alternates and hard-link designs unless adversarial probes prove the guarantees.
3. Implement the selected design behind the existing internal opt-in. Keep the cached private-copy path available only as a temporary comparison/fallback where the chosen policy explicitly permits it; never fail open to host object discovery.
4. Preserve and extend adversarial tests for pre-existing and late alternates, nested repositories, linked worktrees/common directories, source mutation, races, packed and fragmented stores, and cleanup after timeout/cancellation.
5. Measure controlled cold and warm preparation plus repeated trusted operations on exact-content-equivalent packed and fragmented stores. Record filesystem, kernel, Git version, repository shape, cache state, sample distributions, and any source-mutation assumptions.
6. Run exact-head native Linux trusted-Git and M004 regression workflows, cross-platform compile checks for changed shared code, ADR integrity, and full CI. Record whether the design meets the C010 thresholds; retain C010's no-admission disposition unless every production gate is met separately.

## 4. Invariants

- A trusted Git child cannot discover a repository, object directory, alternate, worktree, or config outside the daemon-selected canonical repository context.
- Creating nested `.git` metadata after admission cannot redirect an already-authorized daemon Git operation.
- A concurrent object-store mutation cannot cause the child to consume an unvalidated alternate or escape the selected repository's trust boundary.
- No hard links to mutable source object files, no host-Git fallback after required setup failure, and no weakening of typed command authorization.
- Cache and snapshot state remain bounded and lease-safe; cleanup follows child completion, timeout, cancellation, and daemon shutdown.
- No production backend registration, default change, persisted preference, Linux host admission, mutation route, or remote mediation is implied.

## 5. Acceptance criteria

- A documented design decision is backed by executable adversarial evidence for both metadata races and source-object mutation.
- Exact-content-equivalent packed and fragmented repositories are measured with a controlled cold/warm protocol. The selected implementation materially improves cold fragmented preparation and does not regress repeated trusted operation latency beyond the C010 budget.
- Existing trusted-Git behavior, object-view cache lease safety, M004 regression coverage, and Windows/macOS compilation remain intact.
- The implementation and closure record state any filesystem/kernel capability requirements and fail-closed behavior. No C010 production admission is claimed by this corrective.

## 6. Stop conditions

Stop the no-copy path if it depends on Git recursively trusting an alternate file, a mutable hard link, a host-wide mount mutation, or an unqualified helper. If the only safe generic path still requires full copies on supported ext4 hosts, record the measured tradeoff and defer admission rather than removing source-mutation or alternate-race protections.

## 7. Closure record

On completion, create `plans/closure/linux-execution-isolation/020-status.md`, update this plan, the Linux isolation roadmap, and `plans/registry.md`, and perform the unblock audit. State the exact code and CI heads, native and cross-platform evidence, benchmark protocol and results, the chosen mutation/trust model, and whether any downstream plan became newly eligible. C010 remains no-admission unless separately requalified.
