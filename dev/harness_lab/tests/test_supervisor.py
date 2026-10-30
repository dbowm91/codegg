from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

from dev.harness_lab.supervisor import (
    AcpClient,
    SubjectSpec,
    _assert_daemon_identity,
    _default_daemon_fingerprint,
    _live_daemon_id,
    _owned_daemon_pids,
    HarnessError,
    stop_lane_daemon,
    _bounded_managed_test_log_tail,
    _project_job_observation,
    run_exec_trial,
)


#: A fake headless `codegg exec`.
#:
#: It records the argv it was given and replays one fixed outcome, so the
#: headless trial is exercised without a model, a daemon, or a provider. The
#: artifact is written relative to the child's cwd, which is the trial's
#: private workspace.
_FAKE_EXEC_TEMPLATE = '''#!/usr/bin/env python3
import json
import sys
import time

MODE = "__MODE__"
LOG = "__LOG__"
ARTIFACT = "__ARTIFACT__"
CONTENT = "__CONTENT__"

with open(LOG, "a", encoding="utf-8") as handle:
    handle.write(json.dumps(sys.argv[1:]) + "\\n")

if MODE == "hang":
    while True:
        time.sleep(3600)

tokens = None if MODE == "no-usage" else 26
if MODE in ("success", "no-usage"):
    with open(ARTIFACT, "w", encoding="utf-8") as handle:
        handle.write(CONTENT)
if MODE == "refusal":
    sys.stderr.write("Tool 'write' non-retryable error: permission denied: "
                     "verified tool authority is missing workspace path policy\\n")

print(json.dumps({"success": True, "result": "fixture turn complete",
                  "toolsUsed": ["write"], "tokensUsed": tokens,
                  "durationMs": 12, "error": None, "code": None}))
'''

#: Terminals the ledger records for a trial that did not pass.
FAILURE_TERMINALS = {"assertion_failed", "execution_failed", "cancelled", "denied",
                     "timeout", "budget_exhausted", "infrastructure_error",
                     "indeterminate", "interrupted_unknown"}


