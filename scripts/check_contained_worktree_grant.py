#!/usr/bin/env python3
"""Guard: the contained worktree grant and ADR-0048 must name the same path.

WHAT THIS GUARD DOES
    ADR-0048 (accepted) grants the contained managed-worktree creation child a
    writable set of exactly three roots: the checkout, the daemon scratch and
    ``<common-gitdir>/worktrees/``. The 2026-10-23 implementation review found
    that five artifacts described a **narrower** grant than the code delivers:
    ADR-0048's clause 3, two comments in ``src/git_contained_worktree.rs`` and
    the containment test in ``tests/contained_worktree_c008.rs`` all called the
    shared root "the linked worktree's own administration directory". A
    described grant that is narrower than the delivered one is a false
    security claim, and the test that was supposed to catch it was one of the
    five.

    This guard is the backstop that keeps the descriptions and the code from
    diverging again:

    1. ADR-0048's clause 3 must name **one** ``<common-gitdir>`` path, and it
       must be described as the **shared** administration root — the parent of
       every linked worktree's own administration directory. The appositive
       that describes the path must not call it the worktree's own directory.
    2. The grant must produce **exactly one** writable root under the
       repository's Git directory, and that root must be the path ADR-0048
       names. The comparison is **derived from the ADR's text**, not from a
       copy of the path: the ADR's ``<common-gitdir>/...`` span is turned into
       its component path, and the code must build its administration root
       from exactly those components.
    3. The two code comments that describe the grant must name the same path
       the ADR names, and must call it shared.
    4. The containment test that asserts the grant must be **named** for the
       shared root. The original defect included a test named after the linked
       worktree's own administration directory — the test whose job is to
       assert containment described a narrower grant than the code delivers.
    5. The structural refusal in ``assert_grant`` — every write root under the
       Git directory that is not the administration root is refused — must
       still be there. That refusal is what makes claim 2 an enforced
       property rather than a comment.

WHAT THIS GUARD DELIBERATELY DOES NOT DO
    It does not decide whether the shared root's reach over other linked
    worktrees' ``HEAD`` files is acceptable. That is an **open capability
    question** recorded in ADR-0048's amendment; the maintainer has not chosen
    a disposition, and this guard does not choose one either.

    It does not narrow the grant. Narrowing the accepted path to the linked
    worktree's own administration directory is a **new ADR**.

    It does not prove that the child obtained the grant at runtime. That is
    the launch's own assertion (``obtained_write_root_is_present``) and the
    hosted containment lane's job.

USAGE
    python3 scripts/check_contained_worktree_grant.py            # check the tree
    python3 scripts/check_contained_worktree_grant.py --self-test  # prove it bites
"""

from __future__ import annotations

import argparse
import re
import shutil
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

ADR_REL = "plans/adrs/ADR-0048-contained-git-execution-for-managed-worktree-creation.md"
SOURCE_REL = "src/git_contained_worktree.rs"
TEST_REL = "tests/contained_worktree_c008.rs"

# The containment test that asserts the grant must be named for the shared
# root. The original defect was a test named after the linked worktree's own
# administration directory: the test whose job is to assert containment
# described a narrower grant than the code delivers.
REQUIRED_TEST_NAME = "shared_administration_root"
FORBIDDEN_TEST_NAME = "only_the_administration_directory"

# The ADR names its Git-directory paths with this symbolic prefix. The guard
# derives the concrete path from the components that follow it.
COMMON_GITDIR = "<common-gitdir>"

# The two code comments that describe the grant, and the function each one
# documents. Both are guarded by the signature they precede.
COMMENT_SIGNATURES: tuple[tuple[str, str], ...] = (
    ("build_worktree_grant's comment", "pub fn build_worktree_grant("),
    ("assert_grant's comment", "fn assert_grant("),
)

# The structural refusal that keeps every other path under the repository's
# Git directory read-only to the child. Compared with the whitespace removed,
# so a rustfmt reflow cannot hide it.
REFUSAL_MARKER = "write.starts_with(gitdir)&&write!=administration_root"


