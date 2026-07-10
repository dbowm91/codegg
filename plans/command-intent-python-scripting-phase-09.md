# Phase 09: RTK-Aware Projection, Redaction, and Context Promotion

## Objective

Complete the projection side of the command-intent roadmap by making command, test, git, search, and Python output use a single policy-driven context boundary. Raw artifacts should remain durable in the Phase 07 run store, while model-facing output is produced through bounded projectors with optional RTK compression, exact-span preservation, redaction, and explicit promotion policy.

This phase should make it possible to answer deterministically what entered context, why it entered context, whether RTK was used, which exact raw spans support the projection, and how the user can inspect or promote more.

## Scope

This phase covers:

- unified projection inputs and outputs;
- projector selection by run kind;
- RTK eligibility and fallback;
- exact raw-span mapping;
- secret/sensitive-output redaction;
- context-promotion decisions;
- token budgeting;
- projection persistence and reproducibility;
- TUI/protocol integration with Phase 08 surfaces;
- regression and quality tests.

## Existing substrate to reuse

Reuse:

- existing shell projector traits and native projectors;
- `ProjectionResult`, `ProjectorRoute`, and `PlanRtkPolicy`;
- RTK backend/config/discovery work already present in codegg;
- `eggcontext` token utilities;
- test runner structured reports;
- Python projection and diff generation;
- Phase 07 artifacts/run manifests;
- Phase 08 context-promotion UI and protocol events;
- security redaction/scanning primitives.

## Design principles

1. Raw artifacts are authoritative.
2. Projection is deterministic where possible.
3. RTK is optional and never required for correctness.
4. Exact supporting spans must be retained for compressed/summarized output.
5. Redaction occurs before model promotion.
6. Context budgets are explicit and per-run/per-session aware.
7. Projection metadata must be persisted so a run is auditable.

## Workstream A: Define the unified projection contract

Create or finalize a frontend-independent contract:

```rust
pub struct ProjectionRequest {
    pub run_id: RunId,
    pub run_kind: RunKind,
    pub artifact_refs: Vec<ArtifactRef>,
    pub projector_route: ProjectorRoute,
    pub context_budget: ProjectionBudget,
    pub redaction_policy: RedactionPolicy,
    pub rtk_policy: RtkProjectionPolicy,
    pub promotion_target: PromotionTarget,
}

pub struct ProjectionResult {
    pub projection_id: ProjectionId,
    pub text: String,
    pub structured: Option<serde_json::Value>,
    pub source_spans: Vec<ArtifactSpanRef>,
    pub omitted_regions: Vec<OmittedRegion>,
    pub truncation: TruncationReport,
    pub redactions: Vec<RedactionRecord>,
    pub rtk: RtkResultMetadata,
    pub token_estimate: usize,
    pub safe_for_model: bool,
}
```

Projectors should consume artifact references/ranges rather than transient stdout strings where possible.

## Workstream B: Projector registry and selection

Add a typed projector registry for:

- raw/truncated shell output;
- error-retention build/lint output;
- test reports;
- git status;
- git diff;
- git log/show;
- search/file listing;
- Python analyze output;
- Python transform diff/change report;
- Python verify/test output;
- policy/denial reports.

Selection should be determined by `CommandPlan`/`RunKind`, not guessed from text after execution.

Each projector must define:

- supported artifact types;
- default budget;
- exact-span behavior;
- redaction requirements;
- RTK eligibility;
- fallback projector.

## Workstream C: RTK policy and execution

Finalize RTK policies:

- Disabled;
- Eligible;
- RequiredForPromotion only where explicitly configured;
- PreferNativeThenRtk;
- RtkOnlyForOverflow.

Recommended behavior:

1. Run native structured projector first.
2. If result fits budget, do not invoke RTK.
3. If oversized and eligible, invoke RTK on bounded/raw artifact input.
4. Validate RTK output and span references.
5. If RTK fails or is unavailable, use deterministic truncation/error-retention fallback.
6. Record RTK binary/version/config and compression statistics.

RTK must not rewrite exact diffs or diagnostics in ways that lose actionable line/file references. Preserve exact error/diff spans separately.

## Workstream D: Exact-span preservation

Every non-trivial projection should map claims or excerpts back to raw artifacts:

```rust
pub struct ArtifactSpanRef {
    pub artifact_id: ArtifactId,
    pub byte_start: u64,
    pub byte_end: u64,
    pub line_start: Option<u64>,
    pub line_end: Option<u64>,
    pub role: SpanRole,
}
```

