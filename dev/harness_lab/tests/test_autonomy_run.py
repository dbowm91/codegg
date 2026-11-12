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

import hashlib
import json
import os
import pathlib
import subprocess
import tempfile
import unittest
from typing import Any
from unittest import mock

from dev.harness_lab import autonomy, autonomy_run, budget, h007_results, scoring
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


class JobIdArgumentTelemetryTests(unittest.TestCase):
    def test_failed_unbound_job_call_keeps_only_argument_shape(self) -> None:
        projector = autonomy_run._ToolCallProjector()
        for call_id, argument_state in (("missing", "missing"), ("present", "string")):
            projector.absorb({"method": "session/update", "_received_elapsed_ms": 10,
                              "params": {"update": {
                                  "sessionUpdate": "tool_call", "toolCallId": call_id,
                                  "title": "job", "codeggAction": "status",
                                  "codeggJobIdArgumentState": argument_state,
                                  "codeggArgumentsSha256": "a" * 64,
                              }}})
            projector.absorb({"method": "session/update", "_received_elapsed_ms": 20,
                              "params": {"update": {
                                  "sessionUpdate": "tool_call_update", "toolCallId": call_id,
                                  "status": "failed", "rawOutput": "private error details",
                              }}})

        calls = projector.calls()
        self.assertEqual([call["job_id_argument_state"] for call in calls],
                         ["missing", "string"])
        safe = autonomy_run.diagnostics.summarize_job_tool_sequence(calls)
        self.assertEqual(safe["unbound_job_tool_call_count"], 2)
        self.assertEqual(safe["unbound_call_state_counts"], {
            "status:missing:failed": 1,
            "status:string:failed": 1,
        })
        encoded = json.dumps(calls)
        self.assertNotIn("private error details", encoded)
        self.assertNotIn("private-job-id", encoded)

    def test_failed_background_test_submit_keeps_only_allowlisted_error_class(self) -> None:
        projector = autonomy_run._ToolCallProjector()
        for call_id, error_class in (
                ("typed", "tool_execution_failed"),
                ("unknown", "PRIVATE_ERROR_CLASS")):
            projector.absorb({"method": "session/update", "_received_elapsed_ms": 10,
                              "params": {"update": {
                                  "sessionUpdate": "tool_call", "toolCallId": call_id,
                                  "title": "test", "codeggAction": "submit",
                                  "codeggArgumentsSha256": "a" * 64,
                              }}})
            projector.absorb({"method": "session/update", "_received_elapsed_ms": 20,
                              "params": {"update": {
                                  "sessionUpdate": "tool_call_update", "toolCallId": call_id,
                                  "status": "failed", "rawOutput": "PRIVATE_PATH PRIVATE_TEST",
                                  "codeggToolErrorClass": error_class,
                              }}})

        calls = projector.calls()
        self.assertEqual(calls[0]["tool_error_class"], "tool_execution_failed")
        self.assertNotIn("tool_error_class", calls[1])
        summary = autonomy_run.diagnostics.summarize_job_tool_sequence(calls)
        self.assertEqual(summary["failed_test_submit_error_class_counts"], {
            "tool_execution_failed": 1,
            "unobserved": 1,
        })
        encoded = json.dumps({"calls": calls, "summary": summary})
        for private_value in ("PRIVATE_PATH", "PRIVATE_TEST", "PRIVATE_ERROR_CLASS"):
            self.assertNotIn(private_value, encoded)

    def test_job_id_request_digest_matches_or_rejects_result_identity_privately(self) -> None:
        requested_id = "5a4c2d88-29a7-4ea7-b617-1833bcb6c80b"
        other_id = "efb875a1-5c33-4148-ae3a-20b3e82061aa"
        digest = hashlib.sha256(requested_id.encode("utf-8")).hexdigest()
        projector = autonomy_run._ToolCallProjector()
        for call_id, result_id in (("match", requested_id), ("mismatch", other_id)):
            projector.absorb({"method": "session/update", "_received_elapsed_ms": 10,
                              "params": {"update": {
                                  "sessionUpdate": "tool_call", "toolCallId": call_id,
                                  "title": "job", "codeggAction": "status",
                                  "codeggJobIdArgumentState": "string",
                                  "codeggJobIdArgumentSha256": digest,
                                  "codeggArgumentsSha256": "a" * 64,
                              }}})
            projector.absorb({"method": "session/update", "_received_elapsed_ms": 20,
                              "params": {"update": {
                                  "sessionUpdate": "tool_call_update", "toolCallId": call_id,
                                  "status": "completed",
                                  "rawOutput": f"job running {result_id}; private result text",
                              }}})

        calls = projector.calls()
        self.assertEqual([call["job_id_argument_matches_result"] for call in calls],
                         ["matched", "mismatched"])
        safe = autonomy_run.diagnostics.summarize_job_tool_sequence(calls)
        correlation = safe["job_id_request_result_correlation"]
        self.assertEqual(correlation["state_counts"], {"matched": 1, "mismatched": 1})
        self.assertFalse(correlation["all_applicable_calls_match"])
        encoded = json.dumps(safe)
        self.assertNotIn(requested_id, encoded)
        self.assertNotIn(other_id, encoded)
        self.assertNotIn(digest, encoded)
        self.assertNotIn("private result text", encoded)


