# Research Module

The research module (`src/research/`) implements a structured research pipeline that takes a question, collects evidence from multiple sources, constructs claims, detects contradictions, and produces verified output reports. It is used by the `research` built-in agent for deep investigation tasks.

## Architecture

```
ResearchRequest
      │
      ▼
ResearchCoordinator::run()
      │
      ├──► Phase 0: Create run (store creates dir + run.json)
      ├──► Phase 1: Planning (deterministic plan from request params)
      ├──► Phase 2: Source collection (adapter chain → deduplicate → budget)
      ├──► Phase 3: Evidence extraction (chunk + optional LLM-backed extraction)
      ├──► Phase 4: Claim construction (deterministic fallback or LLM-backed)
      ├──► Phase 5: Contradiction/gap detection (deterministic)
      ├──► Phase 6: Synthesis (render requested output profiles)
      ├──► Phase 7: Verification (structural + optional semantic)
      └──► Finalize (status → Completed, artifact dir written)
```

## Pipeline Phases

| Phase | Status | Description |
|-------|--------|-------------|
| 0 | `Planning` | Create run directory and `request.json` |
| 1 | `Planning` | Generate `ResearchPlan` from request mode and question |
| 2 | `Collecting` | Dispatch to source adapters, deduplicate by URI, apply budget |
| 3 | `Extracting` | Chunk source content into `EvidenceSpan`s; optional LLM summarization |
| 4 | `Claiming` | Build `ClaimRecord`s from evidence; deterministic or model-backed |
| 5 | `Contradicting` | Flag conflicting recommendations and low-confidence claims |
| 6 | `Synthesizing` | Render output profiles (reports, agent answers, evidence bundles) |
| 7 | `Verifying` | Structural citation checks + optional LLM semantic verification |

## Key Types (`src/research/types.rs`)

- **`ResearchRequest`** — Full parameterization: question, mode, audience, depth, output profiles, constraints, sources, budget
- **`ResearchMode`** — `Landscape`, `ArchitectureDecision`, `LibraryEvaluation`, `ApiInvestigation`, `DebuggingInvestigation`, `SecurityReview`, `SpecDigest`, `NarrowAnswer`
- **`ResearchAudience`** — `Human`, `AgentPlanner`, `AgentCoder`, `AgentReviewer`, `AgentDebugger`
- **`ResearchDepth`** — `Low`, `Medium`, `High`
- **`ResearchOutputProfile`** — `HumanFullReport`, `HumanBrief`, `AgentAnswer`, `AgentHandoff`, `EvidenceBundle`
- **`ResearchBudget`** — `max_sources`, `max_chunks_per_source`, `max_evidence_spans`, `max_model_calls`, `max_output_tokens`, `allow_network`
- **`SourceRecord`** — Collected source with URI, type, quality, locator, content hash
- **`EvidenceSpan`** — Extracted text span with source reference and locator
- **`ClaimRecord`** — Claim with type (`Fact`, `Comparison`, `Recommendation`, `Risk`, `Caveat`, `OpenQuestion`, `Inference`), confidence, evidence references, caveats
- **`ContradictionRecord`** — Detected contradiction between claims
- **`ResearchBundle`** — Complete artifact bundle (request + status + plan + sources + evidence + claims + contradictions)

## Source Adapters (`src/research/sources/`)

Each adapter implements `ResearchSourceAdapter` with a `collect()` method:

| Adapter | File | What it collects |
|---------|------|------------------|
| `LocalRepoSource` | `local_repo.rs` | Local file paths from the project root |
| `UrlSource` | `url.rs` | Fetched URL content |
| `CratesIoSource` | `crates_io.rs` | crates.io metadata |
| `GitHubSource` | `github.rs` | GitHub files and issues |
| `DocsRsSource` | `docs_rs.rs` | docs.rs documentation |
| `AdvisorySource` | `advisory.rs` | Security advisory databases |
| `SearchProviderSource` | `search_provider.rs` | External search API results |

Network-only adapters skip gracefully when `allow_network: false`.

## Evidence Extraction (`src/research/extract.rs`)

- **Deterministic chunking**: Local files chunked by 100-line windows with 10-line overlap. URL text chunked by heading breaks or ~2000-char windows.
- **LLM-backed extraction** (optional): When a `Provider` is available, sends chunks to the model for relevance scoring and summary generation.
- Budget-limited: stops at `max_evidence_spans`.

## Claim Construction (`src/research/claims.rs`)

- **Deterministic fallback**: One low-confidence `Inference` claim per evidence span. Used when no model is available or on LLM error.
- **LLM-backed**: Sends evidence briefs to model with `CLAIM_CONSTRUCTION_PROMPT` template, parses structured JSON response into typed `ClaimRecord`s.

## Verification (`src/research/verify.rs`)

### Structural (deterministic)
- Every evidence must reference an existing source
- Every non-OpenQuestion claim's evidence IDs must exist
- Every contradiction must reference existing claims
- Warnings on empty sources/claims and high-severity contradictions

### Semantic (optional, LLM-backed)
- Per-claim batch verification (5 claims per call)
- Returns `supported`, `partially_supported`, `unsupported`, or `unverifiable`
- Unsupported claims cause the run to fail with `ResearchError::VerificationFailed`

## Artifact Store (`src/research/store.rs`)

File-based artifact storage under `<artifact_root>/<run_id>/`:

```
<run_id>/
  request.json          # Original ResearchRequest
  run.json              # ResearchRunStatus (timing, counts, state)
  plan.json             # ResearchPlan
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

Optional SQLite indexing via `upsert_metadata()` / `list_metadata()` for cross-run queries.

## Output Profiles

| Profile | File | Format |
|---------|------|--------|
| `HumanFullReport` | `report.md` | Full markdown report with sources, evidence, claims, contradictions |
| `HumanBrief` | `brief.md` | Condensed claim summary |
| `AgentAnswer` | `agent-answer.md` | Structured answer for agent consumption |
| `AgentHandoff` | `handoff.ctx.md` | Context file for agent-to-agent handoff |
| `EvidenceBundle` | `evidence-bundle.json` | Raw evidence + claims JSON |

## Rerun and Resynthesis

- **`rerun()`**: Re-runs the full pipeline from a previous request and diffs old vs new claims (`ClaimDiff`: added, removed, unchanged)
- **`resynthesize()`**: Re-renders output profiles from an existing run's evidence/claims without re-collecting sources

## Integration

The research module is consumed by the `research` built-in agent (`assets/agents/research.toml`). The agent's tool set includes `research` and `webfetch` tools. The coordinator is constructed with `ResearchCoordinator::new(project_root, artifact_root)` and configured with optional LLM provider via `.with_provider()`.
