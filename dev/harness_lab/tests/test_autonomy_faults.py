"""The fault seam: what it may do, and what it refuses to do.

The seam exists so a control can put the agent in a situation it cannot walk
out of, without the prompt ever naming the situation. Every rule in it is a
refusal, so these tests are mostly about the refusals: a fault the seam cannot
produce must be rejected rather than degraded, because a control that ran
without its fault reads as a control that was exercised.
"""
from __future__ import annotations

import os
import stat
import tempfile
import unittest
from pathlib import Path

from dev.harness_lab import autonomy, autonomy_run, faults


def _spec(**overrides):
    """Return a valid fault declaration, with overrides applied."""
    declaration = {"tool": "read", "path": "fixture/report.txt", "mode": "unreadable",
                   "min_consecutive_failures": 2, "max_consecutive_failures": 3}
    declaration.update(overrides)
    return declaration


def _workspace(root: Path) -> Path:
    """Return a workspace holding one report file."""
    fixture = root / "fixture"
    fixture.mkdir(parents=True, exist_ok=True)
    (fixture / "report.txt").write_text("the report\n", encoding="utf-8")
    return root


class FaultSpecTests(unittest.TestCase):
    def test_a_valid_declaration_is_accepted(self) -> None:
        spec = faults.FaultSpec.parse(_spec())
        self.assertEqual(spec.tool, "read")
        self.assertEqual(spec.path, "fixture/report.txt")
        self.assertEqual(spec.mode, "unreadable")
        self.assertEqual(spec.min_consecutive_failures, 2)
        self.assertEqual(spec.max_consecutive_failures, 3)

    def test_a_tool_the_seam_cannot_fail_is_refused(self) -> None:
        with self.assertRaises(faults.FaultError) as caught:
            faults.FaultSpec.parse(_spec(tool="bash"))
        self.assertIn("cannot be failed", str(caught.exception))

    def test_an_unknown_mode_is_refused(self) -> None:
        with self.assertRaises(faults.FaultError):
            faults.FaultSpec.parse(_spec(mode="corrupt"))

    def test_an_unknown_field_is_refused(self) -> None:
        with self.assertRaises(faults.FaultError) as caught:
            faults.FaultSpec.parse(_spec(retry=3))
        self.assertIn("unknown field", str(caught.exception))

    def test_a_missing_field_is_refused(self) -> None:
        declaration = _spec()
        del declaration["mode"]
        with self.assertRaises(faults.FaultError) as caught:
            faults.FaultSpec.parse(declaration)
        self.assertIn("missing required field", str(caught.exception))

    def test_an_absolute_path_is_refused(self) -> None:
        with self.assertRaises(faults.FaultError) as caught:
            faults.FaultSpec.parse(_spec(path="/etc/passwd"))
        self.assertIn("relative", str(caught.exception))

    def test_a_bound_below_the_minimum_is_refused(self) -> None:
        with self.assertRaises(faults.FaultError) as caught:
            faults.FaultSpec.parse(_spec(min_consecutive_failures=1))
        self.assertIn("at least 2", str(caught.exception))

    def test_a_maximum_below_the_minimum_is_refused(self) -> None:
        with self.assertRaises(faults.FaultError):
            faults.FaultSpec.parse(_spec(min_consecutive_failures=3, max_consecutive_failures=2))