class GuardError(Exception):
    """The guard cannot read what it has to compare against."""


# --- Reading the ADR -------------------------------------------------------


def _section_bounds(text: str, heading: str) -> tuple[int, int]:
    """The character range of a ``## `` section, matched on the whole heading."""
    lines = text.splitlines(keepends=True)
    offsets: list[int] = []
    offset = 0
    for line in lines:
        offsets.append(offset)
        offset += len(line)
    start = None
    for index, line in enumerate(lines):
        if line.strip() == heading:
            start = offsets[index]
            break
    if start is None:
        raise GuardError(f"ADR-0048 has no '{heading}' section")
    end = len(text)
    for index, line in enumerate(lines):
        if offsets[index] <= start:
            continue
        if line.startswith("## "):
            end = offsets[index]
            break
    return start, end


def _clause_bounds(text: str, number: int) -> tuple[int, int]:
    """The character range of a numbered clause inside the Decision section."""
    section_start, section_end = _section_bounds(text, "## Decision")
    section = text[section_start:section_end]
    match = re.compile(rf"^{number}\.\s", re.MULTILINE).search(section)
    if match is None:
        raise GuardError(
            f"ADR-0048's Decision section has no numbered clause {number}"
        )
    following = re.compile(r"^\d+\.\s", re.MULTILINE).search(section, match.end())
    end = section_start + (following.start() if following else len(section))
    return section_start + match.start(), end


def _adr_clause_three(text: str) -> str:
    """ADR-0048's clause 3 — the clause that states the writable set."""
    start, end = _clause_bounds(text, 3)
    return text[start:end]


def _named_paths(text: str) -> list[str]:
    """The ``<common-gitdir>/...`` spans a piece of text names."""
    return [
        span
        for span in re.findall(r"`([^`\n]+)`", text)
        if span.startswith(COMMON_GITDIR + "/")
    ]


def _components(named: str) -> list[str]:
    """The component path a ``<common-gitdir>/...`` span names."""
    rest = named[len(COMMON_GITDIR) + 1 :]
    return [part for part in rest.split("/") if part]


def _appositive(clause: str, named: str) -> str:
    """The clause text that describes the named path, up to its sentence end."""
    index = clause.index(named) + len(named)
    window = clause[index:]
    stop = re.search(r"\.(?:\s|$)", window)
    if stop is not None:
        window = window[: stop.start()]
    return " ".join(window.split())


def _check_adr_clause(clause: str) -> tuple[list[str], str | None]:
    """ADR-0048's clause 3 must name the shared administration root.

    Returns the findings and the named path, so the code-side checks can
    compare against the ADR rather than against a copy of the path.
    """
    problems: list[str] = []
    named_paths = _named_paths(clause)
    if len(named_paths) != 1:
        return (
            [
                "ADR-0048's clause 3 names "
                f"{len(named_paths)} `{COMMON_GITDIR}/...` paths, expected exactly one: "
                f"{named_paths}"
            ],
            None,
        )
    named = named_paths[0]

    appositive = _appositive(clause, named)
    if "own administration directory" in appositive.lower():
        problems.append(
            f"ADR-0048's clause 3 describes {named} as the linked worktree's own "
            "administration directory; it is the shared administration root, the parent "
            "of every linked worktree's own administration directory"
        )
    if "shared" not in appositive.lower():
        problems.append(
            f"ADR-0048's clause 3 does not describe {named} as the shared administration "
            f"root; it describes it as {appositive!r}"
        )
    if "administration root" not in appositive.lower():
        problems.append(
            f"ADR-0048's clause 3 does not call {named} an administration root"
        )
    if "parent" not in clause.lower():
        problems.append(
            f"ADR-0048's clause 3 does not state that {named} is the parent of every "
            "linked worktree's own administration directory"
        )
    return problems, named


# --- Reading the code ------------------------------------------------------


