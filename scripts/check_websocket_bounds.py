#!/usr/bin/env python3
"""Reject unbounded outbound channels in server WebSocket adapters."""

from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parent.parent
SERVER_ROOT = ROOT / "src" / "server"


def main() -> int:
    failures: list[str] = []
    for path in sorted(SERVER_ROOT.rglob("*.rs")):
        content = path.read_text()
        if re.search(r"(?:tokio::sync::)?mpsc::unbounded_channel", content):
            failures.append(
                f"{path.relative_to(ROOT)}: unbounded_channel is forbidden in server WebSocket adapters"
            )

    if failures:
        for failure in failures:
            print(f"ERROR: {failure}")
        return 1

    print("OK: server WebSocket adapters use bounded outbound channels.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
