#!/usr/bin/env python3
"""Guard: every diff consumer fences its Git read through the consistency fence.

WHAT THIS GUARD DOES
    A Git read carries a **consistency class** — the object generation it
    observed. A consumer that needs one stable generation (a diff that
    describes what is about to be published) must not receive the bytes
    without that class, and must not receive them at all when the class is
    inadmissible. `egggit` supplies the vocabulary (`GitReadFact`,
    `GenerationConsistent`) and `crate::git_consistency::admit_generation_bound`
    is the fence.

    The fence is easy to declare and easy to drop: a consumer that calls
    `.into_value()` on a diff fact takes the content and silently discards
    the class, and every other check in the tree still passes. This guard
    asserts, for every diff consumer in `src/`:

    1. **No direct consumption.** A diff read must not be chained straight
       into `.into_value()`, and a value bound from a diff read must not be
       consumed by `.into_value()` in the same scope. `.into_value()` is the
       shape that discards the consistency class.

    2. **The fence is reached.** The consuming scope must reach
       `admit_generation_bound(` — directly, or through a local helper it
       calls (the "one admission door" shape) — or propagate the read's
       class onward with `.class()`. A scope that neither admits the read
       nor propagates its class has dropped it.

WHAT THIS GUARD DELIBERATELY DOES NOT DO
    It does not prove that the fence is *correct*, only that it is
    *reached*. A fence that admits everything would pass this guard; the
    fence's own behaviour is proved by the Rust tests next to it.

    It does not follow a diff fact across more than one call. A consumer
    that hands its fact to a helper in another module is checked at the
    call site, not inside that helper.

    It scans `src/` only. `crates/egggit` is where the read is *produced*,
    not consumed; a consumer outside `src/` is a deliberate decision that
    this guard does not cover.

USAGE
    python3 scripts/check_git_consistency_consumers.py            # check the tree
    python3 scripts/check_git_consistency_consumers.py --self-test  # prove it bites
"""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional, Tuple


ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "src"

# The egggit diff reads. Each of them returns a `GitReadFact`, so each of
# them carries a consistency class that a generation-bound consumer must
# not drop.
DIFF_READ_FUNCTIONS: Tuple[str, ...] = (
    "diff_summary",
    "file_diff",
    "diff_text",
    "changed_files",
)

# The fence.
FENCE_FUNCTION = "admit_generation_bound"

# The method that propagates a read's class onward.
CLASS_PROPAGATION = ".class("

# The method that discards a read's class.
VALUE_CONSUMPTION = ".into_value("

_DIFF_CALL_RE = re.compile(r"\b(" + "|".join(DIFF_READ_FUNCTIONS) + r")\s*\(")
_FUNCTION_RE = re.compile(
    r"(?m)^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?"
    r"(?:const[ \t]+)?(?:async[ \t]+)?(?:unsafe[ \t]+)?"
    r"(?:extern[ \t]+r?\"[^\"]*\"[ \t]+)?fn[ \t]+(\w+)"
)
_LET_BINDING_RE = re.compile(
    r"let[ \t]+(?:mut[ \t]+)?(\w+)[ \t]*=[ \t]*(?:[\w]+::[ \t]*)*$"
)
_CALL_RE = re.compile(r"\b(\w+)[ \t]*\(")

# The regions that are not code: comments, string literals (including raw
# ones), and character literals. A lifetime is not a character literal —
# `<'a, 'b>` has no closing quote after the `a` — so the character-literal
# form only matches a quoted single character.
_MASK_RE = re.compile(
    r"r(?P<hashes>#+)\"(?:[^\"]|\"(?!(?P=hashes)))*\"(?P=hashes)"
    r"|r\"[^\"]*\""
    r"|//[^\n]*"
    r"|/\*.*?\*/"
    r"|\"(?:\\.|[^\"\\\n])*\""
    r"|'(?:\\.|[^\\'\n])'",
    re.DOTALL,
)


