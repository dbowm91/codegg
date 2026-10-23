"""The independent verifier for the background-job-wait case.

This case is about behaviour over time, not about an edited file: the
agent has to start a slow build as a background job, use the waiting time
to read the project, and then let the build finish and report it. The only
record of that behaviour is the harness-written trial transcript, so the
transcript is what this verifier judges.

Why the tests are split the way they are
----------------------------------------
The shipped case carries no transcript: the harness writes one only when
it runs a live trial. A standalone run of the shipped case therefore has
no evidence at all, and a verifier that *skipped* the transcript
assertions would report success for a case nobody solved — the case would
be vacuous. So the transcript-dependent properties live in their own test
methods that **fail loudly** when the transcript is absent, and the
artifact check lives in a separate method that needs no transcript at
all.

The consequence is deliberate: ``python3 -m unittest discover -s verifier
-v`` on the shipped case exits non-zero (the transcript tests fail, and so
does the artifact check, because the build has not run). On a live trial
the harness writes the transcript and the artifact exists, and the
verifier exits 0 only when the agent behaved as the prompt asks.
"""
from __future__ import annotations

import hashlib
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))

from loader import (  # noqa: E402
    case_root,
    fixture_root,
    load_transcript,
    transcript_path,
)

#: The file the build writes once it has finished.
ARTIFACT_NAME = "build-output.txt"

#: Digests of the frozen files the agent was given and must not edit.
#: Editing ``build.sh`` would let an agent shorten the wait, and editing
#: ``src/`` would let it fabricate an artifact, so both are frozen.
FROZEN_DIGESTS = {
    "README.md":
        "da1bde30e37c6026fb4ade04aecc14833c13ac49751581b54ae2a60c9369d897",
    "build.sh":
        "36ef81d32b5ce5d66e9aefbe4fb00af4a6cf4e82d5baa31decf387735047d2e1",
    "src/01-header.txt":
        "2158471fa46fcc8f0383c942f2ffbb0c44431f9cd43a4e8f451c0c50aaa66a98",
    "src/02-body.txt":
        "becbb056d2900221dec303c95fefb0b31503a765f63525b3ef85c785520e2d22",
    "src/03-footer.txt":
        "159a6bb5780773f660145ab8c3e9d0f4d403798014d0b7c5996c87576369a65d",
    "expected/build-output.expected.txt":
        "fc50bb1599e6c3d351834e9bec7a819b96d82615f9b6431f5d03c0b1e9ad60c8",
}


def project_root() -> Path:
    """Return the directory that holds ``build.sh`` and ``src/``.

    Normally that is ``fixture/``. The harness copies ``fixture/`` into a
    private lane workspace and copies ``verifier/`` somewhere else, so the
    tree is located through the case root rather than through this file's
    own location.
    """
    for candidate in (fixture_root(), case_root()):
        if (candidate / "build.sh").is_file():
            return candidate
    return fixture_root()


def staged_sources(root: Path) -> list[Path]:
    """Return the source files the build stages, in the order it stages them."""
    src = root / "src"
    if not src.is_dir():
        return []
    return sorted(
        (path for path in src.iterdir() if path.is_file() and path.suffix == ".txt"),
        key=lambda path: path.name,
    )


def expected_artifact_text(root: Path) -> str:
    """Re-derive the artifact the build writes, from the fixture's own sources.

    The verifier does not trust a copy of the expected output sitting in
    the tree; it rebuilds the expectation from the sources it can read.
    """
    sources = staged_sources(root)
    parts = ["build: ok\n", "result: BUILD_OK\n", f"sources: {len(sources)}\n"]
    for source in sources:
        parts.append(source.read_text(encoding="utf-8"))
    return "".join(parts)


def ordered_tool_calls(transcript: dict) -> list[str]:
    """Return the tool names of the trial in call order."""
    calls = transcript.get("tool_calls")
    if not isinstance(calls, list):
        return []
    entries = []
    for position, call in enumerate(calls):
        if not isinstance(call, dict):
            continue
        index = call.get("index")
        key = index if isinstance(index, int) and not isinstance(index, bool) else position
        entries.append((key, position, str(call.get("tool", ""))))
    entries.sort(key=lambda entry: (entry[0], entry[1]))
    return [tool for _, _, tool in entries]


