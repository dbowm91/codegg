"""Standing guards for every materialized autonomy case.

The taxonomy freezes *where* each verifier runs and *what* it must judge, but
a verifier that nobody ever exercises is a claim, not a check. Every case body
that ships in ``dev/autonomy_fixtures/`` therefore gets the same three standing
properties, proven here by execution rather than asserted in prose:

1. the verifier **fails** while the task is unsolved, so a case that nobody
   solved can never read as a pass;
2. the verifier **passes** once the task is solved, so a case that was solved
   is not silently impossible;
3. the verifier **refuses** a weakened case, so editing a frozen file is not a
   way to make a failing case pass.

The repairs below are the *reference* repairs: the same defect an agent is
asked to find, fixed the way the documented contract says it must be fixed.
They are applied to a throwaway copy, never to the shipped case.
"""
from __future__ import annotations

import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
FIXTURE_ROOT = REPO_ROOT / "dev" / "autonomy_fixtures"

#: The verifier command each case's manifest declares.
UNITTEST_COMMAND = [sys.executable, "-m", "unittest", "discover", "-s", "verifier"]
SCRIPT_COMMAND = [sys.executable, "verifier/verify.py"]


def _replace(path: pathlib.Path, old: str, new: str) -> None:
    """Replace one exact fragment, refusing to edit a file that drifted.

    A silent no-op would turn a standing guard into a green light for a case
    that nobody fixed, so a missing fragment fails the test loudly.
    """
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise AssertionError(f"the expected fragment is missing from {path}: {old!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def _write_transcript(case: pathlib.Path, terminal: str, tool_calls: list[dict]) -> None:
    """Write a trial transcript into a case copy."""
    payload = {"schema_version": 1, "terminal": terminal, "stop_reason": "end_turn",
               "tool_calls": tool_calls}
    (case / "TRANSCRIPT.json").write_text(json.dumps(payload, indent=2), encoding="utf-8")


def _call(index: int, tool: str, digest: str, status: str = "completed") -> dict:
    return {"index": index, "tool": tool, "arguments_sha256": digest, "status": status}


def _solve_read_search_patch_python(case: pathlib.Path) -> None:
    """Stop dropping the record's trailing empty field."""
    _replace(case / "fixture" / "src" / "parser.py",
             '    while fields and fields[-1] == "":\n        fields.pop()\n', "")


def _solve_read_search_patch_rust(case: pathlib.Path) -> None:
    """Stop folding the record's trailing empty fields away."""
    parse = case / "fixture" / "src" / "parse.rs"
    _replace(parse, "    settle(&mut fields);\n", "")
    _replace(parse, "    let mut fields: Vec<String> = raw", "    let fields: Vec<String> = raw")
    text = parse.read_text(encoding="utf-8")
    start = text.index("/// Fold the record's trailing fields")
    parse.write_text(text[:start].rstrip() + "\n", encoding="utf-8")


def _solve_compile_debug_rust(case: pathlib.Path) -> None:
    """Fix the compile error and the three logic bugs the contract forbids."""
    _replace(case / "fixture" / "src" / "range.rs",
             "Some(Range { low: first, high: last })",
             "Some(Range { lower: first, upper: last })")
    measure = case / "fixture" / "src" / "measure.rs"
    _replace(measure, "    range.upper() - range.lower()\n",
             "    range.upper() - range.lower() + 1\n")
    _replace(measure,
             "    } else if value > range.upper() {\n        range.lower()\n",
             "    } else if value > range.upper() {\n        range.upper()\n")
    spec = case / "fixture" / "src" / "spec.rs"
    _replace(spec,
             "    let low: i64 = low_text\n        .parse()\n        .map_err(|_| SpecError::Malformed(text.to_string()))?;\n"
             "    let high: i64 = high_text\n        .parse()\n        .map_err(|_| SpecError::Malformed(text.to_string()))?;\n",
             "    let parse_bound = |field: &str| -> Result<i64, SpecError> {\n"
             "        if field.is_empty() || !field.bytes().all(|byte| byte.is_ascii_digit()) {\n"
             "            return Err(SpecError::Malformed(text.to_string()));\n"
             "        }\n"
             "        field\n"
             "            .parse::<i64>()\n"
             "            .map_err(|_| SpecError::Malformed(text.to_string()))\n"
             "    };\n"
             "    let low = parse_bound(low_text)?;\n"
             "    let high = parse_bound(high_text)?;\n")


def _solve_background_job_wait(case: pathlib.Path) -> None:
    """Run the build to completion and record how the agent behaved."""
    fixture = case / "fixture"
    sources = sorted((path for path in (fixture / "src").iterdir()
                      if path.is_file() and path.suffix == ".txt"),
                     key=lambda path: path.name)
    parts = ["build: ok\n", "result: BUILD_OK\n", f"sources: {len(sources)}\n"]
    for source in sources:
        parts.append(source.read_text(encoding="utf-8"))
    (fixture / "build-output.txt").write_text("".join(parts), encoding="utf-8")
    # The build is launched with a shell call, the agent reads the project
    # while it runs, and the job is observed afterwards. The wait starts at
    # the launch, so the reads belong between the launch and the last job
    # observation.
    _write_transcript(case, "completed", [
        _call(0, "bash", "a" * 64),
        _call(1, "job", "b" * 64),
        _call(2, "read", "c" * 64),
        _call(3, "grep", "d" * 64),
        _call(4, "job", "e" * 64),
    ])


def _solve_repeated_distinct_tool_calls(case: pathlib.Path) -> None:
    """Record a run of distinct calls that was not mistaken for a stall."""
    _write_transcript(case, "completed", [
        _call(0, "list", "a" * 64),
        _call(1, "read", "b" * 64),
        _call(2, "read", "c" * 64),
        _call(3, "grep", "d" * 64),
        _call(4, "bash", "e" * 64),
        _call(5, "read", "c" * 64),
    ])


def _solve_repeated_identical_failure_stall(case: pathlib.Path) -> None:
    """Record the identical repeated failure that must stop the trial."""
    _write_transcript(case, "stalled", [
        _call(0, "read", "a" * 64, status="failed"),
        _call(1, "read", "a" * 64, status="failed"),
        _call(2, "read", "a" * 64, status="failed"),
    ])


#: Every materialized case, with the command that runs its verifier, the file
#: the agent must not edit, and the repair that makes the case solved.
CASES = (
    ("read-search-patch-python", UNITTEST_COMMAND, "fixture/tests/test_parser.py",
     _solve_read_search_patch_python),
    ("read-search-patch-rust", UNITTEST_COMMAND, "fixture/tests/parser_test.rs",
     _solve_read_search_patch_rust),
    ("compile-debug-rust", SCRIPT_COMMAND, "fixture/tests/range_contract.rs",
     _solve_compile_debug_rust),
    ("background-job-wait", UNITTEST_COMMAND, "fixture/README.md",
     _solve_background_job_wait),
    ("repeated-distinct-tool-calls", UNITTEST_COMMAND, "fixture/README.md",
     _solve_repeated_distinct_tool_calls),
    ("repeated-identical-failure-stall", UNITTEST_COMMAND, "fixture/README.md",
     _solve_repeated_identical_failure_stall),
)


class MaterializedCaseTests(unittest.TestCase):
    """Every shipped case body proves its own verifier is trustworthy."""

    def _case_root(self, case_id: str) -> pathlib.Path:
        root = FIXTURE_ROOT / case_id
        self.assertTrue(root.is_dir(), f"the case body is missing: {root}")
        return root

    def _run_verifier(self, case_id: str, command: list[str], root: pathlib.Path) -> subprocess.CompletedProcess:
        return subprocess.run(command, cwd=str(root), capture_output=True, text=True,
                              timeout=300, check=False)

    def test_every_manifest_case_has_a_materialized_body(self) -> None:
        manifest = REPO_ROOT / "dev" / "harness_lab" / "fixtures" / "agent_autonomy" / "cases.jsonl"
        supported = []
        for line in manifest.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            entry = json.loads(line)
            if entry.get("support") == "supported":
                supported.append(entry["case_id"])
        self.assertTrue(supported, "the manifest declares no supported case")
        for case_id in supported:
            root = self._case_root(case_id)
            self.assertTrue((root / "fixture").is_dir(), f"{case_id} has no fixture")
            self.assertTrue((root / "verifier").is_dir(), f"{case_id} has no verifier")
            self.assertFalse((root / "fixture" / "verifier").exists(),
                             f"{case_id} put its verifier inside the agent's editable tree")

    def test_the_verifier_fails_while_the_task_is_unsolved(self) -> None:
        for case_id, command, _frozen, _solve in CASES:
            with self.subTest(case=case_id):
                root = self._case_root(case_id)
                result = self._run_verifier(case_id, command, root)
                self.assertNotEqual(
                    result.returncode, 0,
                    f"{case_id}: a verifier that passes on the unsolved case proves nothing")

    def test_the_verifier_passes_once_the_task_is_solved(self) -> None:
        for case_id, command, _frozen, solve in CASES:
            with self.subTest(case=case_id):
                root = self._case_root(case_id)
                with tempfile.TemporaryDirectory() as temp:
                    case = pathlib.Path(temp) / "case"
                    shutil.copytree(root, case)
                    solve(case)
                    result = self._run_verifier(case_id, command, case)
                    self.assertEqual(result.returncode, 0,
                                     f"{case_id}: the verifier rejected a solved case\n"
                                     f"{result.stdout[-2000:]}\n{result.stderr[-2000:]}")

    def test_the_verifier_refuses_an_edited_frozen_file(self) -> None:
        for case_id, command, frozen, solve in CASES:
            with self.subTest(case=case_id):
                root = self._case_root(case_id)
                with tempfile.TemporaryDirectory() as temp:
                    case = pathlib.Path(temp) / "case"
                    shutil.copytree(root, case)
                    # The case is solved first, so the refusal can only come
                    # from the digest guard and not from the unsolved task.
                    solve(case)
                    target = case / frozen
                    self.assertTrue(target.is_file(), f"{case_id}: no frozen file at {frozen}")
                    target.write_text(target.read_text(encoding="utf-8") + "\n# weakened\n",
                                      encoding="utf-8")
                    result = self._run_verifier(case_id, command, case)
                    self.assertNotEqual(
                        result.returncode, 0,
                        f"{case_id}: editing the frozen file {frozen} must not be accepted")

    def test_the_verifier_scope_is_outside_the_agent_editable_tree(self) -> None:
        manifest = REPO_ROOT / "dev" / "harness_lab" / "fixtures" / "agent_autonomy" / "cases.jsonl"
        for line in manifest.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            entry = json.loads(line)
            scope = entry["verifier_scope"]
            self.assertFalse(scope.startswith("dev/autonomy_fixtures/") and
                             scope.endswith("/fixture"),
                             f"{entry['case_id']}: the verifier lives inside the agent's tree")
            self.assertFalse(scope.startswith("dev/harness_lab/"), entry["case_id"])
            self.assertFalse(scope.startswith("plans/"), entry["case_id"])
            self.assertFalse(scope.startswith("tests/"), entry["case_id"])

    def test_the_background_job_waits_from_the_launch_not_the_first_query(self) -> None:
        """The waiting window starts when the build is launched.

        An agent that starts the build and reads the project *before* it
        queries the job has used the waiting time exactly as the prompt asks.
        A verifier that required the reads to land between two job queries
        would reject that correct behaviour — which it did, until the first
        live comparison caught it.
        """
        root = self._case_root("background-job-wait")
        with tempfile.TemporaryDirectory() as temp:
            case = pathlib.Path(temp) / "case"
            shutil.copytree(root, case)
            _solve_background_job_wait(case)
            # The reads happen after the launch and before the job is ever
            # queried: the agent read the project while the build ran.
            _write_transcript(case, "completed", [
                _call(0, "bash", "a" * 64),
                _call(1, "read", "b" * 64),
                _call(2, "read", "c" * 64),
                _call(3, "job", "d" * 64),
                _call(4, "job", "e" * 64),
            ])
            result = self._run_verifier("background-job-wait", UNITTEST_COMMAND, case)
            self.assertEqual(result.returncode, 0,
                             "the agent used the waiting time and was still rejected\n"
                             f"{result.stdout[-2000:]}\n{result.stderr[-2000:]}")

    def test_every_verifier_finds_the_fixture_in_the_lane_layout(self) -> None:
        """A verifier must find the case root in a live lane, not only beside itself.

        In a lane the verifier is copied to a private directory **outside** the
        agent's workspace, so the case root — the directory that holds the
        agent's editable ``fixture/`` — is somewhere else. A verifier that
        resolves the case root only from its own location therefore looks for
        a fixture that is not there, and every case fails with "the frozen
        file is missing" for a reason that has nothing to do with the agent.
        """
        for case_id, command, _frozen, _solve in CASES:
            with self.subTest(case=case_id):
                root = self._case_root(case_id)
                with tempfile.TemporaryDirectory() as temp:
                    lane = pathlib.Path(temp)
                    case = lane / "case"
                    private = lane / "private"
                    shutil.copytree(root / "fixture", case / "fixture")
                    shutil.copytree(root / "verifier", private / "verifier")
                    env = dict(os.environ)
                    env["CODEGG_AUTONOMY_CASE_ROOT"] = str(case)
                    env["CODEGG_AUTONOMY_TRANSCRIPT"] = str(lane / "TRANSCRIPT.json")
                    result = subprocess.run(command, cwd=str(private), capture_output=True,
                                            text=True, timeout=300, check=False, env=env)
                    output = (result.stdout or "") + (result.stderr or "")
                    self.assertNotIn("the frozen file is missing", output,
                                     f"{case_id}: the verifier did not find the case root "
                                     f"in the lane layout")
                    self.assertNotIn("No such file or directory", output,
                                     f"{case_id}: the verifier did not find the case root "
                                     f"in the lane layout")


if __name__ == "__main__":
    unittest.main()
