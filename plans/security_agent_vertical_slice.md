# Security Agent Vertical Slice Plan

## Purpose

Implement the smallest useful security-agent workflow slice before full evidence-based finding synthesis.

The previous `security_agent_workflow.md` plan is intentionally broad. This handoff narrows the next pass to a safe, testable vertical slice:

1. Discover changed hunks.
2. Select deterministic `securityContext` presets.
3. Build review targets.
4. Request or stage `securityContext` inputs for those targets.
5. Produce review prompts from risk markers.
6. Explicitly avoid confirmed findings for marker-only evidence.

This pass should establish the workflow skeleton without overcommitting to full security reasoning.

## Non-Goals

Do not implement full finding synthesis yet.

Do not emit confirmed vulnerabilities from risk markers alone.

Do not add dependency/CVE lookup.

Do not add taint analysis.

Do not add exploit generation or offensive automation.

Do not mutate files.

Do not add network scanning.

Do not require a live LSP server for most tests.

Do not block normal agent/coding flows.

## Desired Output of This Slice

The workflow should be able to return a structured report like:

```json
{
  "targets": [...],
  "review_prompts": [...],
  "findings": [],
  "notes": [
    "marker-only evidence is reported as review prompts, not findings"
  ]
}
```

`findings` should remain empty in this pass unless an existing deterministic check already proves a concrete issue. Prefer leaving findings out entirely until the next phase.

## Phase 1 — Add Workflow Data Types

Add a new module in the most fitting place after inspecting current structure.

Recommended candidates:

```text
src/security_review.rs
src/agent/security_review.rs
src/tool/security_review.rs
crates/codegg-core/src/security_review.rs
```

Use whichever matches the current repo layout.

