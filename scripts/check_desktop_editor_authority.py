#!/usr/bin/env python3
"""Fail if the desktop editor holds a second copy of the document text.

Product contract
----------------

The editor's substrate is shared and already implemented: the frontend-neutral
``DocumentController`` owns the canonical text replica, and the daemon owns
the document. The desktop editor adds a *surface* over that substrate, and the
surface must not become a second owner of the text.

Three rules, each checked mechanically:

**H — the host serves a window, not a document.**
    The host's viewport payload is the one bounded slice of document text
    that may cross the bridge. It is cut from the controller's own snapshot
    through ``DocumentSnapshot::line_range`` and bounded by
    ``MAX_VIEWPORT_LINES``. The host holds no other text: no field named
    ``text``/``content``/``lines``/``body``/``buffer``/``rope``/``source``/
    ``document``, no other whole-document line array, no owned snapshot or
    rope, and no filesystem read of its own — the host must not serve a
    document the controller did not open.

**R — the renderer renders the window and nothing else.**
    The renderer holds the bounded slice the host sent, and no second copy.
    No state or field typed as a whole-document string or line array, no
    concatenation of the viewport into one document string, and no filesystem
    or network read of a local path: the renderer reads the document only
    through the host.

**C — the contract is one contract.**
    Every ``*View`` the host serves has a matching TypeScript interface, and
    the fields match field for field. The editor's DTO mirror is the
    ``bindings:check`` equivalent for the editor, because the shared
    ``scripts/check-bindings.mjs`` reads ``src-tauri/src/bridge.rs`` and
    ``src/bridge-types.ts`` only.

Non-vacuity
-----------

The guard asserts the declarations it reasons about still exist, so it cannot
pass by the editor being deleted, renamed away, or emptied. It also asserts
that it is wired into both automatic lanes: a guard that is not run
automatically protects nothing.

Usage::

    python3 scripts/check_desktop_editor_authority.py
    python3 scripts/check_desktop_editor_authority.py --self-test

The ``--self-test`` injects each defect class into a copy of the tree and
proves the guard rejects it, so the guard cannot be vacuous.

Exit code 1 if violations are found.
"""

from __future__ import annotations

import re
import shutil
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# The host's editor module. It is the desktop's half of the editor.
HOST_MODULE = Path("apps/desktop/src-tauri/src/document.rs")

# The renderer's editor layer.
RENDERER_DIR = Path("apps/desktop/src/editor")
CONTRACT_MODULE = RENDERER_DIR / "contract.ts"

# The panel that docks the editor, and the registry entry that names it.
PANEL_MODULE = Path("apps/desktop/src/panels/EditorPanel.tsx")
REGISTRY_MODULE = Path("apps/desktop/src/panels/registry.ts")

# The lanes this guard has to run in.
GUARD_REL = "scripts/check_desktop_editor_authority.py"
VERIFY_SH_REL = Path("scripts/verify.sh")
CI_WORKFLOW_REL = Path(".github/workflows/ci.yml")
GUARD_INVOCATION = re.compile(r"python3\s+" + re.escape(GUARD_REL))

RENDERER_SUFFIXES = (".ts", ".tsx")
TEST_SUFFIXES = (".test.ts", ".test.tsx", ".spec.ts", ".spec.tsx")

# The field names that would only make sense for a retained copy of the text.
# A field named `text` is a finding whatever its annotation: a bounded
# command name is not text, and this editor has no such field.
TEXT_FIELD_NAMES = (
    "text",
    "content",
    "lines",
    "body",
    "buffer",
    "rope",
    "source",
    "document",
)

# The one bounded slice of document text that may cross the bridge.
#
# It is recorded here rather than allowed by name, so a second slice — or a
# renamed one — is a finding rather than a silent second copy.
PERMITTED_HOST_FIELD = ("EditorViewportView", "visible_lines")
PERMITTED_RENDERER_FIELD = ("EditorViewportView", "visibleLines")

# Types that hold a whole document.
DOCUMENT_TYPES = ("DocumentBuffer", "DocumentSnapshot", "Rope")

# Materialized whole-document collections.
TEXT_CONTAINER_TYPES = ("Vec<String>", "Vec<Line>")

