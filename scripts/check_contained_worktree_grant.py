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
       from exactly those components. The scan covers the **whole**
       ``build_worktree_grant`` body: a writable root added after the
       sandbox configuration is built is still a writable root.
    3. The two code comments that describe the grant must name the same path
       the ADR names, and must call it shared.
    4. The containment test that asserts the grant must be **named** for the
       shared root. The original defect included a test named after the linked
       worktree's own administration directory — the test whose job is to
       assert containment described a narrower grant than the code delivers.
    5. The structural refusal in ``assert_grant`` — every write root under the
       Git directory that is not the administration root is refused — must
       still be there. That refusal is what makes claim 2 an enforced
       property rather than a comment. It is checked by **behaviour**: the
       condition is lifted out of ``assert_grant`` and applied to synthetic
       write roots, so a refusal that has been neutered (``&& false``) or
       made vacuous (``|| true``) is rejected rather than merely looking
       like the original.

WHAT THIS GUARD DELIBERATELY DOES NOT DO
    It does not decide whether the shared root's reach over other linked
    worktrees' ``HEAD`` files is acceptable, and it does not enforce the
    disposition the maintainer chose for it. That disposition is **defence in
    depth** — a pinned ``core.hooksPath`` — and it is enforced by
    ``scripts/check_contained_worktree_hooks_path.py``.

    It does not narrow the grant. Narrowing the accepted path to the linked
    worktree's own administration directory is a **new ADR**.

    It does not prove that the child obtained the grant at runtime. That is
    the launch's own assertion (``obtained_write_root_is_present``) and the
    hosted containment lane's job.

    It does not run Rust. The refusal is evaluated in Python, from the
    condition the code carries, against synthetic write roots; a condition
    written in a form the guard cannot evaluate is a finding, not a pass.

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

# The refusal in ``assert_grant`` is a Rust condition over three paths: the
# write root under test (``write``), the repository's Git directory
# (``gitdir``) and the shared administration root
# (``administration_root``). The guard lifts that condition out of the code
# and evaluates it against synthetic write roots, so the refusal is checked
# by what it does rather than by what its text looks like.
REFUSAL_IDENTIFIERS: tuple[str, ...] = ("write", "gitdir", "administration_root")

# A condition that is always true or always false is not a refusal.
_TAUTOLOGY_RE = re.compile(
    r"(?:\bfalse\s*&&|&&\s*false\b|\btrue\s*\|\||\|\|\s*true\b)", re.IGNORECASE
)

# The synthetic repository the refusal is exercised against.
_SYNTHETIC_GITDIR = "/repo/.git"
_SYNTHETIC_ADMINISTRATION_ROOT = "/repo/.git/worktrees"

# The synthetic write roots the refusal is exercised against, and what each
# one must be given.
_SYNTHETIC_WRITE_ROOTS: dict[str, str] = {
    "another root under the Git directory": "/repo/.git/hooks",
    "the administration root itself": _SYNTHETIC_ADMINISTRATION_ROOT,
    "a path under the administration root": _SYNTHETIC_ADMINISTRATION_ROOT + "/other",
    "a root outside the Git directory": "/repo/checkout",
}


class GuardError(Exception):
    """The guard cannot read what it has to compare against."""


class RefusalEvaluationError(Exception):
    """The refusal condition uses something the guard cannot evaluate."""


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


# The regions of a Rust source that are not code: comments, string literals
# (including raw ones) and character literals. A lifetime is not a character
# literal — `<'a, 'b>` has no closing quote after the `a` — so the
# character-literal form only matches a quoted single character.
#
# This is the same masking technique `scripts/check_git_consistency_consumers.py`
# uses, kept as a local copy because the guards in this repository are
# independent scripts: none of them imports another.
_MASK_RE = re.compile(
    r"r(?P<hashes>#+)\"(?:[^\"]|\"(?!(?P=hashes)))*\"(?P=hashes)"
    r"|r\"[^\"]*\""
    r"|//[^\n]*"
    r"|/\*.*?\*/"
    r"|\"(?:\\.|[^\"\\\n])*\""
    r"|'(?:\\.|[^\\'\n])'",
    re.DOTALL,
)


