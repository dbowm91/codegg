# LSP Phase 4 Final Consistency and Validation Closure

## Purpose

Close the remaining Phase 4 gaps after:

```text
07378349ebb45dd13164ff9c0111ddfc8d6920d3
```

The repository now has the intended Phase 4 architecture and most corrective work in place. The remaining issues are concentrated in consistency between capability policy, preview safety, compatibility reporting, fixture assertions, shutdown evidence, and documentation/version claims.

This plan is deliberately narrow and tailored for a smaller implementation model. Do not add new LSP features. Do not refactor Phase 3 lifecycle code unless a failing test proves a regression.

## Final Closure Definition

Phase 4 is complete when all of the following are true:

1. Every Phase 4 operation, including rename, formatting, and code actions, uses fail-closed capability gating.
2. No typed preview operation proceeds when capability state is unknown.
3. Formatting validates `allowed_root` before requesting or constructing a preview.
4. Formatting detects stale base state even when the server returns zero edits.
5. The real-server harness uses the same override-aware effective capability snapshot as production.
6. Every type-hierarchy override is backed by an actual real-server request or removed.
7. gopls implementation lookup is exercised against the interface fixture.
8. clangd implementation lookup is genuinely enabled and asserted against the virtual override fixture.
9. TypeScript implementation expectations are internally consistent and actually exercised or removed.
10. Tier 2 shutdown requirements are justified per server with evidence, not blanket-classified.
11. CI and documentation use the same exact server versions.
12. Compatibility reports distinguish protocol success, semantic success, skipped checks, and known limitations.
13. Existing Phase 2 and Phase 3 suites remain green.
14. Documentation marks Phase 4 complete only after the final pinned matrix passes.

## Primary Files

```text
crates/egglsp/src/operations/navigation.rs
crates/egglsp/src/operations/rename.rs
crates/egglsp/src/operations/formatting.rs
crates/egglsp/src/operations/code_actions.rs
crates/egglsp/src/service.rs
crates/egglsp/src/compatibility.rs
crates/egglsp/tests/real_server_smoke.rs
.github/workflows/lsp-real-server.yml
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

## Non-Goals

Do not:

- add new language servers;
- add new LSP methods;
- apply workspace edits automatically;
- execute `workspace/executeCommand`;
- redesign the operations module layout;
- redesign restart ownership or process supervision;
- broaden dynamic registration support;
- alter the TUI.

# Pass 1 — Route Every Operation Through Fail-Closed Capability Gating

## Current Problem

`LspOperations::require_capability()` is now fail-closed, but several Phase 4 operations bypass it and use optional snapshot checks:

```rust
if let Some(snapshot) = self.capability_snapshot_for_file(file_path).await {
    ...
}
```

When no snapshot exists, these methods proceed and send requests.

Affected methods include at least:

```text
prepare_rename_typed
rename_preview_typed indirectly through prepare
format_preview_typed
code_action_summaries
preview_code_action
```

Audit all Phase 4 operation modules for the same pattern.

## Required Changes

Use:

```rust
self.require_capability(file_path, LspSemanticOperation::DocumentFormatting)
    .await?;
