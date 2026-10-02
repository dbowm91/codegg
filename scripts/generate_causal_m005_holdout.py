#!/usr/bin/env python3
"""Generate the frozen M005 causal-frontier holdout corpus.

Writes `assets/tool-advisor/causal-frontier-m005-holdout.json`: a fresh
post-M004-freeze stateful holdout (minimum 160 scenarios) plus all plan
section 7 safety cases. Every gold promotion decision cites the host fact
and contract making it admissible.

The oracle below is an INDEPENDENT transcription of the frozen M001/M002
semantics (fact derivation from `CausalStateSnapshot::from_inputs`,
contract preconditions from `native_causal_contract`, structured-signal
definition from `has_structured_signal`, selection from the M005 freeze
record) -- it never imports or calls the implementation. Any divergence
between oracle gold and implementation output is a loud test failure to
be hand-adjudicated against the frozen sources, never silently absorbed.

Usage:
  python3 scripts/generate_causal_m005_holdout.py [--check]

--check regenerates to a temp file and diffs against the checked-in
corpus, failing on any difference (frozen means frozen).
"""

from __future__ import annotations

import hashlib
import itertools
import json
import sys
import tempfile
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parent.parent
OUT_PATH = WORKSPACE / "assets/tool-advisor/causal-frontier-m005-holdout.json"

SCHEMA_VERSION = 1
MAX_PROMOTIONS = 2
SCHEMA_BUDGET = 16 * 1024

# --- Frozen inputs (must match assets/tool-advisor/causal-frontier-m005-freeze.json) ---
CATALOG_FP = "0ac06de84189b093c55481ec190068a7dff5030b868c1ed804af4042cf3fd944"
ONTOLOGY_VERSION = 1

# --- Transcribed fact derivation (CausalStateSnapshot::from_inputs): each
# input field maps 1:1 onto the presence of one fact. ---
INPUT_TO_FACT = {
    "has_active_goal": "active_goal",
    "has_active_work_plan": "active_work_plan",
    "has_actionable_item": "actionable_work_item",
    "has_in_progress_item": "in_progress_work_item",
    "has_blocked_item": "blocked_work_item",
    "artifact_handle_count": "artifact_handle_available",
    "touched_file_count": "touched_files_available",
    "test_evidence_count": "test_evidence_available",
    "has_failed_tests": "failed_test_evidence",
    "unresolved_error_count": "unresolved_error",
    "security_finding_count": "security_finding",
    "lsp_preview_available": "lsp_preview_available",
    "context_read_available": "context_read_available",
    "unmet_test_acceptance": "unmet_test_acceptance",
    "unmet_commit_acceptance": "unmet_commit_acceptance",
    "unmet_artifact_acceptance": "unmet_artifact_acceptance",
    "unmet_delegated_run_acceptance": "unmet_delegated_run_acceptance",
}
COUNT_INPUTS = {
    "artifact_handle_count",
    "touched_file_count",
    "test_evidence_count",
    "unresolved_error_count",
    "security_finding_count",
}

STRUCTURED_FACTS = {
    "active_goal",
    "active_work_plan",
    "actionable_work_item",
    "in_progress_work_item",
    "blocked_work_item",
    "unmet_test_acceptance",
    "unmet_commit_acceptance",
    "unmet_artifact_acceptance",
    "unmet_delegated_run_acceptance",
}

