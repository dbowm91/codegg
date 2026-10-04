# Desktop Frontend and IDE Foundation Milestone 006-E — Agent Change Review (Saved Documents)

Status: ready for handoff

Repository baseline: `66d40ec1` (M006-D merged)

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-006--tui-ide-vertical-slice`

Long-term requirements:

- `architecture/tui.md` — non-modal primary views, project-scoped authority
- `architecture/document.md` — the dirty-buffer rejection is correct as written

Applicable ADRs:

- None. See §2. The user decision of record: **M006-E is scoped to review
  against saved documents only.** Apply-into-dirty-buffer is explicitly out of
  scope and is deferred to a future ADR.

Primary class: capability

## 1. Objective

Give the TUI a review step between "an agent proposed a change" and "the change
reached disk", so a user can see the exact per-file diff and either accept or
reject it — with the daemon's dirty-buffer conflict surfaced rather than hidden.

## 2. Why this milestone is ready

- M006-A and M006-D are merged and closed, so the editor, the controller, and
  the project-scoped navigator all exist.
- The apply path already exists and is already daemon-owned:
  `/lsp-preview-apply` builds a digest-bound `LspPreviewApplyRequestDto` from
  the review registry and sends `CoreRequest::LspPreviewApply`. This milestone
  inserts a *review* in front of that call; it does not replace it.
- Patches in the registry are unified diffs (`@@ -a,b +c,d @@` blocks), and
  `src/tui/components/diff.rs` already renders `DiffHunk`/`DiffLine` with
  scrolling and an inline/side-by-side toggle. The rendering half already
  exists.
- **No ADR is required**, by explicit product decision. The contested part of
  M006-E was always whether an agent apply may merge into a *dirty* buffer.
  That question is deferred. What remains — reviewing a change to a **saved**
  document and letting the daemon decide — changes no ownership and no
  authority.
- Fits `plans/003-planning-process.md` §5: one capability boundary
  (propose → review → accept or reject), no schema migration, no unresolved
  decision.

## 3. Current implementation evidence

- `src/tui/app/mod.rs:5160` — `BuiltinSlashAction::LspPreviewApply`. Today the
  command takes a preview id, reports staleness as a toast, resolves the
  request, and **applies immediately**. There is no review step and no diff on
  screen. That is the gap.
- `src/tool/lsp.rs:788` `refresh_preview_staleness` returns
  `(is_stale, rendered_detail)`.
- `src/tool/lsp.rs:798` `preview_apply_request` returns the digest-bound
  `LspPreviewApplyRequestDto` with per-file `patches: Vec<LspPreviewPatchDto>`,
  each carrying `path`, `patch` (a unified diff), and `original_hash`.
- `crates/egglsp/src/edit.rs:301` `generate_unified_patch` confirms the patch
  format is a standard unified diff.
- `src/tui/components/diff.rs` — `DiffViewer` with a **public** `hunks` field,
  `render`, `handle_scroll`, and `toggle_mode`. Public `hunks` is what lets a
  review surface reuse the renderer without touching its internals.
- `src/lsp/mutation.rs` — the dirty-buffer rejection: `lock_clean_paths` before
  the workspace lock, `is_managed_document_dirty` inside it, and
  `dirty_document_service_rejects_preview_before_disk_write` proving a dirty
  target fails closed with no disk write.

Known gaps:

- No unified-diff *parser* exists on the TUI side; `diff.rs` only computes a
  diff from old/new content it is handed.
- No review state, no review view, and no accept/reject key path.

## 4. Invariants that must not regress

- **The daemon remains the sole authority for whether a change may be
  applied.** The review surface decides only what the user *asked for*; it
  never decides whether the apply is legal.
- **`src/lsp/mutation.rs` is not modified.** The dirty-buffer rejection, its
  ordering, and its test are untouched. Rejecting an illegal apply is the
  daemon's job and its answer is shown verbatim.
- Accepting a review issues exactly the same `CoreRequest::LspPreviewApply`
  with exactly the same digest-bound request that `/lsp-preview-apply` issues
  today. No second apply path is created.
- Rejecting a review changes **nothing**: no request is sent, no document is
  touched, no checkpoint is created.
- The review parser is bounded on patch count, patch byte length, hunk count,
  and line count, and reports truncation rather than rendering a partial diff
  silently.
- A preview that is already applied, absent, or stale is refused before the
  review is built, with a specific message.
- No protocol, storage, migration, or authorization change. The wire contract
  is exactly the one that exists today.
- `Route::Editor`, the file tree, and the composer keep working unchanged.

## 5. Scope

### In scope

- A bounded unified-diff parser producing the existing `DiffHunk`/`DiffLine`
  types.
- Review state: preview id, kind, title, provenance, per-file patches, stale
  flag, daemon verdict, scroll, and focus.
- A review view that renders the diff, the file list, and the accept/reject
  affordance.
- `/review <preview-id>` to open a review, and keys to accept, reject, and
  scroll.
- Accepting routes through the existing `LspPreviewApply` request.
- The daemon's rejection — including the dirty-buffer message — rendered in the
  review view rather than only as a transient toast.
- Focused tests for the parser, the state machine, and the reject path.

### Explicitly out of scope

- **Apply into a dirty buffer.** Deferred to a future ADR. This milestone adds
  no three-way merge, no conflict markers, and no rebase UI.
- Editing the proposed change before accepting it.
- LSP diagnostics, hover, definition, references, symbols, or semantic tokens.
  That is M006-B, which requires its own ADR.
- Semantic (LSP-driven) code actions and refactorings as a distinct review
  kind; the surface renders whatever the preview registry holds, and does not
  interpret `kind`.
- Agent-initiated review requests (the agent offering a review card in chat).
  This milestone is a user-invoked surface over an existing preview.
- Any change to the preview registry, its digests, or its staleness rules.
- Git staging, committing, or branching.
- Graphical (Monaco/Tauri) review UI.

## 6. Required production changes

### Core/domain

None. No `codegg-core` type is added.

### Storage and migrations

None. A review is view state on `App`, not persisted; it dies with the view,
which is correct because the underlying preview is itself turn-local.

### Protocol and DTOs

None. The review reads the *existing* `LspPreviewApplyRequestDto` the tool
already builds. Adding a review-specific DTO would be a protocol change and
would drag M006-B's ADR into a milestone that does not need it.

### Runtime and concurrency

- Accepting spawns a scoped TUI task and carries a request id plus a review
  generation, exactly like `EditorOpened`. A completion for a review the user
  already closed or replaced is discarded rather than rendered.
- The diff parse is bounded and synchronous, so it does not need a task; a
  pathological patch is truncated with a notice instead of stalling a frame.
- No lock is held across the accept round trip.

### Frontend or operator surface

- `src/tui/unified_diff.rs` — the parser, producing the existing diff types.
- `src/tui/app/state/change_review.rs` — the review state and state machine.
- `src/tui/components/change_review.rs` — the review widget, reusing
  `DiffViewer`'s rendering by replacing its public `hunks`.
- `src/tui/commands/change_review.rs` — open, accept, reject, scroll.
- A `/review` builtin command alongside the existing `/lsp-preview-apply`, which
  stays exactly as it is for scripted and agent-driven use.
- A review key path that claims a strict subset only while a review is open.
- Help-overlay bindings.

### Security and authorization

- The review surface sends no request of its own. Accepting reuses
  `preview_apply_request`, which is digest-bound, and the daemon re-verifies
  containment, authorization, and the clean-buffer precondition.
- The parser never executes or evaluates patch content. It renders text.
- Patch text is length-bounded before parsing so a hostile preview cannot
  force an unbounded allocation.
- A stale or applied preview is refused before the review is built, so the user
  is never shown a diff that no longer reflects the candidate.
- The review view shows paths from the registry without resolving them as
  authority; they are display strings, and the daemon's own containment check
  is what actually bounds the apply.

### Documentation and static guards

- `architecture/tui.md`: the review surface, its accept/reject semantics, and
  the explicit statement that the daemon decides legality.
- `architecture/document.md`: the review surface surfaces the existing
  dirty-buffer rejection and does not relax it.
- `plans/registry.md` and the roadmap: register M006-E and record that the
  dirty-buffer half is deferred to a future ADR.
- No new static guard. The invariants that matter here — one apply path, and
  the daemon's authority — are structural: the review literally calls the same
  function `/lsp-preview-apply` calls, which a reviewer can read in one place.
  A regex guard over that would be weaker than reading the call site.

## 7. Ordered work packages

### Work package A — Bounded unified-diff parser

Intent: turn the registry's patch text into the diff types the existing
renderer already consumes.

Required changes: parse `@@ -a,b +c,d @@` hunk headers and their
`-`/`+`/context lines into `Vec<DiffHunk>`/`Vec<DiffLine>`, carrying line
numbers. Cap the number of patches, bytes per patch, hunks per patch, and lines
per hunk. Record whether any bound was hit so the view can say the diff is
partial. Ignore `\ No newline at end of file` and treat a `---`/`+++` file
header as metadata, not as content.

Acceptance evidence: a round-trip test over a known patch asserting exact line
numbers and tags; truncation at each bound; a malformed header is skipped
rather than panicking; a patch with no hunks yields an empty, non-panicking
result.

### Work package B — Review state and state machine

Intent: hold one review with the same request/generation discipline the editor
uses.

Required changes: `ChangeReviewState` with `Default` meaning *no review open*,
plus `open`, `accept`, `reject`, and completion-application methods that refuse
a mismatched request id or generation. A rejection must be distinguishable from
a success so the view can show the daemon's message.

Acceptance evidence: stale completions are discarded; reject leaves no pending
request; reopening with a different preview id supersedes the previous one.

### Work package C — Review view

Intent: show the diff, the files, and the verdict.

Required changes: a widget that renders a per-file header list and the active
file's diff by constructing a `DiffViewer` and replacing its public `hunks`.
Render the stale warning, the truncation notice, and the daemon verdict inline
rather than only as a toast. Degrade to a message when there is nothing to
review.

Acceptance evidence: renders file names, +/- lines, the accept/reject hint, and
the daemon's rejection message; a zero-area view does not panic.

### Work package D — Commands and keys

Intent: make it reachable and make accept route through the existing path.

Required changes: `/review <preview-id>` that resolves the candidate, refuses a
stale or applied preview, and opens the review without applying anything;
accept that builds the same `LspPreviewApplyRequestDto` and issues the same
`CoreRequest::LspPreviewApply`; reject that changes nothing; a strict-subset key
path.

Acceptance evidence: accept issues exactly one request and rejects issue zero;
a stale preview never reaches the review; the existing `/lsp-preview-apply` is
unmodified and still works.

### Work package E — Guards and verification

Intent: prove the boundary held.

Required changes: a test asserting that the review path and the legacy path
build an identical `CoreRequest::LspPreviewApply`; guard and suite runs.

Acceptance evidence: the two paths produce equal requests; all TUI tests pass;
the guards and `verify.sh quick` are green; hosted CI is green.

## 8. Failure, cancellation, restart, and contention semantics

- **Unknown, already-applied, or stale preview:** refused before the review is
  built, with a specific message. The user is never shown a diff that no
  longer corresponds to the candidate.
- **Daemon rejection (including dirty buffer):** shown in the review view
  verbatim, and the review stays open so the user can read why. The change is
  not applied.
- **Malformed patch:** the offending hunk is skipped and the rest is shown;
  the parser never panics on hostile or truncated text.
- **Truncated diff:** displayed as partial. Presenting a truncated diff as
  complete would be worse than showing nothing.
- **Superseded review:** a completion for a review the user already rejected or
  replaced is discarded by generation.
- **Concurrent accepts:** the daemon's own preview id and digest binding decide;
  the second attempt fails as already-applied rather than double-applying.
- **Project switch:** an open review is closed, because its preview belongs to
  the previous project and turn.

## 9. Compatibility and migration

No schema, storage, protocol, configuration, or API change. The wire contract
is the existing `LspPreviewApply`. `/lsp-preview-apply` is unchanged, so
scripted and agent-driven use keeps working exactly as before; `/review` is
additive and user-invoked.

No version skew is introduced because no wire surface changes. As with M006-D,
this is precisely what keeps M006-E separable from the ADR-gated M006-B.

## 10. Required tests

### Focused unit tests

- Parser: exact line numbers and tags for a known patch; context lines; the
  `---`/`+++` header is metadata; a `@@` header with no lines; a malformed
  header; a missing trailing newline; each bound truncating with a flag.
- State: default is closed; open/accept/reject transitions; a stale completion
  is refused; reopening supersedes.

### Integration tests

- The review path and the legacy path build an equal
  `CoreRequest::LspPreviewApply` — the test that most directly protects against
  a second apply path appearing.
- A scripted transport that answers the apply with the daemon's dirty-buffer
  error, asserting the review surfaces that message and the document is
  unchanged.

### Restart and recovery tests

Not applicable; a review is view state. A fresh `App` has no review open, which
is the cold-start case.

### Contention and cancellation tests

- A completion arriving after reject is discarded.
- Two accepts against one preview id: the daemon decides, and the frontend does
  not double-report success.

### Security and negative tests

- A patch with a huge hunk is truncated, not rendered unbounded.
- A patch containing shell-looking or escape-sequence text is rendered as
  literal text, never evaluated.
- A stale preview is refused before a review exists.

### Migration and compatibility tests

The legacy `/lsp-preview-apply` path's existing tests are the compatibility
evidence; they must pass unchanged.

## 11. Required verification commands

```bash
# narrow tests first
cargo test --lib tui::unified_diff
cargo test --lib tui::app::state::change_review
cargo test --lib tui::

