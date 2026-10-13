#!/usr/bin/env python3
"""Static guard: raw MCP model path must stay broker-mediated (M001).

Fails when agent model-path code calls `McpService::call_tool*` directly or
reintroduces an `mcp__` execution fork outside the adapter.

Allowed:
  - src/tool/mcp_adapter.rs (the canonical adapter)
  - src/mcp/* (transport owner)
  - src/search_backend/* (native eggsearch wrappers, the non-model
    compatibility consumers that intentionally retain `call_tool_structured`)
  - tests and #[cfg(test)] modules
  - comments / doc comments

Forbidden:
  - src/agent/* containing `.call_tool(` or `.call_tool_structured(`
  - src/agent/tool_batch.rs containing `mcp_tool_calls` or `mcp__` dispatch
    fork markers (`starts_with("mcp__")` for execution routing)
  - src/agent/request_preparation.rs containing `list_filtered_tools`
    (the retired second definition origin)

Run:
  python3 scripts/check_mcp_broker_adapter.py
Exit 0 on success, 1 on failure.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

FORBIDDEN_AGENT_CALL = re.compile(r"\.call_tool(_structured)?\s*\(")
FORBIDDEN_FORK = re.compile(r"mcp_tool_calls")
FORBIDDEN_SECOND_ORIGIN = re.compile(r"list_filtered_tools")
# Execution-routing fork in agent code (registry-derived revision filtering
# in request_preparation is allowed; see allowlist below).
FORK_IN_AGENT = re.compile(r'starts_with\("mcp__"\)')

ALLOWED_PREFIXES = (
    "src/tool/mcp_adapter.rs",
    "src/mcp/",
    "src/search_backend/",
    "tests/",
)


def is_allowed(rel: str) -> bool:
    return rel.startswith(ALLOWED_PREFIXES) or "/tests/" in rel or rel.endswith("_test.rs")


def strip_comments(line: str) -> str:
    s = line.strip()
    if s.startswith("//") or s.startswith("///") or s.startswith("//!") or s.startswith("#"):
        return ""
    # best-effort inline comment strip (does not handle strings containing //)
    if "//" in line:
        return line.split("//", 1)[0]
    return line


def main() -> int:
    violations: list[str] = []

    for path in sorted((ROOT / "src").rglob("*.rs")):
        rel = str(path.relative_to(ROOT))
        try:
            lines = path.read_text().splitlines()
        except Exception:
            continue
        for idx, raw in enumerate(lines, start=1):
            line = strip_comments(raw)
            if not line.strip():
                continue
            # Agent model path must not call transport directly.
            if rel.startswith("src/agent/") and not is_allowed(rel):
                if FORBIDDEN_AGENT_CALL.search(line):
                    violations.append(f"  {rel}:{idx}: direct MCP transport call in agent path: {line.strip()}")
                if FORBIDDEN_FORK.search(line):
                    violations.append(f"  {rel}:{idx}: retired mcp fork marker: {line.strip()}")
                if FORBIDDEN_SECOND_ORIGIN.search(line):
                    violations.append(f"  {rel}:{idx}: retired second MCP definition origin: {line.strip()}")
                # Allow the two registry-derived occurrences in
                # request_preparation.rs (revision + single-origin comment)
                # and the tool_surface backend classifier, but forbid any
                # other execution-routing fork.
                if FORK_IN_AGENT.search(line):
                    if rel == "src/agent/request_preparation.rs":
                        continue
                    if rel == "src/agent/tool_surface.rs":
                        continue
                    if "no `mcp__*` fork" in line or "M001" in line:
                        continue
                    violations.append(f"  {rel}:{idx}: mcp execution fork outside adapter: {line.strip()}")

    # The adapter itself must exist and own the transport call.
    adapter = ROOT / "src/tool/mcp_adapter.rs"
    if not adapter.exists():
        violations.append("  src/tool/mcp_adapter.rs: missing canonical MCP broker adapter")
    else:
        text = adapter.read_text()
        if ".call_tool_structured" not in text:
            violations.append("  src/tool/mcp_adapter.rs: adapter must call McpService::call_tool_structured via pinned client")

    if violations:
        print("ERROR: Raw MCP model bypass detected outside the broker adapter.")
        print("All model-controlled mcp__* calls must go through McpToolAdapter + ToolBroker.")
        print()
        for v in violations:
            print(v)
        print()
        return 1
    print("OK: No raw MCP model bypass outside the broker adapter.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
