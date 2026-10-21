"""Bounded candidate-proposal primitives; no production or automatic Git mutation.

Trust boundary
--------------
The proposer runs in a disposable candidate worktree and its output is
evidence, never an accepted change. Two things narrow it further:

* the proposer may be pinned to one approved ``provider/model`` route, and
  the resolved route is recorded in the receipt, so a proposal is always
  attributed to the route that produced it;
* an optional budget policy is admitted *before* any provider work and then
  enforced with a spend guard, so a live proposal cannot exceed the caps the
  admission gate admitted.

Fail-closed contract
--------------------
* A malformed model id is refused, never normalized or "fixed up".
* A budget policy that cannot be enforced is a refusal, never a downgrade.
* A usage figure the CLI did not report is unknown, never zero.
"""
from __future__ import annotations

from dataclasses import dataclass
import json
import hashlib
import os
from pathlib import Path
import re
import shutil
import signal
import statistics
import subprocess
import threading
import time
from typing import Any

from . import budget


STATES = {"BASELINED", "DIAGNOSED", "PROPOSING", "PATCHED", "BUILDING", "QUALIFYING", "EVALUATING", "COMPARING", "ACCEPTED", "REJECTED", "BLOCKED", "INTERRUPTED_UNKNOWN"}
#: The OpenCode CLI version whose `run --auto --format json` surface this
#: adapter supports. Verified against the installed 1.19.3 `opencode run
#: --help`: `run [message..]`, `--auto` (auto-approve permissions that are not
#: explicitly denied), `--format json` (raw JSON events), `--dir`, `--model`,
#: and `--agent` are all present with unchanged meanings. The JSON *event
#: payload* shape still needs one authenticated bounded invocation to confirm
#: against a live proposer; until then the pin only certifies the CLI surface.
SUPPORTED_OPENCODE_VERSION = "1.19.3"
TERMINAL = {"ACCEPTED", "REJECTED", "BLOCKED", "INTERRUPTED_UNKNOWN"}
_ALLOWED_TRANSITIONS = {
    "BASELINED": {"DIAGNOSED", "BLOCKED"}, "DIAGNOSED": {"PROPOSING", "BLOCKED"},
    "PROPOSING": {"PATCHED", "REJECTED", "BLOCKED"}, "PATCHED": {"BUILDING", "REJECTED", "BLOCKED"},
    "BUILDING": {"BUILDING", "QUALIFYING", "PROPOSING", "REJECTED", "BLOCKED"},
    "QUALIFYING": {"BUILDING", "EVALUATING", "REJECTED", "BLOCKED"},
    "EVALUATING": {"BUILDING", "COMPARING", "PROPOSING", "REJECTED", "BLOCKED"},
    "COMPARING": {"BUILDING", "ACCEPTED", "REJECTED", "BLOCKED"},
    "ACCEPTED": set(), "REJECTED": set(), "BLOCKED": set(),
}
# Every non-terminal state may record an interruption. A crash between a
# checkpoint's append-only event write and its current-state pointer
# replacement is reconciled to a recorded ``interrupted_unknown`` outcome, so
# the ambiguous attempt is preserved for manual inspection instead of being
# silently continued or resubmitted.
ALLOWED = {state: (set(targets) | {"INTERRUPTED_UNKNOWN"}) if state not in TERMINAL else set(targets)
           for state, targets in _ALLOWED_TRANSITIONS.items()}
ALLOWED["INTERRUPTED_UNKNOWN"] = set()
INTERRUPTED_UNKNOWN_OUTCOME = "interrupted_unknown"
OPENCODE_EVENT_TYPES = {"session_start", "session_end", "step_start", "step_finish", "text", "reasoning", "tool_use", "tool_result", "tool", "error", "permission"}
#: A proposer model id is a ``<provider>/<model>`` pair. The OpenCode CLI
#: accepts ``--model provider/model``; any other shape is refused rather than
#: repaired, because a receipt must name the exact route that was requested.
PROPOSER_MODEL_PATTERN = re.compile(r"\A[A-Za-z0-9._-]+/[A-Za-z0-9._-]+\Z")
#: The reason recorded when the CLI reported no usage at all. An absent
#: measurement is unknown, never zero.
USAGE_NOT_REPORTED = "not_reported_by_the_cli"


def validate_proposer_model(model: str | None) -> str | None:
    """Validate a proposer model id, or return None for an unpinned route.

    The id must be a ``<provider>/<model>`` pair whose halves are non-empty
    and match ``[A-Za-z0-9._-]+``. A malformed id raises :class:`ValueError`
    and is never normalized, trimmed or otherwise "fixed up": the id is the
    route, and a receipt must attribute a proposal to the exact route that
    produced it.
    """
    if model is None:
        return None
    if not isinstance(model, str) or not PROPOSER_MODEL_PATTERN.match(model):
        raise ValueError(
            "proposer model must be a <provider>/<model> id whose non-empty halves match "
            f"[A-Za-z0-9._-]+; got {model!r}")
    return model


class CheckpointInterrupted(ValueError):
    """Raised when a checkpoint's recorded tail and its current-state pointer disagree.

    The disagreement is reconciled to a recorded ``interrupted_unknown`` outcome
    *before* the error is raised, so the ambiguous attempt is preserved for
    manual inspection instead of being silently continued or resubmitted. The
    error is a refusal, not an opaque failure: it names the exact sequence
    numbers that were ambiguous.
    """


@dataclass
class Budget:
    max_iterations: int = 1
    max_candidate_retries: int = 0
    max_proposer_calls: int = 1
    max_wallclock_seconds: float = 600.0
    iterations: int = 0
    retries: int = 0
    proposer_calls: int = 0
    started: float = 0.0

    def __post_init__(self) -> None:
        if min(self.max_iterations, self.max_proposer_calls) < 1 or self.max_candidate_retries < 0 or self.max_wallclock_seconds <= 0:
            raise ValueError("budgets must be positive (retries may be zero)")
        self.started = time.monotonic()

    def consume(self, kind: str) -> None:
        if time.monotonic() - self.started >= self.max_wallclock_seconds:
            raise RuntimeError("wallclock budget exhausted")
        attr, ceiling = {"iteration": ("iterations", self.max_iterations), "retry": ("retries", self.max_candidate_retries), "proposer": ("proposer_calls", self.max_proposer_calls)}[kind]
        value = getattr(self, attr) + 1
        if value > ceiling:
            raise RuntimeError(f"{kind} budget exhausted")
        setattr(self, attr, value)


def _read_event_log(events_path: Path) -> tuple[list[dict[str, Any]], bool]:
    """Read an append-only event log, tolerating one torn trailing record.

    A process killed *inside* the append can leave a partially written final
    line. That record was never acknowledged by the pointer and is not a
    committed event, so it is reported as a torn tail and dropped instead of
    being parsed as corruption. A corrupt record anywhere else is corruption.
    """
    if not events_path.exists():
        return [], False
    lines = events_path.read_text(encoding="utf-8").splitlines()
    events: list[dict[str, Any]] = []
    torn_tail = False
    for index, line in enumerate(lines):
        if not line.strip():
            continue
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            if index == len(lines) - 1:
                torn_tail = True
            else:
                raise ValueError("checkpoint event log contains a corrupt record")
    return events, torn_tail


def _truncate_event_log(events_path: Path, events: list[dict[str, Any]]) -> None:
    """Rewrite an event log without its torn trailing record.

    Only the never-committed torn bytes are dropped; every complete event is
    rewritten byte-for-byte in the format ``_save`` uses, so the log stays
    append-only for every record that was ever acknowledged.
    """
    temporary = events_path.with_suffix(".recovered")
    temporary.write_text("".join(json.dumps(event, sort_keys=True) + "\n" for event in events),
                         encoding="utf-8")
    os.replace(temporary, events_path)