# Declarations that read or enumerate the filesystem. The editor must obtain
# every byte through the controller.
FS_PATTERNS = (
    ("std::fs", re.compile(r"\bstd::fs::")),
    ("tokio::fs", re.compile(r"\btokio::fs::")),
    ("read_to_string", re.compile(r"\bread_to_string\b")),
    ("read_to_end", re.compile(r"\bread_to_end\b")),
    ("read_dir", re.compile(r"\bread_dir\b")),
    ("File::open", re.compile(r"\bFile::open\b")),
    ("OpenOptions", re.compile(r"\bOpenOptions\b")),
)

# Buffer construction must stay inside the controller.
BUFFER_CONSTRUCTORS = (
    ("DocumentBuffer::", re.compile(r"\bDocumentBuffer::")),
    ("Rope::", re.compile(r"\bRope::")),
)

# The renderer's filesystem and network reads.
RENDERER_FS_PATTERNS = (
    ("fs-module", re.compile(r"""from\s+['"](?:node:)?fs(?:/promises)?['"]""")),
    ("fs-require", re.compile(r"""require\s*\(\s*['"](?:node:)?fs(?:/promises)?['"]""")),
    ("read-file", re.compile(r"\b(?:readFileSync|readFile)\s*\(")),
    ("fetch", re.compile(r"(?<![\w.$])fetch\s*\(")),
    ("xhr", re.compile(r"\bXMLHttpRequest\b")),
    ("websocket", re.compile(r"\bWebSocket\b")),
)

# The viewport's lines must not be concatenated into one document string.
CONCATENATION_CALL = re.compile(r"\.\s*(?:join|concat)\s*\(")
LINE_IDENTIFIER = re.compile(r"\b(?:visibleLines|lines)\b")


@dataclass(frozen=True)
class Finding:
    file: Path
    line_no: int
    line_text: str
    rule: str


# ── Reading a source file ─────────────────────────────────────────────────


def strip_comments(text: str) -> str:
    """Blank out the comments, keeping every offset.

    A comment that says "no filesystem path crosses the boundary" must not be
    read as a filesystem call, so string literals are tracked: a comment only
    begins outside a string, and every blanked character becomes a space, so
    the line numbering of the original survives.
    """
    out = list(text)
    index = 0
    length = len(text)
    quote: str | None = None
    while index < length:
        char = text[index]
        if quote is not None:
            if char == "\\":
                index += 2
                continue
            if char == quote:
                quote = None
            index += 1
            continue
        if char in "'\"`":
            quote = char
            index += 1
            continue
        if char == "/" and index + 1 < length:
            nxt = text[index + 1]
            if nxt == "/":
                cursor = index
                while cursor < length and text[cursor] != "\n":
                    out[cursor] = " "
                    cursor += 1
                index = cursor
                continue
            if nxt == "*":
                cursor = index
                while cursor < length and not (
                    text[cursor] == "*" and cursor + 1 < length and text[cursor + 1] == "/"
                ):
                    if text[cursor] != "\n":
                        out[cursor] = " "
                    cursor += 1
                for position in range(index, min(cursor + 2, length)):
                    if text[position] != "\n":
                        out[position] = " "
                index = cursor + 2
                continue
        index += 1
    return "".join(out)


def strip_rust_line_comment(line: str) -> str:
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


# ── The host's Rust fields ────────────────────────────────────────────────

# A `name: Type,` struct field or enum-variant field.
RUST_FIELD_RE = re.compile(
    r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?P<name>[a-z_][a-z0-9_]*)\s*:\s*(?P<type>[^,]+?),?\s*$"
)

# `struct Name {` / `enum Name {`.
RUST_TYPE_RE = re.compile(r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:struct|enum)\s+(?P<name>[A-Z]\w*)")

