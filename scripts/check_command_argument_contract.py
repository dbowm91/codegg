#!/usr/bin/env python3
"""Guard: a slash command's argument is extracted once, and `is_open` is total.

Two invariants live here, both about the TUI's command surface:

1. **The argument contract.** A slash command's argument is extracted once,
   where the typed line is resolved to a registry entry
   (`Command::argument_from_typed_line`). A handler receives the argument and
   nothing else — never the typed line, never an argument that still starts
   with the command's name or with one of its aliases. So neither the
   dispatcher nor any handler may read `command_palette.query`: the palette's
   query is the *whole typed line*, and code that reads it re-derives the
   argument by hand, which is how six commands ended up searching for the
   literal `"/search foo"` and deleting the task `"/task-del 5"`.

2. **`Dialog::is_open` covers every variant.** The predicate answers "is a
   dialog on screen?". It matched 50 of the enum's 53 variants and omitted
   `PanelManager` and `Control`, so a caller asking the question was told
   "no" while the panel manager was on screen.

## The scanned surface, and why it is checked rather than assumed

A built-in command's handler lives in one of the dispatcher's family modules
under `src/tui/commands/`, and the dispatcher's own body routes to them. Both
are scanned: the dispatcher because it owns the `argument` each handler is
given, and the family modules because that is where a handler now lives.

That surface moved once already. Milestone 010 emptied the dispatcher body
into 45 family modules, leaving a delegation match, and this guard scanned
only the one function — so its real check went **vacuous**: it reported clean
while the arms it was written for were somewhere else, and only `--self-test`
noticed, because the injection anchor it used (`B::Help => {`) no longer
existed and the injection silently fell back to a synthetic fixture that
names a different arm. A scan that can silently lose its subject is worse
than no scan, so the surface's own presence is asserted here: a missing
dispatcher function, a missing family-module directory, and a directory that
has fewer modules than the floor below are all reported as findings.

The Rust parsing is done by `scripts/rust_test_scan.py`, the shared
Rust-aware scanner: it knows about raw strings, byte strings, character
literals and comments, and a regex read of Rust misreads a lifetime as a
character literal. The scanner is imported, not reimplemented. Its mask keeps
the original's length and newline count, so every line number reported here
is the file's own.

Usage:
    python3 scripts/check_command_argument_contract.py            # scan the tree
    python3 scripts/check_command_argument_contract.py --self-test
Exit codes: 0 clean, 1 violations found, 2 self-test failure.
"""

from __future__ import annotations

import re
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import rust_test_scan as rts  # noqa: E402

# The dispatcher that routes the built-in slash commands to their handlers.
DISPATCHER_REL = "src/tui/app/mod.rs"
DISPATCHER_FN = "dispatch_builtin_command"

# Its family modules: the surface the arms moved to.
COMMANDS_DIR_REL = "src/tui/commands"

# The module count this surface has today (57). A floor, not an equality: a
# module may be added without editing this guard, and a scan that stops
# seeing the directory reports a gap instead of passing clean.
MIN_SOURCE_FILES = 40

# The dialog enum and its "is a dialog open" predicate.
DIALOG_REL = "src/tui/app/types.rs"

# `Dialog::None` is the *absence* of a dialog, so `is_open` must not list it.
# Every other variant is a dialog that can be on screen.
ABSENT_VARIANTS = frozenset({"None"})

# A use of the palette's query: the field and the member name.
QUERY = re.compile(r"command_palette\s*\.\s*query")

# `String` mutators. A write cannot be a handler's reading of the argument:
# the line the palette holds is the whole typed line, and appending to it is
# how the palette is typed into. Everything that is not a write is a read.
QUERY_WRITERS = frozenset(
    {
        "push",
        "push_str",
        "clear",
        "truncate",
        "pop",
        "insert",
        "insert_str",
        "retain",
        "drain",
        "extend",
        "reserve",
        "reserve_exact",
        "shrink_to_fit",
        "replace_range",
        "remove",
        "swap_remove",
        "as_mut_vec",
        "splice",
    }
)

