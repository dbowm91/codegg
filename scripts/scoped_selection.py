#!/usr/bin/env python3
"""Compute the change-scoped verification selection.

Given a base ref, this computes which workspace members and which test
targets a change can affect, using the machine-readable ownership manifest
`docs/test-ownership.toml`.

It is a **selector**, not a filter: it never decides that a test does not
need to run. It answers "which tests could this change affect?" and the
answer is always a superset of what the change actually affects.

Selection algorithm
-------------------

1. Changed files are computed against the base ref using **merge-base**
   semantics: `git merge-base <base> HEAD` is the comparison point, so a
   branch that has been merged into the base is still compared against the
   commit the branch forked from. Uncommitted work (staged, unstaged, and
   untracked files) is included too — a developer asking "what does my
   change affect?" means everything in their tree, not just what they have
   committed.

2. A changed file inside a member's `root` marks that member affected. The
   longest matching member root wins, so a nested member is not swallowed by
   an enclosing one.

3. An affected member contributes:
     - its library test target, always; and
     - the integration targets of every group whose `paths` pattern matches
       the changed file.

4. A member that **depends on** an affected member is affected too. The
   manifest's `[[dependencies]]` section declares the workspace's dependency
   edges, and the guard proves that the declared edge set is exactly
   `cargo metadata`'s set of workspace-local edges — so an edge cannot be
   silently omitted. A member pulled in this way contributes its **full**
   surface (its library test target and every integration target it owns),
   because the coupling runs through the dependent's own public API and any
   of the dependent's tests can reach the changed code. The pull-in is
   transitive.

5. A changed file inside a member but matching **no** group pattern takes the
   catch-all action: the whole member is widened to its full surface (every
   integration target it owns, plus its library test target). A file the
   manifest does not cover must never be silently skipped.

6. A changed file **outside** every member root — a script, a doc, an asset,
   a workflow — takes the out-of-member action: the whole workspace is
   widened. The repository's change-trigger routing classifies those files
   for the *guards*; the guards still run in full, but no test target can be
   ruled out, so the safe scope is the entire workspace.

7. If the diff cannot be computed (an unknown base ref, a shallow clone with
   no merge base), the selection fails loudly and exits non-zero. It never
   falls back to "no tests".

Output
------

`--format text` (default) prints the scope in prose: the base, the changed
files considered, the members and targets selected, the targets skipped, and
the reason for each skip.

`--format json` prints the same selection as JSON, for `scripts/verify.sh` to
consume.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST = REPO_ROOT / "docs" / "test-ownership.toml"
DEFAULT_BASE = "origin/main"


class SelectionError(RuntimeError):
    """The scope could not be determined safely."""


# ── git facts ───────────────────────────────────────────────────────────────


def _git(*args: str, cwd: Path | None = None) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=cwd or REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise SelectionError(
            f"git {' '.join(args)} failed: {result.stderr.strip() or result.stdout.strip()}"
        )
    return result.stdout


def changed_files(base: str, cwd: Path | None = None) -> tuple[str, list[str]]:
    """The changed files, and the merge base they were computed against."""
    try:
        merge_base = _git("merge-base", base, "HEAD", cwd=cwd).strip()
    except SelectionError as error:
        raise SelectionError(
            f"could not compute a merge base against {base!r}: {error}. "
            f"On a shallow clone, fetch enough history for the base "
            f"(`git fetch --deepen`) or pass an explicit base ref."
        ) from error
    if not merge_base:
        raise SelectionError(
            f"no merge base between {base!r} and HEAD. On a shallow clone, fetch "
            f"enough history for the base or pass an explicit base ref."
        )

    files: set[str] = set()
    for args in (
        ("diff", "--name-only", f"{merge_base}..HEAD"),
        ("diff", "--name-only", "HEAD"),
        ("diff", "--name-only", "--cached", "HEAD"),
        ("ls-files", "--others", "--exclude-standard"),
    ):
        output = _git(*args, cwd=cwd)
        files.update(line for line in output.splitlines() if line.strip())
    return merge_base, sorted(files)


# ── manifest facts ──────────────────────────────────────────────────────────


def load_manifest() -> dict:
    import tomllib

    try:
        with MANIFEST.open("rb") as handle:
            return tomllib.load(handle)
    except FileNotFoundError as error:
        raise SelectionError(
            f"ownership manifest not found at {MANIFEST}. The scoped mode cannot "
            f"determine a safe scope without it."
        ) from error
    except tomllib.TOMLDecodeError as error:
        raise SelectionError(
            f"ownership manifest {MANIFEST} is not valid TOML: {error}"
        ) from error


def member_for_path(path: str, members: list[dict]) -> dict | None:
    """The member that owns a path: the one with the longest matching root."""
    best: dict | None = None
    for member in members:
        root = member.get("root", "").rstrip("/")
        if not root:
            continue
        if path == root or path.startswith(f"{root}/"):
            if best is None or len(root) > len(best.get("root", "").rstrip("/")):
                best = member
    return best


# ── the selection ───────────────────────────────────────────────────────────


def select(base: str, explicit_files: list[str] | None = None) -> dict:
    manifest = load_manifest()
    members = manifest.get("members", []) or []
    groups = manifest.get("groups", []) or []
    dependencies = manifest.get("dependencies", []) or []

    merge_base, files = changed_files(base)
    if explicit_files is not None:
        # An explicit file list replaces the computed diff. This is how the
        # selection algorithm is exercised against a known input, without
        # having to manufacture a matching working tree.
        merge_base = f"(explicit file list; merge base {merge_base})"
        files = sorted(dict.fromkeys(explicit_files))

    # Which groups of which member cover a path.
    by_member: dict[str, list[dict]] = {}
    for group in groups:
        by_member.setdefault(group.get("member", ""), []).append(group)

    affected: dict[str, dict] = {}
    out_of_member: list[str] = []
    unmapped_in_member: list[str] = []

    for path in files:
        member = member_for_path(path, members)
        if member is None:
            out_of_member.append(path)
            continue
        name = member["name"]
        entry = affected.setdefault(
            name,
            {
                "member": member,
                "targets": set(),
                "reasons": [],
                "widen": False,
                "pulled_in": False,
            },
        )
        matched = False
        for group in by_member.get(name, []):
            for pattern in group.get("paths", []) or []:
                if _path_match(pattern, path):
                    matched = True
                    for target_pattern in group.get("targets", []) or []:
                        entry["targets"].add(target_pattern)
                    entry["reasons"].append(f"{path} -> group {group.get('id')}")
        if not matched:
            unmapped_in_member.append(path)
            entry["widen"] = True
            entry["reasons"].append(f"{path} -> no group covers it; member widened")

    widen_workspace = bool(out_of_member)

    # ── dependency pull-in ──────────────────────────────────────────────
    #
    # A member that depends on an affected member is affected too. The
    # pull-in is transitive: a crate that depends on a crate that depends
    # on the changed crate is also affected.
    #
    # A member pulled in this way contributes its FULL surface, because the
    # coupling runs through the dependent's own source and public API.
    member_by_name = {m["name"]: m for m in members}
    dependents: dict[str, set[str]] = {}
    for edge in dependencies:
        dependents.setdefault(edge.get("member", ""), set()).add(
            edge.get("dependent", "")
        )

    # A worklist, not a fixed-point loop over `affected`: a member that is
    # *both* directly affected by a path and pulled in by a dependency must
    # still contribute its FULL surface. Skipping the pull-in because the
    # member is already present would silently drop the dependency widening
    # for exactly the case where a change touches two areas at once.
    pulled_in: dict[str, list[str]] = {}
    seen_edges: set[tuple[str, str]] = set()
    queue: list[str] = list(affected)
    while queue:
        name = queue.pop(0)
        for dependent in sorted(dependents.get(name, set())):
            if dependent not in member_by_name:
                # A dependency edge naming an unknown member is the
                # guard's failure to report, not the selector's to
                # guess about. The guard runs before this mode.
                continue
            edge = (name, dependent)
            if edge in seen_edges:
                continue
            seen_edges.add(edge)
            pulled_in.setdefault(dependent, []).append(name)
            if dependent not in affected:
                affected[dependent] = {
                    "member": member_by_name[dependent],
                    "targets": set(),
                    "reasons": [],
                    "widen": True,
                    "pulled_in": True,
                }
                queue.append(dependent)

    for dependent, sources in pulled_in.items():
        entry = affected[dependent]
        entry["pulled_in"] = True
        entry["reasons"].append(
            "depends on "
            + ", ".join(sorted(sources))
            + "; the dependent's full surface is selected because the coupling "
            "runs through its own source and public API"
        )

    selection_members = []
    for name, entry in sorted(affected.items()):
        member = entry["member"]
        patterns = sorted(entry["targets"])
        concrete = sorted(_expand(patterns, name, manifest))
        if entry.get("pulled_in"):
            # The full surface: every integration target the member owns.
            concrete = sorted(
                set(concrete) | set(_member_targets().get(name, {}).get("integration", []))
            )
        required = sorted(_required_features(name, concrete))
        selection_members.append(
            {
                "name": name,
                "lib_test_target": member.get("lib-test-target"),
                "target_patterns": patterns,
                "targets": concrete,
                "widened": entry["widen"],
                "pulled_in_by_dependency": sorted(pulled_in.get(name, [])),
                "required_features": required,
                "reasons": sorted(set(entry["reasons"])),
            }
        )

    if widen_workspace:
        selection_members = [
            {
                "name": "WORKSPACE",
                "lib_test_target": None,
                "target_patterns": [],
                "targets": [],
                "widened": True,
                "pulled_in_by_dependency": [],
                "required_features": [],
                "reasons": [
                    "a changed file lies outside every workspace member; no test "
                    "target can be ruled out, so the scope widens to the workspace"
                ],
            }
        ]

    return {
        "base": base,
        "merge_base": merge_base,
        "changed_files": files,
        "out_of_member": out_of_member,
        "unmapped_in_member": unmapped_in_member,
        "members": selection_members,
        "widen_workspace": widen_workspace,
    }


def _expand(patterns: list[str], member: str, manifest: dict) -> set[str]:
    """Expand target patterns against the member's real targets."""
    import fnmatch

    facts = _member_targets()
    pool = set(facts.get(member, {}).get("integration", []))
    lib_target = facts.get(member, {}).get("lib-test-target")
    if lib_target:
        pool.add(lib_target)
    matched: set[str] = set()
    for pattern in patterns:
        for name in pool:
            if fnmatch.fnmatchcase(name, pattern):
                matched.add(name)
    return matched


