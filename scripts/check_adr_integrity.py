#!/usr/bin/env python3
"""Check active ADR number uniqueness and repository-local ADR path references."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ADR_NAME = re.compile(r"ADR-(\d{4})-[a-z0-9][a-z0-9-]*\.md$")
ADR_PATH = re.compile(r"plans/adrs/ADR-\d{4}-[a-z0-9][a-z0-9-]*\.md")


def active_adrs(root: Path) -> tuple[dict[str, Path], list[str]]:
    directory = root / "plans/adrs"
    by_number: dict[str, Path] = {}
    errors: list[str] = []
    if not directory.is_dir():
        return {}, [f"missing active ADR directory: {directory}"]
    for path in sorted(directory.glob("ADR-*.md")):
        match = ADR_NAME.fullmatch(path.name)
        if not match:
            errors.append(f"invalid active ADR filename: {path.relative_to(root)}")
            continue
        number = match.group(1)
        if number in by_number:
            errors.append(
                f"duplicate active ADR-{number}: {by_number[number].relative_to(root)} and "
                f"{path.relative_to(root)}"
            )
        else:
            by_number[number] = path
    return by_number, errors


def check(root: Path) -> list[str]:
    active, errors = active_adrs(root)
    active_paths = {path.relative_to(root).as_posix() for path in active.values()}
    for path in sorted(root.rglob("*.md")):
        relative = path.relative_to(root)
        if ".git" in relative.parts or "target" in relative.parts or "archive" in relative.parts:
            continue
        try:
            content = path.read_text(encoding="utf-8")
        except (UnicodeError, OSError) as exc:
            errors.append(f"cannot read {relative}: {exc}")
            continue
        for reference in ADR_PATH.findall(content):
            if reference not in active_paths:
                errors.append(f"dangling ADR path in {relative}: {reference}")
    return errors


def self_test() -> list[str]:
    import tempfile

    failures: list[str] = []
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        adrs = root / "plans/adrs"
        adrs.mkdir(parents=True)
        merged_names = [
            "ADR-0021-native-editor-completion-read-and-local-edit.md",
            "ADR-0022-daemon-authorized-workspace-directory-listing.md",
            "ADR-0023-windows-privileged-launch-vs-guest-isolation.md",
            "ADR-0024-windows-two-stage-native-launch-and-mxc-requalification.md",
            "ADR-0025-windows-mxc-helper-diagnostic-corrective.md",
            "ADR-0026-windows-mxc-capture-and-handle-semantics-review.md",
            "ADR-0027-windows-sandbox-capability-successors.md",
            "ADR-0028-windows-broad-read-compatibility-profile.md",
            "ADR-0029-windows-sandbox-capability-successors-renumbered.md",
            "ADR-0030-windows-broad-read-profile-renumbered.md",
            "ADR-0031-authenticated-idle-continuation-delegated-turn-admission.md",
            "ADR-0032-command-syntax-and-execution-authority.md",
            "ADR-0033-directory-tied-project-identity-and-client-working-directory.md",
            "ADR-0034-command-syntax-and-execution-authority.md",
            "ADR-0035-composable-linux-execution-isolation.md",
        ]
        for name in merged_names:
            (adrs / name).write_text(f"# {name}\n", encoding="utf-8")
        (root / "plans/active.md").write_text(
            "See `plans/adrs/ADR-0021-native-editor-completion-read-and-local-edit.md` and "
            "`plans/adrs/ADR-0033-directory-tied-project-identity-and-client-working-directory.md`, "
            "and `plans/adrs/ADR-0035-composable-linux-execution-isolation.md`.\n",
            encoding="utf-8",
        )
        (root / "plans/archive/windows-adr-id-aliases").mkdir(parents=True)
        (root / "plans/archive/windows-adr-id-aliases/ADR-0001-alias.md").write_text(
            "Historical `plans/adrs/ADR-9999-not-an-active-link.md`.\n", encoding="utf-8"
        )
        (root / "plans/archive/linux-command-adr-id-aliases/ADR-0027-alias.md").parent.mkdir(
            parents=True, exist_ok=True
        )
        (root / "plans/archive/linux-command-adr-id-aliases/ADR-0027-alias.md").write_text(
            "Archived alias; see migration manifest.\n", encoding="utf-8"
        )
        if check(root):
            failures.append("valid active ADR plus archived alias should pass")
        active, active_errors = active_adrs(root)
        expected = {"0027", "0028", "0029", "0030", "0031", "0032", "0033"}
        if active_errors or not expected.issubset(active):
            failures.append("merged Windows/Linux/command ADR fixture should have unique identities")
        (adrs / "ADR-0023-second.md").write_text("# duplicate\n", encoding="utf-8")
        if not any("duplicate active ADR-0023" in error for error in check(root)):
            failures.append("duplicate active ADR number should fail")
        (adrs / "ADR-0023-second.md").unlink()
        (adrs / "ADR-0032-second.md").write_text("# duplicate command ID\n", encoding="utf-8")
        if not any("duplicate active ADR-0032" in error for error in check(root)):
            failures.append("duplicate command ADR identity should fail")
        (adrs / "ADR-0032-second.md").unlink()
        (root / "plans/active.md").write_text(
            "See `plans/adrs/ADR-0027-missing.md`.\n", encoding="utf-8"
        )
        if not any("dangling ADR path" in error for error in check(root)):
            failures.append("dangling active ADR link should fail")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--self-test", action="store_true", help="run synthetic fixtures")
    args = parser.parse_args()
    if args.self_test:
        failures = self_test()
        if failures:
            print("ADR integrity fixture failures:", file=sys.stderr)
            for failure in failures:
                print(f"- {failure}", file=sys.stderr)
            return 1
        print("ADR integrity fixtures passed")
        return 0
    fixture_failures = self_test()
    if fixture_failures:
        print("ADR integrity fixture failures:", file=sys.stderr)
        for failure in fixture_failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    errors = check(args.root.resolve())
    if errors:
        print("ADR integrity check failed:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1
    print("ADR integrity fixtures passed; active ADR IDs are unique and local paths resolve")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
