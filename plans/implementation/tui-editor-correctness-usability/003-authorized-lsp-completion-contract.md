# TUI Editor Correctness/Usability M003 — Authorized LSP Completion Contract

Status: implemented; closed

Implementation: `44671d690`
Closure: `plans/closure/tui-editor-correctness-usability/003-status.md`

Prior status was `blocked pending acceptance of proposed ADR-0021`. The readiness
investigation confirmed no LSP synced-version witness exists yet and that a
daemon last-pushed-revision ledger is buildable inside this milestone's scope;
that design is covered by the acceptance record in the ADR.

Repository baseline: `main` `6b611b5ca7f019783a91f04530d5ae56124f7ee9`; verify current HEAD at activation.
Roadmap: `plans/subsystems/tui-editor-correctness-usability-roadmap.md` M003.
Predecessors: M005 document ownership, M006-B LSP read and M006-C TUI read interface (closed).
Required architecture decision: `plans/adrs/ADR-0021-native-editor-completion-read-and-local-edit.md` (proposed, recommended Option B). Accepted ADR-0008, 0010, 0011, 0012, 0018 remain binding.
Primary class: capability / native protocol and authorization infrastructure.

## 1. Objective

Add a bounded, inert, warm-server-only daemon LSP completion **read** contract to the shared native protocol and client, supplying typed candidates with explicit supported/unsupported local-edit metadata and a trustworthy document/revision binding. This milestone exposes no popup and applies **no edits**; M004 owns user interaction and controller mutation.

## 2. Readiness gate and current evidence

C001/C002 may progress independently. M003 cannot begin before ADR-0021 is explicitly accepted or superseded with an equivalent binding/authority decision. `LspReadOperation` currently has no Completion variant. `egglsp::LspOperations::completion` returns raw `lsp_types::CompletionItem` but internally calls `get_or_create_client`. `completion_bounded` emits model-facing `CompletionCandidate` and intentionally strips `textEdit`, `additionalTextEdits` and `command`; it must **not** be reused as an editor insert authority. `src/core/daemon_lsp.rs` shows the warm-only pattern `find_existing_client_for_root_hint`, daemon-root containment, typed request/response and `file.read` gate. `codegg-client` has typed LSP read surfaces. `DocumentController` and daemon document service manage revisions and writer leases.

## 3. Invariants

- The daemon is the sole LSP server owner; a request must never start one, even transitively.
- Daemon resolves session→project→workspace, attachment and target; caller-supplied path/revision are locators/preconditions, never authority.
- Completion fetch requires `via_session + file.read`, checked before any language server interaction. Do not make a weaker `project.observe` shortcut.
- Returned candidates are inert data. Raw LSP `command` cannot be executed; a candidate containing snippets, additional/multi-file edits, or unsafe ranges is explicitly non-applicable in v1, not silently changed to prefix substitution.
- The **exact synchronized document snapshot** must be bound to the request/result. Client optimistic revisions are not assumed to be identical to daemon LSP versions until acknowledged. An unsynchronized draft must yield an explicit pending/stale result, never an "applicable" candidate.
- All output is bounded and does not enter durable session or chat history.
- M006-C and ADR-0012 existing read behavior remains unchanged. The existing agent LSP preview/write path is untouched.

## 4. Scope and non-goals

**In:** ADR acceptance gate; narrow typed native `LspCompletionGet` request and response (or accepted equivalent); daemon completion family read and authorization descriptor; warm-only operation with filtered normalized simple text edits; revision/attachment binding; client accessors; version skew/size limits; targeted tests, guard and docs.

**Out:** TUI completion popup/keybindings/mutation; model tool `completion_bounded` behavior; code-action execution, LSP resolve/write, snippet expansion, additional edits/imports, disk writes, new indexing daemon, whole-buffer content transfer, new persistent storage or scheduler authority.

## 5. Required production changes

**Core/domain:** inspect actual `DocumentService` attachment IDs, current synced content and version APIs; select existing canonical document identity fields and revision only after ADR acceptance. Return a typed stale/unsynced result when LSP may have seen a different snapshot. Refactor egglsp operation to take an already-warm client (or use a daemon adapter bound to verified warm key); never call create-or-get on a cold request.

**Protocol:** add a dedicated bounded request/result DTO family pursuant to approved ADR-0021, with explicit session, workspace-relative document, real attachment identity, synced version token and cursor position. Result status must distinguish Ready with zero items, NotReady, Stale/Unsynced, Unsupported and transport/authorization Error. Cap at 100 candidates and 256 KiB total result; cap each label/detail/edit string and preserve server order/truncation. Edit representations include only supported single-document plain-text insert/replace ranges with UTF-16 coordinates, guarded against out-of-snapshot ranges. No unbounded JSON passthrough. Old daemon must explicitly reject unsupported variant.

**Authorization/security:** add explicit operation descriptor in `crates/codegg-core/src/authorization/policy.rs`, route through `src/core/daemon_family.rs` and `src/core/daemon_lsp.rs`, and update `architecture/authorization.md` plus `scripts/check_authorization_matrix.py` coverage as necessary. Bound inputs before work, daemon-resolve containment; preserve document redaction and no command execution.

