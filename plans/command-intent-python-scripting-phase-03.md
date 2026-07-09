# Phase 03: Projection Pipeline Unification and RTK Policy

## Objective

Unify command-output projection so shell commands, managed argv runs, test runs, git operations, searches, and future Python scripts all produce a common model-facing `ProjectionResult` while preserving raw artifacts. RTK should become an optional projection backend selected by policy when enabled and useful.

## Relationship to previous phases

Phase 01 introduced command intent. Phase 02 introduced planning and backend route metadata. Phase 03 makes projection the explicit context boundary. It should generalize the existing shell projection machinery without removing existing projectors or changing all execution paths at once.

## Current substrate

The shell architecture already documents:

- `CommandRun`, `CommandExit`, `CommandOutputStore`, `OutputHandle`, and `default_command_projection`;
- `CommandOutputProjector`, `ProjectionRequest`, `ProjectionResult`;
- raw/truncated/error-retention projectors;
- native projectors for `git status`, `git diff`, `git log`, `cargo check`, and `cargo test`;
- `RtkDiscovery`, `RtkAvailability`, `CompressionEligibility`, `classify_command()`, and `RtkProjector`;
- `ShellCommandRunBridge` to mirror shell events into the command output store.

This phase should promote those concepts into a command-wide projection contract.

## Target invariant

Every executable route should produce:

1. raw stdout/stderr/log/diff artifacts, stored by handle;
2. structured metadata: exit, duration, cwd, origin, backend, intent, permissions, truncation state;
3. a bounded projection suitable for model context;
4. optional RTK-compressed projection when eligible;
5. exact preserved spans for high-value data such as file paths, line numbers, compiler errors, test failures, and selected diff hunks.

Raw artifacts must not be replaced by RTK output. RTK only affects the model-facing projection.

## Proposed type refinements

If existing shell types are reusable, keep them and add adapters. If they are too shell-specific, introduce shared projection types and convert shell results into them.

```rust
pub struct ProjectionRequest {
    pub run_id: CommandRunId,
    pub intent: CommandIntentKind,
    pub backend: ExecutionBackendKind,
    pub stdout: Option<OutputHandle>,
    pub stderr: Option<OutputHandle>,
    pub artifacts: Vec<ArtifactHandle>,
    pub exit: CommandExit,
    pub context_budget: ProjectionBudget,
    pub rtk_policy: RtkProjectionPolicy,
    pub redaction_policy: RedactionPolicy,
}

pub struct ProjectionResult {
    pub run_id: CommandRunId,
    pub status: ProjectionStatus,
    pub summary: String,
    pub sections: Vec<ProjectionSection>,
    pub preserved_spans: Vec<ExactSpan>,
    pub artifact_handles: Vec<ArtifactHandle>,
    pub truncation: Option<TruncationReport>,
    pub rtk: Option<RtkProjectionMetadata>,
    pub model_text: String,
}

pub struct ProjectionSection {
    pub title: String,
    pub kind: ProjectionSectionKind,
    pub text: String,
    pub source_handle: Option<OutputHandle>,
}
```

Keep `model_text` as a stable rendered view to minimize churn in current tool returns, but preserve structured sections for future TUI/protocol uses.

## Artifact model

Introduce or normalize artifact handles for:

- stdout;
- stderr;
- combined logs;
- raw command output;
- test reports;
- diffs;
- Python script bodies later;
- generated JSON reports;
- RTK compressed projection bodies if persisted.

If `OutputHandle` already covers most of this, avoid creating a second handle type prematurely. The important property is durable lookup and bounded promotion.

## Projector registry

Add a deterministic projector selection layer keyed by `ProjectorRoute` from Phase 02.

```rust
pub trait CommandProjector: Send + Sync {
    fn name(&self) -> &'static str;
    fn supports(&self, request: &ProjectionRequest) -> bool;
    fn project(&self, request: ProjectionRequest) -> Result<ProjectionResult, ProjectionError>;
}
```

