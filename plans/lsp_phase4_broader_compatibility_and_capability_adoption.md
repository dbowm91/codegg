# LSP Phase 4: Broader Server Compatibility and Higher-Level Capability Adoption

## Purpose

Begin the next LSP roadmap phase after completion of Phase 3 supervision and restart lifecycle work through:

```text
48b54ced5001e9d13ed48a2468f4e92a2c8e9751
```

Phase 3 established a stable operational foundation:

- real Tier 1 server initialization and readiness;
- supervised process ownership;
- bounded shutdown, kill, and reap;
- generation-safe restart and publication;
- serialized restart ownership;
- cancellation-safe async lease release;
- document replay and stale diagnostic provenance;
- live `Ready` / `Degraded` outcomes;
- real-server compatibility reports;
- deterministic lifecycle and race coverage.

Phase 4 should now broaden language-server compatibility and expose higher-value LSP capabilities to Codegg workflows. The work must preserve the central safety rule already established by the repository:

```text
read-only semantic operations may be executed directly;
mutation-producing operations must remain preview-only until explicitly applied by a higher-level user-approved path.
```

This plan is tailored for a smaller implementation model. Execute the passes in order. Do not mix lifecycle changes into this phase unless a newly added server reveals a reproducible lifecycle defect.

## Phase 4 Outcomes

At completion:

1. Codegg has measured compatibility profiles for a Tier 2 server matrix.
2. Capability normalization accurately represents server-advertised support rather than relying on broad heuristics.
3. Compatibility reports distinguish protocol support, semantic correctness, and known limitations.
4. Workspace symbols, completion, semantic tokens, declaration, implementation, document highlights, signature help, rename, code actions, and formatting have explicit support policies.
5. Read-only operations are available through typed `egglsp` APIs and capability-gated Codegg tools.
6. Mutation-producing operations return structured previews and never modify the workspace automatically.
7. Real-server fixtures validate operation semantics, not merely successful response parsing.
8. Tier 2 CI remains opt-in and version-pinned.
9. Agent-facing context uses higher-level LSP evidence selectively and within bounded budgets.
10. Existing Phase 2 and Phase 3 suites remain unchanged and green.

## Initial Tier 2 Server Matrix

Implement servers in this order:

```text
1. gopls
2. typescript-language-server
3. clangd
```

Rationale:

- `gopls` adds a compiled language with modules, workspace symbols, implementation queries, rename, formatting, and strong semantic support.
- `typescript-language-server` adds a Node-based server, project/config discovery, JavaScript/TypeScript dual-language behavior, code actions, and rich completion/signature help.
- `clangd` adds compile-database behavior, C/C++ header/source relationships, implementation/declaration distinctions, and different indexing/readiness characteristics.

Do not begin all three simultaneously. Complete `gopls` profile and smoke coverage before adding TypeScript, then add clangd.

## Primary Files

Likely production files:

```text
crates/egglsp/src/capability.rs
crates/egglsp/src/client.rs
crates/egglsp/src/compatibility.rs
crates/egglsp/src/context.rs
crates/egglsp/src/error.rs
crates/egglsp/src/lib.rs
crates/egglsp/src/operations.rs
crates/egglsp/src/service.rs
crates/egglsp/src/workspace_edit.rs
src/lsp/semantic_context.rs
src/lsp/hunk_nav_collector.rs
src/tool/lsp.rs
```

Likely test and CI files:

```text
crates/egglsp/tests/real_server_smoke.rs
crates/egglsp/tests/production_protocol_stdio.rs
crates/egglsp/tests/production_service_stdio.rs
tests/lsp_composite_stdio.rs
.github/workflows/lsp-real-server.yml
```

Documentation:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

Possible new modules:

```text
crates/egglsp/src/navigation.rs
crates/egglsp/src/completion.rs
crates/egglsp/src/semantic_tokens.rs
crates/egglsp/src/code_actions.rs
crates/egglsp/src/formatting.rs
crates/egglsp/src/rename.rs
```

Prefer extending existing modules when cohesive. Create a new module only when a capability has enough request/response normalization and tests to justify it.

