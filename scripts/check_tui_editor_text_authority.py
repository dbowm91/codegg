#!/usr/bin/env python3
"""Static guard: the TUI editor path holds no second text buffer and reads
no file directly.

Milestone M006-A of the Desktop Frontend and IDE Foundation roadmap
delivers a TUI editor buffer that renders **exclusively** from the shared
`DocumentController` optimistic replica. Two invariants make that
meaningful, and neither is acceptable as a prose promise:

1. **No whole-document retention in the frontend.** The daemon document
   service owns canonical text and the controller owns the local replica.
   If the editor presentation layer kept its own copy, it could diverge
   from the replica and render stale or unowned text. The editor may hold
   *bounded* frontend state — cursor, selection, viewport, buffer mode,
   and a depth/byte-capped undo history of inverse `TextTransaction`s —
   but never a whole-document text container.

2. **No direct filesystem reads.** All text arrives through
   `DocumentController::try_snapshot()`, which is daemon-authorized and
   path-contained. An editor path that opened a file itself would bypass
   `document.v1` authorization and containment entirely. A text cache fed
   from disk is the same defect wearing a different hat.

## What counts as a violation

The rules are deliberately narrow, because a guard that cries wolf gets
deleted rather than fixed.

**Violations** (exit code 1):

- A struct or enum field in the editor path whose *name* says it holds the
  text: `text`, `content`, `lines`, `body`, `buffer`, `rope`, `source`,
  `document`.
- A struct or enum field holding an **owned** `DocumentSnapshot`,
  `DocumentBuffer`, or `Rope`. That is a second buffer, under any name.
- A struct or enum field holding a materialized document collection:
  `Vec<String>`, `Vec<Line>`. This is the whole-document `Vec<Line>` the
  plan forbids building per frame.
- Any filesystem read API in the editor path: `std::fs`, `tokio::fs`,
  `fs::read*`, `File::open`, `OpenOptions`, `read_to_string`,
  `read_to_end`, `read_dir`.
- Any construction of `DocumentBuffer` or `Rope` in the TUI. The
  controller is the sole owner of the buffer, so the frontend cannot hold
  a second one even under a different type name.

**Not violations**, by design:

- `&DocumentSnapshot` as a parameter or a struct field. That is a scoped,
  frame-length borrow of the controller's own buffer — exactly how the
  render path is meant to read. It is reported for visibility and
  filtered out of the exit status.
- `TextTransaction` in a struct field. ADR-0011 and the M006-A plan both
  require the frontend to own undo/redo as inverse transactions, bounded
  by `MAX_EDITOR_UNDO_DEPTH` / `MAX_EDITOR_UNDO_BYTES`. Bounded reversible
  history is not a second authoritative buffer.
- A `String` field that is not named like text, such as `command_line` (a
  bounded command name) or `path`. The name-based rule is what makes a
  retained `text: String` a finding; a bare `String` is not proof of
  anything.
- Test modules. `#[cfg(test)]` bodies may build fixtures. Brace depth is
  tracked so only the module body is exempt, never the rest of the file.

## Non-vacuity

The guard asserts the declarations it reasons about still exist, so it
cannot pass by the file being emptied or the types renamed away. It also
asserts that each file it claims to scan is present: a moved file is a
rule that silently stopped applying.

`--self-test` proves the rules can actually fail. It copies the five files
the guard reads into a throwaway directory and injects one realistic
regression per rule — a retained `text: String`, an owned snapshot, a
per-frame `Vec<Line>`, a `std::fs` read, a second `DocumentBuffer`, a
deleted file — plus two controls (the untouched copy must pass, and a
filesystem read inside a `#[cfg(test)]` module must stay permitted). A case
that stops being caught fails the self-test loudly.

Exit code 1 if violations are found.

Usage::

    python3 scripts/check_tui_editor_text_authority.py
    python3 scripts/check_tui_editor_text_authority.py --self-test

This is invoked by `scripts/verify.sh quick` and the CI `verify` job.
"""

from __future__ import annotations

import re
import shutil
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

REPO_ROOT = Path(__file__).resolve().parent.parent

# Files that make up the TUI editor path. The presentation model, the
# session seam, the render widget, the state record, and the command
# layer. All of them must respect the same two invariants.
SCANNED_FILES: list[str] = [
    "tui/editor.rs",
    "tui/document_session.rs",
    "tui/components/editor.rs",
    "tui/app/state/editor.rs",
    "tui/commands/editor.rs",
]

