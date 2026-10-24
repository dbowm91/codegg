#!/usr/bin/env python3
"""Static guard: the published composer keymap is the implemented one.

`plans/implementation/tui-composer-helix-parity-post-closure-corrective/003-the-vertical-keys-follow-a-multiline-composer.md`
§6.5 makes this guard the milestone's closing check. The milestone records
the divergence in §3.3: the composer's claim table says one thing about when
`j` and `k` belong to the composer, the published keymaps say another, and
nothing compared them.

| Surface | What it published |
|---|---|
| `architecture/tui.md`'s claim table | "composer buffer has more than one line" |
| `architecture/tui.md`'s page paragraph | "page the composer when it overflows" |
| `docs/tui.md`'s tables and prose | "does not fit its box" |
| the implementation's `ClaimCondition::label` | "composer content overflows the box" |

Four surfaces, three rules, and no guard: a reader of any one of them is
told something the reader of another is not.

## What this guard reads

Three inputs, and no more:

* `src/tui/composer_claims.rs` — the claim table (`COMPOSER_CLAIMS`) and the
  condition's wording (`ClaimCondition::label`). This is the **model**: the
  guard reads the table's entries and the label the code publishes, and it
  never re-derives either from the Rust around them.
* `architecture/tui.md` — the published claim table (the one whose header row
  is `| Letter | Verb | Condition | Lands with |`) and the page paragraph.
* `docs/tui.md` — the composer's word-motion table, its `[NOR]`-mode keymap
  table, and the two paragraphs that say when the vertical keys belong to the
  composer.

## What counts as a violation

**Violations** (exit code 1):

- **Rule D1 — a letter the two do not share.** A letter in the claim table
  that the published table does not name, or a letter the published table
  names that the claim table does not claim. One of them is a verb the code
  has and the reader cannot find; the other is a verb the reader is told
  exists and the code does not have.
- **Rule D2 — a different set of conditional letters.** The published table's
  conditional claims must be the claim table's conditional claims. A claim
  that is conditional in the code and unconditional in the table swallows the
  key in a state the table does not warn about, and the other way round
  promises the key where the code lets it fall through.
- **Rule D3 — a condition in different words.** The published table's
  condition must carry the implementation's condition wording. This is the
  rule the divergence in §3.3 is an instance of.
- **Rule D4 — prose that states another rule, and prose that still states the
  old one.** Every published surface that says *when* a claim applies must
  carry the implementation's wording, and no published surface may still
  carry a superseded phrasing.
- **Rule D5 — something the guard needs but cannot find.** A table, a row or
  an input that cannot be located is a violation, not a clean tree: a guard
  that found nothing because it looked in the wrong place reports success
  and means nothing.

## The one wording

The implementation's conditional label is **read from the source**
(`ClaimCondition::label`), never hardcoded here. The published surfaces must
carry that wording verbatim, compared after normalization: lowercased, with
`*` and backticks stripped and whitespace collapsed. The published surfaces
must therefore say, for example, "composer content spans more than one row"
and not a paraphrase of it — a paraphrase is a second wording, and the
milestone's acceptance criterion is *one* rule in *one* wording.

The phrasings the correction supersedes are rejected where they still
appear: "overflows the box", "does not fit its box" and "more than one
line".

Usage::

    python3 scripts/check_tui_composer_keymap_drift.py            # check the tree
    python3 scripts/check_tui_composer_keymap_drift.py --self-test # prove the check bites
    python3 scripts/check_tui_composer_keymap_drift.py --root DIR # check another tree
"""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

GUARD = "check_tui_composer_keymap_drift"
REPO_ROOT = Path(__file__).resolve().parent.parent

IMPL_RS = "src/tui/composer_claims.rs"
ARCHITECTURE_MD = "architecture/tui.md"
DOCS_MD = "docs/tui.md"

# The published claim table's header row. A table that cannot be found is a
# violation, not a clean tree.
CLAIM_TABLE_HEADER = ("letter", "verb", "condition", "lands with")

# The docs sections that carry the composer's published keymap.
WORD_MOTIONS_HEADING = "### Moving the caret in `[NOR]` mode"
NOR_KEYMAP_HEADING = "### The composer's own keymap in `[NOR]` mode"

