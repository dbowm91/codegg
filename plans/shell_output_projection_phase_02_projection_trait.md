# Shell Output Projection Phase 2: Projection Trait and Generic Projectors

## Objective

Introduce the projection abstraction that converts raw command artifacts into explicit model-facing and TUI-facing views. This phase should make projection a first-class concept in codegg and provide conservative built-in projectors before RTK or command-specific native projectors are added.

By the end of this phase, codegg should distinguish exact raw views from truncated, structured, lossy, redacted, and externally compressed views. Even when the initial implementation is simple, projection metadata should be explicit enough to support future RTK integration and context-budget-aware selection.

## Dependency

This phase assumes Phase 1 has landed or is substantially available:

- command runs have stable IDs
- raw stdout/stderr are retained out-of-band
- command metadata is captured
- model-visible output passes through a single projection boundary

## Design Direction

Add a trait such as:

```rust
pub trait CommandOutputProjector: Send + Sync {
    fn name(&self) -> &'static str;

    fn supports(&self, request: &ProjectionRequest) -> ProjectionSupport;

    fn project(
        &self,
        request: ProjectionRequest,
        raw: &CommandOutputStore,
    ) -> Result<ProjectionResult, ProjectionError>;
}
```

The trait should be independent of RTK. RTK will later be one implementation of this trait or one backend behind a projector.

`ProjectionRequest` should contain enough information to make policy decisions:

```rust
pub struct ProjectionRequest<'a> {
    pub run: &'a CommandRun,
    pub target: ProjectionTarget,
    pub policy: ProjectionPolicy,
    pub budget: ProjectionBudget,
    pub exact_requested: bool,
    pub allow_lossy: bool,
    pub allow_external_backend: bool,
}
```

`ProjectionTarget` should distinguish model context from local TUI display:

```rust
pub enum ProjectionTarget {
    ModelContext,
    TuiTranscript,
    TuiDetail,
    ToolExpansion,
}
```

Model context should be stricter about redaction and token budget. Local TUI detail can usually show more raw output.

## Projection Result Model

A projection result should not just be a string. It should carry provenance and risk metadata.

```rust
pub struct ProjectionResult {
    pub text: String,
    pub projector: String,
    pub kind: ProjectionKind,
    pub exactness: ProjectionExactness,
    pub redaction: RedactionState,
    pub omitted: Vec<OmittedRange>,
    pub expansion_handles: Vec<ExpansionHandle>,
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub estimated_input_tokens: Option<usize>,
    pub estimated_output_tokens: Option<usize>,
    pub warnings: Vec<String>,
}
```

Suggested enums:

```rust
pub enum ProjectionKind {
    Raw,
    Truncated,
    ErrorRetention,
    Structured,
    ExternalCompressed,
    Summary,
}

pub enum ProjectionExactness {
    Exact,
    ExactRange,
    Truncated,
    Lossy,
    Parsed,
    PartialRawArtifact,
}
```

`OmittedRange` should identify stream and byte or line ranges. Byte ranges are more exact. Line ranges are friendlier. Supporting both is ideal, but byte ranges are sufficient initially.

## Built-in Generic Projectors

Implement at least three conservative projectors.

### RawProjector

Returns exact stdout/stderr content within the model or TUI budget. This projector should be used when output is small or exact output is requested.

For model context, include:

- command string
- cwd if useful
- exit state
- stdout text
- stderr text
- raw handles

If output is non-UTF-8, use a safe lossy marker or refuse exact text projection while preserving raw handles.

### TruncatedProjector

For long output, preserve a bounded head and tail with explicit omission markers.

Example shape:

```text
Command: cargo test
Exit: 101
Duration: 2.14s
stdout: 148.2 KiB, stderr: 41.9 KiB
Projection: truncated head/tail; raw retained as cmd://42/raw

--- stdout head ---
...

--- omitted 132.7 KiB from stdout; expand cmd://42/stdout?range=... ---

--- stdout tail ---
...

--- stderr ---
...
```

