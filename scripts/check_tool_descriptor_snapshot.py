#!/usr/bin/env python3
"""Static guard: external tool discovery stays generation-correct (M002).

Ensures:
  - no second MCP definition origin (`list_filtered_tools`) in agent code;
  - plugin hook output is confined via `confine_hook_definitions` (both
    cache-hit and uncached paths) and never quietly clears `defer_loading`;
  - the descriptor snapshot (`ToolDescriptorSnapshot` + fingerprint + plugin
    activation + agent identity) participates in `tool_def_cache` identity.

Run:
  python3 scripts/check_tool_descriptor_snapshot.py
Exit 0 on success, 1 on failure.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REQ = ROOT / "src/agent/request_preparation.rs"
SNAP = ROOT / "src/agent/tool_descriptor_snapshot.rs"
COORD = ROOT / "src/agent/coordinator.rs"


def fail(msg: str) -> int:
    print(f"ERROR: {msg}")
    return 1


def main() -> int:
    violations: list[str] = []

    if not SNAP.exists():
        violations.append("src/agent/tool_descriptor_snapshot.rs missing (snapshot owner required)")

    try:
        req = REQ.read_text().splitlines()
    except Exception as e:
        return fail(f"cannot read request_preparation.rs: {e}")

    text = "\n".join(req)

    # 1. No second origin.
    for idx, line in enumerate(req, start=1):
        s = line.strip()
        if s.startswith("//"):
            continue
        if "list_filtered_tools" in line:
            violations.append(f"  request_preparation.rs:{idx}: retired second MCP origin: {s}")

    # 2. Hook confinement present in both paths (at least two call sites:
    #    cache-hit + uncached).
    confine_uses = [i for i, l in enumerate(req) if "confine_hook_definitions" in l]
    if len(confine_uses) < 2:
        violations.append(
            f"  request_preparation.rs: expected >=2 confine_hook_definitions uses (cache-hit + uncached), found {len(confine_uses)}"
        )

    # 3. Hook path must not quietly clear defer_loading.
    for idx, line in enumerate(req):
        if "dispatch_tool_definition" in line:
            window = "\n".join(req[idx : idx + 20])
            if "defer_loading: None" in window:
                violations.append(
                    f"  request_preparation.rs:{idx+1}: hook path clears defer_loading (must preserve canonical deferral)"
                )

    # 4. Snapshot participation in cache identity.
    for token in [
        "ToolDescriptorSnapshot",
        "snapshot_fingerprint",
        "plugin_activation_revision",
        "current_agent_name",
    ]:
        if token not in text:
            violations.append(f"  request_preparation.rs: missing snapshot token '{token}'")

    # 5. Coordinator cache carries the M002 owners.
    try:
        coord = COORD.read_text()
    except Exception as e:
        return fail(f"cannot read coordinator.rs: {e}")
    if "plugin_activation" not in coord.lower() and "ToolDefCache" not in coord:
        violations.append("  coordinator.rs: ToolDefCache missing M002 generation owners")
    # Tuple must have the three trailing M002 fields (13 total). Cheap check:
    # the type alias spans multiple lines; count commas in the alias block.
    m = re.search(r"type ToolDefCache = \((.*?)\);", coord, re.S)
    if m:
        commas = m.group(1).count(",")
        # 13 fields => 12 commas (trailing comma still counts if present).
        if commas < 11:
            violations.append(
                f"  coordinator.rs: ToolDefCache has {commas+1} fields, expected >=13 (M002 agent/plugin/snapshot owners)"
            )
    else:
        violations.append("  coordinator.rs: cannot locate ToolDefCache alias")

    # 6. Adapter single origin still holds (registry list, not service list).
    if "tool_registry.list()" not in text:
        violations.append("  request_preparation.rs: definitions must derive from tool_registry.list() (single origin)")

    if violations:
        print("ERROR: Tool descriptor snapshot/discovery invariants violated (M002).")
        print()
        for v in violations:
            print(v)
        print()
        return 1
    print("OK: Tool descriptor snapshot/discovery invariants hold.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