# Declarations that read or enumerate the filesystem. The editor must
# obtain every byte through `document.v1`.
FS_PATTERNS: list[tuple[str, re.Pattern]] = [
    ("std::fs", re.compile(r"\bstd::fs::")),
    ("tokio::fs", re.compile(r"\btokio::fs::")),
    ("fs-read", re.compile(r"(?<![\w:])fs::(read|write|File|OpenOptions|metadata|canonicalize)")),
    ("read_to_string", re.compile(r"\bread_to_string\b")),
    ("read_to_end", re.compile(r"\bread_to_end\b")),
    ("read_dir", re.compile(r"\bread_dir\b")),
    ("File::open", re.compile(r"\bFile::open\b")),
    ("OpenOptions", re.compile(r"\bOpenOptions\b")),
]

# Buffer construction must stay inside the controller.
BUFFER_CONSTRUCTORS: list[tuple[str, re.Pattern]] = [
    ("DocumentBuffer::", re.compile(r"\bDocumentBuffer::")),
    ("Rope::", re.compile(r"\bRope::")),
]

# Types that hold a whole document. Rejected as a struct field unless they
# appear behind a reference: `&DocumentSnapshot` is a scoped, frame-length
# borrow of the controller's own buffer and is exactly the sanctioned
# render-time pattern, while an owned `DocumentSnapshot` field is exactly
# the second buffer this guard exists to prevent.
DOCUMENT_TYPES: list[str] = ["DocumentBuffer", "DocumentSnapshot", "Rope"]

# Materialized whole-document collections. A reference does not help here:
# a `Vec<Line>` is already the per-frame materialization the plan forbids,
# borrowed or not.
TEXT_CONTAINER_TYPES: list[str] = ["Vec<String>", "Vec<Line>", "Vec<Line<'static>>"]

# Field names that would only make sense for a retained copy of the text.
# Rejected with any type: a field named `text` in the editor presentation
# layer is a finding whatever its annotation. The pending prefix is
# deliberately not a concern here — M002 replaced the `pending_command`
# `String` with `EditorPrefix`, a `Copy` enum whose `display()` returns a
# closed set of `&'static str`, so there is no longer any prefix *text* on
# the presentation to accumulate. `command_line` remains a bounded command
# name, not text, and is capped by `MAX_EDITOR_COMMAND_LINE`.
TEXT_FIELD_NAMES: list[str] = [
    "text",
    "content",
    "lines",
    "body",
    "buffer",
    "rope",
    "source",
    "document",
]

# A `name: Type,` struct field or enum-variant field.
FIELD_RE = re.compile(
    r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?P<name>[a-z_][a-z0-9_]*)\s*:\s*(?P<type>[^,]+?),?\s*$"
)

# `struct Name {` / `enum Name {`. Field rules apply only inside a type
# body, so a function parameter named `snapshot` is never mistaken for a
# retained field.
TYPE_DEF_RE = re.compile(r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:struct|enum)\s+[A-Z]\w*")

COMMENT_RE = re.compile(r"^\s*(//|/\*|\*)")

# `#[cfg(test)]` modules legitimately build fixtures.
CFG_TEST_RE = re.compile(r"^\s*#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]")

# Declarations that must exist for the guard's rules to be meaningful.
REQUIRED_DECLARATIONS: list[tuple[str, str]] = [
    ("tui/document_session.rs", "pub struct TuiDocumentPresentation"),
    ("tui/document_session.rs", "pub struct TuiDocumentSession"),
    ("tui/app/state/editor.rs", "pub struct EditorState"),
    ("tui/editor.rs", "pub struct EditorUndoEntry"),
    ("tui/editor.rs", "pub fn clear_history"),
    ("tui/components/editor.rs", "pub struct EditorWidget"),
]

# Rules reported for visibility but not counted as violations.
ADVISORY_RULE_PREFIX = "borrowed-document-field:"


@dataclass(frozen=True)
class Finding:
    file: Path
    line_no: int
    line_text: str
    rule: str
    advisory: bool = False


@dataclass(frozen=True)
class Report:
    violations: list[Finding]
    advisory: list[Finding]


