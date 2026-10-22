"""Versioned score contract for harness-lab candidate qualification.

This module is the acceptance contract only. It decides whether a measured
candidate is an acceptable improvement over a measured baseline, and it can
refuse to decide ("insufficient evidence") instead of forcing a pass or a fail.
It is deliberately standalone and pure: no I/O, no clock, no randomness, no
network. The same inputs always produce the same receipt.

Trust boundary
--------------
The contract lives under ``dev/harness_lab/``, a path ``improve.check_scope``
forbids candidate patches from touching. A candidate therefore cannot edit the
contract that judges it, and the contract never reads candidate-authored text.

Null is never zero
------------------
``Observation.value is None`` (or ``measured is False``) means *not measured*.
It is never read as ``0``. A candidate that did not report usage has not
reported zero usage, so missing telemetry yields ``insufficient_evidence`` and
never an accepted zero-valued score. No metric is summed with ``or 0``
anywhere in this module.

Decision order
--------------
1. A protected negative-control regression rejects the candidate. This check
   runs first and is not overridable by any other signal.
2. ``evidence_pairs`` below the contract floor is ``insufficient_evidence``.
3. A required criterion that is unmeasured on either side is
   ``insufficient_evidence``.
4. Otherwise the weighted comparison decides ``accepted`` or ``rejected``:
   the candidate must improve at least one *substantive* comparable criterion
   and regress none.
5. The objective is classified from the substantive improvements.

Substantive improvement
------------------------
An improvement is *substantive* when its criterion declares
``improvement_accepts``. A criterion that declares it false is advisory: it
records what moved without letting that movement carry the acceptance. The
default contract declares both ``reliability.success_rate`` and
``efficiency.duration_ms`` advisory, so a candidate is accepted only by a
correctness result that actually moved — a defect that was remediated, not a
failure that was relabelled or a millisecond-scale artifact. A median
wall-clock delta is a fact about the machine that produced it, not evidence
that the candidate did the task better; accepting on it alone would let a
candidate that exited early — or changed nothing at all — buy an acceptance
with a millisecond-scale artifact. A contract that genuinely targets
reliability or latency must *predeclare* that intent by setting
``improvement_accepts`` on the criterion it optimizes, which is the plan's
"predeclared threshold" requirement made executable.

The advisory floor is symmetric: a criterion whose regression does not block
(``regression_blocks`` false) is advisory about improvement unless it says
otherwise. A regression is recorded either way; only acceptance is gated.

Objective semantics
-------------------
``objective`` names the declared objective a candidate's *measured
improvements* satisfy, in the precedence ``correctness_fix`` > ``reliability``
> ``efficiency``. It is ``none`` when the outcome is ``insufficient_evidence``
(no decision supports an objective) or when a protected control regressed (a
safety regression is not an improvement objective, and a correctness fix
requires its mandatory controls to hold). Protected criteria never contribute
to the improvement set, the objective, or the weighted aggregate: a negative
control is a guard, not an optimization target.
"""
from __future__ import annotations

import math
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from typing import Any, Dict, Optional, Tuple

SCORE_CONTRACT_VERSION = 2

DECISION_ACCEPTED = "accepted"
DECISION_REJECTED = "rejected"
DECISION_INSUFFICIENT_EVIDENCE = "insufficient_evidence"

OBJECTIVE_CORRECTNESS_FIX = "correctness_fix"
OBJECTIVE_RELIABILITY = "reliability"
OBJECTIVE_EFFICIENCY = "efficiency"
OBJECTIVE_NONE = "none"

CRITERION_KINDS = frozenset({"correctness", "reliability", "efficiency"})
CRITERION_DIRECTIONS = frozenset({"higher_is_better", "lower_is_better"})

_CONTRACT_KEYS = frozenset({"version", "criteria", "minimum_evidence_pairs"})
_CRITERION_KEYS = frozenset({"id", "kind", "direction", "weight", "protected", "required",
                             "regression_blocks", "improvement_accepts"})


