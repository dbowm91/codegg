#!/usr/bin/env python3
"""Static guard: the TUI builds one apply request, in one place.

Milestone M016 of `plans/implementation/tui-editor-helix-parity/` gives the
editor's LSP previews a way to become an applicable, reviewed edit. The
editor's preview used to be a *read result* — a picture of what would change —
and ADR-0008's apply path consumes a *candidate*: the patches, the base each
was computed against, and the digest that binds a request to them. M016
carries that candidate from the daemon's read result to the review surface, and
the review's accept sends it.

That is a second way to reach an apply, and the second way is the dangerous
one. Two apply paths drift: one of them grows a weaker precondition, a
different digest binding, or a different session scope, and the user approves
a change under one set of rules that another path does not honour. The
invariant has to be *structural* — one function builds the request, everyone
calls it — or it is a convention that survives exactly until it does not.

The compiler cannot see this. Two builders are two correct programs. The
failure is a semantic one: the same write path, reached two ways, with two
precondition sets.

## What this guard reads

Every `.rs` file under `src/tui/`, and nothing else. It looks for
`CoreRequest::LspPreviewApply` — the request variant — and classifies each
occurrence:

* a **construction** is an expression that builds the variant;
* a **pattern** is a `let` binding or a `match` arm that takes it apart.

Test modules and comments are excluded. A test that asserts on the variant's
shape is not an apply path; a doc comment that *names* it is not one either.

## What counts as a violation

**Violations** (exit code 1):

- **Rule 1 — the variant appears outside the one builder's file.** Any other
  file under `src/tui/` that mentions `CoreRequest::LspPreviewApply` is
  building or matching an apply request outside the builder. That is a second
  apply path, however small.
- **Rule 2 — the builder's file does not contain exactly one construction.**
  Zero constructions means the builder stopped building the request; two
  means a second construction crept in beside it.
- **Rule 3 — the construction is not inside the builder function.** A
  construction in the same file but in another function is still a second
  place to build an apply.
- **Rule 4 — the builder function is gone.** The invariant the rest of the
  codebase documents ("there is one apply path") names a function; if the
  function disappears, the claim is stale even if the file still compiles.

## What the guard requires to still be true

- `pub fn build_apply_request(` — the one builder.
- `CoreRequest::LspPreviewApply` — the request variant it builds.

## Non-vacuity

`--self-test` runs synthetic sources through the same analysis the real tree
goes through, and each case states which rule it proves. The real tree's
non-vacuity is proved separately, by injection: adding a second construction
to the builder's file, or mentioning the variant in another file, must make
this guard fail.

Exit code 1 on violations, 2 on a shape the guard cannot resolve.

Usage::

    python3 scripts/check_tui_editor_apply_path.py
    python3 scripts/check_tui_editor_apply_path.py --self-test

This is invoked by `scripts/verify.sh quick` and the CI `verify` job.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
TUI_ROOT = REPO_ROOT / "src" / "tui"

# The file that owns the one builder.
THE_BUILDER_FILE = Path("src/tui/commands/change_review.rs")
# The one builder's name.
THE_BUILDER_FN = "build_apply_request"
# The request variant. Matched exactly, so `LspPreviewApplyRequestDto` and
# `LspPreviewApplyResult` — different types, in the same family — do not
# count as apply paths.
VARIANT = "CoreRequest::LspPreviewApply"

BUILDER_RE = re.compile(r"\bfn\s+" + THE_BUILDER_FN + r"\s*\(")


class ParseError(Exception):
    """A shape the guard cannot resolve."""


@dataclass(frozen=True)
class Occurrence:
    """One mention of the request variant."""

    path: Path
    line_no: int
    offset: int
    kind: str  # "construction" or "pattern"


@dataclass(frozen=True)
class Violation:
    """One violation, tagged with the rule that found it."""

    rule: str
    path: Path
    line_no: int
    detail: str


# ── Text surgery ─────────────────────────────────────────────────────────────


def strip_comments(text: str) -> str:
    """Comment-free text with the line count preserved, so lines stay true.

    Both `//` and `/* */` are removed. String and char literals are
    respected: a `"` inside a literal is not the end of one, and a `/*`
    inside a literal is not a comment.
    """
    out: list[str] = []
    index = 0
    length = len(text)
    in_string = False
    in_char = False
    in_block_comment = False
    while index < length:
        char = text[index]
        if in_block_comment:
            if text[index : index + 2] == "*/":
                in_block_comment = False
                out.append("  ")
                index += 2
                continue
            out.append("\n" if char == "\n" else " ")
            index += 1
            continue
        if in_string:
            out.append(char)
            if char == "\\" and index + 1 < length:
                out.append(text[index + 1])
                index += 2
                continue
            if char == '"':
                in_string = False
            index += 1
            continue
        if in_char:
            out.append(char)
            if char == "\\" and index + 1 < length:
                out.append(text[index + 1])
                index += 2
                continue
            if char == "'":
                in_char = False
            index += 1
            continue
        if text[index : index + 2] == "//":
            while index < length and text[index] != "\n":
                out.append(" ")
                index += 1
            continue
        if text[index : index + 2] == "/*":
            in_block_comment = True
            out.append("  ")
            index += 2
            continue
        if char == '"':
            in_string = True
        elif char == "'":
            in_char = True
        out.append(char)
        index += 1
    return "".join(out)


def strip_test_modules(text: str) -> str:
    """Blank out `#[cfg(test)]` modules, keeping the line count.

    A test that matches on the request variant is not an apply path. The
    module is removed by brace matching, so the removal is exact rather than
    a guess about where the module ends. The attribute and the `mod` line
    are often separate, so the search looks forward for the module's
    opening brace rather than requiring them to share a line.
    """
    lines = text.splitlines(keepends=True)
    out = list(lines)
    index = 0
    while index < len(lines):
        if "cfg(test)" not in lines[index]:
            index += 1
            continue
        # Find the module's opening brace: the next line that opens one.
        depth = 0
        started = False
        line_index = index
        while line_index < len(lines):
            line = lines[line_index]
            if line_index != index:
                out[line_index] = "\n" * line.count("\n")
            for char in line:
                if char == "{":
                    depth += 1
                    started = True
                elif char == "}":
                    depth -= 1
            if started and depth <= 0:
                break
            line_index += 1
        index = line_index + 1
    return "".join(out)


def _matching_brace(text: str, open_index: int) -> int:
    """The index of the `}` that closes the `{` at `open_index`."""
    depth = 0
    index = open_index
    while index < len(text):
        char = text[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return index
        index += 1
    raise ParseError("an unbalanced brace near the request variant")


def classify(text: str, path: Path) -> list[Occurrence]:
    """Every mention of the request variant, construction or pattern."""
    occurrences: list[Occurrence] = []
    offset = 0
    while True:
        found = text.find(VARIANT, offset)
        if found < 0:
            break
        brace = text.find("{", found)
        if brace < 0:
            raise ParseError(f"{path}: the request variant has no body")
        close = _matching_brace(text, brace)
        tail = text[close + 1 :].lstrip()
        # A match arm looks like `Ok(... { .. }) => {}`: the `=>` follows the
        # closing paren, not the closing brace.
        if tail.startswith(")"):
            tail = tail[1:].lstrip()
        if tail.startswith("=>"):
            kind = "pattern"
        elif tail.startswith("=") and not tail.startswith("=="):
            kind = "pattern"
        else:
            kind = "construction"
        line_no = text.count("\n", 0, found) + 1
        occurrences.append(Occurrence(path=path, line_no=line_no, offset=found, kind=kind))
        offset = close + 1
    return occurrences


def builder_span(text: str) -> tuple[int, int]:
    """The byte range of the one builder's body."""
    match = BUILDER_RE.search(text)
    if match is None:
        raise ParseError(f"the builder `{THE_BUILDER_FN}` is gone")
    brace = text.find("{", match.end())
    if brace < 0:
        raise ParseError("the builder has no body")
    close = _matching_brace(text, brace)
    return brace, close


