# Command Intent and Python Scripting Final Closure Pass

## Objective

Close the remaining gaps in the command-intent, Python-scripting, RunStore, projection, and active-routing roadmap after the Phase 06–10 implementation and first closure pass.

This pass is intentionally narrow. It should not introduce new command families, broaden Python privileges, redesign the run-store schema, or expand active routing. Its purpose is to remove the final mismatches between implemented capability and advertised capability, tighten several permission defaults, complete RunStore coverage for native routes, and produce explicit validation evidence.

The target end state is:

- active routing remains opt-in and defaults to Observe;
- every enabled action in the TUI is actually implemented;
- every active structured backend produces one canonical durable run record;
- permission defaults reflect semantic risk, not merely structural routability;
- test-run persistence has one documented authority;
- the complete validation matrix has been run and recorded.

## Current state to preserve

Preserve the following:

- `CommandIntentMode::Observe` remains the global default;
- `CODEGG_ROUTING_DISABLE=1` remains authoritative over all family configuration;
- complex shell and validation failures fall back to raw/legacy execution;
- destructive, outside-workspace, and dependency-install capabilities default to Deny;
- merge, rebase, cherry-pick, revert, and other conflict-prone git mutations default to Ask;
- TestRunner persists into `.codegg/runs/` in addition to the compatibility test index;
- redaction remains centralized in `ProjectionSelector::project()`;
- Python Landlock and portable fallback evidence remain distinct;
- the adversarial routing, Python sandbox, and context-projection suites remain mandatory.

## Non-goals

- Do not enable active routing by default.
- Do not add rollback unless the implementation is small, correct, and fully tested; disabling the unsupported action is preferred.
- Do not add network-enabled or dependency-install Python modes.
- Do not add new TUI tabs or redesign the run-detail dialog.
- Do not remove the legacy test index without a compatibility/migration reason.
- Do not add new projectors or RTK modes.
- Do not broaden native git support beyond currently classified operations.

## Workstream A: Make TUI capability flags truthful

### Problem

`RunCellView::from_manifest()` currently derives some action flags from run shape rather than confirmed backend support. In particular, Python runs with changed files can advertise rollback even though no rollback engine is implemented. Artifact viewing may also be marked available merely because artifact metadata exists.

### Required changes

1. Replace heuristic action flags with explicit capability derivation.

Introduce a helper or structured capability model such as:

```rust
pub struct RunActionCapabilities {
    pub can_rerun: bool,
    pub can_rollback: bool,
    pub can_promote: bool,
    pub can_view_artifact: bool,
}
```

or equivalent private helpers used by `RunCellView::from_manifest()` and `RunDetailView::from_manifest()`.

2. Set `can_rollback = false` unless a real rollback handler exists and is covered by integration tests.

3. If rollback remains deferred:

- keep the Changes/Diff views;
- remove or disable rollback keybindings/actions;
- ensure protocol/UI models do not suggest rollback is available;
- document rollback as deferred.

4. Only set `can_view_artifact = true` when the UI path can actually read artifact content through `RunStore::read_artifact()`.

5. If the current UI only displays metadata:

- set `can_view_artifact = false`; or
- rename the capability/action to `can_view_artifact_metadata` and avoid implying content access.

6. Ensure `can_promote` checks:

- artifact/projection is marked safe for model;
- promotion target exists;
- redaction state is acceptable;
- the run is not explicitly excluded.

7. Ensure rerun availability depends on a valid `RerunDescriptor`, not only its presence:

- argv/script reference is complete;
- cwd/workspace root are valid;
- backend family is supported;
- no unsafe implicit shell reconstruction is required.

### Tests

Add unit tests covering:

- Python run with changes but no rollback backend => `can_rollback == false`;
- run with artifact metadata but no viewer => `can_view_artifact == false`;
- run with valid ranged artifact viewer => `can_view_artifact == true`;
- unsafe-for-model artifact cannot be promoted;
- invalid rerun descriptor does not advertise rerun;
- valid TestRunner rerun advertises rerun.

### Acceptance criteria

- No enabled TUI action reaches an unimplemented handler.
- Action capabilities are identical between compact and detail views.
- Capability fields survive serialization/reload without changing meaning.

## Workstream B: Tighten remaining permission defaults

### Problem

Several operations remain structurally safe to route but semantically risky enough that automatic Allow is too permissive.

### Required policy changes

1. Git mutation defaults:

- `git add` => Allow;
- read-only stash forms => no mutation permission;
- `git stash push` => Ask by default unless an explicit user policy permits it;
- `git commit` => Ask because hooks may execute arbitrary code;
- `git checkout`, `git switch`, `git restore` => Ask when worktree state may be overwritten or changed;
- merge/rebase/cherry-pick/revert => Ask;
- push/pull/reset --hard/clean -f/branch -D => RawShell or Deny from active routing.

