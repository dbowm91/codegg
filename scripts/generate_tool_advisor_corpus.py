#!/usr/bin/env python3
"""Deterministic generator for the tool-advisor qualification corpus.

Produces ``assets/tool-advisor/corpus.jsonl`` for C001
(content-derived corpus and split integrity).

Design guarantees (asserted before writing):
  - 256 cases with globally unique case ids, group ids, and contexts, so the
    normalized model-visible input signature is unique per case;
  - 216 leakage components: 40 counterfactual pairs (shared
    ``generated_variant_family`` + shared ``semantic_group``) plus 176
    singletons, each with its own variant family and semantic group;
  - tag floors: >=32 no-tool, >=32 multi-tool, >=64 hard-negative,
    >=32 unknown-tool, >=32 counterfactual pairs, >=10 task families;
  - every counterfactual pair shares one ordered candidate list while its two
    relevance maps differ, so candidate-only memorization cannot pass;
  - hard-negative cases carry a lexically close distractor that shares at
    least two normalized content tokens with the context;
  - no private repository content: every string is built from local template
    vocabularies below.

Usage:
  python3 scripts/generate_tool_advisor_corpus.py
"""

from __future__ import annotations

import json
import random
import re
import sys
from pathlib import Path

# Frozen C001 seed. This seed was selected as the first candidate whose
# content-derived partitions meet every C001 floor, including >=40 final-test
# leakage groups with zero exact/normalized/template cross-split overlap.
# The frozen partition fingerprints are recorded in the C001 closure record;
# changing the seed re-freezes every fingerprint and requires requalification.
SEED = int(sys.argv[1]) if len(sys.argv) > 1 else 20260922
OUTPUT = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("assets/tool-advisor/corpus.jsonl")

# (name, description, category, disclosure)
TOOLS: dict[str, list[tuple[str, str, str, str]]] = {
    "filesystem": [
        ("read", "Read bounded file contents", "ReadOnly", "deferred"),
        ("write", "Write content to files", "Edit", "deferred"),
        ("glob", "Find files by path pattern", "ReadOnly", "core"),
        ("list_dir", "List directory entries", "ReadOnly", "core"),
    ],
    "search": [
        ("grep", "Search literal text in files", "ReadOnly", "core"),
        ("semantic_search", "Search code by semantic similarity", "ReadOnly", "deferred"),
        ("symbol_search", "Find workspace symbols by name", "ReadOnly", "deferred"),
        ("replace", "Replace text across files", "Edit", "deferred"),
    ],
    "git": [
        ("git_status", "Show working tree status", "ReadOnly", "core"),
        ("git_diff", "Show unstaged changes", "ReadOnly", "core"),
        ("git_log", "Show commit history", "ReadOnly", "deferred"),
        ("git_blame", "Show line authorship", "ReadOnly", "deferred"),
    ],
    "lsp": [
        ("lsp_definition", "Jump to a symbol definition using language server semantics", "ReadOnly", "deferred"),
        ("lsp_references", "Find all references to a symbol", "ReadOnly", "deferred"),
        ("lsp_hover", "Show symbol documentation", "ReadOnly", "core"),
        ("lsp_rename", "Rename a symbol workspace-wide", "Edit", "deferred"),
    ],
    "verification": [
        ("test_run", "Run the focused test suite", "Execute", "deferred"),
        ("typecheck", "Type-check the workspace", "Execute", "deferred"),
        ("lint", "Run static linters", "Execute", "core"),
        ("coverage", "Measure test coverage", "Execute", "deferred"),
    ],
    "research": [
        ("web_search", "Search the web for documentation", "ReadOnly", "deferred"),
        ("fetch_url", "Fetch a URL as text", "ReadOnly", "deferred"),
        ("docs_lookup", "Look up API documentation", "ReadOnly", "core"),
        ("summarize", "Summarize provided text", "ReadOnly", "deferred"),
    ],
    "context": [
        ("context_read", "Read a persisted context artifact", "ReadOnly", "deferred"),
        ("goal_get", "Show the active goal", "ReadOnly", "core"),
        ("plan_get", "Show the active work plan", "ReadOnly", "core"),
        ("memory_recall", "Recall curated memory entries", "ReadOnly", "deferred"),
    ],
    "plugin": [
        ("plugin_install", "Install an extension plugin", "Execute", "deferred"),
        ("plugin_enable", "Enable an installed plugin", "Execute", "core"),
        ("plugin_search", "Search the plugin catalog", "ReadOnly", "deferred"),
        ("browser_test", "Drive a browser test bundle", "Execute", "deferred"),
    ],
    "shell": [
        ("shell_exec", "Execute a shell command", "Execute", "deferred"),
        ("shell_history", "Show recent shell history", "ReadOnly", "core"),
        ("env_get", "Read environment variables", "ReadOnly", "core"),
        ("process_list", "List running processes", "ReadOnly", "deferred"),
    ],
    "structured": [
        ("json_query", "Query JSON documents by path", "ReadOnly", "deferred"),
        ("json_edit", "Edit JSON documents", "Edit", "deferred"),
        ("table_filter", "Filter tabular data", "ReadOnly", "core"),
        ("schema_validate", "Validate data against a schema", "Execute", "deferred"),
    ],
}

