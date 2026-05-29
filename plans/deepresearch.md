# Codegg Deep Research Agent Implementation Plan

Status: handoff plan for implementation by a smaller coding model.
Target repo: `dbowm91/codegg`.
Primary goal: add a durable, source-grounded research subsystem that can produce both detailed human-facing reports and compact agent-facing handoffs.

## 0. Context and constraints

This plan is based on the current visible codegg project shape: a Rust 2021 crate named `codegg`, described as a lightweight pure-Rust AI coding agent; provider support for Anthropic/OpenAI/Google/Azure/Bedrock/etc.; TUI support with slash commands; server mode; session persistence through SQLite/`sqlx`; LSP support; MCP status; skills; memory; context compaction; and an existing main-agent/subagent architecture. The manifest already includes useful dependencies for this work: `tokio`, `serde`, `serde_json`, `reqwest`, `html2text`, `comrak`, `ignore`, `walkdir`, `grep`, `regex`, `tiktoken`, `sqlx`, `chrono`, `uuid`, `ulid`, and optional server/TUI features.

The important architecture correction is this: do not implement deep research as a normal subagent only. Implement it as a service-like subsystem owned by the runtime/session layer. The main agent, planner agent, reviewer agent, and TUI should call into it through a typed request API. Research artifacts must live outside the main chat context and be passed back as compact references or summaries.

The first implementation should be deliberately bounded. Do not build a fully autonomous browser clone. Implement a reproducible research pipeline with local repo sources, explicit URL sources, durable artifacts, structured evidence, claim records, report rendering, and agent handoff rendering. Add external search-provider adapters only after the core run/store/output model is stable.

## 1. Product shape

The subsystem should support one procurement pipeline and multiple consumption contracts.

The shared procurement pipeline is:

```text
ResearchRequest
→ ResearchPlan
→ source collection
→ source ranking / filtering
→ evidence extraction
→ claim construction
→ contradiction / gap pass
→ synthesis
→ citation / support verification
→ output rendering
```

The output profiles are distinct:

```text
HumanFullReport
  Long-form report with detailed context, comparison matrices, source summaries,
  caveats, open questions, and recommendations.

HumanBrief
  Short decision memo, still cited and caveated.

AgentAnswer
  Direct answer to a narrow agent question, optimized for low token load.

AgentHandoff
  Dense operational context package for planner/coder/reviewer agents.

EvidenceBundle
  Structured sources, evidence spans, claims, contradiction notes, and confidence.
```

Human use expands context. Agentic use compresses context and preserves pointers. A human asking “compare Axum, Actix Web, Rocket, Poem, Warp, Salvo, and Loco” should receive the full decision surface. A planner agent asking “Axum or Actix for this codegg adapter?” should receive a clear recommendation with source/evidence handles, caveats, and validation tasks.

## 2. Non-goals for the first pass

Do not add a vector database in the first pass. Do not add full web-search autonomy in the first pass. Do not make the main agent paste entire source documents into its own context. Do not make the final Markdown report the canonical store. The canonical store should be structured JSON/JSONL records plus source snapshots or source references.

Do not require server mode. The MVP should work from CLI and later integrate into the TUI. Do not require a new database migration unless the existing DB layer is easy to extend. A filesystem-backed research run directory is sufficient for the first iteration.

## 3. Files and modules to inspect before coding

Before making changes, inspect these areas and adapt path names to the actual repo layout:

```text
Cargo.toml
src/main.rs
src/config* or src/config/**
src/agent* or src/agent/**
src/session* or src/session/**
src/tui/**
src/tools/**
src/db/** or src/storage/**
src/providers/** or src/llm/**
src/mcp/**
docs/ARCHITECTURE.md
architecture/agent.md
architecture/tui.md
AGENTS.md
```

Record the actual module names in the implementation PR notes. If there is already an artifact/session store, use it. If not, create a new research artifact store with minimal coupling.

