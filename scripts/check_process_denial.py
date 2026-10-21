#!/usr/bin/env python3
"""Guard: every denied-process surface must keep its C005 pre-effect refusal.

WHAT THIS GUARD DOES
    M005 corrective C005 makes the restricted shared-checkout profile refuse
    every unqualified process surface *before* a process exists. This guard
    is the static backstop for that contract:

    1. Every root in ``docs/writer-disposition.toml`` whose disposition is
       ``denied-process`` must declare **how** it is denied: either a
       ``denial_site`` file plus the ``denial_markers`` that identify the
       refusal in that file, or an explicit ``denial_status = "open"`` with
       a recorded reason. A surface that silently lost its refusal fails
       the guard.
    2. The code's own capability table
       (``src/tool/process_capability.rs``) must agree with the manifest:
       every tool the table classifies as spawning a process or using a
       persistent one must be a declared ``denied-process`` root, and every
       declared ``denied-process`` model tool must be classified. A tool
       that starts a process and is not classified fails the guard.
    3. The C005 contract's own landmarks must exist: the core authority, the
       tool-layer contract, the broker's final dispatch, the scheduler's
       submission gate and the final-executor gate.

WHAT THIS GUARD DELIBERATELY DOES NOT DO
    It does not attempt to decide whether a given command is safe, and it
    does not grant any permission. A process's write set is not statically
    enumerable, so the contract is a refusal and not an analysis. The guard
    reasons about *declared enforcement sites and markers* -- never about
    command text.

    It also does not pretend that a declared marker proves the refusal is
    correct. The proof that a refusal actually happens is the fixture suite
    (``tests/process_denial_c005.rs``); this guard proves the refusal is
    still *wired*, which is the failure mode a static check can catch.

USAGE
    python3 scripts/check_process_denial.py            # check the tree
    python3 scripts/check_process_denial.py --self-test  # prove it bites
"""

from __future__ import annotations

import argparse
import re
import shutil
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST_PATH = ROOT / "docs" / "writer-disposition.toml"

# --- The C005 contract's landmarks -----------------------------------------

# Each landmark is a file plus the markers that must be present in it. A
# landmark is the *contract itself*: the authority, the policy handle, the
# refusal points. Losing one is not a style change; it is the contract
# disappearing.
CONTRACT_LANDMARKS: list[tuple[str, tuple[str, ...]]] = [
    (
        "crates/codegg-core/src/process_admission.rs",
        (
            "pub struct ProcessAdmissionService",
            "pub enum ProcessDenialReason",
            "pub struct PersistentProcessRegistry",
            "pub async fn admit_scope",
            "pub async fn preflight_reservation",
        ),
    ),
    (
        "src/tool/process_admission.rs",
        (
            "pub trait ProcessAdmissionPolicy",
            "pub async fn admit_process",
            "pub async fn admit_tool_process",
            "pub async fn emit_process_denial",
        ),
    ),
    (
        "src/tool/process_capability.rs",
        (
            "pub enum ToolProcessCapability",
            "pub fn process_capability",
        ),
    ),
    (
        "src/tool/broker.rs",
        ("c005-denial: broker-final-dispatch",),
    ),
    (
        "src/tool/mod.rs",
        ("c005-denial: tool-boundary",),
    ),
    (
        "src/managed_process.rs",
        ("c005-denial: managed-process-gate",),
    ),
    (
        "src/scheduler/submission.rs",
        ("c005-denial: scheduler-submission",),
    ),
    (
        "src/scheduler/executors.rs",
        (
            "c005-denial: managed-argv-executor",
            "c005-denial: test-job-executor",
            "c005-denial: python-job-executor",
        ),
    ),
    (
        "src/scheduler/tool_program_executor.rs",
        ("c005-denial: tool-program-executor",),
    ),
    (
        "src/interactive_process.rs",
        ("c005-denial: interactive-pty",),
    ),
    (
        "src/shell/daemon.rs",
        ("c005-denial: human-shell-gate",),
    ),
    (
        "src/shell/runtime.rs",
        ("c005-denial: shell-runtime",),
    ),
    (
        "src/tool/plugin.rs",
        ("c005-denial: process-backed-plugin",),
    ),
    (
        "src/tool/formatter.rs",
        ("c005-denial: formatter-command",),
    ),
    (
        "crates/codegg-core/src/audit.rs",
        ('"process_denial"',),
    ),
]

CAPABILITY_TABLE_PATH = "src/tool/process_capability.rs"

# The capability table's entries: a tool name and the shape of its
# effective runtime effect.
_TABLE_ENTRY = re.compile(
    r'\(\s*"(?P<name>[a-z_]+)"\s*,\s*'
    r"ToolProcessCapability::(?P<shape>Spawns|Delegates|UsesPersistentProcess|SubmitsProcessWork)"
)


