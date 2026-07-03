# Shell Output Projection Phase 5: RTK Discovery and Backend Skeleton

## Objective

Add RTK as an optional, detected command-output compressor backend without making it a hard dependency or default execution path. This phase should discover RTK, validate basic behavior, expose capability state, and add a backend skeleton behind the existing projection abstraction. Actual broad RTK use should remain limited until Phase 6.

The goal is to make RTK integration technically possible while preserving codegg's correctness guarantees: raw output retention, explicit projection metadata, safe fallback, and no silent command-semantic changes.

## Dependency

This phase assumes:

- Phase 1 command event model and raw output retention exist.
- Phase 2 projection abstraction exists.
- Phase 4 config includes `shell.output.rtk` or equivalent.
- Projection results can identify external/lossy compressor metadata.

Native projectors from Phase 3 are strongly preferred but not strictly required for this skeleton.

## Design Direction

RTK should be implemented as a backend under codegg's projector system, not as a global shell hook and not as a blind command rewriter outside codegg's execution path.

There are two possible integration modes:

1. Post-process mode: run the command normally, retain raw output, pass captured output to RTK for compression if RTK supports this behavior.
2. Wrapper mode: invoke `rtk <command...>` and treat RTK output as the command output.

Post-process mode is safer because raw capture is preserved from the single real command execution. If RTK does not support post-processing existing output, wrapper mode must be restricted to low-risk commands until codegg can prove raw output is still captured and exit semantics are preserved.

This phase should not use RTK broadly. It should establish detection, capability probing, data types, and safe fallback.

## RTK Configuration

Use the config shape from Phase 4 or equivalent:

```toml
[shell.output.rtk]
enabled = false
path = "rtk"
eligible_only = true
timeout_ms = 5000
allow_side_effecting_commands = false
```

Recommended defaults:

- `enabled = false`
- `eligible_only = true`
- `allow_side_effecting_commands = false`
- `timeout_ms = 5000`

`projection = "rtk"` should not imply RTK is installed. If RTK is unavailable, codegg should fall back to `safe` and surface a warning.

## Discovery

Add an RTK discovery component. Suggested names:

- `RtkDiscovery`
- `RtkBackendProbe`
- `ExternalCompressorRegistry`

Discovery should answer:

```rust
pub struct RtkAvailability {
    pub state: RtkState,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
    pub diagnostics: Vec<String>,
}

pub enum RtkState {
    Disabled,
    Available,
    NotFound,
    Broken,
    TimedOut,
    UnsupportedVersion,
}
```

Discovery steps:

1. If RTK config is disabled, return `Disabled`.
2. Resolve configured path or search `$PATH`.
3. Run a bounded version command if available, such as `rtk --version`.
4. Capture version text and timing.
5. Mark unavailable states without panicking.

Do not block codegg startup on slow discovery. Either run discovery lazily on first RTK use or enforce a short timeout.

## Capability Probing

Add a small probe suite that can be run in tests and optionally at runtime.

Questions to answer:

- Does RTK execute successfully for a trivial command?
- Does RTK preserve the wrapped command's non-zero exit code?
- Does RTK preserve stderr or merge streams?
- Does RTK emit valid UTF-8?
- Does RTK respect timeout/cancellation?
- Does RTK fail open or fail closed when wrapping fails?
- Does RTK alter ANSI/color output in a way codegg must account for?

Example probe commands should be safe and portable where possible:

```text
printf 'hello\n'
sh -c 'echo out; echo err >&2; exit 7'
```

On Windows support, adapt shell probes or skip with platform-specific guards if codegg does not yet target Windows shell execution.

The result should be represented explicitly:

```rust
pub struct RtkCapabilities {
    pub preserves_exit_code: CapabilityState,
    pub preserves_stderr: CapabilityState,
    pub supports_post_process: CapabilityState,
    pub supports_wrapper_mode: CapabilityState,
    pub utf8_output: CapabilityState,
}
```

## Backend Skeleton

Add an `RtkProjector` implementing the projection trait, but keep it conservative.

Skeleton behavior:

1. If RTK disabled, return unsupported.
2. If RTK unavailable, return unsupported with diagnostic.
3. If command is not eligible, return unsupported.
4. If RTK invocation fails, return `ProjectionError::BackendUnavailable` or a recoverable error.
5. The selector should fall back to native/generic projection.

