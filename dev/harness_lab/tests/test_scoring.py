import json
import math
import unittest

from dev.harness_lab.scoring import (DECISION_ACCEPTED, DECISION_INSUFFICIENT_EVIDENCE,
                                     DECISION_REJECTED, OBJECTIVE_CORRECTNESS_FIX,
                                     OBJECTIVE_EFFICIENCY, OBJECTIVE_NONE, OBJECTIVE_RELIABILITY,
                                     SCORE_CONTRACT_VERSION, Criterion, Observation,
                                     ScoreContract, ScoreOutcome, contract_from_mapping,
                                     default_contract, score_candidate)


def _contract(criteria, minimum_evidence_pairs=1):
    return ScoreContract(version=SCORE_CONTRACT_VERSION, criteria=tuple(criteria),
                         minimum_evidence_pairs=minimum_evidence_pairs)


def _obs(criterion_id, value, measured=True):
    return Observation(criterion_id=criterion_id, value=value, measured=measured)


def _strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for key, item in value.items():
            yield key
            yield from _strings(item)
    elif isinstance(value, (list, tuple)):
        for item in value:
            yield from _strings(item)


class DefaultContractTests(unittest.TestCase):
    def test_default_contract_is_valid_versioned_and_json_serializable(self) -> None:
        contract = default_contract()
        self.assertEqual(contract.version, SCORE_CONTRACT_VERSION)
        self.assertGreaterEqual(contract.minimum_evidence_pairs, 1)
        kinds = {criterion.kind for criterion in contract.criteria}
        self.assertEqual(kinds, {"correctness", "reliability", "efficiency"})
        directions = {criterion.direction for criterion in contract.criteria}
        self.assertEqual(directions, {"higher_is_better", "lower_is_better"})
        self.assertTrue(any(criterion.protected for criterion in contract.criteria))
        self.assertTrue(all(criterion.required for criterion in contract.criteria))
        for criterion in contract.criteria:
            self.assertIs(contract.criterion(criterion.id), criterion)
        with self.assertRaises(KeyError):
            contract.criterion("absent.criterion")
        encoded = json.dumps(contract.as_dict(), sort_keys=True)
        self.assertIn("correctness.pass_rate", encoded)
        self.assertEqual(contract_from_mapping(contract.as_dict()), contract)


class ObjectiveTests(unittest.TestCase):
    def _contract(self):
        return _contract([
            Criterion(id="correctness.pass_rate", kind="correctness", direction="higher_is_better"),
            Criterion(id="reliability.success_rate", kind="reliability", direction="higher_is_better"),
            Criterion(id="efficiency.duration_ms", kind="efficiency", direction="lower_is_better"),
        ])

    def test_correctness_improvement_is_accepted_as_a_correctness_fix(self) -> None:
        outcome = score_candidate(
            self._contract(),
            [_obs("correctness.pass_rate", 0.5), _obs("reliability.success_rate", 0.9),
             _obs("efficiency.duration_ms", 1000.0)],
            [_obs("correctness.pass_rate", 1.0), _obs("reliability.success_rate", 0.9),
             _obs("efficiency.duration_ms", 1000.0)],
            evidence_pairs=1)
        self.assertEqual(outcome.decision, DECISION_ACCEPTED)
        self.assertEqual(outcome.objective, OBJECTIVE_CORRECTNESS_FIX)
        self.assertIn("correctness_criterion_improved", outcome.reasons)
        self.assertEqual(outcome.protected_violations, ())
        self.assertIsNotNone(outcome.score)

    def test_reliability_only_improvement_is_accepted_as_reliability(self) -> None:
        outcome = score_candidate(
            self._contract(),
            [_obs("correctness.pass_rate", 1.0), _obs("reliability.success_rate", 0.5),
             _obs("efficiency.duration_ms", 1000.0)],
            [_obs("correctness.pass_rate", 1.0), _obs("reliability.success_rate", 0.8),
             _obs("efficiency.duration_ms", 1000.0)],
            evidence_pairs=1)
        self.assertEqual(outcome.decision, DECISION_ACCEPTED)
        self.assertEqual(outcome.objective, OBJECTIVE_RELIABILITY)
        self.assertIn("reliability_criterion_improved", outcome.reasons)
        self.assertNotIn("correctness_criterion_improved", outcome.reasons)

    def test_efficiency_only_improvement_is_accepted_as_efficiency(self) -> None:
        outcome = score_candidate(
            self._contract(),
            [_obs("correctness.pass_rate", 1.0), _obs("reliability.success_rate", 0.9),
             _obs("efficiency.duration_ms", 1000.0)],
            [_obs("correctness.pass_rate", 1.0), _obs("reliability.success_rate", 0.9),
             _obs("efficiency.duration_ms", 700.0)],
            evidence_pairs=1)
        self.assertEqual(outcome.decision, DECISION_ACCEPTED)
        self.assertEqual(outcome.objective, OBJECTIVE_EFFICIENCY)
        self.assertIn("efficiency_criterion_improved", outcome.reasons)