# --- Transcribed contract preconditions (native_causal_contract pilot set).
# requires_all: facts all required. requires_any: one non-empty group of
# which at least one fact group must be fully present (here: single group).
# forbids: facts that must be absent. Tools absent from this table are
# uncontracted (never promoted, always discoverable). ---
CONTRACTS = {
    "context_read": {"requires_all": {"artifact_handle_available"}, "requires_any": [], "forbids": set()},
    "goal_get": {"requires_all": {"active_goal"}, "requires_any": [], "forbids": set()},
    "goal_update_progress": {"requires_all": {"active_goal"}, "requires_any": [], "forbids": set()},
    "work_plan_get": {"requires_all": {"active_work_plan"}, "requires_any": [], "forbids": set()},
    "work_plan_update_item": {
        "requires_all": {"active_work_plan"},
        "requires_any": [{"actionable_work_item", "in_progress_work_item", "blocked_work_item"}],
        "forbids": set(),
    },
    "lsp_preview_apply": {"requires_all": {"lsp_preview_available"}, "requires_any": [], "forbids": set()},
    "commit": {
        "requires_all": {"unmet_commit_acceptance"},
        "requires_any": [],
        "forbids": {"unresolved_error"},
    },
}
# Contracted but unconditionally admissible (empty preconditions).
ALWAYS_ADMISSIBLE = {
    "read", "glob", "grep", "lsp", "write", "edit", "git",
    "test", "verify", "task", "research", "webfetch", "websearch",
}

# --- Transcribed CORE_PALETTE (src/tool/disclosure.rs): tools NOT listed
# here are palette-deferred and form the promotion domain. The M005
# qualification test asserts the live palette fingerprint still matches the
# frozen value, so any drift here breaks loudly instead of agreeing. ---
CORE_PALETTE = {
    "bash", "read", "edit", "write", "glob", "grep", "list", "diff",
    "apply_patch", "task", "test", "verify", "git", "question", "skill",
    "todoread", "todowrite", "plan_enter", "plan_exit", "tool_search",
    "websearch", "webfetch", "repo_search", "lsp", "text_equal",
    "text_diff_explain", "text_replace_check", "validate_json",
    "validate_toml", "command_preflight", "path_normalize",
    "text_security_inspect", "context_read",
}

UNCONTRACTED_POOL = ["mcp__ext__search", "my_plugin__do", "future_tool", "plugin__custom"]


def derive_facts(inputs: dict) -> set[str]:
    facts = set()
    for field, fact in INPUT_TO_FACT.items():
        value = inputs.get(field, 0 if field in COUNT_INPUTS else False)
        if isinstance(value, int) and not isinstance(value, bool):
            if value > 0:
                facts.add(fact)
        elif value:
            facts.add(fact)
    return facts


def admissible(tool: str, facts: set[str]) -> tuple[bool, list[str], list[str]]:
    """Return (is_admissible, satisfying_facts, blocking_facts)."""
    if tool in ALWAYS_ADMISSIBLE:
        return True, [], []
    contract = CONTRACTS.get(tool)
    if contract is None:
        return False, [], []  # uncontracted: not a promotion candidate
    missing = sorted(f for f in contract["requires_all"] if f not in facts)
    present_forbidden = sorted(f for f in contract["forbids"] if f in facts)
    if missing or present_forbidden:
        return False, [], missing + [f"forbidden:{f}" for f in present_forbidden]
    for group in contract["requires_any"]:
        if not (group & facts):
            return False, [], [f"requires_any_missing:{'|'.join(sorted(group))}"]
    cited = sorted(contract["requires_all"] | set().union(*(group & facts for group in contract["requires_any"])))
    return True, cited, []


