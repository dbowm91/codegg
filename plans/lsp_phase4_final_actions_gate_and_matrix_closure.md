# LSP Phase 4 Final Actions Gate and Matrix Closure

## Purpose

Close the last Phase 4 verification gap after:

```text
6001a82d12e28c87a13ea788464fcc7a09605f4e
```

The Phase 4 production API is functionally complete. The remaining closure work is not feature work; it is a final evidence gate and a few correctness/tightening items that determine whether the pinned matrix can be treated as authoritative.

Current state:

- The compatibility matrix aggregation script exists and has unit coverage.
- The workflow now attempts to aggregate downloaded per-server artifacts.
- Operation records are typed and substantially authoritative.
- Position encoding is centralized.
- The hung-server harness test no longer flakes on `/bin/sleep` exiting after stdin close.
- Documentation still must not claim Phase 4 completion until the final five-server matrix and aggregate manifest are demonstrably green.

Remaining closure concerns:

1. A successful five-server GitHub Actions run is not yet independently visible.
2. The aggregate manifest must be inspected against real workflow artifacts, not only synthetic unit tests.
3. UTF-8 byte-offset conversion is intentionally permissive; callers must prove they never use non-character-boundary offsets for Rust `str` slicing, or the converter must be tightened.
4. The force-kill path is no longer strongly tested by the `/bin/sleep` harness case.
5. Known-limitation policy still needs a final audit so protocol-failed limitations are not presented as semantic passes.
6. Documentation should only claim Phase 4 closure after the above gates pass.

This plan is tailored for a smaller implementation model. Execute passes in order. Do not add new LSP features or servers.

## Final Closure Definition

Phase 4 is closed only when:

1. All five pinned real-server jobs pass on the same commit.
2. The matrix-summary job passes on that same workflow run.
3. The aggregate manifest lists exactly:

```text
rust-analyzer
basedpyright
gopls
typescript-language-server
clangd
```

4. Every server manifest has the same commit SHA and workflow run ID.
5. Every report referenced by the manifest exists and validates.
6. Required and advertised-required operation records pass according to typed outcome fields.
7. Known limitations are explicitly scoped and not mislabeled as full semantic passes.
8. UTF-8 position conversion is either safe for all current call sites or changed to reject non-character-boundary offsets.
9. A deterministic force-kill-required harness test covers the actual kill path.
10. Documentation points to or records the final workflow run/artifact evidence.
11. Phase 2 and Phase 3 regression suites remain green.

## Primary Files

```text
.github/workflows/lsp-real-server.yml
scripts/aggregate_lsp_compatibility_manifest.py
scripts/test_aggregate_lsp_compatibility_manifest.py
crates/egglsp/src/position.rs
crates/egglsp/src/compatibility.rs
crates/egglsp/tests/real_server_smoke.rs
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
plans/lsp_phase4_final_cleanup_and_verification_pass.md
```

## Non-Goals

Do not:

- add new LSP operations;
- add new language servers;
- weaken required semantic assertions to make the matrix green;
- remove preview-only safety boundaries;
- modify Phase 3 restart/process ownership except to fix a directly failing regression;
- redesign operation modules;
- change model-facing tool schemas.

# Pass 1 — Run and Inspect the Real GitHub Actions Matrix

## Goal

Move from repository-local machinery to actual external evidence.

## Required Workflow Run

Trigger or wait for the LSP real-server workflow on the current head commit.

Required jobs:

```text
rust-analyzer
basedpyright
gopls
typescript-language-server
clangd
matrix-summary
```

The closure run is valid only when all six jobs pass.

## Required Evidence Capture

Record in the final handoff:

```text
commit SHA
workflow run ID
workflow URL
job IDs or job names
artifact names
aggregate manifest artifact name/ID
per-server artifact names/IDs
```

## Required Artifact Inspection

Download or inspect the aggregate manifest artifact and verify:

```text
complete == true
expected_servers length == 5
servers object has exactly 5 keys
all server entries use the current commit SHA
all server entries use the same workflow_run_id
all referenced report paths exist in the downloaded artifacts
summary counts are present and nonzero
```

For each per-server report, verify:

```text
server_id matches server manifest
server_version is non-empty
position_encoding is present
shutdown_trace is present
operation_support is non-empty
no invalid operation outcome combination exists
all Required records pass
all advertised RequiredIfAdvertised records pass
known limitations include non-empty reasons
```

## Failure Policy

If any server job fails:

- do not mark Phase 4 complete;
- preserve the failing report/log tail;
- fix the underlying fixture, server pin, or claim;
- rerun the full matrix.

If only the summary job fails:

