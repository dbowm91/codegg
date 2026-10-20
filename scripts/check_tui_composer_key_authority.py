#!/usr/bin/env python3
"""Static guard: the composer's Normal-mode claim table owns its keys.

ADR-0042 gives the main composer a closed set of Normal-mode letters, and
the app's global binding map is consulted only for letters the composer does
*not* claim. The rule that makes the table worth having is the **fallthrough
rule**:

    A claimed letter is consumed by the composer even when the composer's
    verb is currently a no-op. It must not silently become an unrelated app
    action.

The failure this guard exists to prevent is the one the interactive terminal
panel already records: *"a focused panel is consulted before Normal-mode
actions, so an unclaimed `d` would silently become `OpenDiff` and never delete
anything."* The composer is the third surface with that problem, and the
first whose verbs arrive later than its claims — for most of the table's life
every entry but one was reserved, so every one of those entries was a no-op
that must still swallow its key. The exception was ``i``, whose verb is live:
it is the app's only route back to Insert mode, so a reserved ``i`` would
swallow the key and leave the user with no way to type. M006 closed the
table, so every claim is live and the fallthrough rule is now carried by
behaviour rather than by consumption.

The guard reads two files:

* ``src/tui/composer_claims.rs`` — the claim table itself. It is the single
  source of truth for what the composer owns; there is deliberately no second
  copy of it anywhere, so this file is the only place the table is read.
* ``src/tui/input.rs`` — the two binding maps, ``is_normal_only``, and the
  help's own rows.

It fails when:

* a claim is malformed, or two claims share a letter;
* a claimed letter can still reach an app-level action (the fallthrough
  violation);
* a claimed letter that *is* bound in the app maps is not a conditional
  claim, so the condition that saves it is not written down;
* a re-spelled app-level chord is missing from either binding map;
* a re-spelled app-level action has no chord to reach it by, or still answers
  to a key the composer can claim — which is how the old letter keeps reaching
  it, since all four old letters are claimed;
* a claimed capital letter is not reachable under both the shifted and the
  unshifted encoding;
* a claimed key is bound under a modifier that Insert mode does not skip,
  without its action being listed in ``is_normal_only`` — the capital-letter
  text guarantee;
* the two maps disagree on a caret motion, so the composer would have a
  capability in one keymap and not in the other;
* **the help's composer section is not derived from the claim table** — a
  hand-written list is a second copy of the table, and a second copy is the
  drift the claim table exists to prevent;
* **a live verb has no label**, so the derived row could not describe it;
* **the help advertises a claimed key for a different action** — the class
  M006 hit with the ``a`` → "Focus sidebar" row, where the key was claimed by
  the composer while the help still taught it as the sidebar's. The one
  documented exception is a *conditional* claim's fall-through row, which
  carries its condition;
* **the help advertises a reserved claim**, which would teach a gesture that
  does nothing;
* **a re-spelled action is not documented under a modifier chord**, or is
  documented under its old bare letter.

The two help rules are separate on purpose. One asks "does the help describe
the composer's verbs?"; the other asks "does the help describe the *app's*
re-spelled verbs?". They read different tables and catch different classes.

The guard is scoped to the composer. It does not police the editor's or the
terminal panel's keys, and it does not touch their guards.

Usage::

    python3 scripts/check_tui_composer_key_authority.py
    python3 scripts/check_tui_composer_key_authority.py --self-test
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CLAIMS_SRC = REPO_ROOT / "src" / "tui" / "composer_claims.rs"
INPUT_SRC = REPO_ROOT / "src" / "tui" / "input.rs"

# The app-level actions ADR-0042 moved because the composer claims the letter
# they used to own. The list is the guard's declaration of *which* actions
# were re-spelled; the chords themselves are read from the binding maps, so
# there is one source of truth for the chords and one for the fact that they
# moved.
#
# The value is the action's **old claimed letter** — the letter the composer
# now owns. It is not a list of every spelling the action used to have: a bare
# spelling of a letter the composer does *not* claim is a different thing, and
# the plan's invariant ("a letter the composer does not claim must resolve to
# the same app action it resolves to today") requires it to keep working. The
# default map's `/` for focus-prompt is the live example: it is the gesture
# that enters command mode, and command mode is what makes the rest of a
# typed slash command land as text.
RESPELLED_ACTIONS = {
    "ToggleAgentRun": "x",
    "OpenDiff": "d",
    "FocusPrompt": "i",
    "FocusSidebar": "a",
}

# The actions that move the composer caret.
#
# A caret motion is a **Helix-parity capability, not a keymap flavour**: `h`
# and `l` are Helix's horizontal motions, and ADR-0042 and the composer plan
# both record them as "no conflict — the existing action already moves the
# composer caret, so the composer keeps it". That premise is only true if the
# binding exists in **every** map the app can be configured with.
#
# It was not. The vim map has always bound `h` and `l`; the default map bound
# neither, and `vim_mode` defaults to false. In the default configuration the
# composer's Normal-mode caret therefore had no letter that could move it —
# `h` and `l` were unbound, and `w`, `b` and `e` are claimed by the composer
# and consumed as pending verbs. The composer showed a caret that no key
# could move.
#
# G9 is the rule that keeps the premise true. It is derived from the maps
# rather than from a list of letters, so it catches any caret motion that
# drifts between the two configurations — not only `h` and `l`.
CARET_MOTION_ACTIONS = {"Left", "Right", "Home", "End"}

# The modifiers a claim can be reached under. A claim is a *bare* letter, so
# NONE is always one of them; a capital claim adds SHIFT because terminals
# disagree about whether a shifted key carries the modifier.
BARE = ("NONE",)
BARE_PLUS_SHIFT = ("NONE", "SHIFT")

# The conditions a claim may carry. `Always` is the ordinary case;
# `MultiLineBuffer` is the one conditional ADR-0042 defines, for `j`/`k`.
KNOWN_CONDITIONS = {"Always", "MultiLineBuffer"}


@dataclass(frozen=True)
class Claim:
    """One row of the composer's claim table."""

    key: str
    verb: str
    status: str
    encodings: str
    condition: str
    line_no: int = 0

    @property
    def claimed_keys(self) -> tuple[tuple[tuple[str, ...], str], ...]:
        """The (modifiers, char) keys this claim answers to."""
        if self.encodings == "CapitalBoth":
            return ((("NONE",), self.key), (("SHIFT",), self.key))
        return ((("NONE",), self.key),)

    @property
    def is_conditional(self) -> bool:
        return self.condition != "Always"


