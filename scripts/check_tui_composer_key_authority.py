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
first whose verbs arrive later than its claims — every entry in the table but
one is reserved today, so every one of those entries is currently a no-op that
must still swallow its key. The exception is ``i``, whose verb is live: it is
the app's only route back to Insert mode, so a reserved ``i`` would swallow
the key and leave the user with no way to type.

The guard reads two files:

* ``src/tui/composer_claims.rs`` — the claim table itself. It is the single
  source of truth for what the composer owns; there is deliberately no second
  copy of it anywhere, so this file is the only place the table is read.
* ``src/tui/input.rs`` — the two binding maps and ``is_normal_only``.

It fails when:

* a claim is malformed, or two claims share a letter;
* a claimed letter can still reach an app-level action (the fallthrough
  violation);
* a claimed letter that *is* bound in the app maps is not a conditional
  claim, so the condition that saves it is not written down;
* a re-spelled app-level chord is missing from either binding map;
* a re-spelled app-level action has no chord to reach it by, or still answers
  to a key the composer can claim — which is how the old letter keeps reaching
  it, since all three old letters are claimed;
* a claimed capital letter is not reachable under both the shifted and the
  unshifted encoding;
* a claimed key is bound under a modifier that Insert mode does not skip,
  without its action being listed in ``is_normal_only`` — the capital-letter
  text guarantee.

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
}

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

    @property
    def is_bare_char(self) -> bool:
        """Whether Insert mode skips this binding because it is bare text."""
        return self.modifiers == ("NONE",) and self.char is not None


@dataclass
class Parsed:
    claims: list[Claim] = field(default_factory=list)
    default_map: list[Binding] = field(default_factory=list)
    vim_map: list[Binding] = field(default_factory=list)
    normal_only: set[str] = field(default_factory=set)
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
                )
                for code in sorted(loop.code_items)
            ]
        if not action:
            problems.append(f"input.rs:{line_no}: a map.insert call has no InputAction")
            return []
        # A non-character key (Enter, Esc, Tab, arrows). The guard only
        # reasons about letters, but it must still know the binding exists so
        # the `is_normal_only` cross-check is not fooled.
        return [Binding(modifiers=modifiers, char=None, action=action, line_no=line_no)]

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


def parse_normal_only(source: str) -> set[str]:
    """The actions listed in `is_normal_only`."""
    match = NORMAL_ONLY_BLOCK.search(source)
    if not match:
        return set()
    return set(NORMAL_ONLY_ENTRY.findall(match.group(1)))


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

    return findings


# ── Self-test ──────────────────────────────────────────────────────────


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


def _binding(mods: tuple[str, ...], char: str | None, action: str) -> Binding:
    return Binding(modifiers=mods, char=char, action=action, line_no=1)


def _baseline(**overrides) -> Parsed:
    """A synthetic tree that satisfies every check.

    The re-spelled actions are bound in both maps by chords, so a test case
    that injects one violation is not drowned in the others.
    """
    chords = [
        _binding(("CONTROL", "ALT"), "x", "ToggleAgentRun"),
        _binding(("CONTROL", "ALT"), "d", "OpenDiff"),
        _binding(("CONTROL", "ALT"), "e", "FocusPrompt"),
    ]
    parsed = Parsed(
        claims=[_claim("q", verb="WordForward")],
        default_map=list(chords),
        vim_map=list(chords),
        normal_only=set(),
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
        "single-owner, and leaks no key to an app-level action"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
