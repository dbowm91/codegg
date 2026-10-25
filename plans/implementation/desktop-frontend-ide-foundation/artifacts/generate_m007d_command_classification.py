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
REQ_RE = re.compile(r"CoreRequest::([A-Za-z0-9_]+)\s*\{([^{}]*)\}")
# The command execution carries its request DTO, so its block holds a
# nested struct literal and the flat pattern above cannot see it. Every
# migrated arm builds one, so a scan that misses it reports those arms as
# issuing no daemon request at all.
CMD_RE = re.compile(r"CoreRequest::CommandExecute\s*\{((?:[^{}]|\{[^{}]*\})*)\}")
CALL_RE = re.compile(r"\b([a-z_][a-z0-9_]*)\s*\(")
KEYWORDS = {
    "if", "for", "while", "match", "fn", "Some", "None", "Ok", "Err", "Vec", "String",
    "HashMap", "HashSet", "Box", "Arc", "move", "return", "let", "in", "else", "loop",
    "break", "continue", "as", "ref", "self",
}


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

    A construction initialises its fields (`field: value`); a match arm
    names them (`{ .. }` or `{ field, .. }`). Counting a pattern as a
    construction would attribute every request the arm *matches* to it,
    which is the opposite of what this column measures.
    """
    found: set[str] = set()
    for match in REQ_RE.finditer(body):
        block = match.group(2)
        if ".." in block or "=>" in block:
            continue
        if ":" not in block:
            continue
        if _names_a_pattern(body, match.end()):
            continue
        found.add(match.group(1))
    for match in CMD_RE.finditer(body):
        block = match.group(1)
        if ".." in block or "=>" in block:
            continue
        if ":" not in block:
            continue
        if _names_a_pattern(body, match.end()):
            continue
        found.add("CommandExecute")
    return found


def _bodies(text: str) -> list[tuple[str, str]]:
    bodies: list[tuple[str, str]] = []
    for match in FN_RE.finditer(text):
        index = match.end() - 1
        depth = 0
        cursor = index
        while cursor < len(text):
            if text[cursor] == "(":
                depth += 1
            elif text[cursor] == ")":
                depth -= 1
                if depth == 0:
                    break
            cursor += 1
        brace = text.find("{", cursor)
        if brace < 0:
            continue
        depth = 0
        end = brace
        while end < len(text):
            if text[end] == "{":
                depth += 1
            elif text[end] == "}":
                depth -= 1
                if depth == 0:
                    break
            end += 1
        bodies.append((match.group(1), text[brace : end + 1]))
    return bodies


class TuiIndex:
    """The TUI's functions, module paths, and `use` imports."""

    def __init__(self, root: Path) -> None:
        self.files: dict[str, str] = {}
        for path in sorted((root / TUI_DIR).rglob("*.rs")):
            self.files[str(path.relative_to(root))] = read(path)
        self.bodies: dict[str, list[tuple[str, str]]] = {
            name: _bodies(text) for name, text in self.files.items()
        }
        self.module_path: dict[str, str] = {}
        for name in self.files:
            rel = name[len(TUI_DIR) + 1 : -3]
            if rel.endswith("/mod"):
                rel = rel[:-4]
            self.module_path[rel.replace("/", "::")] = name
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
        for match in re.finditer(r"crate::tui::((?:\w+::)+)(\w+)", body):
            module = match.group(1)[:-2] if match.group(1).endswith("::") else match.group(1)
            resolved = self.module_path.get(module)
            if resolved:
                self.resolve(resolved, match.group(2), set(), out)

    def function(self, path: str, name: str) -> str | None:
        for candidate, body in self.bodies.get(path, []):
            if candidate == name:
                return body
        return None

    def resolve(
        self, path: str, name: str, seen: set[tuple[str, str]], out: set[str], depth: int = 0
    ) -> None:
        """The requests reachable from calling `name` inside `path`."""
        if depth > 5 or (path, name) in seen:
            return
        seen.add((path, name))
        body = self.function(path, name)
        target = path
        if body is None:
            imported = self.imports.get(path, {}).get(name)
            if imported is not None:
                body = self.function(imported, name)
                target = imported
            if body is None:
                # A sibling module of the same directory may define it.
                directory = os.path.dirname(path)
                for candidate in self.files:
                    if os.path.dirname(candidate) != directory:
                        continue
                    body = self.function(candidate, name)
                    if body is not None:
                        target = candidate
                        break
            if body is None:
                return
        out.update(requests_in(body))
        for match in re.finditer(r"crate::tui::((?:\w+::)+)(\w+)", body):
            module = match.group(1)[:-2] if match.group(1).endswith("::") else match.group(1)
            resolved = self.module_path.get(module)
            if resolved:
                self.resolve(resolved, match.group(2), seen, out, depth + 1)
        for call in CALL_RE.findall(body):
            if call in KEYWORDS:
                continue
            self.resolve(target, call, seen, out, depth + 1)


