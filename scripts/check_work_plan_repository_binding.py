#!/usr/bin/env python3
"""Static lint: repository-Plan binding ownership guard (Eggplan M003).

Enforces the CodeGG-owned boundaries from
``plans/implementation/eggplan-assessment-integration/004-repository-plan-binding-and-writeback.md``:

  1. ``RepositoryStore::open`` / ``open_read_only`` production use is confined
     to the approved application binding module
     (``src/work_plan_repository_binding.rs``), plus the crate re-export and
     tests. Tools, arbiter, scheduler, and the WorkOrder coordinator never
     open an Eggplan store.
  2. ``eggplan-repo`` is absent from ``codegg-core`` (and every other
     non-root crate manifest).
  3. Only the validated translator in the binding module rewrites
     ``SubjectRevision.repository_id``; no blind repository-id replacement
     exists anywhere else.
  4. Bound completion cannot reach the ordinary CodeGG ``Completed``
     transition: the arbiter refuses a bound plan, and the only site that
     finalizes a bound plan calls ``finalize_closure``.
  5. No code copies the repository-local ``.eggplan`` state root into a
     managed worktree.
  6. The v68 migration exists and adds the three binding tables.

Run:

  python3 scripts/check_work_plan_repository_binding.py

Exit code 0 on success, 1 on failure.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

APPROVED_OWNER = "src/work_plan_repository_binding.rs"
SUBJECT_OWNER = "crates/egggit/src/subject.rs"
STATE_DIR = ".eggplan"

FAILURES: list[str] = []


def fail(message: str) -> None:
    FAILURES.append(message)


def read(rel: str) -> str:
    return (ROOT / rel).read_text(encoding="utf-8")


def code_lines(rel: str) -> list[tuple[int, str]]:
    """Non-comment source lines with 1-based numbers."""
    out: list[tuple[int, str]] = []
    for i, line in enumerate(read(rel).splitlines(), 1):
        stripped = line.strip()
        if stripped.startswith("//"):
            continue
        out.append((i, line.split("//")[0]))
    return out


def rust_sources() -> list[Path]:
    paths: list[Path] = []
    for base in ("src", "crates", "tests", "benches"):
        directory = ROOT / base
        if directory.is_dir():
            paths.extend(sorted(directory.rglob("*.rs")))
    return paths


def rel(path: Path) -> str:
    return str(path.relative_to(ROOT))


def is_test_path(path: str) -> bool:
    return path.startswith("tests/") or "/tests/" in path or path.endswith("_test.rs")


def main() -> int:
    if not (ROOT / APPROVED_OWNER).exists():
        fail(f"missing approved binding module: {APPROVED_OWNER}")

    sources = rust_sources()

    # 1. Repository store ownership.
    store_call = re.compile(r"RepositoryStore::(open|open_read_only|open_with_options)\b")
    for path in sources:
        name = rel(path)
        if name == APPROVED_OWNER or is_test_path(name):
            continue
        for lineno, line in code_lines(name):
            if store_call.search(line):
                fail(
                    f"{name}:{lineno}: RepositoryStore construction outside the approved "
                    f"binding module {APPROVED_OWNER}"
                )

    # 2. eggplan-repo is application-layer only.
    for manifest in sorted(ROOT.rglob("Cargo.toml")):
        name = rel(manifest)
        if name == "Cargo.toml":
            continue
        text = manifest.read_text(encoding="utf-8")
        if re.search(r"^\s*eggplan-(repo|core|codegg-compat)\b", text, re.MULTILINE):
            fail(
                f"{name}: Eggplan crate dependency must stay in the root manifest "
                "(application layer only)"
            )

    # 3. Repository-id translation is confined to the validated translator.
    translate = re.compile(r"translate_historical_subject")
    for path in sources:
        name = rel(path)
        for lineno, line in code_lines(name):
            if not re.search(r"SubjectRevision", line):
                continue
            if re.search(r"repository_id:\s*\w", line) and "translate_historical_subject" not in name:
                # Constructing an Eggplan subject with a literal repository id
                # outside the binding module is a blind relabel.
                if re.search(r"repository_id:\s*(binding\.|store\.|\"|[a-z_]+\.to_string\(\))", line):
                    fail(
                        f"{name}:{lineno}: SubjectRevision repository_id rewrite outside the "
                        "validated binding translator"
                    )
    if translate.search(read(APPROVED_OWNER)) is None:
        fail(f"{APPROVED_OWNER}: validated subject translator is missing")

    # 4. Bound completion cannot use the ordinary Completed transition.
    arbiter = read("src/work_plan_arbiter.rs")
    if "repository_bound_plan_requires_guarded_closure" not in arbiter:
        fail(
            "src/work_plan_arbiter.rs: the ordinary turn-end completion path must refuse a "
            "repository-bound plan (guarded Eggplan closure is the only closure authority)"
        )
    finalizer = read(APPROVED_OWNER)
    if "finalize_closure" not in finalizer:
        fail(f"{APPROVED_OWNER}: guarded repository closure must go through finalize_closure")
    for path in sources:
        name = rel(path)
        if name == APPROVED_OWNER or is_test_path(name):
            continue
        for lineno, line in code_lines(name):
            if "finalize_closure" in line:
                fail(
                    f"{name}:{lineno}: guarded repository closure is owned by {APPROVED_OWNER}"
                )

    # 5. The repository-local state root is never copied into a worktree.
    for path in sources:
        name = rel(path)
        for lineno, line in code_lines(name):
            if "excluding_path" in line and name == SUBJECT_OWNER:
                continue
            if STATE_DIR not in line:
                continue
            if re.search(r"worktree", name) and not is_test_path(name):
                fail(
                    f"{name}:{lineno}: the repository-local Eggplan state root must never be "
                    "copied into a managed worktree"
                )
    for path in sources:
        name = rel(path)
        if is_test_path(name):
            continue
        for lineno, line in code_lines(name):
            if re.search(r"(copy|copy_dir|write|create_dir|rename)\w*\s*\([^)]*eggplan", line):
                fail(
                    f"{name}:{lineno}: no code may create or copy an Eggplan state root"
                )

    # 6. The v68 migration and the three binding tables exist.
    schema = read("crates/codegg-core/src/session/schema.rs")
    if not re.search(r"async fn migrate_v68\b", schema):
        fail("crates/codegg-core/src/session/schema.rs: migrate_v68 is missing")
    if "crate::work_plan::WORK_PLAN_REPOSITORY_BINDING_SCHEMA_STATEMENTS" not in schema:
        fail(
            "crates/codegg-core/src/session/schema.rs: migrate_v68 must apply the binding "
            "schema statements"
        )
    binding = read("crates/codegg-core/src/work_plan/repository_binding.rs")
    for table in (
        "work_plan_eggplan_binding",
        "work_plan_eggplan_item_binding",
        "work_order_eggplan_binding",
    ):
        if f"CREATE TABLE IF NOT EXISTS {table}" not in binding:
            fail(
                f"crates/codegg-core/src/work_plan/repository_binding.rs: missing table {table}"
            )
    storage = read("crates/codegg-core/src/storage/mod.rs")
    if not re.search(r"STORAGE_LAYOUT_VERSION:\s*u32\s*=\s*68", storage):
        fail("crates/codegg-core/src/storage/mod.rs: STORAGE_LAYOUT_VERSION must be 68")

    if FAILURES:
        print("repository-binding ownership check failed:")
        for failure in FAILURES:
            print(f"  - {failure}")
        return 1
    print("repository-binding ownership check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
