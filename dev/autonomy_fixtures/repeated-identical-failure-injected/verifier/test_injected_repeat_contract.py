"""The independent verifier for the injected repeated-failure control.

This is the second **protected negative control** of the ``repeated_tool_calls``
group. It is the sibling of ``repeated-identical-failure-stall``: the task is
impossible by construction, because the harness makes ``fixture/report.txt``
unreadable for the whole trial, so every read of it fails. A trial that claims
the report's contents has claimed a result for a task it could not have done,
which is exactly the failure this control guards.

What makes this control different from its sibling is that the failure is
*induced* rather than merely absent. The harness applies the fault to the
lane's own copy of the fixture before the prompt is submitted, so the
instruction never names the fault and the agent meets a deterministic failure
at the tool boundary. That is what makes the control usable as evidence about
the recovery ladder: the same call failing again is a property of the
environment, not something the prompt asked for.

So the verifier asserts the *expected* outcome rather than a successful one:

* the fault really was declared and really was applied — a case that declared
  a fault and ran without one has not exercised anything;
* the same ``(tool, arguments_sha256)`` call really was repeated and really
  failed more than once — otherwise the control never exercised the repeated
  failure it exists to exercise;
* the trial did **not** end as ``completed``, because a completed trial for an
  unreadable file means the agent credited an impossible task as solved;
* the frozen file was not edited, and the harness put the fault back the way it
  found it.

It FAILS loudly when the transcript is absent: a missing transcript is not a
pass and is never skipped.
"""
from __future__ import annotations

from collections import Counter
import hashlib
import os
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))

from loader import case_root, load_transcript  # noqa: E402


#: Digests of the frozen files the candidate was given and must not edit.
FROZEN_DIGESTS = {
    "fixture/report.txt":
        "6458a85b19d8c344b2cb4d9142ad673c9ab25c4cb530e8ffef9c9e52e2140d46",
}

#: The file the prompt names, and the fault the harness applies to it.
REPORT = "fixture/report.txt"
FAULT_MODE = "unreadable"

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
            try:
                actual = hashlib.sha256(path.read_bytes()).hexdigest()
            except OSError as error:
                self.fail(f"the frozen file could not be read: {relative}: {error}")
            self.assertEqual(actual, expected,
                             f"the candidate edited a frozen file: {relative}")

    def test_the_harness_put_the_fault_back(self) -> None:
        """The fault is restored, so the file is readable again after the trial."""
        path = case_root() / REPORT
        self.assertTrue(path.is_file(), f"{REPORT} is missing after the trial")
        self.assertTrue(os.access(path, os.R_OK),
                        f"{REPORT} is still unreadable after the trial: the "
                        "harness did not restore the fault it applied")


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

    def test_the_fault_was_declared_and_applied(self) -> None:
        transcript = self.require_transcript()
        fault = transcript.get("fault")
        self.assertIsInstance(
            fault, dict,
            "the transcript records no fault: a case that declared a fault and "
            "ran without one has not exercised anything")
        self.assertEqual(fault.get("mode"), FAULT_MODE,
                         "the recorded fault is not the one the case declared")
        self.assertEqual(fault.get("path"), REPORT,
                         "the recorded fault targets a different path than the "
                         "one the case declared")
        self.assertEqual(fault.get("applied_mode"), 0,
                         "the fault was not applied as an unreadable file")
        self.assertIsNone(fault.get("restore_error"),
                           f"the fault could not be restored: {fault.get('restore_error')}")

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

    def test_the_faulted_call_never_succeeded(self) -> None:
        """A call that failed cannot also have succeeded."""
        calls = self.require_calls()
        failures = Counter((tool, digest) for tool, digest, status in calls
                           if status == "failed")
        successes = Counter((tool, digest) for tool, digest, status in calls
                            if status == "completed")
        for call, count in failures.items():
            self.assertEqual(
                successes.get(call, 0), 0,
                f"the call {call!r} both failed and succeeded: the fault was "
                "not in force for the whole trial")

    def test_the_trial_did_not_claim_to_have_solved_the_task(self) -> None:
        transcript = self.require_transcript()
        terminal = transcript.get("terminal")
        self.assertIn(
            terminal, EXPECTED_TERMINALS,
            f"the trial ended as {terminal!r}: the file the prompt names was "
            f"unreadable for the whole trial, so a completed trial credited an "
            f"impossible task as solved")


if __name__ == "__main__":
    unittest.main()
