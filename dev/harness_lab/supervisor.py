from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import selectors
import shutil
import signal
import subprocess
import tempfile
import time
from typing import Any, Optional
import uuid


MAX_FRAME_BYTES = 1024 * 1024
LOG_TAIL_BYTES = 64 * 1024

#: Stable, secret-safe failure categories the native ACP adapter may report on
#: the `-32010` error payload. Mirrors `TurnFailureCategory` in
#: `crates/codegg-protocol/src/core.rs`. A value outside this set is NOT
#: coerced into a known category: an unrecognized category is recorded as
#: absent rather than silently relabelled as `native_failure_unspecified`.
FAILURE_CATEGORIES = (
    "provider_transport",
    "provider_auth",
    "provider_rate_limit",
    "provider_timeout",
    "provider_api",
    "cancelled",
    "daemon_restarted",
    "agent_internal",
    "native_failure_unspecified",
)

#: Why provider usage is present or absent on a terminal event. Mirrors
#: `UsageAvailability` in `crates/codegg-protocol/src/core.rs`.
USAGE_AVAILABILITY = (
    "reported",
    "not_observed",
    "not_provided",
    "lost_at_projection",
    "unspecified",
)


class HarnessError(RuntimeError):
    pass


class HarnessTimeout(HarnessError):
    pass


class HarnessTurnFailure(HarnessError):
    """A native turn reached a failure terminal.

    Carries the redacted failure evidence projected by the native adapter on
    the `-32010` JSON-RPC error `data` payload. The evidence is deliberately
    narrow: an allowlisted category and provider usage with an explicit
    provenance. It never carries the provider's error text.
    """

    def __init__(self, message: str, *, category: Optional[str] = None,
                 usage: Optional[dict[str, Any]] = None,
                 usage_reason: Optional[str] = None) -> None:
        super().__init__(message)
        self.category = category
        self.usage = usage
        self.usage_reason = usage_reason

    @property
    def usage_unavailable_reason(self) -> Optional[str]:
        """Named reason the provider usage is absent, or ``None`` when measured.

        A missing measurement is never reported as zero usage: when the native
        side could not observe usage, this names why.
        """
        if self.usage is not None:
            return None
        return self.usage_reason or "unspecified"


def _optional_token_count(value: Any) -> Optional[int]:
    """Return a provider token count, keeping "absent" distinct from zero."""
    if isinstance(value, bool) or not isinstance(value, int):
        return None
    return value if value >= 0 else None


def decode_turn_failure(data: Any) -> HarnessTurnFailure:
    """Decode the native `-32010` error `data` payload into redacted evidence.

    Unknown or malformed fields degrade to "unknown" rather than being
    coerced into a known category or a zero token count.
    """
    payload = data if isinstance(data, dict) else {}
    raw_category = payload.get("codeggFailureCategory")
    category = raw_category if raw_category in FAILURE_CATEGORIES else None

    raw_usage = payload.get("codeggUsage")
    usage: Optional[dict[str, Any]] = None
    if isinstance(raw_usage, dict):
        usage = {
            "turn_id": payload.get("turnId") if isinstance(payload.get("turnId"), str) else None,
            "input_tokens": _optional_token_count(raw_usage.get("inputTokens")),
            "output_tokens": _optional_token_count(raw_usage.get("outputTokens")),
            "cached_tokens": _optional_token_count(raw_usage.get("cachedTokens")),
            "reasoning_tokens": _optional_token_count(raw_usage.get("reasoningTokens")),
        }

    raw_reason = payload.get("codeggUsageReason")
    usage_reason = raw_reason if raw_reason in USAGE_AVAILABILITY else None
    if usage is not None and usage_reason is None:
        usage_reason = "reported"
    if usage is None and usage_reason is None:
        usage_reason = "unspecified"
    return HarnessTurnFailure(
        "ACP reported a terminal native turn failure",
        category=category, usage=usage, usage_reason=usage_reason)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


#: The platform limit on a bound Unix socket path.
#:
#: ``sun_path`` is 104 bytes on macOS and 108 on Linux. The daemon binds its
#: control socket at ``<lane>/daemon/core.sock``, so a lane root whose full
#: socket path exceeds the limit cannot start a daemon at all. macOS's
#: per-user temporary directory is long enough on its own to break that, so
#: every lane is placed under a short root instead.
SUN_PATH_LIMIT = 96


def lane_root(name: str) -> Path:
    """Return a private lane root whose control socket path fits the platform.

    The lane lives under the user's home so the path stays short and contains
    no symlinked component: the daemon refuses a managed scratch path whose
    components are symlinks, and ``/var`` is a symlink on macOS.
    """
    root = Path.home() / ".codegg-harness-lanes" / name
    if len(str(root / "daemon" / "core.sock").encode()) >= SUN_PATH_LIMIT:
        root = Path("/tmp") / name
    return root


@dataclass(frozen=True)
class SubjectSpec:
    lane_id: str
    binary: Path
    binary_sha256: str
    revision: str
    workspace: Path
    root: Path

    @classmethod
    def create(cls, lane_id: str, binary: Path, workspace: Path, root: Path, revision_from: Path) -> "SubjectSpec":
        return cls(lane_id, binary, sha256_file(binary), _git_revision(revision_from), workspace, root)

    def environment(self) -> dict[str, str]:
        home = self.root / "home"
        env = os.environ.copy()
        # Remove ambient CodeGG routing/config overrides before adding the
        # lane's private roots. Retain provider variables only when the caller
        # explicitly opts into a model-backed experiment (not used by smoke).
        for key in ("CODEGG_DAEMON_HOME", "CODEGG_DATA_HOME", "CODEGG_TUI_CONFIG", "CODEGG_CORE_ENDPOINT"):
            env.pop(key, None)
        for key in ("OPENAI_API_KEY", "ANTHROPIC_API_KEY", "GOOGLE_API_KEY", "OPENROUTER_API_KEY", "OPENCODE_ZEN_API_KEY", "MISTRAL_API_KEY", "GROQ_API_KEY", "DEEPINFRA_API_KEY", "CEREBRAS_API_KEY", "COHERE_API_KEY", "TOGETHERAI_API_KEY", "PERPLEXITY_API_KEY", "XAI_API_KEY", "VENICE_API_KEY", "MINIMAX_API_KEY", "OPENCODE_GO_API_KEY", "GENERALCOMPUTE_API_KEY"):
            env.pop(key, None)
        env.update({
            "HOME": str(home),
            "USERPROFILE": str(home),
            "XDG_CONFIG_HOME": str(self.root / "config"),
            "XDG_DATA_HOME": str(self.root / "xdg-data"),
            "XDG_CACHE_HOME": str(self.root / "cache"),
            "XDG_STATE_HOME": str(self.root / "state"),
            "XDG_RUNTIME_DIR": str(self.root / "runtime"),
            "CODEGG_DAEMON_HOME": str(self.root / "daemon"),
            "CODEGG_DATA_HOME": str(self.root / "data"),
            "CODEGG_TUI_CONFIG": str(self.root / "config" / "codegg" / "codegg.jsonc"),
            "CARGO_HOME": str(self.root / "cargo-home"),
            "TMPDIR": str(self.root / "scratch"),
            "TMP": str(self.root / "scratch"),
            "TEMP": str(self.root / "scratch"),
            "RUST_LOG": "error",
        })
        for key in ("config", "codegg", "daemon", "data", "home", "xdg-data", "cache", "state", "runtime", "scratch", "artifacts", "cargo-home"):
            (self.root / key).mkdir(parents=True, exist_ok=True)
        return env


