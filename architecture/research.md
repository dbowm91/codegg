# Research Module

The research module implements a structured research pipeline: collect
evidence from multiple sources, construct claims, detect contradictions,
and produce verified output reports. It is used by the `research` built-in
agent and exposed as the `research` tool.

## Purpose

Answer open-ended, comparative, or multi-hop questions that go beyond a
single websearch lookup. The pipeline gathers sources, chunks and extracts
evidence, builds a claim graph, checks for contradictions, synthesizes
formatted reports, and optionally verifies citations with an LLM.

## Where It Lives

| Artifact | Path |
|----------|------|
| Core pipeline | `src/research/` (22 `.rs` files: 14 top-level + 8 under `sources/`, of which 7 implement `ResearchSourceAdapter`) |
| Tool surface | `src/tool/research.rs` |
| Service facade | `src/research/service.rs` |
| Specialized runtime | `src/research/runtime.rs` |
| Trigger heuristics | `src/research/triggers.rs` |

### Module Layout

| File | Purpose |
|------|---------|
| `coordinator.rs` | `ResearchCoordinator` — owns the 8-phase pipeline |
| `types.rs` | All domain types: `ResearchRequest`, `SourceRecord`, `EvidenceSpan`, `ClaimRecord`, etc. |
| `store.rs` | `ResearchStore` — file-based artifact storage + optional SQLite index |
| `extract.rs` | Deterministic and LLM-backed evidence extraction |
| `claims.rs` | Deterministic and LLM-backed claim construction |
| `verify.rs` | Structural citation checks + optional semantic verification |
| `synthesis.rs` | Render output profiles (reports, answers, bundles) |
| `llm.rs` | LLM caller helper for model-backed phases |
| `templates.rs` | Prompt templates for evidence extraction, claim construction, verification |
| `service.rs` | `ResearchService` — agent-friendly wrapper around the coordinator |
| `runtime.rs` | Bounded contracts for the specialized multi-agent research runtime |
| `triggers.rs` | Auto-invocation heuristics based on task keywords |
| `error.rs` | `ResearchError` enum |
| `mod.rs` | Module declaration and re-exports |
| `sources/` | Source adapter implementations |

## How It Works

### Pipeline Phases

```
ResearchRequest
      │
      ▼
ResearchCoordinator::run()
      │
      ├── Phase 0: Create run (store writes request.json + run.json)
      ├── Phase 1: Planning (deterministic plan from request params)
      ├── Phase 2: Source collection (adapter chain → deduplicate → budget)
      ├── Phase 3: Evidence extraction (chunk + optional LLM-backed extraction)
      ├── Phase 4: Claim construction (deterministic fallback or LLM-backed)
      ├── Phase 5: Contradiction/gap detection (deterministic)
      ├── Phase 6: Synthesis (render requested output profiles)
      ├── Phase 7: Verification (structural + optional semantic)
      └── Finalize (status → Completed, artifact dir written)
```

Each phase updates `ResearchRunStatus` in `run.json` via
`store.update_run_status()`.

### Source Adapters

Each adapter implements `ResearchSourceAdapter` (defined in
`src/research/sources/mod.rs`):

| Adapter | File | What it collects |
|---------|------|------------------|
| `LocalRepoSource` | `local_repo.rs` | Local files by keyword search or explicit path |
| `UrlSource` | `url.rs` | Fetched URL content (HTML→text via `html2text`) |
| `CratesIoSource` | `crates_io.rs` | crates.io metadata (name, description, downloads, license) |
| `GitHubSource` | `github.rs` | GitHub repo metadata, file content, issues |
| `DocsRsSource` | `docs_rs.rs` | docs.rs documentation pages |
| `EggsearchSource` | `eggsearch.rs` | External search via `search_backend::dispatch_research_search_structured()` |

**Registered in coordinator** (in
`ResearchCoordinator::new()`): `LocalRepoSource`, `UrlSource`,
`CratesIoSource` (if TLS available),
`GitHubSource` (if TLS available), `DocsRsSource` (if TLS available),
`EggsearchSource`.

`AdvisorySource` (`sources/advisory.rs`) exists but is **not** registered
in the coordinator. It fetches crate version metadata from crates.io to
detect yanked versions.

Network-only adapters return `ResearchError::NetworkNotAllowed` when
`budget.allow_network` is false. The coordinator logs that skip at
`debug` level rather than dropping it silently, and warns when a run
that *was* permitted network collection produced zero sources.

