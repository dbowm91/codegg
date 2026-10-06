#!/usr/bin/env python3
"""Regenerate immutable v1 decision-contract fixtures from frozen advisor cases."""

import hashlib
import json
from copy import deepcopy
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "assets/tool-advisor/corpus.jsonl"
OUTPUT = ROOT / "assets/decision-runtime/compatibility-v1.jsonl"
CASE_IDS = (
    "filesystem-semantic-000-variant-1",  # multiple relevant candidates
    "filesystem-semantic-000-variant-2",  # one relevant candidate
    "filesystem-semantic-001-variant-2",  # explicit no-tool label
    "filesystem-semantic-005-variant-1",  # unknown/synthetic candidate family
)


def main() -> None:
    lines = SOURCE.read_bytes().splitlines()
    cases = {}
    for raw in lines:
        case = json.loads(raw)
        if case["case_id"] in CASE_IDS:
            cases[case["case_id"]] = (raw, case)
    if set(cases) != set(CASE_IDS):
        raise SystemExit("frozen source corpus is missing one or more preregistered fixture cases")

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    rendered = []
    for case_id in CASE_IDS:
        raw, case = cases[case_id]
        candidate_rows = [
            {"id": c["name"], "label": c["name"], "description": c["description"]}
            for c in case["candidates"]
        ]
        if case_id == "filesystem-semantic-005-variant-1":
            candidate_rows.append({
                "id": "mcp__unknown_server__synthetic_7f3a",
                "label": "unseen external tool",
                "description": "synthetic identity used to verify opaque candidate ids",
            })
        request = {
            "request_id": case_id,
            "schema_version": 1,
            "state": [{"key": "context", "value": case["context"]}],
            "spec": {
                "kind": "rank",
                "instruction": "Estimate relevance independently for each candidate.",
                "multi_relevance": True,
                "candidates": candidate_rows,
            },
        }
        if case.get("none", False):
            status = {"status": "abstained", "reason": "frozen source case labels no relevant candidate"}
            answer = None
        else:
            status = {"status": "answered"}
            answer = {
                "kind": "rank",
                "candidates": [
                    {
                        "candidate_id": c["id"],
                        "relevance": label / 3.0,
                        "ranking_score": float(label),
                    }
                    for c in candidate_rows
                    for label in [case.get("relevance", {}).get(c["id"], 0)]
                ],
            }
        response = {
            "request_id": case_id,
            "schema_version": 1,
            "status": status,
            "answer": answer,
            "provenance": {
                "backend": "frozen-advisor-label-fixture",
                "model": None,
                "runtime_version": "corpus-labels-v1",
                "latency_micros": 0,
            },
        }
        record = {
            "fixture_schema_version": 1,
            "fixture_id": case_id,
            "source_case_id": case_id,
            "source_sha256": hashlib.sha256(raw).hexdigest(),
            "request": request,
            "response": response,
        }
        rendered.append(json.dumps(record, sort_keys=True, separators=(",", ":")))

    def add_contract(fixture_id, request, response, valid=True):
        rendered.append(json.dumps({
            "fixture_schema_version": 1,
            "fixture_id": fixture_id,
            "source_case_id": None,
            "source_sha256": None,
            "request": request,
            "response": response,
            "expect_request_valid": valid,
        }, sort_keys=True, separators=(",", ":")))

    def response(request_id, status, answer=None):
        return {
            "request_id": request_id,
            "schema_version": 1,
            "status": status,
            "answer": answer,
            "provenance": {"backend": "contract-fixture", "model": None,
                           "runtime_version": "v1", "latency_micros": 0},
        }

    state = [{"key": "context", "value": "fixture state"}]
    add_contract("contract-binary", {
        "request_id": "contract-binary", "schema_version": 1, "state": state,
        "spec": {"kind": "binary", "question": "is the condition true?"},
    }, response("contract-binary", {"status": "answered"},
                {"kind": "binary", "probability_true": 0.9, "confidence": 0.8}))
    add_contract("contract-choice", {
        "request_id": "contract-choice", "schema_version": 1, "state": state,
        "spec": {"kind": "choice", "question": "choose one", "options": [
            {"id": "accept", "label": "Accept"}, {"id": "reject", "label": "Reject"}]},
    }, response("contract-choice", {"status": "answered"},
                {"kind": "choice", "option_id": "accept", "probability": 0.75}))
    add_contract("contract-score", {
        "request_id": "contract-score", "schema_version": 1, "state": state,
        "spec": {"kind": "score", "rubric": "quality", "minimum": 0.0, "maximum": 5.0},
    }, response("contract-score", {"status": "answered"},
                {"kind": "score", "value": 4.0, "confidence": 0.7}))

    rank_request = {
        "request_id": "contract-rank-synthetic", "schema_version": 1, "state": state,
        "spec": {"kind": "rank", "instruction": "rank candidates independently",
                 "multi_relevance": True, "candidates": [{
                     "id": "mcp__unknown_server__synthetic_7f3a",
                     "label": "unseen external tool", "description": "opaque external identity"}, {
                     "id": "tool/read", "label": "read", "description": "read bounded file contents"}]},
    }
    rank_response = response(rank_request["request_id"], {"status": "answered"}, {
        "kind": "rank", "candidates": [
            {"candidate_id": "mcp__unknown_server__synthetic_7f3a", "relevance": 0.6, "ranking_score": 2.0},
            {"candidate_id": "tool/read", "relevance": 0.0, "ranking_score": 0.0}],
    })
    add_contract("contract-rank-synthetic", rank_request, rank_response)
    permutation = deepcopy(rank_request)
    permutation["spec"]["candidates"].reverse()
    add_contract("contract-rank-order-permutation", permutation, rank_response)
    add_contract("contract-unsupported", {
        "request_id": "contract-unsupported", "schema_version": 1, "state": state,
        "spec": {"kind": "rank", "instruction": "rank", "multi_relevance": True,
                 "candidates": [{"id": "x", "label": "X", "description": "candidate"}]},
    }, response("contract-unsupported", {"status": "unsupported", "reason": "backend lacks Rank"}))
    add_contract("contract-unavailable", {
        "request_id": "contract-unavailable", "schema_version": 1, "state": state,
        "spec": {"kind": "binary", "question": "is it true?"},
    }, response("contract-unavailable", {"status": "unavailable", "reason": "backend is off"}))
    add_contract("contract-oversized-rejection", {
        "request_id": "contract-oversized-rejection", "schema_version": 1,
        "state": [{"key": "context", "value": "x" * (16 * 1024 + 1)}],
        "spec": {"kind": "binary", "question": "is it true?"},
    }, None, valid=False)
    OUTPUT.write_text("\n".join(rendered) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
