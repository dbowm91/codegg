#!/usr/bin/env python3
"""An escaped defect has a record, and the record names a guard that exists.

`docs/postmortems/` carries one record per defect that escaped the evidence
that existed when it was written. A guard exists because something got past the
suite; the reason does not live in the guard, because the comment budget bounds
how much a guard may say, and it does not live in a closure record, because a
closure record answers "did this milestone land?". It lives here.

That only works if the record cannot rot into a plausible document. So this
guard is deliberately not a format checker alone. It answers four questions for
every record:

1. **Is it shaped like a record?** A name matching `NNNN-<kebab-slug>.md`, a
   unique number in a numbering that has no gaps, and all eight declared
   sections in order with a body in each.
2. **Does the content carry facts?** `Status` names a declared state, `Date` is
   a parseable date, and `Reproduction` is a command or a test path rather than
   prose.
3. **Does the guard it names exist?** A `scripts/` path resolves and carries a
   `--self-test`; a `<path>.rs::<test fn>` claim resolves to a test function
   that is actually defined in that file, per `scripts/rust_test_scan.py`'s
   inventory. A record that names a test instead of a guard script is allowed
   because a test is sometimes the guard — but only when it exists.
4. **Does the named guard mention the defect?** Every `Must mention:` token on a
   claim must appear in the named guard — in a script's text, or in the named
   test function's own body. This is the rule that makes the guard unsatisfiable
   by form: a plausible-looking path with nothing to do with the defect fails,
   because the guard cannot have deleted every reference to the thing it
   prevents.

Rust is never scanned with a regex. The test inventory comes from
`scripts/rust_test_scan.py`, the shared Rust-aware scanner, which is also what
keeps this guard from becoming a second Rust parser; a regex over Rust misreads
lifetimes as character literals and undercounts `tests/` badly.

Modes:
    python3 scripts/check_postmortems.py             # static guard
    python3 scripts/check_postmortems.py --self-test # non-vacuity

`--self-test` copies the tree, injects one defect of each class it is meant to
catch, and requires every one of them to be rejected — with the reason the
class is supposed to produce — while accepting the unmodified copy.
"""

from __future__ import annotations

import argparse
import datetime
import re
import shutil
import sys
import tempfile
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import rust_test_scan as rts  # noqa: E402

RECORD_DIR = "docs/postmortems"
README_REL = f"{RECORD_DIR}/README.md"
REGISTRY_REL = "plans/registry.md"

# The record's sections, in order. A section outside this tuple is a deliberate
# edit to this guard, never a side effect of one record: the convention says
# eight, and a ninth that only one record carries is how a template decays.
DECLARED_SECTIONS = (
    "Status",
    "Date",
    "Symptom",
    "Reproduction",
    "Root cause",
    "Why the existing evidence missed it",
    "Guard added",
    "Blast radius",
)

# The README's template block, delimited so the guard can compare it against
# `DECLARED_SECTIONS` instead of trusting that the two were edited together.
TEMPLATE_START = "<!-- template:start -->"
TEMPLATE_END = "<!-- template:end -->"

# The one sentence that separates this directory from `plans/closure/`.
BOUNDARY_SENTENCE = (
    "a closure record says what a milestone built; a postmortem says what "
    "escaped and what now prevents it."
)

# `NNNN-<kebab-slug>.md`. Four digits exactly, so the sort order is the
# numbering order and a three-digit record cannot hide.
RECORD_NAME = re.compile(r"^(?P<number>\d{4})-(?P<slug>[a-z0-9]+(?:-[a-z0-9]+)*)\.md$")

DATE_TOKEN = re.compile(r"^\d{4}-\d{2}-\d{2}$")

HEADING = re.compile(r"^## (?P<name>[^#].*?)\s*$")

FENCE = "```"

STATUS_STATES = ("fixed", "mitigated", "open")

# A `Self-test:` field that says nothing. The field exists to state the
# injection; these values are the shapes that pass a "non-empty" check while
# stating nothing.
VAGUE_SELF_TEST = {"n/a", "na", "none", "nil", "tbd", "todo", "unknown", "-", "—"}