```

and equivalent calls for:

```text
PrepareRename
Rename
CodeAction
DocumentFormatting
```

For `prepare_rename_typed()`:

- `Unsupported` may be converted into `PrepareRenameResult::Unavailable` if preserving the current return type is important;
- `Unknown` must remain `LspError::NotInitialized`;
- do not treat unknown as unavailable;
- do not send `prepareRename` when unknown.

For `rename_preview_typed()`:

- require rename support explicitly;
- require prepare-rename only when the server advertises it;
- if prepare-rename is unsupported but rename is supported, preserve direct-rename policy only if that is already documented and tested;
- do not issue a duplicate best-effort prepare request after a typed prepare result has already been obtained.

For code actions and formatting:

- no request may be sent before `require_capability()` succeeds.

## Required Tests

```text
prepare_rename_unknown_sends_no_request
rename_unknown_sends_no_request
formatting_unknown_sends_no_request
code_action_unknown_sends_no_request
unsupported_preview_operation_returns_unavailable
supported_preview_operation_sends_one_request
```

Use fake-server request counters to prove no protocol request was emitted.

## Acceptance Criteria

- All Phase 4 operations use the centralized service decision path.
- No typed preview method contains fail-open optional-snapshot gating.

# Pass 2 — Restore Root Validation in Typed Formatting

## Current Problem

`format_preview_typed()` accepts `allowed_root` but currently ignores it.

## Required Change

Before reading the file or sending the LSP request, validate:

```text
canonical file path is inside canonical allowed_root
```

Reuse the existing root-validation helper from the edit/preview layer. Do not create a second inconsistent path policy.

Required behavior:

```text
inside root -> continue
outside root -> LspError::PathOutsideRoot
allowed_root None -> continue
canonicalization failure -> structured path error
```

## Required Tests

```text
format_preview_rejects_outside_root
format_preview_accepts_inside_root
format_preview_root_check_happens_before_request
```

The final test must prove no server request is sent for an out-of-root path.

## Acceptance Criteria

- Typed formatting has the same root boundary as rename and code-action previews.

# Pass 3 — Fix Zero-Edit Formatting Stale Detection

## Current Problem

When the server returns no edits, `format_preview_typed()` computes `final_disk_hash` but hard-codes:

```rust
base_stale: false
```

## Required Change

Use:

```rust
let base_stale = final_disk_hash != before_hash;
```

in both zero-edit and non-zero-edit branches.

Prefer a single finalization helper to avoid duplicate logic:

```rust
fn finalize_formatting_preview(
    before_hash: String,
    after_hash: String,
    final_disk_hash: String,
    ...
) -> FormattingPreview
```

## Required Tests

```text
formatting_zero_edits_external_change_sets_base_stale
formatting_zero_edits_unchanged_file_not_stale
formatting_nonzero_edits_external_change_sets_base_stale
```

## Acceptance Criteria

- Staleness semantics are identical regardless of edit count.

# Pass 4 — Use Production-Equivalent Capability Snapshots in the Real-Server Harness

## Current Problem

The harness currently builds:

```rust
LspCapabilitySnapshot::from_capabilities(&server_caps, ...)
```

This discards profile overrides and runtime-observed state.

## Required Architecture

Preferred approach:

1. initialize the client with the same profile override path used by production;
2. retrieve the stored normalized snapshot from the client/service;
3. merge observed push diagnostics through the same effective snapshot accessor;
4. use that snapshot for checks and report output.

If the standalone harness cannot use `LspService`, add a small shared helper that applies:

```text
raw ServerCapabilities
profile observed_capabilities
observed push diagnostics
```

using exactly the same logic as production.

Do not duplicate capability derivation in test code.

## Required Tests

```text
real_server_snapshot_includes_profile_override
real_server_snapshot_includes_observed_push_diagnostics
real_server_report_matches_production_capability_decision
```

## Acceptance Criteria

- Compatibility reports and production operation gating use equivalent snapshots.

# Pass 5 — Add Real Type-Hierarchy Evidence or Remove Overrides

## Current Problem

Profiles claim type-hierarchy support through overrides, but the real-server smoke suite does not issue type-hierarchy requests.

## Required Real-Server Checks

For each profile with `type_hierarchy = Some(true)`:

```text
rust-analyzer
gopls
clangd
```

exercise:

```text
textDocument/prepareTypeHierarchy
typeHierarchy/supertypes and/or typeHierarchy/subtypes
```

Use fixture positions with explicit relationships:

```text
Rust: trait + implementing struct
Go: interface + concrete type
C++: virtual base + derived override
```

Required assertions:

```text
prepare returns at least one item
follow-up returns expected symbol or file
report records actual server version
```

## Failure Policy

If a pinned server does not support the operation:

- remove the override; or
- scope the override to only the server/version where the real check passes.

Do not retain an override based only on version metadata.

## Acceptance Criteria

- Every remaining type-hierarchy override has a passing real request trace.

# Pass 6 — Complete Implementation Fixtures and Assertions

## gopls

The Go fixture now includes `Greeter` and `Person`.

Required assertion:

```text
query textDocument/implementation at the Greeter interface or Greet method
expect Person or its Greet implementation
```

Confirm the configured position is exact and the returned file is `main.go`.

## clangd

The C++ fixture now includes `WidgetBase` and `Widget`, but the capability expectation still disables implementation.

Required changes:

```text
expected_capabilities.implementation = true
implementation_position points to WidgetBase::add declaration
expected file includes widget.hpp or widget.cpp as appropriate
```

If clangd 18.1.8 still fails:

- capture raw response and stderr;
- verify the position and compile database;
- try the base declaration and method identifier positions;
- only mark a known limitation after proving the fixture and request are correct.

Do not silently disable the assertion.

## TypeScript

The fixture currently sets `implementation = true` but has no implementation position.

Choose one:

### Preferred

Add a real interface + implementation:

```typescript
interface Greeter {
    greet(name: string): string;
}

