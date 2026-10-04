# Shared Editor Document Foundation Post-Closure Corrective C001 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/editor-document-foundation-post-closure-corrective/001-document-controller-linearization-scheduler-and-recovery.md`

Source subsystem roadmap: `plans/subsystems/editor-document-foundation-post-closure-corrective-addendum.md#c001--documentcontroller-linearization-scheduler-ownership-and-recovery`

Parent roadmap: `plans/subsystems/editor-document-foundation-roadmap.md#milestones`

Repository baseline reviewed: `1dcbce6d9b07ea50a24f385ba63db6eed59cee95`

Implementation commits:

- `1dcbce6d` — linearize document controller lifecycle, save fences, and flush recovery
- The closure commit adds the final resync-rearm regression test and planning evidence.

## 1. Executive finding

C001 repairs the shared `DocumentController` lifecycle without changing the daemon-owned `DocumentService`, one-writer lease, `document.v1` protocol, LSP mirror, or disk-authoritative agent tools. Keystroke mutations now use a short synchronous replica lock and are independent of async operation serialization. Save captures a local edit sequence fence, destructive transitions freeze edits, lifecycle epochs fence completions, and one worker owns debounce plus serial flush work until it exits.

All local focused verification and both required hosted workflows passed on the corrected production head. No high or medium controller correctness finding remains. The historical M005-D closure remains immutable; this record is the current strict closure authority for its client-controller portion and restores parent M005 to strict closed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Synchronous local mutation without false queue exhaustion | `ReplicaState` is guarded by a short `std::sync::Mutex`; `apply_local()` no longer uses `try_lock`; `QueueFull` is emitted only at the configured transaction bound | pass |
| Save fence preserves typing and later edits | `save_fence_keeps_edits_accepted_while_save_is_in_flight`; `tui_kind_controller_keeps_edit_through_save_and_reopen` | pass |
| Close/reload cannot race accepted edits | Lifecycle phase is set before request; `apply_local()` returns `LifecycleBusy` while destructive phases run; `close_fence_rejects_typing_and_failed_close_restores_attachment` | pass |
| Lifecycle cancellation restores usability | `LifecycleGuard` restores close/open/save/reload/resync/reconnect phase and public state on cancellation; `cancelled_close_releases_the_lifecycle_fence` | pass |
| Recovery restores network scheduling | Successful resync/reconnect enables the network and rearms pending work; `successful_resync_rearms_pending_network_flush` | pass |
| Exactly one flush worker owns debounce and drain | The ownership flag stays set through debounce and flush; the worker relinquishes it under the replica lock and rechecks pending work; `blocked_flush_owns_one_worker_through_a_local_edit_burst` verifies 25 edits and maximum change concurrency of one | pass |
| Open failure and repeated-open behavior are deterministic | `failed_open_returns_to_closed_state`; `repeated_open_is_rejected_without_a_second_remote_open`; second open returns `AlreadyOpen` | pass |
| TUI-kind trajectory retains and converges edits across save/reopen | `tui_kind_controller_keeps_edit_through_save_and_reopen`; existing managed LSP trajectory remains green | pass |
| Daemon/document/LSP architecture and authority remain unchanged | Focused `document_service::tests`, `document_lsp_integration`, and repository boundary guards | pass |

## 3. Production implementation evidence

The prior separate async attachment/state mutexes are consolidated into
`ReplicaState`, protected by a synchronous mutex used only for bounded in-memory
bookkeeping. The async operation mutex still serializes requests whose daemon
ordering is significant. Replica lifecycle phases and epochs identify open,
save, reload, close, resync, and reconnect windows. No synchronous guard is
held across `.await`.

`apply_local()` applies the rope transaction, queues it, increments
`local_edit_seq`, and marks idle replicas dirty while holding the replica lock.
It remains permitted during `Saving`; other destructive/reconciliation phases
reject edits as `LifecycleBusy`. `AlreadyOpen` prevents one controller from
silently orphaning an earlier attachment.

## 4. Verification executed

