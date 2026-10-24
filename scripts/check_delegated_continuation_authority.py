#!/usr/bin/env python3
"""Guard: a delegated run reaches a provider entry only as its original
attempt or as a claimed continuation.

WHAT THIS GUARD DOES
    A delegated run's provider entry is the place where the child's own turn
    begins. Two things must be true there, and neither is obvious from the
    call site:

    1. **The attempt is accounted for.** The run's attempt record says which
       attempt is running: the run's first, or a continuation claimed against
       a terminal failure. A dispatch that resolves no attempt has no
       authority behind it, so the attempt record is what the guard reads.

    2. **A continuation carries its seeded context.** A claimed continuation
       is seeded from the run's recorded transcript, read back from the store
       and re-verified. A continuation that reaches a provider entry without
       that context would run on an empty conversation and present itself as
       a continuation.

    The scheduler side has the matching rule: a terminal delegated run may be
    resumed only by a claimed continuation. Without that gate, any job that
    names a terminal run could resurrect it.

WHAT THIS GUARD DELIBERATELY DOES NOT DO
    It does not prove the runtime behaviour; it proves the **shape** of the
    guard is present in the source, in the right order, so a later edit that
    drops the check is caught by the guard rather than by a test nobody ran.
    The runtime behaviour is proved by the Rust tests next to it.

    It is a static, ordered-landmark check, not a type checker. A landmark
    that is renamed in the source must be renamed here too; the guard fails
    loudly when a landmark it expects is missing, so a rename cannot silently
    turn the check into a no-op.

USAGE
    python3 scripts/check_delegated_continuation_authority.py            # check
    python3 scripts/check_delegated_continuation_authority.py --self-test  # bite
"""

from __future__ import annotations

import argparse
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional, Tuple


ROOT = Path(__file__).resolve().parent.parent

WORKER = Path("src") / "agent" / "worker.rs"
EXECUTOR = Path("src") / "scheduler" / "executors.rs"

# The landmarks that must appear, in this order, before the provider entry.
#
# `PROVIDER_ENTRY` is the child's own turn: the place the provider is
# reached. Everything before it in the list is a precondition of that reach.
WORKER_LANDMARKS: Tuple[Tuple[str, str], ...] = (
    ("provenance", "delegated_attempt_provenance("),
    ("seed", "seed_delegated_context("),
    ("seed-refusal", "a claimed continuation may not reach a provider entry"),
)
WORKER_PROVIDER_ENTRY = "agent_loop.run("

# The scheduler side: a terminal run is resumed only by a claimed
# continuation, and the attempt ordinal comes from the attempt record.
EXECUTOR_LANDMARKS: Tuple[Tuple[str, str], ...] = (
    ("attempt-record", "attempt_store.attempts("),
    ("continuation-record", "attempt_store.continuation("),
    ("continuation-gate", "a terminal delegated run may only be resumed by a claimed continuation"),
)


@dataclass(frozen=True)
class Finding:
    file: str
    line: int
    rule: str
    message: str


def _line_of(text: str, index: int) -> int:
    return text.count("\n", 0, index) + 1


def _find_call(text: str, marker: str, start: int) -> int:
    """The next *call* of `marker` at or after `start`.

    A function's own definition carries the same marker, so a definition is
    skipped: the guard must find the call site, not the declaration. The
    lookup is textual rather than syntactic on purpose — rustfmt may reflow a
    call across lines, and a guard that breaks when the file is reformatted
    protects nothing.
    """
    index = text.find(marker, start)
    while index >= 0:
        line_start = text.rfind("\n", 0, index) + 1
        prefix = text[line_start:index].strip()
        if not prefix.endswith("fn"):
            return index
        index = text.find(marker, index + len(marker))
    return -1


