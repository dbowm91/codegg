#!/usr/bin/env python3
"""Static guard: every prefix leaf glyph is claimed exactly once, and reachable.

Milestone M010 of `plans/implementation/tui-editor-helix-parity/` grows the
`space` prefix from one leaf to nineteen. That is the first milestone in the
roadmap where a **leaf table** is large enough for its own failure modes to
matter, and they are the same failure modes the *chord* tables had:

* two rows with the same glyph — the first wins, the second is dead code that
  reads like a feature;
* a glyph the dispatch cannot deliver — a leaf that no keypress can ever
  reach, so the key is advertised and does nothing;
* a glyph the **guard block** already claimed — `editor_buffer_key` resolves
  its mode-independent chords *before* any mode dispatch, so a leaf behind one
  of them is unreachable in exactly the way M001's `Ctrl+r` was.

None of these are visible to the compiler. A duplicate row compiles, a
non-ASCII glyph compiles, and a leaf behind the guard block compiles — and the
user sees a key that does nothing, which is the M002 failure mode this
roadmap exists to end.

## What this guard reads

Two files, and no more:

* `src/tui/editor.rs` — the leaf tables (`GOTO_LEAVES`, `SPACE_LEAVES`,
  `OPEN_BRACKET_LEAVES`, `CLOSE_BRACKET_LEAVES`, `MATCH_LEAVES`) and
  `EditorPrefix::leaves()`, which says which table each prefix uses.
* `src/tui/commands/editor.rs` — the **guard block** of `editor_buffer_key`,
  through `scripts/check_tui_editor_key_authority.py`, which already models
  the dispatch chain's stages.

The guard-block chords are read from the *other* guard rather than re-parsed
here, on purpose: two parsers for one dispatch shape is two models of it, and
the day they disagree one of them is wrong. `check_tui_editor_key_authority`
is imported as a module — it is a plain script with a `main()` guarded by
`__name__`, so importing it runs no side effect.

## What counts as a violation

**Violations** (exit code 1):

- **Rule 1 — a duplicated glyph in one table.** The lookup finds the first
  match, so the second row is dead code.
- **Rule 2 — a glyph that is not printable ASCII.** The dispatch compares a
  `char` and a `KeyCode::Char`; a non-ASCII glyph is either unreachable or
  reachable only on a keyboard layout the build does not claim to support.
- **Rule 3 — a duplicated glyph across the tables of one prefix.** A prefix's
  tables are one namespace: two of its own tables claiming the same glyph is
  the same defect as rule 1, seen from the prefix rather than the table.
  (Today every prefix has exactly one table, so this is rule 1 seen again —
  it is here so the day a prefix grows a second table, the guard is already
  asking the right question.)
- **Rule 4 — a leaf whose chord the guard block already claims.** The guard
  block runs before any mode dispatch, so a leaf behind one of its chords can
  never run.
- **Rule 5 — a leaf table no prefix binds.** A table nothing reaches is a
  grammar that was written and never wired, which is how a leaf ends up
  "documented" and absent.

## What the guard requires to still be true

Checked against **comment-free** text, so a doc comment that names a table
cannot keep a deleted grammar alive:

- `pub fn leaves(` — the accessor that binds a prefix to its table.
- `pub const` leaf tables of the shape `&[PrefixLeaf]`.

## Non-vacuity

`--self-test` runs synthetic tables through the same analysis the real tree
goes through, and each case states which rule it proves. The real tree's
non-vacuity is proved separately, by injection: duplicating a glyph in a real
table must make this guard fail.

Exit code 1 on violations, 2 on a shape the guard cannot resolve.

Usage::

    python3 scripts/check_tui_editor_leaf_authority.py
    python3 scripts/check_tui_editor_leaf_authority.py --self-test

This is invoked by `scripts/verify.sh quick` and the CI `verify` job.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import check_tui_editor_key_authority as key_authority  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parent.parent
EDITOR = REPO_ROOT / "src" / "tui" / "editor.rs"
COMMANDS = REPO_ROOT / "src" / "tui" / "commands" / "editor.rs"

# `pub const NAME: &[PrefixLeaf] = &[` — the table's opening.
TABLE_RE = re.compile(r"pub\s+const\s+([A-Z_][A-Z0-9_]*)\s*:\s*&\[PrefixLeaf\]\s*=\s*&\[")
# The glyph field of one row: `glyph: 'x',`.
GLYPH_RE = re.compile(r"glyph\s*:\s*'((?:\\.|[^'\\])*)'")
# One prefix binding arm: `EditorPrefix::Space => SPACE_LEAVES,` and the
# `|`-separated form the empty arms use.
PREFIX_ARM_RE = re.compile(r"EditorPrefix::([A-Za-z_]\w*)")

# A Rust char literal's escapes. Only the ones a glyph can actually be.
CHAR_ESCAPES = {
    "n": "\n",
    "r": "\r",
    "t": "\t",
    "0": "\0",
    "\\": "\\",
    "'": "'",
    '"': '"',
}


class ParseError(Exception):
    """A shape the guard cannot resolve."""


@dataclass(frozen=True)
class Leaf:
    """One row of one leaf table."""

    table: str
    glyph: str
    line_no: int


@dataclass(frozen=True)
class Violation:
    """One violation, tagged with the rule that found it.

    The tag is what lets `--self-test` assert *which* rule fired.
    """

    rule: int
    message: str


def decode_char_literal(contents: str) -> str:
    """Decode the body of a Rust char literal into the character it names."""
    if not contents:
        raise ParseError("empty char literal")
    if not contents.startswith("\\"):
        if len(contents) != 1:
            raise ParseError(f"char literal holds {len(contents)} characters: {contents!r}")
        return contents
    escape = contents[1:]
    if escape.startswith("x"):
        try:
            return chr(int(escape[1:], 16))
        except ValueError as error:
            raise ParseError(f"unreadable hex escape: {escape!r}") from error
    if escape.startswith("u{"):
        try:
            return chr(int(escape[2:-1], 16))
        except ValueError as error:
            raise ParseError(f"unreadable unicode escape: {escape!r}") from error
    if escape in CHAR_ESCAPES:
        return CHAR_ESCAPES[escape]
    raise ParseError(f"unknown char escape: {escape!r}")


def strip_comment(line: str) -> str:
    """Drop a trailing `//` comment, respecting string and char literals.

    The leaf tables are full of char literals — `'\''` contains a slash — so a
    naive scan would read one as the start of a comment.
    """
    in_string = False
    in_char = False
    escaped = False
    index = 0
    while index < len(line):
        char = line[index]
        if escaped:
            escaped = False
        elif char == "\\":
            escaped = True
        elif in_string:
            if char == '"':
                in_string = False
        elif in_char:
            if char == "'":
                in_char = False
        elif char == '"':
            in_string = True
        elif char == "'":
            in_char = True
        elif char == "/" and line[index : index + 2] == "//":
            return line[:index]
        index += 1
    return line


def strip_comments(text: str) -> str:
    """Comment-free text with the line count preserved, so lines stay true."""
    return "\n".join(strip_comment(line) for line in text.splitlines())


def parse_tables(text: str) -> dict[str, list[Leaf]]:
    """Every leaf table in the source, with its rows."""
    lines = strip_comments(text).splitlines()
    tables: dict[str, list[Leaf]] = {}
    index = 0
    while index < len(lines):
        match = TABLE_RE.search(lines[index])
        if match is None:
            index += 1
            continue
        name = match.group(1)
        if name in tables:
            raise ParseError(f"leaf table {name} is declared twice")
        rows: list[Leaf] = []
        cursor = index + 1
        while cursor < len(lines) and "]" not in lines[cursor]:
            glyph_match = GLYPH_RE.search(lines[cursor])
            if glyph_match is not None:
                rows.append(
                    Leaf(
                        table=name,
                        glyph=decode_char_literal(glyph_match.group(1)),
                        line_no=cursor + 1,
                    )
                )
            cursor += 1
        if cursor >= len(lines):
            raise ParseError(f"leaf table {name} is not closed")
        tables[name] = rows
        index = cursor + 1
    if not tables:
        raise ParseError("no leaf tables found")
    return tables


def parse_prefix_bindings(text: str) -> list[tuple[str, str | None]]:
    """Which table each prefix binds, from `EditorPrefix::leaves()`.

    `(prefix, None)` is a prefix with no leaves — the capture prefixes, whose
    next key is data rather than a glyph.
    """
    lines = text.splitlines()
    try:
        base, body = key_authority.function_body(lines, "leaves")
    except StopIteration as error:
        raise ParseError("`fn leaves(` not found in src/tui/editor.rs") from error
    body = [strip_comment(line) for line in body]
    bindings: list[tuple[str, str | None]] = []
    pending: list[str] = []
    for offset, raw in enumerate(body):
        line = raw.strip()
        if not line:
            continue
        pending.append(line)
        if "=>" not in line:
            continue
        arm = " ".join(pending)
        pending = []
        head, _, tail = arm.partition("=>")
        target = tail.strip().rstrip(",").strip()
        if target == "&[]":
            table = None
        elif re.fullmatch(r"[A-Z_][A-Z0-9_]*", target):
            table = target
        else:
            raise ParseError(
                f"src/tui/editor.rs:{base + offset + 1}: unrecognised leaf target {target!r}"
            )
        for prefix in PREFIX_ARM_RE.findall(head):
            bindings.append((prefix, table))
    if not bindings:
        raise ParseError("`fn leaves(` binds no prefix")
    return bindings


def ascii_upper(char: str) -> str:
    """Rust's `char::to_ascii_uppercase`, exactly.

    Python's `str.upper()` also folds non-ASCII and the German sharp s, which
    the dispatch's `leaf_lookup_char` does not — so the two would disagree
    about which chords a leaf is reachable by.
    """
    if "a" <= char <= "z":
        return chr(ord(char) - 32)
    return char


def leaf_chords(glyph: str) -> set[tuple[str, str]]:
    """The `(modifiers, char)` chords a leaf can be reached by.

    The dispatch's own rule (`leaf_lookup_char`) accepts a key with no
    modifier or with `SHIFT`, and upper-cases the glyph on `SHIFT`. So a leaf
    for `G` is reachable as `Char('G')` with no modifier **and** as
    `Char('G')` or `Char('g')` with `SHIFT`, while a leaf for `g` is reachable
    only as `Char('g')` with no modifier — `SHIFT` would arrive upper-cased
    and miss.
    """
    chords: set[tuple[str, str]] = {("NONE", glyph)}
    for code in range(0x20, 0x7F):
        char = chr(code)
        if ascii_upper(char) == glyph:
            chords.add(("SHIFT", char))
    return chords


def analyse(
    tables: dict[str, list[Leaf]],
    bindings: list[tuple[str, str | None]],
    guard_chords: list[key_authority.Chord],
) -> list[Violation]:
    """The one analysis: violations, in the order they are found."""
    violations: list[Violation] = []

    bound_tables = {table for _, table in bindings if table is not None}

    for prefix, table in bindings:
        if table is not None and table not in tables:
            violations.append(
                Violation(
                    5,
                    f"prefix `{prefix}` binds leaf table `{table}`, which is not declared",
                )
            )

    for name, rows in tables.items():
        if name not in bound_tables:
            violations.append(
                Violation(
                    5,
                    f"leaf table `{name}` is declared but no prefix binds it",
                )
            )

        seen: dict[str, Leaf] = {}
        for row in rows:
            if not (0x20 <= ord(row.glyph) <= 0x7E):
                violations.append(
                    Violation(
                        2,
                        f"leaf table `{name}` row {row.glyph!r} is not printable ASCII",
                    )
                )
            prior = seen.get(row.glyph)
            if prior is not None:
                violations.append(
                    Violation(
                        1,
                        f"leaf table `{name}` re-declares glyph {row.glyph!r} "
                        f"(line {prior.line_no}), already claimed at line {row.line_no} "
                        "— unreachable",
                    )
                )
            else:
                seen[row.glyph] = row

    for prefix in sorted({prefix for prefix, _ in bindings}):
        prefix_tables = [table for name, table in bindings if name == prefix and table]
        seen: dict[str, tuple[str, int]] = {}
        for table in prefix_tables:
            for row in tables.get(table, []):
                prior = seen.get(row.glyph)
                if prior is not None:
                    # The same table claiming the same glyph twice is rule 1's
                    # finding; rule 3 is about a prefix's *tables* colliding.
                    if prior[0] != table:
                        violations.append(
                            Violation(
                                3,
                                f"prefix `{prefix}` claims glyph {row.glyph!r} in both "
                                f"`{prior[0]}` and `{table}` — unreachable",
                            )
                        )
                else:
                    seen[row.glyph] = (table, row.line_no)

    for prefix, table in bindings:
        if table is None:
            continue
        for row in tables.get(table, []):
            reachable = leaf_chords(row.glyph)
            for guard in guard_chords:
                if guard.char is None:
                    violations.append(
                        Violation(
                            4,
                            f"leaf table `{table}` glyph {row.glyph!r} is behind the "
                            f"guard block's {guard.label()} — unreachable",
                        )
                    )
                    continue
                for combo in guard.combos:
                    if combo not in (frozenset({"NONE"}), frozenset({"SHIFT"})):
                        continue
                    modifier = next(iter(combo))
                    if (modifier, guard.char) in reachable:
                        violations.append(
                            Violation(
                                4,
                                f"leaf table `{table}` glyph {row.glyph!r} is behind the "
                                f"guard block's {guard.label()} — unreachable",
                            )
                        )

    return violations


def guard_block_chords() -> list[key_authority.Chord]:
    """The chords `editor_buffer_key`'s guard block claims before any dispatch."""
    try:
        analysis = key_authority.analyse(COMMANDS.read_text())
    except key_authority.ParseError as error:
        raise ParseError(
            "the key-authority guard could not resolve the dispatch chain: " f"{error}"
        ) from error
    return analysis.guard_chords