2. Add worktree-aware policy where practical:

- when checkout/switch/restore can be proven non-destructive against a clean worktree, Allow may be used;
- otherwise default to Ask;
- if worktree state is unavailable, fail conservative to Ask.

3. Formatter/write defaults:

- read-only formatter checks (`--check`, `--diff`, equivalent) => Allow;
- writing formatters (`cargo fmt`, `black`, `prettier --write`, `isort`) => Ask unless explicit configuration allows automatic formatting;
- do not infer read-only behavior from tool name alone; inspect argv.

4. Project-code execution:

- `make`, `cmake --build`, `npm run ...`, `pnpm run ...`, `yarn ...`, `bun run ...` => Ask;
- tests may remain allowed only through the validated TestRunner path and existing test permission policy;
- arbitrary managed argv remains Ask unless in a narrow read-only allowlist.

5. Keep permission and routing decisions separate:

- `validate_for_active_routing()` confirms structural eligibility;
- permission resolution confirms execution authorization;
- pending Ask requests must prevent active dispatch.

6. Persist final decisions in `RunManifest.permissions`, including:

- capability;
- default;
- effective decision;
- source of decision (default/config/user/session);
- reason.

### Tests

Add focused tests for:

- `git add` => Allow;
- `git commit` => Ask;
- `git stash push` => Ask;
- checkout/switch/restore with unknown worktree => Ask;
- merge/rebase/cherry-pick/revert => Ask;
- `cargo fmt -- --check` or equivalent read-only form => Allow;
- writing formatter => Ask;
- `npm run build` and `make` => Ask;
- unresolved Ask blocks active dispatch.

### Acceptance criteria

- No arbitrary project-code command is auto-authorized merely because it has a structured backend.
- Permission records in RunStore match the decision used at dispatch.
- Observe mode still performs no permission side effects.

## Workstream C: Complete native git and search RunStore persistence

### Problem

Bash, Python, and TestRunner persist runs, but native git/search routes must also produce complete canonical records.

### Required changes

1. For native git read routes:

- begin a `RunKind::GitRead` run;
- persist invocation and parsed argv;
- record native backend/tool name;
- persist structured output and any raw output needed for provenance;
- persist projection metadata and source spans;
- complete with status and duration.

2. For managed/native git mutation routes:

- use `RunKind::GitMutation`;
- persist permission decisions;
- record pre/post worktree or repository evidence where available;
- persist conflicts and changed paths;
- complete accurately on non-zero exit.

3. For search/read routes:

- use `RunKind::Search` for rg/grep/fd/find and analogous search execution;
- use `RunKind::NativeTool` only where no more specific kind applies;
- persist bounded raw output and final projection;
- retain exact source spans for promoted matches.

4. Ensure one canonical logical run per user command:

- BashTool should not persist an outer raw-shell run when it delegates to a structured backend;
- delegated backend should own the canonical run;
- child runs require an explicit `parent_run_id` and documented reason.

5. Add a dispatch/run ownership model:

```rust
pub enum RunOwnership {
    Caller,
    DelegatedBackend,
    ChildOf(RunId),
}
```

or an equivalent internal mechanism preventing duplicate unlinked records.

6. Add incomplete-run handling for native routes:

- process cancellation;
- task panic/error;
- persistence failure after begin;
- projection failure after execution.

### Tests

Add integration tests asserting:

- `git status` active route produces one `GitRead` manifest;
- safe git mutation produces one `GitMutation` manifest;
- `rg pattern src` produces one `Search` manifest;
- fallback/raw route produces one RawShell manifest, not a native plus shell duplicate;
- delegated test/Python execution does not create duplicate Bash records;
- interrupted native run remains listable as Incomplete/Cancelled.

### Acceptance criteria

- Every active backend produces one complete canonical run record.
- No duplicate unlinked records are created for one command.
- Native git/search artifacts are readable through RunStore ranged reads.

## Workstream D: Clarify test persistence authority

### Problem

TestRunner now writes both the canonical RunStore and the legacy `.codegg/test-runs/` index. The relationship is useful but must be explicit to avoid divergent state and duplicate semantics.

### Required changes

1. Declare `.codegg/runs/` authoritative for:

- complete run history;
- artifacts;
- projections;
- permissions;
- rerun descriptors;
- TUI/protocol views.

2. Declare `.codegg/test-runs/` compatibility-only for:

- previous-failures lookup if still required;
- legacy tooling/readers.

3. Ensure writes occur in a defined order:

- complete canonical RunStore record first where possible;
- update compatibility index second;
- a compatibility-index failure must not invalidate an otherwise complete canonical run.