def oracle(inputs: dict, eligible: list[str], omitted: set[str],
           bypass: set[str], deferred: set[str],
           schema_bytes: dict[str, int]) -> dict:
    """Independent gold computation for one scenario surface."""
    facts = derive_facts(inputs)
    structured = bool(facts & STRUCTURED_FACTS)
    admissible_map: dict[str, bool] = {}
    citations: dict[str, list[str]] = {}
    premature: list[str] = []
    uncontracted: list[str] = []
    for tool in eligible:
        if tool in bypass:
            continue  # bypass: never a promotion candidate
        if tool in ALWAYS_ADMISSIBLE or tool in CONTRACTS:
            ok, cited, _ = admissible(tool, facts)
            admissible_map[tool] = ok
            if ok:
                citations[tool] = cited
            elif tool in CONTRACTS:
                premature.append(tool)
        else:
            uncontracted.append(tool)
    gold: dict = {
        "facts": sorted(facts),
        "structured_signal": structured,
        "premature": sorted(premature),
        "uncontracted": sorted(uncontracted),
        "promoted": [],
        "promoted_schema_bytes": 0,
        "citations": [],
    }
    if not structured:
        gold["no_change_reason"] = "abstained_insufficient_state"
        return gold
    candidates = sorted(
        t for t, ok in admissible_map.items() if ok and t in deferred
    )
    if not candidates:
        gold["no_change_reason"] = "no_admissible_deferred_in_universe"
        return gold
    selected: list[str] = []
    total = 0
    for tool in candidates:
        if len(selected) >= MAX_PROMOTIONS:
            break
        size = schema_bytes.get(tool)
        if size is None or total + size > SCHEMA_BUDGET:
            continue
        selected.append(tool)
        total += size
    if not selected:
        gold["no_change_reason"] = "schema_budget_exhausted"
        return gold
    gold["promoted"] = selected
    gold["promoted_schema_bytes"] = total
    gold["citations"] = [
        {"tool": t, "facts": citations[t], "contract": t} for t in selected
    ]
    gold["no_change_reason"] = None
    return gold


# --- Scenario surface templates -------------------------------------------
# Each template: tools as (name, deferred, schema_bytes). required flags,
# omissions, aliases, and state come from the scenario wrapper.

def t_heavy() -> list[tuple[str, bool, int]]:
    return [
        ("commit", True, 420), ("goal_get", True, 310),
        ("goal_update_progress", True, 350), ("work_plan_get", True, 480),
        ("work_plan_update_item", True, 520), ("lsp_preview_apply", True, 290),
        ("read", False, 150), ("test", False, 200),
        ("mcp__ext__search", True, 260), ("future_tool", True, 180),
    ]


def t_mixed() -> list[tuple[str, bool, int]]:
    return [
        ("commit", False, 420), ("goal_get", True, 310),
        ("work_plan_get", True, 480), ("lsp_preview_apply", True, 290),
        ("grep", True, 140), ("read", False, 150),
        ("my_plugin__do", True, 220),
    ]


def t_uncontracted_only() -> list[tuple[str, bool, int]]:
    return [
        ("read", False, 150), ("test", False, 200),
        ("mcp__ext__search", True, 260), ("my_plugin__do", True, 220),
        ("future_tool", True, 180), ("plugin__custom", True, 240),
    ]


def t_all_immediate() -> list[tuple[str, bool, int]]:
    return [
        ("commit", False, 420), ("goal_get", False, 310),
        ("work_plan_get", False, 480), ("read", False, 150),
        ("mcp__ext__search", False, 260),
    ]


def t_generics_deferred() -> list[tuple[str, bool, int]]:
    return [
        ("grep", True, 140), ("glob", True, 130), ("read", False, 150),
        ("test", False, 200), ("future_tool", True, 180),
    ]


TEMPLATES = {
    "heavy": t_heavy, "mixed": t_mixed, "uncontracted_only": t_uncontracted_only,
    "all_immediate": t_all_immediate, "generics_deferred": t_generics_deferred,
}

BASE_INPUTS: dict = {
    "has_active_goal": False, "has_active_work_plan": False,
    "has_actionable_item": False, "has_in_progress_item": False,
    "has_blocked_item": False, "artifact_handle_count": 0,
    "touched_file_count": 0, "test_evidence_count": 0,
    "has_failed_tests": False, "unresolved_error_count": 0,
    "security_finding_count": 0, "lsp_preview_available": False,
    "context_read_available": False, "unmet_test_acceptance": False,
    "unmet_commit_acceptance": False, "unmet_artifact_acceptance": False,
    "unmet_delegated_run_acceptance": False,
}


def make_inputs(**overrides) -> dict:
    inputs = dict(BASE_INPUTS)
    inputs.update(overrides)
    return inputs