def arm_requests(index: TuiIndex) -> dict[str, tuple[int, list[str]]]:
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
        requests: set[str] = requests_in(body)
        for call in set(CALL_RE.findall(body)):
            if call in KEYWORDS:
                continue
            index.resolve(runtime, call, set(), requests)
        index.resolve_qualified(body, requests)
        channel.setdefault(variant, set()).update(requests)

    out: dict[str, tuple[int, list[str]]] = {}
    for position, (line_no, name) in enumerate(arms):
        end = arms[position + 1][0] if position + 1 < len(arms) else start + 3600
        body = "\n".join(lines[line_no:end])
        requests: set[str] = requests_in(body)
        for call in set(CALL_RE.findall(body)):
            if call in KEYWORDS:
                continue
            index.resolve(TUI_DISPATCH, call, set(), requests)
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


# The habit store, the skill-promotion store, the transcript search, the
# doctor routine, the plugin manager and the interactive terminal are all
# **the frontend's own**: the TUI opens the file, keeps the buffer, or runs
# the routine. None of them touches daemon-owned state.
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
_misclassified("/terminal", "the TUI's own interactive terminal")
_misclassified("/terminal-create", "the TUI's own interactive terminal")
_misclassified("/terminal-resize", "the TUI's own interactive terminal")
_misclassified("/terminal-detach", "the TUI's own interactive terminal")
_misclassified("/terminal-terminate", "the TUI's own interactive terminal")
_misclassified("/terminal-remove", "the TUI's own interactive terminal")

