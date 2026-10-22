"""Offline guards for the autonomy taxonomy runner.

The runner is the instrument that makes the taxonomy mean something: it runs
a case as a live agent task, keeps the verifier outside the agent's reach,
records what the agent actually did, and lets the shared score contract judge
the pair. These tests pin the parts that must hold **before** a provider is
ever called, and the parts that make the comparison honest.

They never call a provider. A live run is exercised by the C004 closure run,
not by the unit suite.
"""
from __future__ import annotations

import json
import pathlib
import tempfile
import unittest

from dev.harness_lab import autonomy, autonomy_run, budget, scoring
from dev.harness_lab.supervisor import HarnessError

REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
FIXTURE_ROOT = REPO_ROOT / "dev" / "autonomy_fixtures"


def _case(case_id: str) -> autonomy.AutonomyCase:
    suite = autonomy.load_autonomy_suite()
    return suite.case(case_id)


def _run(case_id: str, solved: bool, protected: bool = False,
         terminal: str = "completed", duration_ms: int = 1000) -> dict:
    return {"case_id": case_id, "solved": solved, "protected": protected,
            "terminal": terminal, "verifier_confirmed": solved,
            "duration_ms": duration_ms}


def _control_run(holds: bool) -> dict:
    """Return a protected control run whose verifier holds, or does not.

    A control holds when its own verifier confirms the trial. The control's
    task is impossible, so it is never "solved".
    """
    return {"case_id": "control", "solved": False, "protected": True,
            "terminal": "stalled" if holds else "completed",
            "verifier_confirmed": holds, "duration_ms": 1000}


class BuildTranscriptTests(unittest.TestCase):
    def test_the_transcript_records_what_the_agent_did_and_not_what_it_said(self) -> None:
        case = _case("repeated-distinct-tool-calls")
        transcript = autonomy_run.build_transcript(
            case,
            [{"index": 0, "tool": "read", "arguments_sha256": "a" * 64, "status": "completed"},
             {"index": 1, "tool": "read", "arguments_sha256": "a" * 64, "status": "failed"}],
            terminal="completed", stop_reason="end_turn")
        self.assertEqual(transcript["schema_version"], 1)
        self.assertEqual(transcript["case_id"], case.case_id)
        self.assertEqual(transcript["terminal"], "completed")
        self.assertEqual(len(transcript["tool_calls"]), 2)
        self.assertEqual(transcript["tool_calls"][1]["status"], "failed")

    def test_the_transcript_carries_the_protected_flag(self) -> None:
        case = _case("repeated-identical-failure-stall")
        transcript = autonomy_run.build_transcript(case, [], terminal="stalled",
                                                   stop_reason=None)
        self.assertTrue(transcript["protected"])
        self.assertEqual(transcript["terminal"], "stalled")


class RunCaseRefusalTests(unittest.TestCase):
    """A run that cannot be trusted must stop before a provider is called."""

    def test_a_deferred_case_is_refused(self) -> None:
        suite = autonomy.load_autonomy_suite()
        deferred = [case for case in suite.cases if case.support != "supported"]
        self.assertTrue(deferred, "the manifest declares no deferred case to refuse")
        with self.assertRaises(HarnessError):
            autonomy_run.run_case(pathlib.Path("/nonexistent"), deferred[0],
                                  "opencode_go/muse-spark-1.3-contributor",
                                  budget_policy=budget.BudgetPolicy(),
                                  lane_root=pathlib.Path("/nonexistent"))

    def test_a_model_without_a_provider_is_refused(self) -> None:
        with self.assertRaises(HarnessError):
            autonomy_run.run_case(pathlib.Path("/nonexistent"), _case("read-search-patch-rust"),
                                  "muse-spark-1.3-contributor",
                                  budget_policy=budget.BudgetPolicy(),
                                  lane_root=pathlib.Path("/nonexistent"))

    def test_an_unknown_profile_is_refused(self) -> None:
        with self.assertRaises(HarnessError):
            autonomy_run.run_case(pathlib.Path("/nonexistent"), _case("read-search-patch-rust"),
                                  "opencode_go/muse-spark-1.3-contributor",
                                  budget_policy=budget.BudgetPolicy(),
                                  lane_root=pathlib.Path("/nonexistent"),
                                  profile="anything-goes")

    def test_a_missing_case_body_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            with self.assertRaises(HarnessError):
                autonomy_run.run_case(pathlib.Path("/nonexistent"), _case("read-search-patch-rust"),
                                      "opencode_go/muse-spark-1.3-contributor",
                                      budget_policy=budget.BudgetPolicy(),
                                      lane_root=pathlib.Path(temp),
                                      repository=pathlib.Path(temp))

    def test_a_refused_budget_stops_the_case_before_the_prompt(self) -> None:
        # A policy with no cap cannot be admitted, so the case must be
        # refused rather than started against a provider.
        policy = budget.BudgetPolicy()
        with self.assertRaises(HarnessError):
            autonomy_run.run_case(pathlib.Path("/nonexistent"), _case("read-search-patch-rust"),
                                  "opencode_go/muse-spark-1.3-contributor",
                                  budget_policy=policy,
                                  lane_root=pathlib.Path("/nonexistent"))

    def test_a_lane_whose_socket_path_is_too_long_is_refused(self) -> None:
        # The daemon binds its control socket inside the lane. A lane whose
        # socket path does not fit the platform's sun_path limit cannot
        # start, and the daemon's own failure names the socket rather than
        # the cause. The runner refuses the lane up front.
        policy = budget.BudgetPolicy(token_limit=400000, max_provider_requests=40,
                                     max_wallclock_seconds=3600.0, cost_observable=False)
        with tempfile.TemporaryDirectory() as temp:
            deep = pathlib.Path(temp) / ("x" * 200)
            with self.assertRaises(HarnessError) as caught:
                autonomy_run.run_case(pathlib.Path("/nonexistent"), _case("read-search-patch-rust"),
                                      "opencode_go/muse-spark-1.3-contributor",
                                      budget_policy=policy,
                                      lane_root=deep)
            self.assertIn("sun_path", str(caught.exception))