def build_scenario(sid: str, family: str, inputs: dict,
                   template: str, rationale: str,
                   required: list[str] | None = None,
                   never_reduce: list[str] | None = None,
                   omissions: list[dict] | None = None,
                   aliases: dict | None = None,
                   current_step: list[str] | None = None,
                   byte_overrides: dict | None = None,
                   renames: dict | None = None,
                   extra_tools: list[tuple[str, bool, int]] | None = None) -> dict:
    tools = []
    schema_bytes: dict[str, int] = {}
    entries: list[tuple[str, bool, int]] = list(TEMPLATES[template]())
    entries += list(extra_tools or [])
    wire_of: dict[str, str] = {}  # canonical -> wire name in the fixture
    for name, deferred, size in entries:
        if byte_overrides and name in byte_overrides:
            size = byte_overrides[name]
        wire = (renames or {}).get(name, name)
        wire_of[name] = wire
        tools.append({"name": wire, "canonical": name, "deferred": deferred,
                      "schema_bytes": size})
        schema_bytes[name] = size
    required_set = set(required or [])
    never_reduce_set = set(never_reduce or [])
    bypass = required_set | never_reduce_set
    omitted = {o["name"] for o in (omissions or [])}
    assert not (bypass & omitted), f"{sid}: bypass tool omitted"
    # Eligible: every surfaced tool except omissions (required tools stay
    # eligible; they bypass filtering but remain visible).
    eligible = [t for t in schema_bytes if t not in omitted]
    deferred = set()
    for entry_name, entry_deferred, _entry_size in entries:
        if entry_deferred and entry_name not in omitted:
            deferred.add(entry_name)
    gold = oracle(inputs, eligible, omitted, bypass, deferred, schema_bytes)
    if current_step is None:
        # Default current step: promoted tools plus any required tools --
        # the step genuinely needs what the frontier discloses.
        current_step = sorted(set(gold["promoted"]) | required_set)
    return {
        "schema_version": SCHEMA_VERSION,
        "id": sid,
        "family": family,
        "rationale": rationale,
        "inputs": inputs,
        "surface": {
            "tools": tools,
            "required": sorted(required_set),
            "never_reduce": sorted(set(never_reduce or [])),
            "omissions": omissions or [],
            "aliases": aliases or {},
        },
        "gold": {
            "facts": gold["facts"],
            "structured_signal": gold["structured_signal"],
            "promoted": gold["promoted"],
            "promoted_schema_bytes": gold["promoted_schema_bytes"],
            "no_change_reason": gold["no_change_reason"],
            "citations": gold["citations"],
            "current_step": sorted(current_step),
            "premature": gold["premature"],
            "uncontracted": gold["uncontracted"],
        },
    }


