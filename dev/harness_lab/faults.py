"""Fault injection seam for the autonomy taxonomy.

A case may declare a *fault*. A fault is a property of the lane's
**environment**, never of the prompt: the harness applies it to the lane's own
copy of the fixture before the prompt is submitted, so the agent meets a
deterministic failure at the tool boundary while the instruction never names
it. That is the whole point. A control that taught the model to loop would
measure the model, not the recovery ladder.

The seam is deliberately narrow, and every rule below is a refusal rather
than a repair:

* Only the tools whose failure the environment can reproduce deterministically
  are accepted. The seam is the filesystem, so that is ``read``. A fault the
  environment cannot produce would leave the control unexercised and would
  read as a pass, so an unsupported tool is refused.
* A declared fault is applied or the trial is refused. It is never ignored,
  never softened, and never applied to a case that did not declare it.
* The fault lives inside the lane's private workspace and nowhere else, and it
  is restored when the trial ends.
* The bound is the number of consecutive identical failures the case declares.
  When the bound is reached the runner stops the trial, so a faulted case is
  bounded even when the agent never gives up.

Trust boundary
--------------
The module touches the filesystem only, inside a workspace the caller names.
It never starts a process, never opens a socket, and never reads a credential.
"""
from __future__ import annotations

import hashlib
import json
import os
import shutil
import stat
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping, Optional

#: The tools whose failure the environment boundary can reproduce
#: deterministically. The seam is the filesystem; a tool the filesystem cannot
#: fail on demand has no seam here and is refused.
FAULT_TOOLS = ("read",)

#: The fault modes the seam can produce.
#:
#: ``unreadable``
#:     The target keeps its path and its content but loses its read permission,
#:     so the tool fails on an error that a retry could plausibly fix.
#: ``missing``
#:     The target is moved out of the workspace, so the tool fails with a
#:     "not found" error.
FAULT_MODES = ("unreadable", "missing")

#: The smallest number of consecutive identical failures that can exercise a
#: repeated-failure control. One failure is an ordinary error; a control needs
#: the repeat.
MIN_CONSECUTIVE_FAILURES = 2

#: The keys a fault declaration may carry.
FAULT_KEYS = frozenset({
    "tool", "path", "mode", "min_consecutive_failures", "max_consecutive_failures",
})

#: The status an observed tool call carries when it failed. A call that started
#: and never finished is ``unknown``: it is neither a success nor a failure and
#: never counts towards a repeated failure.
STATUS_FAILED = "failed"

#: The envelopes the product puts around a tool's own error text.
#:
#: A tool that fails does **not** report a failed call on the ACP wire: the
#: broker wraps the tool's error in an *error-marked value* and the call is
#: published as a successful call whose output is the error text. The ACP
#: ``tool_call_update`` therefore carries ``status: "completed"`` for a call
#: that in fact failed, and the ACP status alone cannot tell a failed tool
#: from a successful one.
#:
#: These are the product's error envelopes, taken from
#: ``src/tool/broker.rs`` (the ``ToolValue::infrastructure_error`` arms) and
#: ``src/tool/contract.rs`` (the ``ToolValue::denied`` / timeout arms). The
#: harness reads them out of the reported output so that a faulted tool is
#: observable at all.
#:
#: The coupling is guarded, not silent: ``tests/test_autonomy_faults.py``
#: asserts that every marker here is still the product's marker, so a change
#: to the product's error envelope fails the harness suite instead of quietly
#: leaving the control unexercised.
TOOL_ERROR_ENVELOPES = (
    "Tool error: ",
    "Tool not found: ",
    "Tool is disabled: ",
    "Execution backend unavailable: ",
)


def looks_like_tool_error(raw_output: str) -> bool:
    """Return whether a reported tool output carries the product's error envelope.

    The envelope is only recognised at the **start of the reported output**,
    which is where the product puts it. A tool that succeeded and whose own
    content happens to contain the same words later in its output is not a
    failure.

    The argument is the raw ``rawOutput`` the ACP notification carries: the
    product reports the tool's output as a debug rendering, so the envelope
    follows a quote rather than starting the string.
    """
    for envelope in TOOL_ERROR_ENVELOPES:
        if ('"' + envelope) in raw_output:
            return True
    return False


class FaultError(ValueError):
    """Raised when a fault declaration cannot be honoured.

    The error is a refusal, never a repair. A case that declares a fault the
    seam cannot produce must not run as if it had none.
    """


