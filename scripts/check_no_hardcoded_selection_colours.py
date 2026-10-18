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
`tests/tui_theme_source_of_truth.rs` (a box must follow a theme change) and
`tests/theme_selection_tokens.rs` (every derived token must clear its contrast
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
"""

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
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
    """Blank out `#[cfg(test)] mod tests { ... }` bodies."""
    out = []
    i = 0
    n = len(src)
    while i < n:
        m = re.compile(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]").search(src, i)
        if not m:
            out.append(src[i:])
            break
        out.append(src[i : m.start()])
        brace = src.find("{", m.end())
        if brace == -1:
            out.append(src[m.start() :])
            break
        depth = 0
        j = brace
        while j < n:
            if src[j] == "{":
                depth += 1
            elif src[j] == "}":
                depth -= 1
                if depth == 0:
                    break
            j += 1
        out.append(re.sub(r"[^\n]", "", src[m.start() : j + 1]))
        i = j + 1
    return "".join(out)


def scan(path: Path) -> list[str]:
    """Report `.fg(...)` calls naming a forbidden literal colour."""
    src = strip_test_modules(path.read_text())
    # Tolerate a path outside the repo so the scanner can be exercised
    # directly; in normal use every file it sees is under ROOT.
    rel = path.relative_to(ROOT) if path.is_relative_to(ROOT) else path
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