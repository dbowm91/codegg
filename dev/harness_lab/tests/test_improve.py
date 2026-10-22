import hashlib
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
import unittest.mock
from pathlib import Path

from dev.harness_lab import budget
from dev.harness_lab import improve
from dev.harness_lab import scoring
from dev.harness_lab.improve import (_build_with_compile_correction,
                                     authorize_binary_fingerprint, authorize_build_transition, Budget,
                                     candidate_patch_sha256, check_scope, changed_paths,
                                     Checkpoint, CheckpointInterrupted, create_candidate_worktree,
                                     decision_reasons, evaluation_is_complete, expected_trial_keys,
                                     guarded_propose, index_trials, manual_dry_run, OptimizationCheckpoint,
                                     parse_opencode_events, protected_control_keys, run_opencode,
                                     score_candidate_trials, SUPPORTED_OPENCODE_VERSION,
                                     USAGE_NOT_REPORTED, validate_proposer_model)

REPO_ROOT = Path(__file__).resolve().parents[3]
GROUP_ID = "native-tool-smoke"
SUITE_DIGEST = "suite-digest"


def _warm_executable(path: Path) -> None:
    """Pay the one-time first-exec cost of a freshly written executable.

    A hardened host scans a newly written executable the first time it runs,
    and that scan can outlast the five-second version preflight in
    ``run_opencode``. Paying the cost once, before the call under test,
    keeps the preflight's timeout about the preflight.
    """
    subprocess.run([str(path), "--version"], capture_output=True, timeout=120, check=False)

# A child that reaches a failpoint between the append-only event write and the
# current-state pointer replacement, then parks so the parent can signal it.
_CHECKPOINT_CRASH_CHILD = """
import os
import sys
import time
from pathlib import Path

sys.path.insert(0, sys.argv[1])
from dev.harness_lab import improve

checkpoint_path, marker_path = Path(sys.argv[2]), Path(sys.argv[-1])
checkpoint = improve.Checkpoint(checkpoint_path, "base-revision", "suite-digest")


def pausing_replace(source, destination):
    with open(marker_path, "w", encoding="utf-8") as handle:
        handle.write("failpoint\\n")
        handle.flush()
    while True:
        time.sleep(0.05)


os.replace = pausing_replace
checkpoint.transition("DIAGNOSED", {"failpoint": "between-append-and-pointer"})
"""

_OPTIMIZATION_CRASH_CHILD = """
import os
import sys
import time
from pathlib import Path

sys.path.insert(0, sys.argv[1])
from dev.harness_lab import improve

checkpoint_path, marker_path = Path(sys.argv[2]), Path(sys.argv[-1])
state = improve.OptimizationCheckpoint(checkpoint_path, {"base_revision": "abc", "suite_sha256": "suite"})


def pausing_replace(source, destination):
    with open(marker_path, "w", encoding="utf-8") as handle:
        handle.write("failpoint\\n")
        handle.flush()
    while True:
        time.sleep(0.05)


os.replace = pausing_replace
state.record("iteration_started", {"iteration": 1, "candidate": "one"})
"""

_LEDGER_CRASH_CHILD = """
import os
import sys
import time
from pathlib import Path

sys.path.insert(0, sys.argv[1])
from dev.harness_lab import ledger

ledger_root, workspace, marker_path = Path(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[-1])
real_open = Path.open


def pausing_open(self, mode="r", *args, **kwargs):
    if self.name == "index.jsonl" and "a" in mode:
        with open(marker_path, "w", encoding="utf-8") as handle:
            handle.write("failpoint\\n")
            handle.flush()
        while True:
            time.sleep(0.05)
    return real_open(self, mode, *args, **kwargs)


Path.open = pausing_open
ledger.record(ledger_root, {"daemon": {"revision": "abc"}, "wall_time_ms": 120},
              task_id="task-one", group_id="native-tool-smoke", candidate="candidate-one",
              workspace=workspace, expected_file="tool-receipt.txt", expected_text="ok",
              suite_sha256="suite-digest", case_sha256="case-digest")
"""


def _trial_row(candidate: str, task_id: str, repeat: int, terminal: str, duration_ms,
               group_id: str = GROUP_ID, suite_sha256: str = SUITE_DIGEST) -> dict:
    """Build a recorded ledger manifest row for a scoring fixture."""
    return {"run_id": f"{candidate}-{task_id}-{repeat}", "task_id": task_id, "repeat": repeat,
            "group_id": group_id, "candidate": candidate, "suite_sha256": suite_sha256,
            "case_sha256": "case-digest", "terminal": terminal,
            "metrics": {"wall_time_ms": duration_ms, "input_tokens": 10, "output_tokens": 5,
                        "cached_tokens": None, "reasoning_tokens": None, "cost_usd": None,
                        "tool_started": 1, "tool_finished": 1, "provider_calls": 1,
                        "repeated_tool_calls": 0, "recovery_followups": 0, "unavailable": []},
            "subject": {"revision": "revision"}}


SCORING_CASES = [
    {"schema_version": 1, "task_id": "task-pass", "group_id": GROUP_ID},
    {"schema_version": 1, "task_id": "task-defect", "group_id": GROUP_ID},
    {"schema_version": 1, "task_id": "task-flaky", "group_id": GROUP_ID},
    {"schema_version": 1, "task_id": "control-cancel", "group_id": GROUP_ID,
     "expected_terminal": "cancelled"},
    {"schema_version": 1, "task_id": "control-timeout", "group_id": GROUP_ID,
     "expected_terminal": "timeout"},
]


def _baseline_rows(candidate: str = "baseline") -> dict:
    """Baseline trials: one broken task, one flaky task, two intact controls."""
    return {
        ("task-pass", 0): _trial_row(candidate, "task-pass", 0, "passed", 100),
        ("task-defect", 0): _trial_row(candidate, "task-defect", 0, "assertion_failed", 100),
        ("task-flaky", 0): _trial_row(candidate, "task-flaky", 0, "execution_failed", 100),
        ("control-cancel", 0): _trial_row(candidate, "control-cancel", 0, "cancelled", 100),
        ("control-timeout", 0): _trial_row(candidate, "control-timeout", 0, "timeout", 100),
    }