class ProtectedControlTests(unittest.TestCase):
    def _contract(self):
        return _contract([
            Criterion(id="correctness.pass_rate", kind="correctness", direction="higher_is_better"),
            Criterion(id="control.no_regression", kind="correctness", direction="higher_is_better",
                      protected=True),
        ])

    def test_protected_control_regression_rejects_even_with_other_improvements(self) -> None:
        outcome = score_candidate(
            self._contract(),
            [_obs("correctness.pass_rate", 0.5), _obs("control.no_regression", 1.0)],
            [_obs("correctness.pass_rate", 1.0), _obs("control.no_regression", 0.0)],
            evidence_pairs=1)
        self.assertEqual(outcome.decision, DECISION_REJECTED)
        self.assertEqual(outcome.protected_violations, ("control.no_regression",))
        self.assertIn("protected_control_regressed", outcome.reasons)
        self.assertEqual(outcome.objective, OBJECTIVE_NONE)

    def test_protected_regression_wins_over_insufficient_evidence(self) -> None:
        outcome = score_candidate(
            self._contract(),
            [_obs("correctness.pass_rate", 0.5), _obs("control.no_regression", 1.0)],
            [_obs("correctness.pass_rate", 1.0), _obs("control.no_regression", 0.0),
             _obs("reliability.success_rate", None, measured=False)],
            evidence_pairs=0)
        self.assertEqual(outcome.decision, DECISION_REJECTED)
        self.assertEqual(outcome.protected_violations, ("control.no_regression",))
        self.assertEqual(outcome.reasons, ("protected_control_regressed",))

    def test_improved_protected_control_is_not_an_objective(self) -> None:
        contract = _contract([
            Criterion(id="control.no_regression", kind="correctness", direction="higher_is_better",
                      protected=True),
        ])
        outcome = score_candidate(contract, [_obs("control.no_regression", 0.0)],
                                  [_obs("control.no_regression", 1.0)], evidence_pairs=1)
        self.assertEqual(outcome.decision, DECISION_REJECTED)
        self.assertEqual(outcome.objective, OBJECTIVE_NONE)
        self.assertIn("no_criterion_improved", outcome.reasons)


