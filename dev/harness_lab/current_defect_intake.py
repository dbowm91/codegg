"""Trusted, offline current-CodeGG defect reproduction and eligibility."""
from __future__ import annotations

import hashlib
import json
import os
import platform
import re
import signal
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

SCHEMA = "current_codegg_defect_dossier_v1"
TEST_ID = "h007-native-job-sequence"
TEST_PATH = Path("dev/harness_lab/tests/test_h007_native.py")
HOLDOUT_PATHS = (
    Path("dev/harness_lab/fixtures/agent_autonomy_h007/cases.jsonl"),
    Path("dev/harness_lab/fixtures/build_provenance_catalog/fixture/Cargo.toml"),
    Path("dev/harness_lab/fixtures/build_provenance_catalog/fixture/Cargo.lock"),
    Path("dev/harness_lab/fixtures/build_provenance_catalog/reference/catalog-format/src/lib.rs"),
    Path("dev/harness_lab/fixtures/build_provenance_catalog/reference/catalog-store/src/lib.rs"),
    Path("dev/harness_lab/fixtures/build_provenance_catalog/reference/catalog-index/src/lib.rs"),
    Path("dev/harness_lab/fixtures/build_provenance_catalog/reference/catalog-query/src/lib.rs"),
    Path("dev/harness_lab/fixtures/build_provenance_catalog/reference/catalog-cli/src/main.rs"),
)
_ASSERTION = re.compile(r"AssertionError:\s*(.{1,256})")
BUILD_TIMEOUT_SECONDS = 1800


class IntakeError(ValueError):
    """Current-defect intake refused an untrusted or incomplete reproduction."""


def _sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _git(root: Path, *args: str) -> str:
    result = subprocess.run(["git", "-C", str(root), *args], capture_output=True,
                            text=True, timeout=15, check=False)
    if result.returncode:
        raise IntakeError("repository identity could not be verified")
    return result.stdout.strip()


def _read_pinned(root: Path, relative: Path) -> bytes:
    """Read a controller-owned evidence file without following a symlink."""
    root = root.resolve(strict=True)
    path = root / relative
    if path.is_symlink():
        raise IntakeError(f"controller-owned evidence path is a symlink: {relative.as_posix()}")
    resolved = path.resolve(strict=True)
    if not resolved.is_relative_to(root) or not resolved.is_file():
        raise IntakeError(f"controller-owned evidence path escaped the repository: {relative.as_posix()}")
    return resolved.read_bytes()


def _classify(exit_code: int, output: str, observation: dict[str, Any] | None = None
              ) -> tuple[str, str | None]:
    if "skipped=" in output or "skipped)" in output:
        return "unknown", None
    if exit_code == 0 and re.search(r"\bOK\b", output):
        return "completed", None
    if exit_code != 0 and "FAILED" in output:
        # A unittest assertion before the native subject trial (for example,
        # missing rustup or a malformed fixture) is setup, not CodeGG evidence.
        # Only post-trial assertions may be attributed to the native contract.
        if not _is_native_trial_observation(observation):
            return "unknown", None
        match = _ASSERTION.search(output)
        if match:
            # Store only a digest of the bounded assertion text; tracebacks can
            # contain private paths and test output must not enter the dossier.
            return "native_contract_red", _sha((TEST_ID + "\0" + match.group(1).strip()).encode())
    if exit_code != 0 and "ERROR" in output:
        return "verifier_or_setup_error", None
    return "unknown", None


def _is_native_trial_observation(observation: dict[str, Any] | None) -> bool:
    return (isinstance(observation, dict)
            and observation.get("schema") == "h007_native_intake_observation_v1"
            and observation.get("task_id") == "h007-dependent-rust-and-exact-job"
            and observation.get("trial_returned") is True
            and observation.get("native_terminal") == "end_turn"
            and isinstance(observation.get("binary_sha256"), str)
            and bool(re.fullmatch(r"[0-9a-f]{64}", observation["binary_sha256"]))
            and isinstance(observation.get("tool_call_count"), int)
            and observation["tool_call_count"] > 0
            and isinstance(observation.get("event_gap_count"), int)
            and observation["event_gap_count"] >= 0)


