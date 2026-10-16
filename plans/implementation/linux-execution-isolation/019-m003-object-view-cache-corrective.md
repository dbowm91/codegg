# Linux Execution Isolation C011 — Trusted Git Object View Cost Corrective

Status: implemented; conditionally closed with cold-fragmented production performance still blocked
Repository baseline: `4cd4068a` (current reconciled Linux isolation branch)

Source roadmap: `plans/subsystems/linux-execution-isolation-roadmap.md`
Long-term requirements: `plans/000-long-term-specification.md` §§4, 17, 27, 29; `plans/001-terminology-and-domain-model.md` §8.
Applicable ADRs: accepted ADR-0035 and ADR-0034; ADR-0001, ADR-0002, ADR-0004, ADR-0016.
Primary class: capability / polish / security invariant

## 1. Objective

Replace per-command full Git object-store copying with a bounded daemon-owned cache of immutable private object views. Preserve protection against repository alternates created after admission and source-object mutation after a view is admitted. Qualify the repeated-command path on packed and fragmented repositories. This is an architecture corrective and candidate qualification; it does not admit Linux M003 for production or unblock object-writing Git mutations.

## 2. Why this milestone is ready

C010 recorded no production admission because private-view performance was impractical or unqualified. The current `egggit::process::prepare_private_object_view` makes a new sibling temporary object store for every object-reading command, reflinking where supported and copying all admitted loose/pack files otherwise. The private snapshot tests define the required immutability and late-alternate guarantees. Repeated trusted Git operations are the affected routine path.

## 3. Current implementation evidence

- `crates/egggit/src/process.rs` owns trusted Git environment construction, object-directory admission, per-command private snapshot creation, and its lifetime through child completion.
- Existing Linux traversal uses descriptor-relative no-follow opens, admits only regular files in Git loose-object fanout and pack namespaces, excludes `objects/info`, and bounds the entry count.
- Existing tests cover alternate refusal and late alternate creation, source-object mutation after admission, ordinary and linked-worktree layouts, and lifecycle cleanup.
- C010 closure `plans/closure/linux-execution-isolation/018-status.md` records contradictory exploratory measurements and a no-admission decision. Those results remain historical and are not overwritten.
- Initial implementation replaces per-command view destruction with a process-shared, bounded cache: four entries / 2 GiB maximum, immutable snapshot leases held by each Git child, Linux inotify invalidation for object namespace changes and queue/watch loss, and SHA-256 inventory fallback where notifications are unavailable. Cache shutdown is wired to `CoreDaemon` drop; active command leases keep their root alive until child completion.
- Local Linux correctness suite passed `cargo test -p egggit --locked -- --test-threads=1` (108 passed, 1 ignored). Cache reuse/invalidation and eight concurrent callers have focused regression coverage. Windows GNU and macOS test-target checks compile; existing warnings only on Windows.
- Exploratory ext4 measurements on the current mixed repository (4,875 loose objects / 43 packs; 44,692 KiB loose and 77,367 KiB packed) with 100 warm samples measured preparation median/p95/max 0.041/0.060/0.068 ms; protected `git status` p95 was 10.674 ms versus 10.385 ms paired baseline. Thirty cache-miss samples after per-file `POSIX_FADV_DONTNEED` measured preparation median/p95/max 3.689/4.482/7.637 s. This is best-effort file-page eviction, not global cache flushing. The cold fragmented gate fails.
- A packed comparison store was built from a copy of the same shallow repository metadata, then repacked to one regular pack plus one cruft pack (52,369 objects, 33,761 KiB total). Its object enumeration contains 249 fewer unreachable objects than the mixed source (52,618), so the pair is not exact-content-equivalent. On the packed copy, 100 warm samples measured preparation median/p95/max 0.040/0.057/0.062 ms and protected `git status` p95 15.947 ms versus 16.243 ms baseline. Thirty cold misses measured preparation median/p95/max 719.272/845.980/900.675 ms. Packed shape meets the preparation budget on this host; this remains candidate evidence only.

## 4. Invariants

- A running Git child reads only an immutable private object view with no `objects/info` metadata.
- A view is keyed to the canonical selected object directory and a collision-resistant fingerprint of the admitted object namespace; cache hits may not cross repositories or reuse a view after an observed object namespace change.
- Source files are never hard-linked. Cached views are daemon-owned, private, immutable after creation, bounded by entry count and total bytes, and reclaimed after eviction once no child holds a lease.
- Concurrent source changes during validation or copying fail closed or produce a self-contained snapshot; no candidate path may follow a symlink or external alternate.
- Git command argv, worktree identity, config isolation, and the typed trusted-read allowlist remain unchanged.
- No production backend, default, persisted preference, or Git mutation/remote route is enabled by this work.

