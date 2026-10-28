# M001 — Shared Frontend LSP Request Coordinator: Closure

Status: **closed**  
Closed on: 2026-10-28
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
| Reusable diagnostics snapshot/event reconciliation | Both `src/tui/commands/editor.rs` and `apps/desktop/src-tauri/src/document.rs` subscribe through the existing authorized project route, establish a snapshot/cursor baseline, consume complete-set updates and re-pull/resubscribe after gaps or loss. `diagnostics_stream_gap_recovers_from_authoritative_pull` proves gap detection, stale retention and recovery; the desktop reconnect test proves replacement on a new connection generation. | Pass |
| TUI production diagnostics subscription | `production_tui_diagnostics_uses_the_authorized_live_subscription` uses the production `App::on_key` diagnostics gesture, existing `CoreClient`, coordinator and authorized live daemon to receive the diagnostic view. Its live run found and fixed the missing direct-project scope mapping for `LspDiagnosticsGet`. `attach_core_client` resubscribes an open editor on reconnection. | Pass |
| Desktop production diagnostics subscription | `desktop_document_diagnostics_subscribe` owns a loss-aware subscription, coordinator ticket and stream reconciler; renderer session subscription tests prove cleanup and re-subscribe when `connectionGeneration` changes. The visible lifecycle E2E confirms one active `desktop-sub-*` per generation through reconnect/reload; the visible session E2E exercises cold diagnostics and preserves `no_warm_server`. | Pass |
| Delayed-response suppression | `diagnostics_invalidation_cancels_delayed_response_without_touching_reads` cancels a pending diagnostics ticket and proves the unrelated hover read can still finish. Desktop route/session freshness checks reject stale results after route or connection changes. | Pass |
| Two-client operation | `two_authorized_clients_keep_independent_diagnostics_subscriptions` opens two authorized client subscriptions to the same project, verifies distinct subscription IDs and the same complete diagnostics snapshot, then unsubscribes each independently. Shared client stream tests consume the same update under distinct per-client subscription IDs. | Pass |
| Freshness and local edit authority | Request tickets are finished only for the active scope and document generation. TUI completion candidates remain inert until checked by `DocumentController`; desktop routes discard stale route/session answers and keep edits under the host document controller. Mixed UTF-8 input is converted to UTF-16 positions in the desktop host test. | Pass |
| Cold, empty, and loss semantics | Client reconciliation tests cover `Ready(empty)`, `NotReady`, gaps, stale retention and authoritative recovery. The visible desktop trajectory observes `no_warm_server` distinctly from an empty clean result and confirms cold reads do not start an LSP server. Desktop reconnect/reload E2E verifies a new subscription and current catalog after each generation change. | Pass |
| Authorization and compatibility | `python3 scripts/check_authorization_matrix.py` passes. No wire DTO, capability, schema, standalone non-socket LSP, or process-launch authority changed. Both consumers use the existing `file.read`-gated daemon routes. | Pass |

## Verification

- 2026-10-28 correctness qualification on the visible ARM64 macOS host, based
  on branch commit `dcb1d852e` with the recorded worktree changes:
  `cargo test --locked --offline --target aarch64-apple-darwin -p codegg-client`
  passed 77 tests (52 unit, 9 GUI client, 16 projection driver) and doc tests;
  the client cases include gap recovery, delayed-response invalidation and
  independent per-frontend streams.
- Focused daemon-bound tests passed:
  `cargo test --locked --target aarch64-apple-darwin -p codegg --features
  lsp-test-support --test lsp_test_support --
  production_tui_diagnostics_uses_the_authorized_live_subscription` (1/1),
  and the same command filtered to
  `two_authorized_clients_keep_independent_diagnostics_subscriptions` (1/1).
  The production TUI gesture returned `Ready` diagnostics from the live daemon;
  two authorized clients received distinct subscriptions and the same complete
  snapshot, then unsubscribed independently.
- Desktop renderer `npm run typecheck` passed. The focused Vitest run
  `npm test -- src/editor/session.test.ts` passed 7/7, including reconnect
  resubscription and silent duplicate-completion handling.
- `apps/desktop/scripts/verify-desktop.sh host` passed all 96 Tauri host unit
  tests and formatting. Its Clippy step could not compile third-party
  `schemars`/`indexmap` and `cpufeatures` dependencies on this host; the errors
  occurred before project Clippy diagnostics.
- Visible desktop E2E passed all 16 tests (5 lifecycle, 2 autostart, 9 session).
  It confirmed active diagnostics subscriptions across reconnect/reload and
  preserved the cold status `no_warm_server` without starting a language
  server. Its 31 cold request-to-result samples were p50 0 ms / p95 1 ms;
  31 key-to-paint samples were p50 71.56 ms / p95 74.31 ms.
- Repository qualification passed `cargo fmt --all -- --check`,
  `scripts/check_comment_budget.py`, `scripts/rust_test_scan.py --validate`,
  `scripts/check_test_assertion_quality.py`, `scripts/check_test_duplication.py`,
  and `git diff --check`.
- A broader run filtered to all `lsp_read_delivery_m013` tests initially
  reported 11 passed and 7 failed. The TUI diagnostics failure was fixed by
  adding the missing `LspDiagnosticsGet` direct-project scope. Its focused
  production test now passes. Six remaining failures are in pending M003
  server-status authorization and M005 selection-range behavior; those
  milestones remain blocked on their recorded ADR decisions and were not
  included in M001's acceptance.

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

## Closure decision

M001 is closed. The coordinator and diagnostics reconciler are consumed in both
production frontends, and their lifecycle, freshness, authorization, cold
status and recovery contracts pass the focused root and visible-host
qualification. M002 is active; it owns the remaining gesture audit,
parity/lifecycle qualification and measured latency record.
