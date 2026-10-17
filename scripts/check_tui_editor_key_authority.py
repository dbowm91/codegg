#!/usr/bin/env python3
"""Static guard: every editor key chord has exactly one owner.

Milestone M001 of the TUI Editor Helix Parity roadmap repairs a binding that
could never run. `handle_normal_key` declared

    (KeyModifiers::CONTROL, KeyCode::Char('r')) => editor_redo

but `editor_buffer_key` resolves the mode-independent chords **before** it
dispatches on mode, and had already claimed `Ctrl+r` for `RequestResync`. The
arm was dead code: Normal mode advertised undo with no reachable redo, and
because the three tests covering redo called `editor_redo(&mut state)` directly
instead of pressing a key, nothing failed. The help overlay even listed both
`Ctrl+R` (resync) and `Ctrl+R (redo)`, which is the contradiction a shadow
produces in prose.

M001 of `plans/implementation/tui-editor-helix-parity/` fixes the instance.
This guard exists so the *class* cannot recur silently: the dispatch chain is a
sequence of ordered stages, so a chord declared in a later stage is unreachable
the moment an earlier stage claims it. That failure is invisible to the compiler
and to any test that calls a helper directly.

## The stages

Chords are attributed to three owners, in the order the route evaluates them:

1. `editor_buffer_key`'s guard block — the mode-independent chords
   (`Ctrl+s` -> save, `Ctrl+r` -> resync) and the Normal-only `R` -> reload.
   These run **before** any mode dispatch.
2. `handle_normal_key` — the Normal-mode verbs.
3. `handle_insert_key` — the Insert-mode verbs.

## What counts as a violation

**Violations** (exit code 1):

- A chord declared in `handle_normal_key` that the guard block already claims
  for Normal mode. The guard returns first; the arm is dead.
- A chord declared in `handle_insert_key` that the guard block already claims
  for Insert mode. Same failure, other mode.
- The same `(modifiers, char)` chord declared by **two** arms of one dispatch
  table. The earlier arm wins, so the later one is unreachable. This is not
  hypothetical either: `(m, KeyCode::Char('d')) if m == none` (delete the held
  selection) precedes `(m, KeyCode::Char('D')) | (m, KeyCode::Char('d')) if m ==
  shift || m == none` (delete to end of line), so the `Char('d')` alternative of
  the second arm could never match.

## Not violations, by design

- `KeyCode::Char(ch)` arms that bind the character to an identifier, such as
  `handle_insert_key`'s `KeyCode::Char(ch) if key.modifiers == ...`. That arm
  deliberately owns *every* printable character in Insert; it does not name a
  specific chord.
- Non-`Char` key codes (`Down`, `PageDown`, `Backspace`, `Esc`, ...). The guard
  block only ever claims `Char` chords, so no collision is possible, and
  widening the scope would only add parsing surface to keep honest.
- A guard whose mode predicate excludes the table's mode. `R` -> reload is
  Normal-only, so it constrains `handle_normal_key` and not
  `handle_insert_key`.
- The App layer's Normal-only pane gestures (`handle_editor_key`), which are a
  separate stage above this file's dispatch chain and are already documented at
  their call site.

## Failing loudly on unknown syntax

A guard that skips input it does not understand passes by not looking. Every
arm-shaped block inside a scanned dispatch table must resolve to a chord or to an
explicitly recognised non-`Char`/catch-all arm; anything else is an **error**,
not a skip. Adding a binding in a new shape therefore fails the guard until the
guard learns the shape, which is the only ordering that makes it load-bearing.

## Non-vacuity

The guard asserts the three functions and the three guard-block chords still
exist and that all three tables are non-empty, so it cannot pass by the file
being emptied, the functions renamed, or the guards deleted.

Exit code 1 on violations, 2 on a shape it cannot resolve.

Usage::

    python3 scripts/check_tui_editor_key_authority.py

This is invoked by `scripts/verify.sh quick` and the CI `verify` job.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
TARGET = REPO_ROOT / "src" / "tui" / "commands" / "editor.rs"

# Every modifier the dispatch tables may name. A modifier outside this set is an
# unresolved shape rather than a silent pass.
MODIFIERS = ("NONE", "SHIFT", "CONTROL", "ALT", "SUPER", "HYPER", "META")

# Declarations that must exist for the guard's rules to mean anything.
REQUIRED_SUBSTRINGS = [
    "fn editor_buffer_key(",
    "fn handle_normal_key(",
    "fn handle_insert_key(",
    "BufferKey::RequestSave",
    "BufferKey::RequestResync",
    "BufferKey::RequestReload",
]

# `KeyCode::Char('s')`, capturing just the glyph.
CHAR_GLYPH = r"'((?:\\.|[^'\\]){1})'"
CHAR_LITERAL = r"KeyCode::Char\(\s*" + CHAR_GLYPH + r"\s*\)"
CHAR_LITERAL_RE = re.compile(CHAR_LITERAL)
GUARD_RE = re.compile(
    r"if\s+key\.code\s*==\s*" + CHAR_LITERAL + r"\s*&&\s*key\.modifiers\s*==\s*KeyModifiers::(\w+)"
)
# A line that opens a guard statement of the shape this guard understands.
GUARD_IF_RE = re.compile(r"\s*if\s+key\.code\s*==")
NORMAL_ONLY_RE = re.compile(r"mode\s*==\s*[^\n]*InputMode::Normal")
GUARD_PREDICATE_RE = re.compile(
    r"\b(?:m|key\.modifiers)\s*==\s*((?:KeyModifiers::)?[A-Za-z_]\w*)"
)
ALT_RE = re.compile(
    r"\(\s*((?:KeyModifiers::)?[A-Za-z_]\w*)\s*,\s*(KeyCode::\w+(?:\(\s*[^()]*\s*\))?)\s*\)"
)
ANY_KEYCODE_RE = re.compile(r"KeyCode::\w+")
# `KeyCode::Char(ch)` where the char is a binding rather than a literal.
CATCHALL_CHAR_RE = re.compile(r"KeyCode::Char\(\s*[a-z_]\w*\s*\)")
LET_BINDING_RE = re.compile(
    r"\blet\s+([a-z_]\w*)\s*(?::\s*\w+\s*)?=\s*KeyModifiers::(\w+)\s*;"
)


class ParseError(Exception):
    """A dispatch shape the guard cannot resolve."""


@dataclass(frozen=True)
class Chord:
    """One `(modifiers, char)` claim, attributed to an owner and a line.

    `char` is `None` for a wildcard claim: `handle_insert_key` deliberately owns
    *every* printable character in Insert mode, so any guard chord falling
    inside that modifier set for Insert is dead.
    """

    modifiers: frozenset[str]
    char: str | None
    owner: str
    modes: frozenset[str]
    line_no: int
    text: str

    def label(self) -> str:
        target = f"'{self.char}'" if self.char is not None else "any printable char"
        return f"{'|'.join(sorted(self.modifiers))}+{target}"


def strip_comment(line: str) -> str:
    """Drop a trailing `//` comment, respecting string and char literals.

    Rust char literals matter here: `KeyCode::Char('/')` contains a slash, and a
    naive scan would read it as the start of a comment.
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


