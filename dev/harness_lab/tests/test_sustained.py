"""Tests for the sustained scenario instrument.

The live sustained run is not exercised here: it needs a real route and a real
budget. What is exercised here is everything the instrument decides *before* a
provider request, plus the controlled interruption behaviour against the local
deterministic fixture provider, which is where the resource-cleanup claim is
actually proven.
"""
import tempfile
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
import shutil
import shlex
import subprocess
import unittest
from pathlib import Path

from dev.harness_lab import budget, long_horizon_report, recovery_controls_v2, sustained
from dev.harness_lab.supervisor import (
    HarnessError,
    _h006_compiler_diagnostics,
    _h006_failure_log_category,
    _h006_managed_test_diagnostics,
    run_tool_trial,
)

REPO_ROOT = Path(__file__).resolve().parents[3]


def _observed_completed_job(calls, job_id):
    """Return true only when the exact submitted job was observed completed."""
    return any(
        call.get("tool") == "job"
        and call.get("job", {}).get("job_id") == job_id
        and call.get("job", {}).get("status") == "completed"
        for call in calls
    )


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


class GapQualificationTest(unittest.TestCase):
    def test_zero_gap_bound_needs_59_instrumented_trials_to_reach_five_percent(self):
        self.assertGreater(sustained._one_sided_95_upper_bound(0, 58), 0.05)
        self.assertLessEqual(sustained._one_sided_95_upper_bound(0, 59), 0.05)
        self.assertIsNone(sustained._one_sided_95_upper_bound(0, 0))

    def test_a_gap_receipt_is_unmeasured_and_not_a_solved_trial(self):
        original = sustained.pilot.run_live_task
        sustained.pilot.run_live_task = lambda *_args, **_kwargs: {
            "terminal": "completed",
            "ok": True,
            "event_observation_state": "unmeasured_gap",
            "event_gap_count": 1,
            "measurement_failures": 1,
            "provider_usage": None,
        }
        try:
            with tempfile.TemporaryDirectory() as temp:
                receipt = sustained.run_sustained_scenario(
                    Path("/unused"),
                    "opencode_go/muse-spark-1.3-contributor",
                    budget_policy=_policy(),
                    plans=[sustained.TrialPlan("gap-case", "worklog")],
                    root=Path(temp) / "scenario",
                    repository=REPO_ROOT,
                )
        finally:
            sustained.pilot.run_live_task = original

        trial = receipt["trials"][0]
        self.assertEqual(trial["terminal"], "infrastructure_error")
        self.assertFalse(trial["ok"])
        self.assertEqual(trial["attempt_id"], "gap-case:0")
        self.assertEqual(receipt["measurement_failures"], 1)
        gap = receipt["event_gap_measurement"]
        self.assertEqual(gap["unit"], "instrumented_completed_trial")
        self.assertEqual(gap["instrumented_trials"], 1)
        self.assertEqual(gap["gap_trials"], 1)
        self.assertEqual(gap["qualification_state"], "insufficient_evidence")


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


class TypedRecoveryControlTest(unittest.TestCase):
    def test_repeated_failed_reads_are_typed_at_the_native_acp_boundary(self):
        binary = REPO_ROOT / "target" / "debug" / "codegg"
        if not binary.exists():
            self.skipTest("the debug binary is not built")
        repeated = [{"name": "read", "arguments": {"path": "absent.txt"}}
                    for _ in range(4)]
        trial = run_tool_trial(
            binary, "workspace-write", timeout=60.0,
            task_id="h002-repeated-identical-failure",
            tool_sequence=repeated, expected_file="absent.txt",
            expected_text=None, expected_exists=False,
            expected_tool_failures=4, agent_steps=5)
        self.assertEqual(trial["tool_failures"], 4)
        self.assertEqual(len(trial["tool_calls"]), 4)
        self.assertTrue(all(call["status"] == "failed" for call in trial["tool_calls"]))
        suite = recovery_controls_v2.load_suite()
        case = next(row for row in suite.cases
                    if row["case_id"] == "repeated-identical-failure-bounded")
        control = recovery_controls_v2.evaluate_control(
            case, trial["tool_calls"], suite_sha256=suite.digest, terminal="completed",
            stop_owner="agent_completed")
        self.assertTrue(control["control_exercised"],
                        (trial["tool_calls"], trial["workspace_revision_changed_paths"]))
        self.assertTrue(control["control_held"], control)

    def test_same_read_after_workspace_change_is_measured_as_progress(self):
        binary = REPO_ROOT / "target" / "debug" / "codegg"
        if not binary.exists():
            self.skipTest("the debug binary is not built")
        sequence = [
            {"name": "read", "arguments": {"path": "state.txt"}},
            {"name": "write", "arguments": {"path": "state.txt", "content": "new state"}},
            {"name": "read", "arguments": {"path": "state.txt"}},
        ]
        trial = run_tool_trial(
            binary, "workspace-write", timeout=60.0,
            task_id="h002-changing-state-read", tool_sequence=sequence,
            initial_files={"state.txt": "old state"}, expected_file="state.txt",
            expected_text="new state", expected_exists=True, agent_steps=5)
        suite = recovery_controls_v2.load_suite()
        case = next(row for row in suite.cases
                    if row["case_id"] == "changed-state-read-is-progress")
        control = recovery_controls_v2.evaluate_control(
            case, trial["tool_calls"], suite_sha256=suite.digest,
            terminal="completed", stop_owner="agent_completed")
        self.assertTrue(control["control_exercised"])
        self.assertTrue(control["control_held"])
        self.assertNotEqual(trial["tool_calls"][0]["state_revision"],
                            trial["tool_calls"][2]["state_revision"])

    def test_hard_turn_budget_is_a_separate_typed_stop_owner(self):
        binary = REPO_ROOT / "target" / "debug" / "codegg"
        if not binary.exists():
            self.skipTest("the debug binary is not built")
        repeated = [{"name": "read", "arguments": {"path": "absent.txt"}}
                    for _ in range(4)]
        trial = run_tool_trial(
            binary, "workspace-write", timeout=60.0,
            task_id="h002-hard-turn-budget", tool_sequence=repeated,
            expected_file="absent.txt", expected_text=None, expected_exists=False,
            expected_tool_failures=3, agent_steps=4)
        suite = recovery_controls_v2.load_suite()
        case = next(row for row in suite.cases
                    if row["case_id"] == "turn-budget-owner-is-explicit")
        control = recovery_controls_v2.evaluate_control(
            case, trial["tool_calls"], suite_sha256=suite.digest,
            terminal=trial["stop_reason"],
            stop_owner=trial["stop_owner"])
        self.assertTrue(trial["hard_budget_stop"])
        self.assertEqual(control["actual_stop_owner"], "agent_turn_budget")
        self.assertTrue(control["control_exercised"])


