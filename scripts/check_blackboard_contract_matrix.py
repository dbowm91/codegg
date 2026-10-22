#!/usr/bin/env python3
"""Guard: the blackboard contract matrix must stay frozen and reachable.

WHAT THIS GUARD DOES
    Milestone M008 compares five agent configurations (S0-S4) against each
    other. A comparison is only meaningful when every arm runs the same
    contract: the same group membership rules, the same claim semantics, the
    same peer-observation trust class, the same checkout-lease authority and
    the same build-admission bound. The frozen statement of that contract is
    ``architecture/blackboard_contract_matrix.md``, and this guard is what
    keeps it frozen.

    Specifically it checks that:

    1. Every one of the six capabilities the blackboard added is still named
       in the matrix. Losing a row is not a formatting change; it is a
       capability that stopped being part of the comparison.
    2. Every row still names a **production consumer**: a file in the tree,
       and a distinctive marker of the call site that consumes the capability.
    3. Every consumer marker survives the removal of the file's test-only
       modules. A consumer that has retreated into a ``#[cfg(test)]`` module
       is no longer a production consumer, and a matrix that still claims it
       is one is comparing configurations against a contract the tree no
       longer implements.
    4. Every consumer marker is a real marker, not an empty string, and not a
       substring so short that it matches anything.

WHAT THIS GUARD DELIBERATELY DOES NOT DO
    It does not prove that any capability *works*. That is the fixture suite's
    job (``tests/blackboard_trajectory_m008.rs`` and the per-milestone
    closures). This guard proves the wiring is still there, which is the
    failure mode a static check can honestly see.

    It does not pin line numbers. A line number in a frozen table is a
    point-in-time fact that goes stale when an unrelated line is inserted
    above it, and the point-in-time facts belong in the closure record.

    It does not require the shared-checkout profile to be available on every
    platform. The platform gate is part of the contract.

USAGE
    python3 scripts/check_blackboard_contract_matrix.py            # check the tree
    python3 scripts/check_blackboard_contract_matrix.py --self-test  # prove it bites
"""

from __future__ import annotations

import argparse
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import check_writer_disposition as disposition  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
MATRIX = ROOT / "architecture" / "blackboard_contract_matrix.md"

# The six capabilities M001-M007 added. The plan's work package A names
# them explicitly: "Require each new group join, blackboard claim, peer
# event, session-worktree launch, group lease and build optimization to have
# a production consumer."
REQUIRED_CAPABILITIES: tuple[str, ...] = (
    "group join",
    "blackboard claim",
    "peer event",
    "session-worktree launch",
    "group lease",
    "build optimization",
)

# The header of the matrix table. The guard finds the table by its header
# rather than by a line number, so inserting a section above the table does
# not silently point the guard at a different table.
TABLE_HEADER = (
    "| Capability | Authority | Production consumer | User-reachable path | Evidence |"
)

# A marker shorter than this would match by accident in almost any file.
MIN_MARKER_LENGTH = 12


class MatrixError(Exception):
    """A defect in the contract matrix itself."""


def _split_row(row: str) -> list[str]:
    """Split a markdown table row into its cells."""
    cells = row.strip().strip("|").split("|")
    return [cell.strip() for cell in cells]


def _backticked(cell: str) -> list[str]:
    """The backticked spans of a cell, in order."""
    spans: list[str] = []
    inside = False
    current: list[str] = []
    for char in cell:
        if char == "`":
            if inside:
                spans.append("".join(current))
                current = []
            inside = not inside
            continue
        if inside:
            current.append(char)
    return spans


def parse_matrix(text: str) -> dict[str, tuple[str, str]]:
    """The matrix rows, as ``capability -> (consumer file, marker)``.

    Raises :class:`MatrixError` when the table is missing or malformed. A
    matrix the guard cannot parse is a matrix the guard cannot defend, so it
    fails loudly rather than checking an empty set.
    """
    lines = text.splitlines()
    header_index = None
    for index, line in enumerate(lines):
        if line.strip() == TABLE_HEADER:
            header_index = index
            break
    if header_index is None:
        raise MatrixError(
            f"{MATRIX.relative_to(ROOT)} does not contain the contract matrix table "
            f"(header {TABLE_HEADER!r} not found)"
        )

    rows: dict[str, tuple[str, str]] = {}
    for line in lines[header_index + 2 :]:
        stripped = line.strip()
        if not stripped.startswith("|"):
            break
        cells = _split_row(stripped)
        if len(cells) != 5:
            raise MatrixError(
                f"contract matrix row has {len(cells)} cells, expected 5: {stripped}"
            )
        capability = cells[0].strip()
        if not capability:
            raise MatrixError(f"contract matrix row has an empty capability: {stripped}")
        consumer_cell = cells[2]
        spans = _backticked(consumer_cell)
        if len(spans) < 2:
            raise MatrixError(
                f"contract matrix row {capability!r} does not name a consumer file and "
                f"marker in backticks: {consumer_cell!r}"
            )
        consumer_file = spans[0]
        marker = spans[1]
        rows[capability] = (consumer_file, marker)
    if not rows:
        raise MatrixError("the contract matrix table has no rows")
    return rows


