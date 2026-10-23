#!/usr/bin/env python3
"""Guard: the in-code comment corpus must not re-inflate, and a comment that
names a path must name one that exists.

WHAT THIS GUARD DOES
    It measures the comment corpus of the Rust sources in `src/`, `crates/`
    and `tests/` with the shared Rust-aware scanner
    (`scripts/rust_test_scan.py`), and it enforces four rules against the
    budget recorded in `docs/comment-budget.toml`:

    R1  stale path reference — a comment names a repository path that does
        not exist.
    R2  unresolvable doc pointer — a comment names an `architecture/`
        document, or a section inside one, that does not exist.
    R3  oversized comment block — a comment block is longer than the
        configured maximum.
    R4  comment budget — a file's comment mass exceeds its recorded
        ceiling, or the corpus exceeds the recorded global ceiling.

WHY THE BUDGET IS A RATCHET AND NOT A SNAPSHOT
    The recorded ceiling is the corpus size at the last bank. It may be
    lowered freely; it may not be exceeded. A reduction that is not banked
    is reported as an unbanked reduction, and the global ceiling is checked
    for *both* directions: the corpus may not grow past the recorded value,
    and the recorded value may not sit above the measured one. That is what
    makes the corpus unable to re-inflate: the only way to spend the lines a
    reduction frees is to edit the manifest, which is a visible, reviewed
    diff in the same change as the comment that took them.

    A budget that merely recorded "whatever the tree currently is" would
    forbid nothing. This one forbids growth.

WHAT THIS GUARD DELIBERATELY DOES NOT DO
    It does not judge prose. It does not require a comment to be useful, and
    it cannot tell an *explanation* from a *restatement*. It measures mass
    and it resolves references. The judgement about what a comment should
    say was made by a human, in the reduction itself; this guard only keeps
    that judgement from being undone silently.

    It does not scan Rust with a regex. `scripts/rust_test_scan.py` is the
    shared Rust-aware scanner, because a regex scan misreads lifetimes as
    character literals and undercounts `tests/` badly.

USAGE
    python3 scripts/check_comment_budget.py             # check the tree
    python3 scripts/check_comment_budget.py --self-test  # prove it bites
    python3 scripts/check_comment_budget.py --bank       # rewrite the manifest
"""

from __future__ import annotations

import argparse
import ast
import fnmatch
import math
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST_REL = "docs/comment-budget.toml"
SCANNER_REL = "scripts/rust_test_scan.py"

SOURCE_TREES = ("src", "crates", "tests")

# A repository path named by a comment. The prefixes are the ones the
# repository actually uses, so a bare word is never read as a path.
PATH_PREFIXES = (
    "src/",
    "crates/",
    "tests/",
    "plans/",
    "architecture/",
    "docs/",
    "scripts/",
    "assets/",
    "examples/",
    "apps/",
    "benches/",
    ".opencode/",
    ".github/",
)
PATH_RE = re.compile(
    r"(?<![\w/.-])(?:" + "|".join(re.escape(p) for p in PATH_PREFIXES) + r")[A-Za-z0-9_./\-*?]*"
)
# `architecture/<doc>.md` followed by a section marker.
DOC_POINTER_RE = re.compile(r"architecture/([a-z0-9_\-]+\.md)")
SECTION_RE = re.compile(r"§\s*\"([^\"]+)\"")
# A numbered section marker, e.g. `§6.1`, names a numbered heading.
NUMBERED_SECTION_RE = re.compile(r"§\s*(\d+(?:\.\d+)*)")
# Contract language: a rule the code cannot enforce on its own.
CONTRACT_RE = re.compile(
    r"\b(?:must(?: not)?|never|always|only|invariant|refuse[sd]?|forbidden|"
    r"prohibited|guarantee[sd]?|shall)\b",
    re.IGNORECASE,
)
# A milestone-prefixed marker that a guard greps for in a comment.
LANDMARK_RE = re.compile(r"^(?:[cC]\d{3}|[mM]\d{3,4}|ADR-\d{4})[-a-z0-9]*:\s*\S")
# An owning document: the place the contract is already written down.
OWNER_RE = re.compile(
    r"architecture/[A-Za-z0-9_\-./]+\.md|plans/[A-Za-z0-9_\-./]+\.md|"
    r"ADR-\d{4}|\b[MC]\d{3,4}\b"
)


