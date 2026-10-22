#!/usr/bin/env python3
"""Security test-filter execution control (C004 WP3).

A CI step that runs `cargo test --lib some_filter_` is **green when the filter
matches zero tests**: cargo prints `0 passed; 0 failed; ...; 42 filtered out`
and exits 0. The step looks like it qualified something, and it qualified
nothing. The same is true one level down of a filter that selects only
**self-skipping** tests — a test that returns early is reported by cargo as
`ok`, so a filter that picks only self-skipping tests is also green and also
qualified nothing.

This repository has already paid for that lesson once. `tests/linux_m004_process_ipc.rs`
carries the note:

    The hosted job that runs this probe used to filter on this test name
    before the test existed, so the step matched zero tests and stayed
    vacuously green.

Every security filter is now registered in `docs/security-test-filters.toml`,
and this control closes both classes:

* **statically** (this file, no runner needed) — the registered command is
  still the command the workflow runs, the filter still selects at least one
  test *declared in the source*, the step still runs through the counting
  wrapper, and a filter that selects a self-skipping test is required to run
  with `--nocapture` so the skip marker is observable at all;
* **at runtime** (`--run`) — the wrapper asserts the executed count is
  non-zero, that every registered test name actually reported `ok`, and that
  no self-skip marker was emitted.

Modes:
    python3 scripts/check_test_filter_execution.py             # static guard
    python3 scripts/check_test_filter_execution.py --self-test # non-vacuity
    python3 scripts/check_test_filter_execution.py --run -- <command...>

A workflow step uses the wrapper like this:

    run: |
      python3 scripts/check_test_filter_execution.py --run -- \
        cargo test --locked --lib protected_mount_ -- --nocapture

`--self-test` proves the guard is not vacuous. It injects defects into a copy
of the tree and requires the guard to reject each, and it drives the runtime
counter directly with a zero-test output, a self-skipping output and a
healthy output, so the counter is shown to be able to say "zero" and to see a
skip.
"""

from __future__ import annotations

import argparse
import hashlib
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST_PATH = Path("docs") / "security-test-filters.toml"

# A self-skip is a test that returns early instead of running its body. The
# repository uses two literal prefixes for them; both are recognised so a
# single-prefix detector cannot miss one class.
SELF_SKIP_PREFIXES = ("skipped: ", "skipping ")

# `test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 11 filtered out;`
CARGO_RESULT = re.compile(
    r"test result:\s*\w+\.\s*(?P<passed>\d+)\s+passed;\s*(?P<failed>\d+)\s+failed",
    re.IGNORECASE,
)
# `Summary [ 396.677s] 16029 tests run: 16029 passed, 8 skipped`
NEXTEST_SUMMARY = re.compile(
    r"Summary\s*\[[^\]]*\]\s*(?P<run>\d+)\s+tests run:\s*(?P<passed>\d+)\s+passed",
    re.IGNORECASE,
)
# libtest per-test witness: `test some::path ... ok`
LIBTEST_WITNESS = re.compile(r"^test\s+(?P<name>\S+)\s+\.\.\.\s+(?P<verdict>ok|FAILED|ignored)", re.MULTILINE)
# nextest per-test witness: `PASS [   0.010s] ( 8417/16029) codegg::some::path`
NEXTEST_WITNESS = re.compile(
    r"^\s*(?P<verdict>PASS|FAIL|SLOW|TIMEOUT)\s+\[[^\]]*\]\s*\(\s*\d+/\d+\)\s+(?P<name>\S+)",
    re.MULTILINE,
)