class Person implements Greeter {
    greet(name: string): string { ... }
}
```

Set an implementation position and assert the class/method target.

### Acceptable

Set `implementation = false` and document that TypeScript implementation lookup is outside the current pinned matrix.

Do not keep an impossible expectation.

## Required Tests

```text
gopls_implementation_semantic_assertion
clangd_implementation_semantic_assertion
typescript_implementation_expectation_is_consistent
```

## Acceptance Criteria

- Every enabled implementation check has a valid target and assertion.

# Pass 7 — Tighten Signature-Help Semantics in the Harness

## Current Problem

The smoke harness treats a null `signatureHelp` response as passing even when the fixture expects signature help.

## Required Change

When the fixture explicitly opts into signature help:

```text
null response -> failure
0 signatures -> failure
non-empty signatures -> validate expected label substring when provided
```

Extend fixture expectation data:

```rust
struct SignatureHelpExpectation {
    position: Position,
    expected_label_substrings: Vec<String>,
}
```

Do not reuse a location-oriented expectation type for signature help.

## Required Tests

```text
signature_help_null_fails_when_required
signature_help_expected_label_passes
signature_help_wrong_label_fails
```

## Acceptance Criteria

- Semantic pass means the expected signature was actually returned.

# Pass 8 — Investigate and Narrow Tier 2 Shutdown Limitations

## Current Problem

All Tier 2 fixtures classify force-killed shutdown as a known limitation.

## Required Instrumentation

Record per server:

```text
shutdown request sent timestamp
shutdown response received timestamp
exit notification sent timestamp
writer flush result
stdin/write-half close result
runtime wait result
process exit event
stderr tail
```

Confirm the exact sequence:

```text
send shutdown
await response
send exit
flush
close writer/stdin
await authoritative process exit
force-kill only after deadline
```

Audit writer ownership for clones that keep stdin open.

## Required Classification

For each pinned server independently:

```text
graceful exit succeeds -> shutdown Requirement::Required
harness bug found -> fix and keep Required
proven upstream behavior -> KnownLimitation with evidence in report
```

Do not use the generic phrase `daemon mode` without evidence.

Reduce the current 60-second wait where appropriate after behavior is understood.

## Required Tests

```text
gopls_shutdown_trace
typescript_shutdown_trace
clangd_shutdown_trace
```

## Acceptance Criteria

- Shutdown status is evidence-backed per server.
- Blanket Tier 2 known-limitation classification is removed.

# Pass 9 — Reconcile Exact Versions Across CI, Reports, and Documentation

## Current Drift

The workflow currently installs:

```text
gopls v0.16.1
typescript-language-server v4.3.3
typescript v5.5.4
clangd v18.1.8
```

The README currently claims TypeScript language server v5.3.0.

## Required Decision

Choose one exact TypeScript server version and use it everywhere.

Preferred options:

```text
keep CI at 4.3.3 and revert docs
or
upgrade CI to 5.3.0, rerun the full matrix, and update all docs/reports
```

Do not update documentation without a passing real-server run.

Search all repository text for:

```text
4.3.3
5.3.0
18.1.3
18.1.8
```

Update:

```text
workflow
README
architecture/lsp.md
SKILL.md
AGENTS.md
profile tested-version metadata
compatibility report examples
```

## Acceptance Criteria

- One exact version set exists across code, CI, reports, and docs.

# Pass 10 — Clarify Compatibility Report Semantics

## Current Problem

A server can be reported as passing even when advertised operations fail under fixture-level `KnownLimitation` grading.

## Required Report Model

Add or clarify status dimensions:

```rust
pub enum CompatibilityCheckStatus {
    Passing,
    Unsupported,
    Skipped,
    PassingWithKnownLimits,
    Failing,
}
```

Ensure every check records:

```text
server advertised capability
fixture enabled check
request sent
semantic assertion passed
requirement level
known limitation reason
```

Preferred operation record:

```rust
pub struct LspOperationCompatibility {
    pub operation: String,
    pub advertised: bool,
    pub exercised: bool,
    pub request_succeeded: bool,
    pub semantic_assertion_passed: bool,
    pub requirement: CompatibilityRequirement,
    pub known_limit: Option<String>,
}
```

Do not label a skipped advertised operation as passing.

## Required Documentation Language

Use distinctions such as:

```text
protocol-compatible
semantically validated
advertised but fixture-limited
known limitation
not exercised
```

## Required Tests

```text
advertised_but_not_exercised_is_not_passing
known_limitation_preserves_failure_detail
semantic_failure_is_visible_in_report
unsupported_is_distinct_from_skipped
```

## Acceptance Criteria

- “Passing” means an exercised semantic assertion passed.

# Pass 11 — Re-run the Final Pinned Matrix

## Tier 1

```bash
cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- rust_analyzer --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- basedpyright --nocapture
```

## Tier 2

```bash
cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- gopls --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- typescript --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- clangd --nocapture
```

## Required Evidence Per Server

```text
exact version
capability snapshot source
implementation result where enabled
type-hierarchy result where overridden
signature-help semantic result where enabled
shutdown trace and classification
compatibility JSON path
stderr tail
```

## Acceptance Criteria

- No required check is silently skipped.
- No profile override lacks evidence.
- No operation marked passing failed semantically.

# Pass 12 — Full Regression and Documentation Closure

## Required Commands

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
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

Before the final matrix passes, use:

```text
Phase 4 final consistency validation in progress.
```

After it passes, use:

```text
Phase 4 complete for the pinned Tier 1 and Tier 2 matrix; compatibility outside those exact versions remains experimental.
```

Do not duplicate long version lists in multiple prose blocks unless generated from one source.

# Exact Execution Order for a Smaller Model

1. Replace optional snapshot checks with `require_capability()`.
2. Restore formatting root validation.
3. Fix zero-edit formatting stale detection.
4. Make the harness use effective override-aware capabilities.
5. Add type-hierarchy checks or remove overrides.
6. Enable and validate gopls/clangd implementation checks.
7. Resolve the TypeScript implementation mismatch.
8. Tighten signature-help assertions.
9. Instrument and classify shutdown per server.
10. Reconcile exact versions.
11. Clarify compatibility report semantics.
12. Run the final pinned matrix and full regression suite.
13. Update documentation only after evidence passes.

# Recommended Commit Sequence

```text
1. fix(egglsp): fail closed for rename formatting and code-action operations
2. fix(egglsp): restore formatting root and zero-edit stale checks
3. refactor(egglsp): use effective capabilities in real-server reports
4. test(egglsp): validate type hierarchy on pinned servers
5. test(egglsp): enable real Go C++ and TypeScript implementation assertions
6. test(egglsp): strengthen signature-help semantic checks
7. fix(egglsp): make Tier 2 shutdown classification evidence-based
8. ci(lsp): reconcile exact pinned server versions
9. refactor(egglsp): make compatibility report statuses semantically precise
10. docs(lsp): close Phase 4 against the final pinned matrix
```

# Mandatory Final Checklist

- [ ] Rename preview fails closed on unknown capability.
- [ ] Formatting preview fails closed on unknown capability.
- [ ] Code-action operations fail closed on unknown capability.
- [ ] Formatting rejects out-of-root files before sending a request.
- [ ] Zero-edit formatting detects external file changes.
- [ ] Real-server reports use effective override-aware snapshots.
- [ ] Every type-hierarchy override has a real request trace or is removed.
- [ ] gopls implementation is semantically asserted.
- [ ] clangd implementation is semantically asserted or evidence-backed as a limitation.
- [ ] TypeScript implementation configuration is internally consistent.
- [ ] Required signature help does not pass on null.
- [ ] Shutdown classification is per server and evidence-backed.
- [ ] CI and documentation use identical exact versions.
- [ ] Skipped or known-limit checks are not mislabeled as passing.
- [ ] Tier 1 and Tier 2 pinned reports pass.
- [ ] Phase 3 lifecycle suites remain green.

# Final Handoff Output

The implementing model must report:

```text
commits created
all fail-closed operation call sites
formatting root and stale tests
capability snapshot source used by harness
real type-hierarchy evidence per server
implementation result per Tier 2 server
signature-help assertion results
shutdown trace and classification per server
exact version matrix
compatibility artifact paths
workspace check, Clippy, and test results
remaining known limitations
```

After this plan passes, Phase 4 can be closed without remaining consistency, preview-safety, capability-evidence, or compatibility-reporting caveats for the exact pinned server matrix.
