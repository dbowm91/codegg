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
| Core pipeline | `src/research/` (15 files) |
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
`src/research/sources/mod.rs:15`):

| Adapter | File | What it collects |
|---------|------|------------------|
| `LocalRepoSource` | `local_repo.rs` | Local files by keyword search or explicit path |
| `UrlSource` | `url.rs` | Fetched URL content (HTML→text via `html2text`) |
| `CratesIoSource` | `crates_io.rs` | crates.io metadata (name, description, downloads, license) |
| `GitHubSource` | `github.rs` | GitHub repo metadata, file content, issues |
| `DocsRsSource` | `docs_rs.rs` | docs.rs documentation pages |
| `EggsearchSource` | `eggsearch.rs` | External search via `search_backend::dispatch_research_search_structured()` |

**Registered in coordinator** (`coordinator.rs:37-52`):
`LocalRepoSource`, `UrlSource`, `CratesIoSource` (if TLS available),
`GitHubSource` (if TLS available), `DocsRsSource` (if TLS available),
`EggsearchSource`.

`AdvisorySource` (`sources/advisory.rs`) exists but is **not** registered
in the coordinator. It fetches crate version metadata from crates.io to
detect yanked versions.

Network-only adapters return `ResearchError::NetworkNotAllowed` when
`budget.allow_network` is false.

### Eggsearch Integration

`EggsearchSource` (`sources/eggsearch.rs`) is the sole external search
adapter. It:

1. Translates `ResearchMode` to upstream eggsearch workflow values
   (line 28-38).
2. Builds a structured input with query, workflow, depth, and flags
   (line 41-61).
3. Routes security queries through `dispatch_security_search_structured()`
   and all others through `dispatch_research_search_structured()`
   (line 289-293).
4. Parses grouped results from `groups[*].results` in response order
   (line 69-119).
5. Converts each result to a `SourceRecord` with `trust=external_untrusted`
   notes (line 171-253).

The bounded `external_untrusted` string is a display projection, not the
authoritative input for source conversion.

### Evidence Extraction (`extract.rs`)

- **Deterministic chunking**: Local files chunked by 100-line windows with
  10-line overlap (`chunk_local_file()`, line 30). URL text chunked by
  heading breaks or ~2000-char word-boundary windows (`chunk_url_text()`,
  line 80).
- **LLM-backed extraction** (`extract_evidence_with_model()`, line 317):
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

### Contradiction Detection (`coordinator.rs:423-482`)

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
| `ResearchSourceSpec` | 77 | Source specification with `SourceSpecType` and value |
| `SourceRecord` | 95 | Collected source with URI, type, quality, locator, content hash |
| `EvidenceSpan` | 162 | Extracted text span with source reference and locator |
| `ClaimRecord` | 176 | Claim with type, confidence, evidence references, caveats |
| `ContradictionRecord` | 225 | Detected contradiction between claims |
| `ResearchRunStatus` | 245 | Timing, counts, state, error |
| `ResearchPlan` | 282 | Scope, comparison axes, source classes, stopping conditions |
| `ResearchBundle` | 295 | Complete artifact bundle loaded from store |
| `ResearchRunResult` | 317 | Completion result with outputs and artifact dir |

### Claim Types (`ClaimType`, line 189)

`Fact`, `Comparison`, `Recommendation`, `Risk`, `Caveat`, `OpenQuestion`,
`Inference` — serialized as `snake_case`.

### Source Types (`SourceType`, line 111)

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

`call_llm()` (line 19): Sends a single user message (optional system
prompt) to the provider, collects text deltas, returns concatenated text.
120s timeout. Temperature 0.3.

`call_llm_json()` (line 83): Same but strips markdown code fences and
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