def _require_text(value: Any, name: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{name} must be a non-empty string")
    return value


def _require_int(value: Any, name: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValueError(f"{name} must be an integer")
    return value


def _require_bool(value: Any, name: str) -> bool:
    if not isinstance(value, bool):
        raise ValueError(f"{name} must be a boolean")
    return value


def _require_weight(value: Any, name: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"{name} must be a real number")
    number = float(value)
    if not math.isfinite(number):
        raise ValueError(f"{name} must be finite")
    if number < 0:
        raise ValueError(f"{name} must not be negative")
    return number


@dataclass(frozen=True)
class Criterion:
    """One predeclared acceptance criterion.

    id: stable machine-readable identifier, unique within a contract.
    kind: objective family, one of ``correctness``, ``reliability``,
        ``efficiency``.
    direction: ``higher_is_better`` or ``lower_is_better``.
    weight: non-negative finite weight used by the weighted aggregate.
    protected: a protected negative control. Its regression rejects the
        candidate; it never contributes to the improvement set, the objective,
        or the weighted aggregate.
    required: when true the criterion must be measured on both sides, or the
        outcome is ``insufficient_evidence``.
    regression_blocks: when true a regression in this criterion rejects the
        candidate. A criterion whose regression is advisory records the
        regression without blocking acceptance. This is the contract's
        regression floor: a correctness fix is not disqualified by a slightly
        slower run, while a correctness or reliability regression always is.
    improvement_accepts: when true an improvement in this criterion is
        *substantive* and can accept a candidate on its own. When false the
        criterion is advisory: it records what moved without letting that
        movement carry the acceptance. A marginal latency win over a
        candidate that changed no measured outcome is not an improvement, so
        the default contract declares the raw duration advisory and a
        deliberate latency campaign must predeclare ``improvement_accepts``
        on the criterion it optimizes.
    """

    id: str
    kind: str
    direction: str
    weight: float = 1.0
    protected: bool = False
    required: bool = True
    regression_blocks: bool = True
    improvement_accepts: bool = True

    def __post_init__(self) -> None:
        _require_text(self.id, "criterion id")
        if not isinstance(self.kind, str) or self.kind not in CRITERION_KINDS:
            raise ValueError(f"unsupported criterion kind: {self.kind!r}")
        if not isinstance(self.direction, str) or self.direction not in CRITERION_DIRECTIONS:
            raise ValueError(f"unsupported criterion direction: {self.direction!r}")
        _require_weight(self.weight, "criterion weight")
        _require_bool(self.protected, "criterion protected")
        _require_bool(self.required, "criterion required")
        _require_bool(self.regression_blocks, "criterion regression_blocks")
        _require_bool(self.improvement_accepts, "criterion improvement_accepts")


@dataclass(frozen=True)
class ScoreContract:
    """A frozen, versioned set of criteria plus its evidence floor."""

    version: int
    criteria: Tuple[Criterion, ...]
    minimum_evidence_pairs: int = 1

    def __post_init__(self) -> None:
        version = _require_int(self.version, "contract version")
        if version != SCORE_CONTRACT_VERSION:
            raise ValueError(f"unsupported score contract version: {version}")
        minimum = _require_int(self.minimum_evidence_pairs, "minimum_evidence_pairs")
        if minimum < 1:
            raise ValueError("minimum_evidence_pairs must be at least 1")
        criteria = tuple(self.criteria)
        for criterion in criteria:
            if not isinstance(criterion, Criterion):
                raise ValueError("contract criteria must be Criterion instances")
        identifiers = [criterion.id for criterion in criteria]
        if len(set(identifiers)) != len(identifiers):
            raise ValueError("contract criterion ids must be unique")
        object.__setattr__(self, "criteria", criteria)

    def criterion(self, criterion_id: str) -> Criterion:
        """Return the criterion with ``criterion_id`` or raise ``KeyError``."""
        for criterion in self.criteria:
            if criterion.id == criterion_id:
                return criterion
        raise KeyError(criterion_id)

    def as_dict(self) -> Dict[str, Any]:
        """Return a JSON-serializable view of the contract."""
        return {"version": self.version,
                "minimum_evidence_pairs": self.minimum_evidence_pairs,
                "criteria": [{"id": criterion.id,
                              "kind": criterion.kind,
                              "direction": criterion.direction,
                              "weight": float(criterion.weight),
                              "protected": criterion.protected,
                              "required": criterion.required,
                              "regression_blocks": criterion.regression_blocks,
                              "improvement_accepts": criterion.improvement_accepts}
                             for criterion in self.criteria]}


@dataclass(frozen=True)
class Observation:
    """One criterion value observed on one side of a comparison.

    ``value is None`` means the metric was not measured. It is never ``0``.
    ``measured is False`` marks the metric as unavailable even if a value was
    supplied.
    """

    criterion_id: str
    value: Optional[float]
    measured: bool = True

    def __post_init__(self) -> None:
        _require_text(self.criterion_id, "observation criterion_id")
        if self.value is not None:
            if isinstance(self.value, bool) or not isinstance(self.value, (int, float)):
                raise ValueError("observation value must be a real number or None")
            if not math.isfinite(float(self.value)):
                raise ValueError("observation value must be finite")
        _require_bool(self.measured, "observation measured")


@dataclass(frozen=True)
class ScoreOutcome:
    """The contract's verdict for one candidate.

    ``score`` is ``None`` whenever the decision is ``insufficient_evidence``:
    a missing measurement must never be representable as a zero score. It is
    also ``None`` when no criterion was comparable on both sides, because an
    undefined aggregate is not a zero aggregate.

    ``objective`` names the objective the candidate's substantive improvements
    satisfy. It is ``none`` when the outcome is ``insufficient_evidence`` (no
    decision supports an objective), when a protected control regressed (a
    safety regression is not an improvement objective, and a correctness fix
    requires its mandatory controls to hold), and when only advisory criteria
    improved (a movement that cannot carry an acceptance cannot name one
    either).
    """

    decision: str
    score: Optional[float]
    objective: str
    reasons: Tuple[str, ...] = ()
    protected_violations: Tuple[str, ...] = ()

    def as_dict(self) -> Dict[str, Any]:
        """Return a JSON-serializable view of the outcome."""
        return {"decision": self.decision,
                "score": None if self.score is None else float(self.score),
                "objective": self.objective,
                "reasons": list(self.reasons),
                "protected_violations": list(self.protected_violations)}


def default_contract() -> ScoreContract:
    """Return the default contract for harness-lab candidate qualification.

    The criteria are:

    ``correctness.pass_rate`` (correctness, higher is better): fraction of
        non-control task cases whose expected terminal is independently
        confirmed at the candidate. This is the defect-fix signal.
    ``reliability.success_rate`` (reliability, higher is better): fraction of
        paired trials that completed without an avoidable tool failure, a
        premature stall, or an infrastructure error. It is **reported, not
        accepting**: a candidate that removed one avoidable tool failure but
        left the task failing has not remediated a defect, and a candidate
        loop that accepts it would buy an acceptance by changing how a
        failure is labelled.
    ``efficiency.duration_ms`` (efficiency, lower is better): median
        wall-clock duration of the paired trials. Its weight keeps a
        millisecond-scale metric commensurate with the ``[0, 1]`` rates in the
        weighted aggregate. It is **advisory in both directions**: a slower
        run is recorded but does not disqualify a candidate that fixed a real
        defect, and a faster run does not accept a candidate that fixed
        nothing. A candidate is accepted only by a substantive improvement —
        a correctness result that actually moved.
    ``control.no_regression`` (correctness, higher is better, protected):
        fraction of the protected negative-control cases that still terminate
        as expected. A candidate that breaks an expected denial, failure,
        cancellation, or timeout is a safety regression, not an improvement;
        the control is a guard, so it never counts toward the objective.

    Only ``correctness.pass_rate`` accepts an improvement in the default
    contract, because only a correctness result is a *defect remediation*:
    a non-control task that failed at baseline now meets its expected
    terminal. The milestone's acceptance rule is exactly that — a candidate
    is accepted for a verified correctness improvement, and a candidate that
    changed nothing else is rejected rather than celebrated. ``reliability``
    and ``efficiency`` remain measured and reported, and a contract that
    genuinely runs a reliability or latency campaign declares that intent by
    setting ``improvement_accepts`` on the criterion it optimizes.

    ``minimum_evidence_pairs`` is 1. The orchestrator may raise it to the
    number of paired repeats it actually measured.
    """
    return ScoreContract(
        version=SCORE_CONTRACT_VERSION,
        minimum_evidence_pairs=1,
        criteria=(
            Criterion(id="correctness.pass_rate", kind="correctness",
                      direction="higher_is_better", weight=1.0),
            Criterion(id="reliability.success_rate", kind="reliability",
                      direction="higher_is_better", weight=1.0,
                      improvement_accepts=False),
            Criterion(id="efficiency.duration_ms", kind="efficiency",
                      direction="lower_is_better", weight=0.001,
                      regression_blocks=False, improvement_accepts=False),
            Criterion(id="control.no_regression", kind="correctness",
                      direction="higher_is_better", weight=1.0, protected=True),
        ))


def contract_from_mapping(mapping: Mapping[str, Any]) -> ScoreContract:
    """Build a strict, versioned contract from a mapping.

    The mapping shape is ``{"version": 1, "minimum_evidence_pairs": 1,
    "criteria": [{"id": ..., "kind": ..., "direction": ..., "weight": ...,
    "protected": ..., "required": ...}]}``. Unknown keys, an unsupported
    version, a malformed criterion, a duplicate or empty criterion id, a
    negative or non-finite weight, and a ``minimum_evidence_pairs`` below one
    all raise ``ValueError``. An unsupported version is rejected, never
    silently reinterpreted.
    """
    if not isinstance(mapping, Mapping):
        raise ValueError("score contract must be a mapping")
    unknown = sorted(str(key) for key in set(mapping) - _CONTRACT_KEYS)
    if unknown:
        raise ValueError(f"unsupported score contract keys: {', '.join(unknown)}")
    if "version" not in mapping:
        raise ValueError("score contract requires a version")
    version = _require_int(mapping["version"], "contract version")
    if version != SCORE_CONTRACT_VERSION:
        raise ValueError(f"unsupported score contract version: {version}")
    if "criteria" not in mapping:
        raise ValueError("score contract requires criteria")
    raw_criteria = mapping["criteria"]
    if not isinstance(raw_criteria, (list, tuple)):
        raise ValueError("score contract criteria must be a list")
    criteria = []
    for entry in raw_criteria:
        if not isinstance(entry, Mapping):
            raise ValueError("score contract criterion must be a mapping")
        unknown = sorted(str(key) for key in set(entry) - _CRITERION_KEYS)
        if unknown:
            raise ValueError(f"unsupported criterion keys: {', '.join(unknown)}")
        for field in ("id", "kind", "direction"):
            if field not in entry:
                raise ValueError(f"score contract criterion requires a {field}")
        criteria.append(Criterion(id=entry["id"], kind=entry["kind"],
                                  direction=entry["direction"],
                                  weight=entry.get("weight", 1.0),
                                  protected=entry.get("protected", False),
                                  required=entry.get("required", True),
                                  regression_blocks=entry.get("regression_blocks", True),
                                  improvement_accepts=entry.get("improvement_accepts", True)))
    return ScoreContract(version=version, criteria=tuple(criteria),
                         minimum_evidence_pairs=mapping.get("minimum_evidence_pairs", 1))


def _is_measured(observation: Optional[Observation]) -> bool:
    """Return True only for an observation that carries a real measurement."""
    return observation is not None and observation.measured and observation.value is not None


def _is_better(criterion: Criterion, candidate: float, baseline: float) -> bool:
    if criterion.direction == "lower_is_better":
        return candidate < baseline
    return candidate > baseline


def _is_worse(criterion: Criterion, candidate: float, baseline: float) -> bool:
    if criterion.direction == "lower_is_better":
        return candidate > baseline
    return candidate < baseline


def _normalized(criterion: Criterion, value: float) -> float:
    """Flip a lower-is-better value so that larger always means better."""
    return -value if criterion.direction == "lower_is_better" else value


def _index_observations(observations: Sequence[Observation], side: str) -> Dict[str, Observation]:
    """Index one side's observations, refusing ambiguous duplicates."""
    indexed: Dict[str, Observation] = {}
    for observation in observations:
        if not isinstance(observation, Observation):
            raise ValueError(f"{side} observations must be Observation instances")
        if observation.criterion_id in indexed:
            raise ValueError(f"{side} side has a duplicate observation for criterion "
                             f"{observation.criterion_id}")
        indexed[observation.criterion_id] = observation
    return indexed


def _weighted_aggregate(criteria: Sequence[Criterion],
                        observations: Mapping[str, Observation]) -> Optional[float]:
    """Return the weighted mean of the normalized candidate values.

    ``None`` is returned when no criterion is comparable or when the
    comparable weights sum to zero: an undefined aggregate is never reported
    as ``0.0``.
    """
    if not criteria:
        return None
    total_weight = sum(criterion.weight for criterion in criteria)
    if total_weight <= 0:
        return None
    total = sum(criterion.weight * _normalized(criterion, observations[criterion.id].value)
                for criterion in criteria)
    return float(total / total_weight)


def score_candidate(contract: ScoreContract,
                    baseline: Sequence[Observation],
                    candidate: Sequence[Observation],
                    *,
                    evidence_pairs: int = 0) -> ScoreOutcome:
    """Score a candidate against a baseline under ``contract``.

    ``baseline`` and ``candidate`` carry one observation per criterion per
    side. ``evidence_pairs`` is the number of complete paired trials the
    caller actually measured; the contract's ``minimum_evidence_pairs`` is the
    floor below which no decision is possible.
    """
    if not isinstance(contract, ScoreContract):
        raise ValueError("score_candidate requires a ScoreContract")
    if isinstance(evidence_pairs, bool) or not isinstance(evidence_pairs, int):
        raise ValueError("evidence_pairs must be an integer")
    baseline_by_id = _index_observations(baseline, "baseline")
    candidate_by_id = _index_observations(candidate, "candidate")

    protected_violations = []
    for criterion in contract.criteria:
        if not criterion.protected:
            continue
        base = baseline_by_id.get(criterion.id)
        cand = candidate_by_id.get(criterion.id)
        if not (_is_measured(base) and _is_measured(cand)):
            continue
        if _is_worse(criterion, cand.value, base.value):
            protected_violations.append(criterion.id)

    comparable = [criterion for criterion in contract.criteria
                  if not criterion.protected
                  and _is_measured(baseline_by_id.get(criterion.id))
                  and _is_measured(candidate_by_id.get(criterion.id))]
    score = _weighted_aggregate(comparable, candidate_by_id)
    baseline_score = _weighted_aggregate(comparable, baseline_by_id)

    # 1. A protected negative control regressed: reject, whatever else changed.
    if protected_violations:
        return ScoreOutcome(decision=DECISION_REJECTED, score=score, objective=OBJECTIVE_NONE,
                            reasons=("protected_control_regressed",),
                            protected_violations=tuple(protected_violations))

    # 2. Not enough complete paired trials to decide anything.
    if evidence_pairs < contract.minimum_evidence_pairs:
        return ScoreOutcome(decision=DECISION_INSUFFICIENT_EVIDENCE, score=None,
                            objective=OBJECTIVE_NONE, reasons=("insufficient_evidence_pairs",))

    # 3. A required criterion was not measured on both sides.
    if any(criterion.required
           and not (_is_measured(baseline_by_id.get(criterion.id))
                    and _is_measured(candidate_by_id.get(criterion.id)))
           for criterion in contract.criteria):
        return ScoreOutcome(decision=DECISION_INSUFFICIENT_EVIDENCE, score=None,
                            objective=OBJECTIVE_NONE, reasons=("required_criterion_unmeasured",))

    # 4. Weighted comparison over the criteria measured on both sides.
    improved = [criterion for criterion in comparable
                if _is_better(criterion, candidate_by_id[criterion.id].value,
                              baseline_by_id[criterion.id].value)]
    regressed = [criterion for criterion in comparable
                 if _is_worse(criterion, candidate_by_id[criterion.id].value,
                              baseline_by_id[criterion.id].value)]
    # An improvement is *substantive* when its criterion declares that an
    # improvement may accept a candidate on its own. An advisory improvement is
    # recorded but never carries the acceptance: a marginal latency win over a
    # candidate that changed no measured outcome is not an improvement.
    substantive = [criterion for criterion in improved if criterion.improvement_accepts]

    reasons = []
    objective = OBJECTIVE_NONE
    for kind, reason, value in (("correctness", "correctness_criterion_improved",
                                 OBJECTIVE_CORRECTNESS_FIX),
                                ("reliability", "reliability_criterion_improved",
                                 OBJECTIVE_RELIABILITY),
                                ("efficiency", "efficiency_criterion_improved",
                                 OBJECTIVE_EFFICIENCY)):
        if any(criterion.kind == kind for criterion in improved):
            reasons.append(reason)
        # The objective names what the *substantive* improvements satisfy. An
        # advisory movement is reported, but it never names an objective.
        if objective == OBJECTIVE_NONE and any(criterion.kind == kind for criterion in substantive):
            objective = value
    if not improved:
        reasons.append("no_criterion_improved")
    elif not substantive:
        reasons.append("no_substantive_criterion_improved")
    # A regression only rejects the candidate when the criterion declares a
    # blocking floor. An advisory regression is recorded, because a defect
    # fix that is slightly slower is still a defect fix.
    blocking_regressed = [criterion for criterion in regressed if criterion.regression_blocks]
    advisory_regressed = [criterion for criterion in regressed if not criterion.regression_blocks]
    if blocking_regressed:
        reasons.append("criterion_regressed")
    if advisory_regressed:
        reasons.append("secondary_criterion_regressed")
    if score is not None and baseline_score is not None and score < baseline_score:
        reasons.append("weighted_score_below_baseline")

    # 5. Accepted only when something substantive improved and no blocking
    #    floor regressed.
    decision = (DECISION_ACCEPTED if substantive and not blocking_regressed
                else DECISION_REJECTED)
    return ScoreOutcome(decision=decision, score=score, objective=objective,
                        reasons=tuple(reasons), protected_violations=())
