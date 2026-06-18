# LSP Phase 4 Corrective Pass: Capability Truthfulness, Protocol Edge Cases, Tool Completion, and Tier 2 Validation

## Purpose

Complete the corrective work identified after the first Phase 4 implementation series ending at:

```text
e77b64951bd29c958e9580c4d1842aee1d7c38ac
```

The first Phase 4 pass successfully added broad functionality:

- Tier 2 profiles for `gopls`, `typescript-language-server`, and `clangd`;
- generalized real-server fixtures;
- declaration, implementation, document-highlight, workspace-symbol, and signature-help APIs;
- bounded completion and semantic-token decoding;
- preview-only rename, code actions, and formatting;
- partial LspTool adoption;
- Tier 2 CI jobs;
- extensive architecture and skill documentation.

The remaining work is corrective rather than additive. The main risks are false capability claims, profile overrides being lost at call time, push diagnostics being mislabeled as advertised support, incorrect protocol edge-case handling, incomplete model-facing exposure, weak real-server evidence, and an oversized `operations.rs` module that is becoming difficult to maintain.

This plan is tailored for a smaller implementation model. Execute each pass in order. Do not add new LSP methods until all correctness and validation gates in this plan pass.

## Phase 4 Closure Definition

Phase 4 is complete only when all of the following are true:

1. `OneOf<bool, Options>` providers treat `false` as unsupported.
2. Every capability-gated call uses one stored, override-aware normalized snapshot.
3. Profile-level observed capability overrides are preserved through service, operations, semantic-context, and tool layers.
4. Push diagnostics are represented as observed behavior, not falsely advertised support.
5. Null `prepareRename` is represented as not renameable.
6. Signature-help parameter offsets are decoded safely for non-ASCII labels and never panic.
7. Semantic-token decoding rejects arithmetic overflow and invalid modifier bits.
8. Capability-unknown state is distinct from supported and unsupported.
9. Rename, formatting, and workspace-symbol operations are exposed through `LspTool` with bounded output.
10. Preview DTOs detect stale base files or concurrent disk changes.
11. Tier 2 real-server tests actually run and produce inspectable passing artifacts.
12. Documentation calls Tier 2 compatibility experimental until those real-server runs pass.
13. The clangd CI installation is reproducible and does not use deprecated `apt-key`.
14. `operations.rs` is split into cohesive modules without changing the public façade.
15. Existing Phase 2 and Phase 3 suites remain green.

## Primary Files

```text
crates/egglsp/src/capability.rs
crates/egglsp/src/client.rs
crates/egglsp/src/compatibility.rs
crates/egglsp/src/diagnostics.rs
crates/egglsp/src/error.rs
crates/egglsp/src/lib.rs
crates/egglsp/src/operations.rs
crates/egglsp/src/service.rs
crates/egglsp/src/edit.rs
crates/egglsp/tests/real_server_smoke.rs
crates/egglsp/tests/production_protocol_stdio.rs
crates/egglsp/tests/production_service_stdio.rs
src/lsp/semantic_context.rs
src/tool/lsp.rs
.github/workflows/lsp-real-server.yml
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

Possible new modules:

```text
crates/egglsp/src/operations/mod.rs
crates/egglsp/src/operations/navigation.rs
crates/egglsp/src/operations/completion.rs
crates/egglsp/src/operations/semantic_tokens.rs
crates/egglsp/src/operations/rename.rs
crates/egglsp/src/operations/code_actions.rs
crates/egglsp/src/operations/formatting.rs
```

## Non-Goals

Do not implement:

- additional language servers;
- multi-root workspace support;
- pull-diagnostics workflow adoption beyond truthful capability modeling;
- automatic workspace-edit application;
- automatic code-action execution;
- completion edit application;
- semantic-token delta updates;
- arbitrary dynamic-registration support;
- TUI redesign;
- unrelated lifecycle changes.

# Pass 1 — Correct Boolean Provider Normalization

## Current Problem

Several LSP provider fields use `Option<OneOf<bool, Options>>`, but the current code frequently checks only `is_some()`. This treats:

```rust
Some(OneOf::Left(false))
```

as supported.

Affected or potentially affected fields include:

```text
document_highlight_provider
rename_provider
document_formatting_provider
document_range_formatting_provider
references_provider
definition_provider
declaration_provider
implementation_provider
workspace_symbol_provider
document_symbol_provider
hover_provider
folding_range_provider
selection_range_provider
inlay_hint_provider
call_hierarchy_provider
```

Audit every provider field in `ServerCapabilities`, not only the fields added in Phase 4.

## Required Helpers

Add small typed helpers in `capability.rs`:

```rust
fn one_of_bool_or_options_supported<T>(value: &Option<lsp_types::OneOf<bool, T>>) -> bool {
    match value {
        Some(lsp_types::OneOf::Left(enabled)) => *enabled,
        Some(lsp_types::OneOf::Right(_)) => true,
        None => false,
    }
}
```

For capability enums with different shapes, add explicit helpers rather than forcing generic conversions.

Examples:

```rust
fn code_action_provider_supported(
    value: &Option<lsp_types::CodeActionProviderCapability>,
) -> bool;

