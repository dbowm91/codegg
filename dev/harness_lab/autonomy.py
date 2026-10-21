"""Versioned, provider-free benchmark taxonomy for sustained agent cases.

This module loads and validates the frozen case manifest at
``dev/harness_lab/fixtures/agent_autonomy/cases.jsonl``. It is the taxonomy
contract for work package A of
``plans/implementation/harness-lab-post-pilot-corrective/004-sustained-agent-benchmark-qualification.md``.

Trust boundary
--------------
The manifest is the *contract*; the task fixtures and the hidden verifiers it
names live outside this package, under the ``verifier_scope`` prefix each case
declares. A candidate patch may never reach that prefix, and
:func:`verify_isolation` refuses a suite whose verifier scope falls under the
candidate-editable prefixes it is given. A candidate therefore cannot edit the
verifier that judges it.

Strictness
----------
The loader rejects, never repairs: an unsupported ``schema_version``, a missing
required field, an unknown field, a duplicate ``case_id``, a family outside
:data:`SUPPORTED_FAMILIES`, a ``support`` mode outside ``{"supported",
"deferred"}``, a deferred case without a reason, a supported case carrying a
reason, a non-boolean ``protected``, an empty ``allowed_tools``, a
non-positive or non-integer budget, a negative / non-finite / boolean cost, a
``network`` value that is neither ``"none"`` nor an explicit endpoint
disclosure, and a verifier without a real argv list. A missing ``network`` is a
validation error: the plan requires an explicit disclosure, so "no network" is
never inferred.

No I/O beyond the manifest
--------------------------
The module parses and validates; it never runs a verifier, never opens a
socket and never starts a process. It imports no network or process module, so
it can be imported in a provider-free, offline check.
"""
from __future__ import annotations

import hashlib
import json
import math
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Optional, Tuple

AUTONOMY_SCHEMA_VERSION = 1

#: The six separately scoped case families named by the plan's work package A.
SUPPORTED_FAMILIES = ("read_search_patch", "compile_debug", "background_job",
                      "repeated_tool_calls", "delegation", "integration")

SUPPORT_SUPPORTED = "supported"
SUPPORT_DEFERRED = "deferred"
SUPPORT_MODES = (SUPPORT_SUPPORTED, SUPPORT_DEFERRED)

#: Verifier kinds the loader accepts. ``script`` covers any argv that exits
#: non-zero on failure; the manifest uses it because this repository's Python
#: is standard-library only.
VERIFIER_KINDS = ("pytest", "cargo_test", "script")

#: Manifest location relative to the repository root.
MANIFEST_RELPATH = "dev/harness_lab/fixtures/agent_autonomy/cases.jsonl"

_CASE_KEYS = frozenset({
    "schema_version", "case_id", "family", "group_id", "title", "prompt",
    "verifier", "verifier_scope", "budget", "abort", "allowed_tools",
    "network", "protected", "support", "deferral_reason",
})
_REQUIRED_CASE_KEYS = tuple(sorted(_CASE_KEYS - {"deferral_reason"}))
_VERIFIER_KEYS = frozenset({"kind", "command", "cwd"})
_BUDGET_KEYS = frozenset({"max_provider_requests", "max_tool_calls",
                          "max_wallclock_seconds", "max_cost_usd"})
_ABORT_KEYS = frozenset({"timeout_seconds", "stall_seconds"})
_NETWORK_KEYS = frozenset({"endpoints", "reason"})


def _repo_root() -> Path:
    """Return the repository root that contains this development package."""
    return Path(__file__).resolve().parents[2]


def _resolve_manifest(root: Optional[Path] = None) -> Path:
    """Return the manifest path for ``root``.

    ``root`` may be the repository root (the manifest then sits at
    :data:`MANIFEST_RELPATH` under it) or the directory that directly holds
    ``cases.jsonl``. ``None`` selects the repository this package lives in.
    """
    if root is None:
        return _repo_root() / MANIFEST_RELPATH
    base = Path(root)
    nested = base / MANIFEST_RELPATH
    if nested.is_file():
        return nested
    direct = base / "cases.jsonl"
    if direct.is_file():
        return direct
    raise FileNotFoundError(f"autonomy manifest not found: expected {nested} or {direct}")


