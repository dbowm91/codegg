# LSP Phase 4 Final Closure: Capability Correctness, Preview Fidelity, and Tier 2 Harness Hardening

## Purpose

Close the remaining Phase 4 correctness and validation gaps after:

```text
c1825d9ca1123c4c79020cab24cd620fedc0aed3
```

The repository now has the broad Phase 4 surface in place:

- Tier 2 profiles for `gopls`, `typescript-language-server`, and `clangd`;
- override-aware normalized capability snapshots;
- observed push-diagnostics tracking;
- declaration, implementation, document-highlight, signature-help, completion, semantic-token, rename-preview, code-action-preview, formatting-preview, and workspace-symbol operations;
- complete LspTool discovery;
- preview-only mutation safety boundaries;
- split operations modules;
- pinned local Tier 2 smoke runs;
- CI jobs and compatibility artifacts.

The remaining work is narrow but important. Current issues can still produce false capability support, fail-open requests before initialization, stale or inaccurate preview hashes/diffs, and compatibility reports that pass because assertions were disabled or downgraded rather than genuinely exercised.

This plan is tailored for a smaller implementation model. Execute passes in order. Do not add new LSP features.

## Final Phase 4 Closure Definition

Phase 4 is closed when:

1. `CodeActionProviderCapability::Simple(false)` is unsupported.
2. Capability `Unknown` never silently proceeds.
3. Observed push diagnostics are merged into the authoritative capability decision path.
4. No production path rebuilds a weaker snapshot without profile overrides.
5. Formatting hashes and diffs are computed from raw server edits, never truncated preview DTOs.
6. Rename stale-base detection checks every affected file.
7. Go and C++ fixtures exercise real implementation relationships.
8. Type-hierarchy overrides are backed by actual real-server requests or removed.
9. Generic smoke-runner behavior contains no server-ID policy branches.
10. Forced shutdown behavior is investigated per server and only retained as a known limitation when proven.
11. Semantic-token modifier handling has one explicit, tested policy.
12. clangd CI uses an exact reproducible version source.
13. Tier 2 documentation reports only assertions actually exercised.
14. Phase 2 and Phase 3 regression suites remain green.

## Primary Files

