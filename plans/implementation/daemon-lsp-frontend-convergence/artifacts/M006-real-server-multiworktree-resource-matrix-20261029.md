# M006 Real-Server Worktree Resource Matrix — 2026-10-29

Status: qualification evidence; M006 remains active

## Environment and method

- Apple M4 Pro, macOS 26.6.2, native ARM64 Rust 1.89.0.
- rust-analyzer `1.89.0 (29483883 2025-08-04)`.
- Pyright CLI `1.1.411`; the installed `pyright-langserver` wrapper did not return a version from the `--version` probe during the earlier compatibility smoke.
- Each matrix ran serially in its own test process with `lsp-real-server-tests` enabled. Temporary independent fixture roots were used for separate worktrees.
- Same-root 1/2/4 editor points are repeated/concurrent acquisitions through one `LspService`, not four independent frontend processes. The assertions require all acquisitions to resolve to one client key and one child.
- Multiworktree starts used opt-in `max_concurrent_starts=2`, `max_admitted_starts=8`. Three distinct roots had to produce three client keys and three direct child servers.
- Readiness was awaited before warm document-symbol reads. Service shutdown was followed by a process-tree assertion that no child remained.
- Resource sampling ran `ps -Ao pid,ppid,rss,%cpu,comm` 40 times at 50 ms intervals. It followed descendants of the Rust test process, excluded `ps`, summed RSS and CPU across the sampled descendant tree, and retained each metric's peak sample. RSS is KB; CPU is the sum of instantaneous process percentages and can exceed 100%.
- `get_or_create_client` elapsed time is a single cold or concurrent-acquisition observation, not p50/p95. This matrix does not claim disk impact or sustained utilization.

## Reproduction commands

```bash
RUSTC=/Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin/rustc \
  rustup run 1.89-aarch64-apple-darwin cargo test -p egglsp --locked \
  --features lsp-real-server-tests --test real_server_smoke \
  rust_analyzer_same_root_and_multiworktree_resource_matrix \
  -- --exact --test-threads=1 --nocapture

RUSTC=/Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin/rustc \
  rustup run 1.89-aarch64-apple-darwin cargo test -p egglsp --locked \
  --features lsp-real-server-tests --test real_server_smoke \
  pyright_three_worktree_resource_matrix \
  -- --exact --test-threads=1 --nocapture
```

## Measurements

| Server | Root/editor shape | Acquisition ms | Warm symbol read ms | Direct children | Peak descendant RSS KB | Peak aggregate sampled CPU |
|---|---|---:|---:|---:|---:|---:|
| rust-analyzer | 1 root; 1, 2, 4 acquisitions | cold 1,065; second 0; concurrent additional `[0, 0]` | 66 | 1 | 623,696 | 116.4% |
| rust-analyzer | 3 roots; start budget 2/8 | `[699, 720, 742]` | `[11, 11, 12]` | 3 | 1,793,040 | 379.0% |
| Pyright | 1 root; 1, 2, 4 acquisitions | cold 15,667; concurrent additional `[0, 0, 0]` | 1 | 1 | 204,024 | 128.6% |
| Pyright | 3 roots; start budget 2/8 | `[15,471, 15,474, 15,478]` | `[2, 2, 1]` | 3 | 484,032 | 382.5% |

Both servers reused one process for repeated same-root acquisitions and retained a distinct child for each of three roots. All four measured service shutdowns exited their child processes. The large Pyright acquisition values are consistent across its one-root and three-root trials on this host; the test records those observations but does not attribute the delay to a CodeGG cause.

## Repeat spot-check after adding teardown assertions

The same serial tests were run again on the same host after asserting that the
one-root service also leaves no child before the three-root phase:

| Server | Root/editor shape | Acquisition ms | Warm symbol read ms | Direct children | Peak descendant RSS KB | Peak aggregate sampled CPU |
|---|---|---:|---:|---:|---:|---:|
| rust-analyzer | 1 root; 1, 2, 4 acquisitions | cold 3,302; second 0; concurrent additional `[0, 0]` | 11 | 1 | 629,872 | 128.0% |
| rust-analyzer | 3 roots; start budget 2/8 | `[851, 851, 851]` | `[11, 11, 12]` | 3 | 1,308,672 | 372.2% |
| Pyright | 1 root; 1, 2, 4 acquisitions | cold 15,934; concurrent additional `[0, 0, 0]` | 2 | 1 | 179,004 | 129.9% |
| Pyright | 3 roots; start budget 2/8 | `[15,542, 15,542, 15,542]` | `[2, 1, 2]` | 3 | 452,808 | 374.4% |

