#!/usr/bin/env python3
"""Static guard: the `/files` navigator's three published keymaps are one.

`plans/implementation/file-picker-rework-post-closure-corrective/002-box-exit-verb-and-editor-handoff.md`
§3.5 records the defect this guard closes: three surfaces publish the
navigator's verbs, and nothing compared them.

* the widget's `HINT` constant called `l` an **open** while the tables called
  it a **peek**;
* the hint named `⇧Q` and not `q`, so the box's own exit verb was in the
  tables and not in the hint;
* the tables and the hint could each drift on their own afterwards.

The milestone's acceptance criterion is that the hint, the
`architecture/tui.md` table and the `docs/tui.md` table describe **one set of
verbs**, and that a static guard with a non-vacuous `--self-test` keeps them
from drifting.

## What this guard reads

Four inputs, and no more:

* `src/tui/components/dialogs/files.rs` — the widget's `HINT` constant. This
  is the **model**: it is the one string the panel host's composer status line
  and the help dialog read, so it is what the code believes the keys do. The
  guard reads that one string literal and nothing else in the Rust file — it
  never re-derives the keymap from `handle_key`, which is what a guard that
  parsed the Rust *code* would have to do.
* `src/tui/panels/mod.rs` — the panel's `ADVERTISED_VERBS` list, the verbs its
  status line must show.
* `architecture/tui.md` — the `/files` key table.
* `docs/tui.md` — the browsing-files key table, and the status line it
  quotes.

## What counts as a violation

**Violations** (exit code 1):

- **Rule 1 — the two tables describe different verbs.** One set of verbs, two
  tables: a reader of one is told something the reader of the other is not.
- **Rule 2 — the hint names a key no table names.** A verb the code
  publishes and the documents do not.
- **Rule 3 — the tables name a key the hint does not.** The reverse
  direction, and the one that matters most: the hint is the *model*, so a
  verb only the documents know is a verb the code does not have. The
  exception is the **docked-blind** keys below.
- **Rule 4 — a key the guard does not know.** A key the guard cannot read is
  a key the guard cannot check.
- **Rule 5 — the hint and a table describe a verb with different words.**
  This is what makes `l` a *peek* in all three surfaces rather than an
  *open* in one of them.
- **Rule 6 — the `q` verb does not name the box.** `q` closes the **modal
  box**; in the docked panel it does nothing, and the table has to say so.
- **Rule 7 — `⇧Q` is not the panel's exit.** The closed decision: `q` in the
  docked panel does nothing, and `Shift+Q` is what leaves it.
- **Rule 8 — the quoted status line has drifted from the hint.**
  `docs/tui.md` shows the hint as the reader sees it; a stale quote is the
  same drift in one more place.
- **Rule 9 — the panel's advertised verbs have drifted from the hint.** The
  panel's own test names the verbs its status line must show. That list is a
  **fourth copy** of the vocabulary, and it drifted once: the hint moved to
  `peek` and the copy did not, and the drift reached CI as a failed test
  rather than as a finding here.
- **Rule 10 — the hint says a word the panel chrome owns.** The chrome
  mints `Shift+← back` for a **focused** panel — the way out to the composer
  — and an **unfocused** panel's line must not say "back" at all: it has not
  been entered, so it has nothing to go back from. The navigator's `h` is
  therefore **up** one level, and the hint may not spend the chrome's word.

## The docked-blind keys

The hint is **one** string for **two** surfaces: the modal box and the
docked panel. It cannot name every key, because the panel host answers the
arrow keys and `Esc` before the box is asked — `docs/tui.md` says so
("A docked box never sees the arrow keys, `Esc`, or uppercase
`H`/`J`/`K`/`L`"). Those keys are in the tables and not in the hint, and the
guard knows them by name rather than by guessing.

## The spellings the surfaces use

The hint is compact (`^d`, `⇧Q`, `enter`) and the tables are readable
(`Ctrl+d`, `Shift+Q`, `Enter`). The guard accepts both spellings of the same
key and compares them in one canonical form, so a guard that demanded one
spelling would be a style guard rather than a drift guard.

Usage::

    python3 scripts/check_tui_files_keymap_drift.py            # check the tree
    python3 scripts/check_tui_files_keymap_drift.py --self-test # prove the check bites
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

WIDGET_RS = "src/tui/components/dialogs/files.rs"
PANEL_RS = "src/tui/panels/mod.rs"
ARCHITECTURE_MD = "architecture/tui.md"
DOCS_MD = "docs/tui.md"

# The heading each key table lives under. A table that cannot be found is a
# violation, not a clean tree: a guard that finds nothing because it looked
# in the wrong place reports success and means nothing.
ARCHITECTURE_HEADING = "### The `/files` navigator: the ownership split"
DOCS_HEADING = "## Browsing files"

# The keys the navigator answers to. A key outside this set is a key nobody
# published, so it is a violation rather than a silent addition.
KNOWN_KEYS = frozenset(
    {
        "j",
        "k",
        "ctrl+d",
        "ctrl+u",
        "l",
        "→",
        "h",
        "←",
        "enter",
        "i",
        "r",
        "q",
        "shift+q",
        "esc",
        "↑",
        "↓",
    }
)

# The keys a **docked** box never sees: the panel host answers them before
# the box is asked, so the compact hint cannot name them.
DOCKED_BLIND_KEYS = frozenset({"esc", "↑", "↓"})

# The words the **panel chrome** owns. The chrome mints `Shift+← back` for a
# focused panel — the way out to the composer — so "back" in a panel's status
# line means *that* and nothing else. A widget's hint shares that line, so it
# may not spend the word on its own motion.
CHROME_OWNED_WORDS = frozenset({"back"})

# The compact spellings the hint uses, and the readable ones the tables use,
# for the same key.
CANONICAL_SPELLINGS = {"^d": "ctrl+d", "^u": "ctrl+u", "⇧q": "shift+q"}

# The separator the hint and the quoted status line use between verbs.
VERB_SEPARATOR = "·"

# The widget's own published model: one string literal, read and never
# re-derived from the Rust around it.
HINT_CONST_RE = re.compile(
    r"pub\s+const\s+HINT\s*:\s*&str\s*=\s*\"(?P<body>(?:[^\"\\]|\\.)*)\"\s*;"
)

# The panel's own advertised verbs: a named list, read by name rather than by
# shape, so a renamed or deleted list is a finding and not a silent pass.
ADVERTISED_VERBS_RE = re.compile(
    r"const\s+ADVERTISED_VERBS\s*:\s*&\[&str\]\s*=\s*&\[(?P<body>[^\]]*)\]\s*;"
)

# A Rust string literal.
STRING_LITERAL_RE = re.compile(r"\"((?:[^\"\\]|\\.)*)\"")

# A code span in a table's key cell.
CODE_SPAN_RE = re.compile(r"`([^`]+)`")

# A rendered panel status line: a block glyph, then the panel's name.
STATUS_LINE_RE = re.compile(r"^.\s*Files\s+" + re.escape(VERB_SEPARATOR) + r"\s")


class ParseError(Exception):
    """The tree does not have the shape the guard reads."""


@dataclass(frozen=True)
class Violation:
    """One thing the guard found."""

    rule: int
    message: str


@dataclass(frozen=True)
class Verb:
    """One verb of the navigator's published keymap."""

    keys: frozenset[str]
    word: str
    text: str


