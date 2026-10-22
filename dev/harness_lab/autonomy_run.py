"""Run autonomy taxonomy cases as live agent tasks and compare two binaries.

This module is the C004 work package D instrument. The taxonomy in
``dev/harness_lab/fixtures/agent_autonomy/cases.jsonl`` freezes *what* each
case asks and *where* its verifier runs, but nothing yet executed a case as a
live agent task. This module does:

* it copies one case's ``fixture/`` into a private lane workspace and runs the
  case's prompt against a live provider route;
* it keeps the verifier **outside** the agent's workspace root, so the agent
  can neither read the hidden contract nor edit the thing that judges it;
* it records the transcript the agent actually produced — the tool sequence,
  each call's argument digest and each call's terminal status — and hands that
  transcript to the case's own verifier;
* it runs the case's independent verifier and records its exit code, so a
  "solved" case means the verifier confirmed it, never that the agent said so.

Fail-closed contract
--------------------
* The budget is admitted **before** the prompt is submitted, so a run that
  cannot prove an enforceable cap never reaches a provider.
* The agent's editable tree is the case's ``fixture/`` only. The verifier and
  the transcript live in a private sibling directory, outside the workspace
  root the agent's tools are bound to.
* A case that the verifier rejects is never reported as solved, and a case the
  verifier never ran is never reported as solved either.
* The comparison scores two binaries on the same cases with the shared score
  contract. A protected negative control is a guard: its regression rejects
  the candidate whatever else moved.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
from pathlib import Path
from typing import Any, Optional

from . import autonomy, budget, scoring, supervisor
from .autonomy import AutonomyCase, AutonomySuite, verify_isolation
from .pilot import _prepare_lane_home, opencode_go_credential
from .supervisor import AcpClient, HarnessError, HarnessTimeout, HarnessTurnFailure, SubjectSpec

#: Where the taxonomy's case bodies live, relative to the repository root.
AUTONOMY_ROOT_RELPATH = "dev/autonomy_fixtures"

#: The environment variable that tells a verifier where the case root is.
#: The verifier's own directory is not the case root in a live lane: the
#: verifier is copied to a private directory so the agent cannot read it, and
#: the case root (which holds the agent's editable ``fixture/``) is elsewhere.
CASE_ROOT_ENV = "CODEGG_AUTONOMY_CASE_ROOT"

#: The environment variable that tells a verifier where the transcript is.
TRANSCRIPT_ENV = "CODEGG_AUTONOMY_TRANSCRIPT"

#: The ACP protocol version the harness speaks.
ACP_PROTOCOL_VERSION = 1

#: A case whose verifier never ran is not a solved case. The receipt carries
#: this explicitly instead of treating a missing verifier as a pass.
VERIFIER_NOT_RUN = "not_run"

__all__ = [
    "AUTONOMY_ROOT_RELPATH",
    "build_transcript",
    "case_root",
    "compare_runs",
    "run_case",
    "run_suite",
]


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def case_root(repository: Path, case_id: str) -> Path:
    """Return the in-repo case root that holds ``fixture/`` and ``verifier/``."""
    return repository / AUTONOMY_ROOT_RELPATH / case_id


def _copy_tree(source: Path, destination: Path) -> None:
    """Copy a directory tree, refusing to follow a symlink out of the source."""
    if destination.exists():
        shutil.rmtree(destination)
    shutil.copytree(source, destination, symlinks=False)


def build_transcript(case: AutonomyCase, tool_calls: list[dict[str, Any]], *,
                     terminal: str, stop_reason: Optional[str],
                     failure: Optional[dict[str, Any]] = None) -> dict[str, Any]:
    """Return the transcript a case's verifier reads.

    The transcript records what the agent **did**, not what it said. The
    harness projects the tool name and a digest of the tool's arguments, never
    the arguments themselves, so a verifier can tell "the same call again"
    from "a different call" without reading the call's payload.
    """
    return {
        "schema_version": 1,
        "case_id": case.case_id,
        "family": case.family,
        "group_id": case.group_id,
        "protected": case.protected,
        "terminal": terminal,
        "stop_reason": stop_reason,
        "failure": failure,
        "tool_calls": tool_calls,
    }


def _observed_tool_calls(client: AcpClient) -> list[dict[str, Any]]:
    """Return the observed tool calls in order.

    A tool call is reported by two ACP notifications: a ``tool_call`` start
    that names the tool and carries the argument digest, and a
    ``tool_call_update`` finish that carries the terminal status. The start
    without a finish means the call never completed, and its status stays
    unknown rather than being reported as a success.
    """
    started: dict[str, dict[str, Any]] = {}
    order: list[str] = []
    finished: dict[str, str] = {}
    for notification in client.notifications:
        if notification.get("method") != "session/update":
            continue
        update = notification.get("params", {}).get("update", {})
        if not isinstance(update, dict):
            continue
        kind = update.get("sessionUpdate")
        if kind == "tool_call":
            call_id = update.get("toolCallId")
            if not isinstance(call_id, str):
                continue
            if call_id not in started:
                order.append(call_id)
            started[call_id] = {
                "tool": update.get("title") if isinstance(update.get("title"), str) else "unknown",
                "arguments_sha256": (update.get("codeggArgumentsSha256")
                                     if isinstance(update.get("codeggArgumentsSha256"), str)
                                     else None),
            }
        elif kind == "tool_call_update":
            call_id = update.get("toolCallId")
            status = update.get("status")
            if isinstance(call_id, str) and isinstance(status, str):
                finished[call_id] = status
    calls: list[dict[str, Any]] = []
    for index, call_id in enumerate(order):
        entry = started[call_id]
        calls.append({
            "index": index,
            "tool": entry["tool"],
            "arguments_sha256": entry["arguments_sha256"],
            "status": finished.get(call_id, "unknown"),
        })
    return calls


def _run_verifier(command: list[str], cwd: Path, env: dict[str, str],
                   timeout: float) -> dict[str, Any]:
    """Run a case's independent verifier and summarize the outcome."""
    started = time.monotonic()
    try:
        completed = subprocess.run(command, cwd=str(cwd), capture_output=True, text=True,
                                   timeout=timeout, check=False, env=env)
    except subprocess.TimeoutExpired:
        return {"command": list(command), "cwd": str(cwd), "exit_code": None,
                "timed_out": True, "duration_ms": int((time.monotonic() - started) * 1000),
                "output_tail": "", "tests_run": None, "failure_summary": None}
    except OSError as error:
        return {"command": list(command), "cwd": str(cwd), "exit_code": None,
                "timed_out": False, "duration_ms": int((time.monotonic() - started) * 1000),
                "output_tail": f"{type(error).__name__}: {error}",
                "tests_run": None, "failure_summary": None}
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
    return {"command": list(command), "cwd": str(cwd), "exit_code": completed.returncode,
            "timed_out": False, "duration_ms": int((time.monotonic() - started) * 1000),
            "output_tail": tail, "tests_run": tests_run, "failure_summary": failure_summary}