Local commands used the installed arm64 Rust toolchain and `/tmp` for Unix
socket fixtures; this avoids the host's long default temporary socket path.

- `cargo test -p codegg-document --locked` — 5 tests passed.
- `cargo test -p codegg-client --locked` — 22 unit tests, 9 GUI client tests, 16 projection-driver tests, and doc tests passed.
- `cargo test --test document_client_trajectory --locked` — 3 tests passed.
- `cargo test --lib tui::document_session --locked` — 1 test passed.
- `cargo test document_service::tests --lib --locked` — 7 tests passed.
- `cargo test --features lsp-test-support --test document_lsp_integration --locked` — 1 test passed.
- `scripts/verify.sh quick` — passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.
- `git diff --check` — passed.
- Hosted root CI run [37177976742](https://github.com/dbowm91/codegg/actions/runs/37177976742) — SUCCESS on `1dcbce6d`.
- Hosted Desktop E2E run [37177976771](https://github.com/dbowm91/codegg/actions/runs/37177976771) — SUCCESS on `1dcbce6d`, including the built-app WebDriver trajectory.

The default x86_64 toolchain initially could not link against this machine's
arm64-only MacPorts libraries. The arm64 toolchain completed all listed local
Rust verification; hosted CI independently passed on its supported runner.

## 5. Invariant review

- Any successful `apply_local()` retains its text in the rope and pending queue until acknowledged or a user-directed destructive transition.
- Save cannot mark the replica clean when the local edit sequence advanced after its captured fence.
- `QueueFull` and `QueueBytes` represent configured limits, not mutex contention.
- One controller owns at most one flush worker; changes remain serial at the transport boundary.
- Daemon text/revision remain canonical. The client remains an optimistic replica.
- One writer per document, LSP mirror behavior, agent disk authority, protocol compatibility, and Rust 1.89 MSRV policy are unchanged.

## 6. Failure and recovery review

Open transport/protocol failures leave `Closed`. Save transport failure retains
the pending draft, disables automatic network work, and reports `Disconnected`;
disk conflict remains `Conflict`; unexpected protocol responses require
resync. Close and reload set a lifecycle fence before sending requests and
restore the prior usable state after failure or cancellation. Resync and
reconnect freeze edits during reconciliation, preserve matching queued drafts,
restore network scheduling on success, and retain divergent drafts with an
explicit recovery state. A failed worker exits only after releasing ownership
under the same lock used by `apply_local()`, then rechecks whether work arrived.

## 7. Migration and compatibility review

No storage migration or wire change was introduced. `AlreadyOpen` and
`LifecycleBusy` are local client errors. The one-controller/one-attachment
contract is explicit; callers switch documents through close/create/open.

## 8. Security review

Requests continue through the existing scoped project/document operations,
writer lease checks, daemon authorization, path containment, and workspace
mutation authority. This change adds no protocol authority, path access, audit
payload, or additional text owner.

## 9. Documentation and operations

The corrective addendum is closed and points to this current strict evidence.
The editor roadmap now records M005 as strictly closed while preserving the
historical M005-D record. The desktop roadmap and registry return M006 to
eligible-for-fresh-planning, with a fresh TUI presentation audit required
before any M006 implementation plan is written.

## 10. Unresolved findings

| Severity | Finding | Required action |
|---|---|---|
| none | No unresolved high/medium controller correctness issue | none |

## 11. Roadmap disposition

M005-D and parent M005 are strictly closed under this corrective evidence.
M006 is eligible for fresh TUI presentation audit/planning, not implementation
handoff. The separate M002 Windows evidence condition remains independent.
Graphical IDE/editor work remains long-term.

## 12. Registry updates

- Close C001 and the post-closure corrective addendum; retain this record as current strict authority for M005-D.
- Restore parent M005 to strict closed without rewriting `plans/closure/editor-document-foundation/004-status.md`.
- Remove M006 from blocked work and add it to milestones eligible for fresh planning; its next gate is a fresh TUI presentation audit.
- Audited every registered blocked plan and affected roadmap dependency: M006 was the only registered plan blocked by C001. No other plan is unblocked by this closure.