def _execute(root: Path, binary: Path, lane: Path) -> dict[str, Any]:
    """Run only the controller-owned native sequence test, never manifest argv."""
    lane.mkdir(mode=0o700, parents=True, exist_ok=False)
    env = os.environ.copy()
    env["CODEGG_H007_CODEGG_BINARY"] = str(binary)
    observation_path = lane / "native-observation.json"
    env["CODEGG_H007_INTAKE_OBSERVATION"] = str(observation_path)
    env["TMPDIR"] = str(lane)
    env["TMP"] = str(lane)
    env["TEMP"] = str(lane)
    env.pop("OPENCODE_GO_API_KEY", None)
    for key in ("OPENAI_API_KEY", "ANTHROPIC_API_KEY", "GOOGLE_API_KEY",
                "OPENROUTER_API_KEY", "OPENCODE_ZEN_API_KEY", "MISTRAL_API_KEY",
                "GROQ_API_KEY", "DEEPINFRA_API_KEY", "CEREBRAS_API_KEY",
                "COHERE_API_KEY", "TOGETHERAI_API_KEY", "PERPLEXITY_API_KEY",
                "XAI_API_KEY", "VENICE_API_KEY", "MINIMAX_API_KEY",
                "GENERALCOMPUTE_API_KEY"):
        env.pop(key, None)
    command = [sys.executable, "-m", "unittest", "-v",
               "dev.harness_lab.tests.test_h007_native.H007NativeJobSequenceTest.test_native_tool_sequence_repairs_two_stages_and_overlaps_downstream_work"]
    process = subprocess.Popen(command, cwd=root, env=env, stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               start_new_session=True)
    try:
        stdout, stderr = process.communicate(timeout=600)
    except subprocess.TimeoutExpired:
        # The native test can start CodeGG daemons and Cargo workers. A timeout
        # must reap the whole lane process group before another lane starts.
        os.killpg(process.pid, signal.SIGKILL)
        stdout, stderr = process.communicate(timeout=30)
        output = (stdout + b"\n" + stderr)[-32768:].decode(errors="replace")
        return {"lane_id": lane.name,
                "terminal": "timeout", "failure_signature": None,
                "exit_code": process.returncode, "output_sha256": _sha(output.encode()),
                "test_sha256": _sha(_read_pinned(root, TEST_PATH))}
    output = (stdout + b"\n" + stderr)[-32768:].decode(errors="replace")
    observation = None
    if observation_path.is_file() and not observation_path.is_symlink():
        try:
            observation = json.loads(observation_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            observation = None
    terminal, signature = _classify(process.returncode, output, observation)
    return {"lane_id": lane.name,
            "terminal": terminal, "failure_signature": signature,
            "exit_code": process.returncode, "output_sha256": _sha(output.encode()),
            "native_observation_sha256": (_sha(observation_path.read_bytes())
                                          if observation is not None else None),
            "test_sha256": _sha(_read_pinned(root, TEST_PATH))}


def _selected_rust_tools(root: Path) -> tuple[Path, Path, str, str]:
    cargo = shutil.which("cargo")
    rustc = shutil.which("rustc")
    rustup = shutil.which("rustup")
    if not cargo or not rustc:
        raise IntakeError("Cargo and rustc must both be available for trusted reproduction")
    cargo_path = Path(cargo).resolve()
    rustc_path = Path(rustc).resolve()
    if rustup and cargo_path == Path(rustup).resolve():
        selected_cargo = subprocess.run([rustup, "which", "cargo"], cwd=root,
                                        capture_output=True, text=True, timeout=15, check=False)
        selected_rustc = subprocess.run([rustup, "which", "rustc"], cwd=root,
                                        capture_output=True, text=True, timeout=15, check=False)
        if selected_cargo.returncode or selected_rustc.returncode:
            raise IntakeError("the selected Rust toolchain could not be resolved")
        cargo_path = Path(selected_cargo.stdout.strip()).resolve(strict=True)
        rustc_path = Path(selected_rustc.stdout.strip()).resolve(strict=True)
    if cargo_path.parent != rustc_path.parent:
        raise IntakeError("Cargo and rustc resolve to different toolchains")
    version = subprocess.run([str(rustc_path), "--version", "--verbose"], cwd=root,
                             capture_output=True, text=True, timeout=15, check=False)
    if version.returncode:
        raise IntakeError("the selected rustc version could not be verified")
    host = next((line.removeprefix("host: ") for line in version.stdout.splitlines()
                 if line.startswith("host: ")), "unknown")
    return cargo_path, rustc_path, version.stdout.splitlines()[0], host


def _build_binary(root: Path, target: Path) -> tuple[Path, str, str]:
    root = root.resolve(strict=True)
    configured_target = os.environ.get("CODEGG_HARNESS_INTAKE_TARGET_DIR")
    if configured_target:
        target = Path(configured_target).expanduser()
        if target.is_symlink():
            raise IntakeError("private Cargo target directory must not be a symlink")
        target = target.resolve(strict=False)
        if target == root or target.is_relative_to(root):
            raise IntakeError("private Cargo target directory must remain outside the source checkout")
        target.mkdir(mode=0o700, parents=True, exist_ok=True)
        if not target.is_dir():
            raise IntakeError("private Cargo target directory is not a directory")
    else:
        target.mkdir(mode=0o700, parents=True, exist_ok=False)
    cargo, rustc, rustc_version, host = _selected_rust_tools(root)
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target)
    env["CARGO_BUILD_JOBS"] = "2"
    env["CARGO_NET_OFFLINE"] = "true"
    env.setdefault("CARGO_PROFILE_DEV_DEBUG", "0")
    built = subprocess.run([str(cargo), "build", "--locked", "--bin", "codegg",
                            "--bin", "codegg-sandbox-helper"], cwd=root, env=env,
                           stdin=subprocess.DEVNULL, capture_output=True, text=True,
                           timeout=BUILD_TIMEOUT_SECONDS, check=False)
    if built.returncode:
        raise IntakeError("current source could not build offline; no defect was admitted")
    binary = target / "debug" / ("codegg.exe" if os.name == "nt" else "codegg")
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise IntakeError("private current-source build did not produce CodeGG")
    return binary, rustc_version, host