## Non-Goals

Do not implement during Phase 4:

- automatic workspace-edit application;
- automatic code-action execution;
- server-initiated command execution;
- multi-root workspace support;
- arbitrary dynamic-registration support beyond the capabilities needed by the selected servers;
- universal support for every LSP server;
- TUI redesign;
- lifecycle/restart refactoring without a concrete regression;
- language-specific AST parsing inside `egglsp`;
- model-facing exposure of unbounded completion or semantic-token payloads.

# Pass 0 — Baseline and Compatibility Report Schema Audit

## Goal

Record the post-Phase-3 baseline and identify report fields required for broader compatibility.

## Required Commands

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo test -p egglsp --features lsp-test-support --tests
cargo test --features lsp-test-support --test lsp_composite_stdio
```

Run Tier 1 real-server tests when binaries are available and preserve their reports as the baseline.

## Report Schema Additions

Extend `LspCompatibilityReport` only where necessary. Recommended optional fields:

```rust
pub struct LspCompatibilityReport {
    // existing fields
    pub protocol_version: Option<String>,
    pub dynamic_registrations: Vec<String>,
    pub operation_support: Vec<LspOperationCompatibility>,
    pub fixture_language: Option<String>,
    pub project_model: Option<String>,
}
```

Suggested operation record:

```rust
pub struct LspOperationCompatibility {
    pub operation: String,
    pub advertised: bool,
    pub request_succeeded: bool,
    pub semantic_assertion_passed: bool,
    pub known_limit: Option<String>,
}
```

Do not duplicate existing check data if the current `checks` vector can express the same information cleanly. Prefer extending check names/details over schema expansion when possible.

## Acceptance Criteria

- Existing reports deserialize after any schema change.
- New fields are optional or backward-compatible.
- Tier 1 reports remain valid.

# Pass 1 — Correct Capability Normalization

## Current Problems to Address

The current normalized snapshot is intentionally small but contains two weak assumptions:

```text
supports_diagnostics = true for every initialized server
supports_type_hierarchy inferred from call_hierarchy support
```

These are not precise enough for broader compatibility.

## Extend `LspCapabilitySnapshot`

Add normalized fields for:

```rust
pub supports_declaration: bool,
pub supports_implementation: bool,
pub supports_document_highlight: bool,
pub supports_signature_help: bool,
pub supports_rename: bool,
pub supports_prepare_rename: bool,
pub supports_code_actions: bool,
pub supports_document_formatting: bool,
pub supports_range_formatting: bool,
pub supports_inlay_hints: bool,
pub supports_folding_ranges: bool,
pub supports_selection_ranges: bool,
pub supports_document_links: bool,
pub supports_execute_command: bool,
```

Keep existing fields.

## Diagnostics Capability

Represent diagnostic support explicitly:

```rust
pub enum DiagnosticSupport {
    Push,
    Pull,
    PushAndPull,
    Unknown,
}
```

If this is too invasive for the first pass, use:

```rust
pub supports_push_diagnostics: bool,
pub supports_pull_diagnostics: bool,
```

Do not continue treating every initialized server as guaranteed push-diagnostics-capable.

For servers that publish diagnostics without explicitly advertising a provider, compatibility profiles may record observed push behavior. Keep advertised and observed support distinct.

## Type Hierarchy

Remove the call-hierarchy heuristic.

If the current `lsp-types` version does not expose type-hierarchy server capability cleanly:

- set advertised type hierarchy to false by default;
- add an optional compatibility-profile override such as `observed_capabilities`;
- or upgrade `lsp-types` only if the upgrade is narrow and all protocol tests remain green.

Do not infer type hierarchy from call hierarchy.

## Capability Detail

For providers that can be either `bool` or options, retain useful options where needed:

```rust
pub struct LspCapabilityDetails {
    pub rename_prepare_provider: bool,
    pub code_action_kinds: Vec<String>,
    pub completion_trigger_characters: Vec<String>,
    pub signature_trigger_characters: Vec<String>,
    pub semantic_token_legend: Option<SemanticTokenLegendSnapshot>,
}
```

Avoid placing large raw `ServerCapabilities` values into agent-facing structures.

## Extend `LspSemanticOperation`

Add operation variants matching the new normalized capabilities:

```text
Declaration
Implementation
DocumentHighlight
SignatureHelp
Rename
CodeAction
DocumentFormatting
RangeFormatting
InlayHints
FoldingRanges
SelectionRanges
```

Use structured `LspUnavailable` responses consistently.

## Tests

Add unit tests for:

- bool and options provider forms;
- absent providers;
- rename with and without prepare support;
- code-action kinds;
- formatting/range-formatting distinction;
- removal of type-hierarchy heuristic;
- diagnostics advertised versus observed support.

## Acceptance Criteria

- No unsupported capability is inferred from an unrelated capability.
- New operation support is normalized and tested.
- Existing callers compile without defaulting new fields incorrectly.

# Pass 2 — Add Tier 2 Compatibility Profiles

## Profile Fields

Add profiles for:

```text
gopls
typescript-language-server
clangd
```

Each profile must specify:

```text
server ID
executable candidates
default arguments
root markers
initialization options
workspace configuration
readiness policy
restart policy
known limitations
observed capability overrides, if required
```

## `gopls` Profile

Suggested values:

```text
server_id: gopls
executable: gopls
args: serve or default stdio invocation according to tested version
root markers: go.work, go.mod, .git
readiness: diagnostics or bounded warmup based on observed behavior
```

Fixture project:

```text
go.mod
main.go
helper/helper.go
```

Include:

- one type error;
- interface and concrete implementation;
- function declaration/use;
- rename-safe identifier;
- format-dirty source;
- workspace symbol target.

## TypeScript Profile

Server identifiers should distinguish executable from language:

```text
server_id: typescript-language-server
executable: typescript-language-server
args: --stdio
root markers: tsconfig.json, jsconfig.json, package.json, .git
```

Fixture:

```text
package.json
tsconfig.json
src/main.ts
src/helper.ts
```

Include:

- type mismatch;
- imported function reference;
- interface implementation;
- completion site;
- signature-help site;
- code-action opportunity;
- rename-safe identifier.

Do not require `npm install` from test code. CI installs pinned server and TypeScript versions.

## `clangd` Profile

Suggested fields:

```text
server_id: clangd
executable: clangd
args: --background-index=false or other deterministic test-safe flags
root markers: compile_commands.json, compile_flags.txt, CMakeLists.txt, .git
readiness: bounded warmup or diagnostics
```

Fixture:

```text
compile_commands.json
include/widget.hpp
src/widget.cpp
src/main.cpp
```

Include:

- declaration/definition split;
- virtual interface and concrete implementation;
- references across header/source;
- one diagnostic;
- format-dirty source.

## Profile Lookup

Add:

```rust
pub fn tier2_profiles() -> Vec<LspCompatibilityProfile>
pub fn all_profiles() -> Vec<LspCompatibilityProfile>
```

Keep `tier1_profiles()` unchanged.

## Tests

Verify exact candidates, args, roots, readiness, and limitations for every profile.

## Acceptance Criteria

- Three Tier 2 profiles exist.
- Generic client code contains no server-ID branches for Tier 2 quirks.
- Profiles are data-driven and unit tested.

# Pass 3 — Generalize the Real-Server Fixture Harness

## Current Problem

The current smoke harness is centered on Rust and Python fixtures. Phase 4 needs reusable operation contracts without a large `match server_id` block.

## Typed Fixture Contract

Introduce a fixture descriptor:

```rust
struct RealServerFixture {
    tempdir: TempDir,
    root: PathBuf,
    language_id: String,
    source_files: Vec<PathBuf>,
    primary_source: PathBuf,
    secondary_source: Option<PathBuf>,
    diagnostics_expectation: DiagnosticsExpectation,
    symbols: Vec<ExpectedSymbol>,
    positions: FixturePositions,
    mutation_targets: MutationTargets,
}
```

Suggested positions:

```rust
struct FixturePositions {
    definition: Option<Position>,
    declaration: Option<Position>,
    implementation: Option<Position>,
    references: Option<Position>,
    hover: Option<Position>,
    completion: Option<Position>,
    signature_help: Option<Position>,
    rename: Option<Position>,
    document_highlight: Option<Position>,
}
```

## Operation Expectations

Use typed semantic expectations rather than generic minimum counts:

```rust
struct LocationExpectation {
    min_locations: usize,
    expected_files: Vec<PathBuf>,
}
```

For completion/signature help:

```rust
struct CompletionExpectation {
    expected_label_substrings: Vec<String>,
}