@dataclass
class Finding:
    rule: str
    file: str
    line: int
    message: str


@dataclass
class FunctionSpan:
    name: str
    start: int
    end: int


def _mask_non_code(text: str) -> str:
    """Blank out comments and literal contents, preserving every offset.

    Brace matching and structural scanning over Rust source are only
    reliable when a brace inside a string literal or a comment is not
    counted, so the mask replaces those regions with spaces and leaves the
    length of the text untouched.
    """
    return _MASK_RE.sub(lambda match: re.sub(r"[^\n]", " ", match.group(0)), text)


def _brace_matches(masked: str) -> Dict[int, int]:
    """Map each `{` offset to the offset of the `}` that closes it.

    One pass over the masked text, with a stack, so the cost does not grow
    with the square of the file size the way a per-function scan would.
    """
    matches: Dict[int, int] = {}
    stack: List[int] = []
    for index, char in enumerate(masked):
        if char == "{":
            stack.append(index)
        elif char == "}" and stack:
            matches[stack.pop()] = index
    return matches


def _function_spans(masked: str, braces: Dict[int, int]) -> List[FunctionSpan]:
    """Every function in the file, with the extent of its body."""
    spans: List[FunctionSpan] = []
    for match in _FUNCTION_RE.finditer(masked):
        open_at = masked.find("{", match.end())
        if open_at == -1:
            continue
        end = braces.get(open_at)
        if end is None:
            continue
        spans.append(FunctionSpan(name=match.group(1), start=match.start(), end=end))
    return spans


def _enclosing_function(spans: List[FunctionSpan], offset: int) -> Optional[FunctionSpan]:
    """The innermost function whose body contains `offset`."""
    best: Optional[FunctionSpan] = None
    for span in spans:
        if span.start <= offset <= span.end:
            if best is None or span.start > best.start:
                best = span
    return best


def _line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def _call_closing_paren(masked: str, open_at: int) -> Optional[int]:
    """The offset of the `)` that closes the call whose `(` is at `open_at`."""
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


def _chain_after(masked: str, call_end: int) -> str:
    """The method-call chain that continues the expression at `call_end`.

    An empty string means the call is not chained — the result is bound,
    passed, or matched instead. Only a chain can carry `.into_value()`
    straight off the read.
    """
    index = call_end
    while index < len(masked) and masked[index].isspace():
        index += 1
    if index >= len(masked) or masked[index] != ".":
        return ""
    start = index
    depth = 0
    while index < len(masked):
        char = masked[index]
        if char in "([{":
            depth += 1
        elif char in ")]}":
            if depth == 0:
                break
            depth -= 1
        elif char == ";" and depth == 0:
            break
        index += 1
    return masked[start:index]


def _bound_name(masked: str, call_start: int) -> Optional[str]:
    """The name a `let` binds the diff read to, when the read is its RHS."""
    line_start = masked.rfind("\n", 0, call_start) + 1
    prefix = masked[line_start:call_start]
    match = _LET_BINDING_RE.search(prefix)
    if match is None:
        return None
    return match.group(1)


def _imports_diff_read(masked: str, name: str) -> bool:
    """True when the file imports the diff read by its bare name."""
    pattern = r"(?m)^[ \t]*use[ \t]+[^;]*\b" + re.escape(name) + r"\b[^;]*;"
    for match in re.finditer(pattern, masked):
        if "egggit" in match.group(0):
            return True
    return False


def _is_diff_call(masked: str, match: re.Match, name: str) -> bool:
    """True when the match is a call of the diff read, not a definition."""
    prefix = masked[max(0, match.start() - 64) : match.start()]
    if re.search(r"\bfn[ \t]+$", prefix):
        return False
    if "egggit::" in prefix:
        return True
    return _imports_diff_read(masked, name)