# The word motions. The word-motion table is the one that names the most of
# them.
WORD_MOTION_LETTERS = frozenset({"w", "b", "e"})

# The phrasings the correction supersedes. A published surface that still
# carries one states the old rule.
SUPERSEDED_PHRASINGS = (
    "overflows the box",
    "does not fit its box",
    "more than one line",
)

# The ways a published surface says *when* a claim applies. A surface that
# carries none of them states no condition at all, so it cannot disagree
# with the implementation about one; a surface that carries one must carry
# the implementation's wording.
CONDITION_MARKERS = (
    "only when",
    "only in",
    "unless",
    "if the ",
    "if a ",
    "when the ",
    "when your ",
    "when it ",
    "when its ",
    "when there ",
)

# The claim table's own table.
CLAIMS_RE = re.compile(
    r"pub\s+const\s+COMPOSER_CLAIMS\b[^=]*=\s*&\[(?P<body>.*?)\n\];",
    re.DOTALL,
)
KEY_RE = re.compile(r"\bkey:\s*'(?P<key>[A-Za-z])'")
CONDITION_RE = re.compile(r"\bcondition:\s*ClaimCondition::(?P<variant>\w+)")
LABEL_SOME_RE = re.compile(
    r"Self::(?P<variant>\w+)\s*=>\s*Some\(\s*\"(?P<label>[^\"]*)\"\s*\)"
)
LABEL_NONE_RE = re.compile(r"Self::(?P<variant>\w+)\s*=>\s*None\b")


class ParseError(Exception):
    """The tree does not have the shape the guard reads."""


@dataclass(frozen=True)
class Violation:
    """One thing the guard found."""

    rule: str
    message: str


@dataclass(frozen=True)
class Claim:
    """One entry of the implemented claim table."""

    key: str
    variant: str
    label: str | None
    conditional: bool


@dataclass(frozen=True)
class Row:
    """One row of a published table."""

    cells: tuple[str, ...]
    line: int


@dataclass(frozen=True)
class Table:
    """One published table."""

    header: tuple[str, ...]
    rows: tuple[Row, ...]
    start_line: int
    end_line: int


@dataclass(frozen=True)
class Paragraph:
    """One prose paragraph."""

    line: int
    end_line: int
    text: str


@dataclass(frozen=True)
class Location:
    """One published surface the guard reads."""

    name: str
    path: str
    line: int | None
    text: str


@dataclass(frozen=True)
class Model:
    """Everything the guard reads."""

    claims: tuple[Claim, ...]
    conditional_label: str
    arch_table: Table
    arch_page: Paragraph
    docs_word_table: Table
    docs_key_table: Table
    docs_jk: Paragraph
    docs_page: Paragraph


def normalize(text: str) -> str:
    """One text in one form: lowercased, unmarked, whitespace-collapsed.

    The published surfaces are markdown, so the same words arrive wrapped in
    `**` and `` ` `` and broken across lines. Normalizing removes the
    markup and the line breaks, and nothing else: the wording is compared,
    not the typesetting.
    """
    unmarked = text.lower().replace("`", "").replace("*", "")
    return " ".join(unmarked.split())


def states_a_condition(text: str) -> bool:
    """Whether a published surface says *when* a claim applies."""
    haystack = normalize(text)
    return any(marker in haystack for marker in CONDITION_MARKERS)


def letters_in(cell: str) -> frozenset[str]:
    """The single letters a table's key cell names."""
    found: set[str] = set()
    for token in re.split(r"[/,]", cell):
        token = token.strip().strip("`*").strip()
        if len(token) == 1 and token.isascii() and token.isalpha():
            found.add(token)
    return frozenset(found)


def split_row(line: str) -> tuple[str, ...]:
    """The cells of a pipe row."""
    stripped = line.strip()
    if stripped.startswith("|"):
        stripped = stripped[1:]
    if stripped.endswith("|"):
        stripped = stripped[:-1]
    return tuple(cell.strip() for cell in stripped.split("|"))


def is_separator(cells: tuple[str, ...]) -> bool:
    """Whether a row is the table's `|---|---|` rule."""
    return bool(cells) and all(cell and set(cell) <= set("-: ") for cell in cells)