@dataclass
class FileFacts:
    rel: str
    comment_lines: int
    comment_words: int
    code_lines: int
    blocks: int
    longest_block: int
    ratio: float


def load_scanner():
    sys.path.insert(0, str(ROOT / "scripts"))
    import rust_test_scan  # noqa: PLC0415

    return rust_test_scan


def rust_files(root: Path) -> list[Path]:
    out: list[Path] = []
    for tree in SOURCE_TREES:
        base = root / tree
        if not base.exists():
            continue
        for path in sorted(base.rglob("*.rs")):
            if "target" in path.parts:
                continue
            out.append(path)
        for path in sorted(base.rglob("*.inc")):
            if "target" in path.parts:
                continue
            out.append(path)
    return out


def comment_spans(rts, text: str):
    """Comment spans of a source file, with their line numbers."""
    scan = rts.scan_source(text)
    spans = []
    for com in scan.comments:
        start = scan.text.count("\n", 0, com.start)
        end = scan.text.count("\n", 0, max(com.start, com.end - 1))
        spans.append((start, end, com))
    return scan, spans


def measure(rts, path: Path, root: Path) -> FileFacts:
    text = path.read_text(encoding="utf-8")
    scan, spans = comment_spans(rts, text)
    lines = text.splitlines()
    comment_lines = 0
    comment_words = 0
    for start, end, com in spans:
        body = scan.text[com.start : com.end]
        comment_lines += end - start + 1
        comment_words += len(body.split())
    code_lines = sum(1 for line in scan.code().splitlines() if line.strip())
    blocks = 0
    longest = 0
    previous_end = None
    block_start = None
    block_end = None
    for start, end, _com in spans:
        if previous_end is not None and start == previous_end + 1:
            block_end = end
        else:
            if block_start is not None:
                blocks += 1
                longest = max(longest, block_end - block_start + 1)
            block_start = start
            block_end = end
        previous_end = end
    if block_start is not None:
        blocks += 1
        longest = max(longest, block_end - block_start + 1)
    rel = path.relative_to(root).as_posix()
    ratio = comment_lines / code_lines if code_lines else float(comment_lines)
    return FileFacts(rel, comment_lines, comment_words, code_lines, blocks, longest, ratio)


def guard_landmarks(root: Path) -> dict[tuple[str, str], list[str]]:
    """Every contract landmark the repository's guards require, and where.

    A guard that identifies a call site by a comment marker depends on that
    comment. A comment reduction that removes the marker silently disables
    the guard, so the marker is part of the guard's contract.

    The landmarks are read out of the guards themselves: a module-level
    constant whose name mentions `LANDMARK` and whose value is a list of
    `(path, markers)` pairs. The module is parsed, not executed, so reading
    a guard's contract can never run it.
    """
    found: dict[tuple[str, str], list[str]] = {}
    scripts = root / "scripts"
    if not scripts.is_dir():
        return found
    for guard in sorted(scripts.glob("check_*.py")):
        try:
            tree = ast.parse(guard.read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError, SyntaxError):
            continue
        for node in tree.body:
            if not isinstance(node, ast.Assign) or not isinstance(node.value, ast.List):
                continue
            targets = node.targets
            if len(targets) != 1 or not isinstance(targets[0], ast.Name):
                continue
            if "LANDMARK" not in targets[0].id.upper():
                continue
            for element in node.value.elts:
                if not (isinstance(element, ast.Tuple) and len(element.elts) == 2):
                    continue
                try:
                    target = ast.literal_eval(element.elts[0])
                    markers = ast.literal_eval(element.elts[1])
                except (ValueError, SyntaxError):
                    continue
                if not isinstance(target, str) or not isinstance(markers, (list, tuple)):
                    continue
                for marker in markers:
                    if isinstance(marker, str):
                        found.setdefault((target, marker), []).append(guard.name)
    return found


def path_exists(root: Path, candidate: str) -> bool:
    if "*" in candidate or "?" in candidate:
        pattern = candidate
        for base in SOURCE_TREES + (
            "architecture", "docs", "plans", "scripts", "assets", ".opencode", ".github",
        ):
            directory = root / base
            if not directory.exists():
                continue
            for path in directory.rglob("*"):
                if not path.is_file():
                    continue
                rel = path.relative_to(root).as_posix()
                if fnmatch.fnmatch(rel, pattern):
                    return True
        return False
    return (root / candidate).exists()