- treat aggregation as not closed;
- fix the script or workflow;
- rerun the summary against the same artifacts if possible, otherwise rerun the full workflow.

## Acceptance Criteria

- A reviewer can find one workflow run proving the full pinned matrix.
- No closure statement depends only on local commit-message claims.

# Pass 2 — Validate the Aggregator Against Real Artifact Layout

## Goal

Confirm the script works with `actions/download-artifact` output as actually produced by GitHub.

## Required Checks

Inspect the downloaded directory layout from the summary job:

```text
target/lsp-compatibility/downloaded/
  lsp-compat-rust-analyzer/
  lsp-compat-basedpyright/
  lsp-compat-gopls/
  lsp-compat-typescript-language-server/
  lsp-compat-clangd/
```

Each directory should contain:

```text
server-manifest.json
<server report>.json
optional stderr/version files
```

## Script Requirements

The aggregate script must:

- recursively discover `server-manifest.json` files;
- reject missing server labels;
- reject duplicate labels;
- reject commit mismatch;
- reject workflow run ID mismatch;
- reject missing reports;
- reject report metadata disagreement;
- reject invalid operation outcome invariants;
- reject missing shutdown traces;
- reject failing Required records;
- reject advertised failing RequiredIfAdvertised records;
- write a new root aggregate manifest.

## Add a Realistic Layout Test

The current synthetic tests are useful. Add one test that mirrors GitHub's downloaded artifact layout exactly:

```text
tmp/downloaded/lsp-compat-rust-analyzer/server-manifest.json
tmp/downloaded/lsp-compat-rust-analyzer/rust-analyzer.json
...
```

Test name:

```text
aggregates_github_download_artifact_directory_layout
```

## Acceptance Criteria

- The summary job behavior is proven against the real artifact layout.
- The aggregate manifest is produced by validation, not copied from an individual server job.

# Pass 3 — Resolve or Truthfully Downgrade Rust-Analyzer Matrix Failure

## Goal

The pinned matrix must be five of five, or documentation must remove the unsupported claim.

## Required Investigation

If rust-analyzer still fails, preserve:

```text
failing operation
operation record
raw check detail
server version
rustc version
request file and position
raw LSP response or protocol error
stderr tail
compatibility report JSON
```

Likely area is type hierarchy, but do not assume.

## Valid Resolutions

### A. Fix the fixture/request

Use if rust-analyzer supports the operation but the test is wrong.

Examples:

- wrong trait/name position;
- unopened document;
- stale document version;
- wrong subtype/supertype expectation;
- fixture does not trigger rust-analyzer analysis in time.

### B. Pin the tested version

Use if the claim is valid for one exact rust-analyzer version and invalid for another.

Requirements:

- deterministic installation;
- version recorded in report and docs;
- passing real-server run on the pinned version.

### C. Remove or narrow the claim

Use if the pinned version does not support the operation reliably.

Requirements:

- remove profile override or mark operation unsupported;
- update docs and fixture requirement;
- ensure aggregate report shows unsupported/skipped truthfully, not passing.

## Prohibited Resolution

Do not convert a failing required operation to a known limitation merely to preserve a green matrix unless the report records a scoped, evidence-backed limitation and the docs explicitly say the operation is not semantically validated.

## Acceptance Criteria

- rust-analyzer either passes the matrix or no longer claims unsupported capability support.

# Pass 4 — Decide and Enforce UTF-8 Offset Safety

## Current Concern

`lsp_utf8_to_byte_offset()` intentionally returns byte offsets inside multibyte characters. That is semantically valid if and only if every caller treats UTF-8 offsets as raw byte positions and never uses them to slice `str` directly.

The helper name says it returns a byte offset, but callers may reasonably assume it is safe for Rust slicing because the UTF-16 and UTF-32 conversions necessarily return character-boundary byte offsets.

## Required Audit

Search all callers of:

```text
lsp_units_to_byte_offset
lsp_unit_range_to_byte_offsets
```

Classify each use:

```text
safe raw byte position only
used for string slicing
used for range validation
used for TextEdit application
used for semantic-token bounds
```

## Policy Choice

Choose one policy and enforce it consistently.

### Preferred Safety Policy

Return only Rust `str` character boundaries from all encodings.

Implementation:

```rust
fn lsp_utf8_to_byte_offset(text: &str, units: u32) -> Option<usize> {
    let offset = usize::try_from(units).ok()?;
    if offset > text.len() || !text.is_char_boundary(offset) {
        return None;
    }
    Some(offset)
}
```

Rationale: every consumer receives offsets that are safe for slicing.

### Alternative Raw-Byte Policy