class InsufficientEvidenceTests(unittest.TestCase):
    def _contract(self):
        return _contract([
            Criterion(id="correctness.pass_rate", kind="correctness", direction="higher_is_better"),
            Criterion(id="reliability.success_rate", kind="reliability", direction="higher_is_better"),
        ], minimum_evidence_pairs=2)

    def test_unmeasured_required_criterion_is_insufficient_evidence_without_a_score(self) -> None:
        outcome = score_candidate(
            self._contract(),
            [_obs("correctness.pass_rate", 0.5), _obs("reliability.success_rate", 0.9)],
            [_obs("correctness.pass_rate", 1.0), _obs("reliability.success_rate", None, measured=False)],
            evidence_pairs=2)
        self.assertEqual(outcome.decision, DECISION_INSUFFICIENT_EVIDENCE)
        self.assertIsNone(outcome.score)
        self.assertIsNot(outcome.score, 0.0)
        self.assertEqual(outcome.reasons, ("required_criterion_unmeasured",))
        self.assertEqual(outcome.objective, OBJECTIVE_NONE)

    def test_evidence_pairs_below_the_contract_floor_is_insufficient_evidence(self) -> None:
        outcome = score_candidate(
            self._contract(),
            [_obs("correctness.pass_rate", 0.5), _obs("reliability.success_rate", 0.9)],
            [_obs("correctness.pass_rate", 1.0), _obs("reliability.success_rate", 0.95)],
            evidence_pairs=1)
        self.assertEqual(outcome.decision, DECISION_INSUFFICIENT_EVIDENCE)
        self.assertIsNone(outcome.score)
        self.assertEqual(outcome.reasons, ("insufficient_evidence_pairs",))

    def test_missing_usage_is_never_read_as_zero_usage(self) -> None:
        contract = _contract([
            Criterion(id="correctness.pass_rate", kind="correctness", direction="higher_is_better"),
            Criterion(id="efficiency.duration_ms", kind="efficiency", direction="lower_is_better",
                      required=False),
        ])
        baseline = [_obs("correctness.pass_rate", 1.0), _obs("efficiency.duration_ms", 1000.0)]
        candidate = [_obs("correctness.pass_rate", 1.0),
                     _obs("efficiency.duration_ms", None, measured=False)]
        outcome = score_candidate(contract, baseline, candidate, evidence_pairs=1)
        # The unmeasured optional criterion is not comparable, so only the
        # measured correctness criterion contributes. A zero coerced for the
        # missing usage would have produced (1.0 + 0.0) / 2 == 0.5.
        self.assertEqual(outcome.score, 1.0)
        self.assertNotEqual(outcome.score, 0.5)
        self.assertEqual(outcome.decision, DECISION_REJECTED)
        self.assertIn("no_criterion_improved", outcome.reasons)

    def test_unmeasured_required_criterion_never_yields_a_zero_score(self) -> None:
        contract = _contract([
            Criterion(id="correctness.pass_rate", kind="correctness", direction="higher_is_better"),
        ])
        outcome = score_candidate(contract, [_obs("correctness.pass_rate", 0.5)],
                                  [_obs("correctness.pass_rate", None)], evidence_pairs=1)
        self.assertEqual(outcome.decision, DECISION_INSUFFICIENT_EVIDENCE)
        self.assertIsNone(outcome.score)
        self.assertNotEqual(outcome.score, 0.0)


class ContractStrictnessTests(unittest.TestCase):
    def _mapping(self, **overrides):
        mapping = {"version": SCORE_CONTRACT_VERSION,
                   "minimum_evidence_pairs": 1,
                   "criteria": [{"id": "correctness.pass_rate", "kind": "correctness",
                                 "direction": "higher_is_better"}]}
        mapping.update(overrides)
        return mapping

    def test_unsupported_contract_version_is_rejected(self) -> None:
        for version in (0, 2, SCORE_CONTRACT_VERSION + 1):
            with self.subTest(version=version):
                with self.assertRaises(ValueError):
                    contract_from_mapping(self._mapping(version=version))

    def test_missing_version_is_rejected(self) -> None:
        mapping = self._mapping()
        del mapping["version"]
        with self.assertRaises(ValueError):
            contract_from_mapping(mapping)

    def test_boolean_version_is_rejected(self) -> None:
        with self.assertRaises(ValueError):
            contract_from_mapping(self._mapping(version=True))

    def test_unknown_contract_key_is_rejected(self) -> None:
        with self.assertRaises(ValueError):
            contract_from_mapping(self._mapping(extra="value"))

    def test_unknown_criterion_key_is_rejected(self) -> None:
        mapping = self._mapping()
        mapping["criteria"][0]["extra"] = "value"
        with self.assertRaises(ValueError):
            contract_from_mapping(mapping)

    def test_bad_kind_is_rejected(self) -> None:
        mapping = self._mapping()
        mapping["criteria"][0]["kind"] = "safety"
        with self.assertRaises(ValueError):
            contract_from_mapping(mapping)

    def test_bad_direction_is_rejected(self) -> None:
        mapping = self._mapping()
        mapping["criteria"][0]["direction"] = "sideways"
        with self.assertRaises(ValueError):
            contract_from_mapping(mapping)

    def test_duplicate_criterion_id_is_rejected(self) -> None:
        mapping = self._mapping()
        mapping["criteria"].append({"id": "correctness.pass_rate", "kind": "correctness",
                                    "direction": "higher_is_better"})
        with self.assertRaises(ValueError):
            contract_from_mapping(mapping)

    def test_empty_criterion_id_is_rejected(self) -> None:
        mapping = self._mapping()
        mapping["criteria"][0]["id"] = ""
        with self.assertRaises(ValueError):
            contract_from_mapping(mapping)

    def test_negative_and_non_finite_weights_are_rejected(self) -> None:
        for weight in (-1.0, float("inf"), float("nan"), -0.5):
            with self.subTest(weight=weight):
                mapping = self._mapping()
                mapping["criteria"][0]["weight"] = weight
                with self.assertRaises(ValueError):
                    contract_from_mapping(mapping)

    def test_minimum_evidence_pairs_below_one_is_rejected(self) -> None:
        for minimum in (0, -1):
            with self.subTest(minimum=minimum):
                with self.assertRaises(ValueError):
                    contract_from_mapping(self._mapping(minimum_evidence_pairs=minimum))

    def test_malformed_mapping_shapes_are_rejected(self) -> None:
        for mapping in ([], "contract", {"version": SCORE_CONTRACT_VERSION},
                        {"version": SCORE_CONTRACT_VERSION, "criteria": {}},
                        {"version": SCORE_CONTRACT_VERSION, "criteria": ["criterion"]}):
            with self.subTest(mapping=mapping):
                with self.assertRaises(ValueError):
                    contract_from_mapping(mapping)


