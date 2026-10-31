from __future__ import annotations

import json
import os
import pathlib
import shlex
import shutil
import subprocess
import tempfile
import unittest

from dev.harness_lab.supervisor import run_tool_trial

REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
STARTER = REPO_ROOT / "dev/harness_lab/fixtures/build_provenance_catalog/fixture"
REFERENCE = REPO_ROOT / "dev/harness_lab/fixtures/build_provenance_catalog/reference"


class H007NativeJobSequenceTest(unittest.TestCase):
    def test_native_tool_sequence_repairs_two_stages_and_overlaps_downstream_work(self):
        configured_binary = os.environ.get("CODEGG_H007_CODEGG_BINARY")
        binary = (pathlib.Path(configured_binary) if configured_binary else
                  REPO_ROOT / "target/debug/codegg")
        if not binary.is_file():
            self.skipTest("build CodeGG and set CODEGG_H007_CODEGG_BINARY")
        rustup = shutil.which("rustup")
        self.assertTrue(rustup, "native Rust fixture requires the configured toolchain")
        cargo = subprocess.run([rustup, "which", "cargo"], capture_output=True,
                               text=True, check=True).stdout.strip()
        rustc = subprocess.run([rustup, "which", "rustc"], capture_output=True,
                               text=True, check=True).stdout.strip()
        self.assertTrue(pathlib.Path(cargo).is_file())
        self.assertTrue(pathlib.Path(rustc).is_file())

        def test(crate: str, *, background: bool = True):
            return {"name": "test", "arguments": {
                "scope": "custom",
                "command": f"{shlex.quote(cargo)} test --offline --locked -p {crate}",
                "workdir": ".",
                **({"execution_mode": "background"} if background else {}),
            }}

        def wait():
            return {"name": "job", "arguments": {
                "action": "wait", "job_id": "{job_id}", "timeout_ms": 300000}}

        def observe_wait():
            return {"name": "job", "arguments": {
                "action": "wait", "job_id": "{job_id}", "timeout_ms": 1000}}

        def write(path: str, source: str):
            return {"name": "write", "arguments": {"path": path, "content": source}}

        format_source = (REFERENCE / "catalog-format/src/lib.rs").read_text()
        store_source = (REFERENCE / "catalog-store/src/lib.rs").read_text()
        index_source = (REFERENCE / "catalog-index/src/lib.rs").read_text()
        query_source = (REFERENCE / "catalog-query/src/lib.rs").read_text()
        cli_source = (REFERENCE / "catalog-cli/src/main.rs").read_text()
        sequence = [
            {"name": "read", "arguments": {"path": "README.md"}},
            test("catalog-format"), wait(),
            write("catalog-format/src/lib.rs", format_source),
            test("catalog-format"), wait(),
            test("catalog-store"), wait(),
            write("catalog-store/src/lib.rs", store_source),
            test("catalog-store"), wait(),
            write("catalog-index/src/lib.rs", index_source),
            {"name": "test", "arguments": {
                "scope": "custom",
                "command": f"{shlex.quote(cargo)} test --offline --locked -p catalog-index --test corpus",
                "workdir": ".", "execution_mode": "background"}},
            write("catalog-query/src/lib.rs", query_source),
            {"name": "job", "arguments": {"action": "status", "job_id": "{job_id}"}},
            write("catalog-cli/src/main.rs", cli_source),
            {"name": "job", "arguments": {"action": "status", "job_id": "{job_id}"}},
            write("CONTINUATION.md",
                  "completed_phases:\n- versioned_records\n- durable_import\n"
                  "- dependency_index\nnext_phase: provenance_queries\njob_id: {job_id}\n"),
            observe_wait(),
            {"name": "job", "arguments": {"action": "status", "job_id": "{job_id}"}},
            write("catalog-query/PHASE-NOTES.md",
                  "provenance query implementation is underway while the index job runs\n"),
            wait(),
            write("CONTINUATION.md",
                  "completed_phases:\n- versioned_records\n- durable_import\n"
                  "- dependency_index\n- provenance_queries\n- cli_export\n"
                  "next_phase: none\njob_id: {job_id}\n"),
            test("catalog-cli", background=False),
        ]
        with tempfile.TemporaryDirectory(prefix="h007-native-snapshot-") as directory:
            clean_snapshot = pathlib.Path(directory) / "fixture"
            shutil.copytree(STARTER, clean_snapshot, ignore=shutil.ignore_patterns("target"))
            trial = run_tool_trial(
                binary, "workspace-write", timeout=360.0,
                task_id="h007-dependent-rust-and-exact-job", tool_sequence=sequence,
                agent_steps=24, expected_file="CONTINUATION.md", expected_text="next_phase: none",
                expected_exists=True, expected_tool_failures=2,
                workspace_snapshot=clean_snapshot,
                rustc_executable=pathlib.Path(rustc),
                source_root=(pathlib.Path(os.environ["CODEGG_H007_SOURCE_ROOT"])
                             if os.environ.get("CODEGG_H007_SOURCE_ROOT") else None),
                causal_trace_output=(pathlib.Path(os.environ["CODEGG_S012_TRACE_OUTPUT"])
                                     if os.environ.get("CODEGG_S012_TRACE_OUTPUT") else None))

        intake_observation = os.environ.get("CODEGG_H007_INTAKE_OBSERVATION")
        if intake_observation:
            observation_path = pathlib.Path(intake_observation)
            observation = {
                "schema": "h007_native_intake_observation_v1",
                "task_id": trial["task_id"],
                "trial_returned": True,
                "binary_sha256": trial["source_sha256"],
                "tool_call_count": len(trial["tool_calls"]),
                "event_gap_count": trial["event_gap_count"],
                "native_terminal": trial.get("ledger_manifest", {}).get(
                    "terminal", trial.get("stop_reason")),
            }
            with observation_path.open("x", encoding="utf-8") as destination:
                destination.write(json.dumps(observation, sort_keys=True) + "\n")

        self.assertTrue(trial["tool_receipt_created"])
        self.assertEqual(trial["event_gap_count"], 0)
        self.assertEqual(trial["causal_trace"]["schema"], "harness_causal_trace_v1")
        self.assertEqual(trial["causal_trace"]["state"], "complete")
        self.assertTrue(trial["causal_trace"]["daemon_generation"])
        self.assertGreater(trial["causal_trace"]["event_count"], 0)
        trace_output = os.environ.get("CODEGG_S012_TRACE_OUTPUT")
        if trace_output:
            from dev.harness_lab import causal_trace
            rows = causal_trace.read_trace(pathlib.Path(trace_output))
            self.assertTrue(rows)
            self.assertTrue(all(row["daemon_generation"] == trial["causal_trace"]["daemon_generation"]
                                for row in rows))
        calls = trial["tool_calls"]
        jobs = [(index, call.get("tool"), call.get("job", {}))
                for index, call in enumerate(calls) if isinstance(call.get("job"), dict)]
        submitted = [job for _index, tool, job in jobs
                     if tool == "test" and job.get("action") == "submit"]
        target_id = next((job.get("job_id") for job in reversed(submitted)
                          if job.get("job_id")), None)
        self.assertIsNotNone(target_id)
        observations = [(index, job.get("status")) for index, _tool, job in jobs
                        if job.get("job_id") == target_id]
        running = [index for index, status in observations if status == "running"]
        terminal = [index for index, status in observations if status == "completed"]
        self.assertTrue(running and terminal and min(terminal) > min(running),
                        f"exact job {target_id!r} observations: {observations!r}")
        self.assertTrue(any(call["tool"] in {"write", "test"}
                            for call in calls[min(running) + 1:min(terminal)]))
        self.assertTrue(any(call["tool"] == "write" for call in calls[max(terminal) + 1:]))
        failed_indices = [index for index, _tool, job in jobs if job.get("status") == "failed"]
        successful_indices = [index for index, _tool, job in jobs
                              if job.get("status") == "completed"]
        self.assertGreaterEqual(sum(any(success > failed for success in successful_indices)
                                    for failed in failed_indices), 2)
        self.assertEqual(trial["source_sha256"].__len__(), 64)
        receipt_output = os.environ.get("CODEGG_S012_RECEIPT_OUTPUT")
        if trace_output and receipt_output:
            source_root = pathlib.Path(os.environ["CODEGG_H007_SOURCE_ROOT"])
            source_revision = subprocess.run(
                ["git", "-C", str(source_root), "rev-parse", "HEAD"],
                capture_output=True, text=True, check=True).stdout.strip()
            receipt = {
                "task_id": trial["task_id"],
                "source_revision": source_revision,
                "binary_sha256": trial["source_sha256"],
                "causal_trace": {
                    "schema": trial["causal_trace"]["schema"],
                    "state": trial["causal_trace"]["state"],
                    "daemon_generation": trial["causal_trace"]["daemon_generation"],
                    "sha256": trial["causal_trace"]["sha256"],
                },
                "task_outcome": "passed",
                "independent_verifier": {"kind": "h007_native_test", "tests_run": 1,
                                         "exit_code": 0},
            }
            receipt_path = pathlib.Path(receipt_output)
            receipt_path.parent.mkdir(parents=True, exist_ok=True)
            with receipt_path.open("x", encoding="utf-8") as destination:
                destination.write(json.dumps(receipt, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    unittest.main()
