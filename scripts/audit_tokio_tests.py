#!/usr/bin/env python3
"""Audit Tokio test runtime flavors.

Scans Rust source files for tests using `current_thread` that contain
concurrency-sensitive patterns (tokio::spawn, channels, subprocesses, etc.).
These tests may need `multi_thread` to avoid deadlocks or race conditions.

Exit codes:
  0 — no candidates found (or --no-fail specified)
  1 — candidates found (informational, not a failure)

Usage:
    python3 scripts/audit_tokio_tests.py [--json] [--no-fail] [--summary]
"""

import argparse
import re
import sys
from pathlib import Path

# Concurrency-sensitive patterns that may require multi-thread runtime
CONCURRENCY_PATTERNS = [
    (r"tokio::spawn\b", "tokio::spawn"),
    (r"spawn_blocking\b", "spawn_blocking"),
    (r"tokio::process::", "tokio::process"),
    (r"tokio::sync::broadcast", "tokio::sync::broadcast"),
    (r"tokio::sync::mpsc", "tokio::sync::mpsc"),
    (r"tokio::sync::oneshot", "tokio::sync::oneshot"),
    (r"tokio::sync::watch", "tokio::sync::watch"),
    (r"tokio::sync::Mutex", "tokio::sync::Mutex"),
    (r"tokio::sync::RwLock", "tokio::sync::RwLock"),
    (r"tokio::time::sleep", "tokio::time::sleep"),
    (r"tokio::time::interval", "tokio::time::interval"),
    (r"timeout\(", "timeout()"),
    (r"TcpListener::bind", "TcpListener::bind"),
    (r"TcpStream::connect", "TcpStream::connect"),
    (r"UnixListener::bind", "UnixListener::bind"),
    (r"UnixStream::connect", "UnixStream::connect"),
    (r"tokio::net::", "tokio::net"),
    (r"Command::new", "Command::new (subprocess)"),
    (r"std::process::Command", "std::process::Command"),
    (r"\.output\(\)", ".output() (subprocess)"),
    (r"\.spawn\(\)", ".spawn() (subprocess)"),
]

# Patterns in test function bodies that indicate real concurrency usage
# (not just imports or type definitions)
BODY_PATTERNS = [
    (r"tokio::spawn\s*\(", "tokio::spawn"),
    (r"spawn_blocking\s*\(", "spawn_blocking"),
    (r"tokio::process::Command", "tokio::process::Command"),
    (r"\.send\(", "channel send"),
    (r"\.recv\(", "channel recv"),
    (r"\.write_all\(", "async write"),
    (r"\.read_to_end\(", "async read"),
    (r"\.accept\(", "socket accept"),
    (r"tokio::time::sleep", "tokio::time::sleep"),
    (r"tokio::time::timeout", "tokio::time::timeout"),
    (r"tokio::select!", "tokio::select!"),
    (r"tokio::join!", "tokio::join!"),
    (r"tokio::spawn_blocking", "spawn_blocking"),
    (r"std::process::Command.*\.output\(\)", "subprocess output"),
    (r"std::process::Command.*\.spawn\(\)", "subprocess spawn"),
]

# Files/directories to skip
SKIP_PATHS = {
    "target",
    ".git",
    "node_modules",
    "examples",
}


def find_rust_files(root: Path) -> list[Path]:
    """Find all Rust source files, excluding build artifacts."""
    files = []
    for path in root.rglob("*.rs"):
        parts = path.relative_to(root).parts
        if any(part in SKIP_PATHS for part in parts):
            continue
        files.append(path)
    return sorted(files)


def extract_test_blocks(content: str) -> list[tuple[str, int, int]]:
    """Extract test function blocks with their start/end line numbers.

    Returns list of (function_body, start_line, end_line).
    """
    blocks = []
    lines = content.split("\n")

    i = 0
    while i < len(lines):
        line = lines[i]

        # Look for #[tokio::test(flavor = "current_thread")]
        if "tokio::test" in line and "current_thread" in line:
            # Find the function start
            j = i + 1
            while j < len(lines) and not lines[j].strip().startswith("async fn "):
                j += 1
                if j - i > 5:  # Safety: don't look too far
                    break

            if j < len(lines) and lines[j].strip().startswith("async fn "):
                # Find the function body (opening brace)
                k = j
                while k < len(lines) and "{" not in lines[k]:
                    k += 1

                if k < len(lines):
                    # Find the matching closing brace
                    brace_count = 0
                    start = k
                    while k < len(lines):
                        brace_count += lines[k].count("{") - lines[k].count("}")
                        if brace_count <= 0:
                            break
                        k += 1

                    body = "\n".join(lines[start : k + 1])
                    blocks.append((body, i + 1, k + 1))  # 1-indexed

        i += 1

    return blocks


def check_concurrency_usage(body: str) -> list[str]:
    """Check if a function body uses concurrency-sensitive patterns."""
    matches = []
    for pattern, name in BODY_PATTERNS:
        if re.search(pattern, body):
            matches.append(name)
    return sorted(set(matches))


def audit_file(filepath: Path) -> list[dict]:
    """Audit a single Rust file for current_thread tests with concurrency."""
    content = filepath.read_text(errors="replace")
    results = []

    blocks = extract_test_blocks(content)
    for body, start_line, end_line in blocks:
        patterns = check_concurrency_usage(body)
        if patterns:
            results.append(
                {
                    "file": str(filepath),
                    "start_line": start_line,
                    "end_line": end_line,
                    "patterns": patterns,
                }
            )

    return results


def main():
    parser = argparse.ArgumentParser(description="Audit Tokio test runtime flavors")
    parser.add_argument(
        "--json", action="store_true", help="Output results as JSON"
    )
    parser.add_argument(
        "--no-fail", action="store_true",
        help="Always exit 0 regardless of candidates found (advisory mode)",
    )
    parser.add_argument(
        "--summary", action="store_true",
        help="Print only a count summary instead of full listing",
    )
    parser.add_argument(
        "paths",
        nargs="*",
        default=["."],
        help="Paths to scan (default: current directory)",
    )
    args = parser.parse_args()

    root = Path.cwd()
    candidates = []

    for path_str in args.paths:
        path = Path(path_str)
        if not path.exists():
            continue

        if path.is_file() and path.suffix == ".rs":
            results = audit_file(path)
            candidates.extend(results)
        elif path.is_dir():
            for filepath in find_rust_files(path):
                results = audit_file(filepath)
                candidates.extend(results)

    if args.json:
        import json
        print(json.dumps(candidates, indent=2))
    elif args.summary:
        if not candidates:
            print("No concurrency-sensitive current_thread tests found.")
        else:
            print(f"Found {len(candidates)} current_thread test(s) with concurrency patterns.")
    else:
        if not candidates:
            print("No concurrency-sensitive current_thread tests found.")
        else:
            print(f"Found {len(candidates)} current_thread test(s) with concurrency patterns:\n")
            for c in candidates:
                rel_path = Path(c["file"]).relative_to(root) if Path(c["file"]).is_relative_to(root) else c["file"]
                print(f"  {rel_path}:{c['start_line']}-{c['end_line']}")
                for p in c["patterns"]:
                    print(f"    - {p}")
                print()

            print("These tests may need `multi_thread` runtime to avoid deadlocks.")
            print("Review each candidate and restore multi_thread where needed.")

    if args.no_fail:
        return 0
    return 1 if candidates else 0


if __name__ == "__main__":
    sys.exit(main())
