# LSP Capability, Cache, and Semantic Context Plan

## Purpose

Pivot from security-review-specific LSP integration to reusable LSP infrastructure for the whole harness. The security review workflow now has a mature LSP consumer path: optional `securityContext`, async execution, result receipts, source preview, hunk context, and hunk-focused navigation. The next LSP phase should extract and harden shared infrastructure so other Codegg features can consume semantic context without duplicating security review plumbing.

This pass should focus on:

- server capability discovery and normalization;
- diagnostics/cache lifecycle;
- workspace/document ownership boundaries;
- reusable semantic context request APIs;
- clear fallback behavior when servers lack a capability;
- tests that do not require live language servers.

## Current State

Known current LSP/security review state:

- `LspTool` supports read-only operations including diagnostics, symbols, references, call hierarchy, type hierarchy, and `securityContext`.
- Security review can optionally use `LspSecurityContextExecutor` in local TUI mode.
- Remote/socket mode falls back deterministically when no executor exists.
- `securityContext` is bounded and read-only.
- Source preview and hunk navigation are TUI-only UX around review output.
- Broader consumers do not yet have a clean semantic context abstraction comparable to security review.

## Non-Goals

Do not make LSP mandatory.

Do not require live LSP servers in unit tests.

Do not introduce mutation/edit/apply operations.

Do not add agent-controlled shell execution.

Do not expand offensive security behavior.

Do not rewrite the whole LSP layer in one pass.

Do not change model routing or agent loop semantics except where needed to expose read-only semantic context.

## Phase 1 — Inventory Current LSP Surfaces

Map the current LSP implementation and consumer paths.

Search:

```bash
rg "struct LspTool|enum LspOperation|securityContext|callHierarchy|typeHierarchy|diagnostic|workspaceSymbol|documentSymbol|references|definition" src crates tests -n
rg "LspService|egglsp|capabilit|initialize|server_capabilities|Diagnostics|cache" src crates tests -n
rg "LspSecurityContextExecutor|SecurityContextExecutor|semantic context|SemanticContext" src crates tests -n
```

Document:

- public LSP tool operations;
- operation request/response shapes;
- server capability storage, if any;
- diagnostics cache behavior;
- lifecycle of document open/change/close, if present;
- how local TUI and remote/socket paths differ.

Deliverable:

- small architecture note update in `architecture/lsp.md` describing the current runtime ownership and gaps.

Acceptance criteria:

- no behavior change required in this phase;
- inventory identifies exact modules to refactor;
- security review remains unchanged.

## Phase 2 — Capability Discovery and Normalization

Introduce a reusable capability snapshot for each LSP server/workspace.

Suggested type:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspCapabilitySnapshot {
    pub language_id: Option<String>,
    pub server_name: Option<String>,
    pub supports_diagnostics: bool,
    pub supports_document_symbols: bool,
    pub supports_workspace_symbols: bool,
    pub supports_definition: bool,
    pub supports_references: bool,
    pub supports_hover: bool,
    pub supports_completion: bool,
    pub supports_call_hierarchy: bool,
    pub supports_type_hierarchy: bool,
    pub supports_semantic_tokens: bool,
}
```

Add normalized helpers:

```rust
impl LspCapabilitySnapshot {
    pub fn supports(&self, op: LspSemanticOperation) -> bool;
    pub fn fallback_reason(&self, op: LspSemanticOperation) -> Option<String>;
}
```

Operation enum:

```rust
pub enum LspSemanticOperation {
    Diagnostics,
    DocumentSymbols,
    WorkspaceSymbols,
    Definition,
    References,
    Hover,
    CallHierarchy,
    TypeHierarchy,
    SemanticTokens,
    SecurityContext,
}
```

Acceptance criteria:

- capability detection uses actual initialized server capabilities where available;
- unsupported capabilities return structured unavailable responses, not panics;
- docs list fallback behavior;
- tests cover capability snapshots without live LSP.

## Phase 3 — Centralize LSP Fallback Responses

Create a small reusable response convention for unsupported/unavailable LSP operations.

Suggested type:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspUnavailable {
    pub operation: String,
    pub reason: String,
    pub server: Option<String>,
    pub language_id: Option<String>,
}
```