@dataclass(frozen=True)
class Row:
    """One row of a published key table."""

    keys: frozenset[str]
    text: str


@dataclass(frozen=True)
class Model:
    """Everything the guard reads."""

    hint: str
    architecture: tuple[Row, ...]
    docs: tuple[Row, ...]
    docs_text: str
    advertised: tuple[str, ...]


def canonical(token: str) -> str:
    """One key in one form, whichever surface spelled it."""
    lowered = token.strip().lower()
    return CANONICAL_SPELLINGS.get(lowered, lowered)


def contains_word(text: str, word: str) -> bool:
    """Whether `text` says `word`.

    A stem matches: the hint's ``open`` is satisfied by a table's
    ``opened``, because the two are the same verb described in different
    prose.
    """
    return re.search(r"\b" + re.escape(word), text, re.IGNORECASE) is not None


def contains_any_word(text: str, words: tuple[str, ...]) -> bool:
    return any(contains_word(text, word) for word in words)


def read_hint(source: str) -> str:
    """The widget's own published model: the `HINT` constant's literal."""
    match = HINT_CONST_RE.search(source)
    if match is None:
        raise ParseError(
            "the widget's `HINT` constant was not found; the guard reads the "
            "navigator's model from it and has nothing else to check"
        )
    return unescape(match.group("body"))


