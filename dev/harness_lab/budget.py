"""Provider-free budget admission preflight for live Harness Lab trials.

The preflight answers one question: *may a live trial start?* It is a pure
function of the declared budget policy. It performs no I/O, opens no socket,
spawns no subprocess and never contacts a provider, so it can run before the
first provider request, before a daemon spawn and before any child process.

Fail-closed contract
--------------------
* A trial is admitted only when at least one **enforceable hard cap** is
  declared. Tokens observed *after* a provider response are telemetry, not a
  cap, so they never admit a trial.
* A declared cap that cannot be enforced is a **refusal**, never a downgrade.
  A missing capability is a refusal, never a guess.
* A cost cap is real only when its enforcement is explicitly stated
  (``controller_enforced`` or ``provider_enforced``). A local price estimate is
  never inferred, defaulted or guessed.
* A provider quota counts only when it is enforceable **and** still holds
  across provider retries: a quota that resets on a retry does not bound the
  trial.

Missing-source selection
------------------------
``detail["missing_sources"]`` lists every cap that is not in force, and
``missing_source`` is its first entry:

* caps that were **requested but are unusable** (invalid value, unstated
  enforcement, non-enforceable or non-retry-safe quota) come first, in the
  fixed priority order, because they are the actionable defect;
* caps that were **never requested** follow, in the same fixed priority order.

The fixed priority order is ``token_limit`` -> ``cost_limit_usd`` ->
``max_provider_requests`` -> ``max_wallclock_seconds`` -> ``provider_quota``.
The quota retry-safety pseudo-source (``provider_quota_retry_safety``) is
reported in place of ``provider_quota`` when a quota is enforceable but does
not survive provider retries.
"""
from __future__ import annotations

from dataclasses import dataclass
import math
from typing import Any, Dict, List, Mapping, Optional, Tuple


STATUS_ADMITTED = "admitted"
STATUS_UNVERIFIABLE = "budget_unverifiable"

COST_ENFORCEMENT_MODES = ("controller_enforced", "provider_enforced")
QUOTA_SCOPES = ("account", "key", "project", "request")

MISSING_TOKEN_LIMIT = "token_limit"
MISSING_COST_LIMIT = "cost_limit_usd"
MISSING_MAX_PROVIDER_REQUESTS = "max_provider_requests"
MISSING_MAX_WALLCLOCK_SECONDS = "max_wallclock_seconds"
MISSING_PROVIDER_QUOTA = "provider_quota"
MISSING_PROVIDER_QUOTA_RETRY_SAFETY = "provider_quota_retry_safety"

#: Deterministic priority used to choose a single missing source.
CAP_PRIORITY: Tuple[str, ...] = (
    MISSING_TOKEN_LIMIT,
    MISSING_COST_LIMIT,
    MISSING_MAX_PROVIDER_REQUESTS,
    MISSING_MAX_WALLCLOCK_SECONDS,
    MISSING_PROVIDER_QUOTA,
)

_POLICY_KEYS = frozenset({
    "cost_observable",
    "token_limit",
    "cost_limit_usd",
    "cost_enforcement",
    "max_provider_requests",
    "max_wallclock_seconds",
    "provider_quota",
})
_QUOTA_KEYS = frozenset({
    "source",
    "enforceable",
    "retry_safe",
    "scope",
    "observed_limit",
    "observed_at",
})
_QUOTA_REQUIRED_KEYS = ("source", "enforceable", "retry_safe", "scope")

__all__ = [
    "STATUS_ADMITTED",
    "STATUS_UNVERIFIABLE",
    "COST_ENFORCEMENT_MODES",
    "QUOTA_SCOPES",
    "CAP_PRIORITY",
    "ProviderQuota",
    "BudgetPolicy",
    "BudgetAdmission",
    "admit_budget",
    "budget_policy_from_mapping",
]


@dataclass(frozen=True)
class ProviderQuota:
    """A provider-authoritative ceiling the operator trusts to bound spend."""

    source: str
    enforceable: bool
    retry_safe: bool
    scope: str
    observed_limit: Optional[float] = None
    observed_at: Optional[str] = None


@dataclass(frozen=True)
class BudgetPolicy:
    """The declared budget for one trial. Absent caps are simply not declared."""

    token_limit: Optional[int] = None
    cost_limit_usd: Optional[float] = None
    cost_enforcement: Optional[str] = None
    max_provider_requests: Optional[int] = None
    max_wallclock_seconds: Optional[float] = None
    provider_quota: Optional[ProviderQuota] = None
    #: Whether the provider route reports a cost at all. A controller-enforced
    #: cost cap is only enforceable when the cost is observable; a route that
    #: reports no cost cannot be held to a dollar ceiling the controller
    #: measures. Absent means "not declared", which is not a claim that cost
    #: is unobservable.
    cost_observable: Optional[bool] = None


