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
`Ctrl+R (resync)` and `Ctrl+R (redo)`, which is the contradiction a shadow
produces in prose.

M001 of `plans/implementation/tui-editor-helix-parity/` fixes the instance.
This guard exists so the *class* cannot recur silently: the dispatch chain is a
sequence of ordered stages, so a chord declared in a later stage is unreachable
the moment an earlier stage claims it. That failure is invisible to the compiler
and to any test that calls a helper directly.

## The stages

Chords are attributed to three owners, in the order the route evaluates them:

1. `editor_buffer_key`'s guard block — the mode-independent chords
   (`Ctrl+s` -> save, `Ctrl+r` -> resync). These run **before** any mode
   dispatch.
2. `handle_normal_key` — the Normal-mode verbs.
3. `handle_insert_key` — the Insert-mode verbs.

The guard block used to claim a third chord, the Normal-only `R` -> reload.
M004 removed it, and this list moved with it: Helix gives `R` to
`replace_with_yanked`, and a Normal-only guard-block `R` would have shadowed
the verb while surviving it — dead in the one sense that mattered. Reload is
`:e`. The guard asserts the two remaining chords by name so this block cannot
pass by being emptied.

## The two dispatch-table shapes

`handle_normal_key` matches on the `(modifiers, code)` tuple:

    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('d')) => ...
        (m, KeyCode::Char('j')) | (m, KeyCode::Down) if m == none || m == shift => ...
    }

`handle_insert_key` still matches on `key.code` alone:

    match key.code {
        KeyCode::Char(ch) if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT => ...
        KeyCode::Backspace => ...
    }

Milestone M011 restructures the Insert table onto the tuple form, so this
guard reads **both** shapes. The two headers are mutually exclusive by
construction — `match key.code {` cannot match `match (key.modifiers,
key.code) {`, and the tuple header cannot match the legacy one — so the Insert
lookup can never mistake the Normal table for the Insert table, and a
tuple-form Insert table is never missed. `match_table_any` takes the **first**
dispatch table in the function body, whichever shape it is written in.

## How an arm is attributed

A **chord** is a `(modifiers, char)` claim over a character. It is what the
guard block can shadow, so it is what the shadowing rule is about and what the
summary line counts. `char is None` marks the **wildcard** catch-all, which
deliberately owns every printable character in its modifier set.

* The **tuple shape** spells the modifier in the pattern. A spelled-out
  modifier (`(KeyModifiers::CONTROL, KeyCode::Char('d'))`) matches exactly that
  modifier and is *not* subject to the arm's `if` predicate; a binding
  (`(m, KeyCode::Char('j')) if m == none || m == shift`) takes its modifier
  set from the predicate. A `KeyCode::Char(ch)` alternative is the wildcard:
  it is recorded as a wildcard claim over the modifiers it admits, so the
  catch-all is never silently dropped from the model.
