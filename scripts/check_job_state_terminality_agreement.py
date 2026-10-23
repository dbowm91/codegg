#!/usr/bin/env python3
"""Fail when a job-state terminality claim disagrees with the domain.

The authority is `JobState::is_terminal()` in
`crates/codegg-core/src/jobs/mod.rs`: a job is finished when it is
`Completed`, `Failed`, `Cancelled`, `TimedOut` or `Expired`. `Interrupted` is
**not** terminal, because an interrupted attempt may still be requeued.

Two rules are enforced.

**Rule 1 — no hand-written job-state terminality list in production code.**
A list of `JobState::` variants written out by hand in production code —
outside `crates/codegg-core/src/jobs/` — is a violation when

* it is one of the domain's two canonical sets — the terminal set or its
  complement — because those two sets must be *derived* from the domain's
  predicate, never copied; or
* it is the `states:` argument of a job-store query whose claim is named for
  terminality (`terminal`, `final`, `finished`, `settled`) or for liveness
  (`live`, `active`, `unfinished`, `outstanding`), and the list is not the
  matching canonical set. This is the shape of the defect this milestone
  fixed: a query named `terminal_jobs` that selected `Interrupted` as well as
  the finished states.

**Rule 2 — every projection of a job state covers every variant.** A new
`JobState` variant that `JobState::ALL` or one of the projections does not
mention is a violation, so a variant cannot be added while the projections
keep answering for the old set.

The test trees are exempt from rule 1: a test that writes the set out is
pinning the contract on purpose and fails loudly when the contract moves,
while a production list changes behaviour silently.

Rust is parsed with `scripts/rust_test_scan.py`, the shared Rust-aware
scanner. A regex scan misreads lifetimes as character literals and
undercounts the tree.

Usage:
    python3 scripts/check_job_state_terminality_agreement.py [--self-test]
"""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(Path(__file__).resolve().parent))

import rust_test_scan  # noqa: E402  (path is set above)

# The domain that owns `JobState` and its terminality predicate.
DOMAIN_JOBS_DIR = "crates/codegg-core/src/jobs"
DOMAIN_JOBS_FILE = f"{DOMAIN_JOBS_DIR}/mod.rs"

# The projections that must cover every variant (rule 2).
PROJECTIONS = (
    (f"{DOMAIN_JOBS_DIR}/evidence.rs", "job_success_evidence"),
    ("src/scheduler/job_notifications.rs", "job_status_projection"),
)

# Identifier tokens that say a claim is about terminality or about liveness.
TERMINAL_WORDS = frozenset(
    {"terminal", "terminals", "final", "finals", "finished", "settled"}
)
LIVE_WORDS = frozenset({"live", "lives", "active", "unfinished", "outstanding"})

_VARIANT_USE = re.compile(r"\bJobState::([A-Z][A-Za-z0-9_]*)")
_LET_BINDING = re.compile(r"\blet\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)")
_FN_NAME = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)")
_STATES_FIELD = re.compile(r"\bstates\s*:\s*$")


class Finding:
    """One violation, with the place it was found."""

    def __init__(self, location: str, message: str) -> None:
        self.location = location
        self.message = message

    def __str__(self) -> str:
        return f"{self.location}: {self.message}"


def _mask(text: str) -> str:
    """The structural view of a source text: comments and literals blanked."""
    return rust_test_scan.scan_source(text).mask()


def _line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def _body_of(text: str, path: str, function_name: str) -> str | None:
    """The body of one function, or None when the file has no such function."""
    functions = rust_test_scan.find_functions(
        text, Path(path), path, path.split("/")[0] if "/" in path else "src"
    )
    for function in functions:
        if function.name == function_name:
            return function.body_text
    return None


def _enum_variants(masked: str) -> list[str]:
    """The variants of the `JobState` enum, in declaration order."""
    start = masked.find("pub enum JobState")
    if start < 0:
        return []
    brace = masked.find("{", start)
    if brace < 0:
        return []
    depth = 0
    end = len(masked)
    for index in range(brace, len(masked)):
        if masked[index] == "{":
            depth += 1
        elif masked[index] == "}":
            depth -= 1
            if depth == 0:
                end = index
                break
    body = masked[brace + 1 : end]
    variants: list[str] = []
    for line in body.splitlines():
        token = line.strip().rstrip(",").strip()
        if not token or token.startswith("//") or token.startswith("#["):
            continue
        if re.fullmatch(r"[A-Z][A-Za-z0-9_]*", token):
            variants.append(token)
    return variants