Support roles such as:

- exact excerpt;
- supporting diagnostic;
- failure summary;
- diff hunk;
- omitted repetitive region;
- redacted region.

Provide expansion APIs so TUI/protocol clients can open the exact source range.

## Workstream E: Redaction pipeline

Apply redaction before context promotion, with records retained in projection metadata.

Detect and redact at minimum:

- API keys/tokens;
- private keys;
- auth headers/cookies;
- `.env` values;
- common cloud credentials;
- SSH material;
- user-configured secret patterns;
- sensitive absolute paths where configured.

Requirements:

- raw local artifacts may remain unredacted only under explicit local-only policy;
- projections must state that redaction occurred without exposing the secret;
- exact raw span expansion should require appropriate local authorization;
- RTK should receive redacted input by default unless policy explicitly allows local raw processing.

## Workstream F: Context promotion policy

Define deterministic promotion decisions:

```rust
pub enum PromotionDecision {
    Exclude,
    IncludeProjection,
    IncludeSelectedSpans(Vec<ArtifactSpanRef>),
    StoreOnly,
    RequireUserConfirmation,
}
```

Inputs should include:

- run kind/status;
- risk/sandbox state;
- projection size;
- session context budget;
- whether output contains actionable failure information;
- user command semantics (`!`, `!!`, explicit promote);
- model/tool origin;
- redaction state;
- prior related run projections.

Avoid repeatedly promoting nearly identical long outputs. Use deduplication hashes and failure-change detection where available.

## Workstream G: Token budgeting

Use `eggcontext` to estimate tokens and enforce:

- per-projection soft/hard budgets;
- total tool-output budget per turn;
- reserved budget for manager/reviewer agents;
- lower budgets for successful/no-op runs;
- higher bounded budgets for failures with actionable diagnostics;
- differential promotion for repeated tests.

Persist requested and actual token estimates in the run manifest.

## Workstream H: Python-specific projection

### Analyze

- concise stdout/result summary;
- tables/statistics where structurally available;
- no script body by default;
- policy/sandbox warning if degraded;
- exact stdout spans.

### Transform

- changed-file summary;
- bounded unified diff excerpts;
- exact diff hunk references;
- omitted-hunk counts;
- stdout/stderr only if actionable;
- rollback availability.

### Verify

- structured pass/fail summary;
- failed test diagnostics;
- bounded subprocess output;
- exact log references.

## Workstream I: Test-runner and repeated-run optimization

Use previous-run indexes to project deltas:

- new failures;
- resolved failures;
- unchanged failures;
- duration regressions;
- changed test counts.

For unchanged long failures, promote the concise delta plus handles rather than the full repeated log.

## Workstream J: Persist projection metadata

Store in Phase 07 run records:

- projection ID/version;
- projector name/version;
- input artifact digests;
- output artifact reference;
- source spans;
- redaction records;
- RTK metadata;
- requested/actual budget;
- promotion decision;
- model-context insertion event.

This enables reproducibility and later debugging of context mistakes.

## Workstream K: Tests

Add tests for:

- projector selection by run kind;
- deterministic fallback without RTK;
- RTK unavailable/failure behavior;
- exact-span roundtrip to raw artifact;
- diff/error line preservation;
- redaction before RTK/model context;
- promotion exclusion for unsafe/unredacted output;
- token hard-budget enforcement;
- repeated test delta projection;
- Python Analyze/Transform/Verify projections;
- artifact digest mismatch handling;
- projection metadata persistence;
- TUI expansion of exact spans.

Add adversarial fixtures containing fake secrets, repetitive logs, malformed UTF-8, ANSI escapes, huge lines, prompt-injection text, and misleading summaries.

## Validation commands

```bash
cargo test -p codegg --lib shell::projector
cargo test -p codegg --lib python_script::projection
cargo test -p codegg --lib test_runner
cargo test -p codegg --lib command_intent::plan
cargo test -p codegg-core run_store
cargo test -p eggcontext
```

Projection integration tests:

```bash
cargo test --test shell_projection_harness
cargo test --test shell_projection_phase10
```

Full capped suite:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

## Acceptance criteria

- all execution families use a shared projection contract;
- raw artifacts remain authoritative and expandable;
- RTK is optional, measured, and safely fallible;
- exact supporting spans are persisted;
- secrets are redacted before model promotion;
- context promotion is explicit and budget-aware;
- repeated long outputs are delta-projected;
- Python/test/git/search projections are structured and bounded;
- run manifests record exactly what entered model context.
