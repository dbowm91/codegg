#!/usr/bin/env python3
"""Static guard for the TUI's single activity cadence (M004).

The terminal UI has **one** spinner, **one** frame table, and **one**
cadence. `src/tui/components/spinner.rs` owns all three: the braille frame
table, the 100 ms frame interval, and the pure `spinner_frame(elapsed)`
chooser. Everything else draws a frame by asking that module.

This matters because the cost of getting it wrong is invisible until it is
expensive. There used to be two independent animations — a block-progress
widget stepping every 80 ms and a braille indicator stepping once per whole
elapsed second. They disagreed on glyph family *and* on speed, so the
throbber a user actually watches during a turn took ten seconds to complete
one rotation, while the one nobody looked at kept up. Two sources of truth
for "is anything happening" is how the faster one stopped being the visible
one.

Provider reasoning control (M004) adds a **persistent** indicator: the
daemon-owned extended-thinking selection, shown in the footer status bar's
activity chain next to the daemon-resolved policy line. A persistent
indicator is exactly where a second cadence creeps in: it is the obvious
place to hang a spinner that "shows the reasoning is active", and it would
compete with the one activity indicator for the user's attention.

This guard proves four properties that are easy to regress silently:

1. **The reasoning surface defines no frame table.** An animation frame
   table is recognised by its *glyphs*, not by the binding's name: an array
   of braille or block characters is a frame table whatever it is called.
2. **The reasoning surface creates no timer.** The reasoning command module
   must not start an interval, a sleep, or a tick loop of its own.
3. **The reasoning surface draws no frame.** It must not reach for the
   frame table or the frame chooser; the indicator is static text.
4. **The reasoning indicator is content in the status bar's chain, never a
   condition on the bar.** The chip is pushed unconditionally into the
   activity list, and the status bar's own layout never mentions the
   reasoning state. A conditional chip would make the bar's contents — and
   therefore its width budget — depend on the selection.

The indicator's surface moved once already. The branch that introduced it
put the chip in the working strip's reserved row 0; `main` later deleted the
working strip as part of its buffer-chrome roadmap, and the indicator moved
to the footer status bar. The property that survived the move is the one
that mattered: the indicator is **content**, and no surface's presence or
height is a function of the reasoning state.

The guard also keeps a **census** of the frame tables under `src/tui/`. The
activity indicator's table is the one in `src/tui/components/spinner.rs`.
Exactly one other table is known and documented below; it is a per-row
status affordance on a tool-call header, not the activity indicator, and it
predates M004. The census is exact: a documented exemption that has been
renamed, resized, or removed is a failure, so the exemption list cannot
quietly become a growing allowlist.

Exit code 0 if all checks pass, 1 if any fail.

`--self-test` injects each violation into a temporary copy of the tree and
proves the guard rejects it, so a green run is not vacuous.
"""

from __future__ import annotations

import re
import shutil
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# The one module that may own the activity indicator's frame table, frame
# interval, and frame chooser.
FRAME_TABLE_OWNER = Path("src") / "tui" / "components" / "spinner.rs"

# The reasoning surface: the command module that owns the option set and the
# write path, and the render module that draws the indicator.
REASONING_COMMAND = Path("src") / "tui" / "commands" / "reasoning.rs"
REASONING_RENDER = Path("src") / "tui" / "app" / "render.rs"

# The status bar: the surface that renders the indicator's chip. It must not
# mention the reasoning state at all, so the bar's height and width budget
# can never become a function of the selection.
STATUS_BAR = Path("src") / "tui" / "components" / "status_bar.rs"

# Frame tables that are known and are **not** the activity indicator's.
#
# `TOOL_SPINNER_FRAMES` is the running glyph on a tool-call header: a
# per-row status affordance inside the transcript, on its own 120 ms tick.
# It is not a second activity indicator and it does not compete with the
# strip's phase line, but it *is* a second frame table, so it is recorded
# here rather than left to be discovered as a surprise. The glyph count is
# part of the record: if the table changes shape, the record is stale and
# the guard says so instead of accepting it silently.
FRAME_TABLE_EXEMPTIONS = {
    (Path("src") / "tui" / "components" / "messages.rs", "TOOL_SPINNER_FRAMES"): 8,
}