struct SignatureExpectation {
    expected_label_substrings: Vec<String>,
}
```

## Fixture Factory

Use a trait or enum:

```rust
trait RealServerFixtureFactory {
    fn create(&self) -> RealServerFixture;
}
```

A simple enum with per-language constructors is acceptable. Avoid an over-engineered plugin system.

## Harness Output

Every operation should produce a compatibility check with:

```text
advertised support
request outcome
semantic assertion outcome
duration
known limitation
```

## Acceptance Criteria

- Rust and Python use the generalized harness.
- Adding Go, TypeScript, and C++ does not duplicate the entire smoke runner.
- Fixture assertions are language-specific data, not scattered server-ID branches.

# Pass 4 — Add Read-Only Navigation Operations

Implement these first because they do not mutate the workspace:

```text
textDocument/declaration
textDocument/implementation
textDocument/documentHighlight
textDocument/signatureHelp
workspace/symbol hardening
```

## Typed APIs

Add client/service APIs:

```rust
pub async fn declaration(...)
pub async fn implementation(...)
pub async fn document_highlights(...)
pub async fn signature_help(...)
pub async fn workspace_symbols(...)
```

Normalize response variants into stable internal DTOs.

### Location normalization

Declaration/implementation responses may contain:

```text
Location
Location[]
LocationLink[]
null
```

Normalize to a common type preserving:

```text
target URI
target range
selection range
origin selection range when provided
```

Do not discard `LocationLink` metadata unnecessarily.

### Document highlights

Preserve highlight kind:

```text
Text
Read
Write
```

### Signature help

Bound and normalize:

```text
active signature
active parameter
signature labels
parameter labels/documentation summaries
```

Do not forward full unbounded Markdown documentation into agent context.

## Service-Level Capability Gating

Every operation must:

- inspect normalized capability support;
- return `LspUnavailable` when unsupported;
- include server/language metadata;
- preserve operational health/freshness notes.

## Real-Server Checks

Required Tier 2 checks:

```text
gopls: implementation, declaration/definition, workspace symbols
typescript-language-server: implementation, signature help, document highlights
clangd: declaration, implementation, document highlights
```

## Acceptance Criteria

- Read-only APIs are typed and capability-gated.
- Response variants are normalized.
- Real fixtures assert semantically correct target files/symbols.

# Pass 5 — Harden Completion and Semantic Tokens

## Completion

Completion is already represented in the capability snapshot but needs a bounded, agent-safe operation.

Add normalized output:

```rust
pub struct CompletionCandidate {
    pub label: String,
    pub detail: Option<String>,
    pub kind: Option<String>,
    pub sort_text: Option<String>,
    pub filter_text: Option<String>,
    pub insert_text_preview: Option<String>,
    pub deprecated: bool,
}
```

Rules:

- cap candidates by configurable limit;
- preserve server order unless a deterministic score is applied;
- do not resolve every completion item automatically;
- strip or truncate large documentation/edit payloads;
- never apply completion edits.

Optional second-stage API:

```rust
resolve_completion_item
```

Implement only if needed by Tier 2 tests.

## Semantic Tokens

The current capability snapshot records semantic-token support but needs accurate legend and decoding.

Add:

```rust
pub struct SemanticTokenLegendSnapshot {
    pub token_types: Vec<String>,
    pub token_modifiers: Vec<String>,
}

