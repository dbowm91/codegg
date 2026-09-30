#!/usr/bin/env python3
"""Static lint: Eggplan staged-assessment adoption boundary (M002).

Enforces the dependency/ownership boundary from
``plans/implementation/eggplan-assessment-integration/003-staged-production-assessment-adoption.md``
(§2/§3):

  1. Exactly one immutable Eggplan revision is pinned in the root
     ``Cargo.toml`` for both ``eggplan-core`` and
     ``eggplan-codegg-compat`` (``rev =``, never a branch).
  2. No production dependency on ``eggplan-repo``, ``eggplan-cli``,
     ``eggplan-projection``, ``eggplan-markdown``, or
     ``eggplan-integrations`` in any CodeGG manifest.
  3. ``eggplan-*`` use is confined to the application layer: the root
     ``codegg`` package only (``src/work_plan_eggplan.rs`` plus tests).
     ``codegg-core`` and every other crate stay Eggplan-free, so the
     pure assessor remains usable without Eggplan repository/runtime
     dependencies.
  4. The assessor swap stays staged: the legacy pure
     ``codegg_core::work_plan::assess_work_plan`` keeps its definition
     and its unit-test coverage.

Run:

  python3 scripts/check_eggplan_assessment_boundary.py

Exit code 0 on success, 1 on failure.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

EXPECTED_REV = "0d4a6af7adc6f80f975aca1bfe9bae04e2eb27d8"

PURE_CRATES = ("eggplan-core", "eggplan-codegg-compat")

FORBIDDEN_CRATES = (
    "eggplan-repo",
    "eggplan-cli",
    "eggplan-projection",
    "eggplan-markdown",
    "eggplan-integrations",
)

# Application-layer files allowed to name `eggplan_*` in non-test code.
ALLOWED_EGGPLAN_USE_PREFIXES = (
    "src/work_plan_eggplan.rs",
    "src/work_plan_arbiter.rs",
    "src/tool/work_plan.rs",
    "src/agent/loop.rs",
    "src/tool/goal.rs",
)

FAILURES: list[str] = []


def fail(message: str) -> None:
    FAILURES.append(message)


def main() -> int:
    root_manifest = (ROOT / "Cargo.toml").read_text(encoding="utf-8")

    # 1. Exact immutable pin for both pure crates, same revision, no branch.
    for crate in PURE_CRATES:
        pattern = (
            r"^" + re.escape(crate) + r'\s*=\s*\{[^}]*'
            r'git\s*=\s*"https://github\.com/eggstack/eggplan\.git"[^}]*'
            r"rev\s*=\s*\"" + re.escape(EXPECTED_REV) + r'"[^}]*\}'
        )
        if not re.search(pattern, root_manifest, re.MULTILINE):
            fail(
                f"root Cargo.toml must pin {crate} to immutable rev {EXPECTED_REV} "
                "(git + rev, never a branch)"
            )
    if re.search(r"eggplan[^=]*=\s*\{[^}]*branch\s*=", root_manifest):
        fail("Eggplan dependencies must not use a branch pin")
    core_pin = re.search(r"^eggplan-core\s*=\s*\{[^}]*\}", root_manifest, re.MULTILINE)
    compat_pin = re.search(
        r"^eggplan-codegg-compat\s*=\s*\{[^}]*\}", root_manifest, re.MULTILINE
    )
    if core_pin and compat_pin:
        core_rev = re.search(r'rev\s*=\s*"([^"]+)"', core_pin.group(0))
        compat_rev = re.search(r'rev\s*=\s*"([^"]+)"', compat_pin.group(0))
        if (
            not core_rev
            or not compat_rev
            or core_rev.group(1) != compat_rev.group(1)
        ):
            fail("eggplan-core and eggplan-codegg-compat must resolve from one exact revision")

    # 2. No forbidden Eggplan crate in any CodeGG manifest or lockfile.
    manifests = list(ROOT.rglob("Cargo.toml"))
    manifests = [p for p in manifests if ".git" not in p.parts and "target" not in p.parts]
    for manifest in manifests:
        try:
            text = manifest.read_text(encoding="utf-8")
        except (FileNotFoundError, IsADirectoryError):
            continue
        rel = str(manifest.relative_to(ROOT))
        for crate in FORBIDDEN_CRATES:
            if re.search(r"(?m)^" + re.escape(crate) + r"\s*=", text):
                fail(f"{rel}: forbidden production dependency surface {crate}")
    lock = ROOT / "Cargo.lock"
    if lock.exists():
        lock_text = lock.read_text(encoding="utf-8")
        for crate in FORBIDDEN_CRATES:
            if re.search(r'(?m)^name = "' + re.escape(crate) + r'"$', lock_text):
                fail(f"Cargo.lock resolves forbidden crate {crate}")

    # 3. Eggplan use confined to the application layer.
    for path in list(ROOT.joinpath("crates").rglob("*.rs")):
        rel = str(path.relative_to(ROOT))
        try:
            text = path.read_text(encoding="utf-8")
        except (FileNotFoundError, IsADirectoryError):
            continue
        for i, line in enumerate(text.splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("//"):
                continue
            code = line.split("//")[0]
            if re.search(r"\beggplan_(core|codegg_compat)\b", code):
                fail(f"{rel}:{i}: eggplan use outside the application layer")
    for path in list(ROOT.joinpath("src").rglob("*.rs")):
        rel = str(path.relative_to(ROOT))
        if rel.startswith(ALLOWED_EGGPLAN_USE_PREFIXES) or "/tests/" in rel:
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except (FileNotFoundError, IsADirectoryError):
            continue
        for i, line in enumerate(text.splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("//"):
                continue
            code = line.split("//")[0]
            if re.search(r"\beggplan_(core|codegg_compat)\b", code):
                fail(f"{rel}:{i}: eggplan use outside the approved application-layer modules")
    for rel in ("crates/codegg-core/Cargo.toml",):
        text = (ROOT / rel).read_text(encoding="utf-8")
        if "eggplan" in text:
            fail(f"{rel}: codegg-core must not depend on Eggplan crates")

    # 4. The legacy pure assessor keeps its definition (staged swap only).
    assessment = (ROOT / "crates/codegg-core/src/work_plan/assessment.rs").read_text(
        encoding="utf-8"
    )
    if "pub fn assess_work_plan" not in assessment:
        fail("codegg-core must keep the pure assess_work_plan compatibility API")

    # 5. Production Git-backed supported-evidence call sites go through the
    # facade. `src/work_plan_arbiter.rs` and `src/tool/work_plan.rs` must
    # not call the legacy assessor directly (outside unit tests); the
    # facade module itself and integration tests may (oracle/fallback).
    for rel in ("src/work_plan_arbiter.rs", "src/tool/work_plan.rs"):
        text = (ROOT / rel).read_text(encoding="utf-8")
        for i, line in enumerate(text.splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("//"):
                continue
            code = line.split("//")[0]
            if "assess_work_plan" in code and "assess_work_plan_with_eggplan" not in code:
                fail(
                    f"{rel}:{i}: production call sites must use "
                    "assess_work_plan_with_eggplan, not assess_work_plan directly"
                )
    # 6. Current-subject capture stays scheduler-owned: only
    # `src/scheduler/` may call `capture_git_source_subject`. The facade
    # captures through `scheduler::assessment_subject`.
    for path in list(ROOT.joinpath("src").rglob("*.rs")):
        rel = str(path.relative_to(ROOT))
        if rel.startswith("src/scheduler/") or "/tests/" in rel or rel.startswith("tests/"):
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except (FileNotFoundError, IsADirectoryError):
            continue
        for i, line in enumerate(text.splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("//"):
                continue
            if "capture_git_source_subject" in line.split("//")[0]:
                fail(f"{rel}:{i}: subject capture outside the scheduler-owned boundary")

    if FAILURES:
        print("eggplan assessment boundary guard FAILED:")
        for failure in FAILURES:
            print(f"  - {failure}")
        return 1
    print("eggplan assessment boundary guard ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
