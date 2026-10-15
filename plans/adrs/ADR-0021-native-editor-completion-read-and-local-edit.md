# ADR-0021: Native Editor Completion — Authorized Read and Revision-Checked Local Edit

Status: accepted (Option B)

Date: 2026-10-13

Acceptance: 2026-10-13 — project maintainer accepted the recommended Option B
(dedicated typed completion read + controller-owned application) with the
M003 readiness proviso that the daemon last-pushed-revision ledger is built
inside M003 scope. No implementation authority existed before this date.

Decision owners: project maintainers

Related specifications: `plans/000-long-term-specification.md` (TUI-first, locality, correctness, project authorization), `plans/001-terminology-and-domain-model.md`, `plans/003-planning-process.md`.

Affected roadmap: `plans/subsystems/tui-editor-correctness-usability-roadmap.md`, M003/M004.

Related accepted ADRs: ADR-0008 (LSP agent preview/write authority), ADR-0010 (shared frontend client), ADR-0011 (document owner and replica), ADR-0012 (warm-only, daemon-owned LSP read), ADR-0018 (validated workspace-relative navigation targets).

## Context

M006-C exposes hover, definition, references, symbols and semantic tokens through the daemon-owned `LspReadGet` read family. It intentionally has **no editor completion**. `egglsp::LspOperations::completion` returns raw `lsp_types::CompletionItem`; `completion_bounded` is a model-facing **read-only** projection and drops `textEdit`, `additionalTextEdits` and `command`. This second method cannot safely drive IDE insertion. Its raw helper also calls `get_or_create_client`, so routing through it unchanged could violate ADR-0012's warm-only rule.

The editor's single text owner is `DocumentController`, and `src/tui/commands/editor.rs::submit_edit` applies a `TextTransaction` and records a bounded inverse only after acceptance. An LSP completion response describes a potentially stale Unicode range over the document the server examined; it does not authorize the server to modify the workspace.

This ADR decides the interface between an inert completion read and a human-approved local edit before the milestone adds a new native public protocol.

## Decision drivers

- Match the server's actual edit range and Unicode position encoding rather than guessing a preceding-word prefix.
- Use the **same** document revision that was synchronized to the daemon-owned language server. A local optimistic revision may not yet be synchronized; do not silently compare unrelated revision counters.
- Prevent LSP-supplied `command`, `additionalTextEdits`, out-of-workspace edits and snippets from becoming an automatic mutation path.
- Retain ADR-0012's warm-only service, `via_session + file.read` and daemon-resolved workspace.
- Bound item count, string bytes, total wire size and request concurrency; old daemon responses fail explicitly, not as empty success.
- Reuse the frontend's local edit + undo/redo pipeline, including writer lease and dirty conflict semantics.

## Considered options

### A — Candidate label/prefix insertion only

Return display text and insert a guessed identifier prefix. Smallest transport and UI, but incorrect for replacement ranges, member access, Unicode offsets and snippets. A read-only/model-facing `CompletionCandidate` does not convey edit semantics. Rejected for editor use.

### B — Dedicated typed completion read + controller-owned application (recommended)

Introduce one typed, bounded completion request/response family, kept separate from mutation/apply. It carries an explicit synchronized attachment/revision token and structured *inert* single-document edit metadata. The TUI validates the candidate against the current controller snapshot and applies exactly one `TextTransaction`. Unsupported items remain visible but cannot be applied. No server-command dispatch.

A dedicated family avoids changing the semantics of existing `LspReadRequestDto` shapes and keeps revision/application preconditions explicit. It adds a protocol variant, authorization matrix row and version compatibility work.

### C — Add `Completion` to `LspReadOperation` and infer apply edits client-side

Reuses existing family but invites accidental use of its point request (which has no document revision or edit contract) as authoritative. It can only be safe if it gains the same explicit binding and DTO validation as B; then it has comparable protocol complexity. Not selected.

### D — Ask daemon to apply completion edits directly

Combines user-interface preference with daemon filesystem/document authority; risks bypassing the controller/undo path, makes preview and writer-lease semantics harder, and crosses ADR-0008/0011 ownership. Rejected.

## Decision (accepted: Option B)

