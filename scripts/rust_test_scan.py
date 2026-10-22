#!/usr/bin/env python3
"""Rust-aware scanner for the repository's test suite.

This module is the shared foundation of two static guards:

- `scripts/check_test_assertion_quality.py` (milestone M006)
- `scripts/check_test_duplication.py` (milestone M007)

It exists because a regular expression over Rust source cannot answer the
questions those guards ask. The failure modes measured while preparing the
plans were:

1. **Lifetimes read as character literals.** A pattern of the form
   `'(?:\\.|[^'\\])'` matches `&'a T` and swallows the code up to the next
   apostrophe. A scanner built that way found 1,204 of the 4,978 test
   functions in `tests/` -- an undercount of 76%.
2. **A raw-string pattern without the `r` prefix matches a closing quote.**
   `r?#*"(?:[^"]|"(?!#))*"` starts at the closing quote of a plain string
   because a word character precedes it, and consumes the following
   function header -- seven test functions in
   `tests/security_permission/ssrf.inc` alone.
3. **`#[tokio::test]` is followed by `async fn`.** A pattern that requires
   `fn` immediately after the attribute matches no asynchronous test at all
   -- 2,371 in `tests/`.
4. **A multi-line string literal silently moves every line number after
   it.** When a scanner replaces a literal's contents with a placeholder it
   must keep the newlines *inside* the literal. Measured on this tree,
   298 of 1,349 files contain a multi-line string literal, and the
   reported line number drifted by up to 226 lines. The counts stayed
   correct; only the line numbers moved -- which is the more dangerous
   failure, because a removal table built on shifted line numbers removes
   the wrong function.

The scanner below therefore:

- treats string, raw-string, byte-string and character literals as opaque;
- distinguishes a character literal from a **lifetime**;
- skips Rust's **nestable** block comments whole;
- matches `#[tokio::test]` to the `async fn` that follows it;
- produces a **length-preserving** mask, so a body span found in the mask
  can be sliced out of the original and a reported line number is the
  function's own line;
- produces a **content-preserving** digest, so two calls that differ only
  in a string argument stay distinct while two identical calls stay equal.

Two properties are load-bearing and are checked per file rather than
assumed:

- the parsed test-function count equals the ground-truth attribute count;
- the mask's newline count equals the original's.

Run `python3 scripts/rust_test_scan.py --self-test` to exercise the lexer
against fixtures that reproduce each failure mode above, and
`python3 scripts/rust_test_scan.py --validate` to check the scanner
against the repository's ground truth.
"""

from __future__ import annotations

import hashlib
import re
import sys

from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# Scans are cached per root so a guard that runs several checks over the
# same tree pays for the parse once. The cache is keyed on the tree's
# file signature, so a file edited between two calls is re-parsed rather
# than served stale.
_FUNCTION_CACHE: dict[str, tuple[tuple, list["TestFunction"]]] = {}
_FUNCTION_MAP_CACHE: dict[str, tuple[tuple, dict[str, dict[str, "Function"]]]] = {}
_SOURCE_CACHE: dict[str, tuple[tuple, list["SourceFile"]]] = {}

# The three source trees the repository's test functions live in.
TREES = ("tests", "src", "crates")

# A test attribute is `#[test]` or `#[tokio::test]` (with or without
# arguments). `#[test_case]` and `#[tokio::test_with]` are *not* test
# attributes and must not be counted.
_TEST_ATTRIBUTE_PREFIXES = ("test", "tokio::test")

# A panic-capable construct: anything that can fail a test.
_PANIC_CONSTRUCT = re.compile(
    r"(?:\bassert(?:_eq|_ne)?!\s*\("
    r"|\bdebug_assert(?:_eq|_ne)?!\s*\("
    r"|\bassert_matches!\s*\("
    r"|\bpanic!\s*\("
    r"|\bunreachable!\s*\("
    r"|\btodo!\s*\("
    r"|\bunimplemented!\s*\("
    r"|\.expect(?:_err)?\s*\("
    r"|\.unwrap(?:_err)?\s*\()"
)

# A compile-time assertion: a `const` item inside a body. The value of
# such an item is checked by the compiler, not at run time.
_COMPILE_TIME_ITEM = re.compile(r"\bconst\s+(?:_|[A-Z_][A-Z0-9_]*)\s*:")

# A skip marker. The suite uses `skipped:`, `SKIP:` and `Skipped:`.
_SKIP_MARKER = re.compile(r"\b(?:skipped|SKIP|Skipped)\s*:")

# A `return` statement.
_RETURN_STATEMENT = re.compile(r"\breturn\b")

# Filesystem write calls, used by the `ignored-repository-writer` class.
_WRITE_CALL = re.compile(
    r"(?:\bfs::write\b"
    r"|\bfs::write_all\b"
    r"|\bfs::create_dir(?:_all)?\b"
    r"|\bfs::copy\b"
    r"|\bfs::remove_file\b"
    r"|\bFile::create\b"
    r"|\bwrite_derived_view_atomic\b"
    r"|\.write_all\s*\()"
)

# `env!("CARGO_MANIFEST_DIR")` names the crate root, so any path built
# from it lands inside the repository tree.
_MANIFEST_DIR = re.compile(r"CARGO_MANIFEST_DIR")


