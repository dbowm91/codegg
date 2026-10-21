"""Tests for the sustained scenario instrument.

The live sustained run is not exercised here: it needs a real route and a real
budget. What is exercised here is everything the instrument decides *before* a
provider request, plus the controlled interruption behaviour against the local
deterministic fixture provider, which is where the resource-cleanup claim is
actually proven.
"""
import tempfile
import unittest
from pathlib import Path

from dev.harness_lab import budget, sustained
from dev.harness_lab.supervisor import HarnessError, run_tool_trial

REPO_ROOT = Path(__file__).resolve().parents[3]


def _policy(**overrides):
    mapping = {"token_limit": 100000, "max_provider_requests": 4,
               "max_wallclock_seconds": 600}
    mapping.update(overrides)
    return budget.budget_policy_from_mapping(mapping)


class AdmissionTest(unittest.TestCase):
    """A scenario never starts without an enforceable cap."""

    def test_a_refused_budget_refuses_the_scenario(self):
        policy = budget.budget_policy_from_mapping({"cost_limit_usd": 1.0,
                                                     "cost_enforcement": "controller_enforced",
                                                     "cost_observable": False})
        with self.assertRaises(HarnessError):
            sustained.run_sustained_scenario(
                Path("/nonexistent"), "opencode_go/muse-spark-1.3-contributor",
                budget_policy=policy,
                plans=[sustained.TrialPlan("t1", "ranges")])

    def test_an_empty_plan_is_refused(self):
        with self.assertRaises(HarnessError):
            sustained.run_sustained_scenario(
                Path("/nonexistent"), "opencode_go/muse-spark-1.3-contributor",
                budget_policy=_policy(), plans=[])

    def test_an_unknown_fault_is_refused(self):
        with self.assertRaises(HarnessError):
            sustained.run_sustained_scenario(
                Path("/nonexistent"), "opencode_go/muse-spark-1.3-contributor",
                budget_policy=_policy(),
                plans=[sustained.TrialPlan("t1", "ranges", fault="explode")])

    def test_an_unknown_task_is_refused(self):
        with self.assertRaises(HarnessError):
            sustained.run_sustained_scenario(
                Path("/nonexistent"), "opencode_go/muse-spark-1.3-contributor",
                budget_policy=_policy(),
                plans=[sustained.TrialPlan("t1", "not-a-task")])


class CleanupEvidenceTest(unittest.TestCase):
    """Cleanup is observed, not asserted."""

    def test_a_lane_without_a_daemon_reports_clean(self):
        with tempfile.TemporaryDirectory() as temp:
            lane = Path(temp) / "lane"
            (lane / "daemon").mkdir(parents=True)
            evidence = sustained.lane_cleanup_evidence(lane)
            self.assertTrue(evidence["clean"])
            self.assertFalse(evidence["daemon_socket_present"])
            self.assertEqual(evidence["surviving_processes"], [])

    def test_a_leftover_daemon_socket_is_reported(self):
        with tempfile.TemporaryDirectory() as temp:
            lane = Path(temp) / "lane"
            (lane / "daemon").mkdir(parents=True)
            (lane / "daemon" / "core.sock").write_text("", encoding="utf-8")
            evidence = sustained.lane_cleanup_evidence(lane)
            self.assertFalse(evidence["clean"])
            self.assertTrue(evidence["daemon_socket_present"])


class InterruptionTest(unittest.TestCase):
    """A stopped lane must not stop the other lane.

    The fixture provider is the deterministic half of the claim: the
    cancellation path is exercised for real against the native ACP adapter and
    the local fixture provider, and a second, independent lane must still
    complete afterwards.
    """

    def test_a_cancelled_lane_does_not_stop_the_next_lane(self):
        binary = REPO_ROOT / "target" / "debug" / "codegg"
        if not binary.exists():
            self.skipTest("the debug binary is not built")
        # The cancelled lane is reported as a cancellation, never as a pass.
        with self.assertRaises(HarnessError) as raised:
            run_tool_trial(binary, "workspace-write", timeout=60.0,
                           cancel_task=True, task_id="sustained-cancel")
        self.assertIn("cancelled", str(raised.exception))
        following = run_tool_trial(binary, "workspace-write", timeout=60.0,
                                   task_id="sustained-after-cancel")
        self.assertTrue(following.get("ok"))

    def test_a_transport_fault_lane_does_not_stop_the_next_lane(self):
        binary = REPO_ROOT / "target" / "debug" / "codegg"
        if not binary.exists():
            self.skipTest("the debug binary is not built")
        # The faulted lane is a failure, and the controller reports a failure
        # rather than a false pass.
        with self.assertRaises(HarnessError):
            run_tool_trial(binary, "workspace-write", timeout=60.0,
                           fail_after_tools=True, fault_kind="transport",
                           task_id="sustained-transport-fault")
        following = run_tool_trial(binary, "workspace-write", timeout=60.0,
                                   task_id="sustained-after-fault")
        self.assertTrue(following.get("ok"))


if __name__ == "__main__":
    unittest.main()