class Checkpoint:
    def __init__(self, path: Path, base_revision: str, suite_digest: str) -> None:
        self.path = path
        self.events_path = path.with_suffix(".jsonl")
        if self.path.exists() or self.events_path.exists():
            raise FileExistsError("candidate checkpoint already exists; use resume with matching immutable inputs")
        self.state = "BASELINED"
        self.base_revision = base_revision
        self.suite_digest = suite_digest
        self.attempt_id = 0
        self._save("BASELINED", {"base_revision": base_revision, "suite_digest": suite_digest})

    @classmethod
    def resume(cls, path: Path, base_revision: str, suite_digest: str) -> "Checkpoint":
        events_path = path.with_suffix(".jsonl")
        events, torn_tail = _read_event_log(events_path)
        if not events or any(event.get("seq") != index for index, event in enumerate(events, 1)):
            raise ValueError("checkpoint event sequence is missing or corrupt")
        if events[0].get("state") != "BASELINED":
            raise ValueError("checkpoint does not begin at BASELINED")
        for previous, current in zip(events, events[1:]):
            prior_state = previous.get("state")
            next_state = current.get("state")
            if prior_state not in STATES or next_state not in ALLOWED.get(prior_state, set()):
                raise ValueError("checkpoint contains an invalid state transition")
            expected_attempt = int(previous.get("attempt_id", 0)) + (next_state == "PROPOSING")
            if current.get("attempt_id") != expected_attempt:
                raise ValueError("checkpoint attempt sequence is invalid")
        if any(event.get("base_revision") != base_revision or event.get("suite_digest") != suite_digest
               for event in events):
            raise ValueError("checkpoint baseline or fixture suite changed; refusing resume")
        if not path.exists():
            raise ValueError("checkpoint pointer is missing")
        pointer = json.loads(path.read_text(encoding="utf-8"))
        if pointer != events[-1]:
            pointer_index = next((index for index, event in enumerate(events) if event == pointer), None)
            if pointer_index is None:
                raise ValueError("checkpoint pointer does not match the append-only event tail")
            raise cls._reconcile_interruption(path, events_path, events, pointer, pointer_index,
                                               torn_tail=torn_tail)
        last = events[-1]
        state = last.get("state")
        if state == "INTERRUPTED_UNKNOWN":
            raise CheckpointInterrupted(last.get("evidence", {}).get("reason")
                                        or "checkpoint was interrupted; its outcome is recorded as "
                                           f"{INTERRUPTED_UNKNOWN_OUTCOME}")
        if state not in STATES or state in TERMINAL:
            raise ValueError("checkpoint is terminal or has an unknown state")
        checkpoint = cls.__new__(cls)
        checkpoint.path, checkpoint.events_path = path, events_path
        checkpoint.state = state
        checkpoint.base_revision = base_revision
        checkpoint.suite_digest = suite_digest
        checkpoint.attempt_id = int(last.get("attempt_id", 0))
        return checkpoint

    @classmethod
    def _reconcile_interruption(cls, path: Path, events_path: Path, events: list[dict[str, Any]],
                                pointer: dict[str, Any], pointer_index: int,
                                torn_tail: bool) -> "CheckpointInterrupted":
        """Record the ambiguous interruption and refuse to continue.

        The tail advanced past the pointer, so the process died between the
        append-only event write and the pointer replacement: the interrupted
        attempt's outcome is unknown. The interruption is recorded as an
        ``interrupted_unknown`` outcome (the ledger's interrupted terminal) and
        the resume is refused, so no duplicate event and no duplicate trial can
        be produced by silently continuing.
        """
        unpointed = events[pointer_index + 1:]
        reason = ("checkpoint pointer is stale: the append-only event tail advanced to seq "
                  f"{events[-1].get('seq')} ({events[-1].get('state')}) while the pointer still names "
                  f"seq {pointer.get('seq')} ({pointer.get('state')}); the interrupted attempt's outcome "
                  f"is ambiguous and was recorded as {INTERRUPTED_UNKNOWN_OUTCOME}")
        checkpoint = cls.__new__(cls)
        checkpoint.path, checkpoint.events_path = path, events_path
        checkpoint.state = events[-1].get("state")
        checkpoint.base_revision = events[-1].get("base_revision")
        checkpoint.suite_digest = events[-1].get("suite_digest")
        checkpoint.attempt_id = int(events[-1].get("attempt_id", 0))
        if events[-1].get("state") == "INTERRUPTED_UNKNOWN":
            # A repeated resume is idempotent: the interruption is already
            # recorded, so no second event is appended for the same tail.
            raise CheckpointInterrupted(reason)
        if torn_tail:
            _truncate_event_log(events_path, events)
        checkpoint._save("INTERRUPTED_UNKNOWN",
                         {"outcome": INTERRUPTED_UNKNOWN_OUTCOME, "reason": reason,
                          "pointer_seq": pointer.get("seq"),
                          "unpointed_seq": [event.get("seq") for event in unpointed],
                          "unpointed_state": events[-1].get("state"),
                          "torn_tail": torn_tail, "source_mutated": False})
        raise CheckpointInterrupted(reason)

    def transition(self, state: str, evidence: dict[str, Any]) -> None:
        if state not in STATES or state not in ALLOWED[self.state]:
            raise RuntimeError(f"invalid candidate transition {self.state} -> {state}")
        self.state = state
        if state == "PROPOSING":
            self.attempt_id += 1
        self._save(state, evidence)

    def _save(self, state: str, evidence: dict[str, Any]) -> None:
        if self.events_path.exists():
            with self.events_path.open(encoding="utf-8") as prior:
                sequence = sum(1 for _ in prior) + 1
        else:
            sequence = 1
        event = {"schema_version": 1, "seq": sequence,
                 "state": state, "attempt_id": self.attempt_id, "base_revision": self.base_revision,
                 "suite_digest": self.suite_digest, "evidence": evidence}
        self.path.parent.mkdir(parents=True, exist_ok=True)
        with self.events_path.open("a", encoding="utf-8") as output:
            output.write(json.dumps(event, sort_keys=True) + "\n")
            output.flush()
            os.fsync(output.fileno())
        temporary = self.path.with_suffix(".tmp")
        temporary.write_text(json.dumps(event, sort_keys=True, indent=2) + "\n", encoding="utf-8")
        os.replace(temporary, self.path)


def _step_usage(part: dict[str, Any]) -> dict[str, Any]:
    """Return the usage figures one ``step_finish`` event reported.

    The OpenCode CLI reports each step's own usage on that step's
    ``step_finish`` event. A figure the CLI did not report stays ``None``:
    it is unknown, never zero.
    """
    tokens = part.get("tokens") if isinstance(part.get("tokens"), dict) else {}
    cache = tokens.get("cache") if isinstance(tokens.get("cache"), dict) else {}

    def figure(value: Any) -> int | float | None:
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            return None
        return value

    cost = part.get("cost")
    if isinstance(cost, bool) or not isinstance(cost, (int, float)):
        cost = None
    return {"total": figure(tokens.get("total")), "input": figure(tokens.get("input")),
            "output": figure(tokens.get("output")), "reasoning": figure(tokens.get("reasoning")),
            "cache_read": figure(cache.get("read")), "cache_write": figure(cache.get("write")),
            "cost_usd": cost}


def _aggregate_usage(steps: list[dict[str, Any]]) -> dict[str, Any] | None:
    """Sum the per-step usage the CLI reported into one invocation total.

    The CLI reports each step's own usage, so the invocation total is the
    sum over the steps. A figure that no step reported stays ``None``: it is
    unknown, not zero.

    The cost is the CLI's own reported figure. It is not a
    provider-authoritative cost, and it is never derived from the token
    counts.
    """
    if not steps:
        return None
    keys = ("total", "input", "output", "reasoning", "cache_read", "cache_write")
    totals = dict.fromkeys(keys, 0)
    observed = dict.fromkeys(keys, False)
    cost_usd = 0.0
    cost_observed = False
    for step in steps:
        for key in keys:
            value = step.get(key)
            if value is not None:
                totals[key] += value
                observed[key] = True
        if step.get("cost_usd") is not None:
            cost_usd += float(step["cost_usd"])
            cost_observed = True
    return {"tokens": {key: (totals[key] if observed[key] else None) for key in keys},
            "cost_usd": cost_usd if cost_observed else None,
            "steps": len(steps)}


def parse_opencode_events(stdout: str) -> dict[str, Any]:
    if len(stdout.encode()) > 4 * 1024 * 1024:
        raise ValueError("OpenCode event output exceeds 4 MiB")
    events = []
    for number, line in enumerate(stdout.splitlines(), 1):
        if not line.strip():
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(f"malformed OpenCode JSON event on line {number}") from error
        if not isinstance(event, dict) or event.get("type") not in OPENCODE_EVENT_TYPES:
            raise ValueError(f"unsupported OpenCode event on line {number}")
        if event["type"] in {"error", "permission"}:
            raise ValueError(f"OpenCode proposal emitted {event['type']} event")
        events.append(event)
    if not events:
        raise ValueError("OpenCode returned no structured events")
    usage = _aggregate_usage([_step_usage(event.get("part", {})) for event in events
                               if event["type"] == "step_finish"])
    return {"event_count": len(events), "types": [event["type"] for event in events],
            "text": "".join(str(event.get("part", {}).get("text", "")) for event in events if event["type"] == "text"),
            "usage": usage,
            "usage_reason": None if usage is not None else USAGE_NOT_REPORTED}


def run_opencode(worktree: Path, prompt: str, timeout: float = 300.0, executable: str | None = None,
                 model: str | None = None) -> dict[str, Any]:
    # The route is validated before anything else, so a malformed id is
    # refused whether or not a CLI is installed.
    model = validate_proposer_model(model)
    binary = executable or shutil.which("opencode")
    if not binary:
        raise FileNotFoundError("opencode executable is unavailable; manual proposer remains available")
    version = subprocess.run([binary, "--version"], cwd=worktree, capture_output=True, text=True, timeout=5)
    observed = version.stdout.strip() if version.stdout else ""
    if version.returncode != 0 or observed != SUPPORTED_OPENCODE_VERSION:
        # The version gate is a distinct, nameable refusal: it is not a
        # transport fault and it must not be reported as one.
        raise RuntimeError(
            f"OpenCode version preflight failed: expected {SUPPORTED_OPENCODE_VERSION}, "
            f"observed {observed or 'unknown'} from {binary}")
    env = os.environ.copy()
    env.pop("CODEGG_DAEMON_HOME", None)
    # An unpinned run keeps the CLI's own default route and emits no
    # ``--model`` flag at all; a pinned run names the approved route
    # explicitly, between ``--dir`` and the prompt.
    argv = [binary, "run", "--auto", "--format", "json", "--dir", str(worktree)]
    if model is not None:
        argv.extend(["--model", model])
    argv.append(prompt)
    process = subprocess.Popen(argv,
                               cwd=worktree, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               start_new_session=(os.name == "posix"))
    outputs: dict[str, bytearray] = {"stdout": bytearray(), "stderr": bytearray()}
    overflow: set[str] = set()

    def drain(name: str, stream: Any, limit: int) -> None:
        while chunk := stream.read(8192):
            remaining = limit - len(outputs[name])
            if remaining > 0:
                outputs[name].extend(chunk[:remaining])
            if len(chunk) > remaining:
                overflow.add(name)

    assert process.stdout is not None and process.stderr is not None
    readers = [threading.Thread(target=drain, args=("stdout", process.stdout, 4 * 1024 * 1024), daemon=True),
               threading.Thread(target=drain, args=("stderr", process.stderr, 64 * 1024), daemon=True)]
    for reader in readers:
        reader.start()
    try:
        process.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        if os.name == "posix":
            os.killpg(process.pid, signal.SIGTERM)
        else:
            process.terminate()
        try:
            process.wait(timeout=0.25)
        except subprocess.TimeoutExpired:
            if os.name == "posix":
                os.killpg(process.pid, signal.SIGKILL)
            process.kill()
            process.wait()
        for reader in readers:
            reader.join(timeout=2)
        process.stdout.close()
        process.stderr.close()
        raise TimeoutError("OpenCode proposer exceeded its wallclock budget")
    for reader in readers:
        reader.join(timeout=2)
    if any(reader.is_alive() for reader in readers):
        process.stdout.close()
        process.stderr.close()
        raise RuntimeError("OpenCode output pipes did not close after process exit")
    process.stdout.close()
    process.stderr.close()
    if overflow:
        raise ValueError("OpenCode output exceeded bounded capture limits")
    if process.returncode != 0:
        raise RuntimeError(f"OpenCode proposal failed with exit {process.returncode}; stderr bytes={len(outputs['stderr'])} (content redacted)")
    parsed = parse_opencode_events(outputs["stdout"].decode(errors="replace"))
    return {"version": version.stdout.strip(), "event_count": parsed["event_count"], "event_types": parsed["types"],
            "text_sha256": __import__("hashlib").sha256(parsed["text"].encode()).hexdigest(), "returncode": process.returncode,
            # The route the proposal is attributed to. ``None`` means the CLI's
            # own unpinned default was used.
            "model": model,
            "proposer_usage": parsed["usage"],
            "proposer_usage_reason": parsed["usage_reason"]}