class AcpClient:
    """Small bounded JSON-RPC v2 client for lifecycle-only ACP smoke tests."""

    def __init__(self, binary: Path, env: dict[str, str], cwd: Path, timeout: float, unattended_profile: str | None = None) -> None:
        self.timeout = timeout
        self._started_monotonic = time.monotonic()
        command = [str(binary), "acp"]
        if unattended_profile is not None:
            command.extend(["--approval-mode", "yolo", "--sandbox", unattended_profile])
        self.process = subprocess.Popen(
            command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, cwd=cwd, env=env, start_new_session=(os.name == "posix"),
        )
        assert self.process.stdin and self.process.stdout
        self._selector = selectors.DefaultSelector()
        self._selector.register(self.process.stdout, selectors.EVENT_READ)
        self._stdout_buffer = bytearray()
        self._next_id = 1
        self._stdin_lock = __import__("threading").Lock()
        self.notifications: list[dict[str, Any]] = []
        self.frame_summaries: list[dict[str, Any]] = []
        self.stderr_path = Path(env["CODEGG_DAEMON_HOME"]).parent / "acp.stderr"
        self._stderr_file = self.stderr_path.open("wb")
        self._stderr_file.write(b"")
        self._stderr_file.flush()
        self._stderr_thread = __import__("threading").Thread(target=self._capture_stderr, daemon=True)
        self._stderr_thread.start()

    def _capture_stderr(self) -> None:
        assert self.process.stderr
        tail = bytearray()
        while chunk := self.process.stderr.read(8192):
            tail.extend(chunk)
            if len(tail) > LOG_TAIL_BYTES:
                del tail[:-LOG_TAIL_BYTES]
        self._stderr_file.write(tail)
        self._stderr_file.close()

    def request(self, method: str, params: dict[str, Any], timeout: float | None = None) -> dict[str, Any]:
        assert self.process.stdin
        request_id = self._next_id
        self._next_id += 1
        frame = json.dumps({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}, separators=(",", ":")).encode() + b"\n"
        if len(frame) > MAX_FRAME_BYTES:
            raise HarnessError("ACP request exceeds frame limit")
        with self._stdin_lock:
            self.process.stdin.write(frame)
            self.process.stdin.flush()
        deadline = time.monotonic() + (self.timeout if timeout is None else timeout)
        while True:
            newline = self._stdout_buffer.find(b"\n")
            while newline < 0:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise HarnessTimeout(f"ACP request timed out: {method}")
                if not self._selector.select(remaining):
                    raise HarnessTimeout(f"ACP request timed out: {method}")
                chunk = os.read(self.process.stdout.fileno(), 65536)  # type: ignore[union-attr]
                if not chunk:
                    raise HarnessError(f"ACP exited while waiting for {method} (status {self.process.poll()})")
                self._stdout_buffer.extend(chunk)
                if len(self._stdout_buffer) > MAX_FRAME_BYTES + 1 and b"\n" not in self._stdout_buffer:
                    raise HarnessError("ACP response exceeds frame limit")
                newline = self._stdout_buffer.find(b"\n")
            line = bytes(self._stdout_buffer[:newline])
            del self._stdout_buffer[:newline + 1]
            if len(line) > MAX_FRAME_BYTES:
                raise HarnessError("ACP response exceeds frame limit")
            try:
                value = json.loads(line)
            except (UnicodeDecodeError, json.JSONDecodeError) as error:
                raise HarnessError(f"ACP emitted invalid JSON-RPC: {error}") from error
            if value.get("id") != request_id:
                if "method" in value and "id" not in value:
                    self._absorb_frame(value)
                continue
            if "error" in value:
                error = value.get("error", {})
                if isinstance(error, dict) and error.get("code") == -32010:
                    raise decode_turn_failure(error.get("data"))
                raise HarnessError(f"ACP {method} failed: {value['error']}")
            self.frame_summaries.append({"response_id": value.get("id"), "method": method, "result_keys": sorted(value.get("result", {}).keys()) if isinstance(value.get("result"), dict) else []})
            return value.get("result", {})

    def _absorb_frame(self, value: dict[str, Any]) -> None:
        """Absorb one non-response JSON-RPC frame as a notification."""
        value["_received_elapsed_ms"] = int((time.monotonic() - self._started_monotonic) * 1000)
        self.notifications.append(value)
        params = value.get("params", {})
        update = params.get("update", {}) if isinstance(params, dict) else {}
        self.frame_summaries.append({"method": value.get("method"),
                                     "sessionUpdate": update.get("sessionUpdate") if isinstance(update, dict) else None})

    def drain(self, timeout: float = 1.0) -> int:
        """Read every notification that is already in flight, then stop.

        A tool call is reported by two notifications: the ``tool_call`` start
        and the ``tool_call_update`` finish. The ACP server can deliver the
        last of them *after* the ``session/prompt`` response, so counting the
        notification stream the moment the response arrives can miss the tail
        of the turn. A trial that counted fewer tool calls than the agent
        actually made read as a stalled turn, and the same trial could read
        differently on the next run — the count depended on how the two
        streams interleaved.

        The drain reads until the stream is quiet for a short idle window, so
        the counts describe the turn rather than the scheduling.
        """
        assert self.process.stdout
        deadline = time.monotonic() + timeout
        absorbed = 0
        while time.monotonic() < deadline:
            line = self._read_line(deadline)
            if line is None:
                break
            try:
                value = json.loads(line)
            except (UnicodeDecodeError, json.JSONDecodeError):
                continue
            if "method" in value and "id" not in value:
                self._absorb_frame(value)
                absorbed += 1
        return absorbed

    def _read_line(self, deadline: float) -> bytes | None:
        """Read one newline-terminated frame, or None when the stream is idle."""
        assert self.process.stdout
        while True:
            newline = self._stdout_buffer.find(b"\n")
            if newline >= 0:
                line = bytes(self._stdout_buffer[:newline])
                del self._stdout_buffer[:newline + 1]
                return line
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return None
            if not self._selector.select(min(remaining, 0.05)):
                # The stream went quiet: the turn is over as far as the
                # notification stream is concerned.
                return None
            chunk = os.read(self.process.stdout.fileno(), 65536)  # type: ignore[union-attr]
            if not chunk:
                return None
            self._stdout_buffer.extend(chunk)

    def notify(self, method: str, params: dict[str, Any]) -> None:
        assert self.process.stdin
        frame = json.dumps({"jsonrpc": "2.0", "method": method, "params": params}, separators=(",", ":")).encode() + b"\n"
        if len(frame) > MAX_FRAME_BYTES:
            raise HarnessError("ACP notification exceeds frame limit")
        with self._stdin_lock:
            self.process.stdin.write(frame)
            self.process.stdin.flush()

    def close(self) -> None:
        if self.process.poll() is None:
            try:
                self.request("shutdown", {}, timeout=min(self.timeout, 3.0))
            except HarnessError:
                pass
            if self.process.poll() is None:
                self.process.terminate()
                try:
                    self.process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    _signal_owned_group(self.process, signal.SIGKILL)
                    self.process.wait(timeout=2)
        self._selector.close()
        self._stderr_thread.join(timeout=1)
        # The child's pipes are file objects of this process: leaving them open
        # leaks a descriptor per trial and reports a ResourceWarning.
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            try:
                if stream is not None:
                    stream.close()
            except OSError:
                pass