TEST_ATTRIBUTE = re.compile(r"^\s*#\[(?:tokio::)?test(?:\s*\([^)]*\))?\]\s*$")
FN_DECL = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)")
MOD_DECL = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{?\s*$")


# ---------------------------------------------------------------------------
# Registry
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class FilterEntry:
    """One registered security filter."""

    workflow: str
    job: str
    step: str
    command: str
    filter: str
    tests: tuple[str, ...]
    security: str

    @property
    def label(self) -> str:
        # The command is part of the label because one step can run more than
        # one filter (the protected-mount regression step runs two).
        return f"{self.workflow}::{self.job}::{self.step}::{self.command}"


def _split_list(raw: str) -> tuple[str, ...]:
    raw = raw.strip()
    if raw.startswith("[") and raw.endswith("]"):
        raw = raw[1:-1]
    items = []
    for part in raw.split(","):
        part = part.strip().strip('"').strip("'")
        if part:
            items.append(part)
    return tuple(items)


def parse_manifest(path: Path) -> tuple[list[FilterEntry], list[str]]:
    """Read the registry. Returns the entries and any parse problems."""
    problems: list[str] = []
    if not path.is_file():
        return [], [f"{path}: the security test-filter registry is missing"]

    text = path.read_text(encoding="utf-8")
    entries: list[FilterEntry] = []
    current: dict[str, str] = {}
    in_filter = False

    def flush() -> None:
        nonlocal current, in_filter
        if not in_filter:
            current = {}
            return
        missing = [
            key
            for key in ("workflow", "job", "step", "command", "filter", "security")
            if not current.get(key)
        ]
        if missing:
            problems.append(f"{path}: a [[filter]] entry is missing {', '.join(missing)}: {current!r}")
        else:
            tests = _split_list(current.get("tests", ""))
            if not tests:
                problems.append(f"{path}: the entry for {current.get('step')!r} names no test")
            else:
                entries.append(
                    FilterEntry(
                        workflow=current["workflow"],
                        job=current["job"],
                        step=current["step"],
                        command=current["command"],
                        filter=current["filter"],
                        tests=tests,
                        security=current["security"],
                    )
                )
        current = {}
        in_filter = False

    pending_key: str | None = None
    pending_items: list[str] = []

    for raw in text.splitlines():
        line = raw.strip()
        if line.startswith("[[filter]]"):
            flush()
            in_filter = True
            current = {}
            continue
        if line.startswith("[") and not line.startswith("[["):
            flush()
            continue
        if not in_filter or not line or line.startswith("#"):
            continue

        # A list value may span several lines: `tests = [` ... `]`.
        if pending_key is not None:
            if "]" in line:
                head, _, tail = line.partition("]")
                if head.strip():
                    pending_items.append(head.strip())
                remainder = tail.strip().rstrip(",").strip()
                if remainder:
                    pending_items.append(remainder)
                current[pending_key] = "[" + ", ".join(pending_items) + "]"
                pending_key = None
                pending_items = []
            else:
                pending_items.append(line.strip())
            continue

        if "=" not in line:
            continue
        key, _, value = line.partition("=")
        key = key.strip()
        value = value.strip()
        if value.startswith("["):
            if "]" in value:
                current[key] = value
            else:
                pending_key = key
                pending_items = []
                head = value[1:].strip()
                if head:
                    pending_items.append(head)
            continue
        current[key] = value.strip('"').strip("'")
    flush()
    return entries, problems


# ---------------------------------------------------------------------------
# Source inspection
# ---------------------------------------------------------------------------


def source_roots_for(command: str) -> list[str]:
    """The source files whose test names a command can select.

    Derived from the command rather than declared per entry, so the registry
    cannot point at the wrong file:

    * `--test NAME`  -> the integration test file `tests/NAME.rs`;
    * `-p PKG`       -> that package's `src` tree;
    * otherwise      -> the root package's `src` tree.

    `cargo test -p PKG <filter>` runs the package's library target, so a
    package-scoped command needs no `--lib` to resolve.
    """
    match = re.search(r"--test\s+([A-Za-z0-9_]+)", command)
    if match:
        return [f"tests/{match.group(1)}.rs"]
    package = re.search(r"-p\s+([A-Za-z0-9_]+)", command)
    if package:
        candidate = f"crates/{package.group(1)}/src"
        if (ROOT / candidate).is_dir():
            return [candidate]
    return ["src"]


def iter_source_files(root: Path, source_root: str) -> list[Path]:
    base = root / source_root
    if base.is_file():
        return [base]
    if not base.is_dir():
        return []
    return sorted(p for p in base.rglob("*.rs") if "target" not in p.parts)


def module_path_for(root: Path, path: Path, source_root: str) -> str:
    """The module path a source file contributes, by the Rust 2018 layout.

    `src/lsp/m004.rs` is `lsp::m004`, `src/lsp/mod.rs` is `lsp`, and
    `src/lib.rs` is the crate root. Cargo matches a filter against the *full*
    test path, so `lsp::m004::tests::constrained_lsp_process_is_owned_by_m004_scope`
    has to be reconstructible from the file plus its inline modules.
    """
    try:
        relative = path.relative_to(root / source_root)
    except ValueError:
        return ""
    parts = list(relative.parts)
    if parts and parts[-1] == "mod.rs":
        parts = parts[:-1]
    elif parts and parts[-1].endswith(".rs"):
        parts[-1] = parts[-1][:-3]
    return "::".join(parts)


# The registry has many entries that share a source root, so the source scan
# is memoised: without it the guard reads the whole `src/` tree once per
# entry, which is slow enough to matter in the quick gate.
#
# The memo key is the source root's **content**, not the tree's path. The
# self-test checks the same source under several directories, and keying by
# path would make every copy pay for a full re-read and re-parse.
_DECLARED_CACHE: dict[tuple[str, str], dict[str, str]] = {}
_SOURCE_TEXT_CACHE: dict[tuple[str, str], list[str]] = {}
_MARKER_NAMES_CACHE: dict[tuple[str, str], set[str]] = {}
_FINGERPRINT_CACHE: dict[tuple[str, str], str] = {}


def source_fingerprint(root: Path, source_root: str) -> str:
    """A content digest of a source root.

    Two trees with the same digest hold the same source, so they can share a
    parsed scan. The digest covers the file contents and their relative
    paths, so a renamed test or an edited file produces a different digest.
    """
    cache_key = (str(root), source_root)
    cached = _FINGERPRINT_CACHE.get(cache_key)
    if cached is not None:
        return cached
    digest = hashlib.sha256()
    for path in iter_source_files(root, source_root):
        try:
            relative = path.relative_to(root / source_root)
        except ValueError:
            relative = path
        digest.update(str(relative).encode("utf-8"))
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    cached = digest.hexdigest()
    _FINGERPRINT_CACHE[cache_key] = cached
    return cached


def source_texts(root: Path, source_root: str) -> list[str]:
    key = (str(root), source_root)
    cached = _SOURCE_TEXT_CACHE.get(key)
    if cached is None:
        cached = [
            path.read_text(encoding="utf-8", errors="replace")
            for path in iter_source_files(root, source_root)
        ]
        _SOURCE_TEXT_CACHE[key] = cached
    return cached


def declared_tests(root: Path, source_root: str) -> dict[str, str]:
    """Map every declared test function to its full module path."""
    key = (source_root, source_fingerprint(root, source_root))
    cached = _DECLARED_CACHE.get(key)
    if cached is not None:
        return cached
    found: dict[str, str] = {}
    for path in iter_source_files(root, source_root):
        text = path.read_text(encoding="utf-8", errors="replace")
        file_module = module_path_for(root, path, source_root)
        stack: list[tuple[str, int]] = []
        depth = 0
        pending_test_attribute = False
        for raw in text.splitlines():
            module = MOD_DECL.match(raw)
            if module and raw.rstrip().endswith("{"):
                stack.append((module.group(1), depth))
            opens = raw.count("{")
            closes = raw.count("}")
            depth += opens - closes
            while stack and depth <= stack[-1][1]:
                stack.pop()
            if TEST_ATTRIBUTE.match(raw):
                pending_test_attribute = True
                continue
            declaration = FN_DECL.match(raw)
            if declaration and pending_test_attribute:
                parts = [p for p in ([file_module] if file_module else [])]
                parts.extend(name for name, _ in stack)
                parts.append(declaration.group(1))
                found["::".join(parts)] = declaration.group(1)
                pending_test_attribute = False
    _DECLARED_CACHE[key] = found
    return found


def filter_matches(filter_text: str, full_path: str, function_name: str) -> bool:
    """Cargo matches a filter as a substring of the full test path.

    The function name is not consulted separately: a path-qualified filter
    such as `managed_process::windows::tests::restricted_` must not be
    widened into a bare-name match, or a test in another module whose name
    merely contains `restricted_` would be counted as selected when cargo
    would not run it.
    """
    return filter_text in full_path


def selected_tests(root: Path, entry: FilterEntry) -> dict[str, str]:
    selected: dict[str, str] = {}
    for source_root in source_roots_for(entry.command):
        for full_path, function_name in declared_tests(root, source_root).items():
            if filter_matches(entry.filter, full_path, function_name):
                selected[full_path] = function_name
    return selected


def marker_file_names(root: Path, source_root: str) -> set[str]:
    """The declared test names that appear in a file carrying a self-skip marker.

    The result is deliberately **conservative**: a name that merely appears in
    some marker-bearing file counts, even when the file that declares the
    test is a different one. Over-attributing here can only add a
    `--nocapture` requirement, which is the safe direction; narrowing it to
    the declaring file could drop a requirement the runtime counter depends
    on.

    The set is computed once per source root rather than once per registry
    entry: the registry has twenty entries and most of them share the root
    package's `src` tree, and scanning every name against every marker-bearing
    file per entry made the quick gate take the better part of a minute.
    """
    key = (source_root, source_fingerprint(root, source_root))
    cached = _MARKER_NAMES_CACHE.get(key)
    if cached is not None:
        return cached
    declared = declared_tests(root, source_root)
    distinct = sorted(set(declared.values()))
    names: set[str] = set()
    for text in source_texts(root, source_root):
        if not any(prefix in text for prefix in SELF_SKIP_PREFIXES):
            continue
        for name in distinct:
            if name in text:
                names.add(name)
    _MARKER_NAMES_CACHE[key] = names
    return names


def self_skipping_tests(root: Path, entry: FilterEntry) -> list[str]:
    """The selected tests whose own source contains a self-skip marker."""
    skipping: list[str] = []
    for source_root in source_roots_for(entry.command):
        names = marker_file_names(root, source_root)
        for full_path, function_name in declared_tests(root, source_root).items():
            if function_name not in names:
                continue
            if filter_matches(entry.filter, full_path, function_name):
                skipping.append(full_path)
    return sorted(set(skipping))


# ---------------------------------------------------------------------------
# Static checks
# ---------------------------------------------------------------------------


def folded_wrapper_runs(workflow_text: str) -> list[tuple[int, str]]:
    """Find `run:` plain scalars that YAML folds onto a single shell line.

    A plain (non-block) `run:` scalar that continues on a more indented
    following line is folded by YAML into one line. When the folded block
    carries the wrapper, the wrapper invocation, the `--` separator and the
    exit check are concatenated onto one shell line, so the step stops
    being the step the registry registered — the guard would then certify a
    command the runner never executes.
    """
    lines = workflow_text.split("\n")
    found: list[tuple[int, str]] = []
    index = 0
    while index < len(lines):
        line = lines[index]
        match = re.match(r"^(\s*)run:\s*(\S.*?)\s*$", line)
        if not match:
            index += 1
            continue
        indent, value = match.group(1), match.group(2)
        if value[:1] in ("|", ">"):
            # A block scalar keeps its own line structure.
            index += 1
            continue
        block = [line]
        cursor = index + 1
        while cursor < len(lines):
            candidate = lines[cursor]
            if candidate.strip() == "":
                break
            if len(candidate) - len(candidate.lstrip()) <= len(indent):
                break
            block.append(candidate)
            cursor += 1
        if len(block) > 1 and any("check_test_filter_execution.py" in item for item in block):
            found.append((index + 1, "\n".join(block)))
        index = max(cursor, index + 1)
    return found


def check_registry(root: Path) -> list[str]:
    problems: list[str] = []
    entries, manifest_problems = parse_manifest(root / MANIFEST_PATH)
    problems.extend(manifest_problems)
    if not entries:
        problems.append(f"{MANIFEST_PATH}: the registry selects no filters")
        return problems

    seen: set[str] = set()
    folded_reported: set[str] = set()
    for entry in entries:
        if entry.label in seen:
            problems.append(f"{entry.label}: the filter is registered more than once")
        seen.add(entry.label)

        workflow_text = (root / entry.workflow).read_text(encoding="utf-8") if (root / entry.workflow).is_file() else None
        if workflow_text is None:
            problems.append(f"{entry.workflow}: unreadable, so the filter cannot be checked")
            continue

        if entry.command not in workflow_text:
            problems.append(
                f"{entry.label}: the registered command is not in the workflow any more: {entry.command!r}"
            )
        if entry.filter not in entry.command:
            problems.append(
                f"{entry.label}: the registered filter {entry.filter!r} does not appear in its own command"
            )
        if "scripts/check_test_filter_execution.py --run" not in workflow_text:
            problems.append(
                f"{entry.workflow}: no step runs through `scripts/check_test_filter_execution.py --run`, "
                "so a filter that matches zero tests would still be green"
            )

        if entry.workflow not in folded_reported:
            folded_reported.add(entry.workflow)
            for line_number, block in folded_wrapper_runs(workflow_text):
                problems.append(
                    f"{entry.workflow}:{line_number}: the `run:` value is a plain scalar that YAML folds "
                    "onto one shell line, so the wrapper invocation and the command it guards would be "
                    "concatenated and the step would not run the registered command; use a block scalar "
                    f"(`run: |`)\n{block}"
                )

        selected = selected_tests(root, entry)
        if not selected:
            problems.append(
                f"{entry.label}: the filter {entry.filter!r} selects **zero** declared tests, so the "
                f"step would qualify nothing ({entry.security})"
            )
            continue

        missing = [name for name in entry.tests if not any(name in path for path in selected)]
        if missing:
            problems.append(
                f"{entry.label}: the registered test(s) {', '.join(missing[:3])} are not selected by the "
                f"filter {entry.filter!r}, so the registry and the source disagree"
            )

        skipping = self_skipping_tests(root, entry)
        if skipping and "--nocapture" not in entry.command:
            problems.append(
                f"{entry.label}: the filter selects self-skipping tests ({', '.join(skipping[:3])}) but "
                "its command does not pass `--nocapture`, so the skip marker cannot be observed"
            )

    return problems


# ---------------------------------------------------------------------------
# Runtime checks
# ---------------------------------------------------------------------------


def executed_count(output: str) -> int | None:
    """The number of tests the run actually executed.

    Cargo reports `N passed; M failed` — a self-skipping test is counted as
    `passed`, which is why the skip marker is checked separately. Nextest
    reports `N tests run: N passed`.
    """
    total = 0
    found = False
    for match in CARGO_RESULT.finditer(output):
        found = True
        total += int(match.group("passed")) + int(match.group("failed"))
    for match in NEXTEST_SUMMARY.finditer(output):
        found = True
        total = max(total, int(match.group("run")))
    return total if found else None


def self_skip_lines(output: str) -> list[str]:
    hits = []
    for line in output.splitlines():
        stripped = line.strip()
        if any(stripped.startswith(prefix) for prefix in SELF_SKIP_PREFIXES):
            hits.append(stripped)
    return hits


def witness_names(output: str) -> set[str]:
    """Every test the run reported on, by name."""
    names: set[str] = set()
    for match in LIBTEST_WITNESS.finditer(output):
        names.add(match.group("name"))
    for match in NEXTEST_WITNESS.finditer(output):
        names.add(match.group("name"))
    return names


def check_output(output: str, expected_tests: tuple[str, ...], observe_skips: bool) -> list[str]:
    """The runtime assertions a wrapper run must satisfy."""
    problems: list[str] = []

    count = executed_count(output)
    if count is None:
        problems.append(
            "the run printed no test-result line, so the executed count cannot be established — a step "
            "that reports nothing has qualified nothing"
        )
    elif count == 0:
        problems.append(
            "the filter matched **zero** tests, so the step qualified nothing; cargo reports this as a pass"
        )

    if expected_tests:
        witnesses = witness_names(output)
        if witnesses:
            missing = [name for name in expected_tests if not any(name in w for w in witnesses)]
            if missing:
                problems.append(
                    f"the run did not report {', '.join(missing[:3])} as executed; the filter did not "
                    "select the tests it names"
                )

    if observe_skips:
        markers = self_skip_lines(output)
        if markers:
            problems.append(
                f"the run emitted {len(markers)} self-skip marker(s); a self-skip is not a skip-free "
                f"run: {markers[0][:160]}"
            )
    return problems


def expectations_for(command: list[str], root: Path) -> tuple[str, ...]:
    """The registered test names for a command.

    The wrapper looks the command up in the registry rather than taking the
    expectations on the command line, so a step cannot quietly narrow what it
    claims to have executed. A command that is not registered is refused: only
    a registered security filter may run through the wrapper.
    """
    joined = " ".join(command)
    entries, _ = parse_manifest(root / MANIFEST_PATH)
    for entry in entries:
        if entry.command == joined:
            return entry.tests
    return ()


def run_wrapped(command: list[str], expected_tests: tuple[str, ...], root: Path) -> int:
    """Run a command and assert it executed the tests it names."""
    observe_skips = "--nocapture" in command
    completed = subprocess.run(command, capture_output=True, text=True)
    output = (completed.stdout or "") + (completed.stderr or "")
    sys.stdout.write(completed.stdout or "")
    sys.stderr.write(completed.stderr or "")

    problems: list[str] = []
    if completed.returncode != 0:
        problems.append(f"the command failed with exit status {completed.returncode}")
    problems.extend(check_output(output, expected_tests, observe_skips))

    if problems:
        for problem in problems:
            print(f"filter-execution control failed: {problem}", file=sys.stderr)
        return 1
    count = executed_count(output)
    print(
        f"filter-execution control passed: {count} test(s) executed, "
        f"{len(expected_tests) or 'no'} registered test(s) witnessed, no self-skip marker emitted",
        file=sys.stderr,
    )
    return 0


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------


def mutate(root: Path, relative: str, old: str, new: str) -> None:
    path = root / relative
    text = path.read_text(encoding="utf-8")
    assert old in text, f"fixture could not be injected: {old!r} not in {relative}"
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def mutate_all(root: Path, relative: str, old: str, new: str) -> None:
    """Replace every occurrence, not just the first.

    The guard checks the workflow as a whole, so dropping one wrapper
    invocation out of several in the same file would leave the string present
    and the defect would go unreported.
    """
    path = root / relative
    text = path.read_text(encoding="utf-8")
    assert old in text, f"fixture could not be injected: {old!r} not in {relative}"
    path.write_text(text.replace(old, new), encoding="utf-8")


def _fold_wrapper_run(work: Path, probe: FilterEntry) -> None:
    """Re-introduce the folded plain scalar that YAML collapses onto one line.

    This is the defect the guard's folded-scalar check exists for: a `run:`
    value that continues on a more indented source line is folded by YAML, so
    the wrapper invocation, the `--` separator and the exit check end up
    concatenated on one shell line and the step never runs the registered
    command.
    """
    path = work / probe.workflow
    lines = path.read_text(encoding="utf-8").split("\n")
    for index, line in enumerate(lines):
        stripped = line.strip()
        if not stripped.startswith("run:"):
            continue
        indent = len(line) - len(line.lstrip())
        value = stripped[len("run:") :].strip()
        if "check_test_filter_execution.py --run --" in value:
            head, _, tail = value.partition("--run -- ")
            lines[index] = " " * indent + "run: " + head + "--run --"
            lines.insert(index + 1, " " * (indent + 2) + tail)
            break
        if value in ("|", ">"):
            block: list[str] = []
            cursor = index + 1
            while cursor < len(lines):
                candidate = lines[cursor]
                if candidate.strip() == "":
                    break
                if len(candidate) - len(candidate.lstrip()) <= indent:
                    break
                block.append(candidate)
                cursor += 1
            if not any("check_test_filter_execution.py" in item for item in block):
                index += 1
                continue
            lines[index] = " " * indent + "run: " + block[0].strip()
            for offset, item in enumerate(block[1:]):
                lines[index + 1 + offset] = " " * (indent + 2) + item.strip()
            break
    else:
        raise AssertionError(f"no wrapper step found in {probe.workflow}")
    path.write_text("\n".join(lines), encoding="utf-8")


def self_test() -> int:
    """Prove the guard is not vacuous."""
    failures: list[str] = []
    case_count = 0

    with tempfile.TemporaryDirectory() as tmp:
        pristine = Path(tmp) / "pristine"
        _copy_inputs(ROOT, pristine)
        baseline = check_registry(pristine)
        if baseline:
            failures.append(
                "the guard reports failures on an unmodified tree, so its checks are not measuring "
                f"what they claim: {baseline}"
            )

        entries, _ = parse_manifest(pristine / MANIFEST_PATH)
        if not entries:
            failures.append("the registry is empty, so there is nothing to self-test against")
        else:
            probe = entries[0]
            # A second probe whose workflow keeps the wrapper in a block scalar,
            # so the folded-scalar check is proved against both shapes.
            block_probe = next(
                (entry for entry in entries if "windows-process-launcher" in entry.workflow),
                probe,
            )
            cases = [
                (
                    "the workflow step stops running through the counting wrapper",
                    lambda work: mutate_all(
                        work,
                        probe.workflow,
                        "scripts/check_test_filter_execution.py --run",
                        "cargo test --locked",
                    ),
                ),
                (
                    "the registered command is no longer the command the workflow runs",
                    lambda work: mutate(work, probe.workflow, probe.command, "cargo test --locked --lib no_such_filter_"),
                ),
                (
                    "the filter stops selecting any declared test",
                    lambda work: _rename_tests(work, probe),
                ),
                (
                    "the registry entry loses its security rationale",
                    lambda work: mutate(
                        work,
                        str(MANIFEST_PATH),
                        f'security = "{probe.security}"',
                        'security = ""',
                    ),
                ),
                (
                    "the registry names a test the filter does not select",
                    lambda work: mutate(
                        work,
                        str(MANIFEST_PATH),
                        f'tests = ["{probe.tests[0]}"',
                        'tests = ["no_such_registered_test_zzz"',
                    ),
                ),
                (
                    "the wrapper step's run value is a plain scalar that YAML folds onto one line",
                    lambda work: _fold_wrapper_run(work, probe),
                    "plain scalar",
                ),
                (
                    "a block-scalar wrapper step is folded back onto one shell line",
                    lambda work: _fold_wrapper_run(work, block_probe),
                    "plain scalar",
                ),
            ]

            for case in cases:
                name, inject = case[0], case[1]
                expected = case[2] if len(case) > 2 else ""
                case_count += 1
                work = Path(tmp) / name.replace(" ", "_").replace("/", "_")
                _copy_inputs(pristine, work)
                try:
                    inject(work)
                except AssertionError as exc:
                    failures.append(f"{name}: fixture could not be injected: {exc}")
                    continue
                reported = check_registry(work)
                if not reported:
                    failures.append(f"{name}: the guard did not reject the injected defect")
                elif expected and not any(expected in problem for problem in reported):
                    failures.append(
                        f"{name}: the guard rejected the injected defect, but not with the check that "
                        f"owns it (no problem mentioned {expected!r}): {reported}"
                    )
                shutil.rmtree(work, ignore_errors=True)

    # The runtime counter must be able to say "zero" and must be able to see a
    # self-skip. Without these two, a green step could mean a filter that
    # selected nothing.
    zero_output = (
        "running 0 tests\n"
        "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 42 filtered out; "
        "finished in 0.00s\n"
    )
    if not check_output(zero_output, (), observe_skips=True):
        failures.append("the runtime control accepted a zero-test run; it must reject it")

    skip_output = (
        "running 1 test\n"
        "test delegated_tool_program_child_carries_the_offline_ceiling_to_the_owner ... ok\n"
        "skipped: set CODEGG_RUN_M005_OFFLINE_NATIVE=1 to run the native delegated-child proof\n"
        "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 17 filtered out; "
        "finished in 0.01s\n"
    )
    if not check_output(skip_output, (), observe_skips=True):
        failures.append("the runtime control accepted a self-skipping run; it must reject it")

    skipping_class_b = (
        "running 1 test\n"
        "test some::probe ... ok\n"
        "skipping native M004 LSP probe: /usr/bin/python3 unavailable\n"
        "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; "
        "finished in 0.01s\n"
    )
    if not check_output(skipping_class_b, (), observe_skips=True):
        failures.append("the runtime control accepted a `skipping` marker; it must reject it")

    good_output = (
        "running 2 tests\n"
        "test a ... ok\n"
        "test b ... ok\n"
        "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 40 filtered out; "
        "finished in 0.01s\n"
    )
    if check_output(good_output, ("a",), observe_skips=True):
        failures.append("the runtime control rejected a healthy run; it must accept it")

    missing_witness = (
        "running 1 test\n"
        "test some::other ... ok\n"
        "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 40 filtered out; "
        "finished in 0.01s\n"
    )
    if not check_output(missing_witness, ("a",), observe_skips=True):
        failures.append("the runtime control accepted a run that did not execute the named test; it must reject it")

    if failures:
        for failure in failures:
            print(f"self-test failed: {failure}", file=sys.stderr)
        return 1
    print(
        f"test-filter-execution self-test passed: {case_count} injected defects were each rejected by "
        "the guard, and the runtime counter rejected a zero-test run, a self-skipping run and a run that "
        "did not execute the named test, and accepted a healthy one"
    )
    return 0


def _copy_inputs(source: Path, destination: Path) -> None:
    """Copy only the files the guard reads.

    The guard looks at the registry, the workflow files and the source trees
    the registered commands point at. Copying the whole repository six times
    per self-test would be slow enough to make the guard unpleasant to run,
    and would drag in `target/` and `node_modules/`.
    """
    destination.mkdir(parents=True, exist_ok=True)
    entries, _ = parse_manifest(source / MANIFEST_PATH)
    wanted: set[Path] = {MANIFEST_PATH}
    for entry in entries:
        wanted.add(Path(entry.workflow))
        for source_root in source_roots_for(entry.command):
            wanted.add(Path(source_root))
    for relative in wanted:
        origin = source / relative
        if origin.is_dir():
            shutil.copytree(
                origin,
                destination / relative,
                ignore=shutil.ignore_patterns("target", ".git"),
                dirs_exist_ok=True,
            )
        elif origin.is_file():
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(origin, target)


def _rename_tests(work: Path, probe: FilterEntry) -> None:
    """Rename every test function in the probe's source so the filter cannot match."""
    source_root = source_roots_for(probe.command)
    if not source_root:
        raise AssertionError("the probe command has no statically resolvable source")
    files = iter_source_files(work, source_root[0])
    if not files:
        raise AssertionError("the probe command's source tree is empty")
    path = files[0]
    text = path.read_text(encoding="utf-8")
    # The replacement name must not *contain* the original: a filter such as
    # `production_private_index_...` would still match a renamed
    # `zz_renamed_production_private_index_...` and the defect would go
    # unreported.
    renamed = re.sub(
        r"^(\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+)[A-Za-z_][A-Za-z0-9_]*",
        r"\1zz_renamed_test",
        text,
        flags=re.MULTILINE,
    )
    if renamed == text:
        raise AssertionError("no test function could be renamed")
    path.write_text(renamed, encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT, help="repository root to check")
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects each injected defect instead of passing vacuously",
    )
    parser.add_argument(
        "--run",
        action="store_true",
        help="run the given command and assert it executed the tests it names",
    )
    parser.add_argument(
        "--expect-tests",
        default="",
        help="comma-separated test names the run must have reported as executed",
    )
    parser.add_argument("command", nargs=argparse.REMAINDER, help="the command to run under --run")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    if args.run:
        command = [part for part in args.command if part != "--"]
        if not command:
            print("filter-execution control failed: --run needs a command", file=sys.stderr)
            return 1
        root = args.root.resolve()
        expected = _split_list(args.expect_tests)
        if not expected:
            expected = expectations_for(command, root)
            if not expected:
                print(
                    "filter-execution control failed: the command is not a registered security "
                    "filter, so nothing can be asserted about what it executed; register it in "
                    f"{MANIFEST_PATH}",
                    file=sys.stderr,
                )
                return 1
        return run_wrapped(command, expected, root)

    problems = check_registry(args.root.resolve())
    if problems:
        for problem in problems:
            print(f"test-filter-execution guard failed: {problem}", file=sys.stderr)
        return 1
    entries, _ = parse_manifest(args.root.resolve() / MANIFEST_PATH)
    print(f"test-filter-execution guard passed: {len(entries)} registered security filter(s) verified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