def pinned_scenarios() -> list[dict]:
    s: list[dict] = []
    # Safety: denied mutating tool with an otherwise matching contract.
    s.append(build_scenario(
        "m005-pinned-001", "safety_denied_mutating",
        make_inputs(has_active_work_plan=True, has_actionable_item=True,
                    unmet_commit_acceptance=True),
        "heavy", "commit acceptance is unmet so commit is admissible, but commit "
        "is denied upstream: it must never appear in any causal set.",
        omissions=[{"name": "commit", "reason": "Denied"}],
        current_step=["work_plan_get", "work_plan_update_item"]))
    s.append(build_scenario(
        "m005-pinned-002", "safety_denied_mutating",
        make_inputs(has_active_work_plan=True, has_actionable_item=True,
                    unmet_commit_acceptance=True, unresolved_error_count=1),
        "heavy", "denied commit plus a recorded error: commit is doubly "
        "excluded (withheld and forbidden); promotion falls to plan tools.",
        omissions=[{"name": "commit", "reason": "Denied"}],
        current_step=["work_plan_get", "work_plan_update_item"]))
    # Safety: parent-ceiling exclusion.
    s.append(build_scenario(
        "m005-pinned-003", "safety_parent_ceiling",
        make_inputs(has_active_work_plan=True, has_in_progress_item=True),
        "heavy", "work_plan_get is excluded by the parent capability ceiling "
        "and must stay excluded while work_plan_update_item promotes.",
        omissions=[{"name": "work_plan_get", "reason": "ParentCeiling"}],
        current_step=["work_plan_update_item"]))
    s.append(build_scenario(
        "m005-pinned-004", "safety_parent_ceiling",
        make_inputs(has_active_goal=True),
        "heavy", "goal_get is ceiling-excluded under an active goal: no "
        "admissible deferred candidate remains, so nothing promotes.",
        omissions=[{"name": "goal_get", "reason": "ParentCeiling"},
                   {"name": "goal_update_progress", "reason": "ParentCeiling"}],
        current_step=[]))
    # Safety: stale LSP preview.
    s.append(build_scenario(
        "m005-pinned-005", "safety_stale_preview",
        make_inputs(has_active_work_plan=True, has_actionable_item=True),
        "heavy", "a stale (unstaged) preview is absence: lsp_preview_apply is "
        "inadmissible and only plan tools promote.",
        current_step=["work_plan_get", "work_plan_update_item"]))
    # Safety: no active WorkPlan.
    s.append(build_scenario(
        "m005-pinned-006", "safety_no_workplan",
        make_inputs(has_active_goal=True),
        "heavy", "goal without plan: goal tools are admissible and promote; "
        "plan tools stay inadmissible.",
        current_step=["goal_get", "goal_update_progress"]))
    # Safety: completed WorkPlan (terminal: no plan facts; leftover test
    # evidence alone is not structured signal).
    s.append(build_scenario(
        "m005-pinned-007", "safety_completed_plan",
        make_inputs(test_evidence_count=2),
        "heavy", "completed plan leaves only test evidence: no structured "
        "signal, so the frontier abstains.",
        current_step=[]))
    # Safety: cancelled WorkPlan with a leftover recorded error.
    s.append(build_scenario(
        "m005-pinned-008", "safety_cancelled_plan",
        make_inputs(unresolved_error_count=1),
        "heavy", "cancelled plan leaves only an error record: errors alone "
        "are not structured signal; abstain.",
        current_step=[]))
    # Safety: failed test then verification transition.
    s.append(build_scenario(
        "m005-pinned-009", "safety_failed_then_verify",
        make_inputs(has_failed_tests=True, test_evidence_count=1),
        "heavy", "failed tests with evidence but no plan or acceptance: "
        "failure evidence alone does not justify promotion; abstain.",
        current_step=[]))
    s.append(build_scenario(
        "m005-pinned-010", "safety_failed_then_verify",
        make_inputs(has_failed_tests=True, test_evidence_count=1,
                    has_active_work_plan=True, has_actionable_item=True,
                    unmet_test_acceptance=True),
        "heavy", "failed tests under an active plan with unmet test "
        "acceptance: structured; plan tools promote, verification stays "
        "core-immediate.",
        current_step=["work_plan_get", "work_plan_update_item"]))
    # Safety: unmet commit acceptance before/after test evidence.
    s.append(build_scenario(
        "m005-pinned-011", "safety_commit_acceptance_evidence",
        make_inputs(has_active_work_plan=True, has_actionable_item=True,
                    unmet_commit_acceptance=True),
        "heavy", "unmet commit acceptance before any test evidence: commit "
        "is admissible and promotes first in canonical order.",
        current_step=["commit", "work_plan_get"]))
    s.append(build_scenario(
        "m005-pinned-012", "safety_commit_acceptance_evidence",
        make_inputs(has_active_work_plan=True, has_actionable_item=True,
                    unmet_commit_acceptance=True, test_evidence_count=3),
        "heavy", "unmet commit acceptance after test evidence: evidence does "
        "not gate commit; commit still promotes.",
        current_step=["commit", "work_plan_get"]))
    # Safety: unmet commit acceptance with a recorded error.
    s.append(build_scenario(
        "m005-pinned-013", "safety_commit_blocked_by_error",
        make_inputs(has_active_work_plan=True, has_actionable_item=True,
                    unmet_commit_acceptance=True, unresolved_error_count=2),
        "heavy", "committing with a recorded unresolved error is premature: "
        "commit must not promote; plan tools do.",
        current_step=["work_plan_get", "work_plan_update_item"]))
    # Safety: uncontracted MCP tool.
    s.append(build_scenario(
        "m005-pinned-014", "safety_uncontracted_mcp",
        make_inputs(has_active_work_plan=True, has_actionable_item=True,
                    unmet_commit_acceptance=True),
        "uncontracted_only", "deferred universe holds only uncontracted "
        "tools: nothing is causally classifiable, so nothing promotes.",
        current_step=[]))
    # Safety: forged/mismatched names (lookalikes carry no contract).
    s.append(build_scenario(
        "m005-pinned-015", "safety_name_mismatch",
        make_inputs(has_active_work_plan=True, has_actionable_item=True,
                    unmet_commit_acceptance=True),
        "mixed", "lookalike names carry no contract and stay uncontracted: "
        "commit is already immediate here, so grep and work_plan_get "
        "promote while commitx and work_plan_update_items disclose nothing.",
        extra_tools=[("commitx", True, 200), ("work_plan_update_items", True, 210)],
        current_step=["commit", "work_plan_get"]))
    # Safety: state drift between turns (linked pair).
    s.append(build_scenario(
        "m005-pinned-016", "safety_state_drift",
        make_inputs(has_active_work_plan=True, has_actionable_item=True,
                    unmet_commit_acceptance=True),
        "heavy", "turn one: commit acceptance unmet, commit promotes.",
        current_step=["commit", "work_plan_get"]))
    s.append(build_scenario(
        "m005-pinned-017", "safety_state_drift",
        make_inputs(has_active_work_plan=True, has_actionable_item=True,
                    unmet_commit_acceptance=True, unresolved_error_count=1),
        "heavy", "turn two: an error is recorded, commit goes premature and "
        "drops out of promotion while plan tools remain.",
        current_step=["work_plan_get", "work_plan_update_item"]))
    # Required bypass is never promoted.
    s.append(build_scenario(
        "m005-pinned-018", "required_bypass",
        make_inputs(has_active_work_plan=True, has_actionable_item=True,
                    unmet_commit_acceptance=True),
        "heavy", "an admissible commit marked required bypasses filtering: "
        "it stays visible but is never counted as a causal promotion.",
        required=["commit"],
        current_step=["commit", "work_plan_get", "work_plan_update_item"]))
    s.append(build_scenario(
        "m005-pinned-019", "required_bypass",
        make_inputs(has_active_goal=True),
        "heavy", "never-reduce goal_get bypasses: visible but never "
        "promoted; goal_update_progress promotes alone.",
        never_reduce=["goal_get"],
        current_step=["goal_get", "goal_update_progress"]))
    # Byte budget skips the giant and continues.
    s.append(build_scenario(
        "m005-pinned-020", "budget_skip",
        make_inputs(has_active_work_plan=True, has_actionable_item=True,
                    unmet_commit_acceptance=True),
        "heavy", "work_plan_get carries a 20 KiB schema: it is skipped and "
        "the budget continues to work_plan_update_item.",
        byte_overrides={"work_plan_get": 20 * 1024},
        current_step=["commit", "work_plan_update_item"]))
    # Bound bites: four admissible deferred tools, exactly two promote.
    s.append(build_scenario(
        "m005-pinned-021", "bound_bites",
        make_inputs(has_active_goal=True, has_active_work_plan=True,
                    has_actionable_item=True, unmet_commit_acceptance=True),
        "heavy", "goal plus plan plus commit acceptance: four admissible "
        "deferred tools, but the frozen bound allows exactly two in "
        "canonical order.",
        current_step=["commit", "goal_get"]))
    # Alias promotion resolves to the canonical name.
    s.append(build_scenario(
        "m005-pinned-022", "alias_promotion",
        make_inputs(has_active_work_plan=True, has_actionable_item=True,
                    unmet_commit_acceptance=True),
        "heavy", "the ci_commit wire alias promotes as canonical commit.",
        renames={"commit": "ci_commit"},
        aliases={"ci_commit": "commit"},
        current_step=["commit", "work_plan_get"]))
    # No state at all abstains.
    s.append(build_scenario(
        "m005-pinned-023", "abstention",
        make_inputs(),
        "heavy", "empty host state carries no structured signal: abstain "
        "even though the surface is rich.",
        current_step=[]))
    # Core immediacy: context_read with a handle never promotes (already core).
    s.append(build_scenario(
        "m005-pinned-024", "core_immediacy",
        make_inputs(artifact_handle_count=1, context_read_available=True,
                    unmet_artifact_acceptance=True),
        "generics_deferred", "artifact acceptance with a handle: artifact "
        "expansion is core-immediate, so only deferred generics promote.",
        current_step=["glob", "grep"]))
    # Delegated-run acceptance alone is structured (acceptance fact).
    s.append(build_scenario(
        "m005-pinned-025", "acceptance_only",
        make_inputs(unmet_delegated_run_acceptance=True),
        "generics_deferred", "delegated-run acceptance alone carries "
        "structured signal; always-admissible deferred generics promote.",
        current_step=["glob", "grep"]))
    # Disabled tool with a matching contract stays out.
    s.append(build_scenario(
        "m005-pinned-026", "safety_disabled",
        make_inputs(has_active_goal=True),
        "heavy", "disabled lsp-adjacent preview tooling is unaffected, and "
        "disabled goal_update_progress must not promote while goal_get does.",
        omissions=[{"name": "goal_update_progress", "reason": "DisabledByModel"}],
        current_step=["goal_get"]))
    # Blocked item still counts as structured plan signal.
    s.append(build_scenario(
        "m005-pinned-027", "blocked_item",
        make_inputs(has_active_work_plan=True, has_blocked_item=True),
        "mixed", "a blocked item keeps the plan actionable for disclosure: "
        "work_plan_get is admissible and grep is always admissible, so both "
        "promote; commit has no acceptance.",
        current_step=["work_plan_get"]))
    # In-progress item variant.
    s.append(build_scenario(
        "m005-pinned-028", "in_progress_item",
        make_inputs(has_active_work_plan=True, has_in_progress_item=True,
                    lsp_preview_available=True),
        "mixed", "in-progress item plus a staged preview: plan, preview, and "
        "generic tools are admissible, and canonical order promotes grep and "
        "lsp_preview_apply within the bound of two.",
        current_step=["lsp_preview_apply", "work_plan_get"]))
    return s