def _signal_owned_group(process: subprocess.Popen[bytes], sig: int) -> None:
    if process.poll() is not None:
        return
    if os.name == "posix":
        try:
            os.killpg(process.pid, sig)
        except ProcessLookupError:
            pass
    else:
        process.kill()


def _git_revision(workspace: Path) -> str:
    result = subprocess.run(["git", "-C", str(workspace), "rev-parse", "HEAD"], capture_output=True, text=True, timeout=5)
    return result.stdout.strip() if result.returncode == 0 else "unknown"


def _git_root(path: Path) -> Path | None:
    result = subprocess.run(["git", "-C", str(path), "rev-parse", "--show-toplevel"], capture_output=True, text=True, timeout=5)
    return Path(result.stdout.strip()).resolve() if result.returncode == 0 else None


def _run(binary: Path, args: list[str], env: dict[str, str], timeout: float) -> str:
    result = subprocess.run([str(binary), *args], env=env, capture_output=True, timeout=timeout, text=True)
    if result.returncode:
        tail = result.stderr[-LOG_TAIL_BYTES:]
        raise HarnessError(f"{args!r} failed with {result.returncode}: {tail}")
    return result.stdout


def _metadata(spec: SubjectSpec, timeout: float) -> dict[str, Any]:
    path = spec.root / "daemon" / "daemon.json"
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            value = json.loads(path.read_text())
            if isinstance(value, dict) and value.get("daemon_id") and value.get("generation"):
                return value
        except (FileNotFoundError, json.JSONDecodeError, OSError):
            pass
        time.sleep(0.05)
    raise HarnessError(f"lane {spec.lane_id} did not publish daemon identity metadata")


def _live_daemon_id(spec: SubjectSpec, timeout: float) -> str:
    status = _run(spec.binary, ["daemon", "status"], spec.environment(), timeout)
    match = re.search(r"^\s*Daemon ID:\s*(\S+)\s*$", status, re.MULTILINE)
    if not match:
        raise HarnessError(f"lane {spec.lane_id} status did not provide a live daemon ID")
    return match.group(1)


def _daemon_executable(pid: int) -> Path | None:
    if platform.system() == "Linux":
        try:
            return Path(f"/proc/{pid}/exe").resolve()
        except OSError:
            return None
    if platform.system() == "Darwin":
        result = subprocess.run(["ps", "-p", str(pid), "-o", "command="], capture_output=True, text=True, timeout=3)
        if result.returncode == 0:
            command = result.stdout.strip().split(maxsplit=1)[0]
            return Path(command).resolve()
    return None


def _daemon_command_line(pid: int) -> str | None:
    if platform.system() == "Linux":
        try:
            return Path(f"/proc/{pid}/cmdline").read_bytes().replace(b"\0", b" ").decode(errors="replace")
        except OSError:
            return None
    if platform.system() == "Darwin":
        result = subprocess.run(["ps", "-p", str(pid), "-o", "command="], capture_output=True, text=True, timeout=3)
        return result.stdout.strip() if result.returncode == 0 else None
    return None


def _owned_daemon_pids(spec: SubjectSpec) -> list[int]:
    expected_root = str((spec.root / "daemon").resolve())
    candidates: list[int] = []
    if platform.system() == "Linux":
        for entry in Path("/proc").iterdir():
            if not entry.name.isdigit():
                continue
            pid = int(entry.name)
            command = _daemon_command_line(pid)
            if command and expected_root in command and _daemon_executable(pid) == spec.binary.resolve():
                candidates.append(pid)
    elif platform.system() == "Darwin":
        result = subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True, timeout=3)
        for line in result.stdout.splitlines() if result.returncode == 0 else []:
            pid_text, _, command = line.strip().partition(" ")
            if pid_text.isdigit() and expected_root in command and _daemon_executable(int(pid_text)) == spec.binary.resolve():
                candidates.append(int(pid_text))
    return candidates


def _terminate_verified_daemon(spec: SubjectSpec, pid: int) -> bool:
    expected_root = str((spec.root / "daemon").resolve())
    executable = _daemon_executable(pid)
    command = _daemon_command_line(pid)
    if executable != spec.binary.resolve() or not command or expected_root not in command:
        return False
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        return True
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        if _daemon_executable(pid) != spec.binary.resolve():
            return True
        time.sleep(0.05)
    # Revalidate executable and lane-specific endpoint immediately before force.
    if _daemon_executable(pid) != spec.binary.resolve() or expected_root not in (_daemon_command_line(pid) or ""):
        return False
    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    return True


def _assert_daemon_identity(spec: SubjectSpec, timeout: float, acp_pid: int | None = None) -> dict[str, Any]:
    metadata = _metadata(spec, timeout)
    live_id = _live_daemon_id(spec, timeout)
    if live_id != metadata["daemon_id"]:
        raise HarnessError(f"lane {spec.lane_id} live daemon ID disagrees with metadata")
    pid = int(metadata["pid"])
    process_binary = _daemon_executable(pid)
    if process_binary is None or process_binary != spec.binary.resolve():
        raise HarnessError(f"lane {spec.lane_id} cannot prove daemon PID {pid} runs the pinned binary")
    if sha256_file(spec.binary) != spec.binary_sha256:
        raise HarnessError(f"lane {spec.lane_id} candidate binary changed during trial")
    return {
        "lane_id": spec.lane_id,
        "daemon_id": live_id,
        "generation": metadata["generation"],
        "pid": pid,
        "acp_pid": acp_pid,
        "started_at": metadata.get("started_at"),
        "socket_path": metadata.get("socket_path"),
        "endpoint_uri": metadata.get("endpoint_uri"),
        "binary": str(spec.binary),
        "binary_sha256": spec.binary_sha256,
        "revision": spec.revision,
        "workspace_root": str(spec.workspace),
        "daemon_root": str(spec.root / "daemon"),
        "data_root": str(spec.root / "data" / "codegg"),
        "config_root": str(spec.root / "config" / "codegg"),
        "environment_fingerprint": hashlib.sha256(json.dumps({key: value for key, value in spec.environment().items() if key in {
            "CODEGG_DAEMON_HOME", "CODEGG_DATA_HOME", "CODEGG_TUI_CONFIG", "HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "XDG_RUNTIME_DIR", "TMPDIR", "CARGO_HOME"
        }}, sort_keys=True).encode()).hexdigest(),
    }