RUST_STRUCT_RE = re.compile(r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?struct\s+(?P<name>[A-Z]\w*)")


def scan_rust_types(path: Path) -> tuple[list[Finding], dict[str, list[str]]]:
    """Every field of every type body in a Rust module.

    Returns the findings the field rules raise and the field names of each
    type, so the contract check can compare them with the TypeScript side.
    """
    findings: list[Finding] = []
    fields: dict[str, list[str]] = {}
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()

    type_name: str | None = None
    type_depth: int | None = None
    depth = 0

    for line_no, raw in enumerate(lines, 1):
        code = strip_rust_line_comment(raw)
        in_type_body = type_depth is not None and depth > type_depth
        if in_type_body and type_name is not None:
            match = RUST_FIELD_RE.match(code)
            if match:
                name = match.group("name")
                field_type = match.group("type").strip()
                fields.setdefault(type_name, []).append(name)
                finding = check_rust_field(path, line_no, raw, type_name, name, field_type)
                if finding is not None:
                    findings.append(finding)

        if type_depth is None:
            declaration = RUST_TYPE_RE.match(code)
            if declaration and "{" in code:
                type_name = declaration.group("name")
                type_depth = depth
        depth += code.count("{") - code.count("}")
        if type_depth is not None and depth <= type_depth:
            type_depth = None
            type_name = None

    return findings, fields


def check_rust_field(
    path: Path,
    line_no: int,
    raw: str,
    type_name: str,
    name: str,
    field_type: str,
) -> Finding | None:
    """H — the host's field rules."""
    permitted = (type_name, name) == PERMITTED_HOST_FIELD
    if name in TEXT_FIELD_NAMES and not permitted:
        return Finding(path, line_no, raw.strip(), f"retained-text-field:{name}")
    if not permitted:
        for container in TEXT_CONTAINER_TYPES:
            # A leading word boundary only: the container ends in `>`, which
            # is not a word character, so a trailing `\b` would never match.
            if re.search(rf"\b{re.escape(container)}", field_type):
                return Finding(
                    path, line_no, raw.strip(), f"retained-text-container:{container}"
                )
    for document in DOCUMENT_TYPES:
        if not re.search(rf"\b{re.escape(document)}\b", field_type):
            continue
        return Finding(path, line_no, raw.strip(), f"retained-document-field:{document}")
    return None


def check_host_module(path: Path) -> list[Finding]:
    """H — the whole host module, not only its fields."""
    findings: list[Finding] = []
    text = path.read_text(encoding="utf-8")
    for line_no, raw in enumerate(text.splitlines(), 1):
        code = strip_rust_line_comment(raw)
        if not code.strip():
            continue
        for label, pattern in FS_PATTERNS:
            if pattern.search(code):
                findings.append(Finding(path, line_no, raw.strip(), f"filesystem:{label}"))
        for label, pattern in BUFFER_CONSTRUCTORS:
            if pattern.search(code):
                findings.append(Finding(path, line_no, raw.strip(), f"buffer-owner:{label}"))
    return findings


# ── The renderer's TypeScript fields ──────────────────────────────────────

# A `name: Type;` interface member.
TS_FIELD_RE = re.compile(
    r"^\s*(?:readonly\s+)?(?P<name>[A-Za-z_$][A-Za-z0-9_$]*)\??\s*:\s*(?P<type>[^;]+);\s*$"
)

# `interface Name {` / `type Name = {`.
TS_TYPE_RE = re.compile(
    r"^\s*(?:export\s+)?(?:interface|type)\s+(?P<name>[A-Z]\w*)\s*(?:=\s*)?\{"
)

# A `string[]` or `Array<string>` type.
TS_STRING_ARRAY = re.compile(r"\bstring\s*\[\s*\]|\bArray\s*<\s*string\s*>")


def scan_typescript_types(path: Path) -> tuple[list[Finding], dict[str, list[str]]]:
    """Every member of every interface or object type in a TypeScript module."""
    findings: list[Finding] = []
    fields: dict[str, list[str]] = {}
    code = strip_comments(path.read_text(encoding="utf-8"))

    type_name: str | None = None
    type_depth: int | None = None
    depth = 0

    for line_no, raw in enumerate(code.splitlines(), 1):
        in_type_body = type_depth is not None and depth > type_depth
        if in_type_body and type_name is not None:
            match = TS_FIELD_RE.match(raw)
            if match:
                name = match.group("name")
                field_type = match.group("type").strip()
                fields.setdefault(type_name, []).append(name)
                finding = check_ts_field(path, line_no, raw, type_name, name, field_type)
                if finding is not None:
                    findings.append(finding)

        if type_depth is None:
            declaration = TS_TYPE_RE.match(raw)
            if declaration:
                type_name = declaration.group("name")
                type_depth = depth
        depth += raw.count("{") - raw.count("}")
        if type_depth is not None and depth <= type_depth:
            type_depth = None
            type_name = None

    return findings, fields


def check_ts_field(
    path: Path,
    line_no: int,
    raw: str,
    type_name: str,
    name: str,
    field_type: str,
) -> Finding | None:
    """R — the renderer's field rules."""
    permitted = (type_name, name) == PERMITTED_RENDERER_FIELD
    is_text = bool(re.search(r"\bstring\b", field_type))
    if name in TEXT_FIELD_NAMES and is_text and not permitted:
        return Finding(path, line_no, raw.strip(), f"retained-text-field:{name}")
    if TS_STRING_ARRAY.search(field_type) and not permitted:
        return Finding(path, line_no, raw.strip(), "retained-text-array")
    return None


def check_renderer_module(path: Path) -> list[Finding]:
    """R — the whole renderer module, not only its fields."""
    findings: list[Finding] = []
    code = strip_comments(path.read_text(encoding="utf-8"))
    lines = code.splitlines()
    for line_no, raw in enumerate(lines, 1):
        if not raw.strip():
            continue
        for label, pattern in RENDERER_FS_PATTERNS:
            if pattern.search(raw):
                findings.append(Finding(path, line_no, raw.strip(), f"renderer-read:{label}"))
        for match in CONCATENATION_CALL.finditer(raw):
            window = raw[max(0, match.start() - 160) : match.start()]
            if LINE_IDENTIFIER.search(window):
                findings.append(
                    Finding(path, line_no, raw.strip(), "viewport-concatenated")
                )
    return findings


# ── The checks ────────────────────────────────────────────────────────────


def check_host(root: Path) -> list[Finding]:
    """H — the host serves a window, not a document."""
    path = root / HOST_MODULE
    if not path.is_file():
        return [Finding(path, 0, "file not found", "missing-file")]
    findings, _ = scan_rust_types(path)
    findings.extend(check_host_module(path))
    return findings


def check_renderer(root: Path) -> list[Finding]:
    """R — the renderer renders the window and nothing else."""
    findings: list[Finding] = []
    directory = root / RENDERER_DIR
    if not directory.is_dir():
        return [Finding(directory, 0, "directory not found", "missing-directory")]
    for path in sorted(directory.rglob("*")):
        if not path.is_file() or path.suffix not in RENDERER_SUFFIXES:
            continue
        if path.name.endswith(TEST_SUFFIXES):
            continue
        module_findings, _ = scan_typescript_types(path)
        findings.extend(module_findings)
        findings.extend(check_renderer_module(path))
    return findings


def to_camel(name: str) -> str:
    return re.sub(r"_([a-z])", lambda match: match.group(1).upper(), name)


def check_contract(root: Path) -> list[Finding]:
    """C — every host DTO has a matching TypeScript interface."""
    findings: list[Finding] = []
    host_path = root / HOST_MODULE
    contract_path = root / CONTRACT_MODULE
    if not host_path.is_file() or not contract_path.is_file():
        return [Finding(contract_path, 0, "file not found", "missing-file")]
    _, host_fields = scan_rust_types(host_path)
    _, contract_fields = scan_typescript_types(contract_path)
    for name, rust_names in host_fields.items():
        if not name.endswith("View"):
            continue
        ts_names = contract_fields.get(name)
        if ts_names is None:
            findings.append(
                Finding(contract_path, 0, f"the host's `{name}` has no contract", "contract-drift")
            )
            continue
        for rust_name in rust_names:
            if to_camel(rust_name) not in ts_names:
                findings.append(
                    Finding(
                        contract_path,
                        0,
                        f"`{name}.{rust_name}` is missing from the contract",
                        "contract-drift",
                    )
                )
        for ts_name in ts_names:
            if ts_name not in [to_camel(rust_name) for rust_name in rust_names]:
                findings.append(
                    Finding(
                        contract_path,
                        0,
                        f"`{name}.{ts_name}` is not served by the host",
                        "contract-drift",
                    )
                )
    return findings


def check_wiring(root: Path) -> list[Finding]:
    """W — the guard runs in both automatic lanes."""
    findings: list[Finding] = []
    verify_sh = root / VERIFY_SH_REL
    if not verify_sh.is_file():
        findings.append(
            Finding(verify_sh, 0, "the file that wires the guards cannot be read", "missing-file")
        )
    else:
        body = run_guards_body(verify_sh.read_text(encoding="utf-8"))
        if not GUARD_INVOCATION.search(body):
            findings.append(
                Finding(
                    verify_sh,
                    0,
                    f"`run_guards()` does not invoke `{GUARD_REL}`",
                    "guard-not-wired",
                )
            )
    ci_workflow = root / CI_WORKFLOW_REL
    if not ci_workflow.is_file():
        findings.append(
            Finding(ci_workflow, 0, "the CI workflow cannot be read", "missing-file")
        )
    else:
        if not GUARD_INVOCATION.search(ci_workflow.read_text(encoding="utf-8")):
            findings.append(
                Finding(
                    ci_workflow,
                    0,
                    f"the CI workflow does not invoke `{GUARD_REL}`",
                    "guard-not-wired",
                )
            )
    return findings


def run_guards_body(verify_sh: str) -> str:
    """The body of `run_guards()` in `scripts/verify.sh`.

    The guard list lives in that function and must not be duplicated
    elsewhere, so the wiring check reads the function itself.
    """
    lines = verify_sh.splitlines()
    collected: list[str] = []
    inside = False
    for line in lines:
        if not inside:
            if line.startswith("run_guards()"):
                inside = True
            continue
        if line.startswith("}"):
            break
        collected.append(line)
    return "\n".join(collected)


# ── Non-vacuity ───────────────────────────────────────────────────────────

REQUIRED_DECLARATIONS = (
    (HOST_MODULE, "pub struct DesktopDocumentSession"),
    (HOST_MODULE, "pub struct EditorViewportView"),
    (HOST_MODULE, "pub const MAX_VIEWPORT_LINES"),
    (HOST_MODULE, "pub struct UndoStack"),
    (HOST_MODULE, "line_range("),
    (HOST_MODULE, "read_bytes("),
    (CONTRACT_MODULE, "export interface EditorViewportView"),
    (CONTRACT_MODULE, "visibleLines"),
    (RENDERER_DIR / "session.ts", "export function useEditorSession"),
    (RENDERER_DIR / "EditorSurface.tsx", "export function EditorSurface"),
    (PANEL_MODULE, "export function EditorPanel"),
    (REGISTRY_MODULE, "editor:"),
)


def check_non_vacuous(root: Path) -> list[Finding]:
    """Assert the declarations the rules reason about still exist."""
    findings: list[Finding] = []
    for relative, declaration in REQUIRED_DECLARATIONS:
        path = root / relative
        if not path.is_file():
            findings.append(Finding(path, 0, f"{relative} is missing", "missing-file"))
            continue
        if declaration not in path.read_text(encoding="utf-8"):
            findings.append(
                Finding(
                    path,
                    0,
                    f"expected declaration `{declaration}` is absent",
                    "guard-would-be-vacuous",
                )
            )
    return findings


def collect_findings(root: Path) -> list[Finding]:
    findings: list[Finding] = []
    findings.extend(check_host(root))
    findings.extend(check_renderer(root))
    findings.extend(check_contract(root))
    findings.extend(check_wiring(root))
    findings.extend(check_non_vacuous(root))
    return findings


# ── The self-test ─────────────────────────────────────────────────────────

# Each defect class: a name, the rule it must trip, and the injection that
# produces it. The injection replaces the first occurrence of `anchor` with
# `defect`, so the defect lands in a real file rather than in a scratch one.
DEFECTS: tuple[tuple[str, str, str, str], ...] = (
    (
        "host-text-field",
        "retained-text-field:text",
        "pub struct EditorOpenView {\n    /// The workspace-relative path of the open document.\n    pub relative_path: String,",
        "pub struct EditorOpenView {\n    /// The workspace-relative path of the open document.\n    pub relative_path: String,\n    pub text: String,",
    ),
    (
        "host-whole-document-array",
        "retained-text-container:Vec<String>",
        "pub struct EditorStateView {\n    /// The presentation key of the controller's state.\n    pub state: String,",
        "pub struct EditorStateView {\n    /// The presentation key of the controller's state.\n    pub state: String,\n    pub whole: Vec<String>,",
    ),
    (
        "host-buffer-construction",
        "buffer-owner:DocumentBuffer::",
        "use serde::Serialize;",
        "use serde::Serialize;\n\nfn _self_test_defect() -> DocumentBuffer { DocumentBuffer::new() }",
    ),
    (
        "host-filesystem-read",
        "filesystem:std::fs",
        "use serde::Serialize;",
        "use serde::Serialize;\n\nfn _self_test_defect(path: &str) { let _ = std::fs::read_to_string(path); }",
    ),
    (
        "renderer-text-field",
        "retained-text-field:text",
        "export interface EditorOpenView {",
        "export interface EditorOpenView {\n  text: string;",
    ),
    (
        "renderer-whole-document-array",
        "retained-text-array",
        "export interface EditorOpenView {",
        "export interface EditorOpenView {\n  documentLines: string[];",
    ),
    (
        "renderer-viewport-concatenation",
        "viewport-concatenated",
        "export interface EditorOpenView {",
        "export interface EditorOpenView {\n  const whole: string = viewport.visibleLines.join('');",
    ),
    (
        "renderer-filesystem-read",
        "renderer-read:fs-module",
        "export interface EditorOpenView {",
        "export interface EditorOpenView {\n  import { readFileSync } from 'node:fs';",
    ),
    (
        "contract-drift",
        "contract-drift",
        "  /** The workspace-relative path of the open document. */\n  relativePath: string;",
        "  /** The workspace-relative path of the open document. */",
    ),
    (
        "unwired",
        "guard-not-wired",
        "",
        "",
    ),
    (
        "vacuous",
        "guard-would-be-vacuous",
        "pub const MAX_VIEWPORT_LINES: usize = 200;",
        "pub const _REMOVED: usize = 0;",
    ),
)


def _copy_tree(root: Path, destination: Path) -> None:
    """Copy the files the guard reads into a scratch tree."""
    for relative in (
        HOST_MODULE,
        CONTRACT_MODULE,
        RENDERER_DIR / "session.ts",
        RENDERER_DIR / "EditorSurface.tsx",
        RENDERER_DIR / "fixtures.ts",
        PANEL_MODULE,
        REGISTRY_MODULE,
        VERIFY_SH_REL,
        CI_WORKFLOW_REL,
    ):
        source = root / relative
        if not source.is_file():
            continue
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)