_BINDING = re.compile(r"let\s+(?P<field>\w+)\s*=\s*(?P<init>[^;]*);")
_JOIN_CHAIN = re.compile(r"gitdir(?:\.join\(\"[^\"]+\"\))*")
_JOIN_COMPONENT = re.compile(r"\.join\(\"([^\"]+)\"\)")
_EXPRESSION = re.compile(
    r"paths\.([A-Za-z_][A-Za-z0-9_]*(?:\s*\.\s*[A-Za-z_][A-Za-z0-9_]*)*"
    r"(?:\s*\([^()]*\))*)"
)
_BARE_ROOT = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)\.clone\(\)$")
_TEST_NAME = re.compile(r"^fn\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*\(", re.MULTILINE)

WRITE_ROOTS_MARKER = "let (write_roots, expects_pointer) = match step {"
WRITE_ROOTS_END = "let mut config"


def _doc_comment(source: str, signature: str) -> str:
    """The contiguous ``///`` block immediately above a function signature."""
    index = source.find(signature)
    if index < 0:
        raise GuardError(f"{SOURCE_REL} no longer defines '{signature}'")
    block: list[str] = []
    for line in reversed(source[:index].rstrip("\n").splitlines()):
        stripped = line.strip()
        if not stripped.startswith("///"):
            break
        block.append(stripped)
    block.reverse()
    if not block:
        raise GuardError(f"{SOURCE_REL} no longer documents '{signature}'")
    return "\n".join(block)


def _granted_expressions(source: str) -> list[str]:
    """The write roots the grant builds, as source expressions."""
    start = source.find(WRITE_ROOTS_MARKER)
    if start < 0:
        raise GuardError(
            f"{SOURCE_REL} no longer builds the grant's write roots in the expected shape"
        )
    end = source.find(WRITE_ROOTS_END, start)
    block = source[start : end if end >= 0 else len(source)]
    expressions = []
    for match in _EXPRESSION.finditer(block):
        expression = " ".join(match.group(1).split())
        if expression not in expressions:
            expressions.append(expression)
    return expressions


def _gitdir_rooted_fields(source: str) -> dict[str, bool]:
    """Which bound fields are rooted at the repository's Git directory."""
    fields: dict[str, bool] = {}
    for match in _BINDING.finditer(source):
        field = match.group("field")
        rooted = re.search(r"\bgitdir\b", match.group("init")) is not None
        fields[field] = fields.get(field, False) or rooted
    return fields


def _administration_field(source: str, components: list[str]) -> str | None:
    """The field bound to the administration root the ADR's components name."""
    for match in _BINDING.finditer(source):
        chain = _JOIN_CHAIN.search(match.group("init"))
        if chain is None:
            continue
        if _JOIN_COMPONENT.findall(chain.group(0)) == components:
            return match.group("field")
    return None


def _check_grant_shape(source: str, components: list[str]) -> list[str]:
    """The grant must produce exactly one write root under the Git directory."""
    problems: list[str] = []
    field = _administration_field(source, components)
    if field is None:
        return [
            "the grant does not build its administration root from the path ADR-0048 "
            f"names ({'/'.join(components)}); the accepted path is not what the code grants"
        ]

    fields = _gitdir_rooted_fields(source)
    rooted: list[str] = []
    for expression in _granted_expressions(source):
        bare = _BARE_ROOT.fullmatch(expression)
        if bare is not None:
            name = bare.group(1)
            if name not in fields:
                problems.append(
                    "the grant adds a writable root the guard cannot place outside the "
                    f"repository's Git directory: {expression}"
                )
                continue
            if fields[name]:
                rooted.append(expression)
            continue
        if "gitdir" in expression:
            rooted.append(expression)
            continue
        problems.append(
            "the grant adds a writable root the guard cannot place outside the "
            f"repository's Git directory: {expression}"
        )

    if len(rooted) > 1:
        problems.append(
            "the grant produces "
            f"{len(rooted)} writable roots under the repository's Git directory "
            f"({rooted}), expected exactly one"
        )
    elif len(rooted) == 1:
        bare = _BARE_ROOT.fullmatch(rooted[0])
        if bare is not None and bare.group(1) != field:
            problems.append(
                "the grant's writable root under the repository's Git directory is "
                f"{rooted[0]}, not the administration root ADR-0048 names ({field})"
            )
    else:
        problems.append(
            "the grant produces no writable root under the repository's Git directory, "
            "so the administration root ADR-0048 names is not granted"
        )
    return problems


