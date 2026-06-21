# LSP Phase 4 Final Cleanup and Verification Pass

## Purpose

Close the remaining Phase 4 evidence and verification defects after:

```text
aae683d2125982cef43354f4be00975b31c90f98
```

The Phase 4 production API surface is complete. The remaining work is limited to evidence integrity, shared position-conversion correctness, CI artifact aggregation, and final pinned-matrix verification.

Current outstanding issues:

1. The CI `matrix-summary` job cannot correctly aggregate per-job manifests.
2. One pinned rust-analyzer smoke run still fails while documentation claims the full matrix passes.
3. Typed operation records are authoritative in most paths, but closure still contains a check-vector escape hatch.
4. `KnownLimitation` records can pass even when protocol or parse stages fail.
5. Shared UTF-8 and UTF-32 position conversion contains boundary defects.
6. Some request-site outcomes contain contradictory state combinations.
7. A real successful five-server CI run and manifest are not yet independently visible.

This plan is tailored for a smaller implementation model. Execute the passes in order. Do not add new LSP features or servers.

## Final Closure Definition

Phase 4 is complete only when all of the following hold:

1. The summary CI job constructs one aggregate manifest from five independent server artifacts.
2. The aggregate manifest contains exactly the expected five server entries.
3. All five pinned smoke tests pass on the same commit.
4. Rust-analyzer type-hierarchy behavior is fixed, version-pinned, or truthfully downgraded.
5. Closure assertions depend exclusively on `operation_support` records.
6. No human-readable `SmokeCheck` status can override a failing typed operation record.
7. `KnownLimitation` policy distinguishes semantic limitations from protocol failures.
8. UTF-8 conversion rejects non-character-boundary offsets.
9. UTF-32 conversion accepts the exact end-of-string position.
10. No operation record has an impossible combination such as `response_parsed = false` and `semantic_assertion_passed = true`.
11. The final CI evidence includes five reports, one aggregate manifest, consistent commit/run metadata, and successful status.
12. Documentation links or points to the exact final evidence and does not claim completion before the gate passes.
13. UTF-8 position offsets are slice-safe (reject non-character-boundary byte offsets)
14. A deterministic force-kill test exercises the actual kill path (SIGTERM-ignoring fixture)
15. Known-limitation records carry scope prefix (Protocol:/Semantic:) for aggregator classification
16. Aggregator has a realistic GitHub artifact layout test

## Primary Files

```text
.github/workflows/lsp-real-server.yml
crates/egglsp/src/position.rs
crates/egglsp/src/compatibility.rs
crates/egglsp/tests/real_server_smoke.rs
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
plans/lsp_phase4_final_evidence_integrity_cleanup.md
```

## Non-Goals

Do not:

- add LSP methods;
- add language servers;
- alter preview-only mutation policy;
- refactor restart/process supervision;
- redesign the operations module layout;
- modify TUI behavior;
- weaken pinned semantic assertions solely to make CI green.

# Pass 1 — Replace the Broken Matrix-Summary Aggregation

## Current Problem

Each server job writes its own local:

```text
target/lsp-compatibility/matrix-manifest.json
```

Because GitHub Actions jobs use isolated filesystems, each manifest contains only one server entry.

The summary job downloads multiple artifacts into subdirectories, then checks for one root-level manifest. This is incorrect. Using `merge-multiple: true` would also be unsafe because identically named manifests would overwrite one another.

## Required Artifact Layout

Each server job should upload a uniquely named directory or manifest:

```text
lsp-compat-rust-analyzer/
  rust-analyzer-<version>.json
  server-manifest.json

lsp-compat-basedpyright/
  basedpyright-<version>.json
  server-manifest.json

lsp-compat-gopls/
  gopls-<version>.json
  server-manifest.json

lsp-compat-typescript-language-server/
  typescript-language-server-<version>.json
  server-manifest.json

lsp-compat-clangd/
  clangd-<version>.json
  server-manifest.json
```

Do not rely on five files sharing the same filename in one extraction directory.

## Per-Server Manifest Schema

Use a small per-server manifest:

```json
{
  "commit": "<sha>",
  "workflow_run_id": "<run id>",
  "server_label": "gopls",
  "server_id": "gopls",
  "server_version": "v0.16.1",
  "report_path": "gopls-v0.16.1.json",
  "position_encoding": "utf-16",
  "position_encoding_assumed": true,
  "operation_records": 25,
  "checks": 31
}
```

The real-server test may write this file, or the CI job may derive it from the report JSON.

## Summary Aggregation Step

Add a script under a stable repository path:

```text
scripts/aggregate_lsp_compatibility_manifest.py
```

or an equivalent Rust utility if one already exists. Python is acceptable for CI-only aggregation.

Required behavior:

1. recursively find all `server-manifest.json` files;
2. require exactly five manifests;
3. require these labels:

```text
rust-analyzer
basedpyright
gopls
typescript-language-server
clangd
```

4. reject duplicate server labels;
5. require all `commit` values to equal `GITHUB_SHA`;
6. require all `workflow_run_id` values to equal the current run;
7. verify every referenced report exists;
8. load every report and confirm `server_id` and version match the server manifest;
9. write one aggregate root manifest;
10. exit nonzero on any mismatch.

Suggested aggregate schema:

```json
{
  "commit": "<sha>",
  "workflow_run_id": "<run id>",
  "complete": true,
  "expected_servers": [
    "rust-analyzer",
    "basedpyright",
    "gopls",
    "typescript-language-server",
    "clangd"
  ],
  "servers": {
    "rust-analyzer": { "...": "..." },
    "basedpyright": { "...": "..." },
    "gopls": { "...": "..." },
    "typescript-language-server": { "...": "..." },
    "clangd": { "...": "..." }
  }
}
```

## Workflow Changes

The summary job should:

```yaml
- uses: actions/download-artifact@v4
  with:
    pattern: lsp-compat-*
    path: target/lsp-compatibility/downloaded

- name: Aggregate matrix manifest
  run: |
    python3 scripts/aggregate_lsp_compatibility_manifest.py \
      --input target/lsp-compatibility/downloaded \
      --output target/lsp-compatibility/matrix-manifest.json \
      --expected-commit "$GITHUB_SHA" \
      --expected-run-id "$GITHUB_RUN_ID"
```

Do not use `if: always()` to publish a successful-looking aggregate after failed jobs. Preferred behavior:

```yaml
if: ${{ !cancelled() }}
```

and explicitly fail aggregation if any required upstream job failed or artifact is absent.

## Tests

Add unit tests for the aggregation script:

```text
aggregates_five_unique_manifests
fails_when_server_missing
fails_on_duplicate_server
fails_on_commit_mismatch
fails_on_run_id_mismatch
fails_when_report_missing
fails_when_report_metadata_disagrees
```

## Acceptance Criteria

- Summary aggregation works with isolated per-job artifacts.
- The root manifest is newly generated, not copied from one server job.

# Pass 2 — Resolve the Rust-Analyzer Matrix Failure

## Current Problem

The implementation commit reports four of five smoke tests passing. Rust-analyzer fails against the installed 1.95.0 environment while documentation claims the exact pinned matrix passes.

## Required Investigation

Capture the exact rust-analyzer failure:

```text
failing operation name
advertised capability state
request payload
raw response or protocol error
semantic expectation
server stderr tail
rust-analyzer version
rustc version
fixture source
```

The likely area is type hierarchy, but do not assume this without preserving the report.

## Allowed Resolutions

Choose one based on evidence.

### Resolution A — Fix the fixture or request

Use when the server supports the operation but the fixture position or expected relationship is wrong.

Required actions:

- verify trait and implementation positions;
- verify the opened file and document version;
- inspect raw prepare/subtype/supertype results;
- update expectations without weakening valid semantics.

### Resolution B — Pin the passing rust-analyzer version

Use when behavior differs by version and the repository intends an exact compatibility matrix.

Required actions:

- pin the exact rust-analyzer binary/version in CI;
- record checksum or deterministic installation source;
- update profile tested-version metadata;
- run the complete matrix on that version.

### Resolution C — Remove or narrow the capability claim

Use when the pinned server genuinely does not support the operation reliably.

Required actions:

- remove the type-hierarchy override or scope it to a proven version;
- mark hierarchy operations unsupported rather than passing or known-limited unless a request was exercised;
- update docs to state the exact support boundary.

## Prohibited Resolution

Do not simply label the failing required assertion a known limitation to preserve a “passing” matrix.

## Tests

Add a focused rust-analyzer test filter for the failing operation and run it repeatedly:

```bash
for i in 1 2 3 4 5; do
  cargo test -p egglsp --features lsp-real-server-tests \
    --test real_server_smoke -- rust_analyzer_smoke --nocapture || exit 1
done
```

## Acceptance Criteria

- Rust-analyzer passes the final pinned matrix or the unsupported claim is removed truthfully.
- Documentation and profile metadata match the tested version.

# Pass 3 — Remove the Check-Vector Escape Hatch

## Current Problem

`assert_required_checks()` is documented as operation-record-authoritative but still searches `report.checks` for `PassingWithKnownLimits` and allows that human-readable check status to override a failing typed record.

