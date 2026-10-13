# Eggsearch Evidence Fidelity and Agent Workflow Roadmap

Status: active (M001 historical closure; C001 corrective ready; M002-M005 dependency-blocked)
Planning baseline: CodeGG main `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03` (2026-10-13)
Upstream evidence: eggsearch `v0.4.1` published/qualified; upstream main declares `0.4.2`. CodeGG's managed pin remains `0.3.9` until the independently registered legacy-retirement M001 lands.

Authoritative references:
- `plans/000-long-term-specification.md` (explicit ownership; progressive disclosure)
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`
- `plans/subsystems/search-eggsearch-integration-roadmap.md` (closed predecessor)
- `plans/subsystems/search-eggsearch-legacy-retirement-corrective-addendum.md` (separate ACTIVE migration)
- `plans/closure/tool-surface-upstream-compatibility/007-status.md` (closed 0.3.9 alignment)
- `plans/subsystems/eggsearch-evidence-workflows-post-closure-corrective-addendum.md` (C001 strict retrieval-contract successor)
- https://github.com/eggstack/eggsearch/blob/main/architecture/codegg-contract.md
- https://github.com/eggstack/eggsearch/blob/main/docs/tool-matrix.md

## 1. Purpose and ownership

Improve CodeGG's consumption of eggsearch evidence: no duplicate network/provider engines, no replacement of the CodeGG research coordinator, no new competing tool-discovery layer. Eggsearch owns provider routing, fetch/cache, result/source identities, retrieval warnings, next-action *suggestions*, and evidence packaging. CodeGG owns stable native tool schemas, permissions, runtime transport, evidence/claim orchestration, model-visible projection, and policy-governed tool disclosure.

This is a successor capability/quality line, **not** a reopening of completed eggsearch-integration milestones. The parallel legacy-retirement M001 adopts a qualified eggsearch release and reconciles providers; its M002 removes the builtin execution branch. Do not implement those changes here or change their historical closure evidence.

## 2. Findings from repository audit

1. `src/research/sources/eggsearch.rs::convert_structured` ignores `StructuredSearchResult.success` and discards retrieval-level failures/coverage when converting to `Vec<SourceRecord>`. A partial/failed search must not look like a successful absence.
2. Source conversion ignores canonical nested `metadata.published_at`; replaces source identity with a new UUID each collection while preserving eggsearch `stable_id` only in free-text notes; always emits `trust=external_untrusted` even where upstream reports local provenance.
3. `src/search_backend/eggsearch.rs` retains full diagnostic structured values but displays a prefix of potentially verbose diagnostic JSON, limited by byte count. Bounded output can exclude more relevant evidence/warnings.
4. Native `repo_search` advertises legacy `profile`/`mode` rather than canonical task `goal`/`sources`; `research_search` and `security_search` similarly favor historical argument forms.
5. Native `websearch` does not expose recent upstream exact date, domain and language/region constraint semantics. These are capability-qualified, not guaranteed native filtering.
6. Eggsearch supplies bounded `next_actions`, `suggested_fetches` and metadata for tool hydration. CodeGG already has `tool_search` and role-specific disclosure; no second discovery mechanism is justified.

## 3. Invariants

- Shared daemon-owned MCP transport; explicit per-runtime `SearchRuntimeContext`; no process spawning per research call.
- Existing policies gate **all** tool execution, model visibility, URLs, SSRF/network access, and programmatic invocation. Upstream metadata never grants new authority.
- Source text and metadata, including `local_trusted` provenance, are **not** instruction-trusted.
- Full upstream structured evidence is captured internally when available; external model output is bounded, contextual, and trust-framed. Provider failures, absence and uncertainty are never conflated.
- Unknown additive upstream JSON fields and enum values do not crash CodeGG. No silent narrowing/rewriting of explicit search constraints.
- Preserve existing local grep/glob/Git/LSP semantics and CodeGG's synthesis/claim graph.
- Legacy provider retirement and managed sidecar pin remain owned by their pre-existing M001/M002 plans; no new persistent search cache, provider clients, auto-crawl, network CI lane, or standalone agent workflow scheduler.

## 4. Dependency graph

```text
Evidence M001 (historically closed) ---> Post-closure C001 (READY)
                                        tagged 0.4.1 dimension normalization
                                                      |
                                                      | hard
                                                      v
Existing legacy-retirement M001 (READY) ----------> M004 projection (blocked)
                    |                                     |
             +------+-------+                             |
             |              |                             |
             v              v                             |
        M002 repo        M003 web                         |
        goals (blocked)  constraints (blocked)            |
             |                                            |
             +------------------------> M005 hydration <--+
                                       (blocked)
