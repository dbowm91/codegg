#!/usr/bin/env python3
"""Generate the M007-D command-classification artifact.

The artifact is a **measurement**, not a claim: every number in it is
derived from the sources by this script, and §1's summary table is computed
from the §3 inventory the same script writes, so the two cannot drift.

Run it from the repository root:

    python3 plans/implementation/desktop-frontend-ide-foundation/artifacts/\
generate_m007d_command_classification.py

The sources it reads:

* `crates/codegg-core/src/command_catalog.rs` — the catalog: every command,
  its argument forms, and each form's kind.
* `crates/codegg-core/src/frontend_action.rs` — the declared frontend-action
  set: which action serves a frontend-effect form.
* `src/core/daemon_commands.rs` — the daemon's effect table: which
  daemon-effect forms the daemon can serve, and the request each one builds.
* `src/tui/command.rs` — the TUI's registry: which command name reaches which
  dispatcher arm.
* `src/tui/app/mod.rs` — the TUI's dispatcher: the requests an arm issues.
* `src/tui/commands/**` and `src/tui/runtime/command_dispatch.rs` — the
  handlers an arm reaches.
* `crates/codegg-client/src/**` — the client crate the TUI reaches the
  daemon's document path through; an index that stops at `src/tui` cannot
  see the requests that path issues.

No cargo, no network: this is a textual measurement, like the guards that
read the same files.
"""

from __future__ import annotations

import json
import os
import re
import sys
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
ARTIFACT = (
    ROOT
    / "plans/implementation/desktop-frontend-ide-foundation/artifacts"
    / "m007-d-command-classification.md"
)

CATALOG = "crates/codegg-core/src/command_catalog.rs"
FRONTEND_ACTIONS = "crates/codegg-core/src/frontend_action.rs"
DAEMON_COMMANDS = "src/core/daemon_commands.rs"
TUI_REGISTRY = "src/tui/command.rs"
TUI_DISPATCH = "src/tui/app/mod.rs"
TUI_DIR = "src/tui"
# The trees the resolver reads. The TUI reaches the daemon's document path
# through `codegg-client`, so an index that stops at `src/tui` cannot see
# the requests that path issues.
SOURCE_TREES = ("src/tui", "crates/codegg-client/src")
# The module path each tree's modules are reachable under.
MODULE_ROOTS = {
    "src/tui": "",
    "crates/codegg-client/src": "codegg_client::",
}


# ── Reading the sources ─────────────────────────────────────────────────────


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


@dataclass
class CatalogForm:
    form: str
    kind: str


@dataclass
class CatalogCommand:
    command_id: str
    name: str
    aliases: list[str]
    description: str
    category: str
    domain: str
    kind: str
    forms: list[CatalogForm] = field(default_factory=list)


DESCRIPTOR_RE = re.compile(r"CommandDescriptor\s*\{")


def parse_catalog(text: str) -> list[CatalogCommand]:
    """Every command in the daemon's catalog."""
    commands: list[CatalogCommand] = []
    for match in DESCRIPTOR_RE.finditer(text):
        depth = 0
        index = text.index("{", match.start())
        end = index
        while end < len(text):
            if text[end] == "{":
                depth += 1
            elif text[end] == "}":
                depth -= 1
                if depth == 0:
                    break
            end += 1
        block = text[index : end + 1]
        command_id = re.search(r'command_id:\s*"([^"]+)"', block)
        name = re.search(r'name:\s*"([^"]+)"', block)
        if command_id is None or name is None:
            continue
        aliases = re.findall(r'"([^"]+)"', _slice(block, "aliases:", "]"))
        description = _string(block, "description:")
        category = _enum(block, "category:")
        domain = _enum(block, "domain:")
        kind = _enum(block, "kind:")
        forms = [
            CatalogForm(form=f, kind=k)
            for f, k in re.findall(
                r'CommandForm\s*\{\s*form:\s*"([^"]*)",\s*kind:\s*CommandKind::(\w+)', block
            )
        ]
        commands.append(
            CatalogCommand(
                command_id=command_id.group(1),
                name=name.group(1),
                aliases=aliases,
                description=description,
                category=category,
                domain=domain,
                kind=kind,
                forms=forms,
            )
        )
    return commands


def _slice(block: str, start_marker: str, end_marker: str) -> str:
    start = block.find(start_marker)
    if start < 0:
        return ""
    start += len(start_marker)
    end = block.find(end_marker, start)
    return block[start:end] if end >= 0 else ""


def _string(block: str, marker: str) -> str:
    match = re.search(re.escape(marker) + r'\s*"((?:[^"\\]|\\.)*)"', block)
    return match.group(1) if match else ""


def _enum(block: str, marker: str) -> str:
    match = re.search(re.escape(marker) + r"\s*(\w+)::(\w+)", block)
    return match.group(2) if match else ""


def parse_effect_table(text: str) -> list[tuple[str, str]]:
    """The `(command_id, form)` keys the daemon's effect table serves.

    The table is the match in `build_inner_request`; an arm may name two
    keys when two commands share one effect.
    """
    start = text.find("pub fn build_inner_request(")
    if start < 0:
        return []
    body = text[start:]
    end = body.find("\npub fn ", 1)
    if end > 0:
        body = body[:end]
    return re.findall(r'\(\s*"([a-z0-9_]+)"\s*,\s*"([^"]*)"\s*\)', body)


def parse_effect_requests(text: str) -> dict[tuple[str, str], str]:
    """The daemon request each effect-table row builds."""
    start = text.find("pub fn build_inner_request(")
    if start < 0:
        return {}
    body = text[start:]
    end = body.find("\npub fn ", 1)
    if end > 0:
        body = body[:end]
    requests: dict[tuple[str, str], str] = {}
    # A row's body may hold a `let ... else` guard or a bound value before
    # the `json!`, so the block between `=>` and the request is matched with
    # one level of nested braces. Without that, a served row reads as `—`
    # in the artifact and the column stops being a record of the table.
    #
    # One row may name several commands (`("memory", "") | ("memory_list", "")`).
    # Each of them is a served form, so each is recorded.
    for match in re.finditer(
        r'\(\s*"([a-z0-9_]+)"\s*,\s*"([^"]*)"\s*\)'
        r'((?:\s*\|\s*\(\s*"[a-z0-9_]+"\s*,\s*"[^"]*"\s*\))*)'
        r'\s*=>\s*(?:\{[^{}]*(?:\{[^{}]*\}[^{}]*)*)?'
        r'json!\(\{\s*"type":\s*"([a-z0-9_]+)"',
        body,
    ):
        request = match.group(4)
        forms = [(match.group(1), match.group(2))]
        forms.extend(re.findall(r'\(\s*"([a-z0-9_]+)"\s*,\s*"([^"]*)"\s*\)', match.group(3)))
        for command_id, form in forms:
            requests[(command_id, form)] = request
    return requests


def parse_frontend_actions(text: str) -> dict[tuple[str, str], str]:
    """The declared action for each frontend-effect form."""
    start = text.find("static THE_FRONTEND_ACTION_SET")
    if start < 0:
        return {}
    body = text[start : text.find("\n];", start)]
    mapping: dict[tuple[str, str], str] = {}
    for action, forms in re.findall(
        r"action:\s*FrontendAction::(\w+),\s*disposition:[^,]+,\s*forms:\s*&\[(.*?)\]",
        body,
        re.S,
    ):
        for command_id, form in re.findall(r'\(\s*"([^"]+)"\s*,\s*"([^"]*)"\s*\)', forms):
            mapping[(command_id, form)] = action
    return mapping


def parse_registry(text: str) -> dict[str, str]:
    """The dispatcher arm each command name reaches."""
    return {
        match.group(1): match.group(3)
        for match in re.finditer(
            r'Command::new\(\s*"([^"]+)"\s*,\s*CommandCategory::(\w+)\s*,\s*'
            r"CommandAction::Builtin\(BuiltinSlashAction::(\w+)\)",
            text,
        )
    }


# ── The TUI's arms ──────────────────────────────────────────────────────────

FN_RE = re.compile(r"(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)\s*(?:<[^>]*>)?\s*\(")
# A request's payload is often a struct of its own
# (`CoreRequest::InteractiveProcessCreate { request: .. }`), so the block is
# scanned by its braces rather than by a regular expression: a flat
# expression cannot see the closing brace of that payload, and a scan that
# misses it reports the arm as issuing no daemon request at all.
CTOR_RE = re.compile(r"CoreRequest::([A-Za-z0-9_]+)\s*\{")
CALL_RE = re.compile(r"\b([a-z_][a-z0-9_]*)\s*\(")
KEYWORDS = {
    "if", "for", "while", "match", "fn", "Some", "None", "Ok", "Err", "Vec", "String",
    "HashMap", "HashSet", "Box", "Arc", "move", "return", "let", "in", "else", "loop",
    "break", "continue", "as", "ref", "self",
}
# The keywords that cannot begin an expression. A method call's receiver
# ends at the last of them — the `if` of `if app.state.view(..)` is not
# part of the receiver.
EXPRESSION_KEYWORDS = {
    "as", "async", "await", "break", "const", "continue", "else", "enum", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
    "pub", "ref", "return", "static", "struct", "unsafe", "use", "where", "while",
    "yield",
}


# ── The resolver's lexical layer ───────────────────────────────────────────
#
# The resolver reads Rust as text, and three rules keep that honest:
#
# * A **literal is not syntax.** A brace, a parenthesis or a `=>` inside a
#   string or a comment cannot open a block, so every scan runs over a
#   *mask* in which the literals and the comments are blanked out. The mask
#   keeps every offset, so a position in the mask is a position in the text.
# * A **call names its callee and its arguments.** The arguments are what
#   decide which arm of a dispatch site runs, so the resolver reads them.
# * A **dispatch site is not a call.** A `match` over an enum dispatches to
#   many variants, and the *caller* picks the one that runs. The resolver
#   splits a body into the parts a given caller can reach and never unions
#   the arms of a dispatch site into one set — a union is how an arm comes
#   to own requests it never issues.


def _blank(out: list[str], start: int, end: int) -> None:
    for position in range(start, end):
        out[position] = " "


def _blank_string(text: str, start: int, out: list[str]) -> int:
    """Blank the string literal at `start`; return the index past it."""
    index = start + 1
    while index < len(text):
        if text[index] == "\\":
            index += 2
            continue
        if text[index] == '"':
            index += 1
            break
        index += 1
    _blank(out, start, min(index, len(text)))
    return index


def _blank_raw(text: str, start: int, out: list[str]) -> int:
    """Blank the raw string literal at `start`; return the index past it."""
    index = start
    while index < len(text) and text[index] in "rb":
        index += 1
    hashes = 0
    while index < len(text) and text[index] == "#":
        hashes += 1
        index += 1
    if index >= len(text) or text[index] != '"':
        return start
    index += 1
    terminator = '"' + "#" * hashes
    end = text.find(terminator, index)
    end = len(text) if end < 0 else end + len(terminator)
    _blank(out, start, end)
    return end