def changed_paths(worktree: Path) -> list[str]:
    result = subprocess.run(["git", "-C", str(worktree), "status", "--porcelain", "-z"], capture_output=True, timeout=5)
    if result.returncode:
        raise RuntimeError("cannot inspect candidate worktree state")
    tokens = result.stdout.split(b"\0")
    paths: list[str] = []
    index = 0
    while index < len(tokens):
        record = tokens[index]
        index += 1
        if not record:
            continue
        status = record[:2].decode(errors="replace")
        paths.append(record[3:].decode(errors="replace"))
        if "R" in status or "C" in status:
            if index < len(tokens) and tokens[index]:
                paths.append(tokens[index].decode(errors="replace"))
                index += 1
    return sorted(paths)


def _tracked_paths(worktree: Path) -> set[str]:
    result = subprocess.run(["git", "-C", str(worktree), "ls-files", "-z"],
                            capture_output=True, timeout=5)
    if result.returncode:
        raise RuntimeError("candidate tracked file list is unavailable")
    return {path.decode(errors="replace") for path in result.stdout.split(b"\0") if path}


def create_candidate_worktree(repository: Path, branch: str, destination: Path,
                              base_revision: str) -> dict[str, str]:
    repository = repository.expanduser().resolve(strict=True)
    destination = destination.expanduser().resolve()
    top = subprocess.run(["git", "-C", str(repository), "rev-parse", "--show-toplevel"],
                         capture_output=True, text=True, timeout=5)
    if top.returncode:
        raise ValueError("candidate source is not a Git worktree")
    source_root = Path(top.stdout.strip()).resolve()
    if destination.is_relative_to(source_root) or destination.exists():
        raise ValueError("candidate destination must be a new directory outside the source worktree")
    if branch in {"main", "master"} or branch.startswith("main/"):
        raise ValueError("candidate worktree cannot use a protected main branch name")
    checked = subprocess.run(["git", "check-ref-format", "--branch", branch], capture_output=True, timeout=5)
    if checked.returncode:
        raise ValueError("candidate branch name is invalid")
    base = subprocess.run(["git", "-C", str(source_root), "rev-parse", "--verify", f"{base_revision}^{{commit}}"],
                          capture_output=True, text=True, timeout=5)
    if base.returncode:
        raise ValueError("candidate baseline is not a local commit")
    exists = subprocess.run(["git", "-C", str(source_root), "show-ref", "--verify", "--quiet", f"refs/heads/{branch}"],
                            timeout=5)
    if exists.returncode == 0:
        raise ValueError("candidate branch already exists")
    created = subprocess.run(["git", "-C", str(source_root), "worktree", "add", "-b", branch,
                              str(destination), base.stdout.strip()], capture_output=True, text=True, timeout=30)
    if created.returncode:
        raise RuntimeError("candidate worktree creation failed; Git output is redacted from the receipt")
    return {"branch": branch, "baseline_revision": base.stdout.strip(), "worktree": str(destination)}


def candidate_patch_sha256(worktree: Path, paths: list[str] | None = None) -> str:
    import hashlib
    worktree = worktree.resolve(strict=True)
    paths = changed_paths(worktree) if paths is None else paths
    tracked_diff = subprocess.run(["git", "-C", str(worktree), "diff", "--binary", "HEAD"],
                                  capture_output=True, timeout=10)
    if tracked_diff.returncode:
        raise RuntimeError("candidate diff could not be captured")
    digest = hashlib.sha256(tracked_diff.stdout)
    tracked = _tracked_paths(worktree)
    for relative in paths:
        target = worktree / relative
        if target.is_symlink():
            raise ValueError(f"candidate patch may not introduce symlink path: {relative}")
        if target.is_file() and relative not in tracked:
            digest.update(relative.encode())
            digest.update(b"\0")
            digest.update(target.read_bytes())
    return digest.hexdigest()


def check_scope(paths: list[str], allowed_prefixes: tuple[str, ...]) -> None:
    forbidden = ("dev/harness_lab/", "plans/closure/", "plans/implementation/", "plans/registry.md")
    for path in paths:
        if path.startswith(forbidden) or not any(path == prefix.rstrip("/") or path.startswith(prefix) for prefix in allowed_prefixes):
            raise ValueError(f"candidate changed forbidden or out-of-scope path: {path}")


def _build_with_compile_correction(*, build, propose, gates: list[Any], started: float,
                                   max_wallclock: float, timeout: float, proposer: str,
                                   max_candidate_retries: int, checkpoint: "Checkpoint | None",
                                   correction_prompt: str, allowed_prefixes: tuple[str, ...],
                                   changelog: str, worktree: Path, patch_digest: str
                                   ) -> tuple[str, int]:
    """Build a candidate, correcting a compile failure at most N times.

    This is the bounded compile-correction mechanic. It is extracted so the
    bounding is directly testable: each retry is separately counted and gated,
    the build is re-run after every correction, and the retry count is
    exhausted rather than unbounded.

    ``build`` performs one build attempt and raises on failure. ``propose``
    asks the proposer for a correction and returns its receipt. Both are
    injected so the mechanic can be qualified against a labelled fault
    injection without a live provider.
    """
    import hashlib

    retry_count = 0
    while True:
        remaining = max_wallclock - (time.monotonic() - started)
        if remaining <= 0:
            raise TimeoutError("candidate qualification wallclock budget exhausted during build gates")
        try:
            gates.append(build())
            return patch_digest, retry_count
        except (RuntimeError, TimeoutError) as error:
            if proposer != "opencode" or retry_count >= max_candidate_retries:
                raise
            retry_count += 1
            if checkpoint is not None:
                checkpoint.transition("PROPOSING", {"attempt_kind": "compile_correction",
                                                      "retry": retry_count,
                                                      "failure_sha256": hashlib.sha256(str(error).encode()).hexdigest()})
            prompt = (correction_prompt.strip() + "\n\n" if correction_prompt.strip() else "") + \
                "Correct this candidate's compile gate failure. This is a compile-correction retry, not a new candidate. " + \
                "Keep changes within the already authorized path scope and preserve CHANGELOG.harness.md. " + \
                "Do not claim tests passed. Failure evidence (bounded):\n" + str(error)[-4096:]
            remaining = max_wallclock - (time.monotonic() - started)
            if remaining <= 0:
                raise TimeoutError("candidate qualification wallclock budget exhausted before compile correction")
            receipt = propose(prompt)
            corrected_paths = changed_paths(worktree)
            check_scope(corrected_paths, allowed_prefixes + (changelog,))
            if changelog not in corrected_paths:
                raise ValueError(f"compile correction removed required {changelog}")
            corrected_text = (worktree / changelog).read_text(encoding="utf-8")
            if any(section.casefold() not in corrected_text.casefold()
                   for section in ("Root cause", "Contracts", "Validation", "Accepted risks")):
                raise ValueError("compile correction invalidated required candidate changelog sections")
            if any(word.casefold() in corrected_text.casefold() for word in ("<candidate", "TODO", "TBD", "pending")):
                raise ValueError("compile correction left an unresolved changelog placeholder")
            patch_digest = candidate_patch_sha256(worktree, corrected_paths)
            if checkpoint is not None:
                checkpoint.transition("PATCHED", {"attempt_kind": "compile_correction", "retry": retry_count,
                                                   "changed_paths": corrected_paths,
                                                   "patch_sha256": patch_digest,
                                                   "proposer_receipt": receipt})
                checkpoint.transition("BUILDING", {"retry": retry_count, "patch_sha256": patch_digest})


def manual_dry_run(path: Path, max_iterations: int = 1, max_candidate_retries: int = 0) -> dict[str, Any]:
    budget = Budget(max_iterations=max_iterations, max_candidate_retries=max_candidate_retries,
                    max_proposer_calls=max_iterations)
    budget.consume("iteration")
    checkpoint = Checkpoint(path, "dry-run-no-source-revision", "dry-run-no-suite")
    checkpoint.transition("DIAGNOSED", {"assessment": "synthetic deterministic smoke; no model call"})
    budget.consume("proposer")
    checkpoint.transition("PROPOSING", {"proposer": "manual", "attempt": 1})
    checkpoint.transition("REJECTED", {"reason": "no patch supplied to ManualProposer", "source_mutated": False})
    return {"status": checkpoint.state, "states": ["BASELINED", "DIAGNOSED", "PROPOSING", "REJECTED"],
            "budget": {"iterations": budget.iterations, "retries": budget.retries, "proposer_calls": budget.proposer_calls},
            "source_mutated": False, "checkpoint": str(path), "provider_calls": 0}


