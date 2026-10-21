"""Tests for the provider-free budget admission preflight."""
from __future__ import annotations

import ast
import json
import math
import pathlib
import unittest

import dev.harness_lab.budget as budget_module
from dev.harness_lab.budget import (BudgetExhausted, BudgetPolicy, ProviderQuota,
                                    STATUS_ADMITTED, STATUS_UNVERIFIABLE,
                                    SpendGuard, admit_budget,
                                    budget_policy_from_mapping)


def _quota(enforceable: bool = True, retry_safe: bool = True) -> ProviderQuota:
    return ProviderQuota(source="provider_account_quota", enforceable=enforceable,
                          retry_safe=retry_safe, scope="account")


class BudgetAdmissionTests(unittest.TestCase):
    def test_empty_policy_is_refused_with_the_first_missing_source(self) -> None:
        result = admit_budget(BudgetPolicy())
        self.assertEqual(result.status, STATUS_UNVERIFIABLE)
        self.assertEqual(result.missing_source, "token_limit")
        self.assertEqual(result.enforced_caps, ())
        self.assertEqual(result.detail["missing_sources"][0], "token_limit")
        self.assertTrue(result.detail["reason"])

    def test_token_limit_only_is_admitted(self) -> None:
        result = admit_budget(BudgetPolicy(token_limit=1000))
        self.assertEqual(result.status, STATUS_ADMITTED)
        self.assertEqual(result.enforced_caps, ("token_limit",))
        self.assertIsNone(result.missing_source)
        self.assertEqual(result.detail["reason"], "")
        self.assertEqual(result.detail["missing_sources"], [])
        self.assertEqual(result.detail["invalid_caps"], [])

    def test_wallclock_only_is_admitted(self) -> None:
        result = admit_budget(BudgetPolicy(max_wallclock_seconds=600.0))
        self.assertEqual(result.status, STATUS_ADMITTED)
        self.assertEqual(result.enforced_caps, ("max_wallclock_seconds",))

    def test_single_provider_request_is_a_valid_cap(self) -> None:
        result = admit_budget(BudgetPolicy(max_provider_requests=1))
        self.assertEqual(result.status, STATUS_ADMITTED)
        self.assertEqual(result.enforced_caps, ("max_provider_requests",))
        self.assertEqual(result.detail["invalid_caps"], [])

    def test_zero_provider_requests_is_invalid_and_refused(self) -> None:
        result = admit_budget(BudgetPolicy(max_provider_requests=0))
        self.assertEqual(result.status, STATUS_UNVERIFIABLE)
        self.assertEqual(result.missing_source, "max_provider_requests")
        self.assertEqual(result.enforced_caps, ())
        recorded = result.detail["invalid_caps"]
        self.assertEqual(len(recorded), 1)
        self.assertEqual(recorded[0]["cap"], "max_provider_requests")
        self.assertTrue(recorded[0]["reason"])

    def test_cost_limit_without_stated_enforcement_is_refused(self) -> None:
        result = admit_budget(BudgetPolicy(cost_limit_usd=1.0, cost_enforcement=None))
        self.assertEqual(result.status, STATUS_UNVERIFIABLE)
        self.assertEqual(result.missing_source, "cost_limit_usd")
        self.assertEqual(result.enforced_caps, ())
        self.assertNotIn("cost_limit_usd", result.enforced_caps)

    def test_cost_limit_with_controller_enforcement_is_admitted(self) -> None:
        result = admit_budget(BudgetPolicy(cost_limit_usd=2.5, cost_enforcement="controller_enforced", cost_observable=True))
        self.assertEqual(result.status, STATUS_ADMITTED)
        self.assertEqual(result.enforced_caps, ("cost_limit_usd",))

    def test_cost_limit_with_provider_enforcement_is_admitted(self) -> None:
        result = admit_budget(BudgetPolicy(cost_limit_usd=2.5, cost_enforcement="provider_enforced"))
        self.assertEqual(result.status, STATUS_ADMITTED)
        self.assertEqual(result.enforced_caps, ("cost_limit_usd",))

    def test_unknown_cost_enforcement_is_rejected_by_the_parser(self) -> None:
        with self.assertRaises(ValueError):
            budget_policy_from_mapping({"cost_limit_usd": 1.0, "cost_enforcement": "guessed"})

    def test_non_enforceable_provider_quota_is_refused(self) -> None:
        result = admit_budget(BudgetPolicy(provider_quota=_quota(enforceable=False)))
        self.assertEqual(result.status, STATUS_UNVERIFIABLE)
        self.assertEqual(result.missing_source, "provider_quota")
        self.assertEqual(result.enforced_caps, ())

    def test_provider_quota_without_retry_safety_is_refused(self) -> None:
        result = admit_budget(BudgetPolicy(provider_quota=_quota(retry_safe=False)))
        self.assertEqual(result.status, STATUS_UNVERIFIABLE)
        self.assertEqual(result.missing_source, "provider_quota_retry_safety")
        self.assertEqual(result.enforced_caps, ())
        self.assertIn("provider_quota_retry_safety", result.detail["missing_sources"])

    def test_enforceable_retry_safe_provider_quota_is_admitted(self) -> None:
        result = admit_budget(BudgetPolicy(provider_quota=_quota()))
        self.assertEqual(result.status, STATUS_ADMITTED)
        self.assertIn("provider_quota", result.enforced_caps)
        self.assertIsNone(result.missing_source)

    def test_multiple_missing_caps_follow_the_fixed_priority(self) -> None:
        result = admit_budget(BudgetPolicy(cost_limit_usd=1.0, max_provider_requests=0))
        self.assertEqual(result.status, STATUS_UNVERIFIABLE)
        self.assertEqual(result.missing_source, "cost_limit_usd")
        self.assertEqual(result.detail["missing_sources"][:2], ["cost_limit_usd", "max_provider_requests"])
        self.assertEqual(set(result.detail["missing_sources"]),
                         {"cost_limit_usd", "max_provider_requests", "token_limit",
                          "max_wallclock_seconds", "provider_quota"})

    def test_missing_caps_without_any_request_use_the_fixed_priority(self) -> None:
        result = admit_budget(BudgetPolicy())
        self.assertEqual(result.detail["missing_sources"],
                         ["token_limit", "cost_limit_usd", "max_provider_requests",
                          "max_wallclock_seconds", "provider_quota"])
        self.assertEqual(result.missing_source, result.detail["missing_sources"][0])

    def test_invalid_numeric_caps_are_recorded_and_refused(self) -> None:
        cases = (
            (BudgetPolicy(token_limit=-1), "token_limit"),
            (BudgetPolicy(token_limit=0), "token_limit"),
            (BudgetPolicy(max_provider_requests=0), "max_provider_requests"),
            (BudgetPolicy(cost_limit_usd=float("nan"), cost_enforcement="controller_enforced"), "cost_limit_usd"),
            (BudgetPolicy(cost_limit_usd=float("inf"), cost_enforcement="controller_enforced"), "cost_limit_usd"),
            (BudgetPolicy(max_wallclock_seconds=float("nan")), "max_wallclock_seconds"),
            (BudgetPolicy(max_wallclock_seconds=float("inf")), "max_wallclock_seconds"),
            (BudgetPolicy(token_limit=True), "token_limit"),
            (BudgetPolicy(max_provider_requests="2"), "max_provider_requests"),
        )
        for policy, expected_cap in cases:
            with self.subTest(policy=policy):
                result = admit_budget(policy)
                self.assertEqual(result.status, STATUS_UNVERIFIABLE)
                self.assertEqual(result.enforced_caps, ())
                recorded = result.detail["invalid_caps"]
                self.assertEqual(len(recorded), 1)
                self.assertEqual(recorded[0]["cap"], expected_cap)
                self.assertEqual(result.missing_source, expected_cap)

    def test_parser_rejects_unknown_keys_and_bad_types(self) -> None:
        with self.assertRaises(ValueError):
            budget_policy_from_mapping({"token_limitt": 10})
        with self.assertRaises(ValueError):
            budget_policy_from_mapping({"provider_quota": {"source": "s", "enforceable": True,
                                                            "retry_safe": True, "scope": "account",
                                                            "extra": 1}})
        with self.assertRaises(ValueError):
            budget_policy_from_mapping({"token_limit": "100"})
        with self.assertRaises(ValueError):
            budget_policy_from_mapping({"max_provider_requests": [1]})
        with self.assertRaises(ValueError):
            budget_policy_from_mapping({"cost_enforcement": 5})
        with self.assertRaises(ValueError):
            budget_policy_from_mapping({"provider_quota": {"source": "s", "enforceable": True,
                                                            "retry_safe": True, "scope": "planet"}})
        with self.assertRaises(ValueError):
            budget_policy_from_mapping({"provider_quota": {"source": "", "enforceable": True,
                                                            "retry_safe": True, "scope": "account"}})
        with self.assertRaises(ValueError):
            budget_policy_from_mapping({"provider_quota": {"source": "s", "enforceable": "yes",
                                                            "retry_safe": True, "scope": "account"}})
        with self.assertRaises(ValueError):
            budget_policy_from_mapping({"provider_quota": {"source": "s", "enforceable": True,
                                                            "scope": "account"}})
        with self.assertRaises(ValueError):
            budget_policy_from_mapping("not-a-mapping")

    def test_parser_accepts_a_policy_without_any_cap(self) -> None:
        policy = budget_policy_from_mapping({})
        self.assertEqual(policy, BudgetPolicy())
        result = admit_budget(policy)
        self.assertEqual(result.status, STATUS_UNVERIFIABLE)
        self.assertEqual(result.missing_source, "token_limit")

    def test_parser_round_trip_admits_a_stated_cost_cap(self) -> None:
        policy = budget_policy_from_mapping({
            "token_limit": 20000,
            "cost_limit_usd": 1.25,
            "cost_enforcement": "controller_enforced",
            "cost_observable": True,
            "max_provider_requests": 3,
            "max_wallclock_seconds": 900,
            "provider_quota": {"source": "provider_account_quota", "enforceable": True,
                               "retry_safe": True, "scope": "account",
                               "observed_limit": 10.0, "observed_at": "2026-10-21T00:00:00Z"},
        })
        result = admit_budget(policy)
        self.assertEqual(result.status, STATUS_ADMITTED)
        self.assertEqual(result.enforced_caps,
                         ("cost_limit_usd", "max_provider_requests", "max_wallclock_seconds",
                          "provider_quota", "token_limit"))

    def test_parser_defers_boolean_limits_to_the_admission_check(self) -> None:
        policy = budget_policy_from_mapping({"token_limit": True})
        result = admit_budget(policy)
        self.assertEqual(result.status, STATUS_UNVERIFIABLE)
        self.assertEqual(result.detail["invalid_caps"][0]["cap"], "token_limit")

    def test_as_dict_is_json_serializable(self) -> None:
        refused = admit_budget(BudgetPolicy())
        encoded = json.dumps(refused.as_dict(), sort_keys=True)
        decoded = json.loads(encoded)
        self.assertEqual(decoded["status"], STATUS_UNVERIFIABLE)
        self.assertEqual(decoded["enforced_caps"], [])
        self.assertEqual(decoded["missing_source"], "token_limit")
        self.assertEqual(sorted(decoded["detail"]), ["invalid_caps", "missing_sources", "reason"])
        admitted = admit_budget(BudgetPolicy(token_limit=10))
        encoded = json.dumps(admitted.as_dict(), sort_keys=True)
        decoded = json.loads(encoded)
        self.assertEqual(decoded["status"], STATUS_ADMITTED)
        self.assertEqual(decoded["enforced_caps"], ["token_limit"])
        self.assertIsNone(decoded["missing_source"])

    def test_module_has_no_network_or_subprocess_imports(self) -> None:
        source = pathlib.Path(budget_module.__file__).read_text(encoding="utf-8")
        imported = set()
        for node in ast.walk(ast.parse(source)):
            if isinstance(node, ast.Import):
                imported.update(alias.name.split(".")[0] for alias in node.names)
            elif isinstance(node, ast.ImportFrom):
                if node.level == 0 and node.module:
                    imported.add(node.module.split(".")[0])
        forbidden = {"socket", "http", "urllib", "requests", "subprocess", "asyncio"}
        self.assertEqual(imported & forbidden, set())
        self.assertFalse(any(name.startswith("dev.harness_lab") for name in imported))

    def test_admission_is_deterministic(self) -> None:
        policy = BudgetPolicy(cost_limit_usd=1.0, max_provider_requests=0, provider_quota=_quota(retry_safe=False))
        first = admit_budget(policy)
        second = admit_budget(policy)
        self.assertEqual(first.missing_source, second.missing_source)
        self.assertEqual(first.enforced_caps, second.enforced_caps)
        self.assertEqual(first.as_dict(), second.as_dict())
        self.assertEqual(list(first.enforced_caps), sorted(first.enforced_caps))

    def test_refused_cap_blocks_an_otherwise_bounded_policy(self) -> None:
        policy = BudgetPolicy(token_limit=1000, cost_limit_usd=1.0, cost_enforcement=None)
        result = admit_budget(policy)
        self.assertEqual(result.status, STATUS_UNVERIFIABLE)
        self.assertEqual(result.missing_source, "cost_limit_usd")
        self.assertEqual(result.enforced_caps, ("token_limit",))

    def test_non_finite_limits_are_never_coerced_to_zero(self) -> None:
        policy = BudgetPolicy(max_wallclock_seconds=float("nan"))
        result = admit_budget(policy)
        # The declared value is never rewritten into a usable ceiling.
        self.assertTrue(math.isnan(policy.max_wallclock_seconds))
        self.assertEqual(result.status, STATUS_UNVERIFIABLE)
        self.assertEqual(result.enforced_caps, ())
        self.assertEqual(result.detail["invalid_caps"][0]["cap"], "max_wallclock_seconds")