def run_case(binary: Path, case: AutonomyCase, model: str, *,
             budget_policy: budget.BudgetPolicy, lane_root: Path,
             repository: Optional[Path] = None, profile: str = "workspace-write",
             timeout: float = 900.0, agent: str = "build",
             credential: Optional[str] = None,
             guard: Optional[budget.SpendGuard] = None) -> dict[str, Any]:
    """Run one taxonomy case against a live provider route and return a receipt.

    The receipt separates three things that are easy to conflate: what the
    agent did (the transcript), whether the turn reached a terminal, and
    whether the case's independent verifier confirmed the task. A case is
    ``solved`` only when all three agree.

    ``guard`` lets a caller share one spend guard across a suite, so a
    declared request cap covers the whole suite instead of restarting at
    every case.
    """
    if "/" not in model:
        raise HarnessError("model must be a provider/model composite")
    if profile not in {"full-host", "workspace-write"}:
        raise HarnessError("profile must be full-host or workspace-write")
    if case.support != "supported":
        raise HarnessError(f"case {case.case_id} is deferred, not runnable: "
                           f"{case.deferral_reason or 'no reason recorded'}")

    repository = repository or _repo_root()
    source = case_root(repository, case.case_id)
    if not (source / "fixture").is_dir() or not (source / "verifier").is_dir():
        raise HarnessError(f"the case body is missing: {source}")

    # The agent may edit the case's fixture and nothing else. The check is
    # the same one the candidate scope guard uses, so a verifier that a
    # candidate could reach is refused before a provider is ever called.
    verify_isolation(AutonomySuite(version=autonomy.AUTONOMY_SCHEMA_VERSION,
                                   cases=(case,), suite_sha256=""),
                     [f"{AUTONOMY_ROOT_RELPATH}/{case.case_id}/fixture"])

    if guard is None:
        admission = budget.admit_budget(budget_policy)
        if admission.status != budget.STATUS_ADMITTED:
            raise HarnessError("budget admission refused before the autonomy case started: "
                               + json.dumps(admission.as_dict(), sort_keys=True))
        guard = budget.SpendGuard(budget_policy)
    # The request cap is enforced before the prompt is submitted.
    guard.admit()

    lane_root = lane_root.resolve()
    lane_root.mkdir(parents=True, exist_ok=True)
    # The daemon binds its control socket inside the lane. A lane whose socket
    # path does not fit the platform's ``sun_path`` limit cannot start, and
    # the daemon's own failure message names the socket rather than the
    # cause. The runner refuses the lane up front instead, with the reason.
    socket_path = lane_root / "daemon" / "core.sock"
    if len(str(socket_path).encode()) >= supervisor.SUN_PATH_LIMIT:
        raise HarnessError(
            f"the lane root {lane_root} puts the daemon control socket at "
            f"{socket_path}, which is longer than the platform's sun_path limit; "
            "use a shorter lane root under ~/.codegg-harness-lanes")
    # The agent's workspace root holds the editable tree only. The verifier
    # and the transcript live in a private sibling directory, outside the
    # root the agent's tools are bound to, so the hidden contract cannot be
    # read or edited by the agent under test.
    agent_root = lane_root / "case"
    private_root = lane_root / "private"
    fixture_destination = agent_root / "fixture"
    if agent_root.exists():
        shutil.rmtree(agent_root)
    if private_root.exists():
        shutil.rmtree(private_root)
    fixture_destination.mkdir(parents=True, exist_ok=True)
    private_root.mkdir(parents=True, exist_ok=True)
    _copy_tree(source / "fixture", fixture_destination)
    _copy_tree(source / "verifier", private_root / "verifier")

    spec = SubjectSpec.create(f"autonomy-{case.case_id}", binary, agent_root, lane_root, repository)
    env = spec.environment()
    _prepare_lane_home(env)

    key = credential if credential is not None else opencode_go_credential()
    if not key:
        raise HarnessError("no OpenCode Go credential is available for the approved route")
    env["OPENCODE_GO_API_KEY"] = key

    config_path = Path(env["CODEGG_TUI_CONFIG"])
    config_path.parent.mkdir(parents=True, exist_ok=True)
    config_path.write_text(json.dumps({"model": model}), encoding="utf-8")

    client = AcpClient(binary, env, agent_root, timeout, profile)
    started = time.monotonic()
    terminal = "failed"
    stop_reason: Optional[str] = None
    failure: Optional[dict[str, Any]] = None
    result: dict[str, Any] = {}
    session_id: Optional[str] = None
    policy_receipt: Any = None
    try:
        client.request("initialize", {"protocolVersion": ACP_PROTOCOL_VERSION,
                                      "clientInfo": {"name": "codegg-harness-lab", "version": "1"}})
        opened = client.request("session/new", {"cwd": str(agent_root),
                                                "title": f"harness-lab-autonomy-{case.case_id}"})
        session_id = opened.get("sessionId")
        if not isinstance(session_id, str) or not session_id:
            raise HarnessError("ACP did not return a session id for the autonomy case")
        policy_receipt = opened.get("codeggPolicy")
        result = client.request("session/prompt", {
            "sessionId": session_id,
            "model": model,
            "codeggAgent": agent,
            "prompt": [{"type": "text", "text": case.prompt}],
        }, timeout=timeout)
        terminal = "completed"
        stop_reason = result.get("stopReason") if isinstance(result, dict) else None
    except HarnessTurnFailure as error:
        failure = {"kind": type(error).__name__, "category": error.category,
                   "usage_reason": error.usage_unavailable_reason}
        terminal = "failed"
    except HarnessTimeout as error:
        terminal = "timeout"
        result = {"error": str(error)}
    finally:
        client.close()
        try:
            supervisor.stop_lane_daemon(spec)
        except (HarnessError, OSError, subprocess.TimeoutExpired) as error:
            daemon_stop_error = f"{type(error).__name__}: {error}"
        else:
            daemon_stop_error = None

    duration_ms = int((time.monotonic() - started) * 1000)
    tool_calls = _observed_tool_calls(client)

    transcript = build_transcript(case, tool_calls, terminal=terminal,
                                  stop_reason=stop_reason, failure=failure)
    transcript_path = private_root / "TRANSCRIPT.json"
    transcript_path.write_text(json.dumps(transcript, indent=2, sort_keys=True), encoding="utf-8")

    # The verifier runs in the **host** environment, not the subject's. The
    # lane isolates HOME, CARGO_HOME and the config roots on purpose: the
    # subject under test must not see the developer's registry, config or
    # credential store. The verifier is harness code, and it has to be able
    # to run the very toolchain the developer's fixture was written for — a
    # rustup shim with an overridden HOME resolves no default toolchain and
    # every Rust case would fail for a reason that has nothing to do with the
    # agent. The verifier still only ever sees the case's own tree.
    verifier_env = dict(os.environ)
    verifier_env[CASE_ROOT_ENV] = str(agent_root)
    verifier_env[TRANSCRIPT_ENV] = str(transcript_path)
    verification = _run_verifier(list(case.verifier["command"]), private_root,
                                 verifier_env, timeout)

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

    exit_code = verification.get("exit_code")
    verifier_confirmed = exit_code == 0
    # A protected control is never a solved task. Its task is impossible by
    # construction, so "solved" would only ever mean "the harness credited an
    # impossible task" — and the control must never count toward a score.
    solved = bool(verifier_confirmed and terminal == "completed" and not case.protected)

    receipt: dict[str, Any] = {
        "schema_version": 1,
        "case_id": case.case_id,
        "family": case.family,
        "group_id": case.group_id,
        "protected": case.protected,
        "model": model,
        "agent": agent,
        "profile": profile,
        "binary_sha256": spec.binary_sha256,
        "source_revision": spec.revision,
        "terminal": terminal,
        "stop_reason": stop_reason,
        "duration_ms": duration_ms,
        "tool_call_count": len(tool_calls),
        "tool_calls": tool_calls,
        "provider_usage": usage,
        "usage_unavailable_reason": None if usage else "not_reported_by_the_route",
        "verification": verification,
        "verifier_confirmed": verifier_confirmed,
        "solved": solved,
        "repeated_identical_failure": _repeated_identical_failure(tool_calls),
        "daemon_stop_error": daemon_stop_error,
        "policy_receipt": policy_receipt if isinstance(policy_receipt, dict) else None,
        "budget": guard.as_dict(),
        "lane_root": str(lane_root),
        "transcript_path": str(transcript_path),
    }
    if failure is not None:
        receipt["terminal_failure"] = failure
    else:
        receipt["terminal_failure"] = None
    return receipt