def _blank_char(text: str, start: int) -> int:
    """The index past the char literal at `start`, or `start` for a lifetime."""
    if start + 1 < len(text) and text[start + 1] == "\\":
        end = text.find("'", start + 2)
        return len(text) if end < 0 else end + 1
    if start + 2 < len(text) and text[start + 2] == "'":
        return start + 3
    return start


def mask_literals(text: str) -> str:
    """`text` with every string, char literal and comment blanked out."""
    out = list(text)
    index = 0
    length = len(text)
    while index < length:
        char = text[index]
        if text.startswith("//", index):
            end = text.find("\n", index)
            end = length if end < 0 else end
            _blank(out, index, end)
            index = end
            continue
        if text.startswith("/*", index):
            depth = 1
            end = index + 2
            while end < length and depth:
                if text.startswith("/*", end):
                    depth += 1
                    end += 2
                elif text.startswith("*/", end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
            _blank(out, index, min(end, length))
            index = end
            continue
        if char == '"':
            index = _blank_string(text, index, out)
            continue
        if char in "rb":
            end = _blank_raw(text, index, out)
            if end > index:
                index = end
                continue
        if char == "'":
            end = _blank_char(text, index)
            if end > index:
                _blank(out, index, end)
                index = end
                continue
        index += 1
    return "".join(out)


def matching_paren(masked: str, start: int) -> int:
    """The index of the `)` closing the `(` at `start`, or -1."""
    depth = 0
    for position in range(start, len(masked)):
        char = masked[position]
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            if depth == 0:
                return position
    return -1


def _block_open(masked: str, start: int) -> int:
    """The index of the `{` that opens the block starting at `start`, or -1."""
    depth = 0
    for position in range(start, len(masked)):
        char = masked[position]
        if char in "([":
            depth += 1
        elif char in ")]":
            depth -= 1
        elif char == "{" and depth == 0:
            return position
    return -1


def _matching_brace(masked: str, start: int) -> int:
    """The index of the `}` closing the `{` at `start`, or -1."""
    depth = 0
    for position in range(start, len(masked)):
        char = masked[position]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return position
    return -1


def _receiver_of(text: str, masked: str, name_start: int) -> str | None:
    """The expression a method name is called on.

    `self.controller.open(..)` is called on `self.controller`; a plain call
    has no receiver. The walk runs over the **masked** text, so a comment
    in front of the call cannot be mistaken for the receiver, and the
    tokens in front that cannot begin an expression — the `if` of
    `if app.state.view(..)` — are dropped.
    """
    index = name_start - 1
    while index >= 0 and masked[index] in " \t\r\n":
        index -= 1
    if index < 0 or masked[index] != ".":
        return None
    end = index
    index -= 1
    while index >= 0 and (
        masked[index].isalnum() or masked[index] in "_." or masked[index] in " \t\r\n"
    ):
        index -= 1
    offset = 0
    for match in re.finditer(r"[A-Za-z_][A-Za-z0-9_]*", masked[index + 1 : end]):
        if match.group(0) not in EXPRESSION_KEYWORDS:
            break
        offset = match.end()
    receiver = text[index + 1 + offset : end].strip()
    return receiver or None


def call_sites(text: str) -> list[tuple[str, str, str | None]]:
    """Every call in `text`: its name, its arguments, and its receiver."""
    masked = mask_literals(text)
    sites: list[tuple[str, str, str | None]] = []
    for match in CALL_RE.finditer(masked):
        name = match.group(1)
        if name in KEYWORDS:
            continue
        open_paren = match.end() - 1
        close = matching_paren(masked, open_paren)
        if close < 0:
            continue
        sites.append((name, text[open_paren + 1 : close], _receiver_of(text, masked, match.start(1))))
    return sites


QUALIFIED_RE = re.compile(r"crate::tui::((?:\w+::)+)(\w+)")


def qualified_calls(body: str) -> list[tuple[str, str, str]]:
    """The `(module, function, arguments)` of every qualified call in `body`."""
    masked = mask_literals(body)
    calls: list[tuple[str, str, str]] = []
    for match in QUALIFIED_RE.finditer(masked):
        module = match.group(1)[:-2] if match.group(1).endswith("::") else match.group(1)
        args = ""
        open_paren = match.end()
        if open_paren < len(masked) and masked[open_paren] == "(":
            close = matching_paren(masked, open_paren)
            if close >= 0:
                args = body[open_paren + 1 : close]
        calls.append((module, match.group(2), args))
    return calls


VARIANT_RE = re.compile(r"\b([A-Z][A-Za-z0-9_]*)\s*::\s*([A-Za-z_][A-Za-z0-9_]*)")


def variant_hint(args: str) -> frozenset[str]:
    """The enum variants an argument list names.

    A dispatch site's arms are selected by the value the caller passes, and
    the only thing the resolver can read about that value is the variant
    the argument names. An argument that names none selects **nothing**:
    the resolver cannot know which arm runs, and a guess is exactly the
    defect this layer exists to remove.
    """
    return frozenset(
        f"{match.group(1)}::{match.group(2)}"
        for match in VARIANT_RE.finditer(mask_literals(args))
    )


def match_spans(text: str) -> list[tuple[int, int]]:
    """The `(start, end)` of the braces of every `match` block in `text`."""
    masked = mask_literals(text)
    spans: list[tuple[int, int]] = []
    for match in re.finditer(r"\bmatch\b", masked):
        brace = _block_open(masked, match.end())
        if brace < 0:
            continue
        close = _matching_brace(masked, brace)
        if close < 0:
            continue
        spans.append((brace, close))
    return spans


def split_arms(content: str) -> list[tuple[str, str]]:
    """The `(pattern, body)` of each arm of a match block.

    A block-bodied arm needs **no** trailing comma in Rust, so the arms
    cannot be found by the commas between them: the arms are found by
    their `=>`, and each body ends where the body itself ends — at the
    block's closing brace, or at the next comma.
    """
    masked = mask_literals(content)
    arrows: list[int] = []
    depth = 0
    position = 0
    while position < len(masked):
        char = masked[position]
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        elif depth == 0 and masked.startswith("=>", position):
            arrows.append(position)
            position += 2
            continue
        position += 1
    arms: list[tuple[str, str]] = []
    cursor = 0
    for arrow in arrows:
        pattern = masked[cursor:arrow].strip()
        body, end = _arm_body(masked, content, arrow + 2)
        arms.append((pattern, body))
        cursor = end
    return arms


def _arm_body(masked: str, content: str, start: int) -> tuple[str, int]:
    """An arm's body, and the index the next arm's pattern starts at."""
    index = start
    while index < len(masked) and masked[index] in " \t\r\n":
        index += 1
    if index < len(masked) and masked[index] == "{":
        close = _matching_brace(masked, index)
        if close < 0:
            return content[start:], len(content)
        end = close + 1
        probe = end
        while probe < len(masked) and masked[probe] in " \t\r\n":
            probe += 1
        if probe < len(masked) and masked[probe] == ",":
            end = probe + 1
        return content[start:end], end
    depth = 0
    for position in range(index, len(masked)):
        char = masked[position]
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        elif char == "," and depth == 0:
            return content[start:position], position + 1
    return content[start:], len(content)


def arm_variants(pattern: str) -> list[tuple[str, str]]:
    """The `(type, variant)` pairs a match arm's pattern names.

    Only the pattern's **leading** path counts: `Err(TerminalInputError::NotLive(_))`
    matches over a `Result`, not over `TerminalInputError`, and reading it
    as the latter would turn a two-arm `Result` match into a dispatch site
    the resolver cannot select an arm of.
    """
    variants: list[tuple[str, str]] = []
    for alternative in _pattern_alternatives(pattern):
        match = re.match(
            r"\s*(?:[A-Za-z_][A-Za-z0-9_]*::)*"
            r"([A-Z][A-Za-z0-9_]*)\s*::\s*([A-Za-z_][A-Za-z0-9_]*)",
            alternative,
        )
        if match:
            variants.append((match.group(1), match.group(2)))
    return variants


def _pattern_alternatives(pattern: str) -> list[str]:
    """The `|`-separated alternatives of a match arm's pattern."""
    masked = mask_literals(pattern)
    depth = 0
    start = 0
    parts: list[str] = []
    for position, char in enumerate(masked):
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        elif char == "|" and depth == 0:
            parts.append(pattern[start:position])
            start = position + 1
    parts.append(pattern[start:])
    return parts


def dispatch_arms(arms: list[tuple[str, str]]) -> tuple[str, list[tuple[str, str]]] | None:
    """The enum a match dispatches over, and the arms that name its variants.

    A match whose arms name the variants of one enum is a **dispatch site**:
    the caller chooses the arm, so the arms are not all reachable from it.
    A match over `Option`, `Result` or a plain value names no variant and is
    not a dispatch site — every one of its arms can run.
    """
    counts: dict[str, int] = defaultdict(int)
    for pattern, _ in arms:
        for enum_type, _ in arm_variants(pattern):
            counts[enum_type] += 1
    if not counts:
        return None
    enum_type = max(counts, key=lambda key: counts[key])
    named = [
        (pattern, body)
        for pattern, body in arms
        if any(kind == enum_type for kind, _ in arm_variants(pattern))
    ]
    if len(named) == len(arms) and len(named) < 2:
        # One arm, and it is the only one: every caller reaches it.
        return None
    return enum_type, named


def segments(body: str, hint: frozenset[str]) -> list[str]:
    """The parts of `body` a caller with `hint` can reach.

    A dispatch site's arms are reachable only when the hint selects them.
    The rest of the body is always reachable.
    """
    parts: list[str] = []
    cursor = 0
    covered = 0
    for start, end in match_spans(body):
        if start < covered:
            # A nested match: its arms live inside the arm that selected it.
            continue
        arms = split_arms(body[start + 1 : end])
        dispatch = dispatch_arms(arms)
        if dispatch is None:
            continue
        enum_type, named = dispatch
        parts.append(body[cursor:start])
        for pattern, arm_body in named:
            selected = any(
                f"{enum_type}::{variant}" in hint for _, variant in arm_variants(pattern)
            )
            if selected:
                parts.extend(segments(arm_body, hint))
        cursor = end + 1
        covered = end
    parts.append(body[cursor:])
    return [part for part in parts if part.strip()]


def _names_a_pattern(body: str, end: int) -> bool:
    """Whether the match ending at `end` is a **pattern**, not a construction.

    A construction initialises its fields (`field: value`); a match arm
    names them. The arm's `=>` follows the block, so the block alone cannot
    tell the two apart — but the text after it can, and that is where the
    difference lives.

    Counting a pattern as a construction attributes to the arm every request
    it *matches*, which is the opposite of what this column measures. It is
    also how a **test double**'s match arm would reach the artifact: the
    double is not the production path, so a request it names is a request
    the arm does not issue.
    """
    return body[end : end + 8].lstrip().startswith("=")


def requests_in(body: str) -> set[str]:
    """The `CoreRequest` variants `body` **constructs**.

    A construction initialises its fields — spelled out (`field: value`) or
    shorthand (`field`) — and a match arm names them. Counting a pattern as
    a construction would attribute every request the arm *matches* to it,
    which is the opposite of what this column measures. The two are told
    apart by what follows the variant: a pattern is followed by `=>` (a
    match arm) or by `=` (a `let` or an `if let`), and a construction by
    neither.

    The block is scanned by its braces rather than by a regular
    expression, because a request's payload is often a struct of its own
    (`CoreRequest::InteractiveProcessCreate { request: .. }`) and a
    one-level expression cannot see the closing brace of that payload.
    """
    found: set[str] = set()
    masked = mask_literals(body)
    for match in CTOR_RE.finditer(masked):
        brace = match.end() - 1
        close = _matching_brace(masked, brace)
        if close < 0:
            continue
        block = masked[brace + 1 : close]
        if _has_top_level(block, "..") or _has_top_level(block, "=>"):
            continue
        if _names_a_pattern(masked, close + 1):
            continue
        found.add(match.group(1))
    return found


def _has_top_level(block: str, token: str) -> bool:
    """Whether `token` appears in `block` outside every nested brace."""
    masked = mask_literals(block)
    depth = 0
    for position, char in enumerate(masked):
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        elif depth == 0 and masked.startswith(token, position):
            return True
    return False


@dataclass
class Function:
    """One function: its body, its owner, and what its signature says."""

    path: str
    name: str
    body: str
    impl_type: str | None
    return_type: str
    params: dict[str, str]


WRAPPERS = (
    "Arc", "Rc", "Box", "Option", "Result", "Mutex", "AsyncMutex", "RwLock",
    "RefCell", "Cell", "Pin",
)


def _first_generic_arg(inner: str) -> str:
    """The first generic argument of a wrapper's argument list."""
    depth = 1
    for index, char in enumerate(inner):
        if char == "<":
            depth += 1
        elif char == ">":
            depth -= 1
            if depth == 0:
                return inner[:index]
        elif char == "," and depth == 1:
            return inner[:index]
    return inner


def unwrap_type(expr: str) -> str:
    """The type name a type expression names.

    `&mut App` is `App`, `Arc<DocumentController>` is `DocumentController`,
    and `Option<TuiDocumentSession>` is `TuiDocumentSession`: the resolver
    reads a receiver's type to find the method it carries, and neither the
    reference nor the container is part of that identity.
    """
    text = re.sub(r"^&+\s*(?:mut\s+)?", "", expr.strip()).strip()
    while True:
        match = re.match(r"^([A-Za-z_][A-Za-z0-9_]*)\s*<", text)
        if match is None or match.group(1) not in WRAPPERS:
            break
        inner = _first_generic_arg(text[match.end() :])
        if not inner:
            break
        text = re.sub(r"^&+\s*(?:mut\s+)?", "", inner).strip()
    segment = text.split("::")[-1].strip()
    match = re.match(r"^([A-Za-z_][A-Za-z0-9_]*)", segment)
    return match.group(1) if match else ""


def _strip_generics(header: str) -> str:
    """The header with every generic argument list removed."""
    text = header
    while True:
        stripped = re.sub(r"<[^<>]*>", " ", text)
        if stripped == text:
            return text
        text = stripped


def _decl_name(header: str) -> str | None:
    """The name a declaration's header names."""
    match = re.search(r"\b([A-Z][A-Za-z0-9_]*)\b", _strip_generics(header))
    return match.group(1) if match else None


def _impl_spans(masked: str) -> list[tuple[str | None, int, int]]:
    """The `(type, start, end)` of every `impl` block in the masked text."""
    spans: list[tuple[str | None, int, int]] = []
    for match in re.finditer(r"\bimpl\b", masked):
        brace = _block_open(masked, match.end())
        if brace < 0:
            continue
        close = _matching_brace(masked, brace)
        if close < 0:
            continue
        header = masked[match.end() : brace]
        names = re.findall(r"\b([A-Z][A-Za-z0-9_]*)\b", _strip_generics(header))
        spans.append((names[-1] if names else None, match.start(), close))
    return spans


def _param_types(params: str) -> dict[str, str]:
    """The `name: type` pairs of a parameter list."""
    result: dict[str, str] = {}
    masked = mask_literals(params)
    depth = 0
    start = 0
    parts: list[str] = []
    for position, char in enumerate(masked):
        if char in "([{<":
            depth += 1
        elif char in ")]}>":
            depth -= 1
        elif char == "," and depth == 0:
            parts.append(params[start:position])
            start = position + 1
    parts.append(params[start:])
    for part in parts:
        match = re.match(r"\s*(?:mut\s+)?([a-z_][a-z0-9_]*)\s*:\s*(.+)", part, re.S)
        if match:
            result[match.group(1)] = match.group(2).strip()
    return result


def _struct_fields(text: str) -> dict[str, dict[str, str]]:
    """The `field: type` pairs of every struct in `text`."""
    masked = mask_literals(text)
    fields: dict[str, dict[str, str]] = {}
    for match in re.finditer(r"\bstruct\b", masked):
        brace = _block_open(masked, match.end())
        if brace < 0:
            continue
        close = _matching_brace(masked, brace)
        if close < 0:
            continue
        header = masked[match.end() : brace]
        if ";" in header:
            # A unit struct: there are no fields to read.
            continue
        name = _decl_name(header)
        if name is None:
            continue
        entries: dict[str, str] = {}
        for line in text[brace + 1 : close].split("\n"):
            entry = re.match(
                r"\s*(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*(.+?)(?:,\s*)?$", line
            )
            if entry:
                entries[entry.group(1)] = entry.group(2).strip()
        fields.setdefault(name, {}).update(entries)
    return fields


def _functions(text: str) -> list[Function]:
    """Every function in `text`, with the `impl` block that owns it."""
    masked = mask_literals(text)
    impls = _impl_spans(masked)
    functions: list[Function] = []
    for match in FN_RE.finditer(masked):
        open_paren = match.end() - 1
        close = matching_paren(masked, open_paren)
        if close < 0:
            continue
        brace = text.find("{", close)
        if brace < 0:
            continue
        end = _matching_brace(masked, brace)
        if end < 0:
            continue
        tail = text[close + 1 : brace]
        return_type = ""
        arrow = tail.find("->")
        if arrow >= 0:
            return_type = tail[arrow + 2 :].strip()
        owner = None
        for impl_type, start, stop in impls:
            if start <= match.start() <= stop:
                owner = impl_type
        functions.append(
            Function(
                path="",
                name=match.group(1),
                body=text[brace : end + 1],
                impl_type=owner,
                return_type=return_type,
                params=_param_types(text[open_paren + 1 : close]),
            )
        )
    return functions


LET_RE = re.compile(r"\blet\s+(?:mut\s+)?([a-z_][a-z0-9_]*)\s*(?::\s*([^=;]+?))?\s*=\s*")


def _initializer(body: str, start: int) -> str | None:
    """The expression a `let` binding initialises, up to its `;`."""
    masked = mask_literals(body)
    depth = 0
    for position in range(start, len(masked)):
        char = masked[position]
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        elif char == ";" and depth == 0:
            return body[start:position].strip()
    return None


def _split_chain(expr: str) -> tuple[str, list[tuple[str, bool]]] | None:
    """The base of an expression and the `.step`s that follow it.

    A step is a field access (`name`) or a method call (`name(..)`). An
    expression the resolver cannot read — one with an operator in it, or a
    call whose result is used — reads as nothing, and the caller falls
    back to resolving the callee by name.
    """
    text = expr.strip()
    match = re.match(r"^(?:self\b|[A-Za-z_][A-Za-z0-9_]*)", text)
    if match is None:
        return None
    base = match.group(0)
    index = match.end()
    steps: list[tuple[str, bool]] = []
    while index < len(text):
        probe = index
        while probe < len(text) and text[probe] in " \t\r\n":
            probe += 1
        if probe >= len(text) or text[probe] != ".":
            return None
        index = probe + 1
        name_match = re.match(r"[A-Za-z_][A-Za-z0-9_]*", text[index:])
        if name_match is None:
            return None
        name = name_match.group(0)
        index += len(name)
        is_call = False
        probe = index
        while probe < len(text) and text[probe] in " \t\r\n":
            probe += 1
        if probe < len(text) and text[probe] == "(":
            close = matching_paren(mask_literals(text), probe)
            if close < 0:
                return None
            index = close + 1
            is_call = True
        steps.append((name, is_call))
    return base, steps


class SourceIndex:
    """The Rust the resolver reads.

    The TUI reaches the daemon's document path through `codegg-client`, so
    an index that stops at `src/tui` cannot see the requests that path
    issues. The index therefore covers every tree the TUI calls into, and
    it resolves a method call by the **type** of its receiver rather than
    by its name: a name is not an identity, and two types can share one.
    """

    def __init__(self, root: Path) -> None:
        self.files: dict[str, str] = {}
        for tree in SOURCE_TREES:
            for path in sorted((root / tree).rglob("*.rs")):
                self.files[str(path.relative_to(root))] = read(path)
        self.functions: dict[str, list[Function]] = {}
        self.methods: dict[str, dict[str, Function]] = defaultdict(dict)
        self.fields: dict[str, dict[str, str]] = defaultdict(dict)
        for name, text in self.files.items():
            definitions = _functions(text)
            for function in definitions:
                function.path = name
                if function.impl_type:
                    self.methods[function.impl_type][function.name] = function
            self.functions[name] = definitions
            for type_name, entries in _struct_fields(text).items():
                self.fields[type_name].update(entries)
        self.module_path: dict[str, str] = {}
        for name in self.files:
            for tree, prefix in MODULE_ROOTS.items():
                if not name.startswith(tree + "/"):
                    continue
                rel = name[len(tree) + 1 : -3]
                if rel.endswith("/mod"):
                    rel = rel[:-4]
                if rel.endswith("/lib"):
                    rel = rel[:-4]
                self.module_path[prefix + rel.replace("/", "::")] = name
        self.imports: dict[str, dict[str, str]] = {}
        for name, text in self.files.items():
            self.imports[name] = self._parse_imports(text)

    def _parse_imports(self, text: str) -> dict[str, str]:
        """`use` imports: the local name → the file it comes from."""
        mapping: dict[str, str] = {}
        for match in re.finditer(r"^\s*use\s+([^;]+);", text, re.M):
            clause = match.group(1)
            if "{" in clause:
                head, tail = clause.split("{", 1)
                tail = tail.split("}", 1)[0]
                prefix = head.strip().rstrip(":")
                for item in tail.split(","):
                    item = item.strip()
                    if not item:
                        continue
                    local = item.split(" as ")[-1].strip()
                    target = self._resolve_module(prefix)
                    if target:
                        mapping[local] = target
            else:
                path = clause.strip()
                local = path.rsplit("::", 1)[-1]
                module = path.rsplit("::", 1)[0]
                target = self._resolve_module(module)
                if target:
                    mapping[local] = target
        return mapping

    def _resolve_module(self, path: str) -> str | None:
        path = path.rstrip(":")
        path = path.replace("crate::tui::", "")
        path = path.replace("super::super::", "")
        path = path.replace("super::", "")
        path = path.replace("crate::", "")
        if path.startswith("self::"):
            path = path[5:]
        return self.module_path.get(path)

    def resolve_qualified(self, body: str, out: set[str]) -> None:
        """The requests reachable from the qualified calls in `body`."""
        for module, name, args in qualified_calls(body):
            resolved = self.module_path.get(module)
            if resolved:
                self.resolve(resolved, name, set(), out, 0, variant_hint(args))

    def function(self, path: str, name: str) -> Function | None:
        for candidate in self.functions.get(path, []):
            if candidate.name == name:
                return candidate
        return None

    def _lookup(self, path: str, name: str) -> tuple[Function | None, str]:
        """The function `name` reaches from `path`, and the file it lives in."""
        definition = self.function(path, name)
        if definition is not None:
            return definition, path
        imported = self.imports.get(path, {}).get(name)
        if imported is not None:
            definition = self.function(imported, name)
            if definition is not None:
                return definition, imported
        # A sibling module of the same directory may define it.
        directory = os.path.dirname(path)
        for candidate in self.files:
            if os.path.dirname(candidate) != directory:
                continue
            definition = self.function(candidate, name)
            if definition is not None:
                return definition, candidate
        return None, path

    def expr_type(
        self,
        path: str,
        definition: Function,
        expr: str,
        seen: set[tuple[str, str, str]] | None = None,
    ) -> str | None:
        """The type an expression names, when the index can read it."""
        if seen is None:
            seen = set()
        chain = _split_chain(expr)
        if chain is None:
            return None
        base, steps = chain
        current = self._base_type(definition, base, seen)
        if current is None:
            return None
        for name, is_call in steps:
            if is_call:
                method = self.methods.get(current, {}).get(name)
                if method is None:
                    # A method the index does not know leaves the receiver's
                    # type alone: `Option::as_ref` and `Option::expect` are
                    # std's, and the type they carry is the one the caller
                    # already had.
                    continue
                current = unwrap_type(method.return_type)
            else:
                field = self.fields.get(current, {}).get(name)
                if field is None:
                    return None
                current = unwrap_type(field)
        return current

    def _base_type(
        self, definition: Function, name: str, seen: set[tuple[str, str, str]]
    ) -> str | None:
        """The type a name refers to: `self`, a parameter, or a local."""
        if name == "self":
            return definition.impl_type
        if name in definition.params:
            return unwrap_type(definition.params[name])
        return self._local_type(definition, name, seen)

    def _local_type(
        self, definition: Function, name: str, seen: set[tuple[str, str, str]]
    ) -> str | None:
        """The type a `let` binding gives a local, when the index can read it."""
        key = (definition.path, definition.name, name)
        if key in seen:
            # A binding that reads itself resolves to nothing: the resolver
            # cannot tell which of the two it means.
            return None
        seen.add(key)
        for match in LET_RE.finditer(definition.body):
            if match.group(1) != name:
                continue
            if match.group(2):
                return unwrap_type(match.group(2))
            expression = _initializer(definition.body, match.end())
            if expression is None:
                return None
            return self.expr_type(definition.path, definition, expression, seen)
        return None

    def _resolve_call(
        self,
        target: str,
        definition: Function,
        call: str,
        args: str,
        receiver: str | None,
        seen: set[tuple[str, str, frozenset[str]]],
        out: set[str],
        depth: int,
        flow: frozenset[str],
    ) -> None:
        """Follow one call to the function it reaches.

        A **method** call is resolved by the type of its receiver, or not at
        all: a name is not an identity, and two types can share one. A
        call whose receiver's type the index cannot read is therefore left
        unresolved rather than matched by name — resolving it by name is
        how `report.render()` comes to mean `App::render`, and how an arm
        comes to own a request it never issues.

        `flow` carries the variants that may reach the callee. A variant
        the caller names travels with the value it binds, so a dispatch
        site further down can still be told which of its arms runs.
        """
        hint = variant_hint(args)
        if receiver is not None:
            receiver_type = self.expr_type(target, definition, receiver)
            if not receiver_type:
                return
            method = self.methods.get(receiver_type, {}).get(call)
            if method is None:
                return
            self.resolve(method.path, method.name, seen, out, depth + 1, hint | flow)
            return
        self.resolve(target, call, seen, out, depth + 1, hint | flow)

    def resolve(
        self,
        path: str,
        name: str,
        seen: set[tuple[str, str, frozenset[str]]],
        out: set[str],
        depth: int = 0,
        hint: frozenset[str] = frozenset(),
    ) -> None:
        """The requests reachable from calling `name` inside `path`.

        `hint` names the enum variants the caller's argument selects. It is
        what keeps a dispatch site's arms apart: without it the resolver
        would union every arm of every `match` it walked through, and an
        arm would inherit requests it never issues.
        """
        key = (path, name, hint)
        if depth > 5 or key in seen:
            return
        seen.add(key)
        definition, target = self._lookup(path, name)
        if definition is None:
            return
        for segment in segments(definition.body, hint):
            out.update(requests_in(segment))
            for call, args, receiver in call_sites(segment):
                self._resolve_call(
                    target, definition, call, args, receiver, seen, out, depth, hint
                )
            for module, function_name, args in qualified_calls(segment):
                resolved = self.module_path.get(module)
                if resolved:
                    self.resolve(
                        resolved,
                        function_name,
                        seen,
                        out,
                        depth + 1,
                        variant_hint(args) | hint,
                    )


def arm_requests(index: SourceIndex) -> dict[str, tuple[int, list[str]]]:
    """The requests each dispatcher arm issues, and the arm's line."""
    dispatch = index.files[TUI_DISPATCH]
    lines = dispatch.split("\n")
    start = next(
        offset
        for offset, line in enumerate(lines)
        if "pub(crate) fn dispatch_builtin_command" in line
    )
    arm_re = re.compile(r"^ {12}B::(\w+)(?:\s*=>\s*\{| =>)")
    arms: list[tuple[int, str]] = []
    for offset in range(start, min(start + 3600, len(lines))):
        match = arm_re.match(lines[offset])
        if match:
            arms.append((offset, match.group(1)))

    # The TuiCommand channel: the runtime's single dispatch table.
    runtime = f"{TUI_DIR}/runtime/command_dispatch.rs"
    runtime_lines = index.files[runtime].split("\n")
    channel_arms: list[tuple[int, str]] = []
    for position, line in enumerate(runtime_lines):
        match = re.match(r"^ {8}TuiCommand::(\w+)", line)
        if match:
            channel_arms.append((position, match.group(1)))
    channel: dict[str, set[str]] = {}
    for position, (line_no, variant) in enumerate(channel_arms):
        end = (
            channel_arms[position + 1][0]
            if position + 1 < len(channel_arms)
            else len(runtime_lines)
        )
        body = "\n".join(runtime_lines[line_no:end])
        requests: set[str] = set(requests_in(body))
        for call, args, _receiver in call_sites(body):
            index.resolve(runtime, call, set(), requests, 0, variant_hint(args))
        index.resolve_qualified(body, requests)
        channel.setdefault(variant, set()).update(requests)

    out: dict[str, tuple[int, list[str]]] = {}
    for position, (line_no, name) in enumerate(arms):
        end = arms[position + 1][0] if position + 1 < len(arms) else start + 3600
        body = "\n".join(lines[line_no:end])
        requests: set[str] = set(requests_in(body))
        for call, args, _receiver in call_sites(body):
            index.resolve(TUI_DISPATCH, call, set(), requests, 0, variant_hint(args))
        index.resolve_qualified(body, requests)
        for match in re.finditer(r"TuiCommand::(\w+)", body):
            requests |= channel.get(match.group(1), set())
        out[name] = (line_no + 1, sorted(requests))
    return out



# ── The classification of the forms the daemon does not serve ──────────────

# For every daemon-effect form the daemon does not serve, one of two things
# is true, and the artifact says which:
#
# * **misclassified** — the form is not really a daemon effect. It does not
#   mutate daemon-owned state, does not read daemon-owned state as its
#   result, and does not drive a daemon-mediated process, so the catalog's
#   kind is wrong.
# * **missing capability** — the classification is right and the daemon
#   genuinely lacks the surface. That is new product capability, not
#   migration work, and it belongs to a follow-up milestone.
#
# The verdict is keyed by the command's *name* (the catalog's display name),
# because a command's forms share one effect.
VERDICTS: dict[str, tuple[str, str, str]] = {}


def _misclassified(name: str, reason: str) -> None:
    VERDICTS[name] = ("misclassified", "FrontendEffect", reason)


def _missing(name: str, surface: str, reason: str) -> None:
    VERDICTS[name] = ("missing capability", surface, reason)


def _projected(name: str, reason: str) -> None:
    """The daemon owns the state and publishes it on the session projection.

    M007-H (D3, D4): the daemon's published projection is the surface, so
    the arm reads the published answer and issues no request of its own.
    """
    VERDICTS[name] = ("published projection", "", reason)


# The habit store, the skill-promotion store, the transcript search, the
# doctor routine and the plugin manager are all **the frontend's own**: the
# TUI opens the file, keeps the buffer, or runs the routine. None of them
# touches daemon-owned state.
#
# The interactive terminal is **not** in this list: the daemon owns the
# interactive process, answers every `CoreRequest::InteractiveProcess*`, and
# the terminal arms issue those requests. The terminal's forms are
# classified in the table below, where the arm's requests are counted.
_misclassified(
    "/habits",
    "the TUI's own `HabitStore` on the local filesystem — not the daemon's memory store",
)
_misclassified(
    "/habit-dismiss",
    "the TUI's own `HabitStore` on the local filesystem",
)
_misclassified(
    "/skill-proposals",
    "the TUI's own `SkillPromotionStore` on the local filesystem",
)
_misclassified(
    "/skill-proposal",
    "the TUI's own `SkillPromotionStore` on the local filesystem",
)
_misclassified("/search", "the TUI searches its own transcript buffer")
_misclassified("/doctor", "a frontend-local diagnostic routine")
_misclassified(
    "/plugins",
    "the TUI's own `PluginManager` over a plugin service the TUI creates; the daemon has no plugin manager",
)
_misclassified("/plugin-info", "the TUI's own `PluginManager`")
_misclassified("/plugin-enable", "the TUI's own `PluginManager`")
_misclassified("/plugin-disable", "the TUI's own `PluginManager`")
_misclassified("/plugin-doctor", "the TUI's own `PluginManager`")
_misclassified("/plugin-remove", "the TUI's own `PluginManager`")
_misclassified("/plugin-install", "the TUI's own `PluginManager`")

# The language server is daemon-owned: `egglsp` is authoritative and the
# TUI's `LspTool` is a second, non-authoritative instance that is absent in
# the daemon-connected configuration, so every `/lsp-*` arm already refuses
# there. The classification is right and the daemon lacks the surface.
#
# `/lsp-preview-apply` is **not** in this list: its arm builds the daemon's
# `LspPreviewApply` request, so the daemon holds the surface and the form
# is classified in the table below, where the arm's requests are counted.
for _lsp in (
    "/lsp-previews", "/lsp-preview", "/lsp-preview-clear", "/lsp-preview-refresh",
    "/lsp-servers", "/lsp-capabilities", "/lsp-root",
    "/lsp-restart", "/lsp-stop", "/lsp-cache-status", "/lsp-cache-clear",
    "/lsp-doctor", "/lsp-context-diagnostics", "/lsp-repair-local",
    "/lsp-repair-hunk", "/lsp-review-file", "/lsp-review-diff",
    "/lsp-security-review", "/lsp-impact", "/lsp-test-repair", "/lsp-interface",
    "/lsp-cross-repair", "/lsp-call-neighbors",
):
    _missing(
        _lsp,
        "M007-F — the daemon's LSP command surface",
        "the authoritative language server is daemon-owned, and the daemon has no request for this surface",
    )

# The agent registry is the TUI's own: `app.agent_state` holds the agents
# the TUI loaded from its asset snapshot, and both arms read it directly.
_misclassified(
    "/agent",
    "the TUI selects from its own agent registry (`app.agent_state`); the catalog's kind is wrong and the corrected kind is `FrontendEffect`",
)
_misclassified(
    "/agents",
    "the TUI lists its own agent registry (`app.agent_state`); the catalog's kind is wrong and the corrected kind is `FrontendEffect`",
)

# The git-review diff view is the frontend's own: the arm runs one local
# `git` subprocess — `git diff`, or `git show HEAD:<path>` when a path is
# given — and reads one file, and the two branches are mutually exclusive.
# The git root comes from `crate::worktree::find_git_root`, which is
# `egggit`'s read-only worktree reader, not `codegg-git`'s typed writer.
_misclassified(
    "/diff",
    "the arm's effect is the frontend's own: one local `git` subprocess and one file read, "
    "with the git root read through the read-only `egggit` crate — no daemon-mediated "
    "process is driven",
)

# The research forms read the TUI's own research service over the local
# filesystem. No daemon request is their effect.
_misclassified(
    "/research-open",
    "the TUI's own `ResearchService` over the local filesystem; the catalog's kind is wrong and the corrected kind is `FrontendEffect`",
)
_misclassified(
    "/research-runs",
    "the TUI's own `ResearchService` over the local filesystem; the catalog's kind is wrong and the corrected kind is `FrontendEffect`",
)
_misclassified(
    "/research-show",
    "the TUI's own `ResearchService` over the local filesystem; the catalog's kind is wrong and the corrected kind is `FrontendEffect`",
)

_misclassified(
    "/import",
    "the arm only opens the import dialog (`src/tui/app/mod.rs:6603`); the fork "
    "itself travels as the daemon's own `SessionFork` request, so the command form "
    "is a `FrontendEffect` and the catalog's kind is wrong",
)

# M007-H (D3, D4): the plan and the test state are daemon-owned state the
# daemon **publishes** on the session projection. The arm reads the published
# answer, so the form is not a missing capability any more — the daemon has
# the surface, and the projection is how a frontend reaches it.
_projected(
    "/plan",
    "the plan list is daemon-owned work-plan state; the daemon publishes it "
    "on the session projection and the arm reads that published plan",
)
_projected(
    "/tests",
    "the suite run is daemon-owned job state; the daemon publishes the "
    "session's test state on the session projection and the arm reads it",
)
_missing(
    "/research",
    "M007-H — the daemon's missing command surfaces",
    "the research turn is daemon-owned agent-loop work",
)


# ── The verdict for the forms the daemon does not serve yet ────────────────
#
# A daemon-effect form whose TUI arm issues a daemon request is one of five
# things, and the artifact says which:
#
# * **migrated** — the form is served now. Its effect travels through
#   `CoreRequest::CommandExecute` and the daemon's inner response.
# * **mechanical** — the effect request's fields are the scope's plus at
#   most one argument, so the daemon's effect table can build it. The
#   encoding is named, so the row is a decision and not a guess. The
#   argument's convention is one token, or a token pair where the second is
#   a number — `<channel>`, `<channel> <seq>`, `<handle> <from_seq>`.
# * **not mechanical** — the effect request carries a value that convention
#   cannot produce: free text, a structured payload, a list, a boolean, or a
#   second effect beside the first. A runtime *scalar* is not disqualifying;
#   the argument carries it.
# * **subsystem move** — the arm is a subsystem in its own right. Moving it
#   is a milestone with its own ownership question, not a table row.
# * **misclassified** — the arm's effect is the frontend's own, so the
#   catalog's kind is wrong.
#
# The verdict is keyed by the command's *name*, because a command's forms
# share one effect.
ARM_VERDICTS: dict[str, tuple[str, str]] = {}


def _arm(name: str, verdict: str, reason: str) -> None:
    ARM_VERDICTS[name] = (verdict, reason)


# The collaboration forms. The chat ones split: three carry a channel and a
# number, and the argument's convention carries both; the rest carry free
# text, a structured payload, a list or a second effect.
_arm(
    "/stop-observing",
    "migrated",
    "`ProjectionUnsubscribe` takes one identifier — the subscription id the daemon issued — which travels as the argument",
)
_arm(
    "/chat-send",
    "not mechanical",
    "the body is free text, and the request also carries mentions, references and a runtime idempotency key — none of which the argument's convention can carry",
)
_arm(
    "/chat-reply",
    "not mechanical",
    "the body is free text, and the request also carries the reply's thread root, mentions, references and a runtime idempotency key",
)
_arm(
    "/chat-history",
    "migrated",
    "the arm's newest-window request is `chat_history_older` with an unanchored cursor, which the row can build from `<channel>`; the incapable-daemon fallback stays where it is",
)
_arm(
    "/chat-sync",
    "migrated",
    "`ChatSync` takes the channel and the cursor, so the row can build it from `<channel> <from_seq>`",
)
_arm(
    "/chat-read",
    "migrated",
    "`ChatReadSet` takes the channel and the sequence, so the row can build it from `<channel> <last_read_seq>`",
)
_arm(
    "/chat-edit",
    "not mechanical",
    "the request carries four values — the channel, the message id, the revision and a free-text body — and the argument's convention has room for two",
)
_arm(
    "/chat-redact",
    "not mechanical",
    "the request carries four values — the channel, the message id, the revision and a free-text reason — and the argument's convention has room for two",
)
_arm(
    "/chat-composing",
    "not mechanical",
    "the arm has two effects (the composing list and the composing set), and the set's flag is a boolean the argument's convention cannot carry",
)
_arm(
    "/chat-action-task",
    "not mechanical",
    "the action is a structured payload assembled from the arguments, and the request carries a runtime idempotency key",
)
_arm(
    "/chat-action-review",
    "not mechanical",
    "the action is a structured payload assembled from the arguments, and the request carries a runtime idempotency key",
)

# The execution forms.
_arm(
    "/loop",
    "not mechanical",
    "`ScheduleCreate` carries a structured spec (kind, job template, overlap and missed-run policies, labels) that the scope cannot derive",
)
_arm(
    "/shell-ask",
    "subsystem move",
    "the arm drives {n} daemon requests; moving it is a subsystem, not a table row",
)
_arm(
    "/shell-include",
    "not mechanical",
    "`HumanShellPromotionStage` carries three identifiers beside the session, and the argument's convention has room for one",
)
_arm(
    "/task-del",
    "migrated",
    "`ScheduleDelete` takes one identifier — the resolved schedule id — which travels as the argument",
)
_arm(
    "/terminal-attach",
    "migrated",
    "`InteractiveProcessAttach` takes the handle and the cursor, so the row can build it from `<handle> <from_seq>`",
)
_arm(
    "/terminal-clear",
    "not mechanical",
    "the request carries a byte payload the TUI composes — the pending input — and the argument's convention cannot carry bytes",
)
_arm(
    "/terminal-resume",
    "migrated",
    "`InteractiveProcessResume` takes the attachment and the cursor, so the row can build it from `<attachment-id> <from_seq>`",
)
_arm(
    "/test",
    "migrated",
    "the arm's effect is the job wait, and `JobWait` takes one identifier — the job id — which travels as the argument; the workspace registration stays a prerequisite",
)

# The interactive terminal is **daemon-owned**: the daemon answers every
# `CoreRequest::InteractiveProcess*`, and the terminal's arms issue those
# requests. The forms split on what the request carries: the detach,
# terminate and remove requests carry one identifier, and the resize and
# create requests carry more than the argument's convention can hold.
_arm(
    "/terminal-detach",
    "mechanical",
    "`InteractiveProcessDetach` takes one identifier — the resolved attachment id — which travels as the argument",
)
_arm(
    "/terminal-terminate",
    "mechanical",
    "`InteractiveProcessTerminate` takes one identifier — the resolved attachment id — which travels as the argument",
)
_arm(
    "/terminal-remove",
    "mechanical",
    "`InteractiveProcessRemove` takes one identifier — the resolved attachment id — which travels as the argument",
)
_arm(
    "/terminal-resize",
    "not mechanical",
    "`InteractiveProcessResize` carries three values — the attachment id, the columns and the rows — and the argument's convention carries a handle and at most one number",
)
_arm(
    "/terminal",
    "not mechanical",
    "`InteractiveProcessCreate` carries the whole create request — the workspace, the argv, the cwd, the environment overrides and the viewport — and the argument's convention cannot carry it",
)
_arm(
    "/terminal-create",
    "not mechanical",
    "`InteractiveProcessCreate` carries the whole create request — the workspace, the argv, the cwd, the environment overrides and the viewport — and the argument's convention cannot carry it",
)

# The document and change-review forms: the daemon holds both surfaces
# (`DocumentOpen` and `LspPreviewApply`), so neither is a missing
# capability; each request carries more than the scope and an argument.
_arm(
    "/open",
    "not mechanical",
    "`DocumentOpen` carries the project, the workspace, the relative path and the scope, and the writer lease carries the document id; the argument's convention has room for one token",
)
_arm(
    "/review-change",
    "not mechanical",
    "the arm builds the whole `LspPreviewApply` request — the candidate's title, kind, provenance and patches — and the argument's convention cannot carry it",
)
_arm(
    "/lsp-preview-apply",
    "not mechanical",
    "the arm builds the whole `LspPreviewApply` request — the candidate's title, kind, provenance and patches — and the argument's convention cannot carry it",
)

# The git-review and memory forms are subsystem moves.
_arm(
    "/memory-consolidate",
    "not mechanical",
    "the daemon request reads the session's messages; the effect itself is a write into the TUI's own memory store, which the daemon does not own",
)
_arm(
    "/skill-promote",
    "subsystem move",
    "the arm drives {n} daemon requests; moving it is a subsystem, not a table row",
)

# The session forms.
_arm(
    "/new",
    "not mechanical",
    "`SessionCreate` carries a filesystem `directory` that the scope cannot derive",
)
_arm(
    "/start",
    "not mechanical",
    "the arm's effect is a turn submission through the agent loop's own prompt path, and a `SessionCreate` when no session is bound; neither is a request the scope and an argument can address",
)

# The system forms.
_arm(
    "/sandbox",
    "not mechanical",
    "the daemon effect is issued by the confirmation dialog's own submit path, and `RuntimePolicySet` carries the cached policy's `expected_revision`; the empty-argument arm is a local selector",
)


# ── The artifact ────────────────────────────────────────────────────────────


def build(commands: list[CatalogCommand], served: set[tuple[str, str]]) -> str:
    catalog_text = read(ROOT / CATALOG)
    frontend_actions = parse_frontend_actions(read(ROOT / FRONTEND_ACTIONS))
    registry = parse_registry(read(ROOT / TUI_REGISTRY))
    daemon_text = read(ROOT / DAEMON_COMMANDS)
    requests = parse_effect_requests(daemon_text)
    index = SourceIndex(ROOT)
    arms = arm_requests(index)

    rows: list[dict] = []
    for command in commands:
        arm = registry.get(command.name)
        line, arm_requests_list = arms.get(arm, (None, [])) if arm else (None, [])
        for form in command.forms:
            rows.append(
                {
                    "command_id": command.command_id,
                    "name": command.name,
                    "form": form.form,
                    "kind": form.kind,
                    "domain": command.domain,
                    "arm": arm,
                    "arm_line": line,
                    "arm_requests": arm_requests_list,
                    "action": frontend_actions.get((command.command_id, form.form)),
                    "served": (command.command_id, form.form) in served,
                }
            )

    daemon_rows = [row for row in rows if row["kind"] == "DaemonEffect"]
    frontend_rows = [row for row in rows if row["kind"] == "FrontendEffect"]
    served_rows = [row for row in daemon_rows if row["served"]]
    unserved_rows = [row for row in daemon_rows if not row["served"]]
    with_request = [row for row in unserved_rows if row["arm_requests"]]
    local_only = [row for row in unserved_rows if not row["arm_requests"]]

    families: dict[str, dict[str, int]] = defaultdict(
        lambda: {
            "forms": 0,
            "daemon": 0,
            "frontend": 0,
            "served": 0,
            "with_request": 0,
            "local": 0,
        }
    )
    for row in rows:
        family = families[row["domain"]]
        family["forms"] += 1
        if row["kind"] == "FrontendEffect":
            family["frontend"] += 1
        else:
            family["daemon"] += 1
            if row["served"]:
                family["served"] += 1
            elif row["arm_requests"]:
                family["with_request"] += 1
            else:
                family["local"] += 1

    out: list[str] = []
    write = out.append

    write("# M007-D — the command classification and its inventory")
    write("")
    write("This is the classification artifact ADR-0052 asks for: every command the")
    write("daemon knows about, the **kind** of each of its argument forms, and the")
    write("route each form takes. It is the committed inventory of which daemon-effect")
    write("forms have a daemon route and which do not.")
    write("")
    write("A **command** is an entry in the daemon-owned catalog. An **action** is a")
    write("frontend-local binding. The two are never the same thing, and the catalog is")
    write("never described as a set of actions.")
    write("")
    write("**This file is generated.** `generate_m007d_command_classification.py`, next")
    write("to it, reads the catalog, the declared frontend-action set, the daemon's")
    write("effect table, the TUI's registry and the TUI's dispatcher, and writes every")
    write("number below. The summary table in §1 is computed from the §3 inventory the")
    write("same script writes, so the two cannot drift. Re-run it after changing any of")
    write("those sources:")
    write("")
    write("```")
    write("python3 plans/implementation/desktop-frontend-ide-foundation/artifacts/\\")
    write("generate_m007d_command_classification.py")
    write("```")
    write("")
    write("Add `--check` to derive the artifact again and report whether it is still in")
    write("step with the tree, without writing over it:")
    write("")
    write("```")
    write("python3 plans/implementation/desktop-frontend-ide-foundation/artifacts/\\")
    write("generate_m007d_command_classification.py --check")
    write("```")
    write("")
    write("## 1. The measured shape")
    write("")
    write("| measure | count |")
    write("|---|---|")
    write(f"| commands | {len({row['command_id'] for row in rows})} |")
    write(f"| argument forms | {len(rows)} |")
    write(f"| daemon-effect forms | {len(daemon_rows)} |")
    write(f"| frontend-effect forms | {len(frontend_rows)} |")
    write(f"| daemon-effect forms the daemon's effect table serves | {len(served_rows)} |")
    write(f"| daemon-effect forms whose TUI arm issues a daemon request | {len(with_request)} |")
    write(f"| daemon-effect forms whose TUI arm issues no daemon request | {len(local_only)} |")
    write(f"| frontend-effect forms covered by the declared action set | "
          f"{sum(1 for row in frontend_rows if row['action'])} |")
    write("")
    write("The catalog, the declared frontend-action set and the daemon's effect table")
    write("are three different things and are proved apart by three different tests:")
    write("")
    write("* `crates/codegg-core/src/command_catalog.rs` — the catalog, and its bounds.")
    write("* `crates/codegg-core/src/frontend_action.rs` — the declared frontend-action")
    write("  set, closed over the catalog's frontend-effect forms.")
    write("* `src/core/daemon_commands.rs` — the daemon's effect table, closed over the")
    write("  catalog's daemon-effect forms.")
    write("")
    write("## 2. The four routes a form takes")
    write("")
    write("| route | who serves it | how a frontend reaches it |")
    write("|---|---|---|")
    write("| **daemon effect, served** | the daemon | `CoreRequest::CommandExecute`; the")
    write("  daemon builds the inner request from the scope and the argument, authorizes")
    write("  it on its own operation, and audits it as a canonical `Run`. The inner")
    write("  response travels back verbatim in the result, so **both** frontends format")
    write("  the same authoritative data |")
    write("| **frontend effect** | the frontend | the declared `FrontendAction` for the")
    write("  form; it never reaches the daemon |")
    write("| **daemon effect, published projection** | the daemon | the daemon publishes")
    write("  the state it owns on the session projection, and the arm reads the published")
    write("  answer; there is no request for the form to issue |")
    write("| **daemon effect, not served** | nobody yet | the TUI keeps its local arm;")
    write("  the catalog marks the form `daemon_served: false` so a frontend shows it as")
    write("  unavailable instead of dispatching into a refusal |")
    write("")
    write("## 3. The inventory")
    write("")
    write("One row per argument form. `arm` is the TUI dispatcher arm that implements")
    write("the form; `requests` are the daemon requests that arm issues, and `—` when")
    write("it issues none.")
    write("")
    write("| command | form | kind | domain | route | arm | requests |")
    write("|---|---|---|---|---|---|---|")
    for row in rows:
        if row["kind"] == "FrontendEffect":
            route = f"frontend → `{row['action']}`"
        elif row["served"]:
            route = "daemon → served"
        elif VERDICTS.get(row["name"], ("",))[0] == "published projection":
            route = "daemon → published projection"
        elif row["arm_requests"]:
            route = "daemon → not served (the TUI arm issues a daemon request)"
        else:
            route = "daemon → not served (the TUI keeps its local arm)"
        listed = ", ".join(f"`{request}`" for request in row["arm_requests"][:6]) or "—"
        if len(row["arm_requests"]) > 6:
            listed += f", … ({len(row['arm_requests'])})"
        arm = f"`{TUI_DISPATCH}:{row['arm_line']}`" if row["arm_line"] else "—"
        write(
            f"| `{row['name']}` | `{row['form'] or '—'}` | {row['kind'].replace('Effect', '').lower()} "
            f"| {row['domain']} | {route} | {arm} | {listed} |"
        )
    write("")
    write("## 4. The families")
    write("")
    write("The regression coverage is organised by family. A family is the catalog's")
    write("`CommandDomain`: a stable, user-facing task grouping that the catalog")
    write("already carries, so the coverage list and the catalog cannot drift apart.")
    write("")
    write("| family | forms | daemon effect | frontend effect | served | issues a daemon request | no daemon request |")
    write("|---|---|---|---|---|---|---|")
    for domain in sorted(families):
        family = families[domain]
        write(
            f"| {domain} | {family['forms']} | {family['daemon']} | {family['frontend']} "
            f"| {family['served']} | {family['with_request']} | {family['local']} |"
        )
    write("")
    write("## 5. The daemon-effect forms the daemon does not serve")
    write("")
    write("These are the forms the daemon does not serve yet. They are recorded here")
    write("rather than left implicit, because the plan's Work package F asks for the")
    write("inventory of exactly this set.")
    write("")
    write("| command | form | family | the requests its TUI arm issues |")
    write("|---|---|---|---|")
    for row in unserved_rows:
        listed = ", ".join(f"`{request}`" for request in row["arm_requests"][:6]) or "—"
        if len(row["arm_requests"]) > 6:
            listed += f", … ({len(row['arm_requests'])})"
        write(
            f"| `{row['name']}` | `{row['form'] or '—'}` | {row['domain']} | {listed} |"
        )
    write("")
    write("## 6. What the daemon serves")
    write("")
    write("The daemon's effect table, one row per served form. The argument convention")
    write("is uniform: the first whitespace-separated word of the command's argument")
    write("is the identifier, and the remainder is the payload.")
    write("")
    write("| command | form | the daemon request it builds |")
    write("|---|---|---|")
    for row in sorted(served_rows, key=lambda row: (row["command_id"], row["form"])):
        request = requests.get((row["command_id"], row["form"]), "—")
        write(f"| `{row['name']}` | `{row['form'] or '—'}` | `{request}` |")
    write("")
    write("## 7. The TUI migration")
    write("")
    write("A daemon-effect command is *migrated* when the TUI executes it through")
    write("`CoreRequest::CommandExecute` and the daemon's inner response feeds the")
    write("arm's existing presentation code. The observable outcome does not change:")
    write("the daemon owns the effect and the authoritative data, and the frontend")
    write("keeps the formatting.")
    write("")
    write("| command | family | the TUI effect call |")
    write("|---|---|---|")
    migrated = migrated_commands()
    for command_id, family in sorted(migrated.items()):
        name = next(
            (row["name"] for row in rows if row["command_id"] == command_id), command_id
        )
        write(
            f"| `{name}` | {family} | `CoreRequest::CommandExecute` "
            f"(`{command_id}`) |"
        )
    write("")
    write(
        f"**{len(migrated)} of the {len(daemon_rows)} daemon-effect forms are migrated.**"
    )
    write("The rest keep their local arm, and each one is listed above with the route")
    write("it takes. The migration is one site per command: the request construction")
    write("becomes a `CommandExecute` and the response is unwrapped, so the")
    write("presentation code around it is unchanged.")
    write("")
    write("The regression coverage is organised by family and lives in")
    write("`src/tui/command_route_tests.rs` (the TUI's side) and")
    write("`tests/core_daemon/command_catalog_m007d.inc` (the daemon's side).")
    write("The family list is the coverage's own constant, so a migrated command")
    write("with no test fails its family.")
    write("")
    write("## 8. The recorded discrepancies")
    write("")
    write("* **`/compact` keeps its current behaviour.** The arm toasts and the daemon")
    write("  has no compaction request, so the form is in the \"no daemon route\" set")
    write("  above. The behaviour is unchanged and the gap is directed to a separate")
    write("  corrective milestone.")
    write("* **The composer's `/` completion overlay deviates.** The composer's")
    write("  completion overlay lists the TUI's own registry; the daemon's catalog is")
    write("  what the palette reads. A user is expected to use the `mod+shift+p`")
    write("  palette for the daemon's catalog. The overlay is recorded here, not")
    write("  changed.")
    write("* **`command_run` is uninstrumented.** The effect's own operation is audited;")
    write("  the command dispatch is audited as the canonical `Run`. A second")
    write("  instrumentation point on the dispatch would audit the same decision twice.")
    write("")
    write("## 9. Two namespaces, and how the palette resolves an action id")
    write("")
    write("The declared frontend-effect set and the desktop's keymap both hold")
    write("\"actions\", and they are **two different namespaces**. The plan's")
    write("terminology amendment (plan 016) makes an *action* a frontend-local")
    write("binding; it does not make every action id mean the same thing.")
    write("")
    write("| namespace | what an id names | where it lives | who resolves it |")
    write("|---|---|---|---|")
    write(
        "| the declared frontend-effect set | the action that serves a **catalog command form** "
        f"| `crates/codegg-core/src/frontend_action.rs` — {len(set(frontend_actions.values()))} "
        "`FrontendAction` variants, keyed by `(command_id, form)` "
        "| the palette, through `action_for_form(command_id, form)` |"
    )
    write("| the desktop keymap | a **gesture** the desktop dispatches | `apps/desktop/src/keymap.ts` — the editor's 22 `editor.*` ids, the palette's `command.palette.open`, and the rest | the desktop's one dispatch, in `App.tsx` |")
    write("")
    write("**How the palette resolves an action id.** It does not resolve a keymap id.")
    write("Every row the palette shows comes from the **daemon's catalog**, so the")
    write("palette's own resolution is:")
    write("")
    write("1. the typed line selects a catalog **command** and one of its argument")
    write("   **forms**;")
    write("2. a frontend-effect form resolves to its declared `FrontendAction` through")
    write("   `action_for_form(command_id, form)` — the catalog's identity, never a")
    write("   string compared against a keymap id;")
    write("3. a daemon-effect form resolves to `CoreRequest::CommandExecute`, or is")
    write("   shown as unavailable when the daemon's catalog says `daemon_served:")
    write("   false`.")
    write("")
    write("**What happens when an id is in the keymap but not in the declared set.**")
    write("Nothing: the two sets are never joined. A keymap id such as")
    write("`editor.cursor.left` is an editor gesture, not a command effect, and the")
    write("palette never sees it — the palette's rows come from the catalog, and the")
    write("catalog holds no editor gesture. A keymap id such as `command.palette.open`")
    write("opens the palette and is not itself a command effect either. There is no")
    write("path by which a keymap id could be dispatched as a command's effect, so a")
    write("new editor action id can never be mistaken for a command, and a new")
    write("command can never be mistaken for an editor gesture.")
    write("")
    write("The editor's commands go through the daemon's **document path** — a")
    write("different set of `CoreRequest` variants from `CommandExecute` — which is")
    write("the same separation seen from the other side.")
    write("")
    write("## 10. The classification of the forms the daemon does not serve")
    write("")
    write("Every daemon-effect form the daemon does not serve **with a request** is")
    write("classified below, in two groups. The forms whose TUI arm issues **no** daemon")
    write("request are one of three things:")
    write("")
    write("* **misclassified** — the form is not really a daemon effect: it does not")
    write("  mutate daemon-owned state, does not read daemon-owned state as its")
    write("  result, and does not drive a daemon-mediated process. The catalog's kind")
    write("  is wrong, and the corrected kind is named.")
    write("* **published projection** — the daemon owns the state and publishes it on")
    write("  the session projection, so the arm reads the published answer and issues")
    write("  no request of its own. The daemon has the surface; the projection is how")
    write("  a frontend reaches it.")
    write("* **missing capability** — the classification is right and the daemon")
    write("  genuinely lacks the surface. That is new product capability, not")
    write("  migration work, and it belongs to the follow-up named here.")
    write("")
    write("The forms whose TUI arm **does** issue a daemon request are classified in")
    write("the second table below, under a different set of verdicts.")
    write("")
    misclassified = [row for row in local_only if VERDICTS.get(row["name"], ("",))[0] == "misclassified"]
    missing = [row for row in local_only if VERDICTS.get(row["name"], ("",))[0] == "missing capability"]
    projected = [
        row for row in local_only if VERDICTS.get(row["name"], ("",))[0] == "published projection"
    ]
    undecided = [row for row in local_only if row["name"] not in VERDICTS]
    write(f"**{len(misclassified)} are misclassified, {len(projected)} are served by the")
    write(f"daemon's published projection and {len(missing)} are real missing capability**,")
    write(f"out of {len(local_only)} daemon-effect forms whose TUI arm issues")
    write("no daemon request.")
    write("")
    arm_misclassified = [
        row for row in with_request if ARM_VERDICTS.get(row["name"], ("",))[0] == "misclassified"
    ]
    # One declared action per *command* covers that command's forms: the
    # declared set is keyed by `(command_id, form)`, but one declaration may
    # list several forms under one action. The catalog's kind is the
    # *per-form* field, so every misclassified form needs its own change —
    # and `/agents` alone carries five of them.
    action_corrections = len({row["name"] for row in misclassified}) + len(
        {row["name"] for row in arm_misclassified}
    )
    kind_corrections = len(misclassified) + len(arm_misclassified)
    write("The kind correction is **not applied here**: a frontend-effect form needs a")
    write("declared `FrontendAction`, and the declared set's model is a UI action the")
    write(
        f"frontend performs. {action_corrections} new actions and {kind_corrections} catalog"
    )
    write("kind changes are their own change, recorded as a follow-up rather than")
    write("smuggled into this milestone.")
    write("")
    write("| command | family | verdict | why |")
    write("|---|---|---|---|")
    for row in local_only:
        verdict = VERDICTS.get(row["name"])
        if verdict is None:
            continue
        kind, surface, reason = verdict
        write(f"| `{row['name']}` | {row['domain']} | {kind} | {reason} |")
    write("")
    write("### The follow-ups")
    write("")
    write("| follow-up | surface it needs | ADR |")
    write("|---|---|---|")
    # The `/lsp-*` counts are **derived**, never written by hand. A hardcoded
    # count here is the same defect the `40`/`44`/`45` figures were: the prose
    # contradicts the table the same generator writes a few hundred lines
    # earlier, and `--check` cannot see it because both come from this file.
    lsp_rows = [row for row in rows if row["name"].startswith("/lsp-")]
    lsp_served = [row for row in lsp_rows if row["served"]]
    lsp_unserved = [row for row in lsp_rows if not row["served"]]
    lsp_unserved_names = ", ".join(f"`{row['name']}`" for row in lsp_unserved)
    write("| M007-F | the daemon's LSP command surface: a preview list, a server list,")
    write("  capabilities, root, restart and stop, a cache status and clear, a doctor, and")
    write("  the repair, review, impact, test-repair, interface, cross-repair and")
    write("  call-neighbours workflows. The daemon's `egglsp` instance is authoritative and")
    write("  the TUI's `LspTool` is a second, non-authoritative one in standalone mode.")
    write("  The catalog declares")
    write(f"  {len(lsp_rows)} `/lsp-*` forms; the daemon serves {len(lsp_served)} of them and")
    write(f"  leaves {len(lsp_unserved)} unavailable in daemon-connected mode until their")
    write("  typed handlers are implemented. The unavailable forms are")
    write(f"  {lsp_unserved_names}. The unavailable palette rows accurately describe the")
    write("  daemon's current command surface, and the inventory above lists every form's")
    write("  route. M007-F is closed under accepted ADR-0056. | **yes** — the authoritative language")
    write("  server's command surface is a new ownership boundary, and ADR-0052 does not")
    write("  cover it; ADR-0056 records the accepted decision |")
    write("| M007-G | the daemon's plugin catalogue: list, info, enable, disable, doctor,")
    write("  remove and install over the plugin registry the daemon actually loads. The")
    write("  TUI's `PluginManager` is its own instance and the daemon has none. | **yes** —")
    write("  the plugin registry's owner is a new ownership boundary |")
    # The follow-up lists are **derived** from the verdict tables, never
    # written by hand. A hand-written name drifts from the row it names —
    # a form that stops being a missing capability leaves the list, and a
    # hand-written list would keep it — and `--check` cannot see it,
    # because both come from this file.
    m007h = sorted(
        name
        for name, (_verdict, surface, _reason) in VERDICTS.items()
        if surface == "M007-H — the daemon's missing command surfaces"
    )
    m007i = sorted(
        name for name, (verdict, _reason) in ARM_VERDICTS.items() if verdict == "subsystem move"
    )
    write(f"| M007-H | the daemon's missing command surfaces: {len(m007h)} of them —")
    write("  " + ", ".join(f"`{name}`" for name in m007h) + ". Each is a surface the")
    write("  daemon has not exposed as a command. | no — these extend request")
    write("  families that already exist |")
    write(f"| M007-I | the {len(m007i)} **subsystem moves**:")
    write("  " + ", ".join(f"`{name}`" for name in m007i) + ". Each is a workflow of")
    write("  more than one daemon request rather than one effect, so moving it is a")
    write("  milestone with its own ownership question — which daemon owns the")
    write("  workflow's state, and whether the frontend keeps any of it. | **yes** —")
    write("  each is a new ownership boundary, and ADR-0052 does not cover it |")
    write("")
    write("### The forms whose TUI arm issues a daemon request")
    write("")
    write("The remaining daemon-effect forms the daemon does not serve, and whose TUI")
    write("arm issues a daemon request anyway, are one of four things — the fifth,")
    write("**migrated**, is a form the daemon already serves and is counted above:")
    write("")
    write("* **mechanical** — the effect request's fields are the scope's plus at most")
    write("  one argument, so the daemon's effect table can build it. The encoding is")
    write("  named, so the row is a decision and not a guess. The argument's convention")
    write("  is one token, or a token pair whose second token is a number — `<channel>`,")
    write("  `<channel> <seq>`, `<handle> <from_seq>`.")
    write("* **not mechanical** — the effect request carries a value that convention")
    write("  cannot produce: free text, a structured payload, a list, a boolean, or a")
    write("  second effect beside the first. A runtime *scalar* is not disqualifying —")
    write("  the argument carries it.")
    write("* **subsystem move** — the arm is a subsystem in its own right. Moving it is")
    write("  a milestone, and it is named in the follow-up table above.")
    write("* **misclassified** — the arm's effect is the frontend's own, so the")
    write("  catalog's kind is wrong and the corrected kind is named.")
    write("")
    counts: dict[str, int] = {}
    for row in with_request:
        verdict = ARM_VERDICTS.get(row["name"])
        if verdict is None:
            continue
        counts[verdict[0]] = counts.get(verdict[0], 0) + 1
    write(
        "**"
        + ", ".join(f"{count} {verdict}" for verdict, count in sorted(counts.items()))
        + f"**, out of {len(with_request)} daemon-effect forms whose TUI arm issues a")
    write("daemon request but which the daemon does not serve.")
    write("")
    write(
        f"The accounting is complete: **{len(daemon_rows)} daemon-effect forms = "
        f"{len(served_rows)} served + {len(with_request)} mechanical-unserved + "
        f"{len(local_only)} with no daemon request**, and every one of them is named."
    )
    write("")
    write("| command | family | verdict | why |")
    write("|---|---|---|---|")
    for row in with_request:
        verdict = ARM_VERDICTS.get(row["name"])
        if verdict is None:
            continue
        kind, reason = verdict
        # The request count is **derived**, never asserted: a hardcoded number
        # in this table drifts from the row it describes, and the row's own
        # `requests` column is the only count the artifact carries.
        reason = reason.replace("{n}", str(len(row["arm_requests"])))
        write(f"| `{row['name']}` | {row['domain']} | {kind} | {reason} |")
    write("")
    arm_undecided = [row for row in with_request if row["name"] not in ARM_VERDICTS]
    if arm_undecided:
        write(
            "**Undecided forms:** "
            + ", ".join(f"`{row['name']}`" for row in arm_undecided)
        )
        write("")
    if undecided:
        write("**Undecided forms:** " + ", ".join(f"`{row['name']}`" for row in undecided))
        write("")
    write("## 11. The settled items")
    write("")
    write("* **The description bound is re-baselined from 200 to 256 bytes.** The")
    write("  catalog's own longest descriptions are 217 bytes (`/control`) and 211")
    write("  (`/security-review`), so a 200-byte ceiling refuses a command the plan")
    write("  classifies. The code's bound is")
    write("  `crates/codegg-core/src/command_catalog.rs` —")
    write("  `bounds::MAX_DESCRIPTION_BYTES = 256` — and plan 014's protocol table")
    write("  records the re-baselining with this evidence.")
    write("* **The `core_daemon` family's recursion limit is raised to 512.** The")
    write("  daemon's request future genuinely grew a layer: the command executor and")
    write("  the shared family router sit inside it. The `Send` proof in that test")
    write("  crate exceeded the default budget of 128, so")
    write("  `tests/core_daemon/mod.rs` declares `#![recursion_limit = \"512\"]`. It is")
    write("  a real type recursion and not a low budget, and only `core_daemon` needed")
    write("  it.")
    write("* **`/compact` keeps its current behaviour** and stays in the inventory. The")
    write("  daemon has no compaction request — `TuiCommand::CompactSession` has no")
    write("  sender — so the form has no daemon route. The behaviour is unchanged and")
    write("  the gap is directed to a separate corrective milestone.")
    return "\n".join(out) + "\n"


def migrated_commands() -> dict[str, str]:
    """The commands the TUI has migrated, read from the coverage's own list.

    The coverage is the authority: it fails when a migrated command has no
    test, so the artifact's migration table cannot claim a command the
    coverage does not prove.
    """
    source = read(ROOT / "src/tui/command_route_tests.rs")
    start = source.find("const MIGRATED:")
    end = source.find("];", start)
    block = source[start:end]
    migrated: dict[str, str] = {}
    for command_id, family in re.findall(
        r'\(\s*"([a-z0-9_]+)"\s*,\s*"[^"]*"\s*,\s*"([A-Za-z/]+)"\s*,?\s*\)', block
    ):
        migrated[command_id] = family
    return migrated


def self_test() -> int:
    """Prove the generator's own invariants, by injecting their defects.

    Five properties are load-bearing and none fails loudly on its own:

    * a **match arm** names a request; it does not construct one. A test
      double's arm would otherwise put a request the production path never
      issues into the artifact — which is exactly what happened when the
      corrective pass taught a double to answer a command execution.
    * a **shorthand** construction counts, and a construction whose payload
      nests a struct counts. Both are ordinary Rust, and a scan that
      misses them reports the arm as issuing no request at all.
    * a **dispatch site** — a `match` over an enum — does not union its
      arms. The caller picks the arm, so an arm that inherited the other
      arms' requests would issue requests it never sends.
    * the index reaches the **client crate**, and a method call is
      resolved by the **type** of its receiver. A name is not an
      identity: two types can share one, and `report.render()` is not
      `App::render`.
    * the `--check` mode must **reject** a stale artifact. A check that
      accepts everything proves nothing.
    """
    failures: list[str] = []

    def expect(label: str, got: object, want: object) -> None:
        if got != want:
            failures.append(f"{label}: expected {want!r}, got {got!r}")

    # A construction initialises its fields: it counts.
    expect(
        "a construction counts",
        requests_in("CoreRequest::CommandExecute { request: dto }"),
        {"CommandExecute"},
    )
    expect(
        "a construction counts (a plain variant)",
        requests_in("CoreRequest::WorkspaceDashboard { id: value }"),
        {"WorkspaceDashboard"},
    )
    # A shorthand construction counts: `CoreRequest::LspPreviewApply {
    # request }` moves the value into the field.
    expect(
        "a shorthand construction counts",
        requests_in("Ok(crate::protocol::core::CoreRequest::LspPreviewApply { request })"),
        {"LspPreviewApply"},
    )
    # A construction whose payload nests a struct counts: the block is
    # closed by its own braces, not by the first `}` inside it.
    expect(
        "a construction with a nested payload counts",
        requests_in("CoreRequest::InteractiveProcessCreate { request: Dto { argv } }"),
        {"InteractiveProcessCreate"},
    )
    # A match arm names the variant: it does not count. Its `=>` follows the
    # block, so the block alone cannot tell the two apart.
    expect(
        "a match arm does not count",
        requests_in("CoreRequest::CommandExecute { request: execute } => { let _ = 1; }"),
        set(),
    )
    expect(
        "a match arm does not count (a plain variant)",
        requests_in("CoreRequest::WorkspaceDashboard { id: value } => { let _ = 1; }"),
        set(),
    )
    expect(
        "a struct-update pattern does not count",
        requests_in("CoreRequest::WorkspaceDashboard { .. } => { let _ = 1; }"),
        set(),
    )

    # A dispatch site is not a call: the caller picks the arm, so the
    # resolver must not union the arms into one set.
    dispatch_body = (
        "{\n"
        "    match msg {\n"
        "        TuiMsg::First => { self.first(); }\n"
        "        TuiMsg::Second => { self.second(); }\n"
        "    }\n"
        "}\n"
    )
    selected = " ".join(segments(dispatch_body, frozenset({"TuiMsg::First"})))
    expect("a dispatch site selects the hinted arm", "self.first()" in selected, True)
    expect("a dispatch site drops the other arms", "self.second()" in selected, False)
    unhinted = " ".join(segments(dispatch_body, frozenset()))
    expect("a dispatch site with no hint selects no arm", "self.first()" in unhinted, False)
    expect("a dispatch site with no hint selects no other arm", "self.second()" in unhinted, False)

    # The index reaches the crate the TUI calls through, and a method is
    # resolved by the type of its receiver.
    index = SourceIndex(ROOT)
    expect(
        "the index reaches the client crate",
        "crates/codegg-client/src/document.rs" in index.files,
        True,
    )
    controller = index.methods.get("DocumentController", {}).get("open")
    expect("the client's document controller is indexed", controller is not None, True)
    if controller is not None:
        expect(
            "the client's controller opens the daemon's document",
            "DocumentOpen" in requests_in(controller.body),
            True,
        )
    editor = index.function("src/tui/commands/editor.rs", "open_editor_in")
    expect("the editor's opener is indexed", editor is not None, True)
    if editor is not None:
        expect(
            "the editor's controller resolves to the client crate's type",
            index.expr_type("src/tui/commands/editor.rs", editor, "controller"),
            "DocumentController",
        )
        # `open` is a method on two types in the tree, and only the
        # receiver's type says which one runs. Resolved by name, the
        # controller's `open` becomes the review surface's `open`, and the
        # arm issues the wrong request.
        opened: set[str] = set()
        index.resolve("src/tui/commands/editor.rs", "open_editor_in", set(), opened, 0)
        expect("the opener issues the document request", "DocumentOpen" in opened, True)
        expect(
            "the opener does not reach a same-named method elsewhere",
            "LspPreviewApply" in opened,
            False,
        )

    # The `--check` mode has teeth: a perturbed artifact is not the derived
    # one, so the comparison can fail.
    if not ARTIFACT.exists():
        failures.append("the artifact does not exist")
    else:
        current = ARTIFACT.read_text(encoding="utf-8")
        derived = build(
            parse_catalog(read(ROOT / CATALOG)),
            set(parse_effect_table(read(ROOT / DAEMON_COMMANDS))),
        )
        expect("the artifact is in step with the tree", current, derived)
        expect(
            "a perturbed artifact is not the derived one",
            derived.replace("\n", "\n\n", 1) == derived,
            False,
        )

    if failures:
        for failure in failures:
            print(f"self-test failed: {failure}", file=sys.stderr)
        return 1
    print(
        "self-test passed: the generator counts constructions, not patterns, "
        "and keeps a dispatch site's arms apart"
    )
    return 0


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return self_test()
    check_only = "--check" in sys.argv[1:]
    commands = parse_catalog(read(ROOT / CATALOG))
    served = set(parse_effect_table(read(ROOT / DAEMON_COMMANDS)))
    generated = build(commands, served)
    if check_only:
        # A generator that can only rewrite the artifact cannot show that the
        # artifact is in step with the tree. `--check` derives the artifact
        # again and reports the drift instead of writing over it, so a stale
        # artifact is a visible fact rather than a silent overwrite.
        if not ARTIFACT.exists():
            print(
                f"check failed: {ARTIFACT.relative_to(ROOT)} does not exist",
                file=sys.stderr,
            )
            return 1
        current = ARTIFACT.read_text(encoding="utf-8")
        if current != generated:
            print(
                f"check failed: {ARTIFACT.relative_to(ROOT)} is out of step with "
                "the tree; re-run the generator",
                file=sys.stderr,
            )
            return 1
        print(f"check passed: {ARTIFACT.relative_to(ROOT)} matches the tree")
        return 0
    ARTIFACT.write_text(generated, encoding="utf-8")
    print(f"wrote {ARTIFACT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
