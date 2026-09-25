#!/usr/bin/env python3
"""Static lint: execution-subject provenance ownership guard (Eggplan M001).

Enforces the CodeGG-owned provenance invariants from
``plans/implementation/eggplan-assessment-integration/001-durable-execution-subject-provenance.md``:

  1. Subject capture lives only in the approved Git owner:
     ``capture_git_source_subject`` is defined once in
     ``crates/egggit/src/subject.rs`` and called only from the
     scheduler-owned boundary (``src/scheduler/``), plus the crate
     re-export and tests.
  2. ``src/work_plan_evidence.rs`` never captures: no ``egggit`` use, no
     capture/seal calls, no process spawn, no ``current_dir``, no
     provenance writes. Both resolvers load durable attempt provenance
     (``agent_run.run_id`` link) only.
  3. No new raw ``git`` subprocess owner is introduced.
  4. Provenance authority is ``JobAttempt.source_subject``, never
     ``JobRecord`` fields or free-form job labels.
  5. The v67 migration exists and adds ``source_subject_json``.

Run:

  python3 scripts/check_execution_subject_ownership.py

Exit code 0 on success, 1 on failure.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

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


def in_test_module(path: Path, lineno: int) -> bool:
    """Heuristic: True if `lineno` falls inside a `#[cfg(test)] mod tests`
    block (same approach as `check_git_forbidden_patterns`)."""
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except (FileNotFoundError, IsADirectoryError):
        return False
    depth = 0
    in_test = False
    test_depth = 0
    for i, line in enumerate(lines, start=1):
        stripped = line.strip()
        if depth == 0 and "mod tests" in stripped and stripped.endswith("{"):
            for j in range(i - 2, max(-1, i - 5), -1):
                if j < 0:
                    break
                prev = lines[j].strip()
                if not prev:
                    continue
                if "cfg(test)" in prev:
                    in_test = True
                    test_depth = depth
                break
        depth += stripped.count("{") - stripped.count("}")
        if in_test and depth <= test_depth:
            in_test = False
        if i == lineno:
            return in_test
    return False


def main() -> int:
    for rel in [
        "crates/egggit/src/subject.rs",
        "src/work_plan_evidence.rs",
    ]:
        if not (ROOT / rel).exists():
            fail(f"missing module: {rel}")

    # 1. Capture definition is unique to the approved Git owner.
    definition_sites = []
    for path in list(ROOT.joinpath("crates").rglob("*.rs")) + list(
        ROOT.joinpath("src").rglob("*.rs")
    ):
        rel = str(path.relative_to(ROOT))
        for i, line in code_lines(rel):
            if re.search(r"\bfn capture_git_source_subject\s*\(", line):
                definition_sites.append(f"{rel}:{i}")
    unique_files = {site.rsplit(":", 1)[0] for site in definition_sites}
    if unique_files != {"crates/egggit/src/subject.rs"} or len(definition_sites) != 1:
        fail(
            "capture_git_source_subject must be defined exactly once in "
            f"crates/egggit/src/subject.rs; found: {definition_sites}"
        )

    # 1b. Callers of the capture entry point are confined to the
    # scheduler-owned boundary (+ re-export + tests).
    allowed_caller_prefixes = (
        "crates/egggit/src/",
        "src/scheduler/",
        "tests/",
    )
    for path in list(ROOT.joinpath("crates").rglob("*.rs")) + list(
        ROOT.joinpath("src").rglob("*.rs")
    ):
        rel = str(path.relative_to(ROOT))
        if rel.startswith(allowed_caller_prefixes) or "/tests/" in rel:
            continue
        for i, line in code_lines(rel):
            if "capture_git_source_subject" in line:
                fail(f"{rel}:{i}: subject capture outside the scheduler-owned boundary")

    # 2. The evidence resolvers never capture and never read live state.
    evidence = read("src/work_plan_evidence.rs")
    for pattern, label in [
        (r"egggit", "egggit use"),
        (r"capture_git_source_subject", "subject capture call"),
        (
            r"seal_materialized_source_subject|set_attempt_source_subject_started|seal_attempt_source_subject",
            "provenance write",
        ),
        (r"Command::new", "process spawn"),
        (r"current_dir", "process-global cwd read"),
    ]:
        for i, line in enumerate(evidence.splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("//"):
                continue
            if re.search(pattern, line.split("//")[0]):
                fail(f"src/work_plan_evidence.rs:{i}: forbidden {label} in resolver")

    # 2b. AgentRun links resolve through the durable `run_id` key.
    for i, line in enumerate(evidence.splitlines(), 1):
        stripped = line.strip()
        if stripped.startswith("//"):
            continue
        code = line.split("//")[0]
        if "FROM agent_run" in code and "run_id" not in code:
            fail(
                f"src/work_plan_evidence.rs:{i}: agent_run lookup must key on run_id "
                "(C002 corrective)"
            )

    # 3. No new raw git subprocess owner. The governed seam is
    # crates/egggit/src/process.rs; pre-existing typed owners predate M001.
    preexisting_git_owners = {
        "src/git_service.rs",
        "src/git_mutations.rs",
        "src/git_network_ops.rs",
        "src/git_recovery.rs",
        "src/tool/review.rs",
    }
    for path in list(ROOT.joinpath("src").rglob("*.rs")) + list(
        ROOT.joinpath("crates").rglob("*.rs")
    ):
        rel = str(path.relative_to(ROOT))
        if (
            rel == "crates/egggit/src/process.rs"
            or rel in preexisting_git_owners
            or rel.startswith("tests/")
            or "/tests/" in rel
        ):
            continue
        for i, line in code_lines(rel):
            if re.search(r"(?:Command::new|StdCommand::new)\(\s*\"git\"\s*\)", line):
                if in_test_module(path, i):
                    continue
                fail(f"{rel}:{i}: new raw git subprocess owner")

    # 4. Provenance authority is JobAttempt, never JobRecord or labels.
    jobs_mod = read("crates/codegg-core/src/jobs/mod.rs")
    if "pub source_subject" not in jobs_mod:
        fail("JobAttempt must carry the source_subject provenance field")
    record_section = jobs_mod.split("pub struct JobRecord")[1].split("pub struct JobAttempt")[0]
    if "source_subject" in record_section:
        fail("JobRecord must not carry source-subject provenance (attempt-scoped authority)")
    for path in list(ROOT.joinpath("src").rglob("*.rs")) + list(
        ROOT.joinpath("crates").rglob("*.rs")
    ):
        rel = str(path.relative_to(ROOT))
        for i, line in code_lines(rel):
            if re.search(r'"source_subject"\s*,\s*labels|labels.*source_subject', line):
                fail(f"{rel}:{i}: subject provenance must not travel in free-form labels")

    # 5. Migration v67 exists.
    schema = read("crates/codegg-core/src/session/schema.rs")
    if "async fn migrate_v67" not in schema:
        fail("migrate_v67 missing in crates/codegg-core/src/session/schema.rs")
    if "source_subject_json" not in schema:
        fail("migrate_v67 must add the source_subject_json column")

    if FAILURES:
        print("execution-subject ownership guard FAILED:")
        for failure in FAILURES:
            print(f"  - {failure}")
        return 1
    print("execution-subject ownership guard ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
