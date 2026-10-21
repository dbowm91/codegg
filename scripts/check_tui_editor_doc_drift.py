#!/usr/bin/env python3
"""Static guard: the editor's bindings and `docs/tui.md` describe one set.

Milestone M020 of `plans/implementation/tui-editor-helix-parity/` serves the
roadmap's §11 completion definition, whose binding bullet reads:

> The normal-mode keymap, the `:` palette, the help overlay, and `docs/tui.md`
> describe one identical set of bindings, and that set is derived from the
> binding table rather than hand-maintained beside it.

That sentence names three surfaces, and each name is wrong:

* the **help overlay** it names — `src/tui/components/help_overlay.rs` — is
  unreferenced dead code, with no `mod` declaration and no reference anywhere
  in `src/`. A dead file describes nothing;
* the **`:` palette** is `EditorCommandName::ALL`, a *command registry*
  (`w`, `q`, `wq`, `q!`, `e`, `wa`, `new`, `open`, `complete`), not the keymap.
  It was never the same set;
* **`docs/tui.md`** is hand-maintained prose, and nothing in the repository
  compared it to the code. The bullet therefore could not be true — and could
  not be *false* either, because no check existed to say so.

The roadmap's Amendment A4 narrows the bullet to the surfaces that exist and
adds this guard to make the narrowed bullet checkable.

## What this guard reads

Three inputs, and no more:

* `src/tui/commands/editor.rs` — through
  `scripts/check_tui_editor_key_authority.py`, for the claimed Normal-mode
  chords, the Insert-mode chords and the guard block. The completion guard
  already reads that guard's *model* rather than its output, and this guard
  does the same: one analysis, no second parser.
* `src/tui/editor.rs` — through
  `scripts/check_tui_editor_leaf_authority.py`, for the five leaf tables and
  the prefix each one is bound to.
* `docs/tui.md` — the document under test.

## What counts as a violation

**Violations** (exit code 1):

- **Rule 1 — a leaf gesture the code binds and the document never mentions.**
  The code→document direction: a gesture nobody can read about.
- **Rule 2 — a chord the code claims and the document never mentions.** The
  same direction, for the keymap itself.
- **Rule 3 — a key the document declares unbound that the code binds.** The
  document→code direction, and the one that matters most: a document that
  says a key does not work is worse than one that omits it.
- **Rule 4 — a document with no unbound declaration at all.** A guard that
  finds nothing because it looked in the wrong place reports a clean tree and
  means nothing, so the guard fails when the structure it checks is absent.
- **Rule 5 — a token in an unbound list that is not a recognisable binding
  shape.** A list the guard cannot read is a list it cannot check.

## The unbound marker

Rule 3 needs the document to say *which* keys it declares unbound. The two
lists that already exist in `docs/tui.md` gain a marker, which costs a reader
nothing because it is an HTML comment:

```markdown
<!-- unbound: the keys below are deliberately not bound -->
- `gf`, `ga`, `gm`, `gn`, `gp` — files and alternate buffers. …
```

The marker makes the claim checkable in **both** directions, and the
document→code direction is the valuable one: a key that gets implemented and
stays in the list is caught.

## The spellings the document uses

A guard that demanded one spelling of every key would be a style guard, and it
would fail on the document's own inconsistency: `docs/tui.md` writes `Alt+d` in
the Insert-mode section and `Alt-d` in the multi-selection section, and spells
the minus key `Alt-minus`. The guard accepts the spellings the document
actually uses, and says so here rather than in a comment the next editor
would not read.

## What the guard deliberately does not check

The document's prose contains bracketed identifiers (`[INS]`, `[NOR]`,
`[human_shell]`), a JSON path (`[theme].name`), a *negative* example
(`m z` is not a gesture), and the match-mode gestures that take a third key
(`m a (`, `m s (`). A reverse check over every code span in the document
reports fifty of those as defects, and a guard that reports defects that are
not defects is disabled by the next person who hits one. The reverse
direction is therefore checked only where the document makes an explicit,
structured claim — the unbound lists.

## Non-vacuity

`--self-test` runs synthetic bindings and synthetic documents through the same
analysis the real tree goes through, and each case states which rule it
proves — including a case in which an injected defect fires *no* rule, which
is the case that would catch a guard that passes everything.

The real tree's non-vacuity is proved separately, by injection: a leaf added
to a real table without a mention in the document must make this guard fail.

Exit code 1 on violations, 2 on a shape the guard cannot resolve.

Usage::

    python3 scripts/check_tui_editor_doc_drift.py
    python3 scripts/check_tui_editor_doc_drift.py --self-test

This is invoked by `scripts/verify.sh quick` and the CI `verify` job.
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import check_tui_editor_key_authority as key_authority  # noqa: E402
import check_tui_editor_leaf_authority as leaf_authority  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parent.parent
EDITOR = REPO_ROOT / "src" / "tui" / "editor.rs"
COMMANDS = REPO_ROOT / "src" / "tui" / "commands" / "editor.rs"
DOC = REPO_ROOT / "docs" / "tui.md"

# The marker that says "the keys below are deliberately not bound".
UNBOUND_MARKER = "<!-- unbound:"

# How each prefix spells a leaf gesture, keyed by the `EditorPrefix` variant
# the leaf guard reports. The prefix's own `display()` string is the
# authority for the prefix's text; what differs between prefixes is whether a
# space separates it from the glyph.
#
# The `space` glyph is written `Space`, not a blank — a blank would be
# invisible in a code span, and `]Space` is how the document spells it.
LEAF_PREFIXES = {
    "Space": ("space", " "),
    "Match": ("m", " "),
    "Goto": ("g", ""),
    # View mode. The prefix's own text is `z` and its leaves follow it
    # directly — `zc`, `zt`, `zb`, `zj`, `zk`.
    "View": ("z", ""),
    "OpenBracket": ("[", ""),
    "CloseBracket": ("]", ""),
}

# The shape of a chord token the document may write: an optional `Ctrl` and
# an optional `Alt`, each joined with either `+` or `-`, then the target.
CHORD_TOKEN_RE = re.compile(
    r"^(?:(?P<a>Ctrl|Alt)[+-](?:(?P<b>Ctrl|Alt)[+-])?)?(?P<target>.+)$"
)

# A fenced code block, which carries shell commands rather than bindings.
FENCE_RE = re.compile(r"^```.*?^```", re.DOTALL | re.MULTILINE)

# A code span, in both of the forms the document uses: the ordinary one, and
# the doubled one a span whose content is a backtick has to be written as.
CODE_SPAN_RE = re.compile(r"`([^`\n]+)`")
CODE_SPAN_DOUBLED_RE = re.compile(r"``(.+?)``", re.DOTALL)

# The blank character is the first key of the space menu, and the document
# spells that menu by its name — a code span cannot carry a blank and still
# be readable.
BLANK_SPELLING = "space"

# The document's minus key, spelled as a word when it follows a modifier.
MINUS_WORD = "minus"

# The em dash the document uses to separate a key list from its explanation.
EM_DASH = "—"


class ParseError(Exception):
    """A shape the guard cannot resolve."""


@dataclass(frozen=True)
class Violation:
    """One violation, tagged with the rule that found it.

    The tag is what lets `--self-test` assert *which* rule fired.
    """

    rule: int
    message: str


@dataclass
class LeafBinding:
    """One leaf gesture: the prefix that owns it and the glyph it takes."""

    prefix: str
    glyph: str
    spelling: str


@dataclass
class ChordBinding:
    """One chord the code claims, with the spellings a document may use."""

    key: str
    spellings: tuple[str, ...]
    canonical: tuple[str, frozenset[str]]


@dataclass
class Model:
    """Everything the guard compares, in plain data.

    Plain data, not the other guards' objects, so the self-test can build a
    model without building a source tree.
    """

    leaves: list[LeafBinding] = field(default_factory=list)
    chords: list[ChordBinding] = field(default_factory=list)
    document: str = ""


# ── The spellings ────────────────────────────────────────────────────────────


def leaf_spelling(prefix: str, glyph: str) -> str:
    """How the document writes one leaf gesture."""
    try:
        head, separator = LEAF_PREFIXES[prefix]
    except KeyError as error:
        raise ParseError(f"unknown prefix {prefix!r}") from error
    written = "Space" if glyph == " " else glyph
    return f"{head}{separator}{written}"


def _target_spellings(modifiers: frozenset[str], char: str) -> list[str]:
    """The characters a chord may be written as, without its modifier prefix.

    A chord pressed with `SHIFT` arrives as the upper-case character, so an
    arm that admits `SHIFT` can be written either way — and the document
    writes the upper-case one.
    """
    names = set(modifiers)
    if char == " ":
        # The blank is the space menu's first key, and the menu is named.
        return [BLANK_SPELLING]
    targets: list[str] = []
    if "SHIFT" in names:
        targets.append(char.upper())
    targets.append(char)
    if char == "-":
        targets.append(MINUS_WORD)
    return list(dict.fromkeys(targets))


def chord_spellings(modifiers: frozenset[str], char: str) -> tuple[str, ...]:
    """The spellings the document may use for one claimed chord.

    A plain chord is the character itself, a shifted chord is the upper-case
    character, and a modified chord is `Ctrl+x` / `Alt-x` — the document uses
    both the plus and the hyphen, and the minus key is spelled `minus`.
    """
    names = set(modifiers)
    targets = _target_spellings(modifiers, char)
    spellings: list[str] = []
    if "CONTROL" in names and "ALT" in names:
        for target in targets:
            spellings.append(f"Ctrl+Alt+{target}")
            spellings.append(f"Alt+Ctrl+{target}")
    elif "CONTROL" in names:
        for target in targets:
            spellings.append(f"Ctrl+{target}")
            spellings.append(f"Ctrl-{target}")
    elif "ALT" in names:
        for target in targets:
            spellings.append(f"Alt+{target}")
            spellings.append(f"Alt-{target}")
    else:
        spellings.extend(targets)
    return tuple(dict.fromkeys(spellings))


def chord_canonical(modifiers: frozenset[str], char: str) -> tuple[str, frozenset[str]]:
    """The comparable form of a claimed chord: its effective character.

    A chord pressed with `SHIFT` arrives as the upper-case character, so the
    shift folds into the character and the remaining flags are `Ctrl` and
    `Alt`.
    """
    names = set(modifiers)
    effective = char.upper() if "SHIFT" in names else char
    flags = frozenset(
        name for name in ("CONTROL", "ALT") if name in names
    )
    return effective, flags


def token_canonical(token: str) -> tuple[str, frozenset[str]] | None:
    """The comparable form of a chord token the document wrote.

    `None` when the token is not a chord at all — a gesture, a word, or a
    bracketed identifier.
    """
    match = CHORD_TOKEN_RE.match(token)
    if match is None:
        return None
    target = match.group("target")
    if target == MINUS_WORD:
        target = "-"
    if len(target) != 1:
        return None
    flags: set[str] = set()
    for name in (match.group("a"), match.group("b")):
        if name is None:
            continue
        flags.add("CONTROL" if name == "Ctrl" else "ALT")
    return target, frozenset(flags)


# ── Reading the document ─────────────────────────────────────────────────────


def unbound_lists(document: str) -> list[list[str]]:
    """The key lists the document declares unbound.

    A marker line introduces a list; the list runs until a line that is not a
    bullet. A blank line between the marker and the list is not the end of it
    — the marker and its list are two things, and a reader does not expect
    them to be on the same line. Each bullet's keys are the code spans before
    the em dash, so the explanation after it is prose and not a key.
    """
    lists: list[list[str]] = []
    lines = document.splitlines()
    index = 0
    while index < len(lines):
        if UNBOUND_MARKER not in lines[index]:
            index += 1
            continue
        index += 1
        while index < len(lines) and not lines[index].strip():
            index += 1
        keys: list[str] = []
        while index < len(lines):
            stripped = lines[index].strip()
            if not stripped.startswith("-"):
                break
            head = stripped.split(EM_DASH, 1)[0]
            keys.extend(re.findall(r"`([^`]+)`", head))
            index += 1
        lists.append(keys)
    return lists


def code_spans(document: str) -> list[str]:
    """Every code span in the document, with the doubled form read too.

    Three forms, read in this order, because each one can swallow the next:

    - a **fenced block** carries shell commands, not bindings;
    - the **doubled** form carries a span whose content is a backtick — the
      document writes the backtick key as `` ` `` — and a single-span scan
      started earlier in the document would run across it;
    - the **ordinary** form is everything else.
    """
    doubled: list[str] = []

    def take(match: re.Match[str]) -> str:
        doubled.append(match.group(1).strip())
        return " " * len(match.group(0))

    without_fences = FENCE_RE.sub(lambda match: " " * len(match.group(0)), document)
    remainder = CODE_SPAN_DOUBLED_RE.sub(take, without_fences)
    spans = [match.group(1).strip() for match in CODE_SPAN_RE.finditer(remainder)]
    spans.extend(doubled)
    return spans


def document_mentions(document: str, spelling: str) -> bool:
    """Whether the document mentions a binding under one of its spellings.

    The check is a whole-token search rather than a substring search: a
    spelling that is a prefix of another one (`g` inside `gs`) must not
    satisfy a binding it does not name.
    """
    return spelling in code_spans(document)


# ── The analysis ─────────────────────────────────────────────────────────────


def analyse(model: Model) -> list[Violation]:
    """The one analysis: violations, in the order they are found."""
    violations: list[Violation] = []
    document = model.document

    # Rule 1 — every leaf gesture the code binds is documented.
    for leaf in model.leaves:
        if not document_mentions(document, leaf.spelling):
            violations.append(
                Violation(
                    1,
                    f"{leaf.spelling} is bound in the editor but `docs/tui.md` never mentions it",
                )
            )

    # Rule 2 — every chord the code claims is documented.
    for chord in model.chords:
        if not any(document_mentions(document, spelling) for spelling in chord.spellings):
            violations.append(
                Violation(
                    2,
                    f"{chord.key} is claimed by the keymap but `docs/tui.md` never mentions it",
                )
            )

    # Rule 4 — the document declares what is unbound. A document that does
    # not is a document the reverse check cannot read.
    lists = unbound_lists(document)
    if not lists:
        violations.append(
            Violation(
                4,
                f"`docs/tui.md` carries no `{UNBOUND_MARKER}` declaration, so the "
                "reverse direction cannot be checked",
            )
        )
    for keys in lists:
        if not keys:
            violations.append(
                Violation(4, f"`docs/tui.md` has an unbound declaration with no keys in it")
            )

    # Rules 3 and 5 — every declared-unbound key really is unbound, and is a
    # key the guard can recognise.
    bound_leaves = {leaf.spelling for leaf in model.leaves}
    bound_chords = {chord.canonical for chord in model.chords}
    for keys in lists:
        for key in keys:
            gesture = parse_leaf_token(key)
            if gesture is not None:
                if gesture in bound_leaves:
                    violations.append(
                        Violation(
                            3,
                            f"`docs/tui.md` declares {key} unbound, but the editor binds it",
                        )
                    )
                continue
            canonical = token_canonical(key)
            if canonical is None:
                violations.append(
                    Violation(
                        5,
                        f"`docs/tui.md` declares {key} unbound, but {key} is not a "
                        "binding shape this guard can recognise",
                    )
                )
                continue
            if canonical in bound_chords:
                violations.append(
                    Violation(
                        3,
                        f"`docs/tui.md` declares {key} unbound, but the editor binds it",
                    )
                )
    return violations


def parse_leaf_token(token: str) -> str | None:
    """The leaf spelling a token names, or `None` when it names no leaf."""
    for prefix, (head, separator) in LEAF_PREFIXES.items():
        stem = f"{head}{separator}"
        if not token.startswith(stem):
            continue
        rest = token[len(stem) :]
        if rest == "Space":
            rest = " "
        if len(rest) != 1:
            continue
        return leaf_spelling(prefix, rest)
    return None


# ── Building the model from the tree ─────────────────────────────────────────


def build_model() -> Model:
    """The model, read from the tree through the other two guards."""
    editor_text = EDITOR.read_text()
    commands_text = COMMANDS.read_text()

    tables = leaf_authority.parse_tables(editor_text)
    bindings = leaf_authority.parse_prefix_bindings(editor_text)
    prefix_of = {table: prefix for prefix, table in bindings if table is not None}

    leaves: list[LeafBinding] = []
    for table, rows in tables.items():
        prefix = prefix_of.get(table)
        if prefix is None:
            # A table no prefix binds is the leaf guard's rule 5, not this
            # guard's business.
            continue
        if prefix not in LEAF_PREFIXES:
            raise ParseError(f"prefix {prefix!r} has no documented spelling")
        for row in rows:
            leaves.append(
                LeafBinding(
                    prefix=prefix,
                    glyph=row.glyph,
                    spelling=leaf_spelling(prefix, row.glyph),
                )
            )

    analysis = key_authority.analyse(commands_text)
    if analysis.problems:
        raise ParseError(
            "the key guard could not resolve the dispatch: " + "; ".join(analysis.problems)
        )

    chords: list[ChordBinding] = []
    for chord in list(analysis.guard_chords) + list(analysis.normal_chords) + list(
        analysis.insert_chords
    ):
        if chord.char is None:
            # The Insert-mode wildcard claims every printable character; it
            # names no single binding to document.
            continue
        names = key_authority.combo_names(chord.combos)
        spellings = chord_spellings(names, chord.char)
        chords.append(
            ChordBinding(
                key=chord.label(),
                spellings=spellings,
                canonical=chord_canonical(names, chord.char),
            )
        )

    return Model(leaves=leaves, chords=chords, document=DOC.read_text())


# ── The self-test ────────────────────────────────────────────────────────────


@dataclass
class Expectation:
    """What a self-test case expects the analysis to find."""

    rules: list[int]
    note: str


def _leaf(prefix: str, glyph: str) -> LeafBinding:
    return LeafBinding(prefix=prefix, glyph=glyph, spelling=leaf_spelling(prefix, glyph))


def _chord(key: str, modifiers: frozenset[str], char: str) -> ChordBinding:
    return ChordBinding(
        key=key,
        spellings=chord_spellings(modifiers, char),
        canonical=chord_canonical(modifiers, char),
    )


SELF_TEST_CASES: list[tuple[str, Model, Expectation]] = [
    (
        "a clean tree finds nothing",
        Model(
            leaves=[_leaf("Space", "a")],
            chords=[_chord("d", frozenset({"NONE"}), "d")],
            document=(
                "The `space a` leaf opens the code actions.\n"
                "The `d` key deletes.\n"
                "<!-- unbound: the keys below are deliberately not bound -->\n"
                "- `gf` — files, which nothing answers yet.\n"
            ),
        ),
        Expectation([], "nothing to find"),
    ),
    (
        "an undocumented leaf is rule 1",
        Model(
            leaves=[_leaf("Space", "a"), _leaf("Goto", "s")],
            chords=[],
            document=(
                "The `space a` leaf opens the code actions.\n"
                "<!-- unbound: the keys below are deliberately not bound -->\n"
                "- `gf` — files.\n"
            ),
        ),
        Expectation([1], "`gs` is bound and undocumented"),
    ),
    (
        "an undocumented chord is rule 2",
        Model(
            leaves=[],
            chords=[_chord("NONE+'d'", frozenset({"NONE"}), "d")],
            document=(
                "<!-- unbound: the keys below are deliberately not bound -->\n"
                "- `gf` — files.\n"
            ),
        ),
        Expectation([2], "`d` is claimed and undocumented"),
    ),
    (
        "a bound key declared unbound is rule 3",
        Model(
            leaves=[_leaf("Goto", "f")],
            chords=[],
            document=(
                "<!-- unbound: the keys below are deliberately not bound -->\n"
                "- `gf` — files, which nothing answers yet.\n"
            ),
        ),
        Expectation([3], "`gf` is bound but declared unbound"),
    ),
    (
        "a document with no unbound declaration is rule 4",
        Model(
            leaves=[],
            chords=[],
            document="Nothing is declared unbound here.\n",
        ),
        Expectation([4], "the reverse direction cannot be checked"),
    ),
    (
        "an unreadable unbound token is rule 5",
        Model(
            leaves=[],
            chords=[],
            document=(
                "<!-- unbound: the keys below are deliberately not bound -->\n"
                "- `g f` — two words, not a key.\n"
            ),
        ),
        Expectation([5], "the token is not a binding shape"),
    ),
    (
        "a spelling the document uses is accepted",
        Model(
            leaves=[],
            chords=[_chord("ALT+'u'", frozenset({"ALT"}), "u")],
            document=(
                "The `Alt-u` chord undoes.\n"
                "<!-- unbound: the keys below are deliberately not bound -->\n"
                "- `gf` — files.\n"
            ),
        ),
        Expectation([], "`Alt-u` is one of the two spellings the document uses"),
    ),
    (
        "a shifted chord is documented as its upper-case character",
        Model(
            leaves=[],
            chords=[_chord("SHIFT+'A'", frozenset({"SHIFT"}), "A")],
            document=(
                "The `A` key opens above.\n"
                "<!-- unbound: the keys below are deliberately not bound -->\n"
                "- `gf` — files.\n"
            ),
        ),
        Expectation([], "`A` documents the shifted chord"),
    ),
]


def run_self_test() -> int:
    """Run every case; report the ones that disagree with the expectation."""
    failures = 0
    for name, model, expected in SELF_TEST_CASES:
        violations = analyse(model)
        rules = sorted({violation.rule for violation in violations})
        if rules != sorted(expected.rules):
            failures += 1
            print(
                f"self-test: {name}\n"
                f"  expected rules {sorted(expected.rules)}, got {rules}\n"
                + "\n".join(f"  {violation.message}" for violation in violations),
                file=sys.stderr,
            )
        else:
            print(f"self-test: {name} — {expected.note}")
    if failures:
        print(f"self-test: {failures} of {len(SELF_TEST_CASES)} cases failed", file=sys.stderr)
        return 1
    print(f"self-test: {len(SELF_TEST_CASES)} cases passed")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run the synthetic cases instead of checking the tree",
    )
    args = parser.parse_args(argv)
    if args.self_test:
        return run_self_test()
    try:
        model = build_model()
    except (ParseError, leaf_authority.ParseError, key_authority.ParseError) as error:
        print(f"check_tui_editor_doc_drift: cannot resolve the tree: {error}", file=sys.stderr)
        return 2
    violations = analyse(model)
    if violations:
        for violation in violations:
            print(
                f"check_tui_editor_doc_drift: rule {violation.rule}: {violation.message}",
                file=sys.stderr,
            )
        print(
            f"check_tui_editor_doc_drift: {len(violations)} violation(s)", file=sys.stderr
        )
        return 1
    print(
        "check_tui_editor_doc_drift: "
        f"{len(model.leaves)} leaf gestures and {len(model.chords)} chords are documented, "
        "and every key declared unbound is unbound"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