IDENT = re.compile(r"[A-Za-z0-9_]+")

# `Self::Variant` in the `is_open` predicate.
SELF_VARIANT = re.compile(r"Self::([A-Za-z0-9_]+)")


@dataclass(frozen=True)
class Region:
    """One scanned span: its source, its mask, and where it starts.

    `line_base` is the line of the file that the span's first character sits
    on, so a line number computed inside the span is the file's own. The
    dispatcher is scanned as a span rather than as a file, and it must not
    report its findings at line 1.
    """

    rel: str
    text: str
    mask: str
    line_base: int
    functions: tuple


def _region(rel: str, text: str, start: int = 0, end: int | None = None) -> Region:
    """The span `text[start:end]`, masked for structure.

    The mask blanks string literals and comments, and then blanks every
    `#[cfg(test)]`-governed module on top of that: a test that exercises a
    second reader cannot satisfy or violate a rule about shipping code.
    Both steps keep the length and the newline count, so the span's offsets
    and line numbers are the original's.
    """
    span = text[start:end]
    mask = rts.scan_source(span).mask()
    mask = rts.production_mask(mask, rts.scan_source(mask))
    return Region(
        rel=rel,
        text=span,
        mask=mask,
        line_base=text.count("\n", 0, start) + 1,
        functions=tuple(rts.find_functions(span, Path(rel), rel, "src")),
    )


def _function_span(text: str, name: str) -> tuple[int, int, int] | None:
    """The `(item_start, body_start, body_end)` of the named function, or None.

    `body_start` is the opening brace; `item_start` is the item's own first
    character, which includes the `fn` header — a span that began at the
    brace would not be recognised as a function, and the finding would lose
    the name of the function that has to change. `find_functions` records
    `body_end` and the body's own text, so the brace's position is that
    text's length back from the end — no second brace matcher is written
    here.
    """
    for function in rts.find_functions(text, Path("<guard>"), "<guard>", "src"):
        if function.name != name:
            continue
        return (
            function.item_start,
            function.body_end + 1 - len(function.body_text),
            function.body_end,
        )
    return None


def _named_body(region: Region, name: str) -> str | None:
    """The masked body of the named function inside the region."""
    for function in region.functions:
        if function.name != name:
            continue
        start = function.body_end + 1 - len(function.body_text)
        return region.mask[start : function.body_end + 1]
    return None


def _enclosing(region: Region, offset: int) -> str | None:
    """The innermost function whose item contains `offset`."""
    best = None
    for function in region.functions:
        if function.item_start <= offset <= function.body_end:
            if best is None or function.item_start >= best.item_start:
                best = function
    return best.name if best is not None else None


def _is_write(mask: str, index: int) -> bool:
    """Whether the use of the query at `index` writes it instead of reading it.

    A write is an assignment (`= …`, but not `==` or `=>`) or a call to one of
    the `String` mutators. Everything else — `query.trim()`, `query.len()`,
    `&query`, passing it on — reads it, and a handler that reads it is
    re-deriving its own argument.
    """
    cursor = index
    length = len(mask)
    while cursor < length and mask[cursor].isspace():
        cursor += 1
    if cursor >= length:
        return False
    if mask[cursor] == "=":
        return not (cursor + 1 < length and mask[cursor + 1] in "=>")
    if mask[cursor] != ".":
        return False
    name = IDENT.match(mask, cursor + 1)
    if name is None:
        return False
    after = name.end()
    while after < length and mask[after].isspace():
        after += 1
    if after >= length or mask[after] != "(":
        return False
    return name.group(0) in QUERY_WRITERS


def region_findings(region: Region) -> list[str]:
    """Reads of the palette's query inside one region.

    Each finding names the function it sits in, because that is the handler
    that has to change.
    """
    findings: list[str] = []
    for match in QUERY.finditer(region.mask):
        if _is_write(region.mask, match.end()):
            continue
        line = region.line_base + region.mask.count("\n", 0, match.start())
        owner = _enclosing(region, match.start())
        where = f"`{owner}`" if owner else "file scope"
        findings.append(
            f"{region.rel}:{line}: {where} reads `command_palette.query`; the argument is "
            "extracted once, at the registry entry point, and handed to the handler"
        )
    return findings