class ComparisonTests(unittest.TestCase):
    def test_criterion_present_on_one_side_only_is_not_compared(self) -> None:
        contract = _contract([
            Criterion(id="correctness.pass_rate", kind="correctness", direction="higher_is_better"),
            Criterion(id="reliability.success_rate", kind="reliability", direction="higher_is_better",
                      required=False),
        ])
        outcome = score_candidate(contract,
                                  [_obs("correctness.pass_rate", 1.0),
                                   _obs("reliability.success_rate", 0.5)],
                                  [_obs("correctness.pass_rate", 1.0)], evidence_pairs=1)
        self.assertEqual(outcome.decision, DECISION_REJECTED)
        self.assertIn("no_criterion_improved", outcome.reasons)

    def test_one_sided_required_criterion_is_insufficient_evidence(self) -> None:
        contract = _contract([
            Criterion(id="correctness.pass_rate", kind="correctness", direction="higher_is_better"),
            Criterion(id="reliability.success_rate", kind="reliability", direction="higher_is_better"),
        ])
        outcome = score_candidate(contract, [_obs("correctness.pass_rate", 1.0)],
                                  [_obs("correctness.pass_rate", 1.0)], evidence_pairs=1)
        self.assertEqual(outcome.decision, DECISION_INSUFFICIENT_EVIDENCE)
        self.assertIsNone(outcome.score)
        self.assertEqual(outcome.reasons, ("required_criterion_unmeasured",))

    def test_regression_without_improvement_is_rejected(self) -> None:
        contract = _contract([
            Criterion(id="correctness.pass_rate", kind="correctness", direction="higher_is_better"),
            Criterion(id="efficiency.duration_ms", kind="efficiency", direction="lower_is_better"),
        ])
        outcome = score_candidate(contract,
                                  [_obs("correctness.pass_rate", 1.0), _obs("efficiency.duration_ms", 500.0)],
                                  [_obs("correctness.pass_rate", 1.0), _obs("efficiency.duration_ms", 900.0)],
                                  evidence_pairs=1)
        self.assertEqual(outcome.decision, DECISION_REJECTED)
        self.assertIn("criterion_regressed", outcome.reasons)
        self.assertIn("weighted_score_below_baseline", outcome.reasons)
        self.assertIn("no_criterion_improved", outcome.reasons)

    def test_improvement_with_a_regression_is_rejected(self) -> None:
        contract = _contract([
            Criterion(id="correctness.pass_rate", kind="correctness", direction="higher_is_better"),
            Criterion(id="efficiency.duration_ms", kind="efficiency", direction="lower_is_better"),
        ])
        outcome = score_candidate(contract,
                                  [_obs("correctness.pass_rate", 0.5), _obs("efficiency.duration_ms", 500.0)],
                                  [_obs("correctness.pass_rate", 1.0), _obs("efficiency.duration_ms", 900.0)],
                                  evidence_pairs=1)
        self.assertEqual(outcome.decision, DECISION_REJECTED)
        self.assertIn("criterion_regressed", outcome.reasons)

    def test_duplicate_observations_are_refused(self) -> None:
        contract = _contract([
            Criterion(id="correctness.pass_rate", kind="correctness", direction="higher_is_better"),
        ])
        with self.assertRaises(ValueError):
            score_candidate(contract,
                            [_obs("correctness.pass_rate", 1.0), _obs("correctness.pass_rate", 0.5)],
                            [_obs("correctness.pass_rate", 1.0)], evidence_pairs=1)


