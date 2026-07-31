# Agent Runtime, Model Adaptation, and ACP Milestone 005 — Specialized Research Runtime

Status: implemented

Repository baseline: `672479726f1c79bbc931d70f084cd1649e8b2ed4`

Source roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-005--specialized-research-runtime`

Long-term requirements:

- `plans/000-long-term-specification.md#4.2-explicit-ownership`
- `plans/000-long-term-specification.md#4.5-locality-by-default`
- `plans/000-long-term-specification.md#4.6-progressive-disclosure`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`

Primary class: capability

## 1. Objective

Convert `runtime_kind = research` into a bounded host-side research coordinator that uses explicit execution/workspace context, decomposes suitable questions into a small number of non-overlapping evidence tasks, gathers structured source/claim records, and performs final synthesis with citation and uncertainty checks.

The milestone must replace reliance on a monolithic prompt/tool convention as the only orchestration mechanism. It must preserve the existing research service/tool as a reusable evidence/synthesis component where appropriate, while preventing process-global cwd, broad write authority, unbounded fan-out, duplicate source collection, and unstructured child essays.

## 2. Dependencies

Hard dependencies:

- M001 canonical prompt and agent resolution;
- M002 resolved tool/capability surface;
- M003 bounded nested delegation and lineage.

Existing interfaces:

- built-in `research` agent and prompt;
- `ResearchTool` and `ResearchService`;
- websearch/webfetch and domain-specific providers;
- artifact/context stores;
- task/subagent report and event infrastructure;
- explicit `ExecutionContext` available to production turns.

No external paid research provider is required for closure. Captured/local fixtures must cover deterministic behavior.

## 3. Current implementation evidence

Re-audit at implementation time:

- `AgentRuntimeKind::Research` is represented but not dispatched into specialized runtime behavior;
- `assets/agents/research.toml` currently grants research/web tools, `task`, todo tools, and mutating tools at ask/allow levels that do not match the descendant worker's effective filtering;
- the research prompt directs the model to call `research` for deep work and `websearch` for quick lookup;
- `ResearchTool::with_default_service` initializes a service from `std::env::current_dir()`;
- `ResearchTool` invokes `ResearchService::answer_for_agent` with mode/depth and a long timeout;
- child output is generic text with opportunistic report parsing rather than structured evidence records;
- source/citation quality, duplicate sources, conflicts, and unresolved questions need runtime-level representation.

## 4. Invariants

- Research service identity derives from explicit project/workspace execution context, never process-global cwd.
- Research is read-only by default.
- The ordinary scheduler, task service, tool broker, permissions, cancellation, and event log remain authoritative.
- Fan-out, depth, source count, response size, time, model calls, and tool calls are bounded.
- Child scouts return structured evidence and source records, not authoritative final answers.
- The parent coordinator owns final synthesis and citation validation.
- Confirmed, conflicting, weakly supported, and unresolved claims remain distinguishable.
- Full source bodies and large extracts remain handle-backed or bounded.
- Network/tool failure is explicit and cannot be silently converted into confident synthesis.
- Research children cannot widen network, filesystem, shell, mutation, or delegation authority.

## 5. Scope

### In scope

- Define a research specialized-runtime hook using the common M004-compatible interface.
- Define `ResearchRequest`, `ResearchPlan`, `SourceRecord`, `EvidenceRecord`, `ClaimRecord`, `ClaimConflict`, `ResearchEvidenceReport`, and final `ResearchReport` types.
- Classify a request as quick lookup, direct repository/spec investigation, or multi-source research.
- For multi-source work, create a bounded decomposition with non-overlapping child tasks.
- Support a small set of child roles through configuration, for example:
  - source scout;
  - repository/spec investigator;
  - claim verifier.
- Construct research services/tools from explicit `ExecutionContext` and project root.
- Normalize source identity, deduplicate URLs/documents, and retain retrieval timestamp/provider metadata.
- Track claim-to-evidence relationships and conflicting evidence.
- Validate citations/source references before final completion.
- Produce a typed, bounded final report with confidence and unresolved questions.
- Reconcile research agent permissions to read-only defaults and ordinary delegation policy.
- Publish bounded progress and final summaries.

### Out of scope

- Web-scale crawling or browser automation.
- Mandatory external search API keys.
- Autonomous code edits from research findings.
- Persistent global knowledge graph or vector database redesign.
- Arbitrary recursive research trees.
- Full academic systematic-review tooling.
- Citation-style formatting beyond stable source references and a readable report.
- Broad live-provider reliability CI.

## 6. Required production changes

### Specialized runtime and request classification

Use the same specialized-runtime hook established by M004. The research prepare phase should:

1. normalize the question and explicit scope;
2. inspect available repository/web capabilities;
3. decide quick/direct/multi-source mode deterministically or with one bounded planning call;
4. create a small typed plan;
5. allocate child tasks only when they add evidence diversity.

Do not call the full research pipeline recursively from every child.

### Explicit research service construction

Remove production reliance on `DEFAULT_SERVICE` rooted in cwd. Build/inject `ResearchService` from:

- project/workspace execution context;
- configured search backend/providers;
- artifact/cache stores;
- bounded timeout/source limits;
- optional session lineage.

A default convenience constructor may remain only for tests/legacy CLI if clearly isolated and not used by daemon turn paths.

### Structured evidence

A source record should include stable normalized locator, title, provider/source type, retrieval time, bounded excerpt/handle, content digest where available, and trust/relevance metadata.

An evidence record should identify source, claim fragment, support/contradict/uncertain relation, bounded quote/paraphrase, and confidence.

Children return `ResearchEvidenceReport`; they do not directly mark the root research complete.

### Synthesis and validation

The parent finalizer must:

- deduplicate sources and evidence;
- reject missing/unknown source references;
- identify conflicts and unresolved gaps;
- distinguish inference from direct support;
- ensure final citations refer to collected source records;
- produce bounded output and optional artifact handles for extended evidence.

Provider-native structured output may be used when available, but local validation remains required.

### Permissions and tool surface

Make built-in research read-only by default. It may use research, websearch/webfetch, repository read/search, skill, question, and bounded task delegation. Mutating tools should be absent rather than `ask` unless a distinct custom agent explicitly requests them and the parent/session ceiling permits them; the built-in specialized research runtime should still refuse mutation during research.

### Events/projections

Publish plan size, active scout count, source/evidence counts, phase, and bounded result summary. Do not publish complete retrieved documents, API credentials, or hidden reasoning.

## 7. Ordered work packages

### A — Contract and permission reconciliation

- define request/plan/source/evidence/claim/report schemas and bounds;
- document quick/direct/multi-source selection;
- reconcile research agent TOML permissions with actual read-only behavior;
- add failing cwd and unstructured-child fixtures.

### B — Explicit service ownership

- make research service construction accept explicit execution/project context;
- remove daemon production use of cwd-backed default service;
- inject search/artifact/cache dependencies through existing factories;
- define failure/timeout behavior.

### C — Bounded decomposition and scouts

- create deterministic/validated plan;
- spawn configured child roles through M003;
- enforce non-overlap, depth, fan-out, source, and budget limits;
- collect structured reports and reconcile partial failure.

### D — Evidence ledger and synthesis

- normalize/deduplicate sources;
- build claim/evidence/conflict records;
- validate citations;
- synthesize typed final report;
- keep large evidence handle-backed.

### E — Events, docs, and operator surface

- publish bounded phase/progress counts;
- document custom research specialists and limits;
- update architecture and prompt guidance;
- expose diagnostics for unavailable providers and evidence gaps.

## 8. Failure, cancellation, restart, and contention semantics

- Quick lookup failure returns a typed source/tool failure rather than fabricated content.
- Multi-source partial failure may still synthesize only when minimum evidence policy is met; failed branches remain visible as gaps.
- Parent cancellation cascades to scouts, tool calls, and synthesis.
- A cancelled/failed research run cannot publish a complete report.
- Duplicate source retrieval is coalesced within one root plan where safe.
- Concurrent research roots use ordinary global tool/model/scheduler limits; no research-specific unbounded pool.
- Restart may interrupt transient work under existing policy; collected artifacts remain governed by existing retention and are not automatically declared a complete report.
- Timeouts are stage-specific and bounded.

## 9. Compatibility

- Existing `research` tool remains callable by generic agents.
- Existing mode/depth inputs remain supported or receive a documented mapping.
- Built-in `research` agent invocation now gains specialized orchestration.
- Direct quick lookups continue to prefer `websearch` without unnecessary child creation.
- Custom agents extending research inherit runtime kind and read-only defaults unless explicitly validly overridden.
- Existing search backends remain optional and failure-transparent.

## 10. Required tests

Focused:

- explicit execution-context service construction;
- daemon path does not use cwd-backed default;
- quick/direct/multi-source classification;
- plan bounds and non-overlap validation;
- source normalization/deduplication;
- claim/evidence/conflict relations;
- unknown/missing citation rejection;
- read-only tool surface;
- child allowlist/depth/budget behavior;
- partial failure/minimum evidence policy;
- cancellation and timeout.

Production-shaped:

- repository architecture question using repository/spec investigator;
- comparative question with two source scouts and one conflict;
- quick lookup with no child spawn;
- duplicate sources returned by different providers deduplicated;
- unavailable web backend with repository-only evidence and explicit limitation.

Negative/security:

- source content cannot escape artifact/output bounds;
- child cannot mutate files or invoke unapproved shell/Git operations;
- prompt injection inside retrieved text remains data and does not grant tools/authority;
- credentials/provider config never enter reports/events;
- fabricated citation fails validation.

## 11. Verification commands

```bash
cargo fmt --all -- --check
cargo test research::
cargo test tool::research
cargo test search_backend::
cargo test --test subagent
cargo check --workspace
```

Add one deterministic research-runtime integration target using captured/local sources. Run one broad local library suite; do not add live-search CI or mandatory API keys.

## 12. Acceptance criteria

- `Research` selects real host-side planning/evidence/synthesis behavior.
- Research services use explicit workspace/project context.
- Built-in research is read-only and uses bounded delegation.
- Child scouts return structured evidence records.
- Sources are normalized/deduplicated and citations validated.
- Conflicts, limitations, and unresolved questions remain explicit.
- Cancellation, timeout, and partial failure are deterministic.
- Events/reports are bounded and do not expose private content.

## 13. Stop conditions

Stop if:

- correct research requires browser automation or a new web-fetch subsystem;
- explicit service construction requires reopening provider/search ownership beyond injection seams;
- persistent global knowledge storage becomes necessary;
- child mutation or deep recursive delegation is required;
- provider-specific structured output changes belong to M007/M008;
- live external service availability is the only possible closure evidence.

## 14. Closure evidence

Include:

- request/plan/evidence/report schemas;
- explicit service construction and no-cwd evidence;
- quick/direct/multi-source fixture results;
- source dedupe/conflict/citation validation evidence;
- child authority/cancellation evidence;
- focused and broad local verification results;
- known source/provider limitations;
- closure recommendation.