# Braille (U+2800..U+28FF) and block elements (U+2580..U+259F) are the
# glyph families animation frames are drawn from.
FRAME_GLYPH_RANGES = ((0x2800, 0x28FF), (0x2580, 0x259F))

# A string literal.
STRING_LITERAL = re.compile(r'"([^"\\]*)"')

# An array literal: `[` ... `]` holding at least two comma-separated string
# literals. The threshold is two, not one: a single glyph in a list is an
# icon, not a frame table.
ARRAY_LITERAL = re.compile(r"\[[^\[\]]*\]", re.DOTALL)

# The binding a frame table is stored in: an identifier, optionally with a
# type annotation, bound to an array literal.
BINDING = re.compile(
    r"([A-Za-z_][A-Za-z0-9_]*)\s*"
    r"(?::\s*(?:&\s*)?\[\s*&\s*str\s*;\s*\d+\s*\])?\s*=\s*$"
)

# Timer primitives. A second cadence needs one of these.
TIMER_PRIMITIVES = (
    "tokio::time::interval",
    "tokio::time::interval_at",
    "tokio::time::sleep",
    "thread::sleep",
    "tokio::spawn",
)

# The frame table and the frame chooser. Reaching for either from the
# reasoning surface would give the indicator its own animation.
FRAME_CHOOSER_REFERENCES = (
    "SPINNER_FRAMES",
    "spinner_frame",
    "spinner_frame_since",
    "SPINNER_FRAME_INTERVAL",
)


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _strip_test_modules(source: str) -> str:
    """Remove `#[cfg(test)]` module blocks, keeping the rest of the file.

    Truncating at the first `#[cfg(test)]` marker is wrong: a file may
    carry a small test module in the middle, and truncating there hides
    every line of production code that follows. The blocks are removed by
    brace matching instead.
    """
    out: list[str] = []
    index = 0
    length = len(source)
    while True:
        marker = source.find("#[cfg(test)]", index)
        if marker < 0:
            out.append(source[index:])
            break
        out.append(source[index:marker])
        brace = source.find("{", marker)
        if brace < 0:
            out.append(source[marker:])
            break
        depth = 0
        cursor = brace
        in_string = False
        escaped = False
        while cursor < length:
            char = source[cursor]
            if in_string:
                if escaped:
                    escaped = False
                elif char == "\\":
                    escaped = True
                elif char == '"':
                    in_string = False
            elif char == '"':
                in_string = True
            elif char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
                if depth == 0:
                    cursor += 1
                    break
            cursor += 1
        index = cursor
    return "".join(out)


def _glyph_count(literals: list[str]) -> int:
    """How many of the literals hold an animation frame glyph."""
    count = 0
    for literal in literals:
        for char in literal:
            code = ord(char)
            if any(low <= code <= high for low, high in FRAME_GLYPH_RANGES):
                count += 1
                break
    return count


def _binding_name(source: str, start: int) -> str | None:
    """The identifier an array literal is bound to."""
    window = source[max(0, start - 160) : start]
    match = BINDING.search(window)
    if match is None:
        return None
    return match.group(1)


def _frame_tables(root: Path) -> list[tuple[Path, str | None, int]]:
    """Every module under `src/tui/` that defines an animation frame table.

    A frame table is an array literal holding at least two frame glyphs.
    The *glyphs* are the signal, not the binding's name: a second spinner
    called `REASONING_FRAMES` is exactly as wrong as one called
    `SPINNER_FRAMES`, and one called `dots` is no better.
    """
    found: list[tuple[Path, str | None, int]] = []
    tui_root = root / "src" / "tui"
    if not tui_root.is_dir():
        return found
    for path in sorted(tui_root.rglob("*.rs")):
        relative = path.relative_to(root)
        if relative == FRAME_TABLE_OWNER:
            continue
        source = _strip_test_modules(_read(path))
        for match in ARRAY_LITERAL.finditer(source):
            literals = STRING_LITERAL.findall(match.group(0))
            count = _glyph_count(literals)
            if count >= 2:
                found.append((relative, _binding_name(source, match.start()), count))
    return found