## 4. New module layout

Create a new top-level module, adjusting names to match repo conventions:

```text
src/research/mod.rs
src/research/types.rs
src/research/store.rs
src/research/artifacts.rs
src/research/coordinator.rs
src/research/sources/mod.rs
src/research/sources/local_repo.rs
src/research/sources/url.rs
src/research/extract.rs
src/research/claims.rs
src/research/synthesis.rs
src/research/verify.rs
src/research/templates.rs
src/research/error.rs
```

If the repo already has a service/runtime abstraction, register the research subsystem there. If it does not, expose a simple `ResearchCoordinator` that can be constructed from config, project root, optional model client, and artifact root.

## 5. Core data model

Implement serializable types first. Keep them stable, explicit, and easy for smaller agents/tools to inspect.

```rust
pub struct ResearchRequest {
    pub id: ResearchRunId,
    pub question: String,
    pub mode: ResearchMode,
    pub audience: ResearchAudience,
    pub depth: ResearchDepth,
    pub output_profiles: Vec<ResearchOutputProfile>,
    pub constraints: Vec<String>,
    pub sources: Vec<ResearchSourceSpec>,
    pub existing_context_refs: Vec<String>,
    pub budget: ResearchBudget,
    pub created_at: DateTime<Utc>,
}

pub enum ResearchMode {
    Landscape,
    ArchitectureDecision,
    LibraryEvaluation,
    ApiInvestigation,
    DebuggingInvestigation,
    SecurityReview,
    SpecDigest,
    NarrowAnswer,
}

pub enum ResearchAudience {
    Human,
    AgentPlanner,
    AgentCoder,
    AgentReviewer,
    AgentDebugger,
}

pub enum ResearchDepth {
    Low,
    Medium,
    High,
}

pub enum ResearchOutputProfile {
    HumanFullReport,
    HumanBrief,
    AgentAnswer,
    AgentHandoff,
    EvidenceBundle,
}

pub struct ResearchBudget {
    pub max_sources: usize,
    pub max_chunks_per_source: usize,
    pub max_evidence_spans: usize,
    pub max_model_calls: usize,
    pub max_output_tokens: Option<usize>,
    pub allow_network: bool,
}
```

Use `ulid` or `uuid` consistently with the rest of codegg. Prefer ULIDs for sortable run IDs if the project already uses them.

Source and evidence records:

```rust
pub struct SourceRecord {
    pub id: SourceId,
    pub run_id: ResearchRunId,
    pub uri: String,
    pub title: Option<String>,
    pub source_type: SourceType,
    pub source_quality: SourceQuality,
    pub retrieved_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
    pub content_hash: Option<String>,
    pub locator: SourceLocator,
    pub notes: Vec<String>,
}

pub enum SourceType {
    LocalFile,
    LocalSearchResult,
    Url,
    HtmlPage,
    MarkdownPage,
    Pdf,
    GitHubFile,
    GitHubIssue,
    CratesIoMetadata,
    ManualText,
}

pub enum SourceQuality {
    Primary,
    OfficialDocs,
    SourceCode,
    MaintainerComment,
    ReleaseNotes,
    StandardOrSpec,
    Academic,
    Secondary,
    Unknown,
    LowQuality,
}

pub struct EvidenceSpan {
    pub id: EvidenceId,
    pub run_id: ResearchRunId,
    pub source_id: SourceId,
    pub locator: SourceLocator,
    pub text: String,
    pub summary: Option<String>,
    pub extracted_at: DateTime<Utc>,
}

pub struct ClaimRecord {
    pub id: ClaimId,
    pub run_id: ResearchRunId,
    pub text: String,
    pub claim_type: ClaimType,
    pub confidence: Confidence,
    pub evidence_ids: Vec<EvidenceId>,
    pub caveats: Vec<String>,
    pub applies_to: Vec<String>,
}

pub enum ClaimType {
    Fact,
    Comparison,
    Recommendation,
    Risk,
    Caveat,
    OpenQuestion,
    Inference,
}

pub enum Confidence {
    Low,
    Medium,
    High,
}
```

