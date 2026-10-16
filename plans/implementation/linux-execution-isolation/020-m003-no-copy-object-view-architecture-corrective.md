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

The first process-boundary candidate to prototype is a helper-owned private mount namespace that presents the daemon-selected repository read-only and overlays its existing `objects/info` directory with a private tmpfs. This omits repository-controlled alternates without copying object payloads. If `objects/info` is absent, the fallback composes `objects` from read-only pack/fanout mounts and a private info directory. A simple `--ro-bind-data` overlay works only when the alternates file already exists: Bubblewrap cannot create the overlay target beneath a read-only whole-repository mount when the file was absent. The prototype must pin the real gitdir/common-dir, refuse a pre-existing alternate before launch, and establish whether source-object mutation can be excluded by the selected M003 profile or still requires a snapshot.

The candidate must also preserve selected repository identity across nested `.git` creation and linked-worktree/common-dir layouts; reject pre-existing unsupported alternates; avoid source hard links; retain trusted argv/config overrides; and define what concurrent source-object mutation means for each supported launch profile.

### Investigation update (2026-10-16)

The environment-only alternate probe rules out `GIT_ALTERNATE_OBJECT_DIRECTORIES` as a no-copy mechanism. A local Bubblewrap 0.9.0 execution probe mounted a repository read-only and overlaid an empty file at `.git/objects/info/alternates` with `--ro-bind-data`; it ran `git rev-parse --verify HEAD` with a pre-existing external alternate. A follow-up showed that this overlay cannot be created beneath a read-only whole-repository mount if the file was absent. A second probe replaced the object-directory mount with a private tmpfs object root, rebound the selected repository's pack and loose-object fanout directories read-only, and left `objects/info` private. A simpler fast path then overlaid an existing `objects/info` directory with tmpfs. After the child had started, a host-side writer created an alternate pointing at another repository; inside the child the file remained absent, the selected `HEAD` object remained readable, and the outside commit was unavailable. This validates both mount compositions against a late alternate on this host. It does not verify the CodeGG helper handshake, races against in-place object writes, linked worktrees, or production Git-owner integration.

A preliminary warm-cache performance probe on this workstation used the mixed
working repository (Ubuntu x86_64, kernel `6.8.0-147-generic`, ext4, Git
`2.43.0`, Bubblewrap `0.9.0`; 5,145 loose objects / 47,768 KiB and 52,182
packed objects / 78,786 KiB across 44 packs). With the one-directory tmpfs
overlay, 20 `git status --porcelain` samples measured median/p95/max
9.92/10.77/10.87 ms versus direct Git 7.56/8.53/8.56 ms (p95 ratio 1.26).
Ten `git log -n 1` samples measured 4.07/4.21/4.29 ms versus direct Git
1.84/1.89/1.92 ms. The many-mount fallback with 257 pack/fanout mounts measured
97 ms p95 for the same log operation. These warm, single-repository local
samples show the `objects/info` overlay is promising and the fallback needs
optimization; they do not satisfy C010's paired 100-warm/30-cold, exact-content
packed/fragmented admission protocol and do not include the CodeGG helper
handshake or status/ack path.

The source-mutation probe is a hard blocker for applying this fast path
generally. After the child entered its read-only mount, a host-side writer
replaced a selected loose blob with a different, well-formed zlib-compressed
Git blob at the same object path. `git cat-file blob <selected-oid>` returned
the replacement payload successfully. Thus read-only mounts and content
addressing do not preserve the current immutable-generation guarantee. A
no-copy path may only replace snapshots when the selected profile prevents
untrusted writers from changing the object store and all trusted writers are
serialized against the read, or when an immutable filesystem snapshot is
available. The existing `GitExecutionService`/`egggit` command path carries no
daemon-authenticated no-writer lease, so it cannot currently select this fast
path based only on a repository path or sandbox-profile string. FullHost cannot
use the shared live-object fast path under the current threat model; it must
keep the immutable snapshot or use another qualified snapshot mechanism.

### Exact-content local shape comparison (2026-10-16)

The implementation code under test is `0ddafd40609be6e57d09046584e3478d79c7ca63`; the dataset was created from the same repository object store at documentation-only head `aaed0d170a4e4d691811f43d235790812d1a62d6`. Two temporary worktrees were created from the same `.git` source and
normalized to identical object-ID sets (53,269 IDs). The packed store has two
packs and 60,361 KiB packed data; the fragmented store has 53,269 loose files
and 438,624 KiB loose data. `git cat-file --batch-all-objects` set equality
was checked after normalization. Both were measured on the same Ubuntu
x86_64 / kernel `6.8.0-147-generic` / ext4 / Git `2.43.0` / Bubblewrap
`0.9.0` workstation. This exceeds C010's fragmented-store minimum of 4,000
loose objects and 100 MiB object data, but is still one repository and one
host.