Projection result kind should be external/lossy:

```rust
ProjectionKind::ExternalCompressed
ProjectionExactness::Lossy
projector: "rtk"
```

The result must include raw handles when raw output is retained. If raw output cannot be retained for a wrapper-mode invocation, the projector should decline unless config explicitly allows it.

## Eligibility Classification

Add the first version of command risk classification. Suggested names:

- `CommandRiskClassifier`
- `CompressionEligibility`
- `ShellCommandSafety`

Initial categories:

```rust
pub enum CompressionEligibility {
    EligibleReadOnly,
    EligibleWithRawCapture,
    IneligibleSideEffecting,
    IneligibleSecuritySensitive,
    Unknown,
}
```

Initial low-risk RTK candidates:

- `git status`
- `git diff`
- `git show` if read-only and bounded
- `git log`
- `rg`
- `grep`
- `ls`
- `find`
- `fd`
- `tree`
- `cat` for bounded text files only, if already safe

Initial ineligible or high-risk commands:

- `cargo test`
- `cargo build`
- package managers
- migrations
- deploy commands
- commands writing files
- network commands
- security scanners
- commands involving secrets
- unknown shell pipelines unless explicitly allowed

This classification can be conservative. False negatives are acceptable. False positives are dangerous.

## Selection Integration

When policy is `rtk`, update the projection selector:

1. Exact requested output uses raw projection.
2. Native preferred projectors still win when `prefer_native_projectors = true`.
3. RTK projector is attempted only if enabled, available, and eligible.
4. RTK failure falls back to safe native/generic projection.
5. Projection metadata records fallback reason.

Under `safe`, RTK should not be used unless the user explicitly configures RTK as a backend for a command rule.

## TUI and Diagnostics

Expose RTK availability in a diagnostics or config status surface:

```text
RTK: disabled
RTK: available at /usr/local/bin/rtk, version x.y.z
RTK: not found; falling back to safe projection
RTK: broken; version probe timed out
```

When RTK is selected but unavailable, do not fail shell commands. Show a warning and use `safe` projection.

## Tests

Add unit tests for:

1. RTK disabled state.
2. RTK path resolution with configured path.
3. RTK not found state.
4. Version probe timeout handling.
5. Capability result representation.
6. Eligibility classifier marks read-only commands eligible.
7. Eligibility classifier marks known side-effecting commands ineligible.
8. `projection = "rtk"` falls back when RTK unavailable.
9. Native projectors win when `prefer_native_projectors = true`.
10. RTK projection result is labeled external/lossy.

Add integration tests guarded by RTK availability:

- If `rtk` is installed in the test environment, run trivial probe commands.
- Verify non-zero exit code behavior if possible.
- Verify fallback does not fail command execution if RTK errors.

Do not make CI require RTK unless the project explicitly adds an RTK installation step. Optional tests should skip cleanly.

## Success Criteria

- codegg can detect RTK when installed.
- RTK absence does not break shell execution.
- RTK config is parsed and validated.
- An `RtkProjector` skeleton exists behind the projection trait.
- RTK projection is labeled as external/lossy.
- The selector can fall back to safe native/generic projection.
- Eligibility classification prevents default RTK use for side-effecting commands.
- Tests cover disabled, unavailable, fallback, and eligibility behavior.

## Non-Goals

- Do not make RTK the default.
- Do not bundle the RTK binary.
- Do not globally rewrite commands through RTK.
- Do not use RTK for `cargo test` or `cargo build` by default in this phase.
- Do not rerun commands to obtain both raw and compressed output.
- Do not bypass redaction or raw-retention rules.

## Risks and Caveats

The major design hazard is wrapper mode. If RTK can only compress by wrapping the command, codegg must not use it for commands where raw output, side effects, or exact exit semantics matter. This phase should therefore prefer capability discovery and skeleton wiring over broad feature enablement.

The second hazard is making RTK failure user-visible as command failure. RTK is a compressor backend; if it fails, command execution should still complete and codegg should fall back to safe projection.

The third hazard is overclaiming exactness. RTK output should be treated as lossy/external unless proven otherwise for a specific command and mode.
