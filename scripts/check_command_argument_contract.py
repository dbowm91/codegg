#!/usr/bin/env python3
"""Guard: a slash command's argument is extracted once, and `is_open` is total.

Two invariants live here, both about the TUI's command surface:

1. **The argument contract.** A slash command's argument is extracted once,
   where the typed line is resolved to a registry entry
   (`Command::argument_from_typed_line`). A handler receives the argument and
   nothing else — never the typed line, never an argument that still starts
   with the command's name or with one of its aliases. So no arm of the
   built-in dispatcher may read `command_palette.query` itself: the palette's
   query is the *whole typed line*, and a handler that reads it re-derives
   the argument by hand, which is how six commands ended up searching for
   the literal `"/search foo"` and deleting the task `"/task-del 5"`.

2. **`Dialog::is_open` covers every variant.** The predicate answers "is a
   dialog on screen?". It matched 50 of the enum's 53 variants and omitted
   `PanelManager` and `Control`, so a caller asking the question was told
   "no" while the panel manager was on screen.

The Rust parsing is done by `scripts/rust_test_scan.py`, the shared
Rust-aware scanner: it knows about raw strings, byte strings, character
literals and comments, and a regex scan misreads a lifetime as a character
literal. The scanner is imported, not reimplemented.

Usage:
    python3 scripts/check_command_argument_contract.py            # scan the tree
    python3 scripts/check_command_argument_contract.py --self-test
Exit codes: 0 clean, 1 violations found, 2 self-test failure.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import rust_test_scan as rts  # noqa: E402

# The dispatcher that owns the built-in slash commands.
DISPATCHER_REL = "src/tui/app/mod.rs"
DISPATCHER_FN = "dispatch_builtin_command"

# The dialog enum and its "is a dialog open" predicate.
DIALOG_REL = "src/tui/app/types.rs"

# `Dialog::None` is the *absence* of a dialog, so `is_open` must not list it.
# Every other variant is a dialog that can be on screen.
ABSENT_VARIANTS = frozenset({"None"})

# A read of the palette's query: the field and the method call.
QUERY_READ = re.compile(r"command_palette\s*\.\s*query")

# The dispatcher's match arms, used to name the arm a finding sits in.
ARM_HEAD = re.compile(r"\bB::([A-Za-z0-9_]+)\s*=>")

# `Self::Variant` in the `is_open` predicate.
SELF_VARIANT = re.compile(r"Self::([A-Za-z0-9_]+)")


def _function_body(source: str, name: str) -> tuple[str, str] | None:
    """The masked and unmasked body of the named function, or None.

    The mask has string literals and comments blanked, so a match inside a
    literal or a doc comment is not a match. The unmasked copy keeps the
    literals, for reporting the offending line.
    """
    scan = rts.scan_source(source)
    mask = scan.mask()
    for function in rts.find_functions(source, Path("<guard>"), "<guard>", "src"):
        if function.name != name:
            continue
        return mask[function.item_start : function.body_end + 1], source[
            function.item_start : function.body_end + 1
        ]
    return None


def dispatcher_query_reads(source: str) -> list[str]:
    """Reads of the palette's query inside the built-in dispatcher.

    Each finding names the arm it sits in, so the report points at the
    handler that has to change.
    """
    body = _function_body(source, DISPATCHER_FN)
    if body is None:
        return [f"{DISPATCHER_REL}: `{DISPATCHER_FN}` was not found"]
    mask_body, code_body = body
    findings: list[str] = []
    for match in QUERY_READ.finditer(mask_body):
        arm = None
        for head in ARM_HEAD.finditer(mask_body, 0, match.start()):
            arm = head.group(1)
        line_no = code_body.count("\n", 0, match.start()) + 1
        where = f"arm B::{arm}" if arm else "the dispatcher body"
        findings.append(
            f"{DISPATCHER_REL}:{line_no}: {where} reads `command_palette.query`; "
            "the argument is extracted once, at the registry entry point"
        )
    return findings


def _split_top_level(body: str) -> list[str]:
    """Split an enum body on the commas that separate its variants."""
    parts: list[str] = []
    depth = 0
    current: list[str] = []
    for char in body:
        if char in "([{<":
            depth += 1
        elif char in ")]}>":
            depth -= 1
        if char == "," and depth == 0:
            parts.append("".join(current))
            current = []
            continue
        current.append(char)
    parts.append("".join(current))
    return parts


def dialog_variants(source: str) -> list[str]:
    """The variant names of the `Dialog` enum, in declaration order."""
    scan = rts.scan_source(source)
    mask = scan.mask()
    index = mask.find("enum Dialog")
    if index < 0:
        return []
    open_brace = mask.find("{", index)
    if open_brace < 0:
        return []
    depth = 0
    close = -1
    for position in range(open_brace, len(mask)):
        if mask[position] == "{":
            depth += 1
        elif mask[position] == "}":
            depth -= 1
            if depth == 0:
                close = position
                break
    if close < 0:
        return []
    variants: list[str] = []
    for part in _split_top_level(mask[open_brace + 1 : close]):
        name = re.match(r"\s*([A-Za-z0-9_]+)", part)
        if name is None:
            continue
        variant = name.group(1)
        if variant and variant[0].isupper() and variant not in variants:
            variants.append(variant)
    return variants


def is_open_variants(source: str) -> set[str]:
    """The variants the `is_open` predicate lists."""
    body = _function_body(source, "is_open")
    if body is None:
        return set()
    mask_body, _ = body
    return set(SELF_VARIANT.findall(mask_body))


def dialog_predicate_gaps(source: str) -> list[str]:
    """Dialog variants that `is_open` does not list."""
    variants = dialog_variants(source)
    if not variants:
        return [f"{DIALOG_REL}: the `Dialog` enum was not found"]
    listed = is_open_variants(source)
    if not listed:
        return [f"{DIALOG_REL}: `Dialog::is_open` lists no variant"]
    findings: list[str] = []
    for variant in variants:
        if variant in ABSENT_VARIANTS:
            continue
        if variant not in listed:
            findings.append(
                f"{DIALOG_REL}: `Dialog::{variant}` is missing from `Dialog::is_open`; "
                "the predicate must cover every variant that can be on screen"
            )
    return findings


def collect_findings(root: Path = ROOT) -> list[str]:
    findings: list[str] = []
    dispatcher = root / DISPATCHER_REL
    if dispatcher.is_file():
        findings.extend(dispatcher_query_reads(dispatcher.read_text(encoding="utf-8")))
    else:
        findings.append(f"{DISPATCHER_REL}: file not found")
    dialog = root / DIALOG_REL
    if dialog.is_file():
        findings.extend(dialog_predicate_gaps(dialog.read_text(encoding="utf-8")))
    else:
        findings.append(f"{DIALOG_REL}: file not found")
    return findings


# ── Self-test ─────────────────────────────────────────────────────────
#
# Each case injects a violation into the *real* source and proves the same
# detector the tree scan uses reports it. A guard that cannot be shown to
# fail on an injected violation is not evidence of anything.

SYNTHETIC_DISPATCHER = """
impl App {
    pub(crate) fn dispatch_builtin_command(&mut self, action: Action, argument: Option<&str>) {
        let argument = argument.unwrap_or_default().trim();
        match action {
            B::Search => {
                let query = self.dialog_state.command_palette.query.trim().to_string();
                self.messages_state.messages.search(&query);
            }
            B::Help => {
                self.open_help();
            }
        }
    }
}
"""


def run_self_test(root: Path = ROOT) -> int:
    failures = 0

    def check(name: str, condition: bool, detail: str = "") -> None:
        nonlocal failures
        if condition:
            print(f"  ok   {name}")
        else:
            failures += 1
            print(f"  FAIL {name}{': ' + detail if detail else ''}")

    # 1. The real dispatcher is clean.
    dispatcher_path = root / DISPATCHER_REL
    real_dispatcher = (
        dispatcher_path.read_text(encoding="utf-8") if dispatcher_path.is_file() else ""
    )
    findings = dispatcher_query_reads(real_dispatcher)
    check(
        "the real dispatcher reads no palette query",
        not findings,
        "; ".join(findings),
    )

    # 2. An injected arm that reads the palette query is rejected.
    injected = real_dispatcher.replace(
        "            B::Help => {",
        "            B::InjectedViolation => {\n"
        "                let query = self.dialog_state.command_palette.query.clone();\n"
        "                self.messages_state.messages.search(&query);\n"
        "            }\n"
        "            B::Help => {",
        1,
    )
    if injected == real_dispatcher:
        injected = SYNTHETIC_DISPATCHER
    findings = dispatcher_query_reads(injected)
    check(
        "an injected arm that reads the palette query is rejected",
        len(findings) == 1,
        "; ".join(findings),
    )
    check(
        "the injected finding names the arm",
        bool(findings) and "B::InjectedViolation" in findings[0],
        "; ".join(findings),
    )

    # 3. A synthetic dispatcher that reads the query is rejected too, so the
    #    detector does not depend on the real file's shape.
    findings = dispatcher_query_reads(SYNTHETIC_DISPATCHER)
    check(
        "a synthetic dispatcher that reads the palette query is rejected",
        len(findings) == 1,
        "; ".join(findings),
    )

    # 4. A synthetic dispatcher that does not read the query is accepted.
    clean = SYNTHETIC_DISPATCHER.replace(
        "                let query = self.dialog_state.command_palette.query.trim().to_string();\n"
        "                self.messages_state.messages.search(&query);",
        "                self.messages_state.messages.search(argument);",
    )
    findings = dispatcher_query_reads(clean)
    check(
        "a dispatcher that reads only the extracted argument is accepted",
        not findings,
        "; ".join(findings),
    )

    # 5. The real dialog predicate covers every variant.
    dialog_path = root / DIALOG_REL
    real_dialog = dialog_path.read_text(encoding="utf-8") if dialog_path.is_file() else ""
    findings = dialog_predicate_gaps(real_dialog)
    check(
        "the real `Dialog::is_open` covers every variant",
        not findings,
        "; ".join(findings),
    )

    # 6. A variant dropped from the predicate is reported.
    stripped = real_dialog.replace("                | Self::PanelManager\n", "", 1)
    if stripped == real_dialog:
        stripped = real_dialog.replace("                | Self::Queue\n", "", 1)
    findings = dialog_predicate_gaps(stripped)
    check(
        "a dialog variant dropped from `is_open` is reported",
        len(findings) == 1,
        "; ".join(findings),
    )

    # 7. A synthetic enum with an unlisted variant is reported.
    synthetic = """
