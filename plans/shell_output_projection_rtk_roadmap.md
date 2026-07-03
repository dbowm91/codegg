# Shell Output Projection and Optional RTK Backend Roadmap

## Objective

Add a first-class command-output projection system to codegg so shell command results are stored as durable raw artifacts and converted into explicit model-facing views. RTK should be supported as an optional compressor backend, but codegg should own the abstraction, policy selection, raw-output retention, expansion handles, redaction, and native structured projectors.

The product goal is to reduce model-visible token volume from shell commands without making compressed output indistinguishable from raw output. Every lossy, truncated, parsed, or externally compressed projection must be labeled and recoverable by reference to the retained raw command artifact.

## Core Design Principle

Compression must be reversible by reference, even when it is not reversible by content. The model may receive a compact projection, but codegg must preserve raw stdout/stderr and enough metadata for the TUI, model tools, or user commands to request exact output later without rerunning the command.

This avoids a serious correctness problem: command reruns are not equivalent for flaky tests, time-sensitive commands, generated files, build scripts, migration commands, network commands, or mutable repository state.

## Target Architecture

The shell execution path should produce a structured command event, not a plain transcript blob. A command run should capture:

- command ID and stable handles
- original command string
- resolved executable and argv where available
- working directory
- start timestamp and duration
- exit status, signal, or termination reason
- stdout bytes and stderr bytes
- combined output ordering if available
- output byte counts and line counts
- redaction state
- projection state
- compressor/projector name and version
- omitted ranges and expansion handles

The raw output should be stored out-of-band from model context. The model-facing content should be derived through a projection pipeline. Initial projections may be simple raw/truncated views, but the architecture must support structured parsers, native command-specific reducers, RTK-backed compression, and future model-generated summaries.

## Recommended Default Policy

The stable default should be conservative:

```toml
[shell.output]
projection = "safe"
retain_raw = true
redact_model_visible_output = true
max_model_output_tokens = 4000
show_projection_metadata = true

[shell.output.rtk]
enabled = false
path = "rtk"
eligible_only = true
prefer_native_projectors = true
allow_side_effecting_commands = false
```

The `safe` policy should use exact native projections, structured parsing, and conservative truncation with error retention. RTK should be discoverable and opt-in. When enabled, native projectors should still win for commands codegg understands better, especially Rust diagnostics, test failures, and git diffs.

## Roadmap Phases

### Phase 1: Command event model and raw output retention

Create the domain model for shell command events. Store raw stdout/stderr out-of-band. Ensure command execution no longer directly injects unstructured output into model context. Add stable command-output handles such as `cmd://42/stdout`, `cmd://42/stderr`, `cmd://42/combined`, and `cmd://42/projection/model`.

### Phase 2: Projection trait and generic projectors

Introduce a `CommandOutputProjector` abstraction. Add raw, truncated, and error-retention projectors. Every projection should declare whether it is exact, truncated, structured, lossy, redacted, or externally compressed.

### Phase 3: Native structured projectors for Git and Rust

Implement native projectors for high-value commands. Start with `git status`, `git diff`, `git log`, `cargo check`, `cargo build`, and `cargo test`. Prefer structured command modes where codegg controls execution, such as `cargo --message-format=json`.

### Phase 4: Projection policy configuration and TUI metadata

Expose config for `off`, `safe`, `rtk`, and `aggressive` policies. Add per-command rules. Display projection metadata in the TUI so users can see when output is compressed, truncated, parsed, or raw.

### Phase 5: RTK discovery and backend skeleton

Detect RTK on `$PATH` or at a configured path. Probe basic behavior. Add an RTK projector skeleton behind config gates, but do not make RTK the default.

### Phase 6: RTK projector for low-risk commands

Use RTK only for eligible read-only commands unless raw capture can be preserved safely from the RTK execution. Good initial candidates are `git status`, `git diff`, `rg`, `ls`, `find`, `fd`, and `tree`. Avoid default RTK use for tests, builds, package managers, migrations, deploy commands, and network commands.

### Phase 7: Expansion handles and raw-output retrieval UX

Add TUI and model-tool affordances for expanding raw stdout, stderr, combined output, omitted ranges, error regions, and failed-test blocks.

### Phase 8: Redaction and model-visible output safety

Run redaction before any output reaches model context. Ensure native and RTK projections cannot bypass secret filtering. Separate local raw display policy from model-visible output policy.

### Phase 9: Evaluation harness and regression corpus

Create fixtures across Rust, Python, JavaScript/TypeScript, Go, shell, and Git. Compare raw/truncated baseline, native safe projection, RTK projection, and aggressive projection. Measure token reduction and correctness preservation.

### Phase 10: Context-budget and compaction integration

Make projection budget-aware. Prevent downstream conversation compaction from summarizing away critical command metadata such as exit code, failing test, source span, or raw handles.

### Phase 11: Experimental RTK release

Ship RTK support as experimental opt-in. Native safe projection remains default. RTK is discoverable and configurable but not silently used for side-effecting commands.

### Phase 12: Native projector expansion and production stabilization

Expand native projectors for `pytest`, `ruff`, `mypy`, `pyright`, `tsc`, `eslint`, `vitest`, `jest`, `go test -json`, and other common developer commands. Stabilize config, docs, tests, and TUI UX.

## Acceptance Criteria for the Full Line of Work

- Raw stdout/stderr are retained for every shell command unless explicitly disabled.
- Model-facing output is always produced through a projection layer.
- Every projection identifies its projector, exactness, truncation/lossiness, redaction state, and expansion handles.
- Failed commands preserve exit status, failure text, stderr, and relevant surrounding context.
- RTK can be enabled as an optional backend without becoming a hard dependency.
- RTK failures fall back to native projection of raw output.
- High-risk or side-effecting commands do not use RTK by default.
- Native structured projectors are preferred for Rust and Git workflows.
- The TUI clearly shows when output was projected and lets users expand raw output.
- Redaction is applied to model-visible output regardless of projector backend.
- A regression corpus verifies diagnostic preservation, exit-code preservation, failure preservation, and token reduction.

## Non-Goals

- Do not globally rewrite shell commands through RTK outside codegg's execution pipeline.
- Do not make RTK a required runtime dependency.
- Do not treat compressed text as equivalent to raw output.
- Do not rerun commands only to obtain compressed output unless the command is explicitly safe and policy allows it.
- Do not bypass codegg's future LSP, diff/hunk, or context-budgeting systems with opaque terminal compression.

## Implementation Notes

The work should be staged so each phase is valuable independently. Phase 1 and Phase 2 are the architectural foundation. Phase 3 provides immediate native value before RTK. Phase 4 makes the behavior inspectable and configurable. Phase 5 introduces RTK without coupling correctness to it.

The likely long-term shape is that RTK remains useful for broad generic command coverage, while codegg's native projectors handle the most important coding workflows with better semantic grounding and source navigation.