```

- New evidence M001 is **historically closed** at `plans/closure/eggsearch-evidence-workflows/001-status.md`; its typed receipt transport is implemented, but the tagged upstream dimensional status contract escaped original verification.
- Post-closure corrective C001 is **ready** and must strictly close before M004. Authority: `plans/subsystems/eggsearch-evidence-workflows-post-closure-corrective-addendum.md`; handoff: `plans/implementation/eggsearch-evidence-workflows-corrective/001-tagged-retrieval-dimension-normalization.md`. It is independent of the runtime-pin migration, but must requalify on adopted release when available.
- M002 and M003 are **blocked** on the separate legacy-retirement M001 adopting and qualifying the pinned release.
- M004 is **blocked** on *both* post-closure C001 and legacy-retirement M001; do not render unverified completeness status.
- M005 is **blocked** on M002 and M004. Tool execution authority remains CodeGG-owned.
- Legacy-retirement M002 deletion is not a hard dependency of these new milestones; coordinate touched files and avoid parallel edits on one branch.

## 5. Milestones and handoffs

| Milestone | Class | Status | Implementation plan | User-visible exit |
|---|---|---|---|---|
| M001 Evidence identity, metadata, failure-state fidelity | correctness/infrastructure | historically closed; strict retrieval contract subject to C001 | `plans/implementation/eggsearch-evidence-workflows/001-evidence-fidelity-and-retrieval-state.md` | Historical closure `plans/closure/eggsearch-evidence-workflows/001-status.md`; C001 owns tagged-dimension follow-up |
| M002 Repository and specialist-search semantic controls | capability | blocked (legacy M001) | `plans/implementation/eggsearch-evidence-workflows/002-repository-and-specialist-search-goals.md` | Coding/research/security agents express task goals via current provider-neutral fields |
| M003 Constrained web discovery and fetch compatibility | capability | blocked (legacy M001) | `plans/implementation/eggsearch-evidence-workflows/003-web-search-constraints.md` | Exact dates/domains/locales are carried and enforcement limitations disclosed |
| M004 Budgeted semantic model projection | correctness/performance | blocked (legacy M001 + C001) | `plans/implementation/eggsearch-evidence-workflows/004-structured-context-projection.md` | Strict C001 tagged retrieval normalization and legacy-retirement M001 closure both required |
| M005 Policy-gated next-action tool hydration | capability/integration | blocked (M002 + M004) | `plans/implementation/eggsearch-evidence-workflows/005-next-action-disclosure-handoff.md` | Valid suggested follow-ups become discoverable without an extra tool-search hop, never auto-executed |

Strict corrective handoff: **C001 ready**, documented in `plans/implementation/eggsearch-evidence-workflows-corrective/001-tagged-retrieval-dimension-normalization.md`. Existing M001 closure remains immutable and is not evidence of tagged v0.4.1 per-dimension status qualification.

## 6. Cross-cutting requirements

**Security:** Treat every untrusted source field as data; preserve trust markers and injection-hit warnings. Never execute or grant permissions from `next_actions`; names, arguments, locators and suggested targets are untrusted hints. Respect existing per-turn policy, agent/parent ceiling, network disable, sandbox and repo boundaries.

**Compatibility/migration:** M001 must not change serialized research identity without a migration; prefer an additive typed evidence/receipt structure linked to existing `SourceRecord.id`. M002/M003 add optional native parameters and reject conflicting unsupported explicit requests. M004 preserves programmatic full values. No database migration expected without implementation-time evidence.

**Concurrency/cancellation/restart:** No new global state or background jobs. Bounded call/output budgets and cancellation follow existing MCP/tool seams. Retried/replayed retrieval must retain evidence IDs and status even if order differs.

**Observability/quality:** Use existing tests, `doctor search` and tool-result metadata. Measure output bytes/token-proxy, complete-card retention, and status fidelity on frozen fixtures rather than claim cost/quality wins from theoretical estimates. One optional real pinned-binary smoke; normal CI stays offline.

## 7. Deferred

PDF page-selection/headless browser profile exposure, autonomous next-action chains, recurring crawling, search-result persistence, vector indexing and per-provider UI configuration remain deferred until separately evidenced. Optional eggsearch browser/PDF features and sandbox authority require a separate deliberate review.

## 8. Closure requirements

Each milestone gets a `plans/closure/eggsearch-evidence-workflows/NNN-status.md` record with implementation SHA, requirement-to-evidence table, focused and quick verification, semantic failure tests, security/concurrency observations, known limitations and disposition. Mark the next milestone ready only after hard dependencies have accepted closure. Keep historical roadmaps and existing legacy-retirement plans traceable.