def reproduce_current_defect(repo_root: Path, *, defect_id: str) -> dict[str, Any]:
    """Run a frozen failing-first CodeGG contract in two private lanes.

    A green, skipped, setup-failed, mixed, or flaky result is ineligible. The
    command, verifier and candidate scope are controller-owned constants.
    This operation never calls a provider or writes source files.
    """
    root = repo_root.resolve(strict=True)
    if not isinstance(defect_id, str) or not defect_id.strip() or len(defect_id) > 128:
        raise IntakeError("defect id is invalid")
    source_revision = _git(root, "rev-parse", "HEAD")
    if _git(root, "status", "--porcelain"):
        raise IntakeError("current-source reproduction requires a clean checkout")
    test_sha = _sha(_read_pinned(root, TEST_PATH))
    holdouts = {path.as_posix(): _sha(_read_pinned(root, path)) for path in HOLDOUT_PATHS}
    with tempfile.TemporaryDirectory(prefix="harness-current-defect-") as directory:
        base = Path(directory)
        binary, rustc_version, rust_host = _build_binary(root, base / "private-target")
        binary_sha = _sha(binary.read_bytes())
        observations = [_execute(root, binary, base / "lane-a"),
                        _execute(root, binary, base / "lane-b")]
    if _sha(_read_pinned(root, TEST_PATH)) != test_sha:
        raise IntakeError("controller-owned test changed during current-source reproduction")
    if any(_sha(_read_pinned(root, path)) != digest for path, digest in holdouts.items()):
        raise IntakeError("independent holdout changed during current-source reproduction")
    lanes = [row["lane_id"] for row in observations]
    terminals = [row["terminal"] for row in observations]
    same_red = (len(set(lanes)) == 2 and terminals == ["native_contract_red"] * 2
                and observations[0]["failure_signature"] is not None
                and observations[0]["failure_signature"] == observations[1]["failure_signature"]
                and all(row["test_sha256"] == test_sha for row in observations))
    identity = {"schema": SCHEMA, "defect_id": defect_id, "owner": "codegg_harness",
                "evidence_kind": "current_codegg_defect", "source_revision": source_revision,
                "binary_sha256": binary_sha, "command_id": TEST_ID,
                "rustc_version": rustc_version, "rust_host": rust_host,
                "platform_machine": platform.machine(),
                "test_path": TEST_PATH.as_posix(), "test_sha256": test_sha,
                "holdout_sha256": holdouts, "failure_signature": observations[0]["failure_signature"]
                if same_red else None,
                "candidate_scope": ["src/", "crates/"],
                "protected_controls": ["exact submitted job ID reaches running and terminal",
                                       "dependent work overlaps the same job",
                                       "two distinct failure/repair cycles",
                                       "private independent verifier passes on repaired fixture"],
                # Bind the stable trusted observations, excluding volatile
                # output digests and wall time from unittest's text footer.
                "reproduction_evidence": [
                    {key: row.get(key) for key in
                     ("terminal", "failure_signature", "test_sha256",
                      "native_observation_sha256")}
                    for row in observations]}
    dossier_id = _sha(json.dumps(identity, sort_keys=True, separators=(",", ":")).encode())
    return {**identity, "reproductions": observations,
            "dossier_id": f"current-codegg-{dossier_id[:20]}",
            "reproduction_validated": all(row["terminal"] in
                                          {"completed", "native_contract_red"}
                                          for row in observations),
            "defect_eligible": same_red,
            "live_authorized": False,
            "disposition": ("eligible_current_defect" if same_red else
                            "no_reproducible_defect" if terminals == ["completed"] * 2 else
                            "inconclusive_reproduction"),
            "eligible_real_defects": int(same_red),
            "next_action": ("Require fresh separate operator approval before any campaign."
                            if same_red else
                            "Do not launch a campaign; collect a distinct trusted red reproduction.")}


