# TUI Editor Helix Parity M014 — Syntax-Node Text Objects and Matching

Status: ready for handoff
Repository baseline: `52a6dabf365cebdf5e6e7b1a6c33ce1ff785bbc6`

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md#milestone-014--syntax-tree-text-objects-and-structural-selection`
Long-term requirements: `plans/000-long-term-specification.md` §§4, 13, 24–25
Applicable ADRs: ADR-0034 Option B; ADR-0011; ADR-0012; ADR-0021
Primary class: capability

## 1. Objective

Complete the M014 syntax-dependent part of the TUI editor's `m` grammar using
the daemon-owned, read-only LSP structural-read contract. Cover text objects
`f` (function), `t` (type), `a` (argument/parameter), `c` (comment), `T`
(test), `x` (generic syntax element), and replace M009's delimiter-table
`m m` matching with syntax-aware matching. Keep all LSP work asynchronous,
bounded, warm-only, revision-bound, and visibly unavailable when the active
server cannot supply the required structure. Preserve M009's byte-derived
word, quote, and delimiter-pair objects unchanged.

## 2. Why this milestone is ready

- ADR-0034 Option B is accepted and explicitly selects daemon-owned structural
  reads without a frontend parser.
- M009's byte-derived match-mode grammar is closed.
- M005's typed `SelectionRange` operation and asynchronous Alt+O/Alt+I path
  are closed. `DocumentSymbols` is already a typed LSP read operation and
  `egglsp` already exposes warm-client document-symbol reads.
- M014 was excluded from M005 and M006 closure by design. This is a separate
  TUI capability slice, not a reopening of either closure.

## 3. Current implementation evidence

The editor already implements byte-derived text objects and delimiter-pair
matching in `src/tui/editor.rs`. The `m` prefix grammar and UI result routing
live in `src/tui/commands/editor.rs`. M005 provides a typed, bounded
`SelectionRange` read through the authorized daemon/client path. Typed
`DocumentSymbols` reads are available, but they have not been admitted as
syntax-object candidates in the editor. The accepted ADR permits LSP symbol
granularity to approximate syntax nodes only when that approximation is
documented; it does not permit a frontend parser or raw server-command
fallback.

The first work package must establish which requested object kinds can be
provided accurately by the supported LSP read contract. Standard document
symbols and selection ranges do not necessarily identify parameters,
comments, tests, or matching delimiters. Do not infer that support exists from
the presence of `documentSymbolProvider` or `selectionRangeProvider` alone.

Initial protocol audit: LSP `SelectionRange` returns a range and optional
parent-range chain; the specification says these typically, but not
necessarily, correspond to syntax-tree nodes, and the response has no node
kind ([LSP 3.18 Selection Range](https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.18/language/selectionRange.md)).
`DocumentSymbol` provides symbol kinds and hierarchy, but only for symbols
found in a document ([LSP 3.18 Document Symbols](https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.18/language/documentSymbol.md)).
Semantic tokens can label tokens such as `parameter` and `comment`, but they do
not directly provide the complete syntax-node ranges required for those text
objects ([LSP 3.18 Semantic Tokens](https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.18/language/semanticTokens.md)).
The current typed operations in CodeGG expose these generic reads; no
standard LSP matching-delimiter read is present in the protocol feature set.
So the accepted Option B can support selected symbol/range gestures, but the
full M014 exit condition is not yet established. WP1 must say exactly which
gestures are supportable; completing the rest requires a separate owner
decision if it would add a parser or server-specific operation.

## 4. Invariants that must not regress

1. The TUI owns neither a parser nor LSP process lifecycle.
2. Structural reads use the active attachment, authorized workspace-relative
   document, warm server, document revision, cursor, and server generation.
3. A response for a moved cursor, edited document, changed route, replaced
   server, or superseded request cannot alter the selection or cursor.
4. No render, key-dispatch, or input path awaits LSP I/O.
5. Unsupported structure has a visible typed result. It never silently falls
   back to a byte-derived object with different semantics.
6. Responses are bounded by depth, range count, symbol count, and serialized
   bytes. Truncation is explicit and cannot be presented as a complete match.
7. LSP results are read-only display/selection data and grant no file or write
   authority.
8. Existing word, quote, delimiter-object, and M009 bracket behavior remains
   unchanged until the syntax-aware replacement has a qualified result.

## 5. Scope

**In:**

- an evidence-based capability map for each of the seven syntax-dependent
  gestures;
- typed, bounded structural read DTOs only where existing standard LSP
  operations can support a truthful contract;
- asynchronous `m`-grammar integration and visible unavailable states;
- syntax-aware matching only if the daemon can obtain a real matching pair
  from an approved structural source;
- real-server qualification for rust-analyzer and Pyright where applicable;
- documentation that states the actual granularity and unsupported cases.

**Out:**

- Tree-sitter or any other parser in the TUI/frontend;
- arbitrary `workspace/executeCommand`, private server commands, raw JSON-RPC,
  or frontend process launch;
- claiming syntax-tree parity from generic `DocumentSymbol` or
  `SelectionRange` payloads without validating their semantics;
