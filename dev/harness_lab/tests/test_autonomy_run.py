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
import os
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

from dev.harness_lab import autonomy, autonomy_run, budget, scoring
from dev.harness_lab.supervisor import HarnessError

REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
FIXTURE_ROOT = REPO_ROOT / "dev" / "autonomy_fixtures"


def _case(case_id: str) -> autonomy.AutonomyCase:
    suite = autonomy.load_autonomy_suite()
    return suite.case(case_id)


def _run(case_id: str, solved: bool, protected: bool = False,
         terminal: str = "completed", duration_ms: int = 1000) -> dict:
    # A protected control run stands for a control the harness *did* put the
    # trial in the situation of, so it is marked as exercised.
    return {"case_id": case_id, "solved": solved, "protected": protected,
            "terminal": terminal, "verifier_confirmed": solved,
            "duration_ms": duration_ms, "repeated_identical_failure": protected}


def _control_run(holds: bool, exercised: bool = True) -> dict:
    """Return a protected control run whose verifier holds, or does not.

    A control holds when its own verifier confirms the trial. The control's
    task is impossible, so it is never "solved".

    ``exercised`` says whether the harness actually put the trial in the
    situation the control is about. A control that was never exercised is not
    counted as a control that failed to hold.
    """
    return {"case_id": "control", "solved": False, "protected": True,
            "terminal": "stalled" if holds else "completed",
            "verifier_confirmed": holds, "duration_ms": 1000,
            "repeated_identical_failure": exercised}


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


class H006VerifierTranscriptCompatibilityTests(unittest.TestCase):
    def test_production_shaped_job_events_pass_the_unchanged_overlap_verifier(self) -> None:
        suite = autonomy.load_autonomy_suite(
            REPO_ROOT / "dev" / "harness_lab" / "fixtures" / "agent_autonomy_h005")
        case = suite.case("sustained-event-ledger")
        projector = autonomy_run._ToolCallProjector()

        def call(call_id: str, title: str, action: str | None, start: int,
                 finish: int, output: str) -> None:
            projector.absorb({"method": "session/update", "_received_elapsed_ms": start,
                              "params": {"update": {
                                  "sessionUpdate": "tool_call", "toolCallId": call_id,
                                  "title": title, "codeggArgumentsSha256": "a" * 64,
                                  "codeggAction": action,
                              }}})
            projector.absorb({"method": "session/update", "_received_elapsed_ms": finish,
                              "params": {"update": {
                                  "sessionUpdate": "tool_call_update", "toolCallId": call_id,
                                  "status": "completed", "rawOutput": output,
                              }}})

        call("read-1", "read", None, 10, 20, "Inline { output: README }\n")
        call("submit-1", "test", "submit", 30, 40,
             "Inline { output: job queued\njob_id: "
             "74386df4-54d6-41eb-b2fc-0f52ade38983 }")
        call("edit-1", "edit", None, 50, 60, "Inline { output: updated parser }")
        call("edit-2", "apply_patch", None, 70, 80, "Inline { output: updated cli }")
        call("status-1", "job", "status", 90, 100,
             "Inline { output: job running\njob_id: "
             "74386df4-54d6-41eb-b2fc-0f52ade38983 }")
        call("terminal-1", "job", "wait", 110, 120,
             "Inline { output: job completed\njob_id: "
             "74386df4-54d6-41eb-b2fc-0f52ade38983 }")

        tool_calls = projector.calls()
        self.assertNotIn("job", tool_calls[0])
        self.assertEqual(tool_calls[1]["job"]["action"], "submit")
        self.assertEqual(tool_calls[4]["job"]["status"], "running")
        self.assertEqual(tool_calls[5]["job"]["status"], "completed")
        transcript = autonomy_run.build_transcript(
            case, tool_calls, terminal="completed", stop_reason="end_turn")

        with tempfile.TemporaryDirectory() as temp:
            transcript_path = pathlib.Path(temp) / "TRANSCRIPT.json"
            transcript_path.write_text(json.dumps(transcript), encoding="utf-8")
            verifier_root = autonomy_run.case_root(REPO_ROOT, case.case_id) / "verifier"
            environment = dict(os.environ)
            environment[autonomy_run.CASE_ROOT_ENV] = str(
                autonomy_run.case_root(REPO_ROOT, case.case_id))
            environment[autonomy_run.TRANSCRIPT_ENV] = str(transcript_path)
            checked = subprocess.run(
                ["python3", "-m", "unittest", "-v",
                 "test_event_ledger_contract.EventLedgerContract."
                 "test_transcript_proves_real_job_overlap_and_continuation"],
                cwd=verifier_root, env=environment, capture_output=True, text=True,
                timeout=30, check=False)

        self.assertEqual(checked.returncode, 0, checked.stdout + checked.stderr)
        self.assertIn("Ran 1 test", checked.stderr)
        self.assertIn("OK", checked.stderr)

    def test_h006_minimum_duration_is_a_hard_qualification_condition(self) -> None:
        minimum = autonomy_run.H006_MIN_ACTIVE_DURATION_MS
        max_gap = autonomy_run.H006_MAX_ACTIVITY_GAP_MS
        self.assertFalse(autonomy_run._duration_qualified(
            "sustained-event-ledger", minimum - 1,
            [{"finished_elapsed_ms": 0}, {"finished_elapsed_ms": minimum}]))
        self.assertFalse(autonomy_run._duration_qualified(
            "sustained-event-ledger", minimum,
            [{"finished_elapsed_ms": 0}, {"finished_elapsed_ms": minimum - 1}]))
        self.assertFalse(autonomy_run._duration_qualified(
            "sustained-event-ledger", minimum,
            [{"finished_elapsed_ms": minimum}]))
        self.assertFalse(autonomy_run._duration_qualified(
            "sustained-event-ledger", minimum,
            [{"finished_elapsed_ms": 0}, {"finished_elapsed_ms": minimum}]))
        self.assertFalse(autonomy_run._duration_qualified(
            "sustained-event-ledger", minimum,
            [{"started_elapsed_ms": 0, "finished_elapsed_ms": 1},
             {"started_elapsed_ms": max_gap + 2,
              "finished_elapsed_ms": max_gap + 3},
             {"started_elapsed_ms": minimum, "finished_elapsed_ms": minimum + 1}]))
        self.assertTrue(autonomy_run._duration_qualified(
            "sustained-event-ledger", minimum,
            [{"started_elapsed_ms": elapsed_ms, "finished_elapsed_ms": elapsed_ms + 1}
             for elapsed_ms in range(0, minimum + 1, max_gap)]))
        self.assertTrue(autonomy_run._duration_qualified("ordinary-case", 1, []))