@dataclass(frozen=True)
class BudgetAdmission:
    """The preflight verdict. ``status`` is the only admission signal."""

    status: str
    enforced_caps: Tuple[str, ...]
    missing_source: Optional[str]
    detail: Mapping[str, Any]

    def as_dict(self) -> Dict[str, Any]:
        """Return a JSON-serializable copy (plain types, no tuples)."""
        detail = dict(self.detail)
        return {
            "status": self.status,
            "enforced_caps": list(self.enforced_caps),
            "missing_source": self.missing_source,
            "detail": {
                "reason": str(detail.get("reason", "")),
                "missing_sources": list(detail.get("missing_sources", [])),
                "invalid_caps": [dict(entry) for entry in detail.get("invalid_caps", [])],
            },
        }


def _numeric_fault(value: Any, *, integral: bool) -> Optional[str]:
    """Return a short reason when ``value`` is not a usable positive limit."""
    if isinstance(value, bool):
        return "must be a number, not a boolean"
    if integral:
        if not isinstance(value, int):
            return "must be an integer"
    elif not isinstance(value, (int, float)):
        return "must be a number"
    if isinstance(value, float) and not math.isfinite(value):
        return "must be a finite number"
    if value <= 0:
        return "must be greater than zero"
    return None


class BudgetExhausted(RuntimeError):
    """An enforced cap refused further provider work.

    This is the enforcement side of the admission gate. A cap that is
    declared but never enforced is an advisory, and an advisory must not be
    presented as a safety gate.
    """


@dataclass
class SpendGuard:
    """Enforce the caps the admission gate admitted.

    ``max_provider_requests`` is enforced before work starts: a trial that
    would cross the ceiling is refused rather than started and then stopped.

    ``token_limit`` and ``cost_limit_usd`` are observed after the fact, so
    they bound the run to within one trial's overshoot: the trial that
    crosses the ceiling is allowed to finish, and no further trial starts.
    That overshoot is inherent to observing a usage figure after the
    response, and it is reported rather than hidden.
    """

    policy: BudgetPolicy
    provider_requests: int = 0
    input_tokens: int = 0
    output_tokens: int = 0
    total_tokens: int = 0
    cost_usd: float = 0.0
    cost_observed: bool = False
    exhausted_cap: Optional[str] = None
    _outstanding: int = 0

    def admit(self, projected_provider_requests: int = 1) -> None:
        """Refuse to start provider work that would cross a cap.

        The projected requests are **reserved** on admission, so a trial that
        fails before its usage is observed still counts against the cap. That
        direction is deliberate: an over-count stops the run early, while an
        under-count would let a run exceed its ceiling.
        """
        if self.exhausted_cap is not None:
            raise BudgetExhausted(
                f"the {self.exhausted_cap} cap is exhausted; refusing further provider work")
        limit = self.policy.max_provider_requests
        if limit is not None and self.provider_requests + projected_provider_requests > limit:
            self.exhausted_cap = MISSING_MAX_PROVIDER_REQUESTS
            raise BudgetExhausted(
                f"the max_provider_requests cap of {limit} is exhausted; "
                f"refusing a request that would make {self.provider_requests + projected_provider_requests}")
        self.provider_requests += projected_provider_requests
        self._outstanding = projected_provider_requests

    def observe(self, *, provider_requests: int = 0, input_tokens: int = 0,
                output_tokens: int = 0, total_tokens: Optional[int] = None,
                cost_usd: Optional[float] = None) -> None:
        """Record the provider usage observed for a finished trial.

        The reservation made by the matching ``admit`` is replaced by the
        observed request count, so a trial that used more requests than
        projected still counts in full.
        """
        if self._outstanding:
            self.provider_requests += provider_requests - self._outstanding
            self._outstanding = 0
        else:
            self.provider_requests += provider_requests
        self.input_tokens += max(0, input_tokens)
        self.output_tokens += max(0, output_tokens)
        self.total_tokens += max(0, total_tokens if total_tokens is not None
                                 else input_tokens + output_tokens)
        if cost_usd is not None:
            self.cost_observed = True
            self.cost_usd += cost_usd
        if self.exhausted_cap is None:
            self.exhausted_cap = self._crossed_cap()

    def _crossed_cap(self) -> Optional[str]:
        limit = self.policy.token_limit
        if limit is not None and self.total_tokens >= limit:
            return MISSING_TOKEN_LIMIT
        cost_limit = self.policy.cost_limit_usd
        if cost_limit is not None and self.cost_observed and self.cost_usd >= cost_limit:
            return MISSING_COST_LIMIT
        return None

    def as_dict(self) -> Dict[str, Any]:
        """Return a JSON-serializable view of the observed spend."""
        return {"provider_requests": self.provider_requests,
                "input_tokens": self.input_tokens,
                "output_tokens": self.output_tokens,
                "total_tokens": self.total_tokens,
                "cost_usd": None if not self.cost_observed else self.cost_usd,
                "exhausted_cap": self.exhausted_cap}