def _check_comments(source: str, named: str) -> list[str]:
    """The two code comments must name the ADR's path, and call it shared."""
    problems: list[str] = []
    for where, signature in COMMENT_SIGNATURES:
        comment = _doc_comment(source, signature)
        spans = _named_paths(comment)
        if len(spans) != 1:
            problems.append(
                f"the {where} names {len(spans)} `{COMMON_GITDIR}/...` paths, "
                f"expected exactly one: {spans}"
            )
            continue
        if spans[0] != named:
            problems.append(
                f"the {where} names {spans[0]}, which is not the path ADR-0048 names "
                f"({named})"
            )
        if "shared" not in comment.lower():
            problems.append(
                f"the {where} does not call the administration root shared"
            )
    return problems


def _check_containment_test(text: str) -> list[str]:
    """The containment test must be named for the shared administration root."""
    problems: list[str] = []
    names = [match.group("name") for match in _TEST_NAME.finditer(text)]
    if not names:
        return ["the containment test file declares no test function"]
    if not any(REQUIRED_TEST_NAME in name for name in names):
        problems.append(
            "the containment test no longer names a test for the shared administration "
            f"root; a test name containing {REQUIRED_TEST_NAME!r} is required"
        )
    if any(FORBIDDEN_TEST_NAME in name for name in names):
        problems.append(
            "the containment test still names a test after the linked worktree's own "
            f"administration directory; a test name containing {FORBIDDEN_TEST_NAME!r} "
            "describes a narrower grant than the code delivers"
        )
    return problems


def _check_structural_refusal(source: str) -> list[str]:
    """``assert_grant`` must still refuse every other Git-directory write root."""
    if REFUSAL_MARKER not in "".join(source.split()):
        return [
            "assert_grant no longer refuses a write root under the repository's Git "
            "directory that is not the administration root; the enforcement behind "
            "the shared-root claim is gone"
        ]
    return []


# --- The guard --------------------------------------------------------------


def check_tree(tree: Path = ROOT) -> list[str]:
    """The defects in the grant/ADR agreement, as human-readable messages."""
    adr_path = tree / ADR_REL
    source_path = tree / SOURCE_REL
    test_path = tree / TEST_REL
    if not adr_path.is_file():
        return [f"the ADR is missing: {ADR_REL}"]
    if not source_path.is_file():
        return [f"the contained worktree source is missing: {SOURCE_REL}"]
    if not test_path.is_file():
        return [f"the containment test is missing: {TEST_REL}"]

    adr_text = adr_path.read_text(encoding="utf-8", errors="replace")
    source_text = source_path.read_text(encoding="utf-8", errors="replace")
    test_text = test_path.read_text(encoding="utf-8", errors="replace")

    clause = _adr_clause_three(adr_text)
    problems, named = _check_adr_clause(clause)
    if named is None:
        return problems

    problems.extend(_check_grant_shape(source_text, _components(named)))
    problems.extend(_check_comments(source_text, named))
    problems.extend(_check_containment_test(test_text))
    problems.extend(_check_structural_refusal(source_text))
    return problems


# --- Self-test --------------------------------------------------------------


def _copy_tree(destination: Path) -> Path:
    """A writable copy of the files the guard reads."""
    for relative in (ADR_REL, SOURCE_REL, TEST_REL):
        source = ROOT / relative
        if not source.is_file():
            continue
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    return destination


def _mutate_comment(tree: Path, signature: str, old: str, new: str) -> None:
    """Replace a path inside the doc comment that precedes a signature."""
    path = tree / SOURCE_REL
    text = path.read_text(encoding="utf-8")
    index = text.find(signature)
    if index < 0:
        raise AssertionError(f"the mutation could not find '{signature}'")
    head, tail = text[:index], text[index:]
    position = head.rfind(old)
    if position < 0:
        raise AssertionError("the mutation could not find the comment text")
    path.write_text(
        head[:position] + new + head[position + len(old) :] + tail, encoding="utf-8"
    )