FAMILY_SIZES = {
    "filesystem": 26,
    "search": 26,
    "git": 26,
    "lsp": 26,
    "verification": 26,
    "research": 26,
    "context": 26,
    "plugin": 26,
    "shell": 24,
    "structured": 24,
}

# Per-family (none, dual, unknown, hard-negative) tag budgets.
# Sums: none=32, dual=56, unknown=32, hard-negative=64.
TAG_BUDGETS = {
    "filesystem": (3, 6, 3, 7),
    "search": (3, 6, 3, 7),
    "git": (3, 6, 3, 7),
    "lsp": (3, 6, 3, 7),
    "verification": (3, 6, 3, 7),
    "research": (3, 6, 4, 7),
    "context": (3, 6, 4, 7),
    "plugin": (3, 6, 3, 7),
    "shell": (4, 4, 3, 4),
    "structured": (4, 4, 3, 4),
}

# task templates: (template with {object} {detail}, relevant tool index, second relevant or None)
TASKS: dict[str, list[tuple[str, int, int | None]]] = {
    "filesystem": [
        ("Read the module {object} at {detail} and report its public exports", 0, None),
        ("Persist the generated notes for {object} into {detail}", 1, None),
        ("Locate every test fixture matching {object} under {detail}", 2, None),
        ("List the entries of {detail} and identify which belong to {object}", 3, None),
        ("Open {object} in {detail}, then list the sibling entries around it", 0, 3),
        ("Create {detail} for {object} and confirm the write completed", 1, 0),
    ],
    "search": [
        ("Find the literal token {object} across the snapshot {detail}", 0, None),
        ("Locate code semantically related to {object} near {detail}", 1, None),
        ("Find the workspace symbol named {object} declared in {detail}", 2, None),
        ("Replace every occurrence of {object} with the new spelling in {detail}", 3, None),
        ("Find literal uses of {object} in {detail}, then jump to the matching symbol declaration", 0, 2),
        ("Search semantically for {object} around {detail} before renaming it workspace-wide", 1, 2),
    ],
    "git": [
        ("Report which paths changed for {object} in {detail}", 0, None),
        ("Show the unstaged diff touching {object} inside {detail}", 1, None),
        ("List recent commits that touched {object} in {detail}", 2, None),
        ("Attribute the lines of {object} at {detail} to their authors", 3, None),
        ("Check status for {object} in {detail}, then show the unstaged diff", 0, 1),
        ("Review the history of {object} in {detail} and blame the surviving lines", 2, 3),
    ],
    "lsp": [
        ("Jump to the definition of {object} referenced from {detail}", 0, None),
        ("Find every reference to {object} across {detail}", 1, None),
        ("Show the documentation for {object} as seen from {detail}", 2, None),
        ("Rename the symbol {object} everywhere it appears in {detail}", 3, None),
        ("Jump to the definition of {object} in {detail}, then list its other references", 0, 1),
        ("Inspect the documentation of {object} in {detail} before renaming it", 2, 3),
    ],
    "verification": [
        ("Run the focused tests covering {object} in {detail}", 0, None),
        ("Type-check the crate containing {object} at {detail}", 1, None),
        ("Lint the module {object} inside {detail} for style violations", 2, None),
        ("Measure coverage of {object} exercised by {detail}", 3, None),
        ("Type-check {object} in {detail}, then run its focused tests", 1, 0),
        ("Lint {object} at {detail} and measure the resulting coverage", 2, 3),
    ],
    "research": [
        ("Find external documentation for {object} relevant to {detail}", 0, None),
        ("Fetch the reference page for {object} linked from {detail} as text", 1, None),
        ("Look up the API signature of {object} needed by {detail}", 2, None),
        ("Summarize the quoted specification of {object} for {detail}", 3, None),
        ("Search the web for {object} in {detail}, then fetch the top reference page", 0, 1),
        ("Look up the API of {object} for {detail} and summarize the result", 2, 3),
    ],
    "context": [
        ("Read the persisted artifact {object} recorded in {detail}", 0, None),
        ("Show the active goal that tracks {object} in {detail}", 1, None),
        ("Show the work plan entry for {object} scheduled in {detail}", 2, None),
        ("Recall curated memory about {object} stored under {detail}", 3, None),
        ("Read artifact {object} from {detail}, then show the tracking goal", 0, 1),
        ("Show the plan entry for {object} in {detail} and recall related memory", 2, 3),
    ],
    "plugin": [
        ("Install the extension that provides {object} for {detail}", 0, None),
        ("Enable the installed extension backing {object} in {detail}", 1, None),
        ("Search the catalog for an extension covering {object} in {detail}", 2, None),
        ("Drive the browser test bundle exercising {object} in {detail}", 3, None),
        ("Search the catalog for {object} in {detail}, then install the match", 2, 0),
        ("Enable the extension for {object} in {detail} and run its browser tests", 1, 3),
    ],
    "shell": [
        ("Execute the maintenance command for {object} in {detail}", 0, None),
        ("Show recent shell history involving {object} from {detail}", 1, None),
        ("Read the environment variables configuring {object} in {detail}", 2, None),
        ("List the processes currently running {object} under {detail}", 3, None),
        ("Execute the command for {object} in {detail}, then review the shell history", 0, 1),
        ("Read the environment for {object} in {detail} and list matching processes", 2, 3),
    ],
    "structured": [
        ("Query the JSON path {object} inside document {detail}", 0, None),
        ("Edit the JSON document {object} stored at {detail}", 1, None),
        ("Filter the tabular export {object} produced by {detail}", 2, None),
        ("Validate {object} from {detail} against its declared schema", 3, None),
        ("Query path {object} in {detail}, then filter the resulting table", 0, 2),
        ("Edit document {object} at {detail} and validate it against the schema", 1, 3),
    ],
}