def check_one_frame_table(root: Path) -> bool:
    """The TUI has the activity indicator's frame table and no other.

    A documented exemption that has been renamed, resized, or removed is a
    failure: the record must be updated deliberately, not drift.
    """
    tables = _frame_tables(root)
    seen: dict[tuple[Path, str | None], int] = {}
    for relative, binding, count in tables:
        seen[(relative, binding)] = count

    ok = True
    for key, expected_count in FRAME_TABLE_EXEMPTIONS.items():
        actual = seen.get(key)
        if actual is None:
            print(
                f"  [FAIL] the documented exemption {key[0]}:{key[1]} is gone or was renamed;"
                " update the record deliberately"
            )
            ok = False
        elif actual != expected_count:
            print(
                f"  [FAIL] the documented exemption {key[0]}:{key[1]} changed shape"
                f" ({actual} frames, expected {expected_count})"
            )
            ok = False
        else:
            seen.pop(key, None)

    for (relative, binding), count in seen.items():
        print(f"  [FAIL] an undocumented animation frame table exists: {relative}:{binding} ({count} frames)")
        print("         the activity indicator's frame table is src/tui/components/spinner.rs")
        ok = False

    if ok:
        print("  [ok] the TUI has one activity frame table and no undocumented second")
    return ok


def check_reasoning_surface_defines_no_frame_table(root: Path) -> bool:
    """The reasoning surface must not define an animation frame table."""
    path = root / REASONING_COMMAND
    if not path.is_file():
        print(f"  [FAIL] the reasoning surface is missing: {REASONING_COMMAND}")
        return False
    source = _strip_test_modules(_read(path))
    for match in ARRAY_LITERAL.finditer(source):
        literals = STRING_LITERAL.findall(match.group(0))
        if _glyph_count(literals) >= 2:
            print("  [FAIL] the reasoning surface defines an animation frame table:")
            print(f"         {match.group(0)[:80]}")
            print("         the reasoning indicator is static text, not an animation")
            return False
    print("  [ok] the reasoning surface defines no animation frame table")
    return True


def check_reasoning_surface_creates_no_timer(root: Path) -> bool:
    """The reasoning surface must not start a timer of its own."""
    path = root / REASONING_COMMAND
    if not path.is_file():
        print(f"  [FAIL] the reasoning surface is missing: {REASONING_COMMAND}")
        return False
    source = _strip_test_modules(_read(path))
    found = [primitive for primitive in TIMER_PRIMITIVES if primitive in source]
    if found:
        print("  [FAIL] the reasoning surface creates a timer of its own:")
        for primitive in found:
            print(f"         {primitive}")
        print("         the indicator is static text in a row the one frame table redraws")
        return False
    print("  [ok] the reasoning surface creates no timer of its own")
    return True


def check_reasoning_surface_draws_no_frame(root: Path) -> bool:
    """The reasoning surface must not reach for the frame table."""
    path = root / REASONING_COMMAND
    if not path.is_file():
        print(f"  [FAIL] the reasoning surface is missing: {REASONING_COMMAND}")
        return False
    source = _strip_test_modules(_read(path))
    found = [name for name in FRAME_CHOOSER_REFERENCES if name in source]
    if found:
        print("  [FAIL] the reasoning surface draws its own frame:")
        for name in found:
            print(f"         {name}")
        print("         the reasoning indicator is static text, not an animated frame")
        return False
    print("  [ok] the reasoning surface draws no frame of its own")
    return True


