"""Bounded candidate-proposal primitives; no production or automatic Git mutation."""
from __future__ import annotations

from dataclasses import dataclass
import json
import hashlib
import os
from pathlib import Path
import shutil
import signal
import subprocess
import threading
import time
from typing import Any


STATES = {"BASELINED", "DIAGNOSED", "PROPOSING", "PATCHED", "BUILDING", "QUALIFYING", "EVALUATING", "COMPARING", "ACCEPTED", "REJECTED", "BLOCKED"}
SUPPORTED_OPENCODE_VERSION = "1.19.0"
TERMINAL = {"ACCEPTED", "REJECTED", "BLOCKED"}
ALLOWED = {
    "BASELINED": {"DIAGNOSED", "BLOCKED"}, "DIAGNOSED": {"PROPOSING", "BLOCKED"},
    "PROPOSING": {"PATCHED", "REJECTED", "BLOCKED"}, "PATCHED": {"BUILDING", "REJECTED", "BLOCKED"},
    "BUILDING": {"BUILDING", "QUALIFYING", "PROPOSING", "REJECTED", "BLOCKED"},
    "QUALIFYING": {"BUILDING", "EVALUATING", "REJECTED", "BLOCKED"},
    "EVALUATING": {"BUILDING", "COMPARING", "PROPOSING", "REJECTED", "BLOCKED"},
    "COMPARING": {"BUILDING", "ACCEPTED", "REJECTED", "BLOCKED"},
    "ACCEPTED": set(), "REJECTED": set(), "BLOCKED": set(),
}
OPENCODE_EVENT_TYPES = {"session_start", "session_end", "step_start", "step_finish", "text", "reasoning", "tool_use", "tool_result", "tool", "error", "permission"}


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
        events = [json.loads(line) for line in events_path.read_text(encoding="utf-8").splitlines()]
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
        if path.exists():
            pointer = json.loads(path.read_text(encoding="utf-8"))
            if pointer != events[-1]:
                raise ValueError("checkpoint pointer does not match the append-only event tail")
        else:
            raise ValueError("checkpoint pointer is missing")
        last = events[-1]
        if any(event.get("base_revision") != base_revision or event.get("suite_digest") != suite_digest
               for event in events):
            raise ValueError("checkpoint baseline or fixture suite changed; refusing resume")
        state = last.get("state")
        if state not in STATES or state in TERMINAL:
            raise ValueError("checkpoint is terminal or has an unknown state")
        checkpoint = cls.__new__(cls)
        checkpoint.path, checkpoint.events_path = path, events_path
        checkpoint.state = state
        checkpoint.base_revision = base_revision
        checkpoint.suite_digest = suite_digest
        checkpoint.attempt_id = int(last.get("attempt_id", 0))
        return checkpoint

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
    return {"event_count": len(events), "types": [event["type"] for event in events],
            "text": "".join(str(event.get("part", {}).get("text", "")) for event in events if event["type"] == "text")}


def run_opencode(worktree: Path, prompt: str, timeout: float = 300.0, executable: str | None = None) -> dict[str, Any]:
    binary = executable or shutil.which("opencode")
    if not binary:
        raise FileNotFoundError("opencode executable is unavailable; manual proposer remains available")
    version = subprocess.run([binary, "--version"], cwd=worktree, capture_output=True, text=True, timeout=5)
    if version.returncode != 0 or version.stdout.strip() != SUPPORTED_OPENCODE_VERSION:
        raise RuntimeError("OpenCode version preflight failed")
    env = os.environ.copy()
    env.pop("CODEGG_DAEMON_HOME", None)
    process = subprocess.Popen([binary, "run", "--auto", "--format", "json", "--dir", str(worktree), prompt],
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
            "text_sha256": __import__("hashlib").sha256(parsed["text"].encode()).hexdigest(), "returncode": process.returncode}


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