fn semantic_tokens_provider_supported(
    value: &Option<lsp_types::SemanticTokensServerCapabilities>,
) -> bool;
```

Do not infer support from `Option::is_some()` when the contained value may explicitly be false.

## Required Tests

Add unit tests for every relevant bool/options form:

```text
provider_none_is_unsupported
provider_false_is_unsupported
provider_true_is_supported
provider_options_is_supported
```

At minimum cover:

- document highlight;
- rename;
- document formatting;
- range formatting;
- references;
- definition;
- declaration;
- implementation.

## Acceptance Criteria

- No normalized capability returns true for `Some(false)`.
- Shared helpers are used consistently.
- Existing true/options behavior remains unchanged.

# Pass 2 — Store One Authoritative Normalized Capability Snapshot

## Current Problem

The profile-aware constructor exists:

```rust
LspCapabilitySnapshot::from_capabilities_with_override(...)
```

but `LspOperations` and `LspTool` rebuild snapshots from raw `ServerCapabilities` through `from_capabilities()`, discarding profile overrides.

This can make type hierarchy and any future observed capability override disappear during actual operation gating.

## Required Architecture

Store the final normalized snapshot once when the client is initialized.

Preferred location:

```rust
pub struct LspClient {
    raw_capabilities: RwLock<Option<ServerCapabilities>>,
    normalized_capabilities: RwLock<Option<LspCapabilitySnapshot>>,
}
```

or store it in service-level client metadata if that better matches the current architecture.

At initialization:

```text
receive ServerCapabilities
resolve profile overrides
build normalized snapshot once
store raw capabilities for low-level protocol needs
store normalized snapshot for every consumer
```

## Required APIs

Add or update:

```rust
pub async fn normalized_capabilities_for_key(
    &self,
    key: &str,
) -> Option<LspCapabilitySnapshot>;
```

Update all consumers:

```text
LspOperations
LspTool
SemanticContextCollector
health/compatibility reporting
real-server harness
```

They must retrieve the stored normalized snapshot rather than rebuild it.

## Remove Duplicate Reconstruction

Search for:

```text
LspCapabilitySnapshot::from_capabilities(
LspCapabilitySnapshot::from_capabilities_with_override(
```

Only initialization, tests, and compatibility-fixture construction should call these constructors directly.

## Tests

### `profile_override_survives_operation_gating`

Use a profile with `type_hierarchy = Some(true)` and raw capabilities lacking the field. Assert:

```text
stored normalized snapshot supports type hierarchy
LspOperations capability gate permits it
LspTool capability snapshot reports it
```

### `normalized_snapshot_is_not_rebuilt_without_override`

Use a counter/test hook if practical, or verify callers retrieve the exact stored value.

## Acceptance Criteria

- One authoritative normalized snapshot exists per client generation.
- No production caller silently rebuilds a weaker snapshot.
- Profile overrides survive end to end.

# Pass 3 — Separate Advertised and Observed Diagnostics Support

## Current Problem

Push diagnostics are currently marked supported whenever `text_document_sync` exists. Text synchronization is not an advertisement of `publishDiagnostics` behavior.

## Required Model

Replace the current booleans with explicit evidence semantics.

Preferred model:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityEvidence {
    Advertised,
    Observed,
    ProfileExpected,
    Unknown,
}

pub struct DiagnosticCapabilitySnapshot {
    pub pull: Option<CapabilityEvidence>,
    pub push: Option<CapabilityEvidence>,
}
```

A smaller change is acceptable:

```rust
pub supports_pull_diagnostics: bool,
pub observed_push_diagnostics: bool,
pub profile_expects_push_diagnostics: bool,
```

but do not keep naming text-sync as advertised push support.

## Observation Update

When a real `textDocument/publishDiagnostics` notification is received:

```text
mark push diagnostics observed for that client generation
```

Observation must be generation-scoped. A restarted client begins without observed push evidence until a notification arrives again, unless the profile expectation is explicitly used for readiness policy.

## Readiness Policy Interaction

For `WaitForDiagnosticsOrTimeout`:

- the policy may wait for a notification even before push is observed;
- a timeout does not change advertised support;
- a successful notification records observed support.

## Capability Gating

`LspSemanticOperation::Diagnostics` should be considered available when:

```text
pull advertised
or push observed
or profile explicitly expects push and the operation is a wait/read path
```

Document the exact policy.

## Compatibility Reports

Report separately:

```text
pull diagnostics advertised
push diagnostics observed
push diagnostics profile expected
```

## Tests

- text sync alone does not imply push diagnostics;
- first publish notification records observed support;
- observation is generation-scoped;
- pull provider remains advertised support;
- readiness timeout does not fabricate observation.

## Acceptance Criteria

- Advertised and observed diagnostics are never conflated.
- Reports and gating show the evidence source.

# Pass 4 — Make Capability Unknown Explicit

## Current Problem

`require_capability()` currently fails open when no snapshot exists. Unknown capability state is therefore treated as supported.

## Required Result Type

Add an explicit capability decision:

```rust
pub enum CapabilityDecision {
    Supported,
    Unsupported(LspUnavailable),
    Unknown {
        operation: LspSemanticOperation,
        reason: String,
    },
}
```

or map unknown to a structured `LspError::NotInitialized`.

Preferred behavior:

```text
client still initializing -> Unknown / NotInitialized
client ready with provider false/absent -> Unsupported
client ready with provider true/options -> Supported
```

Do not silently proceed when capability publication is incomplete.

## Optional Bounded Wait

A short wait for capability publication is acceptable:

```rust
wait_for_capabilities(timeout <= 1s)
```

but must remain bounded and cancellable.

## Update Callers

Every new operation should surface a clear error:

```text
capability not yet known
server does not advertise capability
operation supported
```

## Tests

- unknown does not proceed;
- ready unsupported returns `LspUnavailable`;
- ready supported proceeds;
- bounded wait resolves when initialization finishes.

## Acceptance Criteria

- Unknown is distinct from supported.
- No Phase 4 operation sends requests solely because capability state is absent.

# Pass 5 — Correct `prepareRename` Null Semantics

## Current Problem

A null `prepareRename` response currently maps to default behavior. In LSP, null means the position cannot be renamed.

## Required Enum

Change:

```rust
pub enum PrepareRenameResult {
    Range { range: Range, placeholder: Option<String> },
    DefaultBehavior,
    NotRenameable,
    Unavailable(LspUnavailable),
}
```

Do not attach `Range::default()` to `DefaultBehavior`.

## Required Conversion

```text
null -> NotRenameable
Range -> Range { placeholder: None }
RangeWithPlaceholder -> Range { placeholder: Some(...) }
DefaultBehavior { true } -> DefaultBehavior
```

If `default_behavior` is false, treat as protocol error or not renameable according to `lsp-types` semantics.

## Rename Preview Behavior

`rename_preview_typed()` must:

- return `NotRenameable` or a structured error before sending rename when prepare says no;
- not synthesize a rename preview from an empty default range;
- preserve server behavior when prepare capability is not advertised but rename is advertised.

## Tests

- null prepare response means not renameable;
- default behavior true is distinct;
- rename request is not sent after not-renameable result;
- unavailable prepare provider still allows direct rename only when rename itself is advertised and policy permits.

## Acceptance Criteria

- Invalid positions are never presented as renameable.

# Pass 6 — Make Signature Help Offset Handling Encoding-Safe

## Current Problem

`ParameterLabel::LabelOffsets` values are used as Rust byte indexes. This can panic on non-ASCII labels and can interpret protocol offsets incorrectly.

## Required Helper

Add a safe conversion helper:

```rust
fn lsp_units_to_byte_offset(
    text: &str,
    units: u32,
    encoding: PositionEncoding,
) -> Option<usize>;
```

Use the negotiated position encoding stored on the client. Default to UTF-16 when no explicit encoding is negotiated.

Supported encodings should match existing client negotiation:

```text
UTF-8
UTF-16
UTF-32 if already supported
```

## Parameter Label Conversion

```rust
fn resolve_parameter_label(
    sig_label: &str,
    label: &ParameterLabel,
    encoding: PositionEncoding,
) -> Result<String, LspError>;
```

Behavior:

- simple string label: return unchanged;
- valid offsets: map safely to byte boundaries;
- malformed offsets: return structured error or empty label with warning;
- never panic.

## Tests

Use labels containing:

```text
ASCII
é
漢字
emoji
mixed surrogate-pair content
```

Test UTF-8 and UTF-16 offsets separately.

## Acceptance Criteria

- No direct slicing with unchecked protocol offsets remains.
- Non-ASCII signatures are safe and correct.

# Pass 7 — Harden Semantic Token Decoding

## Current Problems

- unchecked `u32` additions can overflow;
- modifier bits beyond the legend are silently ignored;
- malformed payloads may be accepted partially.

## Required Changes

Use `checked_add()`:

```rust
let line = prev_line.checked_add(tok.delta_line).ok_or_else(...)?;
let start = if same_line {
    prev_start.checked_add(tok.delta_start).ok_or_else(...)?
} else {
    tok.delta_start
};
```

Validate modifier bitsets:

```text
bit set beyond legend length -> structured error
```

If the legend has more than 32 modifiers, document that the protocol bitset is limited to 32 bits and ignore only unreachable legend entries, not set bits.

Validate token length where possible:

- zero length may be legal but should be tested;
- malformed type index remains an error;
- cap output after successful decode, not before, so malformed earlier entries are not hidden by truncation.

## Tests

- line overflow;
- start overflow;
- modifier bit outside legend;
- valid empty token stream;
- valid multi-line delta stream;
- zero-length token handling.

## Acceptance Criteria

- Malformed streams fail deterministically.
- No arithmetic wraps.

# Pass 8 — Add Preview Base-Freshness Semantics

## Current Problem

Rename and formatting previews avoid writing files, but they do not fully distinguish Codegg non-mutation from an external file change that occurs while the request is in flight.

## Required DTO Fields

Add to preview DTOs:

```rust
pub base_hash: String,
pub final_disk_hash: String,
pub base_stale: bool,
pub source_versions: Vec<VersionedFileEvidence>,
```

A smaller shape is acceptable, but must explicitly signal stale base state.

Suggested type:

```rust
pub struct VersionedFileEvidence {
    pub file: PathBuf,
    pub content_hash: String,
    pub document_version: Option<i32>,
}
```

## Formatting Flow

```text
read base content/hash
request formatting edits
apply edits in memory
re-read disk
compare final disk hash to base hash
set base_stale if changed externally
```

Do not report a clean preview without warning when the base changed.

## Rename Flow

For every affected file that can be read:

- capture hash/version before preview construction;
- re-read before returning;
- record stale files;
- preserve open-document version where available.

Out-of-root files remain rejected or warned according to current policy.

## Tests

- external disk change during format request sets stale flag;
- external disk change during rename preview identifies affected file;
- unchanged disk keeps stale flag false;
- Codegg never writes disk.

## Acceptance Criteria

- Preview consumers can distinguish valid base from stale base.

# Pass 9 — Complete `LspTool` Exposure

## Current Problem

The crate APIs exist, but model-facing exposure is incomplete. Ensure these operations are present and tested:

```text
workspaceSymbol
renamePreview
formatPreview
```

Retain existing:

```text
declaration
implementation
documentHighlights
signatureHelp
completion
semanticTokens
codeActionSummaries
codeActionPreview
```

## Input Schema

Add or verify fields:

```text
query
new_name
formatting options if configurable
max_results / max_candidates / max_tokens / max_actions
```

Use existing naming conventions consistently.

## Output Requirements

Every response includes:

```text
operation
file_path when relevant
result_count
truncated
server generation when available
operational state or warning when degraded
preview stale-base status for mutation previews
```

## Safety

- rename preview never writes;
- formatting preview never writes;
- code-action preview never executes command;
- workspace symbols remain read-only;
- output caps are enforced before serialization.

## Tests

Add tool-level tests for:

- operation dispatch;
- required arguments;
- caps/truncation;
- unavailable capability;
- stale preview metadata;
- command-only code action rejection.

## Acceptance Criteria

- All planned Phase 4 operations are accessible through the tool façade.

# Pass 10 — Validate Observed Capability Overrides with Real Servers

## Current Problem

Observed overrides such as type hierarchy are hard-coded profile claims without direct version-scoped proof.

## Required Model

Extend override metadata:

```rust
pub struct ObservedCapabilityOverride<T> {
    pub value: T,
    pub evidence: String,
    pub tested_version: Option<String>,
}
```

A simpler shape is acceptable:

```rust
pub type_hierarchy: Option<bool>,
pub type_hierarchy_tested_version: Option<String>,
```

but evidence must be visible in compatibility reports.

## Real-Server Checks

For each profile that enables type hierarchy:

```text
rust-analyzer
gopls
clangd
```

add a real-server test that:

- prepares type hierarchy at a known fixture position;
- receives at least one expected item;
- queries subtypes or supertypes when applicable;
- records success/failure and server version.

If the real server does not support the operation at the pinned version, remove or narrow the override.

## Acceptance Criteria

- Every observed override has real-server evidence.
- Compatibility reports include version/provenance.

# Pass 11 — Run and Inspect Tier 2 Real-Server Jobs

## Required Local/CI Execution

Run all three pinned Tier 2 servers:

```bash
cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- gopls --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- typescript --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- clangd --nocapture
```

## Required Evidence

For each server, preserve:

```text
server version
compatibility JSON path
stderr tail
capability snapshot
required operation pass/fail summary
known limitations
```

Do not mark a server passing merely because the test binary launched.

## Required Semantic Assertions

### gopls

- diagnostics observed;
- definition/declaration or implementation target correct;
- workspace symbol present;
- completion contains expected label;
- rename preview affects expected files and does not write;
- formatting preview returns expected change and does not write.

### TypeScript

- project initializes from `tsconfig.json`;
- completion and signature help contain expected labels;
- document highlights identify expected occurrences;
- code action summary is present when fixture supports it;
- rename preview remains non-mutating.

### clangd

- compile database loads;
- declaration/implementation target expected files;
- document highlights work;
- formatting preview is non-mutating;
- type hierarchy check only if override remains enabled.

## Status Policy

Until all required assertions pass:

```text
Tier 2 status = experimental / unverified
```

After passing pinned jobs:

```text
Tier 2 status = experimental / passing on pinned version
```

Do not call Tier 2 generally stable.

## Acceptance Criteria

- Actual compatibility artifacts exist for all three servers.
- Documentation status matches evidence.

# Pass 12 — Make Tier 2 CI Reproducible

## clangd Installation

Replace deprecated `apt-key` use.

Preferred options:

1. checksum-verified LLVM release artifact;
2. signed keyring with `signed-by=` and exact package version;
3. pinned container image containing clangd 18.

Do not rely only on a moving major-version apt package.

## Version Recording

Every CI job must execute and record:

```text
gopls version
typescript-language-server --version
tsc --version
clangd --version
```

Store versions in report metadata or an adjacent artifact file.

## Workflow Behavior

Keep default CI network-free. Tier 2 jobs remain opt-in/path-triggered and non-required until stable.

## Acceptance Criteria

- No deprecated key installation.
- Exact tested versions are visible in artifacts.
- CI environment is reproducible enough to compare reports over time.

# Pass 13 — Split `operations.rs` into Cohesive Modules

## Goal

Reduce maintenance risk without changing the public `LspOperations` façade.

## Required Layout

Preferred structure:

```text
operations/mod.rs
operations/navigation.rs
operations/signature.rs
operations/completion.rs
operations/semantic_tokens.rs
operations/rename.rs
operations/code_actions.rs
operations/formatting.rs
operations/hierarchy.rs
operations/overlay.rs
```

A smaller split is acceptable. At minimum separate:

```text
navigation
completion + semantic tokens
rename + code actions + formatting
```

## Rules

- keep `pub struct LspOperations` in `operations/mod.rs`;
- use private extension methods or `impl LspOperations` blocks in submodules;
- preserve public function names and re-exports;
- move tests with their implementation when practical;
- do not alter behavior during the split;
- make one commit for mechanical movement before corrective changes if necessary.

## Verification

Use `git diff --stat` and targeted tests to ensure the split is behavior-neutral.

## Acceptance Criteria

- No single operations source file remains a multi-thousand-line aggregation point.
- Public APIs remain stable.

# Pass 14 — Documentation Reconciliation

## Required Changes

Update:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

Document:

- correct bool/options normalization;
- authoritative stored normalized snapshot;
- advertised versus observed diagnostics;
- capability unknown state;
- null prepare-rename semantics;
- encoding-safe signature offsets;
- semantic-token malformed-input handling;
- stale-base preview metadata;
- complete tool operation list;
- observed override provenance;
- actual Tier 2 test status;
- reproducible CI versions;
- new operations module layout.

## Remove Premature Completion Claims

Until Tier 2 real-server artifacts pass, change wording from:

```text
Phase 4 complete
```

to:

```text
Phase 4 corrective validation in progress; Tier 2 profiles and operations are implemented but compatibility remains experimental pending pinned real-server evidence.
```

After the final validation gate passes, use:

```text
Phase 4 capability and preview surfaces complete; Tier 2 compatibility passing on pinned versions and still experimental outside that matrix.
```

## Acceptance Criteria

- Documentation does not overstate compatibility.
- Status wording matches actual test evidence.

# Exact Execution Order for a Smaller Model

1. Fix bool/options capability normalization.
2. Store one authoritative override-aware capability snapshot.
3. Separate advertised and observed diagnostics support.
4. Make capability unknown explicit.
5. Fix null prepare-rename semantics.
6. Fix signature-help offset decoding.
7. Harden semantic-token decoding.
8. Add preview stale-base metadata.
9. Complete LspTool operation exposure.
10. Validate observed overrides against real servers.
11. Run and inspect all Tier 2 real-server jobs.
12. Make clangd CI installation reproducible.
13. Split `operations.rs` after behavior is stable.
14. Reconcile documentation and close Phase 4.

Do not start the operations-module split before passes 1-9 are green. Keep behavioral fixes easy to review first.

# Required Verification Matrix

## Focused capability tests

```bash
cargo test -p egglsp --lib capability::
```

## Operation tests

```bash
cargo test -p egglsp --lib operations::
```

## Full egglsp tests

```bash
cargo test -p egglsp --features lsp-test-support --tests
```

## Composite regression

```bash
cargo test --features lsp-test-support --test lsp_composite_stdio
```

## Tier 1 real-server regression

```bash
cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- rust_analyzer --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- basedpyright --nocapture
```

## Tier 2 real servers

```bash
cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- gopls --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- typescript --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- clangd --nocapture
```

## Workspace validation

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

# Mandatory Final Tests

- [ ] `Some(false)` providers are unsupported.
- [ ] Profile override survives operation and tool gating.
- [ ] Text sync alone does not imply push diagnostics.
- [ ] Publish notification records observed push support.
- [ ] Unknown capability does not fail open.
- [ ] Null prepare rename is not renameable.
- [ ] Non-ASCII signature labels do not panic.
- [ ] Semantic-token overflow is rejected.
- [ ] Invalid modifier bits are rejected.
- [ ] External disk changes set preview stale-base metadata.
- [ ] Workspace symbols are exposed through LspTool.
- [ ] Rename preview is exposed through LspTool.
- [ ] Formatting preview is exposed through LspTool.
- [ ] Code actions never execute commands.
- [ ] All three Tier 2 real-server artifacts exist.
- [ ] Observed overrides have real-server evidence.
- [ ] clangd CI uses reproducible installation.
- [ ] Phase 3 lifecycle suites remain green.

# Recommended Commit Sequence

```text
1. fix(egglsp): normalize false boolean providers as unsupported
2. refactor(egglsp): store authoritative override-aware capability snapshots
3. fix(egglsp): separate observed push diagnostics from advertised support
4. fix(egglsp): make unknown capability state explicit
5. fix(egglsp): correct prepare-rename null semantics
6. fix(egglsp): decode signature offsets with negotiated encoding
7. fix(egglsp): reject malformed semantic-token arithmetic and modifiers
8. feat(egglsp): add stale-base evidence to mutation previews
9. feat(lsp): expose remaining Phase 4 operations through LspTool
10. test(egglsp): validate observed capability overrides on real servers
11. ci(lsp): pin and record Tier 2 server environments
12. refactor(egglsp): split operations into cohesive modules
13. docs(lsp): reconcile Phase 4 compatibility status and guarantees
```

# Handoff Discipline for a Smaller Model

1. Implement one pass per commit when possible.
2. Run focused tests after every pass.
3. Do not mix refactoring with protocol behavior changes.
4. Do not mark a capability supported without inspecting boolean branches.
5. Do not rebuild normalized snapshots outside initialization/tests.
6. Do not claim push diagnostics before observation or explicit profile evidence.
7. Do not send a rename request after `NotRenameable`.
8. Do not slice Rust strings with raw protocol offsets.
9. Do not ignore arithmetic overflow from server payloads.
10. Do not expose mutation operations without preview-only guarantees.
11. Do not mark Tier 2 passing without real-server artifacts.
12. Keep Phase 3 lifecycle behavior untouched unless a regression test fails.

# Final Handoff Output

The implementation handoff must report:

```text
commits created
files changed per pass
capability normalization tests
stored snapshot propagation tests
diagnostics evidence tests
protocol edge-case tests
LspTool operation coverage
Tier 1 and Tier 2 real-server versions
compatibility artifact paths
CI workflow changes
workspace check, Clippy, and test results
remaining known limitations
```

After this plan passes, Phase 4 can be considered compatibility-correct for the pinned Tier 1 and Tier 2 matrix, with mutation-producing operations remaining preview-only and broader server compatibility still explicitly experimental outside the tested versions.