def _terminal_states(masked: str) -> set[str]:
    """The states the domain's `is_terminal` names."""
    body = _body_of(masked, DOMAIN_JOBS_FILE, "is_terminal")
    if body is None:
        return set()
    return set(_VARIANT_USE.findall(body))


def _all_states(masked: str) -> list[str]:
    """The states `JobState::ALL` enumerates."""
    marker = "ALL: &'static [JobState] = &["
    start = masked.find(marker)
    if start < 0:
        return []
    start += len(marker)
    end = masked.find("]", start)
    if end < 0:
        return []
    return _VARIANT_USE.findall(masked[start:end])


def _groups(masked: str, known: set[str]) -> list[tuple[int, list[str]]]:
    """Every run of adjacent `JobState::` variants, with its offset.

    Two occurrences belong to the same run when the text between them is
    only whitespace, commas and pipes — the shape of a written-out list. A
    match arm (`JobState::Completed => ...`) puts an `=` and a `>` between two
    occurrences, so its arms are not a run.
    """
    uses = [
        (match.start(), match.group(1))
        for match in _VARIANT_USE.finditer(masked)
        if match.group(1) in known
    ]
    groups: list[tuple[int, list[str]]] = []
    index = 0
    while index < len(uses):
        start, name = uses[index]
        names = [name]
        end = index + 1
        while end < len(uses):
            between = masked[uses[end - 1][0] + len("JobState::") + len(names[-1]) : uses[end][0]]
            # A path qualifier may sit between two members of the same list:
            # `codegg_core::jobs::JobState::Completed`.
            between = re.sub(r"[A-Za-z_][A-Za-z0-9_]*::", "", between)
            if re.fullmatch(r"[\s,|]*", between):
                names.append(uses[end][1])
                end += 1
            else:
                break
        if len(names) >= 2:
            groups.append((start, names))
        index = end
    return groups


def _claim_name(masked: str, offset: int) -> str:
    """The identifier that names a claim.

    The nearest `let <name>` before the list names it; failing that, the
    enclosing function's name does.
    """
    before = masked[:offset]
    functions = list(_FN_NAME.finditer(before))
    last_function = functions[-1].start() if functions else 0
    bindings = [
        match
        for match in _LET_BINDING.finditer(before)
        if match.start() > last_function
    ]
    if bindings:
        return bindings[-1].group(1)
    return functions[-1].group(1) if functions else ""


def _words(identifier: str) -> set[str]:
    return set(re.split(r"[^A-Za-z]+", identifier.lower()))


def _is_query_state_list(masked: str, offset: int) -> bool:
    """True when the list is the `states:` argument of a job-store query.

    A path qualifier (`codegg_core::jobs::JobState::Completed`) or a `vec![`
    may sit between the field and the first variant, so both are stripped
    before the remainder is checked for being nothing but brackets.
    """
    prefix = masked[max(0, offset - 600) : offset]
    marker = prefix.rfind("states:")
    if marker < 0:
        return False
    between = prefix[marker + len("states:") :]
    stripped = re.sub(r"[A-Za-z_][A-Za-z0-9_]*::", "", between).replace("vec!", "")
    return re.fullmatch(r"[\s\[\]]*", stripped) is not None