The candidate was direct Bubblewrap launch with a read-only repository bind
and a private tmpfs overlay on `.git/objects/info`; it did not use CodeGG's
trusted helper or status/ack protocol. On 20 warm `git status --porcelain`
samples, the packed shape measured 14.94 ms p95 versus 12.20 ms direct (1.22x);
the fragmented shape measured 14.87 ms versus 11.85 ms (1.25x). On 20 warm
`git log -n 1` samples, packed measured 4.19 ms p95 versus 1.67 ms direct,
and fragmented measured 4.23 ms versus 1.58 ms direct. These status values
meet C010's warm absolute/ratio bounds on this host; the short log command
shows the fixed namespace-start overhead more clearly.

For comparison, the existing private-copy path's 20-sample warm preparation
p95 was 0.049 ms packed / 0.048 ms fragmented. Warm end-to-end status p95 was
14.90 ms packed versus 13.43 ms paired baseline, and 13.67 ms fragmented versus
13.88 ms baseline. Five cold-advised private-copy misses on the packed shape
had preparation median/p95/max 137.9/147.3/147.3 ms. The fragmented shape's
five cold-advised misses measured 12.65, 18.75, 22.61, 20.67, and 45.18 s
(median 20.67 s, nearest-rank p95/max 45.18 s). The fixture harness uses
per-file `POSIX_FADV_DONTNEED`, not global cache flushing; five cold samples
do not satisfy C010's 30-sample requirement. This comparison makes the
fragmented copy miss impractical and shows the mount fast path is promising
only if the no-writer lease can be enforced.

The threat-boundary review distinguishes authority from snapshot consistency. A read-only mount plus masked alternates can prevent the Git child from following repository-controlled object-store aliases, while concurrent host-side writes can still change bytes consumed by a query. Git does not necessarily validate the requested object bytes against its object ID on ordinary reads; the `cat-file` probe demonstrated that it returned a replacement payload. Do not treat content addressing or a read-only child mount as freezing the host-side object store. Object-generation immutability remains a hard acceptance requirement.

The no-copy path must use the existing trusted sandbox helper/status-ack ownership boundary. Launching `bwrap` directly from `egggit` would duplicate the launcher and bypass CodeGG's helper identity, setup receipt, and acknowledgement protocol. The production shape therefore requires either a helper-owned Git launch request with the same trusted setup handshake or an equivalent adapter through the existing owner; a unit-level mount compiler test alone is insufficient.

## 3. Work packages

1. Review the threat boundary for trusted Git under FullHost and constrained profiles. Treat source mutation as observable unless excluded by the owner. Establish whether M003's protected metadata/object mounts plus serialization of trusted Git writers can support a no-copy constrained-profile view; retain immutable snapshots for FullHost unless an immutable filesystem snapshot is qualified.
2. Compare no-copy process-boundary candidates, including a trusted mount namespace that privately composes the object root without `objects/info`, filesystem-native copy-on-write snapshots where supported, and bounded selective snapshots. Reject environment-only alternates and hard-link designs unless adversarial probes prove the guarantees.
3. Implement the selected design behind the existing internal opt-in. Keep the cached private-copy path available only as a temporary comparison/fallback where the chosen policy explicitly permits it; never fail open to host object discovery.
4. Preserve and extend adversarial tests for pre-existing and late alternates, nested repositories, linked worktrees/common directories, source mutation, races, packed and fragmented stores, and cleanup after timeout/cancellation.
5. Measure controlled cold and warm preparation plus repeated trusted operations on exact-content-equivalent packed and fragmented stores. Record filesystem, kernel, Git version, repository shape, cache state, sample distributions, and any source-mutation assumptions.
6. Run exact-head native Linux trusted-Git and M004 regression workflows, cross-platform compile checks for changed shared code, ADR integrity, and full CI. Record whether the design meets the C010 thresholds; retain C010's no-admission disposition unless every production gate is met separately.

## 4. Invariants

- A trusted Git child cannot discover a repository, object directory, alternate, worktree, or config outside the daemon-selected canonical repository context.
- Creating nested `.git` metadata after admission cannot redirect an already-authorized daemon Git operation.
- A concurrent object-store mutation cannot alter bytes consumed by a trusted read. A no-copy route therefore requires an enforced no-writer lease for its entire lifetime or an immutable filesystem snapshot; readonly child mounts alone are insufficient.
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