class ImproveTests(unittest.TestCase):
    def test_budgets_and_first_terminal_transition(self) -> None:
        budget = Budget(max_iterations=1, max_candidate_retries=0, max_proposer_calls=1)
        budget.consume("iteration")
        with self.assertRaises(RuntimeError):
            budget.consume("iteration")
        with tempfile.TemporaryDirectory() as temp:
            checkpoint = Checkpoint(Path(temp) / "run.json", "abc", "suite")
            resumable = Checkpoint(Path(temp) / "resume.json", "abc", "suite")
            resumable.transition("DIAGNOSED", {})
            self.assertEqual(Checkpoint.resume(Path(temp) / "resume.json", "abc", "suite").state, "DIAGNOSED")
            for state in ("DIAGNOSED", "PROPOSING", "PATCHED", "BUILDING", "QUALIFYING", "EVALUATING", "COMPARING", "REJECTED"):
                checkpoint.transition(state, {})
            with self.assertRaises(RuntimeError):
                checkpoint.transition("ACCEPTED", {})
            with self.assertRaises(ValueError):
                Checkpoint.resume(Path(temp) / "run.json", "changed-base", "suite")
            (Path(temp) / "resume.json").write_text("{}", encoding="utf-8")
            with self.assertRaises(ValueError):
                Checkpoint.resume(Path(temp) / "resume.json", "abc", "suite")
            with self.assertRaises(FileExistsError):
                Checkpoint(Path(temp) / "resume.json", "abc", "suite")

            retry_checkpoint = Checkpoint(Path(temp) / "retry.json", "abc", "suite")
            retry_checkpoint.transition("DIAGNOSED", {})
            retry_checkpoint.transition("PROPOSING", {"attempt_kind": "initial"})
            retry_checkpoint.transition("PATCHED", {"patch_sha256": "first"})
            retry_checkpoint.transition("BUILDING", {})
            retry_checkpoint.transition("PROPOSING", {"attempt_kind": "compile_correction", "retry": 1})
            retry_checkpoint.transition("PATCHED", {"patch_sha256": "corrected"})
            retry_checkpoint.transition("BUILDING", {})
            resumed = Checkpoint.resume(Path(temp) / "retry.json", "abc", "suite")
            self.assertEqual(resumed.state, "BUILDING")
            self.assertEqual(resumed.attempt_id, 2)

    def test_opencode_json_event_parser_and_scope_gate(self) -> None:
        parsed = parse_opencode_events('{"type":"text","part":{"text":"proposal"}}\n{"type":"step_finish"}')
        self.assertEqual(parsed["text"], "proposal")
        with self.assertRaises(ValueError):
            parse_opencode_events("provider prose, not JSON")
        with self.assertRaises(ValueError):
            parse_opencode_events('{"type":"permission"}')
        with self.assertRaises(ValueError):
            check_scope(["dev/harness_lab/supervisor.py"], ("src/",))
        check_scope(["src/acp.rs"], ("src/",))

    def test_manual_dry_run_and_fake_opencode_process(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            dry = manual_dry_run(root / "dry.json")
            self.assertEqual(dry["status"], "REJECTED")
            fake = root / "opencode"
            fake.write_text("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 1.19.3; else echo '{\"type\":\"text\",\"part\":{\"text\":\"fixture\"}}'; fi\n", encoding="utf-8")
            fake.chmod(0o755)
            _warm_executable(fake)
            result = run_opencode(root, "inspect", executable=str(fake))
            self.assertEqual(result["version"], "1.19.3")
            self.assertEqual(result["event_count"], 1)
            slow = root / "opencode-slow"
            slow.write_text("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 1.19.3; else sleep 5; fi\n", encoding="utf-8")
            slow.chmod(0o755)
            _warm_executable(slow)
            with self.assertRaises(TimeoutError):
                run_opencode(root, "inspect", timeout=0.1, executable=str(slow))

    def test_an_unsupported_opencode_version_is_a_named_refusal(self) -> None:
        """An unsupported version is a distinct refusal, not a transport fault."""
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            stale = root / "opencode-stale"
            stale.write_text("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 1.18.9; else echo '{}'; fi\n", encoding="utf-8")
            stale.chmod(0o755)
            _warm_executable(stale)
            with self.assertRaises(RuntimeError) as caught:
                run_opencode(root, "inspect", executable=str(stale))
            message = str(caught.exception)
            self.assertIn("OpenCode version preflight failed", message)
            self.assertIn(SUPPORTED_OPENCODE_VERSION, message)
            self.assertIn("1.18.9", message)

    def test_patch_digest_includes_untracked_candidate_files(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "repo"
            root.mkdir()
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            subprocess.run(["git", "-C", str(root), "config", "user.email", "harness@example.invalid"], check=True)
            subprocess.run(["git", "-C", str(root), "config", "user.name", "Harness Test"], check=True)
            (root / "base.txt").write_text("base", encoding="utf-8")
            subprocess.run(["git", "-C", str(root), "add", "base.txt"], check=True)
            subprocess.run(["git", "-C", str(root), "commit", "-qm", "baseline"], check=True)
            (root / "CHANGELOG.harness.md").write_text("first", encoding="utf-8")
            first = candidate_patch_sha256(root, changed_paths(root))
            (root / "CHANGELOG.harness.md").write_text("second", encoding="utf-8")
            second = candidate_patch_sha256(root, changed_paths(root))
            self.assertNotEqual(first, second)

    def test_candidate_worktree_uses_exact_local_baseline(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "repo"
            root.mkdir()
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            subprocess.run(["git", "-C", str(root), "config", "user.email", "harness@example.invalid"], check=True)
            subprocess.run(["git", "-C", str(root), "config", "user.name", "Harness Test"], check=True)
            (root / "base.txt").write_text("base", encoding="utf-8")
            subprocess.run(["git", "-C", str(root), "add", "base.txt"], check=True)
            subprocess.run(["git", "-C", str(root), "commit", "-qm", "baseline"], check=True)
            revision = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"],
                                      check=True, capture_output=True, text=True).stdout.strip()
            destination = Path(temp) / "candidate"
            result = create_candidate_worktree(root, "candidate/test", destination, revision)
            actual = subprocess.run(["git", "-C", str(destination), "rev-parse", "HEAD"],
                                    check=True, capture_output=True, text=True).stdout.strip()
            self.assertEqual(actual, revision)
            self.assertEqual(result["branch"], "candidate/test")

    def test_optimization_checkpoint_is_hash_linked_and_resume_validates_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "optimize.json"
            inputs = {"base_revision": "abc", "suite_sha256": "suite", "max_iterations": 2}
            checkpoint = OptimizationCheckpoint(path, inputs)
            checkpoint.record("iteration_started", {"iteration": 1, "candidate": "one"})
            resumed = OptimizationCheckpoint.resume(path, inputs)
            self.assertEqual(resumed.run_id, checkpoint.run_id)
            resumed.record("iteration_finished", {"iteration": 1, "candidate": "one"})
            with self.assertRaises(ValueError):
                OptimizationCheckpoint.resume(path, {**inputs, "max_iterations": 3})
            rows = path.with_suffix(".jsonl").read_text(encoding="utf-8").splitlines()
            rows[-1] = rows[-1].replace('"iteration":1', '"iteration":2')
            path.with_suffix(".jsonl").write_text("\n".join(rows) + "\n", encoding="utf-8")
            with self.assertRaises(ValueError):
                OptimizationCheckpoint.resume(path, inputs)


class CrashRecoveryTests(unittest.TestCase):
    """Crash the real checkpoint and ledger writers with real signals."""

    def _spawn_failpoint_child(self, root: Path, script: str,
                               arguments: list[str]) -> subprocess.Popen:
        script_path = root / "failpoint_child.py"
        script_path.write_text(script, encoding="utf-8")
        marker = root / "failpoint.marker"
        process = subprocess.Popen([sys.executable, str(script_path), str(REPO_ROOT), *arguments, str(marker)],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        deadline = time.monotonic() + 30.0
        while time.monotonic() < deadline:
            if marker.exists():
                return process
            if process.poll() is not None:
                stdout, stderr = process.communicate()
                self.fail("failpoint child exited before the failpoint: "
                          + stderr.decode(errors="replace")[-2000:])
            time.sleep(0.02)
        self._kill_and_reap(process, signal.SIGKILL)
        self.fail("failpoint child never reached the failpoint")

    def _kill_and_reap(self, process: subprocess.Popen, number: int) -> str:
        os.kill(process.pid, number)
        try:
            stdout, stderr = process.communicate(timeout=30)
        except subprocess.TimeoutExpired:
            process.kill()
            stdout, stderr = process.communicate(timeout=30)
            self.fail("the failpoint child ignored the signal")
        return stderr.decode(errors="replace")

    @staticmethod
    def _events(path: Path) -> list[dict]:
        return [json.loads(line) for line in path.with_suffix(".jsonl").read_text(encoding="utf-8").splitlines()
                if line.strip()]

    def test_sigkill_between_checkpoint_append_and_pointer_is_recorded(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            checkpoint_path = root / "candidate.json"
            process = self._spawn_failpoint_child(root, _CHECKPOINT_CRASH_CHILD, [str(checkpoint_path)])
            self._kill_and_reap(process, signal.SIGKILL)
            # The append-only tail advanced past the pointer: the crash window
            # between the event write and the pointer replacement.
            events = self._events(checkpoint_path)
            self.assertEqual([event["seq"] for event in events], [1, 2])
            pointer = json.loads(checkpoint_path.read_text(encoding="utf-8"))
            self.assertNotEqual(pointer, events[-1])
            # The resume refuses explicitly instead of raising an opaque error.
            with self.assertRaises(CheckpointInterrupted) as raised:
                Checkpoint.resume(checkpoint_path, "base-revision", "suite-digest")
            self.assertIn("interrupted_unknown", str(raised.exception))
            self.assertIn("seq 2", str(raised.exception))
            # The interruption is a recorded state and the checkpoint loads again.
            recorded = self._events(checkpoint_path)
            self.assertEqual([event["seq"] for event in recorded], [1, 2, 3])
            self.assertEqual(recorded[-1]["state"], "INTERRUPTED_UNKNOWN")
            self.assertEqual(recorded[-1]["evidence"]["outcome"], "interrupted_unknown")
            self.assertEqual(json.loads(checkpoint_path.read_text(encoding="utf-8")), recorded[-1])
            # A repeated resume is idempotent: no duplicate event and no
            # duplicate sequence number is appended.
            with self.assertRaises(CheckpointInterrupted):
                Checkpoint.resume(checkpoint_path, "base-revision", "suite-digest")
            again = self._events(checkpoint_path)
            self.assertEqual([event["seq"] for event in again], [1, 2, 3])
            self.assertEqual(len({event["seq"] for event in again}), 3)

    def test_sigterm_between_optimization_append_and_pointer_is_recorded(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            checkpoint_path = root / "optimize.json"
            inputs = {"base_revision": "abc", "suite_sha256": "suite"}
            process = self._spawn_failpoint_child(root, _OPTIMIZATION_CRASH_CHILD, [str(checkpoint_path)])
            self._kill_and_reap(process, signal.SIGTERM)
            events = self._events(checkpoint_path)
            self.assertEqual([event["seq"] for event in events], [1, 2])
            pointer = json.loads(checkpoint_path.read_text(encoding="utf-8"))
            self.assertNotEqual(pointer, events[-1])
            with self.assertRaises(CheckpointInterrupted) as raised:
                OptimizationCheckpoint.resume(checkpoint_path, inputs)
            self.assertIn("interrupted_unknown", str(raised.exception))
            recorded = self._events(checkpoint_path)
            self.assertEqual([event["seq"] for event in recorded], [1, 2, 3])
            self.assertEqual(recorded[-1]["kind"], "run_interrupted")
            self.assertEqual(recorded[-1]["data"]["outcome"], "interrupted_unknown")
            self.assertEqual(json.loads(checkpoint_path.read_text(encoding="utf-8")), recorded[-1])
            # A repeated resume refuses again and appends no duplicate event.
            with self.assertRaises(CheckpointInterrupted):
                OptimizationCheckpoint.resume(checkpoint_path, inputs)
            self.assertEqual([event["seq"] for event in self._events(checkpoint_path)], [1, 2, 3])

    def test_sigkill_during_ledger_write_leaves_no_duplicate_trial_rows(self) -> None:
        from dev.harness_lab import ledger
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            ledger_root = root / "ledger"
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "tool-receipt.txt").write_text("ok", encoding="utf-8")
            process = self._spawn_failpoint_child(root, _LEDGER_CRASH_CHILD,
                                                  [str(ledger_root), str(workspace)])
            self._kill_and_reap(process, signal.SIGKILL)
            # The run directory was written but the trial was never committed
            # to the append-only ledger index.
            run_dirs = sorted((ledger_root / "runs").iterdir())
            self.assertEqual(len(run_dirs), 1)
            manifest = json.loads((run_dirs[0] / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(manifest["task_id"], "task-one")
            index_path = ledger_root / "index.jsonl"
            self.assertFalse(index_path.exists())
            # The ledger is still loadable and the partial work is never
            # reported as a completed trial.
            rows = ledger.load_runs(ledger_root)
            for row in rows:
                self.assertNotEqual(row["terminal"], "passed")
            for row in rows:
                if row["run_id"] == manifest["run_id"]:
                    self.assertEqual(row["terminal"], "interrupted_unknown")
            self.assertEqual(index_trials(rows, "candidate-one"), {})
            # A resumed attempt commits exactly one row for the interrupted pair.
            ledger.record(ledger_root, {"daemon": {"revision": "abc"}, "wall_time_ms": 120},
                          task_id="task-one", group_id=GROUP_ID, candidate="candidate-one",
                          workspace=workspace, expected_file="tool-receipt.txt", expected_text="ok",
                          suite_sha256=SUITE_DIGEST, case_sha256="case-digest")
            index_rows = [json.loads(line) for line in index_path.read_text(encoding="utf-8").splitlines()
                          if line.strip()]
            self.assertEqual(len(index_rows), 1)
            self.assertEqual((index_rows[0]["task_id"], index_rows[0]["repeat"]), ("task-one", 0))
            indexed = index_trials(ledger.load_runs(ledger_root), "candidate-one")
            self.assertEqual(list(indexed), [("task-one", 0)])

    def test_resumed_run_does_not_append_a_second_row_for_a_recorded_pair(self) -> None:
        from dev.harness_lab import ledger
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            ledger_root = root / "ledger"
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "tool-receipt.txt").write_text("ok", encoding="utf-8")
            cases = [{"schema_version": 1, "task_id": "task-one", "group_id": GROUP_ID, "repeats": 2},
                     {"schema_version": 1, "task_id": "task-two", "group_id": GROUP_ID}]
            receipt = {"daemon": {"revision": "abc"}, "wall_time_ms": 120}
            ledger.record(ledger_root, receipt, task_id="task-one", group_id=GROUP_ID,
                          candidate="candidate-one", workspace=workspace,
                          expected_file="tool-receipt.txt", expected_text="ok",
                          suite_sha256=SUITE_DIGEST, case_sha256=ledger.canonical_digest(cases[0]))
            recorded = ledger.validate_candidate_resume_rows(ledger_root, "candidate-one", cases,
                                                              SUITE_DIGEST, GROUP_ID)
            self.assertEqual(sorted(recorded), [("task-one", 0)])
            # The resume only owes the pairs that were never recorded.
            pending = sorted(expected_trial_keys(cases) - set(recorded))
            self.assertEqual(pending, [("task-one", 1), ("task-two", 0)])
            for task_id, repeat in pending:
                case = next(item for item in cases if item["task_id"] == task_id)
                ledger.record(ledger_root, receipt, task_id=task_id, group_id=GROUP_ID,
                              candidate="candidate-one", workspace=workspace,
                              expected_file="tool-receipt.txt", expected_text="ok",
                              suite_sha256=SUITE_DIGEST, case_sha256=ledger.canonical_digest(case),
                              repeat=repeat)
            index_rows = [json.loads(line) for line in
                          (ledger_root / "index.jsonl").read_text(encoding="utf-8").splitlines() if line.strip()]
            self.assertEqual(len(index_rows), 3)
            self.assertEqual(len({(row["task_id"], row["repeat"]) for row in index_rows}), 3)
            # A second completed row for an already recorded pair is refused
            # instead of being scored twice.
            ledger.record(ledger_root, receipt, task_id="task-one", group_id=GROUP_ID,
                          candidate="candidate-one", workspace=workspace,
                          expected_file="tool-receipt.txt", expected_text="ok",
                          suite_sha256=SUITE_DIGEST, case_sha256=ledger.canonical_digest(cases[0]))
            with self.assertRaises(ValueError):
                index_trials(ledger.load_runs(ledger_root), "candidate-one")


class ScoreContractTests(unittest.TestCase):
    """The score contract decides acceptance from the recorded trials."""

    def _scored(self, candidate_rows: dict, cases: list[dict] | None = None) -> dict:
        return score_candidate_trials(cases if cases is not None else SCORING_CASES,
                                      _baseline_rows(), candidate_rows)

    def test_protected_controls_are_derived_without_touching_the_suite_schema(self) -> None:
        controls = protected_control_keys(SCORING_CASES)
        self.assertEqual(controls, {("control-cancel", 0), ("control-timeout", 0)})
        self.assertNotIn(("task-pass", 0), controls)
        # The derivation is a pure read of the frozen suite cases.
        self.assertEqual(protected_control_keys(SCORING_CASES), controls)

    def test_correctness_fix_is_accepted_with_a_correctness_objective(self) -> None:
        candidate = _baseline_rows("candidate")
        candidate[("task-defect", 0)] = _trial_row("candidate", "task-defect", 0, "passed", 100)
        scored = self._scored(candidate)
        outcome = scored["outcome"]
        self.assertEqual(outcome.decision, scoring.DECISION_ACCEPTED)
        self.assertEqual(outcome.objective, scoring.OBJECTIVE_CORRECTNESS_FIX)
        self.assertEqual(outcome.protected_violations, ())
        self.assertIsNotNone(outcome.score)
        self.assertEqual(scored["evidence_pairs"], len(SCORING_CASES))

    def test_a_reliability_only_improvement_is_rejected_but_still_reported(self) -> None:
        # A candidate that removed one avoidable tool failure but left the
        # task failing has not remediated a defect: the acceptance rule is a
        # correctness improvement. The movement is still reported, because it
        # happened; it just cannot carry the acceptance.
        candidate = _baseline_rows("candidate")
        candidate[("task-flaky", 0)] = _trial_row("candidate", "task-flaky", 0, "timeout", 100)
        outcome = self._scored(candidate)["outcome"]
        self.assertEqual(outcome.decision, scoring.DECISION_REJECTED)
        self.assertEqual(outcome.objective, scoring.OBJECTIVE_NONE)
        primary, reasons = decision_reasons(outcome)
        self.assertEqual(primary, "no_substantive_criterion_improved")
        self.assertIn("reliability_criterion_improved", reasons)
        self.assertNotIn("correctness_criterion_improved", reasons)

    def test_a_predeclared_reliability_contract_can_accept_a_reliability_improvement(self) -> None:
        contract = scoring.ScoreContract(
            version=scoring.SCORE_CONTRACT_VERSION,
            criteria=(
                scoring.Criterion(id="correctness.pass_rate", kind="correctness",
                                  direction="higher_is_better"),
                scoring.Criterion(id="reliability.success_rate", kind="reliability",
                                  direction="higher_is_better", improvement_accepts=True),
            ))
        candidate = _baseline_rows("candidate")
        candidate[("task-flaky", 0)] = _trial_row("candidate", "task-flaky", 0, "timeout", 100)
        outcome = score_candidate_trials(SCORING_CASES, _baseline_rows(), candidate,
                                         contract=contract)["outcome"]
        self.assertEqual(outcome.decision, scoring.DECISION_ACCEPTED)
        self.assertEqual(outcome.objective, scoring.OBJECTIVE_RELIABILITY)

    def test_protected_control_regression_is_rejected_even_with_other_improvements(self) -> None:
        candidate = _baseline_rows("candidate")
        candidate[("task-defect", 0)] = _trial_row("candidate", "task-defect", 0, "passed", 100)
        candidate[("task-flaky", 0)] = _trial_row("candidate", "task-flaky", 0, "timeout", 100)
        candidate[("control-cancel", 0)] = _trial_row("candidate", "control-cancel", 0, "passed", 100)
        outcome = self._scored(candidate)["outcome"]
        self.assertEqual(outcome.decision, scoring.DECISION_REJECTED)
        self.assertTrue(outcome.protected_violations)
        self.assertEqual(outcome.objective, scoring.OBJECTIVE_NONE)
        primary, reasons = decision_reasons(outcome)
        self.assertEqual(primary, "protected_control_regressed")
        self.assertIn("protected_control_regressed", reasons)

    def test_unmeasured_required_criterion_yields_insufficient_evidence_without_a_score(self) -> None:
        # A suite without a negative control leaves the protected criterion
        # unmeasured, which can never be scored as zero.
        cases = [case for case in SCORING_CASES if case["task_id"] not in {"control-cancel",
                                                                          "control-timeout"}]
        candidate = _baseline_rows("candidate")
        candidate[("task-defect", 0)] = _trial_row("candidate", "task-defect", 0, "passed", 100)
        scored = self._scored(candidate, cases)
        outcome = scored["outcome"]
        self.assertEqual(outcome.decision, scoring.DECISION_INSUFFICIENT_EVIDENCE)
        self.assertIsNone(outcome.score)
        self.assertEqual(outcome.objective, scoring.OBJECTIVE_NONE)
        control = next(observation for observation in scored["candidate_observations"]
                       if observation.criterion_id == "control.no_regression")
        self.assertIsNone(control.value)
        self.assertFalse(control.measured)
        primary, reasons = decision_reasons(outcome)
        self.assertEqual(primary, "insufficient_evidence")
        self.assertIn("insufficient_evidence", reasons)

    def test_partially_evaluated_candidate_is_insufficient_evidence(self) -> None:
        candidate = _baseline_rows("candidate")
        del candidate[("task-flaky", 0)]
        candidate[("task-defect", 0)] = _trial_row("candidate", "task-defect", 0, "passed", 100)
        scored = self._scored(candidate)
        outcome = scored["outcome"]
        self.assertEqual(outcome.decision, scoring.DECISION_INSUFFICIENT_EVIDENCE)
        self.assertIsNone(outcome.score)
        self.assertEqual(scored["evidence_pairs"], len(SCORING_CASES) - 1)

    def test_fully_matched_candidate_is_not_accepted(self) -> None:
        outcome = self._scored(_baseline_rows("candidate"))["outcome"]
        self.assertEqual(outcome.decision, scoring.DECISION_REJECTED)
        self.assertEqual(outcome.objective, scoring.OBJECTIVE_NONE)
        primary, reasons = decision_reasons(outcome)
        self.assertEqual(primary, "no_criterion_improved")
        self.assertIn("no_criterion_improved", reasons)

    def test_a_negative_control_that_must_fail_is_not_a_reliability_failure(self) -> None:
        # The control's declared expectation IS an avoidable failure: it
        # failed exactly as the suite requires. It is excluded from the
        # reliability metric, so a candidate that stopped the control from
        # failing cannot buy a reliability improvement with it.
        cases = SCORING_CASES + [{"schema_version": 1, "task_id": "control-stall",
                                  "group_id": GROUP_ID, "expected_terminal": "execution_failed"}]
        baseline = dict(_baseline_rows())
        candidate = dict(_baseline_rows("candidate"))
        baseline[("control-stall", 0)] = _trial_row("baseline", "control-stall", 0,
                                                    "execution_failed", 100)
        candidate[("control-stall", 0)] = _trial_row("candidate", "control-stall", 0,
                                                     "execution_failed", 100)
        scored = score_candidate_trials(cases, baseline, candidate)
        reliability = next(observation for observation in scored["candidate_observations"]
                           if observation.criterion_id == "reliability.success_rate")
        # Five trials where an unexpected avoidable failure is possible; the
        # control that must fail is outside the metric. One of the five did
        # end in an avoidable failure.
        self.assertEqual(reliability.value, 4 / 5)
        # Making the negative control stop failing does not move the metric.
        broken = dict(candidate)
        broken[("control-stall", 0)] = _trial_row("candidate", "control-stall", 0, "passed", 100)
        scored = score_candidate_trials(cases, baseline, broken)
        reliability = next(observation for observation in scored["candidate_observations"]
                           if observation.criterion_id == "reliability.success_rate")
        self.assertEqual(reliability.value, 4 / 5)

    def test_a_candidate_that_only_changed_the_duration_is_not_accepted(self) -> None:
        # The defect this guards: a candidate that fixed nothing — no
        # correctness result moved, no reliability result moved, every
        # negative control held — was accepted because the advisory duration
        # criterion happened to improve by a margin. A marginal latency win
        # over a candidate that does the same work is not an improvement.
        candidate = _baseline_rows("candidate")
        for row in candidate.values():
            row["metrics"]["wall_time_ms"] = 50
        outcome = self._scored(candidate)["outcome"]
        self.assertEqual(outcome.decision, scoring.DECISION_REJECTED)
        self.assertEqual(outcome.objective, scoring.OBJECTIVE_NONE)
        primary, reasons = decision_reasons(outcome)
        self.assertEqual(primary, "no_substantive_criterion_improved")
        self.assertIn("efficiency_criterion_improved", reasons)
        self.assertEqual(outcome.protected_violations, ())

    def test_unmeasured_trial_never_maps_to_zero(self) -> None:
        durations = {("task-pass", 0): 400, ("task-defect", 0): 100, ("task-flaky", 0): 200,
                     ("control-cancel", 0): 300, ("control-timeout", 0): 500}
        baseline = {}
        candidate = {}
        for key, duration in durations.items():
            baseline[key] = _trial_row("baseline", key[0], key[1], "passed", duration)
            candidate[key] = _trial_row("candidate", key[0], key[1], "passed", duration)
        baseline[("task-defect", 0)]["terminal"] = "assertion_failed"
        candidate[("task-defect", 0)]["terminal"] = "passed"
        baseline[("task-flaky", 0)]["terminal"] = "execution_failed"
        candidate[("task-flaky", 0)]["terminal"] = "timeout"
        candidate[("task-pass", 0)]["metrics"]["wall_time_ms"] = None
        scored = score_candidate_trials(SCORING_CASES, baseline, candidate)
        duration = next(observation for observation in scored["candidate_observations"]
                        if observation.criterion_id == "efficiency.duration_ms")
        # The unmeasured trial is excluded from the median, never counted as 0:
        # the measured durations are [100, 200, 300, 500] with a median of 250,
        # while a zero-injected median would be 200.
        self.assertEqual(duration.value, 250.0)
        self.assertTrue(duration.measured)
        # A suite whose durations were never measured is unmeasured, not zero.
        for key, row in candidate.items():
            row["metrics"]["wall_time_ms"] = None
        scored = score_candidate_trials(SCORING_CASES, baseline, candidate)
        duration = next(observation for observation in scored["candidate_observations"]
                        if observation.criterion_id == "efficiency.duration_ms")
        self.assertIsNone(duration.value)
        self.assertFalse(duration.measured)
        self.assertEqual(scored["outcome"].decision, scoring.DECISION_INSUFFICIENT_EVIDENCE)
        self.assertIsNone(scored["outcome"].score)


class ResumeGateTests(unittest.TestCase):
    """The resume gates refuse a stale or repeated candidate."""

    def test_resume_with_a_stale_binary_fingerprint_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "candidate.json"
            checkpoint = Checkpoint(path, "base", "suite")
            checkpoint.transition("DIAGNOSED", {})
            checkpoint.transition("PROPOSING", {})
            checkpoint.transition("PATCHED", {"patch_sha256": "patch"})
            checkpoint.transition("BUILDING", {"patch_sha256": "patch"})
            checkpoint.transition("QUALIFYING", {"binary_sha256": "first-fingerprint"})
            checkpoint.transition("EVALUATING", {"binary_sha256": "first-fingerprint"})
            checkpoint.transition("BUILDING", {"patch_sha256": "patch"})
            checkpoint.transition("QUALIFYING", {"binary_sha256": "second-fingerprint"})
            # The latest recorded fingerprint is the only one a resume may use.
            with self.assertRaises(RuntimeError):
                authorize_binary_fingerprint(checkpoint, "first-fingerprint")
            authorize_binary_fingerprint(checkpoint, "second-fingerprint")

    def test_non_resume_call_at_patched_cannot_enter_building_twice(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "candidate.json"
            checkpoint = Checkpoint(path, "base", "suite")
            checkpoint.transition("DIAGNOSED", {})
            checkpoint.transition("PROPOSING", {})
            checkpoint.transition("PATCHED", {"patch_sha256": "patch"})
            authorize_build_transition(checkpoint, "patch", resume=False)
            checkpoint.transition("BUILDING", {"patch_sha256": "patch"})
            # The same patch is proposed again without a new build receipt.
            checkpoint.transition("PROPOSING", {"attempt_kind": "compile_correction"})
            checkpoint.transition("PATCHED", {"patch_sha256": "patch"})
            with self.assertRaises(ValueError):
                authorize_build_transition(checkpoint, "patch", resume=False)
            # The explicit resume of the same immutable candidate is allowed.
            authorize_build_transition(checkpoint, "patch", resume=True)

    def test_repeats_expansion_drives_the_completeness_check(self) -> None:
        cases = [{"schema_version": 1, "task_id": "a", "group_id": GROUP_ID, "repeats": 2},
                 {"schema_version": 1, "task_id": "b", "group_id": GROUP_ID}]
        self.assertEqual(expected_trial_keys(cases), {("a", 0), ("a", 1), ("b", 0)})
        trials = [{"task_id": "a", "repeat": 0}, {"task_id": "a", "repeat": 1},
                  {"task_id": "b", "repeat": 0}]
        # The raw case count is smaller than the repeat-expanded trial count,
        # so a completeness check against the raw count always fails.
        self.assertEqual(len(cases), 2)
        self.assertNotEqual(len(trials), len(cases))
        self.assertTrue(evaluation_is_complete(trials, cases))
        self.assertFalse(evaluation_is_complete(trials[:2], cases))
        self.assertFalse(evaluation_is_complete(trials + [{"task_id": "c", "repeat": 0}], cases))


if __name__ == "__main__":
    unittest.main()


#: A fake OpenCode CLI. It answers the version preflight with the pinned
#: version and records the argv of every invocation it receives, one argument
#: per line, so a test can assert the exact argv the proposer was given and
#: whether the CLI ran at all.
_FAKE_OPENCODE_TEMPLATE = r"""#!/bin/sh
for arg in "$@"; do printf '%s\n' "$arg" >> "__LOG__"; done
printf -- '---\n' >> "__LOG__"
if [ "$1" = "--version" ]; then echo 1.19.3; exit 0; fi
printf '%s\n' '{"type":"text","part":{"text":"proposal"}}'
__USAGE__
"""

#: The usage a real ``step_finish`` event carries: the CLI's own token counts
#: and its own reported cost.
_FAKE_OPENCODE_USAGE = r"""printf '%s\n' '{"type":"step_finish","part":{"reason":"stop","tokens":{"total":7792,"input":7769,"output":14,"reasoning":9,"cache":{"write":0,"read":0}},"cost":0.0007815}}'
"""


class ProposerRouteAndBudgetTests(unittest.TestCase):
    """The proposer route is pinnable and the proposer spend is bounded."""

    ROUTE = "opencode-go/beam-free"
    OTHER_ROUTE = "opencode-go/minimax-m3.1-flash-free"

    def _fake_opencode(self, root: Path, *, usage: bool = True) -> tuple[str, Path]:
        """Write a fake OpenCode CLI that records every invocation it receives."""
        log = root / "opencode.log"
        fake = root / "opencode"
        body = _FAKE_OPENCODE_TEMPLATE.replace("__USAGE__", _FAKE_OPENCODE_USAGE if usage else "")
        fake.write_text(body.replace("__LOG__", str(log)), encoding="utf-8")
        fake.chmod(0o755)
        _warm_executable(fake)
        log.write_text("", encoding="utf-8")
        return str(fake), log

    @staticmethod
    def _invocations(log: Path) -> list[list[str]]:
        """Return the argv of every recorded invocation, in order."""
        if not log.exists():
            return []
        invocations: list[list[str]] = []
        current: list[str] = []
        for line in log.read_text(encoding="utf-8").splitlines():
            if line == "---":
                invocations.append(current)
                current = []
            else:
                current.append(line)
        return invocations

    @classmethod
    def _proposals(cls, log: Path) -> list[list[str]]:
        """Return the argv of every recorded proposer run.

        The fake also records the version preflight; that is not a proposal.
        """
        return [argv for argv in cls._invocations(log) if argv[:1] == ["run"]]

    def test_an_unpinned_proposer_emits_no_model_flag(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            executable, log = self._fake_opencode(root)
            receipt = run_opencode(root, "inspect", executable=executable)
            proposals = self._proposals(log)
            self.assertEqual(len(proposals), 1)
            # An unpinned run keeps the pre-pinning argv byte for byte.
            self.assertEqual(proposals[0], ["run", "--auto", "--format", "json",
                                            "--dir", str(root), "inspect"])
            self.assertNotIn("--model", proposals[0])
            # The receipt records that no route was pinned.
            self.assertIsNone(receipt["model"])

    def test_a_pinned_proposer_names_the_route_between_dir_and_the_prompt(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            executable, log = self._fake_opencode(root)
            receipt = run_opencode(root, "inspect", executable=executable, model=self.ROUTE)
            proposals = self._proposals(log)
            self.assertEqual(len(proposals), 1)
            self.assertEqual(proposals[0], ["run", "--auto", "--format", "json", "--dir", str(root),
                                            "--model", self.ROUTE, "inspect"])
            # The receipt attributes the proposal to the route.
            self.assertEqual(receipt["model"], self.ROUTE)

    def test_a_malformed_model_id_is_refused_before_the_cli_runs(self) -> None:
        malformed = ("beam-free", "opencode-go/beam/free", "/beam-free", "opencode-go/",
                     "opencode go/beam-free", "opencode-go/beam free", "opencode-go/beam-free!",
                     "", "opencode-go/beam-free\n")
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            executable, log = self._fake_opencode(root)
            for candidate in malformed:
                with self.subTest(model=candidate):
                    with self.assertRaises(ValueError):
                        validate_proposer_model(candidate)
                    with self.assertRaises(ValueError):
                        run_opencode(root, "inspect", executable=executable, model=candidate)
            # A malformed id is refused, never normalized into a usable one.
            self.assertIsNone(validate_proposer_model(None))
            self.assertEqual(validate_proposer_model(self.ROUTE), self.ROUTE)
            # The CLI never ran for any of the refused ids.
            self.assertEqual(self._invocations(log), [])

    def test_a_step_finish_event_yields_the_reported_usage(self) -> None:
        parsed = parse_opencode_events(
            '{"type":"step_finish","part":{"reason":"stop","tokens":{"total":7792,"input":7769,'
            '"output":14,"reasoning":9,"cache":{"write":0,"read":0}},"cost":0.0007815}}\n')
        self.assertIsNone(parsed["usage_reason"])
        self.assertEqual(parsed["usage"]["tokens"],
                         {"total": 7792, "input": 7769, "output": 14, "reasoning": 9,
                          "cache_read": 0, "cache_write": 0})
        # The cost is the CLI's own reported figure.
        self.assertEqual(parsed["usage"]["cost_usd"], 0.0007815)
        self.assertEqual(parsed["usage"]["steps"], 1)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            executable, _log = self._fake_opencode(root)
            receipt = run_opencode(root, "inspect", executable=executable)
            self.assertIsNone(receipt["proposer_usage_reason"])
            self.assertEqual(receipt["proposer_usage"]["tokens"]["total"], 7792)
            self.assertEqual(receipt["proposer_usage"]["tokens"]["cache_read"], 0)
            self.assertEqual(receipt["proposer_usage"]["cost_usd"], 0.0007815)

    def test_a_stream_without_step_finish_reports_unknown_usage(self) -> None:
        parsed = parse_opencode_events('{"type":"text","part":{"text":"proposal"}}\n')
        # An absent measurement is unknown, never zero.
        self.assertIsNone(parsed["usage"])
        self.assertEqual(parsed["usage_reason"], USAGE_NOT_REPORTED)
        self.assertEqual(parsed["usage_reason"], "not_reported_by_the_cli")
        # The text is still parsed.
        self.assertEqual(parsed["text"], "proposal")
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            executable, _log = self._fake_opencode(root, usage=False)
            receipt = run_opencode(root, "inspect", executable=executable)
            self.assertIsNone(receipt["proposer_usage"])
            self.assertEqual(receipt["proposer_usage_reason"], "not_reported_by_the_cli")

    def _qualification_inputs(self, root: Path) -> dict:
        """Build the paths a qualification reads before it reaches a provider."""
        worktree = root / "worktree"
        worktree.mkdir()
        suite = root / "suite.jsonl"
        suite.write_text('{"schema_version": 1, "task_id": "task-one", "group_id": "native-tool-smoke"}\n',
                         encoding="utf-8")
        ledger_root = root / "ledger"
        ledger_root.mkdir()
        return {"worktree": worktree, "suite": suite, "ledger_root": ledger_root,
                "target_dir": root / "target"}

    def test_a_refused_budget_policy_prevents_any_opencode_invocation(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            executable, log = self._fake_opencode(root)
            # The fake shadows the real CLI on PATH, so an invocation would be
            # recorded even if the proposer looked the CLI up itself.
            shadow = root / "bin"
            shadow.mkdir()
            (shadow / "opencode").write_text(Path(executable).read_text(encoding="utf-8"),
                                             encoding="utf-8")
            (shadow / "opencode").chmod(0o755)
            paths = self._qualification_inputs(root)
            policy = budget.BudgetPolicy()
            self.assertNotEqual(budget.admit_budget(policy).status, budget.STATUS_ADMITTED)
            with unittest.mock.patch.dict(os.environ,
                                          {"PATH": f"{shadow}{os.pathsep}{os.environ['PATH']}"}):
                with self.assertRaises(RuntimeError) as raised:
                    improve.qualify_candidate(paths["worktree"], paths["suite"], paths["ledger_root"],
                                              baseline_candidate="baseline", candidate="candidate",
                                              target_dir=paths["target_dir"], allowed_prefixes=("src/",),
                                              budget_policy=policy)
            message = str(raised.exception)
            # The refusal names the admission detail and the refused caps.
            self.assertIn("budget admission refused", message)
            self.assertIn("token_limit", message)
            self.assertIn(budget.STATUS_UNVERIFIABLE, message)
            # The proposer never ran.
            self.assertEqual(self._invocations(log), [])

    def test_spend_guard_exhaustion_refuses_a_second_proposal(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            executable, log = self._fake_opencode(root)
            policy = budget.BudgetPolicy(max_provider_requests=1)
            self.assertEqual(budget.admit_budget(policy).status, budget.STATUS_ADMITTED)
            guard = budget.SpendGuard(policy)
            receipt = guarded_propose(guard, worktree=root, prompt="inspect", timeout=30.0,
                                      model=None, executable=executable)
            self.assertEqual(receipt["proposer_usage"]["tokens"]["total"], 7792)
            self.assertEqual(len(self._proposals(log)), 1)
            # The second proposal is refused before the CLI is invoked.
            with self.assertRaises(budget.BudgetExhausted):
                guarded_propose(guard, worktree=root, prompt="inspect", timeout=30.0,
                                model=None, executable=executable)
            self.assertEqual(len(self._proposals(log)), 1)
            # The observed spend is auditable.
            self.assertEqual(guard.as_dict()["provider_requests"], 1)
            self.assertEqual(guard.as_dict()["total_tokens"], 7792)
            self.assertEqual(guard.as_dict()["cost_usd"], 0.0007815)

    def test_no_budget_policy_preserves_todays_behaviour(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            executable, log = self._fake_opencode(root)
            receipt = guarded_propose(None, worktree=root, prompt="inspect", timeout=30.0,
                                      model=None, executable=executable)
            # No guard and no admission: the receipt carries no budget fields.
            self.assertNotIn("budget_guard", receipt)
            self.assertEqual(self._proposals(log)[0],
                             ["run", "--auto", "--format", "json", "--dir", str(root), "inspect"])
            # A qualification without a policy is refused on its own terms,
            # never by a budget admission it never asked for.
            paths = self._qualification_inputs(root)
            with self.assertRaises(ValueError) as raised:
                improve.qualify_candidate(paths["worktree"], paths["suite"], paths["ledger_root"],
                                          baseline_candidate="baseline", candidate="candidate",
                                          target_dir=paths["target_dir"], allowed_prefixes=("src/",))
            self.assertNotIn("budget admission refused", str(raised.exception))

    def _optimizer_fixture(self, root: Path, model: str | None) -> tuple[Path, Path, Path, Path, str]:
        """Build the ledger, suite and checkpoint an optimizer resume reads.

        The checkpoint's cumulative deadline is already expired, so a resumed
        run stops at its wallclock gate before it reaches any provider work.
        """
        from dev.harness_lab import ledger
        repository = root / "repo"
        repository.mkdir()
        suite = root / "suite.jsonl"
        suite.write_text('{"schema_version": 1, "task_id": "task-one", "group_id": "native-tool-smoke"}\n',
                         encoding="utf-8")
        ledger_root = root / "ledger"
        workspace = root / "workspace"
        workspace.mkdir()
        (workspace / "tool-receipt.txt").write_text("ok", encoding="utf-8")
        ledger.record(ledger_root, {"daemon": {"revision": "abc"}, "wall_time_ms": 120},
                      task_id="task-one", group_id=GROUP_ID, candidate="baseline",
                      workspace=workspace, expected_file="tool-receipt.txt", expected_text="ok",
                      suite_sha256=SUITE_DIGEST, case_sha256="case-digest")
        prompt = "assess the candidate"
        inputs = {"base_revision": "abc",
                  "suite_sha256": hashlib.sha256(suite.read_bytes()).hexdigest(),
                  "baseline_candidate": "baseline", "max_iterations": 3,
                  "max_candidate_retries": 0, "allowed_prefixes": ["src/"],
                  "prompt_sha256": hashlib.sha256(prompt.encode()).hexdigest(),
                  "proposer_model": model,
                  "deadline_unix": time.time() - 60.0}
        checkpoint_path = root / "optimize.json"
        improve.OptimizationCheckpoint(checkpoint_path, inputs)
        return repository, suite, ledger_root, checkpoint_path, prompt

    def test_a_checkpoint_binds_the_proposer_model(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "optimize.json"
            inputs = {"base_revision": "abc", "suite_sha256": "suite", "proposer_model": self.ROUTE}
            improve.OptimizationCheckpoint(path, inputs)
            # A resume that requests a different route is refused, and the
            # refusal names both the recorded and the requested model.
            with self.assertRaises(ValueError) as raised:
                improve.OptimizationCheckpoint.resume(path, {**inputs, "proposer_model": self.OTHER_ROUTE})
            message = str(raised.exception)
            self.assertIn("proposer model changed", message)
            self.assertIn(self.ROUTE, message)
            self.assertIn(self.OTHER_ROUTE, message)
            # The same route is accepted.
            resumed = improve.OptimizationCheckpoint.resume(path, inputs)
            self.assertEqual(resumed.inputs["proposer_model"], self.ROUTE)
            # An unpinned run binds no route, and a resume that names one is
            # still a change of route.
            unpinned = Path(temp) / "unpinned.json"
            improve.OptimizationCheckpoint(unpinned, {**inputs, "proposer_model": None})
            self.assertEqual(improve.OptimizationCheckpoint.resume(
                unpinned, {**inputs, "proposer_model": None}).inputs["proposer_model"], None)
            with self.assertRaises(ValueError):
                improve.OptimizationCheckpoint.resume(unpinned, {**inputs, "proposer_model": self.ROUTE})

    def test_the_optimizer_refuses_a_resume_that_switches_the_proposer_model(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            executable, log = self._fake_opencode(root)
            repository, suite, ledger_root, checkpoint_path, prompt = self._optimizer_fixture(root, self.ROUTE)
            arguments = {"baseline_candidate": "baseline", "candidate_root": root / "candidates",
                         "target_root": root / "target", "checkpoint_path": checkpoint_path,
                         "allowed_prefixes": ("src/",), "prompt": prompt, "resume": True,
                         "proposer_executable": executable}
            # A resume that requests a different route is refused before any
            # provider work.
            with self.assertRaises(ValueError) as raised:
                improve.run_optimizer(repository, suite, ledger_root, model=self.OTHER_ROUTE, **arguments)
            message = str(raised.exception)
            self.assertIn("proposer model changed", message)
            self.assertIn(self.ROUTE, message)
            self.assertIn(self.OTHER_ROUTE, message)
            self.assertEqual(self._invocations(log), [])
            # A resume that keeps the same route is accepted. The checkpoint's
            # deadline has already expired, so the run stops at its wallclock
            # gate without reaching the proposer.
            result = improve.run_optimizer(repository, suite, ledger_root, model=self.ROUTE, **arguments)
            self.assertEqual(result["status"], "INTERRUPTED")
            self.assertEqual(self._invocations(log), [])
            # Without a budget policy there is no guard and no admission, so
            # the receipt carries no budget fields.
            self.assertNotIn("budget_guard", result)


class CompileCorrectionMechanicsTests(unittest.TestCase):
    """C003 work package C: the bounded compile-correction mechanic.

    These tests qualify the MECHANIC against a labelled fault injection. They
    are not a real fix and they are not a live-provider claim: the plan
    requires the fault-injected branch to be labelled separately from a real
    compile correction.
    """

    def _worktree(self, root: Path) -> Path:
        worktree = root / "candidate"
        worktree.mkdir()
        subprocess.run(["git", "init", "-q", str(worktree)], check=True)
        subprocess.run(["git", "-C", str(worktree), "config", "user.email", "harness@example.invalid"], check=True)
        subprocess.run(["git", "-C", str(worktree), "config", "user.name", "Harness Test"], check=True)
        (worktree / "main.rs").write_text("fn main() {}\n", encoding="utf-8")
        (worktree / "CHANGELOG.harness.md").write_text(
            "## Root cause\n\n## Contracts\n\n## Validation\n\n## Accepted risks\n", encoding="utf-8")
        subprocess.run(["git", "-C", str(worktree), "add", "main.rs"], check=True)
        subprocess.run(["git", "-C", str(worktree), "commit", "-qm", "base"], check=True)
        # The correction is modelled as uncommitted candidate edits, which is
        # what `changed_paths` reports. The changelog is part of the candidate
        # change, so it must appear among the changed paths.
        (worktree / "main.rs").write_text("fn main() { println!(\"fixed\"); }\n", encoding="utf-8")
        return worktree

    def test_a_compile_failure_is_corrected_once_and_the_build_is_rerun(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            worktree = self._worktree(root)
            attempts = {"build": 0, "propose": 0}

            def build():
                attempts["build"] += 1
                if attempts["build"] == 1:
                    raise RuntimeError("error[E0425]: cannot find value `missing` in this scope")
                return "build-ok"

            def propose(prompt):
                attempts["propose"] += 1
                self.assertIn("compile-correction retry", prompt)
                return {"event_count": 1}

            gates: list = []
            digest, retries = _build_with_compile_correction(
                build=build, propose=propose, gates=gates, started=time.monotonic(),
                max_wallclock=60.0, timeout=30.0, proposer="opencode",
                max_candidate_retries=1, checkpoint=None, correction_prompt="",
                allowed_prefixes=("main.rs",), changelog="CHANGELOG.harness.md",
                worktree=worktree, patch_digest="digest-0")
            # The build was re-run after the correction, and the correction
            # happened exactly once.
            self.assertEqual(attempts["build"], 2)
            self.assertEqual(attempts["propose"], 1)
            self.assertEqual(retries, 1)
            self.assertEqual(gates, ["build-ok"])
            self.assertTrue(digest)

    def test_the_compile_correction_retry_is_bounded(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            worktree = self._worktree(root)
            attempts = {"build": 0, "propose": 0}

            def build():
                attempts["build"] += 1
                raise RuntimeError("error[E0425]: cannot find value `missing` in this scope")

            def propose(prompt):
                attempts["propose"] += 1
                return {"event_count": 1}

            with self.assertRaises(RuntimeError):
                _build_with_compile_correction(
                    build=build, propose=propose, gates=[], started=time.monotonic(),
                    max_wallclock=60.0, timeout=30.0, proposer="opencode",
                    max_candidate_retries=2, checkpoint=None, correction_prompt="",
                    allowed_prefixes=("main.rs",), changelog="CHANGELOG.harness.md",
                    worktree=worktree, patch_digest="digest-0")
            # One initial attempt plus exactly the two allowed retries.
            self.assertEqual(attempts["build"], 3)
            self.assertEqual(attempts["propose"], 2)

    def test_a_non_opencode_proposer_never_retries(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            worktree = self._worktree(root)
            attempts = {"build": 0, "propose": 0}

            def build():
                attempts["build"] += 1
                raise RuntimeError("compile failure")

            def propose(prompt):
                attempts["propose"] += 1
                return {}

            with self.assertRaises(RuntimeError):
                _build_with_compile_correction(
                    build=build, propose=propose, gates=[], started=time.monotonic(),
                    max_wallclock=60.0, timeout=30.0, proposer="manual",
                    max_candidate_retries=3, checkpoint=None, correction_prompt="",
                    allowed_prefixes=("main.rs",), changelog="CHANGELOG.harness.md",
                    worktree=worktree, patch_digest="digest-0")
            self.assertEqual(attempts["build"], 1)
            self.assertEqual(attempts["propose"], 0)

    def test_a_correction_that_drops_the_changelog_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            worktree = self._worktree(root)
            attempts = {"build": 0}

            def build():
                attempts["build"] += 1
                if attempts["build"] == 1:
                    raise RuntimeError("compile failure")
                return "build-ok"

            def propose(prompt):
                (worktree / "CHANGELOG.harness.md").unlink()
                return {}

            with self.assertRaises(ValueError):
                _build_with_compile_correction(
                    build=build, propose=propose, gates=[], started=time.monotonic(),
                    max_wallclock=60.0, timeout=30.0, proposer="opencode",
                    max_candidate_retries=1, checkpoint=None, correction_prompt="",
                    allowed_prefixes=("main.rs",), changelog="CHANGELOG.harness.md",
                    worktree=worktree, patch_digest="digest-0")