def _check_ordered_landmarks(
    path: Path,
    relative: str,
    landmarks: Tuple[Tuple[str, str], ...],
    terminal: str,
    rules: Tuple[str, str],
) -> List[Finding]:
    """Every landmark must appear, in order, before the terminal marker."""
    findings: List[Finding] = []
    if not path.is_file():
        return [
            Finding(
                relative,
                0,
                rules[0],
                "the file is missing, so the delegated provider entry cannot be shown to be guarded",
            )
        ]
    text = path.read_text(encoding="utf-8")
    terminal_index = text.find(terminal)
    if terminal_index < 0:
        return [
            Finding(
                relative,
                0,
                rules[1],
                f"the provider entry marker {terminal!r} is missing, so the guard cannot tell "
                "whether the attempt is accounted for",
            )
        ]
    cursor = 0
    for (name, landmark), rule in zip(landmarks, (rules[0],) * len(landmarks)):
        index = _find_call(text, landmark, cursor)
        if index < 0:
            findings.append(
                Finding(
                    relative,
                    _line_of(text, cursor),
                    rule,
                    f"the {name} check ({landmark!r}) is missing before the provider entry",
                )
            )
            continue
        if index > terminal_index:
            findings.append(
                Finding(
                    relative,
                    _line_of(text, index),
                    rule,
                    f"the {name} check ({landmark!r}) appears after the provider entry, so the "
                    "provider entry is reachable without it",
                )
            )
            continue
        cursor = index + len(landmark)
    return findings


def check_tree(root: Path) -> List[Finding]:
    findings: List[Finding] = []
    findings.extend(
        _check_ordered_landmarks(
            root / WORKER,
            WORKER.as_posix(),
            WORKER_LANDMARKS,
            WORKER_PROVIDER_ENTRY,
            ("delegated-continuation-unaccounted", "delegated-continuation-unaccounted"),
        )
    )
    findings.extend(
        _check_ordered_landmarks(
            root / EXECUTOR,
            EXECUTOR.as_posix(),
            EXECUTOR_LANDMARKS,
            "resume_for_continuation(",
            ("delegated-continuation-unaccounted", "delegated-continuation-unaccounted"),
        )
    )
    return findings


def _copy_tree(source: Path, destination: Path) -> None:
    for relative in (WORKER, EXECUTOR):
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text((source / relative).read_text(encoding="utf-8"), encoding="utf-8")


def _self_test_cases() -> List[Tuple[str, str, str]]:
    """The injected defects, each with the text that removes the check."""
    return [
        (
            "the attempt provenance is no longer resolved before the provider entry",
            "        delegated_attempt_provenance(attempt_store.as_ref(), transcript_run_id.as_ref()).await?;",
            "        let _provenance = None::<Option<DelegatedAttemptProvenance>>;",
        ),
        (
            "a claimed continuation may reach the provider entry without its seeded context",
            '        "a claimed continuation may not reach a provider entry without its seeded context"',
            '        "a claimed continuation may reach the provider entry"',
        ),
        (
            "a terminal delegated run may be resumed without a claimed continuation",
            '                        "a terminal delegated run may only be resumed by a claimed continuation"',
            '                        "a terminal delegated run may be resumed"',
        ),
    ]


def self_test() -> int:
    """Prove the guard is not vacuous."""
    failures: List[str] = []
    cases = 0
    with tempfile.TemporaryDirectory() as scratch:
        scratch_root = Path(scratch)

        baseline_root = scratch_root / "baseline"
        _copy_tree(ROOT, baseline_root)
        baseline = check_tree(baseline_root)
        if baseline:
            failures.append(
                "the guard reports findings on the unmodified tree, so its checks are not "
                f"measuring what they claim: {baseline}"
            )

        for description, old, new in _self_test_cases():
            cases += 1
            work = scratch_root / str(cases)
            _copy_tree(ROOT, work)
            injected = False
            for relative in (WORKER, EXECUTOR):
                target = work / relative
                text = target.read_text(encoding="utf-8")
                if old in text:
                    target.write_text(text.replace(old, new, 1), encoding="utf-8")
                    injected = True
                    break
            if not injected:
                failures.append(f"{description}: the fixture could not be injected")
                continue
            reported = check_tree(work)
            if not reported:
                failures.append(f"{description}: the guard did not reject the injected defect")
                continue
            print(f"  ok   {description} (rejected by {reported[0].rule})")

    if failures:
        for failure in failures:
            print(f"self-test failed: {failure}", file=sys.stderr)
        return 1
    print(
        f"delegated-continuation-authority self-test passed: {cases} injected defects were each "
        "rejected by the guard, and the unmodified tree produced no finding"
    )
    return 0


def main(argv: List[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects the drift it claims to catch",
    )
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()

    findings = check_tree(ROOT)
    if findings:
        print("delegated-continuation-authority: FAILED", file=sys.stderr)
        for finding in findings:
            print(f"  - {finding.file}:{finding.line}: {finding.message}", file=sys.stderr)
        return 1

    print(
        "delegated-continuation-authority check ok: a delegated run's provider entry is "
        "accounted for by its attempt record, and a continuation carries its seeded context"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
