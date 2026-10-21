"""Live pilot driver for the frozen Harness Lab task fixtures.

This module runs one frozen task fixture against a **live** provider route
through the native CodeGG ACP adapter, records a redacted receipt, and runs the
fixture's own independent verifier afterwards. It is the instrument behind work
packages B and C of
``plans/implementation/harness-lab-post-pilot-corrective/004-sustained-agent-benchmark-qualification.md``
and work package A of
``plans/implementation/harness-lab-post-pilot-corrective/003-live-proposer-correction-and-acceptance.md``.

Why this exists
---------------
The M005 pilot ran a frozen task against a live route, but the driver was
ad-hoc and never committed, so its procedure survived only as a prose string.
This module makes the procedure repeatable and auditable: the same frozen
fixture, the same verifier, the same policy receipt, and a receipt that records
the C001 failed-turn evidence instead of a bare error string.

Isolation
---------
Each run gets a private lane: its own daemon home, data root, config root,
``HOME`` and XDG roots, scratch directory and workspace copy. The developer's
default daemon is never touched, and the lane's own credential is the only
provider credential the lane can see.

Fail-closed contract
--------------------
* A live run requires an **admitted budget policy**. The admission decision is
  taken by :mod:`dev.harness_lab.budget` before any provider request, and the
  run enforces the admitted caps through a :class:`~dev.harness_lab.budget.SpendGuard`.
* The provider request cap is enforced *before* the prompt is submitted.
* A missing measurement is never zero: absent token or cost figures stay
  ``None`` with a named reason.
* The task workspace is a copy. The frozen fixture is never written to.
"""
from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import threading
import time
from pathlib import Path
from typing import Any, Optional

from . import budget
from . import supervisor
from .supervisor import (AcpClient, HarnessError, HarnessTimeout, HarnessTurnFailure,
                         SubjectSpec, sha256_file)

#: The frozen pilot task fixtures, relative to the repository root.
PILOT_ROOT_RELPATH = "dev/harness_lab/pilot/baseline"

#: The frozen task ids the M005 pilot used. ``worklog`` is Task 1 and its
#: receipts are historical evidence; ``ranges`` is Task 2, which M005 never ran.
FROZEN_TASKS = ("worklog", "ranges")

#: The provider ids whose routes this driver may use. The value is the
#: OpenCode CLI's own provider id, which is also the shared profile's id.
OPENCODE_GO = "opencode-go"

#: How long one task prompt may take before the run is abandoned.
DEFAULT_TIMEOUT_SECONDS = 900.0

#: The ACP protocol version this driver speaks.
ACP_PROTOCOL_VERSION = 1

__all__ = [
    "FROZEN_TASKS",
    "OPENCODE_GO",
    "PILOT_ROOT_RELPATH",
    "fixture_digest",
    "opencode_go_credential",
    "run_live_task",
]


def _repo_root() -> Path:
    """Return the repository root that contains this development package."""
    return Path(__file__).resolve().parents[2]


