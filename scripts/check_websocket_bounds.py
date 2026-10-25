#!/usr/bin/env python3
"""Reject unbounded outbound channels and unbounded projection payloads.

Two properties are checked:

1. No server WebSocket adapter may use an unbounded channel.
2. Every bounded projection payload that can cross a WebSocket frame must
   fit the frame. The delegated run transcript is the payload this check
   was extended for: a session snapshot carries at most
   ``MAX_PROJECTION_AGENT_RUNS`` run summaries, each carrying at most
   ``MAX_PROJECTION_TRANSCRIPT_BYTES`` of transcript body, so the worst
   case is the product of the two. A bound that is declared but not
   covered here is a bound nothing holds.
"""

from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parent.parent
SERVER_ROOT = ROOT / "src" / "server"
LIMITS = ROOT / "crates" / "codegg-protocol" / "src" / "projection" / "limits.rs"
WS = ROOT / "src" / "server" / "ws.rs"


def read_const(path: Path, name: str) -> int | None:
    """Read `const NAME: usize = <integer>;` out of a Rust source file.

    The declaration may or may not be `pub`: the WebSocket frame bound is
    private to its module, and the projection bounds are public.

    Returns ``None`` when the constant is absent, which the callers treat
    as a failure rather than as "no bound": a missing bound is an
    unbounded payload wearing a constant's name.
    """
    if not path.exists():
        return None
    pattern = re.compile(
        rf"(?:pub )?const {re.escape(name)}\s*:\s*usize\s*=\s*([0-9_ *]+)\s*;",
    )
    for line in path.read_text().splitlines():
        match = pattern.search(line)
        if match:
            return evaluate_usize(match.group(1))
    return None


def evaluate_usize(expression: str) -> int | None:
    """Evaluate a `usize` constant written as a product of decimal literals.

    Only digits, underscores, `*` and spaces are accepted, so this is an
    arithmetic evaluator and not a general expression parser. A factor that
    is not a decimal literal makes the whole constant unreadable, which the
    callers report as a missing bound.
    """
    factors = expression.replace("_", "").split("*")
    total = 1
    for factor in factors:
        factor = factor.strip()
        if not factor.isdigit():
            return None
        total *= int(factor)
    return total


def transcript_bound_failures(
    transcript_bytes: int | None, agent_runs: int | None, ws_max: int | None
) -> list[str]:
    """The failures implied by the delegated run transcript's declared bound."""
    failures: list[str] = []
    if transcript_bytes is None:
        failures.append(
            f"{LIMITS.relative_to(ROOT)}: MAX_PROJECTION_TRANSCRIPT_BYTES is not declared; "
            "the delegated run transcript payload has no declared bound"
        )
    if agent_runs is None:
        failures.append(
            f"{LIMITS.relative_to(ROOT)}: MAX_PROJECTION_AGENT_RUNS is not declared; "
            "the number of run summaries a snapshot may carry is unknown"
        )
    if ws_max is None:
        failures.append(
            f"{WS.relative_to(ROOT)}: WS_MAX_MESSAGE_SIZE is not declared; "
            "the WebSocket frame bound is unknown"
        )
    if failures:
        return failures
    worst_case = transcript_bytes * agent_runs
    if worst_case > ws_max:
        failures.append(
            f"{LIMITS.relative_to(ROOT)}: the delegated run transcript payload is not covered by "
            f"the WebSocket bound: {agent_runs} runs x {transcript_bytes} bytes = "
            f"{worst_case} bytes, over WS_MAX_MESSAGE_SIZE ({ws_max} bytes)"
        )
    return failures


def check_unbounded_channels() -> list[str]:
    failures: list[str] = []
    for path in sorted(SERVER_ROOT.rglob("*.rs")):
        content = path.read_text()
        if re.search(r"(?:tokio::sync::)?mpsc::unbounded_channel", content):
            failures.append(
                f"{path.relative_to(ROOT)}: unbounded_channel is forbidden in server WebSocket adapters"
            )
    return failures


def self_test() -> int:
    """Prove the transcript bound check is not vacuous.

    The in-bound case must pass and the over-bound case must fail, using the
    same function the real run uses.
    """
    failures: list[str] = []
    if transcript_bound_failures(8 * 1024, 64, 4 * 1024 * 1024):
        failures.append("self-test: an in-bound transcript payload was rejected")
    if not transcript_bound_failures(8 * 1024, 64, 1024):
        failures.append("self-test: an over-bound transcript payload was accepted")
    if not transcript_bound_failures(None, 64, 4 * 1024 * 1024):
        failures.append("self-test: a missing transcript bound was accepted")
    for failure in failures:
        print(f"ERROR: {failure}")
    if failures:
        return 1
    print("OK: the transcript bound check rejects an over-bound and an undeclared bound.")
    return 0


def main() -> int:
    if len(sys.argv) > 1 and sys.argv[1] == "--self-test":
        return self_test()
    failures = check_unbounded_channels()
    failures.extend(
        transcript_bound_failures(
            read_const(LIMITS, "MAX_PROJECTION_TRANSCRIPT_BYTES"),
            read_const(LIMITS, "MAX_PROJECTION_AGENT_RUNS"),
            read_const(WS, "WS_MAX_MESSAGE_SIZE"),
        )
    )
    if failures:
        for failure in failures:
            print(f"ERROR: {failure}")
        return 1

    print("OK: server WebSocket adapters use bounded outbound channels.")
    print("OK: the delegated run transcript payload fits the WebSocket frame bound.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