### Network Authorization

`ResearchBudget.allow_network` is **not** hardcoded. `ResearchService`
carries an `allow_network` policy (`service.rs`):

- Defaults to `false`. A service built with `new()` /
  `with_artifact_root()` never reaches the network.
- `with_search_runtime(...)` derives the flag from the context's resolved
  `SearchConfig`: `allow_network = backend != Disabled`.
- `with_network(bool)` overrides it for callers that made their own
  authorization decision.

Deriving from `[search].backend` means research obeys the **same**
kill-switch as the `websearch` tool (`src/search_backend/mod.rs`):
setting `[search].backend = "disabled"` disables web search *and*
research collection together. There is no second, research-only network
authority.

This matters because `CratesIoSource`, `GitHubSource` and `DocsRsSource`
issue their own HTTP requests via `reqwest` rather than routing through
`search_backend`; only `EggsearchSource` inherits the backend gate
directly. A hardcoded `true` would therefore have let those three bypass
a disabled search backend.

#### Who attaches a search runtime

Network access is only real for callers that supply a `SearchRuntimeContext`:

| Caller | Network? | Notes |
| --- | --- | --- |
| Agent `research` tool | yes | `turn_runtime.rs` passes `search_runtime` into `SessionToolContext`; `build_research_tool` attaches it. |
| TUI `/research` | yes | `launch_tui` calls `bootstrap_search_runtime` and stores the context on `App` via `set_search_runtime`; the handler attaches it. |
| TUI research **browser** (`/research runs`, `/research open`) | n/a | `start_research_list_runs` / `load_run` / `load_section` only read artifacts from disk. They deliberately do not attach a runtime. |
| Daemon-client (socket) mode | config only | The daemon owns the transport, so the CLI builds a config-only context: `[search].backend` still decides the network policy, but eggsearch has no MCP transport. Non-eggsearch adapters still work. |
| `ResearchService::new()` with no runtime | no | Local-only by design. |

### Model-Backed Phases

Three pipeline phases use an LLM: evidence extraction
(`coordinator.rs` phase 3), claim construction (phase 4), and semantic
verification (phase 7b). Each is gated on
`if let (Some(provider), Some(model))`; with no provider all three fall back
to deterministic implementations.

The provider is supplied by the **turn**, not by research:

- `ToolRegistryOptions::research_provider` / `research_model` carry it.
- `turn_runtime.rs` clones the turn's own provider (the same handle the
  background title task reuses as `title_provider`) and pairs it with the
  turn's model.
- `build_research_tool` attaches it via
  `ResearchService::with_model_backed_phases`, an in-place variant of
  `with_provider` — the constructor form cannot upgrade a service that was
  already built with a workspace root.

Both fields are required. A provider without a model id leaves the
coordinator's guard unsatisfied, so `build_research_tool` ignores it rather
than installing a provider that would silently never be called.

`ResearchTool::with_model_backed_phases` mirrors
`ResearchTool::with_search_runtime`'s `Arc::try_unwrap` rule: on a uniquely
owned service it installs the provider, and on a shared handle it warns
instead of aliasing another owner's service.

### Adapter Collection Shape

Adapters split into two kinds, which determines whether flipping
`allow_network` makes them do anything:

| Adapter | Trigger | Effect when network allowed |
|---------|---------|-----------------------------|
| `LocalRepoSource` | `SourceSpecType::Local` spec | Unaffected — no network gate |
| `UrlSource` | `SourceSpecType::Url` specs only | Returns `Ok([])` when no URL specs |
| `CratesIoSource` | crate name parsed from question | Fetches crates.io metadata |
| `GitHubSource` | GitHub refs parsed from question | Fetches issues/files |
| `DocsRsSource` | `docs.rs` URL specs, else question | Fetches docs pages |
| `EggsearchSource` | question + mode | Dispatches external search |

`ResearchService::build_request` emits only a `Local` spec, so
`UrlSource` stays inert by design; discovery is `EggsearchSource`'s job.

### Retrieved Text for Network Sources

`EggsearchSource` stores the retrieved text on each `SourceRecord` as a
`snippet=` note (alongside provenance notes such as `provider=` and
`trust=external_untrusted`). `SourceRecord::note_value(key)` reads it
back with exact-key matching, and `retrieved_text()` prefers the
snippet and falls back to the title.