def check_tree(root: Path) -> list[str]:
    rts = load_scanner()
    manifest_path = root / MANIFEST_REL
    if not manifest_path.exists():
        return [f"{MANIFEST_REL} is missing: the budget cannot be checked"]
    manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    global_budget = manifest.get("global", {})
    files_budget = manifest.get("files", {})
    examples = manifest.get("examples", {})
    known_stale = manifest.get("known_stale", {})
    oversized = manifest.get("oversized", {})
    max_block_lines = int(manifest.get("max_block_lines", 40))
    unlisted_max = int(manifest.get("unlisted_max_comment_lines", 20))

    violations: list[str] = []

    # --- R1 / R2 / R3: per-file comment inspection ------------------------
    measured: list[FileFacts] = []
    for path in rust_files(root):
        rel = path.relative_to(root).as_posix()
        try:
            text = path.read_text(encoding="utf-8")
            scan, spans = comment_spans(rts, text)
        except (OSError, UnicodeDecodeError) as error:
            violations.append(f"R0 {rel}: the file could not be read: {error}")
            continue
        lines = text.splitlines()
        measured.append(_facts_from_scan(rel, text, scan))
        for start, end, com in spans:
            body = scan.text[com.start : com.end]
            line_no = start + 1
            for match in PATH_RE.finditer(body):
                candidate = match.group(0).rstrip(".,;:)]}`\"'")
                if not candidate or candidate in examples:
                    continue
                if candidate.startswith("architecture/"):
                    continue  # R2 owns the architecture/ pointers
                if path_exists(root, candidate):
                    continue
                if candidate in known_stale:
                    print(
                        f"NOTE: {rel}:{line_no}: `{candidate}` is a recorded stale "
                        f"reference — {known_stale[candidate]}"
                    )
                    continue
                violations.append(
                    f"R1 {rel}:{line_no}: the comment names `{candidate}`, which does not exist"
                )
            for doc_match in DOC_POINTER_RE.finditer(body):
                doc = f"architecture/{doc_match.group(1)}"
                if not (root / doc).exists():
                    violations.append(
                        f"R2 {rel}:{line_no}: the comment names `{doc}`, which does not exist"
                    )
                    continue
                doc_text = (root / doc).read_text(encoding="utf-8")
                after = body[doc_match.end() : doc_match.end() + 120]
                section = SECTION_RE.search(after)
                if section:
                    heading = section.group(1)
                    if f"#{heading}" not in doc_text and heading not in doc_text:
                        violations.append(
                            f"R2 {rel}:{line_no}: `{doc}` has no section named `{heading}`"
                        )
                number = NUMBERED_SECTION_RE.search(after)
                if number and not section:
                    prefix = number.group(1)
                    if not re.search(rf"^#+\s*{re.escape(prefix)}\b", doc_text, re.M):
                        violations.append(
                            f"R2 {rel}:{line_no}: `{doc}` has no section numbered `{prefix}`"
                        )
        # R3 — oversized comment blocks.
        previous_end = None
        block_start = None
        block_end = None
        for start, end, _com in spans:
            if previous_end is not None and start == previous_end + 1:
                block_end = end
            else:
                if block_start is not None:
                    _check_block(
                        violations,
                        rel,
                        block_start,
                        block_end,
                        max_block_lines,
                        oversized,
                        "\n".join(lines[block_start : block_end + 1]),
                    )
                block_start = start
                block_end = end
            previous_end = end
        if block_start is not None:
            _check_block(
                violations,
                rel,
                block_start,
                block_end,
                max_block_lines,
                oversized,
                "\n".join(lines[block_start : block_end + 1]),
            )

    # --- R6: a guard landmark must still be there -------------------------
    for (target, marker), guards in sorted(guard_landmarks(root).items()):
        path = root / target
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            violations.append(f"R0 {target}: the file could not be read")
            continue
        if marker not in text:
            violations.append(
                f"R6 {target}: the contract landmark {marker!r}, which "
                f"{', '.join(sorted(set(guards)))} greps for, is gone from the file"
            )

    # --- R4: the budget ---------------------------------------------------
    facts = measured
    total_lines = sum(f.comment_lines for f in facts)
    total_words = sum(f.comment_words for f in facts)
    total_code = sum(f.code_lines for f in facts)

    max_lines = global_budget.get("max_comment_lines")
    if isinstance(max_lines, int) and total_lines > max_lines:
        violations.append(
            f"R4 the corpus holds {total_lines} comment lines, over the recorded "
            f"ceiling of {max_lines}"
        )
    max_words = global_budget.get("max_comment_words")
    if isinstance(max_words, int) and total_words > max_words:
        violations.append(
            f"R4 the corpus holds {total_words} comment words, over the recorded "
            f"ceiling of {max_words}"
        )
    max_ratio = global_budget.get("max_ratio")
    if isinstance(max_ratio, (int, float)) and total_code:
        ratio = total_lines / total_code
        if ratio > float(max_ratio):
            violations.append(
                f"R4 the corpus comment-to-code ratio is {ratio:.4f}, over the recorded "
                f"ceiling of {float(max_ratio):.4f}"
            )
    for fact in facts:
        budget = files_budget.get(fact.rel)
        if budget is None:
            if fact.comment_lines >= unlisted_max:
                violations.append(
                    f"R4 {fact.rel}: {fact.comment_lines} comment lines is over the "
                    f"{unlisted_max}-line floor for an unlisted file; add it to "
                    f"{MANIFEST_REL} with a ceiling"
                )
            continue
        file_max = budget.get("max_comment_lines")
        if isinstance(file_max, int) and fact.comment_lines > file_max:
            violations.append(
                f"R4 {fact.rel}: {fact.comment_lines} comment lines is over the recorded "
                f"ceiling of {file_max}"
            )
        ratio_max = budget.get("max_ratio")
        if isinstance(ratio_max, (int, float)) and fact.ratio > float(ratio_max):
            violations.append(
                f"R4 {fact.rel}: the comment-to-code ratio {fact.ratio:.4f} is over the "
                f"recorded ceiling of {float(ratio_max):.4f}"
            )
    return violations


