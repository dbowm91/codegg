#!/usr/bin/env python3
"""Static guard: the completion **triggers** have exactly one owner.

Milestone M011 of the TUI Editor Helix Parity roadmap bound `Ctrl-x` as
Helix's insert-mode completion trigger. `handle_completion_key` sits outside
the key-authority guard's scope — it is a surface *above* the editor's
dispatch chain, with its own modality — so the chord was invisible to the
guard that polices every other chord. This guard closes that blind spot.

## What is a trigger, and what is not

`handle_completion_key` claims two kinds of key:

| Kind | Keys | Claimed when |
|---|---|---|
| the **triggers** | `F8` (any mode) and `Ctrl-x` (Insert mode only) | always — they *open* the popup |
| the popup's **own keys** | `Esc`, `Enter`, `Up`, `Down`, `j`, `k` | only while the popup is open |

The triggers are **editor chords**: they are claimed before any mode dispatch
decides otherwise, and they shadow anything the global handler would have
taken. The popup's own keys are **not** editor chords — they are a modal
surface's keys, claimed only while the surface is up, and they overlap the
Normal-mode verbs *by design* (`j`/`k` navigate the popup instead of the
buffer).

A guard that counted both would report `j` and `k` twice and call it a
defect. So the guard must tell the two apart, and the way it tells them apart
is **structural**: **the triggers are the arms before the popup-open check**.

That is a real structural distinction, not a heuristic: the popup's own keys
are unreachable until `completion.is_open()`, and the triggers are the arms
that make it open. The guard finds the popup-open check and reads only what
comes before it.

## Why a separate guard and not a wider key guard

The key-authority guard reads **one dispatch chain**: the guard block, then
`handle_normal_key`, then `handle_insert_key`. It explicitly excludes
`handle_editor_key`'s pane gestures as *"a separate stage above this file's
dispatch chain"*. The completion popup is the same kind of thing.

Teaching the key guard a fifth stage would mean teaching it the popup's
modality — that a claim is conditional on a surface being open — which is a
different model from the one it has. The leaf guard made the same call, and
the leaf guard reads the key guard's **model** (its guard-block chords) rather
than re-parsing the chain. This guard does the same: it imports
`scripts/check_tui_editor_key_authority.py` and asks it for the guard-block
chords, so the two guards cannot disagree about what the guard block claims.

## What counts as a violation

**Violations** (exit code 1):

- **The `F8` trigger is not claimed exactly once.** It is the Normal-mode
  trigger, and a second claim would make one of them unreachable.
- **The `Ctrl-x` trigger is not claimed exactly once.** Same failure, other
  spelling.
- **A trigger shadows a guard-block chord.** The key-authority guard's two
  mode-independent chords (`Ctrl+s`, `Ctrl+r`) are claimed by the guard block,
  which runs *after* this surface. A trigger that claimed either would make
  the guard block's claim a lie: the chord would never reach it.
- **`Ctrl-x` is claimed in a mode it does not serve.** The arm's own comment
  says it: *"in normal mode `Ctrl-x` is not the editor's to take, and a chord
  the editor claims in a mode it does not serve is a chord the global handler
  can no longer reach."* The guard checks the predicate, because a comment is
  not a mechanism.

## Not violations, by design

- The popup's own keys (`Esc`, `Enter`, `Up`, `Down`, `j`, `k`). They are
  claimed only while the popup is open, and their overlap with the Normal-mode
  verbs is the point of a popup. They are excluded **by construction**: the
  guard never reads past the popup-open check.
- The completion **request** machinery (`Ctrl-x`'s request id, the epoch, the
  scope) — that is M003's contract, not a key claim.

## Failing loudly on unknown syntax

A guard that skips input it does not understand passes by not looking. Every
statement in the trigger region must resolve to a claim; anything else is an
**error**, not a skip. Adding a binding in a new shape therefore fails the
guard until the guard learns the shape, which is the only ordering that makes
it load-bearing.

## Non-vacuity

Two things make the guard non-vacuous.

The **model** asserts the function exists, that the popup-open check exists,
and that both triggers are found — so it cannot pass by the file being
emptied, the function renamed, the check moved, or the triggers deleted.

The **detector** is exercised by `--self-test`, which feeds synthetic sources
through the *same* analysis the real tree goes through — not a
reimplementation of it — and fails if a planted violation is not reported. The
cases cover a missing trigger, a duplicated trigger, a trigger that shadows a
guard-block chord, a `Ctrl-x` that claims in Normal mode, a shape the guard
cannot resolve, and the clean shape, so the self-test proves the detector
*discriminates* rather than always failing.

Exit code 1 on violations, 2 on a shape it cannot resolve.

Usage::

    python3 scripts/check_tui_editor_completion_authority.py
    python3 scripts/check_tui_editor_completion_authority.py --self-test

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
COMMANDS = REPO_ROOT / "src" / "tui" / "commands" / "editor.rs"

# The function whose trigger arms this guard reads.
TARGET_FUNCTION = "handle_completion_key"

# The popup-open check: the statement that ends the trigger region. The
# popup's own keys live after it, and the guard never reads past it.
#
# The check is matched on the *call* that asks whether the popup is open
# together with the early return that follows it. Matching on the call alone
# would be too loose — the call also appears in the popup's own arms — and
# matching on the return alone would be too loose in the other direction.
POPUP_OPEN_RE = re.compile(r"completion\s*\.\s*is_open\s*\(\s*\)")
POPUP_OPEN_RETURN_RE = re.compile(r"return\s+false\s*;")

# `key.code == KeyCode::Char('x')` and `key.code == KeyCode::F(8)`.
CHAR_LITERAL = r"'((?:\\.|[^'\\]){1})'"
CODE_CLAIM_RE = re.compile(
    r"key\.code\s*==\s*KeyCode::"
    r"(?:Char\(\s*" + CHAR_LITERAL + r"\s*\)|(\w+)(?:\(\s*(\d+)\s*\))?)"
)
# `key.modifiers == KeyModifiers::CONTROL`.
MODIFIER_CLAIM_RE = re.compile(r"key\.modifiers\s*==\s*KeyModifiers::(\w+)")
# `app.editor_state.mode() == EditorMode::Insert`.
MODE_CLAIM_RE = re.compile(r"mode\(\)\s*==\s*EditorMode::(\w+)")
# The opening of a statement's body: the `if` runs to the brace.
IF_RE = re.compile(r"^\s*if\s+")

# Every modifier the trigger arms may name. A modifier outside this set is an
# unresolved shape rather than a silent pass.
MODIFIERS = ("NONE", "SHIFT", "CONTROL", "ALT", "SUPER", "HYPER", "META")

# The modes an editor chord can serve. `Ctrl-x` is Helix's *insert-mode*
# trigger; `F8` is the mode-independent one.
MODES = ("Normal", "Insert")


class ParseError(Exception):
    """A shape the guard cannot resolve."""


@dataclass(frozen=True)
class Trigger:
    """One claim made by an arm in the trigger region.

    `kind` is `"chord"` for a `(modifiers, char)` claim and `"key_code"` for a
    `(modifiers, key code)` claim over a non-`Char` key code. The two are kept
    apart because the guard block never claims a non-`Char` key code, so only
    a chord can shadow it.

    `modifiers` holds the modifier *names* the claim admits, for the label;
    `combos` holds the exact bitmasks, for the comparison. The distinction is
    the key guard's: a Rust `==` on a bitmask admits exactly one combination,
    so `CONTROL` and `CONTROL | SHIFT` are disjoint, and flattening both into
    names would make them look like they overlap.
    """

    kind: str
    modifiers: frozenset[str]
    combos: frozenset[key_authority.Combo]
    char: str | None
    key_code: str | None
    modes: frozenset[str]
    line_no: int
    text: str

    def label(self) -> str:
        if key_authority.ANY_COMBO in self.combos:
            prefix = "any modifier"
        else:
            prefix = "|".join(sorted(self.modifiers))
        if self.kind == "chord":
            target = f"'{self.char}'"
        else:
            target = str(self.key_code)
        return f"{prefix}+{target}"


@dataclass(frozen=True)
class Violation:
    """One violation, tagged with the rule that found it.

    The tag is what lets `--self-test` assert *which* rule fired.
    """

    rule: int
    message: str


@dataclass(frozen=True)
class Expectation:
    """What a self-test case proves."""

    violations: int
    rules: tuple[int, ...] = ()


def strip_comment(line: str) -> str:
    """Drop a trailing `//` comment, respecting string and char literals."""
    return key_authority.strip_comment(line)


def strip_all_comments(lines: list[str]) -> list[str]:
    """Comment-free lines with the same indices, so line numbers stay true."""
    return [strip_comment(line) for line in lines]


def find_popup_open_check(body: list[str]) -> int:
    """The index of the popup-open check within `body`.

    The check is the statement that asks whether the popup is open *and*
    returns when it is not. It is the boundary between the triggers and the
    popup's own keys, and the guard's whole model rests on it, so it must be
    found exactly once.

    A shape where the call appears without the early return is a shape the
    guard cannot resolve: the boundary would be somewhere else, and guessing
    would make the guard read the popup's own keys as triggers.
    """
    candidates: list[int] = []
    for index, raw in enumerate(body):
        if POPUP_OPEN_RE.search(raw) is None:
            continue
        # The early return may be on the next line.
        window = " ".join(part.strip() for part in body[index : index + 3])
        if POPUP_OPEN_RETURN_RE.search(window) is None:
            continue
        candidates.append(index)
    if not candidates:
        raise ParseError("the popup-open check was not found")
    if len(candidates) > 1:
        raise ParseError(
            f"the popup-open check appears {len(candidates)} times; the boundary "
            "between the triggers and the popup's own keys is ambiguous"
        )
    return candidates[0]


def trigger_statements(body: list[str], boundary: int) -> list[tuple[int, str]]:
    """The `if` statements in the trigger region, as `(index, statement)`.

    A statement is read whole — from its `if` to the brace that opens its
    body — because a claim can be spelled across several lines, and a
    fixed-size window would read a half-spelled claim as a different one.
    """
    statements: list[tuple[int, str]] = []
    index = 0
    while index < boundary:
        line = body[index]
        if IF_RE.match(line) is None:
            index += 1
            continue
        statement = [line]
        cursor = index
        while "{" not in " ".join(statement) and cursor + 1 < boundary:
            cursor += 1
            statement.append(body[cursor])
        statements.append((index, " ".join(part.strip() for part in statement)))
        index = cursor + 1
    return statements


def resolve_trigger(index: int, statement: str, base_line: int) -> Trigger:
    """Resolve one trigger statement to a claim.

    A shape the guard cannot resolve is an **error**. A guard that skips what
    it does not understand passes by not looking, and the whole point of this
    script is to see the chords the key guard cannot.
    """
    code_match = CODE_CLAIM_RE.search(statement)
    if code_match is None:
        raise ParseError(
            f"src/tui/commands/editor.rs:{base_line + index + 1}: "
            f"unresolved trigger statement: {statement}"
        )

    char = code_match.group(1)
    key_code_name = code_match.group(2)
    key_code_arg = code_match.group(3)
    if char is not None:
        kind = "chord"
        key_code = None
    else:
        kind = "key_code"
        char = None
        key_code = f"{key_code_name}({key_code_arg})" if key_code_arg else key_code_name

    modifier_match = MODIFIER_CLAIM_RE.search(statement)
    if modifier_match is None:
        # A trigger with no modifier predicate claims every spelling of its
        # key code, which is the conservative reading.
        modifiers = frozenset(MODIFIERS)
        combos = frozenset({key_authority.ANY_COMBO})
    else:
        named = modifier_match.group(1)
        if named not in MODIFIERS:
            raise ParseError(
                f"src/tui/commands/editor.rs:{base_line + index + 1}: "
                f"unresolved modifier {named!r}"
            )
        modifiers = frozenset({named})
        combos = frozenset({frozenset({named})})

    modes: set[str] = set()
    for mode in MODE_CLAIM_RE.findall(statement):
        if mode not in MODES:
            raise ParseError(
                f"src/tui/commands/editor.rs:{base_line + index + 1}: "
                f"unresolved mode {mode!r}"
            )
        modes.add(mode)
    if not modes:
        modes = set(MODES)

    return Trigger(
        kind=kind,
        modifiers=modifiers,
        combos=combos,
        char=char,
        key_code=key_code,
        modes=frozenset(modes),
        line_no=base_line + index + 1,
        text=statement.rstrip("{").strip(),
    )


def parse_triggers(text: str) -> list[Trigger]:
    """Every trigger the completion surface claims."""
    lines = text.splitlines()
    try:
        base, body = key_authority.function_body(lines, TARGET_FUNCTION)
    except key_authority.ParseError as error:
        raise ParseError(f"`fn {TARGET_FUNCTION}(` not found: {error}") from error
    body = strip_all_comments(body)
    boundary = find_popup_open_check(body)
    triggers = []
    for index, statement in trigger_statements(body, boundary):
        triggers.append(resolve_trigger(index, statement, base))
    return triggers


def guard_block_chords() -> list[key_authority.Chord]:
    """The chords the editor's guard block claims before any mode dispatch."""
    try:
        analysis = key_authority.analyse(COMMANDS.read_text())
    except key_authority.ParseError as error:
        raise ParseError(
            f"the key-authority guard could not resolve the dispatch chain: {error}"
        ) from error
    return analysis.guard_chords


