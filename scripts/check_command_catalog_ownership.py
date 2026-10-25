#!/usr/bin/env python3
"""Guard: the daemon owns the command catalog, and no frontend holds a second.

ADR-0052 (accepted 2026-10-24) moved command ownership into the daemon. The
daemon's catalog (`crates/codegg-core/src/command_catalog.rs`) is the single
definition of what commands exist: their identity, display name, aliases,
description, domain, kind, required capability and argument forms. Every
frontend reads that list; none of them compiles its own.

This guard proves four things, statically:

1. **One set.** The daemon's catalog and the TUI's built-in command surface
   are the same set of commands. Every `BuiltinSlashAction` the TUI dispatches
   has exactly one catalog identity, every catalog identity has at least one
   TUI row, the TUI's rows for an identity are exactly the catalog's name and
   aliases, and the canonical row carries the catalog's description.

2. **The daemon executes only through the catalog.** The only `CoreRequest`
   variants that carry a command are `CommandCatalog` and `CommandExecute`,
   and the daemon's command-effect table names only forms the catalog
   classifies as daemon effects. A frontend-effect form can never be reached
   through the executor.

3. **No frontend holds a second catalog.** The desktop renderer holds no
   slash-prefixed command spelling, no command list and no command-kind
   table of its own, and the TUI holds no kind table of its own either — the
   kind lives in the daemon's catalog. The rule forbids the slash-prefixed
   spellings only: a bare alias such as `resume` is indistinguishable from
   ordinary UI prose, so forbidding it would turn the rule into a
   false-positive machine rather than a boundary.

Why the guard is shaped this way
--------------------------------
The classification lives in **one** file, so the guard's job is to prove the
other copies agree with it rather than to re-derive the classification. A
guard that carried its own copy of the command list would be a second
catalog, which is the defect it exists to catch.

The guard self-tests. `--self-test` builds synthetic trees with one defect
each and proves the detector rejects every one of them. A guard that cannot
be shown to fail on an injected defect is not accepted as evidence.

Usage:
    python3 scripts/check_command_catalog_ownership.py
    python3 scripts/check_command_catalog_ownership.py --self-test

Exit codes: 0 clean, 1 violations found, 2 self-test failure.
"""

from __future__ import annotations

import re
import shutil
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_desktop_editor_authority as editor_authority  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parent.parent

CATALOG_PATH = "crates/codegg-core/src/command_catalog.rs"
TUI_COMMAND_PATH = "src/tui/command.rs"
PROTOCOL_PATH = "crates/codegg-protocol/src/core.rs"
DAEMON_COMMANDS_PATH = "src/core/daemon_commands.rs"
DESKTOP_RENDERER_DIR = "apps/desktop/src"

# The `CoreRequest` variants that are allowed to carry a command. Anything
# else that starts with `Command` would be a second, unguarded way to run a
# command.
ALLOWED_COMMAND_VARIANTS = {"CommandCatalog", "CommandExecute"}


# ── Rust source helpers ──────────────────────────────────────────────


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def parse_rust_string(text: str, start: int) -> tuple[str, int] | None:
    """Read one Rust string literal starting at the opening quote.

    Returns the decoded value and the index just past the closing quote, or
    `None` when `start` does not point at a string literal. Rust's
    `\\<newline>` continuation swallows the newline and the whitespace that
    follows it, so the value is the concatenation of the pieces.
    """
    if start >= len(text) or text[start] != '"':
        return None
    i = start + 1
    out: list[str] = []
    while i < len(text):
        ch = text[i]
        if ch == "\\":
            nxt = text[i + 1] if i + 1 < len(text) else ""
            if nxt == "n":
                out.append("\n")
                i += 2
                continue
            if nxt == "t":
                out.append("\t")
                i += 2
                continue
            if nxt == "r":
                out.append("\r")
                i += 2
                continue
            if nxt == '"':
                out.append('"')
                i += 2
                continue
            if nxt == "\\":
                out.append("\\")
                i += 2
                continue
            if nxt == "'":
                out.append("'")
                i += 2
                continue
            if nxt == "\n":
                # Line continuation: skip the newline and the leading
                # whitespace of the next line.
                i += 2
                while i < len(text) and text[i] in " \t":
                    i += 1
                continue
            out.append(nxt)
            i += 2
            continue
        if ch == '"':
            return "".join(out), i + 1
        out.append(ch)
        i += 1
    return None