pub enum Dialog {
    None,
    Model,
    PanelManager,
}

impl Dialog {
    pub fn is_open(&self) -> bool {
        matches!(self, Self::Model)
    }
}
"""
    findings = dialog_predicate_gaps(synthetic)
    check(
        "a synthetic enum with an unlisted variant is reported",
        len(findings) == 1 and "PanelManager" in findings[0],
        "; ".join(findings),
    )

    # 8. The absent variant is not required to be listed.
    complete = """
pub enum Dialog {
    None,
    Model,
}

impl Dialog {
    pub fn is_open(&self) -> bool {
        matches!(self, Self::Model)
    }
}
"""
    findings = dialog_predicate_gaps(complete)
    check(
        "`Dialog::None` is not required in the predicate",
        not findings,
        "; ".join(findings),
    )

    if failures:
        print(f"\nself-test FAILED: {failures} case(s) — the detector does not discriminate.")
        return 2

    print("\nself-test passed: 9 case(s).")
    return 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return run_self_test()

    findings = collect_findings()

    if findings:
        print(
            "slash-command argument contract violated.\n"
            "A built-in command's argument is extracted once, where the typed\n"
            "line is resolved to a registry entry. An arm that reads\n"
            "`command_palette.query` re-derives the argument by hand, and the\n"
            "palette's query is the whole typed line — so the handler sees the\n"
            "command's own name inside its argument.\n"
            "`Dialog::is_open` must cover every variant that can be on screen.\n"
        )
        for finding in findings:
            print(f"  {finding}")
        print(f"\n{len(findings)} violation(s) found.")
        return 1

    print(
        "command argument contract check passed — the dispatcher reads no palette "
        "query and `Dialog::is_open` covers every variant"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
