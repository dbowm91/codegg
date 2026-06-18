# LSP Phase 4 Final Harness Evidence and Matrix Closure

## Purpose

Close the final remaining Phase 4 gaps after:

```text
4255015dc32d7e22bc1ac1fc48b83bf88f2ce401
```

The production API layer is now effectively complete:

- rename works when `prepareRename` is unsupported;
- rename and formatting model-facing paths use checked typed APIs;
- capability gating is fail-closed;
- previews are root-bounded and non-mutating;
- TypeScript, Rust, Go, and C++ fixtures contain real semantic relationships;
- explicit compatibility statuses exist;
- semantic tokens are decoded with the production decoder;
- operation-level compatibility records are present in the report schema.

The remaining work is entirely about compatibility-harness truthfulness and final matrix evidence. Current reports can still misclassify request success versus semantic failure, allow advertised required checks to be skipped, expect the wrong clangd implementation file, omit important operations from the machine-readable matrix, and treat shutdown behavior too generically.

This plan is tailored for a smaller implementation model. Execute passes in order. Do not add new LSP methods or servers.

## Final Closure Definition

Phase 4 is closed only when:

1. clangd implementation lookup accepts the actual override declaration/definition files.
2. Operation compatibility records are emitted at request sites with exact protocol and semantic outcomes.
3. `RequiredIfAdvertised` checks fail when advertised but skipped.
4. Type-hierarchy prepare, subtype, and supertype requests are represented distinctly.
5. A pinned TypeScript code-action fixture produces at least one safe edit-bearing preview.
6. Shutdown traces are structured, stored in reports, and classified per server.
7. Semantic-token coordinate validation uses negotiated LSP position encoding rather than UTF-8 byte length.
8. Rename smoke checks require a non-null preview when the fixture opts in.
9. The operation matrix includes diagnostics, references, hover, document highlights, shutdown, and hierarchy suboperations.
10. The complete pinned Tier 1 and Tier 2 matrix is actually executed and artifacts are preserved.
11. Documentation reflects only assertions proven by the final matrix.
12. Phase 2 and Phase 3 regression suites remain green.

## Primary Files