Rules:

- unsupported operation is not an execution failure;
- unavailable response should be model-safe and concise;
- security review enrichment should keep using fail-soft deterministic fallback;
- direct user/tool calls can surface the unavailable reason.

Acceptance criteria:

- direct tool consumers can distinguish unsupported from failed;
- security review still appends clear notes when enrichment cannot run;
- no existing tests regress.

## Phase 4 — Diagnostics Cache Lifecycle

Define ownership and freshness semantics for diagnostics.

Questions to answer in code/docs:

- Are diagnostics pull-based, push-based, or both?
- Which component owns the latest diagnostics per file?
- How are diagnostics invalidated on file edits, git branch changes, or server restart?
- How are stale diagnostics labeled?
- What happens in remote/socket mode?

Suggested type:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspDiagnosticSnapshot {
    pub file_path: PathBuf,
    pub diagnostics: Vec<LspDiagnosticItem>,
    pub generated_at_ms: i64,
    pub source: LspDiagnosticSource,
    pub freshness: LspDiagnosticFreshness,
}

pub enum LspDiagnosticFreshness {
    Fresh,
    PossiblyStale,
    Stale,
    Unavailable,
}
```

Invalidation rules:

- mark possibly stale on file content changes unless diagnostics are refreshed;
- mark stale on server restart or workspace root change;
- do not silently use stale diagnostics as high-confidence evidence;
- consumers may still show stale diagnostics with labels.

Acceptance criteria:

- diagnostics have explicit freshness metadata;
- security review treats stale diagnostics as lower confidence or notes-only evidence;
- non-security consumers can request latest known diagnostics safely;
- tests cover stale/fresh transitions without live LSP.

## Phase 5 — Shared Semantic Context API

Create a reusable semantic context interface independent of security review.

Suggested request:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticContextRequest {
    pub file_path: PathBuf,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub intent: SemanticContextIntent,
    pub max_symbols: usize,
    pub max_references: usize,
    pub max_diagnostics: usize,
    pub call_depth: u8,
}

pub enum SemanticContextIntent {
    Explain,
    EditPlanning,
    Review,
    SecurityReview,
    TestPlanning,
    Navigation,
}
```

