# A004 Cargo/sccache economics pilot — measured result

Date: 2026-10-28
Source revision: `493e5ee111d133eb1cb99debddcc466122c7f7f9`
Disposition: **No-go for production compiler-cache integration. A004 is closed as a negative pilot.**

## Protocol and execution

The opt-in ignored Rust test `scheduler::submission::tests::a004_cargo_economics_through_production_scheduler` submitted real `cargo check --locked --offline -p codegg-core` jobs through `JobSubmissionService → JobScheduler → ManagedArgvExecutor`. Each repetition used two sibling managed worktrees from the frozen source revision, separate fresh Cargo target directories, and the production repository quota configured either off or at one. Cache arms used the pinned local sccache 0.18.0 binary in client-side mode, with normalized sibling base directories, a task-scoped cache capped at 3 GiB, and `CARGO_INCREMENTAL=0` in both cache and no-cache arms. Arm order was randomized independently for each of three repetitions.

Host/toolchain: Apple Silicon macOS 25.6; Cargo 1.89.0; rustc 1.89.0, `aarch64-apple-darwin`; sccache 0.18.0. The process environment reported the shell architecture as x86_64 while rustc and the test process reported ARM64; no Linux or contained-process qualification was run. One unrelated host Rust compilation overlapped one sample; it is retained in the raw results rather than discarded. All 24 Cargo jobs succeeded, and the harness removed its own worktrees, targets, and cache on completion.

## Results

Each row below summarizes six individual jobs (two sibling worktrees × three repetitions). Wall time is median per Cargo job with the full observed range. CPU is the median of user + system seconds per job. Targets were separate in every arm. Every captured Cargo lock-wait count was zero.

| Cache | Repository quota | n | Median wall (s) | Range (s) | Median CPU (s) | Target bytes | Cargo lock waits |
|---|---:|---:|---:|---:|---:|---:|---:|
| off | 1 | 6 | 106.80 | 98.52–122.57 | 165.70 | 641,365,146 | 0 |
| off | disabled | 6 | 107.94 | 102.84–113.69 | 170.98 | 641,365,146 | 0 |
| on | 1 | 6 | 161.89 | 139.05–181.92 | 280.81 | 641,366,837 | 0 |
| on | disabled | 6 | 178.05 | 145.99–184.21 | 304.54 | 641,366,837 | 0 |

The cache-on target was 1,691 bytes larger per worktree. sccache reported 5,424 executed requests, 638 hits (all assembler/C/C++; **zero Rust hits**), 4,692 Rust misses, no cache errors/timeouts, and a rounded cache size of 2 GiB under the 3 GiB cap. The aggregate hit ratio of 11.84% is not a Rust-cache hit ratio and gives no evidence of reusable Rust compilation across these worktrees. Cache-on median CPU was about 69% higher with quota one and 78% higher with quota disabled; median wall time was about 52% and 65% higher, respectively.

## Decision and limits

Do not add sccache, a compiler-cache service, a shared Cargo target, a global Cargo configuration, or cache-driven placement to production on this evidence. Preserve separate writable targets and the existing opt-in repository quota behavior. A later cache proposal needs a new plan and materially different evidence, such as a supported Rust cache configuration that demonstrates cross-worktree Rust hits and net savings under the actual contained executor.

This pilot establishes only the measured result on the stated macOS/toolchain with the stated sccache version and workload. It does not establish Linux/Windows behavior, native sandbox permission to run a cache server, trust partitioning, warm-cache benefit for changed sources, cross-toolchain correctness, or production cache lifecycle. No cache recommendation is made. A003 remains useful independently: quota admission controlled scheduler overlap; no Cargo lock waits appeared because each job had its own fresh target directory.

## Raw per-job observations

`maxrss` is the platform process-tree peak RSS in bytes as reported by the measurement helper. `cpu_s` is user + system CPU seconds. `quota=null` means repository quota disabled.

| Repetition | Cache | Quota | Worktree | Wall s | User s | System s | Max RSS bytes | Target bytes | Cargo lock waits |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | off | 1 | 0 | 107.480 | 147.402 | 18.664 | 1676902400 | 641365146 | 0 |
| 0 | off | 1 | 1 | 122.574 | 168.921 | 23.216 | 1724071936 | 641365146 | 0 |
| 0 | on | 1 | 0 | 179.613 | 246.833 | 58.348 | 1767718912 | 641366837 | 0 |
| 0 | on | 1 | 1 | 153.597 | 213.450 | 53.795 | 1587150848 | 641366837 | 0 |
| 0 | off | disabled | 0 | 113.687 | 154.987 | 22.002 | 1706475520 | 641365146 | 0 |
| 0 | off | disabled | 1 | 112.164 | 154.758 | 21.574 | 1681424384 | 641365146 | 0 |
| 0 | on | disabled | 0 | 184.210 | 247.504 | 59.408 | 1567719424 | 641366837 | 0 |
| 0 | on | disabled | 1 | 183.566 | 247.496 | 59.114 | 1563492352 | 641366837 | 0 |
| 1 | on | 1 | 0 | 170.181 | 235.100 | 59.275 | 1673035776 | 641366837 | 0 |
| 1 | on | 1 | 1 | 181.920 | 253.392 | 62.253 | 1626161152 | 641366837 | 0 |
| 1 | on | disabled | 0 | 177.526 | 243.755 | 60.839 | 1484554240 | 641366837 | 0 |
| 1 | on | disabled | 1 | 178.582 | 243.495 | 60.994 | 1510703104 | 641366837 | 0 |
| 1 | off | disabled | 0 | 107.513 | 150.232 | 20.552 | 1568112640 | 641365146 | 0 |
| 1 | off | disabled | 1 | 108.361 | 150.242 | 20.932 | 1555152896 | 641365146 | 0 |
| 1 | off | 1 | 0 | 107.189 | 148.148 | 19.586 | 1783267328 | 641365146 | 0 |
| 1 | off | 1 | 1 | 106.406 | 145.722 | 19.605 | 1728806912 | 641365146 | 0 |
| 2 | off | disabled | 0 | 103.671 | 144.956 | 19.963 | 1580220416 | 641365146 | 0 |
| 2 | off | disabled | 1 | 102.840 | 144.954 | 19.753 | 1561985024 | 641365146 | 0 |
| 2 | on | 1 | 0 | 139.053 | 198.473 | 50.081 | 1613479936 | 641366837 | 0 |
| 2 | on | 1 | 1 | 139.299 | 197.492 | 49.645 | 1698070528 | 641366837 | 0 |
| 2 | off | 1 | 0 | 98.524 | 139.297 | 19.322 | 1660452864 | 641365146 | 0 |
| 2 | off | 1 | 1 | 98.745 | 139.596 | 19.230 | 1667678208 | 641365146 | 0 |
| 2 | on | disabled | 0 | 146.688 | 205.966 | 50.725 | 1492631552 | 641366837 | 0 |
| 2 | on | disabled | 1 | 145.986 | 205.993 | 50.515 | 1514700800 | 641366837 | 0 |