```text
crates/egglsp/src/compatibility.rs
crates/egglsp/src/position.rs
crates/egglsp/tests/real_server_smoke.rs
.github/workflows/lsp-real-server.yml
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

## Non-Goals

Do not:

- modify restart/lifecycle architecture;
- add new language servers;
- add new protocol methods;
- change model-facing tool schemas;
- apply workspace edits;
- execute code-action commands;
- refactor production operation modules unrelated to shared helpers.

# Pass 1 — Fix clangd Implementation Expected Files

## Current Problem

The clangd fixture correctly sends `textDocument/implementation` from:

```text
include/widget.hpp
WidgetBase::add declaration
```

but the generalized runner always expects returned locations in `fixture.primary_source`, which is `src/main.cpp`.

The semantically correct results are expected in:

```text
include/widget.hpp  — Widget::add override declaration
src/widget.cpp      — Widget::add definition
```

## Required Fixture Model

Add explicit implementation expectations:

```rust
pub struct ImplementationExpectation {
    pub source_file: PathBuf,
    pub position: Position,
    pub min_locations: usize,
    pub expected_files: Vec<PathBuf>,
    pub expected_label_substrings: Vec<String>,
}
```

Replace:

```text
implementation_position
implementation_source
```

with:

```text
implementation_expectation: Option<ImplementationExpectation>
```

If minimizing churn, add only:

```rust
implementation_expected_files: Vec<PathBuf>
```

but the typed expectation is preferred.

## clangd Configuration

Use:

```text
source_file = include/widget.hpp
position = WidgetBase::add identifier
expected_files = [include/widget.hpp, src/widget.cpp]
min_locations = 1
```

The check passes when at least one returned location matches either expected file.

## TypeScript and gopls

Use explicit expected files there as well:

```text
TypeScript -> src/main.ts
gopls -> main.go
```

Do not keep hidden primary-source assumptions.

## Tests

```text
clangd_implementation_accepts_header_or_definition
typescript_implementation_expected_file_is_explicit
gopls_implementation_expected_file_is_explicit
implementation_check_uses_fixture_expected_files
```

## Acceptance Criteria

- clangd is not rejected for returning the correct override location.
- No implementation check assumes `primary_source` implicitly.

# Pass 2 — Emit Operation Outcomes at Request Sites

## Current Problem

`checks_to_operation_support()` reconstructs machine-readable records from human-readable `SmokeCheck` names and statuses.

This loses distinctions such as:

```text
request succeeded but semantic assertion failed
request failed at protocol layer
response parsing failed
request was never sent
```

## Required Result Type

Introduce a typed internal result:

```rust
struct OperationCheckResult {
    check: SmokeCheck,
    operation: Option<LspOperationCompatibility>,
}
```

or return a tuple:

```rust
(SmokeCheck, Option<LspOperationCompatibility>)
```

Preferred helper:

```rust
fn operation_record(
    operation: impl Into<String>,
    advertised: bool,
    exercised: bool,
    request_succeeded: bool,
    semantic_assertion_passed: bool,
    requirement: CompatibilityRequirement,
    known_limit: Option<String>,
) -> LspOperationCompatibility
```

## Required Semantics

Examples:

### Protocol request succeeds, wrong file returned

```text
exercised = true
request_succeeded = true
semantic_assertion_passed = false
status = Failing
```

### Request times out

```text
exercised = true
request_succeeded = false
semantic_assertion_passed = false
```

### Server does not advertise capability

```text
advertised = false
exercised = false
request_succeeded = false
semantic_assertion_passed = false
status = Unsupported
```

### Fixture skips advertised operation

```text
advertised = true
exercised = false
status = Skipped
```

## Remove Name Parsing

Delete `checks_to_operation_support()` once all operation helpers emit records directly.

Do not infer operations by parsing check names or prefixes.

## Tests

```text
semantic_failure_preserves_request_success
protocol_failure_sets_request_succeeded_false
unsupported_operation_is_not_exercised
skipped_advertised_operation_is_recorded
known_limit_preserves_actual_protocol_outcome
```

## Acceptance Criteria

- Machine-readable operation outcomes are exact.
- Human-readable check names no longer drive report semantics.

# Pass 3 — Enforce Advertised Required Checks

## Current Problem

`assert_required_checks()` currently allows `Skipped` for `RequiredIfAdvertised` checks.

This permits:

```text
server advertises capability
fixture opted into requirement
request never sent
suite passes
```

## Required Assertion Policy

For each `RequiredIfAdvertised` operation:

```text
advertised = false + Unsupported -> allowed
advertised = true + Passing -> allowed
advertised = true + PassingWithKnownLimits -> allowed only when requirement explicitly KnownLimitation
advertised = true + Skipped -> failure
advertised = true + Failing -> failure
```

Use `operation_support` rather than only `checks` where possible.

## Required Checks

At minimum enforce for:

```text
implementation
declaration
signatureHelp
workspaceSymbol
semanticTokens
renamePreview
formatPreview
codeActions
typeHierarchy suboperations
completion
```

## Tests

```text
advertised_required_skipped_fails
unadvertised_unsupported_passes
advertised_known_limit_requires_explicit_known_limit_requirement
advertised_passing_succeeds
```

## Acceptance Criteria

- No advertised required operation can silently remain untested.

# Pass 4 — Represent Type Hierarchy as Three Operations

## Current Problem

The compatibility report uses one coarse `TypeHierarchy` record while the harness issues:

```text
textDocument/prepareTypeHierarchy
typeHierarchy/subtypes
typeHierarchy/supertypes
```

The coarse name matching can report `exercised = false` despite real requests.

## Required Records

Emit separate operation names:

```text
typeHierarchy/prepare
typeHierarchy/subtypes
typeHierarchy/supertypes
```

For every fixture target:

- prepare is required when hierarchy is enabled;
- subtype/supertype records depend on `check_subtypes` / `check_supertypes`;
- semantic assertions include expected item names.

Optional aggregate:

```text
typeHierarchy
```

may remain as a summary only if derived from the three explicit records.

## Tests

```text
type_hierarchy_prepare_recorded
type_hierarchy_subtypes_recorded
type_hierarchy_supertypes_recorded
aggregate_hierarchy_fails_when_required_suboperation_fails
```

## Acceptance Criteria

- Reports show exactly which hierarchy stage passed or failed.

# Pass 5 — Enable a Real Edit-Bearing Code Action Fixture

## Current Problem

The harness can require edit-bearing actions, but no pinned fixture currently sets:

```text
code_action_position = Some(...)
code_action_min_edit_bearing >= 1
```

Therefore code-action preview compatibility is not semantically proven.

## Preferred TypeScript Fixture

Add a deterministic edit-bearing action opportunity.

Preferred candidates:

```text
unused import with organize-imports/source action
missing import quick fix
simple type correction quick fix
```

Choose the most stable action under pinned TypeScript 5.5.4 and typescript-language-server 4.3.3.

Recommended path:

```typescript
import { add, Point, unusedValue } from "./helper";
```

Then request code actions over the unused import diagnostic/range and require an edit-bearing organize-imports or quick-fix action.

If diagnostics are needed in the request, pass the relevant diagnostic from the fixture rather than an empty list.

## Required Assertions

```text
response non-null
response non-empty
at least one edit-bearing action
preview normalization succeeds
no command executes
workspace unchanged
```

Command-only actions may be recorded separately but do not satisfy previewability.

## Tests

```text
typescript_code_action_returns_edit_bearing_action
code_action_preview_does_not_execute_command
code_action_preview_does_not_mutate_disk
code_action_command_only_does_not_satisfy_required_preview
```

## Acceptance Criteria

- The pinned matrix proves at least one safe code-action preview path.

# Pass 6 — Add Structured Shutdown Traces

## Current Problem

The harness computes partial shutdown booleans but does not store a structured trace. Known-limit details still use generic `daemon mode` wording.

## Required Report Type

Add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspShutdownTrace {
    pub shutdown_request_sent: bool,
    pub shutdown_response_received: bool,
    pub exit_notification_sent: bool,
    pub writer_flush_succeeded: bool,
    pub writer_closed: bool,
    pub graceful_exit_observed: bool,
    pub force_kill_requested: bool,
    pub force_kill_succeeded: bool,
    pub elapsed_ms: u64,
    pub stderr_tail: Vec<String>,
}
```