if __name__ == "__main__":
    unittest.main()


class SpendGuardTests(unittest.TestCase):
    """The enforcement side of the admission gate.

    A declared cap that nothing enforces is an advisory, and an advisory must
    not be presented as a safety gate. These tests prove the caps the gate
    admits are actually enforced during a run.
    """

    def test_a_controller_enforced_cost_cap_needs_an_observable_cost(self) -> None:
        result = admit_budget(BudgetPolicy(cost_limit_usd=2.5,
                                            cost_enforcement="controller_enforced"))
        self.assertEqual(result.status, STATUS_UNVERIFIABLE)
        self.assertEqual(result.missing_source, "cost_limit_usd")
        self.assertIn("observable", result.detail["reason"])

    def test_a_provider_enforced_cost_cap_needs_no_observable_cost(self) -> None:
        result = admit_budget(BudgetPolicy(cost_limit_usd=2.5,
                                            cost_enforcement="provider_enforced"))
        self.assertEqual(result.status, STATUS_ADMITTED)

    def test_the_provider_request_cap_is_refused_before_the_work_starts(self) -> None:
        guard = SpendGuard(BudgetPolicy(max_provider_requests=2))
        guard.admit()
        guard.observe(provider_requests=1)
        guard.admit()
        self.assertEqual(guard.provider_requests, 2)
        with self.assertRaises(BudgetExhausted) as caught:
            guard.admit()
        self.assertIn("max_provider_requests", str(caught.exception))
        self.assertEqual(guard.exhausted_cap, "max_provider_requests")

    def test_an_admitted_request_is_reserved_until_it_is_observed(self) -> None:
        """A trial that fails before its usage is observed still counts."""
        guard = SpendGuard(BudgetPolicy(max_provider_requests=1))
        guard.admit()
        self.assertEqual(guard.provider_requests, 1)
        with self.assertRaises(BudgetExhausted):
            guard.admit()

    def test_an_observed_count_replaces_the_reservation(self) -> None:
        guard = SpendGuard(BudgetPolicy(max_provider_requests=4))
        guard.admit()
        guard.observe(provider_requests=3)
        self.assertEqual(guard.provider_requests, 3)

    def test_the_provider_request_cap_counts_the_projected_request(self) -> None:
        guard = SpendGuard(BudgetPolicy(max_provider_requests=1))
        with self.assertRaises(BudgetExhausted):
            guard.admit(projected_provider_requests=2)

    def test_the_token_cap_stops_further_work_after_it_is_crossed(self) -> None:
        guard = SpendGuard(BudgetPolicy(token_limit=100))
        guard.observe(input_tokens=60, output_tokens=45)
        self.assertEqual(guard.total_tokens, 105)
        self.assertEqual(guard.exhausted_cap, "token_limit")
        with self.assertRaises(BudgetExhausted):
            guard.admit()

    def test_an_unobserved_cost_is_not_reported_as_zero(self) -> None:
        guard = SpendGuard(BudgetPolicy(token_limit=1000))
        guard.observe(input_tokens=10, output_tokens=5)
        self.assertIsNone(guard.as_dict()["cost_usd"])
        self.assertFalse(guard.cost_observed)

    def test_the_observed_spend_is_json_serializable(self) -> None:
        guard = SpendGuard(BudgetPolicy(token_limit=1000, max_provider_requests=4))
        guard.observe(provider_requests=1, input_tokens=10, output_tokens=5, cost_usd=0.25)
        payload = guard.as_dict()
        self.assertEqual(json.dumps(payload, sort_keys=True), json.dumps(payload, sort_keys=True))
        self.assertEqual(payload["total_tokens"], 15)
        self.assertEqual(payload["cost_usd"], 0.25)
