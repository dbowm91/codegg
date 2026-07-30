#!/usr/bin/env python3
"""Baseline-aware regression guard for bare #[tokio::test] annotations.

Ensures all new tokio tests specify an explicit runtime flavor. Historical
bare tests are tracked in a checked-in baseline file.

The guard scans every repository-owned Rust source location that is not a
generated, vendor, or build artifact. It rejects any bare `#[tokio::test]`
that is not followed by an unambiguous function definition, and rejects any
existing baseline entry containing unresolved markers, wildcards, or
directory suppressions.

Exit codes:
  0 — no new violations and no stale or malformed baseline entries
  1 — new bare tests, malformed source, stale entries, or malformed baseline

Usage:
    python3 scripts/check-tokio-test-flavors.py [--self-test]
                                                 [--emit-current]
                                                 [--baseline PATH]
"""

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

DEFAULT_BASELINE = REPO_ROOT / "scripts" / "tokio-test-flavor-baseline.txt"

# Exclusions are limited to non-source/build/vendor locations. Each entry
# below is individually justified.
SKIP_PATHS = {
    "target",       # Cargo build output
    ".git",         # VCS metadata
    "node_modules", # Third-party generated/dependency content (only when present)
}

# Match a bare #[tokio::test] without arguments (end of line).
BARE_TOKIO_TEST_RE = re.compile(r"#\s*\[\s*tokio::test\s*\]$")

# Match a tokio::test with explicit flavor (NOT bare).
FLAVORED_TOKIO_TEST_RE = re.compile(r"#\s*\[\s*tokio::test\s*\(")

# Match a function definition.
FN_RE = re.compile(r"^\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)")

# Marker for unresolved bare attribute identities. The presence of this
# token in source identities or baseline entries is always rejected; it
# exists only to make the failure mode explicit in error messages.
UNRESOLVED_MARKER = "UNRESOLVED"


class GuardError(Exception):
    """Raised when a malformed source attribute or baseline entry is found."""


def find_rust_files(root: Path) -> list[Path]:
    """Find all repository-owned Rust source files.

    Only non-source/build/vendor locations are excluded. Each exclusion is
    listed in SKIP_PATHS with an individual justification.
    """
    files = []
    for path in root.rglob("*.rs"):
        parts = path.relative_to(root).parts
        if any(part in SKIP_PATHS for part in parts):
            continue
        files.append(path)
    return sorted(files)


def extract_bare_test_identities(root: Path) -> list[str]:
    """Find all bare #[tokio::test] and return stable identities.

    A bare attribute without an unambiguous following function raises
    GuardError. Unresolved identities are never emitted as baseline-compatible
    entries.
    """
    identities: list[str] = []
    for filepath in find_rust_files(root):
        content = filepath.read_text(errors="replace")
        lines = content.split("\n")
        rel_path = str(filepath.relative_to(root))

        for idx, line in enumerate(lines):
            stripped = line.strip()
            if not BARE_TOKIO_TEST_RE.search(stripped):
                continue
            fn_line = idx + 1
            resolved_fn = None
            while fn_line < len(lines):
                fn_stripped = lines[fn_line].strip()
                if (
                    fn_stripped.startswith("#[")
                    or fn_stripped.startswith("//")
                    or fn_stripped == ""
                ):
                    fn_line += 1
                    continue
                m = FN_RE.match(fn_stripped)
                if m:
                    resolved_fn = m.group(1)
                break
            if resolved_fn is None:
                raise GuardError(
                    f"{rel_path}:{idx + 1}: bare #[tokio::test] is not followed "
                    f"by an unambiguous function definition"
                )
            identities.append(f"{rel_path}::{resolved_fn}")
    return sorted(identities)