- changing M009's byte-derived objects or reopening M005/M006;
- multi-selection redesign unrelated to the existing `m` grammar.

## 6. Ordered work packages

### WP1 — Provider and protocol feasibility audit

For each object (`f`, `t`, `a`, `c`, `T`, `x`) and `m m`, record the exact
standard LSP operation, provider capability, response field that establishes
the requested syntax kind, and behavior from installed rust-analyzer and
Pyright versions. Include nested, malformed, incomplete, and large documents.
Separate generic ranges from kind-labelled ranges. Identify any request that
would require server-specific commands or a local parser.

If a requested kind cannot be represented truthfully through the accepted
Option B contract, stop that gesture's implementation and record the precise
missing contract. Do not relabel generic ranges as functions, arguments,
comments, tests, or matching delimiters. Report whether the remaining M014
scope needs a new owner decision before proceeding.

### WP2 — Bounded daemon structural DTO

For only the gestures WP1 proves feasible, add or extend a typed `LspRead`
payload with normalized object kind, document range, optional inner range,
source revision/generation binding, and truncation/unsupported status. Reuse
the existing session authorization, warm-client resolution, UTF-16 checks,
workspace containment, and cancellation path. Add explicit request and result
bounds. Never expose raw server payloads.

### WP3 — Async editor object selection

Bind the feasible `m a` / `m i` object leaves through the existing coordinator.
Keep the current selection while the read is pending. Apply only a complete,
current result for the same attachment, document, cursor and server
generation. Surface `NotReady`, `Unsupported`, `Ambiguous`, `Stale`, and
truncated outcomes in the editor status surface.

### WP4 — Syntax-aware `m m` decision and implementation

Use only an approved structural source. If no standard LSP operation yields a
matching syntax delimiter and the daemon cannot derive one without introducing
an unapproved parser or server-specific command, leave M009's delimiter-table
behavior intact and stop for a product/architecture decision. Do not present a
delimiter-table result as syntax-aware.

### WP5 — Qualification and documentation

Cover the supported and unsupported providers, stale suppression, cancellation,
truncation, malformed ranges, cold-server no-spawn, and responsive local input.
Run the focused LSP/TUI suites and guards, root quick verification, and the
visible-host editor qualification. Update `docs/tui.md`, the keymap
reachability guard, the M014 roadmap status, and this plan's closure evidence.

## 7. Failure, cancellation, and contention semantics

The interaction is non-blocking. A superseded object query is cancelled where
possible and its response is discarded in all cases. No warm server means
visible `NotReady` and no process start. Multiple plausible servers produce
the existing `Ambiguous` result. Provider refusal and unsupported capability
remain distinct. Budget exhaustion and transport closure do not change the
selection.

## 8. Compatibility

No storage migration or new write capability is expected. DTO changes must be
additive or versioned under the existing protocol compatibility policy. Old
clients continue to use M009's byte-derived object behavior and receive typed
unsupported responses for new reads.

## 9. Required tests and verification

- Typed protocol validation for each structural result and bound.
- Fake-LSP integration scenarios for supported, unsupported, malformed,
  truncated, cold, ambiguous, and cancelled reads.
- App-path delayed-response tests after cursor movement, document edit, route
  change, request supersession, and server restart.
- Real-server tests for every claimed object-kind mapping; fake-server results
  alone cannot establish provider semantics.
- `python3 scripts/check_tui_editor_key_authority.py` and the current
  text-authority/keymap guards.
- Focused `egglsp`, root LSP/TUI, and `scripts/verify.sh quick` runs.
- Visible-host editor E2E with actual animation-frame completion; no fixed-time
  fallback is accepted as key-to-paint evidence.

## 10. Stop conditions

Stop and report before adding a parser dependency, private server command,
server-specific transport, new process authority, or a claim that generic
LSP ranges are syntax-kind ranges. Stop if provider behavior differs in a way
that makes one gesture's semantics misleading. A new architectural choice
must be recorded in a new ADR; accepted ADR-0034 remains immutable.

## 11. Acceptance criteria

- Every claimed syntax-dependent gesture has provider-backed semantic evidence
  and typed, bounded behavior.
- Unsupported providers show an explicit result and never silently use a
  different byte-level operation.
- Asynchronous work never blocks rendering or local input and stale results
  never change editor state.
- `m m` is called syntax-aware only if it uses a qualified structural match
  source; otherwise its current byte-derived behavior is accurately
  documented and M014 remains open for that item.
- Existing byte-derived objects retain their behavior.
- Required tests, guards, and visible-host qualification pass, and closure
  documentation makes no broader parity claim than the evidence supports.

## 12. Closure evidence required

Write `plans/closure/tui-editor-helix-parity/014-status.md` with a gesture by
gesture evidence table, provider/version matrix, bounds, stale/cancellation
results, visible-host behavior, exact commands and outcomes, and any named
unresolved scope. Do not close M014 while any claimed syntax object is merely
an untyped range approximation or while its syntax-aware matching criterion
is unimplemented.
