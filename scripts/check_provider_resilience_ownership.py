#!/usr/bin/env python3
"""Prove production provider/session paths never instantiate FallbackProvider (C003)."""

from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]

# Files allowed to construct FallbackProvider: its own definition/unit tests
# plus any integration-test harness. Everything else (production provider,
# session, agent turn, core) must not acquire a second failover owner.
ALLOW_SUBSTRINGS = (
    "crates/codegg-providers/src/fallback.rs",
    "/tests/",
    "tests.rs",
)


def is_allowed(relative: str) -> bool:
    return any(token in relative for token in ALLOW_SUBSTRINGS)


def main() -> int:
    failures = []
    for path in ROOT.rglob("*.rs"):
        # Skip archived/historical records; only live code matters.
        if "plans/archive" in path.as_posix():
            continue
        try:
            source = path.read_text()
        except OSError:
            continue
        if "FallbackProvider::new" not in source:
            continue
        relative = path.relative_to(ROOT).as_posix()
        if is_allowed(relative):
            continue
        failures.append(
            f"{relative} constructs FallbackProvider outside the library/test allowlist; "
            "production retry ownership lives in src/agent/provider_turn.rs (C003)"
        )
    if failures:
        sys.stderr.write("\n".join(failures) + "\n")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