`SourceLocator` should support at least:

```rust
pub enum SourceLocator {
    FileRange { path: PathBuf, start_line: usize, end_line: usize },
    Url { url: String, heading: Option<String> },
    TextSpan { label: String },
}
```

Add PDF page support later if/when PDF extraction is implemented.

## 6. Research run artifact layout

Create a per-run artifact directory. Default root should be project-local unless config overrides it:

```text
.codegg/research/<run_id>/
  request.json
  run.json
  plan.md
  sources.jsonl
  evidence.jsonl
  claims.jsonl
  contradictions.jsonl
  report.md
  brief.md
  agent-answer.md
  handoff.ctx.md
  bibliography.json
  logs.jsonl
```

`run.json` tracks status:

```rust
pub struct ResearchRunStatus {
    pub run_id: ResearchRunId,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub artifact_dir: PathBuf,
    pub error: Option<String>,
    pub counts: ResearchRunCounts,
}
```

Implement append/read helpers for JSONL. Keep this simple and robust:

```rust
pub trait ResearchStore {
    async fn create_run(&self, request: &ResearchRequest) -> Result<ResearchRunStatus>;
    async fn append_source(&self, source: &SourceRecord) -> Result<()>;
    async fn append_evidence(&self, evidence: &EvidenceSpan) -> Result<()>;
    async fn append_claim(&self, claim: &ClaimRecord) -> Result<()>;
    async fn write_report(&self, run_id: &ResearchRunId, profile: ResearchOutputProfile, text: &str) -> Result<PathBuf>;
    async fn load_run_bundle(&self, run_id: &ResearchRunId) -> Result<ResearchBundle>;
}
```

Do not block on SQLite. Once the JSONL artifact store works, optionally mirror run metadata into the existing SQLite session database.

## 7. Config additions

Add research configuration in the existing config system. Exact file format should match existing codegg config style.

Suggested config shape:

```toml
[research]
enabled = true
artifact_dir = ".codegg/research"
default_depth = "medium"
default_audience = "human"
allow_network = false
max_sources_low = 8
max_sources_medium = 30
max_sources_high = 80
max_chunks_per_source = 20
max_evidence_spans = 200
require_citations = true

[research.source_quality]
prefer_primary_sources = true
deprefer_low_quality_sources = true
```

Default `allow_network` should be conservative if codegg has an existing permission model. If there is no permission gate yet, require an explicit CLI flag such as `--allow-network` for URL fetching.

## 8. CLI command

Add a CLI command before the TUI integration. This creates a debuggable path for the smaller model.

Target UX:

```bash
codegg research "Compare Axum and Actix for codegg's server adapter" \
  --mode architecture-decision \
  --audience human \
  --depth medium \
  --output human-full \
  --output agent-handoff \
  --source local \
  --source url:https://docs.rs/axum/latest/axum/ \
  --source url:https://actix.rs/docs/
```

Minimum implementation:

```text
codegg research <QUESTION>
  --mode <landscape|architecture-decision|library-evaluation|api-investigation|debugging-investigation|security-review|spec-digest|narrow-answer>
  --audience <human|agent-planner|agent-coder|agent-reviewer|agent-debugger>
  --depth <low|medium|high>
  --output <human-full|human-brief|agent-answer|agent-handoff|evidence-bundle>
  --source <local|url:...|file:...|text:...>
  --allow-network
```

If `clap` enum parsing is already used elsewhere, follow existing style. Print final artifact paths at the end.

## 9. TUI slash command

After the CLI path works, add `/research` to the TUI slash-command system.

Suggested commands:

```text
/research <question>
/research --mode architecture-decision --depth medium <question>
/research-runs
/research-open <run_id>
/research-handoff <run_id>
```