CREDENTIAL_PATTERNS = (
    ("sk-", re.compile(r"sk-[A-Za-z0-9_-]{16,}")),
    ("aws-access-key", re.compile(r"AKIA[0-9A-Z]{16}")),
    ("github-token", re.compile(r"gh[pousr]_[A-Za-z0-9]{20,}")),
    ("slack-token", re.compile(r"xox[baprs]-[A-Za-z0-9-]{10,}")),
    ("private-key", re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----")),
)

# A `Guard:` bullet and the two indented fields that make it a claim. The
# indented form keeps the bullet readable in the rendered document, where the
# fields read as its continuation.
GUARD_BULLET = re.compile(r"^-\s+Guard:\s*(?P<rest>.*)$")
CLAIM_FIELD = re.compile(r"^\s+(?P<field>Self-test|Must mention):\s*(?P<rest>.*)$")
BACKTICKED = re.compile(r"`([^`]+)`")

# A record shorter than this has no body worth reading. It is a floor against
# an empty file, not a length policy.
MIN_RECORD_CHARS = 300


# ---------------------------------------------------------------------------
# Reading a record
# ---------------------------------------------------------------------------


def _normalize(text: str) -> str:
    """Lowercase and collapse whitespace, so a wrapped sentence still matches."""
    return re.sub(r"\s+", " ", text).strip().lower()


def _sections(text: str) -> list[tuple[str, str]]:
    """Every `##` section and its body, ignoring headings inside a fence."""
    sections: list[tuple[str, list[str]]] = []
    current: list[str] | None = None
    in_fence = False
    for line in text.splitlines():
        if line.strip().startswith(FENCE):
            in_fence = not in_fence
            if current is not None:
                current.append(line)
            continue
        heading = None if in_fence else HEADING.match(line)
        if heading is not None:
            current = []
            sections.append((heading.group("name").strip(), current))
            continue
        if current is not None:
            current.append(line)
    return [(name, "\n".join(body)) for name, body in sections]


def _template_sections(readme: str) -> list[str] | None:
    """The `##` headings the README's template block declares, or None."""
    start = readme.find(TEMPLATE_START)
    end = readme.find(TEMPLATE_END)
    if start < 0 or end < 0 or end < start:
        return None
    block = readme[start + len(TEMPLATE_START) : end]
    names: list[str] = []
    for line in block.splitlines():
        heading = HEADING.match(line)
        if heading is not None:
            names.append(heading.group("name").strip())
    return names


def _claims(section_body: str) -> list[dict[str, Any]]:
    """The `Guard:` claims in a `Guard added` body."""
    entries: list[dict[str, Any]] = []
    current: dict[str, Any] | None = None
    for line in section_body.splitlines():
        bullet = GUARD_BULLET.match(line)
        if bullet is not None:
            tokens = BACKTICKED.findall(bullet.group("rest"))
            current = {
                "claim": tokens[0] if tokens else "",
                "self_test": "",
                "tokens": [],
            }
            entries.append(current)
            continue
        field = CLAIM_FIELD.match(line)
        if field is None or current is None:
            continue
        if field.group("field") == "Self-test":
            current["self_test"] = field.group("rest").strip()
        else:
            current["tokens"] = BACKTICKED.findall(field.group("rest"))
    return entries


# ---------------------------------------------------------------------------
# Checking one claim against the tree
# ---------------------------------------------------------------------------


def _read_text(path: Path) -> str | None:
    try:
        return path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return None


def _check_claim(root: Path, rel: str, entry: dict[str, Any]) -> list[str]:
    """Resolve one claim and check the defect reference it asserts."""
    violations: list[str] = []
    claim = entry["claim"].strip()
    tokens = list(entry["tokens"])
    self_test = entry["self_test"].strip()

    if not claim:
        return [f"{rel}: a `Guard:` bullet names no backticked path"]
    if not self_test or self_test.lower().strip(".,;:") in VAGUE_SELF_TEST:
        violations.append(
            f"{rel}: the claim on `{claim}` states no self-test injection; "
            "`Self-test:` must name the injection that makes this claim fail"
        )
    if not tokens:
        violations.append(
            f"{rel}: the claim on `{claim}` declares no `Must mention:` token; "
            "a claim that names no defect can never be contradicted"
        )

    if "::" in claim:
        file_rel, _, test_name = claim.rpartition("::")
        if not file_rel.endswith(".rs"):
            return violations + [
                f"{rel}: `{claim}` names a test outside a Rust source file; "
                "a test claim is `<repo path>.rs::<test fn>`"
            ]
        path = root / file_rel
        text = _read_text(path)
        if text is None:
            return violations + [
                f"{rel}: `{claim}` does not exist — `{file_rel}` is not a file "
                "in this tree, so the record names a guard nothing runs"
            ]
        tree = file_rel.split("/", 1)[0]
        functions = rts.find_test_functions(text, path, file_rel, tree)
        matches = [function for function in functions if function.name == test_name]
        if not matches:
            known = ", ".join(sorted(function.name for function in functions)[:3])
            return violations + [
                f"{rel}: `{claim}` names no test function in `{file_rel}`; the "
                f"scanner finds {len(functions)} test(s) there"
                + (f" (for example {known})" if known else "")
            ]
        body = matches[0].body_text
        for token in tokens:
            if token not in body:
                violations.append(
                    f"{rel}: the test named by `{claim}` does not mention "
                    f"`{token}`, so it cannot be the guard for this defect"
                )
        return violations

    if not claim.startswith("scripts/"):
        return violations + [
            f"{rel}: `{claim}` must name a path under scripts/ or a "
            "`<repo path>.rs::<test fn>` test; a guard is one or the other"
        ]

    path = root / claim
    text = _read_text(path)
    if text is None:
        return violations + [
            f"{rel}: `{claim}` does not exist — the record names a guard "
            "nothing runs"
        ]
    if "--self-test" not in text:
        violations.append(
            f"{rel}: `{claim}` does not carry a `--self-test`, so the record's "
            "claim that it bites is unproven"
        )
    for token in tokens:
        if token not in text:
            violations.append(
                f"{rel}: `{claim}` does not mention `{token}`, so it cannot be "
                "the guard for this defect"
            )
    return violations


# ---------------------------------------------------------------------------
# The README, the registry, and the numbering
# ---------------------------------------------------------------------------


def _check_readme(root: Path) -> list[str]:
    text = _read_text(root / README_REL)
    if text is None:
        return [
            f"{README_REL}: does not exist; the convention must be written "
            "down before a record can be an instance of it"
        ]
    violations: list[str] = []
    template = _template_sections(text)
    if template is None:
        violations.append(
            f"{README_REL}: the template block is not delimited by "
            f"`{TEMPLATE_START}` and `{TEMPLATE_END}`"
        )
    elif tuple(template) != DECLARED_SECTIONS:
        violations.append(
            f"{README_REL}: the template lists {template} but the declared "
            f"sections are {list(DECLARED_SECTIONS)}"
        )
    if _normalize(BOUNDARY_SENTENCE) not in _normalize(text):
        violations.append(
            f"{README_REL}: the boundary sentence against `plans/closure/` is "
            "missing; without it the two records look like the same document"
        )
    if Path(__file__).name not in text:
        violations.append(
            f"{README_REL}: does not name `{Path(__file__).name}`, the guard "
            "that enforces it"
        )
    return violations


def _check_registry(root: Path) -> list[str]:
    """A postmortem is not a plan row."""
    text = _read_text(root / REGISTRY_REL)
    if text is None:
        return []
    found = sorted(set(re.findall(r"docs/postmortems/[0-9A-Za-z._-]+\.md", text)))
    return [
        f"{REGISTRY_REL}: names `{item}`; a postmortem is not a plan row and "
        "does not belong in the registry"
        for item in found
    ]


def _record_paths(root: Path) -> list[Path]:
    directory = root / RECORD_DIR
    if not directory.is_dir():
        return []
    return sorted(
        path
        for path in directory.glob("*.md")
        if path.is_file() and path.name != "README.md"
    )


# ---------------------------------------------------------------------------
# One record
# ---------------------------------------------------------------------------


def _check_record(root: Path, path: Path, rel: str) -> list[str]:
    text = _read_text(path)
    if text is None:
        return [f"{rel}: is not readable UTF-8 text"]

    violations: list[str] = []
    if len(text.strip()) < MIN_RECORD_CHARS:
        return [f"{rel}: is empty; a record that says nothing guards nothing"]

    for label, pattern in CREDENTIAL_PATTERNS:
        if pattern.search(text):
            violations.append(
                f"{rel}: contains a credential-shaped string ({label}); a "
                "postmortem quotes a real defect and must not quote a secret"
            )
            break

    if not any(line.startswith("# ") for line in text.splitlines()):
        violations.append(f"{rel}: has no `# ` title")

    sections = _sections(text)
    names = [name for name, _ in sections]
    repeated = sorted({name for name in names if names.count(name) > 1})
    for name in repeated:
        violations.append(f"{rel}: declares the section `{name}` more than once")
    missing = [name for name in DECLARED_SECTIONS if name not in names]
    if missing:
        violations.append(
            f"{rel}: missing required section(s) {missing}; the convention "
            "declares exactly eight"
        )
    unexpected = [name for name in names if name not in DECLARED_SECTIONS]
    if unexpected:
        violations.append(
            f"{rel}: the section(s) {unexpected} are not declared; adding one "
            "is an edit to `scripts/check_postmortems.py`, not to a record"
        )
    ordered = [name for name in names if name in DECLARED_SECTIONS]
    expected_order = [name for name in DECLARED_SECTIONS if name in ordered]
    if not repeated and not missing and not unexpected and ordered != expected_order:
        violations.append(
            f"{rel}: the sections are out of order; expected "
            f"{expected_order}, found {ordered}"
        )

    bodies = {name: body for name, body in sections}
    for name in DECLARED_SECTIONS:
        body = bodies.get(name)
        if body is not None and not body.strip():
            violations.append(f"{rel}: the section `{name}` is empty")

    status = bodies.get("Status", "")
    first = status.strip().split()[0] if status.strip() else ""
    state = first.strip(".,;:*_`").lower()
    if status.strip() and state not in STATUS_STATES:
        violations.append(
            f"{rel}: `Status` says `{first}`, which is not one of "
            f"{list(STATUS_STATES)}"
        )

    date = bodies.get("Date", "")
    first_date = date.strip().splitlines()[0].strip() if date.strip() else ""
    if date.strip():
        if not DATE_TOKEN.match(first_date):
            violations.append(
                f"{rel}: `Date` starts with `{first_date}`, not `YYYY-MM-DD`"
            )
        else:
            try:
                datetime.date.fromisoformat(first_date)
            except ValueError:
                violations.append(
                    f"{rel}: `Date` is `{first_date}`, which is not a real date"
                )

    reproduction = bodies.get("Reproduction", "")
    if reproduction.strip():
        has_block = FENCE in reproduction
        has_test_path = any("::" in token for token in BACKTICKED.findall(reproduction))
        if not (has_block or has_test_path):
            violations.append(
                f"{rel}: `Reproduction` is prose; a record must carry a command "
                "in a fenced block or a backticked test path"
            )

    if "Guard added" in bodies and bodies["Guard added"].strip():
        claims = _claims(bodies["Guard added"])
        if not claims:
            violations.append(
                f"{rel}: `Guard added` names no guard; a record that names "
                "nothing that prevents the defect is a resolution"
            )
        for entry in claims:
            violations.extend(_check_claim(root, rel, entry))

    return violations


# ---------------------------------------------------------------------------
# The tree
# ---------------------------------------------------------------------------


def check_tree(root: Path) -> list[str]:
    """Every postmortem violation in `root`."""
    root = root.resolve()
    violations: list[str] = _check_readme(root)
    violations.extend(_check_registry(root))

    records = _record_paths(root)
    if not records:
        violations.append(
            f"{RECORD_DIR}: holds no record; with nothing to check the "
            "convention is unenforced"
        )

    numbers: dict[int, list[str]] = {}
    named: list[tuple[Path, str, int]] = []
    for path in records:
        rel = path.relative_to(root).as_posix()
        match = RECORD_NAME.match(path.name)
        if match is None:
            violations.append(
                f"{rel}: the name does not match NNNN-<kebab-slug>.md; four "
                "digits keep the numbering sortable and unique"
            )
            continue
        number = int(match.group("number"))
        numbers.setdefault(number, []).append(rel)
        named.append((path, rel, number))

    for number, files in sorted(numbers.items()):
        if len(files) > 1:
            violations.append(
                f"{files[0]}: duplicate number {number:04d}, also used by "
                f"{', '.join(files[1:])}; a number is never reused"
            )
    if numbers:
        for missing in sorted(set(range(1, max(numbers) + 1)) - set(numbers)):
            violations.append(
                f"{RECORD_DIR}: gap in numbering at {missing:04d}; a deleted "
                "record consumes its number, so the gap is the record that "
                "went missing"
            )

    for path, rel, _number in named:
        violations.extend(_check_record(root, path, rel))
    return violations


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------


def _claimed_files(root: Path) -> list[str]:
    """Every file path the records' claims name, so a copy can resolve them."""
    files: list[str] = []
    for path in _record_paths(root):
        text = _read_text(path) or ""
        for name, body in _sections(text):
            if name != "Guard added":
                continue
            for entry in _claims(body):
                claim = str(entry["claim"])
                files.append(claim.rpartition("::")[0] if "::" in claim else claim)
    return files


def _copy_tree(root: Path, destination: Path) -> Path:
    """Copy what the guard reads into a scratch directory."""
    root = root.resolve()
    shutil.copytree(root / RECORD_DIR, destination / RECORD_DIR)
    registry = root / REGISTRY_REL
    if registry.is_file():
        (destination / REGISTRY_REL).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(registry, destination / REGISTRY_REL)
    shutil.copytree(
        root / "scripts",
        destination / "scripts",
        ignore=shutil.ignore_patterns("__pycache__", "*.pyc"),
    )
    for rel in _claimed_files(root):
        source = root / rel
        if not source.is_file():
            continue
        target = destination / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
    return destination


def _only_record(root: Path) -> Path:
    records = _record_paths(root)
    if len(records) != 1:
        raise SystemExit(f"self-test expects one record, found {len(records)}")
    return records[0]


SYNTHETIC_RECORD = """# {number:04d} — A synthetic record for the self-test

## Status

Fixed.

## Date

2026-01-01

## Symptom

A synthetic symptom, long enough that the record clears the emptiness floor the
guard applies before it reads anything else.

## Reproduction

```bash
cargo test -p codegg-protocol --lib
```

## Root cause

A synthetic root cause.

## Why the existing evidence missed it

A synthetic account of the evidence that missed it.

## Guard added

- Guard: `{claim}`
  Self-test: strips the marker out of a copy of the tree.
  Must mention: `{token}`

## Blast radius

A synthetic blast radius.
"""


def _rewrite(path: Path, transform) -> None:
    text = path.read_text(encoding="utf-8")
    updated = transform(text)
    if updated == text:
        raise SystemExit(f"self-test injection changed nothing in {path}")
    path.write_text(updated, encoding="utf-8")


def _replace_first_claim(claim: str):
    def transform(text: str) -> str:
        return re.sub(
            r"(^-\s+Guard:\s*)`[^`]+`",
            lambda match: f"{match.group(1)}`{claim}`",
            text,
            count=1,
            flags=re.MULTILINE,
        )

    return transform


def _self_test() -> int:
    """Prove the guard rejects each defect class and accepts the clean tree."""
    failures: list[str] = []

    with tempfile.TemporaryDirectory(prefix="codegg-postmortem-") as raw:
        clean = _copy_tree(ROOT, Path(raw))
        clean_violations = check_tree(clean)
        if clean_violations:
            for violation in clean_violations:
                print(f"ERROR: {violation}", file=sys.stderr)
            print(
                "ERROR: the unmodified copy must pass the guard",
                file=sys.stderr,
            )
            return 1
        print("  accepted the unmodified copy")

    def missing_section(copy: Path) -> None:
        _rewrite(
            _only_record(copy),
            lambda text: re.sub(
                r"\n## Blast radius\n.*\Z", "\n", text, flags=re.DOTALL
            ),
        )

    def dangling_script(copy: Path) -> None:
        _rewrite(
            _only_record(copy),
            _replace_first_claim("scripts/check_no_such_guard_exists.py"),
        )

    def guard_without_self_test(copy: Path) -> None:
        claim = "scripts/check_test_self_reexec_paths.py"
        record = copy / RECORD_DIR / "0002-synthetic-guard.md"
        record.write_text(
            SYNTHETIC_RECORD.format(
                number=2, claim=claim, token="self-reexec"
            ),
            encoding="utf-8",
        )
        _rewrite(
            copy / claim,
            lambda text: text.replace("--self-test", "self-test"),
        )

    def duplicate_number(copy: Path) -> None:
        record = _only_record(copy)
        shutil.copy2(record, record.with_name("0001-duplicate-record.md"))

    def vanished_test(copy: Path) -> None:
        record = _only_record(copy)
        claim = re.search(r"^-\s+Guard:\s*`([^`]+)`", record.read_text(encoding="utf-8"), re.MULTILINE)
        if claim is None:
            raise SystemExit("self-test found no Guard claim to mutate")
        file_rel = claim.group(1).rpartition("::")[0]
        _rewrite(
            record,
            _replace_first_claim(f"{file_rel}::no_such_test_exists_here"),
        )

    def silent_guard(copy: Path) -> None:
        record = _only_record(copy)
        _rewrite(
            record,
            lambda text: re.sub(
                r"^(\s+Must mention:).*$",
                r"\1 `no_such_defect_token_appears_anywhere`",
                text,
                count=1,
                flags=re.MULTILINE,
            ),
        )

    def malformed_name(copy: Path) -> None:
        record = _only_record(copy)
        shutil.copy2(record, copy / RECORD_DIR / "1-not-four-digits.md")

    def numbering_gap(copy: Path) -> None:
        record = copy / RECORD_DIR / "0003-synthetic-gap.md"
        record.write_text(
            SYNTHETIC_RECORD.format(
                number=3,
                claim="scripts/check_postmortems.py",
                token="credential-shaped",
            ),
            encoding="utf-8",
        )

    def silent_script(copy: Path) -> None:
        # The claimed guard is a *different* script on purpose: a token written
        # literally into this file appears in this file, so claiming *this*
        # guard with an absent token would prove nothing.
        record = copy / RECORD_DIR / "0002-synthetic-silent.md"
        record.write_text(
            SYNTHETIC_RECORD.format(
                number=2,
                claim="scripts/check_test_self_reexec_paths.py",
                token="credential-shaped",
            ),
            encoding="utf-8",
        )

    def credential(copy: Path) -> None:
        _rewrite(
            _only_record(copy),
            lambda text: text.replace(
                "A stored transcript containing a tool call",
                "A stored transcript containing a tool call sk-proj-0123456789abcdefghijklmn",
                1,
            ),
        )

    def empty_record(copy: Path) -> None:
        _only_record(copy).write_text("# 0001 — placeholder\n", encoding="utf-8")

    def not_a_guard(copy: Path) -> None:
        _rewrite(
            _only_record(copy),
            _replace_first_claim("docs/postmortems/README.md"),
        )

    def template_drift(copy: Path) -> None:
        _rewrite(
            copy / README_REL,
            lambda text: text.replace("\n## Blast radius\n", "\n", 1),
        )

    def boundary_removed(copy: Path) -> None:
        _rewrite(
            copy / README_REL,
            lambda text: text.replace(
                "A closure record says what a milestone built; a postmortem says what escaped\n"
                "and what now prevents it. ",
                "",
                1,
            ),
        )

    def out_of_order(copy: Path) -> None:
        _rewrite(
            _only_record(copy),
            lambda text: text.replace("## Status\n", "@@swap@@")
            .replace("## Date\n", "## Status\n")
            .replace("@@swap@@", "## Date\n"),
        )

    def undeclared_section(copy: Path) -> None:
        _rewrite(
            _only_record(copy),
            lambda text: text + "\n## Timeline\n\nA section nobody declared.\n",
        )

    cases = [
        ("a record missing a required section", missing_section, "missing required section"),
        ("sections out of the declared order", out_of_order, "out of order"),
        ("a section nobody declared", undeclared_section, "are not declared"),
        ("a claim on a script that does not exist", dangling_script, "does not exist"),
        ("a claimed guard without --self-test", guard_without_self_test, "--self-test"),
        ("a duplicated number", duplicate_number, "duplicate number"),
        ("a claim on a test function that does not exist", vanished_test, "names no test function"),
        ("a claim whose guard never mentions the defect", silent_guard, "does not mention"),
        ("a script claim that never mentions the defect", silent_script, "does not mention"),
        ("a malformed record name", malformed_name, "does not match"),
        ("a gap in the numbering", numbering_gap, "gap in numbering"),
        ("a credential-shaped string in a record", credential, "credential-shaped"),
        ("an empty record", empty_record, "is empty"),
        ("a claim on something that is not a guard", not_a_guard, "must name a path under scripts/"),
        ("template drift between the README and the guard", template_drift, "template"),
        ("the boundary sentence removed from the README", boundary_removed, "boundary sentence"),
    ]

    for label, mutate, expected in cases:
        with tempfile.TemporaryDirectory(prefix="codegg-postmortem-case-") as raw:
            copy = _copy_tree(ROOT, Path(raw))
            mutate(copy)
            violations = check_tree(copy)
            if not violations:
                failures.append(f"the guard did not reject {label}")
                continue
            if not any(expected in violation for violation in violations):
                failures.append(
                    f"the guard rejected {label} for the wrong reason: "
                    f"expected {expected!r}, got {violations[0]!r}"
                )
                continue
            print(f"  rejected {label}: {violations[0]}")

    if failures:
        for failure in failures:
            print(f"ERROR: {failure}", file=sys.stderr)
        return 1
    print(f"postmortem guard self-test passed ({len(cases)} injections rejected)")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects each defect class it is meant to catch",
    )
    args = parser.parse_args()
    if args.self_test:
        return _self_test()
    violations = check_tree(ROOT)
    if violations:
        for violation in violations:
            print(f"ERROR: {violation}", file=sys.stderr)
        print(f"{len(violations)} postmortem violation(s)", file=sys.stderr)
        return 1
    print("postmortem guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