class H006FailureDiagnosticTest(unittest.TestCase):
    def test_report_projection_is_bounded_and_does_not_retain_raw_output(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory)
            run_dir = workspace / "job-check" / ".codegg" / "test-runs" / "run-1"
            run_dir.mkdir(parents=True)
            stdout_path = run_dir / "stdout.log"
            stdout_path.write_text(
                json.dumps({
                    "reason": "compiler-message",
                    "message": {
                        "level": "error",
                        "code": {"code": "E0425"},
                        "message": "cannot find value PRIVATE_EXECUTOR_TEXT in this scope",
                    },
                }) + "\n" + ("padding\n" * 3000) + "error: could not compile\n",
                encoding="utf-8")
            stderr_path = run_dir / "stderr.log"
            stderr_path.write_text(
                "error: permission denied; PRIVATE_EXECUTOR_TEXT\n", encoding="utf-8")
            stdout_bytes = stdout_path.stat().st_size
            stderr_bytes = stderr_path.stat().st_size
            (run_dir / "report.json").write_text(json.dumps({
                "status": "Failed",
                "exit_code": 101,
                "duration_ms": 17,
                "output_truncated": True,
                "failures": [{
                    "failure_class": "NonzeroExit",
                    "message": "PRIVATE_EXECUTOR_TEXT",
                }],
                "stdout_log": str(stdout_path),
                "stderr_log": str(stderr_path),
                "argv": ["cargo", "test"],
                "cwd": str(run_dir),
            }), encoding="utf-8")

            diagnostic = _h006_managed_test_diagnostics(workspace)

        self.assertEqual(diagnostic, [{
            "status": "failed",
            "exit_code": 101,
            "duration_ms": 17,
            "cwd_scope": "job_check_subpath",
            "output_truncated": True,
            "stdout_log_bytes": stdout_bytes,
            "stderr_log_bytes": stderr_bytes,
            "failure_classes": ["nonzero_exit"],
            "stdout_category": "compilation_failure",
            "stderr_category": "filesystem_permission",
            "compiler_error_codes": ["E0425"],
            "compiler_error_families": ["unresolved_name", "cargo_compile_summary"],
            "cargo_json_compiler_messages": 1,
            "cargo_json_error_messages": 1,
            "cargo_json_error_source_span_count": 0,
            "cargo_json_error_without_source_span_count": 1,
            "cargo_json_os_error_kinds": [],
            "cargo_json_error_origins": [],
            "cargo_json_error_message_kinds": [],
            "cargo_json_error_path_roles": [],
            "cargo_json_error_path_subroles": [],
            "cargo_json_write_error_artifact_kinds": [],
            "cargo_json_write_error_os_numbers": [],
            "cargo_json_build_finished_events": 0,
        }])
        self.assertNotIn("PRIVATE_EXECUTOR_TEXT", json.dumps(diagnostic))

    def test_unknown_h006_output_maps_to_a_closed_category(self):
        self.assertEqual(
            _h006_failure_log_category("PRIVATE_EXECUTOR_TEXT"), "unclassified")

    def test_compiler_diagnostics_project_only_allowlisted_codes_and_families(self):
        diagnostic = _h006_compiler_diagnostics(
            "error[E0425]: cannot find value PRIVATE_EXECUTOR_TEXT in this scope\n"
            "error[E0308]: mismatched types PRIVATE_EXECUTOR_TEXT\n")

        expected = {
            "compiler_error_codes": ["E0308", "E0425"],
            "compiler_error_families": ["unresolved_name", "type_mismatch"],
            "cargo_json_compiler_messages": 0,
            "cargo_json_error_messages": 0,
            "cargo_json_error_source_span_count": 0,
            "cargo_json_error_without_source_span_count": 0,
            "cargo_json_os_error_kinds": [],
            "cargo_json_error_origins": [],
            "cargo_json_error_message_kinds": [],
            "cargo_json_error_path_roles": [],
            "cargo_json_error_path_subroles": [],
            "cargo_json_write_error_artifact_kinds": [],
            "cargo_json_write_error_os_numbers": [],
            "cargo_json_build_finished_events": 0,
        }
        self.assertEqual(diagnostic, expected)
        self.assertNotIn("PRIVATE_EXECUTOR_TEXT", json.dumps(diagnostic))

        cargo_event = json.dumps({
            "reason": "compiler-message",
            "package_id": "path+file:///workspace/job-check/core#job-core@0.1.0",
            "target": {"kind": ["lib"]},
            "message": {
                "level": "error",
                "code": {"code": "E0425"},
                "message": "cannot find value PRIVATE_EXECUTOR_TEXT in this scope",
            },
        })
        self.assertEqual(_h006_compiler_diagnostics(cargo_event), {
            "compiler_error_codes": ["E0425"],
            "compiler_error_families": ["unresolved_name"],
            "cargo_json_compiler_messages": 1,
            "cargo_json_error_messages": 1,
            "cargo_json_error_source_span_count": 0,
            "cargo_json_error_without_source_span_count": 1,
            "cargo_json_os_error_kinds": [],
            "cargo_json_error_origins": ["core:lib"],
            "cargo_json_error_message_kinds": [],
            "cargo_json_error_path_roles": [],
            "cargo_json_error_path_subroles": [],
            "cargo_json_write_error_artifact_kinds": [],
            "cargo_json_write_error_os_numbers": [],
            "cargo_json_build_finished_events": 0,
        })
        os_error_event = json.dumps({
            "reason": "compiler-message",
            "message": {
                "level": "error",
                "message": "failed to open PRIVATE_EXECUTOR_PATH (os error 13)",
                "spans": [],
            },
        })
        os_error_diagnostic = _h006_compiler_diagnostics(os_error_event)
        self.assertEqual(os_error_diagnostic["cargo_json_os_error_kinds"], ["permission_denied"])
        self.assertEqual(os_error_diagnostic["cargo_json_error_without_source_span_count"], 1)
        self.assertNotIn("PRIVATE_EXECUTOR_PATH", json.dumps(os_error_diagnostic))

        driver_error_event = json.dumps({
            "reason": "compiler-message",
            "package_id": "path+file:///workspace/job-check/core#job-core@0.1.0",
            "target": {"kind": ["lib"]},
            "message": {
                "level": "error",
                "message": "unknown option PRIVATE_EXECUTOR_FLAG",
            },
        })
        driver_error_diagnostic = _h006_compiler_diagnostics(driver_error_event)
        self.assertEqual(
            driver_error_diagnostic["cargo_json_error_message_kinds"],
            ["invalid_compiler_argument"],
        )
        self.assertEqual(driver_error_diagnostic["cargo_json_error_origins"], ["core:lib"])
        self.assertNotIn("PRIVATE_EXECUTOR_FLAG", json.dumps(driver_error_diagnostic))

        filesystem_event = json.dumps({
            "reason": "compiler-message",
            "package_id": "path+file:///workspace/job-check/core#job-core@0.1.0",
            "target": {"kind": ["lib"]},
            "message": {
                "level": "error",
                "message": "failed to write PRIVATE_EXECUTOR_PATH: permission denied",
                "spans": [],
            },
        })
        filesystem_diagnostic = _h006_compiler_diagnostics(filesystem_event)
        self.assertEqual(
            filesystem_diagnostic["cargo_json_error_message_kinds"],
            ["filesystem_permission_denied", "filesystem_write_failure"],
        )
        self.assertEqual(filesystem_diagnostic["cargo_json_error_origins"], ["core:lib"])
        self.assertNotIn("PRIVATE_EXECUTOR_PATH", json.dumps(filesystem_diagnostic))

        environment_event = json.dumps({
            "reason": "compiler-message",
            "message": {
                "level": "error",
                "message": "environment variable PRIVATE_ENV is not set",
                "spans": [],
            },
        })
        environment_diagnostic = _h006_compiler_diagnostics(environment_event)
        self.assertEqual(
            environment_diagnostic["cargo_json_error_message_kinds"], ["environment_configuration"])
        self.assertNotIn("PRIVATE_ENV", json.dumps(environment_diagnostic))

    def test_write_path_diagnostic_emits_only_the_contained_path_role(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory) / "workspace"
            target = (workspace / "job-check" / ".cargo-target" / "debug"
                      / "incremental" / "query-cache")
            event = json.dumps({
                "reason": "compiler-message",
                "message": {
                    "level": "error",
                    "message": f"failed to write {target}: permission denied",
                },
            })
            diagnostic = _h006_compiler_diagnostics(event, workspace=workspace)

        self.assertEqual(
            diagnostic["cargo_json_error_path_roles"], ["cargo_target_or_temp"])
        self.assertEqual(
            diagnostic["cargo_json_error_path_subroles"], ["cargo_incremental_state"])
        self.assertNotIn(str(workspace), json.dumps(diagnostic))

    def test_unmapped_write_errno_and_artifact_kind_stay_visible(self):
        """An errno outside the named map must not vanish from the projection.

        `cargo_json_os_error_kinds` only names eight numbers, so a hosted
        write failure on any other one previously reported no syscall at all
        and could not be diagnosed from the sanitized receipt. Cargo 1.100
        also compiles every target under `debug/build/<pkg>/<META>/out/`, so
        the subrole alone cannot say which artifact the write targeted.
        """
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory) / "workspace"
            artifact = (workspace / "job-check" / ".cargo-target" / "debug"
                        / "build" / "job-core" / "607f04dd27ba51d5" / "out"
                        / "libjob_core-607f04dd27ba51d5.rlib")
            event = json.dumps({
                "reason": "compiler-message",
                "package_id": "path+file:///workspace/job-check/core#job-core@0.1.0",
                "target": {"kind": ["lib"]},
                "message": {
                    "level": "error",
                    "message": (
                        f"failed to write {artifact}: "
                        "invalid cross-device link (os error 18)"),
                    "spans": [],
                },
            })
            diagnostic = _h006_compiler_diagnostics(event, workspace=workspace)

        self.assertEqual(
            diagnostic["cargo_json_error_path_subroles"], ["cargo_build_script_artifact"])
        self.assertEqual(diagnostic["cargo_json_write_error_artifact_kinds"], ["rlib"])
        self.assertEqual(diagnostic["cargo_json_write_error_os_numbers"], [18])
        # The number is named, so it is not duplicated as an unmapped one.
        self.assertEqual(diagnostic["cargo_json_os_error_kinds"], [])
        self.assertNotIn("607f04dd27ba51d5", json.dumps(diagnostic))

        mapped = _h006_compiler_diagnostics(json.dumps({
            "reason": "compiler-message",
            "message": {"level": "error", "message": "failed to write out/x.rmeta: "
                        "permission denied (os error 13)"},
        }), workspace=workspace)
        self.assertEqual(mapped["cargo_json_write_error_os_numbers"], [])
        self.assertEqual(mapped["cargo_json_os_error_kinds"], ["permission_denied"])