# ── Self-test ────────────────────────────────────────────────────────
#
# Synthetic tables run through `analyse` — the same analysis the real tree
# goes through. Each case states what it proves.


def leaf(table: str, glyph: str, line_no: int = 1) -> Leaf:
    return Leaf(table=table, glyph=glyph, line_no=line_no)


def chord(modifiers: frozenset[str], char: str) -> key_authority.Chord:
    return key_authority.Chord(
        modifiers=modifiers,
        combos=frozenset({modifiers}),
        char=char,
        owner="guard-block",
        modes=frozenset({"Normal", "Insert"}),
        line_no=1,
        text=f"if key.code == KeyCode::Char('{char}')",
    )


@dataclass(frozen=True)
class Expectation:
    """What a self-test case proves."""

    violations: int
    rules: tuple[int, ...] = ()


SELF_TEST_CASES: list[tuple[str, dict[str, list[Leaf]], list[tuple[str, str | None]], list, Expectation]] = [
    (
        "a clean table has no violations",
        {"T": [leaf("T", "a"), leaf("T", "B")]},
        [("Space", "T")],
        [chord(frozenset({"CONTROL"}), "s")],
        Expectation(violations=0),
    ),
    (
        "a duplicated glyph in one table is a rule-1 violation",
        {"T": [leaf("T", "a"), leaf("T", "a", 2)]},
        [("Space", "T")],
        [],
        Expectation(violations=1, rules=(1,)),
    ),
    (
        "a non-printable glyph is a rule-2 violation",
        {"T": [leaf("T", "\n")]},
        [("Space", "T")],
        [],
        Expectation(violations=1, rules=(2,)),
    ),
    (
        "the same glyph in two tables of one prefix is a rule-3 violation",
        {"A": [leaf("A", "a")], "B": [leaf("B", "a")]},
        [("Space", "A"), ("Space", "B")],
        [],
        Expectation(violations=1, rules=(3,)),
    ),
    (
        "a leaf behind a NONE-modifier guard chord is a rule-4 violation",
        {"T": [leaf("T", "a")]},
        [("Space", "T")],
        [chord(frozenset({"NONE"}), "a")],
        Expectation(violations=1, rules=(4,)),
    ),
    (
        "a leaf behind a SHIFT-modifier guard chord is a rule-4 violation",
        {"T": [leaf("T", "B")]},
        [("Space", "T")],
        [chord(frozenset({"SHIFT"}), "b")],
        Expectation(violations=1, rules=(4,)),
    ),
    (
        "a CONTROL guard chord does not shadow a leaf",
        {"T": [leaf("T", "a")]},
        [("Space", "T")],
        [chord(frozenset({"CONTROL"}), "a")],
        Expectation(violations=0),
    ),
    (
        "a table no prefix binds is a rule-5 violation",
        {"T": [leaf("T", "a")]},
        [("Space", None)],
        [],
        Expectation(violations=1, rules=(5,)),
    ),
    (
        "a prefix bound to a missing table is a rule-5 violation",
        {"T": [leaf("T", "a")]},
        [("Space", "T"), ("Goto", "MISSING")],
        [],
        Expectation(violations=1, rules=(5,)),
    ),
]