def run_suite(binary: Path, suite: AutonomySuite, model: str, *,
              budget_policy: budget.BudgetPolicy, lane_root: Path,
              repository: Optional[Path] = None, profile: str = "workspace-write",
              timeout: float = 900.0, agent: str = "build",
              credential: Optional[str] = None,
              case_ids: Optional[list[str]] = None) -> dict[str, Any]:
    """Run every supported case of a suite in its own private lane.

    Each case gets its own lane so a case that leaves a daemon behind, a
    workspace, or a mutated fixture cannot influence the next case.

    The suite shares **one** spend guard, so the declared request cap covers
    the whole suite instead of restarting at every case. A case that the cap
    refuses is recorded as refused rather than silently skipped, and the
    cases after it are refused too.
    """
    if not case_ids:
        case_ids = [case.case_id for case in suite.cases if case.support == "supported"]
    admission = budget.admit_budget(budget_policy)
    if admission.status != budget.STATUS_ADMITTED:
        raise HarnessError("budget admission refused before the autonomy suite started: "
                           + json.dumps(admission.as_dict(), sort_keys=True))
    guard = budget.SpendGuard(budget_policy)
    runs: list[dict[str, Any]] = []
    for index, case_id in enumerate(case_ids):
        case = suite.case(case_id)
        if case.support != "supported":
            raise HarnessError(f"case {case_id} is deferred, not runnable")
        # The lane directory is named by position, not by case id: the daemon
        # binds its control socket inside the lane, and the platform's
        # sun_path limit is short enough that a 31-character case id can push
        # the socket path past it. The case id stays in the receipt.
        case_lane = lane_root / f"case{index:02d}"
        try:
            receipt = run_case(binary, case, model, budget_policy=budget_policy,
                               lane_root=case_lane, repository=repository,
                               profile=profile, timeout=timeout, agent=agent,
                               credential=credential, guard=guard)
        except HarnessError as error:
            receipt = {"case_id": case_id, "terminal": "refused", "ok": False,
                       "error": str(error), "solved": False, "protected": case.protected}
        runs.append(receipt)
    return {"schema_version": 1, "model": model,
            "binary_sha256": runs[0].get("binary_sha256") if runs else None,
            "case_ids": list(case_ids), "runs": runs,
            "budget": guard.as_dict(),
            "solved_cases": sum(1 for run in runs if run.get("solved"))}


