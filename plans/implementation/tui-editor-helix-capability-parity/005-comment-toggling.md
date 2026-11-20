# TUI Editor Helix Capability Parity Milestone 005 — Comment Toggling

Status: conditionally closed
Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`

Source roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-005--comment-toggling`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Primary class: capability

## 1. Objective

Replace the two `space c` / `space C` "unavailable" statements with working
comment toggles driven by the buffer's own syntax, as one undoable transaction
per gesture, and say why when the buffer's syntax has no comment token.

## 2. Why this milestone is ready

No hard dependency, and no new crate:

- `syntect` is already a direct dependency of the root package and
  `src/tui/editor_highlight.rs` already loads `SyntaxSet::load_defaults_newlines()`
  and `ThemeSet::load_defaults()` as statics.
- `syntax_for(path)` already selects a grammar by the buffer's extension with a
  name fallback, and it is the same selector the highlighter trusts. Reusing it is
  what makes "a file that highlights correctly also toggles comments correctly"
  true by construction rather than by a second table agreeing.
- `syntect`'s `MetadataItems` carries exactly the two tokens needed:
  `line_comment: Option<String>` and `block_comment: Option<(String, String)>`
  (start, end), read from the grammar's own `TM_COMMENT_START`/`TM_COMMENT_END`
  shell variables. The default packdump ships the metadata; the **`metadata`
  cargo feature is what exposes it**, and it is not in `syntect`'s default
  feature set today.
- The edit funnel, the multi-member transaction path, `MAX_DOCUMENT_EDITS`, and
  the read-only refusal are all closed and are what make the toggle one undo step
  rather than a line-by-line mutation.

The interface dependency is on `syntax_for` only, and its contract (extension
first, name second, `None` when nothing matches) is what the refusal path is
built on.

## 3. Current implementation evidence

At the baseline:

- `SPACE_LEAVES` contains `space c` and `space C` as rows whose action is
  `EditorPendingEffect::Unavailable`, with the message "toggle comment: this build
  knows no comment syntax for the edited buffer" and its block twin. They are
  reachable, they are documented, and they do nothing but state the gap.
- `src/tui/editor_highlight.rs` holds `SYNTAX_SET` and `THEME_SET` as `Lazy`
  statics, plus `syntax_for(path)` returning `Option<&'static SyntaxReference>`.
  Nothing reads comment tokens from them.
- `Cargo.toml` declares `syntect = "5"` with default features. `metadata` is
  **not** among `syntect`'s defaults (`default-onig` is), so
  `SyntaxSet::metadata` and `metadata_for_scope` are not compiled in.
- `PrefixLeaf.glyph` is a `char` and its own documentation requires an
  **unmodified** glyph, because "terminals disagree about how capitals are
  encoded" and the dispatch layer owns that reconciliation. A modified leaf is
  therefore not expressible in the current table.
- `Ctrl-c` is `InputAction::ForceQuit` in the default bindings and additionally
  copies a live text selection. It is not available to the editor without taking
  the exit chord.
- The key authority guard reports **2 / 74 / 8** with no shadowed arm, and the
  leaf authority guard polices the six leaf tables.
- `docs/tui.md` documents the two "unavailable" rows, and the doc-drift guard
  checks the document's declarations in both directions.

## 4. Invariants that must not regress

- **One undo step per toggle.** A five-line comment is one history entry, not
  five.
- **A toggle is its own inverse.** Toggling twice returns the document to its
  exact bytes — this is the property that distinguishes a toggle from a "add a
  comment" command, and it must be asserted, not assumed.
- **The toggle does not rewrite content.** It inserts or removes the comment
  token and nothing else: no reflow, no re-indentation of the text, no trailing
  whitespace changes.
- **An unknown syntax is a stated refusal**, never a guess and never a silent
  no-op.
- **No second text buffer**, no mutation outside `apply_local`, read-only
  refusal visible.
- **One owner per gesture.** `Ctrl-c` stays the exit chord; §5 and D2 record why.
- **The document and the code agree** in both directions, in the same change.

## 5. Scope

### In scope