_TARGET_FACTS: dict[str, dict] | None = None


def _member_targets() -> dict[str, dict]:
    global _TARGET_FACTS
    if _TARGET_FACTS is None:
        result = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode != 0:
            raise SelectionError(f"cargo metadata failed: {result.stderr.strip()}")
        metadata = json.loads(result.stdout)
        _TARGET_FACTS = {}
        for package in metadata["packages"]:
            _TARGET_FACTS[package["name"]] = {
                "integration": sorted(
                    t["name"] for t in package["targets"] if t["kind"] == ["test"]
                ),
                "lib-test-target": package["name"].replace("-", "_"),
                # A target gated behind `required-features` cannot be built
                # without them, so the caller has to enable them rather than
                # name a target that will not compile.
                "required-features": {
                    t["name"]: sorted(t.get("required-features", []) or [])
                    for t in package["targets"]
                    if t["kind"] == ["test"]
                },
            }
    return _TARGET_FACTS


def _required_features(member: str, targets: list[str]) -> set[str]:
    """The union of the features the member's selected targets are gated on."""
    facts = _member_targets().get(member, {}).get("required-features", {})
    needed: set[str] = set()
    for target in targets:
        needed.update(facts.get(target, []))
    return needed


def _path_match(pattern: str, path: str) -> bool:
    """`/`-separated glob: `**` crosses separators, `*` and `?` do not."""
    import fnmatch

    def segments(pattern_segments: list[str], path_segments: list[str]) -> bool:
        if not pattern_segments:
            return not path_segments
        head, rest = pattern_segments[0], pattern_segments[1:]
        if head == "**":
            for index in range(len(path_segments) + 1):
                if segments(rest, path_segments[index:]):
                    return True
            return False
        if not path_segments:
            return False
        if not fnmatch.fnmatchcase(path_segments[0], head):
            return False
        return segments(rest, path_segments[1:])

    return segments(pattern.split("/"), path.split("/"))


