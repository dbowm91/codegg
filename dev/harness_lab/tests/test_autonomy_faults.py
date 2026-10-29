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

    def test_a_single_failed_attempt_is_an_observed_failure(self) -> None:
        """One failure is an observed failure even when the agent never repeats.

        The receipt must not report zero failures for a fault the tool
        actually met: the repeat is a separate measurement.
        """
        spec = faults.FaultSpec.parse(_spec())
        observation = faults.observe_fault(self._calls("failed"), spec)
        self.assertEqual(observation["observed_failures"], 1)
        self.assertEqual(observation["longest_consecutive_run"], 1)
        self.assertFalse(observation["exercised"])

    def test_a_success_between_two_failures_is_not_a_repeat(self) -> None:
        spec = faults.FaultSpec.parse(_spec())
        observation = faults.observe_fault(self._calls("failed", "completed", "failed"), spec)
        self.assertEqual(observation["observed_failures"], 2)
        self.assertEqual(observation["longest_consecutive_run"], 1)
        self.assertFalse(observation["exercised"])

    def test_the_bound_is_reached_on_the_total_failures(self) -> None:
        """The watcher's bound is the total failures of the faulted call.

        The watcher must bound a faulted trial even when the agent
        interleaves other calls, so the bound is counted on the total.
        """
        spec = faults.FaultSpec.parse(_spec())
        calls = [{"index": 0, "tool": "read", "arguments_sha256": "a" * 64, "status": "failed"},
                 {"index": 1, "tool": "bash", "arguments_sha256": "b" * 64, "status": "completed"},
                 {"index": 2, "tool": "read", "arguments_sha256": "a" * 64, "status": "failed"},
                 {"index": 3, "tool": "bash", "arguments_sha256": "c" * 64, "status": "completed"},
                 {"index": 4, "tool": "read", "arguments_sha256": "a" * 64, "status": "failed"}]
        observation = faults.observe_fault(calls, spec)
        self.assertEqual(observation["observed_failures"], 3)
        self.assertTrue(observation["bound_reached"])
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