def check_matrix(rows: dict[str, tuple[str, str]], root: Path) -> list[str]:
    """The defects in the matrix, as human-readable messages."""
    problems: list[str] = []

    for capability in REQUIRED_CAPABILITIES:
        if capability not in rows:
            problems.append(
                f"the contract matrix no longer names the {capability!r} capability; "
                f"every capability M001-M007 added must stay in the matrix"
            )

    for capability, (consumer_file, marker) in rows.items():
        if not consumer_file:
            problems.append(f"contract matrix row {capability!r} names an empty consumer file")
            continue
        if not marker:
            problems.append(f"contract matrix row {capability!r} names an empty consumer marker")
            continue
        if len(marker) < MIN_MARKER_LENGTH:
            problems.append(
                f"contract matrix row {capability!r} names a marker too short to be "
                f"distinctive ({marker!r}, {len(marker)} characters, minimum "
                f"{MIN_MARKER_LENGTH})"
            )
            continue

        source = root / consumer_file
        if not source.is_file():
            problems.append(
                f"contract matrix row {capability!r} names a consumer file that does "
                f"not exist: {consumer_file}"
            )
            continue

        text = source.read_text(encoding="utf-8")
        if marker not in text:
            problems.append(
                f"contract matrix row {capability!r} names a consumer marker that is no "
                f"longer present in {consumer_file}: {marker!r}"
            )
            continue

        production = disposition.strip_test_modules(text)
        if marker not in production:
            problems.append(
                f"contract matrix row {capability!r} names a consumer marker that now "
                f"exists only in a test-only module of {consumer_file}: {marker!r}"
            )

    return problems


def run_self_test() -> int:
    """Prove the guard rejects every class of drift it claims to catch."""
    failures: list[str] = []

    def expect_rejected(
        label: str, rows: dict[str, tuple[str, str]], root: Path = ROOT
    ) -> None:
        problems = check_matrix(rows, root)
        if not problems:
            failures.append(f"self-test: {label} was accepted but should have been rejected")
        else:
            print(f"  ok: rejected {label}")

    def expect_accepted(
        label: str, rows: dict[str, tuple[str, str]], root: Path = ROOT
    ) -> None:
        problems = check_matrix(rows, root)
        if problems:
            failures.append(
                f"self-test: {label} was rejected but should have been accepted: {problems}"
            )
        else:
            print(f"  ok: accepted {label}")

    try:
        baseline = parse_matrix(MATRIX.read_text(encoding="utf-8"))
    except (MatrixError, OSError) as error:
        print(f"self-test: cannot read the baseline matrix: {error}", file=sys.stderr)
        return 1

    expect_accepted("the real matrix", baseline)

    # 1. A capability row disappears.
    drifted = dict(baseline)
    drifted.pop("group lease", None)
    expect_rejected("a missing capability row", drifted)

    # 2. The consumer file does not exist.
    drifted = dict(baseline)
    drifted["group join"] = ("src/tool/no_such_file.rs", ".create(NewAgentRunGroup {")
    expect_rejected("a consumer file that does not exist", drifted)

    # 3. The consumer marker is gone from the file.
    drifted = dict(baseline)
    drifted["blackboard claim"] = ("src/tool/task.rs", ".claim(NoSuchClaimType {")
    expect_rejected("a consumer marker that no longer exists", drifted)

    # 4. The consumer marker is too short to be distinctive.
    drifted = dict(baseline)
    drifted["build optimization"] = ("src/scheduler/submission.rs", "apply_res")
    expect_rejected("a marker too short to be distinctive", drifted)

    # 5. The consumer marker is empty.
    drifted = dict(baseline)
    drifted["peer event"] = ("src/agent/follow_up.rs", "")
    expect_rejected("an empty consumer marker", drifted)

    # 6. The consumer marker exists only inside a test-only module. The
    #    marker is planted in a **copy** of a real consumer file inside a
    #    temporary tree, so the self-test proves the production check
    #    against a real file's shape while leaving the working tree
    #    untouched.
    with tempfile.TemporaryDirectory() as scratch:
        scratch_root = Path(scratch)
        copy = scratch_root / "src" / "agent" / "follow_up.rs"
        copy.parent.mkdir(parents=True)
        copy.write_text(
            (ROOT / "src" / "agent" / "follow_up.rs").read_text(encoding="utf-8")
            + (
                "\n\n#[cfg(test)]\nmod matrix_self_test_probe {\n"
                "    fn inject_peer_observations() {}\n"
                "}\n"
            ),
            encoding="utf-8",
        )
        drifted = dict(baseline)
        drifted["peer event"] = ("src/agent/follow_up.rs", "fn inject_peer_observations() {}")
        expect_rejected(
            "a consumer marker that lives only in a test module", drifted, scratch_root
        )

    # 7. The table itself is gone.
    try:
        parse_matrix("no table here\n")
    except MatrixError:
        print("  ok: rejected a matrix with no table")
    else:
        failures.append("self-test: a matrix with no table was accepted")

    # 8. A row with the wrong number of cells.
    try:
        parse_matrix(
            f"{TABLE_HEADER}\n|---|---|---|---|\n| group join | a | b |\n"
        )
    except MatrixError:
        print("  ok: rejected a row with the wrong number of cells")
    else:
        failures.append("self-test: a malformed row was accepted")

    if failures:
        for failure in failures:
            print(f"self-test failure: {failure}", file=sys.stderr)
        return 1
    print("blackboard-contract-matrix self-test ok")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects the drift it claims to catch",
    )
    args = parser.parse_args()

    if args.self_test:
        return run_self_test()

    try:
        rows = parse_matrix(MATRIX.read_text(encoding="utf-8"))
    except (MatrixError, OSError) as error:
        print(f"blackboard contract matrix: {error}", file=sys.stderr)
        return 1

    problems = check_matrix(rows, ROOT)
    if problems:
        print("blackboard contract matrix: FAILED", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 1

    print(
        "blackboard-contract-matrix check ok: "
        f"{len(REQUIRED_CAPABILITIES)} capabilities, {len(rows)} matrix rows, "
        "every consumer present in production code"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
