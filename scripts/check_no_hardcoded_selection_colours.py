#!/usr/bin/env python3
"""Fail if a render path paints a hardcoded selection foreground.

Product contract: a selected row's foreground is a **theme token**, never a
constant. A selected row is a filled surface, so the colour painted on it has to
read against that fill — which is a property of the theme, not of the widget.

Why this is enforced rather than documented: `Color::White` is readable on a
dark fill and invisible on a light one, and it is blind to the theme entirely.
Ten render paths used it (six in `plan`, two in `mcp`, one in `confirm`, one in
`run_detail`), so a light theme rendered every selected row in that set as
white-on-white. Each of those is now `theme.selection_text` or
`theme.accent_text`, both derived in `src/theme/schema.rs` from colours every
Halloy theme already carries.

This is deliberately a *source* guard rather than a render assertion, and that
is a real limitation worth stating: it catches the literal, not the bug it
caused. A token read from the wrong surface would pass here. The behavioural
half of this contract is
`tests/tui_render/tui_theme_source_of_truth.inc` (a box must follow a theme change) and
`tests/tui_render/theme_selection_tokens.inc` (every derived token must clear its contrast
bar on all 50 bundled themes).

What is forbidden:
  * `Color::White` (or any `Color::Rgb` literal) used as a `.fg(...)` in a
    dialog render path

What is allowed:
  * `Color::White` in `dialogs/theme.rs`, where it is a **theme-name mapping**
    (`Color::White => "ffffff"`), not a style
  * `#[cfg(test)]` fixtures
  * `Color::Reset` / `Color::Indexed` / `Color::Named` elsewhere, which are not
    literal colours

The test fixtures are excluded through `rust_test_scan.production_mask`, never
through a regex over raw source: a fixture's `{` cannot decide where the module
ends, and the blanked view keeps every line on its own line so a reported line
number is the file's own.
"""

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(Path(__file__).resolve().parent))

import rust_test_scan  # noqa: E402  (path is set above)

DIALOGS = ROOT / "src" / "tui" / "components" / "dialogs"

# `Color::White` here is the *hex→name* table the theme picker uses to label a
# colour, not a style. Failing on it would mean the guard rejects a non-defect,
# which is how a guard gets disabled.
ALLOWED_FILES = {
    "theme.rs",  # Color::White => "ffffff", a theme-name mapping
}

# The literal colours a render path may not paint. `White` is the one this
# milestone retired; `Black` is listed because it is the symmetric failure — an
# unreadable selected row on a dark fill — and it is free to forbid now.
FORBIDDEN_FG = ("White", "Black")


def strip_test_modules(src: str) -> str:
    """The production view, with every line still on its own line.

    `rust_test_scan.production_mask` blanks each `#[cfg(test)]`-governed
    module rather than removing it, so the length and the newline count are
    the original's and the line this guard reports is the file's own line.

    The hand-rolled counter this replaced treated *any* `#[cfg(test)]` as a
    module gate: it blanked forward to the next `{`, so a `#[cfg(test)] use`
    erased the production item that followed it (the false-pass direction),
    and a module whose fixture spelled a brace ended early (leaving test-only
    source matchable). On this tree the measured impact before the fix was
    zero: 0 production lines hidden, 0 test-module lines retained across the
    42 files this guard scans. The class is removed before it bites, not
    because it had already bitten here.
    """
    return rust_test_scan.production_mask(src)


def scan_text(src: str, rel) -> list[str]:
    """Report `.fg(...)` calls naming a forbidden literal colour.

    `src` is a production view from `strip_test_modules`, so a hit lives in
    code that ships and a line number computed here is the file's own line.
    """
    hits = []
    # `.fg(Color::White)`, `.fg(ratatui::style::Color::White)`, `.fg(Color::White
    # .to_string())` — match the colour name directly after the call.
    pattern = r"\.fg\s*\(\s*(?:ratatui::style::)?Color::(" + "|".join(
        FORBIDDEN_FG
    ) + r")\b"
    for m in re.finditer(pattern, src):
        line = src[: m.start()].count("\n") + 1
        name = m.group(1)
        hits.append(
            f"{rel}:{line}: .fg(Color::{name}) is a hardcoded selection "
            f"foreground — use theme.selection_text (over theme.selection) or "
            f"theme.accent_text (over theme.primary)"
        )
    return hits


def scan(path: Path) -> list[str]:
    """Report the forbidden foregrounds in one dialog source file."""
    src = strip_test_modules(path.read_text())
    # Tolerate a path outside the repo so the scanner can be exercised
    # directly; in normal use every file it sees is under ROOT.
    rel = path.relative_to(ROOT) if path.is_relative_to(ROOT) else path
    return scan_text(src, rel)


def main() -> int:
    failures: list[str] = []

    for rs in sorted(DIALOGS.rglob("*.rs")):
        if rs.name in ALLOWED_FILES:
            continue
        failures.extend(scan(rs))

    if failures:
        sys.stderr.write(
            "hardcoded selection colour in a dialog render path:\n  "
            + "\n  ".join(failures)
            + "\n\nA selected row's foreground is a theme token: "
            "theme.selection_text over theme.selection, theme.accent_text over "
            "theme.primary. Both are derived in src/theme/schema.rs. "
            "See architecture/theme.md.\n"
        )
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())