def strip_all_comments(lines: list[str]) -> list[str]:
    """Comment-free lines with the same indices, so line numbers stay true."""
    return [strip_comment(line) for line in lines]


def function_body(lines: list[str], name: str) -> tuple[int, list[str]]:
    """`(first_index, comment_free_body_lines)` for `fn name(...)`.

    `first_index` is the zero-based file index of `body[0]`, which is the line
    *after* the one holding the opening brace — not `start + 1`, because a
    multi-line signature puts several lines between them.

    The opening brace is excluded, so brace depth inside the body is balanced.
    """
    start = next(
        (i for i, raw in enumerate(lines) if f"fn {name}(" in strip_comment(raw)),
        None,
    )
    if start is None:
        raise ParseError(f"fn {name}( not found")

    depth = 0
    opened = False
    first_index: int | None = None
    body: list[str] = []
    for index in range(start, len(lines)):
        code = strip_comment(lines[index])
        for char in code:
            if char == "{":
                depth += 1
                opened = True
            elif char == "}":
                depth -= 1
                if opened and depth == 0:
                    if first_index is None:
                        raise ParseError(f"fn {name}( has an empty body")
                    return first_index, body
        if opened:
            if first_index is None:
                first_index = index
            body.append(lines[index])
    raise ParseError(f"fn {name}( has unbalanced braces")


