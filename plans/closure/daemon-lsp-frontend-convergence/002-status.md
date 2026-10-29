# M002 — Cross-Frontend LSP Parity and Latency Discipline: Closure

Status: **closed — 2026-10-28**
Previous closure status: superseded; historical measurements below are retained as an initial baseline, not evidence for this closure.
Branch: `plan/daemon-lsp-frontend-convergence-20261027`

## Implementation

M001 is closed in `plans/closure/daemon-lsp-frontend-convergence/001-status.md` after correctness qualification on 2026-10-28.
M002 implementation is carried by `13f3170d0f932e1b60b4e85cb772198e8e58d882`
and `d1e06536547b2eef9df7f8437831bc95fcf1e022`; the 100-edit regression
trajectory is `4bd9afab30347215d8341c3005b0fac7870da854`.

## Gesture coverage matrix

| Gesture | TUI | Desktop | Route and parity decision |
|---|---|---|---|
| Diagnostics | Current-file view backed by project `LspDiagnosticsGet` plus the authorized project subscription. | Host diagnostics view backed by the same pull/subscribe routes. | Shared request coordinator/reconciler; `NotReady` stays distinct from `Ready(empty)`, and stream loss/gaps retain stale state until a pull repairs it. |
| Hover | `space k` issues a scoped `LspReadGet`. | No hover command or view. | TUI-only, documented UI asymmetry. |
| Definition, declaration, implementation, references | Supported editor gestures issue scoped `LspReadGet`. | Four explicit editor controls call `desktop_document_navigate` for definition, declaration, implementation and references. | Both use shared lifecycle; host DTO normalization stays frontend-owned. |
| Document/workspace symbols | TUI document and workspace symbol gestures issue `LspReadGet`. | No corresponding desktop command or view. | TUI-only. |
| Semantic tokens | TUI requests and applies data only to the active editor generation. | No desktop semantic-token view. | TUI-only; reset/freshness remains generation-fenced. |
| Completion | Revision- and position-bound `LspCompletionGet`; inert candidates require checked local application. | Host completion route uses the same coordinator and maps results to desktop DTOs. | Both use latest-wins, scoped debounce and stale discard. |
| Format and rename preview | TUI uses the existing `LspReadGet` preview routes; no optimistic mutation. | No desktop command or view. | TUI-only; checked apply remains outside this milestone. |

The audited absence of desktop hover, symbols, semantic tokens, format and
rename preview remains an explicit asymmetry; M002 does not claim feature
parity where there is no desktop gesture. The desktop host already served
declaration and implementation navigation, so M002 exposes those existing
operations alongside definition and references without adding daemon routes.

## Correctness and latency evidence

- Coordinator admission is scoped by frontend/attachment, route and connection
  generation, project/workspace/session, document/path/server, operation,
  document generation, cursor position and argument. Identical in-flight
  requests coalesce; newer work supersedes/cancels old tickets; capacity is
  bounded. Client tests cover 100 generations, separate attachments,
  cancellation, capacity and invalidation.
- `one_hundred_rapid_edits_discard_a_pending_completion_without_state_growth`
  drives 100 actual local editor edits while a completion is pending. The
  final replica contains all edits, the pending answer is not painted, and no
  request ticket remains. A distinct-cursor completion trajectory proves
  out-of-order older answers cannot overwrite the current result.
- `completion_positions_count_mixed_unicode_as_utf16_units` covers mixed
  UTF-8/UTF-16 offsets in the desktop host; Rust document snapshots and TUI
  completion tests cover the corresponding editor position contract.
- Client reconciliation tests exercise `Ready`, `Ready(empty)`, `NotReady`,
  stale retention, contiguous updates, sequence gaps and authoritative repair.
  TUI/desktop lifecycle tests cover disconnect/reconnect invalidation; the
  visible desktop E2E repeats reconnect and WebView reload twice and checks
  that exactly one desktop subscription/client remains.
- No synchronous keystroke path performs readiness, process startup or LSP
  IPC. The visible editor key-to-paint measurement keeps diagnostics cold and
  reports a bounded sample rather than a threshold gate.

### Request-to-result A/B measurements

The fake-stdio LSP trajectory ran 31 samples for each direct (pre-coordinator
path) and coordinated request condition. CPU load used four worker threads.
Values are milliseconds, p50/p95:

| Condition | Direct | Coordinated, including production debounce |
|---|---:|---:|
| Cold | 0.726 / 1.433 | 43.168 / 56.291 |
| Cold with CPU load | 0.694 / 1.780 | 42.986 / 43.778 |
| Warm | 1.022 / 1.873 | 43.550 / 49.419 |
| Warm with CPU load | 1.387 / 3.791 | 43.181 / 44.404 |