@dataclass(frozen=True)
class Literal:
    """One string, raw string, byte string or character literal."""

    start: int
    end: int
    kind: str
    content: str

    def marker(self) -> str:
        digest = hashlib.sha256(self.content.encode("utf-8")).hexdigest()[:16]
        return f"<{self.kind}:{digest}>"


@dataclass(frozen=True)
class Comment:
    """One line or block comment."""

    start: int
    end: int
    kind: str


@dataclass(frozen=True)
class SourceScan:
    """The literal and comment inventory of one source text."""

    text: str
    literals: tuple[Literal, ...]
    comments: tuple[Comment, ...]
    events: tuple[tuple[int, int, str], ...] = ()

    def mask(self) -> str:
        """A structural view of the source.

        Literals and comments are blanked to spaces, so the text has the
        same length *and* the same newline count as the original. A body
        span found in the mask can therefore be sliced straight out of the
        original, and a line number computed from the mask is the
        function's own line.
        """
        buf = list(self.text)
        for span_start, span_end in (
            [(lit.start, lit.end) for lit in self.literals]
            + [(com.start, com.end) for com in self.comments]
        ):
            for index in range(span_start, span_end):
                if buf[index] != "\n":
                    buf[index] = " "
        return "".join(buf)

    def code(self) -> str:
        """The source with comments removed and literals **kept**.

        `mask` blanks literals, which is right for finding structure but
        wrong for a rule that is about what a literal *says* — a
        `#[link(name = "python3.11")]` attribute is invisible in the mask.
        This view removes only the comments, so it keeps the original's
        length and newline count.
        """
        buf = list(self.text)
        for comment in self.comments:
            for index in range(comment.start, comment.end):
                if buf[index] != "\n":
                    buf[index] = " "
        return "".join(buf)

    def digest(self) -> str:
        """A content-preserving view of the source.

        Comments are dropped. Every literal is replaced by a marker
        derived from its *content*, so two calls that differ only in a
        string argument stay distinct while two identical calls stay
        equal. This is the difference between a correct duplicate check
        and one that collapses every call with a string argument into a
        single false group.

        The digest is **not** offset-aligned with the source: a literal's
        marker has a different length from the literal it replaces. Use
        `digest_slice` to take a span of the digest.
        """
        return self.digest_slice(0, len(self.text))

    def digest_slice(self, start: int, end: int) -> str:
        """The digest of `text[start:end]`.

        The slice is taken in *source* coordinates and the digest is built
        over that span, so the caller's body spans -- which come from the
        length-preserving mask -- line up exactly.
        """
        out: list[str] = []
        index = start
        for event_start, event_end, replacement in self.events:
            if event_start >= end:
                break
            if event_end <= start:
                continue
            if index < event_start:
                out.append(self.text[index : min(event_start, end)])
            out.append(replacement)
            index = max(index, event_end)
        if index < end:
            out.append(self.text[index:end])
        return "".join(out)


@dataclass(frozen=True)
class TestFunction:
    """One test function, with the attributes that govern it."""

    name: str
    file: Path
    rel: str
    tree: str
    line: int
    body_start: int
    body_end: int
    item_start: int
    is_async: bool
    attributes: tuple[str, ...]
    body_text: str
    body_digest: str

    @property
    def location(self) -> str:
        return f"{self.rel}:{self.line}"

    def item_text(self, source_text: str) -> str:
        """The whole item: its governing attributes, the `fn`, and the body."""
        return source_text[self.item_start : self.body_end + 1]

    def has_attribute(self, prefix: str) -> bool:
        return any(attr.strip().startswith(prefix) for attr in self.attributes)

    @property
    def is_ignored(self) -> bool:
        return self.has_attribute("ignore")

    @property
    def has_skip_marker(self) -> bool:
        return _SKIP_MARKER.search(self.body_text) is not None

    @property
    def has_panic_construct(self) -> bool:
        if self.has_attribute("should_panic"):
            return True
        return _PANIC_CONSTRUCT.search(self.body_text) is not None

    @property
    def has_compile_time_assertion(self) -> bool:
        return _COMPILE_TIME_ITEM.search(self.body_text) is not None

    def first_panic_construct(self) -> int:
        """The offset of the first panic-capable construct, or -1."""
        if self.has_attribute("should_panic"):
            return 0
        match = _PANIC_CONSTRUCT.search(self.body_text)
        return match.start() if match else -1

    def first_skip_marker(self) -> int:
        """The offset of the first skip marker, or -1."""
        match = _SKIP_MARKER.search(self.body_text)
        return match.start() if match else -1

    @property
    def skips_before_asserting(self) -> bool:
        """True when the body reaches a skip marker and returns before any
        panic-capable construct.

        This is the shape in which a test reports *passed* having done
        nothing: the runner cannot see a runtime skip, so it counts the
        test as a pass. A runtime `return` is not a skip; nextest reports
        an `#[ignore]`d test as *skipped* and a self-returning test as
        *passed*.
        """
        skip = self.first_skip_marker()
        if skip < 0:
            return False
        panic = self.first_panic_construct()
        if 0 <= panic < skip:
            return False
        segment = self.body_text[skip : panic if panic >= 0 else len(self.body_text)]
        return _RETURN_STATEMENT.search(segment) is not None

    def normalised_body(self) -> str:
        """The body with comments dropped, literals marked, whitespace
        collapsed, and the function's own name dropped.

        The name is dropped as a **whole identifier**, not as a substring.
        A substring replace turns `foo` and `foo_impl` into the same text,
        which collapses seventeen distinct tests in
        `tests/server_feature/projection_transport_real.inc` into one
        false group -- every one of them delegates to a *different*
        `..._impl` helper.
        """
        if self.name:
            text = re.sub(rf"\b{re.escape(self.name)}\b", "SELF", self.body_digest)
        else:
            text = self.body_digest
        return re.sub(r"\s+", " ", text).strip()


