"""The independent verifier for the distinct-tool-calls case.

This case does not judge the agent's answer text; it judges the *transcript*.
The prompt asks for a spread of different investigations, and the failure mode
this case exists to catch is a repeated-call stall detector that fires on a
run of legitimately distinct calls. So the transcript must show:

* a real spread of distinct ``(tool, arguments_sha256)`` pairs — the harness
  does not project raw arguments, so that pair is the only evidence of
  "different call" available here;
* more than one kind of tool, because the prompt asks for reads *and* shell
  commands and a run that only ever called ``read`` did not do the task;
* a ``completed`` terminal, because a run that was stopped as a stall or a
  timeout has failed this case's core property;
* no single call hammered more than twice, because repeatedly issuing one
  identical call is exactly the behaviour this case must not exhibit.

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
        "84c1e056bcbe9562811dc97b40bf8adf1e8c2cc623797e362f6407b680505458",
}

#: The prompt asks for several different files *and* several different
#: commands. Five distinct call pairs is the floor a single repeated call can
#: never reach, and a handful of real investigations clears it comfortably.
MIN_DISTINCT_CALLS = 5

#: The prompt names reads, greps, listings and shell commands. A run that
#: only ever issued one kind of call did not do the task.
MIN_DISTINCT_TOOLS = 3

#: One retry of an identical call is ordinary work; hammering the same call
#: is the repeated-call behaviour this case must not exhibit.
MAX_ATTEMPTS_PER_CALL = 2

#: The only terminal that means "the investigation finished".
TERMINAL_COMPLETED = "completed"

#: Every terminal the harness may report.
TERMINALS = ("completed", "failed", "timeout", "cancelled", "stalled", "refused")

#: The ACP tool statuses the harness records. ``unknown`` is the status of a
#: call that started and never finished; it is neither a success nor a
#: failure, but the call still happened.
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

    def test_the_trial_made_at_least_five_distinct_calls(self) -> None:
        calls = self.require_calls()
        pairs = {(tool, digest) for tool, digest, _ in calls}
        self.assertGreaterEqual(
            len(pairs), MIN_DISTINCT_CALLS,
            f"the trial made only {len(pairs)} distinct calls; the prompt asks "
            f"for several different files and several different commands, and a "
            f"repeated identical call does not add a distinct call")

    def test_the_trial_used_at_least_three_distinct_tools(self) -> None:
        calls = self.require_calls()
        tools = {tool for tool, _, _ in calls}
        self.assertGreaterEqual(
            len(tools), MIN_DISTINCT_TOOLS,
            f"the trial only used {sorted(tools)}; the prompt asks for reads "
            f"and shell commands, so one kind of tool is not the task")

    def test_the_trial_completed_instead_of_stopping_as_a_stall(self) -> None:
        transcript = self.require_transcript()
        self.assertEqual(
            transcript.get("terminal"), TERMINAL_COMPLETED,
            "the trial did not complete: many distinct calls must not be "
            "mistaken for a repeated-call stall")

    def test_no_single_call_was_attempted_more_than_twice(self) -> None:
        calls = self.require_calls()
        counts = Counter((tool, digest) for tool, digest, _ in calls)
        (worst, attempts) = counts.most_common(1)[0]
        self.assertLessEqual(
            attempts, MAX_ATTEMPTS_PER_CALL,
            f"the call {worst!r} was attempted {attempts} times; repeatedly "
            f"issuing one identical call is exactly the behaviour this case "
            f"must not exhibit")


if __name__ == "__main__":
    unittest.main()