def first_string_after(text: str, marker: str) -> str | None:
    """The first string literal that appears after `marker`."""
    idx = text.find(marker)
    if idx < 0:
        return None
    quote = text.find('"', idx + len(marker))
    if quote < 0:
        return None
    parsed = parse_rust_string(text, quote)
    return None if parsed is None else parsed[0]


def string_literals(text: str) -> list[str]:
    """Every **Rust** string literal in `text`.

    This extractor reads double-quoted Rust strings, and it is written for
    the Rust files this guard reads. The renderer is TypeScript and JavaScript,
    which have three quoting styles; see [`quoted_literal_hits`].
    """
    out: list[str] = []
    i = 0
    while i < len(text):
        if text[i] == '"':
            parsed = parse_rust_string(text, i)
            if parsed is None:
                break
            value, end = parsed
            out.append(value)
            i = end
            continue
        i += 1
    return out


def quoted_literal_hits(text: str, name: str) -> list[int]:
    """The offsets where `name` appears as a complete quoted literal.

    A renderer command name is a defect when it is a **literal** — a
    complete quoted string — not when it appears in prose, so the whole
    literal must be the name.

    All three quoting styles count, because the renderer writes
    single-quoted strings: a scan that reads only double quotes sees a
    sixth of the renderer's literals, and a command list written in the
    renderer's own style would pass.
    """
    hits: list[int] = []
    for match in re.finditer(re.escape(name), text):
        start, end = match.start(), match.end()
        if start == 0 or end >= len(text):
            continue
        before = text[start - 1]
        if before in "'\"`" and text[end] == before:
            hits.append(start)
    return hits


# ── The catalog ──────────────────────────────────────────────────────


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
    kind: str
    capability: str | None
    forms: list[CatalogForm] = field(default_factory=list)


DESCRIPTOR_RE = re.compile(r"CommandDescriptor\s*\{")
FORM_RE = re.compile(r"CommandForm\s*\{")


def parse_catalog(text: str) -> list[CatalogCommand]:
    """Parse the catalog's command entries."""
    commands: list[CatalogCommand] = []
    for match in DESCRIPTOR_RE.finditer(text):
        # Bound the block by brace counting so a nested struct does not leak.
        depth = 0
        i = text.index("{", match.start())
        end = i
        while end < len(text):
            if text[end] == "{":
                depth += 1
            elif text[end] == "}":
                depth -= 1
                if depth == 0:
                    break
            end += 1
        block = text[i : end + 1]

        command_id = first_string_after(block, "command_id:")
        name = first_string_after(block, "name:")
        description = first_string_after(block, "description:")
        if command_id is None or name is None:
            continue
        aliases: list[str] = []
        alias_idx = block.find("aliases:")
        if alias_idx >= 0:
            bracket = block.find("[", alias_idx)
            if bracket >= 0:
                close = block.find("]", bracket)
                aliases = [
                    literal
                    for literal in string_literals(block[bracket:close])
                ]
        kind = ""
        kind_match = re.search(r"kind:\s*CommandKind::(\w+)", block)
        if kind_match:
            kind = kind_match.group(1)
        capability = None
        cap_match = re.search(
            r"capability:\s*(Some\(CommandCapability::(\w+)\)|None)", block
        )
        if cap_match and cap_match.group(2):
            capability = cap_match.group(2)
        forms: list[CatalogForm] = []
        forms_idx = block.find("forms:")
        if forms_idx >= 0:
            forms_block = block[forms_idx:]
            for form_match in FORM_RE.finditer(forms_block):
                form_text = forms_block[
                    form_match.start() : form_match.start() + 400
                ]
                form_value = first_string_after(form_text, "form:")
                form_kind_match = re.search(
                    r"kind:\s*CommandKind::(\w+)", form_text
                )
                if form_value is None or form_kind_match is None:
                    continue
                forms.append(
                    CatalogForm(
                        form=form_value, kind=form_kind_match.group(1)
                    )
                )
        commands.append(
            CatalogCommand(
                command_id=command_id,
                name=name,
                aliases=aliases,
                description=description or "",
                kind=kind,
                capability=capability,
                forms=forms,
            )
        )
    return commands


