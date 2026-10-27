# M002 — Cross-Frontend LSP Parity and Latency Discipline: Closure

Status: **closed**
Closed on: 2026-10-27
Branch: `plan/daemon-lsp-frontend-convergence-20261027`

## Implementation

M001 is closed in `plans/closure/daemon-lsp-frontend-convergence/001-status.md`.
M002 implementation is carried by `13f3170d0f932e1b60b4e85cb772198e8e58d882`
and `d1e06536547b2eef9df7f8437831bc95fcf1e022`; the 100-edit regression
trajectory is `4bd9afab30347215d8341c3005b0fac7870da854`.

## Gesture coverage matrix

| Gesture | TUI | Desktop | Route and parity decision |
|---|---|---|---|
| Diagnostics | Current-file view backed by project `LspDiagnosticsGet` plus the authorized project subscription. | Host diagnostics view backed by the same pull/subscribe routes. | Shared request coordinator/reconciler; `NotReady` stays distinct from `Ready(empty)`, and stream loss/gaps retain stale state until a pull repairs it. |
| Hover | `space k` issues a scoped `LspReadGet`. | No hover command or view. | TUI-only, documented UI asymmetry. |
| Definition, declaration, implementation, references | Supported editor gestures issue scoped `LspReadGet`. | `desktop_document_navigate` maps supported operations through `LspReadGet`. | Both use shared lifecycle; host DTO normalization stays frontend-owned. |
| Document/workspace symbols | TUI document and workspace symbol gestures issue `LspReadGet`. | No corresponding desktop command or view. | TUI-only. |
| Semantic tokens | TUI requests and applies data only to the active editor generation. | No desktop semantic-token view. | TUI-only; reset/freshness remains generation-fenced. |
| Completion | Revision- and position-bound `LspCompletionGet`; inert candidates require checked local application. | Host completion route uses the same coordinator and maps results to desktop DTOs. | Both use latest-wins, scoped debounce and stale discard. |
| Format and rename preview | TUI uses the existing `LspReadGet` preview routes; no optimistic mutation. | No desktop command or view. | TUI-only; checked apply remains outside this milestone. |

The audited absence of desktop hover, symbols, semantic tokens, format and
rename preview is intentional and remains visible; M002 does not claim feature
parity where there is no desktop gesture.

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

| Condition | Direct | Coordinated |
|---|---:|---:|
| Cold | 0.267 / 2.938 | 0.225 / 0.274 |
| Cold with CPU load | 0.366 / 0.789 | 0.385 / 0.698 |
| Warm | 0.331 / 0.480 | 0.361 / 0.559 |
| Warm with CPU load | 0.554 / 1.615 | 0.494 / 0.815 |

Cold requests returned `NotReady` and did not create an LSP process; warm
requests returned `Ready`. This is a local fake-server A/B, not a claim about
remote/network latency or a universal improvement.

### Visible-host measurements

On ARM64 macOS with a visible Aqua session, the final `m004-session.e2e.ts`
run captured 31 samples each: cold diagnostics request-to-result p50 0 ms / p95
1 ms with `no_warm_server`, and local editor key-to-paint p50 70.671 ms / p95
72.256 ms while the server stayed cold. A prior visible-host run recorded
71.84 / 74.21 ms for the same 31-sample key-to-paint trajectory. These are
repeatability samples of the completed path, not a before-implementation
baseline or a hard SLO.

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
gestures. The real desktop E2E and production TUI App trajectory use separate
isolated live daemon instances; concurrent attachment behavior is covered in
daemon/client tests, but this closure does not claim one simultaneous,
human-operated TUI and desktop window pair. This is a test topology limit, not
a product single-client assumption: both adapters send typed requests through
the same daemon-owned LSP service and no frontend starts its own server.

## Recommendation

Close M002. All supported read gestures use shared request lifecycle, stale
work is bounded and discarded, diagnostics reconcile after gaps/reconnect,
100 rapid edits are proven, visible-host responsiveness and request timing are
measured, and documented UI asymmetries remain explicit. No arbitrary latency
threshold or unsupported capability claim was introduced.