def expanded_scenarios(start: int) -> list[dict]:
    """Deterministic cartesian expansion over state axes x templates."""
    plan_states = [
        ("noplan", {}),
        ("actionable", {"has_active_work_plan": True, "has_actionable_item": True}),
        ("inprogress", {"has_active_work_plan": True, "has_in_progress_item": True}),
        ("blocked", {"has_active_work_plan": True, "has_blocked_item": True}),
    ]
    acceptances = [
        ("noacc", {}),
        ("test", {"unmet_test_acceptance": True}),
        ("commit", {"unmet_commit_acceptance": True}),
        ("artifact", {"unmet_artifact_acceptance": True}),
    ]
    template_names = ["heavy", "mixed", "uncontracted_only", "all_immediate",
                      "generics_deferred"]
    scenarios: list[dict] = []
    n = start
    for (pname, pstate), (aname, acc), errors, goal, preview, artifacts in itertools.product(
        plan_states, acceptances, [0, 1], [False, True], [False, True], [0, 1]
    ):
        template = template_names[n % len(template_names)]
        inputs = make_inputs(
            **pstate, **acc,
            unresolved_error_count=errors,
            has_active_goal=goal,
            lsp_preview_available=preview,
            artifact_handle_count=artifacts,
            test_evidence_count=1 if acc.get("unmet_test_acceptance") else 0,
        )
        rationale = (
            f"expansion {pname}/{aname}"
            f"{'/errors' if errors else ''}{'/goal' if goal else ''}"
            f"{'/preview' if preview else ''}{'/artifacts' if artifacts else ''} "
            f"over the {template} surface"
        )
        scenarios.append(build_scenario(
            f"m005-holdout-{n:03d}", "expanded_state_surface",
            inputs, template, rationale))
        n += 1
    return scenarios