# ── The TUI's command surface ────────────────────────────────────────


@dataclass
class TuiCommand:
    name: str
    aliases: list[str]
    description: str
    action: str


TUI_COMMAND_RE = re.compile(
    r"Command::new\(\s*\"([^\"]+)\"\s*,\s*CommandCategory::\w+\s*,\s*"
    r"CommandAction::Builtin\(BuiltinSlashAction::(\w+)\)"
)


def parse_tui_commands(text: str) -> list[TuiCommand]:
    """Parse the TUI's built-in command rows.

    Only `built_in_commands()` is scanned: the registry's *dynamic* sources
    (`append_dynamic_commands`) are local to the TUI and out of the
    catalog, and the file's test bodies are not the command surface.
    """
    idx = text.find("fn built_in_commands()")
    if idx < 0:
        return []
    start = text.index("{", idx)
    depth = 0
    end = start
    while end < len(text):
        if text[end] == "{":
            depth += 1
        elif text[end] == "}":
            depth -= 1
            if depth == 0:
                break
        end += 1
    text = text[start : end + 1]
    commands: list[TuiCommand] = []
    for match in TUI_COMMAND_RE.finditer(text):
        name = match.group(1)
        action = match.group(2)
        # The row runs to the start of the next `Command::new(`: chained
        # builder calls and the comments between them are part of the row.
        next_row = text.find("Command::new(", match.end())
        tail = text[match.end() : next_row if next_row > 0 else len(text)]
        aliases: list[str] = []
        alias_idx = tail.find(".with_aliases(")
        if alias_idx >= 0:
            bracket = tail.find("[", alias_idx)
            close = tail.find("]", bracket)
            if bracket >= 0 and close > bracket:
                aliases = string_literals(tail[bracket:close])
        description = ""
        desc_idx = tail.find(".with_description(")
        if desc_idx >= 0:
            quote = tail.find('"', desc_idx)
            if quote >= 0:
                parsed = parse_rust_string(tail, quote)
                if parsed is not None:
                    description = parsed[0]
        commands.append(
            TuiCommand(
                name=name,
                aliases=aliases,
                description=description,
                action=action,
            )
        )
    return commands


