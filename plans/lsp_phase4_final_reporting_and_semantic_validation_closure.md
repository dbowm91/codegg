# LSP Phase 4 Final Closure: Reporting Semantics, Real Capability Evidence, and Raw-vs-Checked API Boundaries

## Purpose

Close the remaining Phase 4 gaps after:

```text
873eb0b2b9e8734a02cf9f033fe599c5a655d90a
```

The production-facing Phase 4 architecture is now largely sound:

- capability snapshots are normalized and profile-aware;
- runtime-observed diagnostics are integrated;
- typed preview operations are mostly fail-closed;
- formatting and rename previews preserve root and stale-base safety;
- completion, semantic tokens, rename, code actions, and formatting are bounded and preview-only;
- Tier 2 profiles and real-server fixtures exist;
- version pinning is reproducible;
- signature-help semantics are now genuinely asserted.

The remaining work is concentrated in two areas:

1. one production behavior gap in rename-without-prepare support;
2. compatibility harness truthfulness and semantic coverage.

This plan is tailored for a smaller implementation model. Execute the passes in order. Do not add new LSP methods or language servers.

## Final Closure Definition

Phase 4 is closed when:

1. Rename works when `renameProvider` is supported but `prepareRename` is not.
2. Rename requires the `Rename` capability explicitly.
3. Typed checked APIs and raw unchecked protocol APIs are clearly separated and documented.
4. clangd implementation lookup is genuinely exercised or narrowly documented as a proven limitation.
5. TypeScript implementation expectations are internally consistent and semantically exercised or removed.
6. rust-analyzer’s type-hierarchy override has a real request trace or is removed.
7. `LspOperationCompatibility` is integrated into `LspCompatibilityReport`.
8. Unsupported, skipped, known-limited, failing, and passing checks are emitted as distinct statuses.
9. No skipped check is represented as `Passing`.
10. Code-action checks require a previewable edit when the fixture opts into code-action validation.
11. Real-server semantic-token checks run the strict decoder against the actual legend.
12. Tier 2 shutdown classifications are evidence-backed per server.
13. Phase 4 documentation reflects exactly what the pinned matrix proves.
14. Existing Phase 2 and Phase 3 regression suites remain green.

## Primary Files

```text
crates/egglsp/src/operations/rename.rs
crates/egglsp/src/operations/code_actions.rs
crates/egglsp/src/operations/semantic_tokens.rs
crates/egglsp/src/compatibility.rs
crates/egglsp/src/client.rs
crates/egglsp/tests/real_server_smoke.rs
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

## Non-Goals

Do not:

- add new servers;
- add new LSP methods;
- execute commands from code actions;
- automatically apply edits;
- redesign restart/process supervision;
- redesign the operations module layout;
- broaden dynamic registration support;
- change TUI behavior.

# Pass 1 — Fix Rename When `prepareRename` Is Unsupported

## Current Problem

`rename_preview_typed()` calls `prepare_rename_typed()` unconditionally. When the server supports rename but not prepare-rename, the typed prepare method returns `Unavailable`, and the rename path stops instead of issuing `textDocument/rename`.

This conflates:

```text
prepareRename unsupported
prepareRename returned null at an invalid position
```

These are distinct protocol states.

## Required Flow

Use this exact decision sequence:

```text
1. require Rename capability
2. inspect effective capability snapshot for PrepareRename
3. if PrepareRename supported:
     call prepareRename
     NotRenameable -> return empty structured preview, no rename request
     Range/DefaultBehavior -> continue
4. if PrepareRename unsupported:
     skip prepareRename and issue rename directly
5. if PrepareRename unknown:
     fail closed with NotInitialized
```

Recommended structure:

```rust
self.require_capability(file_path, LspSemanticOperation::Rename)
    .await?;

let decision = self
    .service
    .capability_decision_for_file(file_path, LspSemanticOperation::PrepareRename)
    .await?;