def load_baseline(path: Path) -> tuple[list[str], list[str]]:
    """Load baseline and return (entries, errors).

    The baseline rejects wildcards, directory suppressions, duplicates,
    malformed lines, and any entry containing unresolved markers.
    """
    try:
        raw_text = path.read_text()
    except FileNotFoundError:
        return [], [f"baseline file not found: {path}"]
    except OSError as exc:
        return [], [f"baseline file unreadable: {path}: {exc}"]

    entries: list[str] = []
    errors: list[str] = []
    seen: set[str] = set()

    for lineno, raw in enumerate(raw_text.splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue

        if "*" in line or line.endswith("/"):
            errors.append(
                f"  line {lineno}: wildcard or directory suppression not allowed: {line}"
            )
            continue

        if "::" not in line:
            errors.append(f"  line {lineno}: missing '::' separator: {line}")
            continue

        if UNRESOLVED_MARKER in line:
            errors.append(
                f"  line {lineno}: unresolved baseline identity is not allowed: {line}"
            )
            continue

        if line in seen:
            errors.append(f"  line {lineno}: duplicate entry: {line}")
            continue

        seen.add(line)
        entries.append(line)

    return sorted(entries), errors


def collect_current_identities(root: Path) -> tuple[list[str], list[str]]:
    """Return (sorted_identities, errors) for the current repository state."""
    try:
        identities = extract_bare_test_identities(root)
    except GuardError as exc:
        return [], [str(exc)]
    return identities, []


def diff_identities(
    current: list[str], baseline: list[str]
) -> tuple[list[str], list[str]]:
    current_set = set(current)
    baseline_set = set(baseline)
    new_violations = sorted(current_set - baseline_set)
    stale_baseline = sorted(baseline_set - current_set)
    return new_violations, stale_baseline


def run_self_test() -> int:
    """Validate detection logic against representative single-file inputs.

    This is a coarse unit-level self test. Authoritative production-path
    coverage lives in scripts/tests/test_check_tokio_test_flavors.py.
    """
    test_cases = [
        ("bare #[tokio::test] alone", ["#[tokio::test]", "async fn t1() {}"]),
        ("bare after blank lines", ["", "", "#[tokio::test]", "async fn t2() {}"]),
        ("bare after #[cfg(test)]", ["#[cfg(test)]", "#[tokio::test]", "async fn t3() {}"]),
        (
            "bare after #[cfg(all(...))]",
            [
                "#[cfg(all(test, feature = \"x\"))]",
                "#[tokio::test]",
                "async fn t4() {}",
            ],
        ),
        (
            "flavored current_thread passes",
            ["#[tokio::test(flavor = \"current_thread\")]", "async fn t5() {}"],
        ),
        (
            "flavored multi_thread passes",
            [
                "#[tokio::test(flavor = \"multi_thread\", worker_threads = 2)]",
                "async fn t6() {}",
            ],
        ),
        (
            "not a test attribute (#[tokio::main])",
            ["#[tokio::main]", "async fn t7() {}"],
        ),
        ("bare with extra whitespace", ["#[ tokio::test ]", "async fn t8() {}"]),
    ]

    passed = 0
    failed = 0
    print("Running self-test...\n")

    for desc, lines in test_cases:
        identities: list[str] = []
        error: str | None = None
        try:
            for idx, line in enumerate(lines):
                stripped = line.strip()
                if not BARE_TOKIO_TEST_RE.search(stripped):
                    continue
                fn_idx = idx + 1
                resolved = None
                while fn_idx < len(lines):
                    fs = lines[fn_idx].strip()
                    if fs.startswith("#[") or fs.startswith("//") or fs == "":
                        fn_idx += 1
                        continue
                    m = FN_RE.match(fs)
                    if m:
                        resolved = m.group(1)
                    break
                if resolved is None:
                    error = f"line {idx + 1}: no function"
                else:
                    identities.append(f"test.rs::{resolved}")
        except Exception as exc:  # pragma: no cover - defensive
            error = str(exc)

        expected_bare = any(BARE_TOKIO_TEST_RE.search(l.strip()) for l in lines) and not any(
            FLAVORED_TOKIO_TEST_RE.search(l.strip()) for l in lines
        )
        if expected_bare:
            ok = bool(identities) and error is None
        else:
            ok = not identities and error is None
        if ok:
            print(f"  PASS: {desc}")
            passed += 1
        else:
            print(f"  FAIL: {desc} (identities={identities}, error={error})")
            failed += 1

    print(f"\nSelf-test results: {passed} passed, {failed} failed")
    return 0 if failed == 0 else 1


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Baseline-aware regression guard for bare #[tokio::test]"
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run the lightweight self-test (see scripts/tests for full coverage)",
    )
    parser.add_argument(
        "--emit-current",
        action="store_true",
        help="Print sorted current bare identities (for baseline regeneration)",
    )
    parser.add_argument(
        "--baseline",
        type=Path,
        default=DEFAULT_BASELINE,
        help=f"Path to baseline file (default: {DEFAULT_BASELINE})",
    )
    args = parser.parse_args()

    if args.self_test:
        return run_self_test()

    current, current_errors = collect_current_identities(REPO_ROOT)
    if current_errors:
        print("Source scan errors:")
        for err in current_errors:
            print(f"  {err}")
        return 1

    if args.emit_current:
        for ident in current:
            print(ident)
        return 0

    baseline, baseline_errors = load_baseline(args.baseline)
    if baseline_errors:
        print("Baseline errors:")
        for err in baseline_errors:
            print(err)
        return 1

    new_violations, stale_baseline = diff_identities(current, baseline)

    if not new_violations and not stale_baseline:
        print(
            f"Tokio flavor guard: {len(current)} bare tests in baseline, no new violations."
        )
        return 0

    if new_violations:
        print(f"NEW bare #[tokio::test] violations ({len(new_violations)}):")
        for v in new_violations:
            print(f"  {v}")
        print()
    if stale_baseline:
        print(f"STALE baseline entries ({len(stale_baseline)}):")
        for s in stale_baseline:
            print(f"  {s}")
        print()

    if new_violations:
        print(
            "New bare tests must use explicit flavor:\n"
            '  #[tokio::test(flavor = "current_thread")]  — for most tests\n'
            '  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]  — for concurrency\n'
        )
    if stale_baseline:
        print(
            "Stale baseline entries must be removed from the baseline file.\n"
            "Converted tests no longer need baseline entries.\n"
        )
    return 1


if __name__ == "__main__":
    sys.exit(main())