Keep current UTF-8 behavior, but make unsafety impossible by type design.

Requirements:

- rename helper to indicate raw byte offsets are not slice-safe;
- add a separate `lsp_units_to_char_boundary_byte_offset()` for slicing callers;
- update all slicing/text-edit callers to use the boundary-safe helper;
- document why semantic-token bounds can accept raw byte positions.

This is more complex and should only be chosen if strict boundary rejection breaks known UTF-8-negotiating servers.

## UTF-32 Fix

Regardless of UTF-8 policy, ensure UTF-32 exact end-of-string offsets succeed.

Required tests:

```text
utf32_exact_end_ascii
utf32_exact_end_non_ascii
utf32_past_end_rejected
```

## Required Tests

For the chosen UTF-8 policy, add explicit tests. For preferred safety:

```text
utf8_offset_inside_multibyte_character_is_none
utf8_character_boundary_after_multibyte_is_some
utf8_exact_end_is_some
utf8_past_end_is_none
```

For raw-byte policy:

```text
utf8_raw_offset_inside_multibyte_is_allowed
slicing_helper_rejects_mid_character_offset
text_edit_application_uses_slicing_helper
signature_label_slicing_uses_slicing_helper
```

## Acceptance Criteria

- No caller can accidentally slice `str` at a non-boundary byte offset.
- Position helper documentation and behavior agree.

# Pass 5 — Strengthen Deterministic Force-Kill Coverage

## Current Concern

The existing hung-server test now accepts `Graceful`, `ForceKilled`, or `TimeoutExpired` because `/bin/sleep` may exit when stdin closes. That avoids flakes but does not prove force-kill behavior.

## Required New Fixture

Add a deterministic process that ignores stdin close and remains alive until killed.

Preferred approaches:

### Unix shell fixture

```bash
sh -c 'trap "" TERM; while true; do sleep 60; done'
```

Then the harness should force kill after the graceful timeout.

### Tiny Rust helper binary

If the repository already has test helper binaries, add one that:

- ignores stdin;
- ignores SIGTERM where platform permits;
- sleeps forever;
- exits only when killed.

Use the shell fixture first if acceptable.

## Required Tests

Keep the tolerant non-LSP smoke test if useful, but add a separate strict test:

```text
smoke_harness_force_kills_process_that_ignores_stdin_close
```

Assertions:

```text
graceful_exit_observed == false
force_kill_requested == true
force_kill_succeeded == true
child_reaped == true
shutdown_trace records the force-kill path
```

## Platform Handling

If the strict fixture is Unix-only, gate it with `#[cfg(unix)]` and add a documented Windows alternative or skip. Do not weaken Unix coverage.

## Acceptance Criteria

- The actual force-kill path is deterministically exercised on supported platforms.

# Pass 6 — Final Known-Limitation Policy Audit

## Goal

Ensure known limitations are not used as a generic pass state.

## Required Audit

Inspect every `CompatibilityRequirement::KnownLimitation` record and classify it as one of:

```text
semantic limitation
protocol limitation
shutdown limitation
environment limitation
```

At minimum, ensure each known limitation has:

```text
known_limit = Some(non-empty reason)
exercised = true unless environment limitation explicitly prevented exercise
request_succeeded/response_parsed fields truthful
status not presented as ordinary Passing
```

## Recommended Schema Addition

If practical, add:

```rust
pub known_limit_scope: Option<KnownLimitationScope>
```

with:

```rust
Semantic
Protocol
Shutdown
Environment
```

If schema churn is not worth it, use a structured prefix in `known_limit`, such as:

```text
semantic: ...
protocol: ...
shutdown: ...
environment: ...
```

and make the aggregator validate the prefix.

## Aggregator Validation

The aggregate script should reject:

```text
KnownLimitation with empty reason
KnownLimitation without scope/prefix
KnownLimitation marked as semantic when request_succeeded or response_parsed is false
Protocol/Shutdown limitation counted as required semantic pass
```

## Acceptance Criteria

- Known limitations remain visible and scoped in the final manifest.
- Protocol failures cannot masquerade as semantically passing support.

# Pass 7 — Correct Documentation Status Discipline

## Before Final Evidence

All docs should use:

```text
Phase 4 functionally complete; final five-server evidence gate pending.
```

## After Final Evidence

Only after a successful workflow run and aggregate manifest inspection, update docs to:

```text
Phase 4 complete for the exact pinned Tier 1 and Tier 2 matrix. The aggregate manifest verifies five server reports from one commit, required operation records, shutdown traces, position-encoding metadata, and exact version evidence. Compatibility outside the pinned matrix remains experimental.
```