def read_advertised(source: str) -> tuple[str, ...]:
    """The verbs the panel's status line must advertise."""
    match = ADVERTISED_VERBS_RE.search(source)
    if match is None:
        raise ParseError(
            "the panel's `ADVERTISED_VERBS` list was not found; the guard "
            "compares the panel's advertised verbs with the hint and has "
            "nothing else to check"
        )
    return tuple(
        unescape(literal)
        for literal in STRING_LITERAL_RE.findall(match.group("body"))
    )


def unescape(body: str) -> str:
    """The escapes a Rust string literal may carry."""
    out: list[str] = []
    index = 0
    while index < len(body):
        character = body[index]
        if character != "\\" or index + 1 >= len(body):
            out.append(character)
            index += 1
            continue
        following = body[index + 1]
        out.append(
            {
                "n": "\n",
                "t": "\t",
                "r": "\r",
                "0": "\0",
                "\\": "\\",
                '"': '"',
                "'": "'",
            }.get(following, "\\" + following)
        )
        index += 2
    return "".join(out)


def parse_hint(hint: str) -> list[Verb]:
    """The hint's verbs: each segment is `<keys> <word>`."""
    verbs: list[Verb] = []
    for segment in hint.split(VERB_SEPARATOR):
        segment = segment.strip()
        if not segment:
            continue
        parts = segment.split()
        if len(parts) != 2:
            raise ParseError(
                f"the hint's segment {segment!r} is not `<keys> <word>`; the "
                "guard cannot tell which keys it names"
            )
        keys_text, word = parts
        keys = frozenset(
            canonical(key) for key in keys_text.split("/") if key.strip()
        )
        verbs.append(Verb(keys=keys, word=word, text=segment))
    if not verbs:
        raise ParseError("the hint names no verb at all")
    return verbs


def parse_key_cell(cell: str) -> frozenset[str]:
    """The keys a table row's key cell names."""
    keys: set[str] = set()
    for span in CODE_SPAN_RE.findall(cell):
        for token in span.split("/"):
            token = token.strip()
            if token:
                keys.add(canonical(token))
    return frozenset(keys)


def find_table(text: str, heading: str) -> tuple[Row, ...]:
    """The first key table under `heading`."""
    lines = text.splitlines()
    start: int | None = None
    for index, line in enumerate(lines):
        if line.strip() == heading:
            start = index
            break
    if start is None:
        raise ParseError(f"the heading {heading!r} was not found")
    rows: list[Row] = []
    header_seen = False
    for line in lines[start + 1 :]:
        stripped = line.strip()
        if stripped.startswith("#"):
            break
        if not stripped.startswith("|"):
            if rows:
                break
            continue
        cells = [cell.strip() for cell in stripped.strip("|").split("|")]
        if not header_seen:
            header_seen = True
            continue
        if all(set(cell) <= set("-: ") for cell in cells):
            continue
        if len(cells) < 2:
            raise ParseError(f"the row {stripped!r} has no meaning column")
        rows.append(Row(keys=parse_key_cell(cells[0]), text=cells[1]))
    if not rows:
        raise ParseError(f"no key table was found under {heading!r}")
    return tuple(rows)


