# M005 — Latency-Sensitive Editor Syntax/Structural Operations

Status: blocked — ADR-0034 selection (hard); this is a **conditional implementation design**, NOT authorization to add a parser.

Primary class: capability/infrastructure.

Repository planning baseline: `80171f45295bebb9ab0b521262c0b295acb93074` (`main`, 2026-10-27). Reinspect live branch before implementation. Roadmap: `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md`; governance: `plans/003-planning-process.md`; long-term specification §§4,13,24–25.

Decision gate: `plans/adrs/ADR-0034-editor-syntax-tree-dependency.md` is **proposed** and currently recommends daemon LSP structural reads. User direction favors frontend-local latency-sensitive *syntactic* editor behavior, which is not equivalent to permitting a Tree-sitter dependency. This plan is executable as written **only if ADR-0034 is accepted or superseded with a local-parser selection**. If daemon-only structural reads are selected, stop and replace this implementation plan with one for that outcome rather than claiming M005 closed. Coordinate specifically with `plans/subsystems/tui-editor-helix-parity-roadmap.md` M014; do not duplicate a separate M014 implementation.

## Objective

Provide fast syntax-sensitive selection and structural motion in TUI, with a portable strategy for desktop, without confusing lexical/syntactic presentation with semantic/cross-file LSP intelligence or creating a second authoritative editor document.

## Current evidence

`src/tui/commands/editor.rs`, `src/tui/editor_lsp.rs` and existing TUI editor motions consume optimistic `DocumentController` snapshots. Desktop `apps/desktop/src-tauri/src/document.rs` supplies bounded viewport state. `codegg-document` is a pure byte-indexed rope/edit library. `egglsp` owns semantic language-server processes, not Helix AST nodes. ADR-0034 documents twenty missing syntax-aware bindings and the tradeoffs of local Tree-sitter vs server structural requests.

## Decision and design package

1. Resolve ADR-0034 explicitly before code: separate *local syntax gestures* (brackets, node range/parents/siblings, syntax-aware text objects, lexical highlight/indent) from *semantic LSP* (hover, definitions, symbol/type information, diagnostics, cross-file rename). Adopt one source for each behavior: never secretly fall back between two inconsistent AST definitions for the same binding.
2. If local selected, qualify a small optional parser/grammar set with exact MSRV, per-platform build cost, binary size and CPU/memory measurements; a frontend-neutral pure syntax snapshot library is preferable to a TUI-only parser where reusable, but it MUST NOT retain independent canonical source text or gain file/process authority. Do not force a heavyweight C grammar dependency onto every build without an accepted cost decision.
3. Derive parse state only from the existing immutable document snapshot/optimistic edit transaction; version it by document attachment and revision/generation. Incremental parse off the render path; bounded per-frame slice/viewport, cancellable background parse, stale-generation discard, resource ceilings and fallback to byte/grapheme motions. Keep UTF-8 bytes vs graphemes vs LSP UTF-16 coordinate conversions explicit.
4. Deliver one vertical Helix M014 structural selection family in the TUI (e.g., expand/shrink node), including clear unsupported state for unbundled language, while maintaining all non-syntax bindings. Port the pure frontend behavior to the desktop only where it can consume exactly the same document replica safely; no second full text buffer in renderer/host, and no GUI browser dependency on spawning LSP.
5. Add capability/availability presentation: local syntax can continue under a cold language server; semantic cross-file actions still show `NotReady`, `Unsupported`, etc. Avoid claiming syntax AST is semantically equivalent to LSP symbols.

## Verification

1000 local incremental edits including invalid UTF-8 boundary requests (must reject), Unicode combining/surrogate edge cases, huge lines, malformed syntax, rapid typing with stale parse completion, cancelled tab change, no grammar, no language server, and daemon outage. Measure key-to-paint p50/p95 with/without grammar and prove no parse or IPC blocks redraw. Run focused codegg-document/TUI tests, static editor text-authority and keymap guards, root `scripts/verify.sh` supported gate and desktop gate if changed. Qualify actual grammar MSRV/build sizes prior to acceptance.

## Storage/security/compatibility

No wire or durable schema changes and no new filesystem/process authority in a frontend. No LSP commands or checked mutations exposed. The local parser is *only* derived presentation state and must not be used to authorize agent file operations. Fallback for unsupported grammar is explicit and cheap.

## Acceptance and stop

If ADR-0034 remains proposed or selects daemon structural reads, do not implement this candidate. With a local-parser decision, close only after one exercised TUI syntax-node gesture, bounded latency evidence, no second editor text, correct stale discard and documented desktop portability (actual desktop delivery may require a scoped follow-up). Record under `plans/closure/daemon-lsp-frontend-convergence/005-status.md` and reconcile Helix M014's status **without modifying past closures**.
