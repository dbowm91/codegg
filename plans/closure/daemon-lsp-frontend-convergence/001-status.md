# M001 — Shared Frontend LSP Request Coordinator: Closure

Status: **closed**  
Closed on: 2026-10-27  
Branch: `plan/daemon-lsp-frontend-convergence-20261027`

## Implementation

The implementation is carried by `be4ccb7aa` (initial shared coordination),
`43a38101f` and `0f8ffbf27` (headless-host correctness fixes), and
`13f3170d0f932e1b60b4e85cb772198e8e58d882` (visible-host completion,
integration corrections, and final product implementation), plus
`d1e06536547b2eef9df7f8437831bc95fcf1e022` (TUI live-daemon trajectory). No
protocol or storage migration was added.

## Requirement and evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Shared request identity and bounded latest-wins coordination | `codegg_client::LspRequestCoordinator` keys requests by frontend, attachment, route/connection generation, project/workspace/session, optional document/path/server, operation, document generation, position and argument. Client tests cover duplicate coalescing, independent attachments, capacity, cancellation, invalidation and 100 rapidly superseding generations. | Pass |
| Reusable diagnostics snapshot/event reconciliation | Both `src/tui/commands/editor.rs` and `apps/desktop/src-tauri/src/document.rs` subscribe through the existing authorized project route, establish a snapshot/cursor baseline, consume complete-set updates and re-pull/resubscribe after gaps or loss. The shared `LspDiagnosticsStream` rejects wrong project/version/cursor and retains stale data until an authoritative result arrives. | Pass |
| TUI production request lifecycle | TUI editor diagnostic, read and completion starters use the coordinator and existing `CoreClient` transport. `production_tui_hover_uses_the_live_daemon_request_coordinator` opens the production editor App, sends the real `space k` gesture through `App::on_key`, and receives a `Ready` hover from the authorized live daemon and fake stdio server. Other App trajectories exercise delayed/out-of-order completion, scope/revision fencing, equivalent-request coalescing, cancellation and typed outcomes. | Pass |
| Desktop production request lifecycle | Tauri host diagnostics, completion and navigation routes use the same coordinator and stream reconciler; renderer session generations fence delivery and cleanup. Host and renderer tests pass. | Pass |
| Freshness and local edit authority | Request tickets are finished only for the active scope and document generation. TUI completion candidates remain inert until checked by `DocumentController`; desktop routes discard stale route/session answers and keep edits under the host document controller. Mixed UTF-8 input is converted to UTF-16 positions in the desktop host test. | Pass |
| Cold, empty, and loss semantics | Client reconciliation tests cover `Ready(empty)`, `NotReady`, gaps, stale retention and authoritative recovery. The visible desktop trajectory observes `no_warm_server` distinctly from an empty clean result and confirms cold reads do not start an LSP server. Desktop reconnect/reload E2E verifies a new subscription and current catalog after each generation change. | Pass |
| Authorization and compatibility | `python3 scripts/check_authorization_matrix.py` passes. No wire DTO, capability, schema, standalone non-socket LSP, or process-launch authority changed. Both consumers use the existing `file.read`-gated daemon routes. | Pass |

## Verification

- `scripts/verify.sh full` on ARM64 macOS, Rust 1.89: passed quick (format,
  generated assets, guards and workspace check), 354 Python harness tests (one
  skipped), workspace Clippy, 16,668 workspace tests (16,659 passed, 9
  skipped), and 8,032 `server,plugins,lsp-test-support` root tests (8,032
  passed, 4 skipped).
- `apps/desktop/scripts/verify-desktop.sh` on visible ARM64 macOS, Rust 1.90:
  passed boundary, renderer typecheck/build, 241 renderer tests, Tauri host
  tests, host format/Clippy, and all 16 visible-window E2E tests (5 lifecycle,
  2 autostart, 9 session).
- E2E exercised real desktop diagnostics subscription and cold pull against an
  isolated live daemon. Status was `no_warm_server`; no language-server
  process was created. The session test's 31 cold request-to-result samples
  measured p50 0 ms and p95 1 ms.
- Desktop reconnect/reload and native close were tested against a live window
  and daemon. The TUI operation uses the production command path through
  `App::on_key` against a live `CoreDaemon` with an authorized client and
  real fake-stdio LSP child. Desktop E2E and TUI integration use separate
  isolated daemon instances; the daemon's concurrent multi-client integration
  tests separately verify independent authorized attachments.

## Security, contention, and residual divergence

The diagnostics stream remains project-scoped and `file.read`-authorized;
session-bound point reads remain separately scoped. No renderer lock spans an
await. The coordinator caps outstanding requests, coalesces identical pending
work, cancels superseded work and invalidates by attachment/frontend lifecycle.
The frontends keep their own view DTO mapping and presentation state. Desktop
does not gain TUI-only hover, symbol, semantic-token, format or rename-preview
gestures as a side effect of sharing lifecycle code.

The visible E2E drives the desktop editor and the live-daemon TUI integration
trajectory drives the TUI App. The tests use separate isolated daemon
instances, so they qualify both production consumers against the same daemon
routes and process ownership model, but do not claim one shared daemon
instance serving simultaneous TUI and desktop windows.

## Recommendation

Close M001. The coordinator and diagnostics reconciler are consumed in both
production frontends, and their lifecycle, freshness, authorization, cold
status and recovery contracts pass the required root and visible-host gates.
M002 is unblocked; it owns the remaining gesture audit, parity/lifecycle
qualification and measured latency record.