Add to `LspCompatibilityReport`:

```rust
#[serde(default)]
pub shutdown_trace: Option<LspShutdownTrace>
```

## Harness Instrumentation

Record each protocol and runtime step directly.

Do not infer `shutdown_response_received = true` solely from final enum variant unless the runtime result explicitly proves it.

## Per-Server Classification

For each pinned server:

```text
rust-analyzer
basedpyright
gopls
typescript-language-server
clangd
```

classify shutdown based on trace evidence.

Replace generic wording such as:

```text
force-killed (daemon mode)
```

with factual detail:

```text
shutdown response received, exit sent, writer closed, process remained alive for 8s, force-kill succeeded
```

## Timeout

Reduce the graceful wait from 60 seconds to a bounded value such as 8–10 seconds once tracing is correct.

Keep an absolute force-kill/reap bound.

## Tests

```text
shutdown_trace_records_protocol_steps
shutdown_trace_records_writer_close
shutdown_trace_records_force_kill
known_limit_detail_is_trace_based
required_shutdown_fails_on_forced_exit
```

## Acceptance Criteria

- Every shutdown result is evidence-backed.
- No unsupported `daemon mode` claim remains.

# Pass 7 — Make Semantic-Token Bounds Encoding-Aware

## Current Problem

The harness compares LSP character offsets against Rust UTF-8 byte length.