MVP TUI behavior can be minimal: run research, show progress events, then show artifact paths. A richer TUI research browser can come later.

## 10. ResearchCoordinator pipeline

Implement a coordinator with explicit stages. Each stage should write artifacts before moving to the next stage.

```rust
pub struct ResearchCoordinator<S, M> {
    store: S,
    model: Option<M>,
    source_adapters: Vec<Box<dyn ResearchSourceAdapter>>,
    config: ResearchConfig,
}

impl<S, M> ResearchCoordinator<S, M> {
    pub async fn run(&self, request: ResearchRequest) -> Result<ResearchRunResult> {
        self.store.create_run(&request).await?;
        let plan = self.plan(&request).await?;
        let sources = self.collect_sources(&request, &plan).await?;
        let evidence = self.extract_evidence(&request, &sources).await?;
        let claims = self.build_claims(&request, &evidence).await?;
        let contradictions = self.check_contradictions(&request, &claims).await?;
        let outputs = self.synthesize_outputs(&request, &claims, &contradictions).await?;
        self.verify_outputs(&request, &outputs, &claims).await?;
        Ok(...)
    }
}
```

Make the first version tolerate `model: None` by generating a simple deterministic report from collected sources/evidence. That lets tests run without API keys.

## 11. Source adapters

Define a trait:

```rust
#[async_trait]
pub trait ResearchSourceAdapter: Send + Sync {
    async fn collect(&self, request: &ResearchRequest, plan: &ResearchPlan) -> Result<Vec<CollectedSource>>;
    fn name(&self) -> &'static str;
}
```

Implement three adapters first.

### 11.1 LocalRepoSource

Use existing filesystem/search dependencies (`ignore`, `walkdir`, `grep`, `regex`) to collect relevant local files. Inputs can include explicit file paths or a local source mode.

MVP behavior:

- If user supplies `--source file:path`, read that file.
- If user supplies `--source local`, perform bounded text search using keywords from the question and plan.
- Respect `.gitignore`/ignore rules if existing project search tools already do this.
- Exclude `target/`, `.git/`, large binaries, lockfiles unless explicitly requested.
- Store file path and line ranges in `SourceLocator::FileRange`.

### 11.2 UrlSource

Use `reqwest` and `html2text`.

MVP behavior:

- Only fetch when `allow_network` is true.
- Fetch explicit `url:` sources.
- Apply timeout from config.
- Reject very large responses or truncate with a clear note.
- Convert HTML to text/Markdown.
- Hash content with `sha2` or existing hash utility.
- Store retrieved timestamp and URL locator.

Do not implement general web search in this adapter. It fetches explicit URLs.

### 11.3 ManualTextSource

Accept text snippets from future TUI/session integrations. For CLI, this can be deferred unless easy.

## 12. Optional future adapters

Do not implement these in the MVP unless the core pipeline is already complete:

```text
CratesIoSource
  Fetch crate metadata, version cadence, license, repository URL, downloads.

GitHubSource
  Fetch repo metadata, files, issues, releases, PRs, maintainer activity.

DocsRsSource
  Fetch docs.rs pages for crate APIs.

AdvisorySource
  Fetch RustSec/advisory metadata for dependency risk.

SearchProviderSource
  Adapter for Tavily/Brave/SerpAPI/Kagi/etc., behind config.
```

These adapters are valuable for Rust dependency scrutiny but should not block the first implementation.

## 13. Evidence extraction

Build deterministic chunking first.

For local files:

- Chunk by function/module if existing code-intelligence utilities can do that cheaply.
- Otherwise chunk by line windows, e.g. 80-160 lines with overlap.
- Preserve path and line range.

For URL text:

- Chunk by headings if possible.
- Otherwise chunk by token/character count.
- Preserve URL and approximate heading.

Then add model-assisted extraction if a model client is available. The extraction prompt should return JSON records, not prose.