def find_tables(text: str) -> list[Table]:
    """Every pipe table in the text, in document order."""
    lines = text.splitlines()
    tables: list[Table] = []
    index = 0
    while index < len(lines):
        if not lines[index].strip().startswith("|"):
            index += 1
            continue
        start = index
        block: list[tuple[int, str]] = []
        while index < len(lines) and lines[index].strip().startswith("|"):
            block.append((index + 1, lines[index]))
            index += 1
        header = split_row(block[0][1])
        rows = tuple(
            Row(cells=split_row(raw), line=line)
            for line, raw in block[1:]
            if not is_separator(split_row(raw))
        )
        tables.append(
            Table(header=header, rows=rows, start_line=start + 1, end_line=index)
        )
    return tables


def section_bounds(text: str, heading: str) -> tuple[int, int]:
    """The 1-based line range of the section under `heading`."""
    lines = text.splitlines()
    start: int | None = None
    for index, line in enumerate(lines):
        if line.strip() == heading:
            start = index + 1
            break
    if start is None:
        raise ParseError(f"the heading {heading!r} was not found")
    end = len(lines) + 1
    for index in range(start, len(lines)):
        if lines[index].strip().startswith("#"):
            end = index + 1
            break
    return start, end


def find_paragraphs(text: str) -> list[Paragraph]:
    """The prose paragraphs: the blocks that are not tables, lists or headings."""
    lines = text.splitlines()
    paragraphs: list[Paragraph] = []
    index = 0
    fenced = False
    while index < len(lines):
        stripped = lines[index].strip()
        if stripped.startswith("```"):
            fenced = not fenced
            index += 1
            continue
        if fenced or not stripped or stripped.startswith("#"):
            index += 1
            continue
        start = index
        block: list[str] = []
        while index < len(lines):
            current = lines[index].strip()
            if not current or current.startswith(("```", "#")):
                break
            block.append(current)
            index += 1
        if index == start:
            index += 1
            continue
        joined = " ".join(block)
        if joined.startswith(("|", "-", ">")):
            continue
        paragraphs.append(Paragraph(line=start + 1, end_line=index, text=joined))
    return paragraphs


def read_claims(source: str) -> list[tuple[str, str]]:
    """Each entry of the claim table: its key and its condition variant."""
    match = CLAIMS_RE.search(source)
    if match is None:
        raise ParseError(
            f"the claim table `COMPOSER_CLAIMS` was not found in {IMPL_RS}; "
            "the guard reads the implemented keymap from it"
        )
    entries: list[tuple[str, str]] = []
    for chunk in match.group("body").split("ComposerClaim {")[1:]:
        key_match = KEY_RE.search(chunk)
        condition_match = CONDITION_RE.search(chunk)
        if key_match is None or condition_match is None:
            raise ParseError(
                "an entry of the claim table has no `key:` or no `condition:`; "
                "the guard cannot tell what that claim is"
            )
        entries.append((key_match.group("key"), condition_match.group("variant")))
    if not entries:
        raise ParseError("the claim table has no entries")
    return entries


def read_condition_labels(source: str) -> dict[str, str | None]:
    """`ClaimCondition::label`: each condition variant's published wording."""
    labels: dict[str, str | None] = {}
    for block in source.split("\nimpl "):
        if not block.startswith("ClaimCondition") or "fn label" not in block:
            continue
        for match in LABEL_NONE_RE.finditer(block):
            labels[match.group("variant")] = None
        for match in LABEL_SOME_RE.finditer(block):
            labels[match.group("variant")] = match.group("label")
    return labels


def find_claim_table(text: str) -> Table:
    """The published claim table — the one with a Condition column."""
    for table in find_tables(text):
        header = tuple(normalize(cell) for cell in table.header)
        if header[: len(CLAIM_TABLE_HEADER)] == CLAIM_TABLE_HEADER:
            return table
    raise ParseError(
        "the published claim table was not found (looked for the header "
        "`| Letter | Verb | Condition | Lands with |`)"
    )


def find_page_paragraph(text: str) -> Paragraph:
    """The paragraph that says which of `Ctrl+u`/`Ctrl+d` pages the composer."""
    for paragraph in find_paragraphs(text):
        haystack = normalize(paragraph.text)
        if (
            "ctrl+u" in haystack
            and "ctrl+d" in haystack
            and "composer" in haystack
            and "transcript" in haystack
        ):
            return paragraph
    raise ParseError(
        "the page paragraph was not found in "
        f"{ARCHITECTURE_MD} (looked for the paragraph that says which of "
        "`Ctrl+u` and `Ctrl+d` pages the composer and which pages the "
        "transcript)"
    )