def _apply_defect(root: Path, relative: Path, anchor: str, defect: str) -> bool:
    path = root / relative
    if not path.is_file():
        return False
    text = path.read_text(encoding="utf-8")
    if anchor == "":
        # The "unwired" defect removes the guard's own invocation.
        text = re.sub(rf"^.*python3\s+{re.escape(GUARD_REL)}.*\n", "", text, flags=re.M)
        path.write_text(text, encoding="utf-8")
        return True
    if anchor not in text:
        return False
    path.write_text(text.replace(anchor, defect, 1), encoding="utf-8")
    return True


def self_test() -> int:
    """Inject each defect class and prove the guard rejects it."""
    failures: list[str] = []
    with tempfile.TemporaryDirectory() as scratch:
        base = Path(scratch)
        for name, rule, anchor, defect in DEFECTS:
            tree = base / name
            tree.mkdir(parents=True, exist_ok=True)
            _copy_tree(REPO_ROOT, tree)
            relative = VERIFY_SH_REL if name == "unwired" else (
                HOST_MODULE
                if name.startswith("host-")
                else (
                    CONTRACT_MODULE
                    if name in {"contract-drift", "renderer-text-field", "renderer-whole-document-array", "renderer-viewport-concatenation", "renderer-filesystem-read"}
                    else HOST_MODULE
                )
            )
            if not _apply_defect(tree, relative, anchor, defect):
                failures.append(f"{name}: the anchor for the injection was not found")
                continue
            findings = collect_findings(tree)
            if not any(finding.rule == rule for finding in findings):
                rules = sorted({finding.rule for finding in findings})
                failures.append(
                    f"{name}: the injected defect was not rejected (expected `{rule}`, saw {rules})"
                )
    if failures:
        print("desktop editor authority guard self-test FAILED.\n")
        for failure in failures:
            print(f"  {failure}")
        return 1
    print(
        f"desktop editor authority guard self-test passed — {len(DEFECTS)} injected "
        "defect class(es) rejected"
    )
    return 0


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return self_test()

    findings = collect_findings(REPO_ROOT)
    if findings:
        print(
            "Desktop editor text-authority violations found.\n"
            "M007-E requires the editor to render only from the\n"
            "DocumentController replica: the host may serve the bounded\n"
            "viewport window and nothing else, and the renderer may hold\n"
            "that window and no second copy of the text.\n"
        )
        for finding in findings:
            relative = finding.file.relative_to(REPO_ROOT)
            location = f"{relative}:{finding.line_no}" if finding.line_no else str(relative)
            print(f"  {location} [{finding.rule}] {finding.line_text}")
        print(f"\n{len(findings)} violation(s) found.")
        return 1

    print(
        "desktop editor authority guard passed — the host serves a bounded "
        "viewport window and the renderer holds no second copy of the text"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