## 5. Scope

### In scope

- Add a daemon-lifetime, bounded cache of private object snapshots with leases held through child completion.
- Validate cache reuse against canonical object-directory identity and a collision-resistant inventory fingerprint which includes relevant file/directory identity, size, and change-time metadata. On Linux, a watched generation can skip repeated full inventory scans; any event invalidates it permanently. Watch setup or event-read failure falls back to fingerprint validation, never to mutable source objects.
- Make concurrent cache misses single-flight per fingerprint; enforce byte/entry bounds and safe eviction without removing active views.
- Add regression tests for reuse, invalidation after loose-object add/replace, packed-store changes, concurrent callers, active-lease eviction, late alternates, source mutation, and bounded cleanup.
- Extend the existing opt-in performance harness to report cache-hit validation cost, cache-miss preparation cost, protected `git status`, and paired baseline separately for packed and fragmented stores.
- Run the documented 100 warm / 30 cold paired protocol where the host allows credible cold-cache control; label unavailable cold control unqualified.

### Out of scope

- Weakening the private-view trust boundary, using hard links, trusting mutable cross-command object contents, or enabling Git alternates.
- Changing Git command policy, trusted repository selection, local Git mutation/remote routes, Linux production admission, Landlock defaults, or accepted ADRs.
- PTY/LSP integration, M004, M005 production policy, cgroups, or broad remote Git mediation.

## 6. Ordered work packages

1. Implement the bounded immutable snapshot cache and lease lifetime in `egggit` without changing callers' command-construction contract.
2. Prove reuse/invalidation/races, repository isolation, active-child lifetime, and cache bounds with deterministic tests.
3. Extend the manual performance harness to distinguish cache validation/hit, cold miss, warm miss, end-to-end status and paired baseline.
4. Run focused egggit tests, the Git policy guard, full relevant hosted trusted-Git workflow, and repeated packed/fragmented qualification on the exact tested commit.
5. Record whether the cache materially reduces routine repeated-call overhead. Keep the C010 no-admission decision in force; update the Linux roadmap and registry only with exact evidence.

## 7. Failure and recovery semantics

Unverifiable source metadata, cache lock poisoning, fingerprint failure, namespace races, or private-view creation failure return the existing typed `BackendUnavailable` disposition before Git spawn. Cache pressure must never trigger an unprotected Git fallback. Eviction removes only the cache's lease; a running child retains its own view. Daemon shutdown releases all cached views after active commands release theirs.

## 8. Compatibility and migration

No public DTO, storage, or migration change. Existing read-only typed Git routes keep the same result contract. Cache state is ephemeral and daemon-local; restart rebuilds it from the selected object stores.

## 9. Required verification

```bash
cargo fmt --all -- --check
cargo test -p egggit --locked -- --test-threads=1
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_sandbox_contract.py
scripts/verify.sh quick
```

Exact-head hosted Linux trusted-Git tests are required for native confidence. Performance measurements must record host/kernel/filesystem, repository shape and size, cache state, sample count, median, p95, and maximum. No cold qualification may be inferred from warm samples.

## 10. Acceptance criteria

- Existing late-alternate and source-mutation guarantees remain green.
- Cache reuse materially avoids repeated full object payload copying while leaving each Git child on an immutable private view.
- Tests prove invalidation, isolation, bounds, concurrent use, active-child lease retention, and cleanup.
- Performance evidence reports cache-hit and cache-miss costs against packed and fragmented stores; cold-cache limitations are explicit.
- C010 remains no-admission unless its full production thresholds and host/tool compatibility matrix are separately met; this corrective cannot itself mark M003 production admitted.

## 11. Stop conditions

Stop if correctness requires mutable shared object files, hard links, an unbounded cache, or trusting repository-controlled alternates. If fingerprint validation approaches the full-copy cost, report the design as ineffective and propose a stronger execution-boundary design rather than weakening validation.

## 12. Closure evidence required

Create `plans/closure/linux-execution-isolation/019-status.md` with exact code SHA, test and hosted run IDs, cache bounds and lifecycle results, packed/fragmented hit and miss measurements, cold-cache limitations, explicit M003 admission disposition, and any remaining gates. Update the registry and subsystem roadmap in the same closure change.