@dataclass(frozen=True)
class Binding:
    """One key binding in one of the built-in maps."""

    modifiers: tuple[str, ...]
    char: str | None
    action: str
    line_no: int = 0
    # The non-character key code (`KeyCode::Left`, `KeyCode::Home`, …).
    #
    # It used to be dropped, because the guard only reasoned about letters.
    # Rule G9 reasons about **caret motions**, and the arrow and Home/End
    # keys are caret motions whose identity the guard has to know: a binding
    # with no character and no code cannot be told apart from any other
    # non-character key, so a map that lost `Left` and kept `Home` would
    # have looked identical to one that kept both.
    code: str | None = None

    @property
    def is_bare_char(self) -> bool:
        """Whether Insert mode skips this binding because it is bare text."""
        return self.modifiers == ("NONE",) and self.char is not None

    @property
    def key_id(self) -> tuple[tuple[str, ...], str | None, str | None]:
        """The binding's key identity: modifiers, character, and key code."""
        return (self.modifiers, self.char, self.code)


@dataclass
class Parsed:
    claims: list[Claim] = field(default_factory=list)
    default_map: list[Binding] = field(default_factory=list)
    vim_map: list[Binding] = field(default_factory=list)
    normal_only: set[str] = field(default_factory=set)
    help_entries: list[HelpEntry] = field(default_factory=list)
    verb_labels: dict[str, str] = field(default_factory=dict)
    #: Whether the help's composer section is built from the claim table's
    #: derived verb list. The guard reads the *call*, because the rows
    #: themselves are computed at runtime and are not in the source.
    help_is_derived: bool = False
    problems: list[str] = field(default_factory=list)


# ── Parsing ────────────────────────────────────────────────────────────
#
# The parsers are deliberately strict: a construct they do not understand is
# reported as a problem rather than skipped. A guard that silently stops
# reading a binding is worse than no guard, because it reads as coverage.

CLAIM_BLOCK = re.compile(r"ComposerClaim\s*\{(?P<body>.*?)\n    \}", re.DOTALL)
CLAIM_FIELD = re.compile(
    r"(?P<name>key|verb|status|encodings|condition)\s*:\s*(?P<value>[^,\n]+),"
)
FOR_BLOCK = re.compile(
    r"for\s*\(\s*(?P<kvar>\w+)\s*,\s*(?P<avar>\w+)\s*\)\s*in\s*\[(?P<items>.*?)\]\s*\{",
    re.DOTALL,
)
LETTER_ITEM = re.compile(r"\(\s*'(?P<char>.+?)'\s*,\s*InputAction::(?P<action>\w+)\s*\)")
CODE_ITEM = re.compile(r"\(\s*KeyCode::(?P<code>\w+)\s*,\s*InputAction::(?P<action>\w+)\s*\)")
INSERT_CALL = re.compile(r"map\.insert\s*\(")
MODIFIERS = re.compile(r"KeyModifiers::(\w+)")
CHAR_KEY = re.compile(r"KeyCode::Char\(\s*'(?P<char>.+?)'\s*\)")
CHAR_VAR = re.compile(r"KeyCode::Char\(\s*(?P<var>\w+)\s*\)")
CODE_KEY = re.compile(r"KeyCode::(?P<code>\w+)")
BARE_KEY = re.compile(r",\s*(?P<var>\w+)\s*$")
ACTION = re.compile(r"InputAction::(\w+)")
BARE_ACTION = re.compile(
    r"^\s*,?\s*(?P<action>\w+)(?:\s*\.clone\(\s*\))?\s*;?\s*$"
)


@dataclass
class Loop:
    """A `for (key_var, action_var) in [...]` binding loop."""

    kvar: str
    avar: str
    letter_items: dict[str, str]
    code_items: dict[str, str]
    start: int
    end: int


def _line_of(text: str, index: int) -> int:
    return text.count("\n", 0, index) + 1


