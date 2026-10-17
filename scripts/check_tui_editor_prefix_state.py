#!/usr/bin/env python3
"""Static guard: the editor has exactly one pending-prefix mechanism.

Milestone M002 of `plans/implementation/tui-editor-helix-parity/` replaces two
ad-hoc prefix mechanisms on `TuiDocumentPresentation` with one typed state
machine, `EditorPrefix`:

* `pending_command: String`, bounded by `MAX_EDITOR_PENDING_COMMAND`, holding
  the `g` of `gg`, and
* `yank_prefix_pending: bool`, holding the `space` of `space y`.

The two were not merely redundant. They disagreed: `gg` **consumed** an
unmatched second key, while `space y` **disarmed and retried** it. Merging them
therefore had to preserve a per-prefix mismatch policy
(`EditorPrefix::swallows_unmatched`), and the milestone's acceptance criteria
require `gg` and `space y` to behave identically to the baseline.

## What counts as a violation

**Violations** (exit code 1):

- Any occurrence of `pending_command`, `yank_prefix_pending`,
  `MAX_EDITOR_PENDING_COMMAND`, `bounded_pending`, or `push_pending` anywhere
  under `src/`. These are the deleted mechanisms, their bound, and the two
  helpers that existed only to service them. A field left behind rather than
  deleted satisfies nothing and is read as complete.
- A `TuiDocumentPresentation` that declares more than one prefix field. The
  point of the type is that a new grammar arrives as a leaf, not a flag, so a
  second `*prefix*` field is the regression this milestone exists to prevent —
  and it would be a *new* name, which a fixed forbidden-identifier list cannot
  catch.
- A `TuiDocumentPresentation` with no prefix field at all: prefix state has to
  live somewhere, and "nowhere" means the gestures silently stopped arming.

## What the guard requires to still be true

Checked by name so it cannot pass against an emptied tree:

- `pub enum EditorPrefix` with an `Idle` variant (the machine needs a resting
  state to disarm into).
- `pub pending_prefix` on the presentation.
- `arm_prefix`, `disarm_prefix`, and `resolve_pending_prefix` all present.
- `MAX_EDITOR_PENDING_PREFIX` present: the bound the indicator is held to.

## Non-vacuity

Every requirement above is an assertion about the real tree, so removing the
type, the field, an operation, or the bound fails the guard. The
forbidden-identifier scan covers `src/` wholesale, so a *test* that resurrects
a deleted name fails the guard too.

Exit code 1 on violations, 2 on a shape the guard cannot resolve.

Usage::

    python3 scripts/check_tui_editor_prefix_state.py

This is invoked by `scripts/verify.sh quick` and the CI `verify` job.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SRC = REPO_ROOT / "src"
PRESENTATION = SRC / "tui" / "document_session.rs"

# Deleted mechanisms and the helpers that only existed to service them.
FORBIDDEN = [
    "pending_command",
    "yank_prefix_pending",
    "MAX_EDITOR_PENDING_COMMAND",
    "bounded_pending",
    "push_pending",
]

REQUIRED_MODEL_DECLARATIONS = [
    "pub enum EditorPrefix",
    "Idle",
    "MAX_EDITOR_PENDING_PREFIX",
]

REQUIRED_STATE_DECLARATIONS = [
    "pub pending_prefix",
    "fn arm_prefix",
    "fn disarm_prefix",
    "fn resolve_pending_prefix",
]

# A struct field: `pub name: Type,` at any indentation, ignoring doc comments.
FIELD_RE = re.compile(r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?([a-z_]\w*)\s*:", re.MULTILINE)
STRUCT_RE = re.compile(r"struct\s+TuiDocumentPresentation\b[^{]*\{")


class ParseError(Exception):
    """A shape the guard cannot resolve."""


def struct_body(text: str, name: str) -> str:
    """The brace-matched body of `struct name { ... }`."""
    pattern = re.compile(r"struct\s+" + re.escape(name) + r"\b[^{]*\{")
    match = pattern.search(text)
    if match is None:
        raise ParseError(f"struct {name} not found")
    depth = 0
    for index in range(match.end() - 1, len(text)):
        char = text[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return text[match.end() : index]
    raise ParseError(f"struct {name} has unbalanced braces")


def rust_sources() -> list[Path]:
    """Every `.rs` file under `src/`, sorted so output is deterministic."""
    if not SRC.is_dir():
        raise ParseError(f"{SRC} is missing")
    return sorted(SRC.rglob("*.rs"))


def strip_comment(line: str) -> str:
    """Drop a trailing `//` comment, respecting string and char literals.

    Comments are where the *history* lives — `EditorPrefix`'s doc comment
    explains that it replaced `pending_command` and `yank_prefix_pending`, and
    that prose is worth keeping. Only code is forbidden, so the scan runs over
    comment-free text with the line count preserved.
    """
    in_string = False
    in_char = False
    escaped = False
    index = 0
    while index < len(line):
        char = line[index]
        if escaped:
            escaped = False
        elif char == "\\":
            escaped = True
        elif in_string:
            if char == '"':
                in_string = False
        elif in_char:
            if char == "'":
                in_char = False
        elif char == '"':
            in_string = True
        elif char == "'":
            in_char = True
        elif char == "/" and line[index : index + 2] == "//":
            return line[:index]
        index += 1
    return line


def strip_comments(text: str) -> str:
    """Comment-free text with the line count preserved, so lines stay true."""
    return "\n".join(strip_comment(line) for line in text.splitlines())


def main() -> int:
    violations: list[str] = []

    try:
        sources = rust_sources()
    except ParseError as error:
        print(f"TUI editor prefix-state guard could not resolve a shape: {error}\n", file=sys.stderr)
        return 2

    raw_text = "\n".join(path.read_text() for path in sources)

    # 1. The deleted names anywhere under src/ — including tests, so a helper
    #    that resurrects one fails the build rather than merely compiling.
    for path in sources:
        for number, line in enumerate(path.read_text().splitlines(), start=1):
            code = strip_comment(line)
            for name in FORBIDDEN:
                if re.search(rf"\b{re.escape(name)}\b", code):
                    violations.append(
                        f"{path.relative_to(REPO_ROOT)}:{number} references deleted "
                        f"`{name}`: {code.strip()}"
                    )

    # 2. Exactly one prefix field on the presentation. This is the invariant
    #    a fixed name list cannot express: the next milestone's flags would be
    #    new names.
    try:
        presentation_text = strip_comments(PRESENTATION.read_text())
        body = struct_body(presentation_text, "TuiDocumentPresentation")
        prefix_fields = [name for name in FIELD_RE.findall(body) if "prefix" in name]
    except (ParseError, OSError) as error:
        print(
            f"TUI editor prefix-state guard could not resolve a shape: {error}\n"
            "Teach the guard the new shape rather than skipping it.\n",
            file=sys.stderr,
        )
        return 2

    if len(prefix_fields) != 1:
        violations.append(
            "TuiDocumentPresentation declares "
            f"{len(prefix_fields)} prefix field(s) {prefix_fields}; exactly one "
            "(`pending_prefix`) is the whole point of M002 — a new grammar adds a "
            "leaf, not a flag"
        )

    # 3. The replacements are still present, anywhere in the tree: the field
    #    and the two operations live on the presentation in `document_session`,
    #    while the resolver lives with the dispatch tables in `commands`.
    for needle in REQUIRED_MODEL_DECLARATIONS + REQUIRED_STATE_DECLARATIONS:
        if needle not in raw_text:
            violations.append(f"expected declaration `{needle}` is absent from src/")

    if violations:
        print(
            "TUI editor prefix-state violations found.\n"
            "The editor must have exactly one pending-prefix mechanism. It had two,\n"
            "and each new Helix grammar wanted a third; M002 replaces both with the\n"
            "`EditorPrefix` state machine. A legacy field left behind rather than\n"
            "deleted satisfies nothing.\n"
        )
        for violation in violations:
            print(f"  {violation}")
        print(f"\n{len(violations)} violation(s) found.")
        return 1

    print(
        "TUI editor prefix-state guard passed — one pending-prefix field "
        f"({prefix_fields[0]}), typed as EditorPrefix, no legacy mechanism left"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())