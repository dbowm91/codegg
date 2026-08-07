# Post-Audit Correctness, Simplification, and Footprint Milestone 003 — TUI Text-Layout Correctness and Render Deduplication

Status: ready

Source subsystem roadmap:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`
- Milestone 003

Repository baseline reviewed: `0323d68e0c37c0495540d39ec0d6d9520f124125`

Primary class: UI correctness and maintainability polish

Dependencies:

- hard: none
- soft: none

Target closure record:

- `plans/closure/post-audit-correctness-simplification/003-status.md`

## 1. Objective

Correct three localized TUI issues without redesigning the renderer:

1. fix multiline thinking-tag offset/boundary handling;
2. unify line-count estimation and actual wrapping around one Unicode display-width-aware implementation;
3. remove literal duplicated ShareDialog rendering while preserving current appearance and key behavior.

## 2. Explicit non-goals

Do not:

- redesign Markdown rendering, message virtualization, theme ownership, scrolling, search UX, image rendering, or TUI architecture;
- replace ratatui/comrak/syntect as part of this milestone;
- change reasoning-tag vocabulary or expose hidden reasoning differently;
- add grapheme-cluster or bidi shaping infrastructure unless existing behavior demonstrably requires it to preserve correctness;
- redesign ShareDialog UX, QR rendering, clipboard behavior, or dialog navigation;
- add snapshot-testing infrastructure solely for this cleanup.

## 3. Current implementation evidence

Inspect at minimum:

- `src/tui/components/messages.rs`;
- `src/tui/components/messages/layout.rs`;
- focused tests for message wrapping, scrolling, reasoning parsing, and layout caches;
- `src/tui/components/dialogs/share.rs`;
- component/widget rendering conventions in adjacent dialogs.

Known defects/opportunities:

- `find_any_tag()` tracks an absolute `char_pos`, creates an absolute `after_pos`, then compares/indexes that value against the current line's length/bytes. This mixes absolute and line-local coordinate spaces and can mis-handle tags after the first line.
- `wrap_count()` uses `unicode_width` display widths while `wrap_to_strings()` falls back in several places to `chars().count()` and character-index chunking. Estimation and rendering can therefore disagree for wide Unicode or combining input.
- ShareDialog constructs essentially the same lines/block/paragraph in both `Widget::render` and `Component::render`.

## 4. Invariants that cannot regress

- visible text content remains unchanged for ordinary ASCII/Markdown input except where current wrapping is incorrect;
- reasoning tags inside fenced code blocks remain ignored according to current semantics;
- supported start/end tag names remain unchanged;
- layout estimation must match the number of visual lines produced by the rendering wrapper for the same width;
- wrapping must never split a UTF-8 code point or panic on Unicode input;
- existing cache invalidation and scroll semantics remain intact;
- ShareDialog appearance, copied state, QR output, key handling, and theme application remain behaviorally equivalent;
- no new rendering dependency is introduced.

## 5. Thinking-tag correction requirements

Refactor `find_any_tag()` or its successor so it explicitly distinguishes:

- byte/character offset within the current line;
- absolute offset within the full message.

Boundary validation must use line-local offsets and the line-local byte slice. Only the final returned match position should be converted to the absolute message coordinate.

Add regression cases covering:

- tag on first line;
- tag on later lines;
- start and end tags;
- adjacent punctuation/whitespace;
- false prefix such as `<thinkingx>`;
- tags inside fenced code blocks;
- multiple candidate tags where earliest absolute match wins;
- Unicode text before the tag on the same/prior lines.

Preserve whatever byte-vs-character offset contract downstream consumers currently expect; inspect callers before changing the return representation.

## 6. Canonical wrapping requirements

Prefer one canonical function that produces wrapped lines or a lightweight intermediate from which both rendering and count can be derived.

Requirements:

- width calculations use terminal display width consistently (`unicode_width` or existing equivalent);
- ASCII behavior remains stable;
- wide CJK/emoji characters do not overflow because a code-point count was mistaken for display columns;
- zero-width/combining characters do not incorrectly advance terminal columns;
- long words/paths/URLs hard-wrap without invalid UTF-8 slicing;
- explicit newlines preserve the current blank-line semantics;
- trailing whitespace behavior remains deliberate and covered by tests;
- line-count estimation should ideally call the same primitive rather than maintaining a second algorithm.

Do not overengineer full grapheme segmentation unless tests demonstrate that current terminal semantics cannot be matched otherwise.

## 7. ShareDialog deduplication requirements

Extract a small internal rendering helper, for example one of:

```text
build_lines(theme) -> Vec<Line>
render_into(buffer, area, theme)
```

Then have both trait entry points delegate to it.

Requirements:

- no duplicated line construction remains;
- `Widget` and `Component` rendering use the correct theme source exactly as before;
- QR generation and clipboard logic remain outside the render helper;
- the helper remains private unless another dialog already has a matching reusable convention.

Do not create a generalized dialog framework for one duplicated function.

## 8. Ordered work packages

### Work package A — Reproduce defects

1. add/fix focused tests showing later-line thinking-tag behavior;
2. add table-driven wrap/count equivalence cases for ASCII, CJK, emoji, combining characters, long paths/URLs, embedded newlines, and width 1/small widths;
3. render ShareDialog through both trait paths and establish equivalent output where a lightweight buffer test already fits repository conventions.

### Work package B — Correct tag scanning

1. separate local and absolute offsets;
2. preserve fenced-code exclusion;
3. remove any unnecessary allocation/lowercasing only if simple and behavior-preserving;
4. pass focused parser tests.

### Work package C — Unify wrapping

1. select the rendering wrapper as the canonical behavior or define one shared implementation;
2. make line estimation derive from that implementation;
3. remove obsolete duplicate algorithm/code paths;
4. verify scroll/layout-cache tests and representative message rendering.

### Work package D — Deduplicate ShareDialog

1. extract private shared rendering construction;
2. delegate both trait implementations;
3. confirm copied/uncopied and QR/no-QR states;
4. avoid unrelated dialog refactors.

## 9. Storage, protocol, migration, and compatibility effects

Storage: none.

Protocol: none.

Migration: none.

Compatibility:

- no keybinding or user-visible feature changes;
- some Unicode wrapping/scroll positions may become more correct than the previous implementation;
- reasoning-tag detection on multiline model output should become deterministic/correct.

## 10. Focused verification

Run the narrowest TUI/message test selectors that exist, plus:

```bash
cargo test --lib tui::components::messages
cargo test --lib tui::components::dialogs::share
scripts/verify.sh quick
```

If test module paths differ, use repository-equivalent selectors.

No terminal emulator or screenshot matrix is required. A manual narrow TUI smoke is optional if existing tests cannot cover scrolling/wrapping interactions.

## 11. Static guards

No new static guard is required.

The removal of duplicate wrapping/rendering code should be evident from source ownership. Do not add grep scripts to enforce one helper name.

## 12. Acceptance criteria

M003 closes only when:

- later-line thinking tags are matched/bounded using correct coordinate spaces;
- fenced-code behavior and supported tags remain unchanged;
- canonical wrapping and line estimation agree for representative Unicode and long-token cases;
- no unsafe byte slicing or display-width/count mismatch remains in the canonical path;
- obsolete duplicate wrapping logic is deleted rather than left dormant;
- ShareDialog has one render-construction path used by both trait implementations;
- UI behavior, QR generation, clipboard behavior, and key handling remain intact;
- focused tests and `scripts/verify.sh quick` pass;
- no broad TUI renderer redesign or new rendering dependency is introduced.

## 13. Stop conditions

Stop and report if:

- downstream layout/cache code depends on intentionally different counting semantics that cannot be reconciled without a larger renderer contract change;
- a correct fix requires a new bidi/grapheme shaping subsystem;
- ShareDialog's two trait implementations intentionally diverge in a way not evident from the current code.

Do not paper over those with duplicated algorithms.

## 14. Required closure evidence

`plans/closure/post-audit-correctness-simplification/003-status.md` must include:

- implementation commit/PR;
- regression cases for tag scanning and Unicode wrapping;
- evidence that render/count share one canonical implementation;
- ShareDialog deduplication summary;
- focused verification commands/outcomes;
- any remaining terminal-display limitations explicitly classified.
