# Linux Execution Isolation C011 — Closure Status

Status: conditionally closed (warm-path cost corrected; cold fragmented misses still fail the production budget)

Source implementation plan:

- `plans/implementation/linux-execution-isolation/019-m003-object-view-cache-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md`

Repository baseline reviewed: `30ee7a4e60050e08fd7699bb6f09d6645a9acdbf`

Implementation commits:

- `5284ad43` — add a bounded daemon-lifetime cache for immutable private Git object views.
- `863537c0` — update current-main terminal-test contract and extend the M005 resolver fixture; no object-view implementation changes.

## 1. Executive finding

C011 is conditionally closed as an architecture corrective. Repeated trusted Git reads now lease bounded immutable snapshots from a daemon-owned cache rather than copying the complete object payload for every command. Warm cache preparation is below one millisecond for both measured repository shapes, and paired protected `git status` latency stayed close to the plain baseline.

Cold-advised cache misses on the fragmented store remain far above the C010 production limits. C010's no-admission decision is unchanged: no Linux host is admitted for production M003, and the cache does not enable object-writing Git routes, change Landlock defaults, or claim production isolation. C011 closes the bounded warm-path corrective; the cold-miss limit remains an M003 production-admission blocker.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Reuse private immutable object views across repeated commands | `cargo test -p egggit --locked trusted_git_object_view_cache_reuses_and_invalidates_repository_generations`; hosted runs [`38925769551`](https://github.com/dbowm91/codegg/actions/runs/38925769551) on `30ee7a4e` and [`38928962604`](https://github.com/dbowm91/codegg/actions/runs/38928962604) on `863537c0` | pass | The first trusted-Git target passed 20/20, including cache reuse/invalidation; the exact later reconciled head passed the expanded target 20/20. |
| Invalidate views after repository object namespace changes and retain safe leases | `cargo test -p egggit --locked -- --test-threads=1`; focused cache tests; hosted trusted-Git regression set | pass | The implementation uses inotify generations on Linux, fingerprint fallback, immutable leases, and bounded eviction. |
| Preserve source mutation and late-alternate protection | Existing egggit immutable-view, alternate, race, and source-mutation tests; hosted trusted-Git regression set | pass | Sources are not hard-linked and object metadata outside the admitted object namespace is not copied into the private view. |
| Bound resident cache memory and lifetime | Code review and cache tests | pass | Four entries / 2 GiB maximum; active Git commands retain an `Arc` lease; daemon shutdown drops the cache owner. |
| Measure warm and cold behavior on packed and fragmented stores | 100 warm and 30 cold-advised samples per shape, recorded in implementation plan §3 | partial | Warm repeated path meets the budget; cold fragmented preparation does not. The packed comparison differs by 249 unreachable objects and is not content-equivalent. |
| Keep production admission and defaults unchanged | C010 closure `plans/closure/linux-execution-isolation/018-status.md`; implementation diff | pass | No M003 admission, persisted preference, backend registration, or Landlock default change. |

## 3. Production implementation evidence

`crates/egggit/src/process.rs` owns a bounded shared cache keyed by canonical selected object directory and the admitted object namespace generation/fingerprint. Cache entries are private snapshots, never hard-linked to source objects, and remain alive while any child holds a lease. Linux inotify invalidates observed namespace changes; watch setup or event-read failure falls back to fingerprint validation. Cache pressure can evict only the cache's reference and cannot trigger an unprotected Git fallback. `CoreDaemon` shutdown releases the cache owner.

Warm measurements on the mixed store (4,875 loose objects, 43 packs, 119 MiB) reported preparation median/p95/max of 0.041/0.060/0.068 ms across 100 samples; protected `git status` p95 was 10.674 ms versus 10.385 ms baseline. On the packed comparison (52,369 enumerated objects, one regular pack and one cruft pack, 33.761 MiB), warm preparation median/p95/max was 0.040/0.057/0.062 ms and protected status p95 was 15.947 ms versus 16.243 ms baseline. The packed repository contains 249 fewer unreachable objects than the mixed source, so these two shapes are not an exact-content pair.