# ── reporting ───────────────────────────────────────────────────────────────


def members_lines(selection: dict) -> list[str]:
    """The `members` format: one tab-separated line per affected member.

    The columns are `<package>\\t<target> <target> ...\\t<feature,feature>`.

    The third column is the union of the cargo features the member's selected
    targets are gated on. It is empty when no selected target needs a feature.
    The caller (`scripts/verify.sh`) enables exactly those features, because
    cargo refuses to build a gated target without them.

    The column count is part of the contract: the caller reads the line with
    `IFS=$'\t' read -r name targets features`, so a format change that drops
    or adds a column silently changes what the caller runs.
    """
    lines = []
    for member in selection["members"]:
        features = ",".join(member.get("required_features", []))
        lines.append(f"{member['name']}\t{' '.join(member['targets'])}\t{features}")
    return lines


def report_text(selection: dict) -> str:
    lines: list[str] = []
    lines.append("Scoped verification selection")
    lines.append(f"  base:        {selection['base']}")
    lines.append(f"  merge base:  {selection['merge_base']}")
    lines.append(f"  changed files considered: {len(selection['changed_files'])}")
    for path in selection["changed_files"]:
        lines.append(f"    - {path}")

    if selection["out_of_member"]:
        lines.append("")
        lines.append("  Files outside every workspace member:")
        for path in selection["out_of_member"]:
            lines.append(f"    - {path}")
        lines.append(
            "    -> no test target can be ruled out for these; the scope widens to the "
            "whole workspace."
        )

    if selection["unmapped_in_member"]:
        lines.append("")
        lines.append("  Files inside a member but covered by no group:")
        for path in selection["unmapped_in_member"]:
            lines.append(f"    - {path}")
        lines.append("    -> the affected member is widened to its full surface.")

    lines.append("")
    lines.append("  Members and targets selected:")
    if not selection["members"]:
        lines.append("    (none)")
    for member in selection["members"]:
        lines.append(f"    {member['name']}")
        if member["lib_test_target"]:
            lines.append(f"      library test target: {member['lib_test_target']}")
        if member.get("pulled_in_by_dependency"):
            lines.append(
                "      pulled in because it depends on: "
                + ", ".join(member["pulled_in_by_dependency"])
            )
        if member["targets"]:
            lines.append(f"      integration targets ({len(member['targets'])}):")
            for target in member["targets"]:
                lines.append(f"        - {target}")
        else:
            lines.append("      integration targets: (none)")
        if member.get("required_features"):
            lines.append(
                "      required features: " + ", ".join(member["required_features"])
            )
        for reason in member["reasons"]:
            lines.append(f"      because {reason}")
    return "\n".join(lines)