let prepared = match decision {
    CapabilityDecision::Supported => Some(
        self.prepare_rename_typed(file_path, line, column).await?
    ),
    CapabilityDecision::Unsupported(_) => None,
    CapabilityDecision::Unknown { .. } => {
        return Err(LspError::NotInitialized(...));
    }
};
```

Do not call `prepare_rename_typed()` solely to discover support.

## Old Name Semantics

When prepare-rename is unsupported:

```text
old_name = None
```

Do not fabricate a placeholder.

When prepare-rename returns `DefaultBehavior`, continue with `old_name = None`.

## Required Tests

```text
rename_supported_prepare_unsupported_sends_rename
rename_supported_prepare_unknown_sends_no_request
rename_unsupported_sends_no_prepare_or_rename
prepare_null_stops_before_rename
prepare_default_behavior_allows_rename
rename_capability_is_checked_explicitly
```

Use request counters to distinguish `prepareRename` from `rename` calls.

## Acceptance Criteria

- Rename support no longer depends on prepare-rename support.
- Unknown remains fail-closed.

# Pass 2 — Define Checked Versus Raw Operation APIs

## Current Problem

Typed model-facing methods are capability-gated, but lower-level public methods can directly send protocol requests:

```text
prepare_rename
rename_preview
code_actions
format_preview
```

This is acceptable only if the distinction is explicit.

## Required API Policy

Choose one of these approaches.

### Preferred

Rename raw methods with `_unchecked` suffix:

```text
prepare_rename_unchecked
code_actions_unchecked
format_preview_unchecked
rename_preview_unchecked
```

Keep typed checked methods as the normal public API.

### Lower-risk alternative

Keep names stable but add explicit documentation:

```text
Low-level protocol wrapper. Does not perform capability gating.
Callers outside egglsp internals should prefer the typed checked API.
```

Mark raw wrappers `#[doc(hidden)]` or `pub(crate)` when no external caller requires them.

## Audit

Search all call sites and classify each as:

```text
checked user/model-facing path
internal protocol helper
real-server harness raw request
legacy compatibility shim
```

No model-facing tool path may call an unchecked wrapper directly.

## Required Tests

A compile-time visibility test is not necessary. Add targeted call-path tests proving the LspTool and typed operations use checked APIs.

## Acceptance Criteria

- API consumers can clearly distinguish checked and unchecked behavior.
- No accidental bypass exists in model-facing paths.

# Pass 3 — Make TypeScript Implementation Coverage Real or Remove It

## Current Problem

The TypeScript fixture sets:

```text
implementation = true
implementation_position = None
```

The runner falls back to a normal definition/call-site position, which does not validate `textDocument/implementation` semantics.

## Preferred Fixture Upgrade

Add a real interface/class pair:

```typescript
interface Greeter {
    greet(name: string): string;
}

class Person implements Greeter {
    greet(name: string): string {
        return `Hello, ${name}`;
    }
}
```

Set `implementation_position` to the interface method or interface name.

Required assertion:

```text
returned implementation targets Person or Person.greet
returned file is src/main.ts or a dedicated implementation file
```

## Acceptable Alternative

Set:

```text
expected_capabilities.implementation = false
implementation_position = None
```

and document that TypeScript implementation lookup is not part of the pinned Phase 4 matrix.

Do not leave `true + None`.

## Required Tests

```text
typescript_implementation_fixture_is_consistent
typescript_implementation_returns_expected_target
```

## Acceptance Criteria

- Enabled implementation checks always have semantic targets.

# Pass 4 — Exercise clangd Implementation or Prove the Limitation

## Current Problem

The C++ fixture contains a correct virtual base and override but still disables implementation lookup.

## Required Investigation

Query from the declaration in `include/widget.hpp`:

```cpp
virtual int add(int a, int b) = 0;
```

Use the exact `add` identifier position.

If the harness currently assumes the primary URI, extend `LocationExpectation` or the implementation target type to carry a source file:

```rust
struct PositionTarget {
    pub file: PathBuf,
    pub position: Position,
}
```

Do not force all semantic requests through `primary_source`.

## Required Assertion

Expect one of:

```text
Widget::add declaration in widget.hpp
Widget::add definition in widget.cpp
```

depending on clangd’s response shape.

## Failure Policy

Only classify as `KnownLimitation` after:

- querying the correct header URI;
- verifying the position;
- confirming compile_commands coverage;
- capturing the raw response;
- testing the pinned clangd 18.1.8 binary.

If it still fails, store the evidence in the report and remove the broad implementation expectation.

