#!/usr/bin/env python3
"""Guard: a command handler may not block on a runtime handle from inside the runtime.

`tokio::runtime::Handle::block_on` calls `context::enter_runtime`, and
`enter_runtime` panics with *"Cannot start a runtime from within a runtime"*
when the calling thread's runtime context is already **entered**. The entered
flag is set only by `context::enter_runtime` — that is, by a real
`Runtime::block_on` — and never by `Runtime::enter()`, which only sets the
current *handle*.

CodeGG's TUI event loop is awaited inside `#[tokio::main(flavor =
"multi_thread")]` (`src/main.rs`), so the whole event loop runs with the
runtime context entered. A synchronous command handler that then calls
`Handle::block_on` therefore terminates the process. Eighteen such call sites
existed in `src/tui/app/mod.rs` (the `/lsp-*` arms and the `/worktree` arm)
and three in `src/mcp/cli.rs` (the `McpCommand::Debug` arm).

The fix is `tokio::task::block_in_place`, which leaves the runtime context
before the inner `block_on` re-enters it. This guard makes the shape
unrepeatable.

The rule
--------

In the repository's production Rust trees, a call to a Tokio **`Handle`**'s
`block_on` must be lexically inside a `block_in_place(...)` argument. A
receiver is recognised as a `Handle` when it is:

  1. `Handle::current()` (with any path prefix), or
  2. a `Handle`-qualified path used as the receiver (`Handle::block_on(`), or
  3. a bare identifier whose name ends in `handle`, case-insensitively — the
     naming convention this repository uses for a `Handle` binding.

`Runtime::block_on` is **not** this rule's subject: blocking on a `Runtime`
value is a different operation, and the repository's legitimate uses of it
(`src/upgrade/managed.rs`, `src/managed_process.rs`) run on a fresh thread
with no runtime context entered. The guard therefore does not flag them.

Scope, stated explicitly
------------------------

  * **Scanned:** every `*.rs` file under `src/` and `crates/`, *including*
    the `#[cfg(test)]` modules inside them. There is no file exemption and
    no line-pattern allowlist: a `Handle::block_on` in a test module is the
    same defect, because a `#[tokio::test]` body runs with the runtime
    context entered. The `lsp_doctor` tests in `src/tui/app/mod.rs` reach the
    production handler and therefore travel this path; after the fix they
    contain no `Handle::block_on` of their own.
  * **Not scanned:** the root `tests/` tree. Those are the integration-test
    families, not shipped code, and the regression test for this milestone
    must be able to drive the production handler from inside a real
    `Runtime::block_on` to prove the defect. It does that by calling the
    production handler, so it needs no exemption of its own.

Parsing
--------

Source is masked with `scripts/rust_test_scan.py` — the repository's
Rust-aware scanner — before it is searched. That is not a nicety: a naive
character-literal pattern reads `&'a T` as a literal and swallows the rest
of the line, and a comment that merely *names* `Handle::block_on` would be
reported as a violation. The mask blanks literals and comments while
preserving length and newline count, so a reported line number is the
violation's own line.

Usage:
    python3 scripts/check_no_nested_runtime_block_on.py            # scan the tree
    python3 scripts/check_no_nested_runtime_block_on.py --self-test
Exit codes: 0 clean, 1 violations found, 2 self-test failure.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import rust_test_scan  # noqa: E402  (path is set above)

REPO_ROOT = Path(__file__).resolve().parent.parent

# ── Scanned surfaces ────────────────────────────────────────────────────────
#
# The code that ships. `tests/` is deliberately excluded — see the module
# docstring for the reasoning.

SCANNED_TREES: list[str] = [
    "src",
    "crates",
]

# ── Detection ───────────────────────────────────────────────────────────────

# Every `block_on(` call site. The receiver is classified separately, because
# `Runtime::block_on` and `futures_executor::block_on` are a different
# operation from a `Handle`'s.
_BLOCK_ON_CALL = re.compile(r"block_on\s*\(")

# `block_in_place(` — the one construct that makes a nested entry legal.
_BLOCK_IN_PLACE = re.compile(r"(?<![A-Za-z0-9_])block_in_place\s*\(")


@dataclass(frozen=True)
class Finding:
    file: str
    line_no: int
    line_text: str


def _line_of(mask: str, index: int) -> int:
    """The 1-based line number of `index` in `mask`."""
    return mask.count("\n", 0, index) + 1


def _line_text(source: str, line_no: int) -> str:
    lines = source.splitlines()
    if 1 <= line_no <= len(lines):
        return lines[line_no - 1].strip()
    return ""


def _matching_paren(mask: str, open_index: int) -> int:
    """The index of the `)` that closes the `(` at `open_index`, or -1."""
    depth = 0
    for index in range(open_index, len(mask)):
        char = mask[index]
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            if depth == 0:
                return index
    return -1


def _matching_paren_backwards(mask: str, close_index: int) -> int:
    """The index of the `(` that opens the `)` at `close_index`, or -1."""
    depth = 0
    for index in range(close_index, -1, -1):
        char = mask[index]
        if char == ")":
            depth += 1
        elif char == "(":
            depth -= 1
            if depth == 0:
                return index
    return -1


def _identifier_ending_at(mask: str, end_index: int) -> tuple[str, int]:
    """The identifier that ends at `end_index` (exclusive), and its start."""
    index = end_index - 1
    while index >= 0 and (mask[index].isalnum() or mask[index] == "_"):
        index -= 1
    return mask[index + 1 : end_index], index + 1


def _is_handle_binding(identifier: str) -> bool:
    """Is `identifier` this repository's name for a `Handle` binding?

    The convention is a name that *ends* in `handle`, so `handle`,
    `runtime_handle` and `rt_handle` are covered while `rt` — a `Runtime` —
    is not.
    """
    return len(identifier) >= len("handle") and identifier.lower().endswith("handle")


def _receiver_is_handle(mask: str, dot_index: int) -> bool:
    """Is the receiver of the method call at `dot_index` a Tokio `Handle`?

    Two shapes are recognised:

      * `Handle::current()` (with any path prefix) — the receiver is a call
        expression, so the matching `(` is walked back to and the qualified
        path in front of it is read; or
      * a bare identifier naming a `Handle` binding.
    """
    index = dot_index - 1
    while index >= 0 and mask[index] in " \t\n":
        index -= 1
    if index < 0:
        return False

    if mask[index] == ")":
        open_index = _matching_paren_backwards(mask, index)
        if open_index == -1:
            return False
        name, name_start = _identifier_ending_at(mask, open_index)
        if name != "current":
            return False
        separator = name_start - 1
        while separator >= 0 and mask[separator] in " \t\n":
            separator -= 1
        if separator < 1 or mask[separator] != ":" or mask[separator - 1] != ":":
            return False
        path_name, _ = _identifier_ending_at(mask, separator - 1)
        return path_name == "Handle"

    name, _ = _identifier_ending_at(mask, index + 1)
    return _is_handle_binding(name)


def _handle_block_on_sites(mask: str) -> list[int]:
    """Every index at which a `Handle`'s `block_on` is called."""
    sites: list[int] = []
    for match in _BLOCK_ON_CALL.finditer(mask):
        name_start = match.start()
        index = name_start - 1
        while index >= 0 and mask[index] in " \t\n":
            index -= 1
        if index < 0:
            continue
        if mask[index] == ".":
            if _receiver_is_handle(mask, index):
                sites.append(name_start)
            continue
        if index >= 1 and mask[index] == ":" and mask[index - 1] == ":":
            path_name, _ = _identifier_ending_at(mask, index - 1)
            if path_name == "Handle":
                sites.append(name_start)
    return sorted(set(sites))