```text
crates/egglsp/src/capability.rs
crates/egglsp/src/client.rs
crates/egglsp/src/service.rs
crates/egglsp/src/operations/navigation.rs
crates/egglsp/src/operations/formatting.rs
crates/egglsp/src/operations/rename.rs
crates/egglsp/src/operations/semantic_tokens.rs
crates/egglsp/src/edit.rs
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
- execute code-action commands;
- redesign lifecycle/restart logic;
- re-merge the operations modules;
- broaden dynamic registration support;
- redesign the TUI.

# Pass 1 — Finish Capability Truthfulness

## 1.1 Fix code-action `false`

Current code uses:

```rust
caps.code_action_provider.is_some()
```

Replace with explicit normalization:

```rust
fn code_action_provider_supported(
    provider: &Option<lsp_types::CodeActionProviderCapability>,
) -> bool {
    match provider {
        Some(lsp_types::CodeActionProviderCapability::Simple(enabled)) => *enabled,
        Some(lsp_types::CodeActionProviderCapability::Options(_)) => true,
        None => false,
    }
}
```

Use this for `supports_code_actions`.

Required tests:

```text
code_action_none_is_unsupported
code_action_simple_false_is_unsupported
code_action_simple_true_is_supported
code_action_options_is_supported
```

## 1.2 Audit remaining provider enums

Search all capability normalization for:

```text
.is_some()
is_some_and(...)
```

Confirm every field with a boolean-bearing enum inspects the boolean value.

Document direct-struct providers that are validly represented by `is_some()`.

## Acceptance Criteria

- No explicit false capability normalizes to true.
- Capability tests cover every bool-bearing enum used by Codegg.

# Pass 2 — Make Unknown Capability Fail Closed

## Current Problem

`require_capability()` logs `CapabilityDecision::Unknown` and returns `Ok(())`.

## Required Behavior

Choose one of these policies:

### Preferred

Return:

```rust
Err(LspError::NotInitialized(format!(
    "capability {} is not yet known for {}",
    op.as_str(),
    file_path.display(),
)))
```

### Acceptable alternative

Wait for capability publication with a short bounded timeout, then:

```text
Supported -> continue
Unsupported -> LspUnavailable
still Unknown -> NotInitialized
```

The wait must be:

- cancellable;
- no longer than 1 second;
- free of polling loops when a watch/notify primitive already exists.

## Required Tests

```text
unknown_capability_does_not_send_request
unknown_capability_returns_not_initialized
unknown_capability_resolves_after_publication
unsupported_capability_returns_unavailable
```

Use a request counter in the fake harness to prove no protocol request was sent while unknown.

## Acceptance Criteria

- Unknown never means supported.
- No Phase 4 request is sent before capability state is authoritative.

# Pass 3 — Unify Observed Diagnostics with the Authoritative Snapshot

## Current Problem

The client tracks observed push diagnostics in an atomic, but `LspService::capability_decision()` reads the stored snapshot without merging that observation.

`capability_snapshot_for_file_impl()` performs a local augmentation, creating two capability views.

## Required Architecture

Make one service-level accessor return the current authoritative snapshot:

```rust
pub async fn effective_capabilities_for_key(
    &self,
    key: &str,
) -> Option<LspCapabilitySnapshot>
```

Implementation:

1. clone the stored override-aware snapshot;
2. read generation-scoped observed state from the live client;
3. set `observed_push_diagnostics`;
4. recompute the coarse diagnostics alias;
5. return the effective snapshot.

Then update:

```text
capability_decision
LspOperations capability lookup
LspTool capability lookup
semantic context
compatibility reporting
```

Use this accessor everywhere.

## Remove raw reconstruction fallback

Delete the production fallback that rebuilds from raw `ServerCapabilities` through `from_capabilities()`.

If no normalized snapshot exists, return `Unknown`.

Constructors may remain in initialization and tests only.

## Generation Scope

Confirm observed push state resets naturally with each new `LspClient` generation.

Add tests:

```text
publish_diagnostics_updates_effective_snapshot
service_capability_decision_sees_observed_push
restart_resets_observed_push_state
no_raw_snapshot_reconstruction_in_production_path
```

## Acceptance Criteria

- One effective snapshot path exists.
- Diagnostics decisions and report output agree.
- Profile overrides and runtime observations coexist.

# Pass 4 — Compute Formatting Results from Raw Edits

## Current Problem

`FormattingPreview` reconstructs after-content from `WorkspaceEditPreview`, whose replacement strings and edit list may already be truncated.

This can corrupt:

```text
after_hash
diff
preview contents
```

## Required Refactor

Split formatting into two stages:

```rust
async fn request_formatting_edits(...) -> Result<Vec<TextEdit>, LspError>;