def find_word_motion_table(text: str) -> Table:
    """The composer's word-motion table.

    The section presents the horizontal motions first and the word motions
    second, so the word-motion table is the one that names the most word
    motions; a tie goes to the later table.
    """
    start, end = section_bounds(text, WORD_MOTIONS_HEADING)
    candidates = [
        table for table in find_tables(text) if start < table.start_line < end
    ]
    if not candidates:
        raise ParseError(
            "the composer's word-motion table was not found in "
            f"{DOCS_MD} (looked under {WORD_MOTIONS_HEADING!r}; the section "
            "holds no table)"
        )
    scored = [
        (len(WORD_MOTION_LETTERS & table_letters(table)), index, table)
        for index, table in enumerate(candidates)
    ]
    best = max(scored, key=lambda item: (item[0], item[1]))
    return best[2]


def find_keymap_table(text: str) -> Table:
    """The `[NOR]`-mode keymap table — the whole of the composer's keymap."""
    start, end = section_bounds(text, NOR_KEYMAP_HEADING)
    candidates = [
        table for table in find_tables(text) if start < table.start_line < end
    ]
    if len(candidates) != 1:
        raise ParseError(
            "the `[NOR]`-mode keymap table was not found in "
            f"{DOCS_MD} (looked under {NOR_KEYMAP_HEADING!r}; found "
            f"{len(candidates)} candidates)"
        )
    return candidates[0]


def find_jk_paragraph(text: str, table: Table) -> Paragraph:
    """The paragraph that says when `j` and `k` belong to the composer."""
    for paragraph in find_paragraphs(text):
        if paragraph.line <= table.end_line:
            continue
        haystack = normalize(paragraph.text)
        if re.search(r"\bj\b", haystack) and re.search(r"\bk\b", haystack):
            return paragraph
    raise ParseError(
        f"the paragraph about `j` and `k` was not found in {DOCS_MD} (looked "
        "for the first paragraph after the word-motion table that names both "
        "letters)"
    )


def find_docs_page_paragraph(text: str, after: Paragraph) -> Paragraph:
    """The paragraph that says what `Ctrl+u` and `Ctrl+d` page."""
    for paragraph in find_paragraphs(text):
        if paragraph.line <= after.end_line:
            continue
        haystack = normalize(paragraph.text)
        if "ctrl+u" in haystack and "ctrl+d" in haystack:
            return paragraph
    raise ParseError(
        f"the paragraph about `Ctrl+u` and `Ctrl+d` was not found in {DOCS_MD}"
    )


def build_model(root: Path) -> Model:
    """Read the tree at `root`."""
    impl_source = (root / IMPL_RS).read_text(encoding="utf-8")
    architecture = (root / ARCHITECTURE_MD).read_text(encoding="utf-8")
    docs = (root / DOCS_MD).read_text(encoding="utf-8")

    entries = read_claims(impl_source)
    labels = read_condition_labels(impl_source)
    claims: list[Claim] = []
    for key, variant in entries:
        if variant not in labels:
            raise ParseError(
                f"the condition variant `ClaimCondition::{variant}` has no label "
                "in `ClaimCondition::label`; the guard reads the condition's "
                "wording from there"
            )
        label = labels[variant]
        conditional = label is not None and normalize(label) != "always"
        claims.append(
            Claim(key=key, variant=variant, label=label, conditional=conditional)
        )

    wordings = {claim.label for claim in claims if claim.conditional}
    if not wordings:
        raise ParseError(
            "the claim table has no conditional claim, so there is no condition "
            "for the published surfaces to state"
        )
    if len(wordings) > 1:
        raise ParseError(
            "the claim table has more than one conditional wording: "
            + ", ".join(sorted(str(wording) for wording in wordings))
        )
    conditional_label = str(wordings.pop())

    arch_table = find_claim_table(architecture)
    arch_page = find_page_paragraph(architecture)
    docs_word_table = find_word_motion_table(docs)
    docs_key_table = find_keymap_table(docs)
    docs_jk = find_jk_paragraph(docs, docs_word_table)
    docs_page = find_docs_page_paragraph(docs, docs_jk)

    return Model(
        claims=tuple(claims),
        conditional_label=conditional_label,
        arch_table=arch_table,
        arch_page=arch_page,
        docs_word_table=docs_word_table,
        docs_key_table=docs_key_table,
        docs_jk=docs_jk,
        docs_page=docs_page,
    )