Cold requests returned `NotReady` and did not create an LSP process; warm
requests returned `Ready`. Coordinated samples include the production
40-ms `LSP_INTERACTIVE_DEBOUNCE`; the resulting latency is an explicit
user-visible cost. This is a local fake-server comparison, not a claim about
remote/network latency, historical before/after improvement, or a universal
SLO. The coordinator provides coalescing, cancellation and stale-result
control.

### Visible-host measurements

On ARM64 macOS with a visible Aqua session, the final session E2E captured 31
samples per condition: cold diagnostics request-to-result p50 0 ms / p95 1 ms
with `no_warm_server`; local editor key-to-paint p50 280.686 ms / p95
285.984 ms unloaded and 288.019 / 306.322 ms with four CPU workers. Cold
desktop definition-route request-to-result was 2 / 4 ms unloaded and 3 / 7 ms
with four CPU workers (p50/p95, n=31 each), also `no_warm_server`. These
measurements varied substantially with host contention; they are observed
samples, not a before-implementation baseline or a hard SLO. Key-to-paint is
local and intentionally does not start or wait for LSP. A warm visible-host
sample is unavailable on this macOS constrained profile: daemon LSP tools are
disabled there, while the product's interactive read route is warm-only. The
warm/cold matrix is therefore measured by the fake-stdio request benchmark.

## Verification

- `scripts/verify.sh full` on ARM64 macOS, Rust 1.89, commit
  `d1e06536547b2eef9df7f8437831bc95fcf1e022`: passed quick and workspace
  checks, 354 Python harness tests (one skipped), workspace Clippy, 16,668
  workspace tests (16,659 passed, 9 skipped), and 8,032 feature-gated root
  tests passed (4 skipped).
- After that full run, the 100-edit trajectory was added and passed focused
  Nextest at `4bd9afab30347215d8341c3005b0fac7870da854`:
  `cargo nextest run -p codegg --locked --features
  server,plugins,lsp-test-support --profile ci --test tui_render --
  one_hundred_rapid_edits_discard_a_pending_completion_without_state_growth`.
  `cargo fmt --all` also passed after the addition.
- `scripts/verify.sh scoped` passed after the 100-edit addition and final plan
  updates: quick formatting/generated-asset checks, all automatic guards, 354
  Python harness tests (one skipped), and the full workspace Nextest sweep at
  4bd9afab30347215d8341c3005b0fac7870da854 (16,669 passed, 9 skipped).
- `apps/desktop/scripts/verify-desktop.sh` on visible ARM64 macOS, Rust 1.90,
  commit `13f3170d0f932e1b60b4e85cb772198e8e58d882`: boundary, renderer
  typecheck/build, 241 renderer tests, Tauri host tests, format, Clippy, and
  16 visible-window E2E tests passed (5 lifecycle, 2 autostart, 9 session).
- `python3 scripts/check_authorization_matrix.py` passed.
- On the current ARM64 macOS host with Rust 1.90, the production shared-daemon
  adapter fixture passed:
  `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib production_tui_and_desktop_adapters_share_one_live_lsp_service -- --nocapture`.
  It attaches the production Tauri `LocalSocketClient` and production TUI
  `SocketCoreClient` to one isolated socket daemon, exercises TUI hover and
  desktop navigation/completion, observes ready diagnostics, and asserts one
  daemon LSP client key and one server process start.
- On the same host, the 31-sample fake-stdio benchmark passed:
  `cargo test -p codegg --locked --features lsp-test-support --test lsp_test_support m002_records_cold_warm_and_cpu_loaded_lsp_request_latency -- --nocapture`.
- The final visible-host desktop session gate passed 10/10, including the
  cold/loaded latency samples:
  `CODEGG_DAEMON_EXECUTABLE=target/debug/codegg bash apps/desktop/e2e/run-e2e.sh session`.
- `npm run typecheck --prefix apps/desktop`, `bash scripts/check-desktop-boundary.sh`,
  and the desktop Tauri `cargo fmt --check` passed on the current worktree.
- The visible E2E cold diagnostics state was `no_warm_server`; no server was
  spawned by the read. Its latency probes record durations and status only;
  no source, diagnostic message, symbol or file path is sent as telemetry.

## Security and residual divergence

Project diagnostics remain `file.read`-authorized; point reads retain their
session/workspace identity. Completion candidates remain inert until the
existing checked local apply path accepts them. `Unsupported` and `Ambiguous`
are terminal statuses, and `Unsynced` is not retried until the document sync
has an acknowledged revision. Coordinator invalidation covers editor edits,
cursor/document generation changes, tab/session/route changes, connection
restart and frontend teardown.