class IsolationTests(unittest.TestCase):
    def test_a_verifier_inside_the_agent_editable_tree_is_refused(self) -> None:
        suite = autonomy.load_autonomy_suite()
        # The agent may edit the fixture. If the editable prefix ever grew to
        # cover the verifier, the independent check would stop being
        # independent, so the guard has to refuse it.
        with self.assertRaises(Exception):
            autonomy.verify_isolation(suite, ["dev/autonomy_fixtures/read-search-patch-rust"])

    def test_the_shipped_cases_keep_the_verifier_outside_the_editable_tree(self) -> None:
        suite = autonomy.load_autonomy_suite()
        for case in suite.cases:
            if case.support != "supported":
                continue
            autonomy.verify_isolation(suite,
                                      [f"dev/autonomy_fixtures/{case.case_id}/fixture"])


class ComparisonTests(unittest.TestCase):
    """The paired comparison is judged by the shared contract, not by hand."""

    def test_a_correctness_improvement_is_accepted(self) -> None:
        contract = scoring.default_contract()
        baseline = [_run("a", solved=False), _run("control", solved=False, protected=True)]
        candidate = [_run("a", solved=True), _run("control", solved=False, protected=True)]
        outcome = autonomy_run.compare_runs(contract, baseline, candidate)["outcome"]
        self.assertEqual(outcome["decision"], "accepted")
        self.assertEqual(outcome["objective"], "correctness_fix")

    def test_no_change_is_not_an_acceptance(self) -> None:
        contract = scoring.default_contract()
        runs = [_run("a", solved=False), _run("control", solved=False, protected=True)]
        outcome = autonomy_run.compare_runs(contract, runs, list(runs))["outcome"]
        self.assertNotEqual(outcome["decision"], "accepted")

    def test_a_protected_control_regression_rejects_the_candidate(self) -> None:
        contract = scoring.default_contract()
        baseline = [_run("a", solved=True), _control_run(True)]
        # The candidate's control no longer holds: the verifier that states
        # what "as expected" means stopped confirming it. That is a safety
        # regression, and it must not ride along with a correctness gain.
        candidate = [_run("a", solved=True), _control_run(False)]
        outcome = autonomy_run.compare_runs(contract, baseline, candidate)["outcome"]
        self.assertNotEqual(outcome["decision"], "accepted")
        self.assertTrue(outcome["protected_violations"])

    def test_a_control_that_never_ran_does_not_hold(self) -> None:
        # A protected control's task is impossible, so "not solved" is
        # vacuous as a measure of the control holding. The control holds when
        # its own verifier confirms the trial.
        contract = scoring.default_contract()
        baseline = [_run("a", solved=True), _control_run(False)]
        candidate = [_run("a", solved=True), _control_run(False)]
        outcome = autonomy_run.compare_runs(contract, baseline, candidate)["outcome"]
        self.assertNotEqual(outcome["decision"], "accepted")
        self.assertFalse(outcome["protected_violations"])

    def test_a_comparison_without_a_measured_control_cannot_be_decided(self) -> None:
        contract = scoring.default_contract()
        baseline = [_run("a", solved=False)]
        candidate = [_run("a", solved=True)]
        outcome = autonomy_run.compare_runs(contract, baseline, candidate)["outcome"]
        # The contract makes the protected control a required criterion. A
        # comparison that never ran it cannot be decided, and an undecided
        # comparison is not an acceptance.
        self.assertEqual(outcome["decision"], "insufficient_evidence")
        self.assertIn("required_criterion_unmeasured", outcome["reasons"])

    def test_a_missing_measurement_is_null_and_never_zero(self) -> None:
        contract = scoring.default_contract()
        # An unmeasured metric is recorded as null with measured=False. It is
        # never folded into the aggregate as a zero, because a zero is a
        # measurement and "we did not look" is not one.
        observation = autonomy_run._observation(contract, "efficiency.duration_ms", None)
        self.assertIsNone(observation.value)
        self.assertFalse(observation.measured)
        measured = autonomy_run._observation(contract, "efficiency.duration_ms", 0.0)
        self.assertEqual(measured.value, 0.0)
        self.assertTrue(measured.measured)

    def test_an_unmeasured_metric_cannot_be_decided_on(self) -> None:
        contract = scoring.default_contract()
        baseline = [_run("a", solved=False), _run("control", solved=False, protected=True)]
        candidate = [_run("a", solved=True), _run("control", solved=False, protected=True)]
        # Neither side measured a duration. The efficiency criterion is
        # required, so the comparison cannot be decided at all rather than
        # being decided on a metric nobody measured.
        for run in baseline + candidate:
            run.pop("duration_ms")
        outcome = autonomy_run.compare_runs(contract, baseline, candidate)["outcome"]
        self.assertEqual(outcome["decision"], "insufficient_evidence")
        self.assertIn("required_criterion_unmeasured", outcome["reasons"])
        self.assertIsNone(outcome["score"])

    def test_a_stalled_trial_is_never_a_success(self) -> None:
        contract = scoring.default_contract()
        baseline = [_run("a", solved=False, terminal="failed"),
                    _run("control", solved=False, protected=True)]
        candidate = [_run("a", solved=False, terminal="stalled"),
                     _run("control", solved=False, protected=True)]
        comparison = autonomy_run.compare_runs(contract, baseline, candidate)
        # A stalled trial is an avoidable failure on both sides, so the
        # reliability rate cannot move and nothing is accepted.
        self.assertNotEqual(comparison["outcome"]["decision"], "accepted")

    def test_reliability_measures_the_run_and_not_the_outcome(self) -> None:
        # A trial that completed but did not solve its task is still a
        # reliable trial: whether the task was solved is the correctness
        # metric's business. Counting one fact twice would make a
        # reliability move depend on the task.
        runs = [_run("a", solved=False, terminal="completed"),
                _run("control", solved=False, protected=True)]
        self.assertEqual(autonomy_run._success_rate(runs), 1.0)
        stalled = [_run("a", solved=False, terminal="stalled"),
                   _run("control", solved=False, protected=True)]
        self.assertEqual(autonomy_run._success_rate(stalled), 0.5)

    def test_a_trial_that_ended_in_an_avoidable_failure_is_not_reliable(self) -> None:
        runs = [_run("a", solved=False, terminal="completed"),
                _run("control", solved=False, protected=True)]
        runs[0]["terminal_failure"] = {"category": "provider_transport"}
        self.assertEqual(autonomy_run._success_rate(runs), 0.5)

    def test_the_paired_case_ids_are_the_intersection(self) -> None:
        contract = scoring.default_contract()
        baseline = [_run("a", solved=False), _run("b", solved=False)]
        candidate = [_run("a", solved=False), _run("c", solved=False)]
        comparison = autonomy_run.compare_runs(contract, baseline, candidate)
        self.assertEqual(comparison["paired_case_ids"], ["a"])