def _protected_spans(mask: str) -> list[tuple[int, int]]:
    """The interior spans of every `block_in_place(...)` call in `mask`.

    A `Handle::block_on` inside one of these spans is legal: `block_in_place`
    leaves the runtime context before the closure runs, so the inner
    `block_on` re-enters it rather than nesting into it.
    """
    spans: list[tuple[int, int]] = []
    for match in _BLOCK_IN_PLACE.finditer(mask):
        open_index = match.end() - 1  # the '(' the pattern ends on
        close_index = _matching_paren(mask, open_index)
        if close_index == -1:
            # Unbalanced parentheses cannot happen in valid Rust; treat the
            # rest of the file as protected rather than reporting noise.
            spans.append((open_index, len(mask)))
            continue
        spans.append((open_index, close_index))
    return spans


def scan_source(rel_path: str, source: str) -> list[Finding]:
    """Return the nested-runtime violations in one source file."""
    mask = rust_test_scan.scan_source(source).mask()
    spans = _protected_spans(mask)

    findings: list[Finding] = []
    for index in _handle_block_on_sites(mask):
        if any(open_index < index < close_index for open_index, close_index in spans):
            continue
        line_no = _line_of(mask, index)
        findings.append(Finding(rel_path, line_no, _line_text(source, line_no)))
    return findings