def _normalize(text: str) -> str:
    """A marker compared against source that rustfmt may have reflowed.

    A marker like `("python_script", ToolProcessCapability::Spawns(...))`
    spans several lines once rustfmt has split the tuple. Comparing the
    *shape* rather than the layout keeps the manifest's markers stable
    across formatting changes, without weakening them: every token in the
    marker still has to be present, in the same order, in the same file.
    """
    collapsed = "".join(text.split())
    return collapsed.replace(",)", ")")


def read_manifest(path: Path) -> dict:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def capability_entries(source: str) -> list[tuple[str, str]]:
    """The code's declared tool capabilities, in declaration order."""
    entries: list[tuple[str, str]] = []
    for match in _TABLE_ENTRY.finditer(source):
        entries.append((match.group("name"), match.group("shape")))
    return entries


def check_manifest_declarations(manifest: dict, tree_root: Path = ROOT) -> list[str]:
    """Every denied-process root must say how it is denied."""
    problems: list[str] = []
    denied = [
        root
        for root in manifest.get("root", [])
        if root.get("disposition") == "denied-process"
    ]
    if not denied:
        problems.append("the manifest declares no denied-process root")
    for root in denied:
        ident = root.get("id", "<unnamed>")
        site = root.get("denial_site")
        markers = root.get("denial_markers")
        status = root.get("denial_status")
        if site is None and status is None:
            problems.append(
                f"entry '{ident}' is a denied-process root but declares neither a "
                "denial_site nor a denial_status"
            )
            continue
        if site is not None:
            if status is not None:
                problems.append(
                    f"entry '{ident}' declares both a denial_site and a denial_status"
                )
                continue
            if not markers:
                problems.append(
                    f"entry '{ident}' names a denial site but declares no denial_markers"
                )
                continue
            site_path = tree_root / site
            if not site_path.is_file():
                problems.append(
                    f"entry '{ident}' names a missing denial site '{site}'"
                )
                continue
            source = site_path.read_text(encoding="utf-8", errors="replace")
            for marker in (m.strip() for m in str(markers).split("|")):
                if not marker:
                    continue
                if _normalize(marker) not in _normalize(source):
                    problems.append(
                        f"entry '{ident}' lost its denial: '{marker}' is no longer "
                        f"present in '{site}'"
                    )
            if root.get("mediation") != "process-gate":
                problems.append(
                    f"entry '{ident}' is denied by a process gate but its mediation is "
                    f"'{root.get('mediation')}'"
                )
        else:
            if status != "open":
                problems.append(
                    f"entry '{ident}' declares an unknown denial_status '{status}'"
                )
            elif not root.get("note"):
                problems.append(
                    f"entry '{ident}' is an open denial without a recorded reason"
                )
    return problems


def check_capability_alignment(manifest: dict, source: str) -> list[str]:
    """The code's capability table must agree with the manifest."""
    problems: list[str] = []
    entries = capability_entries(source)
    if not entries:
        problems.append("the capability table declares no tool capability")
        return problems
    table = {name: shape for name, shape in entries}

    roots = manifest.get("root", [])
    def _capability_name(root: dict) -> str | None:
        # A direct tool is not registered under a model-facing name, so it
        # declares its capability name separately.
        return root.get("tool") or root.get("capability")

    denied_tools = {
        _capability_name(root): root
        for root in roots
        if root.get("disposition") == "denied-process"
        and root.get("surface") in ("model-tool", "direct-tool")
        and _capability_name(root)
    }
    any_tool_roots = {
        _capability_name(root): root
        for root in roots
        if root.get("surface") in ("model-tool", "direct-tool") and _capability_name(root)
    }

    for name, shape in table.items():
        if shape in ("Spawns", "UsesPersistentProcess"):
            if name not in denied_tools:
                problems.append(
                    f"capability table classifies '{name}' as {shape} but the "
                    "manifest does not declare it as a denied-process tool"
                )
        else:
            if name not in any_tool_roots:
                problems.append(
                    f"capability table classifies '{name}' as {shape} but the "
                    "manifest declares no such tool root"
                )

    for name, root in denied_tools.items():
        if name not in table:
            problems.append(
                f"denied-process tool '{name}' is missing from the capability table"
            )
        elif table[name] not in ("Spawns", "UsesPersistentProcess", "SubmitsProcessWork"):
            problems.append(
                f"denied-process tool '{name}' is classified as {table[name]}, which "
                "is not a process surface"
            )
    return problems


def check_contract_landmarks(tree_root: Path = ROOT) -> list[str]:
    """The C005 contract's own landmarks must exist."""
    problems: list[str] = []
    for relative, markers in CONTRACT_LANDMARKS:
        path = tree_root / relative
        if not path.is_file():
            problems.append(f"the C005 contract landmark '{relative}' is missing")
            continue
        source = path.read_text(encoding="utf-8", errors="replace")
        for marker in markers:
            if marker not in source:
                problems.append(
                    f"the C005 contract landmark '{relative}' lost '{marker}'"
                )
    return problems


