# M002 — Cross-Frontend LSP Parity and Latency Discipline

Status: blocked — M001 strict closure (hard)

Primary class: capability/polish.

Repository planning baseline: `48929a172eaf5267b8d1a0255b1a6ac25f6a0837` (requested branch tip before implementation, rebased on `origin/main` at `80171f45295bebb9ab0b521262c0b295acb93074`). Source roadmap: `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md`. Planning governance: `plans/003-planning-process.md`. Do not edit accepted ADRs or historical closure records.

Accepted ADRs: ADR-0010, ADR-0011, ADR-0012, ADR-0018, ADR-0021. Explicitly depends on `001-shared-lsp-client-coordination.md`. No ADR-0034 parser dependency: this milestone optimizes existing asynchronous LSP reads, not syntax-tree editing.

## Objective

Migrate remaining TUI and desktop editor LSP gesture request coordination to the shared M001 coordinator, preserving frontend-specific render state and eliminating repeated in-flight requests and obsolete-result flashes. Establish a measured key-to-paint and request-to-result baseline.

## Evidence and correctness constraints

The TUI already has `EditorLspState`, `EditorCompletionState` and direct `LspReadGet`/completion callers; desktop has `desktop_document_diagnostics`, `desktop_document_completion`, `desktop_document_navigate` with separate DTO-to-view mapping. The daemon's `NotReady` means cold, not empty; `Unsynced` completion means a revision witness is missing. `LspReadGet` and `LspCompletionGet` differ legitimately. Do not erase those differences or claim the GUI offers all TUI semantic-token affordances if unimplemented.

## Work packages

1. Re-audit actual user gestures and event delivery in each frontend. Create a coverage matrix (diagnostics push/pull, hover, goto/refs, symbols, semantic tokens, completion, format/rename previews) identifying native daemon route, UI behavior, and absence of route. Exclude unserved slash commands (M003/M004).
2. Move all supported editor read issuance into the M001 coordination contract. Keep TUI and desktop adapter/view normalization separate where presentation genuinely differs. Put request lifecycle, reconnect and terminal response semantics in `codegg-client`; keep popup layout, status wording and keymap in respective frontends.
3. Debounce high-frequency completion/hover with leading/latest policy appropriate to each gesture; cap per-editor concurrency, collapse equivalent pending reads within the same scoped revision, cancel or ignore stale requests on edits, cursor movements, tab/session/worktree switches, writer-lease loss and daemon restart. Never duplicate a reissued mutating request. Do not retry `Unsupported` or `Ambiguous` as if warming. `Unsynced` may requery only after a confirmed acknowledged document-sync transition; no hot spin.
4. Keep keystroke-to-render fully local; no LSP readiness/startup work on the interactive path. Semantic tokens and diagnostic decorations must be tied to displayed document generation; reset after stale or disconnect. Capture bounded UI latency histograms in reproducible tests, not telemetry containing source text.
5. Provide a two-client parity fixture: TUI and desktop display `Ready`, `NotReady`, `Stale`/stale-discard, `Unsynced`, `Unsupported` and diagnostic gap recovery according to each feature's legal status vocabulary. Prove one daemon LSP generation for the same root/server/profile, not one process per UI.

## Test / verify

`cargo test -p codegg-client --locked`; editor-specific root tests; `cargo check -p codegg --locked --features server`; root quick/full verification per `scripts/verify.sh` supported modes; desktop host/renderer typecheck and E2E from its documented gate. Include fake delayed response, 100 rapid edits, mixed UTF-8/UTF-16 text, two open clients, restart and stream lag; assert all old results discarded and no blocking render. Record host and sample count; compare p50/p95 of key-to-paint and request-to-result before/after with warm and cold server and loaded CPU. If instrumentation needed, keep opt-in and content-free.

## Non-goals / stop

No Tree-sitter dependency or syntax-node motion, no protocol redesign, no host filesystem access, no second LSP process, no optimistic apply of unsupported completion payloads, no arbitrary hard-coded latency pass threshold. If correct flow requires a new daemon revision-tagged semantic DTO, stop for separate protocol plan and ADR review; don't smuggle a new field into M002.

## Closure

`plans/closure/daemon-lsp-frontend-convergence/002-status.md`: audited gesture matrix, changed call sites, before/after histograms, all focused/hosted test runs, stale/disconnect/permission evidence and remaining GUI/TUI asymmetries. Dependency must be marked closed in the registry before work is declared ready.