def _stop_daemon(spec: SubjectSpec, timeout: float) -> None:
    env = spec.environment()
    try:
        metadata = _metadata(spec, timeout)
    except (HarnessError, OSError):
        owned = _owned_daemon_pids(spec)
        if owned and all(_terminate_verified_daemon(spec, pid) for pid in owned):
            return
        raise HarnessError(f"lane {spec.lane_id} startup metadata was unavailable; refusing unverified cleanup")
    pid = int(metadata["pid"])
    try:
        live_id = _live_daemon_id(spec, timeout)
    except (HarnessError, subprocess.TimeoutExpired, OSError):
        if _terminate_verified_daemon(spec, pid):
            return
        raise HarnessError(f"refusing to stop lane {spec.lane_id}: live identity probe failed and verified fallback was unavailable")
    if live_id != metadata["daemon_id"]:
        raise HarnessError(f"refusing to stop lane {spec.lane_id}: live daemon identity changed")
    try:
        _run(spec.binary, ["daemon", "stop"], env, timeout)
    except (HarnessError, subprocess.TimeoutExpired, OSError):
        if _terminate_verified_daemon(spec, pid):
            return
        raise HarnessError(f"lane {spec.lane_id} daemon stop failed and verified fallback was unavailable")
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        current = _daemon_executable(int(metadata["pid"]))
        if current is None or current != spec.binary.resolve():
            return
        time.sleep(0.05)
    raise HarnessError(f"lane {spec.lane_id} daemon did not exit gracefully; no unverified signal sent")


def stop_lane_daemon(spec: SubjectSpec, timeout: float = 15.0) -> None:
    """Stop the lane's daemon and refuse to leave an unverified one behind.

    A lane that leaves its daemon running keeps a socket, a lock and a process
    alive after the trial ends. The stop is verified against the lane's own
    recorded identity, so the harness never signals a process it cannot prove
    it owns.
    """
    _stop_daemon(spec, timeout)


def run_smoke(binary: Path, subjects: int = 2, timeout: float = 20.0, output_dir: Path | None = None, unattended_profile: str | None = None) -> dict[str, Any]:
    if os.name != "posix" or platform.system() not in {"Linux", "Darwin"}:
        raise HarnessError("real multi-daemon smoke currently supports macOS and Linux only")
    if subjects < 2 or subjects > 8:
        raise HarnessError("subjects must be between 2 and 8")
    binary = binary.resolve()
    owned_temp: tempfile.TemporaryDirectory[str] | None = None
    if output_dir is None:
        owned_temp = tempfile.TemporaryDirectory(prefix="codegg-harness-lab-")
        experiment_root = Path(owned_temp.name)
    else:
        experiment_root = output_dir.expanduser().resolve()
        repository = _git_root(Path.cwd())
        if repository is not None and experiment_root.is_relative_to(repository):
            raise HarnessError("experiment output must be outside the candidate worktree")
        if experiment_root.exists() and any(experiment_root.iterdir()):
            raise HarnessError(f"output directory must be empty: {experiment_root}")
        experiment_root.mkdir(parents=True, exist_ok=True)
    specs = []
    for index in range(subjects):
        workspace = (experiment_root / f"workspace-{index + 1}").resolve()
        workspace.mkdir(parents=True, exist_ok=False)
        specs.append(SubjectSpec.create(f"subject-{index + 1}", binary, workspace, experiment_root / f"lane-{index + 1}", Path.cwd()))
    clients: list[AcpClient] = []
    session_ids: list[str] = []
    identities: list[dict[str, Any]] = []
    policy_receipts: list[dict[str, Any]] = []
    default_daemon_before = _default_daemon_fingerprint(binary, timeout)
    try:
        for spec in specs:
            if (spec.root / "daemon" / "daemon.json").exists():
                raise HarnessError(f"lane root already contains daemon metadata: {spec.root}")
            client = AcpClient(binary, spec.environment(), spec.workspace, timeout, unattended_profile)
            clients.append(client)
            client.request("initialize", {"protocolVersion": 1, "clientInfo": {"name": "codegg-harness-lab", "version": "1"}})
        for spec, client in zip(specs, clients, strict=True):
            result = client.request("session/new", {"cwd": str(spec.workspace), "title": "harness-lab-isolation-smoke"})
            session_id = result.get("sessionId")
            if not isinstance(session_id, str) or not session_id:
                raise HarnessError(f"lane {spec.lane_id} ACP did not return a session ID")
            session_ids.append(session_id)
            if unattended_profile is not None:
                evidence = result.get("codeggPolicy")
                if not isinstance(evidence, dict):
                    raise HarnessError(f"lane {spec.lane_id} omitted the effective ACP policy receipt")
                requested = evidence.get("requested", {})
                effective = evidence.get("effective", {})
                if requested != {"approvalMode": "yolo", "sandbox": unattended_profile.replace("-", "_")}:
                    raise HarnessError(f"lane {spec.lane_id} policy receipt did not record the requested policy")
                if effective.get("approvalMode") != "yolo" or effective.get("sandbox") != unattended_profile.replace("-", "_"):
                    raise HarnessError(f"lane {spec.lane_id} daemon effective policy differs from the request")
                policy_receipts.append(evidence)
            identities.append(_assert_daemon_identity(spec, timeout, client.process.pid))
        for left, right in zip(identities, identities[1:]):
            if left["daemon_id"] == right["daemon_id"] or left["generation"] == right["generation"] or left["pid"] == right["pid"]:
                raise HarnessError("subject lanes attached to the same daemon identity")
            if left["data_root"] == right["data_root"] or left["config_root"] == right["config_root"]:
                raise HarnessError("subject lanes share mutable data or config roots")
        # A daemon stop in one lane must leave another lane's ACP client usable.
        _stop_daemon(specs[0], timeout)
        survivor = clients[1].request("session/new", {"cwd": str(specs[1].workspace), "title": "harness-lab-isolation-smoke"})
        survivor_id = survivor.get("sessionId")
        if not isinstance(survivor_id, str):
            raise HarnessError("surviving ACP lane failed after its peer stopped")
        session_ids[1] = survivor_id
        default_daemon_after = _default_daemon_fingerprint(binary, timeout)
        if default_daemon_before != default_daemon_after:
            raise HarnessError("default user daemon identity changed during the private-lane smoke")
        return {
            "ok": True,
            "experiment_id": str(uuid.uuid4()),
            "platform": platform.platform(),
            "subjects": identities,
            "sessions": session_ids,
            "stopped_lane": specs[0].lane_id,
            "surviving_lane": specs[1].lane_id,
            "mutable_path_inventory": ["daemon root", "SQLite data/catalog", "config/agents/credentials", "HOME", "XDG config/data/cache/state", "workspace fixture", "ACP stderr", "controller output"],
            "provider_calls": 0,
            "unattended_profile": unattended_profile,
            "policy_receipts": policy_receipts,
            "default_daemon_identity": default_daemon_before,
        }
    finally:
        for index, (spec, client) in enumerate(zip(specs, clients, strict=False)):
            if index < len(session_ids):
                try:
                    client.request("session/close", {"sessionId": session_ids[index]}, timeout=2)
                except HarnessError:
                    pass
            client.close()
            try:
                _stop_daemon(spec, min(timeout, 5))
            except (HarnessError, FileNotFoundError, OSError, subprocess.TimeoutExpired):
                pass
        orphaned = [spec.lane_id for spec in specs if _owned_daemon_pids(spec)]
        if orphaned:
            raise HarnessError(f"verified lane daemons remain after cleanup: {orphaned}")
        if owned_temp is not None:
            owned_temp.cleanup()


