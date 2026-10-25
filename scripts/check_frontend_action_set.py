#!/usr/bin/env python3
"""Guard: the frontend-action set is declared, finite and closed.

ADR-0052 (accepted 2026-10-24) gives the frontend one bounded set of
actions it may dispatch locally. A frontend-effect command is dispatched by
the frontend from that set; a daemon-effect command never reaches the
frontend at all.

The set lives in `crates/codegg-core/src/frontend_action.rs`: a finite
`FrontendAction` enum plus a declaration of each action's disposition.

This guard proves, statically:

1. **Finite.** There is a declared `FrontendAction` enum and a declaration
   table, and every declaration names an action the enum has.
2. **Closed.** Every frontend-effect form in the daemon's catalog resolves
   to exactly one declared action, and every declared action resolves to at
   least one frontend-effect form. An action with no form is dead weight; a
   form with no action cannot be dispatched by anybody.
3. **Non-mutating.** Every declaration carries a non-mutating disposition —
   `NonMutating`, or `ProcessLaunching` for the one action that launches a
   frontend-local process. There is no mutating disposition in the set, so a
   frontend action can never change daemon-owned state.
4. **Nothing daemon-mutating by accident.** No declared action serves a
   form the catalog classifies as a daemon effect: the daemon owns those.
5. **The frontend dispatches only declared actions.** Every
   `FrontendAction::…` the TUI names is in the declared set, so a frontend
   cannot dispatch an action nobody reviewed.

Why the guard is shaped this way
--------------------------------
The set is the boundary between what the frontend may do and what the
daemon owns. A guard that only checked the enum existed would pass on an
empty set, and a guard that only checked the table existed would pass on a
table of daemon effects. Both halves are checked, and both are checked
against the *catalog*, so the set cannot drift away from the commands it
covers.

The guard self-tests. `--self-test` builds synthetic trees with one defect
each and proves the detector rejects every one of them. A guard that cannot
be shown to fail on an injected defect is not accepted as evidence.

Usage:
    python3 scripts/check_frontend_action_set.py
    python3 scripts/check_frontend_action_set.py --self-test

Exit codes: 0 clean, 1 violations found, 2 self-test failure.
"""

from __future__ import annotations

import re
import shutil
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

CATALOG_PATH = "crates/codegg-core/src/command_catalog.rs"
FRONTEND_ACTION_PATH = "crates/codegg-core/src/frontend_action.rs"
TUI_TREE = "src/tui"

# The dispositions a declaration may carry. There is deliberately no
# mutating disposition: a frontend action changes only what the frontend
# is showing.
ALLOWED_DISPOSITIONS = {"NonMutating", "ProcessLaunching"}


# ── Rust source helpers ──────────────────────────────────────────────


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def parse_rust_string(text: str, start: int) -> tuple[str, int] | None:
    """Read one Rust string literal starting at the opening quote."""
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


def string_literals(text: str) -> list[str]:
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


# ── The catalog ──────────────────────────────────────────────────────


@dataclass
class CatalogForm:
    form: str
    kind: str


@dataclass
class CatalogCommand:
    command_id: str
    forms: list[CatalogForm]


DESCRIPTOR_RE = re.compile(r"CommandDescriptor\s*\{")


def parse_catalog(text: str) -> list[CatalogCommand]:
    commands: list[CatalogCommand] = []
    for match in DESCRIPTOR_RE.finditer(text):
        start = text.index("{", match.start())
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
        block = text[start : end + 1]
        id_idx = block.find("command_id:")
        if id_idx < 0:
            continue
        quote = block.find('"', id_idx)
        parsed = parse_rust_string(block, quote) if quote >= 0 else None
        if parsed is None:
            continue
        command_id = parsed[0]
        forms: list[CatalogForm] = []
        forms_idx = block.find("forms:")
        if forms_idx >= 0:
            forms_block = block[forms_idx:]
            for form_match in re.finditer(r"CommandForm\s*\{", forms_block):
                form_text = forms_block[form_match.start() : form_match.start() + 400]
                form_quote = form_text.find('"', form_text.find("form:"))
                form_parsed = (
                    parse_rust_string(form_text, form_quote)
                    if form_quote >= 0
                    else None
                )
                kind_match = re.search(r"kind:\s*CommandKind::(\w+)", form_text)
                if form_parsed is None or kind_match is None:
                    continue
                forms.append(CatalogForm(form=form_parsed[0], kind=kind_match.group(1)))
        commands.append(CatalogCommand(command_id=command_id, forms=forms))
    return commands


# ── The declared action set ──────────────────────────────────────────


@dataclass
class Declaration:
    action: str
    disposition: str
    forms: list[tuple[str, str]]