class SerializedTranscriptAuditTests(unittest.TestCase):
    def _calls(self) -> list[dict[str, Any]]:
        job_id = "5a4c2d88-29a7-4ea7-b617-1833bcb6c80b"
        digest = hashlib.sha256(job_id.encode("utf-8")).hexdigest()
        return [
            {"index": 0, "tool": "test", "status": "completed",
             "started_elapsed_ms": 10, "finished_elapsed_ms": 20,
             "job": {"job_id": job_id, "action": "submit", "status": "running"}},
            {"index": 1, "tool": "job", "status": "completed",
             "started_elapsed_ms": 30, "finished_elapsed_ms": 40,
             "job_id_argument_state": "string",
             "job_id_argument_matches_result": "matched",
             "job": {"job_id": job_id, "action": "status", "status": "running"}},
            {"index": 2, "tool": "write", "status": "completed",
             "started_elapsed_ms": 50, "finished_elapsed_ms": 60},
            {"index": 3, "tool": "job", "status": "completed",
             "started_elapsed_ms": 70, "finished_elapsed_ms": 80,
             "job_id_argument_state": "string",
             "job_id_argument_matches_result": "matched",
             "job": {"job_id": job_id, "action": "status", "status": "completed"}},
            {"index": 4, "tool": "read", "status": "completed",
             "started_elapsed_ms": 90, "finished_elapsed_ms": 100,
             "job_id_argument_sha256": digest,
             "private_output": "serialized secret sentinel"},
        ]

    def test_exact_persisted_sequence_is_audited_without_retaining_transcript_data(self) -> None:
        calls = self._calls()
        case = autonomy.load_autonomy_suite(
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_h007"
        ).case("build-provenance-catalog")
        transcript = autonomy_run.build_transcript(
            case, calls, terminal="completed", stop_reason="end_turn")
        with tempfile.TemporaryDirectory() as temporary:
            path = pathlib.Path(temporary) / "TRANSCRIPT.json"
            path.write_text(json.dumps(transcript, sort_keys=True), encoding="utf-8")
            diagnostic = autonomy_run.diagnostics.audit_serialized_transcript(
                path, calls, expected_case_id=case.case_id,
                expected_schema_version=transcript["schema_version"],
            )
            self.assertEqual(diagnostic["state"], "observed")
            self.assertTrue(diagnostic["lifecycle_projection_matches_source"])
            self.assertTrue(diagnostic["job_id_request_result_correlation"][
                "all_applicable_calls_match"])
            encoded = json.dumps(diagnostic)
            self.assertNotIn("5a4c2d88-29a7-4ea7-b617-1833bcb6c80b", encoded)
            self.assertNotIn("serialized secret sentinel", encoded)
            self.assertNotIn("TRANSCRIPT.json", encoded)
            self.assertFalse(diagnostic["raw_transcript_retained"])
            self.assertFalse(diagnostic["job_id_digests_retained"])

    def test_serialized_mutation_malformed_json_and_size_limit_fail_closed(self) -> None:
        calls = self._calls()
        case = autonomy.load_autonomy_suite(
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_h007"
        ).case("build-provenance-catalog")
        transcript = autonomy_run.build_transcript(
            case, calls, terminal="completed", stop_reason="end_turn")
        with tempfile.TemporaryDirectory() as temporary:
            path = pathlib.Path(temporary) / "TRANSCRIPT.json"
            changed = dict(transcript)
            changed["tool_calls"] = [dict(call) for call in calls]
            changed["tool_calls"][1] = {
                **changed["tool_calls"][1],
                "job": {**changed["tool_calls"][1]["job"], "status": "failed"},
            }
            path.write_text(json.dumps(changed), encoding="utf-8")
            diagnostic = autonomy_run.diagnostics.audit_serialized_transcript(
                path, calls, expected_case_id=case.case_id,
                expected_schema_version=transcript["schema_version"],
            )
            self.assertFalse(diagnostic["lifecycle_projection_matches_source"])

            path.write_text("{invalid", encoding="utf-8")
            malformed = autonomy_run.diagnostics.audit_serialized_transcript(
                path, calls, expected_case_id=case.case_id,
                expected_schema_version=transcript["schema_version"],
            )
            self.assertEqual(malformed["state"], "invalid_json")

            path.write_text(json.dumps({**transcript, "tool_calls": ["bad-call"]}),
                            encoding="utf-8")
            bad_shape = autonomy_run.diagnostics.audit_serialized_transcript(
                path, calls, expected_case_id=case.case_id,
                expected_schema_version=transcript["schema_version"],
            )
            self.assertEqual(bad_shape["state"], "invalid_shape")

            path.write_bytes(b"x" * (autonomy_run.diagnostics.MAX_SERIALIZED_TRANSCRIPT_BYTES + 1))
            oversized = autonomy_run.diagnostics.audit_serialized_transcript(
                path, calls, expected_case_id=case.case_id,
                expected_schema_version=transcript["schema_version"],
            )
            self.assertEqual(oversized["state"], "oversized")