`ResearchCoordinator::read_source_contents` uses `retrieved_text()` for
`Url`/`HtmlPage`/`MarkdownPage` sources. These were previously skipped
outright as an "MVP limitation", which meant a run could collect web
sources successfully and still emit **zero** evidence and an empty
answer. Text already captured during collection is reused rather than
re-fetched.

### Eggsearch Integration

`EggsearchSource` (`sources/eggsearch.rs`) is the sole external search
adapter. It:

1. Translates `ResearchMode` to upstream eggsearch workflow values
   (`upstream_workflow()`).
2. Builds a structured input with query, workflow, depth, and flags
   (`request_input()`).
3. Routes security queries through `dispatch_security_search_structured()`
   and all others through `dispatch_research_search_structured()`.
4. Parses grouped results from `groups[*].results` in response order
   from grouped response values in order.
5. Converts each result to a `SourceRecord` with `trust=external_untrusted`
   notes, nested publication dates, upstream stable-ID association, and a
   separate `ResearchRetrievalReceipt` (see `RetrievalState` in
   `src/research/types.rs`).

The receipt is structured coordinator data, not status inferred from source
notes. It distinguishes complete, partial, failed, skipped, truncated, and
unknown outcomes; carries bounded warning codes, failed-provider labels,
coverage text, and links from each existing per-run source UUID to eggsearch's
stable ID. Historical v0.4.1 compatibility fixtures demonstrate diagnostic
responses where `retrieval_summary` contains per-role/provider/subquery
`dimensions`; `dimensions[*].state` and
`attempt_outcome` are the status evidence. `has_failures`, job/dimension/role
counts, truncation evidence, and `workflow_coverage` qualify that evidence.
`completed_no_match` is a positive completed outcome for its represented role;
`not_applicable`, skipped, unavailable, partial, failed, interrupted, unknown
states, incomplete role counts, and uncertain result limits cannot prove
absence. These v0.4.1 fixtures are compatibility evidence; the current live
wrapper contract is qualified against Eggsearch v0.4.3 in
`plans/closure/search-eggsearch-pair-compatibility-corrective/002-status.md`.
The compact `retrieval_status` projection omits dimensions, so its
aggregate booleans alone never establish complete retrieval. Unknown older
responses remain unknown unless they carry positively recognized legacy
completion evidence. A complete response with zero sources is a scoped
confirmed no-match; failed or incomplete collection is rendered in research
outputs as a coverage gap. Upstream trust/provenance is
retained as descriptive evidence only; CodeGG continues to mark network source
content `external_untrusted`. Receipts are persisted in the additive
`retrieval.json` run artifact and loaded into `ResearchBundle`; older runs
without the artifact load with an empty receipt list.

The bounded `external_untrusted` string is a display projection, not the
authoritative input for source conversion.

### Evidence Extraction (`extract.rs`)

- **Deterministic chunking**: Local files chunked by 100-line windows with
  10-line overlap (`chunk_local_file()`, line 31). URL text chunked by
  heading breaks or ~2000-char word-boundary windows (`chunk_url_text()`,
  line 81).
- **LLM-backed extraction** (`extract_evidence_with_model()`, line 326):
  When a `Provider` is available, calls the LLM with
  `EVIDENCE_EXTRACTION_PROMPT` for each chunk. Falls back to deterministic
  extraction on error.
- Budget-limited: stops at `budget.max_evidence_spans`.

### Claim Construction (`claims.rs`)

- **Deterministic fallback** (`deterministic_claims()`, line 8): One
  low-confidence `Inference` claim per evidence span. Always used when no
  model is available.
- **LLM-backed** (`build_claims_with_model()`, line 70): Sends evidence
  briefs to the model with `CLAIM_CONSTRUCTION_PROMPT`, parses structured
  JSON into `ClaimRecord`s. Falls back to deterministic on any error.

### Contradiction Detection (`ResearchCoordinator::check_contradictions`)

Deterministic pass that:
1. Groups claims by `applies_to` target.
2. Flags conflicting `Recommendation` claims on the same target.
3. Flags low-confidence `Fact`, `Comparison`, or `Recommendation` claims.

### Verification (`verify.rs`)

**Structural** (`verify_structural()`, line 25):
- Every evidence must reference an existing source.
- Every non-OpenQuestion claim's evidence IDs must exist.
- Every contradiction must reference existing claims.
- Warnings on empty sources/claims and high-severity contradictions.