# static guards (change-triggered)
python3 scripts/check_tui_project_authority.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_execution_ownership.py

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# broader suite
scripts/verify.sh quick
cargo nextest run --workspace --locked --profile ci
```

Do not claim commands that were not actually run in the closure record.

## 12. Documentation updates

- `architecture/tui.md`: the review surface, accept/reject, and the statement
  that the daemon decides legality and the review only records intent.
- `architecture/document.md`: the review surfaces the dirty-buffer rejection and
  does not relax it.
- `plans/registry.md`, the roadmap, and the M006 audit: M006-E closed for saved
  documents; the dirty-buffer half explicitly deferred to a future ADR.

## 13. Acceptance criteria

- `/review <preview-id>` shows the per-file diff of a pending agent change and
  applies nothing.
- Accepting issues exactly the same `CoreRequest::LspPreviewApply` that
  `/lsp-preview-apply` issues, proven by a test comparing the two.
- Rejecting issues no request and changes nothing.
- A daemon rejection, including the dirty-buffer message, is shown in the
  review view rather than only as a transient toast.
- A stale, unknown, or already-applied preview is refused before a review is
  built.
- The parser is bounded on patches, bytes, hunks, and lines, and reports
  truncation.
- `src/lsp/mutation.rs` is byte-identical; no change to the dirty-buffer
  rejection.
- `/lsp-preview-apply`, the editor, the file tree, and the composer are
  unchanged.
- All TUI tests pass, the guards and `verify.sh quick` are green, and hosted
  `CI / verify` completes the full sweep.

## 14. Stop conditions

Stop and report rather than improvise when:

- implementing this appears to require a new protocol variant or DTO — that is
  M006-B and its ADR;
- the only way to accept a review is to construct a request that differs from
  `preview_apply_request`, which would mean a second apply path;
- anything appears to require weakening, bypassing, or re-implementing the
  dirty-buffer rejection in `src/lsp/mutation.rs`;
- the diff parser cannot be made bounded, or cannot be proven safe on
  malformed input;
- the work would expand into the LSP read surface (M006-B/C) or into an
  apply-into-dirty-buffer merge UI;
- an unresolved architecture decision materially changes ownership.

## 15. Closure evidence required

- The parser's bounds and measured behavior at each bound, with truncation
  evidence.
- The equal-request test proving one apply path.
- The scripted dirty-buffer rejection showing the daemon's message surfaced and
  the document unchanged.
- Proof the dirty-buffer rejection is untouched: a diff of
  `src/lsp/mutation.rs` showing no change.
- The exact guard, fmt, clippy, `verify.sh quick`, and suite commands run, with
  results and counts.
- The hosted `CI / verify` run id and an explicit statement of whether the full
  sweep completed.
- A statement that no protocol, storage, migration, or authorization change was
  made, and that no historical closure record was edited.

## 16. Handoff notes

- The review surface is a **second step in front of an existing apply**, not a
  new apply. If you find yourself building a request, call
  `preview_apply_request` instead.
- `DiffViewer.hunks` is public precisely so this milestone can reuse the
  renderer; do not fork or re-implement the diff widget.
- Rejecting must issue **zero** requests. A reject that round-trips to the
  daemon is a defect even if it changes no files.
- The daemon's dirty-buffer error must be shown, not swallowed and not
  paraphrased into something friendlier that loses the actionable detail.
- Keep `/review` and `/lsp-preview-apply` both working; the latter is the
  scripted and agent-driven path and must not regress.
- `Default` for the review state must mean *closed*, so the whole M006-A/M006-D
  surface is untouched until a user opens a review.
- No ADR is needed for this scope. If the implementation finds itself deciding
  whether a dirty buffer may be merged into, that is the stop condition — it
  is the deferred question, not this milestone's.
