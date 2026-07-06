# Shell Output Projection Remaining Plan 2: RTK Operational Readiness

## Objective

Close the remaining RTK-specific readiness gap for the shell-output projection stack. The code now supports optional RTK discovery, capability probing, post-process mode, wrapper mode, strict wrapper parsing, structured raw semantics, and env-gated integration tests. The remaining work is to verify RTK behavior against a real installed binary, tighten documentation around supported/unsupported RTK modes, and ensure operational diagnostics are clear when RTK is unavailable, unsupported, or falling back.

RTK should remain optional and disabled by default. This plan is about making the optional path trustworthy, not making it required.

## Current State

Current RTK behavior appears to include:

- `RtkDiscovery` with availability states.
- `RtkCapabilities` with post-process and wrapper support states.
- `RtkInvocationMode::{PostProcess, Wrapper, Disabled}`.
- Capability probing for stdin post-process and wrapped-command modes.
- `project_post_process()` that pipes retained stdout/stderr to RTK stdin.
- `project_wrapper()` that uses `CommandRun.argv` when available and strict simple-command parsing otherwise.
- `ProjectionRawSemantics` distinguishing original, wrapped, unavailable, and unknown raw semantics.
- Env-gated tests under `CODEGG_RTK_INTEGRATION=1`.

The code is structurally ready. The remaining risk is semantic: RTK's actual CLI behavior and version-to-version contract need to be validated and made visible.

## Workstream 1: Verify RTK CLI Contract Against Supported Versions

### Tasks

1. Install or locate a known RTK binary in a controlled dev environment.
2. Record:
   - binary path
   - `rtk --version` output
   - platform
   - invocation mode results
3. Run the env-gated tests:

```bash
CODEGG_RTK_INTEGRATION=1 cargo test --all-features rtk_integration
```

4. Confirm whether RTK supports stdin post-process mode as currently invoked:

```text
rtk < stdin
```

or whether it requires a subcommand/flag.

5. Confirm whether RTK wrapper mode is correctly invoked as:

```text
rtk <command> <args...>
```

or whether it requires another syntax.

6. Document the verified contract in `architecture/human_shell.md`.

### Acceptance Criteria

- Docs state the exact RTK CLI invocation contract codegg supports.
- If post-process mode is unsupported, codegg reports `supports_post_process = No` and falls back safely.
- If wrapper mode is unsupported, codegg reports `supports_wrapper_mode = No` and falls back safely.
- At least one known RTK version is recorded as verified or unsupported.

## Workstream 2: Strengthen Capability Probe Diagnostics

### Tasks

1. Expand `RtkCapabilities` diagnostics or add a companion diagnostic struct:

```rust
pub struct RtkCapabilityDiagnostics {
    pub post_process_probe: ProbeDiagnostic,
    pub wrapper_probe: ProbeDiagnostic,
    pub exit_code_probe: ProbeDiagnostic,
    pub utf8_probe: ProbeDiagnostic,
}
```

2. Capture enough detail for each probe:
   - command/argument shape, without leaking sensitive data
   - timeout
   - status category
   - output length
   - reason for Yes/No/Unknown
3. Surface diagnostics in debug logs or `/shell` diagnostic output.
4. Keep model-facing output compact; diagnostics should not pollute normal projections.

### Acceptance Criteria

- A maintainer can tell why RTK was or was not selected.
- `projection = "rtk"` fallback has an actionable reason.
- Diagnostics do not include raw command output or secrets.

## Workstream 3: Validate RTK Projection Semantics

### Tasks

Use a synthetic noisy command-output sample and assert semantic expectations for RTK projection.

The sample should include:

- repeated success/noise lines
- warnings
- error-like lines
- file/line-looking spans
- enough bulk to make compression meaningful
- no real secrets

For post-process mode:

1. Store the sample in `CommandOutputStore` as retained stdout/stderr.
2. Project through `RtkProjector::project_post_process()` via the public selector path if possible.
3. Assert:
   - projection is `ProjectionKind::ExternalCompressed`
   - exactness is `ProjectionExactness::Lossy`
   - `raw_semantics = OriginalCommandRaw`
   - expansion handles point to original retained streams
   - output is non-empty
   - output is not a fake placeholder
   - warning metadata includes RTK byte counts

For wrapper mode:

1. Use a temp directory and a simple read-only command.
2. Ensure cwd is propagated.
3. Assert:
   - strict parsing rejects complex no-argv commands
   - argv path accepts paths with spaces
   - raw semantics are explicit
   - fallback occurs cleanly if wrapper mode is unsupported

### Acceptance Criteria