def check_coverage(scenarios: list[dict]) -> None:
    """Fail loudly if any plan section 6 dimension is uncovered."""
    texts = [(s["id"], s["family"], json.dumps(s["inputs"]), json.dumps(s["surface"]))
             for s in scenarios]
    needs = {
        "workplan_status": ["has_actionable_item", "has_in_progress_item", "has_blocked_item"],
        "acceptance_evidence": ["unmet_test_acceptance", "unmet_commit_acceptance",
                                "unmet_artifact_acceptance", "unmet_delegated_run_acceptance"],
        "artifacts": ["artifact_handle_count"],
        "diagnostics_tests": ["has_failed_tests", "test_evidence_count", "unresolved_error_count"],
        "previews": ["lsp_preview_available"],
        "external_uncontracted": ["mcp__ext__search", "future_tool"],
        "aliases": ["ci_commit"],
        "capability_ceilings": ["ParentCeiling"],
    }
    for dimension, markers in needs.items():
        blob = " ".join(t[2] + t[3] for t in texts)
        if not any(m in blob for m in markers):
            raise SystemExit(f"holdout coverage failure: dimension {dimension} uncovered")
    # Every safety family from plan section 7 must be present.
    safety = {s["family"] for s in scenarios if s["family"].startswith("safety_")}
    required_safety = {
        "safety_denied_mutating", "safety_parent_ceiling", "safety_stale_preview",
        "safety_no_workplan", "safety_completed_plan", "safety_cancelled_plan",
        "safety_failed_then_verify", "safety_commit_acceptance_evidence",
        "safety_commit_blocked_by_error", "safety_uncontracted_mcp",
        "safety_name_mismatch", "safety_state_drift", "safety_disabled",
    }
    missing = required_safety - safety
    if missing:
        raise SystemExit(f"holdout coverage failure: safety families missing {sorted(missing)}")