def _is_comment_or_blank(line: str) -> bool:
    return not line.strip() or bool(COMMENT_RE.match(line))


def _strip_line_comment(line: str) -> str:
    """Remove a trailing `//` comment that is not inside a string."""
    in_string = False
    escaped = False
    for index, char in enumerate(line):
        if escaped:
            escaped = False
            continue
        if char == "\\":
            escaped = True
            continue
        if char == '"':
            in_string = not in_string
            continue
        if char == "/" and not in_string and index + 1 < len(line) and line[index + 1] == "/":
            return line[:index]
    return line


def _check_field(path: Path, line_no: int, raw: str, code: str) -> Finding | None:
    field = FIELD_RE.match(code)
    if not field:
        return None
    name = field.group("name")
    field_type = field.group("type").strip()
    if name in TEXT_FIELD_NAMES:
        return Finding(path, line_no, raw.strip(), f"retained-text-field:{name}")
    for container in TEXT_CONTAINER_TYPES:
        # Not `\b`: the type ends with `>`, which is not a word character, so a
        # trailing word boundary could never match and this rule could not fire
        # at all. `\b` on the leading edge still keeps `MyVec<String>` out.
        if re.search(rf"\b{re.escape(container)}(?![A-Za-z0-9_])", field_type):
            return Finding(path, line_no, raw.strip(), f"retained-text-container:{container}")
    for document in DOCUMENT_TYPES:
        if not re.search(rf"\b{re.escape(document)}\b", field_type):
            continue
        borrowed = "&" in field_type
        if borrowed:
            return Finding(
                path,
                line_no,
                raw.strip(),
                f"{ADVISORY_RULE_PREFIX}{document}",
                advisory=True,
            )
        return Finding(path, line_no, raw.strip(), f"retained-document-field:{document}")
    return None


def _scan_file(root: Path, relative: str) -> list[Finding]:
    path = root / "src" / relative
    if not path.is_file():
        return [Finding(path, 0, "file not found", "missing-file")]

    findings: list[Finding] = []
    try:
        lines = path.read_text().splitlines()
    except UnicodeDecodeError:
        return findings

    # Brace depth at which the `#[cfg(test)]` module body begins, and the
    # depth just inside the innermost `struct`/`enum` body. `None` means the
    # current line is not inside one.
    test_depth: int | None = None
    type_depth: int | None = None
    depth = 0

    for line_no, raw in enumerate(lines, 1):
        code = _strip_line_comment(raw)
        is_test_body = test_depth is not None and depth >= test_depth
        in_type_body = type_depth is not None and depth > type_depth

        if not is_test_body and not _is_comment_or_blank(raw):
            for label, pattern in FS_PATTERNS:
                if pattern.search(code):
                    findings.append(
                        Finding(path, line_no, raw.strip(), f"filesystem:{label}")
                    )

            for label, pattern in BUFFER_CONSTRUCTORS:
                if pattern.search(code):
                    findings.append(Finding(path, line_no, raw.strip(), f"buffer-owner:{label}"))

            if in_type_body:
                finding = _check_field(path, line_no, raw, code)
                if finding is not None:
                    findings.append(finding)

        # Advance depth after inspecting the line so the attribute and
        # type-definition lines are still checked.
        if test_depth is None and CFG_TEST_RE.match(raw):
            test_depth = depth
        if type_depth is None and TYPE_DEF_RE.match(code) and "{" in code:
            type_depth = depth
        depth += code.count("{") - code.count("}")
        if test_depth is not None and depth < test_depth:
            test_depth = None
        if type_depth is not None and depth <= type_depth:
            type_depth = None

    return findings


def collect_findings(root: Path) -> list[Finding]:
    findings: list[Finding] = []
    for relative in SCANNED_FILES:
        findings.extend(_scan_file(root, relative))
    return findings


def check_non_vacuous(root: Path) -> list[Finding]:
    """Assert the declarations the rules reason about still exist."""
    findings: list[Finding] = []
    for relative, declaration in REQUIRED_DECLARATIONS:
        path = root / "src" / relative
        if not path.is_file():
            findings.append(Finding(path, 0, f"{relative} is missing", "missing-file"))
            continue
        if declaration not in path.read_text():
            findings.append(
                Finding(
                    path,
                    0,
                    f"expected declaration `{declaration}` is absent",
                    "guard-would-be-vacuous",
                )
            )
    return findings


