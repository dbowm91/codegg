#!/usr/bin/env python3
"""Ensure the provider crate consumes only EggPool's neutral wire package."""

from __future__ import annotations

import subprocess
import sys


def main() -> int:
    result = subprocess.run(
        ["cargo", "tree", "-p", "codegg-providers", "--prefix", "none", "--edges", "normal"],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode:
        sys.stderr.write(result.stderr)
        return result.returncode

    unexpected = []
    for line in result.stdout.splitlines():
        package = line.split(" v", 1)[0]
        if package.startswith("eggpool") and package != "eggpool-wire":
            unexpected.append(package)
    if unexpected:
        sys.stderr.write(
            "codegg-providers must not depend on EggPool runtime/routing packages: "
            + ", ".join(sorted(set(unexpected)))
            + "\n"
        )
        return 1
    if "eggpool-wire" not in result.stdout:
        sys.stderr.write("eggpool-wire is missing from the provider dependency tree\n")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