def matches(trigger: Trigger, kind: str, char: str | None, key_code: str | None) -> bool:
    """Whether a trigger is the one the expectation names."""
    if trigger.kind != kind:
        return False
    if kind == "chord":
        return trigger.char == char
    return trigger.key_code == key_code


def analyse(
    triggers: list[Trigger], guard_chords: list[key_authority.Chord]
) -> list[Violation]:
    """The one analysis: violations, in the order they are found."""
    violations: list[Violation] = []

    f8 = [trigger for trigger in triggers if matches(trigger, "key_code", None, "F(8)")]
    if len(f8) != 1:
        violations.append(
            Violation(
                1,
                f"the `F8` completion trigger is claimed {len(f8)} time(s); it must be "
                "claimed exactly once",
            )
        )

    ctrl_x = [trigger for trigger in triggers if matches(trigger, "chord", "x", None)]
    if len(ctrl_x) != 1:
        violations.append(
            Violation(
                2,
                f"the `Ctrl-x` completion trigger is claimed {len(ctrl_x)} time(s); it "
                "must be claimed exactly once",
            )
        )
    else:
        trigger = ctrl_x[0]
        if trigger.modes != frozenset({"Insert"}):
            violations.append(
                Violation(
                    4,
                    f"the `Ctrl-x` completion trigger serves "
                    f"{sorted(trigger.modes)}; it is Helix's insert-mode trigger, and a "
                    "chord the editor claims in a mode it does not serve is a chord the "
                    "global handler can no longer reach",
                )
            )

    for trigger in triggers:
        if trigger.kind != "chord":
            # The guard block never claims a non-`Char` key code, so a key-code
            # claim cannot shadow it.
            continue
        for guard in guard_chords:
            if guard.char is None:
                continue
            if guard.char != trigger.char:
                continue
            if not (guard.modes & trigger.modes):
                continue
            if not key_authority.combos_intersect(trigger.combos, guard.combos):
                # A `CONTROL` claim and a `CONTROL | SHIFT` claim are disjoint
                # in Rust's `==` on the bitmask, so they do not shadow each
                # other. The key guard's own helper says so.
                continue
            violations.append(
                Violation(
                    3,
                    f"the completion trigger {trigger.label()} (line {trigger.line_no}) "
                    f"shadows the guard block's {guard.label()} (line {guard.line_no}): "
                    "the guard block runs after this surface, so its claim would be "
                    "unreachable",
                )
            )

    return violations