def table_letters(table: Table) -> frozenset[str]:
    """Every letter a table names."""
    found: set[str] = set()
    for row in table.rows:
        found |= letters_in(row.cells[0])
    return frozenset(found)


def conditional_row(
    table: Table, letters: frozenset[str]
) -> tuple[Row | None, list[str]]:
    """The row(s) that name the conditional letters, and the letters missing."""
    missing: list[str] = []
    rows: dict[int, Row] = {}
    for letter in sorted(letters):
        carrying = [
            row for row in table.rows if letter in letters_in(row.cells[0])
        ]
        if not carrying:
            missing.append(letter)
            continue
        for row in carrying:
            rows[row.line] = row
    if not rows:
        return None, missing
    return rows[min(rows)], missing


def analyse(model: Model) -> list[Violation]:
    """Every rule, against one model."""
    d1: list[Violation] = []
    d2: list[Violation] = []
    d3: list[Violation] = []
    d4: list[Violation] = []
    d5: list[Violation] = []

    label = model.conditional_label
    normalized_label = normalize(label)
    implemented = {claim.key for claim in model.claims}
    implemented_conditional = frozenset(
        claim.key for claim in model.claims if claim.conditional
    )

    # --- the published claim table -------------------------------------
    arch_cells: dict[str, tuple[str, int]] = {}
    arch_conditional: set[str] = set()
    for row in model.arch_table.rows:
        if len(row.cells) < 3:
            d5.append(
                Violation(
                    "D5",
                    f"{ARCHITECTURE_MD}: the claim table's row at line "
                    f"{row.line} has no Condition cell",
                )
            )
            continue
        letters = letters_in(row.cells[0])
        if len(letters) != 1:
            d5.append(
                Violation(
                    "D5",
                    f"{ARCHITECTURE_MD}: the claim table's row at line "
                    f"{row.line} does not name exactly one letter: "
                    f"{row.cells[0]!r}",
                )
            )
            continue
        letter = next(iter(letters))
        cell = row.cells[2]
        arch_cells[letter] = (cell, row.line)
        if not normalize(cell).startswith("always"):
            arch_conditional.add(letter)

    # --- Rule D1 — the letters the two do not share --------------------
    for name, published in (
        (f"{ARCHITECTURE_MD}'s claim table", frozenset(arch_cells)),
        (f"{DOCS_MD}'s `[NOR]`-mode keymap table", table_letters(model.docs_key_table)),
    ):
        for missing in sorted(implemented - published):
            d1.append(
                Violation(
                    "D1",
                    f"{name} does not name `{missing}`, which the claim table "
                    "claims",
                )
            )
        for extra in sorted(published - implemented):
            d1.append(
                Violation(
                    "D1",
                    f"{name} names `{extra}`, which the claim table does not "
                    "claim",
                )
            )

    # --- Rule D2 — the conditional letters ------------------------------
    for letter in sorted(implemented_conditional - frozenset(arch_conditional)):
        d2.append(
            Violation(
                "D2",
                f"{ARCHITECTURE_MD}'s claim table marks `{letter}` "
                "unconditional, but the implementation's claim is conditional",
            )
        )
    for letter in sorted(frozenset(arch_conditional) - implemented_conditional):
        d2.append(
            Violation(
                "D2",
                f"{ARCHITECTURE_MD}'s claim table marks `{letter}` conditional, "
                "but the implementation's claim is unconditional",
            )
        )

    # --- Rule D3 — the condition's wording ------------------------------
    for letter in sorted(implemented_conditional):
        entry = arch_cells.get(letter)
        if entry is None:
            d5.append(
                Violation(
                    "D5",
                    f"{ARCHITECTURE_MD}'s claim table has no row for the "
                    f"conditional claim `{letter}`",
                )
            )
            continue
        cell, line = entry
        if normalized_label not in normalize(cell):
            d3.append(
                Violation(
                    "D3",
                    f"{ARCHITECTURE_MD}: the claim table's Condition cell for "
                    f"`{letter}` (line {line}) says {cell!r}, not the "
                    f"implementation's condition {label!r}",
                )
            )

    # --- the published surfaces that carry the condition ----------------
    containment: list[Location] = [
        Location(
            "the page paragraph",
            ARCHITECTURE_MD,
            model.arch_page.line,
            model.arch_page.text,
        ),
        Location(
            "the paragraph about `j` and `k`",
            DOCS_MD,
            model.docs_jk.line,
            model.docs_jk.text,
        ),
        Location(
            "the paragraph about `Ctrl+u` and `Ctrl+d`",
            DOCS_MD,
            model.docs_page.line,
            model.docs_page.text,
        ),
    ]
    staleness: list[Location] = [
        Location(
            f"the claim table's Condition cell for `{letter}`",
            ARCHITECTURE_MD,
            line,
            cell,
        )
        for letter, (cell, line) in sorted(arch_cells.items())
    ]

    for name, table in (
        ("the word-motion table", model.docs_word_table),
        ("the `[NOR]`-mode keymap table", model.docs_key_table),
    ):
        row, missing = conditional_row(table, implemented_conditional)
        for letter in missing:
            d5.append(
                Violation(
                    "D5",
                    f"{DOCS_MD}'s {name} has no row for the conditional claim "
                    f"`{letter}`",
                )
            )
        if row is None:
            continue
        location = Location(
            f"{name}'s row for the conditional letters",
            DOCS_MD,
            row.line,
            " | ".join(row.cells),
        )
        staleness.append(location)
        containment.append(location)

    # --- Rule D4 — the prose, and the stale phrasings -------------------
    for location in containment:
        if not states_a_condition(location.text):
            continue
        if normalized_label not in normalize(location.text):
            d4.append(
                Violation(
                    "D4",
                    f"{location.path}: {location.name} (line {location.line}) "
                    f"states a condition that is not the implementation's "
                    f"{label!r}: {location.text!r}",
                )
            )
    for location in staleness:
        haystack = normalize(location.text)
        stale = [
            phrase for phrase in SUPERSEDED_PHRASINGS if phrase in haystack
        ]
        if not stale:
            continue
        d4.append(
            Violation(
                "D4",
                f"{location.path}: {location.name} (line {location.line}) "
                f"still says {', '.join(repr(phrase) for phrase in stale)}, "
                "which the correction supersedes",
            )
        )

    return d1 + d2 + d3 + d4 + d5