def check_tree(root: Path) -> list[Finding]:
    """Every violation in the tree rooted at `root`."""
    findings: list[Finding] = []
    domain_file = root / DOMAIN_JOBS_FILE
    if not domain_file.is_file():
        return [Finding(DOMAIN_JOBS_FILE, "the domain's job-state module is missing")]

    domain_text = domain_file.read_text(encoding="utf-8")
    domain_masked = _mask(domain_text)
    variants = _enum_variants(domain_masked)
    if not variants:
        return [Finding(DOMAIN_JOBS_FILE, "the `JobState` enum could not be read")]
    known = set(variants)
    terminal = _terminal_states(domain_masked)
    if not terminal:
        return [
            Finding(
                f"{DOMAIN_JOBS_FILE}:is_terminal",
                "the domain's terminality predicate could not be read",
            )
        ]
    complement = known - terminal

    # Rule 2: the enumeration and every projection cover every variant.
    enumerated = _all_states(domain_masked)
    if sorted(enumerated) != sorted(variants):
        findings.append(
            Finding(
                f"{DOMAIN_JOBS_FILE}:JobState::ALL",
                "the domain's enumeration of its own states does not cover every "
                f"variant: enumerated {sorted(enumerated)}, variants {sorted(variants)}",
            )
        )
    for relative, function_name in PROJECTIONS:
        projection_file = root / relative
        if not projection_file.is_file():
            findings.append(
                Finding(relative, f"the projection `{function_name}` is missing")
            )
            continue
        text = projection_file.read_text(encoding="utf-8")
        body = _body_of(_mask(text), relative, function_name)
        if body is None:
            findings.append(
                Finding(
                    f"{relative}:{function_name}",
                    "the projection could not be read",
                )
            )
            continue
        covered = set(_VARIANT_USE.findall(body))
        missing = known - covered
        if missing:
            findings.append(
                Finding(
                    f"{relative}:{function_name}",
                    "the projection does not cover every job state: "
                    f"missing {sorted(missing)}",
                )
            )
        if re.search(r"(^|[\s,|])_\s*=>", body):
            findings.append(
                Finding(
                    f"{relative}:{function_name}",
                    "the projection has a wildcard arm, so a new job state "
                    "would silently fall into it",
                )
            )

    # Rule 1: no hand-written terminality list in production code. The test
    # trees are exempt: a test that writes the set out is pinning the
    # contract on purpose and fails loudly when the contract moves, while a
    # production list changes behaviour silently, which is what this guard
    # is for.
    for source in rust_test_scan.iter_source_files(root):
        relative = source.rel
        if source.tree not in ("src", "crates"):
            continue
        if relative.startswith(f"{DOMAIN_JOBS_DIR}/"):
            continue
        masked = source.mask
        for offset, names in _groups(masked, known):
            location = f"{relative}:{_line_of(masked, offset)}"
            unique = set(names)
            if unique == terminal or unique == complement:
                findings.append(
                    Finding(
                        location,
                        "a hand-written copy of the domain's "
                        + ("terminal" if unique == terminal else "non-terminal")
                        + " job states: derive it with `JobState::terminal_states()` "
                        "or `JobState::non_terminal_states()` instead",
                    )
                )
                continue
            if not _is_query_state_list(masked, offset):
                continue
            claim = _claim_name(masked, offset)
            words = _words(claim)
            if words & TERMINAL_WORDS and unique != terminal:
                findings.append(
                    Finding(
                        location,
                        f"the query named `{claim}` selects {sorted(unique)}, which is "
                        "not the set of states the domain calls terminal "
                        f"({sorted(terminal)})",
                    )
                )
            if words & LIVE_WORDS and unique != complement:
                findings.append(
                    Finding(
                        location,
                        f"the query named `{claim}` selects {sorted(unique)}, which is "
                        "not the set of states the domain does not call terminal "
                        f"({sorted(complement)})",
                    )
                )
    return findings


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------

_FIXTURE_DOMAIN = """\
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Scheduled,
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Interrupted,
    Blocked,
    Expired,
}

impl JobState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobState::Completed
                | JobState::Failed
                | JobState::Cancelled
                | JobState::TimedOut
                | JobState::Expired
        )
    }

    pub const ALL: &'static [JobState] = &[
        JobState::Scheduled,
        JobState::Queued,
        JobState::Running,
        JobState::Completed,
        JobState::Failed,
        JobState::Cancelled,
        JobState::TimedOut,
        JobState::Interrupted,
        JobState::Blocked,
        JobState::Expired,
    ];
}
"""

_FIXTURE_EVIDENCE = """\
use crate::jobs::JobState;

pub trait EvidenceVerdict: Copy {
    fn passed() -> Self;
}

pub fn job_success_evidence<V: EvidenceVerdict>(state: JobState) -> V {
    match state {
        JobState::Completed => V::passed(),
        JobState::Failed
        | JobState::Cancelled
        | JobState::TimedOut
        | JobState::Interrupted
        | JobState::Expired => V::passed(),
        JobState::Scheduled | JobState::Queued | JobState::Running | JobState::Blocked => {
            V::passed()
        }
    }
}
"""

_FIXTURE_STATUS = """\
use codegg_core::jobs::JobState;

pub fn job_status_projection(state: &JobState) -> &'static str {
    match state {
        JobState::Completed => "completed",
        JobState::Failed => "failed",
        JobState::Cancelled => "cancelled",
        JobState::TimedOut => "timed_out",
        JobState::Interrupted => "interrupted",
        JobState::Blocked => "blocked",
        JobState::Expired => "expired",
        JobState::Scheduled => "scheduled",
        JobState::Queued => "queued",
        JobState::Running => "running",
    }
}
"""

_CLEAN_QUERY = """\
use codegg_core::jobs::JobState;

pub fn snapshot() {
    let terminal_jobs = list(JobStoreQuery {
        session_id: None,
        states: JobState::terminal_states(),
        limit: Some(10),
    });
    let live_jobs = list(JobStoreQuery {
        session_id: None,
        states: JobState::non_terminal_states(),
        limit: Some(10),
    });
}
"""

