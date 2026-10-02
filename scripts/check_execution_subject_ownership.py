#!/usr/bin/env python3
"""Static lint: execution-subject provenance ownership guard (Eggplan M001, M003 C001).

Enforces the CodeGG-owned provenance invariants from
``plans/implementation/eggplan-assessment-integration/001-durable-execution-subject-provenance.md``
as repaired by
``plans/implementation/eggplan-assessment-integration/005-m003-c001-dirty-subject-provenance-and-bound-evidence.md``:

  1. Subject capture lives only in the approved Git owner:
     ``capture_git_source_subject`` is defined once in
     ``crates/egggit/src/subject.rs`` and called only from the
     application-level v2 helper (``src/execution_subject_capture.rs``),
     the M002 assessment facade (current-subject reads only, never
     persisted provenance), plus the crate re-export and tests.
  1b. The administrative-exclusion capture entry point (M003) is defined
     exactly once in the approved Git owner and called only from the M003
     repository-binding identity proof.
  1c. All authoritative attempt-start/seal capture goes through the
     approved v2 helper ``capture_attempt_revision``: the scheduler
     boundary never constructs ``ExecutionSubjectRevision`` literals or
     calls raw capture directly.
  2. ``src/work_plan_evidence.rs`` never captures: no ``egggit`` use, no
     capture/seal calls, no process spawn, no ``current_dir``, no
     provenance writes. Both resolvers load durable attempt provenance
     (``agent_run.run_id`` link) only.
  3. No new raw ``git`` subprocess owner is introduced.
  4. Provenance authority is ``JobAttempt.source_subject``, never
     ``JobRecord`` fields or free-form job labels.
  5. The v67 migration exists and adds ``source_subject_json``.
  6. C001 translator rules: the bound historical translator reads only the
     persisted ``eggplan_dirty_digest`` (never the native ``dirty_digest``)
     for bound Eggplan subjects; the E1/C/E2 sandwich proof exists, captures
     the Eggplan side twice through
     ``eggplan_repo::capture_git_subject_fingerprint``, and carries its typed
     changed-during-proof error; no current-worktree historical backfill
     helper exists; ``codegg-core`` has no Eggplan crate dependency.

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

    # 1a. The administrative-exclusion capture entry point (M003) is also
    # defined exactly once in the approved Git owner.
    excluding_sites = []
    for path in list(ROOT.joinpath("crates").rglob("*.rs")) + list(
        ROOT.joinpath("src").rglob("*.rs")
    ):
        rel = str(path.relative_to(ROOT))
        for i, line in code_lines(rel):
            if re.search(r"\bfn capture_git_source_subject_excluding\s*\(", line):
                excluding_sites.append(f"{rel}:{i}")
    excluding_files = {site.rsplit(":", 1)[0] for site in excluding_sites}
    if excluding_files != {"crates/egggit/src/subject.rs"} or len(excluding_sites) != 1:
        fail(
            "capture_git_source_subject_excluding must be defined exactly once in "
            f"crates/egggit/src/subject.rs; found: {excluding_sites}"
        )

    # 1b. Callers of the capture entry points are confined to the governed
    # owners: the application-level v2 helper, the M002 assessment facade
    # (current-subject reads only), the M003 repository-binding identity
    # proof (exclusion form only), plus the crate re-export and tests.
    # The scheduler boundary must go through the v2 helper (rule 1c below),
    # never raw capture. Capture ownership is never widened to a new caller
    # without adding that owner here explicitly.
    allowed_caller_prefixes = (
        "crates/egggit/src/",
        "tests/",
    )
    # Files allowed to call the plain capture form, because they read the
    # *current* subject for a bounded assessment/identity decision and never
    # persist provenance.
    allowed_plain_capture_files = (
        "src/work_plan_eggplan.rs",
        "src/execution_subject_capture.rs",
    )
    # Files allowed to call the exclusion form only.
    allowed_excluding_capture_files = (
        "src/work_plan_repository_binding.rs",
    )
    for path in list(ROOT.joinpath("crates").rglob("*.rs")) + list(
        ROOT.joinpath("src").rglob("*.rs")
    ):
        rel = str(path.relative_to(ROOT))
        if rel.startswith(allowed_caller_prefixes) or "/tests/" in rel:
            continue
        for i, line in code_lines(rel):
            if "capture_git_source_subject" not in line:
                continue
            if rel in allowed_excluding_capture_files:
                if "capture_git_source_subject_excluding" not in line:
                    fail(
                        f"{rel}:{i}: the binding module may only call the approved "
                        "exclusion capture form"
                    )
                continue
            if rel in allowed_plain_capture_files:
                continue
            fail(f"{rel}:{i}: subject capture outside the governed capture owners")

    # 1c. All authoritative attempt-start/seal capture goes through the
    # approved v2 helper. The scheduler boundary must call
    # `capture_attempt_revision` and must never construct
    # `ExecutionSubjectRevision` literals or call raw capture directly.
    for rel in ("src/scheduler/scheduler.rs", "src/scheduler/executor.rs"):
        text = read(rel)
        if "capture_attempt_revision" not in text:
            fail(f"{rel}: authoritative capture must go through capture_attempt_revision")
        for i, line in code_lines(rel):
            if "capture_git_source_subject" in line:
                fail(f"{rel}:{i}: scheduler capture must go through the v2 helper")
            if re.search(r"ExecutionSubjectRevision\s*\{", line):
                fail(
                    f"{rel}:{i}: scheduler must not construct "
                    "ExecutionSubjectRevision literals (use the v2 helper)"
                )

    # 1d. Raw Eggplan subject capture is confined to the v2 helper (attempt
    # provenance) and the binding module (identity proof, assessment,
    # closure). Tools, the arbiter, and the scheduler never touch it. The
    # C001 fingerprint entry point has the same single-owner confinement as
    # `subject_source()`.
    for path in list(ROOT.joinpath("src").rglob("*.rs")):
        rel = str(path.relative_to(ROOT))
        if rel in (
            "src/execution_subject_capture.rs",
            "src/work_plan_repository_binding.rs",
        ) or "/tests/" in rel:
            continue
        for i, line in code_lines(rel):
            if "subject_source()" in line:
                fail(f"{rel}:{i}: raw Eggplan subject capture outside the governed owners")
            if "capture_git_subject_fingerprint" in line:
                fail(
                    f"{rel}:{i}: raw Eggplan fingerprint capture outside the "
                    "governed owners"
                )

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

    # 6. C001 translator and sandwich rules live in the approved binding
    # module.
    binding = read("src/work_plan_repository_binding.rs")
    if "prove_identity_from_captures" not in binding:
        fail(
            "src/work_plan_repository_binding.rs: the E1/C/E2 sandwich core "
            "prove_identity_from_captures is missing"
        )
    if "repository_subject_changed_during_identity_proof" not in binding:
        fail(
            "src/work_plan_repository_binding.rs: the typed "
            "changed-during-proof error is missing"
        )
    # The sandwich captures the Eggplan side twice through the qualified C001
    # fingerprint contract: one helper definition plus the E1 and E2 calls.
    if binding.count("eggplan_subject_fingerprint(") < 3:
        fail(
            "src/work_plan_repository_binding.rs: the identity proof must "
            "capture the Eggplan-compatible fingerprint twice (E1/C/E2 sandwich)"
        )
    if "capture_git_subject_fingerprint" not in binding:
        fail(
            "src/work_plan_repository_binding.rs: the identity proof must go "
            "through eggplan_repo::capture_git_subject_fingerprint"
        )
    if "legacy_dirty_subject_missing_eggplan_digest" not in binding:
        fail(
            "src/work_plan_repository_binding.rs: the legacy-dirty "
            "fail-closed error is missing"
        )
    # The bound translator must read the persisted Eggplan-compatible
    # digest, never the native digest, as the bound Eggplan digest.
    translator = binding.split("pub fn translate_historical_subject", 1)
    if len(translator) != 2:
        fail("src/work_plan_repository_binding.rs: translate_historical_subject is missing")
    else:
        body = translator[1].split("\n// ──", 1)[0]
        body_code = "\n".join(
            line.split("//")[0]
            for line in body.splitlines()
            if not line.strip().startswith("//")
        )
        if "eggplan_dirty_digest" not in body_code:
            fail(
                "translate_historical_subject: bound dirty translation must read "
                "eggplan_dirty_digest"
            )
        if ".dirty_digest" in body_code:
            fail(
                "translate_historical_subject: the native dirty_digest must never "
                "feed a bound Eggplan subject"
            )

    # 6b. No current-worktree historical backfill helper exists anywhere.
    for path in list(ROOT.joinpath("src").rglob("*.rs")) + list(
        ROOT.joinpath("crates").rglob("*.rs")
    ):
        rel = str(path.relative_to(ROOT))
        for i, line in code_lines(rel):
            if re.search(r"fn\s+\w*backfill\w*\s*\(", line) and not in_test_module(path, i):
                fail(f"{rel}:{i}: no historical backfill helper may exist")

    # 6c. codegg-core stays free of every Eggplan crate dependency.
    core_manifest = read("crates/codegg-core/Cargo.toml")
    if re.search(r"^\s*eggplan-", core_manifest, re.MULTILINE):
        fail("crates/codegg-core/Cargo.toml: codegg-core must not depend on Eggplan crates")

    if FAILURES:
        print("execution-subject ownership guard FAILED:")
        for failure in FAILURES:
            print(f"  - {failure}")
        return 1
    print("execution-subject ownership guard ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