**Semantic** (`verify_semantic()`, line 114, optional, LLM-backed):
- Per-claim batch verification (5 claims per call).
- Returns `supported`, `partially_supported`, `unsupported`, or
  `unverifiable`.
- Unsupported claims cause the run to fail with
  `ResearchError::VerificationFailed`.

### Run Status Transitions

The run enters `Verifying` at the **start** of phase 7, not on the
failure path. Every error return from `ResearchCoordinator::run`
routes through `fail_run()`, which persists `RunStatus::Failed`, the
error message, and `finished_at`.

Previously the semantic-failure path set `Verifying` and then returned
the error without ever transitioning to `Failed`, so a failed run was
indistinguishable from an in-flight one in `list_runs()`.

### Synthesis (`synthesis.rs`)

Renders output profiles from the claim graph:

| Profile | File | Format |
|---------|------|--------|
| `HumanFullReport` | `report.md` | Full markdown with sources, evidence, claims, contradictions, bibliography |
| `HumanBrief` | `brief.md` | Condensed recommendation + caveats |
| `AgentAnswer` | `agent-answer.md` | Structured answer with rationale, validation tasks, evidence pointers |
| `AgentHandoff` | `handoff.ctx.md` | Context package for agent-to-agent handoff |
| `EvidenceBundle` | `evidence-bundle.json` | Raw JSON of sources + evidence + claims + contradictions |

## Key Types

All defined in `src/research/types.rs`:

| Type | Line | Purpose |
|------|------|---------|
| `ResearchRequest` | 9 | Full parameterization: question, mode, audience, depth, output profiles, constraints, sources, budget |
| `ResearchMode` | 25 | `Landscape`, `ArchitectureDecision`, `LibraryEvaluation`, `ApiInvestigation`, `DebuggingInvestigation`, `SecurityReview`, `SpecDigest`, `NarrowAnswer` |
| `ResearchAudience` | 38 | `Human`, `AgentPlanner`, `AgentCoder`, `AgentReviewer`, `AgentDebugger` |
| `ResearchDepth` | 48 | `Low`, `Medium`, `High` |
| `ResearchOutputProfile` | 56 | `HumanFullReport`, `HumanBrief`, `AgentAnswer`, `AgentHandoff`, `EvidenceBundle` |
| `ResearchBudget` | 66 | `max_sources`, `max_chunks_per_source`, `max_evidence_spans`, `max_model_calls`, `max_output_tokens`, `allow_network` |

`max_model_calls` is currently **advisory only** — no phase meters model
calls against it, so it bounds nothing today. It is still recorded per
run so the persisted request states the intended ceiling rather than
implying "no model use".
| `ResearchSourceSpec` | 77 | Source specification with `SourceSpecType` and value |
| `SourceRecord` | 95 | Collected source with URI, type, quality, locator, content hash |
| `EvidenceSpan` | 230 | Extracted text span with source reference and locator |
| `ClaimRecord` | 244 | Claim with type, confidence, evidence references, caveats |
| `ContradictionRecord` | 305 | Detected contradiction between claims |
| `ResearchRunStatus` | 325 | Timing, counts, state, error |
| `ResearchPlan` | 362 | Scope, comparison axes, source classes, stopping conditions |
| `ResearchBundle` | 375 | Complete artifact bundle loaded from store |
| `ResearchRunResult` | 399 | Completion result with outputs and artifact dir |

### Claim Types (`ClaimType` in `src/research/types.rs`)

`Fact`, `Comparison`, `Recommendation`, `Risk`, `Caveat`, `OpenQuestion`,
`Inference` — serialized as `snake_case`.

### Source Types (`SourceType` in `src/research/types.rs`)

`LocalFile`, `LocalSearchResult`, `Url`, `HtmlPage`, `MarkdownPage`, `Pdf`,
`GitHubFile`, `GitHubIssue`, `CratesIoMetadata`, `ManualText`.

## Specialized Research Runtime (`runtime.rs`)

The runtime module defines bounded contracts for the multi-agent research
pattern where a parent agent delegates evidence collection to child
agents:

| Constant | Value | Purpose |
|----------|-------|---------|
| `MAX_CHILD_TASKS` | 3 | Maximum concurrent child tasks |
| `MAX_SOURCES` | 32 | Maximum sources per report |
| `MAX_EVIDENCE` | 96 | Maximum evidence records |
| `MAX_CLAIMS` | 48 | Maximum claims |
| `MAX_TEXT_CHARS` | 2,000 | Maximum text length per evidence/claim |

