# TUI Modularity and Render Invariants Milestone 005 — One hint vocabulary

Status: conditionally closed

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`

Source roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-005--one-hint-vocabulary`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior` — "exact keys
  remain configurable"; the hint line describes whatever the configuration
  bound, so the vocabulary must be a rendering concern rather than a key
  concern.

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — the
  hint describes keys; it never decides ownership. This milestone changes what
  a hint *says* and never which stage claims a chord.

Primary class: capability

## 1. Objective

One `HintLine` builder with one separator, one verb vocabulary and one casing,
adopted by panel hints and dialog footers — and a truncation rule that makes it
impossible for a narrow pane to drop the key that leaves, closing invariant I6.

## 2. Why this milestone is ready

Blocked on M001 (hard, verification gate). Interface dependency on M002 and M003
is stable: both produce hint text, and this milestone is the single producer
they adopt. That is why it is ordered after them in the roadmap even though it
depends on neither.

## 3. Current implementation evidence

At baseline `7642b9e38`, measured across `src/tui/**`:

- **Three separators.** Of 93 hint strings, 50 use ` | `, 37 use bare spacing,
  and 6 use `:`. Separately, `·` (U+00B7) appears **382 times across 47
  files** — it is the panel hint line's separator, while dialog border titles
  use `|`. So the product currently has two dialects in simultaneous use.
- **Five spellings of the same exit verb.** `Esc close` (31), `Esc cancel`
  (10), `Esc: cancel` (2), `Esc back` (2), `Press Esc` (2).
- **Casing splits 78 lowercase against 5 Title Case** in the verb position
  (`Enter select` beside `Enter: Continue`).
- **Two producers.** `panels/hosted.rs:163` composes
  `"{title} · {keys} · Shift+← back"` from `Component::keymap_hint()`; dialogs
  compose their own border titles and `HINT` constants independently. 20 files
  implement `keymap_hint`, and several are `Some(HINT.to_string())` over a
  local constant — a per-file vocabulary by construction.
- **`fit_parts` is the truncation rule and is untested.**
  `src/tui/app/mod.rs:58` drops whole parts that do not fit a budget, and its
  doc comment states the reason: a row that ends mid-key names a key that does
  not exist. That reasoning is right. The function has **zero tests** in the
  tree, and nothing asserts which part survives.
- **The budget is computed per frame.**
  `App::render` computes `hint_budget` from the pane width and calls
  `composer_hint_text_bounded`; the hint chunk is a vertical split of the
  prompt chunk, so the budget is the pane width, not the terminal width.
- `render_completions` already uses the target dialect —
  `"↑↓ navigate · Enter run · Esc cancel"` — so the intended vocabulary exists
  in one place and has not spread.

## 4. Invariants that must not regress

- **No hint advertises a key the surface does not handle.** I4 is M006's to
  close; this milestone must not widen it by inventing a verb.
- **No hint loses a verb it used to name**, except where the vocabulary change
  is the same verb spelled consistently.
- **Key names stay recognisable.** `Esc`, `Enter`, `Tab`, `PgUp`, `PgDn` and
  arrow glyphs are proper names and are not lowercased.
- **`fit_parts` keeps its whole-part rule.** A partial key is still never
  rendered.
- **I1, I2, I3b stay green.**
- **The render-cost shape guard is unchanged.** One hint producer per surface
  is a reduction, not a second producer.
- **The comment ratchet holds.**

## 5. Scope

### In scope

- A `HintLine` builder: an ordered list of `(key, verb)` items rendered with
  one separator and one casing policy.
- The vocabulary policy, fixed here: separator ` · ` (space, U+00B7, space);
  item form `<Key> <verb>`; key names keep their conventional capitalisation;
  verbs are lowercase.
- A required-part rule: the exit item is always the first item, so truncation
  from the end cannot remove it.
- Adoption by panel hints (`composer_hints`, `PanelHints`) and by dialog
  footer/hint rows that M003 converted to `Section`.
- Tests for `fit_parts` and for the required-part rule.

### Explicitly out of scope

- Deciding which keys exist, or rebinding any. ADR-0042 governs ownership and
  the two key-authority guards are the arbiter.
- Localisation. The builder is not an i18n layer.
- Changing a hint's *content* where the current text names a real verb. The
  change is spelling, not meaning.
- Fixing `/shell-show`'s advertised-but-unhandled verbs — that is M006.
- The completion popup's layout. It already uses the target dialect; it adopts
  the builder but its geometry does not change.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None.

### Frontend or operator surface

- `HintLine` in `src/tui/components/` with `push(key, verb)`,
  `push_required(key, verb)` and `render(budget) -> String`. It owns the
  separator, the casing rule and the whole-part truncation, replacing the
  scattered `HINT` constants' formatting role.
- `fit_parts` gains the required-part guarantee: a required part is ordered
  first, and a budget that shows any part shows the required one. When the
  required part alone does not fit, the row renders empty rather than a partial
  key — the surface remains exitable because the key is documented, and a
  half-key is the one outcome the existing comment correctly refuses.
- `Component::keymap_hint` keeps its signature; what changes is that an
  implementation returns a built `HintLine` rendering rather than a
  hand-formatted string.

### Security and authorization

None.

### Documentation and static guards

- `architecture/tui.md` — the hint vocabulary and the required-part rule,
  stated as the contract a new surface follows.
- A guard is **not** added. The vocabulary is enforced by `HintLine` being the
  only producer plus a test that scans the rendered hint of every surface; a
  source-regex guard over Rust strings would be the wrong tool here and
  `scripts/rust_test_scan.py` exists precisely because regex scanning of Rust
  misreads it.

## 7. Ordered work packages

### Work package A — The builder

Intent: one producer of hint text.

Required changes: `HintLine` with the four decisions (separator, casing, item
form, required-part ordering) and `render(budget)`.

Acceptance evidence: unit tests assert the separator, that key names keep their
case while verbs are lowercase, that a required part is first, and that
`render` never returns a partial item.

### Work package B — `fit_parts` and the required part

Intent: make the truncation rule testable and make the exit key unlosable.

Required changes: `fit_parts` gains the required-part rule; the doc comment
gains the empty-row case and its justification.

Acceptance evidence: tests at budgets 0, 1, 5, 12, 20 and 80 assert the exit
part is present whenever any part is, and that no result ends mid-item. A test
at a budget below the exit item's width asserts an empty row rather than a
truncated key.

### Work package C — Panel hints

Intent: one vocabulary on the panel side.

Required changes: `composer_hints` in `panels/hosted.rs` and the `PanelHints`
consumers build through `HintLine`. The `" · {keys} · Shift+← back"` template
becomes a `HintLine` with the exit item required.

Acceptance evidence: corpus diff for every panel hint row; the rendered strings
differ only where a separator or casing changed.

### Work package D — Dialog hints

Intent: one vocabulary on the dialog side.

Required changes: the `HINT` constants and border-title hints that M003
converted to `Section` are built through `HintLine`.

Acceptance evidence: per-dialog corpus diff of the hint row only, reviewed
individually, with no dialog losing a verb.

### Work package E — Vocabulary test

Intent: make the uniformity checkable across surfaces without a source guard.

Required changes: a test that renders every surface's hint and asserts the
separator set is exactly `{" · "}` and that no rendered verb is Title Case.

Acceptance evidence: the test fails against a fixture that uses ` | ` and
against one that uses `Esc: cancel`, proving it detects both dialects.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable. Hint rendering is a pure string build per frame. The one
hazard is a hint whose content depends on live state (the panel hint line
already switches between focused and unfocused forms). `HintLine` must be
rebuilt per frame from state rather than cached across a focus change, and a
test asserts the focused and unfocused renderings differ.

## 9. Compatibility and migration

No compatibility surface. A user-visible hint changes spelling in some places —
that is the point — but no key, verb or capability changes. Where a hint
currently names a verb the surface does not handle, this milestone must **not**
silently drop the verb to make I4 cleaner; it leaves it and M006 decides, so the
record of the defect survives one milestone longer rather than disappearing
from the diff.

## 10. Required tests

### Focused unit tests

- `HintLine` separator, casing and item form.
- Required part ordering.
- `render` never returns a partial item.
- `fit_parts` at six budgets including one below the exit item's width.

### Integration tests

- Every panel's hint row and every converted dialog's hint row uses the single
  separator.
- Focused and unfocused panel hints differ.
- I1, I2 and I3b remain green.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- The vocabulary test fails against a ` | ` fixture and against an
  `Esc: cancel` fixture.
- A hint with 20 verbs renders within its budget without panicking and without
  a partial item.

### Migration and compatibility tests

- A test asserts the set of verbs advertised by each surface is unchanged from
  baseline, so "the vocabulary changed" cannot hide "a verb disappeared".

## 11. Required verification commands

```bash
# the builder and truncation rule
cargo test --lib -- tui::components::hint_line
cargo test --test tui_render -- invariants

# the surfaces that compose hints
cargo test --test tui_render -- tui_chat_surfaces
cargo test --test tui_render -- tui_composer_claims
cargo test --test tui_render -- tui_composer_keymap_consistency
cargo test --test tui_render -- tui_info_dialog_panel_chrome

# the render-cost shape guard
python3 scripts/check_tui_render_cost.py

# the comment ratchet
python3 scripts/check_comment_budget.py

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# the routine gate
scripts/verify.sh quick
```

## 12. Documentation updates

- `architecture/tui.md` — the vocabulary, the required-part rule and the
  empty-row case.
- `architecture/tui.md` — one sentence stating that a hint describes a key and
  never decides ownership, pointing at ADR-0042.
- The milestone's corpus diff note, one entry per surface.

## 13. Acceptance criteria

1. `HintLine` is the only producer of a rendered hint string.
2. One separator is in force across panels and dialogs.
3. Verbs are lowercase and key names keep their conventional capitalisation.
4. The exit item is first on every surface, and no budget that shows a part
   omits it.
5. No rendered hint ever ends mid-item.
6. The advertised-verb set per surface is unchanged from baseline.
7. `scripts/verify.sh quick` is green; the render-cost guard is unmodified.

## 14. Stop conditions

Report rather than improvise when:

- a hint's content depends on state in a way the builder cannot express without
  the caller formatting strings itself;
- the required-part rule cannot be honoured for a surface whose exit key is
  genuinely unknown at render time;
- a surface's hint would need a different separator to stay readable at narrow
  widths — report it rather than adding a second separator;
- adopting the builder would change an advertised verb set, which would mean
  the milestone has crossed into M006's scope.

## 15. Closure evidence required

- The builder and its unit tests.
- `fit_parts` tests including the below-minimum case.
- The vocabulary test, with both failing fixtures recorded.
- The per-surface corpus diff.
- The advertised-verb set comparison against baseline.
- `quick` output.
- A closure record at
  `plans/closure/tui-modularity-and-render-invariants/005-status.md`.

## 16. Handoff notes

Pick the dialect that already dominates and defend it: `·` appears 382 times
across 47 files and `Esc close` is the most common exit spelling by 3:1. This
milestone is not an opportunity to choose a new separator, and a "nicer" one
would invalidate the only evidence we have about what users already read.

The `fit_parts` rule is subtler than it looks. "Never drop the exit key" and
"never render a partial key" can conflict at a one-column budget, and the
existing comment already decided which wins: a partial key names a key that
does not exist. Keep that decision and make the exit item first, rather than
inventing an overflow.

Do not clean up `/shell-show` here. Its advertised-but-unhandled verbs are the
evidence M006 needs, and a hint sweep that quietly deletes them destroys the
record.
