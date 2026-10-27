# M001 Closure Readiness

Review baseline: branch `plan/daemon-lsp-frontend-convergence-20261027`, committed `HEAD` `48929a172eaf5267b8d1a0255b1a6ac25f6a0837`, based on `origin/main` `80171f45295bebb9ab0b521262c0b295acb93074`. The implementation under review is currently in the worktree and has not been committed.

## Finding

M001 remains active and is not ready to close. The worktree now has a bounded, cancellation-aware request coordinator wired into the TUI and desktop read paths, plus a transport-neutral diagnostics snapshot/event reconciliation state machine. Client tests and the root all-target check pass. Neither production frontend yet owns a live `LspDiagnosticsSubscribe` lifecycle, and the planned daemon-bound cross-frontend and loss/reconnect evidence is still missing. M002 correctly remains blocked until M001's closure record exists and the registry marks M001 closed.

## Requirement status

| M001 requirement | Current evidence | Work needed to close |
|---|---|---|
| Shared, bounded, scope-aware request coordination | `LspRequestCoordinator` is used by TUI diagnostics/read/completion and desktop diagnostics/completion/navigation. Unit tests cover duplicate keys, capacity, stale scope/generation, invalidation, cancellation, and independent attachments. | Add key-isolation tests across project/session/workspace/path/server ID and exercise superseding policy through frontend adapters. |
| Cancellation/lifetime and reconnect invalidation | Tickets carry cancellation tokens; superseding same frontend/attachment/project/workspace/session/operation cancels prior work. TUI client attachment and desktop route lifecycle invalidate frontend tickets. Keys include connection generation. | Add production disconnect/reconnect and daemon-restart coverage in both hosts; ensure all route/document transitions invalidate promptly. |
| Shared diagnostics reconciliation | `LspDiagnosticsStream` validates the project snapshot/cursor, contiguous envelopes, complete-file updates, gaps and authoritative pulls. `DiagnosticsReconciler::accept_streamed_checked` enforces per-file sequence continuity. Client unit tests cover initial reconciliation, contiguous updates, gap, `NotReady`, and authoritative empty. | No production frontend calls `LspDiagnosticsSubscribe` or consumes this state machine. Wire initial snapshot, stream updates, replay/re-pull, teardown and reconnect in both frontends. |
| Loss-aware project event delivery | `CoreClient::subscribe_loss_aware` and connection generation are available; `SocketCoreClient` adapts the existing socket event subscriber. The stream state machine can mark transport loss stale. | Neither frontend currently binds this subscriber to LSP project stream updates. Deliver only authorized project events (not session projections), and recover on lag/gap/close. |
| Fresh-response admission | TUI read completion checks document revision and point position; coordinator checks scope and generation. Desktop routes check route/session generation and use the coordinator for diagnostic, completion, and navigation calls. Superseded tickets are actively cancelled. | Add delayed-response integration tests for edit, cursor move, document/document-session/worktree switch, route switch and reconnect; prove stale answers cannot clear newer loading/error state. |
| Cold, empty, and denial semantics | Protocol distinctions are preserved. Client unit tests prove `Ready(empty)` replaces a baseline and `NotReady` retains it. | Exercise those outcomes and permission denial through both production adapters, including the `LspDiagnosticsSubscribe` route. |
| M001 integration proof | `cargo test -p codegg-client --locked --offline` passes (47 unit, 25 integration; 0 failures). `cargo check -p codegg --locked --all-targets --offline` and `python3 scripts/check_authorization_matrix.py` pass. Desktop boundary, renderer typecheck, 241 renderer tests, bindings/build, host tests (92 passed, 1 ignored), host fmt and Clippy pass through the desktop gate phases. | Add fake delayed transport and daemon-bound TUI/desktop tests: two authorized attachments, another project with same path, server selection, cold-read no-spawn, denial, restart, stream lag and missing baseline. Desktop E2E and root focused LSP/document suites remain without retained passing results. `scripts/verify.sh quick` remains non-passing: a harness background Cargo job exited 101 without retained stderr. |
| Closure and dependency transition | M001 is active, M002 blocked; there is no M001 closure record. | Record exact commit(s), requirement/evidence matrix, focused/root/desktop results, security and lag cases, and residual divergence in `plans/closure/daemon-lsp-frontend-convergence/001-status.md`. Only then mark M001 closed in `plans/registry.md` and M002 ready. |

## Recommended implementation order

1. Complete production subscriptions for both frontends using the existing connection and the authorized `LspDiagnosticsSubscribe` snapshot/cursor. Use authoritative `LspDiagnosticsGet` after stream gaps, lag or reconnect; do not create a pull/subscribe race.
2. Bind the loss-aware TUI and desktop host event paths to the shared state machine. Keep project diagnostics under `file.read` and out of session snapshots.
3. Wire and test route/document/connection invalidation, delayed response suppression, two attachments, gap/replay, reconnect/restart and no-cold-spawn. Preserve frontend views and `DocumentController` edit ownership.
4. Run all M001 gates, including desktop and authorization checks. The earlier quick verification remains non-passing: its harness suite reported errors in a background Cargo job even after the real toolchain binary was placed first in `PATH`; resolve or document this accurately before closure.
5. Write the closure record only after the integration evidence is complete; then update the registry and unblock M002.

## No new ADR indicated

The daemon already has a dedicated `file.read`-gated project diagnostics subscription, authoritative pull, complete per-file replace events, snapshot cursor, and replay support. The current blocker is client integration and proof. If implementation discovers that existing replay cannot provide the required authorized project-stream delivery or freshness witness, stop and record the specific protocol/authority gap before widening M001.

## M002 handoff

Once M001 closes, M002 still owns its independent work: audit the gesture matrix; migrate remaining supported editor reads; add its 100-edit, UTF-8/UTF-16, two-client parity and restart/lag fixture; collect reproducible warm/cold key-to-paint and request-to-result p50/p95 measurements; and write its own closure record. M001 closure unblocks that work; it does not satisfy M002 acceptance.