def main() -> int:
    check_mode = "--check" in sys.argv
    scenarios = pinned_scenarios() + expanded_scenarios(len(pinned_scenarios()) + 1)
    check_coverage(scenarios)
    assert len(scenarios) >= 160, f"holdout minimum violated: {len(scenarios)}"
    corpus = {
        "schema_version": SCHEMA_VERSION,
        "protocol": "causal-frontier-m005-v1",
        "freeze_asset": "assets/tool-advisor/causal-frontier-m005-freeze.json",
        "contract_catalog_fingerprint": CATALOG_FP,
        "ontology_version": ONTOLOGY_VERSION,
        "max_promotions": MAX_PROMOTIONS,
        "schema_byte_budget": SCHEMA_BUDGET,
        "scenario_count": len(scenarios),
        "scenarios": scenarios,
    }
    rendered = json.dumps(corpus, indent=2, sort_keys=False) + "\n"
    if check_mode:
        current = OUT_PATH.read_text()
        if current != rendered:
            print("holdout corpus differs from generator output; regenerate without --check",
                  file=sys.stderr)
            return 1
        print(f"holdout corpus matches generator output ({len(scenarios)} scenarios)")
        return 0
    OUT_PATH.write_text(rendered)
    digest = hashlib.sha256(rendered.encode()).hexdigest()
    print(f"wrote {OUT_PATH} ({len(scenarios)} scenarios, sha256 {digest})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