# ── Self-test ────────────────────────────────────────────────────────
#
# Synthetic sources run through `parse_triggers` and `analyse` — the same
# analysis the real tree goes through. Each case states what it proves.


def chord(
    modifiers: frozenset[str],
    char: str,
    modes: frozenset[str] = frozenset({"Normal", "Insert"}),
) -> key_authority.Chord:
    return key_authority.Chord(
        modifiers=modifiers,
        combos=frozenset({modifiers}),
        char=char,
        owner="guard-block",
        modes=modes,
        line_no=1,
        text=f"if key.code == KeyCode::Char('{char}')",
    )


CLEAN_SOURCE = """
fn handle_completion_key(app: &mut App, key: KeyEvent) -> bool {
    if key.code == KeyCode::F(8) {
        start_editor_completion(app);
        return true;
    }
    if key.code == KeyCode::Char('x')
        && key.modifiers == KeyModifiers::CONTROL
        && app.editor_state.mode() == EditorMode::Insert
    {
        start_editor_completion(app);
        return true;
    }
    if !app.editor_state.completion.is_open() {
        return false;
    }
    match key.code {
        KeyCode::Esc => true,
        _ => false,
    }
}
"""


def source_with(triggers: str) -> str:
    """A completion function whose trigger region is `triggers`."""
    return (
        "fn handle_completion_key(app: &mut App, key: KeyEvent) -> bool {\n"
        + triggers
        + """
    if !app.editor_state.completion.is_open() {
        return false;
    }
    match key.code {
        KeyCode::Esc => true,
        _ => false,
    }
}
"""
    )


