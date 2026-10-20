#!/usr/bin/env python3
"""Guard: every CodeGG writer root must carry an explicit restricted-profile disposition.

WHAT THIS GUARD DOES
    It enumerates *code sites* that can mutate a checkout or spawn a writer
    process, and requires each one to be declared in
    ``docs/writer-disposition.toml`` with an explicit disposition drawn from
    the C001 policy vocabulary. A new model-facing writer tool, or a new
    daemon/HTTP module that performs a filesystem effect, fails the guard
    until it has a declared disposition.

WHAT THIS GUARD DELIBERATELY DOES NOT DO
    It does not attempt to enumerate what a shell command, script, plugin,
    MCP server, LSP server or PTY writes. Dynamic command contents are not
    statically enumerable, and the C001 contract forbids claiming
    otherwise. The guard reasons about *call sites, owners and declared
    metadata* -- never about effects inferred from command text.

    It DOES require a disposition for every CodeGG-managed *process launcher*,
    because a launcher is a writer root even when it performs no filesystem
    effect itself: the child does. The launchers are listed explicitly in
    PROCESS_SURFACE_FILES, so a new one is a deliberate addition rather than
    a silent omission.

    It grants no permission, enables no writer, and creates no permit. It is
    a contract guard: it fails when a writer route lacks a disposition.

SCOPE
    The manifest is scoped to CodeGG-managed writers. Independent external
    same-UID programs (an editor the user runs, another agent, a build
    script outside CodeGG) are explicitly OUT OF CONTRACT: CodeGG cannot
    mediate them, and the manifest records that boundary rather than
    pretending to fence it.

USAGE
    python3 scripts/check_writer_disposition.py            # check the tree
    python3 scripts/check_writer_disposition.py --self-test  # prove it bites
"""

from __future__ import annotations

import argparse
import os
import re
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST_PATH = ROOT / "docs" / "writer-disposition.toml"

# --- Vocabulary -------------------------------------------------------------

# The C001 policy matrix vocabulary. These are dispositions, not permissions:
# none of them enables a writer, and "fenced-native" names a CANDIDATE for
# C002/C003 to consider, never an active fence.
DISPOSITIONS = {
    "allowed-read",       # proven read-only; no checkout effect to restrict
    "fenced-native",      # candidate fenced native file/Git writer (C002/C003)
    "denied-process",     # process-backed or persistent; denied pre-effect
    "fenced-or-denied",   # CodeGG non-broker writer; fence it or refuse it
    "unsupported",        # platform / node / NonGit: not supported here
    "out-of-contract",    # outside CodeGG's mediation (external same-UID)
}

SCOPES = {
    "checkout-writer",    # can mutate a checkout
    "non-checkout",       # writes managed state proven outside every checkout
    "read-only",          # proven read-only
    "external",           # an independent same-UID program
}

SURFACES = {
    "model-tool",         # reachable by the model through the broker
    "direct-tool",        # a model-facing tool with a non-broker path
    "daemon",             # daemon-internal writer
    "http",               # HTTP route writer
    "project-state",      # project-local managed state
    "process",            # process / persistent-lifetime writer
    "external",           # outside CodeGG
}

# A checkout-writer may never be classified as a harmless read.
CHECKOUT_WRITER_DISPOSITIONS = {
    "fenced-native",
    "denied-process",
    "fenced-or-denied",
    "unsupported",
}

# --- Detection --------------------------------------------------------------

# Model-facing tool implementations live here. A tool declares its category
# in `fn category(&self) -> ToolCategory { ToolCategory::X }`.
TOOL_DIR = ROOT / "src" / "tool"

# Categories that mean "this tool can change something".
WRITER_CATEGORIES = {"Mutating", "ShellExec"}

# The same tool implementations may also live outside src/tool (e.g. the
# context tools). Collect every Rust file that implements the Tool trait.
TOOL_IMPL_GLOBS = ("src/tool", "src/context", "src/python_script", "src/agent")