def admit_budget(policy: BudgetPolicy) -> BudgetAdmission:
    """Decide whether a live trial may start under ``policy``.

    Fails closed: the result is ``STATUS_UNVERIFIABLE`` unless at least one
    enforceable hard cap is declared and no requested cap is unusable.
    """
    if not isinstance(policy, BudgetPolicy):
        raise TypeError("admit_budget requires a BudgetPolicy")

    enforced: List[str] = []
    invalid_caps: List[Dict[str, str]] = []
    # (priority index, missing-source name, reason) for requested-but-unusable caps.
    unusable: List[Tuple[int, str, str]] = []
    # (priority index, missing-source name) for caps that were never requested.
    absent: List[Tuple[int, str]] = []

    for index, name in enumerate(CAP_PRIORITY):
        if name == MISSING_TOKEN_LIMIT:
            value = policy.token_limit
            if value is None:
                absent.append((index, name))
                continue
            fault = _numeric_fault(value, integral=True)
            if fault is not None:
                invalid_caps.append({"cap": name, "reason": fault})
                unusable.append((index, name, fault))
                continue
            enforced.append(name)
        elif name == MISSING_COST_LIMIT:
            value = policy.cost_limit_usd
            if value is None:
                absent.append((index, name))
                continue
            fault = _numeric_fault(value, integral=False)
            if fault is not None:
                invalid_caps.append({"cap": name, "reason": fault})
                unusable.append((index, name, fault))
                continue
            enforcement = policy.cost_enforcement
            if enforcement not in COST_ENFORCEMENT_MODES:
                unusable.append((
                    index,
                    name,
                    "cost enforcement is not stated; "
                    "controller_enforced or provider_enforced is required",
                ))
                continue
            if enforcement == "controller_enforced" and policy.cost_observable is not True:
                # The controller can only hold a dollar ceiling it can
                # measure. A route that reports no cost cannot be enforced
                # against one, and a guessed price must never stand in.
                unusable.append((
                    index,
                    name,
                    "controller_enforced cost requires an observable provider cost; "
                    "the route reports none, so the cap cannot be enforced",
                ))
                continue
            enforced.append(name)
        elif name == MISSING_MAX_PROVIDER_REQUESTS:
            value = policy.max_provider_requests
            if value is None:
                absent.append((index, name))
                continue
            fault = _numeric_fault(value, integral=True)
            if fault is not None:
                invalid_caps.append({"cap": name, "reason": fault})
                unusable.append((index, name, fault))
                continue
            enforced.append(name)
        elif name == MISSING_MAX_WALLCLOCK_SECONDS:
            value = policy.max_wallclock_seconds
            if value is None:
                absent.append((index, name))
                continue
            fault = _numeric_fault(value, integral=False)
            if fault is not None:
                invalid_caps.append({"cap": name, "reason": fault})
                unusable.append((index, name, fault))
                continue
            enforced.append(name)
        else:  # provider quota
            quota = policy.provider_quota
            if quota is None:
                absent.append((index, name))
                continue
            if quota.enforceable is not True:
                unusable.append((index, name, "provider quota is not enforceable"))
                continue
            if quota.retry_safe is not True:
                unusable.append((
                    index,
                    MISSING_PROVIDER_QUOTA_RETRY_SAFETY,
                    "provider quota does not hold across provider retries",
                ))
                continue
            if quota.scope not in QUOTA_SCOPES:
                unusable.append((
                    index,
                    name,
                    "provider quota scope {!r} is not a known scope".format(quota.scope),
                ))
                continue
            enforced.append(name)

    unusable.sort(key=lambda item: item[0])
    absent.sort(key=lambda item: item[0])

    if unusable or not enforced:
        missing_sources = [name for _, name, _ in unusable]
        missing_sources.extend(name for _, name in absent)
        missing_source = missing_sources[0] if missing_sources else None
        if unusable:
            reason = "; ".join(
                "{}: {}".format(name, why) for _, name, why in unusable
            )
        else:
            reason = "no enforceable hard cap is declared (missing {})".format(
                ", ".join(missing_sources)
            )
        status = STATUS_UNVERIFIABLE
    else:
        missing_sources = []
        missing_source = None
        reason = ""
        status = STATUS_ADMITTED

    detail: Dict[str, Any] = {
        "reason": reason,
        "missing_sources": list(missing_sources),
        "invalid_caps": [dict(entry) for entry in invalid_caps],
    }
    return BudgetAdmission(
        status=status,
        enforced_caps=tuple(sorted(enforced)),
        missing_source=missing_source,
        detail=detail,
    )


