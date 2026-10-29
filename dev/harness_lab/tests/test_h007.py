from __future__ import annotations

import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

from dev.harness_lab import autonomy, h007_results

REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
BASE = REPO_ROOT / "dev/harness_lab/fixtures/build_provenance_catalog"
STARTER = BASE / "fixture"
REFERENCE = BASE / "reference"
VERIFIER = REPO_ROOT / "dev/autonomy_fixtures/build-provenance-catalog/verifier"
MANIFEST = REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_h007/cases.jsonl"


def _sha(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _call(command: list[str], *, cwd: pathlib.Path, timeout: int = 300, env=None):
    return subprocess.run(command, cwd=cwd, capture_output=True, text=True,
                          timeout=timeout, check=False, env=env)


class H007CaseAndResultTests(unittest.TestCase):
    def test_case_manifest_is_loadable_and_keeps_provider_authority_ungranted(self):
        suite = autonomy.load_autonomy_suite(MANIFEST.parent)
        case = suite.case("build-provenance-catalog")
        self.assertEqual(case.family, "background_job")
        self.assertEqual(case.budget["max_wallclock_seconds"], 3600)
        self.assertEqual(case.budget["max_provider_requests"], 60)
        self.assertIsNone(case.budget["max_cost_usd"])
        self.assertEqual(case.network, "none")
        self.assertFalse(case.protected)

    def test_h007_task_and_horizon_truth_table_is_independent(self):
        calls = [
            {"tool": "test", "started_elapsed_ms": 0, "finished_elapsed_ms": 10,
             "job": {"job_id": "job-1", "action": "submit", "status": "running"}},
            {"tool": "job", "started_elapsed_ms": 20, "finished_elapsed_ms": 30,
             "job": {"job_id": "job-1", "action": "status", "status": "running"}},
            {"tool": "write", "started_elapsed_ms": 100, "finished_elapsed_ms": 110},
        ]
        for elapsed in range(300000, 1800000, 300000):
            calls.append({"tool": "write", "started_elapsed_ms": elapsed,
                          "finished_elapsed_ms": elapsed + 10})
        calls.extend([
            {"tool": "job", "started_elapsed_ms": 1800000, "finished_elapsed_ms": 1800010,
             "job": {"job_id": "job-1", "action": "wait", "status": "completed"}},
            {"tool": "write", "started_elapsed_ms": 1800100, "finished_elapsed_ms": 1800110},
        ])
        early = h007_results.project_result(
            terminal="completed", verifier_ran=True, verifier_exit_code=0,
            duration_ms=1000, tool_calls=calls[:3], measurement_complete=True,
            continuation_confirmed=True, repair_cycles=2)
        self.assertEqual((early["task_outcome"], early["horizon_outcome"],
                          early["horizon_reason"]),
                         ("passed", "not_observed", "task_completed_before_threshold"))
        long_failure = h007_results.project_result(
            terminal="completed", verifier_ran=True, verifier_exit_code=1,
            duration_ms=1800200, tool_calls=calls, measurement_complete=True,
            continuation_confirmed=True, repair_cycles=2)
        self.assertEqual(long_failure["task_outcome"], "failed")
        self.assertEqual(long_failure["horizon_outcome"], "qualified")
        gap = [dict(call) for call in calls]
        gap[-1]["started_elapsed_ms"] = 3600000
        gap[-1]["finished_elapsed_ms"] = 3600010
        rejected_gap = h007_results.project_result(
            terminal="completed", verifier_ran=True, verifier_exit_code=0,
            duration_ms=3600100, tool_calls=gap, measurement_complete=True,
            continuation_confirmed=True, repair_cycles=2)
        self.assertEqual(rejected_gap["task_outcome"], "passed")
        self.assertEqual(rejected_gap["horizon_reason"], "activity_gap_exceeded")

    def test_exact_job_id_mismatch_and_incomplete_event_capture_block_only_horizon(self):
        calls = [
            {"tool": "test", "started_elapsed_ms": 0, "finished_elapsed_ms": 1,
             "job": {"job_id": "j1", "action": "submit", "status": "running"}},
            {"tool": "job", "started_elapsed_ms": 2, "finished_elapsed_ms": 3,
             "job": {"job_id": "j1", "action": "status", "status": "running"}},
            {"tool": "write", "started_elapsed_ms": 4, "finished_elapsed_ms": 5},
            {"tool": "job", "started_elapsed_ms": 1800000, "finished_elapsed_ms": 1800001,
             "job": {"job_id": "j2", "action": "wait", "status": "completed"}},
        ]
        mismatched = h007_results.project_result(
            terminal="completed", verifier_ran=True, verifier_exit_code=0,
            duration_ms=1800001, tool_calls=calls, measurement_complete=True,
            continuation_confirmed=True, repair_cycles=2)
        self.assertEqual(mismatched["task_outcome"], "passed")
        self.assertEqual(mismatched["horizon_reason"], "job_lifecycle_missing")
        incomplete = h007_results.project_result(
            terminal="completed", verifier_ran=True, verifier_exit_code=0,
            duration_ms=1800001, tool_calls=calls, measurement_complete=False,
            continuation_confirmed=True, repair_cycles=2)
        self.assertEqual(incomplete["task_outcome"], "passed")
        self.assertEqual(incomplete["horizon_reason"], "measurement_incomplete")

    def test_failed_background_job_cannot_qualify_the_horizon(self):
        calls = [
            {"tool": "test", "started_elapsed_ms": 0, "finished_elapsed_ms": 1,
             "job": {"job_id": "j1", "action": "submit", "status": "running"}},
            {"tool": "write", "started_elapsed_ms": 2, "finished_elapsed_ms": 3},
            {"tool": "job", "started_elapsed_ms": 1800000, "finished_elapsed_ms": 1800001,
             "job": {"job_id": "j1", "action": "wait", "status": "failed"}},
            {"tool": "write", "started_elapsed_ms": 1800002, "finished_elapsed_ms": 1800003},
        ]
        result = h007_results.project_result(
            terminal="completed", verifier_ran=True, verifier_exit_code=0,
            duration_ms=1800003, tool_calls=calls, measurement_complete=True,
            continuation_confirmed=True, repair_cycles=2)
        self.assertEqual(result["task_outcome"], "passed")
        self.assertEqual(result["horizon_outcome"], "not_qualified")
        self.assertEqual(result["horizon_reason"], "job_lifecycle_missing")

    def test_summary_keeps_task_and_horizon_denominators_separate(self):
        summary = h007_results.summarize_h007([
            {"task_outcome": "passed", "horizon_outcome": "not_observed"},
            {"task_outcome": "failed", "horizon_outcome": "qualified"},
            {"task_outcome": "inconclusive", "horizon_outcome": "not_qualified"},
        ])
        self.assertEqual(summary["task_denominator"], 3)
        self.assertEqual(summary["task_measured_denominator"], 2)
        self.assertEqual(summary["task_passes"], 1)
        self.assertEqual(summary["horizon_denominator"], 3)
        self.assertEqual(summary["horizon_qualified"], 1)
        self.assertEqual(summary["horizon_not_observed"], 1)

    def test_runner_preserves_legacy_h006_receipt_versions(self):
        case = autonomy.load_autonomy_suite(
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_h005").case("sustained-event-ledger")
        transcript = __import__("dev.harness_lab.autonomy_run", fromlist=["build_transcript"])
        self.assertEqual(transcript.build_transcript(case, [], terminal="completed",
                                                    stop_reason=None)["schema_version"], 3)

    def test_h007_transcript_uses_its_new_versioned_shape(self):
        from dev.harness_lab import autonomy_run
        case = autonomy.load_autonomy_suite(MANIFEST.parent).case("build-provenance-catalog")
        transcript = autonomy_run.build_transcript(
            case, [], terminal="completed", stop_reason="end_turn")
        self.assertEqual(transcript["schema_version"], 4)

    def test_h007_exact_duration_and_gap_boundaries_are_qualified(self):
        calls = []
        calls.append({"tool": "test", "started_elapsed_ms": 0, "finished_elapsed_ms": 1,
                      "job": {"job_id": "exact", "action": "submit", "status": "running"}})
        calls.append({"tool": "write", "started_elapsed_ms": 2, "finished_elapsed_ms": 3})
        for elapsed in range(300000, 1800000, 300000):
            calls.append({"tool": "edit", "started_elapsed_ms": elapsed,
                          "finished_elapsed_ms": elapsed + 1})
        calls.append({"tool": "job", "started_elapsed_ms": 1800000, "finished_elapsed_ms": 1800000,
                      "job": {"job_id": "exact", "action": "wait", "status": "completed"}})
        result = h007_results.project_result(
            terminal="completed", verifier_ran=True, verifier_exit_code=0,
            duration_ms=1800000, tool_calls=calls, measurement_complete=True,
            continuation_confirmed=True, repair_cycles=2)
        self.assertEqual(result["horizon_outcome"], "qualified")
        self.assertEqual(result["horizon_evidence"]["active_work_span_ms"], 1800000)
        self.assertEqual(result["horizon_evidence"]["maximum_activity_gap_ms"], 299999)

    def test_provider_cap_and_wallclock_must_be_explicit_and_within_case_ceiling(self):
        suite = autonomy.load_autonomy_suite(MANIFEST.parent)
        case = suite.case("build-provenance-catalog")
        from dev.harness_lab import autonomy_run, budget
        policy = budget.BudgetPolicy(max_provider_requests=61, max_wallclock_seconds=3600)
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(autonomy_run.HarnessError, "60-request ceiling"):
                autonomy_run.run_case(
                    REPO_ROOT / "target/debug/codegg", case,
                    "opencode_go/muse-spark-1.3-contributor", budget_policy=policy,
                    lane_root=pathlib.Path(temporary) / "lane", timeout=3600,
                    min_free_disk_bytes=10 * 1024**3)

    def test_h007_stall_bound_cancels_when_no_acp_tool_activity_arrives(self):
        from dev.harness_lab.autonomy_run import _ActivityStallWatcher

        class Client:
            notifications = []

            def __init__(self):
                self.cancelled = []

            def notify(self, method, params):
                self.cancelled.append((method, params))

        client = Client()
        watcher = _ActivityStallWatcher(client, "session-1", 0, poll_seconds=0.005)
        watcher.start()
        watcher.thread.join(timeout=1)
        watcher.stop()
        self.assertTrue(watcher.triggered)
        self.assertEqual(client.cancelled,
                         [("session/cancel", {"sessionId": "session-1"})])


class H007FixtureCalibrationTests(unittest.TestCase):
    def test_starter_fails_and_reference_solution_passes(self):
        starter = _call(["cargo", "test", "--offline", "--locked", "--workspace"], cwd=STARTER)
        self.assertNotEqual(starter.returncode, 0)
        self.assertIn("test result: FAILED", starter.stdout)
        reference = _call(["cargo", "test", "--offline", "--locked", "--workspace"], cwd=REFERENCE)
        self.assertEqual(reference.returncode, 0, reference.stdout + reference.stderr)

    def test_reference_passes_independent_cli_holdout_and_frozen_inputs(self):
        metadata = json.loads((BASE.parent / "build_provenance_catalog.case.json").read_text())
        self.assertEqual(metadata["manifest_sha256"], _sha(MANIFEST))
        self.assertEqual(metadata["hidden_verifier_sha256"], _sha(VERIFIER / "test_catalog_contract.py"))
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            lane = root / "lane"
            agent_root = lane / "case"
            private = lane / "private"
            shutil.copytree(REFERENCE, agent_root / "fixture", ignore=shutil.ignore_patterns("target"))
            (private / "verifier").mkdir(parents=True)
            (private / "case-metadata.json").write_text(json.dumps(metadata))
            shutil.copy2(VERIFIER / "test_catalog_contract.py", private / "verifier")
            env = dict(os.environ, CODEGG_AUTONOMY_CASE_ROOT=str(agent_root),
                       CODEGG_AUTONOMY_TRANSCRIPT="")
            result = _call(["python3", "-m", "unittest", "discover", "-s", "verifier", "-v"],
                           cwd=private, timeout=300, env=env)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("OK (skipped=1)", result.stderr)

    def test_frozen_public_contract_mutation_is_rejected(self):
        metadata = json.loads((BASE.parent / "build_provenance_catalog.case.json").read_text())
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            agent_root = root / "case"
            shutil.copytree(REFERENCE, agent_root / "fixture", ignore=shutil.ignore_patterns("target"))
            private = root / "private"
            (private / "verifier").mkdir(parents=True)
            (private / "case-metadata.json").write_text(json.dumps(metadata))
            shutil.copy2(VERIFIER / "test_catalog_contract.py", private / "verifier")
            (agent_root / "fixture/contract.json").write_text("{}\n")
            env = dict(os.environ, CODEGG_AUTONOMY_CASE_ROOT=str(agent_root))
            result = subprocess.run(["python3", "-m", "unittest", "discover", "-s", "verifier"],
                                    cwd=private, env=env, capture_output=True, text=True, timeout=300)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("contract.json", result.stderr)


if __name__ == "__main__":
    unittest.main()
