# Shell Output Projection Remaining Plan 1: CI Validation Observability

## Objective

Close the remaining operational validation gap for the shell-output projection stack by making CI coverage and validation status visible, repeatable, and specific to the projection subsystem. The projection architecture is now feature-complete through Phases 1-10 and validation-polished for RTK wrapper safety, raw semantics, redaction, expansion, and compaction. The main unresolved concern is that hosted GitHub status checks/workflow runs are not visible for recent commits, even though local validation commands are documented.

This plan should ensure maintainers can answer, from GitHub alone, whether the projection stack passed standard validation.

## Current State

Recent commits added or updated:

- `src/shell/rtk.rs`
- `src/shell/projector.rs`
- `src/shell/redactor.rs`
- `tests/shell_projection_harness.rs`
- `tests/shell_projection_phase10.rs`
- `tests/fixtures/shell_projection/**`
- `.codegg/skills/human_shell/SKILL.md`
- `architecture/human_shell.md`

The docs now list canonical local validation commands:

```bash
cargo fmt --check
cargo clippy --all-features --all-targets -- -D warnings
cargo test --all-features
scripts/check-core-boundary.sh
```

Optional RTK validation is documented as:

```bash
CODEGG_RTK_INTEGRATION=1 cargo test --all-features rtk_integration
```

However, recent GitHub commit checks were not visible through combined statuses or workflow runs. The remaining work is to make the same validation visible and dependable in CI.

## Workstream 1: Audit Existing CI

### Tasks

1. Inspect `.github/workflows/` and any repo-level validation scripts.
2. Identify which workflows run on:
   - push to `main`
   - push to active development branches
   - pull requests targeting `main`
   - manual dispatch
3. Confirm whether any existing workflow runs:
   - `cargo fmt --check`
   - `cargo clippy --all-features --all-targets -- -D warnings`
   - `cargo test --all-features`
   - `scripts/check-core-boundary.sh`
4. Confirm whether workflow names/check names appear as required or optional branch checks.
5. Document whether the absence of visible checks is because:
   - workflows do not exist
   - workflows do not trigger on direct pushes
   - connector visibility is limited
   - branch protection is absent
   - checks are only run in another system

### Acceptance Criteria

- The repo has a clear explanation of why recent commits did or did not show visible CI statuses.
- Maintainers know which workflow is authoritative for projection validation.

## Workstream 2: Add a Projection Validation Workflow or Job

### Preferred Option

Add a dedicated job to the existing Rust CI workflow:

```yaml
projection-validation:
  name: shell projection validation
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
    - uses: Swatinem/rust-cache@v2
    - run: cargo test --all-features --test shell_projection_harness
    - run: cargo test --all-features --test shell_projection_phase10
    - run: cargo test --all-features shell::redactor
    - run: cargo test --all-features shell::rtk
```

If the repo convention is to avoid separate subsystem jobs, fold these commands into the existing test job and update the job name/summary to mention shell projection coverage.

### Minimal Option

If full `cargo test --all-features` already runs in CI, add a workflow step that explicitly prints:

```text
shell projection harness covered by cargo test --all-features
```

and update docs to name that job as authoritative.

### Acceptance Criteria

- Shell projection harnesses run in CI or are explicitly covered by an all-features test job.
- The job/check name is clear enough to find from the GitHub UI.
- RTK-absent environments do not fail standard CI.

## Workstream 3: Keep Optional RTK Integration Out of Standard CI

### Tasks

1. Ensure `CODEGG_RTK_INTEGRATION=1` tests skip by default.
2. Add a manual workflow or documented local command for RTK installed environments.
3. If adding a manual workflow, use `workflow_dispatch` only and install RTK explicitly only if that is reliable and acceptable.
4. Do not make RTK installation a default dependency.

### Acceptance Criteria

- Standard CI remains deterministic without RTK installed.
- Maintainers can still run RTK real-binary validation on demand.
- Optional test skip messages are clear.

## Workstream 4: CI Runtime and Low-Power Compatibility

The project often targets lightweight environments. Avoid turning validation into an expensive job without need.

### Tasks

1. Measure rough CI runtime for all-features tests.
2. If runtime is high, split jobs:
   - format/check/clippy
   - core tests
   - shell projection tests
3. Avoid unnecessary matrix expansion.
4. Cache cargo registry and target artifacts.
5. Keep optional RTK validation manual-only.

### Acceptance Criteria

- CI remains reasonably fast.
- Projection coverage is not sacrificed for speed.
- Optional RTK tests do not run unless explicitly requested.

## Workstream 5: Documentation Updates

Update:

- `architecture/human_shell.md`
- `.codegg/skills/human_shell/SKILL.md`
- `AGENTS.md` if it lists authoritative validation commands

Add a short section:

```text
Authoritative CI coverage:
- <workflow name> / <job name>: standard Rust validation
- <workflow name> / <job name>: shell projection harness
- Optional RTK: local or manual workflow only
```

If no hosted CI is intentionally used, state that clearly and name the local validation command as the source of truth.

### Acceptance Criteria

- Future agents do not need to infer CI status from missing GitHub check data.
- Docs identify the authoritative validation path.

## Tests/Checks to Run

Before completing this pass, run or verify CI runs:

```bash
cargo fmt --check
cargo clippy --all-features --all-targets -- -D warnings
cargo test --all-features
scripts/check-core-boundary.sh
```

For targeted shell projection validation:

```bash
cargo test --all-features --test shell_projection_harness
cargo test --all-features --test shell_projection_phase10
cargo test --all-features shell::redactor
cargo test --all-features shell::rtk
```

Optional, local only:

```bash
CODEGG_RTK_INTEGRATION=1 cargo test --all-features rtk_integration
```

## Success Criteria

- Recent commits or PRs show visible validation status in GitHub, or docs explicitly state why hosted CI is not used.
- Shell projection harnesses are covered by standard validation.
- Optional RTK integration remains optional and documented.
- CI/runtime overhead remains acceptable.
- Future repo reviews can cite workflow/job status rather than relying on commit-message claims.

## Non-Goals

- Do not make RTK a required CI dependency.
- Do not redesign the whole CI system if a small workflow/job addition is sufficient.
- Do not add broad OS matrices unless the repo already uses them.
- Do not expand projection features in this pass.

## Handoff Notes

This is primarily an observability and trust pass. The code already has substantial local tests. The goal is to make validation visible and repeatable so later changes to RTK, redaction, or compaction cannot regress silently.