def _matching_brace(text: str, open_index: int) -> int:
    """Index of the `}` matching the `{` at `open_index`."""
    depth = 0
    for i in range(open_index, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return i
    return -1


def _function_body(text: str, name: str) -> tuple[str, int]:
    """The body of a Rust function, and the line it starts on."""
    start = text.find(f"fn {name}(")
    if start < 0:
        raise ValueError(f"function {name} not found")
    open_brace = text.index("{", start)
    close = _matching_brace(text, open_brace)
    if close < 0:
        raise ValueError(f"function {name} has no closing brace")
    return text[open_brace:close], _line_of(text, start)


def parse_claims(source: str) -> tuple[list[Claim], list[str]]:
    """Parse the claim table out of `src/tui/composer_claims.rs`."""
    claims: list[Claim] = []
    problems: list[str] = []

    const_start = source.find("pub const COMPOSER_CLAIMS")
    if const_start < 0:
        return [], ["COMPOSER_CLAIMS was not found in the claim table"]
    const_end = source.find("];", const_start)
    if const_end < 0:
        return [], ["COMPOSER_CLAIMS has no closing `];`"]
    table = source[const_start:const_end]

    blocks = list(CLAIM_BLOCK.finditer(table))
    if not blocks:
        return [], ["COMPOSER_CLAIMS contains no ComposerClaim rows"]

    for block in blocks:
        fields: dict[str, str] = {}
        for field_match in CLAIM_FIELD.finditer(block.group("body")):
            name = field_match.group("name")
            value = field_match.group("value").strip()
            if name in fields:
                problems.append(
                    f"composer_claims.rs:{_line_of(table, block.start())}: "
                    f"the claim for {fields.get('key', '?')!r} sets {name} twice"
                )
            fields[name] = value
        missing = {"key", "verb", "status", "encodings", "condition"} - set(fields)
        if missing:
            problems.append(
                f"composer_claims.rs:{_line_of(table, block.start())}: "
                f"a claim is missing {', '.join(sorted(missing))}"
            )
            continue
        key = fields["key"].strip("'")
        if len(key) != 1:
            problems.append(
                f"composer_claims.rs:{_line_of(table, block.start())}: "
                f"the claim key {fields['key']} is not a single character"
            )
            continue
        verb = fields["verb"].split("::")[-1]
        encodings = fields["encodings"].split("::")[-1]
        condition = fields["condition"].split("::")[-1]
        status = fields["status"].split("::")[-1]
        if encodings not in ("Bare", "CapitalBoth"):
            problems.append(
                f"composer_claims.rs:{_line_of(table, block.start())}: "
                f"the claim for {key!r} has an unknown encoding {encodings!r}"
            )
            continue
        claims.append(
            Claim(
                key=key,
                verb=verb,
                status=status,
                encodings=encodings,
                condition=condition,
                line_no=_line_of(table, block.start()),
            )
        )
    return claims, problems


def _letter_loops(body: str, base_line: int) -> list[Loop]:
    """The `for (key_var, action_var) in [...]` loops in a function body."""
    loops = []
    for match in FOR_BLOCK.finditer(body):
        open_brace = body.index("{", match.end() - 1)
        close = _matching_brace(body, open_brace)
        if close < 0:
            continue
        items = match.group("items")
        loops.append(
            Loop(
                kvar=match.group("kvar"),
                avar=match.group("avar"),
                letter_items={
                    item.group("char"): item.group("action")
                    for item in LETTER_ITEM.finditer(items)
                },
                code_items={
                    item.group("code"): item.group("action")
                    for item in CODE_ITEM.finditer(items)
                },
                start=match.start(),
                end=close,
            )
        )
    return loops


def _enclosing_loop(
    loops: list[Loop], index: int
) -> Loop | None:
    for loop in loops:
        if loop.start <= index <= loop.end:
            return loop
    return None


def _parse_insert(
    body: str,
    call_start: int,
    base_line: int,
    loops: list[Loop],
    problems: list[str],
) -> list[Binding]:
    """Parse one `map.insert(...)` call into its bindings."""
    open_paren = body.index("(", call_start)
    depth = 0
    close = -1
    for i in range(open_paren, len(body)):
        if body[i] == "(":
            depth += 1
        elif body[i] == ")":
            depth -= 1
            if depth == 0:
                close = i
                break
    if close < 0:
        problems.append(
            f"input.rs:{base_line + _line_of(body, call_start) - 1}: "
            f"a map.insert call has no closing parenthesis"
        )
        return []

    call = body[open_paren:close]
    line_no = base_line + _line_of(body, call_start) - 1

    # The call is `(modifiers, key), action`. Split the key tuple off the
    # action expression, so an action that is a loop variable (`action`) is
    # not confused with a key.
    depth = 0
    tuple_close = -1
    key_open = call.index("(", 1)
    for i in range(key_open, len(call)):
        if call[i] == "(":
            depth += 1
        elif call[i] == ")":
            depth -= 1
            if depth == 0:
                tuple_close = i
                break
    if tuple_close < 0:
        problems.append(f"input.rs:{line_no}: a map.insert call has no key tuple")
        return []
    key_text = call[key_open:tuple_close]
    action_text = call[tuple_close + 1 :]

    modifiers = tuple(m.group(1) for m in MODIFIERS.finditer(key_text))
    if not modifiers:
        problems.append(f"input.rs:{line_no}: a map.insert call has no KeyModifiers")
        return []

    char_match = CHAR_KEY.search(key_text)
    char_var_match = CHAR_VAR.search(key_text)
    code_match = CODE_KEY.search(key_text)
    action_match = ACTION.search(action_text)
    loop = _enclosing_loop(loops, call_start)

    # The action is either a literal or the loop's action variable.
    action = action_match.group(1) if action_match else ""
    if not action:
        bare = BARE_ACTION.search(action_text)
        if bare and loop is not None and bare.group("action") == loop.avar:
            action = ""  # resolved per item below
        else:
            problems.append(f"input.rs:{line_no}: a map.insert call has no InputAction")
            return []

    if char_match is not None:
        if not action:
            problems.append(f"input.rs:{line_no}: a map.insert call has no InputAction")
            return []
        return [
            Binding(
                modifiers=modifiers,
                char=char_match.group("char"),
                action=action,
                line_no=line_no,
            )
        ]

    if char_var_match is not None:
        var = char_var_match.group("var")
        if loop is None or loop.kvar != var or not loop.letter_items:
            problems.append(
                f"input.rs:{line_no}: the insert uses the variable {var!r}, which the "
                f"guard cannot resolve to a set of letters; update the guard"
            )
            return []
        return [
            Binding(
                modifiers=modifiers,
                char=letter,
                action=action or loop.letter_items[letter],
                line_no=line_no,
            )
            for letter in sorted(loop.letter_items)
        ]

    if code_match is not None:
        name = code_match.group("code")
        if loop is not None and loop.kvar == name and loop.code_items:
            return [
                Binding(
                    modifiers=modifiers,
                    char=None,
                    action=action or loop.code_items[code],
                    line_no=line_no,
                    code=code,
                )
                for code in sorted(loop.code_items)
            ]
        if not action:
            problems.append(f"input.rs:{line_no}: a map.insert call has no InputAction")
            return []
        # A non-character key (Enter, Esc, Tab, arrows). The guard only
        # reasons about letters, but it must still know the binding exists so
        # the `is_normal_only` cross-check is not fooled, and G9 needs the key
        # code itself to tell one caret motion from another.
        return [
            Binding(
                modifiers=modifiers,
                char=None,
                action=action,
                line_no=line_no,
                code=name,
            )
        ]

    # A bare identifier as the key: the loop's key variable.
    bare_key = BARE_KEY.search(key_text)
    if bare_key is not None and loop is not None and loop.kvar == bare_key.group("var"):
        if not loop.code_items:
            problems.append(
                f"input.rs:{line_no}: the enclosing loop binds no readable keys; "
                f"update the guard"
            )
            return []
        return [
            Binding(
                modifiers=modifiers,
                char=None,
                action=action or loop.code_items[code],
                line_no=line_no,
                code=code,
            )
            for code in sorted(loop.code_items)
        ]

    problems.append(f"input.rs:{line_no}: a map.insert call has no readable key")
    return []


def parse_map(source: str, function: str, include_shared: bool) -> tuple[list[Binding], list[str]]:
    """Parse one built-in binding map."""
    problems: list[str] = []
    bindings: list[Binding] = []
    body, base_line = _function_body(source, function)
    loops = _letter_loops(body, base_line)
    for call in INSERT_CALL.finditer(body):
        bindings.extend(_parse_insert(body, call.start(), base_line, loops, problems))
    if include_shared:
        shared_body, shared_line = _function_body(source, "insert_panel_direction_bindings")
        shared_loops = _letter_loops(shared_body, shared_line)
        for call in INSERT_CALL.finditer(shared_body):
            bindings.extend(
                _parse_insert(shared_body, call.start(), shared_line, shared_loops, problems)
            )
    return bindings, problems


NORMAL_ONLY_BLOCK = re.compile(
    r"fn\s+is_normal_only\s*\([^)]*\)\s*->\s*bool\s*\{(.*?)\n\}", re.DOTALL
)
NORMAL_ONLY_ENTRY = re.compile(r"InputAction::(\w+)")

# The verb labels, from `ComposerVerb::label`.
#
# The return type is part of the pattern on purpose: `ClaimCondition::label`
# returns an `Option`, and matching it here would read the *condition* labels
# as if they were the verb labels.
VERB_LABEL_BLOCK = re.compile(
    r"pub fn label\(self\) -> &'static str \{(?P<body>.*?)\n    \}", re.DOTALL
)
VERB_LABEL_ARM = re.compile(
    r'Self::(?P<verb>\w+)\s*=>\s*"(?P<label>(?:[^"\\]|\\.)*)"'
)


def parse_normal_only(source: str) -> set[str]:
    """The actions listed in `is_normal_only`."""
    match = NORMAL_ONLY_BLOCK.search(source)
    if not match:
        return set()
    return set(NORMAL_ONLY_ENTRY.findall(match.group(1)))


# ── The help ────────────────────────────────────────────────────────────
#
# The help's **composer section** is derived from the claim table, so it is
# not a literal list in the source and the guard cannot read it there. What
# the guard *can* read is the app's own rows — the literal `HelpEntry`s — and
# those are exactly the rows that could contradict the claim table: a row
# that teaches a claimed key as something else is the class of defect the
# claim table exists to prevent, arriving through the documentation instead
# of the dispatch.

HELP_ENTRY_BLOCK = re.compile(
    r"(?<!struct\s)HelpEntry\s*\{"
    r"(?P<body>(?:[^{}]|\"(?:[^\"\\]|\\.)*\")*)"
    r"\}",
    re.DOTALL,
)
HELP_MODE = re.compile(r"\bmode:\s*HelpMode::(?P<value>\w+)")
HELP_KEY = re.compile(r'\bkey:\s*"(?P<value>(?:[^"\\]|\\.)*)"')
HELP_KEY_FIELD = re.compile(r"\bkey\s*:")
HELP_ACTION = re.compile(r'\baction:\s*"(?P<value>(?:[^"\\]|\\.)*)"')
HELP_CONDITION = re.compile(
    r'\bcondition:\s*(?:None|Some\("(?P<value>(?:[^"\\]|\\.)*)"\))'
)

# A letter that a help row's key text names.
#
# The letter must stand **alone**: a chord's components are not the letters
# the composer owns, so `Ctrl+Alt+X` does not advertise the bare `x`, and
# neither does `Ctrl+D/U` — the modifier governs the whole group it heads.
# The rule is the same shape as the claim table's — a claimed key is a *key*,
# not a character that happens to appear in a label.
BARE_LETTER = re.compile(r"(?<![A-Za-z0-9+])(?P<letter>[A-Za-z])")

# A modifier prefix. A key spelled with one is a chord, and every letter in
# it is a chord component.
MODIFIER_PREFIX = re.compile(r"\b(?:Ctrl|Alt|Shift)\+", re.IGNORECASE)

# The help's spelling of a modifier.
MODIFIER_LABELS = {"CONTROL": "Ctrl", "ALT": "Alt", "SHIFT": "Shift"}


@dataclass(frozen=True)
class HelpEntry:
    """One literal help row."""

    mode: str
    key: str
    action: str
    condition: str | None
    line_no: int = 0

    def named_letters(self) -> set[str]:
        """The standalone letters this row's key names.

        A key spelled with a modifier is a **chord**, and every letter in it
        is a chord component rather than a bare key: `Ctrl+D/U` is two
        chords, and neither of them is the bare `d` the composer claims.
        """
        if MODIFIER_PREFIX.search(self.key):
            return set()
        return {m.group("letter") for m in BARE_LETTER.finditer(self.key)}


def parse_help_entries(source: str) -> list[HelpEntry]:
    """The **literal** help rows in the source.

    A row whose key is not a string literal is a *derived* row — the help's
    composer section computes its keys from the claim table — so it is not an
    app row and the rule does not audit it. The derived rows are correct by
    construction; what the rule needs to see is the app's own rows.
    """
    entries: list[HelpEntry] = []
    for block in HELP_ENTRY_BLOCK.finditer(source):
        body = block.group("body")
        key = HELP_KEY.search(body)
        if key is None:
            if HELP_KEY_FIELD.search(body):
                # A derived row: the key is computed, not written down.
                continue
            entries.append(
                HelpEntry(
                    mode="?",
                    key="",
                    action="",
                    condition=None,
                    line_no=_line_of(source, block.start()),
                )
            )
            continue
        action = HELP_ACTION.search(body)
        mode = HELP_MODE.search(body)
        condition = HELP_CONDITION.search(body)
        entries.append(
            HelpEntry(
                mode=mode.group("value") if mode else "?",
                key=key.group("value"),
                action=action.group("value") if action else "",
                condition=condition.group("value") if condition else None,
                line_no=_line_of(source, block.start()),
            )
        )
    return entries


def chord_label(binding: Binding) -> str:
    """The help's spelling of a chord binding: `Ctrl+Alt+X`."""
    if binding.char is None:
        return ""
    modifiers = "+".join(
        MODIFIER_LABELS.get(modifier, modifier.capitalize())
        for modifier in binding.modifiers
    )
    if not modifiers:
        return binding.char.upper()
    return f"{modifiers}+{binding.char.upper()}"


# The claim table's derived verb list — the function the help's composer
# section is built from. The guard reads its **name** out of the source, so
# the two cannot drift: renaming the function breaks the check that the help
# uses it, and the check names the function it looked for.
DERIVATION_CALL = re.compile(r"\b(?P<fn>\w*verb_entries)\s*\(")


def parse_verb_labels(source: str) -> dict[str, str]:
    """The verb labels, from `ComposerVerb::label`."""
    match = VERB_LABEL_BLOCK.search(source)
    if not match:
        return {}
    return {
        arm.group("verb"): arm.group("label")
        for arm in VERB_LABEL_ARM.finditer(match.group("body"))
    }


def parse_tree() -> Parsed:
    parsed = Parsed()
    claims_source = CLAIMS_SRC.read_text()
    input_source = INPUT_SRC.read_text()

    parsed.claims, problems = parse_claims(claims_source)
    parsed.problems.extend(problems)

    parsed.default_map, problems = parse_map(input_source, "default_bindings_internal", True)
    parsed.problems.extend(problems)
    parsed.vim_map, problems = parse_map(input_source, "vim_bindings_internal", True)
    parsed.problems.extend(problems)
    parsed.normal_only = parse_normal_only(input_source)
    parsed.help_entries = parse_help_entries(input_source)
    parsed.verb_labels = parse_verb_labels(claims_source)
    # The help's composer section is **derived**: the rows are computed from
    # the claim table at runtime, so the source carries the *call* rather than
    # the rows. The call is what the guard checks.
    derivation = DERIVATION_CALL.search(claims_source)
    parsed.help_is_derived = derivation is not None and (
        f"{derivation.group('fn')}(" in input_source
    )
    return parsed


# ── The checks ─────────────────────────────────────────────────────────


def _bindings_for(
    bindings: list[Binding], char: str
) -> list[Binding]:
    return [b for b in bindings if b.char == char]


def check(parsed: Parsed) -> list[str]:
    """Run every check and return the violations, one message each."""
    findings: list[str] = list(parsed.problems)
    claims = parsed.claims
    maps = {"default": parsed.default_map, "vim": parsed.vim_map}
    all_bindings = parsed.default_map + parsed.vim_map

    if not claims:
        findings.append("the claim table is empty; the guard would police nothing")
        return findings

    # G1 — the table is closed and single-owner.
    seen: dict[str, Claim] = {}
    for claim in claims:
        if claim.key in seen:
            findings.append(
                f"the letter {claim.key!r} is claimed twice "
                f"(composer_claims.rs:{seen[claim.key].line_no} and "
                f"composer_claims.rs:{claim.line_no}): one key, one owner"
            )
        else:
            seen[claim.key] = claim
        if not claim.verb:
            findings.append(
                f"composer_claims.rs:{claim.line_no}: the claim for {claim.key!r} "
                f"names no verb"
            )

    # The (modifiers, char) pairs the table claims. `Claim.claimed_keys`
    # already yields the whole pair, so it is taken as-is: unpacking it here
    # and re-pairing it with the key would nest the tuple one level deeper
    # than the bindings use, and the comparison would never match — which is
    # exactly the kind of check that looks armed and is not.
    claimed_key_set = {pair for claim in claims for pair in claim.claimed_keys}

    # G2 — the fallthrough rule: a claimed letter must not reach an app-level
    # action. A *conditional* claim is the one documented exception, and G3
    # checks that the condition is written down.
    for claim in claims:
        if claim.is_conditional:
            continue
        for modifiers, char in claim.claimed_keys:
            for map_name, bindings in maps.items():
                for binding in _bindings_for(bindings, char):
                    if binding.modifiers == modifiers:
                        findings.append(
                            f"the claimed letter {char!r} can still reach "
                            f"{binding.action} in the {map_name} map "
                            f"(input.rs:{binding.line_no}): a claimed key must be "
                            f"consumed by the composer and must not fall through"
                        )

    # G3 — a conditional claim must use a condition the guard knows. A claim
    # whose key is still owned by an app-level action needs its condition
    # written down (G2 catches the case where it is not), and a condition
    # the guard cannot evaluate is an implicit condition in all but name.
    for claim in claims:
        if claim.condition not in KNOWN_CONDITIONS:
            findings.append(
                f"the claim for {claim.key!r} (composer_claims.rs:{claim.line_no}) "
                f"has the condition {claim.condition!r}, which the guard does not "
                f"know; a condition that is not written down is how a leak "
                f"reappears"
            )

    # G4 — a re-spelled action is bound in both maps.
    for action in RESPELLED_ACTIONS:
        for map_name, bindings in maps.items():
            if not [b for b in bindings if b.action == action]:
                findings.append(
                    f"the re-spelled action {action} is missing from the "
                    f"{map_name} map; the capability must stay reachable in both maps"
                )

    # G5 — a re-spelled action is reachable by a chord in both maps, and none
    # of its spellings is a key the composer can claim.
    #
    # The rule is "not a *claimed* key", not "not a bare character". The three
    # re-spelled actions' old letters — `x`, `d` and `i` — are all claimed by
    # the composer, so the claim rule already forbids their bare spellings;
    # a bare spelling of a letter the composer does **not** claim is a
    # different thing, and the plan's invariant ("a letter the composer does
    # not claim must resolve to the same app action it resolves to today")
    # requires it to keep working. The default map's `/` for focus-prompt is
    # the live example: it is the gesture that enters command mode, and
    # command mode is what makes the rest of a typed slash command land.
    for action, old_letter in RESPELLED_ACTIONS.items():
        for map_name, bindings in maps.items():
            for binding in [b for b in bindings if b.action == action]:
                if (binding.modifiers, binding.char or "") in claimed_key_set:
                    findings.append(
                        f"{action} is bound to {binding.char!r} under "
                        f"{'+'.join(binding.modifiers)} in the {map_name} map "
                        f"(input.rs:{binding.line_no}), which is a key the composer "
                        f"claims; the composer would take the chord back"
                    )
            # The action must be reachable by a chord in this map: a bare
            # letter alone would not be a re-spelling.
            if not [b for b in bindings if b.action == action and not b.is_bare_char]:
                findings.append(
                    f"the re-spelled action {action} has no non-bare spelling in "
                    f"the {map_name} map; the capability must be reachable by a "
                    f"chord that is not a bare letter"
                )
        # The old letter must not reach it either.
        for map_name, bindings in maps.items():
            for binding in [b for b in bindings if b.action == action]:
                if binding.char == old_letter and binding.is_bare_char:
                    findings.append(
                        f"the old letter {old_letter!r} still reaches {action} in "
                        f"the {map_name} map (input.rs:{binding.line_no})"
                    )

    # G6 — a claimed capital letter answers to both encodings.
    for claim in claims:
        if claim.key.isupper():
            if claim.encodings != "CapitalBoth":
                findings.append(
                    f"the claim for the capital letter {claim.key!r} "
                    f"(composer_claims.rs:{claim.line_no}) is not reachable under "
                    f"both the shifted and the unshifted encoding; a terminal that "
                    f"reports a shifted key without the SHIFT modifier would not "
                    f"reach the verb"
                )
        elif claim.encodings != "Bare":
            findings.append(
                f"the claim for {claim.key!r} "
                f"(composer_claims.rs:{claim.line_no}) accepts a shifted encoding; "
                f"the shifted spelling of a lowercase letter is a different "
                f"character and is not claimed"
            )

    # G7 — an action whose every spelling is a modifier chord must not be
    # listed in `is_normal_only`: such a chord cannot be composed as text, so
    # it is already unambiguous in Insert mode, and listing it would take the
    # action out of the mode the app boots in.
    for action in parsed.normal_only:
        spellings = [b for b in all_bindings if b.action == action]
        if spellings and all(
            b.modifiers != ("NONE",) and not b.is_bare_char for b in spellings
        ):
            findings.append(
                f"{action} is listed in `is_normal_only` but every one of its "
                f"spellings is a modifier chord; a chord is already unambiguous "
                f"in Insert mode and must not be gated out of it"
            )

    # G8 — a claimed key bound under a modifier Insert mode does not skip
    # must have its action listed in `is_normal_only`, or the key stops being
    # ordinary text while composing.
    for claim in claims:
        for modifiers, char in claim.claimed_keys:
            if modifiers == ("NONE",):
                # A bare character is skipped by the bare-character rule.
                continue
            for map_name, bindings in maps.items():
                for binding in _bindings_for(bindings, char):
                    if binding.modifiers != modifiers:
                        continue
                    if binding.action not in parsed.normal_only:
                        findings.append(
                            f"the claimed key {char!r} under "
                            f"{'+'.join(modifiers)} reaches {binding.action} in the "
                            f"{map_name} map (input.rs:{binding.line_no}), and that "
                            f"action is not listed in `is_normal_only`; a shifted "
                            f"letter must stay ordinary text while composing"
                        )

    # G9 — the two maps must agree on caret motions.
    #
    # The check is symmetric: a key bound to a caret motion in one map must
    # be bound to the **same** action in the other. A caret motion that
    # exists in only one configuration is a capability the user has or does
    # not have depending on which keymap they picked, which is the defect
    # this rule exists to prevent — the composer's `h`/`l` were bound in the
    # vim map only, and the composer claim table relied on that binding
    # being in every map.
    #
    # The rule is derived from the maps, not from a list of letters, so it
    # holds for every caret motion and not only for the two this milestone
    # happened to find.
    for map_name, bindings in maps.items():
        other_name = "vim" if map_name == "default" else "default"
        other_bindings = maps[other_name]
        for binding in [b for b in bindings if b.action in CARET_MOTION_ACTIONS]:
            key_text = (
                repr(binding.char)
                if binding.char is not None
                else f"KeyCode::{binding.code}"
            )
            matches = [b for b in other_bindings if b.key_id == binding.key_id]
            if not matches:
                findings.append(
                    f"the {map_name} map binds {key_text} under "
                    f"{'+'.join(binding.modifiers)} to {binding.action} "
                    f"(input.rs:{binding.line_no}), and the {other_name} map does "
                    f"not bind that key at all; a caret motion is a Helix-parity "
                    f"capability, not a keymap flavour, and the composer must not "
                    f"lose it in one configuration"
                )
            elif matches[0].action != binding.action:
                findings.append(
                    f"the {map_name} map binds {key_text} under "
                    f"{'+'.join(binding.modifiers)} to {binding.action} "
                    f"(input.rs:{binding.line_no}), and the {other_name} map binds "
                    f"the same key to {matches[0].action}; the two maps must agree "
                    f"on a caret motion"
                )

    # G10 and G11 — the help against the claim table.
    findings.extend(check_help(parsed))

    return findings


def check_help(parsed: Parsed) -> list[str]:
    """G10 and G11 — the help against the claim table.

    The help's **composer section** is derived from the claim table, so it is
    not a literal list in the source and this rule cannot read its rows there.
    What it *can* read is the app's own rows, and those are exactly the rows
    that could contradict the claim table.
    """
    findings: list[str] = []
    claims = parsed.claims
    live = [claim for claim in claims if claim.status == "Live"]
    reserved = [claim for claim in claims if claim.status != "Live"]
    claimed_by_key: dict[str, Claim] = {claim.key: claim for claim in claims}

    # G10.1 — the help's composer section is built from the claim table's
    # derived verb list.
    #
    # The rows are computed at runtime, so the source carries the *call*
    # rather than the rows. This is the one part of the check that cannot be
    # an observation: it proves the derivation is wired, and the derivation is
    # what makes the rows complete.
    if not parsed.help_is_derived:
        findings.append(
            "the help's composer section is not built from the claim table's derived "
            "verb list; a hand-written list is a second copy of the table, and a "
            "second copy is the drift the claim table exists to prevent"
        )

    # G10.2 — every live claim's verb has a label, so the derived row can
    # describe it. A verb with no label is a claim the help cannot say
    # anything about.
    for claim in live:
        label = parsed.verb_labels.get(claim.verb)
        if label is None:
            findings.append(
                f"the verb {claim.verb} (composer_claims.rs:{claim.line_no}) has no "
                f"label; the help could not describe the claim on {claim.key!r}"
            )
        elif not label.strip():
            findings.append(
                f"the verb {claim.verb} (composer_claims.rs:{claim.line_no}) has an "
                f"empty label; the help could not describe the claim on {claim.key!r}"
            )

    # G10.3 — no app-level row may advertise a **claimed** key for a
    # different action.
    #
    # The rule is scoped to the **Normal-mode** section, because that is the
    # surface the composer's claims live on: the composer owns those letters
    # in the main buffer's Normal mode, and the help's Normal section is that
    # surface's keymap. The Dialog and Command sections describe *other*
    # surfaces — the command palette's own keys — where the same letters mean
    # something else and are not the composer's to own.
    #
    # The one documented exception is a *conditional* claim's fall-through
    # row: the `j`/`k` row says what the pair does on a single-line composer
    # buffer, where the composer does not have them. Such a row carries the
    # condition, and the condition is what makes it honest rather than
    # contradictory.
    #
    # The `a` → "Focus sidebar" row is the class this catches: the key was
    # claimed by the composer while the help still taught it as the sidebar's.
    for entry in parsed.help_entries:
        if not entry.key:
            findings.append(
                f"input.rs:{entry.line_no}: a help row has no readable key; the "
                f"guard cannot audit it"
            )
            continue
        if entry.mode != "Normal":
            continue
        for letter in sorted(entry.named_letters()):
            claim = claimed_by_key.get(letter)
            if claim is None:
                continue
            if claim.status != "Live":
                continue
            if claim.is_conditional and entry.condition:
                # The fall-through row for a conditional claim.
                continue
            findings.append(
                f"the help advertises {letter!r} for {entry.action!r} "
                f"(input.rs:{entry.line_no}), but the composer claims {letter!r} "
                f"(composer_claims.rs:{claim.line_no}); a claimed key has one owner"
            )

    # G10.4 — a **reserved** claim is not advertised. The key is consumed
    # while its verb is pending, so a row teaching it would teach a gesture
    # that does nothing.
    for claim in reserved:
        for entry in parsed.help_entries:
            if entry.mode != "Normal":
                continue
            if claim.key in entry.named_letters():
                findings.append(
                    f"the help advertises {claim.key!r} for {entry.action!r} "
                    f"(input.rs:{entry.line_no}), but that claim is not live "
                    f"(composer_claims.rs:{claim.line_no}); a reserved claim has "
                    f"no behaviour to advertise"
                )

    # G11 — the re-spelled actions.
    #
    # G11.1: each one must be documented under a **modifier chord**. A chord
    # carries a modifier, so it cannot be confused with the bare letter the
    # composer now owns.
    #
    # G11.2: none of them may be documented under its **old bare letter**.
    # That is the pre-M002 spelling, and it is a gesture that no longer
    # reaches the action.
    for action, old_letter in RESPELLED_ACTIONS.items():
        chords = {
            chord_label(binding)
            for binding in parsed.default_map + parsed.vim_map
            if binding.action == action and not binding.is_bare_char and binding.char
        }
        documented = {
            chord
            for chord in chords
            if any(chord in entry.key for entry in parsed.help_entries)
        }
        if not documented:
            findings.append(
                f"the re-spelled action {action} is not documented under a modifier "
                f"chord in the help; the chords it is bound to are "
                f"{sorted(chords) or ['none']}"
            )
        for entry in parsed.help_entries:
            if entry.mode != "Normal":
                continue
            if old_letter in entry.named_letters():
                findings.append(
                    f"the help advertises {action} under its old bare letter "
                    f"{old_letter!r} ({entry.action!r}, input.rs:{entry.line_no}); "
                    f"the action moved to a chord and the bare letter belongs to "
                    f"the composer"
                )

    return findings


def _claim(key: str, **overrides) -> Claim:
    base = {
        "key": key,
        "verb": "SomeVerb",
        "status": "Reserved",
        "encodings": "Bare",
        "condition": "Always",
        "line_no": 1,
    }
    base.update(overrides)
    return Claim(**base)  # type: ignore[arg-type]


def _binding(
    mods: tuple[str, ...], char: str | None, action: str, code: str | None = None
) -> Binding:
    return Binding(modifiers=mods, char=char, action=action, line_no=1, code=code)


def _chords() -> list[Binding]:
    """The chord spelling every re-spelled action must have.

    Each re-spelled action is reachable by a `Ctrl+Alt+<letter>` chord in both
    maps. The letters themselves are the ones the composer claims, so a
    synthetic tree that bound them bare would be violating the rule it is
    meant to test.
    """
    return [
        _binding(("CONTROL", "ALT"), "x", "ToggleAgentRun"),
        _binding(("CONTROL", "ALT"), "d", "OpenDiff"),
        _binding(("CONTROL", "ALT"), "e", "FocusPrompt"),
        _binding(("CONTROL", "ALT"), "s", "FocusSidebar"),
    ]


def _baseline(**overrides) -> Parsed:
    """A synthetic tree that satisfies every check.

    The re-spelled actions are bound in both maps by chords, so a test case
    that injects one violation is not drowned in the others. The synthetic
    help carries the matching chord rows, because the help audit reads the
    app's own rows: a chord row is the only spelling a re-spelled action may
    keep.
    """
    chords = _chords()
    parsed = Parsed(
        claims=[_claim("q", verb="SomeVerb", status="Live")],
        default_map=list(chords),
        vim_map=list(chords),
        normal_only=set(),
        help_entries=[
            HelpEntry(
                mode="Normal",
                key=chord_label(chord),
                action=f"the {chord.action} action",
                condition=None,
                line_no=1,
            )
            for chord in chords
        ],
        verb_labels={"SomeVerb": "the verb"},
        help_is_derived=True,
    )
    for name, value in overrides.items():
        setattr(parsed, name, value)
    return parsed


def self_test() -> int:
    """Prove each violation class is rejected.

    A guard that cannot be shown to fail is not evidence of anything. Each
    case is a synthetic tree fed to the same `check` the real tree uses.
    """
    failures = 0

    def expect(label: str, parsed: Parsed, needle: str) -> None:
        nonlocal failures
        findings = check(parsed)
        if any(needle in f for f in findings):
            print(f"  ok    detects {label}")
        else:
            print(f"  FAIL  did not detect {label} (looked for {needle!r})")
            print(f"        findings were: {findings}")
            failures += 1

    def expect_clean(label: str, parsed: Parsed) -> None:
        nonlocal failures
        findings = check(parsed)
        if findings:
            print(f"  FAIL  false positive on {label}: {findings}")
            failures += 1
        else:
            print(f"  ok    allows {label}")

    expect_clean("a claim nothing else owns", _baseline())

    def _chord_rows() -> list[HelpEntry]:
        """The chord rows a re-spelled action is documented under."""
        return [
            HelpEntry(
                mode="Normal",
                key=chord_label(chord),
                action=f"the {chord.action} action",
                condition=None,
                line_no=1,
            )
            for chord in _chords()
        ]

    # G10.1 — a help whose composer section is hand-written.
    #
    # This is the class the milestone exists to remove: a second copy of the
    # claim table, which drifts the moment a verb lands.
    expect(
        "a help whose composer section is not derived from the claim table",
        _baseline(help_is_derived=False),
        "is not built from the claim table's derived verb list",
    )

    # G10.2 — a live verb with no label.
    expect(
        "a live verb with no label",
        _baseline(verb_labels={}),
        "has no label",
    )

    # G10.3 — a help that advertises a claimed key for a different action.
    #
    # The class M006 hit with the `a` → "Focus sidebar" row: the key was
    # claimed by the composer while the help still taught it as the
    # sidebar's.
    expect(
        "a help that advertises a claimed key for a different action",
        _baseline(
            claims=[_claim("a", verb="InsertAfter", status="Live")],
            help_entries=[
                HelpEntry(
                    mode="Normal",
                    key="a",
                    action="Focus sidebar (agent tree)",
                    condition=None,
                    line_no=1,
                )
            ],
        ),
        "a claimed key has one owner",
    )

    # G10.3 — the fall-through row for a **conditional** claim is the one
    # documented exception, and it must carry its condition.
    expect_clean(
        "a conditional claim's fall-through row",
        _baseline(
            claims=[_claim("j", verb="LineDown", status="Live", condition="MultiLineBuffer")],
            verb_labels={"LineDown": "line down"},
            help_entries=_chord_rows()
            + [
                HelpEntry(
                    mode="Normal",
                    key="j/k",
                    action="Navigate up/down",
                    condition="single-line composer buffer",
                    line_no=1,
                )
            ],
        ),
    )

    # G10.4 — a reserved claim advertised.
    expect(
        "a help that advertises a reserved claim",
        _baseline(
            claims=[_claim("z", verb="SomeVerb", status="Reserved")],
            help_entries=[
                HelpEntry(
                    mode="Normal",
                    key="z",
                    action="a verb that does nothing",
                    condition=None,
                    line_no=1,
                )
            ],
        ),
        "a reserved claim has no behaviour to advertise",
    )

    # G11 — the re-spelled actions.
    #
    # A help that teaches the old bare letter is the pre-M002 spelling, and a
    # gesture that no longer reaches the action.
    expect(
        "a help that lists `x` as the stop key",
        _baseline(
            help_entries=_chord_rows()
            + [
                HelpEntry(
                    mode="Normal",
                    key="x",
                    action="Stop / resume agent",
                    condition=None,
                    line_no=1,
                )
            ]
        ),
        "its old bare letter",
    )
    expect_clean(
        "a help that lists the stop key only as a chord",
        _baseline(help_entries=_chord_rows()),
    )
    expect(
        "a help that omits a re-spelled action's chord",
        _baseline(help_entries=[]),
        "is not documented under a modifier chord",
    )

    # G1 — a letter claimed twice.
    expect(
        "a letter claimed by two owners",
        _baseline(
            claims=[
                _claim("d", verb="DeleteSelection"),
                _claim("d", verb="Paste"),
            ]
        ),
        "claimed twice",
    )

    # G2 — the fallthrough violation: a claimed letter still owned by an
    # app-level action.
    expect(
        "a claimed letter that can reach an app-level action",
        _baseline(
            claims=[_claim("d", verb="DeleteSelection")],
            vim_map=[
                _binding(("NONE",), "d", "OpenDiff"),
                _binding(("CONTROL", "ALT"), "d", "OpenDiff"),
            ],
        ),
        "must not fall through",
    )

    # G3 — a condition the guard does not know.
    expect(
        "a claim with a condition the guard does not know",
        _baseline(claims=[_claim("j", verb="LineDown", condition="SomeOtherRule")]),
        "which the guard does not know",
    )

    # G4 — a re-spelled action missing from a map.
    expect(
        "a re-spelled action missing from the vim map",
        _baseline(vim_map=[]),
        "missing from the vim map",
    )

    # G4 — the `FocusSidebar` half of the M006 re-spelling: the action has to
    # stay reachable in **both** maps. Deleting the bare `a` must not take
    # the sidebar's chord with it.
    expect(
        "a FocusSidebar missing from one map",
        _baseline(
            default_map=_chords(),
            vim_map=[b for b in _chords() if b.action != "FocusSidebar"],
        ),
        "missing from the vim map",
    )

    # G2 — the claim on `a` and the vim map's bare `a` → `FocusSidebar`. This is
    # the exact defect M006 fixed: the composer claimed the letter and the
    # app-level action still owned it, so the key had two owners.
    expect(
        "a claimed `a` still reaching FocusSidebar in the vim map",
        _baseline(
            claims=[_claim("a", verb="InsertAfter", status="Live")],
            vim_map=_chords() + [_binding(("NONE",), "a", "FocusSidebar")],
        ),
        "must not fall through",
    )

    # G5 — a re-spelled action may not keep a **bare letter** spelling. The
    # re-spelling is the point: the bare letter belongs to the composer now,
    # so an app-level action that still answers to it has two owners.
    expect(
        "a FocusSidebar spelling that is a bare letter",
        _baseline(default_map=_chords() + [_binding(("NONE",), "a", "FocusSidebar")]),
        "the old letter 'a' still reaches FocusSidebar",
    )

    # G5 — the old bare letter still reaching the re-spelled action.
    expect(
        "the old bare letter still reaching a re-spelled action",
        _baseline(
            default_map=[
                _binding(("NONE",), "x", "ToggleAgentRun"),
                _binding(("CONTROL", "ALT"), "x", "ToggleAgentRun"),
            ],
        ),
        "the old letter 'x' still reaches ToggleAgentRun",
    )

    # G5 — a re-spelled action bound to a key the composer claims.
    expect(
        "a re-spelled chord that is a claimed key",
        _baseline(
            claims=[_claim("x", verb="SelectLine")],
            default_map=[
                _binding(("NONE",), "x", "ToggleAgentRun"),
                _binding(("CONTROL", "ALT"), "x", "ToggleAgentRun"),
            ],
        ),
        "which is a key the composer claims",
    )

    # G5 — a re-spelled action with no chord to reach it by.
    expect(
        "a re-spelled action with no non-bare spelling",
        _baseline(
            default_map=[_binding(("NONE",), "z", "ToggleAgentRun")],
        ),
        "no non-bare spelling",
    )

    # G6 — a capital claim that is not reachable under both encodings.
    expect(
        "a capital claim that misses an encoding",
        _baseline(claims=[_claim("U", verb="Redo", encodings="Bare")]),
        "not reachable under both",
    )

    # G7 — a chord-only action gated out of Insert mode.
    expect(
        "a chord-only action listed in is_normal_only",
        _baseline(
            default_map=[
                _binding(("CONTROL", "ALT"), "q", "SomeAction"),
                _binding(("CONTROL", "ALT"), "x", "ToggleAgentRun"),
                _binding(("CONTROL", "ALT"), "d", "OpenDiff"),
                _binding(("CONTROL", "ALT"), "e", "FocusPrompt"),
            ],
            vim_map=[
                _binding(("CONTROL", "ALT"), "x", "ToggleAgentRun"),
                _binding(("CONTROL", "ALT"), "d", "OpenDiff"),
                _binding(("CONTROL", "ALT"), "e", "FocusPrompt"),
            ],
            normal_only={"SomeAction"},
        ),
        "already unambiguous in Insert mode",
    )

    # G8 — a claimed key bound under SHIFT without a mode gate.
    expect(
        "a claimed key bound under SHIFT with no is_normal_only entry",
        _baseline(
            claims=[_claim("U", verb="Redo", encodings="CapitalBoth")],
            default_map=[
                _binding(("SHIFT",), "U", "SomeAction"),
                _binding(("CONTROL", "ALT"), "x", "ToggleAgentRun"),
                _binding(("CONTROL", "ALT"), "d", "OpenDiff"),
                _binding(("CONTROL", "ALT"), "e", "FocusPrompt"),
            ],
        ),
        "ordinary text while composing",
    )

    # G9 — a caret motion that exists in only one map. This is the exact
    # shape of the defect this rule was written for: `h` and `l` moved the
    # composer caret in the vim map and in no other map.
    expect(
        "a caret motion bound in the vim map only",
        _baseline(
            default_map=[
                _binding(("NONE",), "x", "ToggleAgentRun"),
                _binding(("CONTROL", "ALT"), "d", "OpenDiff"),
                _binding(("CONTROL", "ALT"), "e", "FocusPrompt"),
            ],
            vim_map=[
                _binding(("NONE",), "h", "Left"),
                _binding(("NONE",), "x", "ToggleAgentRun"),
                _binding(("CONTROL", "ALT"), "d", "OpenDiff"),
                _binding(("CONTROL", "ALT"), "e", "FocusPrompt"),
            ],
        ),
        "does not bind that key at all",
    )

    # G9 — the same key bound to a different action in the two maps.
    expect(
        "the same caret key bound to a different action in the two maps",
        _baseline(
            default_map=[
                _binding(("NONE",), "h", "Left"),
                _binding(("NONE",), "x", "ToggleAgentRun"),
                _binding(("CONTROL", "ALT"), "d", "OpenDiff"),
                _binding(("CONTROL", "ALT"), "e", "FocusPrompt"),
            ],
            vim_map=[
                _binding(("NONE",), "h", "Right"),
                _binding(("NONE",), "x", "ToggleAgentRun"),
                _binding(("CONTROL", "ALT"), "d", "OpenDiff"),
                _binding(("CONTROL", "ALT"), "e", "FocusPrompt"),
            ],
        ),
        "the two maps must agree",
    )

    # G9 — a non-character caret key (the arrow) is compared by its key code,
    # not by its action alone: a map that kept `Home` and lost `Left` must
    # not look identical to one that kept both.
    expect(
        "an arrow caret motion bound in the vim map only",
        _baseline(
            default_map=[
                _binding(("NONE",), None, "Home", code="Home"),
                _binding(("NONE",), "x", "ToggleAgentRun"),
                _binding(("CONTROL", "ALT"), "d", "OpenDiff"),
                _binding(("CONTROL", "ALT"), "e", "FocusPrompt"),
            ],
            vim_map=[
                _binding(("NONE",), None, "Home", code="Home"),
                _binding(("NONE",), None, "Left", code="Left"),
                _binding(("NONE",), "x", "ToggleAgentRun"),
                _binding(("CONTROL", "ALT"), "d", "OpenDiff"),
                _binding(("CONTROL", "ALT"), "e", "FocusPrompt"),
            ],
        ),
        "does not bind that key at all",
    )

    # The real tree must be clean.
    real = parse_tree()
    expect_clean("the delivered tree", real)

    print()
    if failures:
        print(f"self-test FAILED: {failures} case(s) did not behave as required")
        return 1
    print("self-test passed: every violation class was detected and rejected")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard fails on injected violations and passes on the tree",
    )
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    if not CLAIMS_SRC.is_file() or not INPUT_SRC.is_file():
        print(
            f"claim table not found: {CLAIMS_SRC} / {INPUT_SRC}",
            file=sys.stderr,
        )
        return 1

    findings = check(parse_tree())
    if findings:
        print(
            "The composer's claim table does not own its keys.\n"
            "\n"
            "A claimed letter belongs to the composer. It is consumed even while\n"
            "its verb is still reserved, because a reserved claim that falls\n"
            "through becomes an unrelated app action — the failure the terminal\n"
            "panel already records for an unclaimed `d`.\n"
            "\n"
        )
        for finding in findings:
            print(f"  {finding}")
        print(f"\n{len(findings)} violation(s) found.")
        return 1

    print(
        "Composer key-ownership guard passed — the claim table is closed, "
        "single-owner, leaks no key to an app-level action, and every caret "
        "motion is bound the same way in both maps"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