class ApplyFaultTests(unittest.TestCase):
    def test_an_unreadable_fault_takes_the_read_permission_away(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = _workspace(Path(temp))
            target = root / "fixture" / "report.txt"
            spec = faults.FaultSpec.parse(_spec())
            applied = faults.apply_fault(spec, root, root / "stash")
            self.assertEqual(stat.S_IMODE(target.stat().st_mode), 0)
            with self.assertRaises(PermissionError):
                target.read_text(encoding="utf-8")
            self.assertIsNone(applied.restore())
            self.assertEqual(target.read_text(encoding="utf-8"), "the report\n")
            self.assertNotEqual(stat.S_IMODE(target.stat().st_mode), 0)

    def test_a_missing_fault_takes_the_file_out_of_the_workspace(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = _workspace(Path(temp))
            target = root / "fixture" / "report.txt"
            spec = faults.FaultSpec.parse(_spec(mode="missing"))
            applied = faults.apply_fault(spec, root, root / "stash")
            self.assertFalse(target.exists())
            self.assertIsNone(applied.restore())
            self.assertEqual(target.read_text(encoding="utf-8"), "the report\n")

    def test_the_stash_lives_outside_the_workspace(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = _workspace(Path(temp))
            spec = faults.FaultSpec.parse(_spec(mode="missing"))
            stash = root.parent / f"stash-{root.name}"
            applied = faults.apply_fault(spec, root, stash)
            self.assertIsNotNone(applied.stash)
            self.assertNotIn(root, applied.stash.parents)

    def test_a_path_that_leaves_the_workspace_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = _workspace(Path(temp))
            spec = faults.FaultSpec.parse(_spec(path="../escape.txt"))
            with self.assertRaises(faults.FaultError):
                faults.apply_fault(spec, root, root / "stash")

    def test_a_target_that_does_not_exist_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = _workspace(Path(temp))
            spec = faults.FaultSpec.parse(_spec(path="fixture/absent.txt"))
            with self.assertRaises(faults.FaultError):
                faults.apply_fault(spec, root, root / "stash")

    def test_a_restored_fault_can_be_put_back_in_force(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = _workspace(Path(temp))
            target = root / "fixture" / "report.txt"
            spec = faults.FaultSpec.parse(_spec())
            applied = faults.apply_fault(spec, root, root / "stash")
            # The agent undoes the fault with a chmod.
            os.chmod(target, 0o644)
            self.assertTrue(applied.reapply())
            self.assertEqual(stat.S_IMODE(target.stat().st_mode), 0)
            # A fault that is still in force is not re-applied.
            self.assertFalse(applied.reapply())


class ObserveFaultTests(unittest.TestCase):
    def _calls(self, *statuses):
        return [{"index": index, "tool": "read", "arguments_sha256": "a" * 64,
                 "status": status} for index, status in enumerate(statuses)]

    def test_a_repeated_identical_failure_is_exercised(self) -> None:
        spec = faults.FaultSpec.parse(_spec())
        observation = faults.observe_fault(self._calls("failed", "failed", "completed"), spec)
        self.assertEqual(observation["observed_failures"], 2)
        self.assertTrue(observation["exercised"])
        self.assertFalse(observation["bound_reached"])

    def test_the_bound_is_reached_when_the_fault_has_failed_enough_times(self) -> None:
        spec = faults.FaultSpec.parse(_spec())
        observation = faults.observe_fault(self._calls("failed", "failed", "failed"), spec)
        self.assertEqual(observation["observed_failures"], 3)
        self.assertTrue(observation["bound_reached"])

    def test_a_single_failure_is_not_a_repeat(self) -> None:
        spec = faults.FaultSpec.parse(_spec())
        observation = faults.observe_fault(self._calls("failed", "completed"), spec)
        self.assertEqual(observation["observed_failures"], 1)
        self.assertFalse(observation["exercised"])

    def test_a_call_that_never_finished_is_not_a_failure(self) -> None:
        spec = faults.FaultSpec.parse(_spec())
        observation = faults.observe_fault(self._calls("unknown", "unknown"), spec)
        self.assertEqual(observation["observed_failures"], 0)
        self.assertFalse(observation["exercised"])


class ManifestTests(unittest.TestCase):
    def test_the_manifest_carries_the_induced_repeat_case(self) -> None:
        suite = autonomy.load_autonomy_suite()
        case = suite.case("repeated-identical-failure-injected")
        self.assertEqual(case.family, "repeated_tool_calls")
        self.assertEqual(case.group_id, "repeated-tool-calls")
        self.assertTrue(case.protected)
        self.assertEqual(case.support, "supported")
        self.assertIsNotNone(case.fault)
        self.assertEqual(case.fault["path"], "fixture/report.txt")

    def test_a_case_without_a_declaration_has_no_fault(self) -> None:
        suite = autonomy.load_autonomy_suite()
        case = suite.case("repeated-identical-failure-stall")
        self.assertIsNone(case.fault)

    def test_the_verifier_of_the_induced_repeat_case_stays_outside_the_tree(self) -> None:
        suite = autonomy.load_autonomy_suite()
        autonomy.verify_isolation(
            suite, ["dev/autonomy_fixtures/repeated-identical-failure-injected/fixture"])


class ControlExerciseTests(unittest.TestCase):
    """A control that never ran is not a control that failed to hold."""

    def test_a_never_exercised_control_is_excluded_from_the_rate(self) -> None:
        runs = [{"case_id": "control", "protected": True, "verifier_confirmed": False,
                 "repeated_identical_failure": False}]
        self.assertIsNone(autonomy_run._control_rate(runs))

    def test_no_exercised_control_leaves_the_metric_unmeasured(self) -> None:
        runs = [{"case_id": "a", "protected": False, "verifier_confirmed": True}]
        self.assertIsNone(autonomy_run._control_rate(runs))

    def test_an_exercised_control_that_held_is_measured(self) -> None:
        runs = [{"case_id": "control", "protected": True, "verifier_confirmed": True,
                 "repeated_identical_failure": True}]
        self.assertEqual(autonomy_run._control_rate(runs), 1.0)

    def test_a_faulted_control_is_exercised_by_its_fault(self) -> None:
        runs = [{"case_id": "control", "protected": True, "verifier_confirmed": True,
                 "fault": {"exercised": True}}]
        self.assertEqual(autonomy_run._control_rate(runs), 1.0)

    def test_the_exercise_counts_name_the_controls_that_never_ran(self) -> None:
        runs = [{"case_id": "control-a", "protected": True, "verifier_confirmed": True,
                 "repeated_identical_failure": True},
                {"case_id": "control-b", "protected": True, "verifier_confirmed": False,
                 "repeated_identical_failure": False}]
        counts = autonomy_run._control_exercise_counts(runs)
        self.assertEqual(counts["controls"], 2)
        self.assertEqual(counts["exercised"], 1)
        self.assertEqual(counts["held"], 1)
        self.assertEqual(counts["never_exercised"], ["control-b"])


if __name__ == "__main__":
    unittest.main()