def match_table(
    body: list[str], pattern: re.Pattern
) -> tuple[int, list[str]]:
    """The `match` block a dispatch table lives in, brace-matched.

    Scoping to the `match` matters: `handle_normal_key` also contains a small
    `match yank_range(...)` above the main table whose arms are not key claims.
    The `match ... {` header is dropped; it is not an arm.

    `first_index` is the zero-based file index of the first retained line.
    """
    start = next((i for i, raw in enumerate(body) if pattern.search(raw)), None)
    if start is None:
        raise ParseError(f"no dispatch match found ({pattern.pattern})")

    depth = 0
    opened = False
    table: list[str] = []
    first_index: int | None = None
    for index in range(start, len(body)):
        code = body[index]
        for char in code:
            if char == "{":
                depth += 1
                opened = True
            elif char == "}":
                depth -= 1
                if opened and depth == 0:
                    if first_index is None:
                        raise ParseError("empty dispatch match")
                    return first_index, table
        if opened:
            if first_index is None:
                # The `match ... {` header: drop it and record the index of
                # the line after it.
                first_index = index + 1
            else:
                table.append(body[index])
    raise ParseError("dispatch match has unbalanced braces")


def read_bindings(body: list[str]) -> dict[str, frozenset[str]]:
    """Collect `let none = KeyModifiers::NONE;`-style modifier aliases."""
    bindings: dict[str, frozenset[str]] = {}
    for code in body:
        match = LET_BINDING_RE.search(code)
        if match and match.group(2) in MODIFIERS:
            bindings[match.group(1)] = frozenset({match.group(2)})
    return bindings


def resolve_modifier(token: str, bindings: dict[str, frozenset[str]]) -> frozenset[str]:
    token = token.strip()
    if token.startswith("KeyModifiers::"):
        token = token.split("::", 1)[1]
    if token in bindings:
        return bindings[token]
    if token in MODIFIERS:
        return frozenset({token})
    raise ParseError(f"unresolved modifier token {token!r}")


def resolve_predicate(
    block: str, bindings: dict[str, frozenset[str]], line_no: int
) -> frozenset[str]:
    """Resolve an arm's modifier predicate to the modifiers it admits.

    No predicate at all admits every modifier: an unguarded arm claims all
    spellings of its key code, which is the conservative reading.
    """
    tokens = GUARD_PREDICATE_RE.findall(block)
    if not tokens:
        return frozenset(MODIFIERS)
    modifiers: set[str] = set()
    for token in tokens:
        modifiers |= resolve_modifier(token, bindings)
    if not modifiers:
        raise ParseError(f"predicate admitted no modifier at line {line_no}")
    return frozenset(modifiers)


def arm_blocks(table: list[str], base_line: int) -> list[tuple[int, str]]:
    """Split a match body into logical arms, returning `(file_line, pattern)`.

    An arm may span several lines — alternatives, then an `if` guard, then
    `=> {` — and an arm body may itself contain a nested `match` whose arms
    also carry `=>`. Splitting on the first `=>` would therefore swallow the
    inner match, so blocks are accumulated until the arm's own braces close.
    The returned text is the arm's *pattern*, truncated at its `=>`, so a body
    can never be mistaken for a claim.
    """
    blocks: list[tuple[int, str]] = []
    pending: list[str] = []
    pending_depth = 0
    depth = 0
    start_line = 0

    for offset, code in enumerate(table):
        if not pending:
            start_line = base_line + offset + 1
            pending_depth = depth
        pending.append(code)
        depth += code.count("{") - code.count("}")
        joined = " ".join(part.strip() for part in pending)
        if "=>" not in joined:
            continue
        # A `=>` seen while the arm's own block is still open belongs to a
        # nested match — `d` dispatches through one — and does not end the arm.
        # The arm ends when its braces close back to its starting depth.
        if depth > pending_depth:
            continue
        pattern = joined[: joined.index("=>")].strip()
        if pattern == "_":
            # The catch-all arm. It claims nothing, which is the property that
            # lets an unclaimed key reach the composer.
            pending = []
            continue
        blocks.append((start_line, pattern))
        pending = []

    if pending:
        raise ParseError(f"trailing arm without `=>` at line {start_line}")
    return blocks