@dataclass(frozen=True)
class FaultSpec:
    """One declared fault.

    ``min_consecutive_failures`` is what the control needs to observe for the
    fault to have been exercised. ``max_consecutive_failures`` is the bound:
    when that many consecutive identical failures have been observed, the
    runner stops the trial.
    """

    tool: str
    path: str
    mode: str
    min_consecutive_failures: int
    max_consecutive_failures: int

    @staticmethod
    def parse(value: Any) -> "FaultSpec":
        """Return the validated fault declared by ``value``.

        The declaration is rejected, never repaired: an unknown key, a missing
        key, a tool the seam cannot fail, a path that leaves the workspace, a
        mode the seam cannot produce or a bound that is not a positive integer
        are all refusals.
        """
        if not isinstance(value, Mapping):
            raise FaultError("a fault declaration must be a mapping")
        unknown = sorted(str(key) for key in set(value) - FAULT_KEYS)
        if unknown:
            raise FaultError(f"a fault declaration has unknown field(s): {', '.join(unknown)}")
        missing = sorted(key for key in FAULT_KEYS if key not in value)
        if missing:
            raise FaultError(f"a fault declaration is missing required field(s): {', '.join(missing)}")

        tool = value["tool"]
        if not isinstance(tool, str) or tool not in FAULT_TOOLS:
            raise FaultError(
                f"a fault may only be declared for {', '.join(FAULT_TOOLS)}: the seam "
                f"is the environment boundary, and {tool!r} cannot be failed "
                "deterministically from it")
        mode = value["mode"]
        if not isinstance(mode, str) or mode not in FAULT_MODES:
            raise FaultError(
                f"a fault mode must be one of {', '.join(FAULT_MODES)}: {mode!r}")
        path = value["path"]
        if not isinstance(path, str) or not path.strip():
            raise FaultError("a fault path must be a non-empty string")
        if os.path.isabs(path) or path.startswith("~"):
            raise FaultError("a fault path must be relative to the case workspace")
        minimum = _require_positive_int(value["min_consecutive_failures"],
                                        "min_consecutive_failures")
        maximum = _require_positive_int(value["max_consecutive_failures"],
                                        "max_consecutive_failures")
        if minimum < MIN_CONSECUTIVE_FAILURES:
            raise FaultError(
                f"a fault needs at least {MIN_CONSECUTIVE_FAILURES} consecutive "
                "identical failures to exercise a repeated-failure control")
        if maximum < minimum:
            raise FaultError(
                "a fault's max_consecutive_failures must not be below its "
                "min_consecutive_failures")
        return FaultSpec(tool=tool, path=path, mode=mode,
                         min_consecutive_failures=minimum,
                         max_consecutive_failures=maximum)

    def as_dict(self) -> dict[str, Any]:
        """Return a JSON-serializable view of the declaration."""
        return {"tool": self.tool, "path": self.path, "mode": self.mode,
                "min_consecutive_failures": self.min_consecutive_failures,
                "max_consecutive_failures": self.max_consecutive_failures}


@dataclass
class AppliedFault:
    """A fault that has been applied to a lane workspace.

    ``restore`` puts the workspace back the way it was. It is safe to call
    more than once, and it never raises: a fault that cannot be restored is
    reported in the receipt rather than turning a trial into a crash.
    """

    spec: FaultSpec
    workspace: Path
    target: Path
    original_mode: Optional[int] = None
    stash: Optional[Path] = None
    restored: bool = False

    def restore(self) -> Optional[str]:
        """Undo the fault and return an error description, or None on success."""
        if self.restored:
            return None
        self.restored = True
        try:
            if self.spec.mode == "unreadable":
                if self.original_mode is not None and self.target.exists():
                    os.chmod(self.target, stat.S_IMODE(self.original_mode))
            elif self.spec.mode == "missing":
                if self.stash is not None and self.stash.exists():
                    if self.target.exists():
                        shutil.rmtree(self.target) if self.target.is_dir() else self.target.unlink()
                    self.target.parent.mkdir(parents=True, exist_ok=True)
                    shutil.move(str(self.stash), str(self.target))
        except OSError as error:
            return f"{type(error).__name__}: {error}"
        return None

    def as_dict(self) -> dict[str, Any]:
        """Return a JSON-serializable receipt of the applied fault."""
        return {"tool": self.spec.tool, "path": self.spec.path, "mode": self.spec.mode,
                "target": str(self.target),
                "applied_mode": 0 if self.spec.mode == "unreadable" else None,
                "min_consecutive_failures": self.spec.min_consecutive_failures,
                "max_consecutive_failures": self.spec.max_consecutive_failures}

    def reapply(self) -> bool:
        """Put the fault back in force and report whether it had been undone.

        An agent can undo a filesystem fault — a permission fault with a
        ``chmod``, a missing file with a write. The fault is a property of the
        lane for the whole trial, so the harness restores it while the trial
        runs. This returns True when the fault had been undone and was put
        back.
        """
        if self.spec.mode == "unreadable":
            if not self.target.exists():
                return False
            mode = stat.S_IMODE(self.target.stat().st_mode)
            if mode & 0o444:
                os.chmod(self.target, 0o000)
                return True
            return False
        if self.spec.mode == "missing":
            if self.target.exists():
                if self.stash is not None and self.stash.exists():
                    if self.target.is_dir():
                        shutil.rmtree(self.target)
                    else:
                        self.target.unlink()
                    shutil.move(str(self.stash), str(self.target))
                    return True
            return False
        return False