# ---------------------------------------------------------------------------
# Lexing
# ---------------------------------------------------------------------------


def _raw_string_at(text: str, index: int) -> tuple[int, int, str] | None:
    """Parse a raw string literal starting at `index`, if one starts there.

    Handles `r"..."`, `r#"..."#`, `r##"..."##`, `br"..."` and `br#"..."#`.
    """
    limit = len(text)
    cursor = index
    if cursor < limit and text[cursor] == "b":
        cursor += 1
        if cursor >= limit or text[cursor] != "r":
            return None
    if cursor >= limit or text[cursor] != "r":
        return None
    cursor += 1
    hashes = 0
    while cursor < limit and text[cursor] == "#":
        hashes += 1
        cursor += 1
    if cursor >= limit or text[cursor] != '"':
        return None
    cursor += 1
    terminator = '"' + ("#" * hashes)
    end = text.find(terminator, cursor)
    if end < 0:
        return (index, limit, text[cursor:])
    return (index, end + len(terminator), text[cursor:end])


def _plain_string_at(text: str, index: int) -> tuple[int, str]:
    """Parse a plain string literal whose opening quote is at `index`."""
    limit = len(text)
    cursor = index + 1
    while cursor < limit:
        char = text[cursor]
        if char == "\\":
            cursor += 2
            continue
        if char == '"':
            return (cursor + 1, text[index + 1 : cursor])
        cursor += 1
    return (limit, text[index + 1 :])


def _char_literal_at(text: str, index: int) -> tuple[int, int, str] | None:
    """Parse a character literal at `index`, or return None for a lifetime.

    A lifetime is an apostrophe followed by an identifier and *not* by a
    closing quote. Getting this wrong is the failure mode that swallowed
    76% of `tests/`.
    """
    limit = len(text)
    if index + 1 >= limit:
        return None
    char = text[index + 1]
    if char == "\\":
        cursor = index + 1
        while cursor < limit:
            if text[cursor] == "\\":
                cursor += 2
                continue
            if text[cursor] == "'":
                return (index, cursor + 1, text[index + 1 : cursor])
            cursor += 1
        return None
    if index + 2 < limit and text[index + 2] == "'":
        return (index, index + 3, text[index + 1 : index + 2])
    return None


def scan_source(text: str) -> SourceScan:
    """Inventory every literal and comment in `text`."""
    literals: list[Literal] = []
    comments: list[Comment] = []
    index = 0
    limit = len(text)
    while index < limit:
        char = text[index]
        if char == "/" and index + 1 < limit and text[index + 1] == "/":
            end = text.find("\n", index)
            if end < 0:
                end = limit
            comments.append(Comment(index, end, "line"))
            index = end
            continue
        if char == "/" and index + 1 < limit and text[index + 1] == "*":
            depth = 1
            cursor = index + 2
            while cursor < limit and depth > 0:
                if text.startswith("/*", cursor):
                    depth += 1
                    cursor += 2
                elif text.startswith("*/", cursor):
                    depth -= 1
                    cursor += 2
                else:
                    cursor += 1
            comments.append(Comment(index, cursor, "block"))
            index = cursor
            continue
        raw = _raw_string_at(text, index)
        if raw is not None:
            start, end, content = raw
            literals.append(Literal(start, end, "raw", content))
            index = end
            continue
        if char == "b" and index + 1 < limit and text[index + 1] == '"':
            end, content = _plain_string_at(text, index + 1)
            literals.append(Literal(index, end, "byte", content))
            index = end
            continue
        if char == '"':
            end, content = _plain_string_at(text, index)
            literals.append(Literal(index, end, "string", content))
            index = end
            continue
        if char == "'":
            char_literal = _char_literal_at(text, index)
            if char_literal is not None:
                start, end, content = char_literal
                literals.append(Literal(start, end, "char", content))
                index = end
                continue
            index += 1
            continue
        index += 1
    literals_tuple = tuple(literals)
    comments_tuple = tuple(comments)
    events = [(lit.start, lit.end, lit.marker()) for lit in literals_tuple]
    events += [(com.start, com.end, "") for com in comments_tuple]
    events.sort()
    return SourceScan(text, literals_tuple, comments_tuple, tuple(events))


# ---------------------------------------------------------------------------
# Attributes
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Attribute:
    """One `#[...]` attribute."""

    start: int
    end: int
    inner: str
    is_inner: bool


def _iter_attributes(mask: str) -> list[Attribute]:
    """Every attribute in the masked text, in source order."""
    attributes: list[Attribute] = []
    index = 0
    limit = len(mask)
    while True:
        start = mask.find("#[", index)
        if start < 0:
            break
        inner_start = start + 2
        depth = 1
        cursor = inner_start
        while cursor < limit and depth > 0:
            if mask[cursor] == "[":
                depth += 1
            elif mask[cursor] == "]":
                depth -= 1
                if depth == 0:
                    break
            cursor += 1
        end = min(cursor + 1, limit)
        # `#![...]` is an inner attribute; `#[...]` is an outer one.
        is_inner = start >= 2 and mask[start - 1] == "#" and mask[start - 2] != "!"
        # Recover the inner text from the *original* source: the mask has
        # blanked any string literal inside the attribute's arguments.
        inner = _recover_inner(mask, start, end)
        attributes.append(Attribute(start, end, inner, is_inner))
        index = end
    return attributes


