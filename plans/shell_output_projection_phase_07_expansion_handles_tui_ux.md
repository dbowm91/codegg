# Shell Output Projection Phase 7: Expansion Handles and TUI UX

## Objective

Build the user-facing and model-tool-facing affordances for expanding retained raw command output from projection handles. By the end of this phase, a compact projection should be enough for normal model context, but users and tools should be able to recover exact stdout/stderr ranges, omitted regions, failure regions, and full raw streams without rerunning commands.

This phase completes a major correctness promise from the roadmap: lossy, parsed, truncated, or externally compressed projections are acceptable only because raw artifacts are recoverable by reference.

## Dependency

This phase assumes:

- Command runs produce stable `cmd://<id>/<stream>` handles.
- Projection results include `ExpansionHandle` values and omitted ranges.
- TUI messages can display projection metadata.
- RTK paths, if present, preserve truthful raw-handle semantics.

## Design Direction

Add expansion as a first-class operation over `CommandOutputStore`, not as a shell rerun. The expansion surface should work from both the TUI and future model tools.

Supported expansion targets:

- full stdout
- full stderr
- combined stream if available
- byte range
- omitted range
- lines around matched error/failure text
- all failure regions produced by `ErrorRetentionProjector`
- all failing-test blocks produced by `CargoTestProjector`
- selected hunk regions produced by `GitDiffProjector`

The first implementation can expose stdout/stderr/range expansion and add semantic failure/hunk expansions later in the same phase if time allows.

## Command/Handle Syntax

Add or extend shell commands such as:

```text
/shell-expand cmd://42/stdout
/shell-expand cmd://42/stderr#0-4096
/shell-expand 42 stdout
/shell-expand 42 stderr --range 0..4096
/shell-expand 42 --omitted 1
/shell-expand 42 --failures
```

If codegg already has `/shell-include`, `/shell-ask`, or shell detail commands, reuse that surface rather than adding redundant commands. The key is that expansion can target the new projection handles, not only the legacy `ShellOutputStore` entry.

## TUI Detail Panel

Enhance the shell detail view or add a projection detail panel. It should show:

- command ID
- command string
- cwd
- exit state
- duration
- raw-retention status
- stdout/stderr sizes
- output partiality
- projection kind/exactness
- projector name
- omitted ranges
- expansion handles
- RTK/backend warnings if present

For raw output display, use paging and search. Do not load huge retained streams into a single unbounded widget. Respect existing TUI scroll/keybinding conventions.

## Expansion Result Model

Add an explicit expansion result type if one does not already exist:

```rust
pub struct CommandOutputExpansion {
    pub command_id: CommandRunId,
    pub stream: CommandOutputStream,
    pub byte_range: Option<Range<usize>>,
    pub text: String,
    pub exactness: ExpansionExactness,
    pub total_stream_bytes: usize,
    pub returned_bytes: usize,
    pub warnings: Vec<String>,
}
```

`ExpansionExactness` should represent:

- exact full stream
- exact range
- partial raw artifact
- invalid UTF-8 rendered lossily
- unavailable/evicted

Expansion must not pretend evicted or partial bytes are recoverable.

## Model Tool Affordance

If codegg has a tool registry for model-accessible utilities, add a read-only expansion tool later in this phase or prepare the internal API for one. Suggested tool name:

```text
command_output_read
```

Inputs:

- handle string or command ID + stream
- optional byte range
- optional max bytes

Safety constraints:

- Read-only.
- Bounded output.
- Redaction should apply for model-visible targets when Phase 8 lands.
- Reject malformed handles.
- Reject cross-session handles if session identity exists.

Do not expose local raw output to the model if the user executed a normal ephemeral `!command` and did not promote or ask about it. Preserve the central invariant: `!command` is not model context unless promoted.

## Promotion Semantics

Clarify interaction with existing `!`/`!!` behavior:

- `!command`: output remains local/ephemeral; user can inspect in TUI; model cannot read it unless user promotes or requests `/shell-ask`.
- `!!command`: projection can enter model context; raw handles can be referenced in projection; expansion may be model-accessible depending on policy.
- `/shell-include`: includes a projection or selected expansion into model context.
- `/shell-expand`: local TUI operation unless explicitly promoted.

Add tests to prevent accidental model access to ephemeral command raw output.

## Tests

Add tests for:

1. Handle parsing for full stream and range forms.
2. Range expansion returns exact bytes.
3. Invalid ranges are rejected or clamped according to documented semantics.
4. Evicted command output returns unavailable, not empty exact output.
5. Partial raw artifacts report partial exactness.
6. Invalid UTF-8 is represented safely.
7. Projection omitted ranges map to valid expansion handles.
8. TUI metadata includes expansion hints when output is projected.
9. `!command` expansion remains local unless promoted.
10. `!!command` projection can expose model-visible handles according to policy.

## Success Criteria

- Users can expand raw stdout/stderr from `cmd://` handles without rerunning commands.
- Omitted ranges from projections are expandable when retained.
- TUI command detail exposes projection/raw-retention metadata clearly.
- Expansion is bounded and handles partial/evicted output honestly.
- Ephemeral human-shell invariants are preserved.
- Tests cover handle parsing, range expansion, partiality, eviction, and promotion boundaries.

## Non-Goals

- Do not implement full secret redaction here; Phase 8 owns that.
- Do not make all raw output model-visible by default.
- Do not add disk-backed long-term command artifact persistence unless needed.
- Do not rerun commands to recover missing bytes.

## Handoff Notes

This phase should be treated as a correctness feature, not only a UX feature. Projection-based compression is safe only if raw detail can be recovered when needed. Keep the expansion API small, typed, and shared between TUI and future model tools.