def _mask_non_code(text: str) -> str:
    """Blank out comments and literal contents, preserving every offset.

    Every non-code region — a `//` line comment, a `/* ... */` block comment,
    a string literal, a raw string literal or a character literal — is
    replaced by spaces of the same length, so an offset into the masked text
    addresses the same character of the original text.
    """
    return _MASK_RE.sub(lambda match: re.sub(r"[^\n]", " ", match.group(0)), text)


def _matching_brace(masked: str, open_at: int) -> int | None:
    """The offset of the `}` that closes the `{` at ``open_at``."""
    depth = 0
    for index in range(open_at, len(masked)):
        char = masked[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return index
    return None


def _matching_paren(masked: str, open_at: int) -> int | None:
    """The offset of the `)` that closes the `(` at ``open_at``."""
    depth = 0
    for index in range(open_at, len(masked)):
        char = masked[index]
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            if depth == 0:
                return index
    return None


def _function_body(source: str, signature: str) -> str | None:
    """The body of a Rust function, located by its signature.

    The signature must end with the opening `(` of the parameter list. The
    body's braces are matched on the masked text, so a brace inside a string
    literal or a comment is not counted.
    """
    masked = _mask_non_code(source)
    index = masked.find(signature)
    if index < 0:
        return None
    depth = 1  # the signature's own `(`
    open_at: int | None = None
    for position in range(index + len(signature), len(masked)):
        char = masked[position]
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
        elif char == "{" and depth == 0:
            open_at = position
            break
    if open_at is None:
        return None
    close = _matching_brace(masked, open_at)
    if close is None:
        return None
    return source[open_at : close + 1]


def _call_argument_spans(masked: str, name: str) -> list[tuple[int, int]]:
    """The argument spans of every call to ``name(`` in the masked text."""
    spans: list[tuple[int, int]] = []
    for match in re.finditer(re.escape(name) + r"\s*\(", masked):
        open_at = masked.index("(", match.start())
        close = _matching_paren(masked, open_at)
        if close is None:
            continue
        spans.append((open_at + 1, close))
    return spans


def _if_conditions(masked: str, original: str) -> list[str]:
    """Every ``if`` condition in a body, as the source text of the condition.

    The offsets come from the masked text, so a brace inside a string literal
    cannot end the condition early; the text itself is taken from the
    original, because the condition is the code the guard has to read.
    """
    conditions: list[str] = []
    for match in re.finditer(r"\bif\b", masked):
        start = match.end()
        depth = 0
        end: int | None = None
        for index in range(start, len(masked)):
            char = masked[index]
            if char in "([":
                depth += 1
            elif char in ")]":
                depth -= 1
            elif char == "{":
                if depth == 0:
                    end = index
                    break
                depth += 1
            elif char == ";" and depth == 0:
                break
        if end is None:
            continue
        condition = original[start:end].strip()
        if condition:
            conditions.append(condition)
    return conditions


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

# The calls in the grant that hand a path to something that is **not** the
# write grant. Their arguments are not write roots, so the whole-body scan
# must not read them as one:
#
#   * `with_read_only_paths` — the repository's Git directory, granted
#     readable and explicitly not writable;
#   * `with_expected_created_git_pointers` — the checkout's `.git` pointer
#     file, which the child creates;
#   * `launch_spec` — the child's binary and its working directory.
#
# A new path-bearing call in the grant needs an entry here, or the guard
# reports it as a write root it cannot place outside the Git directory. That
# is the intended bias: the guard fails closed on a path it cannot explain.
NON_WRITE_PATH_CALLS: tuple[str, ...] = (
    "with_read_only_paths",
    "with_expected_created_git_pointers",
    "launch_spec",
)


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
    """The write roots the grant builds, as source expressions.

    The scan covers the **whole** ``build_worktree_grant`` body, not a window
    of it. A write root added after the sandbox configuration is built is
    still a write root, and a bounded window — one that ended at the
    configuration — hid exactly that.

    The paths the grant hands to something that is not the write grant (the
    read-only Git directory, the expected pointer file, the child's own
    binary and working directory) are excluded; see
    ``NON_WRITE_PATH_CALLS``.
    """
    body = _function_body(source, "pub fn build_worktree_grant(")
    if body is None:
        raise GuardError(
            f"{SOURCE_REL} no longer defines 'pub fn build_worktree_grant('"
        )
    if WRITE_ROOTS_MARKER not in body:
        raise GuardError(
            f"{SOURCE_REL} no longer builds the grant's write roots in the expected shape"
        )

    masked = _mask_non_code(body)
    excluded: list[tuple[int, int]] = []
    for name in NON_WRITE_PATH_CALLS:
        excluded.extend(_call_argument_spans(masked, name))

    expressions: list[str] = []
    for match in _EXPRESSION.finditer(masked):
        if any(start <= match.start() < end for start, end in excluded):
            continue
        # The span is taken from the original source, so the expression reads
        # as the code does; the masked text is only used to find it.
        expression = " ".join(body[match.start(1) : match.end(1)].split())
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


def _path_components(path: str) -> tuple[str, ...]:
    """A path's components, the way ``Path::starts_with`` compares them."""
    return tuple(
        part for part in path.replace("\\", "/").split("/") if part not in ("", ".")
    )


def _path_starts_with(path: str, prefix: str) -> bool:
    """Rust's ``Path::starts_with``: a component-wise prefix, not a string one."""
    left = _path_components(path)
    right = _path_components(prefix)
    return left[: len(right)] == right


_CONDITION_TOKEN_RE = re.compile(
    r"(?P<operator>&&|\|\||==|!=|\(|\)|!|\.)|(?P<name>[A-Za-z_][A-Za-z0-9_]*)"
)


def _tokenize_condition(text: str) -> list[str]:
    """The tokens of a refusal condition."""
    tokens: list[str] = []
    index = 0
    while index < len(text):
        if text[index].isspace():
            index += 1
            continue
        match = _CONDITION_TOKEN_RE.match(text, index)
        if match is None:
            raise RefusalEvaluationError(
                f"unsupported character {text[index]!r} in the refusal condition"
            )
        tokens.append(match.group(0))
        index = match.end()
    return tokens


class _ConditionEvaluator:
    """Evaluate the small expression language a Rust refusal condition uses.

    The supported forms are the ones the refusal is written in: ``&&``,
    ``||``, ``!``, ``==``, ``!=``, parentheses, the ``true``/``false``
    literals, the three path identifiers, and ``<path>.starts_with(<path>)``.
    Anything else is an error — and an error is a finding, because the guard
    cannot vouch for a refusal it cannot read.
    """

    def __init__(self, tokens: list[str], environment: dict[str, str]):
        self._tokens = tokens
        self._position = 0
        self._environment = environment

    def evaluate(self) -> bool:
        """The condition's verdict under the environment."""
        value = self._or()
        if self._peek() is not None:
            raise RefusalEvaluationError(
                f"unexpected {self._peek()!r} at the end of the refusal condition"
            )
        return value

    # -- token helpers ----------------------------------------------------

    def _peek(self) -> str | None:
        if self._position < len(self._tokens):
            return self._tokens[self._position]
        return None

    def _take(self) -> str:
        token = self._peek()
        if token is None:
            raise RefusalEvaluationError("the refusal condition ends unexpectedly")
        self._position += 1
        return token

    def _accept(self, token: str) -> bool:
        if self._peek() == token:
            self._position += 1
            return True
        return False

    # -- grammar ----------------------------------------------------------

    def _or(self) -> bool:
        left = self._and()
        while self._accept("||"):
            # The right side is evaluated before the two are combined, so a
            # short-circuit never hides a form the guard cannot read.
            right = self._and()
            left = left or right
        return left

    def _and(self) -> bool:
        left = self._unary()
        while self._accept("&&"):
            right = self._unary()
            left = left and right
        return left

    def _unary(self) -> bool:
        if self._accept("!"):
            return not self._unary()
        return self._primary()

    def _primary(self) -> bool:
        if self._accept("("):
            value = self._or()
            if not self._accept(")"):
                raise RefusalEvaluationError(
                    "unbalanced parentheses in the refusal condition"
                )
            return value
        token = self._take()
        if token == "true":
            return True
        if token == "false":
            return False
        if token not in REFUSAL_IDENTIFIERS:
            raise RefusalEvaluationError(
                f"the refusal condition names {token!r}, which the guard cannot evaluate"
            )
        if self._accept("."):
            return self._starts_with(token)
        if self._accept("=="):
            return self._compare(token, negated=False)
        if self._accept("!="):
            return self._compare(token, negated=True)
        raise RefusalEvaluationError(
            f"the refusal condition uses {token!r} in a form the guard cannot evaluate"
        )

    def _starts_with(self, subject: str) -> bool:
        method = self._take()
        if method != "starts_with":
            raise RefusalEvaluationError(
                f"the refusal condition calls {method!r}, which the guard cannot evaluate"
            )
        if not self._accept("("):
            raise RefusalEvaluationError(
                "malformed method call in the refusal condition"
            )
        argument = self._take()
        if not self._accept(")"):
            raise RefusalEvaluationError(
                "malformed method call in the refusal condition"
            )
        if argument not in REFUSAL_IDENTIFIERS:
            raise RefusalEvaluationError(
                f"the refusal condition compares {subject!r} against {argument!r}, which "
                "the guard cannot evaluate"
            )
        return _path_starts_with(self._environment[subject], self._environment[argument])

    def _compare(self, subject: str, negated: bool) -> bool:
        other = self._take()
        if other not in REFUSAL_IDENTIFIERS:
            raise RefusalEvaluationError(
                f"the refusal condition compares {subject!r} against {other!r}, which the "
                "guard cannot evaluate"
            )
        equal = self._environment[subject] == self._environment[other]
        return not equal if negated else equal


def _refusal_conditions(source: str) -> list[str]:
    """The conditions in ``assert_grant`` that refuse a Git-directory root.

    The refusal is the condition that both tests the repository's Git
    directory and compares against the shared administration root: that is
    the refusal ADR-0048's claim rests on.
    """
    body = _function_body(source, "fn assert_grant(")
    if body is None:
        return []
    masked = _mask_non_code(body)
    return [
        condition
        for condition in _if_conditions(masked, body)
        if "starts_with" in condition and "administration_root" in condition
    ]


def _refusal_verdicts(condition: str) -> dict[str, bool]:
    """The refusal condition's verdict on each synthetic write root."""
    verdicts: dict[str, bool] = {}
    for name, write in _SYNTHETIC_WRITE_ROOTS.items():
        environment = {
            "write": write,
            "gitdir": _SYNTHETIC_GITDIR,
            "administration_root": _SYNTHETIC_ADMINISTRATION_ROOT,
        }
        verdicts[name] = _ConditionEvaluator(
            _tokenize_condition(condition), environment
        ).evaluate()
    return verdicts


def _check_structural_refusal(source: str) -> list[str]:
    """``assert_grant`` must still refuse every other Git-directory write root.

    The refusal is checked by **behaviour**, not by text. The condition is
    lifted out of ``assert_grant`` and applied to synthetic write roots: a
    write root under the Git directory that is not the administration root
    must be refused, while the administration root itself and a root outside
    the Git directory must not be. A condition that has been neutered
    (``&& false``) or made vacuous (``|| true``) therefore fails, where a
    match on the condition's characters would not.
    """
    conditions = _refusal_conditions(source)
    if not conditions:
        return [
            "assert_grant no longer refuses a write root under the repository's Git "
            "directory that is not the administration root; the enforcement behind "
            "the shared-root claim is gone"
        ]
    if len(conditions) > 1:
        return [
            "assert_grant carries more than one condition that refuses a write root "
            f"under the repository's Git directory ({len(conditions)} found); the guard "
            "cannot tell which one is the refusal ADR-0048's claim rests on"
        ]

    condition = conditions[0]
    problems: list[str] = []
    if _TAUTOLOGY_RE.search(condition):
        problems.append(
            f"the refusal in assert_grant carries a tautology ({condition!r}); a "
            "condition that is always true or always false is not the refusal the "
            "shared-root claim rests on"
        )
    try:
        verdicts = _refusal_verdicts(condition)
    except RefusalEvaluationError as error:
        problems.append(
            f"the refusal in assert_grant cannot be evaluated ({error}); the guard "
            "cannot vouch for a refusal it cannot read"
        )
        return problems

    if not verdicts["another root under the Git directory"]:
        problems.append(
            "assert_grant no longer refuses a write root under the repository's Git "
            "directory that is not the administration root; the enforcement behind "
            "the shared-root claim is gone"
        )
    if verdicts["the administration root itself"]:
        problems.append(
            "assert_grant refuses the shared administration root itself; the grant it "
            "is supposed to describe could never be launched"
        )
    if not verdicts["a path under the administration root"]:
        problems.append(
            "assert_grant no longer refuses a path under the shared administration "
            "root; a sibling worktree's directory inside the administration root "
            "would become writable, which is the exact grant the shared-root claim "
            "forbids"
        )
    if verdicts["a root outside the Git directory"]:
        problems.append(
            "assert_grant refuses a write root outside the repository's Git directory; "
            "the refusal is wider than the claim it enforces"
        )
    return problems


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


# The end of the grant's sandbox configuration. A write root added after it
# is outside the window the guard used to scan.
_CONFIG_ANCHOR = '        .with_read_only_paths(vec![paths.gitdir.display().to_string()]);\n'


def _mutate_widened_allowed_paths(tree: Path) -> None:
    """Widen the granted write paths after the configuration is built."""
    path = tree / SOURCE_REL
    text = path.read_text(encoding="utf-8")
    if _CONFIG_ANCHOR not in text:
        raise AssertionError("the mutation could not find the grant's configuration")
    widening = (
        "    config = config.with_allowed_paths(\n"
        "        write_roots\n"
        "            .iter()\n"
        '            .chain(std::iter::once(&paths.gitdir.join("hooks")))\n'
        "            .map(|path| path.display().to_string())\n"
        "            .collect(),\n"
        "    );\n"
    )
    path.write_text(
        text.replace(_CONFIG_ANCHOR, _CONFIG_ANCHOR + widening, 1), encoding="utf-8"
    )


def _mutate_gitdir_root_after_config(tree: Path) -> None:
    """Name a Git-directory-rooted path after the configuration is built."""
    path = tree / SOURCE_REL
    text = path.read_text(encoding="utf-8")
    if _CONFIG_ANCHOR not in text:
        raise AssertionError("the mutation could not find the grant's configuration")
    addition = '    let _extra = paths.gitdir.join("hooks");\n'
    path.write_text(
        text.replace(_CONFIG_ANCHOR, _CONFIG_ANCHOR + addition, 1), encoding="utf-8"
    )


REFUSAL_CONDITION = "if write.starts_with(gitdir) && write != administration_root {"


def _mutate_refusal_condition(tree: Path, new: str) -> None:
    """Replace the condition under which ``assert_grant`` refuses a root."""
    path = tree / SOURCE_REL
    text = path.read_text(encoding="utf-8")
    if REFUSAL_CONDITION not in text:
        raise AssertionError("the mutation could not find the refusal condition")
    path.write_text(text.replace(REFUSAL_CONDITION, new, 1), encoding="utf-8")


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
        "a grant that widens the allowed paths after the configuration is built",
        _mutate_widened_allowed_paths,
        "writable roots under the repository's Git directory",
    )
    case(
        "a grant that names a Git-directory root after the configuration is built",
        _mutate_gitdir_root_after_config,
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
    case(
        "a refusal that is neutered with a false conjunct",
        lambda tree: _mutate_refusal_condition(
            tree,
            "if write.starts_with(gitdir) && write != administration_root && false {",
        ),
        "no longer refuses a write root under the repository's Git directory",
    )
    case(
        "a refusal that is made vacuous with a true disjunct",
        lambda tree: _mutate_refusal_condition(
            tree,
            "if write.starts_with(gitdir) && write != administration_root || true {",
        ),
        "refuses the shared administration root itself",
    )
    case(
        "a refusal that permits a path under the administration root",
        lambda tree: _mutate_refusal_condition(
            tree,
            "if write.starts_with(gitdir) && !write.starts_with(administration_root) {",
        ),
        "no longer refuses a path under the shared administration root",
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
