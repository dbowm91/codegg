# Shell Output Projection Phase 6: RTK Invocation for Low-Risk Commands

## Objective

Turn the Phase 5 RTK skeleton into a real optional backend for low-risk, read-only commands while preserving codegg's core invariants: raw output retention, explicit projection metadata, safe fallback, and no silent command-semantic changes.

This phase should not make RTK the default. It should enable controlled RTK invocation only when the command is eligible, RTK is available, config permits external compression, and codegg can either preserve raw output from the same execution or clearly fall back to native projection.

## Dependency

This phase depends on the corrective cleanup pass and Phase 5 skeleton:

- RTK discovery states are reliable.
- Placeholder RTK output is not user-visible in normal runtime.
- Config semantics are documented and tested.
- Selector fallback behavior is verified.
- Raw output handles and exactness metadata are reliable.

Do not start broad RTK invocation until that baseline is stable.

## Design Decision: Post-Process First, Wrapper Only With Constraints

Prefer post-process integration if RTK supports compressing already-captured output. This is the safest mode:

1. Run the user's command normally through codegg.
2. Retain raw stdout/stderr in `CommandOutputStore`.
3. Pass captured output to RTK for projection.
4. Store the RTK-compressed result as `ProjectionKind::ExternalCompressed` / `ProjectionExactness::Lossy`.
5. Keep raw expansion handles.

If RTK only supports wrapper mode (`rtk <command...>`), then Phase 6 must stay narrow. Wrapper mode should only be used for low-risk read-only commands, and only if capability probing confirms exit-code and stream behavior are acceptable.

Do not run both the raw command and the RTK-wrapped command to compare output in normal runtime. That doubles side effects and invalidates flaky/time-sensitive command semantics. Test fixtures may run both under controlled conditions.

## Eligible Commands

Initial allowlist:

- `git status`
- `git diff`
- `git diff --cached`
- `git diff --staged`
- `git show` when bounded/read-only
- `git log`
- `rg`
- `grep`
- `ls`
- `find`
- `fd`
- `tree`
- bounded `cat` for text files only, if existing policy permits it

Explicitly ineligible by default:

- `cargo test`
- `cargo build`
- `cargo check`
- `cargo clippy`
- `npm`, `pnpm`, `yarn`, `pip`, `uv`, `poetry`, `cargo install`
- migrations and deploy commands
- write/delete/move commands
- `curl`, `wget`, `ssh`, `scp`, `rsync`
- `sudo`, `su`
- security scanners
- unknown shell pipelines

Native Git/Rust projectors should still win when `prefer_native_projectors = true`. RTK is mainly for broad generic low-risk output coverage.

## Implementation Steps

### 1. Confirm RTK invocation contract

Inspect the RTK CLI behavior in a local/dev environment. Determine whether it supports:

- post-processing captured output from stdin or file
- wrapper mode only
- preserving original exit code
- preserving stderr distinctly or merging streams
- stable version output
- bounded runtime behavior

Document the result in `architecture/human_shell.md` and comments in `src/shell/rtk.rs`.

### 2. Add invocation mode type

Add an explicit mode enum:

```rust
pub enum RtkInvocationMode {
    PostProcess,
    Wrapper,
    Disabled,
}
```

Capabilities should record which mode is supported. Config may eventually expose a preference, but Phase 6 should choose the safest available mode automatically.

### 3. Implement post-process mode if supported

If RTK can compress existing output:

- Write retained stdout/stderr to bounded temp files or pipe combined text to RTK, depending on RTK's interface.
- Preserve stream labels in the input if RTK only accepts a single stream.
- Enforce timeout.
- Enforce max bytes passed to RTK.
- Capture RTK stdout/stderr separately.
- If RTK fails, return a recoverable backend error and let the selector fall back.

Projection metadata must include:

- RTK path/version
- invocation mode
- input bytes
- output bytes
- timeout used
- raw handles
- warning if streams were merged for compression

### 4. Implement wrapper mode only if necessary

If wrapper mode is the only option:

- Restrict to `EligibleReadOnly` commands.
- Require `allow_side_effecting_commands = false` to remain false for default path.
- Require capability probe for exit-code preservation before enabling.
- Capture the output produced by `rtk <cmd...>` into the same `CommandOutputStore` so raw handles reflect the actual single execution.
- Clearly mark raw output as RTK-wrapped raw, not original raw command output, unless RTK can preserve uncompressed original output.

If wrapper mode cannot preserve raw uncompressed output, the projection must say so. Prefer falling back to native projection unless the user explicitly opts in to wrapper lossy behavior.

### 5. Add fallback warnings

When RTK is requested but not used, attach structured warnings to the `ProjectionResult`:

- RTK unavailable
- RTK disabled
- command ineligible
- native projector preferred
- RTK invocation timed out
- RTK failed with non-zero status
- RTK mode unsupported

The TUI can surface these compactly. The model-facing projection should not be cluttered, but it should know when a requested external backend was skipped.

### 6. Tests

Add unit tests for:

- Allowlist and denylist behavior.
- RTK unavailable fallback.
- RTK disabled fallback.
- Native-preferred path bypasses RTK.
- RTK timeout returns recoverable error.
- RTK failure falls back to safe projection.
- Projection result is `ExternalCompressed` / `Lossy` only when RTK actually produced text.
- Placeholder skeleton output is impossible in normal runtime.

Add optional integration tests gated on RTK availability:

- `git status` fixture/repo command.
- `rg` over a temporary file tree.
- `find` over a temporary file tree.
- RTK non-zero behavior probe for a safe command.

These tests must skip cleanly when RTK is absent.

## Success Criteria

- RTK can be invoked for eligible low-risk commands when installed and enabled.
- RTK remains disabled by default.
- Native Git/Rust projectors still win by default.
- Ineligible or unknown commands never use RTK unless explicitly overridden by future config.
- RTK failures degrade to safe native/generic projection.
- Projection metadata identifies RTK version/path and invocation mode.
- Raw output handles remain truthful.
- Optional RTK integration tests skip cleanly when RTK is unavailable.

## Non-Goals

- Do not use RTK for `cargo test`, `cargo build`, or package manager commands by default.
- Do not bundle RTK.
- Do not implement broad shell-pipeline parsing.
- Do not add model-generated summaries.
- Do not implement full redaction in this phase.

## Handoff Notes

This phase is intentionally conservative. The goal is a safe proof of real RTK invocation, not maximum compression coverage. If RTK's CLI contract cannot preserve codegg's raw-retention invariant, keep RTK in discovery/fallback mode and document the blocker rather than forcing unsafe wrapper behavior.