class H006VerifierTranscriptCompatibilityTests(unittest.TestCase):
    def test_private_root_canary_symlink_is_seeded_for_native_probe(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            fixture_root = root / "fixture"
            fixture_root.mkdir()
            private_root = root / "private"
            private_root.mkdir()
            env: dict[str, str] = {}
            canary = autonomy_run._seed_private_root_canary(private_root)
            link = autonomy_run._seed_private_root_canary_link(fixture_root, private_root)

            autonomy_run._set_harness_private_root_env(env, private_root)

            self.assertEqual(env[autonomy_run.HARNESS_PRIVATE_ROOT_ENV],
                             str(private_root.resolve()))
            self.assertEqual(set(env), {autonomy_run.HARNESS_PRIVATE_ROOT_ENV})
            self.assertEqual(canary.read_text(encoding="utf-8"),
                             "CodeGG private-root denial canary only.\n")
            self.assertTrue(link.is_symlink())
            self.assertEqual(link.resolve(strict=True), canary.resolve(strict=True))

    def test_task_spawn_event_sequence_requires_both_exact_ids(self) -> None:
        projector = autonomy_run._ToolCallProjector()

        def call(call_id: str, action: str | None, output: str,
                 start: int, finish: int) -> None:
            projector.absorb({"method": "session/update", "_received_elapsed_ms": start,
                              "params": {"update": {
                                  "sessionUpdate": "tool_call", "toolCallId": call_id,
                                  "title": "task", "codeggArgumentsSha256": "a" * 64,
                                  "codeggAction": action,
                              }}})
            projector.absorb({"method": "session/update", "_received_elapsed_ms": finish,
                              "params": {"update": {
                                  "sessionUpdate": "tool_call_update", "toolCallId": call_id,
                                  "status": "completed", "rawOutput": output,
                              }}})

        call("board-1", None, "Workstream board: no active children", 10, 20)
        call("spawn-queued", "spawn",
             "<task_result>\nSubagent queued\nTask ID: agt-111\n"
             "Status: pending (no spawner configured)\n</task_result>", 30, 40)
        call("spawn-durable", "spawn",
             "Task queued\nTask: agt-222\nRun: agr-333\nStatus: queued", 50, 60)
        call("spawn-uuid", "spawn",
             "Task queued\nTask: 550e8400-e29b-41d4-a716-446655440000\n"
             "Run: 6ba7b810-9dad-11d1-80b4-00c04fd430c8\nStatus: queued", 70, 80)
        call("spawn-duplicate", "spawn",
             "Task queued\nTask: same-id\nRun: same-id\nStatus: queued", 90, 100)

        projected = projector.calls()
        self.assertNotIn("task_action", projected[0])
        self.assertNotIn("task_spawn_ids_present", projected[0])
        self.assertEqual(projected[1]["task_action"], "spawn")
        self.assertFalse(projected[1]["task_spawn_ids_present"])
        self.assertEqual(projected[2]["task_action"], "spawn")
        self.assertTrue(projected[2]["task_spawn_ids_present"])
        self.assertEqual(projected[2]["task_run_status"], "queued")
        self.assertTrue(projected[3]["task_spawn_ids_present"])
        self.assertFalse(projected[4]["task_spawn_ids_present"])

    def test_task_review_event_sequence_binds_get_result_to_spawned_run(self) -> None:
        projector = autonomy_run._ToolCallProjector()
        task_id = "task-11"
        run_id = "run-12"
        task_digest = hashlib.sha256(task_id.encode()).hexdigest()
        run_digest = hashlib.sha256(run_id.encode()).hexdigest()

        def call(call_id: str, action: str, start: int, finish: int, output: str,
                 metadata_field: str, metadata: dict[str, str],
                 task_hash: str | None = None,
                 run_hash: str | None = None) -> None:
            started = {
                "sessionUpdate": "tool_call", "toolCallId": call_id, "title": "task",
                "codeggArgumentsSha256": "a" * 64, "codeggAction": action,
            }
            if task_hash is not None:
                started["codeggTaskIdSha256"] = task_hash
            if run_hash is not None:
                started["codeggTaskRunIdSha256"] = run_hash
            projector.absorb({"method": "session/update", "_received_elapsed_ms": start,
                              "params": {"update": started}})
            completed = {
                "sessionUpdate": "tool_call_update", "toolCallId": call_id,
                "status": "completed", "rawOutput": output,
                metadata_field: metadata,
            }
            projector.absorb({"method": "session/update", "_received_elapsed_ms": finish,
                              "params": {"update": completed}})

        call("spawn-call", "spawn", 10, 20,
             "Task queued\nTask: task-11\nRun: run-12\nStatus: queued",
             "codeggTaskSpawn", {"task_id_sha256": task_digest,
                                 "run_id_sha256": run_digest, "run_status": "queued",
                                 "agent": "verifier"})
        call("status-call", "status", 30, 40,
             "Run: run-12\nStatus: failed\nTask: task-11\n"
             "Failure stage/class: deadline_exceeded",
             "codeggTaskStatus", {}, run_hash=run_digest)
        call("continue-call", "continue", 50, 60,
             "Run: run-12\nStatus: running", "codeggTaskStatus", {},
             run_hash=run_digest)
        call("completed-status-call", "status", 70, 80,
             "Run: run-12\nStatus: completed\nTask: task-11\n"
             "Result: agent-run-result:run-12",
             "codeggTaskStatus", {}, run_hash=run_digest)
        call("get-call", "get", 30, 40,
             "Recorded result (completed)\nReview evidence\nResult ref: result\n"
             "Task: task-11\nRun: run-12\nStatus: completed",
             "codeggTaskResult", {"task_id_sha256": task_digest,
                                  "run_id_sha256": run_digest, "run_status": "completed"},
             task_hash=task_digest)

        calls = projector.calls()
        self.assertEqual(calls[0]["task_spawn_agent"], "verifier")
        self.assertEqual(calls[1]["task_run_id_sha256"], run_digest)
        self.assertEqual(calls[1]["task_run_status"], "failed")
        self.assertEqual(calls[2]["task_action"], "continue")
        self.assertEqual(calls[2]["task_run_id_sha256"], run_digest)
        self.assertEqual(calls[2]["task_run_status"], "running")
        self.assertEqual(calls[3]["task_run_status"], "completed")
        self.assertTrue(autonomy_run._h007_required_review_observed(
            "review_agent: verifier\nreview_task: task-11\nreview_run: run-12\n"
            "review_status: completed\n"
            "review_findings: inspected public test matrix; no reproducible issue found",
            calls))
        lifecycle = autonomy_run.diagnostics.summarize_reviewer_lifecycle(calls)
        self.assertEqual(lifecycle["reviewer_continue_count"], 1)
        self.assertEqual(lifecycle["continue_after_terminal_failure_count"], 1)
        self.assertTrue(lifecycle["continue_after_terminal_failure_observed"])
        self.assertNotIn(task_id, json.dumps(calls))
        self.assertNotIn(run_id, json.dumps(calls))
        self.assertFalse(autonomy_run._h007_required_review_observed(
            "review_agent: verifier\nreview_task: task-11\nreview_run: run-12\n"
            "review_status: completed\n"
            "review_findings: inspected public test matrix; no reproducible issue found",
            [calls[0], calls[2]]))

        duplicated_gets = calls + [dict(calls[4], index=5)]
        self.assertFalse(autonomy_run._h007_required_review_observed(
            "review_agent: verifier\nreview_task: task-11\nreview_run: run-12\n"
            "review_status: completed\n"
            "review_findings: inspected public test matrix; no reproducible issue found",
            duplicated_gets))

        calls[4]["task_result_run_id_sha256"] = hashlib.sha256(
            b"unrelated-run").hexdigest()
        self.assertFalse(autonomy_run._h007_required_review_observed(
            "review_agent: verifier\nreview_task: task-11\nreview_run: run-12\n"
            "review_status: completed\n"
            "review_findings: inspected public test matrix; no reproducible issue found",
            calls))

        calls[0]["task_spawn_agent"] = "research"
        calls[4]["task_result_run_id_sha256"] = run_digest
        self.assertFalse(autonomy_run._h007_required_review_observed(
            "review_agent: verifier\nreview_task: task-11\nreview_run: run-12\n"
            "review_status: completed\n"
            "review_findings: inspected public test matrix; no reproducible issue found",
            calls))
        calls[0]["task_spawn_agent"] = "verifier"
        calls[1]["task_run_id_sha256"] = hashlib.sha256(b"other-run").hexdigest()
        calls[3]["task_run_id_sha256"] = hashlib.sha256(b"other-run").hexdigest()
        self.assertFalse(autonomy_run._h007_required_review_observed(
            "review_agent: verifier\nreview_task: task-11\nreview_run: run-12\n"
            "review_status: completed\n"
            "review_findings: inspected public test matrix; no reproducible issue found",
            calls))

    def test_task_spawn_id_projection_rejects_missing_ambiguous_and_oversized_ids(self) -> None:
        self.assertFalse(autonomy_run._safe_task_spawn_ids_present(
            "Task queued\nTask: task-id\nStatus: queued"))
        self.assertFalse(autonomy_run._safe_task_spawn_ids_present(
            "Task queued\nTask: same-id\nRun: same-id\nStatus: queued"))
        self.assertFalse(autonomy_run._safe_task_spawn_ids_present(
            "Task queued\nTask: " + "t" * 129 + "\nRun: run-id\nStatus: queued"))

    def test_task_failure_projection_keeps_allowlisted_class_only(self) -> None:
        projector = autonomy_run._ToolCallProjector()
        projector.absorb({"method": "session/update", "_received_elapsed_ms": 10,
                          "params": {"update": {
                              "sessionUpdate": "tool_call", "toolCallId": "failed-review",
                              "title": "task", "codeggAction": "status",
                              "codeggTaskRunIdSha256": "a" * 64,
                          }}})
        projector.absorb({"method": "session/update", "_received_elapsed_ms": 20,
                          "params": {"update": {
                              "sessionUpdate": "tool_call_update", "toolCallId": "failed-review",
                              "status": "completed",
                              "rawOutput": "Status: failed\n"
                              "Failure stage/class: provider_transient\n"
                              "Failure detail: private provider response",
                          }}})

        projected = projector.calls()[0]

        self.assertEqual(projected["task_run_status"], "failed")
        self.assertEqual(projected["task_failure_class"], "provider_transient")
        self.assertNotIn("private provider response", json.dumps(projected))

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
        call("edit-3", "edit", None, 105, 108, "Inline { output: private edit result }")
        call("terminal-1", "job", "wait", 110, 120,
             "Inline { output: job completed\njob_id: "
             "74386df4-54d6-41eb-b2fc-0f52ade38983 }")

        tool_calls = projector.calls()
        self.assertNotIn("job", tool_calls[0])
        self.assertEqual(tool_calls[1]["job"]["action"], "submit")
        self.assertEqual(tool_calls[4]["job"]["status"], "running")
        self.assertEqual(tool_calls[6]["job"]["status"], "completed")
        safe_sequence = autonomy_run.diagnostics.summarize_job_tool_sequence(tool_calls)
        self.assertEqual(safe_sequence["job_tool_call_count"], 3)
        self.assertEqual([event["job_alias"] for event in safe_sequence["events"]],
                         ["J01", "J01", "J01"])
        self.assertEqual([event["job_status"] for event in safe_sequence["events"]],
                         ["queued", "running", "completed"])
        self.assertNotIn("74386df4-54d6-41eb-b2fc-0f52ade38983",
                         json.dumps(safe_sequence))
        safe_activity = autonomy_run.diagnostics.summarize_ordered_tool_activity(tool_calls)
        self.assertEqual(safe_activity["productive_overlap_interval_count"], 1)
        self.assertEqual(safe_activity["zero_productive_overlap_interval_count"], 0)
        self.assertEqual(safe_activity["productive_overlap_intervals"][0][
            "productive_call_count_between"], 1)
        self.assertNotIn("74386df4-54d6-41eb-b2fc-0f52ade38983",
                         json.dumps(safe_activity))
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


class H007RepairEventProjectionTests(unittest.TestCase):
    def test_realistic_acp_event_order_qualifies_one_named_repair_pair(self) -> None:
        projector = autonomy_run._ToolCallProjector()
        seed_id = "74386df4-54d6-41eb-b2fc-0f52ade38983"
        repair_id = "a6ab9df2-94ec-4a8b-9c6a-9f6f0390e334"

        def call(call_id: str, title: str, action: str | None,
                 start: int, finish: int, output: str) -> None:
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

        call("seed-submit", "test", "submit", 10, 20,
             f"Inline {{ output: job queued\njob_id: {seed_id} }}")
        call("seed-status", "job", "status", 30, 40,
             f"Inline {{ output: job failed\njob_id: {seed_id} }}")
        call("source-repair", "edit", None, 50, 60,
             "Inline { output: repaired catalog format implementation }")
        call("repair-submit", "test", "submit", 70, 80,
             f"Inline {{ output: job queued\njob_id: {repair_id} }}")
        call("repair-status", "job", "status", 90, 100,
             f"Inline {{ output: job completed\njob_id: {repair_id} }}")

        calls = projector.calls()
        note = (
            "repair_cycles: 1\n"
            f"job_id_catalog_format_seed: {seed_id}\n"
            "job_state_catalog_format_seed: failed\n"
            f"job_id_catalog_format_repair: {repair_id}\n"
            "job_state_catalog_format_repair: completed\n"
        )
        summary = h007_results.summarize_required_repair_jobs(
            note, calls, (("catalog_format", "catalog_format_seed",
                           "catalog_format_repair"),),
        )

        self.assertEqual(len(calls), 5)
        self.assertEqual(calls[1]["job"]["job_id"], seed_id)
        self.assertEqual(calls[1]["job"]["status"], "failed")
        self.assertEqual(calls[4]["job"]["job_id"], repair_id)
        self.assertEqual(calls[4]["job"]["status"], "completed")
        self.assertEqual(autonomy_run._h007_repair_cycles(calls), 1)
        self.assertTrue(summary["complete"])
        case = autonomy.load_autonomy_suite(
            REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v80"
        ).case("build-provenance-catalog")
        transcript = autonomy_run.build_transcript(
            case, calls,
            terminal="completed", stop_reason="end_turn",
        )
        self.assertEqual(transcript["tool_calls"], calls)


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

    def test_each_same_session_followup_preserves_the_verifier_reserve(self) -> None:
        first_turn = autonomy_run._turn_timeout(
            "build-provenance-catalog", 3600.0, 3600.0, 3600.0)
        self.assertEqual(first_turn, 3300.0)
        self.assertEqual(autonomy_run._turn_timeout(
            "build-provenance-catalog", first_turn, 3600.0, 1200.0), 900.0)
        self.assertEqual(autonomy_run._turn_timeout(
            "build-provenance-catalog", first_turn, 3600.0, 300.0), 0.0)

    def test_other_cases_keep_their_proportional_suite_timeout(self) -> None:
        self.assertEqual(autonomy_run._turn_timeout(
            "ordinary-case", 3600.0, 3600.0, 3600.0), 1200.0)

    def test_trailing_tool_silence_counts_toward_the_activity_gap(self) -> None:
        calls = [{"started_elapsed_ms": elapsed,
                  "finished_elapsed_ms": elapsed + 1}
                 for elapsed in range(0, 1_800_001, 300_000)]
        self.assertTrue(autonomy_run._duration_qualified(
            "build-provenance-catalog", 2_100_001, calls))
        self.assertFalse(autonomy_run._duration_qualified(
            "build-provenance-catalog", 2_100_002, calls))


class ActivityStallWatcherTests(unittest.TestCase):
    class _Client:
        def __init__(self, notifications: list[dict]) -> None:
            self.notifications = notifications
            self.calls: list[tuple[str, dict]] = []

        def notify(self, method: str, params: dict) -> None:
            self.calls.append((method, params))

    class _OneIterationStop:
        def __init__(self) -> None:
            self.calls = 0

        def wait(self, _timeout: float) -> bool:
            self.calls += 1
            return self.calls > 1

    @staticmethod
    def _notification(kind: str) -> dict:
        return {"method": "session/update",
                "params": {"update": {"sessionUpdate": kind}}}

    def test_streamed_message_does_not_hide_a_tool_activity_stall(self) -> None:
        client = self._Client([self._notification("agent_message_chunk")])
        watcher = autonomy_run._ActivityStallWatcher(
            client, "session-1", stall_seconds=1, poll_seconds=0)
        watcher._last_activity = autonomy_run.time.monotonic() - 2
        watcher.stop_event = self._OneIterationStop()

        watcher._run()

        self.assertTrue(watcher.triggered)
        self.assertEqual(client.calls, [("session/cancel", {"sessionId": "session-1"})])

    def test_tool_start_and_terminal_updates_reset_the_stall_clock(self) -> None:
        for kind in ("tool_call", "tool_call_update"):
            with self.subTest(kind=kind):
                client = self._Client([self._notification(kind)])
                watcher = autonomy_run._ActivityStallWatcher(
                    client, "session-1", stall_seconds=1, poll_seconds=0)
                watcher._last_activity = autonomy_run.time.monotonic() - 2
                watcher.stop_event = self._OneIterationStop()

                watcher._run()

                self.assertFalse(watcher.triggered)
                self.assertEqual(client.calls, [])


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


class RequestedManifestProvenanceTests(unittest.TestCase):
    def test_run_suite_passes_exact_selected_manifest_digest_to_receipt(self) -> None:
        manifest_root = REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s016_promptfix_v20"
        suite = autonomy.load_autonomy_suite(manifest_root)
        case = suite.case("build-provenance-catalog")
        policy = budget.BudgetPolicy(max_provider_requests=5)
        with tempfile.TemporaryDirectory() as temp:
            with mock.patch.object(
                    autonomy_run, "run_case",
                    return_value={"case_id": case.case_id, "terminal": "completed",
                                  "solved": False}) as run_case:
                autonomy_run.run_suite(
                    pathlib.Path("/unused/codegg"), suite,
                    "opencode_go/muse-spark-1.3-contributor",
                    budget_policy=policy, lane_root=pathlib.Path(temp),
                    case_ids=[case.case_id])

        self.assertEqual(run_case.call_args.kwargs["requested_manifest_sha256"],
                         suite.suite_sha256)


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


class H007TimeoutDiagnosticTests(unittest.TestCase):
    def test_turn_timeout_replaces_a_stale_end_turn_reason(self) -> None:
        self.assertEqual(
            autonomy_run._h007_timeout_stop_reason("end_turn", timed_out=True),
            "turn_timeout",
        )
        self.assertEqual(
            autonomy_run._h007_timeout_stop_reason("end_turn", timed_out=False),
            "end_turn",
        )

    def test_followup_cancellation_replaces_prior_end_turn_at_prompt_boundary(self) -> None:
        first_prompt = autonomy_run._h007_prompt_stop_class("end_turn")
        cancelled_followup = autonomy_run._h007_prompt_stop_class(
            "end_turn", failure_category="cancelled")
        provider_stall = autonomy_run._h007_prompt_stop_class(
            "end_turn", failure_category="provider_timeout")
        controller_timeout = autonomy_run._h007_prompt_stop_class(
            None, failure_category="turn_timeout")

        self.assertEqual(first_prompt, "end_turn")
        self.assertEqual(cancelled_followup, "cancelled")
        self.assertEqual(provider_stall, "provider_timeout")
        self.assertEqual(controller_timeout, "turn_timeout")
        self.assertNotEqual(cancelled_followup, first_prompt)

    def test_cancellation_source_is_allowlisted_and_unknown_values_are_unavailable(self) -> None:
        self.assertEqual(
            autonomy_run._h007_project_cancellation_source("activity_stall"),
            "activity_stall",
        )
        self.assertEqual(
            autonomy_run._h007_project_cancellation_source("disk_floor"),
            "disk_floor",
        )
        self.assertEqual(
            autonomy_run._h007_project_cancellation_source("external_or_transport"),
            "external_or_transport",
        )
        self.assertIsNone(autonomy_run._h007_project_cancellation_source(
            "sentinel private exception text"))

    def test_prompt_timeout_projection_keeps_only_bounded_timing(self) -> None:
        projected = autonomy_run._h007_prompt_timeout_diagnostic(
            timed_out=True,
            prompt_request_count=8,
            followup_count=7,
            last_request_timeout_seconds=3300.0,
            last_request_elapsed_ms=3_299_900,
            remaining_wallclock_seconds_at_prompt_start=3599.9,
            remaining_wallclock_seconds=299.8,
        )

        self.assertEqual(projected["schema"], "h007_prompt_timeout_diagnostic_v1")
        self.assertEqual(projected["state"], "observed")
        self.assertEqual(projected["prompt_request_count"], 8)
        self.assertEqual(projected["autonomous_followup_count"], 7)
        self.assertEqual(projected["last_request_timeout_seconds"], 3300.0)
        self.assertEqual(projected["remaining_wallclock_seconds_at_timeout"], 299.8)
        self.assertEqual(projected["verifier_reserve_seconds"], 300.0)
        self.assertFalse(projected["exception_text_retained"])
        self.assertFalse(projected["provider_text_retained"])
        self.assertNotIn("exception", projected)
        self.assertNotIn("provider_output", projected)

    def test_prompt_boundary_projection_redacts_unknown_stop_text_and_tracks_review_due(self) -> None:
        first = autonomy_run._h007_prompt_boundary_diagnostic(
            request_index=4,
            started_elapsed_ms=1_790_000,
            remaining_wallclock_start_ms=1_810_000,
            request_timeout_ms=1_500_000,
            request_elapsed_ms=82_000,
            active_work_span_start_ms=1_760_000,
            active_work_span_end_ms=1_805_000,
            tool_call_count_start=140,
            tool_call_count_end=147,
            stop_class="end_turn",
            continuation_decision="continue",
            reviewer_due_marker_received=False,
            reviewer_due_marker_sent_next=True,
            prompt_work_limit_ms=3_300_000,
            prompt_work_consumed_ms_start=1_790_000,
            prompt_work_remaining_ms_start=1_510_000,
            prompt_work_consumed_ms_end=1_872_000,
            prompt_work_remaining_ms_end=1_428_000,
            verifier_reserve_ms=300_000,
            prompt_progress_start={"schema": "h007_prompt_progress_v1",
                                   "state": "observed", "stage_complete_count": 1},
            prompt_progress_end={"schema": "h007_prompt_progress_v1",
                                 "state": "observed", "stage_complete_count": 2},
            no_progress_boundary_count_before=0,
            no_progress_boundary_count=0,
        )
        timed_out = autonomy_run._h007_prompt_boundary_diagnostic(
            request_index=5,
            started_elapsed_ms=3_300_000,
            remaining_wallclock_start_ms=547_827,
            request_timeout_ms=247_827,
            request_elapsed_ms=247_832,
            active_work_span_start_ms=3_294_000,
            active_work_span_end_ms=3_294_629,
            tool_call_count_start=304,
            tool_call_count_end=305,
            stop_class="provider returned secret-bearing exception text",
            continuation_decision="private response text",
            reviewer_due_marker_received=True,
            reviewer_due_marker_sent_next=None,
        )

        self.assertTrue(first["reviewer_due_marker_sent_next"])
        self.assertFalse(first["reviewer_due_marker_received"])
        self.assertEqual(first["prompt_work_limit_ms"], 3_300_000)
        self.assertEqual(first["prompt_work_remaining_ms_end"], 1_428_000)
        self.assertEqual(first["verifier_reserve_ms"], 300_000)
        self.assertEqual(first["prompt_progress_end"]["stage_complete_count"], 2)
        self.assertEqual(timed_out["stop_class"], "other")
        self.assertIsNone(timed_out["continuation_decision"])
        self.assertTrue(timed_out["reviewer_due_marker_received"])
        self.assertIsNone(timed_out["reviewer_due_marker_sent_next"])
        for projected in (first, timed_out):
            self.assertFalse(projected["prompt_text_retained"])
            self.assertFalse(projected["provider_text_retained"])
            self.assertFalse(projected["exception_text_retained"])
            self.assertFalse(projected["raw_tool_arguments_retained"])
            self.assertNotIn("exception", projected)
            self.assertNotIn("provider_output", projected)

    def test_prompt_boundary_preserves_provider_timeout_as_its_own_class(self) -> None:
        projected = autonomy_run._h007_prompt_boundary_diagnostic(
            request_index=2,
            started_elapsed_ms=300_000,
            remaining_wallclock_start_ms=3_300_000,
            request_timeout_ms=1_500_000,
            request_elapsed_ms=240_004,
            active_work_span_start_ms=280_000,
            active_work_span_end_ms=280_000,
            tool_call_count_start=12,
            tool_call_count_end=12,
            stop_class="provider_timeout",
            continuation_decision="turn_failure",
            reviewer_due_marker_received=False,
            reviewer_due_marker_sent_next=None,
        )

        self.assertEqual(projected["stop_class"], "provider_timeout")
        self.assertEqual(projected["continuation_decision"], "turn_failure")
        self.assertFalse(projected["provider_text_retained"])

    def test_prompt_work_accounting_preserves_the_verifier_reserve(self) -> None:
        initial = autonomy_run._h007_prompt_work_accounting(
            run_limit_seconds=3600.0, remaining_wallclock_seconds=3600.0)
        near_timeout = autonomy_run._h007_prompt_work_accounting(
            run_limit_seconds=3600.0, remaining_wallclock_seconds=300.160)
        exhausted = autonomy_run._h007_prompt_work_accounting(
            run_limit_seconds=3600.0, remaining_wallclock_seconds=299.9)

        self.assertEqual(initial, {
            "prompt_work_limit_ms": 3_300_000,
            "prompt_work_consumed_ms": 0,
            "prompt_work_remaining_ms": 3_300_000,
            "verifier_reserve_ms": 300_000,
        })
        self.assertEqual(near_timeout["prompt_work_consumed_ms"], 3_299_840)
        self.assertEqual(near_timeout["prompt_work_remaining_ms"], 160)
        self.assertEqual(exhausted["prompt_work_consumed_ms"], 3_300_000)
        self.assertEqual(exhausted["prompt_work_remaining_ms"], 0)

    def test_prompt_progress_snapshot_is_bounded_and_malformed_notes_are_unavailable(self) -> None:
        suite_root = REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v93"
        case = autonomy.load_autonomy_suite(suite_root).case("build-provenance-catalog")
        stage_lines = "\n".join(
            f"- {stage}: pending" for stage in autonomy_run.H007_S018_REQUIRED_STAGE_ORDER)
        gate_lines = "\n".join(
            f"- {gate}: pending" for gate in autonomy_run.H007_S018_REQUIRED_GATE_ORDER)
        note = (
            "completed_phases: []\nnext_phase: versioned_records\n"
            "contract_phases: []\nnext_contract_phase: versioned_records\n"
            f"required_stages:\n{stage_lines}\nnext_stage: versioned_records\n"
            f"gates:\n{gate_lines}\nnext_gate: starter_repair_format\n"
            "private_note: PRIVATE_CHECKPOINT_SENTINEL\n"
        )
        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            (fixture / "CONTINUATION.md").write_text(note, encoding="utf-8")
            observed = autonomy_run._h007_prompt_progress_snapshot(
                fixture,
                [{"tool": "job", "status": "completed", "arguments": "PRIVATE_ARGS",
                  "rawOutput": "PRIVATE_OUTPUT", "job": {
                      "job_id": "11111111-1111-4111-8111-111111111111",
                      "status": "running"}}],
                case_prompt=case.prompt,
            )
            (fixture / "CONTINUATION.md").write_text(
                "required_stages:\nnot-a-stage-row PRIVATE_MALFORMED_SENTINEL\n"
                "gates:\n", encoding="utf-8")
            malformed = autonomy_run._h007_prompt_progress_snapshot(
                fixture, [], case_prompt=case.prompt)
            (fixture / "CONTINUATION.md").unlink()
            missing = autonomy_run._h007_prompt_progress_snapshot(
                fixture, [], case_prompt=case.prompt)

        self.assertEqual(observed["state"], "observed")
        self.assertEqual(observed["stage_total_count"], 7)
        self.assertEqual(observed["gate_total_count"], 12)
        self.assertEqual(observed["first_missing_stage"], "versioned_records")
        self.assertEqual(observed["first_missing_gate"], "starter_repair_format")
        self.assertFalse(observed["raw_note_retained"])
        self.assertFalse(observed["raw_job_ids_retained"])
        self.assertNotIn("PRIVATE_CHECKPOINT_SENTINEL", json.dumps(observed))
        self.assertNotIn("PRIVATE_ARGS", json.dumps(observed))
        self.assertNotIn("PRIVATE_OUTPUT", json.dumps(observed))
        self.assertNotIn("11111111-1111-4111-8111-111111111111", json.dumps(observed))
        self.assertEqual(malformed["state"], "unavailable")
        self.assertEqual(malformed["reason"], "checkpoint_malformed")
        self.assertNotIn("PRIVATE_MALFORMED_SENTINEL", json.dumps(malformed))
        self.assertEqual(missing["state"], "unavailable")
        self.assertEqual(missing["reason"], "checkpoint_missing_or_unreadable")

    def test_prompt_progress_advancement_resets_the_no_progress_ladder(self) -> None:
        before = {"state": "observed", "stage_complete_count": 2,
                  "gate_complete_count": 3, "repair_pair_complete_count": 0,
                  "success_job_complete_count": 0, "public_artifact_complete_count": 0,
                  "release_overlap": "pending", "reviewer": "pending"}
        unchanged = dict(before)
        after = {**before, "stage_complete_count": 3,
                 "release_overlap": "confirmed"}

        self.assertFalse(autonomy_run._h007_progress_advanced(before, unchanged))
        self.assertTrue(autonomy_run._h007_progress_advanced(before, after))
        self.assertFalse(autonomy_run._h007_progress_advanced(
            {"state": "unavailable"}, after))

    def test_prompt_priorities_make_reviewer_spawn_first_and_require_overlap_work(self) -> None:
        suite_root = REPO_ROOT / "dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v93"
        case = autonomy.load_autonomy_suite(suite_root).case("build-provenance-catalog")
        stage_lines = "\n".join(
            f"- {stage}: pending" for stage in autonomy_run.H007_S018_REQUIRED_STAGE_ORDER)
        gate_lines = "\n".join(
            f"- {gate}: pending" for gate in autonomy_run.H007_S018_REQUIRED_GATE_ORDER)
        job_id = "11111111-1111-4111-8111-111111111111"
        note = (
            "completed_phases: []\nnext_phase: versioned_records\n"
            "contract_phases: []\nnext_contract_phase: versioned_records\n"
            f"required_stages:\n{stage_lines}\nnext_stage: versioned_records\n"
            f"job_command_overlap: {autonomy_run.H007_S018_RELEASE_OVERLAP_COMMAND}\n"
            f"job_id_overlap: {job_id}\njob_id_overlap_running: running\n"
            f"gates:\n{gate_lines}\nnext_gate: starter_repair_format\n"
        )
        calls = [
            {"tool": "test", "status": "completed", "job": {
                "job_id": job_id, "action": "submit", "status": "queued"}},
            {"tool": "job", "status": "completed", "job": {
                "job_id": job_id, "action": "status", "status": "running"}},
        ]
        with tempfile.TemporaryDirectory() as temporary:
            fixture = pathlib.Path(temporary)
            (fixture / "CONTINUATION.md").write_text(note, encoding="utf-8")
            prompt = autonomy_run._h007_followup_prompt(
                fixture, calls, 1600, 1200, case.prompt,
                active_work_span_ms=autonomy_run.H007_S018_FINAL_REVIEW_START_ACTIVE_MS,
                no_progress_boundary_count=2,
            )

        self.assertTrue(prompt.startswith("FIRST NATIVE ACTION"))
        self.assertIn("spawn exactly one native read-only task", prompt)
        self.assertIn("NO-PROGRESS ESCALATION", prompt)
        self.assertIn("gate `starter_repair_format`", prompt)
        self.assertIn("REQUIRED OVERLAP ACTION", prompt)
        self.assertIn("Before polling that ID again", prompt)


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