def _is_under(scope: str, prefix: str) -> bool:
    """Return True when ``scope`` is ``prefix`` itself or lives under it.

    The comparison is path-segment aware: ``srcs/`` is not under ``src``.
    """
    normalized_scope = scope.strip().strip("/")
    normalized_prefix = prefix.strip().strip("/")
    if not normalized_prefix:
        return False
    return (normalized_scope == normalized_prefix
            or normalized_scope.startswith(normalized_prefix + "/"))


def _require_text(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{label} must be a non-empty string")
    return value


def _require_positive_int(value: Any, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValueError(f"{label} must be an integer")
    if value < 1:
        raise ValueError(f"{label} must be a positive integer")
    return value


def _require_bool(value: Any, label: str) -> bool:
    if not isinstance(value, bool):
        raise ValueError(f"{label} must be a boolean")
    return value


def _require_mapping(value: Any, label: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise ValueError(f"{label} must be a mapping")
    return value


def _reject_unknown(mapping: Mapping[str, Any], allowed: frozenset, label: str) -> None:
    unknown = sorted(str(key) for key in set(mapping) - allowed)
    if unknown:
        raise ValueError(f"{label} has unknown field(s): {', '.join(unknown)}")


def _require_known_keys(mapping: Mapping[str, Any], allowed: frozenset, label: str) -> None:
    missing = sorted(key for key in allowed if key not in mapping)
    if missing:
        raise ValueError(f"{label} is missing required field(s): {', '.join(missing)}")


def _is_kebab_case(value: str) -> bool:
    parts = value.split("-")
    if not parts or any(not part for part in parts):
        return False
    return all(part.isalnum() and part.islower() for part in parts)


@dataclass(frozen=True)
class AutonomyCase:
    """One frozen, versioned benchmark case.

    ``network`` is either the literal ``"none"`` or an explicit endpoint
    disclosure mapping. ``support`` is ``"supported"`` or ``"deferred"``; a
    deferred case carries a non-empty ``deferral_reason`` and must never be
    scored.
    """

    case_id: str
    family: str
    group_id: str
    title: str
    prompt: str
    verifier: Mapping[str, Any]
    verifier_scope: str
    budget: Mapping[str, Any]
    abort: Mapping[str, Any]
    allowed_tools: Tuple[str, ...]
    network: Any
    protected: bool
    support: str
    deferral_reason: Optional[str]

    def __post_init__(self) -> None:
        if self.family not in SUPPORTED_FAMILIES:
            raise ValueError(f"unsupported autonomy family: {self.family!r}")
        if self.support not in SUPPORT_MODES:
            raise ValueError(f"unsupported autonomy support mode: {self.support!r}")
        if not isinstance(self.protected, bool):
            raise ValueError(f"autonomy case {self.case_id!r} protected must be a boolean")
        if not self.allowed_tools:
            raise ValueError(f"autonomy case {self.case_id!r} requires a non-empty allowed_tools list")
        if self.support == SUPPORT_DEFERRED and not (self.deferral_reason or "").strip():
            raise ValueError(f"deferred autonomy case {self.case_id!r} requires a non-empty deferral_reason")
        if self.support == SUPPORT_SUPPORTED and self.deferral_reason is not None:
            raise ValueError(f"supported autonomy case {self.case_id!r} must not carry a deferral_reason")

    def as_dict(self) -> Dict[str, Any]:
        """Return a JSON-serializable view of the case."""
        network = dict(self.network) if isinstance(self.network, Mapping) else self.network
        return {"schema_version": AUTONOMY_SCHEMA_VERSION,
                "case_id": self.case_id,
                "family": self.family,
                "group_id": self.group_id,
                "title": self.title,
                "prompt": self.prompt,
                "verifier": dict(self.verifier),
                "verifier_scope": self.verifier_scope,
                "budget": dict(self.budget),
                "abort": dict(self.abort),
                "allowed_tools": list(self.allowed_tools),
                "network": network,
                "protected": self.protected,
                "support": self.support,
                "deferral_reason": self.deferral_reason}


@dataclass(frozen=True)
class AutonomySuite:
    """A loaded, validated manifest plus the digest of its exact bytes."""

    version: int
    cases: Tuple[AutonomyCase, ...]
    suite_sha256: str

    def case(self, case_id: str) -> AutonomyCase:
        """Return the case with ``case_id`` or raise ``KeyError``."""
        for case in self.cases:
            if case.case_id == case_id:
                return case
        raise KeyError(case_id)

    def supported_cases(self) -> Tuple[AutonomyCase, ...]:
        """Return the cases that may be scored."""
        return tuple(case for case in self.cases if case.support == SUPPORT_SUPPORTED)

    def deferred_cases(self) -> Tuple[AutonomyCase, ...]:
        """Return the cases that are deferred and must never be scored."""
        return tuple(case for case in self.cases if case.support == SUPPORT_DEFERRED)

    def protected_cases(self) -> Tuple[AutonomyCase, ...]:
        """Return the protected negative controls."""
        return tuple(case for case in self.cases if case.protected)

    def require_supported(self, case_id: str) -> AutonomyCase:
        """Return the named case, refusing it when it is deferred."""
        return require_supported(self.case(case_id))

    def as_dict(self) -> Dict[str, Any]:
        """Return a JSON-serializable view of the suite."""
        return {"version": self.version,
                "suite_sha256": self.suite_sha256,
                "case_count": len(self.cases),
                "cases": [case.as_dict() for case in self.cases]}


def require_supported(case: AutonomyCase) -> AutonomyCase:
    """Return ``case`` when it is supported, else refuse it with its reason.

    A deferred case is a legitimate outcome for a family whose CodeGG feature
    is not closed and live-capable. Scoring one would be a false pass, so the
    refusal names the recorded reason.
    """
    if not isinstance(case, AutonomyCase):
        raise ValueError("require_supported requires an AutonomyCase")
    if case.support != SUPPORT_SUPPORTED:
        reason = (case.deferral_reason or "").strip() or "no deferral reason recorded"
        raise ValueError(f"autonomy case {case.case_id!r} is deferred and must not be scored: {reason}")
    return case


def _validate_verifier(value: Any, label: str) -> Dict[str, Any]:
    verifier = _require_mapping(value, f"{label} verifier")
    _reject_unknown(verifier, _VERIFIER_KEYS, f"{label} verifier")
    _require_known_keys(verifier, _VERIFIER_KEYS, f"{label} verifier")
    kind = verifier["kind"]
    if kind not in VERIFIER_KINDS:
        raise ValueError(f"unsupported autonomy verifier kind: {kind!r}")
    command = verifier["command"]
    if not isinstance(command, (list, tuple)) or not command:
        raise ValueError(f"{label} verifier requires a command list of strings")
    for argument in command:
        if not isinstance(argument, str) or not argument:
            raise ValueError(f"{label} verifier requires a command list of strings")
    cwd = verifier["cwd"]
    if not isinstance(cwd, str) or not cwd.strip():
        raise ValueError(f"{label} verifier cwd must be a non-empty string")
    return {"kind": kind, "command": list(command), "cwd": cwd}


def _validate_budget(value: Any, label: str) -> Dict[str, Any]:
    budget = _require_mapping(value, f"{label} budget")
    _reject_unknown(budget, _BUDGET_KEYS, f"{label} budget")
    _require_known_keys(budget, _BUDGET_KEYS, f"{label} budget")
    for name in ("max_provider_requests", "max_tool_calls", "max_wallclock_seconds"):
        _require_positive_int(budget[name], f"{label} budget {name}")
    cost = budget["max_cost_usd"]
    if cost is not None:
        if isinstance(cost, bool) or not isinstance(cost, (int, float)):
            raise ValueError(f"{label} budget max_cost_usd must be a non-negative finite number or null")
        if not math.isfinite(float(cost)):
            raise ValueError(f"{label} budget max_cost_usd must be a non-negative finite number or null")
        if float(cost) < 0:
            raise ValueError(f"{label} budget max_cost_usd must be a non-negative finite number or null")
    return {"max_provider_requests": budget["max_provider_requests"],
            "max_tool_calls": budget["max_tool_calls"],
            "max_wallclock_seconds": budget["max_wallclock_seconds"],
            "max_cost_usd": None if cost is None else float(cost)}


def _validate_abort(value: Any, label: str) -> Dict[str, Any]:
    abort = _require_mapping(value, f"{label} abort")
    _reject_unknown(abort, _ABORT_KEYS, f"{label} abort")
    _require_known_keys(abort, _ABORT_KEYS, f"{label} abort")
    timeout_seconds = _require_positive_int(abort["timeout_seconds"], f"{label} abort timeout_seconds")
    stall_seconds = _require_positive_int(abort["stall_seconds"], f"{label} abort stall_seconds")
    if stall_seconds > timeout_seconds:
        raise ValueError(f"{label} abort stall_seconds must not exceed timeout_seconds")
    return {"timeout_seconds": timeout_seconds, "stall_seconds": stall_seconds}


def _validate_network(value: Any, label: str) -> Any:
    if isinstance(value, str):
        if value != "none":
            raise ValueError(f"{label} network must be \"none\" or an object with a non-empty endpoints list")
        return "none"
    network = _require_mapping(value, f"{label} network")
    _reject_unknown(network, _NETWORK_KEYS, f"{label} network")
    if "endpoints" not in network:
        raise ValueError(f"{label} network is missing required field(s): endpoints")
    endpoints = network["endpoints"]
    if not isinstance(endpoints, (list, tuple)) or not endpoints:
        raise ValueError(f"{label} network requires a non-empty endpoints list")
    for endpoint in endpoints:
        if not isinstance(endpoint, str) or not endpoint.strip():
            raise ValueError(f"{label} network endpoints must be a non-empty list of strings")
    reason = network.get("reason")
    if reason is not None and (not isinstance(reason, str) or not reason.strip()):
        raise ValueError(f"{label} network reason must be a non-empty string when present")
    return {"endpoints": list(endpoints), "reason": reason}


def _validate_allowed_tools(value: Any, label: str) -> Tuple[str, ...]:
    if not isinstance(value, (list, tuple)) or not value:
        raise ValueError(f"{label} requires a non-empty allowed_tools list")
    tools = []
    for name in value:
        if not isinstance(name, str) or not name.strip():
            raise ValueError(f"{label} allowed_tools must be a non-empty list of strings")
        if name in tools:
            raise ValueError(f"{label} allowed_tools must not repeat {name!r}")
        tools.append(name)
    return tuple(tools)


def _case_from_mapping(mapping: Mapping[str, Any], line_number: int) -> AutonomyCase:
    """Validate one manifest record and return the frozen case."""
    raw_id = mapping.get("case_id")
    label = (f"autonomy case {raw_id!r}" if isinstance(raw_id, str) and raw_id.strip()
             else f"autonomy case at line {line_number}")
    _reject_unknown(mapping, _CASE_KEYS, label)
    missing = [key for key in _REQUIRED_CASE_KEYS if key not in mapping]
    if missing:
        raise ValueError(f"{label} is missing required field(s): {', '.join(missing)}")

    version = mapping["schema_version"]
    if isinstance(version, bool) or not isinstance(version, int):
        raise ValueError(f"unsupported autonomy schema_version: {version!r}")
    if version != AUTONOMY_SCHEMA_VERSION:
        raise ValueError(f"unsupported autonomy schema_version: {version!r}")

    case_id = mapping["case_id"]
    _require_text(case_id, f"{label} case_id")
    if not _is_kebab_case(case_id):
        raise ValueError(f"{label} case_id must be a kebab-case identifier")

    family = mapping["family"]
    if family not in SUPPORTED_FAMILIES:
        raise ValueError(f"unsupported autonomy family: {family!r}")

    group_id = _require_text(mapping["group_id"], f"{label} group_id")
    title = _require_text(mapping["title"], f"{label} title")
    prompt = _require_text(mapping["prompt"], f"{label} prompt")
    verifier_scope = _require_text(mapping["verifier_scope"], f"{label} verifier_scope")
    verifier = _validate_verifier(mapping["verifier"], label)
    budget = _validate_budget(mapping["budget"], label)
    abort = _validate_abort(mapping["abort"], label)
    allowed_tools = _validate_allowed_tools(mapping["allowed_tools"], label)
    network = _validate_network(mapping["network"], label)
    protected = _require_bool(mapping["protected"], f"{label} protected")

    support = mapping["support"]
    if support not in SUPPORT_MODES:
        raise ValueError(f"unsupported autonomy support mode: {support!r}")
    deferral_reason = mapping.get("deferral_reason")
    if support == SUPPORT_DEFERRED:
        if not isinstance(deferral_reason, str) or not deferral_reason.strip():
            raise ValueError(f"deferred autonomy case {case_id!r} requires a non-empty deferral_reason")
    elif deferral_reason is not None:
        raise ValueError(f"supported autonomy case {case_id!r} must not carry a deferral_reason")

    return AutonomyCase(case_id=case_id, family=family, group_id=group_id, title=title,
                         prompt=prompt, verifier=verifier, verifier_scope=verifier_scope,
                         budget=budget, abort=abort, allowed_tools=allowed_tools,
                         network=network, protected=protected, support=support,
                         deferral_reason=deferral_reason)


def load_autonomy_suite(root: Optional[Path] = None) -> AutonomySuite:
    """Load, validate and return the frozen autonomy suite.

    The suite digest is the SHA-256 of the manifest bytes, so any edit to the
    manifest changes the digest. Every violation of the taxonomy contract
    raises ``ValueError``; the loader never repairs a record.
    """
    path = _resolve_manifest(root)
    raw = path.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    cases = []
    seen = set()
    for number, line in enumerate(raw.decode("utf-8").splitlines(), start=1):
        if not line.strip():
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(f"autonomy manifest line {number} is not valid JSON: {error}") from error
        if not isinstance(record, Mapping):
            raise ValueError(f"autonomy manifest line {number} must be a JSON object")
        case = _case_from_mapping(record, number)
        if case.case_id in seen:
            raise ValueError(f"duplicate autonomy case_id: {case.case_id!r}")
        seen.add(case.case_id)
        cases.append(case)
    if not cases:
        raise ValueError("autonomy manifest contains no cases")
    return AutonomySuite(version=AUTONOMY_SCHEMA_VERSION, cases=tuple(cases), suite_sha256=digest)


def suite_digest(root: Optional[Path] = None) -> str:
    """Return the SHA-256 of the manifest bytes without parsing it."""
    path = _resolve_manifest(root)
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify_isolation(suite: AutonomySuite, candidate_editable_prefixes: Sequence[str]) -> None:
    """Refuse a suite whose verifier lives under a candidate-editable prefix.

    This is the defence against a candidate patching the verifier that judges
    it. A ``verifier_scope`` that is exactly a candidate-editable prefix is a
    violation too, not only a scope nested below one.
    """
    if not isinstance(suite, AutonomySuite):
        raise ValueError("verify_isolation requires an AutonomySuite")
    prefixes = []
    for prefix in candidate_editable_prefixes:
        if not isinstance(prefix, str) or not prefix.strip():
            raise ValueError("candidate editable prefixes must be non-empty strings")
        prefixes.append(prefix)
    for case in suite.cases:
        for prefix in prefixes:
            if _is_under(case.verifier_scope, prefix):
                raise ValueError(
                    f"autonomy case {case.case_id!r} verifier_scope {case.verifier_scope!r} "
                    f"is under candidate-editable prefix {prefix!r}")


__all__ = [
    "AUTONOMY_SCHEMA_VERSION",
    "MANIFEST_RELPATH",
    "SUPPORTED_FAMILIES",
    "SUPPORT_DEFERRED",
    "SUPPORT_MODES",
    "SUPPORT_SUPPORTED",
    "VERIFIER_KINDS",
    "AutonomyCase",
    "AutonomySuite",
    "load_autonomy_suite",
    "require_supported",
    "suite_digest",
    "verify_isolation",
]
