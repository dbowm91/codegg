"""Sustained live trial scenarios with bounded, independent resource limits.

This module is the C004 work package C instrument: it runs an unattended
sequence of live trials against one approved route, each with its own
wallclock limit, and it refuses to start a trial the admitted budget cannot
cover. Every trial is an independent lane, so a cancellation, an abrupt ACP
drop, or an over-budget refusal in one lane never cancels another.

Fail-closed contract
--------------------
* The scenario admits the budget **once**, before any provider request, and
  refuses to start when the admission does not hold.
* A trial that would cross the scenario's provider-request ceiling is not
  started; the scenario stops and records why.
* A stopped trial is never reported as a solved task. The verifier result and
  the terminal classification stay separate.
* Cleanup is *observed*, not asserted: after every trial the lane's daemon
  socket and any surviving process for that lane are recorded.
"""
from __future__ import annotations

import json
import math
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional

from . import budget, pilot, supervisor
from .supervisor import HarnessError

#: The interruption kinds a sustained scenario may inject.
FAULTS = (None, "cancel", "abrupt_acp_drop")

__all__ = ["FAULTS", "TrialPlan", "lane_cleanup_evidence", "run_sustained_scenario"]


@dataclass(frozen=True)
class TrialPlan:
    """One planned trial inside a sustained scenario.

    ``task`` is a frozen pilot task id. ``fault`` is the interruption to
    inject once the agent starts acting; ``None`` runs the task to completion.
    """
    trial_id: str
    task: str
    fault: Optional[str] = None
    max_tool_calls: Optional[int] = None


def lane_cleanup_evidence(lane_root: Path) -> dict[str, Any]:
    """Observe whether the lane left anything running behind it.

    The evidence is what the harness can actually see: the daemon's socket
    file, and any live process whose command line still names the lane. A
    lane that left a daemon behind is reported, not hidden.
    """
    socket_path = lane_root / "daemon" / "core.sock"
    socket_present = socket_path.exists()
    survivors: list[str] = []
    try:
        listing = subprocess.run(["ps", "-eo", "pid=,command="], capture_output=True,
                                 text=True, timeout=20, check=False)
        marker = str(lane_root)
        for line in listing.stdout.splitlines():
            stripped = line.strip()
            if not stripped or marker not in stripped:
                continue
            if "ps -eo" in stripped:
                continue
            survivors.append(stripped)
    except (OSError, subprocess.SubprocessError):
        survivors = []
    return {"daemon_socket_present": socket_present,
            "surviving_processes": survivors,
            "clean": not socket_present and not survivors}


def _one_sided_95_upper_bound(events: int, trials: int) -> Optional[float]:
    """Exact one-sided Clopper-Pearson upper bound for an observed event rate."""
    if trials <= 0 or events < 0 or events > trials:
        return None
    if events == trials:
        return 1.0

    def cumulative_probability(probability: float) -> float:
        return sum(
            math.comb(trials, count)
            * probability**count
            * (1.0 - probability) ** (trials - count)
            for count in range(events + 1)
        )

    low, high = 0.0, 1.0
    for _ in range(80):
        middle = (low + high) / 2
        if cumulative_probability(middle) > 0.05:
            low = middle
        else:
            high = middle
    return high