def admit_budget_policy(policy: "budget.BudgetPolicy") -> "budget.BudgetAdmission":
    """Run the provider-free admission preflight and refuse when it fails.

    The preflight is a pure function of the declared policy: it opens no
    socket, spawns no subprocess and contacts no provider, so it runs before
    the first provider request. A policy that cannot be enforced is a
    refusal, never a downgrade, and the refusal names the refused caps.
    """
    admission = budget.admit_budget(policy)
    if admission.status != budget.STATUS_ADMITTED:
        raise RuntimeError(
            "budget admission refused before the proposer ran: "
            + json.dumps(admission.as_dict(), sort_keys=True))
    return admission


def observe_proposer_usage(guard: "budget.SpendGuard", receipt: dict[str, Any]) -> None:
    """Record the usage a proposer receipt reported into the spend guard.

    An absent measurement is simply not passed to the guard: a figure the CLI
    never reported is unknown, and the guard never sees it as zero.
    """
    usage = receipt.get("proposer_usage") or {}
    tokens = usage.get("tokens") or {}
    observed: dict[str, Any] = {"provider_requests": 1}

    def figure(value: Any) -> int | float | None:
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            return None
        return value

    for key in ("input", "output"):
        value = figure(tokens.get(key))
        if value is not None:
            observed[f"{key}_tokens"] = int(value)
    total = figure(tokens.get("total"))
    if total is not None:
        observed["total_tokens"] = int(total)
    cost = figure(usage.get("cost_usd"))
    if cost is not None:
        observed["cost_usd"] = float(cost)
    guard.observe(**observed)


def guarded_propose(guard: "budget.SpendGuard | None", *, worktree: Path, prompt: str, timeout: float,
                    model: str | None, executable: str | None = None) -> dict[str, Any]:
    """Run one OpenCode proposer invocation under an optional spend guard.

    The guard reserves one provider request *before* the invocation, so a
    run that would cross ``max_provider_requests`` never starts the call.
    The reservation is then replaced by the usage the CLI reported.

    Only the OpenCode proposer call is charged to this guard. The ACP trial
    suite that :func:`qualify_candidate` runs uses a local deterministic
    fixture provider rather than a real provider, so its trials are never
    counted as provider requests here.
    """
    if guard is not None:
        guard.admit(projected_provider_requests=1)
    receipt = run_opencode(worktree, prompt, timeout, executable, model)
    if guard is not None:
        observe_proposer_usage(guard, receipt)
    return receipt


def opencode_propose(worktree: Path, prompt: str, allowed_prefixes: tuple[str, ...], timeout: float = 300.0,
                     executable: str | None = None, model: str | None = None) -> dict[str, Any]:
    worktree = worktree.expanduser().resolve(strict=True)
    model = validate_proposer_model(model)
    top = subprocess.run(["git", "-C", str(worktree), "rev-parse", "--show-toplevel"], capture_output=True, text=True, timeout=5)
    if top.returncode or Path(top.stdout.strip()).resolve() != worktree:
        raise ValueError("OpenCode must run at the root of a dedicated candidate worktree")
    marker = worktree / ".git"
    if not marker.is_file():
        raise ValueError("OpenCode candidate must be a linked Git worktree, not a primary checkout")
    if changed_paths(worktree):
        raise ValueError("candidate worktree must be clean before proposing")
    baseline = subprocess.run(["git", "-C", str(worktree), "rev-parse", "HEAD"], capture_output=True, text=True, timeout=5)
    if baseline.returncode:
        raise ValueError("candidate baseline revision unavailable")
    proposer = run_opencode(worktree, prompt, timeout, executable, model)
    paths = changed_paths(worktree)
    check_scope(paths, allowed_prefixes)
    if not paths:
        raise ValueError("OpenCode returned successfully without a candidate patch")
    if "CHANGELOG.harness.md" not in paths:
        raise ValueError("candidate patch is missing required CHANGELOG.harness.md")
    return {"state": "PATCHED", "baseline_revision": baseline.stdout.strip(), "changed_paths": paths,
            "patch_sha256": candidate_patch_sha256(worktree, paths), "proposer": proposer,
            # The route the proposal is attributed to.
            "proposer_model": model,
            "candidate_worktree": str(worktree), "accepted": False,
            "next_gate": "run bounded candidate qualification; proposal alone cannot be accepted"}


def _run_gate(argv: list[str], cwd: Path, env: dict[str, str], timeout: float) -> dict[str, Any]:
    try:
        result = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, timeout=timeout)
    except subprocess.TimeoutExpired as error:
        raise TimeoutError(f"candidate gate exceeded its {timeout:g}s budget: {argv[0]}") from error
    if result.returncode:
        tail = (result.stderr or result.stdout)[-4096:].decode(errors="replace")
        raise RuntimeError(f"candidate gate failed ({Path(argv[0]).name}, exit {result.returncode}); output tail: {tail}")
    return {"command": argv, "returncode": result.returncode,
            "stdout_sha256": __import__("hashlib").sha256(result.stdout).hexdigest(),
            "stderr_bytes": len(result.stderr)}


PROTECTED_CONTROL_TERMINALS = frozenset({"assertion_failed", "execution_failed", "cancelled", "timeout"})
AVOIDABLE_FAILURE_TERMINALS = frozenset({"execution_failed", "infrastructure_error"})
SCORE_REASON_ACCEPTED = "accepted_by_score_contract"
SCORE_REASON_INSUFFICIENT_EVIDENCE = "insufficient_evidence"
SCORE_REASON_PROTECTED_CONTROL_REGRESSED = "protected_control_regressed"
SCORE_REASON_NO_IMPROVEMENT = "no_criterion_improved"
SCORE_REASON_CRITERION_REGRESSED = "criterion_regressed"


def expected_trial_keys(cases: list[dict[str, Any]]) -> set[tuple[str, int]]:
    """Return the repeat-expanded ``(task_id, repeat)`` keys a suite requires.

    A suite case may declare ``repeats``. A completeness check must compare
    the recorded trials against the *expanded* count: comparing against the
    raw number of cases fails every suite that repeats a case.
    """
    return {(case["task_id"], repeat) for case in cases
            for repeat in range(case.get("repeats", 1))}


def evaluation_is_complete(trials: list[dict[str, Any]], cases: list[dict[str, Any]]) -> bool:
    """Return True when an evaluation covered every repeat-expanded suite trial."""
    expected = expected_trial_keys(cases)
    if len(trials) != len(expected):
        return False
    return {(trial.get("task_id"), trial.get("repeat", 0)) for trial in trials} == expected


def protected_control_keys(cases: list[dict[str, Any]]) -> set[tuple[str, int]]:
    """Return the protected negative-control ``(task_id, repeat)`` keys of a suite.

    The suite schema carries no explicit control marker, and adding one would
    change ``ledger.canonical_digest`` for every case and invalidate every
    stored baseline digest. A case is therefore a protected negative control
    exactly when its ``expected_terminal`` is not ``"passed"``: the schema only
    permits ``passed``, ``assertion_failed``, ``cancelled``, ``timeout`` and
    ``execution_failed``, so every non-``passed`` expectation is a deliberately
    negative case that must keep terminating as expected. A control is a guard,
    never an optimization objective.
    """
    controls: set[tuple[str, int]] = set()
    for case in cases:
        expected_terminal = case.get("expected_terminal", "passed")
        if expected_terminal == "passed":
            continue
        if expected_terminal not in PROTECTED_CONTROL_TERMINALS:
            raise ValueError(f"suite case {case.get('task_id')!r} declares an unsupported negative "
                             f"expected terminal: {expected_terminal!r}")
        for repeat in range(case.get("repeats", 1)):
            controls.add((case["task_id"], repeat))
    return controls


def index_trials(rows: list[dict[str, Any]], candidate: str,
                 suite_sha256: str | None = None) -> dict[tuple[str, int], dict[str, Any]]:
    """Index recorded trials by ``(task_id, repeat)``, refusing a duplicate pair.

    A candidate that records the same ``(task_id, repeat)`` twice would be
    scored twice and would break the paired comparison, so the duplicate is
    refused instead of being silently collapsed.

    Two terminals carry **no measurement**: ``interrupted_unknown`` (a
    reconciled orphan attempt whose outcome is unknown) and
    ``infrastructure_error`` (the harness, the daemon or the transport failed
    before the trial could produce an observation). Neither says anything
    about the candidate, so neither is a completed trial: they are excluded
    from the index and the pair is re-run. A pair may therefore appear twice
    in the ledger — once unmeasured, once measured — and the measured row is
    the one that counts. Two *measured* rows for one pair are still refused.
    """
    indexed: dict[tuple[str, int], dict[str, Any]] = {}
    from . import ledger
    for row in rows:
        if row.get("candidate") != candidate:
            continue
        if suite_sha256 is not None and row.get("suite_sha256") != suite_sha256:
            continue
        if row.get("terminal") in ledger.UNMEASURED_TERMINALS:
            continue
        key = (row.get("task_id"), row.get("repeat", 0))
        if key in indexed:
            raise ValueError(f"candidate {candidate} records duplicate trials for {key}")
        indexed[key] = row
    return indexed