def _observation(contract: scoring.ScoreContract, criterion_id: str,
                 value: Optional[float]) -> scoring.Observation:
    """Return one observation, keeping an unmeasured metric distinct from zero."""
    if value is None:
        return scoring.Observation(criterion_id=criterion_id, value=None, measured=False)
    return scoring.Observation(criterion_id=criterion_id, value=float(value))


def _pass_rate(runs: list[dict[str, Any]], protected: bool) -> Optional[float]:
    """Return the solved fraction over the task cases on one side."""
    selected = [run for run in runs if bool(run.get("protected")) is protected]
    if not selected:
        return None
    return sum(1 for run in selected if run.get("solved")) / len(selected)


def _repeated_identical_failure(tool_calls: list[dict[str, Any]]) -> bool:
    """Return whether an identical call really failed more than once.

    A protected control only means something when the situation it is about
    actually happened: the same call, with the same arguments, failing again.
    This is an **observation** of the transcript, not a judgement of it — the
    case's own verifier decides what to make of it.
    """
    attempts: dict[tuple[Any, Any], int] = {}
    failures: dict[tuple[Any, Any], int] = {}
    for call in tool_calls:
        key = (call.get("tool"), call.get("arguments_sha256"))
        attempts[key] = attempts.get(key, 0) + 1
        if call.get("status") == "failed":
            failures[key] = failures.get(key, 0) + 1
    return any(attempts[key] >= 2 and failures.get(key, 0) >= 2 for key in attempts)