LSP offsets use negotiated position encoding, usually UTF-16.

## Required Shared Helper

Reuse or expose the existing production conversion helper used by signature help and text edits:

```rust
fn lsp_units_to_byte_offset(
    text: &str,
    units: u32,
    encoding: PositionEncoding,
) -> Option<usize>
```

For token validation:

1. convert token start units to a byte offset;
2. convert `start + length` units to a byte offset using checked addition;
3. reject invalid boundaries;
4. validate against line content.

Use the client’s negotiated position encoding.

## Fixture Coverage

Add at least one non-ASCII source line to a fixture that emits semantic tokens:

```text
Unicode identifier or string adjacent to tokenized code
```

The semantic token itself should remain predictable.

## Tests

```text
semantic_token_utf16_bounds_accept_valid_unicode
semantic_token_utf16_bounds_reject_invalid_units
semantic_token_utf8_bounds_work_when_negotiated
semantic_token_length_overflow_fails
```

## Acceptance Criteria

- Token coordinate validation matches negotiated LSP encoding.

# Pass 8 — Strengthen Rename Smoke Semantics

## Current Problem

A null rename response is currently counted as passing if disk remains unchanged.

That proves non-mutation but not preview functionality.

## Fixture Expectations

Add:

```rust
pub struct RenameExpectation {
    pub position: Position,
    pub new_name: String,
    pub min_edits: usize,
    pub expected_files: Vec<PathBuf>,
}
```

Replace the bare optional rename position.

## Required Semantics

When rename is opted in:

```text
null -> failure
malformed response -> failure
0 edits when min_edits > 0 -> failure
returned edit files must match expected_files
on-disk content must remain unchanged
```

If a fixture only wants non-mutation coverage, mark it explicitly optional and separate from semantic rename validation.

## Tests

```text
rename_null_fails_when_expected
rename_zero_edits_fails_when_minimum_required
rename_expected_files_match
rename_disk_unchanged
```

## Acceptance Criteria

- Passing rename checks prove a previewable edit exists.

# Pass 9 — Expand the Operation Matrix

## Current Problem

`operation_support` currently omits important operations.

## Required Additions

Add records for:

```text
diagnostics
references
hover
documentHighlight
definition
documentSymbol
shutdown
typeHierarchy/prepare
typeHierarchy/subtypes
typeHierarchy/supertypes
codeAction/list
codeAction/preview
rename/preview
formatting/preview
```

Keep names stable and documented.

## Operation Naming

Use protocol-like canonical names:

```text
textDocument/references
textDocument/hover
textDocument/documentHighlight
textDocument/implementation
workspace/symbol
textDocument/semanticTokens/full
textDocument/rename
textDocument/codeAction
textDocument/formatting
shutdown
```

For preview-only checks, add a semantic suffix only if needed:

```text
textDocument/rename#preview
```

Document the convention once.

## Tests

```text
operation_matrix_contains_all_exercised_checks
operation_names_are_unique
operation_records_round_trip_json
shutdown_operation_record_matches_trace
```

## Acceptance Criteria

- The report is a complete machine-readable semantic matrix.

# Pass 10 — Execute and Preserve the Full Pinned Matrix

## Required Runs

Run all pinned servers, not only compile the test binary:

```bash
cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- rust_analyzer_smoke --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- basedpyright_smoke --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- gopls_smoke --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- typescript_smoke --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- clangd_smoke --nocapture
```

## Required Artifacts

For each server preserve:

```text
exact server version
compatibility JSON
operation matrix
shutdown trace
stderr tail
fixture metadata
```

Upload through the existing workflow.

## Required CI Evidence

Record workflow run URL or run ID in the handoff output when available.

If GitHub Actions is unavailable, provide local artifact paths and exact commands.

## Acceptance Criteria

- All five pinned servers are actually executed.
- No required advertised operation is skipped.
- Artifact JSON contains operation and shutdown evidence.