# ── The analysis ─────────────────────────────────────────────────────────────


def analyse(files: dict[Path, str]) -> list[Violation]:
    """Every violation in the given sources."""
    violations: list[Violation] = []

    # Rule 1: the variant outside the builder's file.
    for path, text in files.items():
        if path == THE_BUILDER_FILE:
            continue
        body = strip_test_modules(strip_comments(text))
        for occurrence in classify(body, path):
            violations.append(
                Violation(
                    rule="rule-1",
                    path=path,
                    line_no=occurrence.line_no,
                    detail=(
                        f"`{VARIANT}` is mentioned outside "
                        f"{THE_BUILDER_FILE.as_posix()}: a second apply path"
                    ),
                )
            )

    builder_text = files.get(THE_BUILDER_FILE)
    if builder_text is None:
        violations.append(
            Violation(
                rule="rule-4",
                path=THE_BUILDER_FILE,
                line_no=0,
                detail=f"{THE_BUILDER_FILE.as_posix()} is missing — the one builder's file is gone",
            )
        )
        return violations

    body = strip_test_modules(strip_comments(builder_text))
    occurrences = classify(body, THE_BUILDER_FILE)
    constructions = [o for o in occurrences if o.kind == "construction"]

    # Rule 4: the builder function itself.
    try:
        start, end = builder_span(body)
    except ParseError as error:
        violations.append(
            Violation(
                rule="rule-4",
                path=THE_BUILDER_FILE,
                line_no=0,
                detail=str(error),
            )
        )
        return violations

    # Rule 2: exactly one construction.
    if len(constructions) != 1:
        violations.append(
            Violation(
                rule="rule-2",
                path=THE_BUILDER_FILE,
                line_no=constructions[0].line_no if constructions else 0,
                detail=(
                    f"expected exactly one construction of `{VARIANT}` in "
                    f"{THE_BUILDER_FILE.as_posix()}, found {len(constructions)}"
                ),
            )
        )

    # Rule 3: every construction is inside the builder.
    for construction in constructions:
        if not (start <= construction.offset <= end):
            violations.append(
                Violation(
                    rule="rule-3",
                    path=THE_BUILDER_FILE,
                    line_no=construction.line_no,
                    detail=(
                        f"`{VARIANT}` is constructed outside `{THE_BUILDER_FN}`: "
                        "that is a second place to build an apply request"
                    ),
                )
            )
    return violations


