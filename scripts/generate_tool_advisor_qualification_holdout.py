#!/usr/bin/env python3
"""Generate the local-only C001 evaluation holdout.

This generator deliberately has no model, tokenizer, or inference dependency.
Every label is authored by the deterministic family/template table below.
"""

from __future__ import annotations

import hashlib
import json
import sys
from collections import Counter
from pathlib import Path


OUTPUT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("assets/tool-advisor/qualification-v2-holdout.jsonl")
MANIFEST = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("assets/tool-advisor/qualification-v2-holdout-manifest.json")

FAMILIES = (
    ("plugin", "plugin catalog and MCP server", "plugin_search"),
    ("lsp", "language server symbol navigation", "lsp_definition"),
    ("research", "local research notes and citations", "research_search"),
    ("search", "documentation and repository search", "web_search"),
    ("structured", "structured record inspection", "json_query"),
    ("data", "tabular data transformation", "data_query"),
    ("git", "local repository history", "git_log"),
)

CORE = [
    {"name": "grep", "description": "Search literal text in files", "category": "ReadOnly", "disclosure": "core", "synthetic_identity": False},
    {"name": "plan_get", "description": "Show the active work plan", "category": "ReadOnly", "disclosure": "core", "synthetic_identity": False},
]


def candidate(name: str, description: str) -> dict[str, object]:
    return {"name": name, "description": description, "category": "ReadOnly", "disclosure": "deferred", "synthetic_identity": False}


def case(index: int, family: str, description: str, relevant: str) -> dict[str, object]:
    none = index % 8 == 0
    hard_negative = index % 4 == 0
    unknown = index % 5 == 0
    multi = not none and index % 7 == 0
    long_session = index % 6 == 0
    tags = [family, "qualification-v2"]
    if hard_negative:
        tags.append("hard-negative")
    if unknown:
        tags.append("unknown-renamed")
    if multi:
        tags.append("multi-tool")
    if none:
        tags.extend(("no-tool", "abstention"))
    if long_session:
        tags.extend(("context-v2-long-session", "contextual"))
    if index % 8 == 0:
        tags.append("counterfactual")
    deferred = [
        candidate(relevant, f"Use the deferred {description} capability"),
        candidate(f"{family}_fallback", f"Fallback {description} capability"),
    ]
    candidates = CORE + deferred
    if none:
        relevance: dict[str, int] = {}
        preferred: list[str] = []
    elif multi:
        relevance = {relevant: 3, f"{family}_fallback": 2}
        preferred = [relevant, f"{family}_fallback"]
    else:
        relevance = {relevant: 3}
        preferred = [relevant]
    token = f"qv2-{family}-{index:03d}"
    return {
        "schema_version": 1,
        "case_id": f"qualification-v2-{family}-{index:03d}",
        "context": (
            f"In fresh evaluation record {token}, use the {description} tool for "
            f"workspace item {token}. This locally authored context is not drawn "
            f"from the historical corpus."
        ),
        "candidates": candidates,
        "relevance": relevance,
        "preferred_order": preferred,
        "none": none,
        "tags": tags,
        "group_id": f"qualification-v2-group-{index:03d}",
        "provenance": "generated-local-qualification-v2-template-v1",
        "semantic_group": f"qualification-v2-semantic-{family}-{index:03d}",
        "leakage_group": f"qualification-v2-leakage-{family}-{index:03d}",
        "task_family": family,
        "tool_family": family,
        "generated_variant_family": f"qualification-v2-template-{family}-{index:03d}-v1",
        "teacher_probabilities": {},
    }


def canonical_json(row: dict[str, object]) -> str:
    # Match ToolAdvisorCase::canonical_json field order and serde's compact form.
    ordered = {
        "schema_version": row["schema_version"],
        "case_id": row["case_id"],
        "context": row["context"],
        "candidates": row["candidates"],
        "relevance": row["relevance"],
        "preferred_order": row["preferred_order"],
        "none": row["none"],
        "tags": row["tags"],
        "group_id": row["group_id"],
        "provenance": row["provenance"],
        "semantic_group": row["semantic_group"],
        "leakage_group": row["leakage_group"],
        "task_family": row["task_family"],
        "tool_family": row["tool_family"],
        "generated_variant_family": row["generated_variant_family"],
        "teacher_probabilities": row["teacher_probabilities"],
    }
    return json.dumps(ordered, ensure_ascii=False, separators=(",", ":"))


def dataset_fingerprint(rows: list[dict[str, object]]) -> str:
    payload = "".join(canonical_json(row) + "\n" for row in sorted(rows, key=lambda row: str(row["case_id"])))
    return hashlib.sha256(payload.encode()).hexdigest()


def main() -> None:
    rows: list[dict[str, object]] = []
    index = 0
    for family, description, relevant in FAMILIES:
        count = 20 if family in {"plugin", "lsp", "research", "search", "structured"} else 12
        if family == "git":
            count = 16
        for _ in range(count):
            rows.append(case(index, family, description, relevant))
            index += 1
    assert len(rows) == 128
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text("".join(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n" for row in rows))
    raw_sha = hashlib.sha256(OUTPUT.read_bytes()).hexdigest()
    tags = Counter(tag for row in rows for tag in row["tags"])
    family_counts = Counter(str(row["tool_family"]) for row in rows)
    manifest = {
        "schema_version": 1,
        "dataset": str(OUTPUT),
        "dataset_fingerprint": dataset_fingerprint(rows),
        "raw_sha256": raw_sha,
        "case_count": len(rows),
        "minimum_leakage_groups": 96,
        "construction": "deterministic local labels; no selected-model inference; no remote teacher",
        "provenance": "generated-local-qualification-v2-template-v1",
        "family_counts": dict(sorted(family_counts.items())),
        "tag_counts": dict(sorted(tags.items())),
        "required_families": ["plugin", "lsp", "research/search", "structured/data"],
        "required_slices": ["hard-negative", "unknown-renamed", "no-tool", "multi-tool", "context-v2-long-session"],
    }
    MANIFEST.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    print(json.dumps(manifest, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