class ReceiptTests(unittest.TestCase):
    def test_outcome_as_dict_is_json_serializable_without_paths_or_secrets(self) -> None:
        outcome = score_candidate(
            default_contract(),
            [_obs("correctness.pass_rate", 0.5), _obs("reliability.success_rate", 0.9),
             _obs("efficiency.duration_ms", 1000.0), _obs("control.no_regression", 1.0)],
            [_obs("correctness.pass_rate", 1.0), _obs("reliability.success_rate", 0.9),
             _obs("efficiency.duration_ms", 1000.0), _obs("control.no_regression", 1.0)],
            evidence_pairs=1)
        payload = outcome.as_dict()
        encoded = json.dumps(payload, sort_keys=True)
        self.assertIn('"decision": "accepted"', encoded)
        for text in _strings(payload):
            self.assertNotIn("/", text)
            self.assertNotIn("\\", text)
            self.assertNotIn("~", text)
        for key in payload:
            self.assertNotIn(key.lower(), ("token", "secret", "password", "api_key"))
        self.assertIsInstance(payload["score"], float)
        self.assertIsInstance(payload["reasons"], list)
        self.assertIsInstance(payload["protected_violations"], list)

    def test_outcome_as_dict_keeps_an_unscored_outcome_unscored(self) -> None:
        outcome = score_candidate(
            default_contract(),
            [_obs("correctness.pass_rate", 0.5), _obs("reliability.success_rate", 0.9),
             _obs("efficiency.duration_ms", 1000.0), _obs("control.no_regression", 1.0)],
            [_obs("correctness.pass_rate", 1.0), _obs("reliability.success_rate", None, measured=False),
             _obs("efficiency.duration_ms", 1000.0), _obs("control.no_regression", 1.0)],
            evidence_pairs=1)
        payload = outcome.as_dict()
        self.assertEqual(payload["decision"], DECISION_INSUFFICIENT_EVIDENCE)
        self.assertIsNone(payload["score"])
        json.dumps(payload, sort_keys=True)


class DeterminismTests(unittest.TestCase):
    def test_same_inputs_produce_the_same_outcome(self) -> None:
        contract = default_contract()
        baseline = [_obs("correctness.pass_rate", 0.5), _obs("reliability.success_rate", 0.9),
                    _obs("efficiency.duration_ms", 1000.0), _obs("control.no_regression", 1.0)]
        candidate = [_obs("correctness.pass_rate", 1.0), _obs("reliability.success_rate", 0.95),
                     _obs("efficiency.duration_ms", 800.0), _obs("control.no_regression", 1.0)]
        first = score_candidate(contract, baseline, candidate, evidence_pairs=3)
        for _ in range(3):
            again = score_candidate(contract, baseline, candidate, evidence_pairs=3)
            self.assertEqual(first.decision, again.decision)
            self.assertEqual(first.score, again.score)
            self.assertEqual(first.reasons, again.reasons)
            self.assertEqual(first.objective, again.objective)
            self.assertEqual(first.protected_violations, again.protected_violations)

    def test_observation_order_does_not_change_the_outcome(self) -> None:
        contract = default_contract()
        baseline = [_obs("correctness.pass_rate", 0.5), _obs("reliability.success_rate", 0.9),
                    _obs("efficiency.duration_ms", 1000.0), _obs("control.no_regression", 1.0)]
        candidate = [_obs("correctness.pass_rate", 1.0), _obs("reliability.success_rate", 0.95),
                     _obs("efficiency.duration_ms", 800.0), _obs("control.no_regression", 1.0)]
        ordered = score_candidate(contract, baseline, candidate, evidence_pairs=3)
        shuffled = score_candidate(contract, list(reversed(baseline)), list(reversed(candidate)),
                                   evidence_pairs=3)
        self.assertEqual(ordered.decision, shuffled.decision)
        self.assertEqual(ordered.score, shuffled.score)
        self.assertEqual(ordered.reasons, shuffled.reasons)

    def test_outcome_is_a_frozen_value(self) -> None:
        outcome = ScoreOutcome(decision=DECISION_ACCEPTED, score=1.0, objective=OBJECTIVE_NONE,
                                reasons=(), protected_violations=())
        with self.assertRaises(Exception):
            outcome.decision = DECISION_REJECTED
        self.assertFalse(math.isnan(float(outcome.score)))


