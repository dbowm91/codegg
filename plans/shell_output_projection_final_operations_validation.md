# Shell Output Projection Final Operations Validation Plan

## Objective

Close the last operational loose ends for the shell-output projection and optional RTK integration work. The implementation now includes explicit CI projection validation steps and RTK operational readiness improvements, but two things still need a final confirmation pass:

1. A real hosted GitHub Actions run must be observed for the updated CI workflow.
2. The optional RTK real-binary integration tests must be run in an environment with RTK installed, or explicitly recorded as not yet verified.

This plan is intentionally operational. Do not add new projection features in this pass.

## Current State

The repository now has:

- Shell projection evaluation harness coverage.
- Phase 10 context-budget and compaction tests.
- Redactor unit tests.
- RTK unit tests that do not require an RTK binary.
- Explicit shell projection validation steps in `.github/workflows/ci.yml`.
- Env-gated RTK integration tests via `CODEGG_RTK_INTEGRATION=1`.
- RTK structured diagnostics and status summary support.
- RTK stderr warning capping.
- Strict wrapper grammar and structured raw semantics.

Remaining uncertainty:

- Connector-visible status checks/workflow runs have not confirmed that the updated workflow executed.
- The optional RTK real-binary test path has not been confirmed against an actual RTK install in this review trail.

## Workstream 1: Confirm Hosted CI Execution

### Tasks

1. Push or otherwise trigger the updated `.github/workflows/ci.yml` on the active branch.
2. Observe the GitHub Actions run in the GitHub UI or via API.
3. Confirm the `test` job includes the explicit shell projection steps:
   - Shell projection evaluation harness
   - Shell projection context budget tests
   - Shell projection redactor unit tests
   - Shell projection RTK unit tests
4. Confirm each named step passes.
5. If no workflow runs appear:
   - verify Actions are enabled for the repository
   - verify workflow triggers include the branch being pushed
   - verify the workflow path is `.github/workflows/ci.yml`
   - verify branch protection or repository settings do not suppress checks
   - verify the connector/API view is not filtering out push-triggered runs
6. Record the result in `architecture/human_shell.md` or an appropriate validation note.

### Acceptance Criteria

- A hosted CI run is visible and passes the shell projection validation steps; or
- the repo explicitly documents that hosted CI is unavailable and local validation is authoritative.

## Workstream 2: Verify Standard Local Validation

### Tasks

Run the standard local validation path from a clean checkout:

```bash
cargo fmt --check
cargo clippy --all-features --all-targets -- -D warnings
cargo test --all-features
scripts/check-core-boundary.sh
```

Then run the targeted shell projection commands:

```bash
cargo test --all-features --test shell_projection_harness
cargo test --all-features --test shell_projection_phase10
cargo test --all-features --lib shell::redactor
cargo test --all-features --lib shell::rtk
```

Record:

- command
- pass/fail
- test counts where available
- platform
- Rust version
- commit SHA

### Acceptance Criteria

- Standard validation passes on a clean checkout.
- Targeted shell projection validation passes independently.
- Any failures are fixed or documented with follow-up tasks.

## Workstream 3: Run Optional RTK Real-Binary Integration

### Tasks

1. Install or locate RTK on a dev machine.
2. Record the exact binary and version:

```bash
which rtk
rtk --version
```

3. Run the env-gated integration tests:

```bash
CODEGG_RTK_INTEGRATION=1 cargo test --all-features rtk_integration
```

4. Record whether tests verify:
   - RTK post-process contract
   - RTK wrapper contract
   - skip behavior when env is absent
   - raw semantics metadata
   - fallback behavior if a capability is unsupported
5. If RTK is not available or the integration test cannot run, document the state as `not verified` rather than implying success.

### Acceptance Criteria

- RTK integration is verified against at least one concrete RTK version; or
- RTK integration remains explicitly marked as unverified pending a real-binary run.

## Workstream 4: Validate RTK Unsupported-Mode Behavior

The RTK readiness commit added help-text detection for RTK versions that do not support stdin post-process. Confirm that behavior manually or via test.

### Tasks

1. Run a manual stdin smoke check if RTK is installed:

```bash
printf 'hello world\n' | rtk
```

2. Observe whether RTK:
   - compresses stdin
   - prints help
   - exits non-zero
   - hangs
3. Confirm Codegg maps that behavior to the correct capability state.
4. Run wrapper smoke check:

```bash
rtk echo hello
```

5. Confirm Codegg maps wrapper behavior to the correct capability state.

### Acceptance Criteria

- Unsupported stdin mode is not misclassified as supported.
- Wrapper support is accurately classified.
- Fallback is safe and diagnostic messages are actionable.

## Workstream 5: Record Final Operational Status

### Tasks

Add a short status note to `architecture/human_shell.md` or a dedicated validation note under `plans/` after the checks are complete.

Suggested format:

```markdown
## Shell Projection Operational Validation

Commit: <sha>
Date: <date>
Platform: <platform>
Rust: <rustc --version>

Standard validation:
- cargo fmt --check: pass/fail
- cargo clippy --all-features --all-targets -- -D warnings: pass/fail
- cargo test --all-features: pass/fail
- scripts/check-core-boundary.sh: pass/fail

CI validation:
- Workflow: <name/run id>
- Shell projection evaluation harness: pass/fail
- Shell projection context budget tests: pass/fail
- Shell projection redactor unit tests: pass/fail
- Shell projection RTK unit tests: pass/fail

RTK integration:
- RTK binary: <path or not installed>
- RTK version: <version or not verified>
- CODEGG_RTK_INTEGRATION=1 test: pass/fail/not run
- Notes: <post-process/wrapper support>
```

### Acceptance Criteria

- Future repo reviews can distinguish implemented code from verified runtime behavior.
- Optional RTK verification status is explicit.

## Workstream 6: Failure Handling

If validation fails, do not broaden scope. Create a small corrective commit or follow-up plan for the specific failure.

Possible failure categories:

- CI workflow syntax or trigger issue.
- Cargo test filter mismatch.
- Clippy failure from newly added code.
- RTK binary contract mismatch.
- RTK unsupported post-process mode.
- RTK wrapper mode unsupported.
- Platform-specific shell/probe issue.

For RTK-specific failures, prefer disabling or downgrading the unsupported capability over forcing RTK to appear active.

## Success Criteria

- Hosted CI workflow visibility is confirmed or explicitly documented as unavailable.
- Standard local validation passes.
- Targeted shell projection tests pass.
- Optional RTK integration is either verified against a real RTK binary or explicitly marked unverified.
- Final operational status is recorded in the repo.
- No new feature work is introduced.

## Non-Goals

- Do not make RTK required.
- Do not make RTK default.
- Do not add new native projectors.
- Do not redesign shell execution.
- Do not add model-generated summaries.
- Do not expand command eligibility during this pass.

## Handoff Notes

This is the final verification pass for this line of work. The shell-output projection stack is already implemented and hardened. The remaining value is evidentiary: prove the CI path runs, prove local validation passes, and either prove RTK against a real binary or clearly mark that optional backend as pending real-binary verification.