ENUM_RE = re.compile(r"pub enum FrontendAction\s*\{")
DECLARATION_RE = re.compile(r"FrontendActionDeclaration\s*\{")


def parse_action_enum(text: str) -> list[str]:
    match = ENUM_RE.search(text)
    if not match:
        return []
    start = text.index("{", match.start())
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
    body = text[start : end + 1]
    variants: list[str] = []
    for line in body.splitlines():
        variant = re.match(r"\s{4}([A-Z][A-Za-z0-9]*),\s*$", line)
        if variant:
            variants.append(variant.group(1))
    return variants


def parse_declarations(text: str) -> list[Declaration]:
    declarations: list[Declaration] = []
    for match in DECLARATION_RE.finditer(text):
        start = text.index("{", match.start())
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
        block = text[start : end + 1]
        action_match = re.search(r"action:\s*FrontendAction::(\w+)", block)
        disposition_match = re.search(r"disposition:\s*FrontendActionDisposition::(\w+)", block)
        if not action_match or not disposition_match:
            continue
        forms: list[tuple[str, str]] = []
        forms_idx = block.find("forms:")
        if forms_idx >= 0:
            forms_block = block[forms_idx:]
            for form in re.finditer(
                r'\(\s*"([a-z0-9_]+)"\s*,\s*"([^"]*)"\s*\)', forms_block
            ):
                forms.append((form.group(1), form.group(2)))
        declarations.append(
            Declaration(
                action=action_match.group(1),
                disposition=disposition_match.group(1),
                forms=forms,
            )
        )
    return declarations


# ── The checks ───────────────────────────────────────────────────────


def check_set_shape(
    variants: list[str], declarations: list[Declaration]
) -> list[str]:
    findings: list[str] = []
    if not variants:
        findings.append(f"{FRONTEND_ACTION_PATH}: no FrontendAction enum was found")
        return findings
    if not declarations:
        findings.append(
            f"{FRONTEND_ACTION_PATH}: the frontend-action set declares no action"
        )
        return findings
    for declaration in declarations:
        if declaration.action not in variants:
            findings.append(
                f"{FRONTEND_ACTION_PATH}: the declaration names "
                f"FrontendAction::{declaration.action}, which the enum does not have"
            )
        if declaration.disposition not in ALLOWED_DISPOSITIONS:
            findings.append(
                f"{FRONTEND_ACTION_PATH}: {declaration.action} carries the disposition "
                f"{declaration.disposition}; a frontend action may only be "
                f"{sorted(ALLOWED_DISPOSITIONS)}"
            )
        if not declaration.forms:
            findings.append(
                f"{FRONTEND_ACTION_PATH}: {declaration.action} declares no catalog form"
            )
    declared = [declaration.action for declaration in declarations]
    for variant in variants:
        if variant not in declared:
            findings.append(
                f"{FRONTEND_ACTION_PATH}: FrontendAction::{variant} is never declared"
            )
    return findings


def check_closure(
    commands: list[CatalogCommand], declarations: list[Declaration]
) -> list[str]:
    findings: list[str] = []
    by_id = {command.command_id: command for command in commands}
    covered: dict[tuple[str, str], list[str]] = {}
    for declaration in declarations:
        for command_id, form in declaration.forms:
            covered.setdefault((command_id, form), []).append(declaration.action)
            command = by_id.get(command_id)
            if command is None:
                findings.append(
                    f"{FRONTEND_ACTION_PATH}: {declaration.action} serves "
                    f"{command_id!r}, which the catalog does not define"
                )
                continue
            match = next((f for f in command.forms if f.form == form), None)
            if match is None:
                findings.append(
                    f"{FRONTEND_ACTION_PATH}: {declaration.action} serves the form "
                    f"{form!r} of {command_id!r}, which the catalog does not define"
                )
                continue
            if match.kind != "FrontendEffect":
                findings.append(
                    f"{FRONTEND_ACTION_PATH}: {declaration.action} serves the "
                    f"daemon-effect form {command_id!r}/{form!r}; the daemon owns it"
                )
    for command in commands:
        for form in command.forms:
            if form.kind != "FrontendEffect":
                continue
            actions = covered.get((command.command_id, form.form), [])
            if not actions:
                findings.append(
                    f"{CATALOG_PATH}: the frontend-effect form "
                    f"{command.command_id!r}/{form.form!r} has no declared action"
                )
            elif len(actions) > 1:
                findings.append(
                    f"{CATALOG_PATH}: the frontend-effect form "
                    f"{command.command_id!r}/{form.form!r} is served by "
                    f"{sorted(actions)}; it must be served by exactly one"
                )
    return findings