def _iter_sources(root: Path = REPO_ROOT) -> list[tuple[str, str]]:
    sources: list[tuple[str, str]] = []
    for tree in SCANNED_TREES:
        base = root / tree
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*.rs")):
            sources.append(
                (str(path.relative_to(root)), path.read_text(encoding="utf-8"))
            )
    return sources


def collect_findings(root: Path = REPO_ROOT) -> list[Finding]:
    findings: list[Finding] = []
    for rel_path, source in _iter_sources(root):
        findings.extend(scan_source(rel_path, source))
    return findings


# ── Self-test ───────────────────────────────────────────────────────────────
#
# Exercises the exact detector used against the real tree. Each case states
# what it proves.

SELF_TEST_CASES: list[tuple[str, str, int]] = [
    (
        "a bare Handle::current().block_on is caught",
        "src/tui/hypothetical.rs",
        "fn go() {\n"
        "    let v = tokio::runtime::Handle::current().block_on(work());\n"
        "    let _ = v;\n"
        "}\n",
        1,
    ),
    (
        "a Handle binding's .block_on is caught",
        "src/tui/hypothetical.rs",
        "fn go() {\n"
        "    let handle = tokio::runtime::Handle::current();\n"
        "    let v = handle.block_on(work());\n"
        "    let _ = v;\n"
        "}\n",
        1,
    ),
    (
        "the same call inside block_in_place is accepted",
        "src/tui/hypothetical.rs",
        "fn go() {\n"
        "    let v = tokio::task::block_in_place(|| {\n"
        "        tokio::runtime::Handle::current().block_on(work())\n"
        "    });\n"
        "    let _ = v;\n"
        "}\n",
        0,
    ),
    (
        "a Handle binding inside block_in_place is accepted",
        "src/tui/hypothetical.rs",
        "fn go() {\n"
        "    let handle = tokio::runtime::Handle::current();\n"
        "    let v = tokio::task::block_in_place(|| handle.block_on(work()));\n"
        "    let _ = v;\n"
        "}\n",
        0,
    ),
    (
        "one legal and one illegal call in the same file: only the illegal one is reported",
        "src/tui/hypothetical.rs",
        "fn go() {\n"
        "    let a = tokio::task::block_in_place(|| {\n"
        "        tokio::runtime::Handle::current().block_on(work())\n"
        "    });\n"
        "    let b = tokio::runtime::Handle::current().block_on(work());\n"
        "    let _ = (a, b);\n"
        "}\n",
        1,
    ),
    (
        "a Runtime's block_on is not this rule's subject",
        "src/tui/hypothetical.rs",
        "fn go() {\n"
        "    let rt = tokio::runtime::Runtime::new().unwrap();\n"
        "    let v = rt.block_on(work());\n"
        "    let _ = v;\n"
        "}\n",
        0,
    ),
    (
        "a comment that names Handle::block_on is not a violation",
        "src/tui/hypothetical.rs",
        "/// The render pass runs where `Handle::block_on` panics.\n"
        "pub fn draw() {}\n",
        0,
    ),
    (
        "a string literal that names block_in_place does not protect a call",
        "src/tui/hypothetical.rs",
        "fn go() {\n"
        "    let label = \"block_in_place\";\n"
        "    let v = tokio::runtime::Handle::current().block_on(work());\n"
        "    let _ = (label, v);\n"
        "}\n",
        1,
    ),
    (
        "a lifetime is not read as a character literal",
        "src/tui/hypothetical.rs",
        "fn go<'a>(name: &'a str) {\n"
        "    let v = tokio::runtime::Handle::current().block_on(work());\n"
        "    let _ = (name, v);\n"
        "}\n",
        1,
    ),
    (
        "a nested block_in_place around a nested block_in_place is accepted",
        "src/tui/hypothetical.rs",
        "fn go() {\n"
        "    let v = tokio::task::block_in_place(|| {\n"
        "        tokio::task::block_in_place(|| {\n"
        "            tokio::runtime::Handle::current().block_on(work())\n"
        "        })\n"
        "    });\n"
        "    let _ = v;\n"
        "}\n",
        0,
    ),
    (
        "a call after a completed block_in_place is still reported",
        "src/tui/hypothetical.rs",
        "fn go() {\n"
        "    let a = tokio::task::block_in_place(|| {\n"
        "        tokio::runtime::Handle::current().block_on(work())\n"
        "    });\n"
        "    let b = tokio::runtime::Handle::current().block_on(work());\n"
        "    let _ = (a, b);\n"
        "}\n",
        1,
    ),
    (
        "a test module's Handle::block_on is caught too",
        "src/tui/hypothetical.rs",
        "#[cfg(test)]\n"
        "mod tests {\n"
        "    #[tokio::test]\n"
        "    async fn t() {\n"
        "        let handle = tokio::runtime::Handle::current();\n"
        "        let v = handle.block_on(work());\n"
        "        let _ = v;\n"
        "    }\n"
        "}\n",
        1,
    ),
]