def _parse_optional_int(value: Any, name: str) -> Optional[int]:
    """Parse an optional integer field. Values are validated by ``admit_budget``."""
    if value is None:
        return None
    if isinstance(value, bool):
        # A boolean is an int in Python; defer the value check to admission so
        # the refusal is reported as an invalid cap instead of a parse error.
        return value
    if not isinstance(value, int):
        raise ValueError("{} must be an integer or null".format(name))
    return value


def _parse_optional_number(value: Any, name: str) -> Optional[float]:
    """Parse an optional numeric field. Values are validated by ``admit_budget``."""
    if value is None:
        return None
    if isinstance(value, bool):
        return value
    if not isinstance(value, (int, float)):
        raise ValueError("{} must be a number or null".format(name))
    return value


def _parse_provider_quota(value: Any) -> ProviderQuota:
    if not isinstance(value, Mapping):
        raise ValueError("provider_quota must be a mapping or null")
    unknown = sorted(str(key) for key in value if key not in _QUOTA_KEYS)
    if unknown:
        raise ValueError("unknown provider_quota key(s): {}".format(", ".join(unknown)))
    for required in _QUOTA_REQUIRED_KEYS:
        if required not in value:
            raise ValueError("provider_quota is missing required key: {}".format(required))
    source = value["source"]
    if not isinstance(source, str) or not source.strip():
        raise ValueError("provider_quota.source must be a non-empty string")
    enforceable = value["enforceable"]
    if not isinstance(enforceable, bool):
        raise ValueError("provider_quota.enforceable must be a boolean")
    retry_safe = value["retry_safe"]
    if not isinstance(retry_safe, bool):
        raise ValueError("provider_quota.retry_safe must be a boolean")
    scope = value["scope"]
    if not isinstance(scope, str) or scope not in QUOTA_SCOPES:
        raise ValueError(
            "provider_quota.scope must be one of: {}".format(", ".join(QUOTA_SCOPES))
        )
    observed_limit = value.get("observed_limit")
    if observed_limit is not None:
        if isinstance(observed_limit, bool) or not isinstance(observed_limit, (int, float)):
            raise ValueError("provider_quota.observed_limit must be a number or null")
    observed_at = value.get("observed_at")
    if observed_at is not None and not isinstance(observed_at, str):
        raise ValueError("provider_quota.observed_at must be a string or null")
    return ProviderQuota(
        source=source,
        enforceable=enforceable,
        retry_safe=retry_safe,
        scope=scope,
        observed_limit=observed_limit,
        observed_at=observed_at,
    )


def budget_policy_from_mapping(mapping: Mapping[str, Any]) -> BudgetPolicy:
    """Build a :class:`BudgetPolicy` from a strict mapping.

    Unknown keys, unknown quota keys, wrong types, unknown enum values and an
    empty quota source raise :class:`ValueError`. A mapping that declares no
    cap at all is valid; it simply fails admission later.
    """
    if not isinstance(mapping, Mapping):
        raise ValueError("budget policy must be a mapping")
    unknown = sorted(str(key) for key in mapping if key not in _POLICY_KEYS)
    if unknown:
        raise ValueError("unknown budget policy key(s): {}".format(", ".join(unknown)))

    enforcement = mapping.get("cost_enforcement")
    if enforcement is not None:
        if not isinstance(enforcement, str):
            raise ValueError("cost_enforcement must be a string or null")
        if enforcement not in COST_ENFORCEMENT_MODES:
            raise ValueError(
                "cost_enforcement must be one of: {}".format(", ".join(COST_ENFORCEMENT_MODES))
            )

    quota_value = mapping.get("provider_quota")
    quota = None if quota_value is None else _parse_provider_quota(quota_value)

    cost_observable = mapping.get("cost_observable")
    if cost_observable is not None and not isinstance(cost_observable, bool):
        raise ValueError("cost_observable must be a boolean or null")

    return BudgetPolicy(
        token_limit=_parse_optional_int(mapping.get("token_limit"), "token_limit"),
        cost_limit_usd=_parse_optional_number(mapping.get("cost_limit_usd"), "cost_limit_usd"),
        cost_enforcement=enforcement,
        max_provider_requests=_parse_optional_int(
            mapping.get("max_provider_requests"), "max_provider_requests"
        ),
        max_wallclock_seconds=_parse_optional_number(
            mapping.get("max_wallclock_seconds"), "max_wallclock_seconds"
        ),
        provider_quota=quota,
        cost_observable=cost_observable,
    )
