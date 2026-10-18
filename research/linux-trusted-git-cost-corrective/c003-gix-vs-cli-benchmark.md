# C003 — gix versus native Git status benchmark

Status: completed as a bounded status-path decision; no gix adoption and no production route change.

## Question and decision

The tested gix 0.89.0 status path does not justify replacing native Git for this workload. It was slower than direct native Git in both warm and fresh-process samples, added 5.4 MB to this small standalone comparison binary, and expanded the harness dependency graph from 40 to 185 packages. Defer gix. This is not a semantic-parity or production-security qualification for gix across all Git operations.

The existing immutable snapshot route has good warm latency but misses the C010 1,000 ms comparison threshold on every measured cold sample. The cold cost belongs to copying the object database, not Git status parsing. A contained live native-Git route remains a plausible experiment; this benchmark does not admit it, change the production route, or establish its setup cost.

## Method

Harness: `research/linux-trusted-git-cost-corrective/gix-bench/` (isolated Cargo workspace; gix is opt-in with `--features with-gix`). Fixture generator creates 4,096 tracked files of 24 KiB deterministic pseudo-random contents (~100 MiB of object data), commits them, dirties one tracked file, and adds one untracked file. The fragmented fixture retains loose objects; the packed fixture runs `git repack -ad` and `git prune-packed`. Each implementation must report two status entries. This checks count-level parity only, not byte-for-byte `GitPayload` semantics.

Routes:

1. Native Git 2.43.0 with global/system configuration disabled.
2. `egggit::GitEnvPolicy::apply_sync`, which uses the current private immutable object-view route, then Git status.
3. gix 0.89.0 `Repository::status` with `open::Options::isolated()` and `sha1,status` features. System, user, and environment configuration and includes are disabled by gix's isolated options; repository-local configuration remains available. The gix route is not production wired.

Each shape/route has 100 warm samples in one process and 30 fresh-process samples. Before each fresh-process sample the harness calls `posix_fadvise(POSIX_FADV_DONTNEED)` on object files and records advised bytes. This is an advisory request, not proof of kernel page eviction. “Cold” below means a fresh process plus cache-drop advice; it does not claim a verified cold device/page cache. Fixture generation and earlier parity checks may still influence filesystem/device caches. Samples are sequential, single-caller, and include all route work in end-to-end elapsed time. CPU is process-plus-child CPU. Cold RSS is sampled from each one-operation subprocess; warm `ru_maxrss` is cumulative and is not interpreted as per-operation RSS.

Commands:

```bash
cargo check --manifest-path research/linux-trusted-git-cost-corrective/gix-bench/Cargo.toml --features with-gix --locked
cargo run --manifest-path research/linux-trusted-git-cost-corrective/gix-bench/Cargo.toml --release --features with-gix --locked -- bench research/linux-trusted-git-cost-corrective/results
```

## Host and toolchain

- Linux `6.8.0-147-generic`, x86_64; ext4 on `/dev/nvme0n1p2`.
- Intel Core i9-9900K, 8 cores / 16 logical CPUs.
- rustc `1.99.1 (58cf8f9c3 2026-10-13)`, Cargo `1.99.1 (5f94df478 2026-08-27)`.
- Git `2.43.0`; gix `0.89.0` (minimum Rust version documented by the crate is 1.88).

## Results

Values are milliseconds; p95 uses the nearest-rank lower sample index. Each row has 100 warm and 30 fresh-process samples.

| Fixture | Route | Warm p50 / p95 / max | Fresh-process p50 / p95 / max | Fresh-process median CPU | Fresh-process median RSS (self / Git child) |
|---|---|---:|---:|---:|---:|
| Fragmented, 4,098 files / 100,875,466 object bytes | Native Git | 5.17 / 5.56 / 6.06 | 6.46 / 6.92 / 7.13 | 9.82 ms | 7.8 / 5.7 MiB |
| Fragmented | Immutable snapshot + Git | 5.27 / 5.58 / 5.81 | 3,706 / 4,534 / 9,853 | 1,380 ms | 7.8 / 5.7 MiB |
| Fragmented | gix | 8.03 / 8.49 / 9.41 | 11.10 / 12.68 / 313.43 | 9.85 ms | 7.8 / n/a |
| Packed, 4 files / 100,975,256 object bytes | Native Git | 5.44 / 5.66 / 8.14 | 6.88 / 7.28 / 7.54 | 9.57 ms | 7.8 / 5.9 MiB |
| Packed | Immutable snapshot + Git | 5.21 / 5.47 / 5.62 | 2,651 / 4,143 / 4,467 | 159 ms | 7.8 / 6.0 MiB |
| Packed | gix | 7.74 / 8.06 / 8.35 | 10.70 / 10.82 / 13.99 | 9.16 ms | 7.8 / n/a |

Raw per-sample CSV files are retained in `research/linux-trusted-git-cost-corrective/results/`. The packed and fragmented median object-store sizes differ slightly because Git's generated pack representation is not byte-identical to the loose representation.

Binary/dependency-cost comparison is a standalone harness check, not a CodeGG release binary estimate: the optimized no-gix harness binary was 842,440 bytes; the gix-enabled harness binary was 6,265,824 bytes (+5,423,384 bytes). `cargo tree --prefix none` reported 35 unique package names without gix and 129 with the opt-in gix feature. gix remains optional in this throwaway workspace and is absent from the CodeGG dependency graph and lockfile.

## Interpretation and limits

- Warm snapshot status is comparable to direct Git for this fixture, but cold snapshot p95 is 4.1–4.5 seconds, beyond C010's 1,000 ms comparison threshold. Snapshot cold CPU also rises sharply, especially for 4,098 loose objects.
- gix status is roughly 1.5–1.7 times the native-Git median here. It does not offer a performance reason to replace the CLI for status and does not avoid the current snapshot cost unless a separate live-object design is approved.
- Only status entry counts are compared. This does not establish parity for diffs, logs, worktrees, renames, submodules, SHA-256 repositories, unusual paths, malformed pack/index data, alternates, replace refs, reflogs, shallow/promisor repositories, or Git config execution. No 1/4/8-caller contention, cancellation, sandbox startup, full payload comparison, clean build time, or device-cold I/O test was performed.
- `posix_fadvise` is advisory; the experiment records requested bytes and never labels the result as a guaranteed OS-cold measurement.
- The contained native-Git C001 hosted fixture demonstrates a live child can read repository objects and invoke configured commands within the tested M004/M005 boundary, but no timing from that workflow is attributed to this benchmark.

## C003 recommendation

**Defer gix.** Keep the current immutable snapshot route as the trusted production behavior. Treat contained native Git as a separate test-only candidate requiring C001 threat closure, C002 adapter ownership and native qualification, M005 owner/admission, structured-payload and race handling, and independent ADR-0036 acceptance. The snapshot timing is evidence for a corrective design, not authority to relax immutable consistency or switch defaults. C003's bounded gix decision does not close C001/C002/C004/C005 or C010/M003.