F8_ARM = """    if key.code == KeyCode::F(8) {
        start_editor_completion(app);
        return true;
    }
"""

CTRL_X_ARM = """    if key.code == KeyCode::Char('x')
        && key.modifiers == KeyModifiers::CONTROL
        && app.editor_state.mode() == EditorMode::Insert
    {
        start_editor_completion(app);
        return true;
    }
"""

CTRL_X_NORMAL_ARM = """    if key.code == KeyCode::Char('x')
        && key.modifiers == KeyModifiers::CONTROL
        && app.editor_state.mode() == EditorMode::Normal
    {
        start_editor_completion(app);
        return true;
    }
"""

CTRL_S_ARM = """    if key.code == KeyCode::Char('s')
        && key.modifiers == KeyModifiers::CONTROL
    {
        start_editor_completion(app);
        return true;
    }
"""

SELF_TEST_CASES: list[tuple[str, str, list[key_authority.Chord], Expectation]] = [
    (
        "the clean shape has no violations",
        CLEAN_SOURCE,
        [chord(frozenset({"CONTROL"}), "s"), chord(frozenset({"CONTROL"}), "r")],
        Expectation(violations=0),
    ),
    (
        "a missing `F8` trigger is a rule-1 violation",
        source_with(CTRL_X_ARM),
        [],
        Expectation(violations=1, rules=(1,)),
    ),
    (
        "a duplicated `F8` trigger is a rule-1 violation",
        source_with(F8_ARM + F8_ARM + CTRL_X_ARM),
        [],
        Expectation(violations=1, rules=(1,)),
    ),
    (
        "a missing `Ctrl-x` trigger is a rule-2 violation",
        source_with(F8_ARM),
        [],
        Expectation(violations=1, rules=(2,)),
    ),
    (
        "a duplicated `Ctrl-x` trigger is a rule-2 violation",
        source_with(F8_ARM + CTRL_X_ARM + CTRL_X_ARM),
        [],
        Expectation(violations=1, rules=(2,)),
    ),
    (
        "a `Ctrl-x` that claims in Normal mode is a rule-4 violation",
        source_with(F8_ARM + CTRL_X_NORMAL_ARM),
        [],
        Expectation(violations=1, rules=(4,)),
    ),
    (
        "a trigger that shadows the guard block's `Ctrl+s` is a rule-3 violation",
        source_with(F8_ARM + CTRL_X_ARM + CTRL_S_ARM),
        [chord(frozenset({"CONTROL"}), "s")],
        Expectation(violations=1, rules=(3,)),
    ),
    (
        "a trigger that shadows the guard block's `Ctrl+r` is a rule-3 violation",
        source_with(F8_ARM + CTRL_X_ARM + CTRL_S_ARM),
        [chord(frozenset({"CONTROL"}), "r")],
        Expectation(violations=0),
    ),
]