def _function_body(source: str, signature: str) -> str | None:
    """The body of the function whose declaration starts with `signature`.

    The body is found by brace matching, so a nested block does not end it.
    """
    marker = source.find(signature)
    if marker < 0:
        return None
    brace = source.find("{", marker)
    if brace < 0:
        return None
    depth = 0
    cursor = brace
    length = len(source)
    in_string = False
    escaped = False
    while cursor < length:
        char = source[cursor]
        if in_string:
            if escaped:
                escaped = True
            elif char == chr(92):
                escaped = True
            elif char == chr(34):
                in_string = False
        elif char == chr(34):
            in_string = True
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return source[brace + 1 : cursor]
        cursor += 1
    return None


# A conditional on the reasoning state. Any of these would make the chip's
# presence -- and therefore the bar's contents -- a function of the selection.
# The trailing boundary is deliberately absent: the field is spelled
# `reasoning_mode`, and an underscore is a word character, so `\breasoning\b`
# would NOT match `reasoning_mode` -- the one spelling the code actually uses.
CONDITIONAL_ON_REASONING = re.compile(
    r"^\s*(if|else\s+if|match|while)\b.*\breasoning",
    re.IGNORECASE | re.MULTILINE,
)


def check_indicator_is_unconditional_content(root: Path) -> bool:
    """The chip is content in the status bar's chain, never a condition.

    Two properties, both about the same thing:

    - the status summary pushes a `reasoning:` chip **unconditionally** -- no
      `if`, no `match`, no loop whose condition reads the reasoning state;
      and
    - the status bar component itself never mentions the reasoning state, so
      the bar's height and width budget cannot become a function of the
      selection.
    """
    path = root / REASONING_RENDER
    if not path.is_file():
        print(f"  [FAIL] the render surface is missing: {REASONING_RENDER}")
        return False
    source = _strip_test_modules(_read(path))
    body = _function_body(source, "fn build_status_summary")
    if body is None:
        print("  [FAIL] `build_status_summary` was not found; the chip cannot be checked")
        return False
    if "reasoning:" not in body:
        print("  [FAIL] the status summary does not push a `reasoning:` chip at all:")
        print(f"         {body.strip()[:200]}")
        print("         the daemon-owned selection must be visible in the status bar")
        return False
    conditional = CONDITIONAL_ON_REASONING.search(body)
    if conditional is not None:
        print("  [FAIL] the reasoning chip is pushed conditionally:")
        print(f"         {conditional.group(0).strip()[:200]}")
        print("         the chip is content in the chain; it must not be gated on the selection")
        return False

    bar = root / STATUS_BAR
    if not bar.is_file():
        print(f"  [FAIL] the status bar component is missing: {STATUS_BAR}")
        return False
    bar_source = _strip_test_modules(_read(bar))
    if "reasoning" in bar_source.lower():
        print("  [FAIL] the status bar mentions the reasoning state:")
        for line in bar_source.splitlines():
            if "reasoning" in line.lower():
                print(f"         {line.strip()[:160]}")
                break
        print("         the bar's height and width budget must not depend on the selection")
        return False

    print("  [ok] the reasoning indicator is unconditional content in the status bar")
    return True