- Enable `syntect`'s `metadata` feature and read `line_comment` /
  `block_comment` through `syntax_for` (D1).
- `space c`: toggle using the buffer's preferred style — the line token when the
  grammar has one, otherwise the block pair.
- `space C`: force the block pair.
- One transaction per gesture; the comment token placed after the line's existing
  indentation for a line comment; the block pair wrapped around the covered range
  for a block comment.
- `docs/tui.md` and the keymap box updated; the two `Unavailable` rows removed.

### Explicitly out of scope

- **`Ctrl-c` as `toggle_comments`.** D2 records the decision.
- **`space Alt-c`** (Helix's `toggle_line_comments`). `PrefixLeaf.glyph` is an
  unmodified `char`, so a modified leaf is not expressible; widening the leaf
  table is its own change and its own guard change. Recorded, not hidden.
- Comment **insertion at a point** without a toggle. Not a Helix verb.
- Any interaction with the formatter (`=`), which is the LSP preview path.
- Per-language comment style overrides from configuration. That is a
  configuration-surface decision.
- Doc-comment conventions (`///` versus `//`). The grammar's `TM_COMMENT_START` is
  the authority; the plan does not invent a second rule.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None. The toggle is synchronous over the snapshot.

### Frontend or operator surface

- `Cargo.toml`: `syntect` gains the `metadata` feature. This is a feature on an
  existing dependency, not a new crate, but it is a **build and binary cost** and
  the workspace manages both actively — the plan requires the delta to be
  measured and recorded (§7 A).
- `src/tui/editor_highlight.rs`: a comment-token lookup beside `syntax_for`,
  returning the two options for the buffer's syntax.
- `src/tui/editor.rs`: the toggle verbs and their geometry — which lines the
  gesture covers, where the token goes, and how the un-toggle recognises its own
  work.
- `src/tui/commands/editor.rs`: the two `space` leaves move from
  `Unavailable` to the toggle action, and `mutates_buffer` gains the verbs.

### Security and authorization

No change. The toggle writes through `apply_local` and is refused on a read-only
attachment like every other writing verb.

### Documentation and static guards

- `docs/tui.md`: the two rows become working verbs; the "no comment syntax" text
  is replaced by the actual rule, including the refusal when a syntax has no
  token.
- The leaf authority guard's `Unavailable` set shrinks by two and its expected
  figures move in the same change.

## 7. Ordered work packages

### Work package A — The token source, and the cost measurement

Intent: one selector, one grammar, one measured cost.

Required changes: enable `syntect`'s `metadata` feature; add a lookup that takes
the buffer's path, runs `syntax_for`, and reads
`SYNTAX_SET.metadata.metadata_for_scope(&[syntax.scope])` for `line_comment` and
`block_comment`.

**D1 — the comment-token source.** Two candidates were considered.
*(a)* Enable `syntect`'s `metadata` feature and read the grammar's own
`TM_COMMENT_*` variables. *(b)* A hand-maintained extension → token table.
**Recommendation: (a).** (b) is a second source of truth that disagrees with the
highlighter the moment a grammar is added or a language is renamed, and it would
be wrong for every extension it does not list — which is most of them. (a) reuses
`syntax_for`, so the two features cannot drift, and it is a feature flag on a
dependency the workspace already ships rather than a new crate.
**The cost is real and must be measured, not asserted:** record the workspace
build-time delta and the binary-size delta for the feature in the closure record.
If the measurement is judged unacceptable, stop and report rather than silently
switching to (b) — that is a different design with a different failure mode.

Acceptance evidence: a lookup for a `.rs` path returns `Some("//")` and a block
pair; a `.py` path returns its own line token; an unknown extension returns
`None`; the measured deltas are recorded.

### Work package B — Which lines, and where the token goes

Intent: a deterministic geometry that does not depend on the cursor's column.

Required changes: the covered range is the lines the selection spans, or the
cursor's line when the selection is empty. A line comment goes after the line's
existing indentation — not at column zero, and not at the cursor's column. A
block comment wraps the covered range: the start token before the range's first
non-whitespace byte, the end token after its last non-whitespace byte.

Acceptance evidence: an indented line keeps its indentation and gains `// `
before its content; a block toggle on a multi-line range places both tokens
without touching the text between them; a range whose first line is blank is
handled by a stated rule asserted in a test.

### Work package C — Toggle, not add

Intent: the inverse property.

Required changes: before adding, decide whether the covered lines are **already**
commented, and if they are, remove the token instead. The recognition rule must
be stated: a line counts as commented when, after its indentation, it starts with
the line token. The block toggle recognises its own pair the same way. Mixed
ranges comment rather than uncomment, and the plan states that rule.

Acceptance evidence: toggling twice returns the document to its exact bytes — a
property test over several shapes (one line, many lines, mixed indentation, a
range containing a blank line); a partially commented range becomes fully
commented rather than half-uncommented.

### Work package D — Wire the two leaves and the refusal

Intent: the gestures do what the space menu promised.

Required changes: `space c` and `space C` move from `Unavailable` to the toggle
action; a buffer whose syntax has no token produces a notice naming the path's
extension, not a generic failure.

Acceptance evidence: both leaves toggle; an unknown extension produces a notice
and leaves the document byte-identical; both are refused on a read-only
attachment with a notice.

### Work package E — Guards and documentation

Intent: the document and the code agree in both directions.

Required changes: remove the two `Unavailable` rows from the leaf table's
expectations, update both guard figures, document the rule including the refusal,
update the keymap box.

Acceptance evidence: the leaf authority guard passes with the reduced
`Unavailable` set; the doc-drift guard passes in both directions; `docs/tui.md`
no longer claims the capability is absent.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure.** The toggle is one transaction; a controller rejection is
  reported and the document is unchanged. There is no half-commented state.
- **Duplicate delivery.** None; synchronous local gesture.
- **Restart.** No state.
- **Cancellation races.** None; no async work added.
- **Stale generations.** The toggle reads the current snapshot.
- **Bound reached.** A toggle that would exceed `MAX_DOCUMENT_EDITS` is refused
  with the existing notice rather than truncated.
- **Syntax set load failure.** The `SYNTAX_SET` static is loaded once and cannot
  fail at the call site; if it were empty the lookup returns `None` and the
  refusal path runs.

## 9. Compatibility and migration

No storage, protocol, or configuration migration.

**Two compatibility surfaces.**

1. **The dependency's feature set.** Enabling `metadata` changes what is compiled
   into the binary. It adds no API the rest of the workspace consumes and removes
   none, so it is additive at the crate level. The measured build and size deltas
   are the compatibility statement.

2. **The gesture.** `space c` and `space C` change from "state that the feature is
   absent" to "toggle". A user who had learned they were statements gets a
   working verb; that is the intended direction and there is no transitional
   state.

**D2 — `Ctrl-c` is not bound.** Helix binds `Ctrl-c` to `toggle_comments`. CodeGG
binds it to `InputAction::ForceQuit` and to copy-selection, and AGENTS.md records
"leaving the program takes two deliberate presses" as a safety property of this
TUI. Binding `Ctrl-c` in the editor's Normal mode would take the exit chord on
that route and would need a fourth stage in the key authority guard's model. Two
candidates were considered. *(a)* Bind `Ctrl-c` and narrow the exit gesture on the
editor route. *(b)* Bind only `space c`/`space C` and record the divergence.
**Recommendation: (b).** (a) trades a safety property for a convenience the space
menu already provides; the space menu is discoverable through `space ?`, which is
where the editor's own keys are listed. Revisiting this is an owner decision about
the exit gesture, not a milestone of this roadmap.

## 10. Required tests

### Focused unit tests

- The token lookup: `.rs`, `.py`, `.md` (line token only), an extension with no
  grammar, and an empty path.
- The geometry: indented line, blank first line, multi-line range, block wrap.
- The recognition rule: commented, uncommented, and mixed ranges.

### Integration tests

- Both leaves through the real dispatch path.
- The inverse property as a property test over the shapes above.
- The refusal on an unknown extension, asserting the document is byte-identical.
- The read-only refusal with a notice.

### Restart and recovery tests

None. No durable state; the closure record must say so rather than omit it.

### Contention and cancellation tests

- A multi-line toggle asserts exactly one history entry.

### Security and negative tests

- `check_tui_editor_text_authority.py` passes with no allowlist edit.
- A toggle whose transaction would exceed `MAX_DOCUMENT_EDITS` is refused with a
  notice and the document is byte-identical.

### Migration and compatibility tests

- The measured build-time and binary-size deltas for the `metadata` feature.
- The doc-drift guard and the leaf authority guard as the gesture compatibility
  checks.

## 11. Required verification commands

```bash
cargo fmt --check
cargo test -p codegg --lib tui::editor
cargo test --test tui_render
python3 scripts/check_tui_editor_leaf_authority.py
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_comment_budget.py
bash scripts/verify.sh quick
```

The build-cost measurement is a separate command and its exact form must be
recorded in the closure record rather than described loosely.

## 12. Documentation updates

- `docs/tui.md`: `space c` and `space C` described as working toggles; the style
  preference rule (line token first, block pair otherwise); the recognition rule
  for the inverse; the refusal when a syntax has no token; the `Ctrl-c`
  divergence stated with its reason.
- The `space ?` keymap box follows.
- The code comment on the token lookup states that `syntax_for` is the single
  selector and why a second table was rejected.

## 13. Acceptance criteria

- `space c` on a `.rs` buffer comments the covered lines with `//`, after their
  indentation, in one undo step.
- `space c` again removes exactly what it added, returning the document to its
  exact bytes.
- `space C` uses the block pair and is likewise its own inverse.
- A buffer whose syntax has no comment token produces a notice naming the
  extension and leaves the document byte-identical.
- Both gestures are refused on a read-only attachment with a notice.
- The leaf authority guard passes with two fewer `Unavailable` rows; the
  doc-drift guard passes in both directions.
- The `metadata` feature's build-time and binary-size deltas are recorded.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- The `metadata` feature's measured cost is judged unacceptable. That inverts D1
  and selects a design with a different failure mode; report, do not switch
  silently.
- A grammar's `TM_COMMENT_START` is not a prefix of its `TM_COMMENT_END` pair in a
  way the wrap rule cannot express. Report the grammar and the shape.
- The inverse property cannot hold for a shape (for example a block toggle over a
  range whose interior already contains the end token). That is a real
  correctness limit and needs a stated rule, not a guess.
- `Ctrl-c` is requested. That is D2 and an owner decision.
- A modified leaf (`space Alt-c`) is requested. That widens `PrefixLeaf` and its
  guard; it is a separate change.
- The work expands into formatter integration or configuration overrides.

## 15. Closure evidence required

- The measured build-time and binary-size deltas for the `metadata` feature.
- The token lookup's per-extension evidence, including the `None` case.
- The inverse property's test output over every shape.
- One dispatch-path test per leaf.
- The read-only and unknown-syntax refusal evidence, both asserting a
  byte-identical document.
- The leaf authority guard's output with the reduced `Unavailable` set.
- A statement that no migration, protocol variant, or new crate was added.
- The commands actually run, labelled local or CI, with unrun items named.

## 16. Handoff notes

- **`syntax_for` is the selector; do not add a second one.** The whole argument
  for D1 rests on the highlighter and the comment toggle reading the same
  grammar.
- **`metadata` is a feature, not a crate.** If review objects to the cost, the
  fallback is D1(b) and it needs its own justification — a table that disagrees
  with the highlighter is worse than a stated limitation.
- **`PrefixLeaf.glyph` is an unmodified `char`.** Do not try to add
  `space Alt-c`; it cannot be expressed and widening the table is a separate
  change with its own guard.
- **`Ctrl-c` is the exit chord.** AGENTS.md records the two-press exit as a
  safety property. D2 is the decision and it is deliberate.
- **The inverse property is the acceptance test.** A "toggle" that only ever adds
  is the defect this milestone exists to avoid, and it passes a naive test.
- **Serial test constraint.** No environment mutation is introduced; nextest
  profile `ci` applies.
- **Preserve the user's working tree.** No generated asset is touched.
