#!/usr/bin/env python3
"""C001 guard: AgentLoop dispatches once; only ToolBroker may retry."""
from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def main() -> int:
    path = ROOT / "src/agent/tool_batch.rs"
    try:
        source = path.read_text()
    except OSError as error:
        print(f"ERROR: cannot read {path.relative_to(ROOT)}: {error}")
        return 1

    failures: list[str] = []
    if "broker_for_exec" not in source or ".execute(registry, &tool_name_clone" not in source:
        failures.append("src/agent/tool_batch.rs must dispatch the model call through ToolBroker.execute")
    if ".execute_with_retry(" in source:
        failures.append("src/agent/tool_batch.rs must not start a second retry chain")
    if "no qualified retry chain" not in source:
        failures.append("src/agent/tool_batch.rs must document single-dispatch retry ownership")

    if failures:
        print("ERROR: AgentLoop retry admission violates the one-dispatch contract.")
        for failure in failures:
            print(f"  {failure}")
        return 1
    print("OK: AgentLoop dispatches once; retry admission stays in ToolBroker.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