def rows_carrying(rows: tuple[Row, ...], key: str) -> list[Row]:
    return [row for row in rows if key in row.keys]


def analyse(model: Model) -> list[Violation]:
    """Every rule, against one model."""
    violations: list[Violation] = []
    try:
        verbs = parse_hint(model.hint)
    except ParseError as error:
        return [Violation(1, str(error))]

    hint_keys = frozenset().union(*(verb.keys for verb in verbs))
    architecture_keys = frozenset().union(*(row.keys for row in model.architecture))
    docs_keys = frozenset().union(*(row.keys for row in model.docs))

    # Rule 1 — the two tables describe the same verbs.
    if architecture_keys != docs_keys:
        only_architecture = sorted(architecture_keys - docs_keys)
        only_docs = sorted(docs_keys - architecture_keys)
        violations.append(
            Violation(
                1,
                "the two published tables describe different verbs: "
                f"only `architecture/tui.md` has {only_architecture}, "
                f"only `docs/tui.md` has {only_docs}",
            )
        )

    # Rule 2 — the hint names a key no table names.
    for key in sorted(hint_keys - architecture_keys):
        violations.append(
            Violation(2, f"the hint names `{key}` and no published table does")
        )

    # Rule 3 — the tables name a key the hint does not.
    for key in sorted((architecture_keys | docs_keys) - hint_keys):
        if key in DOCKED_BLIND_KEYS:
            continue
        violations.append(
            Violation(
                3,
                f"the published tables name `{key}` and the hint does not; "
                "the hint is the model, so the code has no such verb",
            )
        )

    # Rule 4 — a key the guard does not know.
    for key in sorted((hint_keys | architecture_keys | docs_keys) - KNOWN_KEYS):
        violations.append(
            Violation(4, f"`{key}` is not a key the navigator publishes")
        )

    # Rule 5 — the hint and the tables describe a verb with different words.
    for verb in verbs:
        for key in sorted(verb.keys):
            for label, rows in (
                ("architecture/tui.md", model.architecture),
                ("docs/tui.md", model.docs),
            ):
                carrying = rows_carrying(rows, key)
                if not carrying:
                    continue
                if len(carrying) > 1:
                    violations.append(
                        Violation(
                            5,
                            f"`{key}` is named by {len(carrying)} rows of "
                            f"{label}; the guard cannot tell which one is the "
                            f"verb's row",
                        )
                    )
                    continue
                if not contains_word(carrying[0].text, verb.word):
                    violations.append(
                        Violation(
                            5,
                            f"the hint calls `{key}` a *{verb.word}* and "
                            f"{label} does not say so: {carrying[0].text!r}",
                        )
                    )

    # Rule 6 — `q` names the box, and says the docked panel does nothing.
    for label, rows in (
        ("architecture/tui.md", model.architecture),
        ("docs/tui.md", model.docs),
    ):
        for row in rows_carrying(rows, "q"):
            if not contains_word(row.text, "close"):
                violations.append(
                    Violation(6, f"the `q` row of {label} does not say it closes")
                )
            if not contains_word(row.text, "box"):
                violations.append(
                    Violation(
                        6, f"the `q` row of {label} does not name the box it closes"
                    )
                )
            if not contains_any_word(row.text, ("nothing", "inert", "no-op")):
                violations.append(
                    Violation(
                        6,
                        f"the `q` row of {label} does not say that the docked "
                        "panel does nothing",
                    )
                )

    # Rule 7 — `⇧Q` is the panel's exit.
    for label, rows in (
        ("architecture/tui.md", model.architecture),
        ("docs/tui.md", model.docs),
    ):
        for row in rows_carrying(rows, "shift+q"):
            if not contains_word(row.text, "close"):
                violations.append(
                    Violation(
                        7, f"the `Shift+Q` row of {label} does not say it closes"
                    )
                )
            if not contains_word(row.text, "leave"):
                violations.append(
                    Violation(
                        7,
                        f"the `Shift+Q` row of {label} does not say it leaves "
                        "the side panel",
                    )
                )
            if not contains_any_word(row.text, ("panel", "pane")):
                violations.append(
                    Violation(
                        7,
                        f"the `Shift+Q` row of {label} does not name the panel "
                        "it leaves",
                    )
                )

    # Rule 8 — the quoted status line has drifted from the hint.
    violations.extend(check_status_line(model.docs_text, verbs))

    # Rule 9 — the panel's advertised verbs have drifted from the hint.
    for verb in model.advertised:
        if verb not in model.hint:
            violations.append(
                Violation(
                    9,
                    f"the panel advertises {verb!r} and the hint does not "
                    "publish it; the hint is the model, so the panel names a "
                    "verb the navigator does not have",
                )
            )
    # Rule 10 — the hint spends a word the panel chrome owns.
    for word in sorted(CHROME_OWNED_WORDS):
        if re.search(r"\b" + re.escape(word), model.hint, re.IGNORECASE):
            violations.append(
                Violation(
                    10,
                    f"the hint says {word!r}, which the panel chrome owns: a "
                    "focused panel's `Shift+← back` is the way out to the "
                    "composer, so the navigator's own motion must use another "
                    "word",
                )
            )
    return violations