def _observed_duration_ms(row: dict[str, Any]) -> float | None:
    """Return a trial's measured duration in ms, or None when it was not measured."""
    metrics = row.get("metrics")
    if not isinstance(metrics, dict):
        return None
    value = metrics.get("wall_time_ms")
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    return float(value)


def score_candidate_trials(cases: list[dict[str, Any]],
                           baseline_by_key: dict[tuple[str, int], dict[str, Any]],
                           candidate_by_key: dict[tuple[str, int], dict[str, Any]],
                           contract: Any | None = None) -> dict[str, Any]:
    """Score a candidate's recorded trials against its frozen baseline.

    The decision comes from the versioned score contract in
    :mod:`dev.harness_lab.scoring`; this function only maps recorded trials
    onto the contract's criteria.

    * ``correctness.pass_rate`` — fraction of the *non-control* cases whose
      recorded terminal matched the expected terminal.
    * ``reliability.success_rate`` — fraction of the paired trials that did not
      end in ``execution_failed`` or ``infrastructure_error``.
    * ``efficiency.duration_ms`` — median observed duration of the paired
      trials that reported a duration on *both* sides.
    * ``control.no_regression`` — fraction of the protected negative controls
      that still terminated as expected. The control is a guard: it is fed
      explicitly, and it never enters the improvement set or the aggregate.

    A criterion is only *measured* when its side covers every expected trial
    of that criterion. A trial whose telemetry was not measured maps to an
    unmeasured observation, never to ``0``.
    """
    from . import scoring

    contract = contract if contract is not None else scoring.default_contract()
    expected = {(case["task_id"], repeat): case
                for case in cases for repeat in range(case.get("repeats", 1))}
    controls = protected_control_keys(cases)
    objective_keys = [key for key in expected if key not in controls]
    control_keys = [key for key in expected if key in controls]
    complete = sorted(key for key in expected
                      if key in baseline_by_key and key in candidate_by_key)

    def covers(rows: dict[tuple[str, int], dict[str, Any]], keys: list[tuple[str, int]]) -> bool:
        return bool(keys) and all(key in rows for key in keys)

    def match_rate(rows: dict[tuple[str, int], dict[str, Any]],
                   keys: list[tuple[str, int]]) -> float | None:
        """Fraction of ``keys`` whose recorded terminal matched the expectation."""
        if not covers(rows, keys):
            return None
        matched = sum(1 for key in keys
                      if rows[key].get("terminal") == expected[key].get("expected_terminal", "passed"))
        return matched / len(keys)

    def reliability_rate(rows: dict[tuple[str, int], dict[str, Any]]) -> float | None:
        """Fraction of the paired trials that did not end in an avoidable failure."""
        if not covers(rows, list(expected)):
            return None
        return sum(1 for key in expected
                   if rows[key].get("terminal") not in AVOIDABLE_FAILURE_TERMINALS) / len(expected)

    duration_keys = [key for key in complete
                     if _observed_duration_ms(baseline_by_key[key]) is not None
                     and _observed_duration_ms(candidate_by_key[key]) is not None]
    duration_measured = (covers(baseline_by_key, list(expected))
                         and covers(candidate_by_key, list(expected)) and bool(duration_keys))

    def median_duration(rows: dict[tuple[str, int], dict[str, Any]]) -> float | None:
        """Median observed duration over the trials that reported one on both sides."""
        if not duration_measured:
            return None
        return float(statistics.median([_observed_duration_ms(rows[key]) for key in duration_keys]))

    def observation(criterion_id: str, value: float | None) -> Any:
        if value is None:
            return scoring.Observation(criterion_id=criterion_id, value=None, measured=False)
        return scoring.Observation(criterion_id=criterion_id, value=float(value), measured=True)

    baseline_observations = [
        observation("correctness.pass_rate", match_rate(baseline_by_key, objective_keys)),
        observation("reliability.success_rate", reliability_rate(baseline_by_key)),
        observation("efficiency.duration_ms", median_duration(baseline_by_key)),
        observation("control.no_regression", match_rate(baseline_by_key, control_keys)),
    ]
    candidate_observations = [
        observation("correctness.pass_rate", match_rate(candidate_by_key, objective_keys)),
        observation("reliability.success_rate", reliability_rate(candidate_by_key)),
        observation("efficiency.duration_ms", median_duration(candidate_by_key)),
        observation("control.no_regression", match_rate(candidate_by_key, control_keys)),
    ]
    outcome = scoring.score_candidate(contract, baseline_observations, candidate_observations,
                                      evidence_pairs=len(complete))
    return {"outcome": outcome, "contract": contract, "evidence_pairs": len(complete),
            "baseline_observations": baseline_observations,
            "candidate_observations": candidate_observations,
            "protected_controls": sorted(controls), "complete_pairs": complete}


def decision_reasons(outcome: Any) -> tuple[str, list[str]]:
    """Return the primary decision token and the full ordered token list.

    ``insufficient_evidence`` is its own token: it never reuses or renames an
    existing rejection reason.
    """
    from . import scoring

    if outcome.decision == scoring.DECISION_ACCEPTED:
        primary = SCORE_REASON_ACCEPTED
    elif outcome.decision == scoring.DECISION_INSUFFICIENT_EVIDENCE:
        primary = SCORE_REASON_INSUFFICIENT_EVIDENCE
    elif outcome.protected_violations:
        primary = SCORE_REASON_PROTECTED_CONTROL_REGRESSED
    elif "criterion_regressed" in outcome.reasons:
        primary = SCORE_REASON_CRITERION_REGRESSED
    else:
        primary = SCORE_REASON_NO_IMPROVEMENT
    reasons: list[str] = []
    for token in (primary,) + tuple(outcome.reasons):
        if token not in reasons:
            reasons.append(token)
    return primary, reasons


def authorize_build_transition(checkpoint: Checkpoint, patch_digest: str, resume: bool) -> None:
    """Raise when the checkpoint may not enter ``BUILDING``; return None otherwise.

    A non-resume call may enter ``BUILDING`` from ``PATCHED`` only when this
    checkpoint has not already built this patch. Building the same patch twice
    would evaluate the same candidate twice and duplicate its recorded trials,
    so the second build is refused and the proposal is preserved for manual
    inspection.
    """
    if checkpoint.state != "PATCHED" and not resume:
        raise ValueError("candidate checkpoint exists; pass --resume to continue the same immutable candidate")
    if checkpoint.state in TERMINAL:
        raise ValueError("candidate checkpoint is already terminal")
    if checkpoint.state == "PATCHED" and not resume:
        events, _torn = _read_event_log(checkpoint.events_path)
        already_built = [event for event in events
                         if event.get("state") == "BUILDING"
                         and event.get("evidence", {}).get("patch_sha256") == patch_digest]
        if already_built:
            raise ValueError("candidate checkpoint already built this patch; "
                             "pass --resume to continue the same immutable candidate")


def authorize_binary_fingerprint(checkpoint: Checkpoint, binary_digest: str) -> None:
    """Refuse a resume whose rebuilt binary differs from the checkpointed build.

    The fingerprint that must match is the *latest* one recorded by the
    checkpoint: accepting any previously recorded hash would let a resume
    continue under a binary the checkpoint never saw.
    """
    events, _torn = _read_event_log(checkpoint.events_path)
    fingerprints = [event.get("evidence", {}).get("binary_sha256") for event in events
                    if event.get("evidence", {}).get("binary_sha256")]
    if fingerprints and binary_digest != fingerprints[-1]:
        raise RuntimeError("candidate binary fingerprint differs from the latest checkpointed build; "
                           "refusing resume")