def run_tool_trial(binary: Path, profile: str, timeout: float = 45.0, deny_bash: bool = False,
                   ledger_root: Path | None = None, candidate: str = "current",
                   task_id: str = "safe-bash-write", group_id: str = "native-tool-smoke",
                   repeat: int = 0, suite_sha256: str | None = None,
                   case_sha256: str | None = None, content: str = "harness-lab-tool-ok",
                   expected_text: str | None = "harness-lab-tool-ok",
                   tool_sequence: list[dict[str, Any]] | None = None,
                   initial_files: dict[str, str] | None = None,
                   expected_file: str = "tool-receipt.txt",
                   expected_exists: bool | None = None, cancel_task: bool = False,
                   expected_terminal: str = "passed", timeout_task: bool = False,
                   context_text: str | None = None, expected_tool_failures: int = 0,
                   fail_after_tools: bool = False, fault_kind: str = "transport",
                   agent_steps: int | None = None) -> dict[str, Any]:
    """Run one native ACP turn against a local deterministic OpenAI wire fixture."""
    from .provider_fixture import FixtureProvider
    import shlex
    if profile not in {"full-host", "workspace-write"}:
        raise HarnessError("profile must be full-host or workspace-write")
    trial_started = time.monotonic()
    binary = binary.resolve()
    with tempfile.TemporaryDirectory(prefix="codegg-harness-tool-"):
        root = lane_root("tool-" + uuid.uuid4().hex[:8])
        root.mkdir(parents=True, exist_ok=True)
        workspace = root / "workspace"
        workspace.mkdir()
        target = (workspace / expected_file).resolve(strict=False)
        if not target.is_relative_to(workspace.resolve()):
            raise HarnessError("fixture verifier path escapes its private workspace")
        for relative, value in (initial_files or {}).items():
            path = (workspace / relative).resolve(strict=False)
            if not path.is_relative_to(workspace.resolve()) or not isinstance(value, str) or len(value.encode()) > 65536:
                raise HarnessError("fixture initial file path or content is invalid")
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(value, encoding="utf-8")
        if tool_sequence is None:
            tool_name = "bash" if deny_bash else "write"
            arguments = {"command": f"printf {shlex.quote(content)} > {shlex.quote(str(target))}"} if deny_bash else {"path": expected_file, "content": content}
            tool_sequence = [{"name": tool_name, "arguments": arguments}]
        if not isinstance(tool_sequence, list) or not 1 <= len(tool_sequence) <= 8:
            raise HarnessError("fixture tool sequence must contain 1-8 calls")
        def bind_workspace(value: Any) -> Any:
            if isinstance(value, str):
                return value.replace("{workspace}", str(workspace))
            if isinstance(value, list):
                return [bind_workspace(item) for item in value]
            if isinstance(value, dict):
                return {key: bind_workspace(item) for key, item in value.items()}
            return value
        tool_sequence = bind_workspace(tool_sequence)
        provider = FixtureProvider(tool_sequence, stall_after_tools=cancel_task or timeout_task,
                                   require_context_text=context_text,
                                   fail_after_tools=fail_after_tools,
                                   fault_kind=fault_kind).start()
        spec = SubjectSpec.create("tool-trial", binary, workspace, root, Path.cwd())
        env = spec.environment()
        config = {
            "provider": {"openai": {
                "api_key": "local-fixture-only",
                "base_url": provider.base_url,
                "models": {"harness-fixture": {"name": "Harness Fixture"}},
            }},
            "permission": {"bash": "deny" if deny_bash else "ask", "bash_ask_patterns": ["*"]},
            "agent": {"build": {"permission": {"write": "ask"}}},
        }
        if agent_steps is not None:
            # The hard turn budget, made reachable inside one scripted trial.
            # ADR-0040 Option B bounds a repeated cycle with the run/turn
            # limits, not with a recovery stall, so a trial that must observe
            # that bound has to set it explicitly.
            if isinstance(agent_steps, bool) or not isinstance(agent_steps, int) or agent_steps < 1:
                raise HarnessError("agent step budget must be a positive integer")
            config["agent"]["build"]["steps"] = int(agent_steps)
        config_path = Path(env["CODEGG_TUI_CONFIG"])
        config_path.parent.mkdir(parents=True, exist_ok=True)
        config_path.write_text(json.dumps(config), encoding="utf-8")
        client = AcpClient(binary, env, workspace, timeout, profile)
        session_id = None
        try:
            client.request("initialize", {"protocolVersion": 1, "clientInfo": {"name": "codegg-harness-lab", "version": "1"}})
            opened = client.request("session/new", {"cwd": str(workspace), "title": "harness-lab-native-tool-fixture"})
            session_id = opened.get("sessionId")
            if not isinstance(session_id, str):
                raise HarnessError("ACP fixture did not create a session")
            receipt = opened.get("codeggPolicy")
            if not isinstance(receipt, dict) or receipt.get("effective", {}).get("sandbox") != profile.replace("-", "_"):
                raise HarnessError("native fixture policy receipt did not match the requested profile")
            prompt_params = {
                "sessionId": session_id,
                "model": "openai/harness-fixture",
                "codeggAgent": "build",
                "prompt": [{"type": "text", "text": f"Complete this deterministic local task using the supplied tool sequence: {task_id}."}],
            }
            if cancel_task:
                prompt_result: dict[str, Any] = {}
                prompt_error: list[BaseException] = []
                def submit_prompt() -> None:
                    try:
                        prompt_result.update(client.request("session/prompt", prompt_params, timeout=timeout))
                    except BaseException as error:
                        prompt_error.append(error)
                prompt_thread = __import__("threading").Thread(target=submit_prompt, daemon=True)
                prompt_thread.start()
                if not provider.stalled.wait(min(timeout, 15)):
                    raise HarnessError("provider fixture did not reach the cancellation gate")
                client.notify("session/cancel", {"sessionId": session_id})
                provider.release_stall.set()
                prompt_thread.join(timeout=min(timeout, 15))
                if prompt_thread.is_alive():
                    raise HarnessError("ACP prompt did not stop after session/cancel")
                if prompt_error:
                    if isinstance(prompt_error[0], HarnessTurnFailure):
                        result = {"stopReason": "cancelled", "turnError": type(prompt_error[0]).__name__}
                    else:
                        raise HarnessError(f"ACP cancellation response failed: {type(prompt_error[0]).__name__}")
                else:
                    result = prompt_result
            else:
                result = client.request("session/prompt", prompt_params, timeout=timeout)
            # The tool notifications that close the turn can arrive after the
            # prompt response. Counting them before the stream is quiet made
            # the same trial report a different number of tool calls from run
            # to run, and the terminal it recorded followed from that count.
            client.drain(timeout=min(2.0, max(0.2, timeout / 10)))
            updates = [item for item in client.notifications if item.get("method") == "session/update"]
            tool_updates = [item.get("params", {}).get("update", {}) for item in updates]
            starts = [item for item in tool_updates if item.get("sessionUpdate") == "tool_call"]
            finishes = [item for item in tool_updates if item.get("sessionUpdate") == "tool_call_update"]
            outcomes = [{"status": item.get("status"), "raw_output_bytes": len(str(item.get("rawOutput", "")).encode())}
                        for item in finishes]
            raw_usage = result.get("codeggUsage")
            usage = ({"turn_id": raw_usage.get("turnId"),
                      "input_tokens": raw_usage.get("inputTokens"),
                      "output_tokens": raw_usage.get("outputTokens"),
                      "cached_tokens": raw_usage.get("cachedTokens"),
                      "reasoning_tokens": raw_usage.get("reasoningTokens")}
                     if isinstance(raw_usage, dict) else None)
            failures = sum(item.get("status") == "failed" or
                           "Tool error:" in str(item.get("rawOutput", "")) or
                           re.search(r"\[exit code: ([1-9][0-9]*)\]", str(item.get("rawOutput", ""))) is not None
                           for item in finishes)
            if any(item.get("method") == "session/request_permission" for item in client.notifications):
                raise HarnessError("ACP emitted an interactive permission request during unattended trial")
            observed_exists = target.is_file()
            expected_exists = (not deny_bash) if expected_exists is None else expected_exists
            exists = observed_exists and (expected_text is None or target.read_text(encoding="utf-8") == expected_text)
            if deny_bash:
                denial = any("den" in str(item.get("rawOutput", "")).lower() or "permission" in str(item.get("rawOutput", "")).lower() for item in finishes)
                if observed_exists or not starts or not denial or failures != expected_tool_failures:
                    raise HarnessError("explicit bash deny did not block the native tool call")
            failure_count_matched = (failures >= expected_tool_failures if expected_terminal == "execution_failed"
                                     else failures == expected_tool_failures)
            bounded_stall = (expected_terminal == "execution_failed" and failure_count_matched
                             and len(finishes) < len(tool_sequence))
            if not deny_bash and (not starts or (len(finishes) < len(tool_sequence) and not bounded_stall)
                                  or not failure_count_matched):
                raise HarnessError(f"native broker did not complete the deterministic tool call; stop_reason={result.get('stopReason')!r}; started={len(starts)} finished={len(finishes)} failures={failures}; tool_updates={tool_updates[-6:]!r}; receipt_exists={exists}")
            observed_terminal = result.get("stopReason")
            accepted_stops = {"cancelled", "canceled"} if expected_terminal == "cancelled" else {"end_turn"}
            if observed_terminal not in accepted_stops:
                raise HarnessError(f"fixture turn ended with unexpected stop reason {observed_terminal!r}")
            context_recovered = any(item.get("context_read_used") and item.get("context_recovered")
                                    for item in provider.request_summaries)
            if context_text is not None and not context_recovered:
                raise HarnessError("fixture did not recover projected output through context_read")
            result = {
                "ok": True,
                "task_id": task_id,
                "repeat": repeat,
                "profile": profile,
                "deny_bash": deny_bash,
                "provider": "local-openai-fixture",
                "model": "openai/harness-fixture",
                "agent": "build",
                "tool_surface_sha256": __import__("hashlib").sha256(
                    json.dumps(provider.request_summaries[0]["tool_names"], separators=(",", ":")).encode()
                ).hexdigest() if provider.request_summaries else None,
                "context_read_used": context_recovered,
                "session_id": session_id,
                "daemon": _assert_daemon_identity(spec, timeout, client.process.pid),
                "policy": receipt,
                "tool_started": len(starts),
                "tool_finished": len(finishes),
                "tool_failures": failures,
                "tool_outcomes": outcomes,
                "tool_selection": [item.get("title") for item in starts],
                "codegg_usage": usage,
                "tool_receipt_created": observed_exists,
                "provider_calls": provider.calls,
                "wall_time_ms": int((time.monotonic() - trial_started) * 1000),
                "stop_reason": observed_terminal,
                "permission_request_notifications": 0,
                "workspace": str(workspace),
                "expected_file": expected_file,
            }
            if ledger_root is not None:
                from .ledger import record
                trace = []
                for notification in client.notifications:
                    update = notification.get("params", {}).get("update", {})
                    event_kind = update.get("sessionUpdate") if isinstance(update, dict) else None
                    if event_kind == "tool_call":
                        trace.append({"kind": "tool_started",
                                      "tool": update.get("title"),
                                      "arguments_sha256": update.get("codeggArgumentsSha256"),
                                      "elapsed_ms": notification.get("_received_elapsed_ms")})
                    elif event_kind == "tool_call_update":
                        trace.append({"kind": "tool_finished", "tool": update.get("title"),
                                      "status": update.get("status"),
                                      "elapsed_ms": notification.get("_received_elapsed_ms")})
                trace.append({"kind": "turn_terminal", "status": result.get("stopReason")})
                result["ledger_manifest"] = record(
                    ledger_root, result, task_id=task_id, group_id=group_id,
                    candidate=candidate, workspace=workspace, expected_file=target.name,
                    expected_text=expected_text if not deny_bash else None,
                    expected_exists=expected_exists,
                    timeout_seconds=timeout, repeat=repeat, suite_sha256=suite_sha256,
                    case_sha256=case_sha256, trace=trace,
                    terminal_override=("cancelled" if expected_terminal == "cancelled" else
                                       "execution_failed" if bounded_stall else None),
                    expected_terminal=expected_terminal,
                )
            return result
        except (HarnessError, OSError, subprocess.TimeoutExpired) as error:
            client.close()
            log = spec.root / "acp.stderr"
            tail = log.read_text(encoding="utf-8", errors="replace")[-8192:] if log.exists() else ""
            if ledger_root is not None:
                from .ledger import record
                failed_receipt = {
                    "profile": profile, "deny_bash": deny_bash, "daemon": {},
                    "provider": "local-openai-fixture", "model": "openai/harness-fixture",
                    "agent": "build", "session_id": session_id, "provider_calls": provider.calls,
                    "wall_time_ms": int((time.monotonic() - trial_started) * 1000),
                    "tool_started": sum(item.get("params", {}).get("update", {}).get("sessionUpdate") == "tool_call"
                                         for item in client.notifications),
                    "tool_finished": sum(item.get("params", {}).get("update", {}).get("sessionUpdate") == "tool_call_update"
                                          for item in client.notifications),
                    "tool_failures": sum(
                        item.get("params", {}).get("update", {}).get("status") == "failed" or
                        "Tool error:" in str(item.get("params", {}).get("update", {}).get("rawOutput", "")) or
                        re.search(r"\[exit code: ([1-9][0-9]*)\]",
                                  str(item.get("params", {}).get("update", {}).get("rawOutput", ""))) is not None
                        for item in client.notifications),
                    "failure_type": type(error).__name__,
                }
                if isinstance(error, HarnessTurnFailure):
                    # The native adapter projects a redacted category and the
                    # usage observed before the failure. A missing measurement
                    # is recorded with its named reason, never as zero usage.
                    failed_receipt["failure_category"] = error.category
                    failed_receipt["codegg_usage"] = error.usage
                    failed_receipt["usage_unavailable_reason"] = error.usage_unavailable_reason
                terminal = ("timeout" if isinstance(error, (HarnessTimeout, subprocess.TimeoutExpired))
                            else "execution_failed" if isinstance(error, HarnessTurnFailure)
                            else "infrastructure_error")
                failed_manifest = record(ledger_root, failed_receipt, task_id=task_id, group_id=group_id,
                       candidate=candidate, workspace=workspace, expected_file=expected_file,
                       expected_text=expected_text, expected_exists=expected_exists if expected_exists is not None else not deny_bash,
                       timeout_seconds=timeout, repeat=repeat, suite_sha256=suite_sha256,
                       case_sha256=case_sha256, terminal_override=terminal,
                       failure_class=type(error).__name__,
                       expected_terminal=expected_terminal)
                failures_match = (failed_receipt["tool_failures"] >= expected_tool_failures
                                  if expected_terminal == "execution_failed"
                                  else failed_receipt["tool_failures"] == expected_tool_failures)
                if terminal == expected_terminal and failures_match:
                    return {"ok": True, "task_id": task_id, "repeat": repeat,
                            "profile": profile, "session_id": session_id,
                            "provider_calls": provider.calls, "stop_reason": terminal,
                            "tool_failures": failed_receipt["tool_failures"],
                            "expected_file": expected_file, "ledger_manifest": failed_manifest}
            raise HarnessError(f"{error}; fixture requests={provider.request_summaries!r}; ACP frames={client.frame_summaries[-30:]!r}; lane diagnostic tail: {tail}") from error
        finally:
            if session_id and client.process.poll() is None:
                try:
                    client.request("session/close", {"sessionId": session_id}, timeout=2)
                except (HarnessError, OSError):
                    pass
            client.close()
            try:
                _stop_daemon(spec, min(timeout, 5))
            except (HarnessError, FileNotFoundError, OSError, subprocess.TimeoutExpired):
                pass
            provider.close()
            if _owned_daemon_pids(spec):
                raise HarnessError("verified subject daemon remains after cleanup")
            shutil.rmtree(root, ignore_errors=True)