## Required Tests

```text
clangd_implementation_from_header_declaration
clangd_implementation_known_limit_includes_raw_evidence
```

## Acceptance Criteria

- The fixture tests the correct file and symbol.
- Disabled coverage is evidence-backed, not assumed.

# Pass 5 — Add rust-analyzer Type-Hierarchy Evidence

## Current Problem

rust-analyzer retains a type-hierarchy override, but the Rust fixture does not exercise hierarchy requests.

## Fixture Upgrade

Add:

```rust
trait Greeter {
    fn greet(&self) -> String;
}

struct Person;

impl Greeter for Person {
    fn greet(&self) -> String {
        "hello".to_string()
    }
}
```

Add a `TypeHierarchyExpectation` targeting either the trait or implementing struct.

Exercise:

```text
textDocument/prepareTypeHierarchy
typeHierarchy/subtypes and/or supertypes
```

Assert the returned symbol includes `Person` or `Greeter` as appropriate.

## Failure Policy

If the pinned rust-analyzer version does not support the request:

- remove the override;
- remove the tested-version metadata;
- document the operation as unavailable in the pinned matrix.

## Required Tests

```text
rust_analyzer_type_hierarchy_prepare
rust_analyzer_type_hierarchy_follow_up
```

## Acceptance Criteria

- Every retained override has real operation evidence.

# Pass 6 — Integrate `LspOperationCompatibility` into Reports

## Current Problem

`LspOperationCompatibility` exists but is not stored in `LspCompatibilityReport`.

## Required Schema Change

Add:

```rust
#[serde(default)]
pub operation_support: Vec<LspOperationCompatibility>,
```

Keep backward compatibility with older report JSON.

## Harness Integration

Every semantic operation check should emit an operation record with:

```text
operation
advertised
exercised
request_succeeded
semantic_assertion_passed
requirement
known_limit
```

Examples:

```text
implementation
signatureHelp
workspaceSymbol
semanticTokens/full
renamePreview
formatPreview
codeAction
prepareTypeHierarchy
supertypes
subtypes
shutdown
```

## Relationship to Existing Checks

Keep `checks` for human-readable timeline/stage reporting.

Use `operation_support` for machine-readable semantics.

Do not derive operation records by parsing check names after the fact. Emit them at the operation call site.

## Required Tests

```text
operation_report_serializes_and_deserializes
old_report_without_operation_support_deserializes
advertised_not_exercised_is_recorded
semantic_failure_preserves_request_success
known_limit_preserves_failure_detail
```

## Acceptance Criteria

- Reports can answer whether a capability was advertised, exercised, and semantically validated.

# Pass 7 — Emit Real Skipped and Unsupported Statuses

## Current Problem

Unsupported checks are often represented as:

```rust
SmokeCheck::pass("... skipped: not supported", Optional, 0)
```

`SmokeCheck::status()` therefore reports them as `Passing`.

## Required Refactor

Replace `result: Result<(), String>` as the sole status source with an explicit status field:

```rust
struct SmokeCheck {
    name: String,
    status: CompatibilityCheckStatus,
    requirement: CompatibilityRequirement,
    detail: Option<String>,
    duration_ms: u64,
}
```

Add constructors:

```rust
SmokeCheck::passing(...)
SmokeCheck::failing(...)
SmokeCheck::skipped(...)
SmokeCheck::unsupported(...)
SmokeCheck::known_limit(...)
```

## Status Rules

```text
Passing = request exercised and semantic assertion passed
PassingWithKnownLimits = request exercised and failed/partial under documented limitation
Failing = required semantic/protocol failure
Skipped = fixture chose not to exercise
Unsupported = server did not advertise support
```

Unsupported must not be represented as skipped.

Skipped must not be represented as passing.

## Update Assertions

`assert_required_checks()` should:

- fail Required when not Passing;
- fail RequiredIfAdvertised when advertised and not Passing/PassingWithKnownLimits;
- ignore Unsupported only when not advertised;
- never infer semantics from check-name substrings such as `"skipped"`.

Remove `is_skipped_check()` string parsing.

## Required Tests

