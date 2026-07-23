#!/usr/bin/env python3
"""Static lint: tool broker boundary guard.

Enforces that production AgentLoop code does not invoke Tool::execute,
Tool::execute_structured, or ToolRegistry::execute_capture directly.
All production tool calls must go through the canonical ToolBroker.

Allowed direct calls:
  - Inside src/tool/broker.rs (the broker itself)
  - Inside src/tool/mod.rs (Tool trait default implementations)
  - Inside #[cfg(test)] blocks and tests/
  - Inside src/tool/*/tests modules

Run:
  python3 scripts/check_tool_broker_boundary.py

Exit code 0 on success, 1 on failure.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "src"

# Patterns that indicate a direct tool execution call
DIRECT_CALL_PATTERNS = [
    re.compile(r"\.execute_capture\("),
    re.compile(r"\.execute_structured\("),
    re.compile(r"\.execute\(&input"),
]

# Files where direct calls are permitted (the broker itself, trait defaults)
ALLOWED_FILES = {
    "src/tool/broker.rs",
    "src/tool/mod.rs",
}

# Files/dirs where direct calls are permitted under test cfg
TEST_PATTERNS = [
    re.compile(r"#\[cfg\(test\)\]"),
    re.compile(r"#\[test\]"),
    re.compile(r"#\[tokio::test\]"),
]


def is_test_context(lines: list[str], line_idx: int) -> bool:
    """Check if a line is inside a #[cfg(test)] block or test function."""
    # Simple heuristic: look backwards for #[cfg(test)] or test attributes
    for i in range(line_idx, max(line_idx - 20, -1), -1):
        line = lines[i].strip()
        if line.startswith("#[cfg(test)]"):
            return True
        if line.startswith("#[test]") or line.startswith("#[tokio::test]"):
            return True
    return False


def check_file(rel_path: str, lines: list[str]) -> list[str]:
    """Check a single file for direct tool execution calls."""
    errors = []
    for idx, line in enumerate(lines):
        stripped = line.strip()
        # Skip comments and doc comments
        if stripped.startswith("//") or stripped.startswith("///") or stripped.startswith("//!"):
            continue
        for pattern in DIRECT_CALL_PATTERNS:
            if pattern.search(line):
                errors.append(f"  {rel_path}:{idx + 1}: {stripped}")
                break
    return errors


def main() -> int:
    violations = []

    for rust_file in sorted(SRC.rglob("*.rs")):
        rel = rust_file.relative_to(ROOT)
        rel_str = str(rel)

        # Skip allowed files
        if rel_str in ALLOWED_FILES:
            continue

        # Skip test files
        if "/tests/" in rel_str or rel_str.endswith("_test.rs"):
            continue

        try:
            content = rust_file.read_text()
        except Exception:
            continue

        lines = content.splitlines()
        file_errors = check_file(rel_str, lines)

        # Filter out test-context matches
        filtered = []
        for err_line in file_errors:
            # Extract line number from error string
            match = re.search(r":(\d+):", err_line)
            if match:
                line_idx = int(match.group(1)) - 1
                if is_test_context(lines, line_idx):
                    continue
            filtered.append(err_line)

        violations.extend(filtered)

    if violations:
        print("ERROR: Direct tool execution calls found outside the broker boundary.")
        print("All production tool calls must go through ToolBroker::execute().")
        print()
        for v in violations:
            print(v)
        print()
        print("To fix: route the call through self.tool_broker.execute() instead.")
        return 1

    print("OK: No direct tool execution calls outside the broker boundary.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
