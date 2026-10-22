"""The measurement budget: bounded re-runs, and a cumulative wallclock.

Three separate facts are tested here, and they are separate on purpose:

* a measurement failure says nothing about the candidate, so the pair is
  re-run — but the number of re-runs is capped per pair across every resume,
  and a pair that exhausts the cap is **unmeasured and unresolved** rather
  than a pass, a zero, or a silently dropped trial;
* the wallclock a qualification may consume is **cumulative**: it is measured
  from the start of the first attempt, across every resume, and it is derived
  from recorded evidence — the checkpoint's own accumulated elapsed time and
  the ledger's measured trial durations — never from a value the caller
  asserts;
* a checkpoint that carries no elapsed-time accounting is a **refusal**, not
  a fresh budget.
"""
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from dev.harness_lab import autonomy_run, budget, improve, ledger, sustained


class CheckpointAccountingTests(unittest.TestCase):
    def test_a_checkpoint_records_its_cumulative_elapsed_time(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            checkpoint = improve.Checkpoint(Path(temp) / "candidate.json", "rev", "digest")
            self.assertGreaterEqual(checkpoint.elapsed_seconds, 0.0)
            checkpoint.transition("DIAGNOSED", {"note": "first"})
            events = [json.loads(line) for line in
                      (Path(temp) / "candidate.jsonl").read_text(encoding="utf-8").splitlines()]
            self.assertIn("elapsed_seconds", events[-1]["evidence"])
            self.assertGreaterEqual(events[-1]["evidence"]["elapsed_seconds"], 0.0)

    def test_a_resume_restores_the_cumulative_elapsed_time(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "candidate.json"
            checkpoint = improve.Checkpoint(path, "rev", "digest")
            checkpoint.transition("DIAGNOSED", {"note": "first"})
            checkpoint.transition("PROPOSING", {"note": "second"})
            spent = checkpoint.elapsed_seconds
            resumed = improve.Checkpoint.resume(path, "rev", "digest")
            # The record on disk is the source of truth, so the restored
            # figure is the recorded one, to the recorded precision.
            self.assertAlmostEqual(resumed.elapsed_seconds, spent, places=5)
            self.assertEqual(resumed.state, "PROPOSING")

    def test_a_resume_does_not_hand_out_a_fresh_budget(self) -> None:
        """The restored clock keeps counting from where the last attempt stopped."""
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "candidate.json"
            checkpoint = improve.Checkpoint(path, "rev", "digest")
            checkpoint.transition("DIAGNOSED", {"note": "first"})
            checkpoint.elapsed_seconds = 120.0
            checkpoint.transition("PROPOSING", {"note": "second"})
            resumed = improve.Checkpoint.resume(path, "rev", "digest")
            # The restored budget continues from where the last attempt
            # stopped. It is not reset to its full declared length.
            self.assertGreaterEqual(resumed.elapsed_seconds, 120.0)
            self.assertLess(resumed.elapsed_seconds, 121.0)

    def test_a_checkpoint_without_elapsed_accounting_refuses_to_resume(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "candidate.json"
            improve.Checkpoint(path, "rev", "digest")
            events_path = Path(temp) / "candidate.jsonl"
            lines = events_path.read_text(encoding="utf-8").splitlines()
            stripped = []
            for line in lines:
                event = json.loads(line)
                event["evidence"].pop("elapsed_seconds", None)
                stripped.append(event)
            events_path.write_text(
                "".join(json.dumps(event, sort_keys=True) + "\n" for event in stripped),
                encoding="utf-8")
            # The pointer is rewritten to match the tail, so the refusal the
            # test is about is the accounting one and not the tail check.
            pointer = Path(temp) / "candidate.json"
            pointer.write_text(json.dumps(stripped[-1], sort_keys=True, indent=2) + "\n",
                               encoding="utf-8")
            with self.assertRaises(ValueError) as caught:
                improve.Checkpoint.resume(path, "rev", "digest")
            self.assertIn("elapsed-time accounting", str(caught.exception))

    def test_the_restored_figure_adds_the_ledger_trials_the_checkpoint_does_not_cover(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            checkpoint = improve.Checkpoint(root / "candidate.json", "rev", "digest")
            checkpoint.trials_covered = 0
            ledger_root = root / "ledger"
            _write_ledger_row(ledger_root, task_id="task-a", repeat=0,
                              terminal="passed", wall_time_ms=1500)
            _write_ledger_row(ledger_root, task_id="task-b", repeat=0,
                              terminal="passed", wall_time_ms=2500)
            consumed = improve.restored_elapsed_seconds(checkpoint, ledger_root, "candidate")
            self.assertAlmostEqual(consumed, 4.0, places=3)

    def test_a_trial_whose_duration_was_not_measured_adds_nothing(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            checkpoint = improve.Checkpoint(root / "candidate.json", "rev", "digest")
            ledger_root = root / "ledger"
            _write_ledger_row(ledger_root, task_id="task-a", repeat=0,
                              terminal="passed", wall_time_ms=None)
            consumed = improve.restored_elapsed_seconds(checkpoint, ledger_root, "candidate")
            # The trial contributed nothing; what is left is the checkpoint's
            # own elapsed time and no more.
            self.assertAlmostEqual(consumed, checkpoint.elapsed_seconds, places=6)


class MeasurementFailureTests(unittest.TestCase):
    def test_only_the_unmeasured_rows_count_as_measurement_failures(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            ledger_root = Path(temp) / "ledger"
            _write_ledger_row(ledger_root, task_id="task-a", repeat=0,
                              terminal="infrastructure_error", wall_time_ms=100)
            _write_ledger_row(ledger_root, task_id="task-a", repeat=0,
                              terminal="interrupted_unknown", wall_time_ms=100)
            _write_ledger_row(ledger_root, task_id="task-a", repeat=0,
                              terminal="passed", wall_time_ms=100)
            failures = ledger.measurement_failures_by_pair(ledger_root, "candidate")
            self.assertEqual(failures[("task-a", 0)], 2)

    def test_the_attempt_count_includes_the_measured_rows(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            ledger_root = Path(temp) / "ledger"
            _write_ledger_row(ledger_root, task_id="task-a", repeat=0,
                              terminal="infrastructure_error", wall_time_ms=100)
            _write_ledger_row(ledger_root, task_id="task-a", repeat=0,
                              terminal="passed", wall_time_ms=100)
            attempts = ledger.attempts_by_pair(ledger_root, "candidate")
            self.assertEqual(attempts[("task-a", 0)], 2)

    def test_the_resumed_elapsed_seconds_sum_the_measured_durations(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            ledger_root = Path(temp) / "ledger"
            _write_ledger_row(ledger_root, task_id="task-a", repeat=0,
                              terminal="passed", wall_time_ms=1200)
            _write_ledger_row(ledger_root, task_id="task-b", repeat=0,
                              terminal="passed", wall_time_ms=800)
            elapsed = ledger.resumed_candidate_elapsed_seconds(ledger_root, "candidate")
            self.assertAlmostEqual(elapsed, 2.0, places=3)

    def test_another_candidates_trials_are_not_counted(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            ledger_root = Path(temp) / "ledger"
            _write_ledger_row(ledger_root, task_id="task-a", repeat=0,
                              terminal="passed", wall_time_ms=5000, candidate="other")
            elapsed = ledger.resumed_candidate_elapsed_seconds(ledger_root, "candidate")
            self.assertAlmostEqual(elapsed, 0.0, places=6)


class UnmeasuredTrialTests(unittest.TestCase):
    def test_an_infrastructure_error_is_unmeasured_not_a_candidate_failure(self) -> None:
        runs = [{"case_id": "a", "terminal": "completed", "duration_ms": 10},
                {"case_id": "b", "terminal": "infrastructure_error", "duration_ms": 10}]
        self.assertEqual(autonomy_run._success_rate(runs), 1.0)

    def test_a_run_with_nothing_measured_leaves_the_metric_unmeasured(self) -> None:
        runs = [{"case_id": "b", "terminal": "infrastructure_error", "duration_ms": 10}]
        self.assertIsNone(autonomy_run._success_rate(runs))


class SustainedCumulativeCapTests(unittest.TestCase):
    def test_the_scenario_stops_before_it_crosses_the_cumulative_budget(self) -> None:
        calls: list[float] = []

        def fake_run_live_task(binary, task, model, **kwargs):
            calls.append(kwargs.get("timeout", 0.0))
            return {"trial_id": task, "task_id": task, "fault": None,
                    "terminal": "passed", "ok": True}

        original = sustained.pilot.run_live_task
        sustained.pilot.run_live_task = fake_run_live_task
        try:
            plans = [sustained.TrialPlan(trial_id=f"t{index}", task="worklog",
                                          fault=None) for index in range(4)]
            with tempfile.TemporaryDirectory() as temp:
                receipt = sustained.run_sustained_scenario(
                    Path("/nonexistent-binary"), "model",
                    budget_policy=budget.BudgetPolicy(max_provider_requests=10),
                    plans=plans, profile="workspace-write", root=Path(temp),
                    per_trial_wallclock=60.0, scenario_wallclock=0.0,
                    max_provider_requests=None)
        finally:
            sustained.pilot.run_live_task = original

        self.assertEqual(receipt["attempted_trials"], 0)
        self.assertEqual(receipt["unattempted_trials"], 4)
        self.assertIn("cumulative wallclock", receipt["stopped_reason"])
        self.assertEqual(receipt["scenario_wallclock_seconds"], 0.0)
        self.assertEqual(calls, [])

    def test_the_per_trial_timeout_is_capped_by_the_remaining_cumulative_budget(self) -> None:
        calls: list[float] = []

        def fake_run_live_task(binary, task, model, **kwargs):
            calls.append(kwargs.get("timeout", 0.0))
            return {"trial_id": task, "task_id": task, "fault": None,
                    "terminal": "passed", "ok": True}

        original = sustained.pilot.run_live_task
        sustained.pilot.run_live_task = fake_run_live_task
        try:
            plans = [sustained.TrialPlan(trial_id="t0", task="worklog", fault=None)]
            with tempfile.TemporaryDirectory() as temp:
                sustained.run_sustained_scenario(
                    Path("/nonexistent-binary"), "model",
                    budget_policy=budget.BudgetPolicy(max_provider_requests=10),
                    plans=plans, profile="workspace-write", root=Path(temp),
                    per_trial_wallclock=600.0, scenario_wallclock=5.0,
                    max_provider_requests=None)
        finally:
            sustained.pilot.run_live_task = original

        self.assertEqual(len(calls), 1)
        self.assertLessEqual(calls[0], 5.0)


def _write_ledger_row(ledger_root: Path, *, task_id: str, repeat: int, terminal: str,
                      wall_time_ms, candidate: str = "candidate") -> None:
    """Write one committed ledger row the harness can read back.

    The row is written the way the harness writes one: an immutable index
    row, a run manifest whose checksum the index carries, and the run's own
    event log ending in its terminal.
    """
    import hashlib

    ledger_root.mkdir(parents=True, exist_ok=True)
    index = ledger_root / "index.jsonl"
    run_id = f"{task_id}-{repeat}-{terminal}"
    run_dir = ledger_root / "runs" / run_id
    run_dir.mkdir(parents=True, exist_ok=True)
    manifest = {"schema_version": 1, "run_id": run_id, "candidate": candidate,
                "task_id": task_id, "repeat": repeat, "terminal": terminal,
                "metrics": {"wall_time_ms": wall_time_ms}, "trace_count": 0,
                "group_id": "group", "suite_sha256": "digest", "case_sha256": "case"}
    manifest_path = run_dir / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, sort_keys=True, indent=2) + "\n",
                             encoding="utf-8")
    (run_dir / "events.jsonl").write_text(
        json.dumps({"schema_version": 1, "run_id": run_id, "seq": 1, "kind": "terminal",
                    "terminal": terminal}, sort_keys=True) + "\n",
        encoding="utf-8")
    row = dict(manifest)
    row["manifest_sha256"] = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
    with index.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(row, sort_keys=True) + "\n")


if __name__ == "__main__":
    unittest.main()