def _recover_inner(mask: str, start: int, end: int) -> str:
    """The attribute's inner text, with literals blanked.

    `#[tokio::test(flavor = "multi_thread")]` becomes
    `tokio::test(flavor = "multi_thread")` -- the literal is already a
    run of spaces in the mask, so the argument text is lost. This
    recovers it from the mask's shape, which is enough to identify the
    attribute, and keeps the mask length-preserving.
    """
    return mask[start + 2 : max(start + 2, end - 1)]


def is_test_attribute(inner: str) -> bool:
    """True for `#[test]` and `#[tokio::test]`, false for `#[test_case]`."""
    text = inner.strip()
    for prefix in _TEST_ATTRIBUTE_PREFIXES:
        if not text.startswith(prefix):
            continue
        if len(text) == len(prefix):
            return True
        following = text[len(prefix)]
        if not (following.isalnum() or following == "_"):
            return True
    return False


# ---------------------------------------------------------------------------
# Functions
# ---------------------------------------------------------------------------

_FN_KEYWORD = re.compile(r"\bfn\b")
_IDENTIFIER = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")


def _match_body_start(mask: str, from_index: int) -> int:
    """The index of the opening brace of a function body, or -1."""
    index = from_index
    limit = len(mask)
    paren = 0
    bracket = 0
    while index < limit:
        char = mask[index]
        if char == "(":
            paren += 1
        elif char == ")":
            paren -= 1
        elif char == "[":
            bracket += 1
        elif char == "]":
            bracket -= 1
        elif char == "{" and paren <= 0 and bracket <= 0:
            return index
        index += 1
    return -1