## Required Docs

Update:

```text
README.md
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
plans/lsp_phase4_final_cleanup_and_verification_pass.md
```

## Required Evidence References

Record either:

```text
GitHub workflow run URL
artifact names/IDs
aggregate manifest path
```

or, if run locally:

```text
target/lsp-compatibility/matrix-manifest.json
server report paths
exact commands used
```

Do not claim closure based only on commit messages.

## Acceptance Criteria

- Documentation status matches the actual matrix state.
- No contradictory “4/5 failed but complete” style claim remains.

# Pass 8 — Final Verification Commands

## Local Regression

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p egglsp --lib
cargo test -p egglsp --features lsp-test-support --tests
cargo test --features lsp-test-support --test lsp_composite_stdio
cargo test --workspace --all-features
```

## Position Helper Focus

```bash
cargo test -p egglsp --lib position::
```

Run repeatedly if inexpensive:

```bash
for i in 1 2 3 4 5 6 7 8 9 10; do
  cargo test -p egglsp --lib position:: || exit 1
done
```

## Aggregator Tests

```bash
python3 -m pytest scripts/test_aggregate_lsp_compatibility_manifest.py -v
```

If `pytest` is not guaranteed in CI, either:

- install it in the workflow step; or
- rewrite the script tests using Python stdlib `unittest`.

## Harness Force-Kill Test

```bash
cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- smoke_harness_force_kills_process_that_ignores_stdin_close --nocapture
```

Repeat on supported platforms:

```bash
for i in 1 2 3 4 5; do
  cargo test -p egglsp --features lsp-real-server-tests \
    --test real_server_smoke -- smoke_harness_force_kills_process_that_ignores_stdin_close --nocapture || exit 1
done
```

## Real-Server Matrix

Run through GitHub Actions as the authoritative closure gate. Local runs are useful but not sufficient unless Actions is unavailable.

Expected passing jobs:

```text
rust-analyzer
basedpyright
gopls
typescript-language-server
clangd
matrix-summary
```

## Acceptance Criteria

- All local regressions pass.
- Aggregator tests pass.
- Force-kill strict test passes.
- Five-server workflow and matrix-summary pass.

# Exact Execution Order for a Smaller Model

1. Inspect current workflow artifact layout from a real run or mock it exactly.
2. Add the realistic downloaded-artifact-layout aggregator test.
3. Resolve rust-analyzer failure or downgrade the claim truthfully.
4. Decide UTF-8 offset policy and make callers safe.
5. Add deterministic force-kill-required harness test.
6. Audit known limitations and aggregator validation.
7. Run local regressions and aggregator tests.
8. Run the GitHub Actions matrix.
9. Inspect aggregate manifest and per-server reports.
10. Update docs only after evidence passes.

# Recommended Commit Sequence

```text
1. test(ci): cover GitHub artifact layout aggregation
2. test(egglsp): resolve rust-analyzer pinned matrix failure
3. fix(egglsp): make UTF-8 position offsets slice-safe or type-separated
4. test(egglsp): add deterministic force-kill shutdown coverage
5. refactor(egglsp): scope known limitations in compatibility records
6. ci(lsp): validate final aggregate manifest against real artifacts
7. docs(lsp): close Phase 4 only after five-server evidence gate
```

# Mandatory Final Checklist

- [ ] Five server jobs pass on one commit.
- [ ] Matrix-summary job passes on the same workflow run.
- [ ] Aggregate manifest contains exactly five expected servers.
- [ ] All report commit SHAs match the workflow commit.
- [ ] All workflow run IDs match.
- [ ] Required operation records pass through typed fields.
- [ ] Known limitations are scoped and non-empty.
- [ ] No protocol-failed limitation is presented as a semantic pass.
- [ ] UTF-8 offset policy is safe for every caller.
- [ ] UTF-32 exact end offsets pass tests.
- [ ] Deterministic force-kill test exercises the kill path.
- [ ] Aggregator tests pass against realistic artifact layout.
- [ ] Full workspace checks pass.
- [ ] Documentation references final evidence.

# Final Handoff Output

The implementing model must report:

```text
commits created
rust-analyzer resolution
UTF-8 position policy and caller audit
UTF-32 fix status
strict force-kill test result
known-limitation policy changes
aggregator realistic-layout test result
workflow run URL
job names and statuses
artifact names/IDs
aggregate manifest path
summary counts from manifest
workspace check, Clippy, and test results
remaining known limitations
```

After this plan passes, Phase 4 can be closed without remaining evidence-gate, artifact-aggregation, position-safety, shutdown-coverage, or documentation-status qualifications for the exact pinned server matrix.