def snake_case(name: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


# ── The daemon's effect table ────────────────────────────────────────

EFFECT_KEY_RE = re.compile(r'\(\s*"([a-z0-9_]+)"\s*,\s*"([^"]*)"\s*\)')


def parse_effect_table(text: str) -> list[tuple[str, str]]:
    """The `(command_id, form)` keys the daemon's effect table names.

    The table is the match in `build_inner_request`: every arm is one
    `(command_id, form)` key, and an arm may name two keys when two
    commands share one effect.

    The parser reads the arms rather than a separate list, so a key that
    exists only in a list is not a key the daemon serves. The table's own
    unit test (`the_effect_table_is_closed` in
    `src/core/daemon_commands.rs`) holds the two views of the table
    together.
    """
    idx = text.find("pub fn build_inner_request(")
    if idx < 0:
        return []
    body = text[idx:]
    end = body.find("\npub fn ", 1)
    if end > 0:
        body = body[:end]
    return [(m.group(1), m.group(2)) for m in EFFECT_KEY_RE.finditer(body)]


# ── The protocol ─────────────────────────────────────────────────────


def parse_core_request_variants(text: str) -> list[str]:
    """The variant names of the `CoreRequest` enum."""
    idx = text.find("pub enum CoreRequest {")
    if idx < 0:
        return []
    body = text[idx:]
    depth = 0
    i = body.index("{")
    end = i
    while end < len(body):
        if body[end] == "{":
            depth += 1
        elif body[end] == "}":
            depth -= 1
            if depth == 0:
                break
        end += 1
    body = body[: end + 1]
    variants: list[str] = []
    for line in body.splitlines():
        match = re.match(r"\s{4}([A-Z][A-Za-z0-9]*)\s*(\{|,)", line)
        if match:
            variants.append(match.group(1))
    return variants


# ── The checks ───────────────────────────────────────────────────────


def check_catalog_shape(commands: list[CatalogCommand]) -> list[str]:
    findings: list[str] = []
    seen_ids: set[str] = set()
    seen_names: set[str] = set()
    for command in commands:
        if command.command_id in seen_ids:
            findings.append(
                f"{CATALOG_PATH}: duplicate command identity {command.command_id!r}"
            )
        seen_ids.add(command.command_id)
        if command.name in seen_names:
            findings.append(
                f"{CATALOG_PATH}: duplicate command name {command.name!r}"
            )
        seen_names.add(command.name)
        if not command.forms:
            findings.append(
                f"{CATALOG_PATH}: command {command.command_id!r} declares no form"
            )
        # A command that has a daemon-effect form names the capability that
        # form needs. A command whose forms are *all* frontend effects names
        # none: there is nothing for the daemon to authorize.
        has_daemon_effect = any(
            form.kind == "DaemonEffect" for form in command.forms
        )
        if not has_daemon_effect and command.capability is not None:
            findings.append(
                f"{CATALOG_PATH}: command {command.command_id!r} has no "
                "daemon-effect form but names a daemon capability"
            )
        if has_daemon_effect and command.capability is None:
            findings.append(
                f"{CATALOG_PATH}: command {command.command_id!r} has a "
                "daemon-effect form but names no daemon capability"
            )
    return findings


def check_same_set(
    commands: list[CatalogCommand], tui: list[TuiCommand]
) -> list[str]:
    findings: list[str] = []
    by_id = {command.command_id: command for command in commands}
    tui_by_id: dict[str, list[TuiCommand]] = {}
    for row in tui:
        tui_by_id.setdefault(snake_case(row.action), []).append(row)

    for command_id, rows in sorted(tui_by_id.items()):
        command = by_id.get(command_id)
        if command is None:
            findings.append(
                f"{TUI_COMMAND_PATH}: the TUI dispatches {command_id!r}, which the "
                "daemon's catalog does not define"
            )
            continue
        # A spelling is a name *or* an alias: the TUI may present a command
        # as its own row (a second palette entry with its own wording) or
        # as an alias of the canonical row. Either way the set of
        # spellings must be the catalog's.
        expected = {command.name, *command.aliases}
        actual: set[str] = set()
        for row in rows:
            actual.add(row.name)
            actual.update(row.aliases)
        if actual != expected:
            missing = sorted(expected - actual)
            extra = sorted(actual - expected)
            if missing:
                findings.append(
                    f"{TUI_COMMAND_PATH}: command {command_id!r} is missing the "
                    f"catalog spelling(s) {missing}"
                )
            if extra:
                findings.append(
                    f"{TUI_COMMAND_PATH}: command {command_id!r} has the spelling(s) "
                    f"{extra}, which the catalog does not define"
                )
        canonical = [row for row in rows if row.name == command.name]
        if not canonical:
            findings.append(
                f"{TUI_COMMAND_PATH}: command {command_id!r} has no row for the "
                f"catalog's canonical name {command.name!r}"
            )
        else:
            # The canonical row must carry the catalog's description. A row
            # that presents the same command under another spelling may
            # word it differently — that is presentation, not drift — so
            # only its emptiness is checked.
            for row in canonical:
                if row.description != command.description:
                    findings.append(
                        f"{TUI_COMMAND_PATH}: command {command_id!r} describes itself "
                        f"as {row.description!r}, the catalog as {command.description!r}"
                    )
            for row in rows:
                if row.name != command.name and not row.description:
                    findings.append(
                        f"{TUI_COMMAND_PATH}: command {command_id!r} presents the "
                        f"spelling {row.name!r} with no description"
                    )

    for command in commands:
        if command.command_id not in tui_by_id:
            findings.append(
                f"{CATALOG_PATH}: the catalog defines {command.command_id!r}, which the "
                "TUI never dispatches"
            )
    return findings


def check_effect_table(
    commands: list[CatalogCommand], effect_keys: list[tuple[str, str]]
) -> list[str]:
    findings: list[str] = []
    by_id = {command.command_id: command for command in commands}
    for command_id, form in effect_keys:
        command = by_id.get(command_id)
        if command is None:
            findings.append(
                f"{DAEMON_COMMANDS_PATH}: the effect table names {command_id!r}, which "
                "the catalog does not define"
            )
            continue
        match = next((f for f in command.forms if f.form == form), None)
        if match is None:
            findings.append(
                f"{DAEMON_COMMANDS_PATH}: the effect table names the form {form!r} of "
                f"{command_id!r}, which the catalog does not define"
            )
            continue
        if match.kind != "DaemonEffect":
            findings.append(
                f"{DAEMON_COMMANDS_PATH}: the effect table names the frontend-effect "
                f"form {command_id!r}/{form!r}; only daemon effects may be executed"
            )
    return findings


def check_protocol(text: str) -> list[str]:
    findings: list[str] = []
    variants = [
        variant
        for variant in parse_core_request_variants(text)
        if variant.startswith("Command")
    ]
    for variant in sorted(set(variants) - ALLOWED_COMMAND_VARIANTS):
        findings.append(
            f"{PROTOCOL_PATH}: CoreRequest::{variant} carries a command outside the "
            "catalog; only CommandCatalog and CommandExecute may"
        )
    for variant in sorted(ALLOWED_COMMAND_VARIANTS - set(variants)):
        findings.append(
            f"{PROTOCOL_PATH}: CoreRequest::{variant} is missing; the daemon's "
            "command surface needs it"
        )
    return findings


def check_no_second_catalog(root: Path, commands: list[CatalogCommand]) -> list[str]:
    findings: list[str] = []
    renderer = root / DESKTOP_RENDERER_DIR
    if renderer.is_dir():
        # Only slash-prefixed spellings are forbidden. A bare alias such as
        # `resume` or `summarize` is indistinguishable from ordinary UI
        # prose, so forbidding it would make the rule a false-positive
        # machine rather than a boundary.
        names: set[str] = set()
        for command in commands:
            for spelling in (command.name, *command.aliases):
                if spelling.startswith("/"):
                    names.add(spelling)
        for path in sorted(renderer.rglob("*")):
            if not path.is_file():
                continue
            if path.suffix not in {".ts", ".tsx", ".js", ".jsx"}:
                continue
            # Comments are blanked first: a prose note that names a TUI
            # command is not a command list, and the renderer has one.
            text = editor_authority.strip_comments(read(path))
            for name in sorted(names):
                for offset in quoted_literal_hits(text, name):
                    findings.append(
                        f"{path.relative_to(root)}: the renderer holds the command name "
                        f"{name!r}; the daemon's catalog is the only source of "
                        "command names"
                    )
    tui_command = root / TUI_COMMAND_PATH
    if tui_command.is_file():
        text = read(tui_command)
        if re.search(r"enum\s+CommandKind\b", text):
            findings.append(
                f"{TUI_COMMAND_PATH}: the TUI defines its own command-kind table; the "
                "kind lives in the daemon's catalog"
            )
    return findings


def check_real_tree(root: Path) -> list[str]:
    findings: list[str] = []
    catalog_path = root / CATALOG_PATH
    tui_path = root / TUI_COMMAND_PATH
    protocol_path = root / PROTOCOL_PATH
    daemon_path = root / DAEMON_COMMANDS_PATH
    for path in (catalog_path, tui_path, protocol_path, daemon_path):
        if not path.is_file():
            findings.append(f"{path.relative_to(root)}: required source is missing")
            return findings
    commands = parse_catalog(read(catalog_path))
    if not commands:
        findings.append(f"{CATALOG_PATH}: no command entries were parsed")
        return findings
    findings.extend(check_catalog_shape(commands))
    findings.extend(check_same_set(commands, parse_tui_commands(read(tui_path))))
    findings.extend(check_effect_table(commands, parse_effect_table(read(daemon_path))))
    findings.extend(check_protocol(read(protocol_path)))
    findings.extend(check_no_second_catalog(root, commands))
    return findings


# ── Self-test ────────────────────────────────────────────────────────


def _synthetic_tree(source: Path, dest: Path) -> Path:
    """A copy of the real tree with only the files this guard reads.

    `base` is the caller's temporary directory, never the repository root:
    a self-test that copied the tree into the repository would leave a
    duplicate of every file it reads behind it, untracked and invisible to
    `.gitignore`.
    """
    tree = dest / "tree"
    for rel in (
        CATALOG_PATH,
        TUI_COMMAND_PATH,
        PROTOCOL_PATH,
        DAEMON_COMMANDS_PATH,
    ):
        target = tree / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source / rel, target)
    return tree