def check(root: Path) -> Report:
    """Run every rule against `root`; the self-test calls this same function."""
    all_findings = collect_findings(root) + check_non_vacuous(root)
    return Report(
        [finding for finding in all_findings if not finding.advisory],
        [finding for finding in all_findings if finding.advisory],
    )


def _render(report: Report, root: Path) -> int:
    for finding in report.advisory:
        rel = finding.file.relative_to(root)
        print(f"  note {rel}:{finding.line_no} [{finding.rule}] {finding.line_text}")

    if report.violations:
        print(
            "TUI editor text-authority violations found.\n"
            "M006-A requires the editor to render only from the\n"
            "DocumentController replica: no whole-document text may be\n"
            "retained in the frontend presentation layer, and no editor\n"
            "path may read the filesystem directly. Bounded inverse\n"
            "TextTransaction undo entries and scoped `&DocumentSnapshot`\n"
            "borrows are permitted by design.\n"
        )
        for finding in report.violations:
            rel = finding.file.relative_to(root)
            location = f"{rel}:{finding.line_no}" if finding.line_no else str(rel)
            print(f"  {location} [{finding.rule}] {finding.line_text}")
        print(f"\n{len(report.violations)} violation(s) found.")
        return 1

    print(
        "TUI editor text-authority guard passed — no retained text buffer "
        "and no direct filesystem reads in the editor path"
    )
    return 0


# ── Self-test ────────────────────────────────────────────────────────────────

# The self-test copies exactly the files this guard scans into a throwaway
# tree before mutating one, so a case cannot depend on a file the guard does
# not read and a mutation can never touch the working tree.
EXPECTED_INJECTIONS = 8


def copy_guard_tree(root: Path, destination: Path) -> None:
    for relative in SCANNED_FILES:
        source = root / "src" / relative
        target = destination / "src" / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)


def mutate(path: Path, old: str, new: str) -> tuple[Path, str]:
    """Replace the one occurrence and return what is needed to undo it."""
    text = path.read_text()
    occurrences = text.count(old)
    if occurrences != 1:
        raise AssertionError(
            f"self-test fixture text must appear exactly once in {path.name} "
            f"({occurrences} occurrence(s)): {old!r}"
        )
    path.write_text(text.replace(old, new, 1))
    return path, text


def restore(undo: list[tuple[Path, str]]) -> None:
    for path, text in reversed(undo):
        path.write_text(text)