pub struct DecodedSemanticToken {
    pub line: u32,
    pub start: u32,
    pub length: u32,
    pub token_type: String,
    pub modifiers: Vec<String>,
}
```

Requirements:

- decode delta-encoded tokens correctly;
- validate token type/modifier indexes;
- handle malformed payloads as structured errors;
- cap tokens before agent-facing formatting;
- retain document version/generation metadata where available.

Do not implement semantic-token delta updates unless required by a selected server. Full-document tokens are sufficient for Phase 4.

## Agent Use Policy

Semantic tokens should enrich symbol classification only when:

- server is `Ready` or explicitly usable `Degraded`;
- response is current generation;
- payload is below cap;
- existing syntax/tree information is insufficient.

Do not include raw token streams in model context.

## Tests

- completion list and completion-item array variants;
- candidate truncation;
- semantic-token delta decoding;
- invalid legend index;
- empty token result;
- real-server expected completion labels.

## Acceptance Criteria

- Completion and semantic tokens are bounded and read-only.
- No edit from a completion item is applied.
- Decoding is deterministic and tested.

# Pass 6 — Add Preview-Only Rename

## Safety Rule

Rename must never directly modify files in Phase 4.

## Capability and Preparation

Support:

```text
textDocument/prepareRename
textDocument/rename
```

Add typed preparation result:

```rust
pub enum PrepareRenameResult {
    Range { range: Range, placeholder: Option<String> },
    DefaultBehavior,
    Unavailable(LspUnavailable),
}
```

Normalize server response variants.

## Rename Preview

Reuse or extend existing workspace-edit preview types:

```rust
pub struct RenamePreview {
    pub old_name: Option<String>,
    pub new_name: String,
    pub affected_files: Vec<FileEditPreview>,
    pub edit_count: usize,
    pub warnings: Vec<String>,
    pub server_generation: u64,
}
```

Validation:

- reject empty new name;
- cap file/edit counts;
- reject or warn on edits outside workspace root;
- preserve document versions from versioned edits;
- represent create/rename/delete resource operations as unsupported preview warnings unless already safely supported;
- never execute `workspace/executeCommand` as part of rename.

## Model-Facing Output

Expose compact summary plus bounded diff preview. Do not inject full large edits into context.

## Real-Server Checks

For each selected server advertising rename:

- prepare rename succeeds or documented fallback applies;
- rename edit references expected fixture files;
- current filesystem remains unchanged after preview;
- preview diff matches intended identifier changes.

## Acceptance Criteria

- Rename is preview-only end to end.
- Outside-root and resource-operation cases are safe.
- Real-server tests prove no file mutation.

# Pass 7 — Add Preview-Only Code Actions

## Safety Boundary

Code actions may contain:

```text
WorkspaceEdit
Command
both edit and command
lazy unresolved data
```

Phase 4 may preview edits but must not execute commands.

## Typed API

Add:

```rust
pub struct CodeActionSummary {
    pub title: String,
    pub kind: Option<String>,
    pub preferred: bool,
    pub disabled_reason: Option<String>,
    pub has_edit: bool,
    pub has_command: bool,
    pub diagnostics: Vec<DiagnosticSummary>,
}
```

Add operations:

```rust
list_code_actions
resolve_code_action
preview_code_action
```

Resolve only the selected action, not every action.

## Policy

- return summaries first;
- resolve lazily;
- preview embedded workspace edits;
- mark command-only actions non-previewable;
- never call `workspace/executeCommand`;
- reject commands smuggled through resolved actions;
- preserve action kind and diagnostics relationship.

## Capability Detail

Use advertised code-action kinds when present to classify support:

```text
quickfix
refactor
refactor.extract
refactor.inline
source.organizeImports
```

Do not assume all kinds are available from a boolean provider.

## Real-Server Checks

- TypeScript fixture: quick fix or organize-imports opportunity;
- gopls fixture: quick fix or source action when stable;
- clangd: optional/known limitation if deterministic fixture action is unavailable.

## Acceptance Criteria

- Commands are never executed.
- Edit-bearing actions produce safe previews.
- Action enumeration is bounded and deterministic.

# Pass 8 — Add Preview-Only Formatting

## Operations

Support:

```text
textDocument/formatting
textDocument/rangeFormatting
```

## Preview Types

Add:

```rust
pub struct FormattingPreview {
    pub file: PathBuf,
    pub edit_count: usize,
    pub before_hash: String,
    pub after_hash: String,
    pub diff: String,
    pub server_generation: u64,
}
```

Reuse text-edit application logic in memory only.

## Rules

- do not write files;
- apply edits to an in-memory snapshot;
- validate non-overlapping edit ordering or handle according to LSP edit semantics;
- cap diff size;
- preserve line endings where possible;
- reject edits outside the requested document;
- expose formatting options explicitly.

## Real-Server Checks

Each Tier 2 fixture includes intentionally unformatted source. Assert:

- advertised formatting returns edits;
- in-memory output matches basic expected formatting properties;
- file bytes on disk are unchanged.

## Acceptance Criteria

- Formatting remains preview-only.
- Edit application is deterministic and tested.

# Pass 9 — Adopt Higher-Level Evidence in Codegg Workflows

## Principle

Do not indiscriminately add every LSP response to model context. Add evidence only when it improves the current task.

## Semantic Context Policy

Extend semantic context policy with bounded optional sections:

```text
implementation targets
declaration target
read/write highlights
active signature
selected semantic classifications
workspace-symbol matches
```

Recommended triggers:

- implementation query for interface/trait navigation;
- declaration when definition points to generated/stub code;
- signature help near call sites;
- document highlights for local data-flow hints;
- workspace symbols for repo-wide symbol discovery;
- semantic tokens only as compact type/modifier labels.

## Tool Surface

Expose explicit operations through the LSP tool rather than automatically calling all of them.

Suggested operation names:

```text
lsp_declaration
lsp_implementation
lsp_document_highlights
lsp_signature_help
lsp_workspace_symbols
lsp_completion
lsp_semantic_tokens
lsp_rename_preview
lsp_code_actions
lsp_code_action_preview
lsp_format_preview
```

Follow existing naming conventions if different.

## Context Budgets

Add per-operation caps:

```text
locations
symbols
completion candidates
tokens
code actions
edit files
edit count
diff bytes
```

Record truncation explicitly.

## Health and Freshness

Every result should carry or derive:

```text
server ID
generation
operational state
freshness/age when applicable
truncation status
```

## Acceptance Criteria

- High-level evidence is opt-in/policy-driven.
- Mutation operations expose previews only.
- Agent-facing payloads are bounded.

# Pass 10 — Tier 2 CI and Compatibility Matrix

## Workflow

Extend the existing opt-in real-server workflow.

Use pinned versions for:

```text
gopls
Node.js
typescript
typescript-language-server
clangd/LLVM
```

Run one server per matrix job.

## Trigger Policy

Keep default CI network-free.

Tier 2 workflow may run on:

```text
workflow_dispatch
weekly schedule
changes under crates/egglsp/**, src/lsp/**, or workflow itself
```

Initially non-required.

## Artifacts

Upload:

```text
compatibility JSON
bounded stderr
fixture metadata
operation check summary
```

## Compatibility Status Documentation

Maintain a table:

```text
Server                     Tier   Platform   Status                    Known Limits
gopls                      2      Linux      experimental/passing      ...
typescript-language-server 2      Linux      experimental/passing      ...
clangd                     2      Linux      experimental/passing      ...
```

Do not promote to `passing` until required semantic assertions pass on pinned versions.

## Acceptance Criteria

- Tier 2 jobs are reproducible and isolated.
- Default CI remains unaffected.
- Reports clearly distinguish advertised support from semantic success.

# Pass 11 — Documentation and Final Verification

## Documentation

Update:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

Document:

- Tier 1 versus Tier 2 support;
- normalized capability model;
- advertised versus observed support;
- read-only versus preview-only operation policy;
- completion and semantic-token payload bounds;
- rename/code-action/formatting safety boundaries;
- compatibility report semantics;
- exact CI/test commands.

## Required Verification

### Existing regression suites

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p egglsp --features lsp-test-support --tests
cargo test --features lsp-test-support --test lsp_composite_stdio
cargo test --workspace --all-features
```

### Tier 1 real servers

```bash
cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- rust_analyzer --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- basedpyright --nocapture
```

### Tier 2 real servers

Use one filtered test per server:

```bash
cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- gopls --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- typescript --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- clangd --nocapture
```

## Mandatory Safety Tests

- rename preview does not modify disk;
- code-action preview does not execute command;
- formatting preview does not modify disk;
- outside-root edits are rejected/warned;
- edit and payload caps are enforced;
- unsupported operations return `LspUnavailable`;
- stale-generation results are not presented as current;
- Phase 3 restart and shutdown suites remain green.

# Exact Execution Order for a Smaller Model

1. Audit and correct capability normalization.
2. Add `gopls` profile and fixture.
3. Generalize the fixture harness using `gopls` as the first new consumer.
4. Add read-only declaration/implementation/highlight/signature APIs.
5. Add and validate `gopls` real-server checks.
6. Add TypeScript profile, fixture, and checks.
7. Add clangd profile, fixture, and checks.
8. Harden completion and semantic tokens.
9. Add rename preview.
10. Add code-action listing/resolution/preview.
11. Add formatting preview.
12. Integrate bounded evidence into Codegg tools/context.
13. Extend Tier 2 CI.
14. Update documentation and run full verification.

Do not begin preview-only mutation features before read-only Tier 2 compatibility is stable.

# Recommended Commit Sequence

```text
1. refactor(egglsp): normalize extended server capabilities accurately
2. feat(egglsp): add gopls compatibility profile and fixture
3. refactor(egglsp): generalize real-server operation fixtures
4. feat(egglsp): add declaration implementation highlight and signature APIs
5. test(egglsp): validate gopls semantic compatibility
6. feat(egglsp): add typescript-language-server profile and fixture
7. feat(egglsp): add clangd profile and fixture
8. feat(egglsp): bound completion and decode semantic tokens
9. feat(egglsp): add preview-only rename
10. feat(egglsp): add preview-only code actions
11. feat(egglsp): add preview-only formatting
12. feat(lsp): adopt bounded higher-level evidence in Codegg workflows
13. ci(lsp): add pinned Tier 2 compatibility matrix
14. docs(lsp): document Phase 4 compatibility and safety policy
```

# Final Phase 4 Checklist

## Compatibility

- [ ] gopls profile and pinned test pass.
- [ ] typescript-language-server profile and pinned test pass.
- [ ] clangd profile and pinned test pass.
- [ ] Tier 1 reports remain green.
- [ ] Advertised and observed support are distinct.

## Capability normalization

- [ ] Type hierarchy is not inferred from call hierarchy.
- [ ] Diagnostics support is represented accurately.
- [ ] New operations are capability-gated.
- [ ] Provider options are preserved where needed.

## Read-only operations

- [ ] Declaration normalized.
- [ ] Implementation normalized.
- [ ] Document highlights normalized.
- [ ] Signature help bounded.
- [ ] Workspace symbols semantically asserted.
- [ ] Completion bounded.
- [ ] Semantic tokens decoded safely.

## Preview-only operations

- [ ] Rename never writes files.
- [ ] Code actions never execute commands.
- [ ] Formatting never writes files.
- [ ] Workspace edits are root-bounded and capped.
- [ ] Diffs are bounded and deterministic.

## Workflow adoption

- [ ] Higher-level evidence is opt-in or policy-driven.
- [ ] Agent payloads are bounded.
- [ ] Health, generation, and truncation metadata are preserved.

## CI and docs

- [ ] Tier 2 versions are pinned.
- [ ] Default CI remains network-free.
- [ ] Compatibility artifacts upload.
- [ ] Documentation accurately scopes support.

# Handoff Result

After Phase 4, Codegg will move from a lifecycle-stable Tier 1 LSP integration to a broader, measured compatibility layer across Rust, Python, Go, TypeScript, and C/C++. It will expose higher-value semantic navigation and bounded completion/token evidence while keeping rename, code actions, and formatting strictly preview-only. This creates a safe base for later workflow automation without weakening the process, generation, and evidence-lifecycle invariants completed in Phase 3.