def _self_test_base() -> tuple[str, str]:
    """A ref this self-test can compute a merge base against.

    `origin/main` is the production default and is used whenever it is
    present, so the self-test exercises the production path wherever the
    environment allows it. It is **absent from a shallow CI checkout** —
    `actions/checkout@v4` fetches depth 1 — where `git merge-base
    origin/main HEAD` fails with "Not a valid object name origin/main".
    That is a property of the checkout, not of the selection algorithm, and
    a self-test that fails on it reports a defect that does not exist.

    `HEAD` always exists, and `git merge-base HEAD HEAD` is `HEAD`, so it
    is the fallback. When an explicit file list is supplied — which is how
    the self-test calls `select()` — the merge base is only a reporting
    anchor and does not affect the selection.

    Returns the base and a note describing which one was used, so the run's
    log says which anchor the self-test actually exercised.
    """
    probe = subprocess.run(
        ["git", "rev-parse", "--verify", "--quiet", DEFAULT_BASE],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if probe.returncode == 0:
        return DEFAULT_BASE, f"the production base {DEFAULT_BASE!r}"
    return "HEAD", (
        f"{DEFAULT_BASE!r} is absent from this checkout (a shallow CI "
        f"checkout fetches depth 1), so the merge-base anchor is 'HEAD'"
    )


def _self_test_changed_files() -> list[str]:
    """Prove the changed-file computation against a repository this test makes.

    The selection calls in this self-test pass an **explicit** file list, so
    they never exercise `changed_files()` itself — and that function is the
    one that needs a merge base, which is exactly what a shallow checkout
    cannot provide. A temporary repository gives it real commits to diff,
    and it exists in every environment, so the changed-file path is covered
    rather than skipped.
    """
    failures: list[str] = []
    with tempfile.TemporaryDirectory(prefix="scoped-selection-selftest-") as tmp:
        repo = Path(tmp)

        def run(*args: str) -> str:
            result = subprocess.run(
                ["git", *args],
                cwd=repo,
                capture_output=True,
                text=True,
                check=False,
            )
            if result.returncode != 0:
                raise SelectionError(
                    f"git {' '.join(args)} failed in the fixture: "
                    f"{result.stderr.strip() or result.stdout.strip()}"
                )
            return result.stdout

        try:
            run("init", "-q")
            run("config", "user.email", "selftest@example.invalid")
            run("config", "user.name", "scoped selection self-test")
            (repo / "a.txt").write_text("a\n")
            run("add", "a.txt")
            run("commit", "-q", "-m", "base")
            base = run("rev-parse", "HEAD").strip()
            (repo / "b.txt").write_text("b\n")
            run("add", "b.txt")
            run("commit", "-q", "-m", "head")
            merge_base, files = changed_files(base, cwd=repo)
        except (SelectionError, OSError) as error:
            failures.append(f"the changed-file fixture failed: {error}")
            return failures

        if merge_base != base:
            failures.append(f"expected the merge base {base}, got {merge_base}")
        if files != ["b.txt"]:
            failures.append(f"expected the changed file list ['b.txt'], got {files}")
    return failures


def _self_test() -> int:
    """Prove the selection algorithm against known inputs.

    Four properties are checked, and each of them is a property the caller
    depends on:

    1. the dependency pull-in widens a dependent to its full surface;
    2. the `members` line format has exactly three tab-separated columns, and
       the third is the union of the selected targets' required features;
    3. a member whose targets need no feature produces an **empty** third
       column — the caller expands that array, and an empty expansion under
       `set -u` in bash 3.2 is an error unless the caller guards it;
    4. the changed-file computation resolves a merge base and reports the
       right files — checked against a repository this test creates, so it
       holds in a shallow checkout as well as in a full one.
    """
    failures: list[str] = []

    # 4. The changed-file computation, against a self-made repository.
    failures.extend(_self_test_changed_files())

    # 1. The dependency pull-in.
    selection = {
        "members": [
            {
                "name": "eggsentry",
                "targets": [],
                "required_features": [],
                "pulled_in_by_dependency": [],
            },
            {
                "name": "codegg",
                "targets": ["agent_loop", "tool_programs"],
                "required_features": ["server"],
                "pulled_in_by_dependency": ["eggsentry"],
            },
        ]
    }
    lines = members_lines(selection)
    if len(lines) != 2:
        failures.append(f"expected 2 member lines, got {len(lines)}")
    else:
        name, targets, features = lines[1].split("\t")
        if name != "codegg":
            failures.append(f"expected the dependent 'codegg', got {name!r}")
        if targets != "agent_loop tool_programs":
            failures.append(f"expected the dependent's targets, got {targets!r}")
        if features != "server":
            failures.append(f"expected the required feature 'server', got {features!r}")

    # 2 and 3. The column contract, including the empty-feature case.
    for line in members_lines(selection):
        columns = line.split("\t")
        if len(columns) != 3:
            failures.append(f"member line has {len(columns)} columns, expected 3: {line!r}")
    empty_features = members_lines({"members": [{"name": "eggsentry", "targets": [], "required_features": []}]})[0]
    if not empty_features.endswith("\t"):
        failures.append(
            f"a member with no required feature must end with an empty third column: {empty_features!r}"
        )

    # The real tree's own selection must satisfy the same contract.
    base, base_note = _self_test_base()
    try:
        real = select(base, ["crates/eggsentry/src/command.rs"])
        for line in members_lines(real):
            if len(line.split("\t")) != 3:
                failures.append(f"real selection produced a malformed member line: {line!r}")
        names = {m["name"] for m in real["members"]}
        for expected in ("eggsentry", "codegg-core", "codegg"):
            if expected not in names:
                failures.append(
                    f"a change in eggsentry must pull in {expected!r}; the selection was "
                    f"{sorted(names)}"
                )

        # A member that is BOTH directly affected and pulled in by a dependency
        # must still contribute its full surface. Skipping the pull-in because
        # the member is already present silently drops the dependency widening
        # for exactly the case where a change touches two areas at once.
        both = select(
            base,
            ["crates/eggsentry/src/command.rs", "src/tui/app/mod.rs"],
        )
        root = next((m for m in both["members"] if m["name"] == "codegg"), None)
        if root is None:
            failures.append("the two-area selection did not include the root package")
        else:
            surface = len(_member_targets().get("codegg", {}).get("integration", []))
            if len(root["targets"]) != surface:
                failures.append(
                    f"a root package affected both directly and by dependency must "
                    f"contribute its full surface ({surface} targets); it contributed "
                    f"{len(root['targets'])}"
                )

        # A change confined to the root package's own source must stay
        # precise: the root package has no dependents, so nothing is pulled in.
        root_only = select(base, ["src/tui/app/mod.rs"])
        root_targets = next(
            (m for m in root_only["members"] if m["name"] == "codegg"), None
        )
        if root_targets is None:
            failures.append("the root-only selection did not include the root package")
        elif len(root_targets["targets"]) != 1:
            failures.append(
                f"a root-only change must stay precise (1 target); it selected "
                f"{len(root_targets['targets'])}"
            )
    except SelectionError as error:
        failures.append(f"the real selection failed: {error}")

    for failure in failures:
        print(f"  FAIL: {failure}")
    if failures:
        print(f"scoped_selection --self-test: {len(failures)} failing")
        return 1
    print(f"  ok: the changed-file computation resolves a merge base and the right files")
    print("  ok: the dependency pull-in widens the dependent")
    print("  ok: the `members` line format has exactly three columns")
    print("  ok: a member with no required feature yields an empty third column")
    print("  ok: a member affected both directly and by dependency gets its full surface")
    print("  ok: a root-only change stays precise")
    print("  ok: the real tree's selection satisfies the same contract")
    print(f"  note: merge-base anchor — {base_note}")
    print("scoped_selection --self-test: all checks pass")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--base",
        default=DEFAULT_BASE,
        help=f"the ref to diff against (default: {DEFAULT_BASE})",
    )
    parser.add_argument(
        "--files",
        help=(
            "an explicit comma-separated changed-file list, replacing the computed "
            "diff. Used to exercise the selection algorithm against a known input."
        ),
    )
    parser.add_argument("--format", choices=("text", "json", "members"), default="text")
    parser.add_argument(
        "--self-test",
        action="store_true",
        help=(
            "prove the selection algorithm and the `members` output contract "
            "against known inputs, then exit"
        ),
    )
    args = parser.parse_args()

    if args.self_test:
        return _self_test()

    explicit = None
    if args.files is not None:
        explicit = [path.strip() for path in args.files.split(",") if path.strip()]

    try:
        selection = select(args.base, explicit)
    except SelectionError as error:
        print(f"scoped selection failed: {error}", file=sys.stderr)
        return 2

    if args.format == "json":
        print(json.dumps(selection, indent=2, sort_keys=True))
    elif args.format == "members":
        for line in members_lines(selection):
            print(line)
    else:
        print(report_text(selection))
    return 0


if __name__ == "__main__":
    sys.exit(main())