def check_status_line(docs_text: str, verbs: list[Verb]) -> list[Violation]:
    """The status line `docs/tui.md` quotes shows the hint's own verbs."""
    quoted = [
        line for line in docs_text.splitlines() if STATUS_LINE_RE.match(line.strip())
    ]
    if not quoted:
        return [
            Violation(
                8,
                "`docs/tui.md` quotes no `/files` status line; the quote is the "
                "fourth surface the hint is published through",
            )
        ]
    segments = [verb.text for verb in verbs]
    violations: list[Violation] = []
    for line in quoted:
        head = line.split("Files", 1)[1]
        first = segments[0]
        index = head.find(first)
        if index < 0:
            violations.append(
                Violation(
                    8,
                    f"the quoted status line does not show the hint's first "
                    f"verb ({first!r}): {line.strip()!r}",
                )
            )
            continue
        region = head[index:].split("…")[0]
        shown = [part.strip() for part in region.split(VERB_SEPARATOR) if part.strip()]
        if shown != segments[: len(shown)]:
            violations.append(
                Violation(
                    8,
                    "the quoted status line has drifted from the hint: it shows "
                    f"{shown} where the hint says {segments[: len(shown)]}",
                )
            )
    return violations


def build_model() -> Model:
    """Read the tree."""
    widget = (REPO_ROOT / WIDGET_RS).read_text(encoding="utf-8")
    panel = (REPO_ROOT / PANEL_RS).read_text(encoding="utf-8")
    architecture = (REPO_ROOT / ARCHITECTURE_MD).read_text(encoding="utf-8")
    docs = (REPO_ROOT / DOCS_MD).read_text(encoding="utf-8")
    return Model(
        hint=read_hint(widget),
        architecture=find_table(architecture, ARCHITECTURE_HEADING),
        docs=find_table(docs, DOCS_HEADING),
        docs_text=docs,
        advertised=read_advertised(panel),
    )


# ── The self-test ───────────────────────────────────────────────────────────

CLEAN_HINT = (
    "j/k move · ^d/^u page · l/→ peek · h/← up · enter open · i filter · "
    "r refresh · q/⇧Q close"
)

# The verbs the panel's status line must advertise, and the guard's own copy
# of them: the panel names them, the hint publishes them, and a verb in one
# and not the other is rule 9.
CLEAN_ADVERTISED = ("enter open", "l/→ peek", "^d/^u page")