def _facts_from_scan(rel: str, text: str, scan) -> FileFacts:
    """The file's comment facts, from a scan that has already been made."""
    comment_lines = 0
    comment_words = 0
    previous_end = None
    blocks = 0
    longest = 0
    block_start = None
    block_end = None
    for com in scan.comments:
        start = scan.text.count("\n", 0, com.start)
        end = scan.text.count("\n", 0, max(com.start, com.end - 1))
        body = scan.text[com.start : com.end]
        comment_lines += end - start + 1
        comment_words += len(body.split())
        if previous_end is not None and start == previous_end + 1:
            block_end = end
        else:
            if block_start is not None:
                blocks += 1
                longest = max(longest, block_end - block_start + 1)
            block_start = start
            block_end = end
        previous_end = end
    if block_start is not None:
        blocks += 1
        longest = max(longest, block_end - block_start + 1)
    code_lines = sum(1 for line in scan.code().splitlines() if line.strip())
    ratio = comment_lines / code_lines if code_lines else float(comment_lines)
    return FileFacts(rel, comment_lines, comment_words, code_lines, blocks, longest, ratio)


def _check_block(
    violations: list[str],
    rel: str,
    start: int,
    end: int,
    max_block_lines: int,
    oversized: dict,
    body: str = "",
) -> None:
    length = end - start + 1
    key = f"{rel}:{start + 1}"
    if length <= max_block_lines:
        return
    if key not in oversized:
        violations.append(
            f"R3 {key}: the comment block is {length} lines, over the "
            f"{max_block_lines}-line maximum"
        )
    if CONTRACT_RE.search(body) and not OWNER_RE.search(body):
        violations.append(
            f"R5 {key}: the comment block restates a contract over {max_block_lines} lines "
            f"without naming the document that owns it"
        )


def _ceil_ratio(value: float) -> float:
    """A recorded ratio ceiling must never sit below the measured ratio."""
    return math.ceil(value * 10000) / 10000