def check_tree() -> list[str]:
    manifest = read_manifest(MANIFEST_PATH)
    problems: list[str] = []
    problems.extend(check_manifest_declarations(manifest, ROOT))
    capability_path = ROOT / CAPABILITY_TABLE_PATH
    if not capability_path.is_file():
        problems.append(f"the capability table '{CAPABILITY_TABLE_PATH}' is missing")
    else:
        problems.extend(
            check_capability_alignment(
                manifest, capability_path.read_text(encoding="utf-8", errors="replace")
            )
        )
    problems.extend(check_contract_landmarks())
    return problems


# --- Self-test -------------------------------------------------------------


def _run_against(tree: Path) -> list[str]:
    """Run the guard's checks against an alternate tree."""
    manifest_path = tree / "docs" / "writer-disposition.toml"
    problems: list[str] = []
    manifest = read_manifest(manifest_path)
    problems.extend(check_manifest_declarations(manifest, tree))
    capability_path = tree / CAPABILITY_TABLE_PATH
    if capability_path.is_file():
        problems.extend(
            check_capability_alignment(
                manifest, capability_path.read_text(encoding="utf-8", errors="replace")
            )
        )
    else:
        problems.append(f"the capability table '{CAPABILITY_TABLE_PATH}' is missing")
    problems.extend(check_contract_landmarks(tree))
    return problems


def _copy_tree(destination: Path) -> Path:
    """A writable copy of the parts of the tree the guard reads."""
    relatives = {"docs/writer-disposition.toml", CAPABILITY_TABLE_PATH}
    relatives.update(relative for relative, _ in CONTRACT_LANDMARKS)
    # Every declared denial site is read by the guard, so the copy must
    # carry it too.
    manifest = read_manifest(ROOT / "docs" / "writer-disposition.toml")
    for root in manifest.get("root", []):
        site = root.get("denial_site")
        if site:
            relatives.add(site)
    for relative in sorted(relatives):
        source = ROOT / relative
        if not source.is_file():
            continue
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    return destination


def self_test() -> int:
    failures: list[str] = []

    def case(name: str, mutate, expect_fragment: str) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tree = _copy_tree(Path(tmp))
            mutate(tree)
            problems = _run_against(tree)
            if not any(expect_fragment in problem for problem in problems):
                failures.append(
                    f"the '{name}' violation was not reported (expected a problem "
                    f"containing '{expect_fragment}')"
                )

    def drop_marker(relative: str, marker: str):
        def mutate(tree: Path) -> None:
            path = tree / relative
            text = path.read_text(encoding="utf-8")
            path.write_text(text.replace(marker, "removed-by-self-test"), encoding="utf-8")

        return mutate

    def drop_declaration(tree: Path) -> None:
        path = tree / "docs/writer-disposition.toml"
        text = path.read_text(encoding="utf-8")
        path.write_text(
            text.replace('denial_site = "src/tool/bash.rs"\n', "", 1), encoding="utf-8"
        )

    def drop_capability(tree: Path) -> None:
        path = tree / CAPABILITY_TABLE_PATH
        text = path.read_text(encoding="utf-8")
        path.write_text(
            text.replace('("bash", ToolProcessCapability::Spawns(ProcessKind::Command)),', "", 1),
            encoding="utf-8",
        )

    def missing_site(tree: Path) -> None:
        path = tree / "docs/writer-disposition.toml"
        text = path.read_text(encoding="utf-8")
        path.write_text(
            text.replace('denial_site = "src/tool/bash.rs"', 'denial_site = "src/tool/gone.rs"', 1),
            encoding="utf-8",
        )

    def drop_contract(tree: Path) -> None:
        path = tree / "src/tool/broker.rs"
        text = path.read_text(encoding="utf-8")
        path.write_text(
            text.replace("c005-denial: broker-final-dispatch", "removed-by-self-test"),
            encoding="utf-8",
        )

    case(
        "a denial marker was removed from its site",
        drop_marker("src/tool/bash.rs", 'admit_tool_process("bash"'),
        "lost its denial",
    )
    case(
        "a denied-process root lost its denial declaration",
        drop_declaration,
        "declares neither a denial_site nor a denial_status",
    )
    case(
        "a process-capable tool left the capability table",
        drop_capability,
        "missing from the capability table",
    )
    case(
        "a denial site names a file that does not exist",
        missing_site,
        "names a missing denial site",
    )
    case(
        "the broker's final dispatch lost its refusal",
        drop_contract,
        "lost 'c005-denial: broker-final-dispatch'",
    )

    if failures:
        for failure in failures:
            print(f"process-denial self-test FAIL: {failure}", file=sys.stderr)
        return 1
    print("process-denial self-test ok")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard fails on injected violations",
    )
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    problems = check_tree()
    if problems:
        for problem in problems:
            print(f"process-denial check: {problem}", file=sys.stderr)
        return 1
    manifest = read_manifest(MANIFEST_PATH)
    denied = [
        root
        for root in manifest.get("root", [])
        if root.get("disposition") == "denied-process"
    ]
    open_items = [root for root in denied if root.get("denial_status") == "open"]
    print(
        "process-denial check ok: "
        f"{len(denied)} denied-process roots, "
        f"{len(denied) - len(open_items)} wired, {len(open_items)} recorded open"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