Key types: `BoundedResearchPlan`, `ResearchTask`, `ChildRole`
(`SourceScout`, `RepositoryInvestigator`, `ClaimVerifier`),
`ResearchEvidenceReport`, `ResearchReport`.

`classify()` (line 108) determines `RequestKind` from the question:
`QuickLookup` (short, narrow), `DirectInvestigation` (repo-focused),
or `MultiSource` (comparative/broad).

`build_plan()` (line 129) creates 0, 1, or 3 child tasks based on kind.
`validate_report()` (line 199) enforces citation integrity and bounds
before the parent accepts a child's output.

### Child fan-out (`specialized_runtime.rs`)

`coordinate_research()` dispatches the planned children to the sub-agent pool
through the module's `fan_out_children()` helper, which runs them concurrently and
collects results in plan order. Children are independent read-only gathers over
disjoint scopes, so coordination cost is the *slowest* child rather than the sum
of all of them. Each child keeps its own `MAX_CHILD_TIMEOUT` (120s) budget and
yields exactly one `ResearchEvidenceReport`, so a hung or failing child degrades
to a failed report instead of stalling its siblings or shortening the ledger.

This matters because `coordinate_research()` is awaited inline while the turn is
being prepared — before the agent loop spawns. Running children sequentially made
a research turn able to sit silent for up to `MAX_CHILD_TASKS * MAX_CHILD_TIMEOUT`
(6 minutes) with no agent output at all, which presented to the user as "research
is broken" rather than "research is working". Concurrent fan-out bounds the same
worst case at one `MAX_CHILD_TIMEOUT`, and the worker publishes
`SubagentStarted`/`SubagentProgress` per child, so the TUI shows all children
running at once.

Determinism is preserved: results are returned in plan order, so ledger
aggregation through `aggregate_research()` is identical to the previous
sequential behavior.

## Trigger Heuristics (`triggers.rs`)

`analyze_trigger()` (line 92) examines task descriptions for keywords to
suggest auto-invoking research:

| Keyword Family | Suggested Mode | Base Confidence |
|---------------|----------------|-----------------|
| Comparison ("compare", "vs", "which is better") | `LibraryEvaluation` | 0.8 |
| API ("api", "protocol", "endpoint") | `ApiInvestigation` | 0.6–0.7 |
| Security ("security", "vulnerability", "cve") | `SecurityReview` | 0.7 |
| Architecture ("architecture", "design", "refactor") | `ArchitectureDecision` | 0.5 |

Confidence is boosted by conceptual failure patterns (+0.2) and large file
surfaces (+0.1). The `TriggerConfig` defaults are in `triggers.rs:14`.

## Artifact Store (`store.rs`)

File-based storage under `<artifact_root>/<run_id>/`:

```
<run_id>/
  request.json          # ResearchRequest (pretty JSON)
  run.json              # ResearchRunStatus (pretty JSON)
  plan.json             # ResearchPlan (pretty JSON)
  sources.jsonl         # SourceRecord lines
  evidence.jsonl        # EvidenceSpan lines
  claims.jsonl          # ClaimRecord lines
  contradictions.jsonl  # ContradictionRecord lines
  report.md             # HumanFullReport output
  brief.md              # HumanBrief output
  agent-answer.md       # AgentAnswer output
  handoff.ctx.md        # AgentHandoff output
  evidence-bundle.json  # EvidenceBundle output
```

Optional SQLite indexing via `upsert_metadata()` / `list_metadata()` /
`load_metadata()` / `delete_metadata()` for cross-run queries. The
`research_run` table stores run_id, question, mode, depth, status, timing,
counts, and project_root.

`ResearchStore` methods: `create_run`, `update_run_status`, `append_source`,
`append_evidence`, `append_claim`, `append_contradiction`, `write_plan`,
`write_report`, `load_run_bundle`, `list_runs`, `load_run_status`,
`overwrite_sources`, `overwrite_evidence`, `overwrite_claims`,
`overwrite_contradictions`.

## Service Layer (`service.rs`)

`ResearchService` is the agent-friendly wrapper:

| Method | Purpose |
|--------|---------|
| `run()` | Full pipeline execution |
| `answer_for_agent()` | Pipeline + extract AgentAnswer text |
| `create_report()` | Pipeline + return HumanFullReport path |
| `list_runs()` | List recent runs with summaries |
| `load_run()` | Load a complete `ResearchBundle` |
| `rerun()` | Re-run from original request + diff claims |
| `resynthesize()` | Re-render profiles from existing claims |
| `list_metadata()` / `load_metadata()` | SQLite index queries |