```text
unsupported_status_is_not_passing
skipped_status_is_not_passing
required_skipped_fails
required_if_advertised_unsupported_is_allowed_only_when_not_advertised
known_limit_is_distinct_from_passing
```

## Acceptance Criteria

- Status semantics are explicit and machine-readable.

# Pass 8 — Strengthen Code-Action Compatibility Assertions

## Current Problem

The harness currently passes code-action checks when:

```text
response is null
action list is empty
no action contains an edit
```

This does not validate the preview-only action surface.

## Fixture Expectation

Add:

```rust
struct CodeActionExpectation {
    pub range: Range,
    pub diagnostics: Vec<Diagnostic>,
    pub required_kind_prefix: Option<String>,
    pub min_edit_bearing_actions: usize,
}
```

The TypeScript fixture should contain a deterministic action opportunity, such as:

```text
missing import
unused import organize action
simple type quick-fix
```

Prefer an edit-bearing action that does not require command execution.

## Required Semantics

When the fixture opts into code-action validation:

```text
null -> failure
empty list -> failure
0 edit-bearing actions -> failure
command-only result -> exercised but not previewable; fail unless declared known limitation
>= required edit-bearing actions -> pass
```

Also invoke the actual preview normalizer on the selected action and verify no command execution occurs.

## Required Tests

```text
code_action_null_fails_when_expected
code_action_empty_fails_when_expected
code_action_command_only_is_not_previewable
code_action_edit_preview_passes
```

## Acceptance Criteria

- Passing means at least one safe preview can be built.

# Pass 9 — Decode Real Semantic Tokens in the Harness

## Current Problem

The smoke test only deserializes the raw token array. It does not exercise the strict decoder or legend validation.

## Required Flow

1. retrieve the normalized semantic-token legend from the effective capability snapshot;
2. request `textDocument/semanticTokens/full`;
3. deserialize the response;
4. run `decode_semantic_tokens()` using the actual legend;
5. require at least one decoded token when the fixture opts in;
6. report malformed indexes/modifiers as failures.

## Fixture Expectations

Add optional expected token types:

```rust
struct SemanticTokenExpectation {
    pub min_tokens: usize,
    pub expected_token_types: Vec<String>,
}
```

Examples:

```text
class
interface
function
method
variable
```

Use only types known to be stable for the pinned server.

## Required Tests

```text
real_semantic_tokens_decode_with_server_legend
semantic_tokens_missing_legend_is_unsupported
semantic_tokens_invalid_stream_is_failure
semantic_tokens_expected_type_present
```

## Acceptance Criteria

- Real compatibility runs exercise the same strict decoder used by production.

# Pass 10 — Make Shutdown Evidence Per-Server and Actionable

## Current Problem

All Tier 2 fixtures classify force-killed shutdown as `KnownLimitation`, using the generic detail `daemon mode`.

## Required Trace

Add a structured shutdown trace:

```rust
pub struct LspShutdownTrace {
    pub shutdown_request_sent: bool,
    pub shutdown_response_received: bool,
    pub exit_notification_sent: bool,
    pub writer_closed: bool,
    pub graceful_exit_observed: bool,
    pub force_kill_requested: bool,
    pub elapsed_ms: u64,
    pub stderr_tail: Vec<String>,
}
```

Store it in the report or as a dedicated operation detail.

## Per-Server Classification

For each pinned Tier 2 server:

```text
gopls
typescript-language-server
clangd
```

run the shutdown trace and choose:

```text
Required if graceful exit succeeds
KnownLimitation only if the exact pinned binary reproducibly requires force kill
Failing if the harness sequence is incomplete or writer closure fails
```

Do not use the phrase `daemon mode` unless the server is actually launched in a documented daemon mode.

## Timeout Reduction

After behavior is understood, reduce the 60-second graceful timeout to a tighter bounded value, such as 5–10 seconds.

## Required Tests

```text
shutdown_trace_records_all_protocol_steps
gopls_shutdown_classification
typescript_shutdown_classification
clangd_shutdown_classification
```

## Acceptance Criteria

- Shutdown status is evidence-backed for each server.

# Pass 11 — Reconcile Documentation and Phase Status

## Required Documentation

Update:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

Document:

- rename without prepare support;
- checked versus unchecked operation APIs;
- exact implementation coverage by server;
- type-hierarchy evidence by server;
- operation-level report schema;
- explicit skipped/unsupported semantics;
- code-action previewability requirements;
- semantic-token decoder validation;
- per-server shutdown results.

## Status Wording

Until all closure tests pass:

```text
Phase 4 final semantic-validation closure in progress.
```

After passing:

```text
Phase 4 complete for the pinned Tier 1 and Tier 2 matrix. Passing operations are both exercised and semantically asserted; skipped, unsupported, and known-limited operations are reported distinctly. Compatibility outside pinned versions remains experimental.
```

Do not claim clangd or TypeScript implementation coverage unless the corresponding fixture check is enabled and passing.

# Exact Execution Order for a Smaller Model

1. Fix rename without prepare-rename support.
2. Clarify raw versus checked APIs.
3. Repair TypeScript implementation coverage.
4. Exercise clangd implementation from the header declaration.
5. Add rust-analyzer type-hierarchy evidence or remove the override.
6. Integrate `LspOperationCompatibility` into reports.
7. Replace implicit pass/skip semantics with explicit statuses.
8. Strengthen code-action assertions.
9. Decode real semantic tokens.
10. Add per-server shutdown traces and classifications.
11. Re-run the pinned matrix.
12. Update documentation only after the matrix passes.

# Recommended Commit Sequence

```text
1. fix(egglsp): support rename without prepare-rename provider
2. docs(egglsp): distinguish checked and unchecked operation APIs
3. test(egglsp): add semantic TypeScript implementation fixture
4. test(egglsp): exercise clangd implementation from header declaration
5. test(egglsp): validate rust-analyzer type hierarchy or remove override
6. refactor(egglsp): integrate operation-level compatibility records
7. refactor(egglsp): emit explicit skipped unsupported and known-limit statuses
8. test(egglsp): require previewable code actions
9. test(egglsp): decode real semantic tokens with server legend
10. test(egglsp): capture evidence-backed shutdown traces
11. docs(lsp): close Phase 4 against the final semantic matrix
```

# Required Verification

## Focused production tests

```bash
cargo test -p egglsp --lib operations::rename
cargo test -p egglsp --lib operations::code_actions
cargo test -p egglsp --lib operations::semantic_tokens
cargo test -p egglsp --lib compatibility
```

## Full egglsp regression

```bash
cargo test -p egglsp --features lsp-test-support --tests
cargo test --features lsp-test-support --test lsp_composite_stdio
```

## Pinned real-server matrix

```bash
cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- rust_analyzer --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- basedpyright --nocapture

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

# Mandatory Final Checklist

- [ ] Rename works when prepare-rename is unsupported.
- [ ] Rename explicitly requires rename capability.
- [ ] Unknown prepare capability remains fail-closed.
- [ ] Checked and unchecked APIs are documented and separated.
- [ ] TypeScript implementation expectation has a real target or is disabled.
- [ ] clangd implementation is exercised from the correct header URI or evidence-backed as limited.
- [ ] rust-analyzer type-hierarchy override has real request evidence or is removed.
- [ ] `operation_support` is part of the report schema.
- [ ] Skipped checks emit `Skipped`.
- [ ] Unsupported checks emit `Unsupported`.
- [ ] Known limitations preserve failure detail.
- [ ] Code-action pass requires a previewable edit-bearing action.
- [ ] Real semantic tokens are decoded with the server legend.
- [ ] Shutdown classification is per server and trace-backed.
- [ ] No required operation is silently skipped.
- [ ] Tier 1 and Tier 2 pinned reports pass.
- [ ] Phase 3 lifecycle tests remain green.

# Final Handoff Output

The implementing model must report:

```text
commits created
rename capability decision flow
checked versus unchecked API audit
TypeScript implementation result
clangd implementation request URI/position and result
rust-analyzer type-hierarchy result
operation-level report examples
status-semantic tests
code-action previewability result
semantic-token decode result per server
shutdown trace per Tier 2 server
compatibility artifact paths
workspace check, Clippy, and test results
remaining known limitations
```

After this plan passes, Phase 4 can be closed without remaining production API, semantic-evidence, or compatibility-reporting qualifications for the exact pinned server matrix.
