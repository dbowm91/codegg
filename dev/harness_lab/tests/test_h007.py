from __future__ import annotations

import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock

from dev.harness_lab import autonomy, h007_results

REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
BASE = REPO_ROOT / "dev/harness_lab/fixtures/build_provenance_catalog"
STARTER = BASE / "fixture"
REFERENCE = BASE / "reference"
VERIFIER = REPO_ROOT / "dev/autonomy_fixtures/build-provenance-catalog/verifier"
MANIFEST = REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_h007/cases.jsonl"
S018_V43_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v43"
    / "cases.jsonl"
)
S018_V44_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v44"
    / "cases.jsonl"
)
S018_V54_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v54"
    / "cases.jsonl"
)
S018_V55_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v55"
    / "cases.jsonl"
)
S018_V64_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v64"
    / "cases.jsonl"
)
S018_V65_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v65"
    / "cases.jsonl"
)
S018_V66_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v66"
    / "cases.jsonl"
)
S018_V67_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v67"
    / "cases.jsonl"
)
S018_V68_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v68"
    / "cases.jsonl"
)
S018_V69_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v69"
    / "cases.jsonl"
)
S018_V70_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v70"
    / "cases.jsonl"
)
S018_V71_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v71"
    / "cases.jsonl"
)
S018_V72_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v72"
    / "cases.jsonl"
)
S018_V73_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v73"
    / "cases.jsonl"
)
S018_V74_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v74"
    / "cases.jsonl"
)
S018_V75_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v75"
    / "cases.jsonl"
)
S018_V76_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v76"
    / "cases.jsonl"
)
S018_V77_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v77"
    / "cases.jsonl"
)
S018_V78_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v78"
    / "cases.jsonl"
)
S018_V79_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v79"
    / "cases.jsonl"
)
S018_V80_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v80"
    / "cases.jsonl"
)
S018_V81_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v81"
    / "cases.jsonl"
)
S018_V82_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v82"
    / "cases.jsonl"
)
S018_V83_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v83"
    / "cases.jsonl"
)
S018_V84_MANIFEST = (
    REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v84"
    / "cases.jsonl"
)