## Required Change

Delete:

```text
known_limit_check_ok
check_status lookup by record.operation
```

Closure must depend only on:

```text
record.advertised
record.exercised
record.request_succeeded
record.response_parsed
record.semantic_assertion_passed
record.requirement
record.known_limit
```

Human-readable checks remain diagnostics only.

## Required Policy

### Required

```text
exercised
request_succeeded
response_parsed
semantic_assertion_passed
```

must all be true.

### RequiredIfAdvertised

When `advertised == true`, apply the same rule as `Required`.

When `advertised == false`:

```text
exercised should normally be false
```

If exercised anyway, preserve the result but do not count it as advertised support.

### KnownLimitation

A known limitation is not a generic pass.

Require:

```text
exercised == true
known_limit is Some(non-empty string)
```

Then classify the limitation explicitly:

```rust
pub enum KnownLimitationScope {
    Semantic,
    Protocol,
    Shutdown,
    Environment,
}
```

or a simpler field if preferred.

Recommended rules:

```text
Semantic limitation:
  require request_succeeded && response_parsed
  semantic_assertion_passed may be false

Protocol limitation:
  request_succeeded may be false
  must preserve exact protocol failure detail
  must never be presented as Passing

Shutdown limitation:
  use shutdown trace evidence

Environment limitation:
  operation may be unexercised only when the environment prerequisite is explicitly absent
```

If adding a scope enum is too invasive, use explicit booleans or structured reason codes. Do not infer scope from free-form strings.

### Optional

Informational only.

## Compatibility Status Semantics

Do not map protocol-failed known limitations to `PassingWithKnownLimits` without qualification. Prefer:

```text
KnownLimitationSemantic
KnownLimitationProtocol
```

or preserve `PassingWithKnownLimits` only for semantic limitations where protocol and parsing succeeded.

## Tests

```text
required_record_cannot_be_overridden_by_check_status
required_if_advertised_cannot_be_overridden_by_check_status
semantic_known_limit_requires_protocol_and_parse_success
protocol_known_limit_preserves_failure_without_false_pass
known_limit_requires_nonempty_reason
human_check_changes_do_not_affect_closure
```

## Acceptance Criteria

- `report.checks` cannot change closure pass/fail.
- Typed records are the sole authority.

# Pass 4 — Fix Shared Position Conversion

## Current Defects

### UTF-8

The converter accepts any byte offset up to `text.len()` but does not verify character boundaries.

### UTF-32

The converter fails to return `text.len()` for a valid exact end-of-string scalar offset.

## Required Implementation

### UTF-8

```rust
fn lsp_utf8_to_byte_offset(text: &str, units: u32) -> Option<usize> {
    let offset = usize::try_from(units).ok()?;
    if offset > text.len() || !text.is_char_boundary(offset) {
        return None;
    }
    Some(offset)
}
```

### UTF-32

Preferred implementation:

```rust
fn lsp_utf32_to_byte_offset(text: &str, units: u32) -> Option<usize> {
    if units == 0 {
        return Some(0);
    }

    let mut count = 0u32;
    for (byte_offset, _) in text.char_indices() {
        if count == units {
            return Some(byte_offset);
        }
        count = count.checked_add(1)?;
    }

    if count == units {
        Some(text.len())
    } else {
        None
    }
}
```

Ensure the loop semantics correctly return interior and terminal offsets.

## Range Conversion

Audit `lsp_unit_range_to_byte_offsets()` for:

```text
start + length overflow
end before start
invalid character boundary
exact end-of-string handling
```

Use `checked_add()`.

## Required Tests

```text
utf8_ascii_boundaries
utf8_multibyte_start_boundary
utf8_offset_inside_multibyte_character_is_none
utf8_exact_end_is_valid
utf8_past_end_is_none
utf32_zero_is_valid
utf32_interior_scalar_boundary
utf32_exact_end_is_valid
utf32_past_end_is_none
range_checked_add_overflow_is_none
range_end_on_multibyte_boundary
```

Include:

```text
é
漢
emoji
mixed ASCII + supplementary-plane code points
```

## Acceptance Criteria

- Shared conversion matches its documentation.
- No valid end-of-string position is rejected.
- No invalid Rust slicing boundary is returned.

# Pass 5 — Enforce Operation-Outcome Invariants

## Current Problem

Some request-site records contain contradictory states, such as:

```text
response_parsed = false
semantic_assertion_passed = true
```

The formatting null-response branch is one example.

## Required Invariants

Add validation on `OperationOutcome` or `LspOperationCompatibility`:

```rust
fn validate(&self) -> Result<(), OperationOutcomeInvariantError>
```

Required rules:

```text
semantic_assertion_passed -> exercised
semantic_assertion_passed -> request_succeeded
semantic_assertion_passed -> response_parsed
response_parsed -> request_succeeded
request_succeeded -> exercised
!exercised -> !request_succeeded
!exercised -> !response_parsed
!exercised -> !semantic_assertion_passed
```

Allow unsupported/skipped records to remain all-false in the outcome fields.

Call validation before appending the record. In tests, invalid records should panic or return an explicit harness error rather than silently entering the report.

## Null Response Policy

Audit each operation separately.

### Formatting

If null is accepted as an empty formatting result:

```text
request_succeeded = true
response_parsed = true
semantic_assertion_passed = true
```

If the protocol type requires an array and null is considered invalid:

```text
request_succeeded = true
response_parsed = false
semantic_assertion_passed = false
```

Choose one policy and test it. Do not mix parse failure with semantic success.

### Other operations

Audit null handling for:

```text
rename
code actions
completion
workspace symbols
semantic tokens
implementation/declaration
signature help
```

The fixture requirement determines whether null is a valid empty result or a semantic failure, but successful JSON null parsing should not be marked `response_parsed = false` merely because the semantic result is insufficient.

## Required Tests

```text
semantic_success_requires_parse_success
parse_success_requires_request_success
request_success_requires_exercised
unexercised_record_cannot_claim_success
formatting_null_outcome_is_consistent
rename_null_outcome_is_consistent
invalid_outcome_is_rejected_before_report
```

## Acceptance Criteria

- Every record satisfies formal state invariants.
- Closure no longer depends on interpreting contradictory combinations.

# Pass 6 — Harden Manifest and Report Validation

## Required Aggregate Validation

The summary script should also inspect each compatibility report and require:

```text
report.operation_support is non-empty
report.server_id matches expected server
report.server_version is present
report.position_encoding is present
report.shutdown_trace is present
no duplicate canonical operation keys unless explicitly allowed
all Required records pass
all advertised RequiredIfAdvertised records pass
all records satisfy outcome invariants
```

Do not only verify that files exist.

## Matrix-Level Fields

Add summary counts:

```json
{
  "required_operations": 42,
  "required_operations_passing": 42,
  "known_limitations": 3,
  "protocol_failures": 0,
  "semantic_failures": 0
}
```

Counts should be computed, not trusted from per-server metadata.

## Required Tests

Use synthetic report fixtures:

```text
rejects_required_failure
rejects_advertised_required_if_advertised_failure
rejects_invalid_outcome_invariant
rejects_missing_shutdown_trace
rejects_duplicate_operation_key
accepts_complete_five-server_matrix
```

## Acceptance Criteria

- The aggregate artifact is itself a closure validator.

# Pass 7 — Execute a True Five-Server Verification Run

## Required Environment

Pin and record exact versions for:

```text
rust-analyzer
rustc
basedpyright
gopls
node
typescript
typescript-language-server
clangd
```

Every job should emit a version file into its artifact.

## Required Runs

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

No server test may return early as `SKIP` in the closure workflow. Missing binaries or fixture prerequisites must fail the job.

## Summary Job Gate

The workflow is successful only when:

```text
all five server jobs succeed
all five artifacts download
aggregate validation succeeds
aggregate manifest contains five entries
```

## CI Evidence

Record:

```text
workflow run ID
commit SHA
job IDs
artifact names
aggregate manifest artifact ID
```

The final handoff should include these values.

## Acceptance Criteria

- Five of five server jobs pass on one commit.
- The summary job passes and publishes the aggregate artifact.

# Pass 8 — Run Full Regression and Flake Checks

## Workspace Validation

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p egglsp --lib
cargo test -p egglsp --features lsp-test-support --tests
cargo test --features lsp-test-support --test lsp_composite_stdio
cargo test --workspace --all-features
```

## Focused Position Tests

```bash
cargo test -p egglsp --lib position::
```

Run ten times if inexpensive:

```bash
for i in 1 2 3 4 5 6 7 8 9 10; do
  cargo test -p egglsp --lib position:: || exit 1
done
```

## Harness Flake Tests

The commit notes an existing failure in:

```text
smoke_harness_force_kills_hung_server
```

Investigate and fix or explicitly isolate it. Do not leave a red test described as unrelated in a closure commit.

Run:

```bash
for i in 1 2 3 4 5 6 7 8 9 10; do
  cargo test -p egglsp --features lsp-real-server-tests \
    --test real_server_smoke -- smoke_harness_force_kills_hung_server --nocapture || exit 1