fn build_formatting_preview(
    before: &str,
    raw_edits: &[TextEdit],
    file_path: &Path,
    ...
) -> Result<FormattingPreview, LspError>;
```

Required order:

```text
read before content
request raw edits
apply raw edits in memory using the canonical edit application path
compute after hash from full after content
compute bounded diff from full after content
build bounded display DTO
re-read disk and compute stale-base state
```

Do not derive authoritative content from `replacement_preview`.

## Canonical edit application

Reuse one UTF-16-aware edit application helper from `edit.rs`.

Do not keep a second tolerant formatter-specific edit applier that silently returns the original on malformed edits.

Malformed edits must return a structured error.

## Required Tests

```text
formatting_long_replacement_hash_uses_full_text
formatting_more_than_100_edits_hash_uses_all_edits
formatting_diff_is_bounded_after_full_application
formatting_invalid_edit_returns_error
formatting_never_writes_disk
```

## Acceptance Criteria

- Truncation affects display only.
- `after_hash` reflects the full server edit result.

# Pass 5 — Complete Rename Stale-Base Detection

## Current Problem

The target file is compared before/after, but secondary affected files are only read after the request.

## Required Change

For every `FileEditPreview`:

```rust
let post_hash = hash(current_disk_content);
if post_hash != fp.original_hash {
    base_stale = true;
}
```

Record both values where useful:

```rust
pub struct VersionedFileEvidence {
    pub file: PathBuf,
    pub base_hash: String,
    pub final_disk_hash: String,
    pub document_version: Option<i32>,
    pub stale: bool,
}
```

If changing the DTO is too invasive, retain `content_hash` but add a per-file `stale` flag and document what the hash represents.

Use open-document version metadata when already available from the client.

## Required Tests

```text
rename_secondary_file_change_sets_base_stale
rename_target_file_change_sets_base_stale
rename_unchanged_files_are_not_stale
rename_version_evidence_covers_all_preview_files
```

## Acceptance Criteria

- Every affected file participates in stale detection.
- DTO documentation matches actual evidence collection.

# Pass 6 — Define Strict Semantic-Token Modifier Policy

## Current Problem

Overflow is rejected, but modifier bits beyond the legend are silently ignored while the corrective plan expected strict validation.

## Preferred Policy

Use strict validation in the low-level decoder:

```rust
let known_mask = if legend.token_modifiers.len() >= 32 {
    u32::MAX
} else {
    (1u32 << legend.token_modifiers.len()) - 1
};

let unknown_bits = bitset & !known_mask;
if unknown_bits != 0 {
    return Err(LspError::RequestFailed(...));
}
```

This makes compatibility tests expose malformed payloads.

If tolerant behavior is retained, return an explicit warning field rather than silently discarding bits.

## Required Tests

```text
modifier_bit_outside_legend_is_error
modifier_bits_within_legend_decode
empty_modifier_legend_rejects_nonzero_bitset
legend_over_32_entries_handles_protocol_limit
```

## Acceptance Criteria

- Policy is explicit, documented, and tested.
- No silent ambiguity remains.

# Pass 7 — Replace Weak Tier 2 Implementation Fixtures

## gopls fixture

Add a true interface relationship:

```go
type Greeter interface {
    Greet() string
}

type Person struct{}
func (Person) Greet() string { return "hello" }
```

Query implementation at the interface or method position and assert the concrete target.

## clangd fixture

Add a virtual base and override:

```cpp
struct WidgetBase {
    virtual int add(int a, int b) = 0;
};