def check_tree(root: Path) -> list[Violation]:
    """Every rule, against the tree at `root`."""
    try:
        model = build_model(root)
    except (OSError, ParseError) as error:
        return [
            Violation(
                "D5",
                f"an input the guard needs cannot be found: {error}",
            )
        ]
    return analyse(model)


# ── The self-test ───────────────────────────────────────────────────────────

CLEAN_RS = """//! The composer's claim table.

/// When a claim is live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimCondition {
    /// The claim is always live.
    Always,
    /// The claim is live only when the composer's content spans more than one row.
    ContentOverflowsBox,
}

/// One entry of the composer's claim table.
pub struct ComposerClaim {
    pub key: char,
    pub condition: ClaimCondition,
}

/// The claims the composer owns.
pub const COMPOSER_CLAIMS: &[ComposerClaim] = &[
    ComposerClaim {
        key: 'a',
        condition: ClaimCondition::Always,
    },
    ComposerClaim {
        key: 'b',
        condition: ClaimCondition::Always,
    },
    ComposerClaim {
        key: 'j',
        condition: ClaimCondition::ContentOverflowsBox,
    },
    ComposerClaim {
        key: 'k',
        condition: ClaimCondition::ContentOverflowsBox,
    },
    ComposerClaim {
        key: 'w',
        condition: ClaimCondition::Always,
    },
];

impl ClaimCondition {
    /// The condition in the words the help and the hint row use.
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Always => None,
            Self::ContentOverflowsBox => Some("composer content spans more than one row"),
        }
    }
}
"""