4. Ensure previous-failure resolution prefers RunStore when available, with fallback to the legacy index during migration.

5. Add consistency diagnostics:

- missing compatibility entry is warning-only;
- mismatched command/status is detectable;
- corrupted compatibility index does not block RunStore listing.

6. Document migration/deprecation criteria for the compatibility index.

### Tests

- RunStore succeeds, legacy index write fails => run remains complete;
- RunStore previous-failure lookup works;
- legacy fallback works when RunStore unavailable;
- corrupted legacy index does not affect canonical run history;
- no duplicate rerun execution occurs from dual indexing.

### Acceptance criteria

- There is one documented source of truth.
- Compatibility storage cannot corrupt or override canonical records.

## Workstream E: Validate artifact viewing and promotion paths

### Required changes

1. If artifact content viewing is supported:

- route reads through `RunStore::read_artifact()`;
- enforce bounded byte ranges;
- display exactness and truncation status;
- reject invalid ranges and path traversal;
- avoid loading full large artifacts into TUI memory.

2. If not supported, disable the action and document metadata-only behavior.

3. Ensure promotion uses projection/artifact safety metadata:

- `safe_for_model` must be true;
- redaction must have completed or policy must explicitly permit promotion;
- byte/token estimate must fit policy;
- range promotion records exact artifact ID and byte range.

4. Persist promotion changes and emit the corresponding protocol event.

5. Ensure promoted artifact ranges survive restart and can be reconstructed from RunStore.

### Tests

- valid bounded artifact read;
- invalid range rejected;
- corrupted hash reported;
- unsafe artifact promotion denied;
- redacted projection promotion allowed with audit record;
- promotion state survives reload.

### Acceptance criteria

- `can_view_artifact` and `can_promote` correspond to operational paths.
- Large artifacts never require an unbounded read.

## Workstream F: Final validation evidence

### Targeted tests

```bash
cargo test -p codegg --lib command_intent
cargo test -p codegg --lib command_intent::plan
cargo test -p codegg --lib command_routing
cargo test -p codegg --lib tool::bash
cargo test -p codegg --lib tool::test
cargo test -p codegg --lib python_script
cargo test -p codegg --lib shell::projector
cargo test -p codegg --lib test_runner
cargo test -p codegg-core run_store
cargo test -p codegg-protocol
```

### Adversarial and integration tests

```bash
cargo test --test command_routing_adversarial
cargo test --test python_sandbox_adversarial
cargo test --test context_projection_adversarial
cargo test --test shell_projection_harness
cargo test --test shell_projection_phase10
```

Add dedicated closure tests if needed:

```bash
cargo test --test command_routing_closure
cargo test --test run_store_routing_integration
cargo test --test run_tui_capabilities
```

### Configuration tests

Validate:

- global Observe;
- global Active;
- global kill switch;
- each family Off/Observe/Active;
- unresolved Ask;
- explicit user Allow/Deny override;
- fallback on persistence failure.

### Full validation

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-features
```

### Evidence file

Add a concise validation record under `plans/` or `docs/validation/`, for example:

```text
docs/validation/command-routing-final-closure.md
```

Record:

- commit SHA;
- platform/architecture;
- Rust version;
- sandbox backend observed;
- exact commands run;
- pass/fail counts;
- skipped environment-gated tests;
- known resource-heavy tests;
- any unresolved caveat.

Do not claim full closure if the capped workspace suite, clippy, or check was skipped without explanation.

## Recommended implementation order

1. Correct TUI capability flags and disable unsupported actions.
2. Tighten git, formatter, and project-code permission defaults.
3. Add native git/search RunStore ownership and persistence.
4. Clarify TestRunner canonical versus compatibility persistence.
5. Validate artifact viewing and promotion behavior.
6. Add closure-specific integration tests.
7. Run targeted and adversarial suites.
8. Run the capped workspace suite, fmt, clippy, and check.
9. Add the final validation evidence document and update architecture docs.

## Closure criteria

This roadmap track is complete when:

- active routing remains opt-in and Observe by default;
- unsupported rollback/artifact actions are disabled or fully implemented;
- git commit, worktree-changing checkout/switch/restore, writing formatters, and arbitrary project scripts are permission-gated;
- native git/search routes persist complete canonical runs;
- one command produces one canonical run record;
- `.codegg/runs/` is the documented source of truth for tests;
- legacy test indexing is compatibility-only and failure-tolerant;
- promotion and artifact viewing enforce safety and bounded ranges;
- all adversarial suites pass;
- the capped full workspace suite, fmt, clippy, and check pass or exceptions are explicitly recorded;
- a final validation evidence document is committed.