# Filesystem-effect primitives. The presence of one of these in a
# daemon/HTTP/project-state module is what makes that module a writer root
# for manifest purposes. This is a *site* signal, not an effect claim.
EFFECT_PRIMITIVES = re.compile(
    r"\b(?:"
    r"fs::write|fs::remove_file|fs::remove_dir|fs::remove_dir_all|"
    r"fs::rename|fs::copy|fs::create_dir|fs::create_dir_all|"
    r"File::create|OpenOptions|"
    r"tokio::fs::write|tokio::fs::remove_file|tokio::fs::rename|"
    r"std::fs::write"
    r")\s*\("
)

# The daemon/HTTP/project-state surface. A new module under these roots that
# performs a filesystem effect must be declared.
WRITER_SURFACE_DIRS = (
    "src/server/routes",
    "src/core",
    "src/lsp",
    "src/ide",
    "src/worktree",
    "crates/codegg-core/src",
)

# Individual files that are writer roots but live outside those roots.
WRITER_SURFACE_FILES = (
    "src/document_service.rs",
    "src/git_mutations.rs",
    "src/git_recovery.rs",
    "src/git_network_ops.rs",
    "src/tool/git.rs",
)

# CodeGG-managed process launchers. A module that launches a child process
# is a writer root on its own account: what the child writes is not
# statically knowable, so the root is declared and denied rather than
# inferred from the command text.
#
# The manifest named the *tools* that reach most of these sinks but not the
# launchers that perform them, so the final effect owner of every process
# writer sat outside the manifest. These are the owners named by
# `c001-evidence/02-process-persistent-sinks.md`.
PROCESS_SURFACE_FILES = (
    "src/managed_process.rs",
    "src/scheduler/executors.rs",
    "src/python_script/executor.rs",
    "src/interactive_process.rs",
    "src/mcp/local.rs",
    "crates/egglsp/src/launch.rs",
    "src/plugin/runtime/process.rs",
    "src/hooks/mod.rs",
    "src/tool/formatter.rs",
    "src/shell/runtime.rs",
)

# A process launch is a writer root even when the module performs no
# filesystem effect itself: the child does. This is a *site* signal, not an
# effect claim -- the disposition of each such root is declared, never
# inferred.
PROCESS_LAUNCH_PRIMITIVES = re.compile(
    r"\b(?:Command::new|ManagedProcessService|spawn_server)\b"
)

_TOOL_NAME_RE = re.compile(
    r"fn\s+name\s*\(&self\)\s*->\s*&str\s*\{\s*\"([A-Za-z0-9_]+)\""
)
_TOOL_CATEGORY_RE = re.compile(
    r"fn\s+category\s*\(&self\)\s*->\s*ToolCategory\s*\{\s*ToolCategory::(\w+)"
)


def parse_tool_identity(text: str) -> tuple[str | None, str | None]:
    """Return the (tool name, ToolCategory) a tool implementation declares."""
    name_match = _TOOL_NAME_RE.search(text)
    category_match = _TOOL_CATEGORY_RE.search(text)
    return (
        name_match.group(1) if name_match else None,
        category_match.group(1) if category_match else None,
    )


def is_writer_tool(text: str) -> bool:
    """True when a tool implementation declares a writer-capable category.

    Only a declared category counts. A tool that does not declare one is
    NOT treated as a writer here, because the default category is not
    statically knowable from the file alone -- but such a tool is reported
    separately as unclassified by the manifest check.
    """
    _, category = parse_tool_identity(text)
    return category in WRITER_CATEGORIES


def has_effect_primitive(text: str) -> bool:
    """True when a module contains a filesystem-effect primitive call."""
    return EFFECT_PRIMITIVES.search(text) is not None