Thirty cold-advised misses measured 3.689/4.482/7.637 s median/p95/max on the fragmented store and 719/846/901 ms on the packed comparison. The cold fragmented result fails the 1 s p95 and 3 s per-sample production gates. Per-file `POSIX_FADV_DONTNEED` is only best-effort page-cache advice, not a global cold-cache guarantee.

## 4. Verification executed

### Commands run

```bash
cargo test -p egggit --locked -- --test-threads=1
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_sandbox_contract.py
scripts/verify.sh quick
```

### Results

- Local egggit suite: 108 passed, 1 ignored.
- Local `scripts/verify.sh quick`: passed on the reconciled implementation branch.
- Hosted C006 qualification run `38925769551`, exact implementation SHA `30ee7a4e`: protected-mount tests 11/11; trusted-Git tests 20/20, including `trusted_git_object_view_cache_reuses_and_invalidates_repository_generations`.
- Exact reconciled-head run `38928962604` on `863537c0`: trusted-Git tests 20/20, protected-mount tests 11/11, and M004/M005 native tests 9/9.
- Separate performance harness: 100 warm and 30 cold-advised samples for each recorded repository shape; raw sample ranges and summary statistics are recorded in plan 019 §3.

## 5. Invariant review

- Running Git commands read only a private immutable object view with no copied `objects/info` metadata.
- The cache key remains bound to the selected canonical object directory and its admitted namespace generation; observed mutation invalidates reuse.
- Source files are not hard-linked, mutable cross-command contents are not trusted, and candidate setup failure never falls back to host objects.
- Active children retain their view through completion; daemon shutdown and bounded eviction release only cache-owned references.
- Trusted Git argv, worktree identity, configuration isolation, and typed read policy are unchanged.
- C010 no-admission, Landlock default, and the absence of Git mutation/remote routes remain unchanged.

## 6. Failure and recovery review

Fingerprint or watch failure, cache poison, source race, or view creation failure returns the existing typed unavailable result before Git spawn. Cache eviction does not unlink a view held by a running child. Cache state is ephemeral; daemon restart rebuilds it from the selected object store. No durable state or migration was added.

## 7. Migration and compatibility review

No public DTO, protocol, configuration, or storage format changed. Cache state is process-local and rebuilt after restart. Existing typed trusted Git reads preserve their result contract. C011 does not qualify object-writing Git or remote transport.

## 8. Security review

The cache reuses private snapshots rather than shared mutable object files. It retains race-safe descriptor-relative traversal, no-follow opens, admitted namespace limits, and the late-alternate/source-mutation guarantees. The cache is bounded by four entries and 2 GiB; eviction is lease-safe. Performance pressure does not authorize reading mutable source objects or bypassing the trusted Git path.

## 9. Documentation and operations

Updated implementation plan 019 with warm/cold packed and fragmented measurements and the explicit cold-fragmented failure. The subsystem roadmap and registry now show C011 as conditionally closed while retaining C010's no-admission decision and M003's production blockers.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Cold-advised fragmented cache misses measured 3.689 s median, 4.482 s p95, and 7.637 s maximum. | A cold or invalidated object view still exceeds the C010 production budget, so routine cold-start performance is not admitted. | Keep M003 production blocked. Any new object-view design must meet C010's controlled packed/fragmented thresholds with exact-content comparisons before a host-admission attempt. |
| medium | Broader M003 host/tool compatibility and contained PTY/LSP owners remain open under M004. | The Linux isolation backend cannot be admitted for ordinary constrained execution. | Continue M004 owner integration and supported-host qualification; do not infer production readiness from C011 warm-path results. |

## 11. Roadmap disposition

C011 is conditionally closed as the bounded warm-path architecture corrective. Its repeated-command path is materially faster and remains protected by immutable private views. The known cold fragmented result is retained as a production blocker under C010; no downstream capability is admitted by this closure.

## 12. Registry updates and unblock audit

- Mark C011 conditionally closed in the subsystem roadmap and registry; retain its implementation and closure records.
- Keep C010's explicit no-admission decision in force. M003 remains blocked on cold fragmented performance, broader compatibility, supported-host qualification, and C002.
- M004 remains active; M005 remains gated on M004. No registered downstream implementation plan becomes newly ready from C011.