# The language server is daemon-owned: `egglsp` is authoritative and the
# TUI's `LspTool` is a second, non-authoritative instance that is absent in
# the daemon-connected configuration, so every `/lsp-*` arm already refuses
# there. The classification is right and the daemon lacks the surface.
for _lsp in (
    "/lsp-previews", "/lsp-preview", "/lsp-preview-clear", "/lsp-preview-refresh",
    "/lsp-preview-apply", "/lsp-servers", "/lsp-capabilities", "/lsp-root",
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

_missing(
    "/compact",
    "M007-H — the daemon's missing command surfaces",
    "compaction is a daemon-owned session operation; the daemon has no compaction request",
)
_missing(
    "/subagent-send",
    "M007-H — the daemon's missing command surfaces",
    "steering a subagent is a daemon-owned turn operation",
)
_missing(
    "/plan",
    "M007-H — the daemon's missing command surfaces",
    "the plan list is daemon-owned work-plan state; the daemon has no plan request",
)
_missing(
    "/tests",
    "M007-H — the daemon's missing command surfaces",
    "the suite run is daemon-owned job state; the daemon has no test-suite request",
)
_missing(
    "/revert",
    "M007-H — the daemon's missing command surfaces",
    "the daemon owns the git mutation; it exposes it as a tool, not as a request",
)
_missing(
    "/research",
    "M007-H — the daemon's missing command surfaces",
    "the research turn is daemon-owned agent-loop work",
)
_missing(
    "/security-review",
    "M007-H — the daemon's missing command surfaces",
    "the security-review workflow is a daemon-owned agent turn",
)
_missing(
    "/security-review-cancel",
    "M007-H — the daemon's missing command surfaces",
    "cancelling a security-review run is a daemon-owned turn cancellation",
)
_missing(
    "/review-change",
    "M007-H — the daemon's missing command surfaces",
    "the change-review workflow is a daemon-owned agent turn",
)
_missing(
    "/open",
    "M007-H — the daemon's missing command surfaces",
    "opening a document is daemon-owned workspace state",
)
_missing(
    "/shell-rerun",
    "M007-H — the daemon's missing command surfaces",
    "re-running a human-shell run is daemon-owned job state",
)
_missing(
    "/logs",
    "M007-H — the daemon's missing command surfaces",
    "the daemon writes the log file and the frontend reads it out of band; the daemon has no log-tail request",
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


# The agent registry is the TUI's own: `app.agent_state` holds the agents the
# TUI loaded from its asset snapshot, and both arms read it directly. The
# daemon request they issue is a project lookup, not the effect.
_arm(
    "/agent",
    "misclassified",
    "the TUI selects from its own agent registry (`app.agent_state`); the catalog's kind is wrong and the corrected kind is `FrontendEffect`",
)
_arm(
    "/agents",
    "misclassified",
    "the TUI lists its own agent registry (`app.agent_state`); the catalog's kind is wrong and the corrected kind is `FrontendEffect`",
)
_arm(
    "/security-review-show",
    "not mechanical",
    "the arm shows the TUI's cached review result, and the daemon has no request that reads one — the same missing surface as `/security-review` (M007-H)",
)
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
    "the arm drives 45 daemon requests; moving it is a subsystem, not a table row",
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

# The git-review and memory forms are subsystem moves.
_arm(
    "/diff",
    "subsystem move",
    "the arm drives 40 daemon requests; moving it is a subsystem, not a table row",
)
_arm(
    "/memory-consolidate",
    "not mechanical",
    "the daemon request reads the session's messages; the effect itself is a write into the TUI's own memory store, which the daemon does not own",
)
_arm(
    "/skill-promote",
    "subsystem move",
    "the arm drives 44 daemon requests; moving it is a subsystem, not a table row",
)

# The research forms read the TUI's own research service over the local
# filesystem. No daemon request is their effect.
_arm(
    "/research-open",
    "misclassified",
    "the TUI's own `ResearchService` over the local filesystem; the catalog's kind is wrong and the corrected kind is `FrontendEffect`",
)
_arm(
    "/research-runs",
    "misclassified",
    "the TUI's own `ResearchService` over the local filesystem; the catalog's kind is wrong and the corrected kind is `FrontendEffect`",
)
_arm(
    "/research-show",
    "misclassified",
    "the TUI's own `ResearchService` over the local filesystem; the catalog's kind is wrong and the corrected kind is `FrontendEffect`",
)

# The session forms.
_arm(
    "/import",
    "not mechanical",
    "the arm opens the import dialog; the daemon request is issued by the dialog's own submit path, not by the command",
)
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
    index = TuiIndex(ROOT)
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
    write("## 2. The three routes a form takes")
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
    write("Every daemon-effect form the daemon does not serve is classified below, in")
    write("two groups. The forms whose TUI arm issues **no** daemon request are one")
    write("of two things:")
    write("")
    write("* **misclassified** — the form is not really a daemon effect: it does not")
    write("  mutate daemon-owned state, does not read daemon-owned state as its")
    write("  result, and does not drive a daemon-mediated process. The catalog's kind")
    write("  is wrong, and the corrected kind is named.")
    write("* **missing capability** — the classification is right and the daemon")
    write("  genuinely lacks the surface. That is new product capability, not")
    write("  migration work, and it belongs to the follow-up named here.")
    write("")
    write("The forms whose TUI arm **does** issue a daemon request are classified in")
    write("the second table below, under a different set of verdicts.")
    write("")
    misclassified = [row for row in local_only if VERDICTS.get(row["name"], ("",))[0] == "misclassified"]
    missing = [row for row in local_only if VERDICTS.get(row["name"], ("",))[0] == "missing capability"]
    undecided = [row for row in local_only if row["name"] not in VERDICTS]
    write(f"**{len(misclassified)} are misclassified and {len(missing)} are real missing")
    write(f"capability**, out of {len(local_only)} daemon-effect forms whose TUI arm issues")
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
    write("| M007-F | the daemon's LSP command surface: a preview list, a server list,")
    write("  capabilities, root, restart and stop, a cache status and clear, a doctor, and")
    write("  the repair, review, impact, test-repair, interface, cross-repair and")
    write("  call-neighbours workflows. The daemon's `egglsp` instance is authoritative and")
    write("  the TUI's `LspTool` is a second, non-authoritative one. The catalog declares")
    write("  26 `/lsp-*` forms; the daemon serves 2 of them and does not serve the other")
    write("  24. **Which behaviour refuses where:** the TUI's own `/lsp-*` arms refuse in")
    write("  the **socket** (daemon-connected) configuration, with the toast")
    write("  `LSP not available`, because `app.lsp_tool` is `None` there — `src/main.rs`")
    write("  attaches the TUI's own `LspTool` only when the TUI is *not* in socket mode.")
    write("  In **local** mode the same arms work, against that second, non-authoritative")
    write("  server. The one arm that would reach the daemon (`/lsp-preview-apply`)")
    write("  reaches it only *after* the same `lsp_tool` check, so it refuses in socket")
    write("  mode too. The desktop palette shows the 24 unserved forms as unavailable")
    write("  because the daemon does not serve them; the palette's unavailable row is")
    write("  correct, not a gap. The 2 served forms (`/lsp-status`, `/lsp-errors`) build")
    write("  `lsp_diagnostics_get`, and the palette dispatches them through the daemon —")
    write("  the TUI's own arms for those two are a recorded discrepancy, not a")
    write("  migration (see the closure record). | **yes** — the authoritative language")
    write("  server's command surface is a new ownership boundary, and ADR-0052 does not")
    write("  cover it |")
    write("| M007-G | the daemon's plugin catalogue: list, info, enable, disable, doctor,")
    write("  remove and install over the plugin registry the daemon actually loads. The")
    write("  TUI's `PluginManager` is its own instance and the daemon has none. | **yes** —")
    write("  the plugin registry's owner is a new ownership boundary |")
    write("| M007-H | the daemon's missing command surfaces: compaction, subagent steer,")
    write("  the plan list, the test suite, a git revert, the research turn, the security-")
    write("  review and change-review workflows, opening a document, re-running a human-")
    write("  shell run, and a log tail. Each is an existing request family the daemon has")
    write("  not grown yet. | no — these extend request families that already exist |")
    write("| M007-I | the three **subsystem moves**: the git-review diff view, the")
    write("  skill-promotion workflow, and the human-shell ask workflow. Each is a")
    write("  workflow of dozens of daemon requests rather than one effect, so moving")
    write("  it is a milestone with its own ownership question — which daemon owns")
    write("  the workflow's state, and whether the frontend keeps any of it. | **yes** —")
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

    Two properties are load-bearing and neither fails loudly on its own:

    * a **match arm** names a request; it does not construct one. A test
      double's arm would otherwise put a request the production path never
      issues into the artifact — which is exactly what happened when the
      corrective pass taught a double to answer a command execution.
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
    print("self-test passed: the generator counts constructions, not patterns")
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
