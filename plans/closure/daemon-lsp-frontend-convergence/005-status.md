# Daemon LSP Frontend Convergence M005 — Closure Status

Status: **closed**
Closed on: 2026-10-29
Repository baseline reviewed: `c85e5b5d2d6a34a38c1977fc9df1bb39e0d5934f` (`origin/main`)

Source implementation plan:

- `plans/implementation/daemon-lsp-frontend-convergence/005-local-editor-syntax-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md`

## 1. Executive finding

M005's bounded structural-selection capability is complete under accepted
ADR-0034 Option B. The daemon exposes typed selection ranges, the TUI expands
and contracts selection asynchronously through the warm LSP path, and the
large-document App-path test proves local input and stale-response fencing
while a daemon response is pending. This does not deliver M014's broader
syntax-node text objects or syntax-aware bracket matching.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Bounded typed `SelectionRange` daemon read | 71-test `lsp_test_support` family; selection-range cases cover nested ranges, cold no-spawn, unsupported, reversed ranges, depth cap and oversized response | Pass | No raw unbounded JSON-RPC result reaches the UI |
| Async TUI expand/shrink | `structural_selection_is_async_and_discards_a_moved_cursor_response` | Pass | Alt+O issues the read and leaves the 128 KiB document unchanged while pending |
| Responsive local input on a large document | Same App-path test handles local Right input before resolving the read; the response channel remains pending | Pass | 128 KiB ASCII fixture; no LSP await occurs on the key path |
| Discard stale cursor, edit, route and server-generation results | Same App-path test applies delayed results after each state change and observes no selection/result publication | Pass | Document revision is changed through `editor_insert_literal` |
| Unsupported/cold behavior and no process start | Fake-LSP selection scenarios in `lsp_test_support` | Pass | Unsupported is explicit; cold warm-only read does not launch a process |
| UTF-16 bounds, invalid ranges and truncation | Fake-LSP selection-range scenarios | Pass | Existing bounded protocol validation retained |

## 3. Production implementation evidence

The typed operation is implemented through `codegg-protocol`, `egglsp`, the
daemon warm-read route, and the frontend-neutral `codegg-client` operation
set. The TUI uses Alt+O/Alt+I and binds results to document revision, cursor
position, editor route and server generation. The editor keeps its existing
selection while work is pending. Desktop behavior and document ownership are
unchanged.

## 4. Verification executed

### Commands run

```bash
RUSTC=/Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin/rustc \
  rustup run 1.89-aarch64-apple-darwin cargo test -p codegg --lib \
  structural_selection_is_async_and_discards_a_moved_cursor_response --locked
RUSTC=/Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin/rustc \
  rustup run 1.89-aarch64-apple-darwin cargo test -p codegg --locked \
  --features server,lsp-test-support --test lsp_test_support
RUSTC=/Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin/rustc \
  scripts/verify.sh quick
```

### Results

- The App-path test passed 1/1 after the final large-document and edit-stale
  assertions were in place.
- The fake-LSP integration family passed 71/71 on this host before the final
  App-test-only change.
- `scripts/verify.sh quick` passed formatting, generated-asset checks, all
  guards, 392 Python harness tests (one skipped), and
  `cargo check --workspace --all-targets --locked`. The final assertion-only
  edit was then compiled and executed by the focused 1/1 App test.
- Authorization matrix, TUI editor key-authority and text-authority guards
  passed. Desktop renderer typecheck and the 10/10 visible session E2E also
  passed during the same closeout; desktop source behavior was not changed by
  M005.

## 5. Invariant review

Selection reads are asynchronous and warm-only. Unsupported capability stays
typed and visible. Returned ranges are bounded display state, never a path or
write authority. The editor keeps one authoritative document and applies no
text mutation from selection results. Staleness checks cover the active
document revision, cursor, route and LSP generation.

## 6. Failure and recovery review

Cold, unsupported, malformed/reversed, truncated and oversized responses are
covered by the fake-LSP suite. Delayed results after cursor movement, local
edit, route change and generation change are discarded by the App-path test.
No process startup or blocking request is introduced on the local key path.

## 7. Migration and compatibility review

The read operation is additive under existing `file.read` session/workspace
scope. No storage migration, parser dependency, document replica or mutation
authority was added. Existing standalone TUI behavior remains unchanged.

## 8. Security review

The daemon validates the authorized workspace-relative document and resolves
only an already-warm server. Returned ranges cannot authorize filesystem or
agent operations. Response count/depth and payload bounds are enforced at the
typed protocol edge.

## 9. Documentation and operations

Updated M005 implementation status, the LSP convergence roadmap, the registry,
the Helix M014 addendum, and this closure record. The accepted ADR is
`plans/adrs/ADR-0034-editor-syntax-tree-dependency.md`.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | Helix M014's broader syntax-node text objects and syntax-aware bracket matching are not provided by `SelectionRange` | No per-kind syntax-tree parity claim | Keep outside M005; plan separately if desired |

## 11. Roadmap disposition

M005 is closed. ADR-0034 Option B's bounded structural-selection slice is
complete; the broader Helix M014 syntax-node scope remains explicitly
unclaimed. M006 remains active and is not unblocked by M005.

## 12. Registry updates

- Mark M005 closed in `plans/registry.md` and the convergence roadmap.
- Keep M006 active for multi-client/worktree resource and retirement
  qualification.