struct Widget final : WidgetBase {
    int add(int a, int b) override;
};
```

Query implementation from the base declaration and assert the override target.

## Restore assertions

Re-enable implementation checks for both servers.

Do not mark them known limitations merely because the previous fixtures were structurally incapable of testing the operation.

## Required Tests

```text
gopls_implementation_targets_concrete_type
clangd_implementation_targets_override
```

## Acceptance Criteria

- Implementation capability is genuinely exercised.
- Passing status reflects semantic behavior, not disabled checks.

# Pass 8 — Validate or Remove Type-Hierarchy Overrides

## Required Real-Server Checks

For every profile that sets `type_hierarchy = Some(true)`:

```text
rust-analyzer
gopls
clangd
```

exercise:

```text
textDocument/prepareTypeHierarchy
typeHierarchy/supertypes or typeHierarchy/subtypes
```

Use fixtures with explicit type relationships.

Required report fields:

```text
server version
operation advertised/overridden
prepare result
follow-up result
semantic assertion
```

## Failure Policy

If a pinned server does not support the operation:

- remove the override; or
- scope the override to only servers/versions where the real test passes.

Version metadata alone is not evidence.

## Acceptance Criteria

- Every override has an actual passing request trace.
- No untested type-hierarchy override remains.

# Pass 9 — Remove Server-ID Policy Branches from the Generic Smoke Runner

## Current Problem

The generic harness branches on server IDs for:

```text
gopls warmup
clangd reference/hover limitations
implementation positions
shutdown limitation classification
```

## Required Fixture/Profile Data

Move these into typed expectation data:

```rust
struct RealServerFixture {
    warmup_after_ready: Duration,
    implementation_position: Option<Position>,
    reference_requirement: CompatibilityRequirement,
    hover_requirement: CompatibilityRequirement,
    shutdown_requirement: CompatibilityRequirement,
    shutdown_timeout: Duration,
    ...
}
```

or equivalent profile-level structures.

The generic runner should execute data, not inspect `server_id`.

Search and remove:

```text
if profile.server_id ==
matches!(profile.server_id
if server_id ==
```

except where selecting a fixture or test entrypoint is unavoidable.

## Acceptance Criteria

- No semantic pass/fail policy is hidden in server-name conditionals.
- Known limitations are declared in fixture/profile data.

# Pass 10 — Investigate Tier 2 Forced Shutdown

## Current Problem

All Tier 2 servers are allowed to force-kill after a long timeout under a blanket daemon-mode explanation.

## Required Investigation

For each server, capture:

```text
shutdown request response
exit notification write result
stdin close timing
process exit timing
stderr tail
child PID status
```

Verify harness sequence is exactly:

```text
send shutdown request
await response
send exit notification
flush writer
close stdin/write half
await process exit
force-kill only after deadline
```

Potential fixes to test:

- close stdin after `exit`;
- ensure writer clone ownership does not keep pipe open;
- wait on the authoritative runtime child;
- avoid background-task references retaining the writer;
- use server-specific documented launch flags only through profile data.

## Required Status Policy

For each server:

```text
graceful exit passes -> Required
proven upstream/server behavior -> KnownLimitation with evidence
harness defect -> fix and keep Required
```

Do not use one blanket rule for all Tier 2 servers.

## Required Tests

Add a focused shutdown test per Tier 2 server or report the smoke shutdown check separately.

## Acceptance Criteria

- Forced kill is either eliminated or individually justified.
- No lifecycle regression is masked by compatibility grading.

# Pass 11 — Pin clangd Exactly

## Required CI Change

Replace moving `clangd-18` installation with one of:

1. exact apt package version plus apt preferences;
2. checksum-verified LLVM release archive;
3. pinned container image digest.

Preferred for simplicity:

```text
checksum-verified official LLVM release archive
```

Record:

```text
requested version
actual clangd --version
artifact checksum
```

Do not rely on a moving major-version repository package.

## Acceptance Criteria

- Re-running CI later uses the same clangd build.
- Version metadata matches documentation.

# Pass 12 — Re-run Tier 2 Compatibility Without Weakened Assertions

## Required semantic matrix

### gopls

```text
initialize/readiness
diagnostics
definition/references/hover
workspace symbols
completion
rename preview
format preview
implementation via interface fixture
type hierarchy only if override remains
shutdown
```

### TypeScript

```text
initialize/readiness
diagnostics
completion
signature help
document highlights
code actions
rename preview
format preview where advertised
shutdown
```

### clangd

```text
initialize/readiness
diagnostics
declaration/references/hover
document highlights
format preview
implementation via virtual base fixture
type hierarchy only if override remains
shutdown
```

## Report policy

Every skipped or known-limitation item must include:

```text
why it is not required
whether the server advertised it
evidence supporting the limitation
```

## Required artifacts

Preserve:

```text
compatibility JSON
server versions
stderr tails
operation summary
shutdown trace
```

## Acceptance Criteria

- Passing status reflects exercised assertions.
- No disabled implementation checks remain.

# Pass 13 — Documentation Reconciliation

Update:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

Document:

- fail-closed unknown capability policy;
- effective capability snapshots;
- observed diagnostics integration;
- raw-edit formatting fidelity;
- per-file stale-base evidence;
- strict semantic-token modifier policy;
- real implementation fixtures;
- type-hierarchy evidence status;
- declarative smoke expectations;
- per-server shutdown results;
- exact clangd pin.

Until the final matrix passes, use:

```text
Phase 4 final closure validation in progress.
```

After it passes:

```text
Phase 4 complete for the pinned Tier 1 and Tier 2 matrix; compatibility outside pinned versions remains experimental.
```

# Exact Execution Order for a Smaller Model

1. Fix code-action boolean normalization.
2. Make unknown capability fail closed.
3. Centralize effective snapshots and observed diagnostics.
4. Refactor formatting to use raw edits.
5. Complete rename stale-file comparison.
6. Enforce semantic-token modifier policy.
7. Upgrade Go and C++ implementation fixtures.
8. Add real type-hierarchy checks or remove overrides.
9. Move server-specific smoke behavior into fixture/profile data.
10. Investigate shutdown per server.
11. Pin clangd exactly.
12. Run the full semantic matrix and collect artifacts.
13. Reconcile documentation.

Do not mix the fixture/harness changes with capability-core changes in one commit.

# Recommended Commit Sequence

```text
1. fix(egglsp): normalize disabled code-action providers correctly
2. fix(egglsp): fail closed on unknown capability state
3. refactor(egglsp): centralize effective capability snapshots and diagnostics evidence
4. fix(egglsp): build formatting previews from raw edits
5. fix(egglsp): validate stale base for every rename file
6. fix(egglsp): enforce semantic-token modifier validation policy
7. test(egglsp): add real Go and C++ implementation fixtures
8. test(egglsp): validate type-hierarchy overrides on pinned servers
9. refactor(egglsp): make real-server smoke policy data-driven
10. fix(egglsp): close Tier 2 servers gracefully or document proven limits
11. ci(lsp): pin clangd to an exact reproducible build
12. docs(lsp): close Phase 4 against the final compatibility matrix
```

# Required Verification

## Focused tests

```bash
cargo test -p egglsp --lib capability::
cargo test -p egglsp --lib operations::
cargo test -p egglsp --features lsp-test-support --tests
cargo test --features lsp-test-support --test lsp_composite_stdio
```

## Tier 1 regression

```bash
cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- rust_analyzer --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- basedpyright --nocapture
```

## Tier 2 final matrix

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

# Mandatory Final Checklist

- [ ] Code-action `Simple(false)` is unsupported.
- [ ] Unknown capability sends no request.
- [ ] Effective snapshot merges observed diagnostics.
- [ ] No production raw-snapshot fallback remains.
- [ ] Formatting uses full raw edits for hash and diff.
- [ ] Secondary rename files participate in stale detection.
- [ ] Invalid semantic-token modifier bits follow documented policy.
- [ ] gopls implementation is tested against a real interface.
- [ ] clangd implementation is tested against a real virtual override.
- [ ] Every type-hierarchy override has real request evidence.
- [ ] Generic smoke runner contains no server-ID policy branches.
- [ ] Shutdown classification is per server and evidence-backed.
- [ ] clangd build is exactly reproducible.
- [ ] Tier 1 and Tier 2 reports pass with no weakened required assertions.
- [ ] Phase 3 lifecycle suites remain green.

# Final Handoff Output

The implementing model must report:

```text
commits created
capability normalization changes
unknown-state behavior
snapshot/diagnostics integration changes
formatting raw-edit tests
rename stale-base tests
semantic-token policy and tests
new Go/C++ fixture structure
type-hierarchy evidence per server
removed server-ID branches
shutdown result per server
exact clangd version/checksum
Tier 1/Tier 2 artifact paths
workspace check, Clippy, and test results
remaining known limitations
```

After this plan passes, Phase 4 can be closed without remaining capability-truthfulness, preview-fidelity, or compatibility-harness qualifications for the pinned matrix.