def _self_test(base: Path) -> int:
    failures: list[str] = []

    def case(name: str, mutate, expect: str) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            # The synthetic tree lives under the temporary directory, so a
            # self-test never leaves a copy of the repository behind.
            tree = _synthetic_tree(base, root)
            mutate(tree)
            findings = check_real_tree(tree)
            if not any(expect in finding for finding in findings):
                failures.append(
                    f"{name}: the injected defect was not reported "
                    f"(expected a finding containing {expect!r}; got {findings!r})"
                )

    def drop_catalog_command(tree: Path) -> None:
        path = tree / CATALOG_PATH
        text = path.read_text(encoding="utf-8")
        marker = 'command_id: "models_refresh",'
        idx = text.index(marker)
        start = text.rindex("CommandDescriptor {", 0, idx)
        end = text.index("        },\n", idx) + len("        },\n")
        path.write_text(text[:start] + text[end:], encoding="utf-8")

    def drift_tui_description(tree: Path) -> None:
        path = tree / TUI_COMMAND_PATH
        text = path.read_text(encoding="utf-8")
        text = text.replace(
            '.with_description("Refresh model list")',
            '.with_description("Refresh the model list")',
            1,
        )
        path.write_text(text, encoding="utf-8")

    def drop_tui_row(tree: Path) -> None:
        path = tree / TUI_COMMAND_PATH
        text = path.read_text(encoding="utf-8")
        idx = text.index('Command::new("/models-refresh"')
        end = text.find("Command::new(", idx + 1)
        path.write_text(text[:idx] + text[end:], encoding="utf-8")

    def effect_table_names_frontend_form(tree: Path) -> None:
        path = tree / DAEMON_COMMANDS_PATH
        text = path.read_text(encoding="utf-8")
        # `exit` is a frontend effect: the frontend owns it, and the daemon
        # may never execute one.
        text = text.replace(
            "    let shape = match (command_id, form) {",
            '    let shape = match (command_id, form) {\n'
            '        ("exit", "") => json!({ "type": "asset_refresh" }),',
            1,
        )
        path.write_text(text, encoding="utf-8")

    def renderer_command_list_single_quoted_tsx(tree: Path) -> None:
        renderer = tree / DESKTOP_RENDERER_DIR
        renderer.mkdir(parents=True, exist_ok=True)
        (renderer / "commands_single_quoted_tsx.tsx").write_text(
            "export const COMMANDS = ['/reload'];\n", encoding="utf-8"
        )

    def renderer_command_list_single_quoted_ts(tree: Path) -> None:
        renderer = tree / DESKTOP_RENDERER_DIR
        renderer.mkdir(parents=True, exist_ok=True)
        (renderer / "commands_single_quoted_ts.ts").write_text(
            "export const COMMANDS = ['/reload'];\n", encoding="utf-8"
        )

    def renderer_command_list_double_quoted_tsx(tree: Path) -> None:
        renderer = tree / DESKTOP_RENDERER_DIR
        renderer.mkdir(parents=True, exist_ok=True)
        (renderer / "commands_double_quoted_tsx.tsx").write_text(
            'export const COMMANDS = ["/reload"];\n', encoding="utf-8"
        )

    def renderer_command_list_template_literal_tsx(tree: Path) -> None:
        renderer = tree / DESKTOP_RENDERER_DIR
        renderer.mkdir(parents=True, exist_ok=True)
        (renderer / "commands_template_literal_tsx.tsx").write_text(
            "export const COMMANDS = [`/reload`];\n", encoding="utf-8"
        )

    def extra_command_variant(tree: Path) -> None:
        path = tree / PROTOCOL_PATH
        text = path.read_text(encoding="utf-8")
        text = text.replace(
            "pub enum CoreRequest {\n    Initialize,",
            "pub enum CoreRequest {\n    Initialize,\n    CommandRun { id: String },",
            1,
        )
        path.write_text(text, encoding="utf-8")

    case(
        "a catalog command the TUI never dispatches",
        drop_catalog_command,
        "models_refresh",
    )
    case(
        "a TUI description that drifted from the catalog",
        drift_tui_description,
        "describes itself as",
    )
    case(
        "a TUI row the catalog does not know",
        drop_tui_row,
        "the TUI never dispatches",
    )
    case(
        "an effect-table entry naming a frontend-effect form",
        effect_table_names_frontend_form,
        "frontend-effect form",
    )
    # The renderer writes single-quoted strings, so the rule is proved
    # against the quoting style a reviewer would actually use, and against
    # the extensions the renderer actually uses.
    case(
        "a renderer command list written with single quotes in a .tsx file",
        renderer_command_list_single_quoted_tsx,
        "commands_single_quoted_tsx.tsx: the renderer holds the command name",
    )
    case(
        "a renderer command list written with single quotes in a .ts file",
        renderer_command_list_single_quoted_ts,
        "commands_single_quoted_ts.ts: the renderer holds the command name",
    )
    case(
        "a renderer command list written with double quotes in a .tsx file",
        renderer_command_list_double_quoted_tsx,
        "commands_double_quoted_tsx.tsx: the renderer holds the command name",
    )
    case(
        "a renderer command list written with a template literal",
        renderer_command_list_template_literal_tsx,
        "commands_template_literal_tsx.tsx: the renderer holds the command name",
    )
    case(
        "a CoreRequest variant that carries a command outside the catalog",
        extra_command_variant,
        "carries a command outside the catalog",
    )

    if failures:
        for failure in failures:
            print(f"SELF-TEST FAILURE: {failure}", file=sys.stderr)
        return 2
    print("self-test passed: 9 injected defect classes were all rejected")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        return _self_test(REPO_ROOT)
    findings = check_real_tree(REPO_ROOT)
    if findings:
        print("command catalog ownership violations:", file=sys.stderr)
        for finding in findings:
            print(f"  - {finding}", file=sys.stderr)
        return 1
    print("command catalog ownership: clean")
    return 0


if __name__ == "__main__":
    sys.exit(main())