These two serial runs are still too few and too host-specific to claim
percentiles or set resource limits. They show that cold acquisition and peak
RSS vary even with the same fixture and machine, especially for
rust-analyzer; the process-count and teardown assertions remained stable.

## Existing admission and lifecycle coverage

- `BoundedStartLspProcessLauncher` rejects invalid limits and a full admitted-start capacity. Its semaphore limits concurrent launches and launch admission; it is released when launch returns. It does **not** cap the number of ready/live servers.
- Existing service tests cover shutdown during delayed initialization and cancellation of waiters. Supervisor tests cover crash/restart and daemon shutdown behavior. The real-server matrix adds process-exit assertions for normal service teardown.
- Production `configured_service` applies the optional `lsp_process_admission` config. Omission preserves the historical direct launcher, as accepted.
- The matrix's `2/8` setting qualifies starts under three-root parallel acquisition, not a total process or memory budget.

## Exact-head hosted platform qualification

On implementation commit `f206acd0543d4bc626c9a8dc65e5c63ec6dd8a57`:

- Routine hosted CI passed as run
  [40912531183](https://github.com/dbowm91/codegg/actions/runs/40912531183).
- Linux M004 process/IPC qualification passed as run
  [40912541817](https://github.com/dbowm91/codegg/actions/runs/40912541817).
  Both `process-ipc` and `restricted-userns-fails-closed` succeeded, including
  the daemon-owned constrained LSP duplex/procfs proof and the constrained LSP
  launch refusal under the restricted user-namespace host policy. The unrelated
  git-cost pilot was skipped.
- Hosted Desktop E2E passed as run
  [40912552091](https://github.com/dbowm91/codegg/actions/runs/40912552091).

This discharges the current-head hosted Linux M004 containment gate and the
desktop/CI qualification for this implementation head. It does not qualify
real rust-analyzer/Pyright resource use on Linux or create a total live-server
budget.

## Remaining closure gates

1. **No live-process/resource cap is selected.** `max_admitted_starts` bounds starts, not ready processes, and this workstream has no approved RSS target or eviction rule. Attached editor servers must not be evicted. Keep this distinction explicit; do not present start admission as a cap on total resource use.
2. **Retirement is only qualified at service shutdown.** No idle retirement policy or lease exists. A separate maintainer decision is needed before implementing idle retirement; if none is selected, closure must explicitly record that idle servers remain until service/scope teardown and narrow the objective accordingly.
3. **Platform matrix is incomplete.** These real-server resource observations are macOS ARM64 only. Linux M004 host-process denial and contained LSP exchange passed on the exact implementation head above. Windows has no registered filesystem-enforcement backend, so constrained LSP launch must remain unavailable there; no Windows LSP resource qualification is claimed.
4. **Scale coverage remains bounded.** The real matrix covers one and three worktrees, one/two/four repeated same-root acquisitions, and one real server process per root. It does not launch four independent editor processes, model parallel agent substreams, measure disk impact, or report p50/p95 across repeated trials.
5. **No long-duration sample.** The 2-second sample window captures a peak during startup/readiness/use, not sustained idle/indexing behavior or a representative workday. A future repeatable baseline would be needed before choosing resource budgets from these values.

These observations establish real-server reuse/isolation and bounded-start behavior on macOS, plus the contained Linux process/IPC contract on the exact implementation head. They do not establish general memory/CPU limits, retirement safety, or Windows qualification. M006 stays active until the accepted contract and closure conditions resolve the remaining gates.

## Corrective qualification update — 2026-10-29

The final mixed frontend/agent service workload, repeated sustained resource
measurements, actual child-restart stale-response test, and resolved M006
closure-policy choice are recorded in
[`M006-sustained-frontend-agent-workload-20261029.md`](M006-sustained-frontend-agent-workload-20261029.md).
That follow-up supersedes this artifact's open question about whether a
total-live-server budget or idle-retirement policy is required: a total-live
count budget is required; automatic idle retirement is not. The budget remains
unimplemented, so M006 remains active.