_DIVERGENT_QUERY = """\
use codegg_core::jobs::JobState;

pub fn snapshot() {
    let terminal_jobs = list(JobStoreQuery {
        session_id: None,
        states: vec![
            JobState::Completed,
            JobState::Failed,
            JobState::Cancelled,
            JobState::TimedOut,
            JobState::Interrupted,
            JobState::Expired,
        ],
        limit: Some(10),
    });
}
"""

_HAND_WRITTEN_NON_TERMINAL = """\
use codegg_core::jobs::JobState;

pub fn sweep() {
    let job_records = list(JobStoreQuery {
        session_id: None,
        states: vec![
            JobState::Scheduled,
            JobState::Queued,
            JobState::Running,
            JobState::Interrupted,
            JobState::Blocked,
        ],
        limit: Some(1),
    });
}
"""

_NEW_VARIANT_DOMAIN = _FIXTURE_DOMAIN.replace(
    "    Expired,\n}", "    Expired,\n    Deferred,\n}"
).replace(
    "        JobState::Expired,\n    ];",
    "        JobState::Expired,\n        JobState::Deferred,\n    ];",
)


def _fixture_tree(root: Path, domain: str, extra: dict[str, str]) -> None:
    (root / DOMAIN_JOBS_DIR).mkdir(parents=True, exist_ok=True)
    (root / DOMAIN_JOBS_DIR / "mod.rs").write_text(domain, encoding="utf-8")
    (root / DOMAIN_JOBS_DIR / "evidence.rs").write_text(
        _FIXTURE_EVIDENCE, encoding="utf-8"
    )
    (root / "src" / "scheduler").mkdir(parents=True, exist_ok=True)
    (root / "src" / "scheduler" / "job_notifications.rs").write_text(
        _FIXTURE_STATUS, encoding="utf-8"
    )
    for relative, text in extra.items():
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")


def _self_test() -> int:
    """Prove the guard is not vacuous, by injecting violations."""
    print("check_job_state_terminality_agreement --self-test")
    failures: list[str] = []

    def report(name: str, condition: bool, detail: str = "") -> None:
        if condition:
            print(f"  ok: {name}")
        else:
            print(f"  FAIL: {name} {detail}")
            failures.append(name)

    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)

        _fixture_tree(root, _FIXTURE_DOMAIN, {"src/scheduler/clean.rs": _CLEAN_QUERY})
        clean = check_tree(root)
        report("a clean tree passes", not clean, f"got {[str(f) for f in clean]}")

        _fixture_tree(
            root, _FIXTURE_DOMAIN, {"src/scheduler/divergent.rs": _DIVERGENT_QUERY}
        )
        divergent = check_tree(root)
        report(
            "a query named `terminal_jobs` that selects `Interrupted` is rejected",
            any("terminal_jobs" in finding.message for finding in divergent),
            f"got {[str(f) for f in divergent]}",
        )

        _fixture_tree(
            root,
            _FIXTURE_DOMAIN,
            {"src/core/sweep.rs": _HAND_WRITTEN_NON_TERMINAL},
        )
        written = check_tree(root)
        report(
            "a hand-written copy of the domain's non-terminal states is rejected",
            any("non-terminal" in finding.message for finding in written),
            f"got {[str(f) for f in written]}",
        )

        _fixture_tree(root, _NEW_VARIANT_DOMAIN, {"src/scheduler/clean.rs": _CLEAN_QUERY})
        variant = check_tree(root)
        report(
            "a new job state the projections do not cover is rejected",
            any("does not cover every job state" in finding.message for finding in variant),
            f"got {[str(f) for f in variant]}",
        )

        _fixture_tree(root, _FIXTURE_DOMAIN, {"src/scheduler/clean.rs": _CLEAN_QUERY})
        (root / DOMAIN_JOBS_DIR / "evidence.rs").unlink()
        missing = check_tree(root)
        report(
            "a missing projection is rejected",
            any("is missing" in finding.message for finding in missing),
            f"got {[str(f) for f in missing]}",
        )

    if failures:
        print(f"check_job_state_terminality_agreement --self-test: {len(failures)} failing")
        return 1
    print("check_job_state_terminality_agreement --self-test: all checks pass")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects injected violations",
    )
    arguments = parser.parse_args()
    if arguments.self_test:
        return _self_test()
    findings = check_tree(REPO_ROOT)
    if findings:
        print("job-state terminality agreement check failed:")
        for finding in findings:
            print(f"  {finding}")
        return 1
    print("job-state terminality agreement check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