def run_sustained_scenario(binary: Path, model: str, *, budget_policy: budget.BudgetPolicy,
                           plans: list[TrialPlan], profile: str = "workspace-write",
                           root: Optional[Path] = None, repository: Optional[Path] = None,
                           per_trial_wallclock: float = 900.0,
                           scenario_wallclock: float | None = None,
                           max_provider_requests: Optional[int] = None,
                           credential: Optional[str] = None) -> dict[str, Any]:
    """Run an unattended sequence of live trials and return one scenario receipt.

    ``max_provider_requests`` bounds the whole scenario independently of the
    admitted policy. The lower of the two ceilings wins, so a generous policy
    cannot silently widen the scenario's own bound.
    """
    if not plans:
        raise HarnessError("a sustained scenario needs at least one trial plan")
    for plan in plans:
        if plan.fault not in FAULTS:
            raise HarnessError(f"unknown fault: {plan.fault}")
        if plan.task not in pilot.FROZEN_TASKS:
            raise HarnessError(f"unknown frozen task: {plan.task}")

    admission = budget.admit_budget(budget_policy)
    if admission.status != budget.STATUS_ADMITTED:
        raise HarnessError("budget admission refused before the sustained scenario started: "
                           + json.dumps(admission.as_dict(), sort_keys=True))

    policy_request_ceiling = budget_policy.max_provider_requests
    scenario_ceiling = max_provider_requests
    if policy_request_ceiling is not None and scenario_ceiling is not None:
        ceiling = min(policy_request_ceiling, scenario_ceiling)
    else:
        ceiling = policy_request_ceiling if scenario_ceiling is None else scenario_ceiling

    guard = budget.SpendGuard(budget_policy)
    lane_root = (root or supervisor.lane_root("sus")).resolve()
    supervisor.private_dir(lane_root)

    started = time.monotonic()
    trials: list[dict[str, Any]] = []
    stopped_reason: Optional[str] = None
    attempted = 0
    unattempted = 0

    for index, plan in enumerate(plans):
        if ceiling is not None and attempted >= ceiling:
            stopped_reason = ("provider-request ceiling reached before this trial; "
                              f"attempted={attempted} ceiling={ceiling}")
            unattempted += len(plans) - index
            break
        try:
            guard.admit(projected_provider_requests=1)
        except budget.BudgetExhausted as error:
            stopped_reason = f"budget refused the next provider request: {error}"
            unattempted += len(plans) - index
            break

        # The lane directory name stays short: the daemon binds its control
        # socket inside it and the platform's ``sun_path`` limit is 104 bytes.
        trial_root = lane_root / f"t{index:02d}"
        trial_started = time.monotonic()
        remaining_cumulative = None
        if scenario_wallclock is not None:
            remaining_cumulative = scenario_wallclock - (time.monotonic() - started)
            if remaining_cumulative <= 0:
                stopped_reason = ("the scenario's cumulative wallclock budget is "
                                  f"exhausted: limit={scenario_wallclock}s")
                unattempted += len(plans) - index
                break
        trial_timeout = per_trial_wallclock
        if remaining_cumulative is not None:
            trial_timeout = min(trial_timeout, remaining_cumulative)
        try:
            receipt = pilot.run_live_task(
                binary, plan.task, model, budget_policy=budget_policy, profile=profile,
                root=trial_root, repository=repository, timeout=trial_timeout,
                credential=credential, fault=plan.fault)
        except HarnessError as error:
            receipt = {"trial_id": plan.trial_id, "task_id": plan.task, "fault": plan.fault,
                       "terminal": "refused", "error": str(error), "ok": False}
        attempted += 1
        usage = receipt.get("provider_usage") or {}
        guard.observe(provider_requests=1,
                      input_tokens=int(usage.get("input_tokens") or 0),
                      output_tokens=int(usage.get("output_tokens") or 0))
        cleanup = lane_cleanup_evidence(trial_root)
        receipt["attempt_id"] = f"{plan.trial_id}:0"
        receipt.setdefault("agent_terminal", receipt.get("terminal"))
        receipt.setdefault("event_observation_state", "not_instrumented")
        receipt.setdefault("event_gap_count", 0)
        receipt.setdefault("measurement_failures", 0)
        receipt.setdefault(
            "measurement_retry_state",
            "unresolved" if receipt.get("event_gap_count") else "not_needed",
        )
        if receipt.get("event_gap_count"):
            receipt["terminal"] = "infrastructure_error"
            receipt["ok"] = False
        receipt["trial_index"] = index
        receipt["trial_wallclock_ms"] = int((time.monotonic() - trial_started) * 1000)
        receipt["cleanup"] = cleanup
        receipt["provider_request_budget_after"] = guard.as_dict()
        trials.append(receipt)

    elapsed_ms = int((time.monotonic() - started) * 1000)
    instrumented = [
        trial for trial in trials
        if trial.get("event_observation_state") in ("observed_complete", "unmeasured_gap")
        and trial.get("agent_terminal") == "completed"
    ]
    gap_trials = sum(1 for trial in instrumented if trial.get("event_gap_count", 0) > 0)
    gap_upper = _one_sided_95_upper_bound(gap_trials, len(instrumented))
    return {
        "schema_version": 1,
        "model": model,
        "profile": profile,
        "policy": admission.as_dict(),
        "scenario_request_ceiling": ceiling,
        "scenario_wallclock_seconds": scenario_wallclock,
        "cumulative_wallclock_ms": elapsed_ms,
        "attempted_trials": attempted,
        "unattempted_trials": unattempted,
        "stopped_reason": stopped_reason,
        "elapsed_ms": elapsed_ms,
        "trials": trials,
        "measurement_failures": sum(
            int(trial.get("measurement_failures", 0)) for trial in trials
        ),
        "event_gap_measurement": {
            "unit": "instrumented_completed_trial",
            "instrumented_trials": len(instrumented),
            "gap_trials": gap_trials,
            "observed_rate": gap_trials / len(instrumented) if instrumented else None,
            "one_sided_95_upper_bound": gap_upper,
            "qualification_state": (
                "qualified_at_or_below_5_percent"
                if gap_upper is not None and gap_upper <= 0.05
                else "insufficient_evidence"
            ),
            "independence_caveat": "Trials sharing a daemon, route, or host may not be independent.",
        },
        "solved_trials": sum(1 for trial in trials if trial.get("ok")),
        "stopped_trials": sum(1 for trial in trials
                              if trial.get("terminal") in ("cancelled", "abrupt_acp_drop",
                                                          "timeout", "failed", "refused")),
        "budget": guard.as_dict(),
        "lane_root": str(lane_root),
        "cleanup_complete": all(trial.get("cleanup", {}).get("clean", False)
                                 for trial in trials),
    }