Extraction prompt template:

```text
You are extracting evidence for an engineering research task.
Question: {question}
Mode: {mode}
Source: {source_title_or_uri}

Return JSON array only. Each item must include:
- text: exact or close paraphrase of the evidence span
- summary: one sentence explaining relevance
- relevance: low|medium|high
- caveats: array of caveats

Do not invent facts. If the source does not contain relevant evidence, return [].
```

Validate model JSON. If parsing fails, store a warning and continue with deterministic evidence.

## 14. Claim construction

Claims are the bridge between raw evidence and reports. Build claims from evidence records and preserve evidence IDs.

Claim builder prompt:

```text
You are building a claim graph for an engineering research task.
Question: {question}
Mode: {mode}
Audience: {audience}

Given evidence records with IDs, produce JSON claims.
Each claim must include:
- text
- claim_type: fact|comparison|recommendation|risk|caveat|open_question|inference
- confidence: low|medium|high
- evidence_ids: array of evidence IDs supporting the claim
- caveats: array
- applies_to: array of technologies/files/components

Rules:
- Every factual or comparative claim must cite at least one evidence_id.
- Recommendations may cite evidence and may include explicit inference caveats.
- Do not cite evidence that does not support the claim.
- Mark uncertain judgments as inference or open_question.
Return JSON only.
```

MVP deterministic fallback: create one low-confidence claim per evidence span saying “Source contains potentially relevant information about X.” This is not ideal, but it keeps the artifact model testable.

## 15. Contradiction and gap pass

Implement a simple pass first:

- Group claims by `applies_to` and `claim_type`.
- Flag conflicting recommendations.
- Flag claims with low confidence but high importance.
- Flag missing evidence for requested comparison axes.

If model-backed:

```text
Given these claims, identify contradictions, stale-source risks, missing comparison axes, and questions that would change the recommendation. Return JSON only.
```

Store results in `contradictions.jsonl`.

## 16. Synthesis outputs

Implement template-based rendering. Do not rely on free-form model prose only.

### 16.1 HumanFullReport template

```markdown
# Research Report: {question}

## Scope

## Executive conclusion

## Method

## Sources reviewed

## Decision matrix / comparison axes

## Findings

Each finding should include claim IDs and evidence/source pointers.

## Risks and caveats

## Open questions

## Recommended validation work

## Bibliography / source list
```

For library/framework comparisons, include axes such as:

```text
maintenance health
release cadence
bus factor signals
API ergonomics
ecosystem gravity
runtime compatibility
middleware/composability
observability/testing fit
performance model
transitive dependency risk
license/governance
fit for agent-generated code
migration/lock-in risk
```

### 16.2 AgentAnswer template

```markdown
# Agent Research Answer

Question: {question}

Answer: {direct answer}

Recommendation: {one-sentence recommendation}

Confidence: {low|medium|high}

Rationale:
- {claim_id}: {short claim text}

Caveats:
- ...

Do not assume:
- ...

Validation tasks:
- ...

Evidence pointers:
- {claim_id} -> {evidence_id/source_id/locator}
```

### 16.3 AgentHandoff template

```markdown
# Research Handoff Context

Purpose: provide compact context for a planner/coder/reviewer agent without loading raw research.

Decision / framing:
...

Operational guidance:
...

Constraints:
...

Relevant claims:
- C001: ... [evidence: E001, E004]

Caveats:
...

Suggested next actions:
...

Artifact refs:
- report: ...
- claims: ...
- sources: ...
```

The handoff must be concise. Default target: 500-2000 tokens depending on depth and audience.

## 17. Citation/support verifier

Implement a verifier that checks structural support, even before semantic support is perfect.

MVP verifier rules:

- Every finding in the report must reference at least one claim ID.
- Every claim used in report findings must have at least one evidence ID unless `claim_type` is `OpenQuestion`.
- Every evidence ID must exist in `evidence.jsonl`.
- Every evidence record must point to an existing source ID.
- AgentAnswer and AgentHandoff must include artifact refs.

If verification fails, write `verification_failed.md` and return an error unless config allows warnings.

Later semantic verifier:

- Ask a model to check whether each claim is actually supported by cited evidence.
- Downgrade confidence or mark unsupported claims.

## 18. Runtime / agent integration

After CLI research runs work, expose research as a service/tool to the existing agent system.

Target API:

```rust
pub struct ResearchService {
    coordinator: ResearchCoordinator<...>,
}

impl ResearchService {
    pub async fn answer_for_agent(&self, req: ResearchRequest) -> Result<AgentResearchAnswer>;
    pub async fn create_handoff(&self, req: ResearchRequest) -> Result<ResearchArtifactRef>;
    pub async fn create_report(&self, req: ResearchRequest) -> Result<ResearchArtifactRef>;
}
```

Planner/reviewer integration should pass `audience = AgentPlanner` or `AgentReviewer`, request `AgentAnswer` or `AgentHandoff`, and receive artifact refs. The caller should not receive raw source text by default.

Trigger heuristics can be added later:

```text
Invoke research when:
- task touches unknown external API/protocol/library
- architecture choice has long-term coupling
- user explicitly asks for comparison or recommendation
- local codebase lacks enough evidence
- previous implementation failed due to conceptual uncertainty
- security/performance correctness depends on external facts

Do not invoke research when:
- task is mechanical
- tests already define desired behavior
- local docs already answer the question
- edit is small and reversible
```

## 19. TUI research browser later

After artifact creation works, add a simple research browser to the TUI.

Views:

```text
Research runs list
Run details
Sources
Claims
Report
Agent handoff
```

Useful commands:

```text
/research-runs
/research-open <run_id>
/research-show report <run_id>
/research-show handoff <run_id>
/research-show claims <run_id>
```

Keep this separate from the core research implementation. The core must be usable headlessly.

## 20. Testing plan

Add unit/integration tests before model-dependent behavior.

Required tests:

```text
research_types_serialize_roundtrip
research_store_creates_run_directory
research_store_appends_and_reads_sources_jsonl
research_store_appends_and_reads_evidence_jsonl
research_store_writes_report
local_repo_source_respects_basic_limits
url_source_requires_allow_network
url_source_rejects_oversized_response_or_truncates
claim_records_preserve_evidence_ids
verifier_rejects_claim_with_missing_evidence
verifier_rejects_report_with_missing_claim_refs
agent_handoff_contains_artifact_refs
```

For tests requiring source data, use temp directories and local files. Avoid network tests by default. Put network tests behind an ignored test or feature flag.

## 21. Suggested implementation order for MiMo v2.5

Follow this order. Do not skip ahead into web search or TUI polish.

1. Inspect current module layout and identify config, CLI, provider/model, session/artifact, and TUI slash-command patterns.
2. Add `src/research/` module with types and errors only.
3. Add filesystem-backed `ResearchStore` using `.codegg/research/<run_id>/`.
4. Add tests for store creation, JSON serialization, and artifact writes.
5. Add a minimal `ResearchCoordinator` that creates a run and writes a deterministic placeholder plan/report/handoff.
6. Add CLI command `codegg research` with question, mode, audience, depth, output, source, and allow-network flags.
7. Add `LocalRepoSource` for explicit `file:` sources and bounded `local` search.
8. Add deterministic chunking/evidence extraction from local files.
9. Add `UrlSource` for explicit `url:` sources behind `--allow-network`.
10. Add claim records with deterministic fallback.
11. Add model-backed planning/claim/synthesis only through the existing provider abstraction. If this is hard, leave prompt templates and keep deterministic output.
12. Add report, brief, agent answer, and handoff renderers.
13. Add structural citation verifier.
14. Add optional TUI `/research` command that calls the same coordinator.
15. Add planner/reviewer integration as a separate follow-up PR.