def qualify_candidate(worktree: Path, suite: Path, ledger_root: Path, *, baseline_candidate: str,
                      candidate: str, target_dir: Path, allowed_prefixes: tuple[str, ...],
                      timeout: float = 900.0, max_wallclock: float = 3600.0,
                      changelog: str = "CHANGELOG.harness.md",
                      checkpoint: Checkpoint | None = None, resume: bool = False,
                      proposer: str = "manual", correction_prompt: str = "",
                      max_candidate_retries: int = 0, model: str | None = None,
                      budget_policy: "budget.BudgetPolicy | None" = None,
                      spend_guard: "budget.SpendGuard | None" = None) -> dict[str, Any]:
    """Build and score a candidate without changing the frozen baseline or default worktree.

    ``model`` pins the OpenCode proposer to one approved ``provider/model``
    route. ``budget_policy`` is the declared budget: it is admitted *before*
    any provider work and then enforced by a spend guard, so a live proposal
    cannot exceed the caps the admission gate admitted. ``spend_guard`` lets
    a caller that already owns a guard (the optimizer) share one cap across
    the whole run rather than resetting it per phase.
    """
    import hashlib
    import sys
    from . import ledger
    from . import scoring

    model = validate_proposer_model(model)
    if budget_policy is not None:
        admit_budget_policy(budget_policy)
    guard = spend_guard
    if guard is None and budget_policy is not None:
        guard = budget.SpendGuard(budget_policy)

    started = time.monotonic()
    worktree = worktree.expanduser().resolve(strict=True)
    suite = suite.expanduser().resolve(strict=True)
    ledger_root = ledger_root.expanduser().resolve(strict=True)
    target_dir = target_dir.expanduser().resolve()
    if target_dir.is_relative_to(worktree):
        raise ValueError("candidate Cargo target directory must be separate from the candidate worktree")
    top = subprocess.run(["git", "-C", str(worktree), "rev-parse", "--show-toplevel"],
                         capture_output=True, text=True, timeout=5)
    if top.returncode or Path(top.stdout.strip()).resolve() != worktree or not (worktree / ".git").is_file():
        raise ValueError("candidate qualification requires a dedicated linked Git worktree")
    branch = subprocess.run(["git", "-C", str(worktree), "branch", "--show-current"],
                            capture_output=True, text=True, timeout=5)
    if branch.returncode or branch.stdout.strip() in {"", "main", "master"}:
        raise ValueError("candidate qualification requires a named non-main branch")
    paths = changed_paths(worktree)
    check_scope(paths, allowed_prefixes + (changelog,))
    patch_digest = candidate_patch_sha256(worktree, paths)
    if changelog not in paths:
        raise ValueError(f"candidate patch must include {changelog}")
    changelog_text = (worktree / changelog).read_text(encoding="utf-8")
    required_sections = ("Root cause", "Contracts", "Validation", "Accepted risks")
    if any(section.casefold() not in changelog_text.casefold() for section in required_sections):
        raise ValueError("candidate changelog is missing a required root cause/contracts/validation/risks section")
    for forbidden in ("<candidate", "TODO", "TBD", "pending"):
        if forbidden.casefold() in changelog_text.casefold():
            raise ValueError("candidate changelog contains an unresolved placeholder")

    cases = [json.loads(line) for line in suite.read_text(encoding="utf-8").splitlines() if line.strip()]
    if not cases or any(not isinstance(row, dict) or row.get("schema_version") != 1 for row in cases):
        raise ValueError("candidate suite must be a non-empty version 1 JSONL manifest")
    suite_digest = hashlib.sha256(suite.read_bytes()).hexdigest()
    baseline_rows = [row for row in ledger.load_runs(ledger_root) if row["candidate"] == baseline_candidate]
    if not baseline_rows:
        raise ValueError("candidate qualification requires existing frozen baseline ledger records")
    # A reconciled orphan attempt carries no measurement and no known outcome:
    # it is never a completed baseline trial, and a baseline that still has an
    # ambiguous attempt is not a frozen baseline to compare against.
    interrupted_baseline = [row for row in baseline_rows
                            if row.get("terminal") == INTERRUPTED_UNKNOWN_OUTCOME]
    if interrupted_baseline:
        raise ValueError("frozen baseline has interrupted attempts whose outcome is unknown; "
                         "inspect them before qualifying a candidate")
    baseline_revision = next((row.get("subject", {}).get("revision") for row in baseline_rows
                              if row.get("subject", {}).get("revision")), None)
    worktree_revision = subprocess.run(["git", "-C", str(worktree), "rev-parse", "HEAD"],
                                       capture_output=True, text=True, timeout=5)
    if not baseline_revision or worktree_revision.returncode or worktree_revision.stdout.strip() != baseline_revision:
        raise ValueError("candidate worktree HEAD must equal the frozen baseline source revision")
    baseline_index = (ledger_root / "index.jsonl").read_bytes()
    baseline_manifests = {row["run_id"]: ledger.canonical_digest(row) for row in baseline_rows}
    baseline_pairs = index_trials(baseline_rows, baseline_candidate, suite_digest)
    expected_keys = expected_trial_keys(cases)
    expected_cases = {(case["task_id"], repeat): case for case in cases
                      for repeat in range(case.get("repeats", 1))}
    if set(baseline_pairs) != expected_keys:
        raise ValueError("frozen baseline does not exactly cover the candidate suite and repeats")
    for case in cases:
        for repeat in range(case.get("repeats", 1)):
            baseline_row = baseline_pairs[(case["task_id"], repeat)]
            if baseline_row.get("group_id") != case.get("group_id", "native-tool-smoke") or \
                    baseline_row.get("case_sha256") != ledger.canonical_digest(case):
                raise ValueError("frozen baseline task definition differs from candidate suite")
    if checkpoint is not None:
        if checkpoint.base_revision != baseline_revision or checkpoint.suite_digest != suite_digest:
            raise ValueError("candidate checkpoint does not match its frozen source and suite")
        events, _torn = _read_event_log(checkpoint.events_path)
        patched = next((event for event in reversed(events) if event.get("state") == "PATCHED"), None)
        if patched is None or patched.get("evidence", {}).get("patch_sha256") != patch_digest:
            raise ValueError("candidate patch changed since its checkpoint; refusing resume")
        authorize_build_transition(checkpoint, patch_digest, resume)
        checkpoint.transition("BUILDING", {"changed_paths": paths, "patch_sha256": patch_digest,
                                            "suite_sha256": suite_digest,
                                            "resumed_from": checkpoint.state if resume else None})
    remaining = max_wallclock - (time.monotonic() - started)
    if remaining <= 0:
        raise TimeoutError("candidate qualification wallclock budget exhausted before build")
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target_dir)
    env.pop("CODEGG_DAEMON_HOME", None)
    gates = []
    for argv in (["cargo", "fmt", "--all", "--", "--check"],
                 ["cargo", "test", "-p", "codegg", "--test", "acp_stdio"]):
        remaining = max_wallclock - (time.monotonic() - started)
        if remaining <= 0:
            raise TimeoutError("candidate qualification wallclock budget exhausted during build gates")
        gates.append(_run_gate(argv, worktree, env, min(timeout, remaining)))
    retry_count = 0
    build_argv = ["cargo", "build", "--bin", "codegg", "--bin", "codegg-sandbox-helper"]
    patch_digest, retry_count = _build_with_compile_correction(
        build=lambda: _run_gate(build_argv, worktree, env,
                                min(timeout, max_wallclock - (time.monotonic() - started))),
        propose=lambda correction: guarded_propose(
            guard, worktree=worktree, prompt=correction,
            timeout=min(timeout, max_wallclock - (time.monotonic() - started)), model=model),
        gates=gates, started=started, max_wallclock=max_wallclock, timeout=timeout,
        proposer=proposer, max_candidate_retries=max_candidate_retries,
        checkpoint=checkpoint, correction_prompt=correction_prompt,
        allowed_prefixes=allowed_prefixes, changelog=changelog, worktree=worktree,
        patch_digest=patch_digest)
    binary = target_dir / "debug" / ("codegg.exe" if os.name == "nt" else "codegg")
    if not binary.is_file():
        raise FileNotFoundError("candidate build did not produce the expected CodeGG executable")
    binary_digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    if checkpoint is not None:
        authorize_binary_fingerprint(checkpoint, binary_digest)
        checkpoint.transition("QUALIFYING", {"gates": gates, "binary_sha256": binary_digest})
        checkpoint.transition("EVALUATING", {"candidate": candidate, "binary_sha256": binary_digest})
    # Any recorded attempt of this candidate - including a reconciled orphan
    # whose outcome is unknown - blocks a fresh run: the interrupted attempt is
    # preserved and never silently resubmitted under a new identity.
    candidate_prior = [row for row in ledger.load_runs(ledger_root) if row["candidate"] == candidate]
    if candidate_prior and not resume:
        raise ValueError("candidate already has ledger trials; pass --resume to continue its exact frozen trials")
    remaining = max_wallclock - (time.monotonic() - started)
    if remaining <= 0:
        raise TimeoutError("candidate qualification wallclock budget exhausted before ACP trials")
    argv = ["dev.harness_lab", "evaluate", "--binary", str(binary),
            "--suite", str(suite), "--ledger", str(ledger_root), "--candidate", candidate,
            "--max-wallclock", str(remaining), "--timeout", str(min(timeout, remaining))]
    if candidate_prior:
        argv.append("--resume-candidate")
    # The evaluation runs the *controller's* harness, not whatever copy of it
    # happens to sit in the candidate tree. The measuring instrument has to be
    # the one this controller owns: a candidate worktree that carried an older
    # or edited copy of the harness would otherwise measure itself. The working
    # directory still points at the candidate worktree, because the trial's
    # subject revision is read from there.
    controller_root = Path(__file__).resolve().parents[2]
    launcher = ("import runpy, sys\n"
                "sys.path.insert(0, {root!r})\n"
                "sys.argv = {argv!r}\n"
                "runpy.run_module('dev.harness_lab', run_name='__main__')\n")
    command = [sys.executable, "-c",
               launcher.format(root=str(controller_root), argv=argv)]
    evaluated = subprocess.run(command, cwd=worktree, env=env, capture_output=True, text=True,
                               timeout=remaining)
    try:
        evaluation = json.loads(evaluated.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError("candidate evaluation did not return a structured receipt") from error
    trials = evaluation.get("trials", [])
    # The completeness check compares against the repeat-expanded trial count:
    # a suite case may declare repeats, and the raw case count is smaller.
    if evaluation.get("budget_exhausted") or not evaluation_is_complete(trials, cases):
        raise RuntimeError("candidate evaluation was incomplete; candidate cannot be accepted")
    if checkpoint is not None:
        checkpoint.transition("COMPARING", {"trials": len(trials), "evaluation_exit": evaluated.returncode,
                                             "baseline_index_prefix_sha256": hashlib.sha256(baseline_index).hexdigest()})
    after = ledger.load_runs(ledger_root)
    after_baseline = {row["run_id"]: ledger.canonical_digest(row) for row in after
                      if row["candidate"] == baseline_candidate}
    if after_baseline != baseline_manifests:
        raise RuntimeError("candidate qualification changed frozen baseline manifests")
    if not (ledger_root / "index.jsonl").read_bytes().startswith(baseline_index):
        raise RuntimeError("candidate evaluation changed the frozen ledger index prefix")
    if candidate_patch_sha256(worktree, changed_paths(worktree)) != patch_digest:
        raise RuntimeError("candidate worktree changed while qualification was running")
    comparison = ledger.compare(ledger_root, baseline_candidate, candidate,
                                expected_terminals={
                                    (case["task_id"], repeat): case.get("expected_terminal", "passed")
                                    for case in cases for repeat in range(case.get("repeats", 1))})
    # ``compare`` reports the repeat-expanded pair count as its denominator.
    if comparison["denominator"] != len(expected_keys) or comparison["unpaired_right"]:
        raise RuntimeError("candidate failed paired non-regression comparison")
    baseline_by_key = index_trials(baseline_rows, baseline_candidate, suite_digest)
    candidate_by_key = index_trials(after, candidate, suite_digest)
    if set(candidate_by_key) != expected_keys:
        raise RuntimeError("candidate ledger does not exactly cover the candidate suite and repeats")
    scored = score_candidate_trials(cases, baseline_by_key, candidate_by_key)
    outcome = scored["outcome"]
    controls = protected_control_keys(cases)
    correctness_improvements = [key for key in sorted(expected_keys)
                                if key not in controls
                                and baseline_by_key[key]["terminal"] != expected_cases[key].get("expected_terminal", "passed")
                                and candidate_by_key[key]["terminal"] == expected_cases[key].get("expected_terminal", "passed")]
    # Every trial whose recorded terminal is not the expected one stays visible:
    # the score contract classifies them, the receipt names them.
    unmatched_trials = [key for key in sorted(expected_keys)
                        if candidate_by_key[key]["terminal"] != expected_cases[key].get("expected_terminal", "passed")]
    accepted = outcome.decision == scoring.DECISION_ACCEPTED
    status = "ACCEPTED" if accepted else "REJECTED"
    primary_reason, reasons = decision_reasons(outcome)
    if accepted and comparison["regressions"]:
        # The contract accepted a candidate that regressed a paired trial: the
        # paired comparison is authoritative and refuses the acceptance.
        raise RuntimeError("candidate failed paired non-regression comparison")
    # An interrupted attempt stays visible: it is reported, never scored.
    interrupted_trials = comparison.get("interrupted_trials", {"left": [], "right": []})
    if checkpoint is not None:
        checkpoint.transition(status, {"correctness_improvements": correctness_improvements,
                                       "paired_comparison": comparison,
                                       "score_contract": scored["contract"].as_dict(),
                                       "score_outcome": outcome.as_dict(),
                                       "evidence_pairs": scored["evidence_pairs"],
                                       "protected_violations": list(outcome.protected_violations),
                                       "unmatched_trials": unmatched_trials,
                                       "interrupted_trials": interrupted_trials,
                                       "reason": primary_reason, "reasons": reasons})
    receipt = {"status": status, "candidate": candidate, "branch": branch.stdout.strip(),
               "binary_sha256": binary_digest, "suite_sha256": suite_digest, "patch_sha256": patch_digest,
               "baseline_candidate": baseline_candidate, "baseline_manifests_unchanged": True,
               "gates": gates, "evaluation_exit": evaluated.returncode,
               "paired_comparison": comparison, "correctness_improvements": correctness_improvements,
               "unmatched_trials": unmatched_trials,
               "compile_correction_retries": retry_count,
               "score": outcome.score, "objective": outcome.objective,
               "reason": primary_reason, "reasons": reasons,
               "protected_violations": list(outcome.protected_violations),
               "interrupted_trials": interrupted_trials,
               "score_contract": scored["contract"].as_dict(), "score_outcome": outcome.as_dict(),
               "evidence_pairs": scored["evidence_pairs"],
               "wall_time_seconds": time.monotonic() - started,
               "accepted": accepted, "next_gate": None if accepted else "propose a candidate with a verified correctness improvement"}
    if guard is not None:
        # The observed spend is auditable: it names the requests the proposer
        # made and the usage the CLI reported for them.
        receipt["budget_guard"] = guard.as_dict()
    return receipt


def refuse_changed_proposer_model(stored_inputs: dict[str, Any], inputs: dict[str, Any]) -> None:
    """Fail closed when a resume would switch the proposer model.

    The proposer model is part of the immutable input set a checkpoint binds.
    A resumed run must not silently switch routes: the recorded receipts would
    then attribute the work to a route that never produced it. The refusal
    names both the recorded and the requested model.
    """
    stored = stored_inputs.get("proposer_model")
    requested = inputs.get("proposer_model")
    if stored != requested:
        raise ValueError(
            "optimization proposer model changed; refusing resume "
            f"(the checkpoint recorded {stored!r}, the run requested {requested!r})")


class OptimizationCheckpoint:
    """Hash-linked, append-only run receipt with an atomic current-state pointer."""

    def __init__(self, path: Path, inputs: dict[str, Any]) -> None:
        self.path = path
        self.events_path = path.with_suffix(".jsonl")
        self.inputs = inputs
        self.inputs_sha256 = hashlib.sha256(json.dumps(inputs, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        if path.exists() or self.events_path.exists():
            raise FileExistsError("optimization checkpoint exists; use --resume with identical inputs")
        self.run_id = __import__("uuid").uuid4().hex
        self.last_hash = "0" * 64
        self.sequence = 0
        self.events: list[dict[str, Any]] = []
        self.record("run_started", {"run_id": self.run_id, "inputs": inputs})

    @classmethod
    def resume(cls, path: Path, inputs: dict[str, Any]) -> "OptimizationCheckpoint":
        events_path = path.with_suffix(".jsonl")
        decoded, _torn_tail = _read_event_log(events_path)
        if not decoded or not path.is_file():
            raise ValueError("optimization checkpoint is incomplete")
        pointer = json.loads(path.read_text(encoding="utf-8"))
        previous = "0" * 64
        for sequence, event in enumerate(decoded, 1):
            digest = event.pop("event_sha256", None)
            if event.get("seq") != sequence or event.get("prev_sha256") != previous:
                raise ValueError("optimization event chain is corrupt")
            actual = hashlib.sha256(json.dumps(event, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
            event["event_sha256"] = digest
            if digest != actual:
                raise ValueError("optimization event checksum is corrupt")
            previous = actual
        if decoded[0].get("kind") != "run_started":
            raise ValueError("optimization pointer does not match its append-only event tail")
        stored_inputs = decoded[0].get("data", {}).get("inputs")
        digest = hashlib.sha256(json.dumps(inputs, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        refuse_changed_proposer_model(stored_inputs or {}, inputs)
        if stored_inputs != inputs or decoded[0].get("inputs_sha256") != digest:
            raise ValueError("optimization inputs changed; refusing resume")
        instance = cls.__new__(cls)
        instance.path, instance.events_path, instance.inputs = path, events_path, inputs
        instance.inputs_sha256, instance.run_id = digest, decoded[0]["data"]["run_id"]
        instance.last_hash, instance.sequence, instance.events = previous, len(decoded), decoded
        if pointer != decoded[-1]:
            pointer_index = next((index for index, event in enumerate(decoded) if event == pointer), None)
            if pointer_index is None:
                raise ValueError("optimization pointer does not match its append-only event tail")
            raise instance._reconcile_interruption(decoded, pointer, pointer_index)
        if decoded[-1].get("kind") == "run_interrupted" and \
                decoded[-1].get("data", {}).get("outcome") == INTERRUPTED_UNKNOWN_OUTCOME:
            raise CheckpointInterrupted(decoded[-1].get("data", {}).get("reason")
                                        or "optimization run was interrupted; its outcome is recorded as "
                                           f"{INTERRUPTED_UNKNOWN_OUTCOME}")
        if decoded[-1]["kind"] == "run_finished":
            raise ValueError("optimization checkpoint is already finished")
        return instance

    def _reconcile_interruption(self, decoded: list[dict[str, Any]], pointer: dict[str, Any],
                                pointer_index: int) -> CheckpointInterrupted:
        """Record the ambiguous interruption and refuse to continue.

        The append-only tail advanced past the pointer, so the process died
        between the event write and the pointer replacement: the interrupted
        run's outcome is unknown. The interruption is recorded as an
        ``interrupted_unknown`` outcome and the resume is refused, so the run
        is never silently continued or resubmitted.
        """
        unpointed = decoded[pointer_index + 1:]
        reason = ("optimization pointer is stale: the append-only event tail advanced to seq "
                  f"{decoded[-1].get('seq')} ({decoded[-1].get('kind')}) while the pointer still names "
                  f"seq {pointer.get('seq')} ({pointer.get('kind')}); the interrupted run's outcome is "
                  f"ambiguous and was recorded as {INTERRUPTED_UNKNOWN_OUTCOME}")
        if decoded[-1].get("kind") == "run_interrupted" and \
                decoded[-1].get("data", {}).get("outcome") == INTERRUPTED_UNKNOWN_OUTCOME:
            # A repeated resume is idempotent: the interruption is already
            # recorded, so no second event is appended for the same tail.
            raise CheckpointInterrupted(reason)
        self.record("run_interrupted", {"outcome": INTERRUPTED_UNKNOWN_OUTCOME, "reason": reason,
                                        "pointer_seq": pointer.get("seq"),
                                        "unpointed_seq": [event.get("seq") for event in unpointed],
                                        "unpointed_kind": decoded[-1].get("kind"),
                                        "source_mutated": False})
        raise CheckpointInterrupted(reason)

    def record(self, kind: str, data: dict[str, Any]) -> None:
        event = {"schema_version": 1, "seq": self.sequence + 1, "kind": kind,
                 "created_unix_ms": int(time.time() * 1000), "inputs_sha256": self.inputs_sha256,
                 "prev_sha256": self.last_hash, "data": data}
        digest = hashlib.sha256(json.dumps(event, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        event["event_sha256"] = digest
        self.path.parent.mkdir(parents=True, exist_ok=True)
        with self.events_path.open("a", encoding="utf-8") as output:
            output.write(json.dumps(event, sort_keys=True, separators=(",", ":")) + "\n")
            output.flush()
            os.fsync(output.fileno())
        temporary = self.path.with_suffix(".tmp")
        temporary.write_text(json.dumps(event, sort_keys=True, indent=2) + "\n", encoding="utf-8")
        os.replace(temporary, self.path)
        self.sequence, self.last_hash = event["seq"], digest
        self.events.append(event)


def run_optimizer(repository: Path, suite: Path, ledger_root: Path, *, baseline_candidate: str,
                  candidate_root: Path, target_root: Path, checkpoint_path: Path,
                  allowed_prefixes: tuple[str, ...], prompt: str, max_iterations: int = 3,
                  max_candidate_retries: int = 0, max_wallclock: float = 3600.0,
                  timeout: float = 900.0, resume: bool = False,
                  proposer_executable: str | None = None,
                  model: str | None = None,
                  budget_policy: "budget.BudgetPolicy | None" = None) -> dict[str, Any]:
    """Schedule assess/propose/build/retest iterations from one frozen baseline.

    ``model`` pins the OpenCode proposer to one approved ``provider/model``
    route and is bound into the checkpoint's immutable inputs, so a resumed
    run cannot silently switch routes. ``budget_policy`` is admitted before
    any provider work and enforced by one spend guard that is shared with the
    candidate qualification, so the declared caps bound the whole run.
    """
    from . import ledger

    if not prompt.strip() or len(prompt.encode()) > 8192:
        raise ValueError("optimization requires a non-empty assessment prompt no larger than 8 KiB")
    if not allowed_prefixes or not 1 <= max_iterations <= 20 or max_candidate_retries < 0:
        raise ValueError("optimization needs an allowlist, 1-20 iterations, and non-negative retries")
    if max_wallclock <= 0 or timeout <= 0:
        raise ValueError("optimization time budgets must be positive")
    model = validate_proposer_model(model)
    guard = None
    if budget_policy is not None:
        # The admission preflight runs before any provider request, before a
        # worktree is created and before any subprocess is spawned.
        admit_budget_policy(budget_policy)
        guard = budget.SpendGuard(budget_policy)
    repository = repository.expanduser().resolve(strict=True)
    suite = suite.expanduser().resolve(strict=True)
    ledger_root = ledger_root.expanduser().resolve(strict=True)
    candidate_root = candidate_root.expanduser().resolve()
    target_root = target_root.expanduser().resolve()
    if candidate_root.is_relative_to(repository) or target_root.is_relative_to(repository):
        raise ValueError("candidate worktrees and build outputs must be outside the source repository")
    baseline_rows = [row for row in ledger.load_runs(ledger_root) if row["candidate"] == baseline_candidate]
    # A reconciled orphan attempt has no known outcome and no revision receipt.
    baseline_revision = next((row.get("subject", {}).get("revision") for row in baseline_rows
                              if row.get("terminal") != INTERRUPTED_UNKNOWN_OUTCOME
                              and row.get("subject", {}).get("revision")), None)
    if not baseline_revision:
        raise ValueError("optimization requires a frozen baseline with source revision receipts")
    base_revision = baseline_revision
    suite_digest = hashlib.sha256(suite.read_bytes()).hexdigest()
    inputs = {"base_revision": base_revision, "suite_sha256": suite_digest,
              "baseline_candidate": baseline_candidate, "max_iterations": max_iterations,
              "max_candidate_retries": max_candidate_retries,
              "allowed_prefixes": list(allowed_prefixes), "prompt_sha256": hashlib.sha256(prompt.encode()).hexdigest(),
              # The proposer route is part of the immutable input set: a resume
              # must not silently switch models.
              "proposer_model": model}
    if resume:
        try:
            stored_inputs = json.loads(checkpoint_path.read_text(encoding="utf-8"))["data"]["inputs"]
        except (OSError, KeyError, json.JSONDecodeError) as error:
            raise ValueError("optimization checkpoint does not contain resumable immutable inputs") from error
        refuse_changed_proposer_model(stored_inputs, inputs)
        if any(stored_inputs.get(key) != value for key, value in inputs.items()):
            raise ValueError("optimization settings changed; refusing resume")
        if not isinstance(stored_inputs.get("deadline_unix"), (int, float)):
            raise ValueError("optimization checkpoint has no cumulative wallclock deadline")
        inputs["deadline_unix"] = stored_inputs["deadline_unix"]
    else:
        inputs["deadline_unix"] = time.time() + max_wallclock
    state = OptimizationCheckpoint.resume(checkpoint_path, inputs) if resume else OptimizationCheckpoint(checkpoint_path, inputs)
    done = {event["data"]["iteration"]: event["data"] for event in state.events
            if event["kind"] == "iteration_finished"}
    active = next((event["data"] for event in reversed(state.events)
                   if event["kind"] == "iteration_started" and event["data"]["iteration"] not in done), None)
    results = [done[index] for index in sorted(done)]

    def receipt(status: str) -> dict[str, Any]:
        result = {"status": status, "iterations": results, "checkpoint": str(checkpoint_path)}
        if guard is not None:
            # The observed spend is auditable: it names the provider requests
            # the proposer made and the usage the CLI reported for them.
            result["budget_guard"] = guard.as_dict()
        return result

    accepted = next((item for item in results if item.get("result", {}).get("accepted")), None)
    if accepted:
        return receipt("ACCEPTED")
    first_iteration = active["iteration"] if active else (max(done, default=0) + 1)
    candidate_root.mkdir(parents=True, exist_ok=True)
    for iteration in range(first_iteration, max_iterations + 1):
        remaining = float(inputs["deadline_unix"]) - time.time()
        if remaining <= 0:
            state.record("run_interrupted", {"reason": "wallclock_budget_exhausted", "iterations_finished": len(done)})
            return receipt("INTERRUPTED")
        candidate = active["candidate"] if active and iteration == first_iteration else f"{state.run_id}-i{iteration:02d}"
        worktree = Path(active["worktree"]) if active and iteration == first_iteration else candidate_root / candidate
        child_path = Path(active["candidate_checkpoint"]) if active and iteration == first_iteration else checkpoint_path.with_name(f"{checkpoint_path.stem}-{candidate}.json")
        if not active:
            state.record("iteration_started", {"iteration": iteration, "candidate": candidate,
                                                "worktree": str(worktree), "candidate_checkpoint": str(child_path)})
        if not worktree.exists():
            branch = f"harness/{state.run_id}-i{iteration:02d}"
            create_candidate_worktree(repository, branch, worktree, base_revision)
        try:
            child = Checkpoint.resume(child_path, base_revision, suite_digest) if child_path.exists() else Checkpoint(child_path, base_revision, suite_digest)
            if child.state == "BASELINED":
                child.transition("DIAGNOSED", {"baseline": baseline_candidate, "iteration": iteration,
                                                "suite_sha256": suite_digest})
                child.transition("PROPOSING", {"proposer": "opencode", "iteration": iteration})
                prior = results[-1]["result"] if results else None
                iteration_prompt = prompt
                if prior:
                    iteration_prompt += "\n\nPrior iteration assessment (deterministic controller summary):\n" + json.dumps(
                        {"status": prior.get("status"), "accepted": prior.get("accepted"),
                         "correctness_improvements": prior.get("correctness_improvements"),
                         "paired_comparison": prior.get("paired_comparison")}, sort_keys=True)[:4096]
                if guard is not None:
                    # The request is reserved before the proposer is started, so
                    # a run that would cross the declared request cap never
                    # begins the call.
                    guard.admit(projected_provider_requests=1)
                proposal = opencode_propose(worktree, iteration_prompt, allowed_prefixes,
                                            min(timeout, remaining), proposer_executable, model)
                if guard is not None:
                    observe_proposer_usage(guard, proposal["proposer"])
                child.transition("PATCHED", {"changed_paths": proposal["changed_paths"],
                                              "patch_sha256": proposal["patch_sha256"],
                                              "proposer_receipt": proposal["proposer"]})
            elif child.state in {"PATCHED", "BUILDING", "QUALIFYING", "EVALUATING", "COMPARING"}:
                proposal = None
            else:
                raise RuntimeError("interrupted proposal has no committed patch receipt; inspect its candidate worktree before resuming")
            result = qualify_candidate(worktree, suite, ledger_root,
                                       baseline_candidate=baseline_candidate, candidate=candidate,
                                       target_dir=target_root / candidate, allowed_prefixes=allowed_prefixes,
                                       timeout=min(timeout, remaining), max_wallclock=remaining,
                                       checkpoint=child, resume=child.state != "PATCHED",
                                       proposer="opencode", correction_prompt=prompt,
                                       max_candidate_retries=max_candidate_retries,
                                       model=model, budget_policy=budget_policy, spend_guard=guard)
            row = {"iteration": iteration, "candidate": candidate, "result": result}
            state.record("iteration_finished", row)
            results.append(row)
            done[iteration] = row
            active = None
            if result.get("accepted"):
                state.record("run_finished", {"status": "ACCEPTED", "iteration": iteration,
                                               "candidate": candidate, "patch_sha256": result["patch_sha256"]})
                return receipt("ACCEPTED")
        except (ValueError, RuntimeError, TimeoutError, OSError) as error:
            interruption = {"iteration": iteration, "candidate": candidate,
                            "failure_class": type(error).__name__}
            if isinstance(error, CheckpointInterrupted):
                # The interruption was already reconciled to a recorded
                # interrupted_unknown outcome inside the checkpoint that owns
                # it. The outer receipt names it so the ambiguous attempt is
                # never silently continued or resubmitted.
                interruption["outcome"] = INTERRUPTED_UNKNOWN_OUTCOME
                interruption["reason"] = str(error)
            state.record("iteration_interrupted", interruption)
            raise
    state.record("run_finished", {"status": "NO_IMPROVEMENT", "iterations": len(results)})
    return receipt("NO_IMPROVEMENT")