def _self_test() -> int:
    """Prove the guard rejects each violation it claims to catch."""
    failures = 0

    def run_case(description: str, mutate) -> None:
        nonlocal failures
        with tempfile.TemporaryDirectory() as raw:
            tmp = Path(raw)
            tree = tmp / "tree"
            shutil.copytree(
                REPO_ROOT,
                tree,
                ignore=shutil.ignore_patterns(
                    ".git", "target", "node_modules", "__pycache__"
                ),
            )
            mutate(tree)
            results = {
                "one frame table": check_one_frame_table(tree),
                "no frame table in the reasoning surface": (
                    check_reasoning_surface_defines_no_frame_table(tree)
                ),
                "no timer": check_reasoning_surface_creates_no_timer(tree),
                "no frame": check_reasoning_surface_draws_no_frame(tree),
                "unconditional chip": check_indicator_is_unconditional_content(tree),
            }
            if not all(results.values()):
                print(f"  [PASS] self-test: {description} is rejected")
            else:
                print(f"  [FAIL] self-test: {description} was NOT rejected")
                failures += 1

    def inject_second_frame_table(tree: Path) -> None:
        """A second spinner, written under a name that says nothing."""
        target = tree / REASONING_COMMAND
        source = target.read_text(encoding="utf-8")
        target.write_text(
            source
            + "\nconst REASONING_DOTS: [&str; 4] = [\"⠋\", \"⠙\", \"⠹\", \"⠸\"];\n",
            encoding="utf-8",
        )

    def inject_timer(tree: Path) -> None:
        """A second cadence in the reasoning surface."""
        target = tree / REASONING_COMMAND
        source = target.read_text(encoding="utf-8")
        target.write_text(
            source.replace(
                "pub fn option_set(app: &App) -> ReasoningOptionSet {",
                "pub fn option_set(app: &App) -> ReasoningOptionSet {\n"
                "    let _timer = tokio::time::interval(std::time::Duration::from_millis(100));",
            ),
            encoding="utf-8",
        )

    def inject_frame_reference(tree: Path) -> None:
        """The reasoning surface reaching for the one frame table."""
        target = tree / REASONING_COMMAND
        source = target.read_text(encoding="utf-8")
        target.write_text(
            source.replace(
                "pub fn option_set(app: &App) -> ReasoningOptionSet {",
                "pub fn option_set(app: &App) -> ReasoningOptionSet {\n"
                "    let _frame = crate::tui::components::spinner::SPINNER_FRAMES[0];",
            ),
            encoding="utf-8",
        )

    def inject_conditional_chip(tree: Path) -> None:
        """The chip made conditional on the reasoning state."""
        target = tree / REASONING_RENDER
        source = target.read_text(encoding="utf-8")
        target.write_text(
            source.replace(
                "        activity.push(format!(\n"
                "            \"reasoning:{}\",\n"
                "            self.ui_state.reasoning_mode.detail()\n"
                "        ));",
                "        if !self.ui_state.reasoning_mode.requests_reasoning() {\n"
                "            activity.push(format!(\n"
                "                \"reasoning:{}\",\n"
                "                self.ui_state.reasoning_mode.detail()\n"
                "            ));\n"
                "        }",
            ),
            encoding="utf-8",
        )

    def inject_bar_dependency(tree: Path) -> None:
        """The status bar itself made to depend on the reasoning state."""
        target = tree / STATUS_BAR
        source = target.read_text(encoding="utf-8")
        target.write_text(
            source.replace(
                "    pub fn set_status(&mut self, status: String) {",
                "    pub fn set_status(&mut self, status: String) {\n"
                "        let _reasoning_rows = self.reasoning_height();\n",
            ),
            encoding="utf-8",
        )

    run_case("a second animation frame table", inject_second_frame_table)
    run_case("a timer in the reasoning surface", inject_timer)
    run_case("the reasoning surface reaching for the frame table", inject_frame_reference)
    run_case("the reasoning chip made conditional", inject_conditional_chip)
    run_case("the status bar made to depend on the reasoning state", inject_bar_dependency)

    if failures:
        print(f"{failures} self-test case(s) FAILED.")
        return 1
    print("All guard self-test cases passed.")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        return _self_test()

    checks = (
        ("the TUI has one activity frame table", check_one_frame_table),
        (
            "the reasoning surface defines no frame table",
            check_reasoning_surface_defines_no_frame_table,
        ),
        ("the reasoning surface creates no timer", check_reasoning_surface_creates_no_timer),
        ("the reasoning surface draws no frame", check_reasoning_surface_draws_no_frame),
        (
            "the reasoning indicator is unconditional content in the status bar",
            check_indicator_is_unconditional_content,
        ),
    )
    results = [(name, fn(REPO_ROOT)) for name, fn in checks]

    print()
    passed = sum(1 for _, ok in results if ok)
    for name, ok in results:
        print(f"[{'PASS' if ok else 'FAIL'}] {name}")
    print(f"\n{passed}/{len(results)} checks passed.")
    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    sys.exit(main())