def fixture_digest(directory: Path) -> str:
    """Return the aggregate digest of a frozen task fixture directory.

    The digest is the SHA-256 of a compact JSON array of ``[filename, sha256]``
    pairs with the filenames sorted ascending. This is the algorithm the M005
    receipt used, so a re-run can prove its baseline copy is byte-identical to
    the frozen one.
    """
    names = sorted(entry.name for entry in directory.iterdir() if entry.is_file())
    pairs = [[name, sha256_file(directory / name)] for name in names]
    encoded = json.dumps(pairs, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def opencode_go_credential(home: Optional[Path] = None) -> Optional[str]:
    """Return the developer's OpenCode Go credential, or ``None``.

    The value is read from OpenCode's own credential store and is never
    printed, logged, or written into a receipt. ``None`` means the route has no
    credential available and the caller must refuse the run rather than fall
    back to an unauthenticated request.
    """
    root = home or Path(os.path.expanduser("~"))
    path = root / ".local" / "share" / "opencode" / "auth.json"
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    if not isinstance(raw, dict):
        return None
    entry = raw.get(OPENCODE_GO)
    if not isinstance(entry, dict):
        return None
    key = entry.get("key")
    if not isinstance(key, str) or not key:
        return None
    return key


def _copy_fixture(source: Path, destination: Path) -> str:
    """Copy a frozen fixture into a private workspace and return its digest.

    The copy is verified against the source digest, so a run always starts from
    a byte-identical baseline and the receipt can prove it.
    """
    destination.mkdir(parents=True, exist_ok=True)
    for entry in sorted(source.iterdir()):
        if entry.is_file():
            shutil.copy2(entry, destination / entry.name)
        elif entry.is_dir():
            shutil.copytree(entry, destination / entry.name, dirs_exist_ok=True)
    source_digest = fixture_digest(source)
    copied_digest = fixture_digest(destination)
    if source_digest != copied_digest:
        raise HarnessError("the task workspace copy is not byte-identical to the frozen fixture")
    return source_digest


def _run_verifier(workspace: Path, command: list[str], timeout: float) -> dict[str, Any]:
    """Run the fixture's independent verifier and summarize the outcome."""
    started = time.monotonic()
    try:
        completed = subprocess.run(command, cwd=workspace, capture_output=True, text=True,
                                   timeout=timeout, check=False)
    except subprocess.TimeoutExpired:
        return {"command": list(command), "exit_code": None, "timed_out": True,
                "duration_ms": int((time.monotonic() - started) * 1000),
                "output_tail": "", "tests_run": None, "failure_summary": None}
    output = (completed.stdout or "") + (completed.stderr or "")
    tests_run = None
    for line in output.splitlines():
        stripped = line.strip()
        if stripped.startswith("Ran ") and " test" in stripped:
            try:
                tests_run = int(stripped.split()[1])
            except (IndexError, ValueError):
                tests_run = None
    failure_summary = None
    if completed.returncode != 0:
        for label in ("failures", "errors"):
            if f"{label}=" in output:
                failure_summary = next(part for part in output.split()
                                       if f"{label}=" in part).strip()
                break
    tail = "\n".join(output.splitlines()[-12:])
    return {"command": list(command), "exit_code": completed.returncode, "timed_out": False,
            "duration_ms": int((time.monotonic() - started) * 1000),
            "output_tail": tail, "tests_run": tests_run, "failure_summary": failure_summary}


def _tool_updates(client: AcpClient) -> tuple[list[str], list[Optional[str]]]:
    """Return the observed tool names and their terminal statuses."""
    sequence: list[str] = []
    statuses: list[Optional[str]] = []
    for notification in client.notifications:
        if notification.get("method") != "session/update":
            continue
        update = notification.get("params", {}).get("update", {})
        if not isinstance(update, dict):
            continue
        kind = update.get("sessionUpdate")
        if kind == "tool_call":
            title = update.get("title")
            name = update.get("rawInput", {}).get("toolName") if isinstance(update.get("rawInput"), dict) else None
            label = name or title
            if isinstance(label, str):
                sequence.append(label)
        elif kind == "tool_call_update":
            status = update.get("status")
            statuses.append(status if isinstance(status, str) else None)
    return sequence, statuses


def _cancel_after_first_tool_call(client: AcpClient, session_id: str) -> None:
    """Cancel the live turn as soon as the agent starts acting.

    The cancellation is issued from a separate thread because the prompt
    request blocks the reader; the thread waits for the first ``tool_call``
    notification so the cancellation lands mid-turn rather than before the
    turn started.
    """
    import threading

    def watcher() -> None:
        deadline = time.monotonic() + client.timeout
        while time.monotonic() < deadline:
            for notification in list(client.notifications):
                if notification.get("method") != "session/update":
                    continue
                update = notification.get("params", {}).get("update", {})
                if isinstance(update, dict) and update.get("sessionUpdate") == "tool_call":
                    try:
                        client.notify("session/cancel", {"sessionId": session_id})
                    except Exception:
                        return
                    return
            time.sleep(0.05)

    thread = threading.Thread(target=watcher, daemon=True)
    thread.start()


def _abrupt_drop_after_first_tool_call(client: AcpClient) -> None:
    """Drop the ACP connection abruptly once the agent starts acting.

    The client process is killed rather than closed, so the daemon sees a
    transport loss instead of a clean shutdown. This is the "restart/abrupt
    ACP drop" case the sustained scenario must survive.
    """
    import threading

    def watcher() -> None:
        deadline = time.monotonic() + client.timeout
        while time.monotonic() < deadline:
            for notification in list(client.notifications):
                if notification.get("method") != "session/update":
                    continue
                update = notification.get("params", {}).get("update", {})
                if isinstance(update, dict) and update.get("sessionUpdate") == "tool_call":
                    try:
                        client.process.kill()
                    except Exception:
                        return
                    return
            time.sleep(0.05)

    thread = threading.Thread(target=watcher, daemon=True)
    thread.start()


def _prepare_lane_home(env: dict[str, str]) -> None:
    """Create the directories a lane home must already own.

    The daemon resolves its scratch path through the platform's cache
    directory. Inside a private lane that path does not exist yet, and the
    daemon refuses a scratch path that is not a real directory. Creating it
    here keeps the lane self-contained instead of inheriting the developer's
    real caches.
    """
    home = Path(env["HOME"])
    for relative in ("Library/Caches", "Library/Logs", "Library/Application Support",
                     ".cache", ".config", ".local/share"):
        (home / relative).mkdir(parents=True, exist_ok=True)


def run_live_task(binary: Path, task: str, model: str, *, budget_policy: budget.BudgetPolicy,
                  profile: str = "workspace-write", root: Optional[Path] = None,
                  repository: Optional[Path] = None, timeout: float = DEFAULT_TIMEOUT_SECONDS,
                  agent: str = "build", credential: Optional[str] = None,
                  verifier: Optional[list[str]] = None, fault: Optional[str] = None) -> dict[str, Any]:
    """Run one frozen task against a live provider route and return a receipt.

    ``model`` is the CodeGG ``provider/model`` composite, for example
    ``opencode_go/muse-spark-1.3-contributor``. The provider half is a registry
    id; the model half is the bare discovered model id.

    ``fault`` injects a mid-turn interruption: ``"cancel"`` cancels the live
    turn after the agent's first tool call, and ``"abrupt_acp_drop"`` kills
    the ACP connection at the same point. Both leave the turn incomplete on
    purpose, so the receipt reports them as stopped, never as solved.
    """
    if task not in FROZEN_TASKS:
        raise HarnessError(f"unknown frozen task: {task}")
    if profile not in {"full-host", "workspace-write"}:
        raise HarnessError("profile must be full-host or workspace-write")
    if fault not in (None, "cancel", "abrupt_acp_drop"):
        raise HarnessError("fault must be null, cancel, or abrupt_acp_drop")
    if "/" not in model:
        raise HarnessError("model must be a provider/model composite")
    admission = budget.admit_budget(budget_policy)
    if admission.status != budget.STATUS_ADMITTED:
        raise HarnessError("budget admission refused before the live task started: "
                           + json.dumps(admission.as_dict(), sort_keys=True))

    repository = repository or _repo_root()
    source = repository / PILOT_ROOT_RELPATH / task
    if not source.is_dir():
        raise HarnessError(f"frozen task fixture is missing: {source}")

    guard = budget.SpendGuard(budget_policy)
    # The request cap is enforced before the prompt is submitted, so the run
    # never crosses the ceiling and stops afterwards.
    guard.admit()

    lane_root = root or supervisor.lane_root("pilot-" + task)
    # The daemon refuses a managed scratch path that contains a symlinked
    # component. On macOS the system temporary directory lives under
    # ``/var`` -> ``/private/var``, so the lane root is resolved to its
    # canonical path before anything is created under it.
    lane_root = lane_root.resolve()
    lane_root.mkdir(parents=True, exist_ok=True)
    workspace = lane_root / "workspace"
    spec = SubjectSpec.create(f"pilot-{task}", binary, workspace, lane_root, repository)
    env = spec.environment()
    workspace.mkdir(parents=True, exist_ok=True)
    _prepare_lane_home(env)
    fixture_sha256 = _copy_fixture(source, workspace)

    key = credential if credential is not None else opencode_go_credential()
    if not key:
        raise HarnessError("no OpenCode Go credential is available for the approved route")
    env["OPENCODE_GO_API_KEY"] = key

    config_path = Path(env["CODEGG_TUI_CONFIG"])
    config_path.parent.mkdir(parents=True, exist_ok=True)
    config_path.write_text(json.dumps({"model": model}), encoding="utf-8")

    prompt_text = (workspace / "PROMPT.md").read_text(encoding="utf-8")
    client = AcpClient(binary, env, workspace, timeout, profile)
    started = time.monotonic()
    terminal: str
    failure: Optional[HarnessTurnFailure] = None
    result: dict[str, Any] = {}
    fault_outcome: Optional[str] = None
    session_id: Optional[str] = None
    try:
        client.request("initialize", {"protocolVersion": ACP_PROTOCOL_VERSION,
                                      "clientInfo": {"name": "codegg-harness-lab", "version": "1"}})
        opened = client.request("session/new", {"cwd": str(workspace),
                                                "title": f"harness-lab-live-pilot-{task}"})
        session_id = opened.get("sessionId")
        if not isinstance(session_id, str) or not session_id:
            raise HarnessError("ACP did not return a session id for the live task")
        policy_receipt = opened.get("codeggPolicy")
        if not isinstance(policy_receipt, dict):
            raise HarnessError("the live task omitted the effective ACP policy receipt")
        if fault in ("cancel", "abrupt_acp_drop"):
            # The turn must really start before the fault lands, so the prompt
            # runs on its own thread and the fault is applied to the live
            # session while that thread is still waiting.
            prompt_params = {
                "sessionId": session_id,
                "model": model,
                "codeggAgent": agent,
                "prompt": [{"type": "text", "text": prompt_text}],
            }
            prompt_result: dict[str, Any] = {}
            prompt_error: list[BaseException] = []

            def submit_prompt() -> None:
                try:
                    prompt_result.update(client.request("session/prompt", prompt_params,
                                                        timeout=timeout))
                except BaseException as error:  # noqa: BLE001 - recorded, then classified
                    prompt_error.append(error)

            prompt_thread = threading.Thread(target=submit_prompt, daemon=True)
            prompt_thread.start()
            if fault == "cancel":
                _cancel_after_first_tool_call(client, session_id)
            else:
                _abrupt_drop_after_first_tool_call(client)
            prompt_thread.join(timeout=min(timeout, 30))
            fault_outcome = fault
            if prompt_error:
                error = prompt_error[0]
                if isinstance(error, HarnessTurnFailure):
                    failure = error
                    terminal = "cancelled"
                else:
                    terminal = "cancelled" if fault == "cancel" else "abrupt_acp_drop"
            else:
                terminal = "cancelled" if fault == "cancel" else "abrupt_acp_drop"
        else:
            result = client.request("session/prompt", {
                "sessionId": session_id,
                "model": model,
                "codeggAgent": agent,
                "prompt": [{"type": "text", "text": prompt_text}],
            }, timeout=timeout)
            terminal = "completed"
    except HarnessTurnFailure as error:
        failure = error
        terminal = "failed"
    except HarnessTimeout as error:
        failure = None
        terminal = "timeout"
        result = {"error": str(error)}
    finally:
        client.close()

    duration_ms = int((time.monotonic() - started) * 1000)
    sequence, statuses = _tool_updates(client)

    raw_usage = result.get("codeggUsage") if isinstance(result, dict) else None
    usage = None
    if isinstance(raw_usage, dict):
        usage = {
            "turn_id": raw_usage.get("turnId"),
            "input_tokens": raw_usage.get("inputTokens"),
            "output_tokens": raw_usage.get("outputTokens"),
            "cached_tokens": raw_usage.get("cachedTokens"),
            "reasoning_tokens": raw_usage.get("reasoningTokens"),
        }
    guard.observe(provider_requests=1,
                  input_tokens=int((usage or {}).get("input_tokens") or 0),
                  output_tokens=int((usage or {}).get("output_tokens") or 0))

    command = verifier or ["python3", "-m", "unittest", "-v"]
    verification = _run_verifier(workspace, command, timeout)

    def digests(directory: Path) -> dict[str, str]:
        return {entry.name: sha256_file(entry)
                for entry in sorted(directory.iterdir()) if entry.is_file()}

    before = digests(source)
    after = digests(workspace)
    changed = sorted(name for name in after
                     if name in before and after[name] != before[name])
    created = sorted(name for name in after if name not in before)
    removed = sorted(name for name in before if name not in after)
    protected_changed = sorted(name for name in set(changed) | set(created)
                                if name.startswith("test_") or name == "README.md")

    receipt: dict[str, Any] = {
        "schema_version": 1,
        "task_id": task,
        "model": model,
        "agent": agent,
        "profile": profile,
        "binary_sha256": spec.binary_sha256,
        "source_revision": spec.revision,
        "fixture_sha256": fixture_sha256,
        "terminal": terminal,
        "fault": fault,
        "fault_outcome": fault_outcome,
        "duration_ms": duration_ms,
        "tool_call_count": len(sequence),
        "tool_sequence": sequence,
        "tool_event_statuses": statuses,
        "provider_usage": usage,
        "usage_unavailable_reason": None if usage else "not_reported_by_the_route",
        "stop_reason": result.get("stopReason") if isinstance(result, dict) else None,
        "changed_task_files": changed,
        "created_files": created,
        "removed_files": removed,
        "protected_files_changed": protected_changed,
        "verification": verification,
        "budget": guard.as_dict(),
        "policy_receipt": policy_receipt if isinstance(policy_receipt, dict) else None,
    }
    if failure is not None:
        receipt["terminal_failure"] = {
            "kind": type(failure).__name__,
            "category": failure.category,
            "usage": failure.usage,
            "usage_reason": failure.usage_reason,
        }
    else:
        receipt["terminal_failure"] = None
    receipt["ok"] = (verification.get("exit_code") == 0
                     and not protected_changed
                     and terminal == "completed")
    return receipt