CLEAN_ARCHITECTURE = """# The TUI

### The composer's claim table

| Letter | Verb | Condition | Lands with |
|---|---|---|---|
| `a` | insert after the caret | always | **live** |
| `b` | word back | always | **live** |
| `j` | line down | **composer content spans more than one row** | **live** |
| `k` | line up | **composer content spans more than one row** | **live** |
| `w` | word forward | always | **live** |

**`Ctrl+u` and `Ctrl+d` page the composer when the composer content spans more
than one row, and the transcript when it does not.**
"""

CLEAN_DOCS = """# The TUI

### Moving the caret in `[NOR]` mode

| Key | What it does |
|---|---|
| `h` | Move the caret one character left |

The word motions move it by word instead of by character:

| Key | What it does |
|---|---|
| `w` | Move to the start of the next word |
| `j` / `k` | Move down / up a row of the box, keeping the column |

`j` and `k` only move the caret when the composer content spans more than one
row. When the composer content spans one row, they scroll the conversation
instead.

`Ctrl+u` and `Ctrl+d` **page**. They page the **composer** when the composer
content spans more than one row, and the **conversation** when it does not.

### The composer's own keymap in `[NOR]` mode

| Key | What it does |
|---|---|
| `a` | Start typing after the character under the caret |
| `b` | Move to the start of the previous word |
| `j` / `k` | Move down / up a row of the box — only when the composer content spans more than one row |
| `w` | Move to the start of the next word |
"""

CLEAN_FILES = {
    IMPL_RS: CLEAN_RS,
    ARCHITECTURE_MD: CLEAN_ARCHITECTURE,
    DOCS_MD: CLEAN_DOCS,
}


@dataclass(frozen=True)
class Case:
    """One synthetic tree and what the guard must say about it."""

    name: str
    files: dict[str, str]
    rules: tuple[str, ...]
    note: str
    contains: tuple[str, ...] = ()


def _files(**changes: str) -> dict[str, str]:
    """The clean fixtures with the given replacements applied."""
    files = dict(CLEAN_FILES)
    files.update(changes)
    return files