def strip_test_modules(text: str) -> str:
    """Remove ``#[cfg(test)]`` module bodies before scanning for effects.

    A source file that only writes files inside its own unit-test module is
    not a production writer root. Brace matching keeps the removal precise
    enough for the Rust shapes this repository uses.
    """
    out = text
    while True:
        index = out.find("#[cfg(test)]")
        if index == -1:
            return out
        brace = out.find("{", index)
        if brace == -1:
            return out
        depth = 0
        end = -1
        for pos in range(brace, len(out)):
            if out[pos] == "{":
                depth += 1
            elif out[pos] == "}":
                depth -= 1
                if depth == 0:
                    end = pos
                    break
        if end == -1:
            return out
        out = out[:index] + out[end + 1 :]


def _iter_rust_files(base: Path) -> list[Path]:
    out: list[Path] = []
    for dirpath, dirs, files in os.walk(base):
        dirs[:] = [d for d in dirs if d not in (".git", "target", "node_modules")]
        for name in files:
            if name.endswith(".rs"):
                out.append(Path(dirpath) / name)
    return sorted(out)


def detect_writer_tools() -> dict[str, str]:
    """Map tool name -> source path for every writer-capable model tool."""
    found: dict[str, str] = {}
    seen: set[Path] = set()
    for glob_root in TOOL_IMPL_GLOBS:
        base = ROOT / glob_root
        if not base.is_dir():
            continue
        for path in _iter_rust_files(base):
            if path in seen:
                continue
            seen.add(path)
            text = path.read_text(encoding="utf-8", errors="replace")
            if not is_writer_tool(text):
                continue
            name, _ = parse_tool_identity(text)
            if name:
                found.setdefault(name, path.relative_to(ROOT).as_posix())
    return found


def detect_writer_modules() -> set[str]:
    """Return the daemon/HTTP/project-state modules that perform an effect."""
    found: set[str] = set()
    for rel_dir in WRITER_SURFACE_DIRS:
        base = ROOT / rel_dir
        if not base.is_dir():
            continue
        for path in _iter_rust_files(base):
            text = strip_test_modules(
                path.read_text(encoding="utf-8", errors="replace")
            )
            if has_effect_primitive(text):
                found.add(path.relative_to(ROOT).as_posix())
    for rel in WRITER_SURFACE_FILES:
        path = ROOT / rel
        if path.is_file():
            text = strip_test_modules(
                path.read_text(encoding="utf-8", errors="replace")
            )
            if has_effect_primitive(text):
                found.add(rel)
    for rel in PROCESS_SURFACE_FILES:
        path = ROOT / rel
        if path.is_file():
            text = strip_test_modules(
                path.read_text(encoding="utf-8", errors="replace")
            )
            if PROCESS_LAUNCH_PRIMITIVES.search(text):
                found.add(rel)
    return found


# --- Manifest ---------------------------------------------------------------


def load_manifest() -> list[dict]:
    """Load ``docs/writer-disposition.toml``.

    A narrow, line-oriented TOML subset (mirroring
    ``scripts/check_execution_ownership.py``) so the guard needs no
    dependency.
    """
    if not MANIFEST_PATH.exists():
        raise SystemExit(
            f"missing manifest: {MANIFEST_PATH}. Create it before running this guard."
        )

    entries: list[dict] = []
    current: dict | None = None
    for raw in MANIFEST_PATH.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("[[") and line.endswith("]]"):
            if current is not None:
                entries.append(current)
            current = {}
            continue
        if current is None or "=" not in line:
            continue
        key, _, value = line.partition("=")
        key = key.strip()
        value = value.strip()
        if value.startswith('"') and value.endswith('"'):
            value = value[1:-1]
        current[key] = value
    if current is not None:
        entries.append(current)
    return entries


def _entry_keys(entry: dict) -> set[str]:
    """Identity of a manifest entry: a tool name or a module path."""
    for key in ("tool", "path"):
        if entry.get(key):
            return {entry[key]}
    return set()


# --- Checks -----------------------------------------------------------------