def _scope_reaches_fence(scope: str, local_bodies: Dict[str, str]) -> bool:
    """True when the scope reaches the fence, directly or through a helper."""
    if FENCE_FUNCTION + "(" in scope:
        return True
    for name in set(_CALL_RE.findall(scope)):
        body = local_bodies.get(name)
        if body is not None and FENCE_FUNCTION + "(" in body:
            return True
    return False


def check_file(path: Path, root: Path) -> List[Finding]:
    """Check one Rust file for unfenced diff consumers."""
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as error:
        return [
            Finding(
                rule="git-consistency-consumer-unreadable",
                file=str(path.relative_to(root)),
                line=1,
                message=f"could not read the file: {error}",
            )
        ]

    # A file that never names a diff read cannot consume one. The check is
    # a plain substring test, so it costs nothing on the files that are not
    # candidates — and the files that are candidates are scanned in full.
    if not any(name in text for name in DIFF_READ_FUNCTIONS):
        return []

    masked = _mask_non_code(text)
    spans = _function_spans(masked, _brace_matches(masked))
    local_bodies: Dict[str, str] = {
        span.name: masked[span.start : span.end + 1] for span in spans
    }
    relative = str(path.relative_to(root))
    findings: List[Finding] = []

    for match in _DIFF_CALL_RE.finditer(masked):
        name = match.group(1)
        if not _is_diff_call(masked, match, name):
            continue
        line = _line_of(text, match.start())
        closing = _call_closing_paren(masked, match.end() - 1)
        call_end = closing + 1 if closing is not None else match.end()
        scope = _enclosing_function(spans, match.start())
        scope_text = masked[scope.start : scope.end + 1] if scope is not None else masked
        scope_name = scope.name if scope is not None else "<module>"

        # 1. No direct consumption: the read must not be chained straight
        #    into `.into_value()`, which is what discards the class.
        chain = _chain_after(masked, call_end)
        if VALUE_CONSUMPTION in chain:
            findings.append(
                Finding(
                    rule="git-consistency-consumer-unfenced",
                    file=relative,
                    line=line,
                    message=(
                        f"{name}() is consumed by .into_value() without passing through "
                        f"{FENCE_FUNCTION}(); the read's consistency class is discarded"
                    ),
                )
            )

        # 2. No consumption through a binding either: a value bound from
        #    the read must not be consumed by `.into_value()` in the same
        #    scope.
        bound = _bound_name(masked, match.start())
        if bound is not None and (bound + VALUE_CONSUMPTION) in scope_text:
            findings.append(
                Finding(
                    rule="git-consistency-consumer-unfenced",
                    file=relative,
                    line=line,
                    message=(
                        f"the diff fact bound as `{bound}` is consumed by .into_value() in "
                        f"{scope_name}() without passing through {FENCE_FUNCTION}(); the read's "
                        "consistency class is discarded"
                    ),
                )
            )

        # 3. The scope must reach the fence, or propagate the class.
        if not _scope_reaches_fence(scope_text, local_bodies) and (
            CLASS_PROPAGATION not in scope_text
        ):
            findings.append(
                Finding(
                    rule="git-consistency-consumer-unfenced",
                    file=relative,
                    line=line,
                    message=(
                        f"{scope_name}() consumes {name}() without reaching "
                        f"{FENCE_FUNCTION}() and without propagating the read's class; the "
                        "consistency class is dropped"
                    ),
                )
            )

    return findings


def check_tree(root: Path) -> List[Finding]:
    """Check every Rust file under `src/`."""
    findings: List[Finding] = []
    source_root = root / "src"
    if not source_root.is_dir():
        return [
            Finding(
                rule="git-consistency-consumer-source-missing",
                file="src",
                line=1,
                message="the source tree is missing; diff consumers cannot be verified",
            )
        ]
    for path in sorted(source_root.rglob("*.rs")):
        findings.extend(check_file(path, root))
    return findings