def opencode_propose(worktree: Path, prompt: str, allowed_prefixes: tuple[str, ...], timeout: float = 300.0,
                     executable: str | None = None) -> dict[str, Any]:
    worktree = worktree.expanduser().resolve(strict=True)
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
    proposer = run_opencode(worktree, prompt, timeout, executable)
    paths = changed_paths(worktree)
    check_scope(paths, allowed_prefixes)
    if not paths:
        raise ValueError("OpenCode returned successfully without a candidate patch")
    if "CHANGELOG.harness.md" not in paths:
        raise ValueError("candidate patch is missing required CHANGELOG.harness.md")
    return {"state": "PATCHED", "baseline_revision": baseline.stdout.strip(), "changed_paths": paths,
            "patch_sha256": candidate_patch_sha256(worktree, paths), "proposer": proposer,
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


def qualify_candidate(worktree: Path, suite: Path, ledger_root: Path, *, baseline_candidate: str,
                      candidate: str, target_dir: Path, allowed_prefixes: tuple[str, ...],
                      timeout: float = 900.0, max_wallclock: float = 3600.0,
                      changelog: str = "CHANGELOG.harness.md",
                      checkpoint: Checkpoint | None = None, resume: bool = False,
                      proposer: str = "manual", correction_prompt: str = "",
                      max_candidate_retries: int = 0) -> dict[str, Any]:
    """Build and score a candidate without changing the frozen baseline or default worktree."""
    import hashlib
    import sys
    from . import ledger

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
    baseline_revision = baseline_rows[0].get("subject", {}).get("revision")
    worktree_revision = subprocess.run(["git", "-C", str(worktree), "rev-parse", "HEAD"],
                                       capture_output=True, text=True, timeout=5)
    if not baseline_revision or worktree_revision.returncode or worktree_revision.stdout.strip() != baseline_revision:
        raise ValueError("candidate worktree HEAD must equal the frozen baseline source revision")
    baseline_index = (ledger_root / "index.jsonl").read_bytes()
    baseline_manifests = {row["run_id"]: ledger.canonical_digest(row) for row in baseline_rows}
    baseline_pairs = {(row["task_id"], row.get("repeat", 0)): row for row in baseline_rows
                      if row.get("suite_sha256") == suite_digest}
    expected_keys = {(case["task_id"], repeat) for case in cases
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
        events = [json.loads(line) for line in checkpoint.events_path.read_text(encoding="utf-8").splitlines()]
        patched = next((event for event in reversed(events) if event.get("state") == "PATCHED"), None)
        if patched is None or patched.get("evidence", {}).get("patch_sha256") != patch_digest:
            raise ValueError("candidate patch changed since its checkpoint; refusing resume")
        prior_binary_hashes = [event.get("evidence", {}).get("binary_sha256") for event in events
                               if event.get("evidence", {}).get("binary_sha256")]
        if checkpoint.state != "PATCHED" and not resume:
            raise ValueError("candidate checkpoint exists; pass --resume to continue the same immutable candidate")
        if checkpoint.state in TERMINAL:
            raise ValueError("candidate checkpoint is already terminal")
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
    while True:
        remaining = max_wallclock - (time.monotonic() - started)
        if remaining <= 0:
            raise TimeoutError("candidate qualification wallclock budget exhausted during build gates")
        try:
            gates.append(_run_gate(build_argv, worktree, env, min(timeout, remaining)))
            break
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
            receipt = run_opencode(worktree, prompt, min(timeout, remaining))
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
    binary = target_dir / "debug" / ("codegg.exe" if os.name == "nt" else "codegg")
    if not binary.is_file():
        raise FileNotFoundError("candidate build did not produce the expected CodeGG executable")
    binary_digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    if checkpoint is not None:
        if prior_binary_hashes and binary_digest not in prior_binary_hashes:
            raise RuntimeError("candidate binary hash differs from its checkpoint; refusing resume")
        checkpoint.transition("QUALIFYING", {"gates": gates, "binary_sha256": binary_digest})
        checkpoint.transition("EVALUATING", {"candidate": candidate, "binary_sha256": binary_digest})
    candidate_prior = [row for row in ledger.load_runs(ledger_root) if row["candidate"] == candidate]
    if candidate_prior and not resume:
        raise ValueError("candidate already has ledger trials; pass --resume to continue its exact frozen trials")
    remaining = max_wallclock - (time.monotonic() - started)
    if remaining <= 0:
        raise TimeoutError("candidate qualification wallclock budget exhausted before ACP trials")
    command = [sys.executable, "-m", "dev.harness_lab", "evaluate", "--binary", str(binary),
               "--suite", str(suite), "--ledger", str(ledger_root), "--candidate", candidate,
               "--max-wallclock", str(remaining), "--timeout", str(min(timeout, remaining))]
    if candidate_prior:
        command.append("--resume-candidate")
    evaluated = subprocess.run(command, cwd=worktree, env=env, capture_output=True, text=True,
                               timeout=remaining)
    try:
        evaluation = json.loads(evaluated.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError("candidate evaluation did not return a structured receipt") from error
    trials = evaluation.get("trials", [])
    if evaluation.get("budget_exhausted") or len(trials) != len(cases):
        raise RuntimeError("candidate evaluation was incomplete; candidate cannot be accepted")
    if any(not trial.get("expected_terminal_matched", False) for trial in trials):
        raise RuntimeError("candidate evaluation contains an unexpected terminal result")
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
    comparison = ledger.compare(ledger_root, baseline_candidate, candidate)
    if comparison["denominator"] != len(cases) or comparison["regressions"] or comparison["unpaired_right"]:
        raise RuntimeError("candidate failed paired non-regression comparison")
    baseline_by_key = {(row["task_id"], row.get("repeat", 0)): row for row in baseline_rows
                       if row.get("suite_sha256") == suite_digest}
    candidate_by_key = {(row["task_id"], row.get("repeat", 0)): row for row in after
                        if row["candidate"] == candidate and row.get("suite_sha256") == suite_digest}
    correctness_improvements = [key for key, expected in
                               { (case["task_id"], repeat): case.get("expected_terminal", "passed")
                                 for case in cases for repeat in range(case.get("repeats", 1)) }.items()
                               if baseline_by_key[key]["terminal"] != expected
                               and candidate_by_key[key]["terminal"] == expected]
    accepted = bool(correctness_improvements)
    status = "ACCEPTED" if accepted else "REJECTED"
    if checkpoint is not None:
        checkpoint.transition(status, {"correctness_improvements": correctness_improvements,
                                       "paired_comparison": comparison,
                                       "reason": None if accepted else
                                       "no correctness improvement; secondary cost/time metrics are unavailable or unqualified"})
    return {"status": status, "candidate": candidate, "branch": branch.stdout.strip(),
            "binary_sha256": binary_digest, "suite_sha256": suite_digest, "patch_sha256": patch_digest,
            "baseline_candidate": baseline_candidate, "baseline_manifests_unchanged": True,
            "gates": gates, "evaluation_exit": evaluated.returncode,
            "paired_comparison": comparison, "correctness_improvements": correctness_improvements,
            "compile_correction_retries": retry_count,
            "wall_time_seconds": time.monotonic() - started,
            "accepted": accepted, "next_gate": None if accepted else "propose a candidate with a verified correctness improvement"}


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
        decoded = [json.loads(line) for line in events_path.read_text(encoding="utf-8").splitlines()]
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
        if pointer != decoded[-1] or decoded[0].get("kind") != "run_started":
            raise ValueError("optimization pointer does not match its append-only event tail")
        stored_inputs = decoded[0].get("data", {}).get("inputs")
        digest = hashlib.sha256(json.dumps(inputs, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        if stored_inputs != inputs or decoded[0].get("inputs_sha256") != digest:
            raise ValueError("optimization inputs changed; refusing resume")
        instance = cls.__new__(cls)
        instance.path, instance.events_path, instance.inputs = path, events_path, inputs
        instance.inputs_sha256, instance.run_id = digest, decoded[0]["data"]["run_id"]
        instance.last_hash, instance.sequence, instance.events = previous, len(decoded), decoded
        if decoded[-1]["kind"] == "run_finished":
            raise ValueError("optimization checkpoint is already finished")
        return instance

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
                  proposer_executable: str | None = None) -> dict[str, Any]:
    """Schedule assess/propose/build/retest iterations from one frozen baseline."""
    from . import ledger

    if not prompt.strip() or len(prompt.encode()) > 8192:
        raise ValueError("optimization requires a non-empty assessment prompt no larger than 8 KiB")
    if not allowed_prefixes or not 1 <= max_iterations <= 20 or max_candidate_retries < 0:
        raise ValueError("optimization needs an allowlist, 1-20 iterations, and non-negative retries")
    if max_wallclock <= 0 or timeout <= 0:
        raise ValueError("optimization time budgets must be positive")
    repository = repository.expanduser().resolve(strict=True)
    suite = suite.expanduser().resolve(strict=True)
    ledger_root = ledger_root.expanduser().resolve(strict=True)
    candidate_root = candidate_root.expanduser().resolve()
    target_root = target_root.expanduser().resolve()
    if candidate_root.is_relative_to(repository) or target_root.is_relative_to(repository):
        raise ValueError("candidate worktrees and build outputs must be outside the source repository")
    baseline_rows = [row for row in ledger.load_runs(ledger_root) if row["candidate"] == baseline_candidate]
    if not baseline_rows or not baseline_rows[0].get("subject", {}).get("revision"):
        raise ValueError("optimization requires a frozen baseline with source revision receipts")
    base_revision = baseline_rows[0]["subject"]["revision"]
    suite_digest = hashlib.sha256(suite.read_bytes()).hexdigest()
    inputs = {"base_revision": base_revision, "suite_sha256": suite_digest,
              "baseline_candidate": baseline_candidate, "max_iterations": max_iterations,
              "max_candidate_retries": max_candidate_retries,
              "allowed_prefixes": list(allowed_prefixes), "prompt_sha256": hashlib.sha256(prompt.encode()).hexdigest()}
    if resume:
        try:
            stored_inputs = json.loads(checkpoint_path.read_text(encoding="utf-8"))["data"]["inputs"]
        except (OSError, KeyError, json.JSONDecodeError) as error:
            raise ValueError("optimization checkpoint does not contain resumable immutable inputs") from error
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
    accepted = next((item for item in results if item.get("result", {}).get("accepted")), None)
    if accepted:
        return {"status": "ACCEPTED", "iterations": results, "checkpoint": str(checkpoint_path)}
    first_iteration = active["iteration"] if active else (max(done, default=0) + 1)
    candidate_root.mkdir(parents=True, exist_ok=True)
    for iteration in range(first_iteration, max_iterations + 1):
        remaining = float(inputs["deadline_unix"]) - time.time()
        if remaining <= 0:
            state.record("run_interrupted", {"reason": "wallclock_budget_exhausted", "iterations_finished": len(done)})
            return {"status": "INTERRUPTED", "iterations": results, "checkpoint": str(checkpoint_path)}
        candidate = active["candidate"] if active and iteration == first_iteration else f"{state.run_id}-i{iteration:02d}"
        worktree = Path(active["worktree"]) if active and iteration == first_iteration else candidate_root / candidate
        child_path = Path(active["candidate_checkpoint"]) if active and iteration == first_iteration else checkpoint_path.with_name(f"{checkpoint_path.stem}-{candidate}.json")
        if not active:
            state.record("iteration_started", {"iteration": iteration, "candidate": candidate,
                                                "worktree": str(worktree), "candidate_checkpoint": str(child_path)})
        if not worktree.exists():
            branch = f"harness/{state.run_id}-i{iteration:02d}"
            create_candidate_worktree(repository, branch, worktree, base_revision)
        child = Checkpoint.resume(child_path, base_revision, suite_digest) if child_path.exists() else Checkpoint(child_path, base_revision, suite_digest)
        try:
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
                proposal = opencode_propose(worktree, iteration_prompt, allowed_prefixes,
                                            min(timeout, remaining), proposer_executable)
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
                                       max_candidate_retries=max_candidate_retries)
            row = {"iteration": iteration, "candidate": candidate, "result": result}
            state.record("iteration_finished", row)
            results.append(row)
            done[iteration] = row
            active = None
            if result.get("accepted"):
                state.record("run_finished", {"status": "ACCEPTED", "iteration": iteration,
                                               "candidate": candidate, "patch_sha256": result["patch_sha256"]})
                return {"status": "ACCEPTED", "iterations": results, "checkpoint": str(checkpoint_path)}
        except (ValueError, RuntimeError, TimeoutError, OSError) as error:
            state.record("iteration_interrupted", {"iteration": iteration, "candidate": candidate,
                                                    "failure_class": type(error).__name__})
            raise
    state.record("run_finished", {"status": "NO_IMPROVEMENT", "iterations": len(results)})
    return {"status": "NO_IMPROVEMENT", "iterations": results, "checkpoint": str(checkpoint_path)}