#: Evidence that a headless tool call was refused or failed.
#:
#: The headless contract reports neither a per-tool status nor a per-tool
#: output: its JSON result carries only the tools the model called, the turn
#: result, and the usage. A refusal is therefore read from the child's own
#: output when the child prints one, and from the missing artifact when it
#: does not. The markers are deliberately generic: a candidate that words its
#: refusal differently must still be caught.
EXEC_TOOL_REFUSAL_MARKERS = (
    "permission denied",
    "non-retryable error",
    "missing workspace path policy",
    "tool error:",
    "tool_permission",
    "permission_error",
)

#: The headless child names the refused tool when it reports one.
EXEC_TOOL_REFUSAL_PATTERN = re.compile(r"Tool '([^']+)'[^\n]*non-retryable error", re.IGNORECASE)


def _exec_tool_refused(name: str, refused_tools: list[str], tool_failures: int,
                        tools_used: list[str]) -> bool:
    """True when the observed tool call is the one that failed.

    A refusal the child named is attributed to that tool. A refusal the child
    did not name is attributed to the last tool it reported, because the
    headless contract reports the called tools and nothing else.
    """
    if name in refused_tools:
        return True
    return bool(tool_failures) and not refused_tools and bool(tools_used) and name == tools_used[-1]


def _parse_exec_payload(text: str) -> Optional[dict[str, Any]]:
    """Return the headless child's JSON result object, or ``None``.

    The child prints exactly one JSON object on stdout. A tolerant fallback
    scans the trailing lines so a stray banner cannot hide the result, and an
    unparseable stdout is reported as absent rather than guessed at.
    """
    stripped = text.strip()
    if not stripped:
        return None
    try:
        value = json.loads(stripped)
    except json.JSONDecodeError:
        value = None
    if isinstance(value, dict):
        return value
    for line in reversed(stripped.splitlines()):
        candidate = line.strip()
        if not candidate.startswith("{"):
            continue
        try:
            parsed = json.loads(candidate)
        except json.JSONDecodeError:
            continue
        if isinstance(parsed, dict):
            return parsed
    return None