def build_synthetic_receipt(*, defect_id: str, source_sha256: str,
                            failure_signature: str) -> dict[str, Any]:
    """Production-shaped synthetic fixture receipt, explicitly never promotable."""
    if not defect_id or len(defect_id) > 128 or not re.fullmatch(r"[0-9a-f]{64}", source_sha256):
        raise IntakeError("synthetic fixture identity is invalid")
    if not failure_signature or len(failure_signature) > 256:
        raise IntakeError("synthetic fixture failure signature is invalid")
    lanes = [{"lane_id": "synthetic-lane-a", "terminal": "native_contract_red",
              "failure_signature": failure_signature},
             {"lane_id": "synthetic-lane-b", "terminal": "native_contract_red",
              "failure_signature": failure_signature}]
    value = {"schema": SCHEMA, "evidence_kind": "synthetic_fixture", "defect_id": defect_id,
             "source_sha256": source_sha256, "reproductions": lanes,
             "reproduction_validated": True, "defect_eligible": False,
             "live_authorized": False, "disposition": "synthetic_fixture_validated"}
    value["dossier_id"] = "synthetic-current-" + _sha(
        json.dumps(value, sort_keys=True, separators=(",", ":")).encode())[:20]
    return value


def report_dossier(dossier: dict[str, Any]) -> dict[str, Any]:
    """Render a compact report; this view alone never validates eligibility."""
    if dossier.get("schema") != SCHEMA:
        raise IntakeError("unsupported current-defect dossier schema")
    return {"schema": SCHEMA, "dossier_id": dossier.get("dossier_id"),
            "defect_id": dossier.get("defect_id"),
            "disposition": dossier.get("disposition"),
            "reproduction_validated": False,
            "dossier_claimed_reproduction_validated": dossier.get("reproduction_validated") is True,
            "defect_eligible": False,
            "dossier_claimed_eligible": dossier.get("defect_eligible") is True,
            "live_authorized": False,
            "eligible_real_defects": 0}


def _report_trusted_observation(dossier: dict[str, Any]) -> dict[str, Any]:
    """Render eligibility only for a dossier just produced by this controller."""
    report = report_dossier(dossier)
    report["reproduction_validated"] = dossier.get("reproduction_validated") is True
    report["defect_eligible"] = dossier.get("defect_eligible") is True
    report["eligible_real_defects"] = int(report["defect_eligible"])
    return report


def validate_dossier(repo_root: Path, dossier: dict[str, Any]) -> dict[str, Any]:
    """Re-execute the trusted test and reject stale or forged stored receipts."""
    if dossier.get("schema") != SCHEMA or not isinstance(dossier.get("defect_id"), str):
        raise IntakeError("stored current-defect dossier is malformed")
    observed = reproduce_current_defect(repo_root, defect_id=dossier["defect_id"])
    pinned = ("dossier_id", "source_revision", "binary_sha256", "rustc_version", "rust_host", "platform_machine",
              "command_id", "test_path", "test_sha256", "holdout_sha256", "failure_signature",
              "reproduction_evidence", "reproduction_validated", "defect_eligible", "live_authorized",
              "disposition")
    if any(dossier.get(key) != observed.get(key) for key in pinned):
        raise IntakeError("stored dossier does not match fresh source, binary and lane observations")
    return _report_trusted_observation(observed)