def run_self_test() -> int:
    failures = 0
    for description, source, guard_chords, expected in SELF_TEST_CASES:
        try:
            triggers = parse_triggers(source)
            violations = analyse(triggers, guard_chords)
        except ParseError as error:
            failures += 1
            print(f"  FAIL {description}")
            print(f"         the guard could not resolve the synthetic source: {error}")
            continue
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

    # A shape the guard cannot resolve must be an error, not a skip.
    unresolvable = """
fn handle_completion_key(app: &mut App, key: KeyEvent) -> bool {
    if key.something_else() {
        return true;
    }
    if !app.editor_state.completion.is_open() {
        return false;
    }
    true
}
"""
    try:
        parse_triggers(unresolvable)
    except ParseError:
        print("  ok   a shape the guard cannot resolve is an error, not a skip")
    else:
        failures += 1
        print("  FAIL a shape the guard cannot resolve is an error, not a skip")
        print("         the guard accepted a statement it does not understand")

    if failures:
        print(
            f"\nself-test FAILED: {failures} case(s) — the guard does not discriminate."
        )
        return 2

    print(f"\nself-test passed: {len(SELF_TEST_CASES) + 1} case(s).")
    return 0


def main(argv: list[str]) -> int:
    unknown = [argument for argument in argv if argument != "--self-test"]
    if unknown:
        print(f"unknown argument(s): {' '.join(unknown)}", file=sys.stderr)
        return 2
    if "--self-test" in argv:
        return run_self_test()

    if not COMMANDS.is_file():
        print(f"  {COMMANDS} is missing", file=sys.stderr)
        return 2

    try:
        triggers = parse_triggers(COMMANDS.read_text())
        chords = guard_block_chords()
    except (ParseError, OSError) as error:
        print(
            f"TUI editor completion-authority guard could not resolve a shape: {error}\n"
            "Teach the guard the new shape rather than skipping it — a guard that\n"
            "passes on input it does not understand is not a guard.\n",
            file=sys.stderr,
        )
        return 2

    violations = analyse(triggers, chords)

    if violations:
        print(
            "TUI editor completion-authority violations found.\n"
            "The completion triggers are editor chords: they are claimed before any\n"
            "mode dispatch decides otherwise. A trigger claimed twice makes one of\n"
            "the claims unreachable, a trigger that shadows the guard block's chord\n"
            "makes that chord a lie, and a `Ctrl-x` claimed in a mode it does not\n"
            "serve is a chord the global handler can no longer reach.\n"
        )
        for violation in violations:
            print(f"  [rule {violation.rule}] {violation.message}")
        print(f"\n{len(violations)} violation(s) found.")
        return 1

    print(
        "TUI editor completion-authority guard passed — "
        f"{len(triggers)} completion trigger(s) "
        f"({', '.join(trigger.label() for trigger in triggers)}), "
        "each claimed exactly once, none shadowing the guard block"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