class H006DeadlineTests(unittest.TestCase):
    def test_turn_can_run_for_thirty_minutes_and_verifier_stays_inside_hard_stop(self) -> None:
        turn = autonomy_run._turn_timeout(
            "sustained-event-ledger", 3540.0, 3600.0, 3600.0)
        self.assertEqual(turn, 3300.0)
        self.assertGreaterEqual(turn, 30 * 60)
        self.assertEqual(autonomy_run._verifier_timeout(
            "sustained-event-ledger", turn, 240.0), 240.0)
        self.assertEqual(autonomy_run._verifier_timeout(
            "sustained-event-ledger", turn, 30.0), 30.0)

    def test_other_cases_keep_their_proportional_suite_timeout(self) -> None:
        self.assertEqual(autonomy_run._turn_timeout(
            "ordinary-case", 3600.0, 3600.0, 3600.0), 1200.0)


class ProviderRequestAccountingTests(unittest.TestCase):
    def test_a005_unavailable_checkout_refusal_is_a_declared_negative_control(self) -> None:
        suite = autonomy.load_autonomy_suite(
            REPO_ROOT / "dev" / "harness_lab" / "fixtures" / "a005_placement")
        case = suite.case("a005-existing-unavailable-refusal")
        self.assertTrue(case.protected)
        self.assertEqual(case.control_trigger, "unavailable_managed_checkout")
        task_call = {"tool": "task", "task_placement": {
            "requested": "existing_managed_read_only", "worktree_id_present": True}}
        self.assertTrue(autonomy_run._declared_control_trigger_exercised(case, [task_call]))
        self.assertTrue(autonomy_run._control_exercised({"control_exercised": True}))
        self.assertFalse(autonomy_run._declared_control_trigger_exercised(
            case, [{"tool": "task", "task_placement": {
                "requested": "existing_managed_read_only", "worktree_id_present": False}}]))

    def test_paired_schedule_randomizes_arms_and_covers_each_pair_per_repeat(self):
        schedule = autonomy_run.paired_schedule(["rust", "python"], 3, 41)
        self.assertEqual(len(schedule), 12)
        for repetition in range(1, 4):
            for case_id in ("rust", "python"):
                arms = [row["arm"] for row in schedule
                        if row["repetition"] == repetition and row["case_id"] == case_id]
                self.assertEqual(sorted(arms), ["baseline", "candidate"])
        self.assertEqual(schedule, autonomy_run.paired_schedule(["rust", "python"], 3, 41))
        self.assertNotEqual(schedule, autonomy_run.paired_schedule(["rust", "python"], 3, 42))

    def test_paired_comparison_checkpoints_each_trial_and_returns_binary_hashes(self) -> None:
        suite = autonomy.load_autonomy_suite()
        case_id = suite.supported_cases()[0].case_id
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            binary = root / "codegg"
            binary.write_bytes(b"frozen test binary")
            lane = root / "comparison"
            policy = budget.BudgetPolicy(max_provider_requests=10,
                                         max_wallclock_seconds=60)
            receipt = {"case_id": case_id, "terminal": "completed", "solved": True,
                       "protected": False, "verifier_confirmed": True,
                       "duration_ms": 1, "provider_request_count": 1}
            with mock.patch.object(autonomy_run, "run_case", return_value=receipt):
                comparison = autonomy_run.run_paired_comparison(
                    binary, binary, suite, "opencode_go/muse-spark-1.3-contributor",
                    budget_policy=policy, lane_root=lane, repetitions=1,
                    seed=7, case_ids=[case_id])

            self.assertEqual(comparison["baseline_binary_sha256"],
                             autonomy_run.sha256_file(binary))
            self.assertEqual(comparison["candidate_binary_sha256"],
                             autonomy_run.sha256_file(binary))
            checkpoint = lane / "comparison-progress.jsonl"
            rows = [json.loads(line) for line in checkpoint.read_text().splitlines()]
            self.assertEqual(len(rows), 2)
            self.assertEqual({row["schedule"]["arm"] for row in rows},
                             {"baseline", "candidate"})
            self.assertTrue(all(row["result"]["solved"] for row in rows))

    def test_resume_completes_only_refused_rows_and_preserves_cumulative_budget(self) -> None:
        suite = autonomy.load_autonomy_suite()
        case_id = suite.supported_cases()[0].case_id
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            binary = root / "codegg"
            binary.write_bytes(b"frozen test binary")
            schedule = autonomy_run.paired_schedule([case_id], 1, 7)
            checkpoint = root / "checkpoint.jsonl"
            rows = []
            for index, row in enumerate(schedule):
                result = {"case_id": case_id, "terminal": "completed" if index == 0 else "refused",
                          "solved": False, "protected": False, "binary_sha256":
                          autonomy_run.sha256_file(binary)}
                if index == 0:
                    result["provider_request_count"] = 1
                rows.append({"schedule": row, "result": result})
            checkpoint.write_text("".join(json.dumps(row) + "\n" for row in rows),
                                   encoding="utf-8")
            lane = root / "resume"
            policy = budget.BudgetPolicy(max_provider_requests=3,
                                         max_wallclock_seconds=60)
            resumed_receipt = {"case_id": case_id, "terminal": "completed", "solved": False,
                               "protected": False, "provider_request_count": 1,
                               "binary_sha256": autonomy_run.sha256_file(binary)}
            def completed_trial(*args, **kwargs):
                kwargs["guard"].admit()
                kwargs["guard"].observe(provider_requests=1)
                return resumed_receipt

            with mock.patch.object(autonomy_run, "run_case", side_effect=completed_trial) as run:
                result = autonomy_run.resume_paired_comparison(
                    checkpoint, binary, binary, suite,
                    "opencode_go/muse-spark-1.3-contributor", budget_policy=policy,
                    lane_root=lane, repetitions=1, seed=7, case_ids=[case_id],
                    prior_provider_requests=12, prior_allowance_usd=0.25,
                    prior_elapsed_seconds=90)

            run.assert_called_once()
            self.assertEqual(result["resumption"]["resumed_trials"], 1)
            self.assertEqual(result["resumption"]["cumulative_provider_requests"], 13)
            self.assertEqual(result["resumption"]["cumulative_allowance_usd"], 0.25)
            self.assertGreaterEqual(result["resumption"]["cumulative_elapsed_seconds"], 90)
            self.assertEqual([row["terminal"] for row in result["schedule"]],
                             ["completed", "completed"])

    def test_counts_only_provider_boundary_admissions(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            log = pathlib.Path(temp) / "acp.stderr"
            log.write_text(
                "{\"request_number\":1,\"limit\":60}\n"
                "{\"request_number\":2,\"limit\":60}\n",
                encoding="utf-8",
            )
            self.assertEqual(autonomy_run._provider_request_admissions(log), 2)

    def test_request_and_allowance_receipts_are_separately_recoverable(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            receipt = pathlib.Path(temp) / "provider-receipts.jsonl"
            receipt.write_text(
                '{"kind":"request","request_number":1,"limit":20}\n'
                '{"kind":"allowance_observed","observation_number":1,'
                '"allowance_micros":1250000,"cumulative_allowance_micros":1250000}\n'
                '{"kind":"request","request_number":2,"limit":20}\n',
                encoding="utf-8",
            )
            self.assertEqual(autonomy_run._provider_request_admissions(receipt), 2)
            self.assertEqual(autonomy_run._provider_allowance_from_receipt(receipt), 1.25)

    def test_missing_provider_log_is_unavailable(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            self.assertIsNone(autonomy_run._provider_request_admissions(
                pathlib.Path(temp) / "missing.log"))

    def test_each_lane_receives_only_the_remaining_shared_suite_cap(self) -> None:
        policy = budget.BudgetPolicy(max_provider_requests=5)
        guard = budget.SpendGuard(policy)
        guard.observe(provider_requests=2)
        self.assertEqual(autonomy_run._remaining_provider_request_cap(policy, guard), 3)

    def test_an_exhausted_shared_cap_binds_zero_remaining_requests(self) -> None:
        policy = budget.BudgetPolicy(max_provider_requests=2)
        guard = budget.SpendGuard(policy)
        guard.observe(provider_requests=2)
        self.assertEqual(autonomy_run._remaining_provider_request_cap(policy, guard), 0)

    def test_an_unbounded_policy_does_not_create_a_lane_request_cap(self) -> None:
        policy = budget.BudgetPolicy(token_limit=100)
        guard = budget.SpendGuard(policy)
        self.assertIsNone(autonomy_run._remaining_provider_request_cap(policy, guard))

    def test_the_transcript_carries_the_protected_flag(self) -> None:
        case = _case("repeated-identical-failure-stall")
        transcript = autonomy_run.build_transcript(case, [], terminal="stalled",
                                                   stop_reason=None)
        self.assertTrue(transcript["protected"])
        self.assertEqual(transcript["terminal"], "stalled")


class RunCaseRefusalTests(unittest.TestCase):
    """A run that cannot be trusted must stop before a provider is called."""

    def test_disk_preflight_uses_nearest_existing_ancestor_for_new_lane(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp).resolve()
            missing_lane = root / "not-created" / "case00"
            self.assertEqual(autonomy_run._nearest_existing_directory(missing_lane), root)


    def test_a_deferred_case_is_refused(self) -> None:
        suite = autonomy.load_autonomy_suite()
        deferred = [case for case in suite.cases if case.support != "supported"]
        self.assertTrue(deferred, "the manifest declares no deferred case to refuse")
        with self.assertRaises(HarnessError):
            autonomy_run.run_case(pathlib.Path("/nonexistent"), deferred[0],
                                  "opencode_go/muse-spark-1.3-contributor",
                                  budget_policy=budget.BudgetPolicy(),
                                  lane_root=pathlib.Path("/nonexistent"))

    def test_sustained_qualification_without_request_cap_is_refused_before_setup(self) -> None:
        with self.assertRaisesRegex(HarnessError, "explicit provider request cap"):
            autonomy_run.run_case(
                pathlib.Path("/nonexistent"), autonomy.load_autonomy_suite(
                    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_h005").cases[0],
                "opencode_go/muse-spark-1.3-contributor",
                budget_policy=budget.BudgetPolicy(),
                lane_root=pathlib.Path("/nonexistent"),
                min_free_disk_bytes=10 * 1024**3)

    def test_h006_preflight_accepts_only_native_reported_helper_sibling(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            binary = pathlib.Path(temp) / "codegg"
            binary.write_bytes(b"fixture executable")
            binary.chmod(0o755)
            report = subprocess.CompletedProcess(
                args=[], returncode=0,
                stdout="Sandbox helper (codegg-sandbox-helper): present at "
                       f"{pathlib.Path(temp) / 'codegg-sandbox-helper'}\n",
                stderr="",
            )
            with mock.patch("dev.harness_lab.autonomy_run.subprocess.run",
                            return_value=report) as run:
                self.assertEqual(autonomy_run._preflight_h006_binary(binary), binary.resolve())
            self.assertEqual(run.call_args.args[0], [str(binary.resolve()),
                                                     "doctor", "installation"])
            self.assertNotIn("OPENCODE_GO_API_KEY", run.call_args.kwargs["env"])

    def test_missing_h006_helper_refuses_before_reserving_a_provider_request(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            binary = pathlib.Path(temp) / "codegg"
            binary.write_bytes(b"fixture executable")
            binary.chmod(0o755)
            lane = pathlib.Path(temp) / "lane"
            policy = budget.BudgetPolicy(token_limit=400000, max_provider_requests=120,
                                         max_wallclock_seconds=3600.0,
                                         cost_observable=False)
            guard = budget.SpendGuard(policy)
            report = subprocess.CompletedProcess(
                args=[], returncode=0,
                stdout="Sandbox helper (codegg-sandbox-helper): MISSING (expected "
                       f"{pathlib.Path(temp) / 'codegg-sandbox-helper'})\n",
                stderr="",
            )
            suite = autonomy.load_autonomy_suite(
                REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_h005")
            with mock.patch("dev.harness_lab.autonomy_run.subprocess.run",
                            return_value=report):
                with self.assertRaisesRegex(HarnessError, "Sandbox helper.*MISSING"):
                    autonomy_run.run_case(
                        binary, suite.case("sustained-event-ledger"),
                        "opencode_go/muse-spark-1.3-contributor",
                        budget_policy=policy, guard=guard, lane_root=lane,
                        repository=REPO_ROOT, timeout=3540,
                        min_free_disk_bytes=10 * 1024**3)
            self.assertEqual(guard.provider_requests, 0)

    def test_helper_failure_watcher_correlates_production_shaped_acp_events(self) -> None:
        # In ACP, title belongs to tool_call. Its terminal update contains the
        # call ID, status, and raw output, without repeating title.
        notifications = [
            {"jsonrpc": "2.0", "method": "session/update", "params": {
                "sessionId": "session-1", "update": {
                    "sessionUpdate": "tool_call", "toolCallId": "call-job-7",
                    "title": "job", "status": "in_progress",
                    "codeggArgumentsSha256": "a" * 64,
                    "codeggAction": "status",
                },
            }},
            {"jsonrpc": "2.0", "method": "session/update", "params": {
                "sessionId": "session-1", "update": {
                    "sessionUpdate": "tool_call_update", "toolCallId": "call-job-7",
                    "status": "failed",
                    "rawOutput": "trusted sandbox helper codegg-sandbox-helper "
                                 "could not be resolved: No such file or directory",
                },
            }},
        ]

        class FakeClient:
            def __init__(self) -> None:
                self.notifications = list(notifications)
                self.sent: list[tuple[str, dict]] = []

            def notify(self, method: str, params: dict) -> None:
                self.sent.append((method, params))

        client = FakeClient()
        watcher = autonomy_run._H006InfrastructureFailureWatcher(client, "session-1")
        watcher._run()
        self.assertTrue(watcher.failure_observed)
        self.assertEqual(client.sent, [("session/cancel", {"sessionId": "session-1"})])

    def test_helper_failure_watcher_ignores_nonterminal_and_unrelated_errors(self) -> None:
        updates = [
            # An in-progress update is not terminal evidence.
            {"sessionUpdate": "tool_call", "toolCallId": "job-running",
             "title": "job", "status": "in_progress"},
            {"sessionUpdate": "tool_call_update", "toolCallId": "job-running",
             "status": "in_progress",
             "rawOutput": "trusted sandbox helper codegg-sandbox-helper could not be resolved"},
            # A failed managed test with a different error is recoverable.
            {"sessionUpdate": "tool_call", "toolCallId": "test-other",
             "title": "test", "status": "in_progress"},
            {"sessionUpdate": "tool_call_update", "toolCallId": "test-other",
             "status": "failed", "rawOutput": "Cargo test failed: assertion did not hold"},
            # The same text from an unrelated tool is not the managed launch error.
            {"sessionUpdate": "tool_call", "toolCallId": "read-other",
             "title": "read", "status": "in_progress"},
            {"sessionUpdate": "tool_call_update", "toolCallId": "read-other",
             "status": "failed",
             "rawOutput": "trusted sandbox helper codegg-sandbox-helper could not be resolved"},
            # A terminal without a matching start cannot be attributed to a job/test.
            {"sessionUpdate": "tool_call_update", "toolCallId": "orphan",
             "status": "failed",
             "rawOutput": "trusted sandbox helper codegg-sandbox-helper could not be resolved"},
        ]

        class FakeClient:
            notifications = [
                {"jsonrpc": "2.0", "method": "session/update", "params": {
                    "sessionId": "session-1", "update": update,
                }} for update in updates
            ]

            def notify(self, _method: str, _params: dict) -> None:
                raise AssertionError("unrelated ACP events must not cancel the session")

        watcher = autonomy_run._H006InfrastructureFailureWatcher(FakeClient(), "session-1")
        watcher._absorb()
        self.assertFalse(watcher.failure_observed)

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


class H007ModelRouteTests(unittest.TestCase):
    def test_authorized_deepseek_route_is_accepted_and_other_routes_are_refused(self) -> None:
        self.assertIn("opencode_go/deepseek-v4.1-flash",
                      autonomy_run.H007_ALLOWED_MODEL_ROUTES)
        self.assertIn("opencode_go/muse-spark-1.3-contributor",
                      autonomy_run.H007_ALLOWED_MODEL_ROUTES)
        autonomy_run._validate_h007_model_route("opencode_go/deepseek-v4.1-flash")
        with self.assertRaisesRegex(HarnessError, "explicit qualification allowlist"):
            autonomy_run._validate_h007_model_route("opencode_go/unapproved-model")


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

    def test_h005_uses_a_separate_manifest_and_body(self) -> None:
        manifest = REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_h005"
        suite = autonomy.load_autonomy_suite(manifest)
        self.assertEqual([case.case_id for case in suite.cases], ["sustained-event-ledger"])
        self.assertEqual(suite.cases[0].family, "background_job")
        root = autonomy_run.case_root(REPO_ROOT, suite.cases[0].case_id)
        self.assertTrue((root / "fixture/Cargo.lock").is_file())
        self.assertTrue((root / "verifier/test_event_ledger_contract.py").is_file())


class DiskFloorWatcherTests(unittest.TestCase):
    def test_a_breached_floor_cancels_before_the_prompt(self) -> None:
        class Client:
            def __init__(self):
                self.notifications = []

            def notify(self, method, params):
                self.notifications.append((method, params))

        with tempfile.TemporaryDirectory() as directory:
            client = Client()
            watcher = autonomy_run._DiskFloorWatcher(
                pathlib.Path(directory), 10**30, client, "session-1")
            watcher.start()
            watcher.stop()
            self.assertTrue(watcher.triggered)
            self.assertEqual(client.notifications[0][0], "session/cancel")

    def test_a_safe_floor_records_free_space_without_cancel(self) -> None:
        class Client:
            def __init__(self):
                self.notifications = []

            def notify(self, method, params):
                self.notifications.append((method, params))

        with tempfile.TemporaryDirectory() as directory:
            client = Client()
            watcher = autonomy_run._DiskFloorWatcher(pathlib.Path(directory), 1,
                                                      client, "session-2")
            watcher.start()
            watcher.stop()
            self.assertIsInstance(watcher.free_bytes, int)
            self.assertFalse(watcher.triggered)
            self.assertEqual(client.notifications, [])

    def test_unreadable_disk_state_cancels_fail_closed(self) -> None:
        class Client:
            def __init__(self):
                self.notifications = []

            def notify(self, method, params):
                self.notifications.append((method, params))

        client = Client()
        watcher = autonomy_run._DiskFloorWatcher(
            pathlib.Path("/path/that/does/not/exist"), 1, client, "session-3")
        watcher.start()
        watcher.stop()
        self.assertEqual(watcher.error, "FileNotFoundError")
        self.assertEqual(client.notifications[0][0], "session/cancel")


if __name__ == "__main__":
    unittest.main()