def _copy_consumers(root: Path, target: Path) -> None:
    """Copy the diff consumers into a scratch tree."""
    for relative in (
        "src/security/workflow/report.rs",
        "src/tool/commit.rs",
        "src/git_service.rs",
    ):
        source = root / relative
        if not source.is_file():
            continue
        destination = target / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(source.read_text(encoding="utf-8"), encoding="utf-8")


def _self_test_cases() -> List[Tuple[str, str, str, str]]:
    """The injected defects, each with the rule that must reject it."""
    return [
        (
            "the consumer chains the diff read straight into .into_value()",
            "    let summary_fact = egggit::diff_summary(root, base)\n"
            "        .await\n"
            "        .map_err(|e| SecurityReviewReadError::Read(e.to_string()))?;\n"
            "    let summary = admit_diff_read(summary_fact)?;",
            "    let summary = egggit::diff_summary(root, base)\n"
            "        .await\n"
            "        .map_err(|e| SecurityReviewReadError::Read(e.to_string()))?\n"
            "        .into_value();",
            "git-consistency-consumer-unfenced",
        ),
        (
            "the consumer takes the diff fact's value directly",
            "let summary = admit_diff_read(summary_fact)?;",
            "let summary = summary_fact.into_value();",
            "git-consistency-consumer-unfenced",
        ),
        (
            "the admission door stops admitting",
            "let admitted = crate::git_consistency::admit_generation_bound(\n"
            "        fact,\n"
            "        crate::git_consistency::live_read_capability(),\n"
            "    )?;",
            "let admitted = fact;",
            "git-consistency-consumer-unfenced",
        ),
    ]


def self_test() -> int:
    """Prove the guard is not vacuous."""
    failures: List[str] = []
    cases = 0
    consumer = ROOT / "src" / "security" / "workflow" / "report.rs"
    if not consumer.is_file():
        print(
            f"self-test failed: {consumer} is missing, so there is nothing to self-test against",
            file=sys.stderr,
        )
        return 1

    with tempfile.TemporaryDirectory() as scratch:
        scratch_root = Path(scratch)

        baseline_root = scratch_root / "baseline"
        _copy_consumers(ROOT, baseline_root)
        baseline = check_tree(baseline_root)
        if baseline:
            failures.append(
                "the guard reports findings on unmodified diff consumers, so its checks are not "
                f"measuring what they claim: {baseline}"
            )

        for description, old, new, expected_rule in _self_test_cases():
            cases += 1
            work = scratch_root / re.sub(r"[^a-z0-9]+", "_", description.lower()).strip("_")
            _copy_consumers(ROOT, work)
            target = work / "src" / "security" / "workflow" / "report.rs"
            text = target.read_text(encoding="utf-8")
            if old not in text:
                failures.append(f"{description}: fixture could not be injected: target not found")
                continue
            target.write_text(text.replace(old, new, 1), encoding="utf-8")

            reported = check_tree(work)
            if not reported:
                failures.append(f"{description}: the guard did not reject the injected defect")
                continue
            if not any(finding.rule == expected_rule for finding in reported):
                failures.append(
                    f"{description}: the guard rejected the injected defect, but not with the "
                    f"check that owns it (no finding had rule {expected_rule!r}): {reported}"
                )
                continue
            print(f"  ok   {description} (rejected by {expected_rule})")

    if failures:
        for failure in failures:
            print(f"self-test failed: {failure}", file=sys.stderr)
        return 1
    print(
        f"git-consistency-consumers self-test passed: {cases} injected defects were each "
        "rejected by the guard, and the unmodified diff consumers produced no finding"
    )
    return 0


def main(argv: List[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects the drift it claims to catch",
    )
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()

    findings = check_tree(ROOT)
    if findings:
        print("git-consistency-consumers: FAILED", file=sys.stderr)
        for finding in findings:
            print(f"  - {finding.file}:{finding.line}: {finding.message}", file=sys.stderr)
        return 1

    print("git-consistency-consumers check ok: every diff consumer fences its read")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
