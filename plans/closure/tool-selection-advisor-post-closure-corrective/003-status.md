# Tool-Selection Advisor Post-Closure Corrective M003 — Closure Status

Status: closed
Source implementation plan: plans/implementation/tool-selection-advisor-post-closure-corrective/003-pre-turn-proactive-tool-disclosure.md
Source subsystem roadmap: plans/subsystems/tool-selection-advisor-post-closure-corrective-addendum.md# m003--pre-turn-proactive-tool-disclosure
Repository baseline reviewed: 10804b1
Implementation commits: 3b6c68a — tool advisor M003 proactive disclosure; 84e6db9 — tool advisor M003 disclosure matrix tests; 10804b1 — tool advisor M003 enter closure review

## 1. Executive finding

M003 is closed. Explicit `promote` mode now performs a bounded, turn-local
advisor projection after the final `ResolvedToolSurface` and before provider
definitions are finalized. A qualified deferred tool can therefore be visible
to the primary model without a preceding `tool_search` call. The projection is
strictly visibility-only: it cannot register tools, change schemas, grant
permissions, widen the authority surface, or execute anything.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Correct request-preparation seam | `src/agent/request_preparation.rs` resolves `ResolvedToolSurface`, projects bounded pre-turn advice, then partitions immediate/deferred provider definitions. |
| No prior search required | The projection is called directly during request preparation; the regression fixture invokes the pure helper without `tool_search`. |
| Strict allowed-surface subset | Candidates come from `candidates_from_surface`; predicted names are intersected with final deferred definitions and revalidated through canonical/wire mappings. |
| Off/observe equivalence | Off returns no projection; observe scores and records a diagnostic while returning no promotions. Unit coverage verifies both paths leave visibility unchanged. |
| Promotion bounds | Configuration clamps promotions to two; the helper enforces count, threshold, candidate-count, and serialized schema-byte budgets. Unit coverage verifies schema-budget overflow. |
| Abstention/failure safety | Abstention, advisor errors, and panics return an empty projection. Unit coverage verifies abstention. |
| Authority negatives | Denied/disabled/plan-ineligible/non-callable/parent-ceiling tools are removed before `ResolvedToolSurface`; required/never-reduce tools are explicitly excluded from learned promotion; non-surface predictions are ignored by the fixture. |
| Search fallback and telemetry | `tool_search` remains registered and reactive; no capture or network path was added. |

## 3. Production implementation evidence

- `AgentLoopServices` receives the configured advisor and bounded disclosure
  settings from `AgentLoop` without changing the default `NoopAdvisor` path.
- `project_preturn_promotions` owns UTF-8-safe context bounding, candidate
  construction, panic/error containment, abstention handling, score threshold,
  canonical/wire-name reconciliation, required/never-reduce protection, and
  schema-byte accounting.
- Promoted tools are ordered ahead of ordinary immediate tools before the
  existing initial-tool cap is applied; the cap cannot silently remove a
  qualified promotion from the front of the palette.
- Promote mode bypasses stale palette reuse because the projection is
  turn-local; observe/off/rerank preserve the existing cache behavior.
- Configuration documents `disclosure_threshold`, `max_promotions`, and
  `max_disclosure_schema_bytes`; defaults remain off and bounded.

## 4. Verification executed

- `cargo test --locked -p codegg --lib agent::request_preparation` — 6 passed.
- `cargo test --locked -p codegg --lib agent::tool_surface` — 5 passed.
- `cargo test --locked -p codegg --lib tool::tool_search` — 4 passed.
- `cargo test --locked -p codegg --test tool_surface_minimization` — 13 passed.
- `cargo test --locked -p codegg --lib tool_advisor` — 18 passed.
- `cargo check --locked` — passed.
- `cargo check --locked --features tool-advisor-training` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — no issues found.
- `scripts/verify.sh quick` — passed.
- `cargo fmt --all` and `git diff --check` — passed.

## 5. Invariant review

The advisor sees only the final policy-filtered surface and a bounded current
objective, never raw denied definitions or full transcript content. Promotion
changes only initial disclosure state. Broker, permission, sandbox, registry,
and execution paths remain authoritative and unchanged. No provider wire
protocol or durable session authority changed.

## 6. Failure and recovery review

Missing/no-op advisors, empty context, abstention, low confidence, schema
budget overflow, advisor errors, and advisor panics all preserve the existing
palette for that turn. Surface changes are recomputed per turn; no authority
decision is cached across a changed surface.

## 7. Migration and compatibility review

The only persisted/configured additions are optional advisor disclosure settings
with bounded defaults. Existing `off`, `observe`, `rerank`, and reactive
`tool_search` behavior remains available. No provider protocol or storage
migration is required.

## 8. Security and privacy review

No permission bypass, hidden-tool promotion, dynamic registration, execution
argument generation, telemetry enablement, model download, or network call was
added. The default advisor and capture paths remain disabled.

## 9. Documentation and operations

`architecture/tool-advisor.md` distinguishes reactive search reranking from
pre-turn disclosure. `architecture/agent-tool-surface.md` documents the
visibility projection and its authority boundary. The configuration settings
and hard caps are represented in `ToolAdvisorConfig`.

## 10. Unresolved findings (severity: critical/high/medium/low)

- None for M003.
- End-to-end live primary-model effectiveness remains intentionally unclaimed;
  it is the evidence requirement of M004.

## 11. Roadmap disposition

M003 is closed and unblocks the dependency gate for M004. M004 is now
dependency-complete in code, but remains blocked on its separate evidence
requirement: operator-configured live primary-model/provider calls, a frozen
held-out trajectory suite, and resource measurements. Those external inputs
are not present in this workspace and cannot be inferred from offline tests.

## 12. Registry updates

- M003 moved from closing to closed.
- M004 was audited after M003 closure and remains blocked only on its named
  live-qualification evidence, not on a missing code dependency.
- M001 and M002 remain closed; their historical closure records were not
  rewritten.