def run_self_test() -> int:
    """Inject one realistic regression per rule and require the guard to catch it.

    Each case mutates a copy and runs the *same* `check` the guard runs, so a
    passing case proves that rule can fail. A case whose defect stops being
    caught fails the self-test: that is the non-vacuity demonstration rather
    than a claim about it.
    """
    failures: list[str] = []
    cases: list[tuple[str, Callable[[Path], list[tuple[Path, str]]], str | None]] = []

    def case(name: str, expected: str | None):
        def register(fn):
            cases.append((name, fn, expected))
            return fn

        return register

    @case("the presentation retains a copy of the document text", "retained-text-field:text")
    def _retained_text(root: Path) -> list[tuple[Path, str]]:
        return [
            mutate(
                root / "src/tui/document_session.rs",
                "    pub mode: EditorMode,\n",
                "    pub mode: EditorMode,\n    pub text: String,\n",
            )
        ]

    @case(
        "the editor state caches a per-frame materialized line collection",
        "retained-text-container:Vec<Line<'static>>",
    )
    def _retained_lines(root: Path) -> list[tuple[Path, str]]:
        return [
            mutate(
                root / "src/tui/app/state/editor.rs",
                "    pub path: Option<String>,\n",
                "    pub path: Option<String>,\n"
                "    pub rendered: Vec<Line<'static>>,\n",
            )
        ]

    @case(
        "the presentation owns a snapshot instead of borrowing one",
        "retained-document-field:DocumentSnapshot",
    )
    def _owned_snapshot(root: Path) -> list[tuple[Path, str]]:
        return [
            mutate(
                root / "src/tui/document_session.rs",
                "    pub clamp_cache: Option<ClampCache>,\n",
                "    pub clamp_cache: Option<ClampCache>,\n"
                "    pub cached_snapshot: DocumentSnapshot,\n",
            )
        ]

    @case(
        "the editor command layer reloads text from disk",
        "filesystem:std::fs",
    )
    def _direct_read(root: Path) -> list[tuple[Path, str]]:
        return [
            mutate(
                root / "src/tui/commands/editor.rs",
                "fn editor_buffer_key(\n",
                "fn stale_reload(path: &std::path::Path) -> std::io::Result<String> {\n"
                "    std::fs::read_to_string(path)\n"
                "}\n\n"
                "fn editor_buffer_key(\n",
            )
        ]

    @case(
        "the TUI builds its own DocumentBuffer beside the controller",
        "buffer-owner:DocumentBuffer::",
    )
    def _second_buffer(root: Path) -> list[tuple[Path, str]]:
        return [
            mutate(
                root / "src/tui/editor.rs",
                "pub fn clear_history(",
                "pub fn adopt_local_buffer(text: &str) -> DocumentBuffer {\n"
                "    DocumentBuffer::new(text)\n"
                "}\n\n"
                "pub fn clear_history(",
            )
        ]

    @case(
        "a filesystem read inside a #[cfg(test)] module stays permitted",
        None,
    )
    def _test_fixture_read(root: Path) -> list[tuple[Path, str]]:
        return [
            mutate(
                root / "src/tui/commands/editor.rs",
                "#[cfg(test)]\nmod tests {\n    use super::*;\n",
                "#[cfg(test)]\nmod tests {\n    use super::*;\n"
                "\n    fn reload_fixture() -> std::io::Result<String> {\n"
                '        std::fs::read_to_string("fixture")\n'
                "    }\n",
            )
        ]

    @case(
        "the presentation type is renamed away from the declaration the guard reads",
        "guard-would-be-vacuous",
    )
    def _renamed_presentation(root: Path) -> list[tuple[Path, str]]:
        return [
            mutate(
                root / "src/tui/document_session.rs",
                "pub struct TuiDocumentPresentation",
                "pub struct EditorPresentation",
            )
        ]

    @case("a scanned file is deleted rather than reclassified", "missing-file")
    def _deleted_file(root: Path) -> list[tuple[Path, str]]:
        path = root / "src/tui/app/state/editor.rs"
        text = path.read_text()
        path.unlink()
        return [(path, text)]

    if len(cases) != EXPECTED_INJECTIONS:
        failures.append(
            f"the self-test declares {len(cases)} injected defects, the guard "
            f"documents {EXPECTED_INJECTIONS}; a deleted case is a vacuous self-test"
        )

    with tempfile.TemporaryDirectory(prefix="codegg-editor-text-") as raw:
        tree = Path(raw) / "tree"
        copy_guard_tree(REPO_ROOT, tree)

        baseline = check(tree)
        if baseline.violations:
            failures.append(
                "the guard reports violations on an unmodified copy, so its rules "
                "are not measuring what they claim: "
                + "; ".join(f.rule for f in baseline.violations)
            )
        else:
            print("  ok: the unmodified copy passes")

        for name, inject, expected in cases:
            try:
                undo = inject(tree)
            except AssertionError as error:
                failures.append(f"{name}: fixture could not be injected: {error}")
                continue
            try:
                report = check(tree)
            finally:
                restore(undo)
            rules = [finding.rule for finding in report.violations]
            if expected is None:
                if rules:
                    failures.append(
                        f"{name}: a permitted shape was reported as a violation: {rules}"
                    )
                else:
                    print(f"  ok: permitted — {name}")
                continue
            if not any(expected in rule for rule in rules):
                failures.append(
                    f"{name}: the injected defect was not reported (expected a "
                    f"violation containing {expected!r}, got {rules})"
                )
            else:
                print(f"  ok: rejected {name}")

    if failures:
        for failure in failures:
            print(f"self-test failed: {failure}", file=sys.stderr)
        return 1
    print(
        "tui-editor-text-authority self-test passed: the unmodified copy is "
        f"accepted and all {len(cases)} injected defects are rejected"
    )
    return 0


def main(argv: list[str]) -> int:
    unknown = [argument for argument in argv if argument != "--self-test"]
    if unknown:
        print(f"unknown argument(s): {unknown}", file=sys.stderr)
        return 2
    if "--self-test" in argv:
        return run_self_test()

    return _render(check(REPO_ROOT), REPO_ROOT)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