# Pass 11 — Full Regression and Documentation Closure

## Regression Commands

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p egglsp --lib
cargo test -p egglsp --features lsp-test-support --tests
cargo test --features lsp-test-support --test lsp_composite_stdio
cargo test --workspace --all-features
```

## Documentation

Update:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

Document:

- exact clangd implementation expectation files;
- direct operation-result emission;
- advertised-required skip policy;
- hierarchy suboperation records;
- edit-bearing code-action evidence;
- shutdown trace schema and per-server results;
- encoding-aware semantic-token validation;
- semantic rename requirements;
- full operation matrix names;
- final pinned artifact locations.

## Status Wording

Before all matrix runs pass:

```text
Phase 4 API complete; final compatibility evidence validation in progress.
```

After all runs and artifacts pass:

```text
Phase 4 complete for the exact pinned Tier 1 and Tier 2 matrix. Required advertised operations are exercised and semantically asserted; skipped, unsupported, known-limited, protocol-failed, and semantic-failed outcomes are represented distinctly. Compatibility outside pinned versions remains experimental.
```

# Exact Execution Order for a Smaller Model

1. Add explicit implementation expected files and fix clangd.
2. Introduce request-site operation result emission.
3. Enforce advertised required operations against skipped states.
4. Split type-hierarchy records into prepare/subtypes/supertypes.
5. Add a deterministic TypeScript edit-bearing code action.
6. Add structured shutdown traces and reduce timeout.
7. Make semantic-token bounds encoding-aware.
8. Strengthen rename fixture expectations.
9. Expand operation matrix coverage.
10. Run and preserve the complete pinned matrix.
11. Run workspace regressions and reconcile documentation.

Do not update Phase 4 status to complete before Pass 10 succeeds.

# Recommended Commit Sequence

```text
1. test(egglsp): make implementation expectations file-explicit
2. refactor(egglsp): emit operation compatibility at request sites
3. test(egglsp): fail advertised required operations when skipped
4. refactor(egglsp): report hierarchy prepare subtype and supertype separately
5. test(egglsp): require an edit-bearing TypeScript code action
6. test(egglsp): persist structured shutdown traces per server
7. fix(egglsp): validate semantic-token ranges with negotiated encoding
8. test(egglsp): require semantic rename previews in opted-in fixtures
9. refactor(egglsp): complete the operation compatibility matrix
10. ci(lsp): run and upload the full pinned server matrix
11. docs(lsp): close Phase 4 against final compatibility evidence
```

# Mandatory Final Checklist

- [ ] clangd implementation accepts header declaration or source definition.
- [ ] Every operation record preserves real request success independently from semantic success.
- [ ] Advertised required skipped checks fail.
- [ ] Type hierarchy has distinct prepare/subtype/supertype records.
- [ ] TypeScript produces at least one edit-bearing code action preview.
- [ ] Shutdown traces are stored in reports.
- [ ] Shutdown known-limit wording is trace-based.
- [ ] Semantic-token bounds use negotiated encoding.
- [ ] Opted-in rename fixtures require non-null edits.
- [ ] Operation matrix includes diagnostics, references, hover, highlights, and shutdown.
- [ ] All five pinned server tests are executed.
- [ ] Compatibility artifacts are preserved.
- [ ] Phase 3 lifecycle regressions remain green.
- [ ] Documentation matches actual matrix evidence.

# Final Handoff Output

The implementing model must report:

```text
commits created
clangd implementation request and accepted files
operation-result emission architecture
advertised/required skip tests
hierarchy suboperation results
TypeScript code-action title/kind/edit count
shutdown trace per server
semantic-token encoding tests
rename semantic preview results
full operation matrix summary
exact pinned versions
compatibility artifact paths or workflow URLs
workspace check, Clippy, and test results
remaining known limitations
```

After this plan passes, Phase 4 can be closed without remaining compatibility-harness, semantic-evidence, or reporting qualifications for the exact pinned server matrix.