def rust_sources(root: Path) -> dict[Path, str]:
    """Every Rust source under `root`, keyed by its path relative to the repo."""
    sources: dict[Path, str] = {}
    for path in sorted(root.rglob("*.rs")):
        sources[path.relative_to(REPO_ROOT)] = path.read_text(encoding="utf-8")
    return sources


# ── Self-test ────────────────────────────────────────────────────────────────


@dataclass(frozen=True)
class SelfTestCase:
    """One self-test case: a source, and the rule it must trigger."""

    name: str
    proves: str
    files: dict[Path, str]
    expect_rule: str | None


BUILDER_BODY = """
pub fn build_apply_request(app: &App) -> Result<CoreRequest, ApplyRefusal> {
    Ok(crate::protocol::core::CoreRequest::LspPreviewApply { request })
}
"""


SELF_TEST_CASES = [
    SelfTestCase(
        name="one construction in the builder is clean",
        proves="the guard does not fire on the shape it exists to accept",
        files={THE_BUILDER_FILE: BUILDER_BODY},
        expect_rule=None,
    ),
    SelfTestCase(
        name="a second construction beside the builder fires rule 2",
        proves="a second construction in the same file is caught",
        files={
            THE_BUILDER_FILE: BUILDER_BODY
            + "\nfn sneaky() -> CoreRequest {\n    CoreRequest::LspPreviewApply { request: r }\n}\n"
        },
        expect_rule="rule-2",
    ),
    SelfTestCase(
        name="a construction outside the builder fires rule 3",
        proves="the construction has to be inside the one builder",
        files={
            THE_BUILDER_FILE: """
pub fn build_apply_request(app: &App) -> Result<CoreRequest, ApplyRefusal> {
    let request = app.request();
    Ok(crate::protocol::core::CoreRequest::LspPreviewApply { request })
}

fn elsewhere(request: Request) -> CoreRequest {
    CoreRequest::LspPreviewApply { request }
}
"""
        },
        expect_rule="rule-3",
    ),
    SelfTestCase(
        name="a mention in another file fires rule 1",
        proves="a second file touching the variant is a second apply path",
        files={
            THE_BUILDER_FILE: BUILDER_BODY,
            Path("src/tui/commands/editor.rs"): """
fn build_it(request: Request) -> CoreRequest {
    CoreRequest::LspPreviewApply { request }
}
""",
        },
        expect_rule="rule-1",
    ),
    SelfTestCase(
        name="a missing builder fires rule 4",
        proves="the invariant names a function; a renamed one is caught",
        files={
            THE_BUILDER_FILE: """
pub fn build_apply_requests(app: &App) -> Result<CoreRequest, ApplyRefusal> {
    Ok(crate::protocol::core::CoreRequest::LspPreviewApply { request })
}
"""
        },
        expect_rule="rule-4",
    ),
    SelfTestCase(
        name="a test module is not an apply path",
        proves="tests that assert on the variant do not count as paths",
        files={
            THE_BUILDER_FILE: BUILDER_BODY
            + """
#[cfg(test)]
mod tests {
    #[test]
    fn shape() {
        match Ok::<(), ()>(()) {
            Ok(crate::protocol::core::CoreRequest::LspPreviewApply { .. }) => {}
            _ => {}
        }
    }
}
"""
        },
        expect_rule=None,
    ),
    SelfTestCase(
        name="a doc comment naming the variant is not an apply path",
        proves="a comment that names the variant does not count as one",
        files={
            THE_BUILDER_FILE: """
/// This is the single place a `CoreRequest::LspPreviewApply` is constructed.
pub fn build_apply_request(app: &App) -> Result<CoreRequest, ApplyRefusal> {
    Ok(crate::protocol::core::CoreRequest::LspPreviewApply { request })
}
"""
        },
        expect_rule=None,
    ),
]