* The **legacy shape** has no modifier in the pattern, so the modifier comes
  from the arm's `if` predicate: `KeyCode::Char('w') if key.modifiers ==
  KeyModifiers::CONTROL` is the chord `(CONTROL, 'w')`. An arm with no
  predicate at all claims every spelling of its key code, which is the
  conservative reading.

Both shapes therefore produce the same model for the same semantics, which is
the point: the guard's verdict must not depend on how the arm is spelled.

## Non-`Char` key codes

`KeyCode::Backspace`, `KeyCode::Delete`, `KeyCode::Left` and friends are not
character chords — the guard block never claims one, so no earlier stage can
shadow it. They are still tracked, as **key-code claims**: a
`(modifiers, key code)` claim. Two arms of one table claiming the same one is
the same defect as a duplicated chord, and M011 binds `Alt-Backspace` and
`Alt-Delete`.

**The decision M011 asked for: an explicit exactly-once check rather than a
second kind of counted chord.** The summary line counts character chords,
because that is what the guard block can shadow; folding key-code claims into
that count would change what "66 Normal-mode chord(s)" means for no gain, and
the line is quoted in the milestone closure. So key-code claims are policed —
two arms claiming the same one is a violation — and the Insert-mode word
chords are checked explicitly:

* `Alt-Backspace` must be claimed **exactly once** by the Insert table.
* `Alt-Delete` must be claimed **exactly once** by the Insert table.

The check reads the same in both shapes. In the legacy shape a modifier-less
arm claims its key code under *every* modifier — `match key.code {
KeyCode::Backspace => ... }` really does catch `Alt-Backspace` — so that
claim is counted there too; in the tuple shape the modifier is spelled out and
the claim is counted from it.

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
- The same `(modifiers, key code)` claim declared by two arms of one table.
- An Insert-mode word chord (`Alt-Backspace`, `Alt-Delete`) that the Insert
  table does not claim exactly once.

## Not violations, by design

- `KeyCode::Char(ch)` arms that bind the character to an identifier, such as
  `handle_insert_key`'s `KeyCode::Char(ch) if key.modifiers == ...`. That arm
  deliberately owns *every* printable character in Insert; it does not name a
  specific chord, so it is modelled as a wildcard rather than as a chord.
- The App layer's Normal-only pane gestures (`handle_editor_key`), which are a
  separate stage above this file's dispatch chain and are already documented at
  their call site.

## Failing loudly on unknown syntax

A guard that skips input it does not understand passes by not looking. Every
arm-shaped block inside a scanned dispatch table must resolve to a chord, to a
key-code claim, or to the catch-all; anything else is an **error**, not a skip.
Adding a binding in a new shape therefore fails the guard until the guard
learns the shape, which is the only ordering that makes it load-bearing.

## Non-vacuity

Two things make the guard non-vacuous.

The **model** asserts the three functions and the three guard-block chords
still exist and that all three tables are non-empty, so it cannot pass by the
file being emptied, the functions renamed, or the guards deleted.

The **detector** is exercised by `--self-test`, which feeds synthetic sources
through the *same* analysis the real tree goes through — not a
reimplementation of it — and fails if a planted violation is not reported. The
cases cover a duplicated Insert chord, an Insert chord shadowing a guard-block
chord, a missing `Alt-Delete` claim, a duplicated `Alt-Backspace` claim, a
shape the guard cannot resolve, and two clean sources (one per table shape) so
the self-test proves the detector *discriminates* rather than always failing.

Exit code 1 on violations, 2 on a shape it cannot resolve.

Usage::

    python3 scripts/check_tui_editor_key_authority.py
    python3 scripts/check_tui_editor_key_authority.py --self-test

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

# One **exact** modifier bitmask, as the set of modifier names it carries.
#
# The distinction matters. A Rust predicate `m == none || m == shift` admits
# two bitmasks — exactly `NONE` and exactly `SHIFT` — and not the combination
# of the two. A predicate `m == alt_shift` admits exactly `ALT | SHIFT` and
# nothing else. Flattening both into a set of modifier *names* loses the
# difference, and then `m == alt` and `m == alt_shift` look like they overlap
# when Rust's `==` on the bitflags says they are disjoint.
Combo = frozenset[str]

# The sentinel for an arm with no modifier predicate: it admits every
# combination, which is the conservative reading of an unguarded arm.
ANY_COMBO = frozenset({"*"})


def combo_names(combos: frozenset[Combo]) -> frozenset[str]:
    """The modifier names an arm's combinations carry.

    The names are what rules 1 and 3 and the printed labels use; the
    *combinations* are what rule 2 compares, because two arms conflict only
    when they can be pressed with the **same** bitmask.
    """
    if ANY_COMBO in combos:
        return frozenset(MODIFIERS)
    names: set[str] = set()
    for combo in combos:
        names |= combo
    return frozenset(names)


def combos_intersect(left: frozenset[Combo], right: frozenset[Combo]) -> bool:
    """Whether two arms can be pressed with the same modifier bitmask.

    The comparison is on the exact bitmask, not on the modifier names: an
    arm that accepts exactly `ALT` and one that accepts exactly `ALT |
    SHIFT` are disjoint, and Rust's `==` on the bitflags agrees. A wildcard
    arm — one with no modifier predicate — intersects everything.
    """
    if ANY_COMBO in left or ANY_COMBO in right:
        return True
    return bool(left & right)

# Declarations that must exist for the guard's rules to mean anything.
REQUIRED_SUBSTRINGS = [
    "fn editor_buffer_key(",
    "fn handle_normal_key(",
    "fn handle_insert_key(",
    "BufferKey::RequestSave",
    "BufferKey::RequestResync",
]

# The two dispatch-table shapes.
#
# The tuple shape is what `handle_normal_key` uses and what `handle_insert_key`
# is being migrated onto; the legacy shape is what `handle_insert_key` uses
# today. The two headers are mutually exclusive by construction, so a lookup
# for one can never match the other: `match key.code {` needs `key.code`
# straight after the keyword, and the tuple header opens with a parenthesis.
TUPLE_TABLE_RE = re.compile(r"match\s*\(\s*key\.modifiers\s*,\s*key\.code\s*\)")
LEGACY_TABLE_RE = re.compile(r"match\s+key\.code\s*\{")

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
# `(modifiers, code)` alternative, one per spelling in an arm's pattern.
#
# The glyph inside `KeyCode::Char(...)` is matched as a **quoted literal**
# rather than as `[^()]*`, because `Char('(')` and `Char(')')` are real Helix
# bindings — M008 claims both — and an alternation that cannot spell them
# makes this guard refuse to parse the arms that claim them. A guard that stops
# parsing stops checking, and stopping on exactly the chords it exists to police
# is the failure mode this whole script is arranged against. The glyph group is
# non-capturing so `findall` still yields exactly `(modifier, code)` pairs.
#
# The code alternative also accepts a **binding** (`KeyCode::Char(ch)`), which
# is the wildcard catch-all in the tuple shape. Without it the alternative
# would be captured as a bare `KeyCode::Char`, which is neither a literal nor a
# binding, and the catch-all would fall out of the model unnoticed.
ALT_RE = re.compile(
    r"\(\s*((?:KeyModifiers::)?[A-Za-z_]\w*)\s*,\s*"
    r"(KeyCode::\w+(?:\(\s*(?:'(?:\\.|[^'\\]){1}'|[a-z_]\w*)\s*\))?)\s*\)"
)
# `KeyCode::Char(ch)` where the char is a binding rather than a literal.
CATCHALL_CHAR_RE = re.compile(r"KeyCode::Char\(\s*[a-z_]\w*\s*\)")
# A non-`Char` key code — `KeyCode::Backspace`, `KeyCode::Down`, ... — which is
# tracked as a key-code claim rather than as a character chord.
NON_CHAR_KEYCODE_RE = re.compile(r"KeyCode::(?!Char\b)(\w+)")
LET_BINDING_RE = re.compile(
    r"\blet\s+([a-z_]\w*)\s*(?::\s*\w+\s*)?=\s*"
    r"((?:KeyModifiers::\w+\s*\|\s*)*KeyModifiers::\w+)\s*;"
)

# The Insert-mode word chords M011 binds, and the check the milestone asked
# for: each is claimed by the Insert table exactly once. See the module
# docstring for why this is an explicit check rather than a counted chord.
INSERT_WORD_CHORDS = (
    ("Backspace", "Alt-Backspace"),
    ("Delete", "Alt-Delete"),
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
    combos: frozenset[Combo]
    char: str | None
    owner: str
    modes: frozenset[str]
    line_no: int
    text: str

    def label(self) -> str:
        target = f"'{self.char}'" if self.char is not None else "any printable char"
        return f"{'|'.join(sorted(self.modifiers))}+{target}"


@dataclass(frozen=True)
class KeyCodeClaim:
    """One `(modifiers, key code)` claim over a non-`Char` key code.

    The guard block never claims a non-`Char` key code, so such a claim cannot
    be shadowed by an earlier stage; it is tracked because two arms of one
    table claiming the same one is the same defect as a duplicated chord.
    """

    modifiers: frozenset[str]
    combos: frozenset[Combo]
    key_code: str
    owner: str
    modes: frozenset[str]
    line_no: int
    text: str

    def label(self) -> str:
        return f"{'|'.join(sorted(self.modifiers))}+{self.key_code}"


@dataclass(frozen=True)
class Violation:
    """One violation, tagged with the rule that found it.

    The tag is what lets `--self-test` assert *which* rule fired. The printed
    text is the message alone, unchanged from the guard's original output.
    """

    rule: int
    message: str


@dataclass
class Analysis:
    """What one dispatch source resolves to."""

    guard_chords: list[Chord]
    normal_chords: list[Chord]
    normal_claims: list[KeyCodeClaim]
    insert_chords: list[Chord]
    insert_claims: list[KeyCodeClaim]
    problems: list[str]
    violations: list[Violation]


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


def match_table_any(
    body: list[str], patterns: tuple[re.Pattern, ...]
) -> tuple[int, list[str]]:
    """The first dispatch table in `body`, in whichever shape it is written.

    The table is the *earliest* line matching any of the patterns, so a body
    carrying both shapes resolves to the one that comes first rather than to
    whichever pattern happens to be listed first. This is how the Insert table
    is found in the legacy `match key.code {` shape and in the tuple
    `match (key.modifiers, key.code) {` shape without the two lookups ever
    confusing one table for the other.
    """
    best: tuple[int, re.Pattern] | None = None
    for pattern in patterns:
        for index, raw in enumerate(body):
            if pattern.search(raw):
                if best is None or index < best[0]:
                    best = (index, pattern)
                break
    if best is None:
        shapes = " | ".join(pattern.pattern for pattern in patterns)
        raise ParseError(f"no dispatch match found ({shapes})")
    return match_table(body, best[1])


def read_bindings(body: list[str]) -> dict[str, frozenset[str]]:
    """Collect `let none = KeyModifiers::NONE;`-style modifier aliases.

    A binding may carry **several** modifiers — `let alt_shift =
    KeyModifiers::ALT | KeyModifiers::SHIFT;` — because the combination is a
    modifier bitmask of its own, and the guard has to tell "exactly ALT"
    from "exactly ALT and SHIFT".
    """
    bindings: dict[str, frozenset[str]] = {}
    for code in body:
        match = LET_BINDING_RE.search(code)
        if not match:
            continue
        names = frozenset(
            part.split("::", 1)[1].strip() for part in match.group(2).split("|")
        )
        if names and names <= frozenset(MODIFIERS):
            bindings[match.group(1)] = names
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
) -> frozenset[Combo]:
    """Resolve an arm's modifier predicate to the bitmasks it admits.

    Each `m == X` term is one **exact** bitmask, so a predicate naming two
    terms admits two bitmasks and not their combination.

    No predicate at all admits every modifier: an unguarded arm claims all
    spellings of its key code, which is the conservative reading.
    """
    tokens = GUARD_PREDICATE_RE.findall(block)
    if not tokens:
        return frozenset({ANY_COMBO})
    combos: set[Combo] = set()
    for token in tokens:
        combos.add(resolve_modifier(token, bindings))
    if not combos:
        raise ParseError(f"predicate admitted no modifier at line {line_no}")
    return frozenset(combos)


def resolve_arm_modifier(
    token: str,
    predicate_combos: frozenset[Combo],
    owner: str,
    line_no: int,
) -> frozenset[Combo]:
    """The modifier bitmasks one `(modifier, code)` alternative admits.

    A pattern that spells the modifier out — `(KeyModifiers::CONTROL,
    KeyCode::Char('d'))` — matches exactly that modifier and nothing else, so
    it is not subject to the arm's `if` predicate. Only a binding like `m`
    takes its bitmasks from the predicate.
    """
    if token.startswith("KeyModifiers::"):
        named = token.split("::", 1)[1]
        if named not in MODIFIERS:
            raise ParseError(
                f"unresolved modifier {token!r} at {owner} line {line_no}"
            )
        return frozenset({frozenset({named})})
    return predicate_combos


def arm_blocks(table: list[str], base_line: int) -> list[tuple[int, str]]:
    """Split a match body into logical arms, returning `(file_line, pattern)`.

    An arm may span several lines — alternatives, then an `if` guard, then
    `=> {` — and an arm body may itself contain a nested `match` whose arms
    also carry `=>`. Splitting on the first `=>` would therefore swallow the
    inner match, so blocks are accumulated until the arm's own braces close.
    The returned text is the arm's *pattern*, truncated at its `=>`, so a body
    can never be mistaken for a claim.

    The arm's body may open on the `=>` line or on the line after it — the
    Insert table's catch-all arm is written with the brace on its own line —
    so an arm is closed only once its body is closed. Cutting the arm at the
    `=>` line instead would hand the orphan braces to the *next* arm, whose
    pattern would then carry a body it does not own, and the chord it names
    would be attributed to the wrong line.
    """
    blocks: list[tuple[int, str]] = []
    pending: list[str] = []
    pending_depth = 0
    depth = 0
    start_line = 0
    arrow_at = 0
    # The arm's body: "none" before the `=>` is seen, "open" for a braced
    # body, "expression" for a body that is not a block, and "pending" when
    # the `=>` ended its line and the next line has yet to say which it is.
    state = "none"

    for offset, code in enumerate(table):
        if not pending:
            start_line = base_line + offset + 1
            pending_depth = depth
            arrow_at = 0
            state = "none"
        elif state == "pending":
            # The previous line ended with the arm's `=>` and nothing else, so
            # this line either opens the arm's body or is the body itself.
            state = "open" if code.strip().startswith("{") else "expression"
        pending.append(code)
        depth += code.count("{") - code.count("}")
        joined = " ".join(part.strip() for part in pending)
        if state == "none":
            position = joined.find("=>")
            if position < 0:
                continue
            arrow_at = position
            rest = joined[position + 2 :]
            if "{" in rest:
                state = "open"
            elif not rest.strip():
                # The `=>` ends its line; the body starts on the next one.
                state = "pending"
                continue
            else:
                state = "expression"
        if state == "open" and depth > pending_depth:
            # The body is still open.
            continue
        if state == "pending":
            continue
        pattern = joined[:arrow_at].strip()
        if pattern == "_":
            # The catch-all arm. It claims nothing, which is the property that
            # lets an unclaimed key reach the composer.
            pending = []
            continue
        if "{" in pattern or "}" in pattern:
            # A pattern never carries a brace. One that does means the split
            # went wrong, and a mis-split that is reported as a clean arm is
            # how a chord disappears from the model without anyone noticing.
            raise ParseError(f"arm pattern carries a brace at line {start_line}: {pattern}")
        if ".." in pattern:
            # A range pattern covers more than its endpoints, so the guard
            # cannot say which characters it claims.
            raise ParseError(f"arm pattern is a range at line {start_line}: {pattern}")
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
                combos=frozenset({frozenset({match.group(2)})}),
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


def collect_table_claims(
    table: list[str],
    base_line: int,
    owner: str,
    mode: str,
    bindings: dict[str, frozenset[str]],
) -> tuple[list[Chord], list[KeyCodeClaim]]:
    """The claims made by the arms of one dispatch table.

    `bindings` comes from the whole function body, not from the table: the
    `let none = KeyModifiers::NONE;` aliases are declared above the `match`.

    Two kinds of claim come out of a table:

    * a **chord** — a `(modifiers, char)` claim over a character, which is
      what the guard block can shadow. `char is None` marks the wildcard
      catch-all, which claims every printable character in its modifier set.
    * a **key-code claim** — a `(modifiers, key code)` claim over a non-`Char`
      key code. The guard block never claims one, so no earlier stage can
      shadow it, but two arms of one table claiming the same one is the same
      defect as a duplicated chord.
    """
    chords: list[Chord] = []
    claims: list[KeyCodeClaim] = []
    for line_no, block in arm_blocks(table, base_line):
        alternatives = ALT_RE.findall(block)
        if not alternatives:
            # The legacy shape: the pattern names key codes directly and the
            # modifier comes from the arm's `if` predicate, if it has one.
            combos = resolve_predicate(block, bindings, line_no)
            modifiers = combo_names(combos)
            literal = CHAR_LITERAL_RE.search(block)
            wildcard = CATCHALL_CHAR_RE.search(block)
            key_codes = NON_CHAR_KEYCODE_RE.findall(block)
            if literal is not None:
                # `KeyCode::Char('w') if key.modifiers == KeyModifiers::CONTROL`
                # names one character and one modifier: it is the chord
                # `(CONTROL, 'w')`, not a shape this guard refuses to parse.
                chords.append(
                    Chord(
                        modifiers=modifiers,
                        combos=combos,
                        char=literal.group(1),
                        owner=owner,
                        modes=frozenset({mode}),
                        line_no=line_no,
                        text=block,
                    )
                )
            elif wildcard is not None:
                # `KeyCode::Char(ch) if ...` — the deliberate catch-all. It
                # owns *every* printable character, so it is recorded as a
                # wildcard claim rather than skipped: a guard chord falling
                # inside its modifier set would be dead in Insert.
                chords.append(
                    Chord(
                        modifiers=modifiers,
                        combos=combos,
                        char=None,
                        owner=owner,
                        modes=frozenset({mode}),
                        line_no=line_no,
                        text=block,
                    )
                )
            for key_code in key_codes:
                claims.append(
                    KeyCodeClaim(
                        modifiers=modifiers,
                        combos=combos,
                        key_code=key_code,
                        owner=owner,
                        modes=frozenset({mode}),
                        line_no=line_no,
                        text=block,
                    )
                )
            if literal is None and wildcard is None and not key_codes:
                raise ParseError(f"unresolved arm at {owner} line {line_no}: {block}")
            continue

        # The tuple shape: every alternative spells its own `(modifier, code)`.
        predicate_combos = resolve_predicate(block, bindings, line_no)
        arm_chords: list[Chord] = []
        arm_claims: list[KeyCodeClaim] = []
        wildcard_combos: set[Combo] = set()
        for modifier_token, code_expr in alternatives:
            arm_combos = resolve_arm_modifier(
                modifier_token, predicate_combos, owner, line_no
            )
            arm_modifiers = combo_names(arm_combos)
            literal = CHAR_LITERAL_RE.search(code_expr)
            if literal is not None:
                arm_chords.append(
                    Chord(
                        modifiers=arm_modifiers,
                        combos=arm_combos,
                        char=literal.group(1),
                        owner=owner,
                        modes=frozenset({mode}),
                        line_no=line_no,
                        text=block,
                    )
                )
                continue
            if CATCHALL_CHAR_RE.search(code_expr) is not None:
                # `KeyCode::Char(ch)` — the wildcard catch-all in the tuple
                # shape. It is one claim over the modifiers it admits, so the
                # alternatives of a single arm are merged into one wildcard
                # chord: the same model the legacy shape produces.
                wildcard_combos |= arm_combos
                continue
            # A non-`Char` key code: `KeyCode::Backspace`, `KeyCode::Delete`,
            # `KeyCode::Down`, ...
            key_code = NON_CHAR_KEYCODE_RE.search(code_expr)
            if key_code is None:
                raise ParseError(f"unresolved key code at {owner} line {line_no}: {block}")
            arm_claims.append(
                KeyCodeClaim(
                    modifiers=arm_modifiers,
                    combos=arm_combos,
                    key_code=key_code.group(1),
                    owner=owner,
                    modes=frozenset({mode}),
                    line_no=line_no,
                    text=block,
                )
            )
        if wildcard_combos:
            arm_chords.insert(
                0,
                Chord(
                    modifiers=combo_names(frozenset(wildcard_combos)),
                    combos=frozenset(wildcard_combos),
                    char=None,
                    owner=owner,
                    modes=frozenset({mode}),
                    line_no=line_no,
                    text=block,
                ),
            )
        chords.extend(arm_chords)
        claims.extend(arm_claims)
    return chords, claims


def analyse(text: str) -> Analysis:
    """Resolve one dispatch source into its claims, problems and violations.

    This is the single analysis the guard runs: the real tree and the
    `--self-test` synthetic sources both go through it, so the self-test
    exercises the detector rather than a copy of it.
    """
    lines = text.splitlines()

    guard_base, guard_lines = function_body(lines, "editor_buffer_key")
    normal_base, normal_lines = function_body(lines, "handle_normal_key")
    insert_base, insert_lines = function_body(lines, "handle_insert_key")

    guard_body = strip_all_comments(guard_lines)
    normal_body = strip_all_comments(normal_lines)
    insert_body = strip_all_comments(insert_lines)

    guard_chords = collect_guard_chords(guard_body, guard_base)

    normal_off, normal_table = match_table(normal_body, TUPLE_TABLE_RE)
    insert_off, insert_table = match_table_any(
        insert_body, (TUPLE_TABLE_RE, LEGACY_TABLE_RE)
    )

    normal_chords, normal_claims = collect_table_claims(
        normal_table,
        normal_base + normal_off,
        "handle_normal_key",
        "Normal",
        read_bindings(normal_body),
    )
    insert_chords, insert_claims = collect_table_claims(
        insert_table,
        insert_base + insert_off,
        "handle_insert_key",
        "Insert",
        read_bindings(insert_body),
    )

    problems = [
        f"expected declaration `{needle}` is absent"
        for needle in REQUIRED_SUBSTRINGS
        if needle not in text
    ]

    for label, chords, claims in (
        ("guard block", guard_chords, []),
        ("handle_normal_key", normal_chords, normal_claims),
        ("handle_insert_key", insert_chords, insert_claims),
    ):
        if not chords and not claims:
            problems.append(f"{label} declared no chords — the guard would be vacuous")

    violations: list[Violation] = []

    # 1. A later stage redeclaring a chord an earlier stage already claims.
    #    A wildcard claim (`char is None`) covers every character, so any guard
    #    chord sharing a modifier with it in a shared mode is dead. The guard
    #    block only ever claims a character chord, so a key-code claim cannot
    #    be shadowed by it and takes no part in this rule.
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
                        Violation(
                            1,
                            f"{TARGET.name}:{chord.line_no} [{owner}] {chord.text} claims "
                            f"{chord.label()}, but the guard block at "
                            f"{TARGET.name}:{guard.line_no} already claims "
                            f"{guard.label()} in {where} — unreachable",
                        )
                    )

    # 2. Two arms of one table claiming the same chord: the earlier wins.
    #
    #    The comparison is on the **exact** modifier bitmask, not on the
    #    modifier names. An arm that accepts exactly `ALT` and one that
    #    accepts exactly `ALT | SHIFT` are disjoint — Rust's `==` on the
    #    bitflags says so — and flagging them would be a false positive the
    #    source would have to be contorted to avoid.
    for chords, claims, owner in (
        (normal_chords, normal_claims, "handle_normal_key"),
        (insert_chords, insert_claims, "handle_insert_key"),
    ):
        seen: dict[tuple[str, str | None], list[tuple[frozenset[Combo], int, str]]] = {}
        entries = [
            (
                chord.combos,
                "wildcard" if chord.char is None else "char",
                chord.char,
                chord.label(),
                chord.line_no,
                chord.text,
            )
            for chord in chords
        ] + [
            (
                claim.combos,
                "code",
                claim.key_code,
                claim.label(),
                claim.line_no,
                claim.text,
            )
            for claim in claims
        ]
        for combos, kind, target, label, line_no, block_text in entries:
            for prior_combos, prior_line, prior_text in seen.get((kind, target), []):
                if combos_intersect(combos, prior_combos):
                    violations.append(
                        Violation(
                            2,
                            f"{TARGET.name}:{line_no} [{owner}] {block_text} re-declares "
                            f"{label}, already claimed at "
                            f"{TARGET.name}:{prior_line} ({prior_text}) — unreachable",
                        )
                    )
                    break
            seen.setdefault((kind, target), []).append((combos, line_no, block_text))

    # 3. The Insert-mode word chords are claimed by the Insert table exactly
    #    once. A modifier-less legacy arm claims its key code under every
    #    modifier, so it counts here as it does in the tuple shape.
    for key_code, chord_name in INSERT_WORD_CHORDS:
        owners = [
            claim
            for claim in insert_claims
            if claim.key_code == key_code and "ALT" in claim.modifiers
        ]
        if len(owners) == 1:
            continue
        where = ", ".join(str(claim.line_no) for claim in owners) or "no line"
        violations.append(
            Violation(
                3,
                f"{TARGET.name} [handle_insert_key] {chord_name} is claimed "
                f"{len(owners)} time(s) by the Insert table ({where}) — the "
                f"chord must have exactly one owner",
            )
        )

    return Analysis(
        guard_chords=guard_chords,
        normal_chords=normal_chords,
        normal_claims=normal_claims,
        insert_chords=insert_chords,
        insert_claims=insert_claims,
        problems=problems,
        violations=violations,
    )


# ── Self-test ────────────────────────────────────────────────────────
#
# Synthetic sources run through `analyse` — the same analysis the real tree
# goes through. Each case states what it proves.


DEFAULT_NORMAL_TABLE = """        (m, KeyCode::Char('j')) if m == none => {
            BufferKey::Consumed
        }
"""


def synthetic_source(insert_table: str, normal_table: str | None = None) -> str:
    """A synthetic editor source carrying the given Insert and Normal tables.

    The three functions and the two guard-block chords are always present: the
    guard's non-vacuity checks require them, so a synthetic source that
    omitted them would fail for the wrong reason.
    """
    normal = normal_table if normal_table is not None else DEFAULT_NORMAL_TABLE
    return (
        "fn editor_buffer_key(state: &mut EditorState, key: KeyEvent) -> BufferKey {\n"
        "    if key.code == KeyCode::Char('s') && key.modifiers == KeyModifiers::CONTROL {\n"
        "        return BufferKey::RequestSave;\n"
        "    }\n"
        "    if key.code == KeyCode::Char('r') && key.modifiers == KeyModifiers::CONTROL {\n"
        "        return BufferKey::RequestResync;\n"
        "    }\n"
        "    handle_normal_key(state, key)\n"
        "}\n"
        "\n"
        "fn handle_normal_key(state: &mut EditorState, key: KeyEvent) -> BufferKey {\n"
        "    let none = KeyModifiers::NONE;\n"
        "    match (key.modifiers, key.code) {\n"
        f"{normal}"
        "        _ => BufferKey::Unhandled,\n"
        "    }\n"
        "}\n"
        "\n"
        "fn handle_insert_key(state: &mut EditorState, key: KeyEvent) -> bool {\n"
        f"{insert_table}"
        "}\n"
    )


# The Insert table as it is written today: `match key.code`, with the
# catch-all's brace on its own line.
LEGACY_INSERT_TABLE = """    match key.code {
        KeyCode::Char(ch)
            if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
        {
            editor_insert_literal(state, &ch.to_string());
            true
        }
        KeyCode::Backspace => {
            editor_delete_backward(state);
            true
        }
        KeyCode::Delete => {
            editor_editing_command(state, EditorCommand::DeleteChar);
            true
        }
        _ => false,
    }
"""

# The Insert table as M011 restructures it: `match (key.modifiers, key.code)`,
# with the catch-all spelled as two alternatives that keep `NONE | SHIFT`.
TUPLE_INSERT_TABLE = """    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char(ch))
        | (KeyModifiers::SHIFT, KeyCode::Char(ch)) => {
            editor_insert_literal(state, &ch.to_string());
            true
        }
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            editor_delete_backward(state);
            true
        }
        (KeyModifiers::ALT, KeyCode::Backspace) => {
            editor_delete_word_backward(state);
            true
        }
        (KeyModifiers::NONE, KeyCode::Delete) => {
            editor_editing_command(state, EditorCommand::DeleteChar);
            true
        }
        (KeyModifiers::ALT, KeyCode::Delete) => {
            editor_delete_word_forward(state);
            true
        }
        _ => false,
    }
"""


@dataclass(frozen=True)
class Expectation:
    """What one self-test case must produce.

    `rules` is the exact set of guard rules that must fire, which is what
    separates "a duplicated chord" (rule 2) from "a chord shadowed by the
    guard block" (rule 1).
    """

    violations: int = 0
    problems: int = 0
    parse_error: bool = False
    rules: tuple[int, ...] = ()
    normal_chars: tuple[str, ...] | None = None
    insert_chars: tuple[str | None, ...] | None = None
    wildcard_modifiers: frozenset[str] | None = None


def _expectation_problems(expected: Expectation, analysis: Analysis) -> list[str]:
    """The mismatches between what a case expects and what the analysis found."""
    problems: list[str] = []
    if len(analysis.violations) != expected.violations:
        problems.append(
            f"expected {expected.violations} violation(s), found {len(analysis.violations)}"
        )
    if len(analysis.problems) != expected.problems:
        problems.append(
            f"expected {expected.problems} problem(s), found {len(analysis.problems)}"
        )
    rules = tuple(sorted({violation.rule for violation in analysis.violations}))
    if rules != expected.rules:
        problems.append(f"expected rule(s) {expected.rules}, found {rules}")
    if expected.normal_chars is not None:
        found = tuple(chord.char for chord in analysis.normal_chords)
        if found != expected.normal_chars:
            problems.append(f"expected Normal chord(s) {expected.normal_chars}, found {found}")
    if expected.insert_chars is not None:
        found = tuple(chord.char for chord in analysis.insert_chords)
        if found != expected.insert_chars:
            problems.append(f"expected Insert chord(s) {expected.insert_chars}, found {found}")
    if expected.wildcard_modifiers is not None:
        wildcards = [
            chord for chord in analysis.insert_chords if chord.char is None
        ]
        if len(wildcards) != 1:
            problems.append(f"expected exactly one Insert wildcard, found {len(wildcards)}")
        else:
            found = wildcards[0].modifiers
            if found != expected.wildcard_modifiers:
                problems.append(
                    f"expected wildcard modifiers {sorted(expected.wildcard_modifiers)}, "
                    f"found {sorted(found)}"
                )
    return problems


def _legacy_insert_table(*extra_arms: str) -> str:
    """The legacy Insert table with extra arms spliced in before the catch-all."""
    marker = "        _ => false,\n"
    head, _, tail = LEGACY_INSERT_TABLE.partition(marker)
    return head + "".join(extra_arms) + marker + tail


def _tuple_insert_table(*extra_arms: str) -> str:
    """The tuple Insert table with extra arms spliced in before the catch-all."""
    marker = "        _ => false,\n"
    head, _, tail = TUPLE_INSERT_TABLE.partition(marker)
    return head + "".join(extra_arms) + marker + tail


SELF_TEST_CASES: list[tuple[str, str, Expectation]] = [
    (
        "the legacy Insert table is clean",
        synthetic_source(LEGACY_INSERT_TABLE),
        Expectation(
            violations=0,
            rules=(),
            normal_chars=("j",),
            insert_chars=(None,),
            wildcard_modifiers=frozenset({"NONE", "SHIFT"}),
        ),
    ),
    (
        "the tuple-form Insert table is clean and its wildcard survives",
        synthetic_source(TUPLE_INSERT_TABLE),
        Expectation(
            violations=0,
            rules=(),
            normal_chars=("j",),
            insert_chars=(None,),
            wildcard_modifiers=frozenset({"NONE", "SHIFT"}),
        ),
    ),
    (
        "a duplicated Insert chord in the legacy table is a rule-2 violation",
        synthetic_source(
            _legacy_insert_table(
                "        KeyCode::Char('x') if key.modifiers == KeyModifiers::CONTROL => {\n"
                "            true\n"
                "        }\n",
                "        KeyCode::Char('x') if key.modifiers == KeyModifiers::CONTROL => {\n"
                "            true\n"
                "        }\n",
            )
        ),
        Expectation(violations=1, rules=(2,)),
    ),
    (
        "a duplicated Insert chord in the tuple table is a rule-2 violation",
        synthetic_source(
            _tuple_insert_table(
                "        (KeyModifiers::CONTROL, KeyCode::Char('x')) => {\n"
                "            true\n"
                "        }\n",
                "        (KeyModifiers::CONTROL, KeyCode::Char('x')) => {\n"
                "            true\n"
                "        }\n",
            )
        ),
        Expectation(violations=1, rules=(2,)),
    ),
    (
        "an Insert chord shadowing the guard block's Ctrl+s is a rule-1 violation",
        synthetic_source(
            _legacy_insert_table(
                "        KeyCode::Char('s') if key.modifiers == KeyModifiers::CONTROL => {\n"
                "            true\n"
                "        }\n",
            )
        ),
        Expectation(violations=1, rules=(1,)),
    ),
    (
        "the same shadow in the tuple table is a rule-1 violation",
        synthetic_source(
            _tuple_insert_table(
                "        (KeyModifiers::CONTROL, KeyCode::Char('s')) => {\n"
                "            true\n"
                "        }\n",
            )
        ),
        Expectation(violations=1, rules=(1,)),
    ),
    (
        "a duplicated Alt-Backspace claim is reported",
        synthetic_source(
            _tuple_insert_table(
                "        (KeyModifiers::ALT, KeyCode::Backspace) => {\n"
                "            true\n"
                "        }\n",
            )
        ),
        Expectation(violations=2, rules=(2, 3)),
    ),
    (
        "a missing Alt-Delete claim is reported",
        synthetic_source(
            _tuple_insert_table().replace(
                "        (KeyModifiers::ALT, KeyCode::Delete) => {\n"
                "            editor_delete_word_forward(state);\n"
                "            true\n"
                "        }\n",
                "",
            )
        ),
        Expectation(violations=1, rules=(3,)),
    ),
    (
        "ALT and ALT|SHIFT are disjoint bitmasks, so the two arms are not a duplicate",
        synthetic_source(
            "    let alt = KeyModifiers::ALT;\n"
            "    let alt_shift = KeyModifiers::ALT | KeyModifiers::SHIFT;\n"
            + TUPLE_INSERT_TABLE.replace(
                "    match (key.modifiers, key.code) {\n",
                "    match (key.modifiers, key.code) {\n"
                "        (m, KeyCode::Char('u')) if m == alt => {\n"
                "            true\n"
                "        }\n"
                "        (m, KeyCode::Char('u')) if m == alt_shift => {\n"
                "            true\n"
                "        }\n",
            )
        ),
        Expectation(violations=0, rules=()),
    ),
    (
        "a shape the guard cannot resolve is an error, not a pass",
        synthetic_source(
            _tuple_insert_table(
                "        (KeyModifiers::BOGUS, KeyCode::Char('x')) => {\n"
                "            true\n"
                "        }\n",
            )
        ),
        Expectation(parse_error=True),
    ),
]


def run_self_test() -> int:
    failures = 0
    for description, source, expected in SELF_TEST_CASES:
        try:
            analysis = analyse(source)
        except ParseError as error:
            if expected.parse_error:
                print(f"  ok   {description}")
            else:
                failures += 1
                print(f"  FAIL {description}: unexpected unresolvable shape: {error}")
            continue
        if expected.parse_error:
            failures += 1
            print(f"  FAIL {description}: expected an unresolvable shape, got a clean analysis")
            continue
        problems = _expectation_problems(expected, analysis)
        if problems:
            failures += 1
            print(f"  FAIL {description}")
            for problem in problems:
                print(f"         {problem}")
            for violation in analysis.violations:
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

    if not TARGET.is_file():
        print(f"  {TARGET} is missing", file=sys.stderr)
        return 2

    text = TARGET.read_text()

    try:
        analysis = analyse(text)
    except ParseError as error:
        print(
            f"TUI editor key-authority guard could not resolve a dispatch shape: {error}\n"
            "Teach the guard the new shape rather than skipping it — a guard that\n"
            "passes on input it does not understand is not a guard.\n",
            file=sys.stderr,
        )
        return 2

    problems = analysis.problems
    violations = analysis.violations

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
            print(f"  {violation.message}")
        print(f"\n{len(problems) + len(violations)} violation(s) found.")
        return 1

    print(
        "TUI editor key-authority guard passed — "
        f"{len(analysis.guard_chords)} guard-block chord(s), "
        f"{len(analysis.normal_chords)} Normal-mode chord(s), "
        f"{len(analysis.insert_chords)} Insert-mode chord(s), no shadowed arm"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