def collect_guard_chords(body: list[str], base_line: int) -> list[Chord]:
    """Chords claimed by the mode-independent / Normal-only guard block.

    Each guard is read as a whole statement rather than through a fixed-size
    sliding window. A window is ambiguous in exactly the way that matters here:
    the `R` -> reload guard wraps across four lines, and a window that holds
    `if key.code == Char('R')` plus `&& key.modifiers == NONE` but not yet
    `&& mode == ...::InputMode::Normal` reads as a mode-*independent* claim,
    which would make the guard block look as though it swallowed `R` in Insert
    mode. Reading to the statement's opening brace removes the guesswork.
    """
    chords: list[Chord] = []
    index = 0
    while index < len(body):
        if not GUARD_IF_RE.match(body[index]):
            index += 1
            continue
        statement = [body[index]]
        cursor = index
        while "{" not in " ".join(statement) and cursor + 1 < len(body):
            cursor += 1
            statement.append(body[cursor])
        joined = " ".join(part.strip() for part in statement)
        match = GUARD_RE.search(joined)
        if match is None:
            raise ParseError(f"unresolved guard statement: {joined}")
        normal_only = NORMAL_ONLY_RE.search(joined) is not None
        chords.append(
            Chord(
                modifiers=frozenset({match.group(2)}),
                char=match.group(1),
                owner="guard-block",
                modes=(
                    frozenset({"Normal"}) if normal_only else frozenset({"Normal", "Insert"})
                ),
                line_no=base_line + index + 1,
                text=joined.rstrip("{").strip(),
            )
        )
        index = cursor + 1

    if not chords:
        raise ParseError("no guard-block chords recognised")
    return chords


def collect_table_chords(
    table: list[str],
    base_line: int,
    owner: str,
    mode: str,
    bindings: dict[str, frozenset[str]],
) -> list[Chord]:
    """Chords claimed by the arms of one dispatch table.

    `bindings` comes from the whole function body, not from the table: the
    `let none = KeyModifiers::NONE;` aliases are declared above the `match`.
    """
    chords: list[Chord] = []
    for line_no, block in arm_blocks(table, base_line):
        alternatives = ALT_RE.findall(block)
        if not alternatives:
            # `KeyCode::Char(ch) if ... =>` — the deliberate Insert-mode
            # catch-all. It owns *every* printable character, so it is recorded
            # as a wildcard claim rather than skipped: a guard chord falling
            # inside its modifier set would be dead in Insert.
            if "KeyCode::Char(" in block:
                if CHAR_LITERAL_RE.search(block):
                    # A Char literal that no `(modifiers, code)` alternative
                    # accounted for. Skipping this would let a binding become
                    # invisible to the guard by changing how it is spelled.
                    raise ParseError(
                        f"unattributed Char arm at {owner} line {line_no}: {block}"
                    )
                chords.append(
                    Chord(
                        modifiers=resolve_predicate(block, bindings, line_no),
                        char=None,
                        owner=owner,
                        modes=frozenset({mode}),
                        line_no=line_no,
                        text=block,
                    )
                )
                continue
            # `KeyCode::Backspace =>`, `KeyCode::Esc =>`, ... — out of scope.
            if ANY_KEYCODE_RE.search(block):
                continue
            raise ParseError(f"unresolved arm at {owner} line {line_no}: {block}")

        modifiers = resolve_predicate(block, bindings, line_no)
        for modifier_token, code_expr in alternatives:
            literal = CHAR_LITERAL_RE.search(code_expr)
            if not literal or CATCHALL_CHAR_RE.search(code_expr):
                continue
            # A pattern that spells the modifier out — `(KeyModifiers::CONTROL,
            # KeyCode::Char('d'))` — matches exactly that modifier and nothing
            # else, so it is not subject to the arm's `if` predicate. Only a
            # binding like `m` takes its modifier set from the predicate.
            if modifier_token.startswith("KeyModifiers::"):
                named = modifier_token.split("::", 1)[1]
                if named not in MODIFIERS:
                    raise ParseError(
                        f"unresolved modifier {modifier_token!r} at {owner} line {line_no}"
                    )
                arm_modifiers = frozenset({named})
            else:
                arm_modifiers = modifiers
            chords.append(
                Chord(
                    modifiers=arm_modifiers,
                    char=literal.group(1),
                    owner=owner,
                    modes=frozenset({mode}),
                    line_no=line_no,
                    text=block,
                )
            )
    return chords