def _sha(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _call(command: list[str], *, cwd: pathlib.Path, timeout: int = 300, env=None):
    if command and command[0] == "cargo":
        # These copied fixtures share package names. A private target prevents
        # Cargo from reusing binaries with CARGO_MANIFEST_DIR from an old lane.
        with tempfile.TemporaryDirectory(prefix="h007-cargo-target-") as target:
            isolated_env = dict(os.environ if env is None else env)
            isolated_env["CARGO_TARGET_DIR"] = target
            return subprocess.run(command, cwd=cwd, capture_output=True, text=True,
                                  timeout=timeout, check=False, env=isolated_env)
    return subprocess.run(command, cwd=cwd, capture_output=True, text=True,
                          timeout=timeout, check=False, env=env)


H007_PHASES = (
    "versioned_records", "durable_import", "dependency_index", "provenance_queries",
    "cli_export", "catalog_bundle", "catalog_delta", "catalog_merge",
    "catalog_replication", "catalog_checkpoint", "reconcile", "reverse_impact",
    "catalog_policy", "impact_view_api", "workspace_audit",
)


def _continuation_text(job_id: str, *, state: str = "completed") -> str:
    phases = "\n".join(f"- {phase}" for phase in H007_PHASES)
    return (f"completed_phases:\n{phases}\nnext_phase: none\n"
            f"job_id_priority: {job_id}\njob_state_priority: {state}\n")


class H007CaseAndResultTests(unittest.TestCase):
    def test_incomplete_normal_end_gets_bounded_same_session_followup(self):
        from dev.harness_lab.autonomy_run import (
            H007_MAX_AUTONOMOUS_FOLLOWUP_TURNS,
            _h007_followup_decision,
        )

        self.assertEqual(_h007_followup_decision(
            stop_reason="end_turn", continuation_confirmed=False,
            followup_count=0, native_progress=True,
            remaining_wallclock_seconds=1200.0), "continue")
        self.assertEqual(_h007_followup_decision(
            stop_reason="end_turn", continuation_confirmed=True,
            followup_count=0, native_progress=True,
            remaining_wallclock_seconds=1200.0),
            "continuation_complete")
        self.assertEqual(_h007_followup_decision(
            stop_reason="end_turn", continuation_confirmed=True,
            followup_count=0, native_progress=True,
            remaining_wallclock_seconds=1200.0,
            active_work_span_ms=20 * 60 * 1000,
            minimum_active_work_span_ms=30 * 60 * 1000), "continue")
        self.assertEqual(_h007_followup_decision(
            stop_reason="end_turn", continuation_confirmed=True,
            followup_count=0, native_progress=True,
            remaining_wallclock_seconds=1200.0,
            active_work_span_ms=30 * 60 * 1000,
            minimum_active_work_span_ms=30 * 60 * 1000),
            "continuation_complete")
        self.assertEqual(_h007_followup_decision(
            stop_reason="end_turn", continuation_confirmed=True,
            followup_count=0, native_progress=False,
            remaining_wallclock_seconds=1200.0,
            active_work_span_ms=20 * 60 * 1000,
            minimum_active_work_span_ms=30 * 60 * 1000), "no_native_progress")
        self.assertEqual(_h007_followup_decision(
            stop_reason="provider_timeout", continuation_confirmed=False,
            followup_count=0, native_progress=False,
            remaining_wallclock_seconds=1200.0), "agent_terminal")
        self.assertEqual(_h007_followup_decision(
            stop_reason="end_turn", continuation_confirmed=False,
            followup_count=0, native_progress=False,
            remaining_wallclock_seconds=1200.0), "no_native_progress")
        # Attempt 23 exhausted the former 40-follow-up cap with live progress,
        # 160 provider requests left, and substantial wall-clock remaining.
        self.assertEqual(_h007_followup_decision(
            stop_reason="end_turn", continuation_confirmed=False,
            followup_count=40, native_progress=True,
            remaining_wallclock_seconds=900.0), "continue")
        self.assertEqual(_h007_followup_decision(
            stop_reason="end_turn", continuation_confirmed=False,
            followup_count=H007_MAX_AUTONOMOUS_FOLLOWUP_TURNS - 1,
            native_progress=True,
            remaining_wallclock_seconds=1200.0), "continue")
        self.assertEqual(_h007_followup_decision(
            stop_reason="end_turn", continuation_confirmed=False,
            followup_count=H007_MAX_AUTONOMOUS_FOLLOWUP_TURNS,
            native_progress=True,
            remaining_wallclock_seconds=1200.0), "followup_limit")
        self.assertEqual(_h007_followup_decision(
            stop_reason="end_turn", continuation_confirmed=False,
            followup_count=0, native_progress=True,
            remaining_wallclock_seconds=0.0), "wallclock_cap")
        self.assertEqual(_h007_followup_decision(
            stop_reason="end_turn", continuation_confirmed=False,
            followup_count=0, native_progress=True,
            remaining_wallclock_seconds=300.0), "verifier_reserve")

    def test_followup_prompt_names_missing_phases_and_exact_job_record(self):
        from dev.harness_lab.autonomy_run import _h007_followup_prompt

        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            (fixture / "CONTINUATION.md").write_text(
                "completed_phases:\n- versioned_records\nnext_phase: none\n",
                encoding="utf-8")
            prompt = _h007_followup_prompt(fixture, [{"tool": "read"}], 900, 1200)

        self.assertIn("lists 1 of 15 required stages", prompt)
        self.assertIn("next required phase is `durable_import`", prompt)
        self.assertIn("`job_id_overlap_running: running`", prompt)
        self.assertIn("`job_id_overlap_terminal: completed`", prompt)
        self.assertIn("write one concise final audit and stop", prompt)
        self.assertIn("The exact-ID overlap gate is still missing", prompt)
        self.assertIn("--release -p catalog-index --test corpus", prompt)
        self.assertIn("fixture/catalog-query/src/lib.rs", prompt)
        self.assertIn("job_id: <exact overlap ID>", prompt)
        self.assertIn("job_id_priority", prompt)
        self.assertIn("never use `job wait`", prompt)
        self.assertIn("Do not repeat a passing full workspace test on unchanged source", prompt)
        self.assertIn("use its typed message to correct the arguments", prompt)
        self.assertIn("Defer the single read-only subagent review", prompt)
        self.assertIn("899", prompt)
        self.assertIn("1200-request", prompt)
        self.assertNotIn("750-request", prompt)

        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            (fixture / "CONTINUATION.md").write_text(
                "completed_phases:\n- versioned_records\nnext_phase: durable_import\n",
                encoding="utf-8")
            task_board = _h007_followup_prompt(
                fixture, [{"tool": "task", "task_action": "board"}], 900, 1200)
            spawn_without_ids = _h007_followup_prompt(
                fixture, [{"tool": "task", "task_action": "spawn",
                           "task_spawn_ids_present": False}], 900, 1200)
            spawned = _h007_followup_prompt(
                fixture, [{"tool": "task", "task_action": "spawn",
                           "task_spawn_ids_present": True,
                           "task_spawn_agent": "research",
                           "status": "completed", "failed": False}], 900, 1200)
            verifier_ready = _h007_followup_prompt(
                fixture, [
                    {"tool": "task", "task_action": "spawn",
                     "task_spawn_ids_present": True, "task_spawn_agent": "verifier",
                     "task_spawn_task_id_sha256": "a" * 64,
                     "task_spawn_run_id_sha256": "b" * 64,
                     "status": "completed", "failed": False},
                    {"tool": "task", "task_action": "status",
                     "task_run_id_sha256": "b" * 64,
                     "task_run_status": "completed", "status": "completed",
                     "failed": False},
                ], 900, 1200)
        self.assertIn("Defer the single read-only subagent review", task_board)
        self.assertIn("implement the next missing phase first", task_board)
        self.assertIn("spawn was attempted", spawn_without_ids)
        self.assertNotIn("returned both exact Task and Run IDs", spawn_without_ids)
        self.assertIn("not the required `verifier` agent", spawned)
        self.assertIn("research, explore, or general child is not an independent review", spawned)
        self.assertIn("`agent: verifier`", spawned)
        self.assertIn("exact native `verifier` Run ID was observed `completed`", verifier_ready)
        self.assertIn("call native `task get` exactly once", verifier_ready)

        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            five_phases = ", ".join(H007_PHASES[:5])
            (fixture / "CONTINUATION.md").write_text(
                f"completed_phases: [{five_phases}]\nnext_phase: catalog_bundle\n",
                encoding="utf-8")
            review_due = _h007_followup_prompt(
                fixture, [{"tool": "task", "task_action": "board"}], 900, 1200)
        self.assertIn("required independent review is now due", review_due)
        self.assertIn("`action: spawn`", review_due)
        self.assertIn("`agent: verifier`", review_due)

    def test_followup_distinguishes_refused_run_observation_and_canonical_completion(self):
        from dev.harness_lab.autonomy_run import _h007_followup_prompt

        case = autonomy.load_autonomy_suite(S018_V64_MANIFEST.parent).case(
            "build-provenance-catalog")
        task_id = "task-verifier-35"
        run_id = "run-verifier-35"
        task_digest = hashlib.sha256(task_id.encode("utf-8")).hexdigest()
        run_digest = hashlib.sha256(run_id.encode("utf-8")).hexdigest()
        spawn = {
            "tool": "task", "task_action": "spawn", "task_spawn_ids_present": True,
            "task_spawn_agent": "verifier", "task_spawn_task_id_sha256": task_digest,
            "task_spawn_run_id_sha256": run_digest, "status": "completed", "failed": False,
        }

        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            (fixture / "CONTINUATION.md").write_text(
                "completed_phases: [versioned_records]\nnext_phase: durable_import\n",
                encoding="utf-8")
            refused = _h007_followup_prompt(
                fixture,
                [spawn, {
                    "tool": "task", "task_action": "status",
                    "task_run_id_sha256": run_digest, "status": "failed", "failed": True,
                    "task_error_type": "RunControlUnauthorized",
                }],
                case_prompt=case.prompt)
            observed = _h007_followup_prompt(
                fixture,
                [spawn, {
                    "tool": "task", "task_action": "status",
                    "task_run_id_sha256": run_digest, "task_run_status": "completed",
                    "status": "completed", "failed": False,
                }],
                case_prompt=case.prompt)

        self.assertIn("The exact verifier Run ID status/wait request was refused", refused)
        self.assertIn("Leave `independent_review` pending", refused)
        self.assertIn("never claim `review_status: completed`", refused)
        self.assertIn("call native `task get` exactly once", observed)
        for field in (
            "review_agent: verifier", "review_task: <exact Task ID>",
            "review_run: <exact Run ID>", "review_status: completed",
            "review_findings: <concise evidence-backed sentence>",
        ):
            self.assertIn(field, observed)
        self.assertNotIn("review_task_id:", observed)
        self.assertNotIn("review_run_id:", observed)

    def test_s018_verifier_waits_for_all_non_review_gates_and_spawns_once(self):
        from dev.harness_lab.autonomy_run import (
            H007_S018_PUBLIC_ARTIFACTS, H007_S018_RELEASE_OVERLAP_COMMAND,
            H007_S018_REVIEW_START_ACTIVE_MS, H007_S018_REQUIRED_GATE_ORDER,
            H007_S018_REQUIRED_STAGE_ORDER,
            H007_SUBAGENT_WALL_CLOCK_TIMEOUT_SECS, _h007_followup_prompt,
            _h007_subagent_config,
        )

        case = autonomy.load_autonomy_suite(S018_V66_MANIFEST.parent).case(
            "build-provenance-catalog")
        self.assertIn("the child timeout is now 1,200 seconds", case.prompt)
        self.assertIn("Do not call task wait at any point", case.prompt)
        self.assertIn("at most five concise findings", case.prompt)
        self.assertEqual(H007_S018_REVIEW_START_ACTIVE_MS, 15 * 60 * 1000)
        self.assertEqual(H007_SUBAGENT_WALL_CLOCK_TIMEOUT_SECS, 1200)
        self.assertEqual(_h007_subagent_config("build-provenance-catalog"),
                         {"wall_clock_timeout_secs": 1200})
        self.assertIsNone(_h007_subagent_config("unrelated-case"))
        overlap_id = "550e8400-e29b-41d4-a716-446655440065"
        overlap_calls = [
            {"tool": "test", "job": {"job_id": overlap_id, "action": "submit"}},
            {"tool": "job", "job": {"job_id": overlap_id, "status": "running"}},
            {"tool": "edit"},
            {"tool": "job", "job": {"job_id": overlap_id, "status": "completed"}},
            {"tool": "write"},
        ]

        def write_public_artifacts(root: pathlib.Path) -> None:
            for relative in H007_S018_PUBLIC_ARTIFACTS:
                artifact = root / relative
                artifact.parent.mkdir(parents=True, exist_ok=True)
                artifact.write_text("{}\n" if artifact.suffix == ".json" else "evidence\n",
                                    encoding="utf-8")

        def write_note(root: pathlib.Path, phases: tuple[str, ...], *,
                       pending_gate: str) -> None:
            phase_lines = "\n".join(f"- {phase}: complete" for phase in phases)
            gate_lines = "\n".join(
                f"- {gate}: {'pending' if gate == pending_gate else 'complete'}"
                for gate in H007_S018_REQUIRED_GATE_ORDER)
            next_phase = ("none" if len(phases) == len(H007_S018_REQUIRED_STAGE_ORDER)
                          else H007_S018_REQUIRED_STAGE_ORDER[len(phases)])
            note = (
                f"completed_phases:\n{phase_lines}\nnext_phase: {next_phase}\n"
                f"job_command_overlap: {H007_S018_RELEASE_OVERLAP_COMMAND}\n"
                f"job_id_overlap: {overlap_id}\njob_id_overlap_running: running\n"
                f"job_id_overlap_terminal: completed\ngates:\n{gate_lines}\n"
            )
            (root / "CONTINUATION.md").write_text(note, encoding="utf-8")

        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            write_public_artifacts(fixture)
            write_note(fixture, H007_S018_REQUIRED_STAGE_ORDER[:5],
                       pending_gate="workspace_qualification")
            early = _h007_followup_prompt(
                fixture, overlap_calls, 1600, 1200, case.prompt)
            self.assertIn("Do not call the native `task` tool or spawn a reviewer before",
                          early)
            self.assertIn("acceptance_audit", early)
            self.assertNotIn("independent review is now due", early)

            write_note(fixture, H007_S018_REQUIRED_STAGE_ORDER[:6],
                       pending_gate="workspace_qualification")
            due = _h007_followup_prompt(
                fixture, overlap_calls, 1600, 1200, case.prompt)
            self.assertIn("Acceptance for `workspace_qualification`", due)
            self.assertIn("Do not call the native `task` tool or spawn a reviewer before",
                          due)
            self.assertIn("Start the one read-only verifier", due)

            task_id = "task-verifier-66"
            run_id = "run-verifier-66"
            task_digest = hashlib.sha256(task_id.encode("utf-8")).hexdigest()
            run_digest = hashlib.sha256(run_id.encode("utf-8")).hexdigest()
            active_verifier = _h007_followup_prompt(
                fixture,
                overlap_calls + [{
                    "tool": "task", "task_action": "spawn", "task_spawn_ids_present": True,
                    "task_spawn_agent": "verifier", "task_spawn_task_id_sha256": task_digest,
                    "task_spawn_run_id_sha256": run_digest, "status": "completed",
                    "failed": False,
                }, {
                    "tool": "task", "task_action": "status", "task_run_id_sha256": run_digest,
                    "task_run_status": "running", "status": "completed", "failed": False,
                }, {
                    "tool": "task", "task_action": "wait", "task_run_id_sha256": run_digest,
                    "task_run_status": "running", "status": "completed", "failed": False,
                }],
                1600, 1200, case.prompt)
            self.assertIn("At final audit only, one `task wait`", active_verifier)
            self.assertIn("Never retrieve a live result", active_verifier)
            self.assertIn("after at least 3 more non-task tool operations", active_verifier)
            self.assertIn("Do not write successful review fields", active_verifier)
            self.assertNotIn("native `task status` or bounded `task wait`", active_verifier)

            reviewer_poll_due = _h007_followup_prompt(
                fixture,
                overlap_calls + [{
                    "tool": "task", "task_action": "spawn", "task_spawn_ids_present": True,
                    "task_spawn_agent": "verifier", "task_spawn_task_id_sha256": task_digest,
                    "task_spawn_run_id_sha256": run_digest, "status": "completed",
                    "failed": False,
                }, {
                    "tool": "task", "task_action": "status", "task_run_id_sha256": run_digest,
                    "task_run_status": "running", "status": "completed", "failed": False,
                }, {"tool": "read"}, {"tool": "edit"}, {"tool": "write"}],
                1600, 1200, case.prompt)
            self.assertIn("Make one native `task status` call now with that exact Run ID",
                          reviewer_poll_due)
            self.assertIn("If it reports `completed`, call `task get` exactly once",
                          reviewer_poll_due)

            completed_review = _h007_followup_prompt(
                fixture,
                overlap_calls + [{
                    "tool": "task", "task_action": "spawn", "task_spawn_ids_present": True,
                    "task_spawn_agent": "verifier", "task_spawn_task_id_sha256": task_digest,
                    "task_spawn_run_id_sha256": run_digest, "status": "completed",
                    "failed": False,
                }, {
                    "tool": "task", "task_action": "status",
                    "task_run_id_sha256": run_digest, "task_run_status": "completed",
                    "status": "completed", "failed": False,
                }, {
                    "tool": "task", "task_action": "get", "task_id_sha256": task_digest,
                    "task_result_task_id_sha256": task_digest,
                    "task_result_run_id_sha256": run_digest,
                    "task_result_run_status": "completed", "status": "completed",
                    "failed": False,
                }, {"tool": "read"}, {"tool": "edit"}, {"tool": "write"}],
                1600, 1200, case.prompt)
            self.assertIn("one-shot native `verifier` lifecycle is complete", completed_review)
            self.assertIn("Treat those fields as the durable review receipt", completed_review)
            self.assertIn("Do not invoke the native `task` tool again for this reviewer",
                          completed_review)
            self.assertIn("including at final audit", completed_review)
            self.assertNotIn("call now with that exact Run ID", completed_review)

            duplicated_review = _h007_followup_prompt(
                fixture,
                overlap_calls + [{
                    "tool": "task", "task_action": "spawn", "task_spawn_ids_present": True,
                    "task_spawn_agent": "verifier", "task_spawn_task_id_sha256": task_digest,
                    "task_spawn_run_id_sha256": run_digest, "status": "completed",
                    "failed": False,
                }, {
                    "tool": "task", "task_action": "status",
                    "task_run_id_sha256": run_digest, "task_run_status": "completed",
                    "status": "completed", "failed": False,
                }, {
                    "tool": "task", "task_action": "get", "task_id_sha256": task_digest,
                    "task_result_task_id_sha256": task_digest,
                    "task_result_run_id_sha256": run_digest,
                    "task_result_run_status": "completed", "status": "completed",
                    "failed": False,
                }, {
                    "tool": "task", "task_action": "get", "task_id_sha256": task_digest,
                    "task_result_task_id_sha256": task_digest,
                    "task_result_run_id_sha256": run_digest,
                    "task_result_run_status": "completed", "status": "completed",
                    "failed": False,
                }],
                1600, 1200, case.prompt)
            self.assertIn("fetched more than once", duplicated_review)
            self.assertIn("single-fetch review gate is now unsatisfied", duplicated_review)
            self.assertIn("keep `independent_review` pending", duplicated_review)

            premature_get_calls = overlap_calls + [{
                "tool": "task", "task_action": "spawn", "task_spawn_ids_present": True,
                "task_spawn_agent": "verifier", "task_spawn_task_id_sha256": task_digest,
                "task_spawn_run_id_sha256": run_digest, "status": "completed",
                "failed": False,
            }, {
                "tool": "task", "task_action": "get", "task_id_sha256": task_digest,
                "task_result_task_id_sha256": task_digest,
                "task_result_run_id_sha256": run_digest,
                "task_result_run_status": "completed", "status": "completed",
                "failed": False,
            }]
            premature_get = _h007_followup_prompt(
                fixture, premature_get_calls, 1600, 1200, case.prompt)
            self.assertIn("happened before an exact completed Run-status observation",
                          premature_get)
            self.assertIn("Make one native `task status` call with the exact verifier Run ID",
                          premature_get)
            self.assertIn("If it reports completed, the next required review call is one",
                          premature_get)
            self.assertNotIn("Do not retrieve it again or spawn a replacement", premature_get)

            status_after_premature_get = _h007_followup_prompt(
                fixture, premature_get_calls + [{
                    "tool": "task", "task_action": "status",
                    "task_run_id_sha256": run_digest, "task_run_status": "completed",
                    "status": "completed", "failed": False,
                }], 1600, 1200, case.prompt)
            self.assertIn("An earlier completed `get` made before this status does not satisfy",
                          status_after_premature_get)
            self.assertIn("Do not call get again after this ordered result",
                          status_after_premature_get)

            failed_spawn = _h007_followup_prompt(
                fixture,
                overlap_calls + [{
                    "tool": "task", "task_action": "spawn", "task_spawn_agent": "verifier",
                    "task_spawn_ids_present": False, "status": "failed", "failed": True,
                }],
                1600, 1200, case.prompt)
            self.assertIn("spawn was already attempted", failed_spawn)
            self.assertIn("Do not retry it or create a replacement", failed_spawn)
            self.assertNotIn("make exactly one native task spawn", failed_spawn)
            self.assertNotIn("review_status: completed", failed_spawn)

    def test_followup_includes_named_phase_acceptance_from_public_case_prompt(self):
        from dev.harness_lab.autonomy_run import (
            _h007_followup_prompt, _h007_phase_requirement,
        )

        case = autonomy.load_autonomy_suite(S018_V43_MANIFEST.parent).case(
            "build-provenance-catalog")
        detail = _h007_phase_requirement(case.prompt, "durable_import")
        self.assertIsNotNone(detail)
        self.assertIn("deterministic, atomic, idempotent imports", detail)
        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            (fixture / "CONTINUATION.md").write_text(
                "completed_phases: [versioned_records]\nnext_phase: durable_import\n",
                encoding="utf-8")
            prompt = _h007_followup_prompt(fixture, case_prompt=case.prompt)
        self.assertIn("Make a functional source change and an assertion-bearing test", prompt)
        self.assertIn("Acceptance for `durable_import`", prompt)
        self.assertIn("deterministic, atomic, idempotent imports", prompt)

    def test_followup_prompt_targets_missing_overlap_without_repeating_live_job(self):
        from dev.harness_lab.autonomy_run import _h007_followup_prompt

        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            (fixture / "CONTINUATION.md").write_text(
                "completed_phases: [versioned_records, durable_import, dependency_index]\n"
                "next_phase: provenance_queries\n", encoding="utf-8")

            running = _h007_followup_prompt(fixture, [
                {"tool": "test", "job": {"job_id": "job-live", "status": "running"}},
                {"tool": "job", "job": {"job_id": "job-live", "status": "running"}},
            ])
            self.assertIn("An exact submitted background test is still running", running)
            self.assertIn("ID: job-live", running)
            self.assertIn("Do not submit another test", running)

            completed_without_overlap = _h007_followup_prompt(fixture, [
                {"tool": "test", "job": {"job_id": "job-done", "status": "running"}},
                {"tool": "job", "job": {"job_id": "job-done", "status": "running"}},
                {"tool": "job", "job": {"job_id": "job-done", "status": "completed"}},
            ])
            self.assertIn("no productive native work occurred between", completed_without_overlap)

            overlapped = _h007_followup_prompt(fixture, [
                {"tool": "test", "job": {"job_id": "job-overlap", "status": "running"}},
                {"tool": "job", "job": {"job_id": "job-overlap", "status": "running"}},
                {"tool": "edit"},
                {"tool": "job", "job": {"job_id": "job-overlap", "status": "completed"}},
            ])
            self.assertIn("already proves one exact submitted test ID", overlapped)
            self.assertIn("do not launch a replacement overlap job", overlapped)

    def test_followup_prompt_counts_inline_completed_phase(self):
        from dev.harness_lab.autonomy_run import _h007_followup_prompt

        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            (fixture / "CONTINUATION.md").write_text(
                "completed_phases: [versioned_records]\nnext_phase: durable_import\n",
                encoding="utf-8")
            prompt = _h007_followup_prompt(fixture)

        self.assertIn("lists 1 of 15 required stages", prompt)
        self.assertIn("next required phase is `durable_import`", prompt)

    def test_continuation_accepts_inline_phase_list(self):
        from dev.harness_lab.autonomy_run import _h007_continuation_confirmed

        job_id = "job-inline-17"
        phases = ", ".join(H007_PHASES)
        note = (f"completed_phases: [{phases}]\nnext_phase: none\n"
                f"job_id_priority: {job_id}\njob_state_priority: completed\n")
        calls = [
            {"tool": "job", "job": {"job_id": job_id, "status": "completed"}},
            {"tool": "edit"},
        ]
        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            (fixture / "CONTINUATION.md").write_text(note, encoding="utf-8")
            self.assertTrue(_h007_continuation_confirmed(calls, fixture))

    def test_continuation_accepts_overlap_terminal_schema_emitted_by_live_prompt(self):
        from dev.harness_lab.autonomy_run import _h007_continuation_confirmed

        job_id = "550e8400-e29b-41d4-a716-446655440000"
        phases = "\n".join(f"- {phase}" for phase in H007_PHASES)
        note = (f"completed_phases:\n{phases}\nnext_phase: none\n"
                f"job_id_overlap: {job_id}\n"
                "job_id_overlap_running: running\n"
                "job_id_overlap_terminal: completed\n")
        calls = [
            {"tool": "test", "job": {"job_id": job_id, "status": "running"}},
            {"tool": "edit"},
            {"tool": "job", "job": {"job_id": job_id, "status": "completed"}},
            {"tool": "write"},
        ]
        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            continuation = fixture / "CONTINUATION.md"
            continuation.write_text(note, encoding="utf-8")
            self.assertTrue(_h007_continuation_confirmed(calls, fixture))

            continuation.write_text(note.replace("terminal: completed", "terminal: failed"),
                                    encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(calls, fixture))

            continuation.write_text(note.replace(job_id, "different-job"), encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(calls, fixture))

            continuation.write_text(note, encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(calls[:-1], fixture))

    def test_documented_phases_rejects_malformed_inline_list(self):
        self.assertIsNone(h007_results.documented_phases(
            "completed_phases: [versioned_records,]\nnext_phase: durable_import\n"))

    def test_documented_block_phases_accept_evidence_and_ignore_pending_entries(self):
        note = (
            "completed_phases:\n"
            "- versioned_records: v1/v2 parsing and checksum tests passed\n"
            "- durable_import: completed\n"
            "- dependency_index: pending\n"
            "- versioned_records: final audit confirmed\n"
            "next_phase: dependency_index\n"
        )
        self.assertEqual(h007_results.documented_phases(note),
                         ["versioned_records", "durable_import"])
        malformed = note.replace("- versioned_records: v1/v2 parsing and checksum tests passed",
                                 "- versioned records: malformed name")
        self.assertIsNone(h007_results.documented_phases(malformed))

    def test_documented_stages_tracks_latest_status_and_rejects_malformed_rows(self):
        note = (
            "required_stages:\n"
            "- versioned_records: completed\n"
            "- durable_import: pending\n"
            "- durable_import: completed (repair verified)\n"
            "next_stage: dependency_index\n"
        )
        self.assertEqual(h007_results.documented_stages(note), {
            "versioned_records": "completed", "durable_import": "completed",
        })
        self.assertIsNone(h007_results.documented_stages(
            note.replace("- durable_import: pending", "- durable import: pending")))
        self.assertIsNone(h007_results.documented_stages(
            note + "required_stages:\n- dependency_index: completed\n"))

    def test_s019_preserves_public_h007_phases_and_separates_contract_phases(self):
        from dev.harness_lab.autonomy_run import (
            H007_S018_CONTRACT_PHASE_ORDER, H007_S018_PUBLIC_ARTIFACTS,
            H007_S018_RELEASE_OVERLAP_COMMAND, H007_S018_REQUIRED_GATE_ORDER,
            H007_S018_REQUIRED_STAGE_ORDER, _h007_continuation_confirmed,
            _h007_followup_prompt, _h007_required_contract_phases,
            _h007_required_phases, _h007_required_stages,
        )

        prompt = (
            "Keep the seven required stages named exactly in completed_phases and "
            "required_stages/next_stage. Track the five exact contract.json implementation "
            "phases separately in contract_phases/next_contract_phase. FINAL CHECKPOINT"
        )
        self.assertEqual(_h007_required_phases(prompt), H007_S018_REQUIRED_STAGE_ORDER)
        self.assertEqual(_h007_required_contract_phases(prompt), H007_S018_CONTRACT_PHASE_ORDER)
        self.assertEqual(_h007_required_stages(prompt), H007_S018_REQUIRED_STAGE_ORDER)

        overlap_id = "550e8400-e29b-41d4-a716-446655440099"
        task_id = "task-verifier-99"
        run_id = "run-verifier-99"
        task_digest = hashlib.sha256(task_id.encode("utf-8")).hexdigest()
        run_digest = hashlib.sha256(run_id.encode("utf-8")).hexdigest()
        phase_lines = "\n".join(
            f"- {phase}: completed" for phase in H007_S018_REQUIRED_STAGE_ORDER)
        contract_phase_lines = "\n".join(
            f"- {phase}: completed" for phase in H007_S018_CONTRACT_PHASE_ORDER)
        stage_lines = "\n".join(
            f"- {stage}: completed" for stage in H007_S018_REQUIRED_STAGE_ORDER)
        gate_lines = "\n".join(
            f"- {gate}: completed" for gate in H007_S018_REQUIRED_GATE_ORDER)
        note = (
            f"completed_phases:\n{phase_lines}\nnext_phase: none\n"
            f"contract_phases:\n{contract_phase_lines}\nnext_contract_phase: none\n"
            f"required_stages:\n{stage_lines}\nnext_stage: none\n"
            f"job_command_overlap: {H007_S018_RELEASE_OVERLAP_COMMAND}\n"
            f"job_id_overlap: {overlap_id}\njob_id_overlap_running: running\n"
            "job_id_overlap_terminal: completed\n"
            "job_id_workspace_tests: workspace-final-01\n"
            "job_state_workspace_tests: completed\n"
            "job_id_clippy: clippy-final-01\n"
            "job_state_clippy: completed\n"
            f"review_agent: verifier\nreview_task: {task_id}\nreview_run: {run_id}\n"
            "review_status: completed (retrieved using native task get)\n"
            "review_findings: public requirement matrix was independently checked\n"
            f"gates:\n{gate_lines}\n"
        )
        calls = [
            {"tool": "test", "job": {"job_id": overlap_id, "action": "submit",
                                         "status": "queued"}},
            {"tool": "job", "job": {"job_id": overlap_id, "status": "running"}},
            {"tool": "edit"},
            {"tool": "job", "job": {"job_id": overlap_id, "status": "completed"}},
            {"tool": "write"},
            {"tool": "task", "task_action": "spawn", "task_spawn_ids_present": True,
             "task_spawn_task_id_sha256": task_digest,
             "task_spawn_run_id_sha256": run_digest, "task_spawn_agent": "verifier",
             "status": "completed", "failed": False},
            {"tool": "task", "task_action": "status",
             "task_run_id_sha256": run_digest, "task_run_status": "completed",
             "status": "completed", "failed": False},
            {"tool": "task", "task_action": "get", "task_id_sha256": task_digest,
             "task_result_task_id_sha256": task_digest,
             "task_result_run_id_sha256": run_digest,
             "task_result_run_status": "completed", "status": "completed",
             "failed": False},
            {"tool": "test", "job": {"job_id": "workspace-final-01", "action": "submit",
                                        "status": "queued"}},
            {"tool": "job", "job": {"job_id": "workspace-final-01", "status": "completed"}},
            {"tool": "test", "job": {"job_id": "clippy-final-01", "action": "submit",
                                        "status": "queued"}},
            {"tool": "job", "job": {"job_id": "clippy-final-01", "status": "completed"}},
        ]
        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            (fixture / "CONTINUATION.md").write_text(note, encoding="utf-8")
            for relative in H007_S018_PUBLIC_ARTIFACTS:
                artifact = fixture / relative
                artifact.parent.mkdir(parents=True, exist_ok=True)
                artifact.write_text("{}\n" if artifact.suffix == ".json" else "evidence\n",
                                    encoding="utf-8")
            self.assertTrue(_h007_continuation_confirmed(
                calls, fixture, case_prompt=prompt))

            wrong_phase_ledger = note.replace(
                "- cli_contract: completed", "- cli_export: pending")
            (fixture / "CONTINUATION.md").write_text(wrong_phase_ledger, encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, case_prompt=prompt))

            wrong_contract_phase = note.replace(
                "- cli_export: completed", "- catalog_merge: completed")
            (fixture / "CONTINUATION.md").write_text(wrong_contract_phase, encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, case_prompt=prompt))

            missing_contract_phase = note.replace("next_contract_phase: none",
                                                  "next_contract_phase: cli_export")
            (fixture / "CONTINUATION.md").write_text(missing_contract_phase, encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, case_prompt=prompt))

            incomplete_stage = note.replace(
                "- cli_contract: completed", "- cli_contract: pending")
            (fixture / "CONTINUATION.md").write_text(incomplete_stage, encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, case_prompt=prompt))

            partial_stages = "\n".join(
                f"- {stage}: {'completed' if index < 4 else 'pending'}"
                for index, stage in enumerate(H007_S018_REQUIRED_STAGE_ORDER))
            partial = note.replace(stage_lines, partial_stages).replace(
                "next_phase: none", "next_phase: cli_contract").replace(
                    "next_stage: none", "next_stage: cli_contract")
            (fixture / "CONTINUATION.md").write_text(partial, encoding="utf-8")
            followup = _h007_followup_prompt(fixture, calls, case_prompt=prompt)
            self.assertIn("4 of 7 required stages", followup)
            self.assertIn("next required phase is `cli_contract`", followup)
            self.assertIn("Do not trust `next_stage: none`", followup)
            self.assertNotIn("Do not trust `next_phase: none` while any are missing", followup)
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, case_prompt=prompt))

    def test_continuation_requires_completed_public_record_and_matching_job(self):
        from dev.harness_lab.autonomy_run import _h007_continuation_confirmed

        failed_run_calls = [
            {"tool": "job", "job": {"job_id": "failed-1", "status": "failed"}},
            {"tool": "edit"},
        ]
        incomplete = """completed_phases:\n- none\nnext_phase: versioned_records\njob_id: pending\n"""
        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            (fixture / "CONTINUATION.md").write_text(incomplete, encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(failed_run_calls, fixture))

            completed_job = "job-17"
            complete = _continuation_text(completed_job, state="completed (release workspace)")
            (fixture / "CONTINUATION.md").write_text(complete, encoding="utf-8")
            calls = [
                {"tool": "job", "job": {"job_id": completed_job, "status": "completed"}},
                {"tool": "edit"},
            ]
            self.assertTrue(_h007_continuation_confirmed(calls, fixture))
            calls.pop()
            self.assertFalse(_h007_continuation_confirmed(calls, fixture))
            calls.append({"tool": "edit"})
            calls[0]["job"]["job_id"] = "different-job"
            self.assertFalse(_h007_continuation_confirmed(calls, fixture))
            calls[0]["job"]["job_id"] = completed_job
            (fixture / "CONTINUATION.md").write_text(
                complete.replace("next_phase: none", "next_phase: workspace_audit"),
                encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(calls, fixture))
            (fixture / "CONTINUATION.md").write_text(
                complete.replace("- versioned_records", "- none"), encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(calls, fixture))
            partial = ("completed_phases:\n- versioned_records\n- durable_import\n"
                       f"next_phase: none\njob_id_priority: {completed_job}\n"
                       "job_state_priority: completed\n")
            (fixture / "CONTINUATION.md").write_text(partial, encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(calls, fixture))

    def test_continuation_gate_checklist_blocks_false_completion(self):
        from dev.harness_lab.autonomy_run import (
            _h007_continuation_confirmed, _h007_followup_prompt,
        )

        job_id = "job-gates-1"
        gate_names = (*H007_PHASES, *h007_results.H007_REQUIRED_CONTINUATION_GATE_ORDER)
        gate_block = "gates:\n" + "\n".join(
            f"- {name}: completed" for name in gate_names) + "\n"
        note = _continuation_text(job_id) + gate_block
        calls = [
            {"tool": "job", "job": {"job_id": job_id, "status": "completed"}},
            {"tool": "edit"},
        ]
        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            continuation = fixture / "CONTINUATION.md"
            continuation.write_text(note, encoding="utf-8")
            self.assertTrue(_h007_continuation_confirmed(
                calls, fixture, require_gate_block=True))

            pending = note.replace(
                "- acceptance_audit: completed", "- acceptance_audit: pending")
            continuation.write_text(pending, encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, require_gate_block=True))
            self.assertFalse(_h007_continuation_confirmed(calls, fixture))
            prompt = _h007_followup_prompt(
                fixture, calls,
                case_prompt=("Keep the seven required stages named exactly. "
                             "FINAL CHECKPOINT requires completed gates"))
            self.assertIn("acceptance_audit", prompt)
            self.assertIn("do not set `next_phase: none`", prompt)

            missing = note.replace("- harness_probes: completed\n", "")
            continuation.write_text(missing, encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, require_gate_block=True))

    def test_review_starts_at_active_time_threshold_while_earlier_gates_remain_pending(self):
        from dev.harness_lab.autonomy_run import (
            H007_S018_REVIEW_START_ACTIVE_MS,
            H007_S018_REQUIRED_STAGE_ORDER,
            _h007_followup_prompt,
        )

        stage_lines = "\n".join(
            f"- {stage}: {'complete' if index < 6 else 'pending'}"
            for index, stage in enumerate(H007_S018_REQUIRED_STAGE_ORDER))
        completed_stage_lines = "\n".join(
            f"- {stage}" for stage in H007_S018_REQUIRED_STAGE_ORDER[:6])
        contract_lines = "\n".join(
            f"- {phase}: complete" for phase in (
                "versioned_records", "durable_import", "dependency_index",
                "provenance_queries", "cli_export"))
        gate_names = (
            "starter_repair_format", "starter_repair_store", "versioned_records",
            "durable_import", "dependency_index", "release_scale_overlap",
            "provenance_queries", "cli_contract", "acceptance_audit",
            "bounded_probes", "workspace_qualification", "independent_review")
        gate_lines = "\n".join(
            f"- {gate}: {'pending' if gate in {'bounded_probes', 'workspace_qualification', 'independent_review'} else 'complete'}"
            for gate in gate_names)
        note = (
            "completed_phases:\n" + completed_stage_lines +
            "\nnext_phase: workspace_qualification\ncontract_phases:\n" +
            contract_lines +
            "\nnext_contract_phase: none\nrequired_stages:\n" + stage_lines +
            "\nnext_stage: workspace_qualification\ngates:\n" + gate_lines +
            "\nnext_gate: bounded_probes\n")
        case_prompt = (
            "FINAL CHECKPOINT. Keep the seven required stages named exactly. "
            "Track contract_phases and required_stages separately.")

        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            (fixture / "CONTINUATION.md").write_text(note, encoding="utf-8")
            before_threshold = _h007_followup_prompt(
                fixture, case_prompt=case_prompt,
                active_work_span_ms=H007_S018_REVIEW_START_ACTIVE_MS - 1)
            at_threshold = _h007_followup_prompt(
                fixture, case_prompt=case_prompt,
                active_work_span_ms=H007_S018_REVIEW_START_ACTIVE_MS)
            finalizing = _h007_followup_prompt(
                fixture, case_prompt=case_prompt,
                active_work_span_ms=50 * 60 * 1000,
                remaining_work_seconds=540)

        self.assertIn("Do not call the native `task` tool or spawn a reviewer before",
                      before_threshold)
        self.assertIn("The 15-minute independent-review start threshold has been reached",
                      at_threshold)
        self.assertIn("even if acceptance, probe, workspace", at_threshold)
        self.assertIn("continue the next missing dependent stage", at_threshold)
        self.assertNotIn("Do not call the native `task` tool or spawn a reviewer before",
                         at_threshold)
        self.assertIn("Only about 9 minutes remain before the reserved verifier window",
                      finalizing)
        self.assertIn("Stop optional edits and exploration now", finalizing)

    def test_gate_evidence_suffixes_and_latest_status_are_parsed(self):
        from dev.harness_lab.autonomy_run import _h007_continuation_confirmed

        job_id = "job-gate-evidence-1"
        gate_lines = [
            f"- {name}: complete (independent evidence recorded)"
            for name in h007_results.H007_REQUIRED_CONTINUATION_GATE_ORDER
        ]
        gate_lines.extend((
            "- release_overlap: pending (initial note before job completion)",
            "- release_overlap: complete (exact job reached running and completed)",
        ))
        note = _continuation_text(job_id) + "gates:\n" + "\n".join(gate_lines) + "\n"
        parsed = h007_results.documented_gates(note)
        self.assertIsNotNone(parsed)
        self.assertEqual(parsed["release_overlap"], "complete")

        calls = [
            {"tool": "job", "job": {"job_id": job_id, "status": "completed"}},
            {"tool": "edit"},
        ]
        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            continuation = fixture / "CONTINUATION.md"
            continuation.write_text(note, encoding="utf-8")
            self.assertTrue(_h007_continuation_confirmed(
                calls, fixture, require_gate_block=True))

            continuation.write_text(
                note + "- release_overlap: pending (later regression)\n",
                encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, require_gate_block=True))

            continuation.write_text(
                note.replace("- acceptance_audit: complete (independent evidence recorded)",
                              "- acceptance audit: complete (malformed gate name)"),
                encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, require_gate_block=True))

    def test_s018_live_contract_uses_prompt_stages_gates_and_public_evidence(self):
        from dev.harness_lab.autonomy_run import (
            H007_S018_RELEASE_OVERLAP_COMMAND, H007_S018_REQUIRED_GATE_ORDER,
            H007_S018_REQUIRED_STAGE_ORDER, _h007_continuation_confirmed,
            _h007_followup_prompt, _h007_phase_requirement,
        )

        case = autonomy.load_autonomy_suite(S018_V67_MANIFEST.parent).case(
            "build-provenance-catalog")
        self.assertIn("Keep the seven required stages named exactly", case.prompt)
        self.assertIn("native task tool is for delegated work and has no todo/todowrite action",
                      case.prompt)
        self.assertNotIn("built-in todo/task tool", case.prompt)
        self.assertIn("job_command_overlap", case.prompt)
        self.assertIn("job_id_overlap_running: running", case.prompt)
        self.assertIn("job_id_overlap_terminal: completed", case.prompt)
        self.assertIn("without submitting another release test", case.prompt)
        initial_gates = h007_results.documented_gates(
            (REPO_ROOT / "dev/autonomy_fixtures/build-provenance-catalog/fixture/CONTINUATION.md")
            .read_text(encoding="utf-8"))
        self.assertEqual(set(initial_gates or {}), set(H007_S018_REQUIRED_GATE_ORDER))
        self.assertEqual(set((initial_gates or {}).values()), {"pending"})
        self.assertEqual(H007_S018_REQUIRED_STAGE_ORDER, (
            "versioned_records", "durable_import", "dependency_index", "provenance_queries",
            "cli_contract", "acceptance_audit", "workspace_qualification",
        ))
        self.assertNotIn("catalog_bundle", H007_S018_REQUIRED_STAGE_ORDER)
        self.assertIn("actual catalog-cli end-to-end coverage",
                      _h007_phase_requirement(case.prompt, "cli_contract") or "")

        overlap_id = "550e8400-e29b-41d4-a716-446655440000"
        phases = "\n".join(
            f"- {name}: implementation and assertion-bearing tests recorded"
            for name in H007_S018_REQUIRED_STAGE_ORDER)
        phases += "\n- versioned_records: final audit confirmed"
        gates = "\n".join(f"- {name}: complete" for name in H007_S018_REQUIRED_GATE_ORDER)
        task_digest = hashlib.sha256(b"task-11").hexdigest()
        run_digest = hashlib.sha256(b"run-12").hexdigest()
        note = (
            f"completed_phases:\n{phases}\nnext_phase: none\n"
            f"job_command_overlap: {H007_S018_RELEASE_OVERLAP_COMMAND}\n"
            f"job_id_overlap: {overlap_id}\njob_id_overlap_running: running\n"
            "job_id_overlap_terminal: completed\n"
            "job_id_workspace_tests: workspace-sweep-02\n"
            "job_state_workspace_tests: completed\n"
            "job_id_clippy: clippy-sweep-03\n"
            "job_state_clippy: completed\n"
            "review_agent: verifier\nreview_task: task-11\nreview_run: run-12\n"
            "review_status: completed (retrieved using native task get)\n"
            "review_findings: one reproducible finding fixed\n"
            f"gates:\n{gates}\n"
        )
        calls = [
            {"tool": "test", "job": {"job_id": overlap_id, "action": "submit",
                                        "status": "queued"}},
            {"tool": "job", "job": {"job_id": overlap_id, "status": "running"}},
            {"tool": "edit"},
            {"tool": "job", "job": {"job_id": overlap_id, "status": "completed"}},
            {"tool": "write"},
            {"tool": "task", "task_action": "spawn", "task_spawn_ids_present": True,
             "task_spawn_task_id_sha256": task_digest,
             "task_spawn_run_id_sha256": run_digest, "task_spawn_agent": "verifier",
             "status": "completed", "failed": False},
            {"tool": "task", "task_action": "status",
             "task_run_id_sha256": run_digest,
             "task_run_status": "completed", "status": "completed", "failed": False},
            {"tool": "task", "task_action": "get", "task_id_sha256": task_digest,
             "task_result_task_id_sha256": task_digest,
             "task_result_run_id_sha256": run_digest,
             "task_result_run_status": "completed",
             "status": "completed", "failed": False},
            {"tool": "test", "job": {"job_id": "workspace-sweep-02",
                                        "action": "submit", "status": "queued"}},
            {"tool": "job", "job": {"job_id": "workspace-sweep-02",
                                       "status": "completed"}},
            {"tool": "test", "job": {"job_id": "clippy-sweep-03",
                                        "action": "submit", "status": "queued"}},
            {"tool": "job", "job": {"job_id": "clippy-sweep-03",
                                       "status": "completed"}},
        ]
        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            (fixture / "CONTINUATION.md").write_text(note, encoding="utf-8")
            (fixture / "PUBLIC_ACCEPTANCE_AUDIT.md").write_text("audited\n", encoding="utf-8")
            (fixture / "HARNESS_PROBE_REPORT.json").write_text(
                json.dumps({"probe_results": []}), encoding="utf-8")
            tools_dir = fixture / "tools"
            tools_dir.mkdir()
            (tools_dir / "catalog_probe.py").write_text("# self-test\n", encoding="utf-8")

            self.assertTrue(_h007_continuation_confirmed(
                calls, fixture, case_prompt=case.prompt))
            from dev.harness_lab.autonomy_run import (
                _h007_public_artifact_status,
            )
            report_path = fixture / "HARNESS_PROBE_REPORT.json"
            report_path.write_text("{malformed report", encoding="utf-8")
            artifact_status = _h007_public_artifact_status(fixture)
            self.assertTrue(artifact_status["PUBLIC_ACCEPTANCE_AUDIT.md"])
            self.assertFalse(artifact_status["HARNESS_PROBE_REPORT.json"])
            self.assertTrue(artifact_status["tools/catalog_probe.py"])
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, case_prompt=case.prompt))
            invalid_report_followup = _h007_followup_prompt(
                fixture, calls, case_prompt=case.prompt)
            self.assertIn("valid public artifact `HARNESS_PROBE_REPORT.json`",
                          invalid_report_followup)
            self.assertIn("must contain a valid JSON object or array",
                          invalid_report_followup)
            self.assertIn("python3 -m json.tool HARNESS_PROBE_REPORT.json",
                          invalid_report_followup)
            report_path.write_text(json.dumps({"probe_results": []}), encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(
                calls[:-1], fixture, case_prompt=case.prompt))
            pending_jobs = h007_results.unobserved_test_job_ids(calls[:-1])
            self.assertEqual(pending_jobs, ["clippy-sweep-03"])
            pending_followup = _h007_followup_prompt(
                fixture, calls[:-1], case_prompt=case.prompt,
                active_work_span_ms=10 * 60 * 1000)
            self.assertIn("`clippy-sweep-03`", pending_followup)
            self.assertIn("Do not resubmit pending work", pending_followup)
            self.assertIn("Submit no new test job while any earlier test job is nonterminal",
                          pending_followup)
            self.assertIn("next native call must be `job status`", pending_followup)
            serial_followup = _h007_followup_prompt(
                fixture, calls, case_prompt=case.prompt,
                active_work_span_ms=10 * 60 * 1000)
            self.assertIn("Submit no second test job until the first is terminal",
                          serial_followup)
            self.assertIn("required final workspace and Clippy jobs completed successfully",
                          serial_followup)

            missing_overlap_states = note.replace(
                "job_id_overlap_running: running\n", "").replace(
                    "job_id_overlap_terminal: completed\n", "")
            (fixture / "CONTINUATION.md").write_text(
                missing_overlap_states, encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, case_prompt=case.prompt))
            overlap_followup = _h007_followup_prompt(
                fixture, calls, case_prompt=case.prompt,
                provider_request_limit=case.budget["max_provider_requests"])
            self.assertIn("native trace already proves", overlap_followup)
            self.assertIn("job_id_overlap_running: running", overlap_followup)
            self.assertIn("job_id_overlap_terminal: completed", overlap_followup)
            self.assertIn("Do not resubmit the release test", overlap_followup)
            self.assertNotIn("Submit exactly `cargo test --offline --locked --release",
                             overlap_followup)
            (fixture / "CONTINUATION.md").write_text(note, encoding="utf-8")

            with tempfile.TemporaryDirectory() as outside:
                outside_tools = pathlib.Path(outside) / "tools"
                outside_tools.mkdir()
                (outside_tools / "catalog_probe.py").write_text(
                    "# outside symlink\n", encoding="utf-8")
                (tools_dir / "catalog_probe.py").unlink()
                tools_dir.rmdir()
                tools_dir.symlink_to(outside_tools)
                self.assertFalse(_h007_continuation_confirmed(
                    calls, fixture, case_prompt=case.prompt))
                tools_dir.unlink()
                tools_dir.mkdir()
                (tools_dir / "catalog_probe.py").write_text("# self-test\n", encoding="utf-8")

            pending = note.replace("- bounded_probes: complete", "- bounded_probes: pending")
            (fixture / "CONTINUATION.md").write_text(pending, encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, case_prompt=case.prompt))
            followup = _h007_followup_prompt(
                fixture, calls, case_prompt=case.prompt,
                provider_request_limit=case.budget["max_provider_requests"])
            self.assertIn("bounded_probes", followup)
            self.assertNotIn("catalog_bundle", followup)
            self.assertNotIn("all fifteen phases", followup)
            self.assertNotIn("job_id_priority", followup)
            self.assertNotIn("750-request", followup)

            failed_review = note.replace(
                "review_status: completed (retrieved using native task get)",
                "review_status: failed")
            (fixture / "CONTINUATION.md").write_text(failed_review, encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, case_prompt=case.prompt))
            calls[5]["task_spawn_agent"] = "research"
            (fixture / "CONTINUATION.md").write_text(note, encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, case_prompt=case.prompt))
            calls[5]["task_spawn_agent"] = "verifier"
            calls[7]["task_result_run_status"] = "failed"
            (fixture / "CONTINUATION.md").write_text(note, encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, case_prompt=case.prompt))

            calls[7]["task_result_run_status"] = "completed"
            calls[7]["task_result_run_id_sha256"] = hashlib.sha256(
                b"different-run").hexdigest()
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, case_prompt=case.prompt))
            calls[7]["task_result_run_id_sha256"] = run_digest
            malformed_review = note.replace("review_run: run-12", "review_run: run id with spaces")
            (fixture / "CONTINUATION.md").write_text(malformed_review, encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, case_prompt=case.prompt))
            (fixture / "CONTINUATION.md").write_text(note, encoding="utf-8")
            for name, content in (
                ("PUBLIC_ACCEPTANCE_AUDIT.md", "audited\n"),
                ("HARNESS_PROBE_REPORT.json", json.dumps({"probe_results": []})),
                ("tools/catalog_probe.py", "# self-test\n"),
            ):
                target = fixture / name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(content, encoding="utf-8")
            short_horizon = _h007_followup_prompt(
                fixture, calls, case_prompt=case.prompt,
                active_work_span_ms=20 * 60 * 1000, continuation_complete=True)
            self.assertIn("restart or failure-preservation", short_horizon)
            self.assertIn("required 30", short_horizon)
            self.assertNotIn("idle", short_horizon.lower())
            self.assertNotIn("waits", short_horizon.lower())

            (fixture / "CONTINUATION.md").write_text(note, encoding="utf-8")
            (fixture / "HARNESS_PROBE_REPORT.json").unlink()
            self.assertFalse(_h007_continuation_confirmed(
                calls, fixture, case_prompt=case.prompt))

    def test_continuation_rejects_symlink_and_oversized_record(self):
        from dev.harness_lab.autonomy_run import (
            H007_CONTINUATION_MAX_BYTES, _h007_continuation_confirmed,
        )

        calls = [{"tool": "job", "job": {"job_id": "job-1", "status": "completed"}},
                 {"tool": "write"}]
        valid = _continuation_text("job-1")
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            outside = root / "outside.md"
            outside.write_text(valid, encoding="utf-8")
            fixture = root / "fixture"
            fixture.mkdir()
            (fixture / "CONTINUATION.md").symlink_to(outside)
            self.assertFalse(_h007_continuation_confirmed(calls, fixture))
            (fixture / "CONTINUATION.md").unlink()
            (fixture / "CONTINUATION.md").write_text(
                valid + ("x" * H007_CONTINUATION_MAX_BYTES), encoding="utf-8")
            self.assertFalse(_h007_continuation_confirmed(calls, fixture))

    def test_case_manifest_is_loadable_and_keeps_provider_authority_ungranted(self):
        suite = autonomy.load_autonomy_suite(MANIFEST.parent)
        case = suite.case("build-provenance-catalog")
        self.assertEqual(case.family, "background_job")
        self.assertEqual(case.budget["max_wallclock_seconds"], 3600)
        self.assertEqual(case.budget["max_provider_requests"], 60)
        self.assertIsNone(case.budget["max_cost_usd"])
        self.assertEqual(case.network, "none")
        self.assertFalse(case.protected)

    def test_s018_v43_manifest_keeps_caps_and_moves_required_overlap_earlier(self):
        suite = autonomy.load_autonomy_suite(S018_V43_MANIFEST.parent)
        case = suite.case("build-provenance-catalog")
        self.assertEqual(case.group_id, "s018-muse-continuation-v43")
        self.assertEqual(case.budget["max_wallclock_seconds"], 3600)
        self.assertEqual(case.budget["max_provider_requests"], 1200)
        self.assertEqual(case.budget["max_tool_calls"], 1600)
        self.assertIsNone(case.budget["max_cost_usd"])
        self.assertIn("dependency_index` → `provenance_queries", case.prompt)
        self.assertIn("--release -p catalog-index --test corpus", case.prompt)
        self.assertIn("fixture/catalog-query/src/lib.rs", case.prompt)
        self.assertNotIn("At the `catalog_delta` → `catalog_merge` transition", case.prompt)

    def test_s018_v44_manifest_preserves_task_and_bounds_child_polling(self):
        case = autonomy.load_autonomy_suite(S018_V44_MANIFEST.parent).case(
            "build-provenance-catalog")
        self.assertEqual(case.group_id, "s018-muse-continuation-v44")
        self.assertEqual(case.budget["max_wallclock_seconds"], 3600)
        self.assertEqual(case.budget["max_provider_requests"], 1200)
        self.assertEqual(case.budget["max_tool_calls"], 1600)
        self.assertIsNone(case.budget["max_cost_usd"])
        self.assertIn("same task, phase order, verifier, provider, and limits", case.prompt)
        self.assertIn("do not call task status/get again until two more required phases", case.prompt)
        self.assertIn("Complete all fifteen phases in this exact order", case.prompt)

    def test_s018_v68_prioritizes_unfinished_stages_without_changing_scope(self):
        previous = json.loads(S018_V67_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(S018_V68_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(S018_V68_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s018-muse-continuation-v68")
        self.assertTrue(case.prompt.startswith("ATTEMPT 40 PRIORITY"))
        self.assertIn("acceptance_audit and workspace_qualification still missing", case.prompt)
        self.assertIn("Keep the seven implementation phases", case.prompt)
        self.assertIn("Keep the dependency-index → exact release-scale background job", case.prompt)
        self.assertIn("reserve the final 300 seconds", case.prompt)
        self.assertEqual(set(previous) - {"group_id", "title", "prompt"},
                         set(current) - {"group_id", "title", "prompt"})
        for key in set(previous) - {"group_id", "title", "prompt"}:
            self.assertEqual(current[key], previous[key], f"v68 changed manifest field {key}")

    def test_s018_v69_requires_parseable_public_probe_artifact(self):
        previous = json.loads(S018_V68_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(S018_V69_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(S018_V69_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s018-muse-continuation-v69")
        self.assertIn("python3 -m json.tool HARNESS_PROBE_REPORT.json", case.prompt)
        self.assertIn("keep `bounded_probes` pending until parsing succeeds", case.prompt)
        self.assertIn("do not mark an artifact-backed gate complete", case.prompt)
        self.assertEqual(set(previous) - {"group_id", "title", "prompt"},
                         set(current) - {"group_id", "title", "prompt"})
        for key in set(previous) - {"group_id", "title", "prompt"}:
            self.assertEqual(current[key], previous[key], f"v69 changed manifest field {key}")

    def test_s018_v70_restores_continuation_ledgers_and_artifact_parse_gate(self):
        previous = json.loads(
            S018_V68_MANIFEST.read_text(encoding="utf-8").splitlines()[0]
        )
        current = json.loads(S018_V70_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(S018_V70_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s018-muse-continuation-v70")
        for field in ("contract_phases", "next_contract_phase", "required_stages", "next_stage"):
            self.assertIn(field, case.prompt)
        self.assertIn("python3 -m json.tool HARNESS_PROBE_REPORT.json", case.prompt)
        self.assertIn("keep bounded_probes pending until parsing succeeds", case.prompt)
        self.assertIn("do not mark artifact-backed gate complete from continuation note alone",
                      case.prompt)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v70 changed manifest field {key}")

    def test_s018_v71_polls_review_while_qualification_runs_and_starts_workspace_early(self):
        previous = json.loads(S018_V70_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(S018_V71_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(S018_V71_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s018-muse-continuation-v71")
        self.assertIn("15 minutes", case.prompt)
        self.assertIn("After every three completed non-task tool operations", case.prompt)
        self.assertIn("make one `task status` call using that exact Run ID", case.prompt)
        self.assertIn("As soon as the Rust implementation and CLI contract are stable", case.prompt)
        self.assertIn("before acceptance/probe evidence is finished", case.prompt)
        self.assertIn("Target all gates and the final checkpoint by minute 45", case.prompt)
        self.assertNotIn("Do not poll the reviewer during implementation", case.prompt)
        self.assertNotIn("25 minutes", case.prompt)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v71 changed manifest field {key}")

    def test_s018_v72_locks_completed_review_and_prioritizes_release_scale_evidence(self):
        previous = json.loads(S018_V71_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(S018_V72_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(S018_V72_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s018-muse-continuation-v72")
        self.assertIn("NON-REPEATABLE REVIEW STATE", case.prompt)
        self.assertIn("do not invoke the native `task` tool again", case.prompt)
        self.assertIn("RELEASE-SCALE OVERLAP PRIORITY", case.prompt)
        self.assertIn("before starting `provenance_queries`", case.prompt)
        self.assertIn("before submitting workspace or Clippy jobs", case.prompt)
        self.assertIn("PUBLIC_ACCEPTANCE_AUDIT.md", case.prompt)
        self.assertIn("by active minute 40", case.prompt)
        self.assertIn("reuse the existing review fields", case.prompt)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v72 changed manifest field {key}")

    def test_s018_v73_requires_exact_successful_workspace_and_clippy_jobs(self):
        previous = json.loads(S018_V72_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(S018_V73_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(S018_V73_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s018-muse-continuation-v73")
        self.assertIn("job_id_workspace_tests", case.prompt)
        self.assertIn("job_id_clippy", case.prompt)
        self.assertIn("matching observed completed terminal state", case.prompt)
        self.assertIn("not available for an in-session repair", case.prompt)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v73 changed manifest field {key}")

    def test_s018_v74_requires_public_contract_traceability_and_final_review(self):
        previous = json.loads(S018_V73_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(S018_V74_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(S018_V74_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s018-muse-continuation-v74")
        self.assertIn("Contract-to-code/test traceability", case.prompt)
        self.assertIn("after active minute 30", case.prompt)
        self.assertIn("no later than active minute 40", case.prompt)
        self.assertIn("completed matrix against the frozen public contract", case.prompt)
        self.assertIn("Do not spawn the single read-only verifier subtask at the 15-minute threshold",
                      case.prompt)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v74 changed manifest field {key}")

    def test_s019_v76_manifest_changes_only_reviewer_timing(self):
        previous = json.loads(S018_V75_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(S018_V76_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(S018_V76_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-diagnostic-v76")
        self.assertIn("task", case.allowed_tools)
        self.assertIn("At the first same-session follow-up after 30 minutes", case.prompt)
        self.assertIn("regardless of whether the final workspace-suite and Clippy jobs have completed",
                      case.prompt)
        self.assertIn("Never call `task get` while that status is queued or running", case.prompt)
        self.assertIn("`timeout_ms: 30000`", case.prompt)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v76 changed manifest field {key}")

    def test_s019_v76_reviewer_starts_at_thirty_minutes_even_when_final_jobs_run(self):
        from dev.harness_lab.autonomy_run import (
            H007_S018_FINAL_REVIEW_START_ACTIVE_MS,
            H007_S018_FINAL_REVIEW_START_MARKER,
            H007_S018_REVIEW_START_ACTIVE_MS,
            _h007_followup_prompt,
        )

        case = autonomy.load_autonomy_suite(S018_V76_MANIFEST.parent).case(
            "build-provenance-catalog")
        continuation = (
            "job_id_workspace_tests: workspace-job\n"
            "job_state_workspace_tests: completed\n"
            "job_id_clippy: clippy-job\n"
            "job_state_clippy: completed\n"
        )
        running_calls = [
            {"tool": "test", "job": {"job_id": "workspace-job", "action": "submit",
                                      "status": "queued"}},
            {"tool": "job", "job": {"job_id": "workspace-job", "status": "running"}},
            {"tool": "test", "job": {"job_id": "clippy-job", "action": "submit",
                                      "status": "queued"}},
            {"tool": "job", "job": {"job_id": "clippy-job", "status": "running"}},
        ]
        completed_calls = [
            {"tool": "test", "job": {"job_id": "workspace-job", "action": "submit",
                                      "status": "queued"}},
            {"tool": "job", "job": {"job_id": "workspace-job", "status": "completed"}},
            {"tool": "test", "job": {"job_id": "clippy-job", "action": "submit",
                                      "status": "queued"}},
            {"tool": "job", "job": {"job_id": "clippy-job", "status": "completed"}},
        ]

        self.assertEqual(H007_S018_REVIEW_START_ACTIVE_MS, 15 * 60 * 1000)
        self.assertEqual(H007_S018_FINAL_REVIEW_START_ACTIVE_MS, 30 * 60 * 1000)
        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            too_early = _h007_followup_prompt(
                fixture, case_prompt=case.prompt,
                active_work_span_ms=H007_S018_FINAL_REVIEW_START_ACTIVE_MS - 1)
            (fixture / "CONTINUATION.md").write_text(
                continuation.replace("job_state_workspace_tests: completed", "job_state_workspace_tests: running")
                .replace("job_state_clippy: completed", "job_state_clippy: running"),
                encoding="utf-8",
            )
            due_while_jobs_run = _h007_followup_prompt(
                fixture, running_calls, case_prompt=case.prompt,
                active_work_span_ms=H007_S018_FINAL_REVIEW_START_ACTIVE_MS)
            (fixture / "CONTINUATION.md").write_text(continuation, encoding="utf-8")
            due_after_jobs = _h007_followup_prompt(
                fixture, completed_calls, case_prompt=case.prompt,
                active_work_span_ms=H007_S018_FINAL_REVIEW_START_ACTIVE_MS)

        self.assertIn("before the measured active-work span reaches 30 minutes", too_early)
        self.assertNotIn(H007_S018_FINAL_REVIEW_START_MARKER, too_early)
        self.assertIn(H007_S018_FINAL_REVIEW_START_MARKER, due_while_jobs_run)
        self.assertIn("Spawn exactly one native read-only task now", due_while_jobs_run)
        self.assertIn("One or both exact final workspace-suite and Clippy jobs are not yet confirmed",
                      due_while_jobs_run)
        self.assertIn("leave that gate open", due_while_jobs_run)
        self.assertNotIn("Do not spawn a reviewer yet", due_while_jobs_run)
        self.assertIn("Never retrieve a live result", due_while_jobs_run)
        self.assertIn("`timeout_ms: 30000`", due_while_jobs_run)
        self.assertIn(H007_S018_FINAL_REVIEW_START_MARKER, due_after_jobs)
        self.assertIn("Both exact final workspace-suite and Clippy jobs are confirmed completed",
                      due_after_jobs)

    def test_s019_v77_manifest_preserves_task_and_adds_public_audit_stress_controls(self):
        previous = json.loads(S018_V76_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(S018_V77_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(S018_V77_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-diagnostic-v77")
        self.assertIn("S019 attempt 22", case.prompt)
        self.assertIn("At the first same-session follow-up after 30 minutes", case.prompt)
        self.assertIn("Never call `task get` while that status is queued or running", case.prompt)
        self.assertIn("delegate one read-only source audit to the reviewer", case.prompt)
        self.assertIn("Create or extend one small helper under `tools/`", case.prompt)
        self.assertIn("Try to break the harness through unusual but bounded sequences", case.prompt)
        self.assertIn("cancel one disposable job owned by this run", case.prompt)
        self.assertIn("Do not access credentials, the hidden verifier", case.prompt)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v77 changed manifest field {key}")

    def test_s019_v78_manifest_preserves_task_and_bounds_reviewer_completion(self):
        previous = json.loads(S018_V77_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(S018_V78_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(S018_V78_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-diagnostic-v78")
        self.assertIn("S019 attempt 23", case.prompt)
        self.assertIn("deadline_exceeded", case.prompt)
        self.assertIn("Keep the seven required stages named exactly", case.prompt)
        self.assertIn("FINAL CHECKPOINT", case.prompt)
        self.assertIn("contract_phases", case.prompt)
        self.assertIn("required_stages", case.prompt)
        self.assertIn("Do not use `done` or other synonyms", case.prompt)
        self.assertIn("check at most five highest-risk claims", case.prompt)
        self.assertIn("After every three productive non-task tool calls", case.prompt)
        self.assertIn("At the first same-session follow-up after 30 minutes", case.prompt)
        self.assertLess(len(case.prompt), 10_000)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v78 changed manifest field {key}")

    def test_s019_v79_preserves_task_and_corrects_followup_cap_stop(self):
        previous = json.loads(S018_V78_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(S018_V79_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(S018_V79_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-diagnostic-v79")
        self.assertIn("S019 attempt 24", case.prompt)
        self.assertIn("stopped at the old 40-follow-up cap", case.prompt)
        self.assertIn("continuation cap is now 100", case.prompt)
        self.assertIn("30 minutes is a minimum, not a stopping point", case.prompt)
        self.assertIn("`end_turn` with required stages, gates, or jobs pending is not completion",
                      case.prompt)
        self.assertIn("The harness runs the unavailable hidden verifier after the session",
                      case.prompt)
        self.assertNotIn("do not set the final checkpoint until the hidden verifier has independently passed",
                         case.prompt)
        self.assertLess(len(case.prompt), 10_000)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v79 changed manifest field {key}")

    def test_lane_cleanup_diagnostic_records_only_owned_process_count(self):
        from dev.harness_lab import supervisor
        from dev.harness_lab.autonomy_run import _owned_lane_daemon_process_count

        with mock.patch.object(supervisor, "_owned_daemon_pids", return_value=[12345, 67890]):
            self.assertEqual(_owned_lane_daemon_process_count(object()), 2)
        with mock.patch.object(supervisor, "_owned_daemon_pids", side_effect=OSError):
            self.assertIsNone(_owned_lane_daemon_process_count(object()))

    def test_s018_v75_preserves_workload_and_adds_contract_edge_audit(self):
        previous = json.loads(S018_V74_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(S018_V75_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(S018_V75_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s018-muse-continuation-v75")
        self.assertIn("two generic assertions failed", case.prompt)
        self.assertIn("one behavior-focused test", case.prompt)
        self.assertIn("try to falsify each claim", case.prompt)
        self.assertIn("HARNESS BOUNDARY PROBES", case.prompt)
        self.assertIn("Do not probe additional private-root paths", case.prompt)
        self.assertIn("hidden verifier remains independent and unchanged", case.prompt)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v75 changed manifest field {key}")

    def test_live_case_root_contains_the_frozen_agent_fixture(self):
        from dev.harness_lab import autonomy_run

        case = autonomy.load_autonomy_suite(MANIFEST.parent).case("build-provenance-catalog")
        case_root = autonomy_run.case_root(REPO_ROOT, case.case_id)
        packaged_fixture = case_root / "fixture"
        files = lambda root: {
            path.relative_to(root).as_posix(): _sha(path)
            for path in root.rglob("*")
            if path.is_file() and "target" not in path.relative_to(root).parts
            and path.relative_to(root).as_posix() != "CONTINUATION.md"
        }
        self.assertTrue(packaged_fixture.is_dir(), "the live runner's editable fixture is missing")
        self.assertTrue((case_root / "verifier").is_dir(), "the independent verifier is missing")
        self.assertEqual(files(packaged_fixture), files(STARTER))
        self.assertEqual((packaged_fixture / "CONTINUATION.md").read_bytes(),
                         (STARTER / "CONTINUATION.md").read_bytes())

    def test_frozen_hashes_match_starter_and_leave_workspace_manifests_editable(self):
        from dev.harness_lab import autonomy_run

        metadata = json.loads((BASE.parent / "build_provenance_catalog.case.json").read_text())
        frozen = metadata["frozen_sha256"]
        mutable_workspace_manifests = {
            "Cargo.toml", "Cargo.lock", "catalog-cli/Cargo.toml",
        }
        self.assertTrue(mutable_workspace_manifests.isdisjoint(frozen))
        self.assertTrue(frozen)
        runtime_fixture = autonomy_run.case_root(REPO_ROOT, "build-provenance-catalog") / "fixture"
        for label, root in (("starter", STARTER), ("runtime", runtime_fixture),
                            ("reference", REFERENCE)):
            for relative, expected in frozen.items():
                path = root / relative
                self.assertTrue(path.is_file(), f"missing frozen {label} input: {relative}")
                self.assertEqual(_sha(path), expected, f"stale frozen {label} digest: {relative}")

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
        self.assertEqual(long_failure["horizon_outcome"], "not_qualified")
        self.assertEqual(long_failure["horizon_reason"], "task_not_passed")
        self.assertEqual(long_failure["horizon_evidence"]["agent_terminal"], "completed")

        timed_out_failure = h007_results.project_result(
            terminal="timed_out", verifier_ran=True, verifier_exit_code=1,
            duration_ms=1800200, tool_calls=calls, measurement_complete=True,
            continuation_confirmed=True, repair_cycles=2)
        self.assertEqual(timed_out_failure["task_outcome"], "failed")
        self.assertEqual(timed_out_failure["horizon_reason"], "task_not_passed")
        self.assertEqual(timed_out_failure["horizon_evidence"]["agent_terminal"], "timed_out")

        timed_out_pass = h007_results.project_result(
            terminal="timed_out", verifier_ran=True, verifier_exit_code=0,
            duration_ms=1800200, tool_calls=calls, measurement_complete=True,
            continuation_confirmed=True, repair_cycles=2)
        self.assertEqual(timed_out_pass["task_outcome"], "passed")
        self.assertEqual(timed_out_pass["horizon_reason"], "agent_terminal_incomplete")
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

    def test_lifecycle_without_productive_overlap_is_reported_as_distinct_evidence(self):
        calls = [
            {"tool": "test", "started_elapsed_ms": 0, "finished_elapsed_ms": 1,
             "job": {"job_id": "j1", "action": "submit", "status": "running"}},
            {"tool": "job", "started_elapsed_ms": 2, "finished_elapsed_ms": 3,
             "job": {"job_id": "j1", "action": "status", "status": "running"}},
            {"tool": "job", "started_elapsed_ms": 100, "finished_elapsed_ms": 101,
             "job": {"job_id": "j1", "action": "status", "status": "completed"}},
            {"tool": "write", "started_elapsed_ms": 1800000, "finished_elapsed_ms": 1800001},
        ]
        result = h007_results.project_result(
            terminal="completed", verifier_ran=True, verifier_exit_code=0,
            duration_ms=1800001, tool_calls=calls, measurement_complete=True,
            continuation_confirmed=False, repair_cycles=2)
        evidence = result["horizon_evidence"]
        self.assertTrue(evidence["exact_job_running_and_terminal"])
        self.assertFalse(evidence["productive_work_during_job"])
        self.assertEqual(result["horizon_reason"], "job_lifecycle_missing")

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

    def test_verifier_setup_timeout_is_inconclusive_not_a_task_failure(self):
        result = h007_results.project_result(
            terminal="completed", verifier_ran=False, verifier_exit_code=1,
            duration_ms=1800001, tool_calls=[], measurement_complete=False,
            continuation_confirmed=True, repair_cycles=2)

        self.assertEqual(result["task_outcome"], "inconclusive")
        self.assertEqual(result["horizon_outcome"], "not_qualified")
        self.assertEqual(result["horizon_reason"], "measurement_incomplete")

    def test_complete_event_capture_distinguishes_verifier_setup_error(self):
        result = h007_results.project_result(
            terminal="completed", verifier_ran=False, verifier_exit_code=1,
            verifier_state="setup_error", duration_ms=1800001, tool_calls=[],
            measurement_complete=True, continuation_confirmed=False, repair_cycles=2)

        self.assertEqual(result["task_outcome"], "inconclusive")
        self.assertEqual(result["verifier_outcome"], "setup_error")
        self.assertEqual(result["horizon_outcome"], "not_qualified")
        self.assertEqual(result["horizon_reason"], "verifier_setup_error")
        self.assertTrue(result["horizon_evidence"]["event_measurement_complete"])

    def test_task_completion_diagnostic_separates_pending_work_from_verifier_error(self):
        continuation = {
            "present": True,
            "document_valid": False,
            "incomplete_stages": ["workspace_qualification"],
            "incomplete_gates": ["independent_review"],
            "incomplete_required_success_jobs": ["workspace_tests", "clippy"],
            "test_job_lifecycle": {"unobserved_count": 1},
            "required_review_observed": False,
            "all_public_artifacts_present": True,
        }
        result = h007_results.summarize_task_completion(
            terminal="completed", task_outcome="inconclusive",
            verifier_outcome="setup_error", continuation=continuation)

        self.assertEqual(result["state"], "incomplete")
        self.assertIn("required_stages_incomplete", result["reasons"])
        self.assertIn("required_gates_incomplete", result["reasons"])
        self.assertIn("required_success_jobs_incomplete", result["reasons"])
        self.assertIn("test_jobs_nonterminal", result["reasons"])
        self.assertIn("required_review_unobserved", result["reasons"])
        self.assertEqual(result["verifier_outcome"], "setup_error")
        self.assertFalse(result["raw_note_retained"])
        self.assertFalse(result["raw_job_ids_retained"])

        complete = h007_results.summarize_task_completion(
            terminal="completed", task_outcome="passed", verifier_outcome="passed",
            continuation={"present": True, "document_valid": True,
                          "incomplete_stages": [], "incomplete_gates": [],
                          "incomplete_required_success_jobs": [],
                          "test_job_lifecycle": {"unobserved_count": 0},
                          "required_review_observed": True,
                          "all_public_artifacts_present": True})
        self.assertEqual(complete["state"], "verified_complete")

    def test_verifier_failure_reconciles_public_evidence_by_safe_domain(self):
        continuation = {
            "present": True,
            "document_valid": True,
            "line_layout": {
                "progress_sections_ordered": True,
                "pointer_fields_follow_progress_sections": True,
                "pointer_fields_in_expected_order": True,
                "pointer_block_is_terminal": True,
                "raw_note_retained": False,
            },
            "phase_order_matches": True,
            "contract_phase_order_matches": True,
            "stage_order_matches": True,
            "next_phase": "none",
            "next_contract_phase": "none",
            "next_stage": "none",
            "next_gate": "none",
            "incomplete_stages": [],
            "incomplete_gates": [],
            "missing_phases": [],
            "missing_contract_phases": [],
            "incomplete_required_success_jobs": [],
            "required_success_jobs": {
                "workspace_tests": {"completed": True},
                "clippy": {"completed": True},
            },
            "test_job_lifecycle": {"all_terminal_observed": True},
            "repair_job_diagnostic": {
                "complete": True,
                "repair_cycle_count_matches": True,
                "observed_repair_cycle_count": 2,
            },
            "required_review_observed": True,
            "release_overlap_confirmed": True,
            "all_public_artifacts_present": True,
        }
        horizon = {
            "exact_job_running_and_terminal": True,
            "productive_work_during_job": True,
            "event_measurement_complete": True,
        }
        verifier = {
            "failure_domain_counts": {
                "continuation_boundary": 1,
                "transcript_job_lifecycle": 1,
            },
        }

        result = h007_results.summarize_task_completion(
            terminal="completed", task_outcome="failed",
            verifier_outcome="assertion_failures", continuation=continuation,
            verifier_diagnostic=verifier, horizon_evidence=horizon,
            job_sequence_diagnostic={"unbound_job_tool_call_count": 0})

        self.assertEqual(result["state"], "verification_failed")
        self.assertIn("independent_verifier_failures", result["reasons"])
        reconciliation = result["verifier_public_reconciliation"]
        self.assertEqual(reconciliation["public_domain_states"], {
            "continuation_boundary": "complete",
            "transcript_job_lifecycle": "complete",
        })
        self.assertEqual(reconciliation["classification"], {
            "continuation_boundary": "verifier_public_projection_disagreement",
            "transcript_job_lifecycle": "verifier_public_projection_disagreement",
        })
        self.assertTrue(reconciliation["public_checks"]["transcript_job_lifecycle"]
                        ["job_tool_calls_bound_to_id"])
        self.assertTrue(reconciliation["public_checks"]["continuation_boundary"]
                        ["pointer_fields_follow_progress_sections"])
        self.assertEqual(reconciliation["unbound_job_tool_call_count"], 0)

        unbound = h007_results.summarize_task_completion(
            terminal="completed", task_outcome="failed",
            verifier_outcome="assertion_failures", continuation=continuation,
            verifier_diagnostic=verifier, horizon_evidence=horizon,
            job_sequence_diagnostic={"unbound_job_tool_call_count": 3})
        unbound_reconciliation = unbound["verifier_public_reconciliation"]
        self.assertEqual(unbound_reconciliation["public_domain_states"], {
            "continuation_boundary": "complete",
            "transcript_job_lifecycle": "incomplete",
        })
        self.assertEqual(unbound_reconciliation["classification"], {
            "continuation_boundary": "verifier_public_projection_disagreement",
            "transcript_job_lifecycle": "public_evidence_incomplete",
        })
        self.assertFalse(unbound_reconciliation["public_checks"][
            "transcript_job_lifecycle"]["job_tool_calls_bound_to_id"])
        self.assertEqual(unbound_reconciliation["unbound_job_tool_call_count"], 3)

        unavailable = h007_results.summarize_task_completion(
            terminal="completed", task_outcome="failed",
            verifier_outcome="assertion_failures", continuation=continuation,
            verifier_diagnostic=verifier, horizon_evidence=horizon)
        unavailable_reconciliation = unavailable["verifier_public_reconciliation"]
        self.assertEqual(unavailable_reconciliation["public_domain_states"][
            "transcript_job_lifecycle"], "unavailable")
        self.assertEqual(unavailable_reconciliation["classification"][
            "transcript_job_lifecycle"], "public_evidence_unavailable")
        self.assertFalse(reconciliation["assertion_text_retained"])
        self.assertFalse(reconciliation["raw_job_ids_retained"])
        self.assertNotIn("PRIVATE_ASSERTION_TEXT", json.dumps(result))

    def test_verifier_failure_with_missing_public_evidence_is_not_a_disagreement(self):
        result = h007_results.summarize_task_completion(
            terminal="completed", task_outcome="failed",
            verifier_outcome="assertion_failures",
            continuation={
                "present": True,
                "document_valid": False,
                "incomplete_stages": ["workspace_qualification"],
                "incomplete_gates": ["independent_review"],
                "repair_job_diagnostic": {"complete": False},
                "test_job_lifecycle": {"all_terminal_observed": False},
                "required_review_observed": False,
                "release_overlap_confirmed": False,
            },
            verifier_diagnostic={
                "failure_domain_counts": {
                    "continuation_boundary": 1,
                    "transcript_job_lifecycle": 1,
                },
            },
            horizon_evidence={
                "exact_job_running_and_terminal": False,
                "productive_work_during_job": False,
                "event_measurement_complete": False,
            })

        self.assertEqual(result["state"], "incomplete")
        self.assertEqual(result["verifier_public_reconciliation"]["classification"], {
            "continuation_boundary": "public_evidence_incomplete",
            "transcript_job_lifecycle": "public_evidence_incomplete",
        })

    def test_lifecycle_disagreement_requires_request_result_and_transcript_parity(self):
        continuation = {
            "present": True, "document_valid": True,
            "phase_order_matches": True, "contract_phase_order_matches": True,
            "stage_order_matches": True, "next_phase": "none",
            "next_contract_phase": "none", "next_stage": "none", "next_gate": "none",
            "incomplete_stages": [], "incomplete_gates": [], "missing_phases": [],
            "missing_contract_phases": [], "incomplete_required_success_jobs": [],
            "required_success_jobs": {"workspace_tests": {"completed": True}},
            "test_job_lifecycle": {"all_terminal_observed": True},
            "repair_job_diagnostic": {
                "complete": True, "repair_cycle_count_matches": True,
                "observed_repair_cycle_count": 2,
            },
            "required_review_observed": True, "release_overlap_confirmed": True,
            "all_public_artifacts_present": True,
            "line_layout": {
                "progress_sections_ordered": True,
                "pointer_fields_follow_progress_sections": True,
                "pointer_fields_in_expected_order": True,
            },
        }
        common = dict(
            terminal="completed", task_outcome="failed",
            verifier_outcome="assertion_failures", continuation=continuation,
            verifier_diagnostic={"failure_domain_counts": {"transcript_job_lifecycle": 1}},
            horizon_evidence={
                "exact_job_running_and_terminal": True,
                "productive_work_during_job": True,
                "event_measurement_complete": True,
            },
        )
        correlated = h007_results.summarize_task_completion(
            **common,
            job_sequence_diagnostic={
                "unbound_job_tool_call_count": 0,
                "failed_test_submit_error_class_counts": {
                    "tool_execution_failed": 1, "PRIVATE_CLASS": 9,
                },
                "failed_job_status_error_class_counts": {
                    "job_not_found": 2, "unobserved": 1, "PRIVATE_CLASS": 9,
                    "unclassified": 1_601, "tool_execution_failed": True,
                },
                "job_id_request_result_correlation": {
                    "instrumentation_observed": True,
                    "applicable_call_count": 3,
                    "state_counts": {"matched": 2, "mismatched": 1, "private": 8},
                    "all_applicable_calls_match": True,
                },
            },
            serialized_transcript_diagnostic={
                "lifecycle_projection_matches_source": True,
            },
        )["verifier_public_reconciliation"]
        self.assertEqual(correlated["public_domain_states"]["transcript_job_lifecycle"],
                         "complete")
        self.assertEqual(correlated["failed_test_submit_error_class_counts"], {
            "tool_execution_failed": 1,
        })
        self.assertEqual(correlated["failed_job_status_error_class_counts"], {
            "job_not_found": 2, "unobserved": 1,
        })
        self.assertEqual(correlated["job_id_request_result_correlation"], {
            "instrumentation_observed": True,
            "applicable_call_count": 3,
            "state_counts": {"matched": 2, "mismatched": 1},
        })

        mismatch = h007_results.summarize_task_completion(
            **common,
            job_sequence_diagnostic={
                "unbound_job_tool_call_count": 0,
                "job_id_request_result_correlation": {
                    "instrumentation_observed": True,
                    "applicable_call_count": 3,
                    "all_applicable_calls_match": False,
                },
            },
            serialized_transcript_diagnostic={
                "lifecycle_projection_matches_source": False,
            },
        )["verifier_public_reconciliation"]
        self.assertEqual(mismatch["public_domain_states"]["transcript_job_lifecycle"],
                         "incomplete")
        self.assertEqual(mismatch["classification"]["transcript_job_lifecycle"],
                         "public_evidence_incomplete")
        self.assertFalse(mismatch["public_checks"]["transcript_job_lifecycle"][
            "job_request_result_ids_match"])
        self.assertFalse(mismatch["public_checks"]["transcript_job_lifecycle"][
            "serialized_transcript_projection_matches"])

    def test_missing_repair_cycle_is_not_reported_as_missing_continuation(self):
        calls = [
            {"tool": "test", "started_elapsed_ms": 0, "finished_elapsed_ms": 1,
             "job": {"job_id": "j1", "action": "submit", "status": None}},
            {"tool": "job", "started_elapsed_ms": 2, "finished_elapsed_ms": 3,
             "job": {"job_id": "j1", "action": "status", "status": "running"}},
            {"tool": "edit", "started_elapsed_ms": 4, "finished_elapsed_ms": 5},
            {"tool": "job", "started_elapsed_ms": 6, "finished_elapsed_ms": 7,
             "job": {"job_id": "j1", "action": "status", "status": "completed"}},
            {"tool": "write", "started_elapsed_ms": 1800000, "finished_elapsed_ms": 1800001},
        ]
        repair_missing = h007_results.project_result(
            terminal="completed", verifier_ran=True, verifier_exit_code=0,
            duration_ms=1800001, tool_calls=calls, measurement_complete=True,
            continuation_confirmed=True, repair_cycles=1)
        self.assertEqual(repair_missing["task_outcome"], "passed")
        self.assertTrue(repair_missing["horizon_evidence"]["continuation_confirmed"])
        self.assertEqual(repair_missing["horizon_reason"], "repair_cycles_insufficient")

        continuation_missing = h007_results.project_result(
            terminal="completed", verifier_ran=True, verifier_exit_code=0,
            duration_ms=1800001, tool_calls=calls, measurement_complete=True,
            continuation_confirmed=False, repair_cycles=2)
        self.assertEqual(continuation_missing["horizon_reason"], "continuation_missing")

    def test_declared_tool_call_cap_prevents_horizon_qualification(self):
        calls = [
            {"tool": "test", "started_elapsed_ms": 0, "finished_elapsed_ms": 1,
             "job": {"job_id": "j1", "action": "submit", "status": None}},
            {"tool": "job", "started_elapsed_ms": 2, "finished_elapsed_ms": 3,
             "job": {"job_id": "j1", "action": "status", "status": "running"}},
            {"tool": "edit", "started_elapsed_ms": 900000, "finished_elapsed_ms": 900001},
            {"tool": "job", "started_elapsed_ms": 1800000, "finished_elapsed_ms": 1800001,
             "job": {"job_id": "j1", "action": "status", "status": "completed"}},
            {"tool": "bash", "started_elapsed_ms": 1800002, "finished_elapsed_ms": 1800003},
        ]
        result = h007_results.project_result(
            terminal="completed", verifier_ran=True, verifier_exit_code=0,
            duration_ms=1800004, tool_calls=calls, measurement_complete=True,
            continuation_confirmed=True, repair_cycles=2, tool_call_limit=4)
        self.assertEqual(result["task_outcome"], "passed")
        self.assertEqual(result["horizon_outcome"], "not_qualified")
        self.assertEqual(result["horizon_reason"], "tool_call_cap_exceeded")
        self.assertTrue(result["horizon_evidence"]["tool_call_cap_exceeded"])
        self.assertEqual(result["horizon_evidence"]["tool_call_count"], 5)

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
            with self.assertRaisesRegex(autonomy_run.HarnessError, "case manifest ceiling"):
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


    def test_test_job_lifecycle_summary_hides_ids_and_requires_terminal_status(self):
        calls = [
            {"tool": "test", "job": {"job_id": "job-a", "action": "submit",
                                      "status": "queued"}},
            {"tool": "job", "job": {"job_id": "job-a", "status": "completed"}},
            {"tool": "test", "job": {"job_id": "job-b", "action": "submit",
                                      "status": "queued"}},
            {"tool": "job", "job": {"job_id": "job-b", "status": "running"}},
        ]

        summary = h007_results.summarize_test_job_lifecycle(calls)

        self.assertEqual(summary["submission_count"], 2)
        self.assertEqual(summary["unique_submitted_job_count"], 2)
        self.assertEqual(summary["terminal_observed_count"], 1)
        self.assertEqual(summary["unobserved_count"], 1)
        self.assertEqual(summary["unobserved_status_counts"], {"running": 1})
        self.assertFalse(summary["all_terminal_observed"])
        self.assertEqual(h007_results.unobserved_test_job_ids(calls), ["job-b"])
        self.assertNotIn("job-a", json.dumps(summary))
        self.assertNotIn("job-b", json.dumps(summary))

        calls.append({"tool": "job", "job": {"job_id": "job-b", "status": "failed"}})
        completed = h007_results.summarize_test_job_lifecycle(calls)
        self.assertTrue(completed["all_terminal_observed"])
        self.assertEqual(completed["terminal_status_counts"], {
            "completed": 1, "failed": 1,
        })

    def test_required_success_jobs_match_exact_observed_completion_without_ids(self):
        calls = [
            {"tool": "test", "job": {"job_id": "workspace-id", "action": "submit",
                                      "status": "queued"}},
            {"tool": "job", "job": {"job_id": "workspace-id", "status": "completed"}},
            {"tool": "test", "job": {"job_id": "clippy-id", "action": "submit",
                                      "status": "queued"}},
            {"tool": "job", "job": {"job_id": "clippy-id", "status": "completed"}},
        ]
        note = (
            "job_id_workspace_tests: workspace-id\n"
            "job_state_workspace_tests: completed\n"
            "job_id_clippy: clippy-id\n"
            "job_state_clippy: completed\n"
        )

        summary = h007_results.summarize_required_success_jobs(
            note, calls, ("workspace_tests", "clippy"),
        )

        self.assertTrue(summary["complete"])
        self.assertEqual(summary["jobs"]["workspace_tests"]["observed_terminal_status"],
                         "completed")
        self.assertEqual(summary["jobs"]["clippy"]["observed_terminal_status"], "completed")
        self.assertFalse(summary["raw_job_ids_retained"])
        self.assertNotIn("workspace-id", json.dumps(summary))
        self.assertNotIn("clippy-id", json.dumps(summary))

        failed_calls = [dict(call) for call in calls]
        failed_calls[-1] = {"tool": "job", "job": {"job_id": "clippy-id",
                                                       "status": "failed"}}
        failed = h007_results.summarize_required_success_jobs(
            note.replace("job_state_clippy: completed", "job_state_clippy: failed"),
            failed_calls, ("workspace_tests", "clippy"),
        )
        self.assertFalse(failed["complete"])
        self.assertEqual(failed["jobs"]["clippy"]["observed_terminal_status"], "failed")

        missing = h007_results.summarize_required_success_jobs(
            note.replace("job_id_clippy: clippy-id\njob_state_clippy: completed\n", ""),
            calls[:2], ("workspace_tests", "clippy"),
        )
        self.assertFalse(missing["complete"])
        self.assertEqual(missing["jobs"]["clippy"]["observed_terminal_status"], "unobserved")

    def test_final_qualification_jobs_are_required_for_valid_continuation(self):
        calls = [
            {"tool": "test", "job": {"job_id": "ws", "action": "submit",
                                      "status": "queued"}},
            {"tool": "job", "job": {"job_id": "ws", "status": "completed"}},
            {"tool": "write"},
        ]
        note = (
            "completed_phases:\n- phase_one\nnext_phase: none\n"
            "job_id_workspace_tests: ws\njob_state_workspace_tests: completed\n"
        )
        summary = h007_results.summarize_continuation_document(
            note, calls, required_phases=("phase_one",),
            required_success_job_labels=("workspace_tests", "clippy"),
        )
        self.assertFalse(summary["document_valid"])
        self.assertEqual(summary["incomplete_required_success_jobs"], ["clippy"])

    def test_continuation_diagnostic_projects_pending_state_without_note_or_job_ids(self):
        calls = [
            {"tool": "test", "job": {"job_id": "private-job-id", "action": "submit",
                                      "status": "queued"}},
            {"tool": "job", "job": {"job_id": "private-job-id", "status": "running"}},
            {"tool": "edit"},
            {"tool": "job", "job": {"job_id": "private-job-id", "status": "completed"}},
            {"tool": "write"},
        ]
        note = (
            "completed_phases:\n"
            "- phase_one\n"
            "next_phase: none\n"
            "job_id_overlap: private-job-id\n"
            "job_state_overlap: running\n"
            "job_state_overlap: completed\n"
        )

        summary = h007_results.summarize_continuation_document(
            note, calls, required_phases=("phase_one",),
        )

        self.assertTrue(summary["document_valid"])
        self.assertEqual(summary["completed_phases"], ["phase_one"])
        self.assertEqual(summary["missing_phases"], [])
        self.assertEqual(summary["next_phase"], "none")
        self.assertTrue(summary["test_job_lifecycle"]["all_terminal_observed"])
        self.assertFalse(summary["raw_note_retained"])
        self.assertFalse(summary["raw_job_ids_retained"])
        self.assertNotIn("private-job-id", json.dumps(summary))
        self.assertNotIn("job_id_overlap", json.dumps(summary))

        pending_note = note.replace("next_phase: none", "next_phase: phase_one")
        pending = h007_results.summarize_continuation_document(
            pending_note, calls, required_phases=("phase_one",),
        )
        self.assertFalse(pending["document_valid"])
        self.assertEqual(pending["next_phase"], "phase_one")

    def test_continuation_line_layout_requires_footer_after_progress(self):
        valid = (
            "completed_phases:\n- phase_one\n"
            "completed_contract_phases:\n- contract_one\n"
            "required_stages:\n- stage_one: complete\n"
            "gates:\n- gate_one: complete\n"
            "next_phase: none\nnext_contract_phase: none\n"
            "next_stage: none\nnext_gate: none\n"
        )
        projected = h007_results.summarize_continuation_line_layout(valid)
        self.assertTrue(projected["progress_sections_ordered"])
        self.assertTrue(projected["pointer_fields_follow_progress_sections"])
        self.assertTrue(projected["pointer_fields_in_expected_order"])
        self.assertTrue(projected["pointer_block_is_terminal"])
        self.assertFalse(projected["raw_note_retained"])
        self.assertNotIn("phase_one", json.dumps(projected))

        lines = valid.splitlines()
        gate_start = lines.index("gates:")
        pointer_start = lines.index("next_phase: none")
        misplaced = "\n".join(
            lines[:gate_start] + lines[pointer_start:] + lines[gate_start:pointer_start]
        ) + "\n"
        moved = h007_results.summarize_continuation_line_layout(misplaced)
        self.assertFalse(moved["pointer_fields_follow_progress_sections"])

        oversized = h007_results.summarize_continuation_line_layout(
            "x" * (h007_results.MAX_CONTINUATION_LAYOUT_CHARS + 1)
        )
        self.assertEqual(oversized["state"], "limit_exceeded")
        self.assertIsNone(oversized["progress_sections_ordered"])
        self.assertFalse(oversized["raw_note_retained"])


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
                       CODEGG_AUTONOMY_TRANSCRIPT="",
                       CARGO_TARGET_DIR=str(agent_root / "fixture" / "target"))
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
            env = dict(os.environ, CODEGG_AUTONOMY_CASE_ROOT=str(agent_root),
                       CARGO_TARGET_DIR=str(agent_root / "fixture" / "target"))
            result = subprocess.run(["python3", "-m", "unittest", "discover", "-s", "verifier"],
                                    cwd=private, env=env, capture_output=True, text=True, timeout=300)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("contract.json", result.stderr)


class ReviewerStartWindowCalibrationTests(unittest.TestCase):
    def test_s019_v94_moves_review_to_existing_15_minute_path_only(self):
        import json
        from pathlib import Path

        from dev.harness_lab.autonomy_run import _h007_review_uses_30_minute_threshold

        fixtures = Path(__file__).resolve().parents[1] / "fixtures"
        v93_path = fixtures / "agent_autonomy_s018_muse_continuation_v93" / "cases.jsonl"
        v94_path = fixtures / "agent_autonomy_s018_muse_continuation_v94" / "cases.jsonl"
        v93 = json.loads(v93_path.read_text(encoding="utf-8"))
        v94 = json.loads(v94_path.read_text(encoding="utf-8"))

        presentation_fields = {"group_id", "title", "prompt"}
        self.assertEqual(
            {key for key in v93 if v93.get(key) != v94.get(key)},
            presentation_fields,
        )
        self.assertTrue(_h007_review_uses_30_minute_threshold(v93["prompt"]))
        self.assertFalse(_h007_review_uses_30_minute_threshold(v94["prompt"]))
        self.assertIn("15-minute active-work threshold", v94["prompt"])
        self.assertIn("ATTEMPT 48 REVIEWER START-WINDOW CORRECTION", v94["prompt"])

    def test_s019_v95_prioritizes_seeded_repairs_and_workspace_gate(self):
        import json
        from pathlib import Path

        from dev.harness_lab.autonomy_run import _h007_review_uses_30_minute_threshold

        fixtures = Path(__file__).resolve().parents[1] / "fixtures"
        v94_path = fixtures / "agent_autonomy_s018_muse_continuation_v94" / "cases.jsonl"
        v95_path = fixtures / "agent_autonomy_s018_muse_continuation_v95" / "cases.jsonl"
        v94 = json.loads(v94_path.read_text(encoding="utf-8"))
        v95 = json.loads(v95_path.read_text(encoding="utf-8"))

        self.assertEqual(
            {key for key in v94 if v94.get(key) != v95.get(key)},
            {"group_id", "title", "prompt"},
        )
        prompt = v95["prompt"]
        self.assertIn("ATTEMPT 49 REQUIRED-GATE RECOVERY PRIORITY", prompt)
        self.assertIn("catalog_store", prompt)
        self.assertIn("both required `clippy` and `workspace_tests`", prompt)
        self.assertIn("by 40 minutes of active work", prompt)
        self.assertIn("every started native tool call has a terminal observation", prompt)
        self.assertFalse(_h007_review_uses_30_minute_threshold(prompt))

    def test_s019_v96_requires_post_failure_repairs_and_early_continuation_closeout(self):
        import json
        from pathlib import Path

        from dev.harness_lab.autonomy_run import _h007_review_uses_30_minute_threshold

        fixtures = Path(__file__).resolve().parents[1] / "fixtures"
        v95_path = fixtures / "agent_autonomy_s018_muse_continuation_v95" / "cases.jsonl"
        v96_path = fixtures / "agent_autonomy_s018_muse_continuation_v96" / "cases.jsonl"
        v95 = json.loads(v95_path.read_text(encoding="utf-8"))
        v96 = json.loads(v96_path.read_text(encoding="utf-8"))

        self.assertEqual(
            {key for key in v95 if v95.get(key) != v96.get(key)},
            {"group_id", "title", "prompt"},
        )
        prompt = v96["prompt"]
        self.assertIn("ATTEMPT 50 REPAIR-PAIR INTEGRITY AND CONTINUATION CLOSEOUT", prompt)
        self.assertIn("A source edit made before observing the seed failure does not count", prompt)
        self.assertIn("Preserve the already-qualified `catalog_store` pair", prompt)
        self.assertIn("all three next-boundary fields must explicitly be `none`", prompt)
        self.assertIn("End normally by 50 minutes of active work", prompt)
        self.assertFalse(_h007_review_uses_30_minute_threshold(prompt))


class RepairCycleEvidenceTests(unittest.TestCase):
    groups = (
        ("catalog_format", "catalog_format_seed", "catalog_format_repair"),
        ("catalog_store", "catalog_store_seed", "catalog_store_repair"),
    )

    @staticmethod
    def _calls(*, insert_between_seed_submit_and_status: bool = False,
               omit_source_change: bool = False):
        calls = []
        for target, seed_label, repair_label in RepairCycleEvidenceTests.groups:
            seed_id = f"{target}-seed-id"
            repair_id = f"{target}-repair-id"
            calls.append({"tool": "test", "job": {
                "job_id": seed_id, "action": "submit", "status": "queued"}})
            if insert_between_seed_submit_and_status:
                calls.append({"tool": "read"})
            calls.append({"tool": "job", "job": {
                "job_id": seed_id, "action": "status", "status": "failed"}})
            if not omit_source_change:
                calls.append({"tool": "edit"})
            calls.append({"tool": "test", "job": {
                "job_id": repair_id, "action": "submit", "status": "queued"}})
            calls.append({"tool": "job", "job": {
                "job_id": repair_id, "action": "status", "status": "completed"}})
        return calls

    @staticmethod
    def _note(cycles: int = 2) -> str:
        rows = [f"repair_cycles: {cycles}"]
        for target, seed_label, repair_label in RepairCycleEvidenceTests.groups:
            rows.extend((
                f"job_id_{seed_label}: {target}-seed-id",
                f"job_state_{seed_label}: failed",
                f"job_id_{repair_label}: {target}-repair-id",
                f"job_state_{repair_label}: completed",
            ))
        return "\n".join(rows) + "\n"

    def test_named_repair_pairs_require_exact_immediate_status_and_source_work(self):
        calls = self._calls()
        summary = h007_results.summarize_required_repair_jobs(
            self._note(), calls, self.groups,
        )

        self.assertTrue(summary["complete"])
        self.assertEqual(summary["qualified_pair_count"], 2)
        self.assertEqual(summary["observed_repair_cycle_count"], 2)
        self.assertTrue(summary["repair_cycle_count_matches"])
        self.assertFalse(summary["raw_job_ids_retained"])
        self.assertNotIn("catalog_format-seed-id", json.dumps(summary))
        self.assertNotIn("catalog_format_repair-id", json.dumps(summary))

        delayed_status = h007_results.summarize_required_repair_jobs(
            self._note(), self._calls(insert_between_seed_submit_and_status=True), self.groups,
        )
        self.assertFalse(delayed_status["groups"]["catalog_format"][
            "failed_job_status_called_immediately"])
        self.assertFalse(delayed_status["complete"])

        missing_edit = h007_results.summarize_required_repair_jobs(
            self._note(), self._calls(omit_source_change=True), self.groups,
        )
        self.assertFalse(missing_edit["groups"]["catalog_store"][
            "productive_change_between_failure_and_rerun"])
        self.assertFalse(missing_edit["complete"])

        shell_only_calls = self._calls()
        for call in shell_only_calls:
            if call.get("tool") == "edit":
                call["tool"] = "bash"
        shell_only = h007_results.summarize_required_repair_jobs(
            self._note(), shell_only_calls, self.groups,
        )
        self.assertFalse(shell_only["groups"]["catalog_format"][
            "productive_change_between_failure_and_rerun"])
        self.assertFalse(shell_only["complete"])

    def test_followup_reports_observed_and_documented_repair_count_mismatch(self):
        from dev.harness_lab.autonomy_run import _h007_followup_prompt

        calls = self._calls()
        for index in range(5):
            seed_id = f"extra-seed-{index}"
            repair_id = f"extra-repair-{index}"
            calls.extend((
                {"tool": "test", "job": {"job_id": seed_id, "action": "submit"}},
                {"tool": "job", "job": {"job_id": seed_id, "action": "status",
                                            "status": "failed"}},
                {"tool": "edit"},
                {"tool": "test", "job": {"job_id": repair_id, "action": "submit"}},
                {"tool": "job", "job": {"job_id": repair_id, "action": "status",
                                            "status": "completed"}},
            ))
        v86_manifest = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v86"
            / "cases.jsonl"
        )
        case_prompt = autonomy.load_autonomy_suite(v86_manifest.parent).case(
            "build-provenance-catalog").prompt
        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            (fixture / "CONTINUATION.md").write_text(
                self._note(cycles=2), encoding="utf-8")
            prompt = _h007_followup_prompt(
                fixture, calls, 1600, 1200, case_prompt=case_prompt,
            )

        self.assertIn("observes 7 qualifying cycle(s)", prompt)
        self.assertIn("CONTINUATION.md documents 2", prompt)
        self.assertIn("Update `repair_cycles` to 7 now", prompt)
        self.assertIn("named-pair requirement is a minimum", prompt)
        self.assertNotIn("catalog_format-seed-id", prompt)
        self.assertNotIn("catalog_store-repair-id", prompt)
        self.assertNotIn("extra-seed-0", prompt)
        self.assertNotIn("extra-repair-0", prompt)

    def test_one_later_success_cannot_satisfy_multiple_failures(self):
        from dev.harness_lab.autonomy_run import _h007_repair_cycles

        calls = []
        for job_id in ("failed-one", "failed-two", "failed-three"):
            calls.extend((
                {"tool": "test", "job": {"job_id": job_id, "action": "submit"}},
                {"tool": "job", "job": {"job_id": job_id, "action": "status",
                                          "status": "failed"}},
            ))
        calls.extend((
            {"tool": "edit"},
            {"tool": "test", "job": {"job_id": "one-success", "action": "submit"}},
            {"tool": "job", "job": {"job_id": "one-success", "action": "status",
                                       "status": "completed"}},
        ))
        self.assertEqual(_h007_repair_cycles(calls), 1)
        self.assertEqual(len(h007_results._observed_repair_cycle_pairs(calls)), 1)

    def test_repeated_terminal_failure_status_does_not_hide_correlated_repair(self):
        calls = [
            {"tool": "test", "job": {"job_id": "format-seed", "action": "submit"}},
            {"tool": "job", "job": {"job_id": "format-seed", "action": "status",
                                      "status": "failed"}},
            {"tool": "edit"},
            # A duplicate terminal poll after the source edit must not move the
            # observed failure past the productive repair action.
            {"tool": "job", "job": {"job_id": "format-seed", "action": "status",
                                      "status": "failed"}},
            {"tool": "test", "job": {"job_id": "format-repair", "action": "submit"}},
            {"tool": "job", "job": {"job_id": "format-repair", "action": "status",
                                      "status": "running"}},
            {"tool": "job", "job": {"job_id": "format-repair", "action": "status",
                                      "status": "completed"}},
        ]
        note = (
            "repair_cycles: 1\n"
            "job_id_catalog_format_seed: format-seed\n"
            "job_state_catalog_format_seed: failed\n"
            "job_id_catalog_format_repair: format-repair\n"
            "job_state_catalog_format_repair: completed\n"
        )
        groups = (("catalog_format", "catalog_format_seed", "catalog_format_repair"),)

        summary = h007_results.summarize_required_repair_jobs(note, calls, groups)
        pairs = h007_results._observed_repair_cycle_pairs(
            calls,
            preferred_job_pairs=h007_results.documented_repair_job_id_pairs(note, groups),
        )

        self.assertTrue(summary["groups"]["catalog_format"]["qualified"])
        self.assertTrue(summary["groups"]["catalog_format"]["repair_cycle_correlated"])
        self.assertEqual(summary["qualified_pair_count"], 1)
        self.assertEqual(summary["observed_repair_cycle_count"], 1)
        self.assertTrue(summary["complete"])
        self.assertEqual(len(pairs), 1)
        self.assertFalse(summary["raw_job_ids_retained"])
        self.assertFalse(summary["raw_job_arguments_retained"])

    def test_named_repair_pair_wins_over_an_unrelated_later_success(self):
        calls = [
            {"tool": "test", "job": {"job_id": "format-seed", "action": "submit"}},
            {"tool": "job", "job": {"job_id": "format-seed", "action": "status",
                                      "status": "failed"}},
            {"tool": "edit"},
            {"tool": "test", "job": {"job_id": "unrelated-success", "action": "submit"}},
            {"tool": "job", "job": {"job_id": "unrelated-success", "action": "status",
                                       "status": "completed"}},
            {"tool": "edit"},
            {"tool": "test", "job": {"job_id": "format-repair", "action": "submit"}},
            {"tool": "job", "job": {"job_id": "format-repair", "action": "status",
                                       "status": "completed"}},
        ]
        note = (
            "repair_cycles: 1\n"
            "job_id_catalog_format_seed: format-seed\n"
            "job_state_catalog_format_seed: failed\n"
            "job_id_catalog_format_repair: format-repair\n"
            "job_state_catalog_format_repair: completed\n"
        )
        group = (("catalog_format", "catalog_format_seed", "catalog_format_repair"),)
        preferred = h007_results.documented_repair_job_id_pairs(note, group)
        pairs = h007_results._observed_repair_cycle_pairs(
            calls, preferred_job_pairs=preferred,
        )
        summary = h007_results.summarize_required_repair_jobs(note, calls, group)

        self.assertEqual(preferred, (("format-seed", "format-repair"),))
        self.assertEqual(len(pairs), 1)
        self.assertEqual(pairs[0]["repair_job_id"], "format-repair")
        self.assertTrue(summary["complete"])

    def test_continuation_gate_rejects_missing_or_duplicate_repair_evidence(self):
        calls = self._calls()
        note = (
            "completed_phases:\n- phase_one\nnext_phase: none\n" + self._note()
        )
        self.assertTrue(h007_results.continuation_document_confirmed(
            note, calls, required_phases=("phase_one",),
            required_repair_job_pairs=self.groups,
        ))
        missing = note.replace("job_state_catalog_store_repair: completed\n", "")
        self.assertFalse(h007_results.continuation_document_confirmed(
            missing, calls, required_phases=("phase_one",),
            required_repair_job_pairs=self.groups,
        ))
        duplicated = note + "job_id_catalog_format_seed: catalog-format-seed-id\n"
        self.assertFalse(h007_results.continuation_document_confirmed(
            duplicated, calls, required_phases=("phase_one",),
            required_repair_job_pairs=self.groups,
        ))

    def test_v80_prompt_only_changes_the_manifest_presentation_and_names_repairs(self):
        previous = json.loads(S018_V79_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(S018_V80_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(S018_V80_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-diagnostic-v80")
        self.assertIn("S019 attempt 25", case.prompt)
        for label in ("catalog_format_seed", "catalog_format_repair",
                      "catalog_store_seed", "catalog_store_repair"):
            self.assertIn(f"job_id_{label}", case.prompt)
            self.assertIn(f"job_state_{label}", case.prompt)
        self.assertIn("the harness checks its trace", case.prompt)
        self.assertIn("edit source before one distinct repair submit", case.prompt)
        self.assertLess(len(case.prompt), 10_000)
        from dev.harness_lab.autonomy_run import _h007_followup_prompt
        with tempfile.TemporaryDirectory() as temporary:
            followup = _h007_followup_prompt(
                pathlib.Path(temporary), [], case.budget["max_tool_calls"],
                case.budget["max_provider_requests"], case_prompt=case.prompt,
            )
        for label in ("catalog_format_seed", "catalog_format_repair",
                      "catalog_store_seed", "catalog_store_repair"):
            self.assertIn(f"job_id_{label}", followup)
            self.assertIn(f"job_state_{label}", followup)
        self.assertIn("immediately next native call `job status`", followup)
        self.assertIn("distinct observed failed-job", followup)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v80 changed manifest field {key}")

    def test_v81_requires_terminal_seed_failure_before_direct_repair(self):
        previous = json.loads(S018_V80_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(S018_V81_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(S018_V81_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-diagnostic-v81")
        self.assertIn("S019 attempt 26", case.prompt)
        self.assertIn(
            "If running, keep waiting/statusing that same ID until terminal failed",
            case.prompt,
        )
        self.assertIn(
            "Do not edit that target or submit repair until terminal failed is observed",
            case.prompt,
        )
        self.assertIn("immediately status its exact ID", case.prompt)
        self.assertIn("repair_cycles equals all observed one-to-one", case.prompt)
        self.assertIn("including any additional qualifying pairs", case.prompt)
        self.assertIn("end immediately; do not continue with optional rework", case.prompt)
        self.assertLess(len(case.prompt), 10_000)

        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v81 changed manifest field {key}")

    def test_v82_restores_named_repair_projection_markers(self):
        previous = json.loads(S018_V81_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(S018_V82_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        previous_case = autonomy.load_autonomy_suite(S018_V81_MANIFEST.parent).case(
            "build-provenance-catalog")
        case = autonomy.load_autonomy_suite(S018_V82_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-diagnostic-v82")
        self.assertIn("S019 attempt 27", case.prompt)
        self.assertIn("terminal failed; running is not failure", case.prompt)
        self.assertIn("Do not edit that target or submit repair until terminal failed is observed",
                      case.prompt)
        self.assertIn("make one direct fix, submit one repair test", case.prompt)
        self.assertIn("poll to terminal completed", case.prompt)
        self.assertIn("repair_cycles equals all observed one-to-one", case.prompt)
        for label in ("catalog_format_seed", "catalog_format_repair",
                      "catalog_store_seed", "catalog_store_repair"):
            self.assertIn(f"job_id_{label}", case.prompt)
            self.assertIn(f"job_state_{label}", case.prompt)

        from dev.harness_lab.autonomy_run import _h007_case_requires_repair_job_pairs
        self.assertFalse(_h007_case_requires_repair_job_pairs(previous_case.prompt))
        self.assertTrue(_h007_case_requires_repair_job_pairs(case.prompt))
        self.assertLess(len(case.prompt), 10_000)

        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v82 changed manifest field {key}")

    def test_v83_requires_exact_seed_status_as_the_next_native_call(self):
        previous = json.loads(S018_V82_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(S018_V83_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(S018_V83_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-diagnostic-v83")
        self.assertIn("S019 attempt 28", case.prompt)
        self.assertIn("the next native call must be `job status` for that exact returned ID",
                      case.prompt)
        self.assertIn("no other call may intervene", case.prompt)
        self.assertIn("immediately status its exact ID", case.prompt)
        for label in ("catalog_format_seed", "catalog_format_repair",
                      "catalog_store_seed", "catalog_store_repair"):
            self.assertIn(f"job_id_{label}", case.prompt)
            self.assertIn(f"job_state_{label}", case.prompt)

        from dev.harness_lab.autonomy_run import _h007_case_requires_repair_job_pairs
        self.assertTrue(_h007_case_requires_repair_job_pairs(case.prompt))
        self.assertLess(len(case.prompt), 10_000)

        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v83 changed manifest field {key}")

    def test_v84_reconciles_only_safe_attempt_28_failure_domains(self):
        previous = json.loads(S018_V83_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(S018_V84_MANIFEST.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(S018_V84_MANIFEST.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-verifier-reconciliation-v84")
        self.assertIn("S019 attempt 29", case.prompt)
        self.assertIn("Attempt 28 completed 35.9 minutes of active work", case.prompt)
        self.assertIn("continuation-boundary and transcript job-lifecycle domains", case.prompt)
        self.assertIn("Do not infer hidden assertion details", case.prompt)
        self.assertIn("Do not inspect the hidden verifier", case.prompt)
        self.assertIn("reserve one claim for the exact-ID overlap and repair sequence",
                      case.prompt)
        self.assertLess(len(case.prompt), 12_000)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v84 changed manifest field {key}")
        self.assertEqual(_sha(S018_V84_MANIFEST),
                         "b95124e46c1ecf325d38d54e6d91723e8043ef3e790b3e2345fd30a2288554ce")

    def test_v86_uses_live_repair_count_feedback_and_preserves_case_contract(self):
        previous_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v85"
            / "cases.jsonl"
        )
        current_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v86"
            / "cases.jsonl"
        )
        previous = json.loads(previous_path.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(current_path.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(current_path.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-live-repair-count-feedback-v86")
        self.assertIn("ATTEMPT 30 RESULT", case.prompt)
        self.assertIn("The harness follow-up now reports the current number", case.prompt)
        self.assertIn("Do not hardcode attempt 30's count", case.prompt)
        self.assertIn("private causal trace", case.prompt)
        self.assertLess(len(case.prompt), 16_000)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v86 changed manifest field {key}")
        self.assertEqual(_sha(current_path),
                         "be4abad68e0ef9173a8e3190ac385857910a2d3dcb9256e68c097a7abd437ef5")

    def test_v87_uses_safe_sequence_diagnostic_and_preserves_case_contract(self):
        previous_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v86"
            / "cases.jsonl"
        )
        current_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v87"
            / "cases.jsonl"
        )
        previous = json.loads(previous_path.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(current_path.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(current_path.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-public-sequence-audit-v87")
        self.assertIn("ATTEMPT 32 RESULT — TASK STILL UNQUALIFIED", case.prompt)
        self.assertIn("one continuation-boundary check and one transcript/job-lifecycle check",
                      case.prompt)
        self.assertIn("Use native CodeGG tools to audit the public task contract", case.prompt)
        self.assertIn("Use one read-only task subagent", case.prompt)
        self.assertIn("missing, duplicated, reordered, and conflicting states", case.prompt)
        self.assertLess(len(case.prompt), 16_000)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v87 changed manifest field {key}")
        self.assertEqual(_sha(previous_path),
                         "be4abad68e0ef9173a8e3190ac385857910a2d3dcb9256e68c097a7abd437ef5")
        self.assertEqual(_sha(current_path),
                         "3c3669423d0cf5fde4f65be43a1491e91c1ff01df8572d31119250dcc35eba83")

    def test_v88_prioritizes_serial_repair_pairs_and_preserves_case_contract(self):
        previous_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v86"
            / "cases.jsonl"
        )
        current_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v88"
            / "cases.jsonl"
        )
        previous = json.loads(previous_path.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(current_path.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(current_path.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-per-pair-feedback-v88")
        self.assertIn("catalog_format` passed its required failure-to-repair sequence",
                      case.prompt)
        self.assertIn("catalog_store` did not", case.prompt)
        self.assertIn("Do not queue both seed jobs before repairing either one", case.prompt)
        self.assertIn("one read-only task subagent", case.prompt)
        self.assertIn("documented reversible probes", case.prompt)
        self.assertIn("bounded read-only helper", case.prompt)
        self.assertNotIn("PUBLIC SEQUENCE AUDIT", case.prompt)
        self.assertLess(len(case.prompt), 16_000)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v88 changed manifest field {key}")
        self.assertEqual(_sha(previous_path),
                         "be4abad68e0ef9173a8e3190ac385857910a2d3dcb9256e68c097a7abd437ef5")
        self.assertEqual(_sha(current_path),
                         "57bf7b463f14e2b2c6f6aedf693b74e207bc88699143dd9df817135561d77113")

    def test_v89_paces_public_gates_and_requires_complete_review_receipt(self):
        previous_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v88"
            / "cases.jsonl"
        )
        current_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v89"
            / "cases.jsonl"
        )
        previous = json.loads(previous_path.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(current_path.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(current_path.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-gate-pacing-review-receipt-v89")
        self.assertIn("ATTEMPT 34 RESULT — TASK STILL UNQUALIFIED", case.prompt)
        self.assertIn("`bounded_probes` and `independent_review` incomplete", case.prompt)
        self.assertIn("Do not postpone `bounded_probes` until final closeout", case.prompt)
        self.assertIn("fetch its matching Task result exactly once", case.prompt)
        for field in (
            "`review_agent`", "`review_task`", "`review_run`", "`review_status`",
            "`review_findings`",
        ):
            self.assertIn(field, case.prompt)
        self.assertIn("2,700 seconds of active work", case.prompt)
        self.assertIn("final 300 seconds for required closeout", case.prompt)
        self.assertLess(len(case.prompt), 16_000)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v89 changed manifest field {key}")
        self.assertEqual(_sha(previous_path),
                         "57bf7b463f14e2b2c6f6aedf693b74e207bc88699143dd9df817135561d77113")
        self.assertEqual(_sha(current_path),
                         "48ede91e68a33949f5e4f50874cb4451d89da11ccad90240206431a1885b274d")

    def test_v90_requires_locked_builds_from_a_clean_relocated_copy(self):
        previous_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v89"
            / "cases.jsonl"
        )
        current_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v90"
            / "cases.jsonl"
        )
        previous = json.loads(previous_path.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(current_path.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(current_path.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-relocated-workspace-v90")
        self.assertIn("ATTEMPT 35 RESULT — PUBLIC TASK COMPLETE, VERIFIER UNAVAILABLE",
                      case.prompt)
        self.assertIn("Task correctness is inconclusive", case.prompt)
        self.assertIn("new temporary path", case.prompt)
        self.assertIn("cargo metadata --locked --no-deps --format-version 1", case.prompt)
        self.assertIn("cargo test --locked --workspace --all-targets", case.prompt)
        self.assertIn("cargo clippy --locked --workspace --all-targets -- -D warnings",
                      case.prompt)
        self.assertIn("Remove the temporary copy and target afterward", case.prompt)
        self.assertIn("do not invoke or inspect the hidden verifier", case.prompt)
        self.assertLess(len(case.prompt), 16_000)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v90 changed manifest field {key}")
        self.assertEqual(_sha(previous_path),
                         "48ede91e68a33949f5e4f50874cb4451d89da11ccad90240206431a1885b274d")
        self.assertEqual(_sha(current_path),
                         "71d2fa63acf3492358509975c6aaa80aa5a15a3929cdf6b5d0105afff82d9fa5")

    def test_v91_integrates_relocated_builds_and_removes_stale_attempt_narrative(self):
        previous_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v89"
            / "cases.jsonl"
        )
        current_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v91"
            / "cases.jsonl"
        )
        previous = json.loads(previous_path.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(current_path.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(current_path.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-integrated-workspace-qualification-v91")
        self.assertIn("ATTEMPT 35 RESULT", case.prompt)
        self.assertIn("ATTEMPT 36 RESULT", case.prompt)
        self.assertIn("Integrate clean relocated workspace checks into the existing",
                      case.prompt)
        self.assertIn("no later than 2,400 active seconds", case.prompt)
        self.assertIn("At the 30-minute active-work follow-up marker", case.prompt)
        self.assertIn("cargo metadata --locked --no-deps --format-version 1", case.prompt)
        self.assertIn("cargo test --locked --workspace --all-targets", case.prompt)
        self.assertIn("cargo clippy --locked --workspace --all-targets -- -D warnings",
                      case.prompt)
        self.assertNotIn("ATTEMPT 34 RESULT", case.prompt)
        self.assertNotIn("ATTEMPT 34 SEQUENCE PRIORITY", case.prompt)
        self.assertLess(len(case.prompt), 16_000)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v91 changed manifest field {key}")
        self.assertEqual(_sha(previous_path),
                         "48ede91e68a33949f5e4f50874cb4451d89da11ccad90240206431a1885b274d")
        self.assertEqual(_sha(current_path),
                         "9e2052da9c1e45dbc5eebd136c5a290e852c1248c0f77ec87399624071c4524d")

    def test_v92_recovers_transient_reviewer_deadline_on_same_run(self):
        previous_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v91"
            / "cases.jsonl"
        )
        current_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v92"
            / "cases.jsonl"
        )
        previous = json.loads(previous_path.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(current_path.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(current_path.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-reviewer-deadline-recovery-v92")
        self.assertIn("ATTEMPT 37 RESULT", case.prompt)
        self.assertIn("deadline_exceeded", case.prompt)
        self.assertIn("1,205,086 ms", case.prompt)
        self.assertIn("action=continue", case.prompt)
        self.assertIn("same `run_id` exactly once", case.prompt)
        self.assertIn("Do not continue `provider_authentication` or `cancelled`",
                      case.prompt)
        self.assertIn("Keep the audit to three checks", case.prompt)
        self.assertNotIn("ATTEMPT 35 RESULT", case.prompt)
        self.assertNotIn("ATTEMPT 36 RESULT", case.prompt)
        self.assertNotIn("oc_sk_", case.prompt)
        self.assertLess(len(case.prompt), 16_000)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v92 changed manifest field {key}")
        self.assertEqual(_sha(previous_path),
                         "9e2052da9c1e45dbc5eebd136c5a290e852c1248c0f77ec87399624071c4524d")
        self.assertEqual(_sha(current_path),
                         "45089183e55c8a5f070fc8b8130ef73a51951352be1f8e4c3eda34e935e22134")

    def test_v93_prioritizes_attempt38_repair_sequence_without_changing_contract(self):
        previous_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v92"
            / "cases.jsonl"
        )
        current_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v93"
            / "cases.jsonl"
        )
        previous = json.loads(previous_path.read_text(encoding="utf-8").splitlines()[0])
        current = json.loads(current_path.read_text(encoding="utf-8").splitlines()[0])
        case = autonomy.load_autonomy_suite(current_path.parent).case(
            "build-provenance-catalog")

        self.assertEqual(case.group_id, "s019-muse-prioritized-repair-v93")
        self.assertIn("ATTEMPT 38 RESULT", case.prompt)
        self.assertIn("zero intervening tool calls", case.prompt)
        self.assertIn("The two required named pairs are still unqualified", case.prompt)
        self.assertIn("Do not submit the distinct repair test until after that edit",
                      case.prompt)
        self.assertIn("failed → edit → completed repair pair", case.prompt)
        self.assertIn("unchanged independent verifier", case.prompt)
        self.assertNotIn("ATTEMPT 37 RESULT", case.prompt)
        self.assertNotIn("oc_sk_", case.prompt)
        self.assertLess(len(case.prompt), 16_000)
        stable_fields = set(previous) - {"group_id", "title", "prompt"}
        self.assertEqual(stable_fields, set(current) - {"group_id", "title", "prompt"})
        for key in stable_fields:
            self.assertEqual(current[key], previous[key], f"v93 changed manifest field {key}")
        self.assertEqual(_sha(previous_path),
                         "45089183e55c8a5f070fc8b8130ef73a51951352be1f8e4c3eda34e935e22134")
        self.assertEqual(_sha(current_path),
                         "1d7914eec515c161a683b6ef8bc2464760638ee91c2871d6b953db6e1f2a086e")

    def test_followup_names_incomplete_repair_pair_without_job_ids(self):
        from unittest.mock import patch
        from dev.harness_lab import autonomy_run
        from dev.harness_lab.autonomy_run import _h007_followup_prompt

        case_path = (
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v88"
            / "cases.jsonl"
        )
        case = autonomy.load_autonomy_suite(case_path.parent).case(
            "build-provenance-catalog")
        diagnostic = {
            "complete": False,
            "observed_repair_cycle_count": 1,
            "documented_repair_cycle_count": 1,
            "groups": {
                "catalog_format": {"qualified": True},
                "catalog_store": {
                    "qualified": False,
                    "failed_terminal_observed": True,
                    "repair_followed_failure": False,
                    "productive_change_between_failure_and_rerun": False,
                    "repair_terminal_observed": True,
                    "job_id": "unsafe-private-job-id",
                },
            },
            "raw_job_ids_retained": False,
            "raw_job_arguments_retained": False,
        }
        with tempfile.TemporaryDirectory() as temporary:
            with patch.object(
                    autonomy_run.h007_results, "summarize_required_repair_jobs",
                    return_value=diagnostic):
                prompt = _h007_followup_prompt(
                    pathlib.Path(temporary), case_prompt=case.prompt)

        self.assertTrue(prompt.startswith(
            "Your previous turn ended normally, but the runner has not confirmed qualification. "
            "TOP PRIORITY"))
        self.assertIn("do not rerun unchanged source", prompt[:900])
        self.assertIn("catalog-store", prompt)
        self.assertIn("observe the seed's terminal failure before submitting its repair rerun",
                      prompt)
        self.assertIn("make a correctness-bearing source change after the observed failure",
                      prompt)
        self.assertNotIn("unsafe-private-job-id", prompt)
        self.assertNotIn("catalog-format:", prompt)


if __name__ == "__main__":
    unittest.main()
