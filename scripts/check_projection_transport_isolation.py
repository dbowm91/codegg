#!/usr/bin/env python3
"""Guard projection-private transport delivery against raw-broadcast drift."""

from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parent.parent
TRANSPORT_FILES = (
    ROOT / "src/server/ws.rs",
    ROOT / "src/core/transport/daemon_socket.rs",
)


def main() -> int:
    failures: list[str] = []
    for path in TRANSPORT_FILES:
        content = path.read_text()
        label = path.relative_to(ROOT)

        if "daemon.subscribe()" in content and "ProjectionStreamEvent { .. }" not in content:
            failures.append(f"{label}: raw daemon subscription lacks projection filtering")

        if path.name == "ws.rs":
            converter = re.search(
                r"fn convert_core_event_to_tui[\s\S]*?\n}\n", content
            )
            if converter is None or "ProjectionStreamEvent { .. } => None" not in converter.group(0):
                failures.append(
                    f"{label}: convert_core_event_to_tui must reject ProjectionStreamEvent"
                )

        for pattern in (
            r"ProjectionStreamId\s*\(\s*[^)]*subscription",
            r"ProjectionStreamId::new\([^)]*subscription",
            r"ProjectionStreamId::new\([^)]*sub_id",
        ):
            if re.search(pattern, content, re.IGNORECASE):
                failures.append(
                    f"{label}: stream identity must come from the persisted descriptor, not a subscription id"
                )
                break

    if failures:
        for failure in failures:
            print(f"ERROR: {failure}")
        return 1

    print("OK: projection-private events have owned receiver and identity guards.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