CLEAN_ARCHITECTURE = """## Key Types & APIs

### The `/files` navigator: the ownership split

| Key | Meaning |
|---|---|
| `j` / `k`, `↓` / `↑` | move the cursor |
| `Ctrl+d` / `Ctrl+u` | page down / page up — two thirds of the box's viewport |
| `Enter` | **the same motion as `l`**: a file is opened in the editor |
| `l` / `→` | **peek into** that directory: it opens in place |
| `h` / `←` | **up one level** |
| `i` | type a **filter** into the pane |
| `r` | **refresh**: re-walk the current root |
| `Esc` | leave insert mode, **keeping the filter** |
| `q` | close the **box** — in the side panel it does **nothing** |
| `Shift+Q` | close the box; **leave** the side panel |
"""

CLEAN_DOCS = """## Browsing files

| Key | What it does |
|---|---|
| `j` / `k`, `↓` / `↑` | move the cursor |
| `Ctrl+d` / `Ctrl+u` | page down / page up (two thirds of the pane) |
| `Enter` | **the same as `l`**: a file is opened in the editor |
| `l` / `→` | **peek into** that directory: it opens in place |
| `h` / `←` | go up one level |
| `i` | type a filter into the pane |
| `r` | **refresh**: re-list the current directory |
| `Esc` | leave the filter mode, keeping the filter |
| `q` | close the **box** — in the docked panel it does **nothing** |
| `Shift+Q` | close the box / **leave** the pane |

### Where the keys are

```
▌ Files · Shift+→/Shift+L in · j/k move · ^d/^u page · l/→ peek · …
```
"""


def clean_model() -> Model:
    return Model(
        hint=CLEAN_HINT,
        architecture=find_table(CLEAN_ARCHITECTURE, ARCHITECTURE_HEADING),
        docs=find_table(CLEAN_DOCS, DOCS_HEADING),
        docs_text=CLEAN_DOCS,
        advertised=CLEAN_ADVERTISED,
    )


@dataclass(frozen=True)
class Expectation:
    """What a synthetic case must produce."""

    rules: tuple[int, ...]
    note: str


def _model(
    hint: str = CLEAN_HINT,
    architecture: str = CLEAN_ARCHITECTURE,
    docs: str = CLEAN_DOCS,
    advertised: tuple[str, ...] = CLEAN_ADVERTISED,
) -> Model:
    return Model(
        hint=hint,
        architecture=find_table(architecture, ARCHITECTURE_HEADING),
        docs=find_table(docs, DOCS_HEADING),
        docs_text=docs,
        advertised=advertised,
    )