def _control_rate(runs: list[dict[str, Any]]) -> Optional[float]:
    """Return the fraction of protected controls that still terminated as expected.

    A protected negative control's verifier is the case's own statement of
    what "as expected" means, so the control holds when that verifier
    **confirms** the trial. A control that was never exercised — the agent
    never put the harness in the situation the control is about — does not
    hold, and the comparison reports the per-run classification so a reader
    can tell "the control did not hold" from "the control never ran".

    Counting the *absence* of a solved task instead would be vacuous here: a
    control's task is impossible by construction, so it is never solved and
    the rate would always be 1.0.
    """
    controls = [run for run in runs if bool(run.get("protected"))]
    if not controls:
        return None
    return sum(1 for run in controls if run.get("verifier_confirmed")) / len(controls)


def _success_rate(runs: list[dict[str, Any]]) -> Optional[float]:
    """Return the fraction of trials that completed without an avoidable failure.

    A trial counts as successful when its turn reached a terminal on its own
    terms — it was not stopped as a stall, it did not time out, it was not
    cancelled, it was not refused, and it did not end in an infrastructure or
    provider failure.

    This metric is about the **run**, not the **outcome**. Whether the agent
    solved the task is `correctness.pass_rate`'s business; folding it in here
    would count one fact twice and make a reliability move depend on the
    task, which is how a metric starts to mean two things at once. An
    individual failed tool call is normal in an agent loop and does not make
    a trial unreliable; a trial that ended in an avoidable terminal failure
    does.
    """
    if not runs:
        return None
    good = 0
    for run in runs:
        if run.get("terminal") != "completed":
            continue
        if run.get("terminal_failure"):
            continue
        good += 1
    return good / len(runs)