def _require_positive_int(value: Any, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise FaultError(f"a fault's {label} must be an integer")
    if value < 1:
        raise FaultError(f"a fault's {label} must be a positive integer")
    return value


def apply_fault(spec: FaultSpec, workspace: Path, stash_root: Path) -> AppliedFault:
    """Apply ``spec`` inside ``workspace`` and return the applied fault.

    The workspace is the lane's private copy of the case's fixture. The
    ``missing`` mode moves the target into ``stash_root``, which lives outside
    the workspace so the agent cannot see the stashed copy.
    """
    workspace = Path(workspace).resolve()
    if not workspace.is_dir():
        raise FaultError(f"the fault workspace is not a directory: {workspace}")
    target = (workspace / spec.path).resolve()
    if target != workspace and workspace not in target.parents:
        raise FaultError(f"a fault path may not leave the case workspace: {spec.path!r}")
    if not target.exists():
        raise FaultError(f"the fault target does not exist: {spec.path!r}")

    applied = AppliedFault(spec=spec, workspace=workspace, target=target)
    if spec.mode == "unreadable":
        applied.original_mode = stat.S_IMODE(target.stat().st_mode)
        os.chmod(target, 0o000)
        return applied

    stash_root = Path(stash_root).resolve()
    stash_root.mkdir(parents=True, exist_ok=True)
    stash = stash_root / target.name
    if stash.exists():
        raise FaultError(f"the fault stash already holds {target.name!r}")
    shutil.move(str(target), str(stash))
    applied.stash = stash
    return applied


def arguments_digest(arguments: Mapping[str, Any]) -> str:
    """Return the digest of a tool's arguments, in the harness's own encoding.

    The harness records a digest rather than the payload, so a verifier can
    tell "the same call again" from "a different call" without reading what the
    call carried. The encoding is the compact, key-sorted JSON form, so the
    same arguments always produce the same digest.
    """
    encoded = json.dumps(arguments, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(encoded.encode("utf-8")).hexdigest()


def _call_failed(call: Mapping[str, Any]) -> bool:
    """Return whether an observed tool call failed.

    A call fails when the ACP wire said so, and also when the product's own
    error envelope shows in the reported output: the product publishes a
    failed tool as a *successful* call carrying an error-marked value, so the
    ACP status alone would report the faulted tool as a success and the
    control would never fire.
    """
    failed = call.get("failed")
    if isinstance(failed, bool):
        return failed
    return call.get("status") == STATUS_FAILED


def observe_fault(tool_calls: list[Mapping[str, Any]], spec: FaultSpec) -> dict[str, Any]:
    """Return what the transcript shows about the fault.

    The observation is deliberately separate from any judgement: it reports the
    number of times the faulted tool really failed, the longest run of
    consecutive identical failures, and whether the declared bound was reached.
    The case's own verifier decides what to make of it.
    """
    attempts: dict[tuple[Any, Any], int] = {}
    failures: dict[tuple[Any, Any], int] = {}
    longest_run = 0
    run = 0
    previous: Optional[tuple[Any, Any]] = None
    for call in tool_calls:
        key = (call.get("tool"), call.get("arguments_sha256"))
        attempts[key] = attempts.get(key, 0) + 1
        if _call_failed(call):
            failures[key] = failures.get(key, 0) + 1
            if key == previous:
                run += 1
            else:
                run = 1
                previous = key
            longest_run = max(longest_run, run)
        else:
            # Any call that did not fail ends the run of identical failures,
            # including a *successful* call of the same tool with the same
            # arguments: a success between two failures is not a repeat.
            previous = None
            run = 0
    repeated = [
        {"tool": key[0], "arguments_sha256": key[1], "attempts": count,
         "failures": failures.get(key, 0)}
        for key, count in attempts.items() if count >= 2
    ]
    repeated.sort(key=lambda entry: (-entry["failures"], -entry["attempts"]))
    # The number of failures of the *faulted tool*, across every call to it.
    # It is deliberately not read off the repeated calls only: an agent that
    # fails once and then investigates differently produced one observed
    # failure, and a receipt that reported zero would say the fault was never
    # met. The repeat is reported separately, as the longest consecutive run.
    observed = sum(count for key, count in failures.items() if key[0] == spec.tool)
    return {
        "observed_failures": observed,
        "longest_consecutive_run": longest_run,
        # The bound the fault watcher enforces, on the *total* failures of the
        # faulted call: the watcher's job is to bound the trial, and an agent
        # that failed the same call three times with other calls in between has
        # still failed it three times.
        "bound_reached": observed >= spec.max_consecutive_failures,
        # The measurement, on the *consecutive* run: a repeat is a repeat only
        # when the identical failing call followed itself without a success in
        # between. A control the agent never reached is not exercised, and an
        # unexercised control is never a pass.
        "exercised": longest_run >= spec.min_consecutive_failures,
        "repeated_calls": repeated,
    }


__all__ = [
    "FAULT_MODES",
    "FAULT_TOOLS",
    "MIN_CONSECUTIVE_FAILURES",
    "AppliedFault",
    "FaultError",
    "FaultSpec",
    "apply_fault",
    "arguments_digest",
    "observe_fault",
]