def run_self_test() -> int:
    failures = 0
    for case in SELF_TEST_CASES:
        violations = analyse(case.files)
        rules = {violation.rule for violation in violations}
        if case.expect_rule is None:
            if violations:
                failures += 1
                print(f"FAIL {case.name}: expected no violation, got {sorted(rules)}")
            else:
                print(f"ok   {case.name} ({case.proves})")
            continue
        if case.expect_rule not in rules:
            failures += 1
            print(
                f"FAIL {case.name}: expected {case.expect_rule}, got "
                f"{sorted(rules) or 'no violation'}"
            )
        else:
            print(f"ok   {case.name} ({case.proves})")
    if failures:
        print(f"\nself-test FAILED: {failures} case(s) — the guard does not discriminate.")
        return 1
    print(f"\nself-test passed: {len(SELF_TEST_CASES)} case(s).")
    return 0


def main(argv: list[str]) -> int:
    unknown = [argument for argument in argv if argument != "--self-test"]
    if unknown:
        print(f"unknown argument(s): {unknown}", file=sys.stderr)
        return 2
    if "--self-test" in argv:
        return run_self_test()

    try:
        files = rust_sources(TUI_ROOT)
    except OSError as error:
        print(f"cannot read the TUI sources: {error}", file=sys.stderr)
        return 2

    try:
        violations = analyse(files)
    except ParseError as error:
        print(f"cannot resolve the source shape: {error}", file=sys.stderr)
        return 2

    if violations:
        for violation in violations:
            print(
                f"{violation.path}:{violation.line_no} [{violation.rule}] {violation.detail}",
                file=sys.stderr,
            )
        print(f"\n{len(violations)} violation(s) found.", file=sys.stderr)
        return 1

    print("TUI editor apply-path guard: one builder, one construction, no second path.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