def main() -> int:
    if not TARGET.is_file():
        print(f"  {TARGET} is missing", file=sys.stderr)
        return 2

    lines = TARGET.read_text().splitlines()
    text = "\n".join(lines)

    try:
        guard_base, guard_lines = function_body(lines, "editor_buffer_key")
        normal_base, normal_lines = function_body(lines, "handle_normal_key")
        insert_base, insert_lines = function_body(lines, "handle_insert_key")

        guard_body = strip_all_comments(guard_lines)
        normal_body = strip_all_comments(normal_lines)
        insert_body = strip_all_comments(insert_lines)

        guard_chords = collect_guard_chords(guard_body, guard_base)

        normal_off, normal_table = match_table(
            normal_body, re.compile(r"match\s*\(\s*key\.modifiers\s*,\s*key\.code\s*\)")
        )
        insert_off, insert_table = match_table(
            insert_body, re.compile(r"match\s+key\.code\s*\{")
        )

        normal_chords = collect_table_chords(
            normal_table,
            normal_base + normal_off,
            "handle_normal_key",
            "Normal",
            read_bindings(normal_body),
        )
        insert_chords = collect_table_chords(
            insert_table,
            insert_base + insert_off,
            "handle_insert_key",
            "Insert",
            read_bindings(insert_body),
        )
    except ParseError as error:
        print(
            f"TUI editor key-authority guard could not resolve a dispatch shape: {error}\n"
            "Teach the guard the new shape rather than skipping it — a guard that\n"
            "passes on input it does not understand is not a guard.\n",
            file=sys.stderr,
        )
        return 2

    problems = [
        f"expected declaration `{needle}` is absent"
        for needle in REQUIRED_SUBSTRINGS
        if needle not in text
    ]

    for label, chords in (
        ("guard block", guard_chords),
        ("handle_normal_key", normal_chords),
        ("handle_insert_key", insert_chords),
    ):
        if not chords:
            problems.append(f"{label} declared no chords — the guard would be vacuous")

    violations: list[str] = []

    # 1. A later stage redeclaring a chord an earlier stage already claims.
    #    A wildcard claim (`char is None`) covers every character, so any guard
    #    chord sharing a modifier with it in a shared mode is dead.
    for chords, owner in (
        (normal_chords, "handle_normal_key"),
        (insert_chords, "handle_insert_key"),
    ):
        for chord in chords:
            for guard in guard_chords:
                shared_mode = chord.modes & guard.modes
                same_char = chord.char is None or guard.char is None or chord.char == guard.char
                if shared_mode and same_char and chord.modifiers & guard.modifiers:
                    where = ", ".join(sorted(shared_mode))
                    violations.append(
                        f"{TARGET.name}:{chord.line_no} [{owner}] {chord.text} claims "
                        f"{chord.label()}, but the guard block at "
                        f"{TARGET.name}:{guard.line_no} already claims "
                        f"{guard.label()} in {where} — unreachable"
                    )

    # 2. Two arms of one table claiming the same chord: the earlier wins.
    for chords, owner in (
        (normal_chords, "handle_normal_key"),
        (insert_chords, "handle_insert_key"),
    ):
        seen: dict[tuple[str, str | None], Chord] = {}
        for chord in chords:
            for modifier in sorted(chord.modifiers):
                key = (modifier, chord.char)
                first = seen.get(key)
                if first is None:
                    seen[key] = chord
                    continue
                violations.append(
                    f"{TARGET.name}:{chord.line_no} [{owner}] {chord.text} re-declares "
                    f"{chord.label()}, already claimed at "
                    f"{TARGET.name}:{first.line_no} ({first.text}) — unreachable"
                )
                break

    if problems or violations:
        print(
            "TUI editor key-authority violations found.\n"
            "The editor dispatch chain is a sequence of ordered stages, so a chord\n"
            "declared in a later stage is dead the moment an earlier stage claims it.\n"
            "That is how Normal mode ended up with `u` for undo and no reachable\n"
            "redo. Every chord needs exactly one owner.\n"
        )
        for problem in problems:
            print(f"  guard-vacuous [{problem}]")
        for violation in violations:
            print(f"  {violation}")
        print(f"\n{len(problems) + len(violations)} violation(s) found.")
        return 1

    print(
        "TUI editor key-authority guard passed — "
        f"{len(guard_chords)} guard-block chord(s), "
        f"{len(normal_chords)} Normal-mode chord(s), "
        f"{len(insert_chords)} Insert-mode chord(s), no shadowed arm"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())