def _match_brace(mask: str, open_index: int) -> int:
    """The index of the brace closing the one at `open_index`, or -1."""
    depth = 0
    index = open_index
    limit = len(mask)
    while index < limit:
        char = mask[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return index
        index += 1
    return -1


# The gap between an attribute and the `fn` it governs may hold the
# function's modifiers. `#[tokio::test] async fn` is the common case:
# without this, the attribute is read as belonging to something else and
# the removal leaves `async` behind.
_MODIFIER_GAP = re.compile(
    r"(?:\s|async\b|unsafe\b|const\b|pub\b|pub\s*\([^)]*\)|extern\b)*"
)


def _is_modifier_gap(gap: str) -> bool:
    return _MODIFIER_GAP.fullmatch(gap) is not None


def _governing_attribute_spans(
    attributes: list[Attribute], mask: str, fn_keyword_start: int
) -> list[Attribute]:
    """The attributes that govern the function whose `fn` is at `fn_keyword_start`.

    The run is the maximal group of attributes separated from the `fn`
    keyword by whitespace and the function's modifiers only. A gap
    carrying any other token ends the group: that attribute belongs to
    something else.
    """
    governing: list[Attribute] = []
    cursor = fn_keyword_start
    for attribute in reversed(attributes):
        if attribute.end > fn_keyword_start:
            continue
        gap = mask[attribute.end : cursor]
        if not _is_modifier_gap(gap):
            break
        governing.append(attribute)
        cursor = attribute.start
    return list(reversed(governing))


def _governing_attributes(
    attributes: list[Attribute], mask: str, fn_keyword_start: int
) -> tuple[str, ...]:
    """The attribute texts that govern the function whose `fn` is at `fn_keyword_start`."""
    return tuple(
        attribute.inner
        for attribute in _governing_attribute_spans(
            attributes, mask, fn_keyword_start
        )
    )


@dataclass(frozen=True)
class TestTemplate:
    """A `#[test]` attribute inside a `macro_rules!` template.

    The attribute is a *generator*: it expands into one test function per
    metavariable binding. It is not a test function itself, so the parsed
    function count is legitimately lower than the attribute count. The
    residual is reported, not tolerated.
    """

    name: str
    rel: str
    tree: str
    line: int


def _find_test_templates(
    text: str, path: Path, rel: str, tree: str
) -> list[TestTemplate]:
    """Every `#[test]` attribute whose function name is a macro metavariable."""
    scan = scan_source(text)
    mask = scan.mask()
    templates: list[TestTemplate] = []
    for attribute in _iter_attributes(mask):
        if not is_test_attribute(attribute.inner):
            continue
        match = _FN_KEYWORD.search(mask, attribute.end)
        if match is None:
            continue
        cursor = match.end()
        while cursor < len(mask) and mask[cursor].isspace():
            cursor += 1
        if cursor >= len(mask) or mask[cursor] != "$":
            continue
        name_match = re.match(r"\$[A-Za-z_][A-Za-z0-9_]*", mask[cursor:])
        if name_match is None:
            continue
        templates.append(
            TestTemplate(
                name=name_match.group(0),
                rel=rel,
                tree=tree,
                line=text.count("\n", 0, cursor) + 1,
            )
        )
    return templates


def find_test_functions(text: str, path: Path, rel: str, tree: str) -> list[TestFunction]:
    """Every test function in one source file."""
    scan = scan_source(text)
    mask = scan.mask()
    attributes = _iter_attributes(mask)
    test_attributes = [
        attribute for attribute in attributes if is_test_attribute(attribute.inner)
    ]
    if not test_attributes:
        return []

    functions: list[TestFunction] = []
    for attribute in test_attributes:
        match = _FN_KEYWORD.search(mask, attribute.end)
        if match is None:
            continue
        name_cursor = match.end()
        while name_cursor < len(mask) and mask[name_cursor].isspace():
            name_cursor += 1
        name_match = _IDENTIFIER.match(mask, name_cursor)
        if name_match is None:
            continue
        name = name_match.group(0)
        body_start = _match_body_start(mask, name_match.end())
        if body_start < 0:
            continue
        body_end = _match_brace(mask, body_start)
        if body_end < 0:
            continue
        # `async fn` is preceded by `async`; the attribute match already
        # skipped to the `fn` keyword, so recover the modifier by looking
        # backwards at the mask between the attribute and the keyword.
        prefix = mask[attribute.end : match.start()]
        is_async = "async" in prefix.split()
        line = text.count("\n", 0, name_match.start()) + 1
        governing_spans = _governing_attribute_spans(attributes, mask, match.start())
        governing = tuple(attribute.inner for attribute in governing_spans)
        # The item starts at the first governing attribute, so removing
        # the item removes its attributes too.
        item_start = match.start()
        if governing_spans:
            item_start = governing_spans[0].start
        functions.append(
            TestFunction(
                name=name,
                file=path,
                rel=rel,
                tree=tree,
                line=line,
                body_start=body_start,
                body_end=body_end,
                item_start=item_start,
                is_async=is_async,
                attributes=governing,
                body_text=text[body_start : body_end + 1],
                body_digest=scan.digest_slice(body_start, body_end + 1),
            )
        )
    return functions


# ---------------------------------------------------------------------------
# Repository traversal
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class SourceFile:
    """One scanned source file."""

    path: Path
    rel: str
    tree: str
    text: str
    scan: SourceScan
    mask: str


def _tree_signature(root: Path) -> tuple:
    """A cheap signature of the source tree: path, mtime and size."""
    signature: list[tuple[str, int, int]] = []
    for tree in TREES:
        base = root / tree
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*")):
            if path.suffix not in (".rs", ".inc"):
                continue
            try:
                stat = path.stat()
            except OSError:
                continue
            signature.append((path.as_posix(), stat.st_mtime_ns, stat.st_size))
    return tuple(signature)


def iter_source_files(root: Path = REPO_ROOT) -> list[SourceFile]:
    """Every Rust source file in the three test-bearing trees."""
    key = str(root)
    signature = _tree_signature(root)
    cached = _SOURCE_CACHE.get(key)
    if cached is not None and cached[0] == signature:
        return cached[1]
    files: list[SourceFile] = []
    for tree in TREES:
        base = root / tree
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*.rs")):
            files.append(_source_file(path, root, tree))
        for path in sorted(base.rglob("*.inc")):
            files.append(_source_file(path, root, tree))
    _SOURCE_CACHE[key] = (signature, files)
    return files


def _source_file(path: Path, root: Path, tree: str) -> SourceFile:
    text = path.read_text(encoding="utf-8")
    rel = path.relative_to(root).as_posix()
    scan = scan_source(text)
    return SourceFile(path=path, rel=rel, tree=tree, text=text, scan=scan, mask=scan.mask())


def collect_test_functions(root: Path = REPO_ROOT) -> list[TestFunction]:
    """Every test function in the repository."""
    key = str(root)
    signature = _tree_signature(root)
    cached = _FUNCTION_CACHE.get(key)
    if cached is not None and cached[0] == signature:
        return cached[1]
    functions: list[TestFunction] = []
    for source in iter_source_files(root):
        functions.extend(
            find_test_functions(source.text, source.path, source.rel, source.tree)
        )
    _FUNCTION_CACHE[key] = (signature, functions)
    return functions


def ground_truth_attribute_counts(root: Path = REPO_ROOT) -> dict[str, int]:
    """The number of `#[test]` / `#[tokio::test]` attributes per tree.

    This is the ground truth the parsed function count is checked
    against. It is measured, not assumed.
    """
    counts = {tree: 0 for tree in TREES}
    for source in iter_source_files(root):
        for attribute in _iter_attributes(source.mask):
            if is_test_attribute(attribute.inner):
                counts[source.tree] += 1
    return counts


def parsed_function_counts(root: Path = REPO_ROOT) -> dict[str, int]:
    """The number of parsed test functions per tree."""
    counts = {tree: 0 for tree in TREES}
    for function in collect_test_functions(root):
        counts[function.tree] += 1
    return counts


def template_counts(root: Path = REPO_ROOT) -> dict[str, int]:
    """The number of `macro_rules!` test templates per tree."""
    counts = {tree: 0 for tree in TREES}
    for source in iter_source_files(root):
        for template in _find_test_templates(
            source.text, source.path, source.rel, source.tree
        ):
            counts[template.tree] += 1
    return counts


def newline_count(text: str) -> int:
    return text.count("\n")


def mask_newline_report(root: Path = REPO_ROOT) -> dict[str, list[str]]:
    """Files whose mask lost or gained a newline.

    A scanner that reports the right counts and the wrong line numbers
    is more dangerous than one that reports neither, because the removal
    tables downstream are built from line numbers.
    """
    mismatches: dict[str, list[str]] = {"lost": [], "gained": []}
    for source in iter_source_files(root):
        original = newline_count(source.text)
        masked = newline_count(source.mask)
        if masked < original:
            mismatches["lost"].append(source.rel)
        elif masked > original:
            mismatches["gained"].append(source.rel)
    return mismatches


def duplicate_groups(
    functions: list[TestFunction] | None = None, root: Path = REPO_ROOT
) -> list[list[TestFunction]]:
    """Every group of test functions with an identical normalised body.

    The comparison is **content-preserving**: a literal is replaced by a
    marker derived from its content, so two calls that differ only in a
    string argument stay distinct while two identical calls stay equal.
    Blanking literals instead would report 456 groups and 1,084 redundant
    copies on this tree -- off by a factor of five -- because every
    distinct command tested by the same shape of assertion collapses into
    one group.
    """
    if functions is None:
        functions = collect_test_functions(root)
    buckets: dict[str, list[TestFunction]] = {}
    for function in functions:
        buckets.setdefault(function.normalised_body(), []).append(function)
    return [
        sorted(group, key=lambda fn: (fn.rel, fn.line))
        for group in buckets.values()
        if len(group) > 1
    ]


def is_cross_file(group: list[TestFunction]) -> bool:
    """True when the group's members live in more than one file."""
    return len({fn.rel for fn in group}) > 1


@dataclass(frozen=True)
class Function:
    """Any function item, not only a test."""

    name: str
    rel: str
    tree: str
    line: int
    item_start: int
    body_end: int
    body_text: str

    @property
    def has_panic_construct(self) -> bool:
        return _PANIC_CONSTRUCT.search(self.body_text) is not None


def find_functions(text: str, path: Path, rel: str, tree: str) -> list[Function]:
    """Every function item in one source file.

    The guard needs this to resolve a *delegating* test body: a test whose
    only statement is a call to a helper that itself asserts is not
    vacuous, and the helper is named in the guard's report.
    """
    scan = scan_source(text)
    mask = scan.mask()
    attributes = _iter_attributes(mask)
    functions: list[Function] = []
    for match in _FN_KEYWORD.finditer(mask):
        name_cursor = match.end()
        while name_cursor < len(mask) and mask[name_cursor].isspace():
            name_cursor += 1
        name_match = _IDENTIFIER.match(mask, name_cursor)
        if name_match is None:
            continue
        body_start = _match_body_start(mask, name_match.end())
        if body_start < 0:
            continue
        body_end = _match_brace(mask, body_start)
        if body_end < 0:
            continue
        governing = _governing_attribute_spans(attributes, mask, match.start())
        item_start = match.start()
        if governing:
            item_start = governing[0].start
        functions.append(
            Function(
                name=name_match.group(0),
                rel=rel,
                tree=tree,
                line=text.count("\n", 0, name_match.start()) + 1,
                item_start=item_start,
                body_end=body_end,
                body_text=text[body_start : body_end + 1],
            )
        )
    return functions


def local_function_map(root: Path = REPO_ROOT) -> dict[str, dict[str, Function]]:
    """Every function in the repository, keyed by file then by name."""
    key = str(root)
    signature = _tree_signature(root)
    cached = _FUNCTION_MAP_CACHE.get(key)
    if cached is not None and cached[0] == signature:
        return cached[1]
    mapping: dict[str, dict[str, Function]] = {}
    for source in iter_source_files(root):
        mapping[source.rel] = {
            fn.name: fn for fn in find_functions(source.text, source.path, source.rel, source.tree)
        }
    _FUNCTION_MAP_CACHE[key] = (signature, mapping)
    return mapping


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------


def _fixture(source: str) -> SourceScan:
    return scan_source(source)


def _self_test() -> int:
    """Exercise the lexer against each measured failure mode."""
    failures: list[str] = []

    def check(name: str, condition: bool, detail: str = "") -> None:
        if condition:
            print(f"  ok: {name}")
        else:
            print(f"  FAIL: {name} {detail}")
            failures.append(name)

    # Failure mode 1: a lifetime must not be read as a character literal.
    source = "fn f<'a>(x: &'a str) -> &'a str { x }\n"
    scan = _fixture(source)
    check(
        "a lifetime is not a character literal",
        not scan.literals,
        f"got {[lit.content for lit in scan.literals]}",
    )
    char_scan = _fixture("fn f() { let c = 'x'; }\n")
    check(
        "a character literal is a literal",
        len(char_scan.literals) == 1 and char_scan.literals[0].kind == "char",
        f"got {char_scan.literals}",
    )
    escape_scan = _fixture(r"fn f() { let c = '\u{1F600}'; let d = '\''; }" + "\n")
    check(
        "escaped character literals are literals",
        len(escape_scan.literals) == 2,
        f"got {escape_scan.literals}",
    )
    # A character literal holding an escaped backslash: `'\\'`. Reading
    # the escape as a unit must leave the *following* quote as the
    # closing quote. Getting this wrong swallows the rest of the file --
    # it cost 2,606 characters of `src/plugin/package.rs` once.
    backslash_scan = _fixture(r"fn f() { let c = '\\'; let d = 1; }" + "\n")
    check(
        "a character literal holding an escaped backslash is one literal",
        len(backslash_scan.literals) == 1
        and backslash_scan.literals[0].end - backslash_scan.literals[0].start == 4,
        f"got {backslash_scan.literals}",
    )

    # Failure mode 2: a raw string must not start at a closing quote.
    raw_source = 'fn a() { let s = "plain"; }\nfn b() { let r = r#"raw "quoted" text"#; }\n'
    raw_scan = _fixture(raw_source)
    check(
        "a plain string and a raw string are both literals",
        len(raw_scan.literals) == 2,
        f"got {[(lit.kind, lit.content) for lit in raw_scan.literals]}",
    )
    check(
        "a raw string keeps its own content",
        raw_scan.literals[1].content == 'raw "quoted" text',
        f"got {raw_scan.literals[1].content!r}",
    )
    hash_scan = _fixture('fn f() { let r = r##"a "# b"##; }\n')
    check(
        "a two-hash raw string is one literal",
        len(hash_scan.literals) == 1 and hash_scan.literals[0].content == 'a "# b',
        f"got {hash_scan.literals}",
    )
    byte_scan = _fixture('fn f() { let b = b"bytes"; let c = br#"raw bytes"#; }\n')
    check(
        "byte strings are literals",
        len(byte_scan.literals) == 2,
        f"got {byte_scan.literals}",
    )

    # Failure mode 3: `#[tokio::test]` is followed by `async fn`.
    tokio_source = """
#[tokio::test]
async fn works() { assert!(true); }
#[test]
fn plain() { assert!(true); }
"""
    tokio_functions = find_test_functions(
        tokio_source, Path("fixture.rs"), "fixture.rs", "tests"
    )
    check(
        "an async test after #[tokio::test] is found",
        {fn.name for fn in tokio_functions} == {"works", "plain"},
        f"got {[fn.name for fn in tokio_functions]}",
    )
    check(
        "the async test is marked async",
        any(fn.is_async for fn in tokio_functions),
    )
    flavor_source = """
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn flavored() { assert!(true); }
"""
    flavor_functions = find_test_functions(
        flavor_source, Path("fixture.rs"), "fixture.rs", "tests"
    )
    check(
        "a #[tokio::test(...)] attribute is a test attribute",
        len(flavor_functions) == 1,
        f"got {flavor_functions}",
    )
    check(
        "#[test_case] is not a test attribute",
        not is_test_attribute("test_case"),
    )
    check(
        "#[tokio::test_with] is not a test attribute",
        not is_test_attribute("tokio::test_with"),
    )

    # Failure mode 4: the mask must keep the original's newline count.
    multiline_source = 'fn f() {\n    let s = r#"line one\nline two"#;\n    let t = "multi\nline";\n}\n'
    multiline_scan = _fixture(multiline_source)
    check(
        "the mask keeps the newline count",
        newline_count(multiline_scan.mask()) == newline_count(multiline_source),
        f"got {newline_count(multiline_scan.mask())} vs {newline_count(multiline_source)}",
    )
    check(
        "the mask keeps the length",
        len(multiline_scan.mask()) == len(multiline_source),
    )

    # Nested block comments are skipped whole.
    nested_source = "fn f() { /* outer /* inner */ still outer */ let s = \"x\"; }\n"
    nested_scan = _fixture(nested_source)
    check(
        "a nested block comment is one comment",
        len(nested_scan.comments) == 1,
        f"got {nested_scan.comments}",
    )
    check(
        "a string after a nested comment is still a literal",
        len(nested_scan.literals) == 1,
        f"got {nested_scan.literals}",
    )

    # A literal containing a brace, a quote and a `//` must not confuse
    # the body matcher.
    tricky_source = """
#[test]
fn tricky() {
    let s = "a } b \\" c // d";
    assert!(s.contains('}'));
}
"""
    tricky_functions = find_test_functions(
        tricky_source, Path("fixture.rs"), "fixture.rs", "tests"
    )
    check(
        "a literal with a brace does not truncate the body",
        len(tricky_functions) == 1
        and tricky_functions[0].body_text.count("{") == 1
        and tricky_functions[0].body_text.rstrip().endswith("}"),
        f"got {tricky_functions}",
    )

    # An unbalanced brace file must not hang or raise.
    unbalanced = find_test_functions(
        "#[test]\nfn broken() { let x = 1;\n", Path("fixture.rs"), "fixture.rs", "tests"
    )
    check("an unbalanced file yields no function", not unbalanced, f"got {unbalanced}")

    # The content-preserving digest must distinguish two calls that differ
    # only in a string argument, and must not collapse them.
    distinct_source = """
#[test]
fn one() { classify("find"); }
#[test]
fn two() { classify("brew"); }
"""
    distinct = find_test_functions(
        distinct_source, Path("fixture.rs"), "fixture.rs", "tests"
    )
    check(
        "bodies differing only in a string argument are not duplicates",
        distinct[0].normalised_body() != distinct[1].normalised_body(),
        f"got {distinct[0].normalised_body()!r} vs {distinct[1].normalised_body()!r}",
    )
    same_source = """
#[test]
fn one() { classify("find"); }
#[test]
fn two() { classify("find"); }
"""
    same = find_test_functions(same_source, Path("fixture.rs"), "fixture.rs", "tests")
    check(
        "bodies with an identical argument are duplicates",
        same[0].normalised_body() == same[1].normalised_body(),
        f"got {same[0].normalised_body()!r} vs {same[1].normalised_body()!r}",
    )

    # A blanked-literal body must NOT be accepted as equivalent: this is
    # the failure mode that produced a spurious 49-copy group.
    blanked_one = scan_source('fn f() { classify("find"); }').digest()
    blanked_two = scan_source('fn f() { classify("brew"); }').digest()
    check(
        "a blanked-literal body is not accepted as equivalent",
        blanked_one != blanked_two,
    )

    # An `#[ignore]`d test is recognised.
    ignore_source = """
#[test]
#[ignore]
fn skipped() { assert!(true); }
"""
    ignore_functions = find_test_functions(
        ignore_source, Path("fixture.rs"), "fixture.rs", "tests"
    )
    check(
        "an #[ignore]d test is recognised",
        len(ignore_functions) == 1 and ignore_functions[0].is_ignored,
        f"got {ignore_functions}",
    )

    # `#[tokio::test] async fn` — the attribute and the `fn` are separated
    # by the `async` modifier, and the attribute still governs the
    # function. Getting this wrong leaves `async` behind when the item
    # is removed.
    tokio_async_source = """
#[tokio::test(flavor = "current_thread")]
async fn works() { assert!(true); }
"""
    tokio_async = find_test_functions(
        tokio_async_source, Path("fixture.rs"), "fixture.rs", "tests"
    )
    check(
        "a #[tokio::test] attribute governs an async fn",
        len(tokio_async) == 1
        and tokio_async[0].is_async
        and tokio_async[0].item_text(tokio_async_source).startswith("#[tokio::test"),
        f"got {tokio_async}",
    )

    # A test whose body delegates to a helper named `<its own name>_impl`
    # must not be reported as a duplicate of a sibling that delegates to a
    # different helper. Dropping the name as a substring instead of as a
    # whole identifier collapses both into `_impl()`.
    delegate_source = """
#[test]
fn peer_close() { runtime().block_on(peer_close_impl()); }
#[test]
fn raw_source() { runtime().block_on(raw_source_impl()); }
"""
    delegate = find_test_functions(
        delegate_source, Path("fixture.rs"), "fixture.rs", "tests"
    )
    check(
        "a body delegating to a differently-named helper is not a duplicate",
        delegate[0].normalised_body() != delegate[1].normalised_body(),
        f"got {delegate[0].normalised_body()!r} vs {delegate[1].normalised_body()!r}",
    )

    # Two bodies that call the *same* helper are duplicates.
    same_delegate_source = """
#[test]
fn peer_close() { runtime().block_on(shared_impl()); }
#[test]
fn raw_source() { runtime().block_on(shared_impl()); }
"""
    same_delegate = find_test_functions(
        same_delegate_source, Path("fixture.rs"), "fixture.rs", "tests"
    )
    check(
        "two bodies delegating to the same helper are duplicates",
        same_delegate[0].normalised_body() == same_delegate[1].normalised_body(),
    )

    # The duplicate detector groups identical bodies and keeps
    # differently-argumented bodies apart.
    detector_source = """
#[test]
fn one() { classify("find"); }
#[test]
fn two() { classify("find"); }
#[test]
fn three() { classify("brew"); }
"""
    detector_functions = find_test_functions(
        detector_source, Path("fixture.rs"), "fixture.rs", "tests"
    )
    groups = duplicate_groups(detector_functions)
    check(
        "the detector reports exactly one duplicate group",
        len(groups) == 1 and len(groups[0]) == 2,
        f"got {[[fn.name for fn in g] for g in groups]}",
    )
    check(
        "a single-member body is not a group",
        not duplicate_groups(detector_functions[:1]),
    )

    if failures:
        print(f"rust_test_scan --self-test: {len(failures)} failing")
        return 1
    print("rust_test_scan --self-test: all checks pass")
    return 0


def _validate() -> int:
    """Check the scanner against the repository's ground truth."""
    print("rust_test_scan --validate")
    truth = ground_truth_attribute_counts()
    parsed = parsed_function_counts()
    templates = template_counts()
    failures = 0
    for tree in TREES:
        # Every test attribute is either a parsed test function or a
        # `macro_rules!` template. A residual that is neither is a
        # scanner defect and is reported, not tolerated.
        residual = truth[tree] - parsed[tree] - templates[tree]
        if residual == 0:
            status = "ok"
        else:
            status = "FAIL"
            failures += 1
        print(
            f"  {status}: {tree}: {truth[tree]} test attributes = "
            f"{parsed[tree]} parsed functions + {templates[tree]} macro templates "
            f"(residual {residual})"
        )
    mismatches = mask_newline_report()
    for direction, files in mismatches.items():
        if files:
            failures += 1
            print(f"  FAIL: mask {direction} a newline in {len(files)} files")
            for rel in files[:10]:
                print(f"    {rel}")
        else:
            print(f"  ok: mask {direction} no newline in any file")
    if failures:
        print(f"rust_test_scan --validate: {failures} failing")
        return 1
    print("rust_test_scan --validate: all checks pass")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        print("rust_test_scan --self-test")
        return _self_test()
    if "--validate" in sys.argv:
        return _validate()
    print(__doc__)
    print("\nusage: rust_test_scan.py [--self-test | --validate]")
    return 0


if __name__ == "__main__":
    sys.exit(main())