Projectors to support or adapt in this phase:

- raw projector;
- truncated projector;
- error-retention projector;
- git status projector;
- git diff projector;
- git log projector;
- cargo check/test projector, or adapter to test runner report;
- test report projector;
- file search projector;
- RTK wrapper projector.

## RTK policy behavior

Implement RTK as a wrapper around another projector, not as a replacement for semantic projectors.

Recommended flow:

```text
semantic projector -> projection result with exact spans -> RTK eligibility check -> RTK compression for compressible sections -> merge exact spans back -> final ProjectionResult
```

Eligibility rules:

- RTK disabled if not configured or not discovered.
- RTK disabled for projections already under budget.
- RTK eligible for long raw output, long test logs, long search output, large diffs, generated reports, and future Python outputs.
- RTK should preserve exact spans selected by semantic projectors.
- RTK failure should degrade to non-RTK projection, not fail the command.

Metadata should record:

- RTK availability state;
- whether RTK was attempted;
- whether compression succeeded;
- input bytes and output bytes;
- exact-span preservation count;
- fallback reason on failure.

## Context budget integration

Use existing `eggcontext`/context utilities where possible. Projection should accept a budget rather than hardcoding output length everywhere.

Suggested budget fields:

```rust
pub struct ProjectionBudget {
    pub max_model_bytes: usize,
    pub max_section_bytes: usize,
    pub max_failures: usize,
    pub max_diff_hunks: usize,
    pub prefer_error_spans: bool,
}
```

Initial defaults can mirror current shell/test output sizes to avoid regressions.

## Redaction

The existing shell projector redaction hook should be preserved and applied before model-facing text is produced. Raw artifacts may remain raw depending on current repo policy, but model-facing projections should redact known secret patterns. Coordinate with `eggsentry` if available.

Do not expand this phase into a full secret-scanning redesign. The goal is to ensure the unified projection path does not regress redaction.

## Integration steps

1. Add shared projection types or adapters.
2. Wrap existing shell projectors in the shared trait.
3. Add `ProjectorRegistry` or deterministic selector from `ProjectorRoute`.
4. Add RTK wrapper behavior with graceful fallback.
5. Add projection budget plumbing.
6. Update shell projection harness fixtures to assert the new common result shape.
7. Add test runner report adapter so `TestReport` can become `ProjectionResult` without reparsing logs.
8. Keep current user-visible shell/test outputs stable unless explicitly covered by tests.

## Tests

Add or extend tests for:

- raw output projection under budget;
- truncation with head/tail preservation;
- error retention with Rust compiler errors;
- pytest failure projection;
- git diff projection preserving file paths and selected hunks;
- RTK unavailable fallback;
- RTK available mock compression;
- exact-span preservation through RTK wrapper;
- redaction before model-facing text;
- artifact handle presence for raw stdout/stderr.

Prefer a mock RTK implementation for unit tests. Do not require an RTK binary for normal CI/unit tests.

## Acceptance criteria

- Common projection types exist and are used by at least shell projection and test report adapter paths.
- Existing shell projection harness still passes or is deliberately migrated.
- RTK policy can be evaluated without an installed RTK binary.
- RTK failures gracefully fall back to ordinary projections.
- Raw artifacts remain addressable by handle.
- Model-facing projections are bounded and redacted.
- Exact spans survive compression wrappers.

## Suggested validation commands

```bash
cargo test --test shell_projection_harness
cargo test --test shell_projection_phase10
cargo test -p codegg --lib shell::rtk
cargo test -p codegg --lib shell::redactor
cargo test -p codegg --lib test_runner
cargo test -p codegg --lib command_intent
```

Broader fallback:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

## Risks and mitigations

The main risk is breaking existing shell/test output expectations. Mitigate by adding adapters first and changing rendering only after tests define the new contract. Another risk is making RTK a hard dependency. Mitigate by treating RTK as optional and by requiring no-binary unit tests for the RTK path.