Default artifact root: `<project_root>/.codegg/research/`.

## LLM Integration (`llm.rs`)

`call_llm()` (line 21): Sends a single user message (optional system
prompt) to the provider, collects text deltas, returns concatenated text.
120s timeout. Temperature 0.3.

`call_llm_json()` (line 87): Same but strips markdown code fences and
parses JSON response.

Used by `extract_evidence_with_model`, `build_claims_with_model`, and
`verify_semantic`.

All model-backed phases in one research run receive the same
`ProviderRequestContext`, projected from the run ID. Valid bounded run IDs
are used directly; malformed or oversized IDs receive a deterministic,
bounded projection created once for the run. The context is transport-only
and is not included in research prompts or persisted as a second identity.

## Testing

```bash
cargo test -p codegg --lib research           # all research unit tests
cargo test -p codegg --lib research::store    # store tests
cargo test -p codegg --lib research::claims   # claim construction tests
cargo test -p codegg --lib research::verify   # verification tests
cargo test -p codegg --lib research::extract  # extraction/chunking tests
cargo test -p codegg --lib research::runtime  # runtime validation tests
cargo test -p codegg --lib research::triggers # trigger heuristic tests
```

## Related Docs

- [agent.md](agent.md) — Built-in `research` agent definition
- [tool.md](tool.md) — Tool registry, research tool registration
- [provider.md](provider.md) — LLM provider interface used for model-backed phases

## Source Verification

Verified 2026-10-06 against source. Corrected: "14 files + sources/ subdir
with 8 adapters" → 22 `.rs` files (14 top-level + 8 under `sources/`, of which
7 implement `ResearchSourceAdapter`; `AdvisorySource` is the 7th and is not
registered). Line refs in the `types.rs` table: `ContradictionRecord` 225 → 237,
`ResearchRunStatus` 245 → 257, `ResearchPlan` 282 → 294, `ResearchBundle`
295 → 307, `ResearchRunResult` 317 → 329; `chunk_local_file` line 30 → 31,
`chunk_url_text` line 80 → 81, `extract_evidence_with_model` 317 → 326,
`call_llm` 19 → 21, `call_llm_json` 83 → 87; contradiction detection is
`ResearchCoordinator::check_contradictions` at `coordinator.rs:443` (not
`:423-482`); coordinator adapter registration `coordinator.rs:36-53`; added the
missing `mod.rs` row to the module-layout table. Verified accurate:
`ResearchSourceAdapter` (`sources/mod.rs:15`), `deterministic_claims`
(`claims.rs:8`), `build_claims_with_model` (`claims.rs:70`),
`verify_structural` (`verify.rs:25`), `verify_semantic` (`verify.rs:114`),
`classify`/`build_plan`/`validate_report` (`runtime.rs:108/129/199`),
`analyze_trigger` (`triggers.rs:92`), the five runtime bounds (3/32/96/48/2000),
all enum variant lists (`ResearchMode` 8, `ResearchAudience` 5, `ResearchDepth`
3, `ResearchOutputProfile` 5, `ClaimType` 7, `SourceType` 10), `ResearchBudget`
6 fields, and the five output-profile artifact filenames (`store.rs:117-121`).

Verified 2026-10-13 against `eggstack/eggsearch` immutable tag `v0.4.1`
(`32769d684417567dfb30fc5adea1475cee7f0ed0`), its
`src/core/retrieval_status.rs`, `src/mcp/tools/research_search.rs`,
`src/mcp/tools/security_search.rs`, and `src/mcp/projection.rs`: diagnostic
research/security tool responses expose `retrieval_summary.dimensions` with
per-dimension state/outcome/truncation evidence plus summary counts and
`workflow_coverage`; compact projection replaces that summary with aggregate
`retrieval_status` and removes workflow coverage. The CodeGG adapter now
reduces those dimensions conservatively and keeps missing/additive states
indeterminate. Offline schema fixtures live under
`tests/fixtures/eggsearch/v0.4.1/`. No live v0.4.1 binary smoke was run. The
previous M001 paragraph's unqualified claim that any complete zero-source
response proves no-match was too broad; completion must now be established
from role-aware tagged retrieval evidence.