Suggested response:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticContextResponse {
    pub file_path: PathBuf,
    pub symbol: Option<SemanticSymbolSummary>,
    pub diagnostics: Vec<LspDiagnosticItem>,
    pub definitions: Vec<SemanticLocation>,
    pub references: Vec<SemanticLocation>,
    pub call_hierarchy: Option<SemanticCallGraphSummary>,
    pub type_hierarchy: Option<SemanticTypeGraphSummary>,
    pub notes: Vec<String>,
    pub truncated: bool,
    pub unavailable: Vec<LspUnavailable>,
}
```

Security review can adapt this response into security-specific evidence, but the API should not be security-only.

Acceptance criteria:

- semantic context API is read-only;
- request caps are enforced;
- unsupported capabilities are represented explicitly;
- response is usable by planner/reviewer/tester agents later;
- security review can continue using existing executor or migrate incrementally.

## Phase 6 — Tool Surface Integration

Expose the shared semantic context API through a stable internal tool path.

Possible options:

1. Add a new `semanticContext` LSP operation while keeping `securityContext` as a specialized wrapper.
2. Keep `securityContext` and add a lower-level `contextSummary` operation.
3. Add Rust-side internal API first; defer external tool exposure.

Recommended:

- implement Rust-side internal API first;
- expose a conservative `semanticContext` operation only after tests prove bounded behavior;
- keep `securityContext` as compatibility wrapper that sets `intent = SecurityReview` and security-specific caps.

Acceptance criteria:

- no breaking change to existing `/security-review --enrich`;
- new operation is read-only and bounded;
- model-facing output is concise and does not dump huge JSON blobs unless JSON mode explicitly requested;
- docs describe operation caps and fallback.

## Phase 7 — Remote/Core Ownership Model

Clarify how LSP services are owned when Codegg runs with headless core and multiple frontends.

Questions:

- Does the core own LSP servers, or does each frontend?
- How does a remote TUI request semantic context?
- How are workspace roots authorized?
- How are per-client requests bounded?
- How are stale diagnostics broadcast or queried?

Recommended policy:

- headless core owns LSP server processes and caches;
- frontends request semantic context over core protocol;
- frontend never starts its own LSP for the same workspace unless explicitly local-only;
- all requests pass through root authorization;
- remote frontend receives structured unavailable/fallback responses if core has no LSP.

Acceptance criteria:

- architecture doc has a clear ownership decision;
- no implementation needs to be complete in this pass beyond docs/interfaces;
- security review local/remote fallback remains correct.

## Phase 8 — Tests

Add tests around pure structs and mocked services.

Suggested tests:

```text
lsp_capability_snapshot_supports_known_operations
lsp_capability_snapshot_reports_unavailable_reason
lsp_unavailable_serializes_roundtrip
lsp_diagnostic_snapshot_marks_possibly_stale_on_file_change
lsp_diagnostic_snapshot_marks_stale_on_server_restart
semantic_context_caps_are_enforced
semantic_context_unsupported_capabilities_do_not_fail_request
semantic_context_security_intent_uses_security_caps
security_context_wrapper_remains_backward_compatible
remote_lsp_unavailable_response_is_fail_soft
```

Use fake providers/adapters; do not require live language servers.

Acceptance criteria:

- tests run under `cargo test --workspace`;
- no timing-sensitive tests;
- no external LSP server dependency;
- security review tests keep passing.

## Phase 9 — Docs Updates

Update:

```text
architecture/lsp.md
architecture/tool.md
AGENTS.md
README.md
.opencode/skills/security/SKILL.md
.opencode/skills/agent-loop/SKILL.md
```

Document:

- LSP ownership model;
- capability discovery and unsupported-operation semantics;
- diagnostics freshness states;
- semantic context API shape;
- securityContext as a specialized semanticContext wrapper;
- local vs remote behavior;
- read-only invariant.

Acceptance criteria:

- docs match implemented behavior;
- docs do not overpromise live-server support;
- docs preserve no-mutation semantics.

## Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted:

```bash
cargo test -p codegg lsp_capability
cargo test -p codegg lsp_diagnostic
cargo test -p codegg semantic_context
cargo test -p codegg security_context
rg "LspCapabilitySnapshot|SemanticContext|LspDiagnosticSnapshot|LspUnavailable|SecurityContext" src crates tests architecture README.md AGENTS.md .opencode
```

## Done Criteria

This phase is complete when:

- LSP capabilities are represented in a normalized snapshot;
- unsupported operations fail soft with structured unavailable responses;
- diagnostics cache/freshness semantics are explicit;
- a shared semantic context API exists or has a concrete internal interface;
- `securityContext` remains backward compatible;
- remote/core LSP ownership is documented;
- tests do not require live LSP servers;
- all LSP behavior remains read-only and bounded.

## Follow-Up Roadmap

After this pass, continue broader LSP work in this order:

1. Wire semantic context into planner/reviewer agents as optional context.
2. Add semantic source navigation beyond security review: definition, references, hover, diagnostics panel.
3. Add project-level LSP status UI: server health, capabilities, diagnostics freshness.
4. Add remote-core LSP request protocol support.
5. Add configurable per-language server startup/health policies.
6. Add richer call/type graph summaries for large-codebase navigation.
7. Only then consider write-aware LSP features, and keep them explicitly human-approved and preview-first.