Do not silently drop stderr. Do not hide non-zero exit state. Omission markers should be mechanically recognizable so later compaction can preserve them.

### ErrorRetentionProjector

For failed or suspicious commands, retain lines matching failure/error patterns, plus bounded context around those lines. This projector should be deterministic and conservative.

Initial pattern classes:

- Rust: `error[`, `error:`, `warning:`, `panicked at`, `thread '`, `assertion`, `FAILED`, `failures:`
- Python: `Traceback`, `AssertionError`, `Exception`, `FAILED`, `ERROR`
- JS/TS: `Error:`, `TypeError`, `ReferenceError`, `SyntaxError`, `FAIL`, `failed`
- Generic: `fatal`, `panic`, `segfault`, `denied`, `not found`, `unresolved`, `timeout`, `failed`, `failure`, `exception`

This should not replace language-specific structured projectors. It is a fallback.

## Projection Selection

Add a simple selector that picks a projector based on request policy and output size.

Initial behavior:

1. If exact output is requested, use `RawProjector` within limits.
2. If total output is below budget, use `RawProjector`.
3. If exit is non-zero or timeout/cancelled, use `ErrorRetentionProjector`.
4. Otherwise use `TruncatedProjector`.

RTK and native structured projectors will later insert into this selection pipeline.

## Budgeting

Add a simple `ProjectionBudget` now, even if token estimation is approximate:

```rust
pub struct ProjectionBudget {
    pub max_output_bytes: usize,
    pub max_output_tokens: Option<usize>,
    pub preferred_output_tokens: Option<usize>,
}
```

A rough bytes-to-tokens estimate is acceptable for phase 2. It is more important to establish the budget plumbing than to get perfect estimates.

## Metadata Banner

Every model-facing projection should include a concise metadata header unless an existing protocol already carries equivalent structured metadata.

Suggested fields:

- command ID
- exit state
- duration
- stdout/stderr byte counts
- projector name
- exactness/lossiness
- raw handles

The header should be stable and compact. It should help the model understand whether it is seeing exact output or a projection.

## Redaction Hook Placeholder

Phase 8 will implement full redaction. This phase should still include a redaction hook in the projection pipeline:

```rust
fn redact_model_visible_output(input: &str, policy: &RedactionPolicy) -> RedactionResult
```

The initial implementation may be minimal, but the call site should be present so future redaction cannot be bypassed by RTK or native projectors.

## Tests

Add tests for:

1. Small output uses raw projection.
2. Long successful output uses truncated projection.
3. Non-zero command output uses error-retention projection.
4. Stderr is preserved or explicitly represented in all projection kinds.
5. Omitted ranges are recorded when output is truncated.
6. Projection metadata identifies projector and exactness.
7. Exact requested output bypasses lossy projectors within configured limits.
8. Non-UTF-8 output does not panic.
9. Partial raw artifacts are labeled as partial.
10. Redaction hook is invoked for model-facing projections.

## Success Criteria

- There is a `CommandOutputProjector` abstraction or equivalent.
- Model-visible command output is represented by a `ProjectionResult`, not just a string.
- Raw, truncated, and error-retention projectors exist.
- Projection selection is centralized.
- Projection metadata identifies exactness, projector name, omitted ranges, and expansion handles.
- Stderr and non-zero exit states are never silently hidden.
- The redaction hook is present in the model-facing path.
- Existing shell UX continues to work.

## Non-Goals

- Do not add RTK yet.
- Do not build command-specific Rust/Git parsers yet.
- Do not build a full token estimator yet.
- Do not build the polished TUI expansion panel yet.
- Do not add model-generated summaries yet.

## Risks and Caveats

Avoid overfitting the trait to the first projectors. It should support future projectors that parse structured JSON, call external tools, or use source navigation. At the same time, avoid designing an excessively abstract plugin framework in this phase. The immediate need is a reliable internal projection boundary.

The other major risk is giving the model a projected output without telling it what was omitted. Projection metadata and raw handles are required, not cosmetic.