def _mutate_adr_clause(tree: Path, old: str, new: str) -> None:
    """Replace a path inside ADR-0048's clause 3."""
    path = tree / ADR_REL
    text = path.read_text(encoding="utf-8")
    start, end = _clause_bounds(text, 3)
    clause = text[start:end]
    if old not in clause:
        raise AssertionError("the mutation could not find the ADR clause text")
    mutated = clause.replace(old, new, 1)
    path.write_text(text[:start] + mutated + text[end:], encoding="utf-8")


def _mutate_second_gitdir_root(tree: Path) -> None:
    """Add a second writable root under the repository's Git directory."""
    path = tree / SOURCE_REL
    text = path.read_text(encoding="utf-8")
    marker = "                paths.administration_root.clone(),\n"
    if marker not in text:
        raise AssertionError("the mutation could not find the grant's write roots")
    path.write_text(
        text.replace(
            marker,
            marker + "                paths.gitdir.join(\"hooks\").clone(),\n",
            1,
        ),
        encoding="utf-8",
    )


def _mutate_test_name(tree: Path, old: str, new: str) -> None:
    """Rename the containment test that asserts the grant."""
    path = tree / TEST_REL
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise AssertionError("the mutation could not find the containment test name")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def self_test() -> int:
    """Prove the guard rejects every class of drift it claims to catch."""
    failures: list[str] = []

    def case(name: str, mutate, expect_fragment: str) -> None:
        with tempfile.TemporaryDirectory(prefix="codegg-contained-grant-") as raw:
            tree = _copy_tree(Path(raw))
            mutate(tree)
            problems = check_tree(tree)
            if not any(expect_fragment in problem for problem in problems):
                failures.append(
                    f"the '{name}' divergence was not reported (expected a problem "
                    f"containing '{expect_fragment}')"
                )
            else:
                print(f"  ok: rejected {name}")

    with tempfile.TemporaryDirectory(prefix="codegg-contained-grant-") as raw:
        tree = _copy_tree(Path(raw))
        problems = check_tree(tree)
        if problems:
            failures.append(
                "the unmodified copy must pass the guard: " + "; ".join(problems)
            )
        else:
            print("  ok: accepted the unmodified tree")

    case(
        "a code comment that names the wrong path",
        lambda tree: _mutate_comment(
            tree,
            "pub fn build_worktree_grant(",
            "<common-gitdir>/worktrees/",
            "<common-gitdir>/worktrees-old/",
        ),
        "which is not the path ADR-0048 names",
    )
    case(
        "an ADR that names the wrong path",
        lambda tree: _mutate_adr_clause(
            tree, "<common-gitdir>/worktrees/", "<common-gitdir>/worktrees-old/"
        ),
        "the grant does not build its administration root from the path ADR-0048 names",
    )
    case(
        "a grant that adds a second writable root under the Git directory",
        _mutate_second_gitdir_root,
        "writable roots under the repository's Git directory",
    )
    case(
        "a containment test still named after the worktree's own directory",
        lambda tree: _mutate_test_name(
            tree,
            "the_grant_names_only_the_shared_administration_root",
            "the_grant_names_only_the_administration_directory",
        ),
        "describes a narrower grant than the code delivers",
    )

    if failures:
        for failure in failures:
            print(f"contained-worktree-grant self-test FAIL: {failure}", file=sys.stderr)
        return 1
    print("contained-worktree-grant self-test ok")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects the drift it claims to catch",
    )
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    try:
        problems = check_tree(ROOT)
    except GuardError as error:
        print(f"contained-worktree-grant check: FAILED: {error}", file=sys.stderr)
        return 1

    if problems:
        print("contained-worktree-grant check: FAILED", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 1

    print(
        "contained-worktree-grant check ok: ADR-0048's clause 3, the two code "
        "comments and the grant name one shared administration root, and the "
        "Git-directory write root is the only one"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
