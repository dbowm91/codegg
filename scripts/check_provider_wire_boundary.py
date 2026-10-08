#!/usr/bin/env python3
"""Ensure the provider crate consumes only EggPool's neutral contract packages.

`codegg-providers` must not depend on EggPool runtime, account, credential,
router, or transport packages: CodeGG owns those. It may depend only on the
neutral, secret-free, sans-I/O contract crates.

The permitted set is explicit and evidence-backed, not a prefix match:

* `eggpool-wire` — wire grammar, codec, and stream authority.
* `eggpool-provider-profile` — the neutral provider-profile contract and the
  single canonical bundled profile asset. Admitted at the EggPool Shared
  Provider Profile Contract M001 closure revision
  (`9ac6a1318e8db3c034b5ab54987317752d5ffea6`). Its closure record
  (`plans/closure/shared-provider-profile-contract/001-status.md`) states its
  `[dependencies]` are exactly `eggpool-wire`, `serde`, `thiserror`, and `toml`,
  with no env/fs/net/process/time, HTTP, DB, or logging imports.

Adding a crate to `PERMITTED` is a boundary change and must cite that evidence
in the entry. Anything else under the `eggpool` namespace is rejected.
"""

from __future__ import annotations

import subprocess
import sys

# Neutral, sans-I/O contract crates CodeGG is allowed to consume.
# Each entry records why it is safe to admit.
PERMITTED = {
    "eggpool-wire": "wire grammar/codec/stream authority",
    "eggpool-provider-profile": (
        "neutral secret-free provider-profile contract; deps are exactly "
        "eggpool-wire, serde, thiserror, toml (EggPool M001 closure record)"
    ),
}


def provider_tree() -> str:
    result = subprocess.run(
        ["cargo", "tree", "-p", "codegg-providers", "--prefix", "none", "--edges", "normal"],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode:
        sys.stderr.write(result.stderr)
        raise SystemExit(result.returncode)
    return result.stdout


def main() -> int:
    stdout = provider_tree()

    unexpected = []
    present = set()
    for line in stdout.splitlines():
        package = line.split(" v", 1)[0]
        if not package.startswith("eggpool"):
            continue
        present.add(package)
        if package not in PERMITTED:
            unexpected.append(package)

    if unexpected:
        sys.stderr.write(
            "codegg-providers must not depend on EggPool runtime/routing/account packages "
            "(permitted neutral contracts: "
            + ", ".join(sorted(PERMITTED))
            + "); found: "
            + ", ".join(sorted(set(unexpected)))
            + "\n"
        )
        return 1

    missing = {"eggpool-wire", "eggpool-provider-profile"} - present
    if missing:
        sys.stderr.write(
            "required EggPool neutral contract missing from the provider dependency tree: "
            + ", ".join(sorted(missing))
            + "\n"
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())