NO_TOOL_TASKS: dict[str, list[str]] = {
    "filesystem": [
        "Explain why the wording of the {object} guide reads clearly, without inspecting {detail}",
        "Summarize the naming convention behind {object} from the quoted paragraph, without opening {detail}",
    ],
    "search": [
        "Paraphrase the quoted description of {object} without searching {detail}",
        "Explain what the term {object} means in prose, without querying {detail}",
    ],
    "git": [
        "Summarize the quoted release note for {object} without inspecting {detail}",
        "Explain the branching convention named {object} in prose, without reading {detail}",
    ],
    "lsp": [
        "Define the term {object} from the quoted glossary, without navigating {detail}",
        "Paraphrase how {object} is described in prose, without opening {detail}",
    ],
    "verification": [
        "Explain the quoted test policy for {object} in prose, without running {detail}",
        "Summarize what the coverage target for {object} means, without executing {detail}",
    ],
    "research": [
        "Restate the quoted paragraph about {object} in your own words, without fetching {detail}",
        "Answer the factual question about {object} from the provided quote, without searching {detail}",
    ],
    "context": [
        "Restate the quoted goal statement for {object} without reading {detail}",
        "Summarize the quoted plan excerpt for {object} in prose, without opening {detail}",
    ],
    "plugin": [
        "Summarize the quoted catalog blurb for {object} without contacting {detail}",
        "Explain the quoted pricing note for {object} in prose, without browsing {detail}",
    ],
    "shell": [
        "Explain the quoted runbook paragraph for {object} without executing {detail}",
        "Paraphrase the quoted exit-code table for {object}, without running {detail}",
    ],
    "structured": [
        "Describe the quoted schema excerpt for {object} in prose, without querying {detail}",
        "Explain what field {object} means according to the quote, without opening {detail}",
    ],
}