The two frontend consumers preserve different display vocabulary and available
gestures. The shared-daemon production adapter fixture exercises both request
paths concurrently against the same daemon and proves one server process; the
visible-window E2E separately qualifies local paint and host interaction. The
fixture is automated rather than a simultaneous human-operated TUI/window
session. No product single-client assumption follows from that test topology.

## Post-M001 implementation and latency progress (2026-10-28)

- The desktop editor now exposes definition, declaration, implementation and
  references controls through its already-served `desktop_document_navigate`
  route. Hover, document/workspace symbols, semantic tokens, format and rename
  preview remain explicit TUI-only gestures; no new daemon route was added.
- `two_authorized_clients_keep_independent_diagnostics_subscriptions` now
  keeps both authorized diagnostics subscriptions active while issuing the
  production TUI hover gesture and a separately scoped peer-coordinator hover
  concurrently against the same warm root/server/profile. Both reads return
  `Ready`, both subscriptions remain valid and independently removable, and
  the daemon's LSP service reports exactly one client key. The peer request
  uses the production `codegg-client` coordinator contract but does not mount
  the desktop Tauri host adapter; the actual desktop visible-window trajectory
  remains separately qualified against its isolated daemon.
- The request benchmark now includes the production 40 ms
  `LSP_INTERACTIVE_DEBOUNCE` in coordinated samples. On ARM64 macOS with Rust
  1.90, 31 samples per condition and four CPU-load workers, direct versus
  coordinator-plus-debounce p50/p95 milliseconds were:

  | Condition | Direct | Coordinated, including debounce |
  |---|---:|---:|
  | Cold | 0.726 / 1.433 | 43.168 / 56.291 |
  | Cold, four CPU workers | 0.694 / 1.780 | 42.986 / 43.778 |
  | Warm | 1.022 / 1.873 | 43.550 / 49.419 |
  | Warm, four CPU workers | 1.387 / 3.791 | 43.181 / 44.404 |

  Cold cases remained `NotReady` and created no LSP client; warm cases were
  `Ready`. These results make the user-visible debounce cost explicit. They
  do not claim a latency improvement from coordination; coordination provides
  coalescing, cancellation and stale-result control.
- The focused desktop surface test passed 16/16, including all four navigation
  controls; the full renderer suite passed 244/244. Renderer typecheck,
  desktop boundary, editor-authority guard and the new Tauri navigation-route
  unit test passed. The earlier Tauri host suite passed 96 tests; the new route
  test passed separately.
- The final visible E2E session phase passed 10/10. The latest 31-sample
  measurements are recorded above. Its key-to-paint probe is local and records
  server status; the current macOS constrained profile cannot provide a warm
  session without violating the daemon's warm-only interactive read contract.
- Current closeout validation on the ARM64 macOS host completed every quick
  gate component: formatting/generated-asset checks, all automatic guards,
  harness tests (357 passed, 1 skipped), and
  `CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets --locked` (passed).
  The combined `scripts/verify.sh quick` wrapper was terminated with exit 143
  at the session time limit during the harness stage; the harness and cargo
  check were then run separately to completion. No canonical wrapper exit
  status is claimed for this closeout.

## Closure decision

M002 exit criteria are met against the verified M001 coordination contract:
the supported desktop navigation gestures are exposed; the audited gesture
matrix records intentional asymmetries; production request coordination and
stale-result handling are covered; and the production TUI and Tauri adapters
have a shared-daemon fixture that proves one LSP process for two clients. The
latency record includes direct versus production-debounced warm/cold request
samples, loaded-CPU request samples, and visible-host local key-to-paint and
request samples with exact sample counts and server status. Existing M001
evidence covers stream gaps, reconnect, stale delayed responses, diagnostics
recovery and independent two-client subscriptions.

The explicit limitation is that a warm visible key-to-paint sample is not
applicable: that path is local-only, and the constrained macOS fixture cannot
warm the server through authorized tools. No interactive route was changed to
start a server to manufacture that sample. GUI/TUI gesture asymmetries remain
documented and are outside this milestone's requirement to claim unsupported
parity. No numerical latency SLO is claimed. M003/M004, M005 and M006 remain
blocked on their separately recorded decisions/policy; closing M002 does not
authorize those implementations.

> **Status reconciled 2026-10-29.** M003 and M004 are **closed** under accepted
> ADR-0056 (`plans/closure/daemon-lsp-frontend-convergence/003-status.md` and
> `004-status.md`), and M005 and M006 are **ready** under their accepted
> decisions. The sentence above records the state at M002's closure.
