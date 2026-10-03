# Shared Editor Document Foundation M005-D — Closure Status

Status: closed

Source implementation plan: `plans/implementation/editor-document-foundation/004-client-replica-and-tui-first-qualification.md`

Source subsystem roadmap: `plans/subsystems/editor-document-foundation-roadmap.md#milestones`

Parent roadmap: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-006--tui-ide-vertical-slice`

Repository baseline reviewed: `7425cf6a`

Implementation commits:

- `44a94b0` — shared optimistic `codegg-client` document controller and TUI ownership adapter
- `53c1908` — observer close lifecycle qualification

## 1. Executive finding

M005-D and parent M005 are complete. `codegg-client` now exposes a rope-backed,
optimistic controller for the additive `document.v1` protocol. It owns a bounded
pending queue, daemon revision, stable change IDs, one debounced serial flush
task, conflict/reconnect state, and a replaceable transport for reconnect.
The TUI has a thin `TuiDocumentSession` adapter over that same controller and
keeps cursor/selection/viewport placeholders outside its text replica.

The controller supports safe coalescing only for adjacent unsent pure
insertions whose byte coordinates prove equivalence. Other transactions retain
their order and boundaries. Snapshot reads and pending result checkpoints clone
the rope-backed `DocumentSnapshot`, not the full text body.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Reusable shared optimistic replica | `codegg_client::DocumentController`, `DocumentBuffer` | pass | The client uses the `codegg-document` transaction core and exposes a rope-backed snapshot. |
| Explicit local/daemon/conflict/transport states | `DocumentState` | pass | Includes synced, local dirty, flushing, resync required, conflict, read-only, disconnected, gone-with-draft, and closed. |
| Bounded queue and flush ownership | 128 transaction / 4 MiB pending insert caps; one debounce task per controller | pass | Offline local drafts remain bounded and do not spawn repeated flush tasks after transport loss. |
| Safe edit coalescing | `adjacent_unsent_insertions_coalesce_before_flush` | pass | Two provably adjacent insertions become one request; other edits remain separate. |
| Stable IDs and uncertain retry | `uncertain_change_retries_with_same_id_and_optimistic_text` | pass | A dropped response retains and retries the identical ID/payload. |
| Reconnect and writer reacquisition | `reconnect_reattaches_and_retries_uncertain_change_with_same_id` | pass | A replacement transport reopens the same scoped file, reacquires the lease, and retries the same change ID. |
| Accepted-prefix and divergence handling | reconnect/resync snapshot comparison | pass | Matching authoritative text acknowledges a prefix; unchanged base retries; other divergence retains the local draft and requires resync/recovery. |
| Daemon restart preserves local draft | `daemon_restart_keeps_gone_document_draft_without_replacing_it` | pass | Missing document becomes `GoneWithLocalDraft`; no disk write or automatic replay occurs. |
| Save waits for pending changes; conflict preserves text | `writer_replica_flushes_conflicts_and_explicit_reload_preserves_authority` | pass | Save flushes, conflict keeps `ab!`, and explicit reload installs `external`. |
| Observer status and read-only policy | `read_only_observer_polls_metadata_without_owning_text_edits` | pass | Revision polling is metadata-only, resnapshot is explicit, writes fail, and observer close detaches cleanly. |
| TUI uses shared controller, no second text owner | `TuiDocumentSession`, `tui_session_opens_through_the_shared_replica` | pass | Presentation state changes while controller text remains canonical in the shared replica. No editor widget was added. |
| Unsaved text reaches LSP | `document_lsp_integration::managed_document_replay_and_disk_refresh_keep_unsaved_text` | pass | Fake stdio LSP sees managed unsaved text; disk refresh does not overwrite it. |
| Daemon writer/observer and lease cleanup | `DocumentService` focused tests | pass | One-writer/read-observer, disconnect release/reacquire, conflict, and revision cases pass. |
| Large-document replica footprint | `mib_snapshot_clones_remain_rope_backed_during_local_edit` | pass | 1 MiB local edit retains cheap rope-backed snapshots; no latency threshold is asserted. |
| No GUI editor dependency | root package and client boundaries, `verify.sh quick` | pass | No Tauri/Monaco dependency or editor widget was introduced. |

## 3. Production implementation evidence

`DocumentController` receives a frontend-neutral `DocumentTransport`; native
`LocalSocketClient` and the TUI `CoreClient` adapter both use the same protocol
controller. `apply_local` validates through `codegg-document`, updates the
optimistic rope synchronously, queues a stable change ID, then schedules at
most one short debounce task. `flush` sends one daemon change at a time. A
transport error disables automatic network flushing while preserving the queue;
reconnect installs a replacement transport, reattaches the scoped path,
reacquires writer ownership when requested, and compares revision/text against
queued transaction boundaries.

A reconnect that proves the server already accepted a change drops that
acknowledged prefix. A server snapshot equal to the exact pending base permits
retry with unchanged IDs. Any unrelated text/revision preserves the replica
and enters resync/recovery. A missing document identity enters
`GoneWithLocalDraft`. The controller does not perform operational transforms
or automatically write drafts to disk.

`TuiDocumentSession` wraps an `Arc<DocumentController>` and has separate
presentation placeholders only. `snapshot()` returns a cheap cloned
`DocumentSnapshot`; it does not stringify a MiB buffer during ordinary reads.

## 4. Verification executed

### Commands run

```bash
rtk cargo test -p codegg-document --locked
rtk cargo test -p codegg-client --locked
rtk cargo test --target aarch64-apple-darwin --test document_client_trajectory --locked
rtk cargo test --target aarch64-apple-darwin -p codegg document_service::tests --lib --locked
rtk cargo test --target aarch64-apple-darwin -p codegg --lib tui:: --locked
rtk cargo test --target aarch64-apple-darwin -p codegg --lib tui::document_session --locked
rtk cargo test --target aarch64-apple-darwin -p codegg --features lsp-test-support --test document_lsp_integration --locked
rtk cargo clippy --workspace --all-targets --locked -- -D warnings
rtk scripts/verify.sh quick
rtk python3 scripts/check_work_plan_repository_binding.py
rtk git diff --check
```

The root test commands used the installed Rust 1.89 arm64 toolchain. The default
x86_64/Rosetta linker initially failed because installed compression/iconv
libraries are arm64; the arm64 reruns passed.

### Results

- `codegg-document`: 5 transaction tests passed.
- `codegg-client`: 40 unit/integration tests passed, including controller retry,
  coalescing, reconnect, restart, and 1 MiB snapshot cases.
- `document_client_trajectory`: 2 writer/observer tests passed.
- `document_service::tests`: 7 daemon service tests passed.
- `tui::` library tests: 961 passed; the new `TuiDocumentSession` test was also
  run separately and passed.
- `document_lsp_integration`: 1 fake-LSP test passed.
- Workspace Clippy with `-D warnings`, `scripts/verify.sh quick`, repository
  binding checks, formatting, and diff checks passed.

M005-C separately qualified checked save, external disk conflict, preview
rejection, and save/edit/workspace-lock races in its immutable closure record.
The M005-D controller trajectory qualifies the frontend state transitions;
the fake-LSP trajectory and daemon mutation tests qualify the daemon/LSP edges.

## 5. Invariant review

- Daemon `DocumentService` remains canonical for accepted unsaved text and
  revision; the controller owns only its optimistic frontend replica.
- LSP remains a managed mirror; disk remains the durable source authority.
- No agent file-tool semantics read from the unsaved editor buffer.
- One writer per document; status polling is project-authorized metadata only.
- Cursor, selection, viewport, keymaps, and rendering are not text authority.
- Pending transactions, inserted bytes, protocol edits, and document text are
  bounded. One controller owns at most one scheduled flush task.
- Local draft recovery remains ephemeral and process-local.

## 6. Failure and recovery review

A transport loss retains local text and pending transactions, disables
background retries, and marks the controller disconnected. Reconnect uses a
new transport and lease. Stable IDs make uncertain retries idempotent. A
server snapshot matching neither a pending result nor the exact pending base
is surfaced as resync-required without overwriting local text. A daemon restart
or missing document leaves the draft available as `GoneWithLocalDraft`.

Checked-save conflict retains local text and remains conflicted until explicit
reload or other user-directed recovery. Reload refuses while pending changes
remain.

## 7. Migration and compatibility review

The protocol additions remain additive; this milestone adds no storage
migration. `codegg-client` adds only the `codegg-document` and async-trait
workspace dependencies. Existing CoreClient and local socket APIs remain
unchanged. No GUI, WebDriver, or live language server is required.

## 8. Security review

The controller supplies only scoped project/workspace/document identifiers and
relative paths to existing daemon operations. It does not bypass document
authorization, writer leases, workspace mutation locks, or protocol bounds.
Document bodies are not added to events or audit metadata. Agent tools remain
disk-authoritative.

## 9. Documentation and operations

Updated `architecture/document.md`, `architecture/client.md`,
`architecture/protocol.md`, and `architecture/tui.md`. The parent desktop/IDE
roadmap and registry are updated with M005 closure and M006 planning eligibility.
Limitations recorded: one writer, no persistent draft recovery, no agent reads
from unsaved buffers, no CRDT/OT, no graphical editor, and no LSP preview apply
into dirty buffers.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| none | No unresolved high/medium M005-D finding | none | none |

## 11. Roadmap disposition

M005-D and parent M005 are closed. All four M005 implementation plans are
closed. The parent M006 TUI IDE vertical slice is eligible for fresh planning
and a TUI presentation audit; no M006 implementation plan is created here.
Graphical IDE work remains long-term/deferred. The separate M002 Windows
qualification condition is unchanged and does not block the completed TUI-first
M005 line.

## 12. Registry updates

- Mark M005-D and parent M005 closed and link this closure record.
- Mark M006 eligible for fresh TUI IDE vertical-slice planning; remove its
  strict-M005 blocker and do not write an M006 implementation plan here.
- Keep graphical IDE expansion deferred and M002 Windows evidence independent.
