# Post-Audit Correctness, Simplification, and Footprint Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/post-audit-correctness-simplification/003-tui-text-layout-correctness-and-render-deduplication.md`

Source subsystem roadmap:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`

Repository baseline reviewed: `0323d68e0c37c0495540d39ec0d6d9520f124125`

Implementation commits or pull requests:

- `71ff2c3` — correct TUI text layout/tag scanning and deduplicate ShareDialog rendering.

## 1. Executive finding

M003 is complete and strictly closed. Multiline reasoning-tag detection now
keeps line-local byte offsets separate from absolute message offsets, while
preserving supported tag names and fenced-code exclusion. User-message wrapping
and line estimation now share one display-width-aware implementation. The
ShareDialog Widget and Component paths delegate to the same private paragraph
construction helper. No storage, protocol, renderer dependency, or dialog UX
contract changed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Later-line thinking tags use correct coordinate spaces | `find_any_tag_uses_line_local_boundaries_and_absolute_results` | pass | Unicode before a later-line tag is covered. |
| Supported tags and fenced-code behavior remain stable | `find_any_tag_preserves_boundaries_and_fenced_code_exclusion` | pass | Covers start/end forms, punctuation/whitespace, false prefixes, fenced code, and earliest match selection. |
| Render/count use one canonical wrapping behavior | `wrap_count` delegates to `wrap_to_strings`; `wrap_to_strings_matches_wrap_count` | pass | The duplicate counting algorithm was deleted. |
| Display width is honored for Unicode | `wrapping_uses_display_width_for_wide_and_combining_text` | pass | CJK, emoji, combining marks, newlines, and narrow widths are covered. |
| Wrapping is UTF-8 safe and hard-wraps long tokens | `wrapping_preserves_utf8_codepoint_boundaries_at_width_one` and existing long-token tests | pass | No byte slicing is used for wrap chunks. |
| ShareDialog has one render-construction path | `build_lines`/`paragraph` called by both trait implementations | pass | QR and copied/uncopied states are compared through `TestBackend`. |
| Existing UI behavior remains intact | ShareDialog equivalence test and focused suites | pass | Key handling, QR generation, clipboard state, and theme selection remain outside the helper. |

## 3. Production implementation evidence

`src/tui/components/messages.rs` now implements wrapping through a single
Unicode display-width-aware `wrap_to_strings` path. Logical newlines and blank
lines are preserved, whitespace is normalized as before, long words are split
only at Unicode scalar boundaries, and `wrap_count` reports the exact number of
lines returned by the wrapper. `find_any_tag` scans each line with local byte
offsets and converts only the selected match to an absolute byte offset for its
callers.

`src/tui/components/dialogs/share.rs` owns private `build_lines` and
`paragraph` helpers. `Widget::render` continues to use the dialog theme, while
`Component::render` continues to use the supplied theme; both now render the
same constructed paragraph. QR generation and clipboard/key behavior were not
moved or changed.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all
rtk cargo test --lib tui::components::messages::tests
rtk cargo test --lib tui::components::dialogs::share::tests
rtk cargo test --lib tui::components::messages
rtk cargo test --lib tui::components::dialogs::share
rtk scripts/verify.sh quick
rtk git diff --check
```

### Results

All focused commands passed. The message suite reported 28 tests and the
ShareDialog suite reported 3 tests. `scripts/verify.sh quick` passed formatting,
generated-agent checks, static guards, and the capped workspace/all-target
check. No hosted CI result was available in this local closure pass; M003 does
not require a terminal-emulator or screenshot matrix.

## 5. Invariant review

- Ordinary ASCII wrapping remains covered by the existing wrap regression
  cases and retains greedy word-wrap behavior.
- Reasoning-tag vocabulary, byte-offset slicing contract, and fenced-code
  exclusion remain unchanged at the caller boundary.
- Wide and zero-width characters are measured with terminal display width;
  wrapping never slices a UTF-8 code point.
- Explicit newline and trailing-newline behavior is covered by the canonical
  wrapper/count equivalence tests.
- Message layout cache invalidation and scroll code were not changed.
- ShareDialog appearance, theme source, copied state, QR output, key handling,
  and clipboard behavior remain intact.
- No rendering dependency or broad renderer redesign was introduced.

## 6. Failure and recovery review

This milestone has no persistence, protocol, daemon, scheduler, or restart
state. Malformed or unsupported tags are ignored as before, fenced code is
excluded from scanning, and all layout behavior is recomputed from the current
text and width. The private ShareDialog helper is stateless and cannot create a
new delivery, cancellation, or recovery path.

## 7. Migration and compatibility review

No storage schema, protocol, configuration, keybinding, or migration change is
present. Existing callers still receive absolute byte offsets from
`find_any_tag`, and the only intended visual differences are corrected Unicode
wrapping and multiline tag handling. The two existing ShareDialog trait entry
points remain available.

## 8. Security review

No authorization, secret, path, process, network, or privilege boundary was
changed. The tag scanner remains bounded by the supplied message and does not
execute or interpret tag contents. Share URLs continue through the existing
clipboard and QR paths without new exposure or logging.

## 9. Documentation and operations

The implementation plan is marked `implemented`, the subsystem roadmap marks
M003 `closed`, and `plans/registry.md` records M003 as closed with M004's former
soft dependency satisfied. This closure record is the operational evidence;
no architecture document required a wording change for this localized fix.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | A glyph whose terminal display width is greater than a one-column area cannot be split without violating UTF-8 safety. | Such an intentionally undersized area may still be clipped by the terminal; normal widths are display-width bounded. | No follow-up is required unless the renderer adopts a different narrow-cell policy. |

No critical, high, or medium finding remains. The low item is an inherent
terminal-cell limitation, not a remaining duplicate algorithm or layout-cache
correctness defect.

## 11. Roadmap disposition

M003 is closed. M004-M007 remain ready; M004's soft final-measurement
dependency on M003 is satisfied. M008 remains blocked because its hard
dependency still includes M004-M007. No future plan became newly dependency
ready, so no downstream status required a change.

## 12. Registry updates

- M003 is removed from the dependency-ready table and recorded as closed by
  this closure record.
- The post-audit subsystem remains active with M004-M007 ready.
- M008 remains blocked on the still-open M004-M007 hard dependencies.
- The implementation plan is marked `implemented`; the subsystem roadmap marks
  M003 `closed`.