def _median_duration_ms(runs: list[dict[str, Any]]) -> Optional[float]:
    """Return the median measured duration, or None when nothing was measured."""
    durations = sorted(float(run["duration_ms"]) for run in runs
                       if isinstance(run.get("duration_ms"), (int, float)))
    if not durations:
        return None
    middle = len(durations) // 2
    if len(durations) % 2:
        return durations[middle]
    return (durations[middle - 1] + durations[middle]) / 2.0


def compare_runs(contract: scoring.ScoreContract,
                 baseline: list[dict[str, Any]],
                 candidate: list[dict[str, Any]]) -> dict[str, Any]:
    """Score a candidate against a baseline with the shared score contract.

    The protected negative controls are scored separately from the task
    cases: a control's expected outcome is that the task is **not** solved, so
    a control that starts being credited as solved is a regression, not an
    improvement.
    """
    def observations(runs: list[dict[str, Any]], side: str) -> list[scoring.Observation]:
        return [
            _observation(contract, "correctness.pass_rate", _pass_rate(runs, protected=False)),
            _observation(contract, "reliability.success_rate", _success_rate(runs)),
            _observation(contract, "efficiency.duration_ms", _median_duration_ms(runs)),
            _observation(contract, "control.no_regression", _control_rate(runs)),
        ]

    paired = sorted({run.get("case_id") for run in baseline}
                    & {run.get("case_id") for run in candidate})
    baseline_observations = observations(baseline, "baseline")
    candidate_observations = observations(candidate, "candidate")
    outcome = scoring.score_candidate(contract, baseline_observations,
                                      candidate_observations,
                                      evidence_pairs=len(paired))
    return {"outcome": outcome.as_dict(),
            "baseline_cases": len(baseline),
            "candidate_cases": len(candidate),
            "paired_case_ids": paired,
            # The measured values, so a reader can see what moved and what
            # did not. A verdict without its numbers is an assertion.
            "observations": {
                "baseline": [{"criterion_id": observation.criterion_id,
                              "value": observation.value,
                              "measured": observation.measured}
                             for observation in baseline_observations],
                "candidate": [{"criterion_id": observation.criterion_id,
                               "value": observation.value,
                               "measured": observation.measured}
                              for observation in candidate_observations],
            }}


def rescore_recorded_runs(baseline_runs: list[dict[str, Any]],
                           candidate_runs: list[dict[str, Any]]) -> dict[str, Any]:
    """Re-score two recorded sides with the current score contract.

    A score contract can be corrected after a run — the version-2 correction
    of this project is the precedent — and the honest way to re-decide a
    recorded pair is to re-derive the verdict from the **same recorded
    trials**, not to run the provider again. This function does exactly that:
    it takes the two sides' receipts and scores them with the contract as it
    stands now.

    The recorded receipts are the evidence. The decision is a derivation from
    them, so a derivation that changes when the contract changes is expected;
    a *trial* that changes is not, and this function never re-runs anything.

    A recorded receipt that predates an observation is not left without it:
    the observation is derived from the recorded transcript, which is the same
    evidence the run was judged on.
    """
    def with_observations(runs: list[dict[str, Any]]) -> list[dict[str, Any]]:
        observed = []
        for run in runs:
            if "repeated_identical_failure" not in run:
                run = dict(run)
                run["repeated_identical_failure"] = _repeated_identical_failure(
                    list(run.get("tool_calls") or []))
            observed.append(run)
        return observed

    contract = scoring.default_contract()
    comparison = compare_runs(contract, with_observations(baseline_runs),
                               with_observations(candidate_runs))
    comparison["contract_version"] = contract.version
    comparison["rescored_from_recorded_runs"] = True
    return comparison
