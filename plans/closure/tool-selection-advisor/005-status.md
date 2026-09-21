# Tool-Selection Advisor Milestone 005 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/tool-selection-advisor/005-advisor-integration-and-qualification.md`
Source subsystem roadmap: `plans/subsystems/tool-selection-advisor-roadmap.md#m005--advisor-integration-and-downstream-qualification`
Repository baseline reviewed: `48e3d31`
Implementation commit: `48e3d31` — advisor integration and qualification

## 1. Executive finding

M005 is complete. The existing policy-filtered `tool_search` path now supports
explicit `off`, `observe`, `rerank`, and bounded `promote` projections. The
default remains `off`; advisor failures, abstentions, and over-threshold
misses preserve deterministic discovery. Production registry construction
threads the optional config without changing default/test constructors.

The qualification harness is deterministic and offline. It compares keyword,
BM25, learned observe, learned rerank, and learned promote by fixture tier,
includes an unknown-tool holdout, reports CPU/prompt bounds, and records policy
negative coverage. No external provider or remote inference is required.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Default-off equivalence | `AdvisorMode::Off`, `NoopAdvisor`, default registry construction, and `tool_search` off test |
| Observe-only behavior | Projection test proves scores do not alter ordering or candidates |
| Candidate-set preserving rerank | `rerank_preserves_current_candidate_set_and_promotion_respects_allowlist` |
| Authority-safe promotion | Only the independently policy-filtered deferred set is eligible; hidden/denied names are excluded |
| Abstention/failure fallback | `abstention_and_failure_leave_discovery_unchanged`; bounded fallback path in `project_discovery` |
| Unknown MCP/plugin descriptors | `assets/tool-advisor/downstream-suite.jsonl`, corpus unknown-tool cases, synthetic holdout |
| Qualification command | `codegg tool-advisor qualify --model <artifact> --suite <jsonl>` |
| Telemetry independence | Projection and search integration have no training-data sink dependency; M004 consent defaults remain unchanged |
| Budget bounds | Promotion cap 2; qualification reports candidate count, serialized candidate bytes, and scoring time |

## 3. Qualification evidence

Command:

```
cargo run --bin codegg --features tool-advisor-training -- tool-advisor qualify \
  --model target/tool-advisor-smoke/model.json \
  --suite assets/tool-advisor/downstream-suite.jsonl --json
```

Downstream suite fingerprint: `d6ad404ede19b73bdebf1e3aceb46af0298c257f38718d2581af2bb03f647973`.
Training/corpus fingerprint: `b191a2f0eb5051622ebec763e6d614ed844be6cbc3f00791c6e1994fc67963c1`.

The separate downstream fixture produced:

- `small`: BM25 MRR `0.500`, learned rerank MRR `0.500`;
- `small-tool-fragile`: BM25 and learned MRR `1.000`;
- `tool-fragile`: BM25 and learned MRR `1.000`, while nDCG@5 improved
  from `0.787` to `0.956`;
- unknown-tool holdout: candidate coverage `1.000`, Recall@1 `0.750`,
  MRR `0.750`, no-tool F1 `1.000`;
- learned scoring: `2 ms`, maximum `3` candidates and `459` serialized
  candidate bytes in this local run; promotion cap `2`.

This is qualification-fixture evidence, not a claim of hosted small-model
trajectory improvement. Because no external primary-model trajectory was
introduced, rerank and promote remain opt-in experimental modes; observe is
the supported non-mutating qualification mode.

## 4. Verification executed

- `cargo fmt --all -- --check` — passed.
- `cargo test --lib tool_advisor -- --nocapture` — 17 passed.
- `cargo test --lib tool::tool_search -- --nocapture` — 4 passed.
- `cargo test --lib tool::catalog -- --nocapture` — 21 passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `scripts/verify.sh quick` — passed.
- `cargo test --workspace --locked -- --test-threads=1` — 4,842 passed,
  one unrelated `tool::lsp_preview_apply::tests::fresh_tool_registry_expires_prior_preview_id`
  failure; the exact test passed in an isolated rerun.
- `git diff --check` — passed.

The workspace failure is an allocator-sensitive pointer-identity assertion in
the pre-existing LSP preview test, outside this workstream. It did not recur
in the isolated exact rerun and no advisor test failed.

## 5. Security and invariant review

The advisor receives only the candidate descriptors supplied by the existing
policy-filtered discovery path. Reranking cannot add a candidate. Promotion
cannot resurrect hidden, denied, disabled, plan-ineligible, or parent-ceiling
tools because those tools never enter its deferred-allowed input. The advisor
never receives execution arguments and cannot invoke a broker or scheduler.
Advisor mode does not enable telemetry, remote transport, or content capture.

## 6. Unresolved findings

- Low: downstream evidence is a deterministic qualification fixture rather
  than a live external-primary-model trajectory; rerank/promote are therefore
  not promoted beyond experimental opt-in.
- Low: the full workspace sweep observed one unrelated allocator-sensitive LSP
  preview pointer-identity failure; its isolated exact rerun passed.

No critical, high, or medium findings remain.

## 7. Roadmap and dependency disposition

M005 is closed and the tool-selection advisor roadmap is closed. The registry
audit found no later registered tool-advisor plan to unblock, and no dependent
plan requires a status change. The final default-off and telemetry-consent
boundaries remain in force; any future live primary-model A/B study would
require a new plan rather than silently broadening this closure.