OBJECTS = [
    "render_pipeline", "auth_token", "snapshot_store", "task_queue", "index_shard",
    "config_manifest", "event_bus", "retry_budget", "schema_registry", "artifact_cache",
    "session_ledger", "policy_table", "query_planner", "merge_queue", "watch_stream",
    "token_bucket", "lease_manager", "diff_viewer", "prompt_cache", "audit_trail",
    "feature_flag", "rollout_plan", "backup_slice", "restore_point", "metric_sink",
    "trace_span", "log_shipper", "secret_vault", "key_ring", "rate_limiter",
    "build_graph", "dep_lock", "test_harness", "fixture_pack", "seed_bundle",
    "migration_chain", "rollback_tag", "canary_probe", "health_gate", "quota_pool",
]

DETAILS = [
    "workspace snapshot ws-0101", "workspace snapshot ws-0102", "workspace snapshot ws-0103",
    "module atlas compiled at rev r2201", "module atlas compiled at rev r2202",
    "runbook excerpt rb-3301", "runbook excerpt rb-3302", "quoted transcript t-4401",
    "quoted transcript t-4402", "plan annex pa-5501", "plan annex pa-5502",
    "snapshot log sl-6601", "snapshot log sl-6602", "fixture index fi-7701",
    "fixture index fi-7702", "catalog page cp-8801", "catalog page cp-8802",
    "audit excerpt ae-9901", "audit excerpt ae-9902", "harness report hr-1011",
    "harness report hr-1012", "coverage map cm-1111", "coverage map cm-1112",
    "history view hv-1211", "history view hv-1212", "status board sb-1311",
    "status board sb-1312", "config overlay co-1411", "config overlay co-1412",
    "memory annex ma-1511",
]


def normalize(text: str) -> str:
    return re.sub(r"\s+", " ", text.casefold()).strip()


def tokens(text: str) -> set[str]:
    return set(re.findall(r"[a-z0-9_]+", normalize(text)))


def candidate(name: str, description: str, category: str, disclosure: str) -> dict:
    return {
        "name": name,
        "description": description,
        "category": category,
        "disclosure": disclosure,
        "synthetic_identity": False,
    }