def bank(root: Path) -> int:
    """Rewrite the manifest's ceilings from the measured tree."""
    rts = load_scanner()
    manifest_path = root / MANIFEST_REL
    manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    facts = [measure(rts, path, root) for path in rust_files(root)]
    total_lines = sum(f.comment_lines for f in facts)
    total_words = sum(f.comment_words for f in facts)
    total_code = sum(f.code_lines for f in facts)

    unlisted_max = int(manifest.get("unlisted_max_comment_lines", 20))
    lines_out = [
        "# docs/comment-budget.toml",
        "#",
        "# The comment budget of the Rust sources in `src/`, `crates/` and `tests/`.",
        "#",
        "# Every value here is a CEILING, and every ceiling is derived from a",
        "# measurement of the tree — `python3 scripts/check_comment_budget.py --bank`",
        "# recomputes them. The budget is a ratchet, not a snapshot: a reduction may",
        "# lower a ceiling, and no ceiling may be exceeded. The only way to spend the",
        "# lines a reduction frees is to edit this file, which shows up in the same",
        "# diff as the comment that took them.",
        "",
        f"max_block_lines = {manifest.get('max_block_lines', 40)}",
        f"unlisted_max_comment_lines = {unlisted_max}",
        "",
        "[global]",
        f"max_comment_lines = {total_lines}",
        f"max_comment_words = {total_words}",
        f"max_ratio = {_ceil_ratio(total_lines / total_code) if total_code else 0.0}",
        "",
        "# Per-file ceilings. A file may shrink; it may not grow past what is",
        "# recorded here. A file that crosses the unlisted floor must be added.",
        "[files]",
    ]
    for fact in sorted(facts, key=lambda f: f.rel):
        if fact.comment_lines < unlisted_max:
            continue
        lines_out.append(
            f'"{fact.rel}" = {{ max_comment_lines = {fact.comment_lines}, '
            f"max_ratio = {_ceil_ratio(fact.ratio)} }}"
        )
    examples = manifest.get("examples", {})
    known_stale = manifest.get("known_stale", {})
    if examples:
        lines_out += ["", "# Illustrative paths a comment names but the repository does not hold.",
                      "# Each one is an example, not a reference."]
        lines_out.append("[examples]")
        for key in sorted(examples):
            lines_out.append(f'"{key}" = "{examples[key]}"')
    known_stale = manifest.get("known_stale", {})
    if known_stale:
        lines_out += ["", "# Stale references that are recorded rather than fixed, with the reason.",
                      "# Each one must be removed when the reason no longer holds."]
        lines_out.append("[known_stale]")
        for key in sorted(known_stale):
            lines_out.append(f'"{key}" = "{known_stale[key]}"')
    oversized = manifest.get("oversized", {})
    if oversized:
        lines_out += ["", "# Comment blocks longer than `max_block_lines`, with the reason they",
                      "# are allowed to be that long."]
        lines_out.append("[oversized]")
        for key in sorted(oversized):
            lines_out.append(f'"{key}" = "{oversized[key]}"')
    manifest_path.write_text("\n".join(lines_out) + "\n", encoding="utf-8")
    print(
        f"banked {len(facts)} files: {total_lines} comment lines, "
        f"{total_words} words, ratio {total_lines / total_code:.4f}"
    )
    return 0


def _copy_tree(root: Path, dest: Path) -> None:
    for tree in SOURCE_TREES + (
        "architecture", "docs", "plans", "scripts", "assets", ".opencode", ".github",
    ):
        source = root / tree
        if source.exists():
            shutil.copytree(source, dest / tree, symlinks=True)