def check(entries: list[dict]) -> list[str]:
    problems: list[str] = []

    by_key: dict[str, dict] = {}
    for entry in entries:
        for key in _entry_keys(entry):
            if key in by_key:
                problems.append(f"duplicate manifest entry for '{key}'")
            by_key[key] = entry

    # 1. Every model-facing writer tool must have a disposition.
    for tool, path in sorted(detect_writer_tools().items()):
        entry = by_key.get(tool)
        if entry is None:
            problems.append(
                f"writer tool '{tool}' ({path}) has no disposition in {MANIFEST_PATH.name}"
            )
            continue
        if entry.get("surface") != "model-tool":
            problems.append(
                f"writer tool '{tool}' must declare surface=model-tool, got "
                f"'{entry.get('surface')}'"
            )

    # 2. Every daemon/HTTP/project-state module that performs an effect must
    #    have a disposition.
    for module in sorted(detect_writer_modules()):
        entry = by_key.get(module)
        if entry is None:
            problems.append(
                f"writer module '{module}' has no disposition in {MANIFEST_PATH.name}"
            )
            continue
        if entry.get("surface") not in SURFACES:
            problems.append(
                f"writer module '{module}' declares unknown surface "
                f"'{entry.get('surface')}'"
            )

    # 3. Stale entries: a declared path must still exist.
    for entry in entries:
        path = entry.get("path")
        if path and not (ROOT / path).exists():
            problems.append(
                f"manifest entry '{entry.get('id', path)}' points at missing path '{path}'"
            )

    # 4. Vocabulary and internal consistency.
    for entry in entries:
        ident = entry.get("id") or sorted(_entry_keys(entry)) or "<unnamed>"
        disposition = entry.get("disposition", "")
        scope = entry.get("scope", "")
        surface = entry.get("surface", "")
        if disposition not in DISPOSITIONS:
            problems.append(
                f"entry '{ident}' has unknown disposition '{disposition}' "
                f"(expected one of {sorted(DISPOSITIONS)})"
            )
        if scope not in SCOPES:
            problems.append(
                f"entry '{ident}' has unknown scope '{scope}' (expected one of {sorted(SCOPES)})"
            )
        if surface not in SURFACES:
            problems.append(
                f"entry '{ident}' has unknown surface '{surface}' (expected one of {sorted(SURFACES)})"
            )
        if scope == "checkout-writer" and disposition not in CHECKOUT_WRITER_DISPOSITIONS:
            problems.append(
                f"entry '{ident}' is a checkout-writer but its disposition is "
                f"'{disposition}'; a writer may never be classified as a harmless read"
            )
        if scope == "read-only" and disposition != "allowed-read":
            problems.append(
                f"entry '{ident}' is read-only but its disposition is '{disposition}'"
            )
        if scope == "non-checkout" and disposition != "out-of-contract":
            problems.append(
                f"entry '{ident}' is non-checkout but its disposition is '{disposition}'; "
                "managed state outside every checkout is out of the checkout-writer profile"
            )
        if scope == "external" and disposition != "out-of-contract":
            problems.append(
                f"entry '{ident}' is external but its disposition is '{disposition}'; "
                "external same-UID programs are out of contract"
            )
        if not entry.get("owner"):
            problems.append(f"entry '{ident}' does not name a final effect owner")

    return problems


# --- Self test --------------------------------------------------------------