class RescoreTests(unittest.TestCase):
    """A contract correction re-decides a recorded pair; it never re-runs it."""

    def test_the_rescore_reads_the_recorded_trials(self) -> None:
        baseline = [_run("a", solved=False), _run("control", solved=False, protected=True)]
        candidate = [_run("a", solved=True), _run("control", solved=False, protected=True)]
        comparison = autonomy_run.rescore_recorded_runs(baseline, candidate)
        self.assertEqual(comparison["outcome"]["decision"], "accepted")
        self.assertEqual(comparison["outcome"]["objective"], "correctness_fix")
        self.assertTrue(comparison["rescored_from_recorded_runs"])
        self.assertEqual(comparison["contract_version"], scoring.SCORE_CONTRACT_VERSION)

    def test_the_rescore_does_not_mutate_the_recorded_trials(self) -> None:
        baseline = [_run("a", solved=False), _run("control", solved=False, protected=True)]
        candidate = [_run("a", solved=True), _run("control", solved=False, protected=True)]
        before = json.dumps([baseline, candidate], sort_keys=True)
        autonomy_run.rescore_recorded_runs(baseline, candidate)
        self.assertEqual(json.dumps([baseline, candidate], sort_keys=True), before)


class ManifestTests(unittest.TestCase):
    def test_every_supported_case_names_a_body_the_runner_can_find(self) -> None:
        suite = autonomy.load_autonomy_suite()
        for case in suite.cases:
            if case.support != "supported":
                continue
            root = autonomy_run.case_root(REPO_ROOT, case.case_id)
            self.assertTrue((root / "fixture").is_dir(), case.case_id)
            self.assertTrue((root / "verifier").is_dir(), case.case_id)

    def test_the_taxonomy_keeps_one_protected_control(self) -> None:
        suite = autonomy.load_autonomy_suite()
        protected = [case for case in suite.cases if case.protected]
        self.assertTrue(protected, "the taxonomy has no protected control left")
        for case in protected:
            self.assertEqual(case.support, "supported",
                             "a protected control that cannot run guards nothing")


if __name__ == "__main__":
    unittest.main()
