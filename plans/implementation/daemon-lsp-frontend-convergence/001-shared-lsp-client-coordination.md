# M001 — Shared Frontend LSP Request Coordinator

Status: ready for handoff

Primary class: infrastructure (vertically qualified in both rich frontends).

Repository planning baseline: `80171f45295bebb9ab0b521262c0b295acb93074` (`main`, 2026-10-27); before implementation inspect current branch HEAD, update file paths and record drift. Source roadmap: `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md`. Planning governance: `plans/003-planning-process.md`. Do not edit accepted ADRs or historical closure records.

Accepted ADRs: ADR-0010, ADR-0011, ADR-0012, ADR-0018, ADR-0021. Long-term §§4, 13, 24–25 in `plans/000-long-term-specification.md`. Hard dependencies: closed M005 document foundation and M006-B daemon LSP read surface. No new ADR, capability, transport family or migration is required.

## Objective

Replace divergent *transport/request lifecycle decisions*, not frontend rendering, with one `codegg-client` LSP coordinator. Prove consumption by **one complete asynchronous LSP operation in the TUI and one in the desktop host** against the same live daemon. Preserve original features and negative semantics.

## Current evidence

`crates/codegg-client/src/lsp.rs` exposes `LocalSocketClient::{lsp_read,lsp_completion,lsp_diagnostics}` and `DiagnosticsReconciler`. `src/tui/commands/editor.rs` independently dispatches and invalidates `LspReadGet`/diagnostics, `src/tui/editor_completion.rs` retains a separate revision/epoch policy, while `apps/desktop/src-tauri/src/document.rs` wraps its own diagnostic/completion/navigation requests and views. Daemon endpoints in `src/core/daemon_lsp.rs` and `src/core/daemon_lsp_completion.rs` already have typed statuses and revisions. Never mistake these duplicated presentation structures for duplicated LSP servers.

## Invariants and non-goals

No new editor text buffer, network call on the synchronous keystroke/render path, transport-specific daemon code in the renderer, remote root selection, implicit warmup, unauthorized disclosure or single-client assumption. Preserve TUI completion popup/edit application, desktop bounded viewport, `DocumentController` ownership, LSP warm-only response policy and isolated `M004LspServiceScope`. Do NOT prematurely unify editor widget state or introduce a generic raw LSP proxy.

## Production changes / packages

1. Inspect both consumers and specify `LspRequestScope` using trusted frontend *attachment*, project/workspace/session, document identity (when required), daemon revision/optimistic generation and server selection; distinguish project-wide diagnostic scope from session-bound point-read scope. A request key must not accidentally coalesce across principals, file paths, server IDs or attachments.
2. Add a small transport-neutral coordinator in `crates/codegg-client/src/lsp.rs` or a dedicated `lsp_coordinator.rs`: inject an existing typed request sender/client abstraction without imposing a root-package dependency. Use a caller-controlled cancellation/lifetime token, per-document latest-generation policy, bounded outstanding work and typed reconnect invalidation. Never hold a renderer lock across an await.
3. Centralize reusable `DiagnosticsReconciler` integration: baseline `LspDiagnosticsGet`, `LspDiagnosticsSubscribe` project gate, complete-set sequence handling, lag/gap/reconnect re-pull and `Ready(empty)` vs `NotReady`. Preserve diagnostic content caps, per-workspace containment and source disclosure checks.
4. Add shared response-admission helpers for exact scope, attachment, document revision, editor generation and process-generation freshness when supplied. Completion candidates remain inert; only the frontend's existing `DocumentController` accepts a checked local edit. Avoid inventing a revision for existing `LspReadGet` results; correlate their initiator scope and discard after edit.
5. Integrate exactly one real diagnostic/read path in TUI and one in desktop through the coordinator. Keep the remaining consumers unchanged for M002. Include transport adapter tests if the TUI's `CoreClient` trait differs from `LocalSocketClient`; do not create a second socket connection solely to reuse helpers.

## Verification

Focused: `cargo test -p codegg-client --locked`; `cargo test -p codegg --locked lsp`; `cargo test -p codegg --locked document` (adjust filters to actual suites). Root: `cargo check -p codegg --locked --all-targets`, `cargo check -p codegg --locked --features server`, `cargo clippy -p codegg --locked --all-targets -- -D warnings`; `python3 scripts/check_authorization_matrix.py`. Desktop: run the repository's `scripts/check-desktop-boundary.sh` and current desktop check/typecheck/test/E2E entry point recorded in its roadmap. Use fake stdio LSP tests plus a production daemon-bound TUI and desktop request. If a command is unavailable on the host, record it unrun rather than pass.

Inject: delayed success after edit/switch; reconnect and daemon-restart; changed project with same filename; same file with two authorized clients; different server ID; connection lag/drop; missing baseline; `NotReady`, `Ready(empty)`, `Ambiguous`, `Unsupported`, permission denial and stale completion. Assert exactly one client-side delivery to each eligible view and no new process spawn on a cold read.

## Storage, protocol, security, compatibility

No persistent schema, migrations or protocol shape changes; only client implementation and thin frontend adapters. Preserve `file.read`, `file.modify`, warm-only semantics and explicit old-daemon refusal. No change to standalone non-socket LSP behavior.

## Acceptance and stop conditions

Close only when production TUI **and** GUI consumers demonstrably share the coordinator and the no-stale/no-false-clean tests pass, without a GUI/TUI process-launch dependency. Stop on a required wire/authority change and write an ADR or successor milestone; do not enlarge M001. Closure record: `plans/closure/daemon-lsp-frontend-convergence/001-status.md` with exact commits, requirement/evidence matrix, focused/root/desktop results, security/lag cases and residual divergence.