## 22. Prompt templates to include in code

Store prompts in `src/research/templates.rs` or equivalent. Keep them short and JSON-oriented.

### Planning prompt

```text
You are a research planner for an engineering agent harness.
Question: {question}
Mode: {mode}
Audience: {audience}
Depth: {depth}
Constraints: {constraints}

Produce a concise research plan with:
- scope
- comparison axes or investigation axes
- source classes to inspect
- exclusion criteria
- stopping conditions
- expected outputs

Return Markdown only.
```

### Evidence extraction prompt

```text
You are extracting evidence for an engineering research task.
Question: {question}
Source ID: {source_id}
Source locator: {locator}

Return JSON array only. Each item:
{
  "text": "evidence text or precise paraphrase",
  "summary": "why this matters",
  "relevance": "low|medium|high",
  "caveats": []
}

Do not invent evidence. Return [] if not relevant.
```

### Claim construction prompt

```text
You are constructing a claim graph.
Question: {question}
Evidence records: {evidence_json}

Return JSON array only. Each item:
{
  "text": "claim",
  "claim_type": "fact|comparison|recommendation|risk|caveat|open_question|inference",
  "confidence": "low|medium|high",
  "evidence_ids": ["..."],
  "caveats": [],
  "applies_to": []
}

Every factual or comparative claim must cite evidence_ids.
```

### Agent answer synthesis prompt

```text
You are answering a narrow research question for another coding agent.
Question: {question}
Claims: {claims_json}
Contradictions/gaps: {contradictions_json}

Return a compact operational answer with:
- direct answer
- recommendation
- confidence
- rationale using claim IDs
- caveats
- do-not-assume list
- validation tasks
- evidence pointers

Do not include raw source text unless necessary.
```

### Human report synthesis prompt

```text
You are writing a human-facing engineering research report.
Question: {question}
Mode: {mode}
Claims: {claims_json}
Contradictions/gaps: {contradictions_json}

Write a detailed Markdown report using the configured template.
Every major finding must reference claim IDs.
Separate evidence-backed claims from inferences and open questions.
```

### Verification prompt, later optional

```text
Check whether each claim is supported by its cited evidence.
Return JSON with claim_id, support_status, explanation, and suggested confidence adjustment.
Do not evaluate uncited knowledge.
```

## 23. Acceptance criteria for the MVP

The implementation is acceptable when all of the following are true:

```text
- `cargo test` passes.
- `codegg research "test question" --source file:... --output human-full --output agent-handoff` creates a run directory.
- The run directory contains request.json, run.json, sources.jsonl, evidence.jsonl, claims.jsonl, report.md, and handoff.ctx.md.
- The report has a scope, method, sources, findings, caveats, open questions, and validation section.
- The handoff is compact and includes artifact refs rather than raw source dumps.
- The verifier catches missing evidence IDs.
- Network fetching is disabled unless explicitly enabled.
- Existing chat/session behavior is unchanged when research is not used.
```

## 24. Follow-up milestones after MVP

After the MVP lands, implement these in order:

1. SQLite metadata index for research runs, if useful for session search and TUI lists.
2. TUI research browser.
3. Planner/reviewer `ResearchTool` integration.
4. Crates.io and GitHub metadata adapters for Rust dependency scrutiny.
5. Source quality policy configuration.
6. Model-backed semantic citation verifier.
7. Research refresh: rerun source fetches and diff changed claims.
8. Re-synthesis from existing evidence for different audiences.
9. Optional external search provider adapters.
10. Optional server API endpoints for research runs.

## 25. Design principle to preserve

The research subsystem exists to reduce context pollution, not increase it. Raw sources and long reports should stay in the research store. Agents should receive compact answers, handoffs, and artifact references. Humans should be able to open the full report and drill into sources when desired.