SELF_TEST_CASES: list[Case] = [
    Case(
        name="a clean tree finds nothing",
        files=dict(CLEAN_FILES),
        rules=(),
        note="the published keymap and the claim table agree",
    ),
    Case(
        name="a letter the claim table has and the published table drops is rule D1",
        files=_files(
            **{
                ARCHITECTURE_MD: CLEAN_ARCHITECTURE.replace(
                    "| `a` | insert after the caret | always | **live** |\n", ""
                )
            }
        ),
        rules=("D1",),
        note="the published claim table does not name `a`",
        contains=("does not name `a`",),
    ),
    Case(
        name="a letter the published table has and the claim table does not is rule D1",
        files=_files(
            **{
                ARCHITECTURE_MD: CLEAN_ARCHITECTURE.replace(
                    "| `w` | word forward | always | **live** |",
                    "| `w` | word forward | always | **live** |\n"
                    "| `z` | select all | always | **live** |",
                )
            }
        ),
        rules=("D1",),
        note="the published claim table names `z`",
        contains=("names `z`",),
    ),
    Case(
        name="a published condition the implementation does not have is rule D2",
        files=_files(
            **{
                ARCHITECTURE_MD: CLEAN_ARCHITECTURE.replace(
                    "| `a` | insert after the caret | always | **live** |",
                    "| `a` | insert after the caret | **composer content spans "
                    "more than one row** | **live** |",
                )
            }
        ),
        rules=("D2",),
        note="the published table marks `a` conditional",
        contains=("marks `a` conditional",),
    ),
    Case(
        name="a conditional claim the published table calls unconditional is rule D2",
        files=_files(
            **{
                ARCHITECTURE_MD: CLEAN_ARCHITECTURE.replace(
                    "| `k` | line up | **composer content spans more than one "
                    "row** | **live** |",
                    "| `k` | line up | always | **live** |",
                )
            }
        ),
        rules=("D2", "D3"),
        note="the published table marks `k` unconditional",
        contains=("marks `k` unconditional",),
    ),
    Case(
        name="a published condition in other words is rule D3",
        files=_files(
            **{
                ARCHITECTURE_MD: CLEAN_ARCHITECTURE.replace(
                    "| `j` | line down | **composer content spans more than one "
                    "row** | **live** |",
                    "| `j` | line down | **composer content overflows its box** | "
                    "**live** |",
                )
            }
        ),
        rules=("D3",),
        note="the published cell does not carry the implementation's wording",
        contains=(
            "composer content spans more than one row",
            "composer content overflows its box",
        ),
    ),
    Case(
        name="published prose that states another rule is rule D4",
        files=_files(
            **{
                DOCS_MD: CLEAN_DOCS.replace(
                    "`j` and `k` only move the caret when the composer content "
                    "spans more than one\nrow.",
                    "`j` and `k` only move the caret when the prompt does not "
                    "fit its box.",
                )
            }
        ),
        rules=("D4",),
        note="the paragraph does not carry the implementation's wording",
        contains=(
            "composer content spans more than one row",
            "does not fit its box",
        ),
    ),
    Case(
        name="a published surface that still says the superseded rule is rule D4",
        files=_files(
            **{
                DOCS_MD: CLEAN_DOCS.replace(
                    "| `j` / `k` | Move down / up a row of the box — only when "
                    "the composer content spans more than one row |",
                    "| `j` / `k` | Move down / up a row of the box — only when "
                    "the composer content spans more than one row (the prompt "
                    "does not fit its box) |",
                )
            }
        ),
        rules=("D4",),
        note="the keymap table still carries the superseded phrasing",
        contains=("still says", "does not fit its box"),
    ),
    Case(
        name="a table the guard needs but cannot find is rule D5",
        files=_files(
            **{
                ARCHITECTURE_MD: CLEAN_ARCHITECTURE.replace(
                    "| Letter | Verb | Condition | Lands with |",
                    "| Key | Verb | Condition | Lands with |",
                )
            }
        ),
        rules=("D5",),
        note="the published claim table is not where the guard looks for it",
        contains=("claim table was not found",),
    ),
    Case(
        name="a row the guard needs but cannot find is rule D5",
        files=_files(
            **{
                DOCS_MD: CLEAN_DOCS.replace(
                    "| `j` / `k` | Move down / up a row of the box, keeping the "
                    "column |\n",
                    "",
                )
            }
        ),
        rules=("D5",),
        note="the word-motion table has no row for the conditional claim",
        contains=("no row for the conditional claim",),
    ),
]


def run_self_test() -> int:
    """Run every case; report the ones that disagree with the expectation."""
    failures = 0
    with tempfile.TemporaryDirectory(prefix="codegg-composer-keymap-drift-") as raw:
        root = Path(raw)
        for index, case in enumerate(SELF_TEST_CASES):
            tree = root / f"case-{index}"
            for relative, content in case.files.items():
                path = tree / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content, encoding="utf-8")
            violations = check_tree(tree)
            rules = tuple(sorted({violation.rule for violation in violations}))
            if rules != tuple(sorted(case.rules)):
                failures += 1
                print(
                    f"self-test: {case.name}\n"
                    f"  expected rules {sorted(case.rules)}, got {rules}\n"
                    + "\n".join(
                        f"  {violation.rule}: {violation.message}"
                        for violation in violations
                    ),
                    file=sys.stderr,
                )
                continue
            missing = [
                needle
                for needle in case.contains
                if not any(needle in violation.message for violation in violations)
            ]
            if missing:
                failures += 1
                print(
                    f"self-test: {case.name}\n"
                    f"  no finding names {missing}\n"
                    + "\n".join(
                        f"  {violation.rule}: {violation.message}"
                        for violation in violations
                    ),
                    file=sys.stderr,
                )
                continue
            print(f"self-test: {case.name} — {case.note}")
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
        "--root",
        type=Path,
        default=REPO_ROOT,
        help="the tree to check (default: the repository this script lives in)",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run the synthetic cases instead of checking the tree",
    )
    args = parser.parse_args(argv)
    if args.self_test:
        return run_self_test()
    violations = check_tree(args.root)
    if violations:
        for violation in violations:
            print(
                f"{GUARD}: rule {violation.rule}: {violation.message}",
                file=sys.stderr,
            )
        print(f"{GUARD}: {len(violations)} violation(s)", file=sys.stderr)
        return 1
    print(
        f"{GUARD}: the published composer keymap and the implemented claim "
        "table agree"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