def command_surface(root: Path = ROOT) -> tuple[list[Region], list[str]]:
    """Every region that can handle a built-in command, and the surface's gaps.

    Returns the regions to scan and the findings that say the surface is not
    the one this guard was written for — a moved dispatcher, an absent family
    directory, or a directory that has gone too small to be the real one.
    """
    regions: list[Region] = []
    gaps: list[str] = []

    dispatcher = root / DISPATCHER_REL
    text = dispatcher.read_text(encoding="utf-8") if dispatcher.is_file() else ""
    if not text:
        gaps.append(f"{DISPATCHER_REL}: file not found")
    else:
        span = _function_span(text, DISPATCHER_FN)
        if span is None:
            gaps.append(
                f"{DISPATCHER_REL}: `{DISPATCHER_FN}` was not found; it owns the `argument` "
                "every handler is given, and a guard that cannot find it is scanning nothing"
            )
        else:
            regions.append(_region(DISPATCHER_REL, text, span[0], span[2] + 1))

    module_dir = root / COMMANDS_DIR_REL
    if not module_dir.is_dir():
        gaps.append(
            f"{COMMANDS_DIR_REL}: directory not found; the dispatcher's family modules are "
            "half of this guard's surface"
        )
    else:
        files = sorted(module_dir.rglob("*.rs"))
        if len(files) < MIN_SOURCE_FILES:
            gaps.append(
                f"{COMMANDS_DIR_REL}: {len(files)} source file(s), fewer than the "
                f"{MIN_SOURCE_FILES} this surface's floor records; the scan is not seeing "
                "the modules it is written for"
            )
        for path in files:
            rel = path.relative_to(root).as_posix()
            regions.append(_region(rel, path.read_text(encoding="utf-8")))

    return regions, gaps


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
    mask = _region(DIALOG_REL, source).mask
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
    body = _named_body(_region(DIALOG_REL, source), "is_open")
    if body is None:
        return set()
    return set(SELF_VARIANT.findall(body))


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
    regions, findings = command_surface(root)
    for region in regions:
        findings.extend(region_findings(region))
    dialog = root / DIALOG_REL
    if dialog.is_file():
        findings.extend(dialog_predicate_gaps(dialog.read_text(encoding="utf-8")))
    else:
        findings.append(f"{DIALOG_REL}: file not found")
    return findings


# ── Self-test ─────────────────────────────────────────────────────────
#
# Each case injects a violation into the *real* source and proves the same
# detector the tree scan uses reports it, or injects the absence of the
# surface itself and proves the guard refuses to pass. A guard that cannot
# be shown to fail on an injected violation is not evidence of anything.

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

CLEAN_DISPATCHER = """
impl App {
    pub(crate) fn dispatch_builtin_command(&mut self, action: Action, argument: Option<&str>) {
        let argument = argument.unwrap_or_default().trim();
        match action {
            B::Search => self.search(argument),
            B::Help => self.open_help(),
        }
    }
}
"""


def _inject_statement(text: str, function: str, statement: str) -> str | None:
    """`text` with `statement` inserted at the start of the named function."""
    span = _function_span(text, function)
    if span is None:
        return None
    return f"{text[: span[1] + 1]}\n            {statement}{text[span[1] + 1 :]}"


def _span_findings(rel: str, text: str, function: str) -> list[str]:
    """The findings for the dispatcher *function span* of `text`."""
    span = _function_span(text, function)
    if span is None:
        return []
    return region_findings(_region(rel, text, span[0], span[2] + 1))