class LongHorizonNativeJobTest(unittest.TestCase):
    fixture = REPO_ROOT / "dev" / "harness_lab" / "fixtures" / "long_horizon_rust_job"
    verifier = fixture / "verifier" / "verify.py"
    case_manifest = REPO_ROOT / "dev" / "harness_lab" / "fixtures" / "long_horizon_rust_job.case.json"

    def test_long_horizon_fixture_and_holdout_digests_are_frozen(self):
        manifest = json.loads(self.case_manifest.read_text(encoding="utf-8"))
        digest = hashlib.sha256()
        for path in sorted(self.fixture.rglob("*")):
            relative_path = path.relative_to(self.fixture)
            if not path.is_file() or "verifier" in relative_path.parts:
                continue
            relative = relative_path.as_posix().encode()
            content = path.read_bytes()
            digest.update(len(relative).to_bytes(4, "big"))
            digest.update(relative)
            digest.update(len(content).to_bytes(8, "big"))
            digest.update(content)
        self.assertEqual(digest.hexdigest(), manifest["fixture_source_sha256"])
        self.assertEqual(hashlib.sha256(self.verifier.read_bytes()).hexdigest(),
                         manifest["hidden_verifier_sha256"])
        public_tests = self.fixture / "tests" / "public_contract.rs"
        self.assertEqual(hashlib.sha256(public_tests.read_bytes()).hexdigest(),
                         manifest["public_contract_sha256"])
        self.assertFalse(manifest["hidden_verifier_writable_by_agent"])

    def _sequence(self):
        run_tests = {"name": "test", "arguments": {
            "scope": "custom", "command": "cargo test --offline --locked",
            "execution_mode": "background"}}
        wait = {"name": "job", "arguments": {
            "action": "wait", "job_id": "{job_id}", "timeout_ms": 300000}}
        return [
            {"name": "read", "arguments": {"path": "README.md"}},
            {"name": "read", "arguments": {"path": "src/lib.rs"}},
            run_tests,
            {"name": "read", "arguments": {"path": "src/parser.rs"}},
            wait,
            {"name": "read", "arguments": {"path": "src/normalize.rs"}},
            {"name": "write", "arguments": {
                "path": "src/normalize.rs",
                "content": "pub fn normalize(parts: Vec<&str>) -> Option<(u32, u32, u32)> {\n    let major = parts[0].parse::<u32>().ok()?;\n    let minor = parts[1].parse::<u32>().ok()?;\n    let patch = parts[2].parse::<u32>().ok()?;\n    Some((major, minor, patch))\n}\n"}},
            run_tests,
            {"name": "read", "arguments": {"path": "README.md"}},
            wait,
            {"name": "read", "arguments": {"path": "src/parser.rs"}},
            {"name": "write", "arguments": {
                "path": "src/parser.rs",
                "content": "pub fn split_version(input: &str) -> Option<Vec<&str>> {\n    let trimmed = input.trim();\n    let normalized = trimmed.strip_prefix('v').unwrap_or(trimmed);\n    if normalized.is_empty() { return None; }\n    let parts: Vec<&str> = normalized.split('.').collect();\n    (parts.len() == 3 && parts.iter().all(|part| !part.is_empty()))\n        .then_some(parts)\n}\n"}},
            run_tests,
            {"name": "read", "arguments": {"path": "src/normalize.rs"}},
            wait,
        ]

    def test_native_acp_runs_coherent_rust_repair_with_real_background_job(self):
        binary = REPO_ROOT / "target" / "debug" / "codegg"
        if not binary.exists():
            self.skipTest("the debug binary is not built")
        trial = run_tool_trial(
            binary, "workspace-write", timeout=240.0,
            task_id="h003-long-horizon-rust-job", tool_sequence=self._sequence(),
            agent_steps=18, expected_file="never-created", expected_text=None,
            expected_exists=False, workspace_snapshot=self.fixture,
            external_verifier=self.verifier, expected_tool_failures=2)
        self.assertTrue(trial["verification"]["returncode"] == 0)
        self.assertEqual(trial["event_gap_count"], 0)
        self.assertEqual(len(trial["source_sha256"]), 64)
        self.assertGreater(trial["wall_time_ms"], 0)
        self.assertEqual(sum(call["tool"] == "test" for call in trial["tool_calls"]), 3)
        self.assertEqual(sum(call["tool"] == "job" for call in trial["tool_calls"]), 3)
        self.assertEqual(sum(call["status"] == "failed" for call in trial["tool_calls"]), 2)
        submissions = [call for call in trial["fixture_calls"] if call["name"] == "test"]
        waits = [call for call in trial["fixture_calls"] if call["name"] == "job"]
        self.assertEqual(len(submissions), 3)
        self.assertEqual([call["job_id"] for call in waits],
                         [call.get("job_id") for call in submissions])
        case = json.loads(self.case_manifest.read_text(encoding="utf-8"))
        case["case_manifest_sha256"] = hashlib.sha256(self.case_manifest.read_bytes()).hexdigest()
        receipt = long_horizon_report.build_receipt(
            case, trial, attempt_id="h003-native-correct-trajectory")
        self.assertEqual(receipt["job_ids"], [call["job_id"] for call in waits])
        self.assertEqual(receipt["independent_verifier"], "passed")
        self.assertEqual(receipt["context_transition"]["state"], "unavailable_not_exposed")
        self.assertNotIn("rawOutput", repr(receipt))

    def test_two_private_rust_job_lanes_run_without_shared_target_state(self):
        binary = REPO_ROOT / "target" / "debug" / "codegg"
        if not binary.exists():
            self.skipTest("the debug binary is not built")
        args = dict(binary=binary, profile="workspace-write", timeout=240.0,
                    tool_sequence=self._sequence(), agent_steps=18,
                    expected_file="never-created", expected_text=None,
                    expected_exists=False, workspace_snapshot=self.fixture,
                    external_verifier=self.verifier, expected_tool_failures=2)
        with ThreadPoolExecutor(max_workers=2) as pool:
            first = pool.submit(run_tool_trial, task_id="h003-lane-a", **args)
            second = pool.submit(run_tool_trial, task_id="h003-lane-b", **args)
            results = [first.result(), second.result()]
        self.assertTrue(all(result["verification"]["returncode"] == 0 for result in results))
        self.assertTrue(all(result["event_observation_state"] == "measured_no_gap"
                            for result in results))
        self.assertNotEqual(results[0]["workspace"], results[1]["workspace"])
        self.assertNotEqual(results[0]["fixture_calls"][4]["job_id"],
                            results[1]["fixture_calls"][4]["job_id"])

    def test_hidden_verifier_names_a_deleted_public_test_shortcut(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory) / "workspace"
            shutil.copytree(self.fixture, workspace,
                            ignore=shutil.ignore_patterns("verifier", "build.rs"))
            unsolved = subprocess.run(["python3", str(self.verifier), str(workspace)],
                                      capture_output=True, text=True, check=False)
            self.assertEqual(unsolved.returncode, 5)
            self.assertIn("hidden Rust contract verification failed", unsolved.stdout)
            (workspace / "tests" / "public_contract.rs").unlink()
            result = subprocess.run(["python3", str(self.verifier), str(workspace)],
                                    capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 3)
        self.assertIn("public contract tests were deleted", result.stdout)

    def test_native_test_rewrite_shortcut_is_rejected_by_hidden_verifier(self):
        binary = REPO_ROOT / "target" / "debug" / "codegg"
        if not binary.exists():
            self.skipTest("the debug binary is not built")
        sequence = [{"name": "write", "arguments": {
            "path": "tests/public_contract.rs", "content": "#[test] fn always_passes() {}\n"}}]
        with self.assertRaises(HarnessError) as raised:
            run_tool_trial(
                binary, "workspace-write", timeout=60.0,
                task_id="h003-test-rewrite-shortcut", tool_sequence=sequence,
                agent_steps=3, expected_file="never-created", expected_text=None,
                expected_exists=False, workspace_snapshot=self.fixture,
                external_verifier=self.verifier)
        self.assertIn("public contract tests were modified", str(raised.exception))

    def test_h006_real_background_job_is_observed_by_exact_id_while_work_continues(self):
        configured_binary = os.environ.get("CODEGG_H006_CODEGG_BINARY")
        binary = (Path(configured_binary) if configured_binary else
                  REPO_ROOT / "target" / "debug" / "codegg")
        self.assertTrue(binary.is_file(), "build CodeGG and set CODEGG_H006_CODEGG_BINARY")
        rustup = shutil.which("rustup")
        self.assertTrue(rustup, "the native test fixture requires the configured Rust toolchain")
        cargo = subprocess.run([rustup, "which", "cargo"], capture_output=True,
                               text=True, check=True).stdout.strip()
        rustc = subprocess.run([rustup, "which", "rustc"], capture_output=True,
                               text=True, check=True).stdout.strip()
        self.assertTrue(Path(cargo).is_file(), "rustup must resolve a real cargo executable")
        self.assertTrue(Path(rustc).is_file(), "rustup must resolve a real rustc executable")
        sequence = [
            {"name": "test", "arguments": {
                "scope": "custom", "command": (
                    f"{shlex.quote(cargo)} test --offline --workspace "
                    "--message-format=json"),
                "workdir": "job-check", "execution_mode": "background"}},
            # This is a bounded wait on the actual submitted job, not a fixture
            # delay. It gives the scheduler time to admit the real Cargo work
            # while the session is free to make useful source changes.
            {"name": "job", "arguments": {
                "action": "wait", "job_id": "{job_id}", "timeout_ms": 5000}},
            {"name": "write", "arguments": {
                "path": "src/parser.rs", "content": "pub fn split_version(input: &str) -> Option<Vec<&str>> {\n    let trimmed = input.trim();\n    let normalized = trimmed.strip_prefix('v').unwrap_or(trimmed);\n    if normalized.is_empty() { return None; }\n    let parts: Vec<&str> = normalized.split('.').collect();\n    (parts.len() == 3 && parts.iter().all(|part| !part.is_empty()))\n        .then_some(parts)\n}\n"}},
            {"name": "write", "arguments": {
                "path": "src/normalize.rs", "content": "pub fn normalize(parts: Vec<&str>) -> Option<(u32, u32, u32)> {\n    let major = parts[0].parse::<u32>().ok()?;\n    let minor = parts[1].parse::<u32>().ok()?;\n    let patch = parts[2].parse::<u32>().ok()?;\n    Some((major, minor, patch))\n}\n"}},
            {"name": "job", "arguments": {"action": "status", "job_id": "{job_id}"}},
            {"name": "write", "arguments": {
                "path": "src/lib.rs", "content": "mod normalize;\nmod parser;\n\npub fn parse_version(input: &str) -> Option<(u32, u32, u32)> {\n    let parts = parser::split_version(input)?;\n    normalize::normalize(parts)\n}\n"}},
            {"name": "job", "arguments": {"action": "status", "job_id": "{job_id}"}},
            # A third write and a third observation. `job wait` reports a
            # status only when the job reaches a terminal state inside its
            # window, so a bounded wait that times out contributes no
            # `running` sample: the assertion below had exactly two chances
            # and both had to land while the job was still live. Every pair
            # of adjacent observations keeps a write between them, so the
            # intervening-write check holds for whichever pair reports
            # `running`.
            {"name": "write", "arguments": {
                "path": "src/lib.rs", "content": "mod normalize;\nmod parser;\n\npub fn parse_version(input: &str) -> Option<(u32, u32, u32)> {\n    let parts = parser::split_version(input)?;\n    normalize::normalize(parts)\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn version_round_trips() {\n        assert_eq!(super::parse_version(\"v1.2.3\"), Some((1, 2, 3)));\n    }\n}\n"}},
            {"name": "job", "arguments": {"action": "status", "job_id": "{job_id}"}},
            {"name": "job", "arguments": {
                "action": "wait", "job_id": "{job_id}", "timeout_ms": 300000}},
        ]
        trial = run_tool_trial(
            binary, "workspace-write", timeout=300.0,
            task_id="h006-real-job-observation", tool_sequence=sequence,
            expected_file="never-created", expected_text=None, expected_exists=False,
            workspace_snapshot=self.fixture, rustc_executable=Path(rustc),
            cargo_home_under_test_workdir=True,
            initial_files={
                "job-check/.cargo/config.toml": "[build]\ntarget-dir = \".cargo-target\"\n",
                "job-check/Cargo.toml": "[workspace]\nmembers = [\"core\", \"index\", \"store\", \"cli\"]\nresolver = \"2\"\n",
                "job-check/core/Cargo.toml": "[package]\nname = \"job-core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
                "job-check/core/src/lib.rs": """#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Row { pub timestamp: i64, pub id: String, pub amount: i64 }

pub fn parse(input: &str) -> (Vec<Row>, Vec<String>) {
    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for (line, text) in input.lines().enumerate() {
        let fields: Vec<_> = text.split('|').collect();
        if fields.len() != 5 || fields[1].is_empty() || fields[4].is_empty()
            || !matches!(fields[2], "open" | "close" | "transfer") {
            errors.push(format!("invalid row {}", line + 1));
            continue;
        }
        match (fields[0].parse::<i64>(), fields[3].parse::<i64>()) {
            (Ok(timestamp), Ok(amount)) => rows.push(Row {
                timestamp, id: fields[1].to_string(), amount,
            }),
            _ => errors.push(format!("invalid number on line {}", line + 1)),
        }
    }
    (rows, errors)
}

#[cfg(test)]
mod tests {
    #[test]
    fn invalid_rows_report_their_line() {
        let (rows, errors) = super::parse("1|a|open|2|source\\nbad");
        assert_eq!(rows.len(), 1);
        assert_eq!(errors, vec!["invalid row 2"]);
    }
}
""",
                "job-check/index/Cargo.toml": "[package]\nname = \"job-index\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\njob-core = { path = \"../core\" }\n",
                "job-check/index/src/lib.rs": """use std::collections::BTreeMap;

pub fn index(row: job_core::Row) -> (i64, usize) { (row.amount, 1) }

pub fn canonicalize(records: impl IntoIterator<Item = job_core::Row>) -> Result<Vec<job_core::Row>, String> {
    let mut by_id = BTreeMap::new();
    for record in records {
        if let Some(existing) = by_id.get(&record.id) {
            if existing != &record { return Err(format!("conflicting duplicate id {}", record.id)); }
        } else { by_id.insert(record.id.clone(), record); }
    }
    let mut rows: Vec<_> = by_id.into_values().collect();
    rows.sort_by(|a, b| (a.timestamp.div_euclid(60), &a.id).cmp(&(b.timestamp.div_euclid(60), &b.id)));
    Ok(rows)
}

pub fn summaries(records: &[job_core::Row]) -> Vec<(i64, i64, usize)> {
    let mut windows: BTreeMap<i64, (i64, usize)> = BTreeMap::new();
    for row in records {
        let entry = windows.entry(row.timestamp.div_euclid(60)).or_default();
        entry.0 += row.amount;
        entry.1 += 1;
    }
    windows.into_iter().map(|(window, (amount, count))| (window, amount, count)).collect()
}
""",
                "job-check/store/Cargo.toml": "[package]\nname = \"job-store\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\njob-core = { path = \"../core\" }\njob-index = { path = \"../index\" }\n",
                "job-check/store/src/lib.rs": """use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

fn encode(row: &job_core::Row) -> String {
    let body = format!("{}|{}|{}", row.timestamp, row.id, row.amount);
    let checksum = body.bytes().fold(0u64, |sum, byte| sum.wrapping_add(u64::from(byte)));
    format!("v1|{body}|{checksum:016x}")
}

fn decode(line: &str) -> Result<job_core::Row, String> {
    let fields: Vec<_> = line.split('|').collect();
    if fields.len() != 5 || fields[0] != "v1" { return Err("invalid journal record".into()); }
    let body = fields[1..4].join("|");
    let expected = body.bytes().fold(0u64, |sum, byte| sum.wrapping_add(u64::from(byte)));
    if u64::from_str_radix(fields[4], 16).ok() != Some(expected) { return Err("checksum mismatch".into()); }
    Ok(job_core::Row {
        timestamp: fields[1].parse().map_err(|_| "invalid timestamp")?,
        id: fields[2].to_string(),
        amount: fields[3].parse().map_err(|_| "invalid amount")?,
    })
}

pub fn store(row: job_core::Row) -> (i64, usize) { job_index::index(row) }

pub fn append(path: &Path, rows: &[job_core::Row]) -> Result<(), String> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    let mut file = OpenOptions::new().create(true).append(true).open(path).map_err(|e| e.to_string())?;
    for row in rows { writeln!(file, "{}", encode(row)).map_err(|e| e.to_string())?; }
    file.sync_all().map_err(|e| e.to_string())
}

pub fn replay(path: &Path) -> Result<Vec<job_core::Row>, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let lines: Vec<_> = text.lines().collect();
    let mut rows = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        match decode(line) {
            Ok(row) => rows.push(row),
            Err(_error) if index + 1 == lines.len() => break,
            Err(error) => return Err(format!("journal line {}: {error}", index + 1)),
        }
    }
    job_index::canonicalize(rows)
}
""",
                "job-check/cli/Cargo.toml": "[package]\nname = \"job-cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\njob-core = { path = \"../core\" }\njob-index = { path = \"../index\" }\njob-store = { path = \"../store\" }\n",
                "job-check/cli/src/lib.rs": """pub fn run(value: i64) -> (i64, usize) {
    job_store::store(job_core::Row { timestamp: value, id: value.to_string(), amount: value })
}
#[cfg(test)]
mod tests {
    #[test]
    fn cli_contract() { assert_eq!(super::run(5), (5, 1)); }

    #[test]
    fn ingests_replays_and_merges_five_shards_at_scale() {
        let root = std::env::temp_dir().join(format!("job-ledger-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let input: String = (0..1_000_000)
            .map(|n| format!("{n}|event-{n}|transfer|{}|source\\n", n % 17))
            .collect();
        let (records, errors) = job_core::parse(&input);
        assert!(errors.is_empty());
        assert_eq!(records.len(), 1_000_000);
        let mut shards: [Vec<job_core::Row>; 5] = std::array::from_fn(|_| Vec::new());
        for (index, record) in records.into_iter().enumerate() {
            shards[index % shards.len()].push(record);
        }
        let mut recovered = Vec::new();
        for (index, shard) in shards.iter().enumerate() {
            let path = root.join(format!("shard-{index}.journal"));
            job_store::append(&path, shard).unwrap();
            recovered.extend(job_store::replay(&path).unwrap());
            assert_eq!(
                job_store::replay(&path).unwrap(),
                job_index::canonicalize(shard.clone()).unwrap()
            );
        }
        let ordered = job_index::canonicalize(recovered).unwrap();
        assert_eq!(ordered.len(), 1_000_000);
        assert_eq!(
            job_index::summaries(&ordered).iter().map(|(_, _, count)| count).sum::<usize>(),
            1_000_000
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
""",
                "job-check/cli/src/main.rs": "fn main() { assert_eq!(job_cli::run(5), (5, 1)); }\n",
            })
        self.addCleanup(shutil.rmtree, Path(trial["workspace"]).parent, ignore_errors=True)
        calls = trial["tool_calls"]
        test_diagnostics = trial.get("managed_test_diagnostics", [])
        receipt_path = os.environ.get("CODEGG_H006_JOB_RECEIPT")
        if receipt_path:
            # Preserve the bounded receipt before assertions so failed lifecycle
            # checks still leave enough sanitized evidence to diagnose the job.
            receipt = {
                "schema_version": 1,
                "provider": "local-openai-fixture",
                "source_sha256": trial["source_sha256"],
                "event_observation_state": trial["event_observation_state"],
                "event_gap_count": trial["event_gap_count"],
                "provider_calls": trial["provider_calls"],
                "wall_time_ms": trial["wall_time_ms"],
                "managed_test_diagnostics": test_diagnostics,
                "tool_calls": [{
                    "tool": call["tool"],
                    "status": call["status"],
                    "started_elapsed_ms": call["started_elapsed_ms"],
                    "finished_elapsed_ms": call["finished_elapsed_ms"],
                    "job": call.get("job"),
                } for call in calls],
            }
            path = Path(receipt_path)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")

        submissions = [call for call in calls if call.get("job", {}).get("action") == "submit"]
        observations = [call for call in calls if call.get("tool") == "job"]
        self.assertEqual(len(submissions), 1)
        job_id = submissions[0]["job"]["job_id"]
        self.assertTrue(job_id)
        exact = [call for call in observations if call.get("job", {}).get("job_id") == job_id]
        running = [call for call in exact if call.get("job", {}).get("status") == "running"]
        self.assertGreaterEqual(
            len(running), 2,
            {"calls": calls, "managed_test_diagnostics": test_diagnostics},
        )
        self.assertTrue(_observed_completed_job(exact, job_id), calls)
        first_running_at = running[0]["finished_elapsed_ms"]
        second_running_at = running[1]["finished_elapsed_ms"]
        intervening = [call for call in calls
                       if first_running_at < call.get("started_elapsed_ms", 0) < second_running_at
                       and call.get("tool") in {"write", "edit", "apply_patch"}]
        self.assertGreaterEqual(len(intervening), 1, calls)
        self.assertEqual(trial["event_observation_state"], "measured_no_gap")

    def test_failed_background_job_cannot_satisfy_h006_completion(self):
        calls = [{"tool": "job", "job": {"job_id": "job-123", "status": "failed"}}]
        self.assertFalse(_observed_completed_job(calls, "job-123"))

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