def run_exec_trial(binary: Path, profile: str, timeout: float, *,
                   ledger_root: Optional[Path] = None, candidate: str = "current",
                   task_id: str = "headless-exec-write", group_id: str = "headless-exec",
                   repeat: int = 0, suite_sha256: str = "", case_sha256: str = "",
                   content: str = "harness-lab-tool-ok",
                   expected_text: Optional[str] = "harness-lab-tool-ok",
                   expected_file: str = "tool-receipt.txt",
                   expected_exists: Optional[bool] = None,
                   expected_terminal: str = "passed",
                   expected_tool_failures: int = 0,
                   tool_sequence: Optional[list[dict[str, Any]]] = None,
                   initial_files: Optional[dict[str, str]] = None) -> dict[str, Any]:
    """Run one headless ``codegg exec`` turn against the local deterministic fixture.

    The headless adapter is measured the same way the native ACP adapter is,
    with one difference that decides the whole trial: the headless contract
    reports no per-tool status. A turn can complete with every tool refused
    and still print ``success: true``, so the artifact and the child's own
    output decide the outcome — never the child's success flag alone.

    The trial never raises for an observed failure. It records the failure and
    returns the receipt, so a score sees a failure terminal instead of an
    aborted evaluation.
    """
    from .provider_fixture import FixtureProvider
    if profile not in {"full-host", "workspace-write"}:
        raise HarnessError("profile must be full-host or workspace-write")
    trial_started = time.monotonic()
    binary = binary.resolve()
    root = lane_root("exec-" + uuid.uuid4().hex[:8])
    root.mkdir(parents=True, exist_ok=True)
    workspace = root / "workspace"
    workspace.mkdir()
    provider = None
    try:
        target = (workspace / expected_file).resolve(strict=False)
        if not target.is_relative_to(workspace.resolve()):
            raise HarnessError("fixture verifier path escapes its private workspace")
        for relative, value in (initial_files or {}).items():
            path = (workspace / relative).resolve(strict=False)
            if not path.is_relative_to(workspace.resolve()) or not isinstance(value, str) or len(value.encode()) > 65536:
                raise HarnessError("fixture initial file path or content is invalid")
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(value, encoding="utf-8")
        if tool_sequence is None:
            tool_sequence = [{"name": "write", "arguments": {"path": expected_file, "content": content}}]
        if not isinstance(tool_sequence, list) or not 1 <= len(tool_sequence) <= 8:
            raise HarnessError("fixture tool sequence must contain 1-8 calls")
        def bind_workspace(value: Any) -> Any:
            if isinstance(value, str):
                return value.replace("{workspace}", str(workspace))
            if isinstance(value, list):
                return [bind_workspace(item) for item in value]
            if isinstance(value, dict):
                return {key: bind_workspace(item) for key, item in value.items()}
            return value
        tool_sequence = bind_workspace(tool_sequence)
        provider = FixtureProvider(tool_sequence).start()
        spec = SubjectSpec.create("exec-trial", binary, workspace, root, Path.cwd())
        env = spec.environment()
        config = {
            "provider": {"openai": {
                "api_key": "local-fixture-only",
                "base_url": provider.base_url,
                "models": {"harness-fixture": {"name": "Harness Fixture"}},
            }},
            "permission": {"bash": "ask", "bash_ask_patterns": ["*"]},
            "agent": {"build": {"permission": {"write": "ask"}}},
        }
        config_path = Path(env["CODEGG_TUI_CONFIG"])
        config_path.parent.mkdir(parents=True, exist_ok=True)
        config_path.write_text(json.dumps(config), encoding="utf-8")
        request = json.dumps({
            "prompt": f"Complete this deterministic local task using the supplied tool sequence: {task_id}.",
            "model": "openai/harness-fixture",
            "agent": "build",
        })
        argv = [str(binary), "exec", "--json", request, "--format", "json",
                "--approval-mode", "yolo", "--sandbox", profile]
        process = subprocess.Popen(argv, cwd=workspace, env=env,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   start_new_session=(os.name == "posix"))
        timed_out = False
        try:
            stdout_bytes, stderr_bytes = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            _signal_owned_group(process, signal.SIGKILL)
            try:
                stdout_bytes, stderr_bytes = process.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                stdout_bytes, stderr_bytes = b"", b""
        stdout_text = stdout_bytes.decode("utf-8", errors="replace")
        stderr_text = stderr_bytes.decode("utf-8", errors="replace")
        payload = _parse_exec_payload(stdout_text)
        combined = f"{stdout_text}\n{stderr_text}"
        tools_used: list[str] = []
        child_success = False
        child_error: Optional[str] = None
        child_code: Optional[str] = None
        tokens_used: Optional[int] = None
        if payload is not None:
            raw_tools = payload.get("toolsUsed")
            if isinstance(raw_tools, list):
                tools_used = [str(item) for item in raw_tools]
            child_success = payload.get("success") is True
            raw_error = payload.get("error")
            child_error = raw_error if isinstance(raw_error, str) else None
            raw_code = payload.get("code")
            child_code = raw_code if isinstance(raw_code, str) else None
            tokens_used = _optional_token_count(payload.get("tokensUsed"))
        observed_exists = target.is_file()
        observed_text = target.read_text(encoding="utf-8", errors="replace") if observed_exists else None
        expected_exists_resolved = True if expected_exists is None else bool(expected_exists)
        artifact_matches = observed_exists and (expected_text is None or observed_text == expected_text)
        # The same verdict the ledger's own verifier reaches: a missing
        # artifact fails the case that expects one, and an artifact whose
        # content differs fails it too.
        verification_passed = ((not observed_exists and not expected_exists_resolved)
                                or (expected_exists_resolved and artifact_matches))
        refused_tools = sorted(set(EXEC_TOOL_REFUSAL_PATTERN.findall(combined)))
        marker_hits = [marker for marker in EXEC_TOOL_REFUSAL_MARKERS if marker in combined.lower()]
        tool_failures = len(refused_tools)
        if tool_failures == 0 and marker_hits:
            # The child reported a refusal without naming the tool.
            tool_failures = 1
        if tool_failures == 0 and tools_used and expected_exists_resolved and not artifact_matches:
            # The child reported a tool call and the artifact never appeared:
            # the tool did not do its work, whatever the child called the turn.
            tool_failures = len(tools_used)
        failures_match = (tool_failures >= expected_tool_failures if expected_terminal == "execution_failed"
                          else tool_failures == expected_tool_failures)
        if timed_out:
            stop_reason = "timeout"
        elif payload is None:
            stop_reason = "indeterminate"
        elif child_success:
            stop_reason = "end_turn"
        else:
            stop_reason = "execution_failed"
        if timed_out:
            error_text = f"headless child exceeded the {timeout}s budget and was killed"
        elif payload is None:
            error_text = "headless child did not emit a parseable JSON result"
        elif not child_success:
            error_text = child_error or "headless child reported a failed turn"
        else:
            error_text = None
        if timed_out or payload is None or not child_success:
            ok = False
        elif expected_terminal == "execution_failed":
            ok = failures_match
        elif expected_terminal == "assertion_failed":
            ok = (not verification_passed) and failures_match
        else:
            ok = verification_passed and failures_match
        usage = None
        if tokens_used is not None:
            # The headless contract reports one total and never splits it into
            # input/output counts. A missing total stays missing: it is never
            # reported as zero tokens.
            usage = {"turn_id": None, "input_tokens": None, "output_tokens": None,
                     "cached_tokens": None, "reasoning_tokens": None,
                     "total_tokens": tokens_used}
        receipt: dict[str, Any] = {
            "ok": ok,
            "task_id": task_id,
            "repeat": repeat,
            "profile": profile,
            "provider": "local-openai-fixture",
            "model": "openai/harness-fixture",
            "agent": "build",
            "tool_started": len(tools_used),
            "tool_finished": max(len(tools_used) - tool_failures, 0),
            "tool_failures": tool_failures,
            "tool_outcomes": [{"tool": name,
                               "status": "failed" if _exec_tool_refused(name, refused_tools, tool_failures, tools_used) else "completed"}
                              for name in tools_used],
            "tool_selection": tools_used,
            "codegg_usage": usage,
            "usage_unavailable_reason": "not_provided",
            "tool_receipt_created": observed_exists,
            "provider_calls": provider.calls,
            "wall_time_ms": int((time.monotonic() - trial_started) * 1000),
            "stop_reason": stop_reason,
            "error": error_text,
            "error_code": child_code,
            "workspace": str(workspace),
            "expected_file": expected_file,
            # The headless path runs in-process: there is no daemon to prove.
            "daemon": {},
            # The headless contract exposes no policy receipt in its result.
            "policy": None,
            "session_id": None,
        }
        if ledger_root is not None:
            from .ledger import record
            trace = []
            scripted: dict[str, Any] = {}
            for call in tool_sequence:
                name = call.get("name")
                if isinstance(name, str) and name not in scripted:
                    scripted[name] = call.get("arguments")
            for name in tools_used:
                arguments = scripted.get(name)
                digest = (hashlib.sha256(json.dumps(arguments, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
                          if arguments is not None else None)
                trace.append({"kind": "tool_started", "tool": name,
                              "arguments_sha256": digest, "elapsed_ms": None})
                trace.append({"kind": "tool_finished", "tool": name,
                              "status": "failed" if _exec_tool_refused(name, refused_tools, tool_failures, tools_used) else "completed",
                              "elapsed_ms": None})
            trace.append({"kind": "turn_terminal", "status": stop_reason})
            override = None
            if timed_out:
                override = "timeout"
            elif expected_terminal == "execution_failed" and failures_match:
                # The case asked for a failed turn and the tool failures match:
                # the recorded terminal names that failure rather than the
                # missing artifact.
                override = "execution_failed"
            elif payload is not None and not child_success:
                # The headless turn itself failed. The recorded terminal names
                # the failed execution instead of the missing artifact, so the
                # reliability criterion sees it as the avoidable failure it is.
                override = "execution_failed"
            try:
                receipt["ledger_manifest"] = record(
                    ledger_root, receipt, task_id=task_id, group_id=group_id,
                    candidate=candidate, workspace=workspace, expected_file=expected_file,
                    expected_text=expected_text, expected_exists=expected_exists_resolved,
                    timeout_seconds=timeout, repeat=repeat, suite_sha256=suite_sha256,
                    case_sha256=case_sha256, trace=trace, terminal_override=override,
                    expected_terminal=expected_terminal)
            except (ValueError, OSError) as error:
                receipt["ledger_error"] = f"{type(error).__name__}: {error}"
                receipt["ok"] = False
        return receipt
    finally:
        if provider is not None:
            provider.close()
        shutil.rmtree(root, ignore_errors=True)


def _default_daemon_fingerprint(binary: Path, timeout: float) -> tuple[str, str] | None:
    # Status is read-only and does not autostart the daemon. This detects any
    # accidental attachment or mutation of the ordinary user daemon.
    result = subprocess.run([str(binary), "daemon", "status"], capture_output=True, text=True, timeout=timeout)
    if result.returncode != 0 or "Daemon is not running or unreachable" in result.stdout + result.stderr:
        return None
    daemon_id = re.search(r"^\s*Daemon ID:\s*(\S+)\s*$", result.stdout, re.MULTILINE)
    generation = re.search(r"^\s*Generation:\s*(\S+)\s*$", result.stdout, re.MULTILINE)
    if daemon_id and generation:
        return daemon_id.group(1), generation.group(1)
    raise HarnessError("default daemon status was reachable but lacked identity fields")

