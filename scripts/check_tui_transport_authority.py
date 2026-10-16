#!/usr/bin/env python3
"""Static guard for TUI transport selection authority.

`AppMode::RemoteCore` names two different frontends:

- the WebSocket attach path (a TUI served over `/tui`), which owns
  `remote_send_tx` and carries messages through that channel; and
- the socket-daemon path (the ordinary local `codegg` TUI talking to the
  local daemon), which owns only `core_client` and leaves
  `remote_send_tx` unset.

Selecting a transport on the mode alone sends socket-daemon traffic into
an absent channel. `send_remote_message` treats a missing channel as a
`tracing::warn!` and drops the message, so the failure is invisible in
the UI.

That has hung turns twice:

- prompt submission left the session pinned in `Working` forever; and
- a permission answer was dropped after the dialog was torn down, so the
  daemon-side waiter stayed parked on an unbounded human wait and
  approving the tool did nothing.

The authoritative transport test is `App::is_ws_attach_transport()`
(mode **and** an owned `remote_send_tx`). Every site that routes a
message over `remote_send_tx` must be gated by it.

This scans for `send_remote_message` call sites whose enclosing branch
condition binds the decision to `AppMode::RemoteCore` without also
requiring the channel. Pure mode reads that are not transport decisions
(daemon-owned storage and preferences) are not flagged.

Exit code 1 if a mode-only transport gate is reintroduced.

Usage::

    python3 scripts/check_tui_transport_authority.py
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SRC = REPO_ROOT / "src" / "tui"

SEND_CALL = re.compile(r"send_remote_message\s*\(")
BRANCH = re.compile(r"^\s*(?:}\s*else\s+)?(?:if|else\s+if|match)\b")
MODE_GATE = re.compile(r"AppMode::RemoteCore")
AUTHORITATIVE = re.compile(r"is_ws_attach_transport")

# How far above a send call to look for the branch that guards it. The
# transport decisions in this codebase are short `if`/`else if` blocks;
# a longer span risks binding an unrelated condition.
LOOKBACK = 12


@dataclass(frozen=True)
class Finding:
    file: Path
    line_no: int
    line_text: str
    gate_line: int
    gate_text: str


def collect_findings() -> list[Finding]:
    findings: list[Finding] = []
    for path in sorted(SRC.rglob("*.rs")):
        try:
            lines = path.read_text().splitlines()
        except UnicodeDecodeError:
            continue
        for i, line in enumerate(lines):
            if not SEND_CALL.search(line):
                continue
            # Walk back to the nearest enclosing branch condition.
            for j in range(i - 1, max(-1, i - LOOKBACK), -1):
                candidate = lines[j]
                if not BRANCH.match(candidate):
                    continue
                if MODE_GATE.search(candidate) and not AUTHORITATIVE.search(candidate):
                    findings.append(
                        Finding(path, i + 1, line.strip(), j + 1, candidate.strip())
                    )
                break
    return findings


def main() -> int:
    findings = collect_findings()
    if findings:
        print(
            "Mode-only TUI transport gates found.\n"
            "`AppMode::RemoteCore` covers both the WebSocket attach frontend\n"
            "(owns `remote_send_tx`) and the socket-daemon frontend (owns only\n"
            "`core_client`). Selecting a transport on the mode alone drops the\n"
            "message into an absent channel, where `send_remote_message` warns\n"
            "and discards it — hanging prompts and permission approvals with no\n"
            "visible error.\n"
            "Gate every `send_remote_message` on `App::is_ws_attach_transport()`.\n"
        )
        for f in findings:
            rel = f.file.relative_to(REPO_ROOT)
            print(f"  {rel}:{f.line_no} gated by {rel}:{f.gate_line}")
            print(f"      gate: {f.gate_text}")
            print(f"      call: {f.line_text}")
        print(f"\n{len(findings)} violation(s) found.")
        return 1
    print(
        "TUI transport-authority guard passed — "
        "every remote send is gated on is_ws_attach_transport()"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())