**Client:** add narrow accessor in `crates/codegg-client/src/lsp.rs` / shared frontend client adapters and tests. The client must not synthesize edits, replay a request after reconnect, or infer a success from old-server absence.

**Storage/migrations:** none.

## 6. Ordered work packages

A. Record ADR-0021 acceptance and contract freeze. Inspect `codegg-document`, `codegg-client`, `src/document_service.rs`, `src/core/daemon_documents.rs` and LSP sync state. Write an explicit mapping table between controller revision, daemon attachment identity and LSP document version before coding. If no reliable synchronization witness exists, stop and plan that prerequisite separately; do not guess revision equality.

B. Define bounded typed DTOs and negative validation cases (missing/invalid path/attachment/version, malformed cursor position, excessive data, unsupported snippet/edit commands). Decide how unsupported candidate metadata is presented while preserving inert label/detail. Add protocol serde/compat tests.

C. Implement daemon dispatch, authorization and warm-only server query. Ensure `egglsp` raw completion call does not transitively spawn a client. Normalize LSP CompletionItem and CompletionList, preserve deterministic order and `is_incomplete`/truncation distinctions. No mutation or LSP command execution.

D. Add client accessor and fake-daemon/real-service trajectory proving Ready/NotReady/Stale, file.read refusal, wrong project/root, document conflict, synchronized draft and version skew, plus cancellation/reconnect semantics.

E. Update architecture/protocol/client/LSP docs and produce closure record. M004 can begin only after this milestone strictly closes.

## 7. Failure, cancellation, restart and contention

Keep at most one active completion request per attachment/generation on the frontend; bound daemon-side parallel requests via existing admission limits. Late responses for old attachments, cursor positions or revisions cannot become applicable suggestions. Transport disconnect cancels/advises retry but does not auto-insert anything. Restart drops ephemeral request/candidates; no durable completion replay. Cold LSP returns NotReady and zero server starts; unacknowledged local edits return Stale/Unsynced. Permission revocation and workspace rebound fail closed.

## 8. Compatibility and migration

Native protocol extension is additive; old clients are unaffected, old daemons return explicit unsupported operation. No database migration or persisted candidate records. Reuse existing `document.v1` canonical identity; do not create a new text owner. Preserve existing `LspReadGet` and `LspPreviewApply` request shapes unless approved ADR specifically authorizes a change.

## 9. Required tests

New `tests/lsp_completion_trajectory.rs` (create as part of implementation) must cover: warmed server returns bounded typed simple completion; cold server zero starts; unauthorized file/project returns denied not NotReady; stale controller version; dirty unsynced buffer; UTF-16 surrogate boundary; label vs textEdit; additionalTextEdits, snippet and command marked non-applicable; candidate count and byte caps; deterministic order/incomplete; unexpected wire payload; old daemon unsupported; scope confusion, reconnect/cancel and overlapping requests. Assert that **no document text or disk bytes change** during any read.

~~~bash
cargo test --lib -p codegg-protocol lsp --locked
cargo test --lib -p codegg-client lsp --locked
cargo test --lib -p egglsp completion --locked
cargo test --test lsp_completion_trajectory --locked
cargo test --test lsp_read_surface_trajectory --locked
python3 scripts/check_authorization_matrix.py
python3 scripts/check_tui_editor_text_authority.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
~~~

Focused new test target does not exist yet; add it. Verify exact package/test spellings against HEAD. Require exact-SHA hosted CI; document unrun platform/terminal cases rather than invent success.

## 10. Acceptance criteria

1. ADR-0021 or superseding decision accepted, with documented mapping between controller attachment, daemon document revision and LSP synced version.
2. One typed authorized read returns bounded inert candidates and explicit Ready/NotReady/Stale/Unsupported states.
3. Cold request cannot launch LSP; denied request never reaches it; controller/disk never mutated by read.
4. Unsupported/snippet/multi-file/edit-command data remains non-applicable, not silently discarded into unsafe simple insertion.
5. Unicode/version/authority and old-daemon negative trajectories pass; no hidden fallback.
6. New client API and protocol docs/tests/guards/hosted evidence complete.

## 11. Stop conditions

**Mandatory stop before production code** if ADR-0021 is still proposed. Stop if no trustworthy document synchronization token can be obtained without a cross-subsystem contract; if new requests would bypass file.read; if success depends on auto-applying server commands/additional edits; or if a model-facing completion helper is silently promoted into an editor mutation surface. Escalate the missing decision rather than improvise.

## 12. Documentation and closure

Update `architecture/lsp.md`, `architecture/protocol.md`, `architecture/document.md`, `architecture/client.md`, `architecture/authorization.md`, `architecture/tui.md` as relevant. Update roadmap/registry. Create `plans/closure/tui-editor-correctness-usability/003-status.md` with ADR status/decision, exact DTO/authority contract, requirement matrix, cold/warm and revision race tests, security checks, hosted exact-SHA runs, known limitations and disposition.

## 13. Handoff notes

Do not modify the already-accepted ADR-0012 to retrofit an editor completion decision. A separate **proposed** ADR is deliberately part of this handoff. M003 closure is about read semantics only; defer user-trigger UX and application to M004.
