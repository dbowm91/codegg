#!/usr/bin/env python3
"""Tool Program Harness — M010 external scenario runner.

Usage:
    python3 scripts/e2e/tool_program_harness.py --mode scripted --scenario all
    python3 scripts/e2e/tool_program_harness.py --mode eggpool --model mimo-v2.5 --no-model-fallback
    python3 scripts/e2e/tool_program_harness.py --mode acp --scenario all  # when ACP available

Modes:
    scripted  — deterministic in-process scenarios via cargo test
    eggpool   — live Eggpool model validation (requires CODEGG_EGGPOOL_URL + CODEGG_EGGPOOL_API_KEY)
    acp       — ACP transport adapter (when available)

This harness is a client of CodeGG, not an alternate executor.
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional


REPO_ROOT = Path(__file__).resolve().parent.parent.parent


@dataclass
class ScenarioResult:
    name: str
    passed: bool
    duration_ms: int
    details: str = ""


@dataclass
class HarnessReport:
    mode: str
    total: int = 0
    passed: int = 0
    failed: int = 0
    skipped: int = 0
    results: list = field(default_factory=list)
    duration_ms: int = 0

    def summary(self) -> str:
        lines = [
            f"=== Tool Program Harness Report ({self.mode} mode) ===",
            f"Total: {self.total}  Passed: {self.passed}  Failed: {self.failed}  Skipped: {self.skipped}",
            f"Duration: {self.duration_ms}ms",
        ]
        for r in self.results:
            status = "PASS" if r.passed else ("SKIP" if r.duration_ms == 0 else "FAIL")
            lines.append(f"  [{status}] {r.name} ({r.duration_ms}ms)")
            if r.details:
                lines.append(f"        {r.details}")
        return "\n".join(lines)


def run_cargo_test(test_name: str, extra_args: Optional[list] = None) -> tuple:
    """Run a cargo test and return (success, duration_ms, output)."""
    cmd = ["cargo", "test", "-p", "codegg", "--test", test_name]
    if extra_args:
        cmd.extend(extra_args)
    cmd.extend(["--", "--test-threads=1"])

    start = time.monotonic()
    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        cwd=str(REPO_ROOT),
        timeout=300,
    )
    duration_ms = int((time.monotonic() - start) * 1000)
    return result.returncode == 0, duration_ms, result.stdout + result.stderr


def run_scripted_scenarios(scenario_filter: str) -> HarnessReport:
    """Run deterministic in-process scenarios via cargo test."""
    report = HarnessReport(mode="scripted")
    start = time.monotonic()

    test_files = [
        ("tool_program_scenarios", "scenario-level tests"),
        ("tool_program_chaos", "chaos/fault-injection tests"),
        ("tool_program_resource_convergence", "resource convergence tests"),
        ("tool_program_model_behavior", "model behavior validation tests"),
    ]

    for test_name, description in test_files:
        if scenario_filter != "all" and scenario_filter not in test_name:
            continue
        report.total += 1
        try:
            passed, duration, output = run_cargo_test(test_name)
            result = ScenarioResult(
                name=f"{test_name} ({description})",
                passed=passed,
                duration_ms=duration,
                details="" if passed else output[-500:],
            )
            report.results.append(result)
            if passed:
                report.passed += 1
            else:
                report.failed += 1
        except subprocess.TimeoutExpired:
            report.results.append(ScenarioResult(
                name=f"{test_name} ({description})",
                passed=False,
                duration_ms=300000,
                details="TIMEOUT after 300s",
            ))
            report.failed += 1
        except Exception as e:
            report.results.append(ScenarioResult(
                name=f"{test_name} ({description})",
                passed=False,
                duration_ms=0,
                details=str(e),
            ))
            report.failed += 1

    # Also run existing tool program tests for broader coverage
    existing_tests = [
        "tool_program_read_palette",
        "tool_program_child_jobs",
        "tool_program_build_test_matrix",
        "tool_program_child_recovery",
        "tool_program_background",
        "tool_program_notifications",
        "tool_program_projection",
        "tool_program_lifecycle",
    ]

    for test_name in existing_tests:
        if scenario_filter != "all" and scenario_filter not in test_name:
            continue
        report.total += 1
        try:
            passed, duration, output = run_cargo_test(test_name)
            result = ScenarioResult(
                name=f"{test_name} (existing)",
                passed=passed,
                duration_ms=duration,
                details="" if passed else output[-500:],
            )
            report.results.append(result)
            if passed:
                report.passed += 1
            else:
                report.failed += 1
        except subprocess.TimeoutExpired:
            report.results.append(ScenarioResult(
                name=f"{test_name} (existing)",
                passed=False,
                duration_ms=300000,
                details="TIMEOUT",
            ))
            report.failed += 1
        except Exception as e:
            report.results.append(ScenarioResult(
                name=f"{test_name} (existing)",
                passed=False,
                duration_ms=0,
                details=str(e),
            ))
            report.failed += 1

    report.duration_ms = int((time.monotonic() - start) * 1000)
    return report


class StdioCoreClient:
    """Small JSONL client for the production ``core-stdio`` transport."""

    def __init__(self, workspace: Path):
        binary = REPO_ROOT / "target" / "debug" / "codegg"
        if not binary.exists():
            build = subprocess.run(
                ["cargo", "build", "-p", "codegg", "--bin", "codegg"],
                cwd=str(REPO_ROOT),
                capture_output=True,
                text=True,
                timeout=300,
            )
            if build.returncode != 0:
                raise RuntimeError("could not build the CodeGG binary for native harness")
        self.catalog = tempfile.TemporaryDirectory(prefix="codegg-m010-catalog-")
        child_env = os.environ.copy()
        child_env["CODEGG_CORE_STDIO_CATALOG"] = self.catalog.name
        self.process = subprocess.Popen(
            [str(binary), "core-stdio"],
            cwd=str(workspace),
            env=child_env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self.request_id = 0

    def request(self, payload: dict) -> dict:
        self.request_id += 1
        envelope = {
            "protocol_version": 2,
            "request_id": f"m010-{self.request_id}",
            "payload": payload,
        }
        assert self.process.stdin is not None
        assert self.process.stdout is not None
        self.process.stdin.write(json.dumps(envelope) + "\n")
        self.process.stdin.flush()
        line = self.process.stdout.readline()
        if not line:
            diagnostics = ""
            if self.process.poll() is not None and self.process.stderr is not None:
                diagnostics = self.process.stderr.read()[-500:]
            raise RuntimeError(
                "core-stdio exited before returning a response "
                f"(exit={self.process.returncode}); stderr={diagnostics!r}"
            )
        try:
            response = json.loads(line)
        except json.JSONDecodeError as error:
            # Keep transport diagnostics bounded and avoid echoing any
            # accidental long output from a misbehaving core process.
            raise RuntimeError(
                f"invalid core-stdio response ({error}); line={line[:240]!r}"
            ) from error
        if response.get("type") == "error":
            message = str(response.get("message", ""))[:240]
            raise RuntimeError(
                f"core request failed: {response.get('code', 'error')}: {message}"
            )
        return response

    def close(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
        self.catalog.cleanup()


def persist_native_source(workspace: Path, source: str) -> tuple[str, int]:
    """Persist an immutable source reference for a native daemon job."""
    digest = hashlib.sha256(source.encode()).hexdigest()
    directory = workspace / ".codegg" / "tool_program_sources"
    directory.mkdir(parents=True, exist_ok=True)
    target = directory / f"{digest}.py"
    if target.exists() and hashlib.sha256(target.read_bytes()).hexdigest() != digest:
        raise RuntimeError("existing native source reference failed digest verification")
    if not target.exists():
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=directory, delete=False
        ) as temporary:
            temporary.write(source)
            temporary_name = temporary.name
        os.replace(temporary_name, target)
    return f"{digest}.py", len(source.encode())


def run_native_scenario() -> ScenarioResult:
    """Exercise the production scheduler/executor through core-stdio."""
    started = time.monotonic()
    client = None
    try:
        workspace = REPO_ROOT
        source = 'result = call({"tool": "read", "path": "Cargo.toml"})\n'
        source_digest = hashlib.sha256(source.encode()).hexdigest()
        source_ref, source_length = persist_native_source(workspace, source)
        client = StdioCoreClient(workspace)
        workspace_response = client.request({"type": "workspace_register", "root": str(workspace)})
        workspace_id = workspace_response["workspace"]["workspace_id"]
        program_id = f"tp-m010-{source_digest[:16]}"
        submitted = client.request(
            {
                "type": "job_submit",
                "spec": {
                    "submission_key": f"m010-native-{source_digest[:16]}",
                    "workspace_id": workspace_id,
                    "session_id": "m010-harness-session",
                    "kind": "tool_program",
                    "priority": "normal",
                    "source": {"kind": "interactive"},
                    "payload": {
                        "kind": "tool_program",
                        "program_id": program_id,
                        "source_digest": source_digest,
                        "ir_digest": None,
                        "authority_digest": "m010-harness-authority",
                        "submission_key": f"m010-native-{source_digest[:16]}",
                        "source_ref": source_ref,
                        "source_length": source_length,
                        "allowed_tools": ["read"],
                    },
                    "timeout_ms": 120000,
                    "retry_max_attempts": 1,
                    "idempotency": "safe_repeat",
                },
            }
        )
        job_id = submitted["job_id"]
        waited = client.request({"type": "job_wait", "job_id": job_id, "timeout_ms": 180000})
        if waited.get("status") != "completed":
            attempts = client.request({"type": "job_attempts", "job_id": job_id})
            raise RuntimeError(
                "native job terminal status was "
                f"{waited.get('status')}; summary={str(waited.get('summary', ''))[:240]}; "
                f"attempts={str(attempts.get('attempts', []))[:400]}"
            )
        job = client.request({"type": "job_get", "job_id": job_id}).get("job")
        if not job or job.get("state") != "completed":
            raise RuntimeError("native job record did not converge to completed")
        listed = client.request(
            {
                "type": "tool_program_list",
                "session_id": "m010-harness-session",
            }
        )
        if not any(p.get("program_id") == program_id for p in listed.get("programs", [])):
            raise RuntimeError("native Tool Program was absent from public inspection")
        inspected = client.request(
            {"type": "tool_program_inspect", "program_id": program_id}
        ).get("detail")
        if not inspected or inspected.get("source_hash") != source_digest:
            raise RuntimeError("native inspection did not retain the source digest")
        page = client.request(
            {"type": "tool_program_call_page", "program_id": program_id, "offset": 0}
        ).get("page")
        if not page or page.get("total_calls") != 1 or len(page.get("calls", [])) != 1:
            raise RuntimeError(f"native call ledger did not expose the completed read call: {page!r}")
        duration = int((time.monotonic() - started) * 1000)
        return ScenarioResult(
            name="native_core_stdio_production_path",
            passed=True,
            duration_ms=duration,
            details="scheduler, executor, JobStore, and public inspection converged",
        )
    except Exception as error:
        duration = int((time.monotonic() - started) * 1000)
        return ScenarioResult(
            name="native_core_stdio_production_path",
            passed=False,
            duration_ms=duration,
            details=str(error),
        )
    finally:
        if client is not None:
            client.close()


def run_native_mode() -> HarnessReport:
    report = HarnessReport(mode="native")
    report.total = 1
    result = run_native_scenario()
    report.results.append(result)
    if result.passed:
        report.passed = 1
    else:
        report.failed = 1
    report.duration_ms = result.duration_ms
    return report


def run_eggpool_mode(model: str, no_fallback: bool) -> HarnessReport:
    """Run live Eggpool model validation."""
    report = HarnessReport(mode="eggpool")
    start = time.monotonic()

    eggpool_url = os.environ.get("CODEGG_EGGPOOL_URL")
    eggpool_key = os.environ.get("CODEGG_EGGPOOL_API_KEY")
    connection_id = os.environ.get("CODEGG_EGGPOOL_CONNECTION_ID")

    if not eggpool_url or not eggpool_key:
        report.total = 1
        report.skipped = 1
        report.results.append(ScenarioResult(
            name="eggpool_live_model",
            passed=False,
            duration_ms=0,
            details="SKIPPED: CODEGG_EGGPOOL_URL or CODEGG_EGGPOOL_API_KEY not set",
        ))
        report.duration_ms = int((time.monotonic() - start) * 1000)
        return report

    if not no_fallback:
        report.total = 1
        report.failed = 1
        report.results.append(ScenarioResult(
            name="eggpool_model_policy",
            passed=False,
            duration_ms=0,
            details="FAIL: live Eggpool mode requires --no-model-fallback",
        ))
        report.duration_ms = int((time.monotonic() - start) * 1000)
        return report

    if not connection_id:
        report.total = 1
        report.failed = 1
        report.results.append(ScenarioResult(
            name="eggpool_connection_selection",
            passed=False,
            duration_ms=0,
            details="FAIL: CODEGG_EGGPOOL_CONNECTION_ID is required for explicit provider selection",
        ))
        report.duration_ms = int((time.monotonic() - start) * 1000)
        return report

    # Verify model identity
    report.total += 1
    identity_started = time.monotonic()
    endpoint_scheme = "unknown"
    try:
        import urllib.request
        import urllib.error
        from urllib.parse import urlparse

        base_url = eggpool_url.rstrip("/")
        endpoint_scheme = urlparse(base_url).scheme or "unknown"

        req = urllib.request.Request(
            f"{base_url}/v1/models",
            headers={"Authorization": f"Bearer {eggpool_key}"},
        )
        resp = urllib.request.urlopen(req, timeout=10)
        models_data = json.loads(resp.read())

        # Check that mimo-v2.5 is available and not the pro variant
        model_ids = [m.get("id", "") for m in models_data.get("data", [])]
        target_model = model
        if target_model not in model_ids:
            report.results.append(ScenarioResult(
                name="eggpool_model_identity",
                passed=False,
                duration_ms=0,
                details=f"Model '{target_model}' not found. Available: {model_ids}",
            ))
            report.failed += 1
        else:
            # Verify not pro variant
            if target_model.endswith("-pro") or "pro" in target_model.lower():
                report.results.append(ScenarioResult(
                    name="eggpool_model_identity",
                    passed=False,
                    duration_ms=0,
                    details=f"Model '{target_model}' appears to be pro variant",
                ))
                report.failed += 1
            else:
                report.results.append(ScenarioResult(
                    name="eggpool_model_identity",
                    passed=True,
                    duration_ms=int((time.monotonic() - identity_started) * 1000),
                    details=(
                        f"Exact model '{target_model}' verified; "
                        f"endpoint_class={endpoint_scheme}; connection_id={connection_id}"
                    ),
                ))
                report.passed += 1
    except Exception as e:
        report.results.append(ScenarioResult(
            name="eggpool_model_identity",
            passed=False,
            duration_ms=int((time.monotonic() - identity_started) * 1000),
            details=f"Connection failed: {type(e).__name__}",
        ))
        report.failed += 1

    # Run one bounded OpenAI-compatible behavior probe only after exact
    # identity succeeds. The response body is never printed or persisted.
    if report.failed == 0:
        report.total += 1
        behavior_started = time.monotonic()
        try:
            body = json.dumps({
                "model": model,
                "messages": [{"role": "user", "content": "Reply with exactly OK."}],
                "max_tokens": 4,
                "temperature": 0,
            }).encode()
            req = urllib.request.Request(
                f"{base_url}/v1/chat/completions",
                data=body,
                method="POST",
                headers={
                    "Authorization": f"Bearer {eggpool_key}",
                    "Content-Type": "application/json",
                },
            )
            response = json.loads(urllib.request.urlopen(req, timeout=30).read())
            returned_model = response.get("model")
            if returned_model is not None and returned_model != model:
                raise RuntimeError("provider returned a different model identity")
            report.results.append(ScenarioResult(
                name="eggpool_model_behavior",
                passed=True,
                duration_ms=int((time.monotonic() - behavior_started) * 1000),
                details="bounded exact-model request completed; response body redacted",
            ))
            report.passed += 1
        except Exception as e:
            report.results.append(ScenarioResult(
                name="eggpool_model_behavior",
                passed=False,
                duration_ms=int((time.monotonic() - behavior_started) * 1000),
                details=f"Behavior request failed: {type(e).__name__}",
            ))
            report.failed += 1

    report.duration_ms = int((time.monotonic() - start) * 1000)
    return report


def run_acp_mode(scenario_filter: str) -> HarnessReport:
    """Run ACP transport adapter scenarios (placeholder)."""
    report = HarnessReport(mode="acp")
    report.total = 1
    report.skipped = 1
    report.results.append(ScenarioResult(
        name="acp_adapter",
        passed=False,
        duration_ms=0,
        details="SKIPPED: ACP adapter not yet available",
    ))
    return report


def main():
    parser = argparse.ArgumentParser(
        description="Tool Program Harness — M010 scenario runner"
    )
    parser.add_argument(
        "--mode",
        choices=["scripted", "native", "eggpool", "acp"],
        default="scripted",
        help="Execution mode (default: scripted)",
    )
    parser.add_argument(
        "--scenario",
        default="all",
        help="Scenario filter (default: all)",
    )
    parser.add_argument(
        "--model",
        default="mimo-v2.5",
        help="Eggpool model ID (default: mimo-v2.5)",
    )
    parser.add_argument(
        "--no-model-fallback",
        action="store_true",
        help="Reject model fallback for Eggpool mode",
    )
    args = parser.parse_args()

    if args.mode == "scripted":
        report = run_scripted_scenarios(args.scenario)
        native = run_native_mode()
        report.total += native.total
        report.passed += native.passed
        report.failed += native.failed
        report.skipped += native.skipped
        report.results.extend(native.results)
        report.duration_ms += native.duration_ms
    elif args.mode == "native":
        report = run_native_mode()
    elif args.mode == "eggpool":
        report = run_eggpool_mode(args.model, args.no_model_fallback)
    elif args.mode == "acp":
        report = run_acp_mode(args.scenario)
    else:
        print(f"Unknown mode: {args.mode}", file=sys.stderr)
        sys.exit(1)

    print(report.summary())
    sys.exit(0 if report.failed == 0 else 1)


if __name__ == "__main__":
    main()