class SubjectEnvironmentTests(unittest.TestCase):
    def test_lane_env_replaces_runtime_and_user_roots(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / "codegg"
            binary.write_bytes(b"candidate")
            binary.chmod(0o755)
            spec = SubjectSpec.create("a", binary, root, root / "lane", root)
            original = dict(os.environ)
            os.environ.update({
                "CODEGG_DAEMON_HOME": "/shared/daemon",
                "CODEGG_DATA_HOME": "/shared/data",
                "CODEGG_TUI_CONFIG": "/shared/config.jsonc",
                "CODEGG_CORE_ENDPOINT": "unix:///shared.sock",
                "OPENAI_API_KEY": "must-not-inherit",
                "XDG_RUNTIME_DIR": "/shared/runtime",
                "RUSTUP_HOME": "/shared/rustup",
            })
            try:
                env = spec.environment()
            finally:
                os.environ.clear()
                os.environ.update(original)
            self.assertEqual(env["CODEGG_DAEMON_HOME"], str(root / "lane" / "daemon"))
            self.assertEqual(env["CODEGG_DATA_HOME"], str(root / "lane" / "data"))
            self.assertEqual(env["HOME"], str(root / "lane" / "home"))
            self.assertEqual(env["XDG_RUNTIME_DIR"], str(root / "lane" / "runtime"))
            self.assertEqual(env["RUSTUP_HOME"], "/shared/rustup")
            self.assertNotEqual(env["RUSTUP_HOME"], env["HOME"] + "/.rustup")
            self.assertNotIn("CODEGG_CORE_ENDPOINT", env)
            self.assertNotIn("OPENAI_API_KEY", env)
            self.assertTrue((root / "lane" / "config").is_dir())


class SubjectEnvironmentAndCrashCleanupTests(unittest.TestCase):
    @unittest.skipUnless(os.name == "posix", "private daemon crash recovery uses POSIX process ownership")
    def test_controller_sigkill_reconciles_only_its_private_subject_daemon(self) -> None:
        configured = os.environ.get("CODEGG_H006_CODEGG_BINARY")
        binary = Path(configured) if configured else Path(__file__).resolve().parents[2] / "target" / "debug" / "codegg"
        if not binary.is_file():
            self.skipTest("set CODEGG_H006_CODEGG_BINARY to an available CodeGG binary")
        repo_root = Path(__file__).resolve().parents[3]
        default_before = _default_daemon_fingerprint(binary, 3.0)
        with tempfile.TemporaryDirectory(prefix="hcc-", dir="/tmp") as temporary:
            root = Path(temporary)
            lane_root = root / "lane"
            workspace = root / "workspace"
            lane_root.mkdir()
            workspace.mkdir()
            ready = root / "controller-ready.json"
            controller = root / "controller.py"
            controller.write_text(r"""
import json, sys, time
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from dev.harness_lab.supervisor import AcpClient, SubjectSpec, _assert_daemon_identity
binary, workspace, lane_root, ready = map(Path, sys.argv[2:])
spec = SubjectSpec.create("crash-subject", binary, workspace, lane_root, Path(sys.argv[1]))
client = AcpClient(binary, spec.environment(), workspace, 30.0)
client.request("initialize", {"protocolVersion": 1, "clientInfo": {"name": "crash-fixture", "version": "1"}})
session = client.request("session/new", {"cwd": str(workspace), "title": "parent-death-fixture"})
identity = _assert_daemon_identity(spec, 10.0, client.process.pid)
ready.write_text(json.dumps({"identity": identity, "session_id": session["sessionId"]}), encoding="utf-8")
while True:
    time.sleep(0.05)
""", encoding="utf-8")
            controller_process = subprocess.Popen(
                [sys.executable, str(controller), str(repo_root), str(binary), str(workspace), str(lane_root), str(ready)],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            sentinel = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
            try:
                deadline = time.monotonic() + 30
                while time.monotonic() < deadline and not ready.exists():
                    if controller_process.poll() is not None:
                        stdout, stderr = controller_process.communicate(timeout=1)
                        daemon_log = lane_root / "daemon" / "daemon.log"
                        log_tail = daemon_log.read_bytes()[-2048:] if daemon_log.exists() else b""
                        self.fail(f"private subject controller failed: {stderr[-2048:]!r}; "
                                  f"daemon log: {log_tail!r}")
                    time.sleep(0.05)
                self.assertTrue(ready.is_file(), "controller never published exact private daemon identity")
                identity = json.loads(ready.read_text(encoding="utf-8"))["identity"]
                self.assertEqual(identity["lane_id"], "crash-subject")
                self.assertEqual(identity["binary_sha256"], hashlib.sha256(binary.read_bytes()).hexdigest())
                controller_process.kill()
                controller_process.wait(timeout=3)
                controller_process.communicate(timeout=1)
                spec = SubjectSpec.create("crash-subject", binary, workspace, lane_root, repo_root)
                remaining_owned = _owned_daemon_pids(spec)
                try:
                    live_daemon_id = _live_daemon_id(spec, 3.0)
                except HarnessError:
                    live_daemon_id = None
                if identity["pid"] in remaining_owned or live_daemon_id is not None:
                    stop_lane_daemon(spec, timeout=10)
                self.assertEqual(_owned_daemon_pids(spec), [])
                with self.assertRaises(HarnessError):
                    _live_daemon_id(spec, 3.0)
                self.assertIsNone(sentinel.poll(), "unrelated sentinel process was stopped")
                self.assertEqual(_default_daemon_fingerprint(binary, 3.0), default_before)
            finally:
                if controller_process.poll() is None:
                    controller_process.kill()
                    controller_process.wait(timeout=3)
                if sentinel.poll() is None:
                    sentinel.terminate()
                    sentinel.wait(timeout=3)

    def test_provider_request_limit_is_forwarded_with_attempt_diagnostics(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / "codegg"
            binary.write_bytes(b"candidate")
            binary.chmod(0o755)
            spec = SubjectSpec.create("a", binary, root, root / "lane", root)
            original = dict(os.environ)
            os.environ["CODEGG_HARNESS_MAX_PROVIDER_REQUESTS"] = "60"
            try:
                env = spec.environment()
            finally:
                os.environ.clear()
                os.environ.update(original)

        self.assertEqual(env["CODEGG_HARNESS_MAX_PROVIDER_REQUESTS"], "60")
        self.assertTrue(env["CODEGG_HARNESS_PROVIDER_REQUEST_RECEIPT"].endswith(
            "artifacts/provider-request-admissions.jsonl"))
        self.assertEqual(env["RUST_LOG"], "error,codegg_providers::request_budget=info")

    def test_provider_request_limit_can_be_bound_to_a_trial_without_parent_env(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / "codegg"
            binary.write_bytes(b"candidate")
            binary.chmod(0o755)
            spec = SubjectSpec.create(
                "a", binary, root, root / "lane", root, provider_request_limit=297
            )
            original = dict(os.environ)
            os.environ.pop("CODEGG_HARNESS_MAX_PROVIDER_REQUESTS", None)
            try:
                env = spec.environment()
            finally:
                os.environ.clear()
                os.environ.update(original)

        self.assertEqual(env["CODEGG_HARNESS_MAX_PROVIDER_REQUESTS"], "297")
        self.assertTrue(env["CODEGG_HARNESS_PROVIDER_REQUEST_RECEIPT"].endswith(
            "artifacts/provider-request-admissions.jsonl"))
        self.assertEqual(env["RUST_LOG"], "error,codegg_providers::request_budget=info")
        child = subprocess.run(
            [sys.executable, "-c", "import os; print(os.environ['CODEGG_HARNESS_MAX_PROVIDER_REQUESTS']); print(os.environ['CODEGG_HARNESS_PROVIDER_REQUEST_RECEIPT'])"],
            env=env, capture_output=True, text=True, check=True)
        self.assertEqual(child.stdout.splitlines()[0], "297")
        self.assertEqual(child.stdout.splitlines()[1], env["CODEGG_HARNESS_PROVIDER_REQUEST_RECEIPT"])

    def test_the_lane_roots_are_private_whatever_the_umask(self) -> None:
        """The lane's ``XDG_CACHE_HOME`` must not inherit the operator's umask.

        The daemon's scratch allocator refuses a cache root that is group- or
        world-writable, so a lane built with a group-granting umask (``0002``)
        would be refused for a reason that has nothing to do with the code
        under test. The lane's directories are created private instead.
        """
        if not hasattr(os, "getuid"):
            self.skipTest("POSIX-only ownership check")
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / "codegg"
            binary.write_bytes(b"candidate")
            spec = SubjectSpec.create("a", binary, root, root / "lane", root)
            previous = os.umask(0o002)
            try:
                spec.environment()
            finally:
                os.umask(previous)
            for key in ("config", "codegg", "daemon", "data", "home", "xdg-data",
                        "cache", "state", "runtime", "scratch", "artifacts",
                        "cargo-home"):
                mode = (root / "lane" / key).stat().st_mode & 0o777
                self.assertEqual(mode, 0o700, f"{key} is {oct(mode)}, not 0700")

    def test_subject_hash_is_pinned_at_creation(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / "codegg"
            binary.write_bytes(b"one")
            spec = SubjectSpec.create("a", binary, root, root / "lane", root)
            binary.write_bytes(b"two")
            self.assertNotEqual(spec.binary_sha256, hashlib.sha256(binary.read_bytes()).hexdigest())


class ExecTrialTests(unittest.TestCase):
    """The headless adapter is measured the same way the ACP adapter is."""

    def _fake_binary(self, root: Path, mode: str, *, artifact: str = "tool-receipt.txt",
                     content: str = "harness-lab-tool-ok") -> tuple[Path, Path]:
        """Write a fake headless `codegg` and return it with its argv log."""
        log = root / "exec.argv.jsonl"
        binary = root / "codegg"
        body = (_FAKE_EXEC_TEMPLATE.replace("__MODE__", mode)
                .replace("__LOG__", str(log))
                .replace("__ARTIFACT__", artifact)
                .replace("__CONTENT__", content))
        binary.write_text(body, encoding="utf-8")
        binary.chmod(0o755)
        log.write_text("", encoding="utf-8")
        return binary, log

    def _lane_root(self, receipt: dict) -> Path:
        """The lane root of a finished trial: the workspace's parent."""
        return Path(receipt["workspace"]).parent

    def test_a_headless_trial_that_writes_the_artifact_passes(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, _ = self._fake_binary(root, "success")
            receipt = run_exec_trial(binary, "workspace-write", 30.0,
                                     ledger_root=root / "ledger")
            self.assertIs(receipt["ok"], True)
            self.assertTrue(receipt["tool_receipt_created"])
            self.assertEqual(receipt["tool_failures"], 0)
            self.assertEqual(receipt["ledger_manifest"]["terminal"], "passed")

    def test_a_refused_headless_tool_is_never_a_pass(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, _ = self._fake_binary(root, "refusal")
            receipt = run_exec_trial(binary, "workspace-write", 30.0,
                                     ledger_root=root / "ledger")
            # The turn completed, so the child reported success. The tool did
            # not: a completed turn with a refused tool is not a pass.
            self.assertIs(receipt["ok"], False)
            self.assertGreaterEqual(receipt["tool_failures"], 1)
            terminal = receipt["ledger_manifest"]["terminal"]
            self.assertNotEqual(terminal, "passed")
            self.assertIn(terminal, FAILURE_TERMINALS)

    def test_a_tool_call_without_an_artifact_is_a_tool_failure(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, _ = self._fake_binary(root, "silent")
            receipt = run_exec_trial(binary, "workspace-write", 30.0,
                                     ledger_root=root / "ledger")
            # The child named no failure: the missing artifact is the only
            # evidence, and it is enough.
            self.assertIs(receipt["ok"], False)
            self.assertGreaterEqual(receipt["tool_failures"], 1)
            self.assertNotEqual(receipt["ledger_manifest"]["terminal"], "passed")

    def test_a_missing_usage_is_recorded_as_none(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, _ = self._fake_binary(root, "no-usage")
            receipt = run_exec_trial(binary, "workspace-write", 30.0,
                                     ledger_root=root / "ledger")
            self.assertIsNone(receipt["codegg_usage"])
            metrics = receipt["ledger_manifest"]["metrics"]
            self.assertIsNone(metrics["input_tokens"])
            self.assertIsNone(metrics["output_tokens"])

    def test_the_lane_is_removed_after_a_successful_trial(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, _ = self._fake_binary(root, "success")
            receipt = run_exec_trial(binary, "workspace-write", 30.0)
            self.assertIs(receipt["ok"], True)
            self.assertFalse(self._lane_root(receipt).exists())

    def test_the_lane_is_removed_after_a_failed_trial(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, _ = self._fake_binary(root, "refusal")
            receipt = run_exec_trial(binary, "workspace-write", 30.0)
            self.assertIs(receipt["ok"], False)
            self.assertFalse(self._lane_root(receipt).exists())

    def test_a_child_that_never_exits_is_bounded_by_the_timeout(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, _ = self._fake_binary(root, "hang")
            started = time.monotonic()
            receipt = run_exec_trial(binary, "workspace-write", 3.0,
                                     ledger_root=root / "ledger")
            elapsed = time.monotonic() - started
            self.assertLess(elapsed, 60.0)
            self.assertIs(receipt["ok"], False)
            self.assertEqual(receipt["stop_reason"], "timeout")
            self.assertEqual(receipt["ledger_manifest"]["terminal"], "timeout")
            self.assertFalse(self._lane_root(receipt).exists())

    def test_the_headless_child_is_invoked_with_the_documented_argv(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, log = self._fake_binary(root, "success")
            run_exec_trial(binary, "workspace-write", 30.0)
            recorded = [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines() if line]
            self.assertEqual(len(recorded), 1)
            argv = recorded[0]
            self.assertEqual(argv[0], "exec")
            self.assertEqual(argv[1], "--json")
            request = json.loads(argv[2])
            self.assertEqual(request["model"], "openai/harness-fixture")
            self.assertEqual(request["agent"], "build")
            self.assertTrue(request["prompt"])
            self.assertEqual(argv[3:], ["--format", "json", "--approval-mode", "yolo",
                                        "--sandbox", "workspace-write"])

    def test_an_unparseable_child_result_is_never_a_pass(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, _ = self._fake_binary(root, "success")
            binary.write_text("#!/bin/sh\necho 'not json'\n", encoding="utf-8")
            binary.chmod(0o755)
            receipt = run_exec_trial(binary, "workspace-write", 30.0,
                                     ledger_root=root / "ledger")
            self.assertIs(receipt["ok"], False)
            self.assertNotEqual(receipt["ledger_manifest"]["terminal"], "passed")

    def test_the_real_headless_binary_never_passes_without_the_artifact(self) -> None:
        binary = Path(__file__).resolve().parents[2] / "target" / "debug" / "codegg"
        if not binary.exists():
            self.skipTest("the debug binary is not built")
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            receipt = run_exec_trial(binary, "workspace-write", 120.0,
                                     ledger_root=root / "ledger")
            terminal = receipt["ledger_manifest"]["terminal"]
            # A pass is only ever recorded for an artifact that exists: the
            # headless child reporting `success` proves nothing on its own.
            if terminal == "passed":
                self.assertTrue(receipt["tool_receipt_created"])
            self.assertFalse(self._lane_root(receipt).exists())


class JobObservationProjectionTests(unittest.TestCase):
    def test_native_json_job_status_projects_exact_identity_and_lifecycle(self) -> None:
        projected = _project_job_observation(
            "job", "status", '{"job_id":"job_abc-123","status":"running","summary":"secret"}')
        self.assertEqual(projected, {
            "action": "status", "job_id": "job_abc-123", "status": "running"})

    def test_missing_or_unrecognized_job_fields_stay_unknown(self) -> None:
        projected = _project_job_observation("job", "status", '{"status":"surprising"}')
        self.assertEqual(projected, {"action": "status", "job_id": None, "status": None})

    def test_native_error_output_preserves_terminal_failure_state(self) -> None:
        projected = _project_job_observation(
            "job", "status", "Error: tool execution failed: job failed\njob_id: job_abc-123")
        self.assertEqual(projected, {"action": "status", "job_id": "job_abc-123", "status": "failed"})

    def test_native_test_submission_extracts_only_bounded_job_identity(self) -> None:
        projected = _project_job_observation(
            "test", None, "background test submitted as job job_abc-123; terminal notification follows")
        self.assertEqual(projected, {"action": "submit", "job_id": "job_abc-123", "status": None})


class ManagedTestDiagnosticsTests(unittest.TestCase):
    def test_reads_only_a_bounded_log_tail_from_the_workspace(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            workspace = Path(temp)
            log = workspace / "job-check" / ".codegg" / "stderr.log"
            log.parent.mkdir(parents=True)
            log.write_text("x" * 5000 + "tail", encoding="utf-8")
            updates = [{"rawOutput": "stderr: [CWD]/job-check/.codegg/stderr.log\n"}]

            tail = _bounded_managed_test_log_tail(updates, workspace)

        self.assertEqual(len(tail), 4096)
        self.assertTrue(tail.endswith("tail"))

    def test_refuses_a_managed_log_path_outside_the_workspace(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            outside = root / "stderr.log"
            outside.write_text("private", encoding="utf-8")
            updates = [{"rawOutput": "stderr: [CWD]/../stderr.log\n"}]

            tail = _bounded_managed_test_log_tail(updates, workspace)

        self.assertEqual(tail, "")


if __name__ == "__main__":
    unittest.main()
