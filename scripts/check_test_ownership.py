#!/usr/bin/env python3
"""Static guard: the test-ownership manifest must stay honest about the tree.

`docs/test-ownership.toml` maps source paths to the workspace member that
owns them and to the test targets that cover them. It is the machine-readable
input to the change-scoped verification mode
(`scripts/scoped_selection.py`), and it is only useful if it is true.

This guard fails when:

- the manifest's schema is malformed, or its `schema-version` is not one this
  guard understands (a consumer must refuse a version it does not know rather
  than guess);
- a member entry names a workspace package that does not exist, or a
  workspace package has no member entry, or a member is declared twice;
- a group names a member that is not declared;
- a group id is used twice;
- a group's source pattern matches no file in the tree (an entry that can
  never fire);
- a group's target pattern matches no test target of its member (an entry
  that names a target that does not exist);
- a real test target exists that no group owns (a test that can never be
  selected, which is how a scoped run silently stops covering it);
- a [[dependencies]] entry names an undeclared member, names the same crate on
  both sides, or is declared more than once;
- a [[dependencies]] entry is not a real workspace dependency (a fabricated
  widening);
- a real workspace dependency has no [[dependencies]] entry (a silent
  under-selection: a change to one crate reaches another crate's tests, and the
  coupling runs through the dependent's own source, so no path rule can see it).

A member's **library test target** is owned by the member entry itself: an
affected member always contributes it. A group may name it as well — the root
package's `root_catch_all` group does — so the pool a group's target pattern
is matched against is the member's integration test targets *plus* its
library test target.

Run with `--self-test` to prove the guard rejects each violation class. A
guard that has never been shown to reject anything is a comment.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import fnmatch
import json
import os
import re
import subprocess
import sys
import tempfile

import tomllib

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST = REPO_ROOT / "docs" / "test-ownership.toml"
SCHEMA_VERSION = 1

# Directories that are never source and can be enormous. Pruned during the
# walk so the guard stays fast on a tree with a populated `target/`.
PRUNED_DIRS = {
    ".git",
    ".venv",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".next",
    "node_modules",
    "target",
    "dist",
}


# ── glob semantics ──────────────────────────────────────────────────────────
#
# Path patterns: `/`-separated segments. `**` matches zero or more segments.
# Within a segment, `*` matches any run of characters and `?` matches one —
# neither crosses a `/`.
#
# Target patterns: plain fnmatch over a target name. A target name never
# contains a `/`, so `*` crossing a separator is not a concern there.

_WILDCARD = re.compile(r"[*?]")


def path_glob_match(pattern: str, path: str) -> bool:
    """Match a `/`-separated glob against a `/`-separated path."""
    return _segments_match(pattern.split("/"), path.split("/"))


def _segments_match(pattern_segments: list[str], path_segments: list[str]) -> bool:
    if not pattern_segments:
        return not path_segments
    head, rest = pattern_segments[0], pattern_segments[1:]
    if head == "**":
        # `**` matches zero or more whole segments.
        for index in range(len(path_segments) + 1):
            if _segments_match(rest, path_segments[index:]):
                return True
        return False
    if not path_segments:
        return False
    if not fnmatch.fnmatchcase(path_segments[0], head):
        return False
    return _segments_match(rest, path_segments[1:])


def target_glob_match(pattern: str, name: str) -> bool:
    """Match a glob over a test target name."""
    return fnmatch.fnmatchcase(name, pattern)


# ── tree and workspace facts ────────────────────────────────────────────────


def repo_files() -> set[str]:
    """Every tracked-looking file in the tree, as repo-relative POSIX paths."""
    found: set[str] = set()
    for dirpath, dirnames, filenames in os.walk(REPO_ROOT):
        dirnames[:] = [d for d in dirnames if d not in PRUNED_DIRS]
        rel_dir = Path(dirpath).relative_to(REPO_ROOT)
        for name in filenames:
            rel = (rel_dir / name).as_posix()
            if rel.startswith("./"):
                rel = rel[2:]
            found.add(rel)
    return found


def workspace_members() -> dict[str, dict]:
    """The workspace's packages and their test targets, from cargo metadata.

    `cargo metadata` is the only authority on which test targets exist: the
    root package auto-discovers 286 of them from `tests/*.rs`, and a manifest
    reader cannot see that.
    """
    metadata = _cargo_metadata()
    members: dict[str, dict] = {}
    for package in metadata["packages"]:
        integration = sorted(
            target["name"] for target in package["targets"] if target["kind"] == ["test"]
        )
        members[package["name"]] = {
            "integration": integration,
            # Cargo names a library's unit-test harness after the crate, with
            # hyphens turned into underscores.
            "lib-test-target": package["name"].replace("-", "_"),
        }
    return members


def workspace_dependency_edges() -> set[tuple[str, str]]:
    """The workspace's local dependency edges, as `(member, dependent)` pairs.

    An edge means "`dependent` depends on `member`". Only workspace-local
    dependencies are in scope; a crate from crates.io is not a member and
    cannot be affected by a change inside this repository.
    """
    metadata = _cargo_metadata()
    workspace = {package["name"] for package in metadata["packages"]}
    edges: set[tuple[str, str]] = set()
    for package in metadata["packages"]:
        for dependency in package["dependencies"]:
            if dependency.get("path") and dependency["name"] in workspace:
                edges.add((dependency["name"], package["name"]))
    return edges


_METADATA: dict | None = None


def _cargo_metadata() -> dict:
    global _METADATA
    if _METADATA is None:
        result = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode != 0:
            raise RuntimeError(f"cargo metadata failed: {result.stderr.strip()}")
        _METADATA = json.loads(result.stdout)
    return _METADATA


# ── manifest loading ────────────────────────────────────────────────────────


def load_manifest(path: Path) -> dict:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def member_pool(member: dict, facts: dict[str, dict]) -> set[str]:
    """The target names a group of this member may name."""
    facts_for_member = facts.get(member["name"], {})
    pool = set(facts_for_member.get("integration", []))
    lib_target = facts_for_member.get("lib-test-target")
    if lib_target:
        pool.add(lib_target)
    return pool


# ── the checks ──────────────────────────────────────────────────────────────


def check_manifest(
    manifest: dict,
    facts: dict[str, dict],
    files: set[str],
    edges: set[tuple[str, str]],
) -> list[str]:
    violations: list[str] = []

    version = manifest.get("schema-version")
    if version != SCHEMA_VERSION:
        violations.append(
            f"schema-version is {version!r}; this guard understands {SCHEMA_VERSION}. "
            f"A consumer must refuse a version it does not understand."
        )
        return violations

    members = manifest.get("members", []) or []
    groups = manifest.get("groups", []) or []

    declared: dict[str, dict] = {}
    for member in members:
        name = member.get("name")
        if not name:
            violations.append("a member entry has no `name`")
            continue
        if name in declared:
            violations.append(f"member {name!r} is declared more than once")
            continue
        declared[name] = member
        if name not in facts:
            violations.append(
                f"member {name!r} is not a workspace package. The workspace has: "
                f"{', '.join(sorted(facts))}"
            )
        root = member.get("root")
        if not root:
            violations.append(f"member {name!r} has no `root`")
        elif not (REPO_ROOT / root).is_dir():
            violations.append(f"member {name!r} names root {root!r}, which is not a directory")

    for name in sorted(set(facts) - set(declared)):
        violations.append(
            f"workspace package {name!r} has no member entry. Every workspace member "
            f"must be represented, or the mode cannot tell whether it is affected."
        )

    seen_ids: set[str] = set()
    owned: dict[str, set[str]] = {name: set() for name in facts}

    for group in groups:
        gid = group.get("id")
        if not gid:
            violations.append("a group entry has no `id`")
            continue
        if gid in seen_ids:
            violations.append(f"group id {gid!r} is used more than once")
        seen_ids.add(gid)

        member_name = group.get("member")
        member = declared.get(member_name)
        if member is None:
            violations.append(
                f"group {gid!r} names member {member_name!r}, which is not declared"
            )
            continue

        paths = group.get("paths", []) or []
        if not paths:
            violations.append(f"group {gid!r} names no source paths; it can never fire")
        for pattern in paths:
            if not any(path_glob_match(pattern, path) for path in files):
                violations.append(
                    f"group {gid!r} names source pattern {pattern!r}, which matches no "
                    f"file in the tree. An entry that can never fire is a lie."
                )

        targets = group.get("targets", []) or []
        if not targets:
            violations.append(f"group {gid!r} names no test targets")
        pool = member_pool(member, facts)
        for pattern in targets:
            if not any(target_glob_match(pattern, name) for name in pool):
                violations.append(
                    f"group {gid!r} names target pattern {pattern!r}, which matches no "
                    f"test target of member {member_name!r}"
                )
        for pattern in targets:
            for name in pool:
                if target_glob_match(pattern, name):
                    owned.setdefault(member_name, set()).add(name)

    # Every real test target must be owned by some group of its own member.
    for member_name, info in sorted(facts.items()):
        for target in info["integration"]:
            if target not in owned.get(member_name, set()):
                violations.append(
                    f"test target {member_name}/{target} exists but no group owns it. "
                    f"An unowned target can never be selected, so a scoped run would "
                    f"silently stop covering it."
                )

    # ── dependency edges ───────────────────────────────────────────────────
    #
    # A path rule cannot see that a change to one crate reaches another
    # crate's tests, because the coupling runs through the dependent's own
    # source. The edge set below is what closes that gap, so it must match
    # the workspace exactly: an edge the workspace has and the manifest does
    # not is a silent under-selection, and an edge the manifest has and the
    # workspace does not is a fabricated widening.
    dependencies = manifest.get("dependencies", []) or []
    declared_edges: set[tuple[str, str]] = set()
    for index, edge in enumerate(dependencies):
        member_name = edge.get("member")
        dependent_name = edge.get("dependent")
        if not member_name or not dependent_name:
            violations.append(
                f"a [[dependencies]] entry (index {index}) does not name both a "
                f"`member` and a `dependent`"
            )
            continue
        if member_name not in declared:
            violations.append(
                f"dependency edge {member_name!r} -> {dependent_name!r} names member "
                f"{member_name!r}, which is not declared"
            )
        if dependent_name not in declared:
            violations.append(
                f"dependency edge {member_name!r} -> {dependent_name!r} names dependent "
                f"{dependent_name!r}, which is not declared"
            )
        if member_name == dependent_name:
            violations.append(
                f"dependency edge {member_name!r} -> {dependent_name!r} names the same "
                f"crate on both sides"
            )
        key = (member_name, dependent_name)
        if key in declared_edges:
            violations.append(
                f"dependency edge {member_name!r} -> {dependent_name!r} is declared more "
                f"than once"
            )
        declared_edges.add(key)
        if key not in edges:
            violations.append(
                f"dependency edge {member_name!r} -> {dependent_name!r} is not a real "
                f"workspace dependency. `cargo metadata` does not report it, so the "
                f"manifest would widen a scope that nothing depends on."
            )

    for member_name, dependent_name in sorted(edges - declared_edges):
        violations.append(
            f"workspace dependency {dependent_name!r} -> {member_name!r} has no "
            f"[[dependencies]] entry. A change to {member_name!r} reaches "
            f"{dependent_name!r}'s tests, and without the edge the scoped mode would "
            f"silently stop selecting them."
        )

    return violations


# ── the check entry point ───────────────────────────────────────────────────


def check() -> bool:
    if not MANIFEST.is_file():
        print(f"  FAIL: ownership manifest not found at {MANIFEST}")
        return False
    try:
        manifest = load_manifest(MANIFEST)
    except tomllib.TOMLDecodeError as error:
        print(f"  FAIL: {MANIFEST} is not valid TOML: {error}")
        return False
    try:
        facts = workspace_members()
    except RuntimeError as error:
        print(f"  FAIL: {error}")
        return False
    violations = check_manifest(manifest, facts, repo_files(), workspace_dependency_edges())
    for message in violations:
        print(f"  FAIL: {message}")
    return not violations


# ── self test ───────────────────────────────────────────────────────────────


def _self_test() -> int:
    """Prove the guard rejects each violation class."""
    original = MANIFEST.read_text(encoding="utf-8")
    facts = workspace_members()
    files = repo_files()
    edges = workspace_dependency_edges()
    staged = MANIFEST.with_name("test-ownership.toml.self-test")
    try:
        staged.write_text(original, encoding="utf-8")
    except OSError as error:
        print(f"  FAIL: self-test could not stage a manifest copy: {error}")
        return 1

    def rejects(mutate, label: str) -> bool:
        manifest = load_manifest(staged)
        mutate(manifest)
        violations = check_manifest(manifest, facts, files, edges)
        if violations:
            print(f"  ok: injected {label} rejected")
            return True
        print(
            f"  FAIL: self-test injected {label} and the guard still passed. "
            f"The guard is not load-bearing."
        )
        return False

    def drop_first_group(manifest: dict) -> None:
        manifest["groups"] = manifest["groups"][1:]

    def unknown_member(manifest: dict) -> None:
        manifest["groups"][0]["member"] = "not-a-member"

    def unknown_target(manifest: dict) -> None:
        manifest["groups"][0]["targets"] = ["definitely_not_a_test_target"]

    def unmatched_path(manifest: dict) -> None:
        manifest["groups"][0]["paths"] = ["src/definitely/not/here/**/*.rs"]

    def missing_member(manifest: dict) -> None:
        manifest["members"] = manifest["members"][1:]

    def duplicate_member(manifest: dict) -> None:
        manifest["members"].append(dict(manifest["members"][0]))

    def duplicate_group_id(manifest: dict) -> None:
        manifest["groups"].append(dict(manifest["groups"][0]))

    def bad_schema_version(manifest: dict) -> None:
        manifest["schema-version"] = 99

    def drop_a_dependency_edge(manifest: dict) -> None:
        manifest["dependencies"] = manifest["dependencies"][1:]

    def invent_a_dependency_edge(manifest: dict) -> None:
        manifest["dependencies"].append(
            {"member": "eggsentry", "dependent": "egglsp"}
        )

    def dependency_edge_with_unknown_member(manifest: dict) -> None:
        manifest["dependencies"].append(
            {"member": "not-a-member", "dependent": "codegg"}
        )

    cases = [
        (drop_first_group, "an unowned test target"),
        (unknown_member, "a group naming an unknown member"),
        (unknown_target, "a group naming a nonexistent test target"),
        (unmatched_path, "a source pattern that matches no file"),
        (missing_member, "a workspace package with no member entry"),
        (duplicate_member, "a duplicated member entry"),
        (duplicate_group_id, "a duplicated group id"),
        (bad_schema_version, "an unknown schema-version"),
        (drop_a_dependency_edge, "a workspace dependency with no manifest edge"),
        (invent_a_dependency_edge, "a dependency edge that is not a real workspace dependency"),
        (dependency_edge_with_unknown_member, "a dependency edge naming an undeclared member"),
    ]
    try:
        for mutate, label in cases:
            if not rejects(mutate, label):
                return 1

        # A malformed manifest must fail, not pass vacuously.
        staged.write_text(original + "\n[[members\nbroken = ", encoding="utf-8")
        try:
            load_manifest(staged)
            print("  FAIL: self-test injected a malformed manifest and it still parsed")
            return 1
        except tomllib.TOMLDecodeError:
            print("  ok: injected malformed manifest rejected")

        # And the unmodified tree must still pass.
        manifest = load_manifest(MANIFEST)
        if check_manifest(manifest, facts, files, edges):
            print("  FAIL: self-test passed but the real tree has violations")
            return 1
        print("  ok: unmodified tree still passes")
        return 0
    finally:
        try:
            staged.unlink()
        except OSError:
            pass


def main() -> int:
    if "--self-test" in sys.argv:
        print("check_test_ownership --self-test")
        return _self_test()

    checks = [
        ("the test-ownership manifest matches the tree and the workspace", check),
    ]
    failed = []
    for name, check_fn in checks:
        try:
            passed = check_fn()
        except OSError as error:
            print(f"  FAIL: {name}: {error}")
            passed = False
        print(f"  {'ok' if passed else 'FAIL'}: {name}")
        if not passed:
            failed.append(name)
    if failed:
        print(f"check_test_ownership: {len(failed)} failing")
        return 1
    print("check_test_ownership: all checks pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