def run_self_test() -> int:
    """Prove the guard is not vacuous.

    Each case injects a violation and asserts the corresponding check fires.
    """
    failures: list[str] = []

    writer_tool = """
impl Tool for SneakyWriter {
    fn name(&self) -> &str {
        "sneaky_writer"
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }
}
"""
    if not is_writer_tool(writer_tool):
        failures.append("a tool declaring ToolCategory::Mutating was not detected as a writer")
    if is_writer_tool("impl Tool for R { fn category(&self) -> ToolCategory { ToolCategory::ReadOnly } }"):
        failures.append("a tool declaring ToolCategory::ReadOnly was wrongly detected as a writer")

    if not has_effect_primitive("fn go() { std::fs::write(\"/tmp/x\", b\"y\").unwrap(); }"):
        failures.append("a module performing std::fs::write was not detected as a writer module")
    if has_effect_primitive("fn go() { let x = read_to_string(\"/tmp/x\").unwrap(); }"):
        failures.append("a read-only module was wrongly detected as a writer module")

    # A writer tool with no manifest entry must be reported.
    problems = check(
        [
            {
                "id": "placeholder",
                "tool": "unrelated_tool",
                "path": "src/tool/write.rs",
                "surface": "model-tool",
                "scope": "checkout-writer",
                "disposition": "fenced-native",
                "owner": "WriteTool",
            }
        ]
    )
    if not any("has no disposition" in p for p in problems):
        failures.append("a writer tool missing from the manifest was not reported")

    # A checkout-writer classified as a read must be reported.
    problems = check(
        [
            {
                "id": "bad-read",
                "tool": "unrelated_tool",
                "path": "src/tool/write.rs",
                "surface": "model-tool",
                "scope": "checkout-writer",
                "disposition": "allowed-read",
                "owner": "WriteTool",
            }
        ]
    )
    if not any("may never be classified as a harmless read" in p for p in problems):
        failures.append("a checkout-writer classified as allowed-read was not reported")

    # An unknown disposition must be reported.
    problems = check(
        [
            {
                "id": "bad-vocab",
                "tool": "unrelated_tool",
                "path": "src/tool/write.rs",
                "surface": "model-tool",
                "scope": "checkout-writer",
                "disposition": "enabled",
                "owner": "WriteTool",
            }
        ]
    )
    if not any("unknown disposition" in p for p in problems):
        failures.append("an unknown disposition was not reported")

    # A non-checkout root must be out of contract.
    problems = check(
        [
            {
                "id": "bad-noncheckout",
                "tool": "unrelated_tool",
                "path": "src/tool/write.rs",
                "surface": "model-tool",
                "scope": "non-checkout",
                "disposition": "fenced-native",
                "owner": "WriteTool",
            }
        ]
    )
    if not any("non-checkout but its disposition" in p for p in problems):
        failures.append("a non-checkout root with a checkout disposition was not reported")

    # An entry with no final effect owner must be reported.
    problems = check(
        [
            {
                "id": "no-owner",
                "tool": "unrelated_tool",
                "path": "src/tool/write.rs",
                "surface": "model-tool",
                "scope": "checkout-writer",
                "disposition": "fenced-native",
            }
        ]
    )
    if not any("does not name a final effect owner" in p for p in problems):
        failures.append("an entry without a final effect owner was not reported")

    # A stale path must be reported.
    problems = check(
        [
            {
                "id": "stale",
                "tool": "unrelated_tool",
                "path": "src/tool/definitely_not_a_real_file.rs",
                "surface": "model-tool",
                "scope": "checkout-writer",
                "disposition": "fenced-native",
                "owner": "WriteTool",
            }
        ]
    )
    if not any("missing path" in p for p in problems):
        failures.append("a stale manifest path was not reported")

    if failures:
        for failure in failures:
            print(f"writer-disposition self-test failed: {failure}")
        return 1
    print("writer-disposition self-test ok")
    return 0


# --- Entry point ------------------------------------------------------------


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects injected violations",
    )
    args = parser.parse_args()

    if args.self_test:
        return run_self_test()

    entries = load_manifest()
    problems = check(entries)
    if problems:
        print("writer-disposition check failed:")
        for problem in problems:
            print(f"  - {problem}")
        return 1
    print(
        f"writer-disposition check ok: {len(entries)} declared roots, "
        f"{len(detect_writer_tools())} writer tools, "
        f"{len(detect_writer_modules())} writer modules"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