def check_frontend_dispatch(root: Path, variants: list[str]) -> list[str]:
    findings: list[str] = []
    tree = root / TUI_TREE
    if not tree.is_dir():
        return findings
    for path in sorted(tree.rglob("*.rs")):
        text = read(path)
        for used in set(re.findall(r"FrontendAction::(\w+)", text)):
            if used not in variants:
                findings.append(
                    f"{path.relative_to(root)}: the frontend dispatches "
                    f"FrontendAction::{used}, which is not in the declared set"
                )
    return findings


def check_real_tree(root: Path) -> list[str]:
    findings: list[str] = []
    catalog_path = root / CATALOG_PATH
    action_path = root / FRONTEND_ACTION_PATH
    for path in (catalog_path, action_path):
        if not path.is_file():
            findings.append(f"{path.relative_to(root)}: required source is missing")
            return findings
    commands = parse_catalog(read(catalog_path))
    if not commands:
        findings.append(f"{CATALOG_PATH}: no command entries were parsed")
        return findings
    action_text = read(action_path)
    variants = parse_action_enum(action_text)
    declarations = parse_declarations(action_text)
    findings.extend(check_set_shape(variants, declarations))
    findings.extend(check_closure(commands, declarations))
    findings.extend(check_frontend_dispatch(root, variants))
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
    for rel in (CATALOG_PATH, FRONTEND_ACTION_PATH):
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

    def undeclared_form(tree: Path) -> None:
        path = tree / FRONTEND_ACTION_PATH
        text = path.read_text(encoding="utf-8")
        text = text.replace(
            'forms: &[("help", "")],', 'forms: &[],', 1
        )
        path.write_text(text, encoding="utf-8")

    def action_without_a_form(tree: Path) -> None:
        path = tree / FRONTEND_ACTION_PATH
        text = path.read_text(encoding="utf-8")
        text = text.replace(
            "pub enum FrontendAction {",
            "pub enum FrontendAction {\n    Unclaimed,",
            1,
        )
        text = text.replace(
            "static THE_FRONTEND_ACTION_SET: &[FrontendActionDeclaration] = &[",
            "static THE_FRONTEND_ACTION_SET: &[FrontendActionDeclaration] = &[\n"
            "    FrontendActionDeclaration {\n"
            "        action: FrontendAction::Unclaimed,\n"
            "        disposition: FrontendActionDisposition::NonMutating,\n"
            "        forms: &[],\n"
            "    },",
            1,
        )
        path.write_text(text, encoding="utf-8")

    def mutating_disposition(tree: Path) -> None:
        path = tree / FRONTEND_ACTION_PATH
        text = path.read_text(encoding="utf-8")
        text = text.replace(
            "disposition: FrontendActionDisposition::NonMutating,",
            "disposition: FrontendActionDisposition::Mutating,",
            1,
        )
        path.write_text(text, encoding="utf-8")

    def serves_a_daemon_effect(tree: Path) -> None:
        path = tree / FRONTEND_ACTION_PATH
        text = path.read_text(encoding="utf-8")
        text = text.replace(
            'forms: &[("help", "")],', 'forms: &[("help", ""), ("reload", "")],', 1
        )
        path.write_text(text, encoding="utf-8")

    def frontend_dispatches_an_undeclared_action(tree: Path) -> None:
        tui = tree / TUI_TREE
        tui.mkdir(parents=True, exist_ok=True)
        (tui / "dispatch.rs").write_text(
            "fn dispatch() { let _ = FrontendAction::NeverDeclared; }\n",
            encoding="utf-8",
        )

    case(
        "a frontend-effect form with no declared action",
        undeclared_form,
        "has no declared action",
    )
    case(
        "a declared action with no catalog form",
        action_without_a_form,
        "declares no catalog form",
    )
    case(
        "a declaration with a mutating disposition",
        mutating_disposition,
        "carries the disposition",
    )
    case(
        "a declared action serving a daemon-effect form",
        serves_a_daemon_effect,
        "serves the daemon-effect form",
    )
    case(
        "the frontend dispatching an undeclared action",
        frontend_dispatches_an_undeclared_action,
        "is not in the declared set",
    )

    if failures:
        for failure in failures:
            print(f"SELF-TEST FAILURE: {failure}", file=sys.stderr)
        return 2
    print("self-test passed: 5 injected defect classes were all rejected")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        return _self_test(REPO_ROOT)
    findings = check_real_tree(REPO_ROOT)
    if findings:
        print("frontend-action set violations:", file=sys.stderr)
        for finding in findings:
            print(f"  - {finding}", file=sys.stderr)
        return 1
    print("frontend-action set: clean")
    return 0


if __name__ == "__main__":
    sys.exit(main())