if __name__ == "__main__":
    unittest.main()


class RegressionFloorTests(unittest.TestCase):
    """The contract's regression floors decide what blocks acceptance.

    A correctness fix must not be disqualified because the fixed run is
    slightly slower, and a candidate that trades correctness or reliability
    for speed must be rejected.
    """

    def _sides(self, correctness: float, reliability: float, duration: float
               ) -> tuple:
        baseline = [
            Observation("correctness.pass_rate", 0.5),
            Observation("reliability.success_rate", 1.0),
            Observation("efficiency.duration_ms", 100.0),
            Observation("control.no_regression", 1.0),
        ]
        candidate = [
            Observation("correctness.pass_rate", correctness),
            Observation("reliability.success_rate", reliability),
            Observation("efficiency.duration_ms", duration),
            Observation("control.no_regression", 1.0),
        ]
        return baseline, candidate

    def test_a_correctness_fix_is_not_rejected_for_being_slower(self) -> None:
        baseline, candidate = self._sides(1.0, 1.0, 150.0)
        outcome = score_candidate(default_contract(), baseline, candidate,
                                          evidence_pairs=4)
        self.assertEqual(outcome.decision, DECISION_ACCEPTED)
        self.assertEqual(outcome.objective, OBJECTIVE_CORRECTNESS_FIX)
        self.assertIn("secondary_criterion_regressed", outcome.reasons)

    def test_a_correctness_regression_still_blocks_acceptance(self) -> None:
        baseline, candidate = self._sides(0.25, 1.0, 50.0)
        outcome = score_candidate(default_contract(), baseline, candidate,
                                          evidence_pairs=4)
        self.assertEqual(outcome.decision, DECISION_REJECTED)
        self.assertIn("criterion_regressed", outcome.reasons)

    def test_a_reliability_regression_still_blocks_acceptance(self) -> None:
        baseline, candidate = self._sides(0.5, 0.5, 50.0)
        outcome = score_candidate(default_contract(), baseline, candidate,
                                          evidence_pairs=4)
        self.assertEqual(outcome.decision, DECISION_REJECTED)
        self.assertIn("criterion_regressed", outcome.reasons)

    def test_efficiency_is_the_only_advisory_floor_in_the_default_contract(self) -> None:
        contract = default_contract()
        advisory = {criterion.id for criterion in contract.criteria
                    if not criterion.regression_blocks}
        self.assertEqual(advisory, {"efficiency.duration_ms"})

    def test_a_non_bool_regression_blocks_is_rejected(self) -> None:
        with self.assertRaises(ValueError):
            Criterion(id="x", kind="correctness", direction="higher_is_better",
                              regression_blocks="yes")

    def test_contract_mapping_accepts_and_round_trips_the_floor(self) -> None:
        contract = contract_from_mapping(default_contract().as_dict())
        self.assertEqual(
            {criterion.id: criterion.regression_blocks for criterion in contract.criteria},
            {criterion.id: criterion.regression_blocks
             for criterion in default_contract().criteria})