class TranscriptContractTests(unittest.TestCase):
    """Properties only the trial transcript can show.

    These tests must fail, never skip, when the transcript is missing: a
    standalone run of the shipped case has no evidence of the agent's
    behaviour, and skipping would report a success that nobody earned.
    """

    def require_transcript(self) -> dict:
        """Return the trial transcript, failing loudly when there is none."""
        try:
            transcript = load_transcript()
        except (OSError, ValueError) as error:
            self.fail(f"the trial transcript could not be parsed: {error}")
        self.assertIsNotNone(
            transcript,
            "no trial transcript was found, so this run has no evidence of the "
            "agent's behaviour. The harness writes one for every live trial; "
            f"looked for {transcript_path() or (case_root() / 'TRANSCRIPT.json')}",
        )
        self.assertIsInstance(transcript, dict)
        return transcript

    def test_the_trial_transcript_is_present_and_parsable(self) -> None:
        transcript = self.require_transcript()
        self.assertIn("terminal", transcript,
                      "the transcript carries no terminal outcome")
        self.assertIsInstance(transcript["terminal"], str)
        self.assertIn("tool_calls", transcript,
                      "the transcript carries no tool calls")
        self.assertIsInstance(transcript["tool_calls"], list)

    def test_the_build_was_started_with_a_shell_command(self) -> None:
        transcript = self.require_transcript()
        tools = ordered_tool_calls(transcript)
        self.assertTrue(
            any(tool in {"bash", "terminal"} for tool in tools),
            f"the agent never started the build; the trial only called {tools}",
        )

    def test_the_agent_observed_the_background_job_with_the_job_tool(self) -> None:
        transcript = self.require_transcript()
        tools = ordered_tool_calls(transcript)
        self.assertTrue(
            "job" in tools,
            "the agent never observed the background job with the job tool; "
            f"the trial only called {tools}",
        )

    def test_the_waiting_time_was_used_to_read_the_project(self) -> None:
        transcript = self.require_transcript()
        tools = ordered_tool_calls(transcript)
        job_positions = [index for index, tool in enumerate(tools) if tool == "job"]
        self.assertTrue(job_positions,
                        "the agent never observed the background job")
        # The wait starts when the build is **launched**, not when the agent
        # first queries the job. An agent that starts the build and then
        # reads the project before it ever touches the job tool has used the
        # waiting time exactly as the prompt asks; requiring the reads to
        # land between two job queries would fail that correct behaviour.
        # The launch is a `bash` call — the harness does not project the tool
        # arguments, so the first `bash` call is the only observable marker
        # of the launch.
        bash_positions = [index for index, tool in enumerate(tools) if tool == "bash"]
        self.assertTrue(bash_positions,
                        "the agent never launched the build with a shell call")
        reads = [index for index, tool in enumerate(tools) if tool in {"read", "grep"}]
        between = [index for index in reads
                   if bash_positions[0] < index < job_positions[-1]]
        self.assertTrue(
            between,
            "the agent did not use the waiting time: no read or grep call "
            "happened after the build was launched and before the job was "
            f"last observed; the trial called {tools}",
        )

    def test_the_slow_build_was_not_reported_as_a_stall(self) -> None:
        transcript = self.require_transcript()
        terminal = transcript.get("terminal")
        self.assertEqual(
            terminal, "completed",
            "the slow but valid build was not carried to completion "
            f"(terminal was {terminal!r}); a long-running call must not be "
            "reported as a stall",
        )


class BuildArtifactTests(unittest.TestCase):
    """The build's own output. This check needs no transcript."""

    def test_the_build_produced_its_result_artifact(self) -> None:
        root = project_root()
        artifact = root / ARTIFACT_NAME
        self.assertTrue(
            artifact.is_file(),
            f"the build artifact is missing: {artifact}. It only appears once "
            "the slow build has run to completion, so its absence means the "
            "build was abandoned rather than finished",
        )
        text = artifact.read_text(encoding="utf-8")
        expected = expected_artifact_text(root)
        self.assertIn(
            expected, text,
            f"{artifact} does not hold the staged build output; expected it "
            "to contain the three header lines followed by the contents of "
            "src/ in name order",
        )


class FrozenFixtureTests(unittest.TestCase):
    """The files the agent was given and must not have edited."""

    def test_the_given_files_were_not_edited(self) -> None:
        root = project_root()
        for relative, expected in FROZEN_DIGESTS.items():
            path = root / relative
            self.assertTrue(path.is_file(), f"the frozen file is missing: {relative}")
            actual = hashlib.sha256(path.read_bytes()).hexdigest()
            self.assertEqual(actual, expected,
                             f"the candidate edited a frozen file: {relative}")


if __name__ == "__main__":
    unittest.main()