- RTK integration tests prove semantic projection behavior, not just process exit.
- Tests skip clearly when env flag or RTK binary is absent.
- Failures identify which RTK mode or contract failed.

## Workstream 4: Version Compatibility Policy

### Tasks

1. Decide whether codegg requires a minimum RTK version.
2. If yes, parse version output enough to reject unsupported versions.
3. If no, document that capabilities are probed behaviorally and version is informational.
4. If version parsing is added, keep it tolerant: RTK version strings may change.
5. Update `RtkState::UnsupportedVersion` handling if it remains unused.

### Acceptance Criteria

- `UnsupportedVersion` is either used meaningfully or documented as reserved.
- Version output is recorded in diagnostics.
- Capability behavior, not version alone, remains the source of truth unless a hard incompatibility is known.

## Workstream 5: User-Facing RTK Status

### Tasks

Add a concise RTK status surface. Possible options:

- `/shell-rtk-status`
- `/shell status` section
- shell detail diagnostic field
- TUI runtime settings panel entry

Minimum fields:

- RTK enabled/disabled by config
- binary path
- version
- availability state
- post-process support
- wrapper support
- selected invocation mode
- last fallback reason if available

### Acceptance Criteria

- Users can determine whether RTK is active without reading logs.
- Disabled/unavailable/unsupported RTK states are clear.
- Status output does not imply RTK is default.

## Workstream 6: Fallback and Warning Polish

### Tasks

1. Normalize fallback warnings for RTK:
   - disabled by config
   - binary not found
   - version probe failed
   - post-process unsupported
   - wrapper unsupported
   - command ineligible
   - strict parser rejected command without argv
   - timeout
   - non-zero RTK status
2. Ensure warnings are preserved in `ProjectionContextMetadata` when model-visible output uses fallback projection.
3. Keep warnings concise in normal model-visible projection text.
4. Prefer detailed diagnostics only in TUI/debug/status surfaces.

### Acceptance Criteria

- Fallback reasons are consistent and actionable.
- Compaction preserves the fact that RTK was requested but skipped/fell back.
- Normal UX is not noisy.

## Workstream 7: Security and Privacy Check

### Tasks

1. Verify RTK invocation never receives model-only redacted text when it should receive raw local command output, unless policy says otherwise.
2. Verify model-visible RTK output still passes through redaction before entering model context.
3. Verify RTK stderr is bounded in warnings and cannot dump sensitive or huge output into model context.
4. Verify RTK integration tests use fake data only.
5. Verify wrapper mode does not permit network/auth/destructive commands through eligibility classification.

### Acceptance Criteria

- RTK cannot bypass redaction.
- RTK stderr/warnings are bounded and safe.
- Wrapper mode remains read-only and constrained.

## Workstream 8: Documentation Updates

Update:

- `architecture/human_shell.md`
- `.codegg/skills/human_shell/SKILL.md`
- `AGENTS.md` if needed

Docs should state:

- RTK is optional, disabled by default, and never bundled.
- Supported RTK invocation modes are behaviorally probed.
- Exact RTK CLI contracts verified by codegg are listed.
- Optional real-binary validation command is listed.
- Normal CI does not require RTK.
- Fallback is safe and expected when RTK is unavailable or unsupported.

## Tests/Checks to Run

Standard:

```bash
cargo fmt --check
cargo clippy --all-features --all-targets -- -D warnings
cargo test --all-features
scripts/check-core-boundary.sh
```

Optional RTK:

```bash
CODEGG_RTK_INTEGRATION=1 cargo test --all-features rtk_integration
```

Manual smoke checks if useful:

```bash
rtk --version
printf 'hello world\n' | rtk
rtk echo hello
```

Only record manual smoke checks if actually run.

## Success Criteria

- RTK CLI behavior is verified against at least one real installed binary or documented as not verified.
- Capability diagnostics explain RTK mode selection and fallback.
- Optional integration tests validate semantic projection behavior.
- User-facing RTK status is available or documented as a future item if intentionally deferred.
- RTK cannot bypass redaction or shell eligibility policy.
- Docs accurately describe supported RTK behavior and limitations.

## Non-Goals

- Do not make RTK required.
- Do not make RTK the default projection backend.
- Do not bundle or install RTK automatically.
- Do not expand side-effecting command eligibility.
- Do not add model-generated summaries.

## Handoff Notes

The shell projection stack is valuable without RTK. If real RTK behavior does not match the assumed CLI contracts, prefer disabling that invocation mode and preserving safe fallback. RTK readiness should be measured by truthful capability detection and recoverable raw handles, not by forcing external compression to appear active.