Initial DTOs:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangedHunk {
    pub file_path: PathBuf,
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewTarget {
    pub file_path: PathBuf,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub preset: String,
    pub reason: SecurityTargetReason,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecurityTargetReason {
    ChangedHunk,
    DependencyMetadata,
    UnsafeCode,
    ProcessExecution,
    FilesystemAccess,
    NetworkBoundary,
    AuthOrSecretHandling,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewPrompt {
    pub file_path: PathBuf,
    pub line: Option<u32>,
    pub preset: String,
    pub category: Option<String>,
    pub title: String,
    pub rationale: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewReport {
    pub targets: Vec<SecurityReviewTarget>,
    pub review_prompts: Vec<SecurityReviewPrompt>,
    pub findings: Vec<SecurityReviewFindingStub>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewFindingStub {
    pub title: String,
    pub note: String,
}
```

For this pass, `SecurityReviewFindingStub` should usually be empty. It exists only to stabilize the output shape for future finding synthesis.

Acceptance criteria:

- target/prompt/report DTOs exist;
- marker-only prompts are structurally separate from findings;
- DTOs are serializable for future TUI/CLI use.

## Phase 2 — Parse Changed Hunks

Add a small parser for unified diff hunks.

Input format should support `git diff --unified=0` and normal unified diff:

```diff
diff --git a/src/foo.rs b/src/foo.rs
--- a/src/foo.rs
+++ b/src/foo.rs
@@ -10,2 +10,4 @@
```

Parser behavior:

- track current file from `+++ b/...`;
- parse hunk headers;
- return `ChangedHunk` entries using new-file positions;
- skip `/dev/null` deleted files;
- skip binary markers;
- tolerate missing counts (`@@ -1 +1 @@` means count 1);
- ignore hunks outside normal file paths.

Recommended function:

```rust
pub fn parse_changed_hunks(diff: &str) -> Vec<ChangedHunk>
```

Hunk header parser:

```rust
fn parse_hunk_header(line: &str) -> Option<(u32, u32, u32, u32)>
```

Acceptance criteria:

- parses single-file diff;
- parses multi-file diff;
- handles omitted count as 1;
- skips deleted/binary files;
- tests are hermetic.

## Phase 3 — Exclusion Rules

Add simple default path exclusions:

```text
vendor/
third_party/
target/
dist/
build/
node_modules/
*.lock? maybe no: keep Cargo.lock for dependency_review
*.min.js
```

Do not exclude:

```text
Cargo.toml
Cargo.lock
build.rs
```

Recommended function:

```rust
pub fn is_security_review_excluded_path(path: &Path) -> bool
```

Acceptance criteria:

- generated/vendor paths are skipped;
- Cargo manifests/lockfiles remain reviewable;
- tests cover both.

## Phase 4 — Preset Selection Heuristics

Add deterministic preset selection from path and optional content snippet.

Recommended function:

```rust
pub fn select_security_preset(path: &Path, content_hint: Option<&str>) -> String
```

Initial rules, ordered:

1. `dependency_review` for:

```text
Cargo.toml
Cargo.lock
build.rs
package.json
pnpm-lock.yaml
yarn.lock
package-lock.json
```

2. `unsafe_review` when content/path contains:

```text
unsafe
atomic
UnsafeCell
raw pointer
ffi
extern "C"
```

3. `web_backend` when path/content contains:

```text
handler
route
router
auth
session
jwt
cookie
middleware
sql
request
response
```

4. `rust_cli` when path/content contains:

```text
cli
command
args
process
Command::new
std::fs
fs::
config
```

5. `rust_server` default for Rust files and unknown service code.

Acceptance criteria:

- deterministic selection;
- tests cover all presets;
- rule order is documented in code comments or docs.

## Phase 5 — Build Security Review Targets

Add:

```rust
pub fn build_security_review_targets(
    hunks: &[ChangedHunk],
    load_content: impl Fn(&Path) -> Option<String>,
) -> Vec<SecurityReviewTarget>
```

For each hunk:

- skip excluded paths;
- choose line:
  - if `new_count > 0`, use `new_start`;
  - else use `None`;
- set `column = Some(1)` when line exists;
- select preset using path + content hint;
- infer reason from selected preset/content:
  - `dependency_review` -> `DependencyMetadata`
  - `unsafe_review` -> `UnsafeCode`
  - `web_backend` with auth/session/jwt -> `AuthOrSecretHandling`
  - path/content with process -> `ProcessExecution`
  - path/content with fs -> `FilesystemAccess`
  - path/content with network/server -> `NetworkBoundary`
  - fallback -> `ChangedHunk`

Dedupe targets by:

```text
file_path + line + preset + reason
```

Acceptance criteria:

- one or more targets created from hunks;
- excluded paths skipped;
- targets include preset and reason;
- duplicates removed.

## Phase 6 — Build SecurityContext Requests

Do not necessarily execute LSP in this pass. First build request payloads to keep the slice testable.

Add:

```rust
pub fn build_security_context_request(target: &SecurityReviewTarget) -> serde_json::Value
```

Output shape:

```json
{
  "operation": "securityContext",
  "file_path": "...",
  "security_preset": "rust_server",
  "max_risk_markers": 80,
  "call_depth": 0
}
```

When line/column exists, include:

```json
"line": <line>,
"column": 1
```

Initial call expansion rule:

```text
call_depth = 0 always in this slice
```

Deep review/escalation can come later.

Acceptance criteria:

- request builder emits valid `securityContext` input;
- `call_depth` is default-off;
- no LSP server required for tests.

## Phase 7 — Convert SecurityContext Packets to Review Prompts

Add a function that accepts parsed `securityContext` JSON output and emits prompts, not findings.

Recommended function:

```rust
pub fn prompts_from_security_context(
    target: &SecurityReviewTarget,
    context_json: &serde_json::Value,
) -> Vec<SecurityReviewPrompt>
```

For each risk marker:

- title: `Review <category>: <label>`;
- file/line from marker if available, else target;
- rationale from marker rationale;
- evidence includes:
  - marker category;
  - matched text;
  - marker rationale;
  - target reason;
  - preset;
  - truncation note if context is truncated.

Do not create findings.

Acceptance criteria:

- risk markers become review prompts;
- no marker becomes a finding;
- truncated contexts add evidence/note;
- malformed/missing fields fail soft with notes or empty prompts.

## Phase 8 — Report Assembly

Add:

```rust
pub fn assemble_security_review_report(
    targets: Vec<SecurityReviewTarget>,
    prompts: Vec<SecurityReviewPrompt>,
    notes: Vec<String>,
) -> SecurityReviewReport
```

Always include note:

```text
risk markers are review prompts, not confirmed findings
```

Keep:

```rust
findings: Vec::new()
```

Acceptance criteria:

- report shape is stable;
- findings are empty for marker-only pass;
- note makes marker/finding boundary explicit.

## Phase 9 — Minimal Invocation Surface

Depending on current architecture, add one of:

### Option A — internal workflow API only

```rust
pub fn plan_security_review_from_diff(diff: &str, repo_root: &Path) -> SecurityReviewReport
```

This creates targets and request payloads but does not execute LSP.

### Option B — tool operation

Add a new read-only tool operation if appropriate:

```json
{
  "operation": "securityReviewPlan",
  "diff": "..."
}
```

Recommendation: start with internal API unless there is already a clean command/tool routing path. Avoid cluttering the `lsp` tool with full workflow orchestration unless the existing architecture prefers that.

Acceptance criteria:

- workflow is callable in tests;
- no command/TUI churn is required unless already obvious;
- no file mutation.

## Phase 10 — Tests

Add tests for each pure layer.

### Hunk parser

```text
security_review_parse_single_hunk
security_review_parse_multiple_files
security_review_parse_omitted_hunk_counts
security_review_skips_deleted_file
security_review_skips_binary_file
```

### Exclusions

```text
security_review_excludes_vendor_target_node_modules
security_review_keeps_cargo_manifest_lock_and_build_rs
```

### Presets

```text
security_review_selects_dependency_review_for_cargo_toml
security_review_selects_dependency_review_for_cargo_lock
security_review_selects_unsafe_review_for_unsafe_content
security_review_selects_web_backend_for_auth_handler
security_review_selects_rust_cli_for_command_process
security_review_defaults_to_rust_server_for_rs_file
```

### Targets

```text
security_review_builds_targets_from_hunks
security_review_dedupes_targets
security_review_assigns_reason_from_preset_or_content
```

### Requests

```text
security_review_builds_security_context_request_with_preset
security_review_request_omits_line_column_when_target_unpositioned
security_review_request_keeps_call_depth_zero
```

### Prompts/report

```text
security_review_marker_becomes_review_prompt
security_review_marker_only_does_not_create_finding
security_review_truncated_context_adds_prompt_evidence
security_review_report_includes_marker_not_finding_note
```

Acceptance criteria:

- tests do not require live LSP;
- no network;
- no filesystem mutation except temp fixtures if needed.

## Phase 11 — Documentation

Update docs only for the vertical slice.

Recommended files:

```text
architecture/lsp.md
architecture/tool.md
architecture/agents.md if present
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

Add concise section:

```markdown
### Security review vertical slice

The security review workflow first identifies changed hunks, selects a `securityContext` preset, and generates review prompts from risk markers. Marker-only evidence does not produce findings. Findings require a later synthesis phase with concrete evidence.
```

Document:

- changed hunks are first target source;
- preset selection is deterministic;
- `securityContext` requests use `call_depth=0` in this slice;
- risk markers become review prompts;
- findings remain empty unless later evidence synthesis is added;
- workflow is read-only.

Acceptance criteria:

- docs match implementation;
- docs do not imply confirmed vulnerabilities;
- docs preserve defensive/no-mutation boundary.

## Phase 12 — Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted:

```bash
cargo test -p codegg security_review
rg "SecurityReviewTarget|SecurityReviewPrompt|SecurityReviewReport|ChangedHunk|parse_changed_hunks" src crates tests architecture .opencode AGENTS.md
rg "risk markers are review prompts|not confirmed findings|marker-only" src crates tests architecture .opencode AGENTS.md
rg "call_depth.*0|security_preset|securityContext" src crates tests architecture .opencode AGENTS.md
```

Manual smoke:

```text
1. Feed a small diff touching a normal Rust file; expect one target and no findings.
2. Feed a Cargo.toml diff; expect dependency_review preset.
3. Feed an unsafe code diff; expect unsafe_review preset.
4. Feed a handler/auth diff; expect web_backend preset.
5. Feed synthetic securityContext JSON with one risk marker; expect one review prompt and zero findings.
```

## Done Criteria

This vertical slice is complete when:

- changed hunk parser exists and is tested;
- exclusion rules exist and are tested;
- preset selection exists and is tested;
- security review targets can be built from hunks;
- securityContext request payloads can be built from targets;
- risk markers convert to review prompts;
- marker-only evidence never creates findings;
- report assembly includes explicit marker-not-finding note;
- docs explain the limited workflow;
- no mutation, exploit workflow, network scanning, or vulnerability-verdict behavior is introduced.

## Next Pass After This

Implement evidence-based finding synthesis:

- add `SecurityReviewFinding` with severity/confidence/evidence/recommendation/tests;
- require at least two evidence sources or a concrete code-path explanation;
- incorporate deterministic preflight checks;
- support optional call expansion for high-risk targets;
- add TUI/CLI presentation for prompts vs findings.
