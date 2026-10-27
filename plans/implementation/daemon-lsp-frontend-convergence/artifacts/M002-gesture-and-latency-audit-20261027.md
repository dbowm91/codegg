# M002 Gesture and Latency Audit

Audit baseline: branch `plan/daemon-lsp-frontend-convergence-20261027`, source tip `48929a172eaf5267b8d1a0255b1a6ac25f6a0837` plus the current uncommitted implementation. This is a source audit, not parity or performance acceptance evidence. M002 remains blocked on strict M001 closure.

## Gesture coverage

| Gesture | TUI behavior and route | Desktop behavior and route | M002 disposition |
|---|---|---|---|
| Diagnostics | Explicit diagnostics panel; `LspDiagnosticsGet`; scoped current-file view. | `desktop_document_diagnostics`; `LspDiagnosticsGet`; DTO maps project files for the view. | Shared coordinator now covers pull requests. Project push/subscription and live parity are not wired. |
| Hover | Explicit editor action routes `LspReadOperation::Hover` through `LspReadGet`; result is scoped to document generation and position. | No desktop hover command or view route found in `apps/desktop/src-tauri/src/document.rs`. | Preserve this as a documented UI asymmetry; do not claim parity. |
| Definition / declaration / implementation / references | Editor actions route these supported reads through `LspReadGet`, with document scope and cursor position. | `desktop_document_navigate` maps supported navigation operation names through `LspReadGet`; result DTOs are host-mapped. | Coordinator coverage exists in both request paths; add matching delayed-response and route-switch evidence. |
| Document symbols | TUI editor action uses `LspReadGet`. | No desktop document-symbol command/view route found. | TUI-only. |
| Workspace symbols | TUI editor query action uses `LspReadGet` with a query and project scope. | No desktop workspace-symbol command/view route found. | TUI-only. |
| Semantic tokens | TUI supports `LspReadOperation::SemanticTokens` and applies data to the active editor generation. | No desktop semantic-token command/view route found. | TUI-only; validate stale/reset behavior. |
| Completion | TUI completion request uses `LspCompletionGet`, revision/position fencing and inert candidates. | `desktop_document_completion` uses `LspCompletionGet` and host-side response mapping. | Shared coordinator covers both; no evidence yet for rapid-edit/cursor or mixed-encoding parity. |
| Format / rename preview | TUI editor routes format and rename preview through `LspReadGet`; rename carries `new_name`. | No desktop format or rename-preview command/view route found. | TUI-only; preserve no optimistic mutation and `DocumentController` authority. |

The TUI source routes are in `src/tui/commands/editor.rs` (operation dispatch, response freshness, and editor actions). Desktop routes are in `apps/desktop/src-tauri/src/document.rs`; the desktop renderer is in `apps/desktop/src/`. This audit did not execute either UI.

## Invalidation and request policy

The current shared coordinator key contains frontend, attachment, route generation, connection generation, project, workspace, session, optional document/path/server, operation, document generation, position, and argument. Equivalent pending work collapses; a newer request in the same frontend/attachment/project/workspace/session/operation class cancels its predecessor. TUI attachment changes and desktop route commit/disconnect invalidate frontend tickets. This still needs tests for edit, cursor, active document, session, worktree, writer-lease, reconnect and daemon restart transitions. The production diagnostics subscription lifecycle is not connected, so stream loss/replay is only covered at the client state-machine level.

No new text buffer or synchronous keystroke network request was added. The request coordinator does not yet centralize every potential LSP call site outside the audited editor read paths. Existing `NotReady`, `Unsupported`, `Ambiguous` and `Unsynced` values are preserved; no retry-on-`Unsynced` loop was introduced.

## Required fixture and measurements still absent

No two-client parity fixture, 100-rapid-edit scenario, mixed UTF-8/UTF-16 scenario, daemon restart/stream-lag integration scenario, or renderer nonblocking assertion has been run for this worktree. No key-to-paint or request-to-result instrumentation/sample exists, so warm/cold and loaded-CPU p50/p95 comparisons cannot be reported. These are acceptance blockers, not zero-latency results. Add bounded, content-free timing capture to a reproducible fixture and record host, sample count, warm/cold state, load method, and before/after distributions in M002 closure evidence.

## Current verification evidence

- `cargo test -p codegg-client --locked --offline`: passed (47 unit tests, 25 integration tests).
- `cargo check -p codegg --locked --all-targets --offline`: passed.
- Desktop boundary, renderer typecheck, 241 renderer tests, bridge bindings, renderer build, host tests (92 passed, 1 ignored), host fmt and Clippy passed. The desktop E2E phase was invoked and skipped because this host has no display or `xvfb-run`; visible-window E2E remains unverified.
- Authorization matrix check passed.
- Root focused LSP/document suite results are not recorded as passing.
- M002 parity/performance fixtures: not run.
- `scripts/verify.sh quick`: earlier run remains non-passing because the harness background Cargo job exited 101; its full stderr was not retained. Do not treat this as green.

## Next acceptance work

1. Close M001 with production authorized diagnostics subscriptions, recovery, disconnect teardown, and real TUI/desktop adapter integration proof.
2. Keep the UI asymmetries above explicit while moving every supported request lifecycle to `codegg-client`.
3. Add event-driven invalidation and bounded, content-free measurements; no instrumentation may include source text, diagnostic messages, symbols or paths.
4. Run the planned 100-edit, mixed-encoding, two-client, restart, lag, stale-response, no-cold-spawn, desktop E2E and root verification gates.
5. Write `plans/closure/daemon-lsp-frontend-convergence/002-status.md` with exact host, samples, distributions, test results and remaining asymmetries. Keep M002 blocked until M001 is formally closed.