class ToolErrorEnvelopeTests(unittest.TestCase):
    """The product reports a failed tool as a *successful* call.

    Error-like output is not typed failure evidence. These tests make sure
    only the ACP terminal status decides the current result.
    """

    def test_the_product_error_envelope_is_recognised(self) -> None:
        self.assertTrue(faults.looks_like_tool_error(
            'Inline { output: "Tool error: tool execution failed: Permission denied (os error 13)" }'))

    def test_a_successful_output_is_not_a_failure(self) -> None:
        self.assertFalse(faults.looks_like_tool_error('Inline { output: "the report\\n" }'))

    def test_the_envelope_must_start_the_reported_output(self) -> None:
        # A tool that succeeded and whose own content mentions the envelope
        # later is not a failure.
        self.assertFalse(faults.looks_like_tool_error(
            'Inline { output: "the report mentions Tool error: sometimes" }'))

    def test_error_looking_success_output_is_not_a_failure(self) -> None:
        projector = autonomy_run._ToolCallProjector()
        projector.absorb({"method": "session/update", "params": {"update": {
            "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "read",
            "codeggArgumentsSha256": "abc"}}})
        projector.absorb({"method": "session/update", "params": {"update": {
            "sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed",
            "rawOutput": 'Inline { output: "Tool error: tool execution failed: '
                         'Permission denied (os error 13)" }'}}})
        calls = projector.calls()
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0]["status"], "completed")
        self.assertFalse(calls[0]["failed"])

    def test_typed_failed_status_is_a_failure(self) -> None:
        projector = autonomy_run._ToolCallProjector()
        projector.absorb({"method": "session/update", "params": {"update": {
            "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "read",
            "codeggArgumentsSha256": "abc"}}})
        projector.absorb({"method": "session/update", "params": {"update": {
            "sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "failed",
            "rawOutput": 'Inline { output: "ordinary content" }'}}})
        self.assertTrue(projector.calls()[0]["failed"])

    def test_a_projected_success_is_not_a_failure(self) -> None:
        projector = autonomy_run._ToolCallProjector()
        projector.absorb({"method": "session/update", "params": {"update": {
            "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "read",
            "codeggArgumentsSha256": "abc"}}})
        projector.absorb({"method": "session/update", "params": {"update": {
            "sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed",
            "rawOutput": 'Inline { output: "the report" }'}}})
        self.assertFalse(projector.calls()[0]["failed"])

    def test_the_receipt_never_carries_the_reported_output(self) -> None:
        projector = autonomy_run._ToolCallProjector()
        projector.absorb({"method": "session/update", "params": {"update": {
            "sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed",
            "rawOutput": 'Inline { output: "secret file content" }'}}})
        self.assertNotIn("secret file content", repr(projector.calls()))

    def test_job_projection_keeps_only_allowlisted_action_id_status_and_timing(self) -> None:
        projector = autonomy_run._ToolCallProjector()
        projector.absorb({"method": "session/update", "_received_elapsed_ms": 120,
                          "params": {"update": {
                              "sessionUpdate": "tool_call", "toolCallId": "j1", "title": "job",
                              "codeggArgumentsSha256": "a" * 64, "codeggAction": "wait"}}})
        projector.absorb({"method": "session/update", "_received_elapsed_ms": 180,
                          "params": {"update": {
                              "sessionUpdate": "tool_call_update", "toolCallId": "j1",
                              "status": "completed", "rawOutput":
                              'Inline { output: "job running\\njob_id: '
                              '12345678-1234-1234-1234-123456789abc\\nprivate output" }'}}})
        projected = projector.calls()[0]
        self.assertEqual(projected["started_elapsed_ms"], 120)
        self.assertEqual(projected["finished_elapsed_ms"], 180)
        self.assertEqual(projected["job"], {
            "job_id": "12345678-1234-1234-1234-123456789abc",
            "action": "wait", "status": "running"})
        self.assertNotIn("private output", repr(projected))

    def test_missing_job_identity_and_status_remain_explicit_unknowns(self) -> None:
        projector = autonomy_run._ToolCallProjector()
        projector.absorb({"method": "session/update", "params": {"update": {
            "sessionUpdate": "tool_call", "toolCallId": "j-missing", "title": "job",
            "codeggArgumentsSha256": "b" * 64, "codeggAction": "status"}}})
        projector.absorb({"method": "session/update", "params": {"update": {
            "sessionUpdate": "tool_call_update", "toolCallId": "j-missing",
            "status": "completed", "rawOutput": "Inline { output: request unavailable }"}}})
        projected = projector.calls()[0]
        self.assertEqual(projected["job"], {"job_id": None, "action": "status", "status": None})
        self.assertNotIn("request unavailable", repr(projected))

        projector.absorb({"method": "session/update", "params": {"update": {
            "sessionUpdate": "tool_call", "toolCallId": "j-no-output", "title": "job",
            "codeggArgumentsSha256": "c" * 64, "codeggAction": "wait"}}})
        projector.absorb({"method": "session/update", "params": {"update": {
            "sessionUpdate": "tool_call_update", "toolCallId": "j-no-output",
            "status": "completed"}}})
        self.assertEqual(projector.calls()[1]["job"], {
            "job_id": None, "action": "wait", "status": None})

    def test_task_placement_receipt_keeps_enum_and_refusal_status_only(self) -> None:
        projector = autonomy_run._ToolCallProjector()
        projector.absorb({"method": "session/update", "params": {"update": {
            "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "task",
            "codeggArgumentsSha256": "abc",
            "codeggTaskPlacement": {"action": "spawn", "requested": "isolated_new",
                                    "worktree_id_present": False}}}})
        projector.absorb({"method": "session/update", "params": {"update": {
            "sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "failed",
            "rawOutput": "private path and task text are not copied into receipts"}}})
        call = projector.calls()[0]
        self.assertEqual(call["task_placement"]["requested"], "isolated_new")
        self.assertEqual(call["status"], "failed")
        self.assertNotIn("rawOutput", call)
        self.assertNotIn("private path", repr(call))


class ObserveFaultFailureFlagTests(unittest.TestCase):
    def test_a_call_the_envelope_marked_is_counted_as_a_failure(self) -> None:
        spec = faults.FaultSpec.parse(_spec())
        calls = [{"tool": "read", "arguments_sha256": "a", "status": "completed", "failed": True},
                 {"tool": "read", "arguments_sha256": "a", "status": "completed", "failed": True}]
        observed = faults.observe_fault(calls, spec)
        self.assertEqual(observed["observed_failures"], 2)
        self.assertTrue(observed["exercised"])

    def test_a_call_without_the_flag_falls_back_to_the_status(self) -> None:
        spec = faults.FaultSpec.parse(_spec())
        calls = [{"tool": "read", "arguments_sha256": "a", "status": "failed"},
                 {"tool": "read", "arguments_sha256": "a", "status": "failed"}]
        self.assertEqual(faults.observe_fault(calls, spec)["observed_failures"], 2)


if __name__ == "__main__":
    unittest.main()
