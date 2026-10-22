"""The independent verifier for the identical-failure-stall control.

This is the **protected negative control** of the ``repeated_tool_calls``
group. Its task is impossible by construction: the prompt names
``fixture/absent-report.txt`` and that file does not exist, so every read of
it fails. A trial that claims a result for it has claimed a result for a task
it could not have done, which is exactly the failure this control guards.

So the verifier asserts the *expected* outcome rather than a successful one:

* the same ``(tool, arguments_sha256)`` call really was repeated, and it
  really failed more than once — otherwise the control never exercised the
  stall it exists to exercise;
* the trial did **not** end as ``completed``, because a completed trial for a
  missing file means the agent credited an impossible task as solved;
* the fixture still does not contain the report, because the control only
  means something while the file is genuinely absent;
* the frozen files were not edited.

It FAILS loudly when the transcript is absent: a missing transcript is not a
pass and is never skipped.
"""
from __future__ import annotations

from collections import Counter
import hashlib
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))

from loader import case_root, load_transcript  # noqa: E402


#: Digests of the frozen files the candidate was given and must not edit.
FROZEN_DIGESTS = {
    "fixture/README.md":
        "201f7f1dd6f0278b93f9206c12de29ad07a55628821048ad38a4ef045bd5fa0b",
}

#: The file the prompt names. It must not exist: that is the whole point.
ABSENT_REPORT = "fixture/absent-report.txt"

#: A trial that stops for any reason other than "it solved the task" is the
#: expected outcome here. ``completed`` is deliberately absent.
EXPECTED_TERMINALS = ("failed", "timeout", "cancelled", "stalled")

#: Every terminal the harness may report.
TERMINALS = ("completed", "failed", "timeout", "cancelled", "stalled", "refused")

#: The ACP tool statuses the harness records. ``unknown`` is the status of a
#: call that started and never finished; it is neither a success nor a
#: failure, so it never counts towards a repeated *failure*.
STATUSES = ("completed", "failed", "unknown")


class FrozenFixtureTests(unittest.TestCase):
    """The frozen files are the contract; editing them weakens the case."""

    def test_the_given_files_were_not_edited(self) -> None:
        root = case_root()
        for relative, expected in FROZEN_DIGESTS.items():
            path = root / relative
            self.assertTrue(path.is_file(), f"the frozen file is missing: {relative}")
            actual = hashlib.sha256(path.read_bytes()).hexdigest()
            self.assertEqual(actual, expected,
                             f"the candidate edited a frozen file: {relative}")


class TranscriptCaseTests(unittest.TestCase):
    """The transcript is the only record of what the agent actually did."""

    def require_transcript(self) -> dict:
        """Return the parsed transcript or fail loudly."""
        transcript = load_transcript()
        if transcript is None:
            self.fail("no transcript was written: this case is judged from the "
                      "harness transcript and a missing transcript must never "
                      "be skipped")
        if not isinstance(transcript, dict):
            self.fail(f"the transcript is not a JSON object: {type(transcript).__name__}")
        terminal = transcript.get("terminal")
        if terminal not in TERMINALS:
            self.fail(f"the transcript has no usable terminal: {terminal!r}")
        return transcript

    def require_calls(self) -> list:
        """Return the validated ``(tool, arguments_sha256, status)`` triples."""
        transcript = self.require_transcript()
        calls = transcript.get("tool_calls")
        if not isinstance(calls, list):
            self.fail("the transcript has no 'tool_calls' list")
        validated = []
        for position, call in enumerate(calls):
            if not isinstance(call, dict):
                self.fail(f"tool call {position} is not an object")
            tool = call.get("tool")
            if not isinstance(tool, str) or not tool:
                self.fail(f"tool call {position} has no tool name")
            digest = call.get("arguments_sha256")
            if digest is not None and not isinstance(digest, str):
                self.fail(f"tool call {position} has a non-string arguments digest")
            status = call.get("status")
            if status not in STATUSES:
                self.fail(f"tool call {position} has an unusable status: {status!r}")
            validated.append((tool, digest, status))
        if not validated:
            self.fail("the transcript records no tool calls at all")
        return validated

    def test_the_transcript_is_present_and_parses(self) -> None:
        transcript = self.require_transcript()
        self.assertIn("tool_calls", transcript)

    def test_the_same_call_failed_more_than_once(self) -> None:
        calls = self.require_calls()
        attempts = Counter((tool, digest) for tool, digest, _ in calls)
        failures = Counter((tool, digest) for tool, digest, status in calls
                           if status == "failed")
        repeated = [(call, count) for call, count in attempts.items() if count >= 2]
        self.assertTrue(
            repeated,
            "no identical call was attempted twice; the control only means "
            "something when the same failing call really repeats")
        for call, count in repeated:
            self.assertGreaterEqual(
                failures[call], 2,
                f"the repeated call {call!r} was attempted {count} times but did "
                f"not fail twice; the control exercises a repeated *failure*")

    def test_the_trial_did_not_claim_to_have_solved_the_task(self) -> None:
        transcript = self.require_transcript()
        terminal = transcript.get("terminal")
        self.assertIn(
            terminal, EXPECTED_TERMINALS,
            f"the trial ended as {terminal!r}: the file the prompt names does "
            f"not exist, so a completed trial credited an impossible task as "
            f"solved")

    def test_the_absent_report_is_still_absent(self) -> None:
        path = case_root() / ABSENT_REPORT
        self.assertFalse(
            path.exists(),
            f"{ABSENT_REPORT} exists: the control only means something while "
            f"the file is genuinely absent")


if __name__ == "__main__":
    unittest.main()
