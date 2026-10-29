# M005 — Daemon-Backed Structural Selection (Helix M014)

Status: closed — ADR-0034 Option B accepted 2026-10-28; stale-response and large-document responsiveness qualification passed 2026-10-29

Primary class: capability/infrastructure.

Repository planning baseline: `80171f45295bebb9ab0b521262c0b295acb93074` (`main`, 2026-10-27). Reinspect live branch before implementation. Roadmap: `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md`; governance: `plans/003-planning-process.md`; long-term specification §§4,13,24–25.

ADR-0034 accepts Option B: extend the daemon-owned read surface with bounded structural ranges. Do not add a frontend parser. Coordinate with the Helix M014 entry; do not claim per-kind Tree-sitter parity beyond the structural range operations actually delivered.

## Objective

Provide asynchronous syntax-structure expansion and contraction in the TUI through the authorized daemon LSP path, without adding a parser, blocking redraw, confusing structural ranges with cross-file semantics, or creating a second authoritative editor document.

## Current evidence

The typed `SelectionRange` operation is implemented through `codegg-protocol`, `egglsp`, the daemon warm-read route, and the frontend-neutral client operation set. `src/tui/commands/editor.rs` binds Alt+O/Alt+I to asynchronous expand/shrink; response admission checks document revision and cursor position before changing the selection. The TUI does not await LSP I/O on the key path. Fake-daemon trajectories cover a bounded nested range chain, explicit unsupported behavior, cold-root no-spawn behavior, reversed-range rejection, depth truncation, and oversized-response rejection. The App-path regression test added during this closeout checks request issuance, unchanged document while pending, and delayed-response suppression after cursor movement.

The focused `codegg` App-path test passes on ARM64 and verifies that Alt+O issues a `SelectionRange` request without changing the document while pending, local cursor input is handled before the response, and delayed results are discarded after cursor movement, a 128 KiB document edit, route change and server-generation change. The 71-test `lsp_test_support` family passes, including bounded selection ranges, cold no-spawn, unsupported capability, reversed-range refusal, depth truncation and oversized-response refusal. The authorization matrix, editor-key authority guard, editor text-authority guard, and root `scripts/verify.sh quick` pass. The visible desktop session E2E passed 10/10; its local key-to-paint evidence is independent of the TUI structural-selection proof. The broader Helix M014 syntax-node text objects and syntax-aware bracket match remain outside M005; ADR-0034 Option B authorizes only the daemon structural-range operation.

## Decision and design package

1. Add a typed `SelectionRange` read operation through `codegg-protocol`, `egglsp` and the existing daemon/client route. Resolve the server only through the warm-client path, validate the authorized workspace-relative document, and cap both requested positions and returned nesting depth/range count.
2. Bind every returned chain to the active attachment, document generation/revision, cursor position and server generation. Return typed `NotReady`, `Unsupported`, `Ambiguous`, `Stale` and truncation outcomes; never return source text or an unbounded raw JSON-RPC value.
3. Implemented: one Helix-style expand/shrink structural selection pair in the TUI uses the async editor request path. Preserve and qualify the current-selection-while-pending behavior and stale discard after cursor, document, route or server-generation changes. No redraw or input handler may await process startup or LSP I/O.
4. A server without `textDocument/selectionRange` reports a visible unsupported state. Do not approximate syntax selection using a byte delimiter fallback on the same chord. Preserve existing byte/grapheme motions and all non-syntax bindings.
5. Keep desktop behavior unchanged unless it can consume the same typed result and revision contract through its existing document replica. No second text buffer or browser-side server process is in scope.

## Verification

Fake-stdio tests must cover range-chain decoding, maximum depth/truncation, invalid or reversed ranges, UTF-16 positions, oversized responses, path escape, no warm server, unsupported capability, delayed response after edit/cursor/tab/route change, and restart generation change. A rapid-input trajectory proves the editor remains responsive while a request is pending and never paints a stale range. A large-file/read-response test proves work and payload stay bounded. Run the focused `egglsp`, root TUI, authorization-matrix and keymap/text-authority guards, then root and desktop gates as affected.

Current host qualification: the 71-test `lsp_test_support` family passed on
ARM64 macOS and covers a warm bounded nested range, explicit unsupported
capability, cold no-spawn behavior, reversed-range refusal, depth truncation,
and oversized-response refusal. The App-path structural-selection test passed
with a 128 KiB document and stale-result checks after cursor movement, edit,
route change and server-generation change. Root quick verification passed;
details are in `plans/closure/daemon-lsp-frontend-convergence/005-status.md`.

## Storage/security/compatibility

This is an additive read-only protocol operation under the existing `file.read` session/workspace scope. It introduces no storage migration, parser dependency, mutation authority or frontend process authority. Returned ranges are derived display state only and never authorize agent file operations.

## Acceptance and stop

Close only after the typed operation, a live TUI structural-selection gesture, bounded response and latency evidence, explicit no-server/unsupported behavior, correct stale discard, and root verification pass. Accept ADR-0034 Option B only after its required contract and negative evidence are linked. Reconcile Helix M014's status without editing historical closure records; per-kind objects not served by `SelectionRange` remain explicitly unclaimed and require their own bounded design if still desired.
