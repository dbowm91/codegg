# Daemon LSP Frontend Convergence M006 — Closure Status

Status: **closed**
Closed on: 2026-10-29
Qualified implementation head after rebase: `a50743445a3a17021cf9bf7e6e6f40c5a010713b`
Rebased onto current `origin/main`: `4aea3df5e93c60aa85f55e805f2a8e9e947ab083`

Source plan:

- `plans/implementation/daemon-lsp-frontend-convergence/006-worktree-aware-lsp-process-admission.md`

Source roadmap:

- `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md`

## 1. Closure finding

M006's process-lifetime admission contract is implemented and qualified. An
opt-in daemon-wide permit authority counts each launched LSP child from
admission until the child has exited and has been reaped. FullHost and Linux
constrained scopes share that authority. A restart generation consumes its
own permit while the previous child is still live. Saturation returns the
typed `LiveProcessBudgetExhausted` refusal immediately. Existing attached
editor servers are never evicted to make room.

With no configured live-server limit, existing admission behavior is
preserved. Automatic idle retirement is not required for closure. The bound
limits live child count; it does not claim to cap RSS, CPU, disk or aggregate
indexing work.

## 2. Configuration and lifecycle contract

| Setting | Default | Contract |
|---|---:|---|
| `lsp_process_admission.max_live_servers` | `null` | Optional daemon-wide maximum for live LSP child processes; zero is invalid |
| `max_concurrent_starts` | Existing behavior | Bounds starts entering the launcher; distinct from the lifetime budget |
| `max_admitted_starts` | Existing behavior | Bounds starts admitted concurrently; released when launch returns, not an active-server cap |

The live-process budget is held by daemon runtime dependencies and forwarded
into constrained Linux turn scopes. The launcher acquires without waiting and
returns a typed refusal when full. Cancellation does not drop a permit while
launch work or a child remains alive. The permit follows the child into its
monitor; successful wait/reap releases it. If reaping cannot be confirmed,
the permit is quarantined so the budget fails closed. Malformed launch
results and unclaimed children are killed and reaped before release.

## 3. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Concurrent starts saturate one shared daemon-wide cap | `crates/egglsp` admission lifecycle tests; FullHost and constrained-scope launchers contend for a single permit | Pass |
| Exhaustion is prompt and typed | Saturation test observes `LiveProcessBudgetExhausted` without waiting for a permit | Pass |
| Caller cancellation does not leak or prematurely release a permit | Unit test for cancelled launch plus `cancellation_after_child_start_reaps_child_and_releases_live_budget` in `production_service_stdio` | Pass |
| Crash releases only after exit/reap | Crashed fake child is waited and its permit becomes available | Pass |
| Restart overlap is counted | Restart-generation test keeps the old generation's permit while the replacement requests a second; old-generation delayed response is rejected by `real_server_restart_discards_an_inflight_old_generation_response` | Pass |
| Daemon shutdown reaps children and releases capacity | `daemon_shutdown_releases_live_process_budget_after_child_reap` | Pass |
| Attached editor process is not evicted | Budget path only refuses new starts; it has no eviction/stop path | Pass by implementation and architecture review |
| Real server and separate-worktree identity behavior remains valid | ARM64 rust-analyzer 1.89.0/Pyright 1.1.411 resource matrices; two sustained mixed frontend/agent service runs across three worktrees | Pass for recorded host/workloads |
| Linux constrained execution retains process and containment boundaries | Exact-head Linux process/IPC and restricted-userns workflow 40953681698 | Pass |

The sustained workload and real-server details, including sample dispersion
and machine/server versions, are recorded in:

- `plans/implementation/daemon-lsp-frontend-convergence/artifacts/M006-sustained-frontend-agent-workload-20261029.md`
- `plans/implementation/daemon-lsp-frontend-convergence/artifacts/M006-real-server-single-workspace-baseline-20261029.md`
- `plans/implementation/daemon-lsp-frontend-convergence/artifacts/M006-real-server-multiworktree-resource-matrix-20261029.md`

The sustained runs use frontend-shaped reads sharing a daemon service and
separate agent service scopes; they do not launch the TUI, desktop renderer
and agent runtime as independent application processes. Measurements are
host-specific and are not a universal resource SLO.

## 4. Verification

The original exact-head qualification ran against `e75ff289c62672640593b4bdafb4fd618b7ac533`; rebasing onto `4aea3df5e93c60aa85f55e805f2a8e9e947ab083` resolved planning-document conflicts only and did not change implementation files. All three hosted gates were rerun successfully at the rebased branch head.

| Gate | Result | Receipt |
|---|---|---|
| Hosted CI | Passed | [run 40953681531](https://github.com/dbowm91/codegg/actions/runs/40953681531) |
| Desktop E2E | Passed | [run 40953681191](https://github.com/dbowm91/codegg/actions/runs/40953681191) |
| Linux M004 process/IPC and restricted-userns qualification | Passed | [run 40953681698](https://github.com/dbowm91/codegg/actions/runs/40953681698) |

The focused local lifecycle group passed 6/6. Production stdio cancellation
and daemon-shutdown tests passed 2/2. The root dependency-plumbing test passed
1/1, workspace Clippy passed with `-D warnings`, and `scripts/verify.sh quick`
passed on the preceding implementation commit; the subsequent changes only
added a test-only cfg and moved the test module to satisfy Clippy, and the
final SHA passed hosted CI.

## 5. Platform and scope limits

| Platform/scope | Disposition |
|---|---|
| Linux FullHost and constrained service scopes | Shared daemon budget; hosted process/IPC, restricted-userns and constrained LSP qualification passed |
| macOS FullHost | FullHost implementation and local lifecycle tests passed; native ARM64 real-server resource evidence is recorded in the artifacts |
| Windows constrained LSP | Unavailable: no filesystem-enforcement backend is registered; no constrained-process claim is made |
| RSS/CPU/disk ceiling | Not provided; process count is the only resource bound in this milestone |
| Automatic idle retirement | Not required; scope teardown or authorized stop owns lifecycle; saturation never evicts an attached server |
| Broader Helix M014 syntax-node text objects | Separate capability; M005's bounded structural-selection slice is closed and this work does not claim broader syntax parity |

## 6. Roadmap disposition

M006 is closed. The opt-in live-child limit is daemon-wide across FullHost
and constrained Linux scopes, survives startup cancellation and restart
overlap, and releases only after successful child reaping. Exact-head CI,
Desktop E2E and Linux qualification passed. Broader Helix M014 syntax-node
text objects remain separate from M005 and do not reopen or block M006.

## 7. Registry updates

- Mark M006 and the daemon LSP/frontend convergence roadmap closed in
  `plans/registry.md` and `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md`.
- Keep broader Helix M014 syntax-node objects separate. ADR-0034 Option B is
  accepted and M005's bounded structural-selection slice is closed, but the
  broader M014 text objects remain unimplemented and need their own bounded
  plan. They are not an M006 dependency.
- No other registered work was blocked on M006; no downstream plan was
  unblocked by this closure.
