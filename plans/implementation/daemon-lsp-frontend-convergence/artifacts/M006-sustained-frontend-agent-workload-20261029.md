# M006 Sustained Frontend and Agent Workload — 2026-10-29

Status: qualification evidence; M006 remains active

## Policy disposition

- A **total-live-server count budget is required** for strict M006 closure.
  The existing concurrent/admitted-start permits are released after launch and
  therefore do not bound ready language-server processes. The workload below
  held six rust-analyzer processes live across three roots and reached a
  repeated-sample aggregate descendant RSS p95 of 2.7 GiB, with one run's peak
  at 3.6 GiB. This makes the missing lifetime bound material to M006's stated
  process/resource objective.
- **Automatic idle retirement is not required** for strict closure. When an
  opt-in total-live budget is full, CodeGG can return a typed, cancellable
  admission-saturated outcome without evicting an attached editor server.
  Processes remain owned until their LSP service/scope is torn down or an
  authorized stop is requested. Retirement is a separate usability feature;
  it must not be introduced as an implicit eviction policy.
- The cap is a **process-count bound**, not an RSS, CPU, disk, or universal
  machine-capacity promise. The measured per-process footprint varies widely
  during the same workload, so this host does not justify a portable memory
  threshold. The count limit must be daemon-owned, opt-in, shared by FullHost
  and constrained turn scopes, and held until child exit is reaped. Saturation
  must refuse promptly with a typed result; warm reads must not queue behind a
  cold start.

This disposition narrows the implementation gate; it does not close M006.
The global lifetime-count budget and its denial/release tests are not yet
implemented. Idle retirement stays out of this milestone's closure criteria.

## Environment and workload

- Apple M4 Pro, macOS 26.6.2, native ARM64, Rust 1.89.0,
  rust-analyzer `1.89.0 (29483883 2025-08-04)`.
- Three independent temporary Rust workspace roots, each populated with 384
  generated public struct symbols to exercise workspace indexing.
- One daemon-owned `LspService` shared by two concurrent frontend-shaped
  workloads per root (TUI document-symbol reads and desktop document-symbol
  reads); a separate service scope per root represented three parallel agent
  LSP workloads (workspace-symbol reads). This tests service ownership and
  process identity; it does not launch the TUI, desktop renderer, or agent
  runtime as separate application processes.
- Each active round sent a managed document change to all six service/root
  pairs, then ran the two shared-daemon reads and one isolated agent
  workspace-symbol read per root. All returned non-empty symbol sets. The run
  performed 54 rounds over 60.6 seconds.
- Resource samples followed descendants of the test process, counted its six
  direct rust-analyzer children, summed RSS over the descendant tree, and
  derived CPU utilization from deltas in cumulative process CPU time divided
  by actual sample elapsed time. This avoids macOS `ps %cpu` rounding to zero.
  Active sample count was 54. The follow-on idle-retention window was 15
  seconds; six samples were available because process-tree sampling slowed
  under load.
- Workspace disk usage summed files under the three temporary roots immediately
  before and after the active window. It excludes server caches outside those
  roots and is not a whole-machine disk measurement.

## Repeated measurements

Two edit-driven runs used the same host, Rust toolchain, server binary,
workload, and sampling code. Values are aggregate across all six server
processes and their descendants; RSS is KiB, CPU is aggregate percent and may
exceed 100% for short bursts.

| Run | Active rounds / seconds | Active RSS p50 / p95 / max KiB | Active CPU p50 / p95 / max | Idle RSS p50 / p95 / max KiB | Idle CPU p50 / p95 / max | Workspace bytes before → after |
|---|---:|---:|---:|---:|---:|---:|
| 1 | 53 / 60.2 | 3,522,816 / 3,522,928 / 3,749,120 | 2.86% / 5.70% / 336.09% | 3,522,928 / 3,522,928 / 3,522,928 | 0% / 0% / 0% | 10,333,759 → 20,562,841 (+10,229,082) |
| 2 | 54 / 60.6 | 833,504 / 2,793,840 / 3,184,656 | 3.79% / 7.48% / 390.47% | 492,336 / 500,144 / 500,144 | 0% / 0% / 0% | 10,332,045 → 20,561,127 (+10,229,082) |

Both runs retained six live direct servers through the idle window and
reaped all six at service teardown. RSS dispersion across the two same-host
runs is large; do not infer a fixed per-server memory cost or set a portable
RAM quota from these observations. The repeated workspace-tree increase is
measured only; this method does not identify which process or cache wrote
those bytes.

## Reproduction

```bash
RUSTC=/Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin/rustc \
CODEGG_RA_BIN=/Users/davidbowman/.cargo/bin/rust-analyzer \
  rustup run 1.89-aarch64-apple-darwin cargo test -p egglsp --locked \
  --features lsp-real-server-tests --test real_server_smoke \
  rust_analyzer_sustained_tui_desktop_agent_worktrees \
  -- --exact --test-threads=1 --nocapture
```

## M006 closure work now defined

1. Add a daemon-owned lifetime permit shared across FullHost and constrained
   LSP service scopes. The opt-in maximum counts launched child servers until
   their exit is reaped, including restart launches; it is not released when
   initialize returns.
2. Return a typed immediate saturation result with cancellation semantics and
   a bounded operator count. Preserve per-root/profile isolation and never
   evict an attached server. Do not claim idle retirement.
3. Prove concurrency, cancellation, process-exit release, restart, daemon
   shutdown, parallel agent scopes, and full-budget behavior with fake
   processes; run the mixed real-server matrix to check for cold-read stalls.
4. Keep Windows constrained LSP explicitly unavailable while no
   filesystem-enforcement backend is registered. Linux containment and
   macOS real-server evidence are separate qualifications.

The stdio supervisor test
`real_server_restart_discards_an_inflight_old_generation_response` replaces an
actual child process while the old request is outstanding, confirms the old
request fails closed, and verifies a fresh response from the replacement. A
separate same-process delayed-response race is not claimed as qualified by this
artifact.