def run_self_test() -> int:
    failures = 0
    for description, rel_path, source, expected in SELF_TEST_CASES:
        found = scan_source(rel_path, source)
        actual = len(found)
        if actual == expected:
            print(f"  ok   {description}")
        else:
            failures += 1
            print(f"  FAIL {description}: expected {expected}, found {actual}")
            for finding in found:
                print(f"         {finding.file}:{finding.line_no}: {finding.line_text}")

    if failures:
        print(f"\nself-test FAILED: {failures} case(s) — the detector does not discriminate.")
        return 2

    print(f"\nself-test passed: {len(SELF_TEST_CASES)} case(s).")
    return 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return run_self_test()

    findings = collect_findings()

    if findings:
        print(
            "A Tokio Handle's block_on outside a block_in_place closure.\n"
            "The TUI event loop and the MCP CLI run inside a multi-threaded\n"
            "runtime whose context is ENTERED, so a nested block_on panics\n"
            "with \"Cannot start a runtime from within a runtime\" and takes\n"
            "the process down.\n"
            "\nWrap the wait in tokio::task::block_in_place(|| { .. }), which\n"
            "leaves the runtime context before the inner block_on re-enters it.\n"
            "See architecture/tui.md, \"Blocking on a runtime handle\".\n"
        )
        for finding in findings:
            print(f"  {finding.file}:{finding.line_no}: {finding.line_text}")
        print(f"\n{len(findings)} violation(s) found.")
        return 1

    print(
        "nested runtime check passed — no Handle::block_on outside a block_in_place"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
