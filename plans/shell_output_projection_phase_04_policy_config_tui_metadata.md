# Shell Output Projection Phase 4: Policy Configuration and TUI Metadata

## Objective

Expose command-output projection behavior through configuration and make projection state visible in the TUI. Users should be able to choose conservative native projection, raw output, RTK-backed projection later, or aggressive compression without code changes. The TUI should clearly indicate when command output is raw, truncated, structured, lossy, redacted, or externally compressed.

This phase prepares the user-facing control surface for RTK while keeping the default safe and native.

## Dependency

This phase assumes:

- Phase 1 command events and raw retention exist.
- Phase 2 projection trait and generic projectors exist.
- Phase 3 native Git/Rust projectors exist or are in progress.
- Projection results include metadata such as projector name, exactness, omitted ranges, and raw handles.

## Design Direction

Add a shell-output projection config section and wire it into projection selection. The initial user-facing policies should be simple:

- `off`: raw output with only minimal safety truncation.
- `safe`: native structured projectors, exact raw for small output, conservative truncation/error retention for long output.
- `rtk`: use RTK for eligible commands once Phase 5/6 is implemented; until then it should validate config but report unavailable.
- `aggressive`: use more aggressive compression/truncation for long output while retaining raw artifacts.

The stable default should be `safe`.

## Proposed Configuration

Add a config shape similar to:

```toml
[shell.output]
projection = "safe" # off | safe | rtk | aggressive
retain_raw = true
redact_model_visible_output = true
max_model_output_tokens = 4000
max_tui_output_bytes = 200000
show_projection_metadata = true
prefer_native_projectors = true

[shell.output.rtk]
enabled = false
path = "rtk"
eligible_only = true
timeout_ms = 5000
allow_side_effecting_commands = false
```

If codegg already has a different config organization, adapt the names to match it. The important part is to represent projection policy, raw retention, model-visible redaction, display metadata, RTK path, RTK enablement, and side-effect safety.

## Per-Command Rules

Add optional per-command rules if the existing config system can support them without too much complexity:

```toml
[[shell.output.rules]]
pattern = "cargo test*"
projector = "native-cargo-test"
max_model_output_tokens = 6000

[[shell.output.rules]]
pattern = "git diff*"
projector = "native-git-diff"

[[shell.output.rules]]
pattern = "find *"
projector = "rtk"
```

If glob rules are too large for this phase, add the config schema but defer rule evaluation. Do not block the main policy work on per-command matching.

## Policy Semantics

### off

Use raw projection for small and medium output. For extremely large output, use exact range truncation with explicit omitted ranges to avoid unbounded model context. `off` should mean no lossy summarization or external compression, not unlimited output.

### safe

Use native structured projectors when supported. Use raw projection for small output. Use error-retention projection for failed unknown commands. Use conservative head/tail truncation for long successful unknown commands. This is the recommended default.

### rtk

Phase 4 should only add the config mode and validation. Phase 5/6 will make it functional. If selected before RTK support is available, codegg should warn and fall back to `safe` rather than failing shell execution.

### aggressive

Use smaller budgets and more eager truncation/compression. Still preserve raw artifacts unless `retain_raw = false` is explicitly set. Do not use aggressive policy to hide exit state or stderr.

## TUI Metadata Display

Add compact projection metadata to command transcript entries. It should be visible enough to build trust but not so noisy that it dominates the TUI.

Suggested compact line:

```text
projection: native-cargo-test · 41.9 KiB -> 3.2 KiB · raw: cmd://44/raw
```

For exact small output:

```text
projection: raw · exact · stdout 2.1 KiB · stderr 0 B
```

For truncated output:

```text
projection: truncated · omitted 184.6 KiB · expand cmd://52/stdout
```

For future RTK output:

```text
projection: rtk · external/lossy · raw retained cmd://61/raw
```

The exact display style should match codegg's TUI conventions. The required fields are projector, exactness/lossiness, reduction if known, and raw handle availability.

## Model-Facing Metadata

Ensure model-facing command output also includes compact metadata. The model should never be given a projection that looks like full raw output when it is not.

Suggested header:

```text
[command 44]
command: cargo test
exit: 101
duration: 2.14s
projection: native-cargo-test; exactness: parsed; raw: cmd://44/raw
stdout: 148.2 KiB; stderr: 41.9 KiB
```

This header should be stable enough that downstream compaction can preserve it.

## Runtime Controls

If codegg has slash-command config controls, add or plan these commands:

```text
/config shell.output.projection safe
/config shell.output.projection off
/config shell.output.projection rtk
/config shell.output.max_model_output_tokens 6000
```

Per-command escape hatches should be considered:

```text
!raw cargo test
!compress=off cargo test
!compress=rtk find . -type f
```

If command parser support for these escape hatches is too invasive, document them as follow-up work. The config path is the required part of this phase.

## Validation and Errors

Config validation should catch:

- unknown projection policy
- negative or zero budgets
- impossible RTK timeout values
- `projection = "rtk"` with RTK disabled or unavailable once RTK detection exists
- `retain_raw = false` combined with lossy projection, if the project wants to warn strongly

Warnings should be visible but should not break basic command execution. If projection config is invalid, fall back to `safe` and surface a config warning.

## Tests

Add tests for:

1. Default config resolves to `safe`.
2. Each policy parses from config.
3. Invalid policy is rejected or falls back with a warning.
4. `off` avoids lossy projectors.
5. `safe` prefers native projectors when available.
6. `aggressive` uses a smaller budget than `safe`.
7. `rtk` policy falls back safely before RTK backend is available.
8. TUI metadata rendering includes projector and exactness.
9. Model-facing metadata includes raw handles when retained.
10. `retain_raw = false` is either rejected for lossy policy or emits a strong warning.

## Success Criteria

- Projection behavior is configurable.
- `safe` is the default policy.
- RTK config exists but does not imply RTK is bundled or required.
- TUI command entries show projection metadata.
- Model-facing command output identifies projection kind and raw handle availability.
- Invalid config degrades safely.
- Tests cover config parsing, selection, and metadata display.

## Non-Goals

- Do not implement RTK discovery or invocation in this phase.
- Do not build the final raw-output expansion panel yet.
- Do not implement every per-command escape hatch if parser changes are large.
- Do not make lossy compression the default.

## Risks and Caveats

Avoid creating a misleading `off` policy that can accidentally dump unbounded output into model context. `off` should disable lossy compression, not safety limits.

Avoid burying projection metadata. The user and model both need to know whether the output was exact, truncated, parsed, or lossy. This is especially important before RTK is introduced.