SELF_TEST_CASES: list[tuple[str, Model, Expectation]] = [
    (
        "a clean tree finds nothing",
        clean_model(),
        Expectation((), "the four surfaces agree"),
    ),
    (
        "the tables describing different verbs is rule 1",
        _model(
            docs=CLEAN_DOCS.replace(
                "| `j` / `k`, `↓` / `↑` | move the cursor |",
                "| `j` / `k` | move the cursor |",
            )
        ),
        Expectation((1,), "the two tables disagree about the arrow keys"),
    ),
    (
        "a verb only the hint names is rule 2",
        _model(
            architecture=CLEAN_ARCHITECTURE.replace(
                "| `q` | close the **box** — in the side panel it does **nothing** |\n",
                "",
            ),
            docs=CLEAN_DOCS.replace(
                "| `q` | close the **box** — in the docked panel it does **nothing** |\n",
                "",
            ),
        ),
        Expectation((2,), "the hint names `q` and neither table does"),
    ),
    (
        "a verb only the tables name is rule 3",
        _model(hint=CLEAN_HINT.replace(" · i filter", "")),
        Expectation((3,), "the tables name `i` and the hint does not"),
    ),
    (
        "a key nobody published is rule 4",
        _model(
            hint=CLEAN_HINT + " · z select",
            architecture=CLEAN_ARCHITECTURE.replace(
                "| `Shift+Q` | close the box; **leave** the side panel |",
                "| `z` | select the row |\n"
                "| `Shift+Q` | close the box; **leave** the side panel |",
            ),
            docs=CLEAN_DOCS.replace(
                "| `Shift+Q` | close the box / **leave** the pane |",
                "| `z` | select the row |\n"
                "| `Shift+Q` | close the box / **leave** the pane |",
            ),
        ),
        Expectation((4,), "`z` is not a key the navigator publishes"),
    ),
    (
        "a hint and a table describing a verb differently is rule 5",
        _model(
            architecture=CLEAN_ARCHITECTURE.replace(
                "| `l` / `→` | **peek into** that directory: it opens in place |",
                "| `l` / `→` | open that directory |",
            )
        ),
        Expectation((5,), "the hint calls `l` a peek and the table does not"),
    ),
    (
        "a `q` row that does not name the box is rule 6",
        _model(
            architecture=CLEAN_ARCHITECTURE.replace(
                "| `q` | close the **box** — in the side panel it does **nothing** |",
                "| `q` | close the listing |",
            )
        ),
        Expectation((6,), "the `q` row does not name the box"),
    ),
    (
        "a `q` row that does not say the panel does nothing is rule 6",
        _model(
            architecture=CLEAN_ARCHITECTURE.replace(
                "in the side panel it does **nothing**",
                "in the side panel it leaves",
            )
        ),
        Expectation((6,), "the `q` row does not say the docked panel is inert"),
    ),
    (
        "a `Shift+Q` row that is not the panel's exit is rule 7",
        _model(
            architecture=CLEAN_ARCHITECTURE.replace(
                "| `Shift+Q` | close the box; **leave** the side panel |",
                "| `Shift+Q` | close the box |",
            )
        ),
        Expectation((7,), "the `Shift+Q` row does not leave the panel"),
    ),
    (
        "a stale quoted status line is rule 8",
        _model(
            docs=CLEAN_DOCS.replace(
                "▌ Files · Shift+→/Shift+L in · j/k move · ^d/^u page · l/→ peek · …",
                "▌ Files · Shift+→/Shift+L in · j/k move · ^d/^u page · l/→ open · …",
            )
        ),
        Expectation((8,), "the quoted status line has drifted from the hint"),
    ),
    (
        "a panel verb the hint does not publish is rule 9",
        _model(advertised=("enter open", "l/→ open", "^d/^u page")),
        Expectation((9,), "the panel still advertises `l` as an open"),
    ),
    (
        "a hint that spends the chrome's word is rule 10",
        _model(
            hint=CLEAN_HINT.replace("h/← up", "h/← back"),
            architecture=CLEAN_ARCHITECTURE.replace(
                "**up one level**", "**back one level**"
            ),
            docs=CLEAN_DOCS.replace("go up one level", "go back one level"),
        ),
        Expectation((10,), "the hint says `back`, which the panel chrome owns"),
    ),
]


def run_self_test() -> int:
    """Run every case; report the ones that disagree with the expectation."""
    failures = 0
    for name, model, expected in SELF_TEST_CASES:
        violations = analyse(model)
        rules = tuple(sorted({violation.rule for violation in violations}))
        if rules != tuple(sorted(expected.rules)):
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
        print(
            f"self-test: {failures} of {len(SELF_TEST_CASES)} cases failed",
            file=sys.stderr,
        )
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
    except (OSError, ParseError) as error:
        print(
            f"check_tui_files_keymap_drift: cannot resolve the tree: {error}",
            file=sys.stderr,
        )
        return 2
    violations = analyse(model)
    if violations:
        for violation in violations:
            print(
                f"check_tui_files_keymap_drift: rule {violation.rule}: "
                f"{violation.message}",
                file=sys.stderr,
            )
        print(
            f"check_tui_files_keymap_drift: {len(violations)} violation(s)",
            file=sys.stderr,
        )
        return 1
    print(
        "check_tui_files_keymap_drift: the hint, the panel's advertised verbs "
        "and the two published tables describe one set of verbs"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