1. Define a new bounded `LspCompletionGet` native read and `LspCompletionResult` response, with an explicit request DTO that binds `session_id`, authorized workspace-relative document path, actual document attachment identity, synchronized snapshot revision/version token, cursor UTF-16 position and bounded trigger intent. The implementation agent must use the real `document.v1` attachment identifiers/revisions, not fabricate a parallel counter. A request whose local revision has not been synchronized to the daemon cannot yield applicable suggestions; return a typed stale/not-ready outcome or wait asynchronously for acknowledged synchronization, without blocking rendering.
2. The daemon resolves the session/project/workspace and attachment, authenticates the request with the existing `ViaSession + FileRead` descriptor and the same containment helper as M006-B, then confirms a compatible **already-warm** language server and document snapshot. No call path may transitively reach `get_or_create_client` for a cold request; refactor the `egglsp` helper as needed.
3. Return at most 100 items, with individually bounded label/detail/filter/insert text, a 256 KiB response budget and a `truncated` flag. Preserve deterministic server order, and expose explicit `Ready`, `NotReady`, `Stale`, `Unsupported` and authorization/error outcomes without conflating emptiness. No raw arbitrary JSON on the frontend.
4. A candidate may be applied only if it is an ordinary single-document **plain-text** insert/replace with an explicit validated LSP range (or a spec-proven insertion-at-cursor shape), bounded replacement text, no `additionalTextEdits`, no command, no cross-document edits and no snippet placeholders. Unsupported variants remain visible with an explanation, not silently downgraded to label insertion. Never run a language-server-provided command.
5. The frontend validates UTF-16→UTF-8 boundaries against the exact same current controller revision and attachment identity, verifies range ordering/containment, constructs exactly one local transaction through the existing editor mutation path, then records one bounded undo step only if the controller accepts. No auto-accept, no hidden disk write, and no `LspPreviewApply`.
6. On every typed edit, resync, reopen, session/tab switch, daemon reconnect, stale result, lost writer lease or permission change, invalidate the candidate set and disarm Enter. The result is a suggestion, not authority. Terminal/side-panel focus, mode changes and input routing remain governed by the existing top-modal and editor focus paths.

## Consequences

**Positive:** Precise editor insertion with no second document buffer, explicit stale refusal and reuse across native clients; no new daemon-side mutation authority.

**Negative:** New typed protocol and authorization row, extension of the `egglsp` raw completion path, and stronger revision/synchronization sequencing; some valid LSP completions involving additional edits/snippets intentionally cannot be accepted initially.

**Deferred:** Separate ADR/workstream if snippet expansion, additional import edits, code actions or multi-file completion application become desirable. Do not infer permission from candidate visibility.

## Compatibility and migration

Additive native request/response only; existing `LspReadGet` / `LspPreviewApply` semantics preserved. Version skew must yield an explicit unsupported-operation outcome on old daemons; never fall back to a guessed local insertion. No new durable storage or migrations. Request/result DTOs have explicit hard caps; a client must discard responses with unexpected document identity/revision.

## Security, contention and recovery

Use `file.read` for fetching suggestions and existing controller writer checks for local application. Sensitive completion detail and source snippets are redacted/omitted from telemetry and durable session projections. Reject malformed Unicode positions, oversized edits, snippet modes, overlapping/multi-document edits, and stale editor generations. Concurrent requests: bounded outstanding per active editor, latest eligible generation wins, old responses discarded. Restart/reconnect clears candidate state and allows deliberate re-query; no replayed acceptance.

## Required verification for acceptance and implementation

Approval requires selecting one option and freezing typed DTO semantics and synchronization contract. Implementation must include negative tests for cold LSP (zero startup), unauthorized project/file, stale/superseded revision, incomplete/unsynced optimistic draft, UTF-16 surrogate boundaries, server-supplied command, snippet and `additionalTextEdits`, oversized payload, duplicate completion, late arrival after project switch, writer-lease loss, and old daemon unsupported response. A successful accepted suggestion must be undoable/redone via the same controller path as a typed character.

## Supersession

None. This ADR supplements rather than rewrites ADR-0012 and ADR-0011.