def main() -> int:
    rng = random.Random(SEED)
    all_tools = [(family, *tool) for family, tools in TOOLS.items() for tool in tools]
    cases: list[dict] = []
    seen_contexts: set[str] = set()
    singleton_ids: set[str] = set()
    pair_index = 0

    def claim_context(text: str) -> str:
        key = normalize(text)
        if key in seen_contexts:
            raise AssertionError(f"duplicate normalized context: {text!r}")
        seen_contexts.add(key)
        return text

    def distractors(family: str, exclude: set[str], count: int, context: str = "") -> list[dict]:
        context_tokens = tokens(context)
        pool = [
            (owner, name, description, category, disclosure)
            for owner, name, description, category, disclosure in all_tools
            if name not in exclude
        ]
        # Rank cross-family distractors by lexical closeness to the context so
        # the benchmark contains genuinely confusable alternatives; seeded
        # jitter keeps the selection diverse instead of deterministic-greedy.
        scored = [
            (-len(context_tokens & tokens(f"{name} {description}")), rng.random(), (owner, name, description, category, disclosure))
            for owner, name, description, category, disclosure in pool
        ]
        scored.sort(key=lambda item: (item[0], item[1]))
        chosen = []
        for _, _, (owner, name, description, category, disclosure) in scored:
            # Prefer cross-family distractors; allow one same-family alternative.
            if owner == family and any(c["name"] in exclude for c in chosen):
                continue
            chosen.append(candidate(name, description, category, disclosure))
            if len(chosen) == count:
                break
        return chosen

    def lexical_distractor(context: str, exclude: set[str]) -> dict | None:
        context_tokens = tokens(context)
        best = None
        best_overlap = 1
        for owner, name, description, category, disclosure in all_tools:
            if name in exclude:
                continue
            overlap = len(context_tokens & tokens(f"{name} {description}"))
            if overlap > best_overlap:
                best_overlap = overlap
                best = candidate(name, description, category, disclosure)
        return best

    def emit(
        family: str,
        context: str,
        candidates: list[dict],
        relevance: dict[str, int],
        tags: list[str],
        group_token: str,
        pair_token: str | None,
        index: int,
    ) -> None:
        nonlocal pair_index
        ordered = [c["name"] for c in candidates]
        preferred = [name for name in ordered if name in relevance]
        none = not relevance
        full_tags = [family, *tags]
        case_id = f"{family}-semantic-{group_token}-variant-{index}"
        group_id = f"{family}-semantic-{group_token}-case-{index}"
        if pair_token is None:
            semantic = f"{family}-semantic-{group_token}"
            variant = f"{family}-semantic-{group_token}-lineage"
        else:
            semantic = f"{family}-semantic-{pair_token}"
            variant = f"{family}-semantic-{pair_token}-counterfactual"
        cases.append(
            {
                "schema_version": 1,
                "case_id": case_id,
                "context": claim_context(context),
                "candidates": candidates,
                "relevance": relevance,
                "preferred_order": preferred,
                "none": none,
                "tags": full_tags,
                "group_id": group_id,
                "provenance": "generated-local-template-v2",
                "semantic_group": semantic,
                "task_family": family,
                "tool_family": family,
                "generated_variant_family": variant,
                "teacher_probabilities": {},
            }
        )

    for family, size in FAMILY_SIZES.items():
        tools = TOOLS[family]
        none_budget, dual_budget, unknown_budget, hard_budget = TAG_BUDGETS[family]
        pair_count = 4
        single_count = size - pair_count * 2
        objects = OBJECTS[:]
        details = DETAILS[:]
        rng.shuffle(objects)
        rng.shuffle(details)
        cursor = 0

        def next_slots() -> tuple[str, str]:
            nonlocal cursor
            obj = objects[cursor % len(objects)]
            det = details[(cursor * 7 + len(family)) % len(details)]
            cursor += 1
            return obj, f"{det} (ref {family[:2]}-{cursor:04d})"

        # --- counterfactual pairs: same ordered candidates, differing relevance.
        for _ in range(pair_count):
            pair_token = f"{pair_index:03d}"
            pair_index += 1
            template, primary, secondary = rng.choice(TASKS[family])
            obj_a, det_a = next_slots()
            obj_b, det_b = next_slots()
            primary_name = tools[primary][0]
            second_name = tools[secondary][0] if secondary is not None else None
            stem_a = template.format(object=obj_a, detail=det_a)
            extra = distractors(family, {primary_name, second_name} if second_name else {primary_name}, 3, stem_a)
            ordered = [candidate(*tools[primary])]
            if second_name:
                ordered.append(candidate(*tools[secondary]))
            ordered.extend(extra)
            pattern = rng.choice(["flip", "abstain", "narrow"])
            if pattern == "flip" and second_name:
                context_a = stem_a
                context_b = (
                    template.format(object=obj_b, detail=det_b)
                    + f" Later review shows {obj_b} is already handled; instead reconcile {second_name} state for {det_b}."
                )
                emit(family, context_a, [dict(c) for c in ordered], {primary_name: 3}, ["counterfactual", "context-sensitive"], pair_token, pair_token, 1)
                emit(family, context_b, [dict(c) for c in ordered], {second_name: 3}, ["counterfactual", "context-sensitive"], pair_token, pair_token, 2)
            elif pattern == "abstain":
                context_a = stem_a
                context_b = (
                    f"From the quoted paragraph alone, without inspecting {det_b}, "
                    f"restate what {obj_b} means in prose."
                )
                emit(family, context_a, [dict(c) for c in ordered], {primary_name: 3}, ["counterfactual", "context-sensitive"], pair_token, pair_token, 1)
                emit(family, context_b, [dict(c) for c in ordered], {}, ["counterfactual", "context-sensitive", "no-tool", "abstention"], pair_token, pair_token, 2)
            else:  # narrow: dual relevance collapses to a single winner.
                if not second_name:
                    taken = {c["name"] for c in ordered}
                    second = next(
                        (d for d in distractors(family, {primary_name}, 8, stem_a) if d["name"] not in taken),
                        None,
                    )
                    assert second is not None
                    ordered.insert(1, second)
                    second_name = second["name"]
                context_a = stem_a
                context_b = (
                    template.format(object=obj_b, detail=det_b)
                    + f" Only the {primary_name} step is still pending for {obj_b}; the {second_name} step already landed."
                )
                emit(family, context_a, [dict(c) for c in ordered], {primary_name: 2, second_name: 2}, ["counterfactual", "context-sensitive", "multi-tool"], pair_token, pair_token, 1)
                emit(family, context_b, [dict(c) for c in ordered], {primary_name: 3}, ["counterfactual", "context-sensitive"], pair_token, pair_token, 2)

        # --- singletons.
        single_kinds: list[str] = (
            ["none"] * none_budget + ["dual"] * dual_budget + ["single"] * (single_count - none_budget - dual_budget)
        )
        assert len(single_kinds) == single_count, (family, len(single_kinds), single_count)
        rng.shuffle(single_kinds)
        unknown_left = unknown_budget
        hard_left = hard_budget
        for position, kind in enumerate(single_kinds):
            group_token = f"{pair_index + position:03d}"
            if kind == "none":
                template = rng.choice(NO_TOOL_TASKS[family])
                obj, det = next_slots()
                context = template.format(object=obj, detail=det)
                filler = distractors(family, set(), 3, context)
                emit(family, context, filler, {}, ["no-tool", "abstention"], group_token, None, 1)
                singleton_ids.add(cases[-1]["case_id"])
                continue
            template, primary, secondary = rng.choice(TASKS[family])
            obj, det = next_slots()
            context = template.format(object=obj, detail=det)
            primary_name = tools[primary][0]
            if kind == "dual":
                second_name = tools[secondary][0] if secondary is not None else distractors(family, {primary_name}, 1)[0]["name"]
                if secondary is None:
                    ordered = [candidate(*tools[primary]), distractors(family, {primary_name}, 1)[0]]
                else:
                    ordered = [candidate(*tools[primary]), candidate(*tools[secondary])]
                relevance = {primary_name: 2, ordered[1]["name"]: 2}
                tags = ["multi-tool"]
            else:
                ordered = [candidate(*tools[primary])]
                relevance = {primary_name: 3}
                tags = []
            exclude = set(relevance)
            ordered.extend(distractors(family, {c["name"] for c in ordered}, 2, context))
            # Top up to at least 3 candidates.
            while len(ordered) < 3:
                ordered.extend(distractors(family, {c["name"] for c in ordered}, 1, context))
            if hard_left > 0:
                # Structural lexical trap: name a real cross-family alternative
                # and echo its descriptor, then explicitly mark that trail
                # stale. The ranker must follow the task (resolve the named
                # object), not the lexical match.
                taken = {c["name"] for c in ordered} | exclude
                decoy = lexical_distractor(context, taken) or next(
                    (
                        candidate(name, description, category, disclosure)
                        for owner, name, description, category, disclosure in all_tools
                        if owner != family and name not in taken
                    ),
                    None,
                )
                assert decoy is not None
                context = (
                    context
                    + f" The transcript mentions {decoy['name']} ({decoy['description']}) output,"
                    + f" but that trail is stale; resolve {obj} directly instead."
                )
                ordered.append(decoy)
                tags.append("hard-negative")
                hard_left -= 1
            if unknown_left > 0 and kind == "single":
                tags.append("unknown-tool")
                unknown_left -= 1
            emit(family, context, ordered, relevance, tags, group_token, None, 1)
            singleton_ids.add(cases[-1]["case_id"])
        pair_index += single_count
        if unknown_left or hard_left:
            # Assign leftovers deterministically. Tags may overlap: a case can
            # be both a hard-negative measurement and an unknown-tool source.
            for case in cases:
                if case["task_family"] != family or case["case_id"] not in singleton_ids:
                    continue
                if unknown_left and len(case["relevance"]) == 1 and "unknown-tool" not in case["tags"]:
                    case["tags"].append("unknown-tool")
                    unknown_left -= 1
                elif hard_left and not case["none"] and "hard-negative" not in case["tags"]:
                    # Only tag when a real lexical decoy is already present;
                    # leftovers must not dilute the hard-negative definition.
                    context_tokens = tokens(case["context"])
                    relevant = set(case["relevance"])
                    if any(
                        len(context_tokens & tokens(f"{cand['name']} {cand['description']}")) >= 2
                        for cand in case["candidates"]
                        if cand["name"] not in relevant
                    ):
                        case["tags"].append("hard-negative")
                        hard_left -= 1
                if not unknown_left and not hard_left:
                    break
        assert unknown_left == 0, (family, unknown_left)
        assert hard_left == 0, (family, hard_left)

    # --- global assertions (fail loudly instead of writing a bad corpus).
    assert len(cases) == 256, len(cases)
    assert len({c["case_id"] for c in cases}) == 256
    assert len({c["group_id"] for c in cases}) == 256
    assert len(seen_contexts) == 256, len(seen_contexts)
    variant_families = {c["generated_variant_family"] for c in cases}
    assert len(variant_families) == 216, len(variant_families)
    semantic_groups = {c["semantic_group"] for c in cases}
    assert len(semantic_groups) == 216, len(semantic_groups)
    from collections import Counter

    tag_counts = Counter(tag for c in cases for tag in c["tags"])
    assert sum(1 for c in cases if c["none"]) >= 32
    assert tag_counts["multi-tool"] >= 32, tag_counts
    assert tag_counts["hard-negative"] >= 64, tag_counts
    assert tag_counts["unknown-tool"] >= 32, tag_counts
    assert len({c["task_family"] for c in cases}) >= 10
    pair_families = [fam for fam in variant_families if fam.endswith("-counterfactual")]
    assert len(pair_families) >= 40, len(pair_families)
    # Counterfactual validity: shared ordered candidate names, differing labels.
    by_variant: dict[str, list[dict]] = {}
    for c in cases:
        by_variant.setdefault(c["generated_variant_family"], []).append(c)
    valid_pairs = sum(
        1
        for fam, members in by_variant.items()
        if len(members) == 2
        and [c["name"] for c in members[0]["candidates"]] == [c["name"] for c in members[1]["candidates"]]
        and members[0]["relevance"] != members[1]["relevance"]
    )
    assert valid_pairs >= 32, valid_pairs
    # Hard-negative honesty: decoy shares >=2 content tokens with the context.
    for c in cases:
        if "hard-negative" not in c["tags"]:
            continue
        context_tokens = tokens(c["context"])
        relevant = set(c["relevance"])
        assert any(
            len(context_tokens & tokens(f"{cand['name']} {cand['description']}")) >= 2
            for cand in c["candidates"]
            if cand["name"] not in relevant
        ), c["case_id"]

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    with OUTPUT.open("w", encoding="utf-8") as handle:
        for case in cases:
            handle.write(json.dumps(case, ensure_ascii=False, sort_keys=True) + "\n")
    print(f"wrote {len(cases)} cases to {OUTPUT}")
    print(f"leakage groups: {len(variant_families)}, valid counterfactual pairs: {valid_pairs}")
    print(dict(tag_counts))
    return 0


if __name__ == "__main__":
    sys.exit(main())