def run_self_test(root: Path = ROOT) -> int:
    failures = 0
    total = 0

    def check(name: str, condition: bool, detail: str = "") -> None:
        nonlocal failures, total
        total += 1
        if condition:
            print(f"  ok   {name}")
        else:
            failures += 1
            print(f"  FAIL {name}{': ' + detail if detail else ''}")

    # 1. The real surface is clean — both halves of it.
    regions, gaps = command_surface(root)
    findings = list(gaps)
    for region in regions:
        findings.extend(region_findings(region))
    check(
        f"the real surface ({len(regions)} region(s)) reads no palette query",
        not findings,
        "; ".join(findings),
    )

    # Nothing below is evidence if the surface above was empty, so name the
    # two halves explicitly rather than trusting the aggregate.
    check(
        "the surface holds the dispatcher and its family modules",
        any(region.rel == DISPATCHER_REL for region in regions)
        and sum(1 for region in regions if region.rel != DISPATCHER_REL) >= MIN_SOURCE_FILES,
        f"{len(regions)} region(s)",
    )

    # 2. A read injected into the real dispatcher is rejected, and the
    #    finding names the function that has to change.
    real_dispatcher = (root / DISPATCHER_REL).read_text(encoding="utf-8")
    injected = _inject_statement(
        real_dispatcher,
        DISPATCHER_FN,
        "let _q = self.dialog_state.command_palette.query.clone();\n",
    )
    findings = _span_findings(DISPATCHER_REL, injected, DISPATCHER_FN) if injected else []
    check(
        "a read injected into the real dispatcher is rejected",
        len(findings) == 1,
        "; ".join(findings),
    )
    check(
        "the dispatcher finding names the dispatcher",
        bool(findings) and DISPATCHER_FN in findings[0],
        "; ".join(findings),
    )

    # 3. A read injected into a real family module is rejected. This is the
    #    half that went unscanned while the arms lived here.
    family = root / COMMANDS_DIR_REL / "help.rs"
    real_family = family.read_text(encoding="utf-8")
    injected_family = _inject_statement(
        real_family,
        "dispatch_help",
        "let _q = self.dialog_state.command_palette.query.clone();\n",
    )
    findings = (
        region_findings(_region(f"{COMMANDS_DIR_REL}/help.rs", injected_family))
        if injected_family
        else []
    )
    check(
        "a read injected into a real family module is rejected",
        len(findings) == 1,
        "; ".join(findings),
    )
    check(
        "the family finding names its module and its function",
        bool(findings) and "help.rs" in findings[0] and "dispatch_help" in findings[0],
        "; ".join(findings),
    )

    # 4. Writes are accepted: the palette's line is written as it is typed
    #    into, and a write derives no argument.
    writes = _inject_statement(
        real_family,
        "dispatch_help",
        "self.dialog_state.command_palette.query = argument.unwrap_or_default().to_string();\n"
        "            self.dialog_state.command_palette.query.push('/');\n"
        "            self.dialog_state.command_palette.query.clear();\n",
    )
    findings = (
        region_findings(_region(f"{COMMANDS_DIR_REL}/help.rs", writes)) if writes else ["no anchor"]
    )
    check("writes to the palette's query are accepted", not findings, "; ".join(findings))

    # 5. A synthetic source with a read is rejected, so the detector does not
    #    depend on the real files' shape.
    findings = _span_findings(DISPATCHER_REL, SYNTHETIC_DISPATCHER, DISPATCHER_FN)
    check(
        "a synthetic dispatcher that reads the palette query is rejected",
        len(findings) == 1,
        "; ".join(findings),
    )

    # 6. A synthetic dispatcher that reads only the argument is accepted.
    findings = _span_findings(DISPATCHER_REL, CLEAN_DISPATCHER, DISPATCHER_FN)
    check(
        "a dispatcher that reads only the extracted argument is accepted",
        not findings,
        "; ".join(findings),
    )

    # 7. A moved dispatcher is a gap, not a pass. This is the exact defect
    #    that made this guard vacuous, so it is asserted directly.
    with tempfile.TemporaryDirectory() as tmp:
        moved = Path(tmp)
        (moved / DISPATCHER_REL).parent.mkdir(parents=True)
        (moved / DISPATCHER_REL).write_text(
            "impl App {\n    fn somewhere_else(&self) {}\n}\n", encoding="utf-8"
        )
        (moved / COMMANDS_DIR_REL).mkdir(parents=True)
        for index in range(MIN_SOURCE_FILES):
            (moved / COMMANDS_DIR_REL / f"module_{index}.rs").write_text(
                "fn handler() {}\n", encoding="utf-8"
            )
        _, gaps = command_surface(moved)
    check(
        "a moved dispatcher function is a gap, not a pass",
        any(DISPATCHER_FN in gap for gap in gaps),
        "; ".join(gaps),
    )

    # 8. An absent family directory is a gap.
    with tempfile.TemporaryDirectory() as tmp:
        absent = Path(tmp)
        (absent / DISPATCHER_REL).parent.mkdir(parents=True)
        (absent / DISPATCHER_REL).write_text(CLEAN_DISPATCHER, encoding="utf-8")
        _, gaps = command_surface(absent)
    check(
        "an absent family-module directory is a gap",
        any(COMMANDS_DIR_REL in gap for gap in gaps),
        "; ".join(gaps),
    )

    # 9. A family directory that has gone too small is a gap, not a pass: a
    #    glob that stops matching is the quiet form of the same defect.
    with tempfile.TemporaryDirectory() as tmp:
        small = Path(tmp)
        (small / DISPATCHER_REL).parent.mkdir(parents=True)
        (small / DISPATCHER_REL).write_text(CLEAN_DISPATCHER, encoding="utf-8")
        (small / COMMANDS_DIR_REL).mkdir(parents=True)
        (small / COMMANDS_DIR_REL / "one.rs").write_text("fn handler() {}\n", encoding="utf-8")
        _, gaps = command_surface(small)
    check(
        "a family directory below the floor is a gap",
        any("floor" in gap for gap in gaps),
        "; ".join(gaps),
    )

    # 10. The real dialog predicate covers every variant.
    real_dialog = (root / DIALOG_REL).read_text(encoding="utf-8")
    findings = dialog_predicate_gaps(real_dialog)
    check(
        "the real `Dialog::is_open` covers every variant",
        not findings,
        "; ".join(findings),
    )

    # 11. A variant dropped from the predicate is reported.
    stripped = real_dialog.replace("                | Self::PanelManager\n", "", 1)
    if stripped == real_dialog:
        stripped = real_dialog.replace("                | Self::Queue\n", "", 1)
    findings = dialog_predicate_gaps(stripped)
    check(
        "a dialog variant dropped from `is_open` is reported",
        len(findings) == 1,
        "; ".join(findings),
    )

    # 12. A synthetic enum with an unlisted variant is reported.
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

    # 13. The absent variant is not required to be listed.
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
        print(
            f"\nself-test FAILED: {failures} of {total} case(s) — "
            "the detector does not discriminate."
        )
        return 2

    print(f"\nself-test passed: {total} case(s).")
    return 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return run_self_test()

    findings = collect_findings()

    if findings:
        print(
            "slash-command argument contract violated.\n"
            "A built-in command's argument is extracted once, where the typed\n"
            "line is resolved to a registry entry. Code that reads\n"
            "`command_palette.query` re-derives the argument by hand, and the\n"
            "palette's query is the whole typed line — so the handler sees the\n"
            "command's own name inside its argument.\n"
            "`Dialog::is_open` must cover every variant that can be on screen.\n"
            "A finding that names `src/tui/app/mod.rs` or a missing module means\n"
            "the dispatch surface moved again; re-point the scan rather than\n"
            "deleting the check.\n"
        )
        for finding in findings:
            print(f"  {finding}")
        print(f"\n{len(findings)} violation(s) found.")
        return 1

    regions, _ = command_surface()
    print(
        "command argument contract check passed — "
        f"{len(regions)} dispatch region(s) read no palette query and "
        "`Dialog::is_open` covers every variant"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