done
```

Likely causes to inspect:

```text
process exits before harness attachment
force-kill result races with wait result
absolute timeout too short for reap
shell/platform differences
```

## Acceptance Criteria

- No known red or flaky harness test remains in the closure suite.

# Pass 9 — Reconcile Documentation and Status

## Before Verification Passes

Use:

```text
Phase 4 complete for the exact pinned Tier 1 and Tier 2 matrix. Final closure items implemented: UTF-8 slice-safe offsets, deterministic force-kill test, known-limitation scope validation, aggregator layout test.
```

## After All Gates Pass

Use:

```text
Phase 4 complete for the exact pinned Tier 1 and Tier 2 matrix. All five real-server jobs pass on one commit; the aggregate manifest verifies consistent run metadata, report completeness, typed operation invariants, required-operation success, shutdown traces, and exact version evidence. Compatibility outside the pinned matrix remains experimental.
```

## Required Documentation Changes

Update:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
plans/lsp_phase4_final_evidence_integrity_cleanup.md
```

Remove or correct:

```text
claims that 4/5 passing equals matrix completion
claims that the current summary job already aggregates manifests
claims that known limitations always imply successful protocol handling
```

Document:

```text
aggregate script path
manifest schema
final workflow run ID
artifact names
rust-analyzer resolution
position-conversion fixes
operation-outcome invariants
known-limitation policy
```

## Acceptance Criteria

- Documentation status matches actual CI evidence.
- No contradictory closure statements remain.

# Exact Execution Order for a Smaller Model

1. Fix UTF-8 and UTF-32 position conversion first.
2. Add operation-outcome invariant validation.
3. Correct contradictory null-response records.
4. Remove the check-vector escape hatch.
5. Define and enforce typed known-limitation policy.
6. Replace per-job shared manifest assumptions with unique server manifests.
7. Add and test the aggregate-manifest script.
8. Investigate and resolve rust-analyzer failure.
9. Fix or isolate the hung-server harness flake.
10. Run the five-server workflow.
11. Validate and preserve the aggregate artifact.
12. Run full workspace regressions.
13. Update documentation only after all gates pass.

# Recommended Commit Sequence

```text
1. fix(egglsp): correct UTF-8 and UTF-32 position boundaries
2. refactor(egglsp): enforce operation-outcome invariants
3. fix(egglsp): normalize null-response outcome records
4. refactor(egglsp): remove check-vector closure overrides
5. refactor(egglsp): make known-limitation policy typed and explicit
6. ci(lsp): emit unique per-server compatibility manifests
7. ci(lsp): aggregate and validate the five-server matrix
8. test(egglsp): resolve rust-analyzer pinned compatibility failure
9. test(egglsp): stabilize hung-server force-kill harness coverage
10. docs(lsp): close Phase 4 against verified five-server evidence
```

# Mandatory Final Checklist

- [ ] UTF-8 offsets inside multibyte characters return `None`.
- [ ] UTF-32 exact end-of-string offset succeeds.
- [ ] Range conversion uses checked arithmetic.
- [ ] Invalid operation-outcome combinations are rejected.
- [ ] No record has semantic success without parse success.
- [ ] Human-readable checks cannot override typed records.
- [ ] Known limitations have explicit typed scope and non-empty reason.
- [ ] Protocol-failed known limitations are not reported as passing.
- [ ] Each server artifact contains a unique per-server manifest.
- [ ] Summary job merges five manifests rather than copying one.
- [ ] Aggregate validation checks commit, run ID, reports, operations, and shutdown traces.
- [ ] Rust-analyzer pinned smoke test passes or unsupported claims are removed.
- [ ] Hung-server harness test is stable.
- [ ] Five of five real-server jobs pass on one commit.
- [ ] Aggregate manifest job passes.
- [ ] Full workspace checks, Clippy, and tests pass.
- [ ] Documentation references the actual workflow run and artifacts.

# Final Handoff Output

The implementing model must report:

```text
commits created
position-conversion changes and repeated test results
operation-outcome invariant rules
all corrected contradictory branches
removed check-vector closure code
known-limitation policy and examples
per-server manifest paths
aggregate script tests
rust-analyzer root cause and resolution
hung-server flake root cause and resolution
five server job results
workflow run ID and job IDs
aggregate manifest artifact name/ID
workspace check, Clippy, and test results
remaining known limitations
```

After this plan passes, Phase 4 can be closed without remaining cleanup, evidence-integrity, matrix-aggregation, or verification qualifications for the exact pinned server matrix.