def _inject(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"self-test could not find the anchor text in {path}: {old!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def _synthetic_tree(root: Path) -> Path:
    """A minimal tree the self-test can mutate without copying the real one.

    The real tree is checked in place (the unmodified tree must pass). The
    defect classes are then injected into this small tree, so the self-test
    does not copy the repository once per case.
    """
    for tree in SOURCE_TREES + (
        "architecture", "docs", "plans", "scripts", "assets", ".opencode", ".github",
    ):
        (root / tree).mkdir(parents=True, exist_ok=True)
    marker = "c007-denial: " + "probe"
    probe = root / "src" / "probe.rs"
    probe.write_text(
        "// A small probe module.\n"
        "//\n"
        "// Its comments are ordinary: they state no rule and name no path.\n"
        f"// {marker}\n"
        "pub fn probe() -> u32 {\n"
        "    1\n"
        "}\n",
        encoding="utf-8",
    )
    guard = root / "scripts" / "check_probe.py"
    guard.parent.mkdir(parents=True, exist_ok=True)
    guard.write_text(
        "CONTRACT_LANDMARKS = [(\"src/probe.rs\", ("
        + repr(marker)
        + ",))]\n",
        encoding="utf-8",
    )
    manifest = root / MANIFEST_REL
    manifest.write_text(
        "max_block_lines = 50\n"
        "unlisted_max_comment_lines = 20\n"
        "\n"
        "[global]\n"
        "max_comment_lines = 6\n"
        "max_comment_words = 60\n"
        "max_ratio = 10.0\n"
        "\n"
        "[files]\n"
        "\n"
        "[examples]\n"
        '"src/small.rs" = "a known illustrative path"\n'
        '"src/large.rs" = "a known illustrative path"\n'
        '"src/foo.rs" = "a known illustrative path"\n'
        '"src/bar.rs" = "a known illustrative path"\n'
        '"src/http/ratelimit.rs" = "a known illustrative path"\n'
        '"tests/test_foo.py" = "a known illustrative path"\n',
        encoding="utf-8",
    )
    return probe


def _self_test() -> int:
    """Prove the guard rejects each defect class and accepts the clean tree."""
    failures: list[str] = []
    violations = check_tree(ROOT)
    if violations:
        failures.append(f"the unmodified tree must pass the guard: {violations[0]}")
        _report(failures)
        return 1
    print("  accepted the unmodified tree")

    with tempfile.TemporaryDirectory(prefix="codegg-comment-budget-") as raw:
        scratch = Path(raw)
        probe = _synthetic_tree(scratch)
        original = probe.read_text(encoding="utf-8")

        violations = check_tree(scratch)
        if violations:
            failures.append(f"the synthetic tree must pass the guard: {violations[0]}")
        else:
            print("  accepted the synthetic tree")

        cases = [
            (
                "an over-budget file",
                "\n" + "// budget probe\n" * 40,
                lambda v: v.startswith("R4"),
            ),
            (
                "a stale path reference",
                "\n// see `src/does_not_exist.rs` for the probe\n",
                lambda v: v.startswith("R1"),
            ),
            (
                "an unresolvable doc pointer",
                "\n// see `architecture/no_such_document.md` for the probe\n",
                lambda v: v.startswith("R2"),
            ),
            (
                "an oversized comment block",
                "\n" + "// probe\n" * 60,
                lambda v: v.startswith("R3"),
            ),
            (
                "an orphaned contract block",
                "\n" + "// the render must never draw a half widget.\n" * 60,
                lambda v: v.startswith("R5"),
            ),
        ]
        for label, payload, accepts in cases:
            probe.write_text(original + payload, encoding="utf-8")
            violations = check_tree(scratch)
            hit = next((v for v in violations if accepts(v)), None)
            if hit is None:
                failures.append(f"the guard did not reject {label}")
            else:
                print(f"  rejected {label}: {hit}")
            probe.write_text(original, encoding="utf-8")

        # R6: a comment marker a guard greps for must still be there.
        probe.write_text(
            original.replace("// c007-denial: probe\n", ""), encoding="utf-8"
        )
        violations = check_tree(scratch)
        hit = next((v for v in violations if v.startswith("R6")), None)
        if hit is None:
            failures.append("the guard did not reject a removed guard landmark")
        else:
            print(f"  rejected a removed guard landmark: {hit}")
        probe.write_text(original, encoding="utf-8")

        # The known illustrative paths must stay acceptable.
        probe.write_text(
            original
            + "\n// the illustrative paths `src/small.rs`, `src/large.rs`, "
            "`src/foo.rs`, `src/bar.rs`, `src/http/ratelimit.rs` and "
            "`tests/test_foo.py` are examples, not references\n",
            encoding="utf-8",
        )
        violations = check_tree(scratch)
        if any(v.startswith("R1") for v in violations):
            failures.append(
                "the guard flagged a known illustrative path: "
                + next(v for v in violations if v.startswith("R1"))
            )
        else:
            print("  accepted the known illustrative paths")
        probe.write_text(original, encoding="utf-8")

    if failures:
        _report(failures)
        return 1
    print("comment budget guard self-test passed")
    return 0


def _report(failures: list[str]) -> None:
    for failure in failures:
        print(f"ERROR: {failure}", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true", help="prove the guard bites")
    parser.add_argument("--bank", action="store_true", help="rewrite the manifest from the tree")
    args = parser.parse_args()
    if args.self_test:
        return _self_test()
    if args.bank:
        return bank(ROOT)
    violations = check_tree(ROOT)
    if violations:
        for violation in violations:
            print(f"ERROR: {violation}", file=sys.stderr)
        print(f"{len(violations)} comment-budget violation(s)", file=sys.stderr)
        return 1
    print("comment budget guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