def run_self_test() -> int:
    failures = 0
    for description, tables, bindings, guard_chords, expected in SELF_TEST_CASES:
        violations = analyse(tables, bindings, guard_chords)
        rules = tuple(sorted({violation.rule for violation in violations}))
        if len(violations) != expected.violations or rules != expected.rules:
            failures += 1
            print(f"  FAIL {description}")
            print(
                f"         expected {expected.violations} violation(s) with rules "
                f"{expected.rules}, got {len(violations)} with rules {rules}"
            )
            for violation in violations:
                print(f"         violation: {violation.message}")
        else:
            print(f"  ok   {description}")

    if failures:
        print(
            f"\nself-test FAILED: {failures} case(s) — the guard does not discriminate."
        )
        return 2

    print(f"\nself-test passed: {len(SELF_TEST_CASES)} case(s).")
    return 0


def main(argv: list[str]) -> int:
    unknown = [argument for argument in argv if argument != "--self-test"]
    if unknown:
        print(f"unknown argument(s): {' '.join(unknown)}", file=sys.stderr)
        return 2
    if "--self-test" in argv:
        return run_self_test()

    for path in (EDITOR, COMMANDS):
        if not path.is_file():
            print(f"  {path} is missing", file=sys.stderr)
            return 2

    try:
        editor_text = EDITOR.read_text()
        tables = parse_tables(strip_comments(editor_text))
        bindings = parse_prefix_bindings(editor_text)
        chords = guard_block_chords()
    except (ParseError, OSError) as error:
        print(
            f"TUI editor leaf-authority guard could not resolve a shape: {error}\n"
            "Teach the guard the new shape rather than skipping it — a guard that\n"
            "passes on input it does not understand is not a guard.\n",
            file=sys.stderr,
        )
        return 2

    violations = analyse(tables, bindings, chords)

    if violations:
        print(
            "TUI editor leaf-authority violations found.\n"
            "A leaf table is a namespace: two rows with the same glyph leave one of\n"
            "them dead, a glyph the dispatch cannot deliver is a key that does\n"
            "nothing, and a leaf behind the guard block can never run at all.\n"
        )
        for violation in violations:
            print(f"  [rule {violation.rule}] {violation.message}")
        print(f"\n{len(violations)} violation(s) found.")
        return 1

    glyph_count = sum(len(rows) for rows in tables.values())
    print(
        "TUI editor leaf-authority guard passed — "
        f"{len(tables)} leaf table(s), {glyph_count} leaf glyph(s), "
        f"{len(bindings)} prefix binding(s), no duplicate and no unreachable leaf"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
