# Shell Output Projection Phase 10: Context Budget and Compaction Integration

## Objective

Integrate command-output projection with codegg's broader context budgeting, model routing, and conversation compaction systems. Projections should not be treated as isolated strings. They should carry metadata that helps the context manager preserve critical command facts while avoiding duplicate or excessive compression.

The end state is budget-aware command output: mini/workhorse/frontier model tiers can receive appropriately sized projections, downstream compaction understands projection metadata, and raw expansion handles remain available when detail is omitted.

## Dependency

This phase assumes:

- Projection result metadata is stable.
- Expansion handles are implemented.
- Redaction is implemented or clearly configured.
- Evaluation harness exists or is in progress.
- Config policies for `off`, `safe`, `rtk`, and `aggressive` are available.

## Design Direction

Projection selection should receive budget intent from the context manager, not only static config. A failed command near the current task may deserve more tokens than an old successful command. A mini model may need a denser projection than a frontier model with a larger context window. A review/security task may need richer diff evidence.

Add a context-aware budget layer that can choose:

- preferred projection token budget
- hard max projection token budget
- whether exact raw output is requested
- whether lossy projection is allowed
- whether native projectors should include richer details
- whether omitted ranges should be more granular

## Budget Sources

Budget should be derived from:

- configured `shell.output.max_model_output_tokens`
- model profile/context window
- current remaining context budget
- command exit state
- command recency
- whether command was auto-promoted with `!!`
- task type, such as coding, review, security, or planning
- user explicit command, such as raw/no-compress/aggressive

## Suggested Policy Matrix

### Small/mini models

- Prefer dense structured projections.
- Keep unknown successful output very small.
- Preserve failed command cause and source spans.
- Include raw handles, not large raw excerpts.

### Workhorse coding models

- Allow moderate detail for failed commands.
- Include native cargo/git summaries with relevant file spans.
- Include small hunk previews for diffs.
- Avoid full raw logs unless explicitly requested.

### Frontier/planning models

- Prefer structured summaries and task-relevant excerpts.
- Avoid dumping raw logs simply because context is available.
- Preserve provenance and expansion handles.

### Review/security modes

- Preserve diff and evidence detail more conservatively.
- Avoid overcompressing scanner/evidence output.
- Ensure redaction and provenance metadata are explicit.

## Compaction Metadata

Downstream conversation compaction should preserve these fields:

- command ID
- command string
- cwd if relevant
- exit state
- duration
- projector name
- exactness/lossiness
- raw handle availability
- failed test names
- diagnostic file/line spans
- warning/error counts
- omitted-range handles
- redaction state
- RTK/backend warning state

Do not let a later summary turn this:

```text
cargo test failed in projection native-cargo-test; raw cmd://44/raw; failing test parser::handles_nested_blocks at crates/parser/src/block.rs:418
```

into this:

```text
Tests failed.
```

The compacted form must retain enough actionable detail and recovery handles.

## Data Model Changes

Add or refine a compact projection metadata struct:

```rust
pub struct ProjectionContextMetadata {
    pub command_id: CommandRunId,
    pub command: String,
    pub exit_label: String,
    pub projector: String,
    pub exactness: ProjectionExactness,
    pub raw_available: bool,
    pub expansion_handles: Vec<ExpansionHandle>,
    pub critical_facts: Vec<ProjectionFact>,
    pub warnings: Vec<String>,
}
```

`ProjectionFact` can initially be a typed enum or a simple key/value shape:

- failed test
- diagnostic span
- changed file
- hunk summary
- error code
- stderr excerpt
- redaction applied

## Integration Steps

### 1. Identify context packing boundary

Find where tool outputs, shell outputs, and promoted command projections are packed into model context. Route shell projection through a budget object produced at that boundary.

### 2. Add model-tier-aware budget construction

Map model profile/tier to default projection budget. Keep static config as a ceiling or baseline. Make this easy to override later.

### 3. Preserve projection metadata in context ledger

If codegg's context ledger already tracks tool output projections, add a shell-output projection variant or metadata field. Store enough information for compaction and recovery.

### 4. Teach compaction to preserve critical facts

Update compaction prompt or deterministic compaction code so command projection metadata is explicitly retained. Prefer deterministic metadata preservation over relying only on model summarization.

### 5. Avoid double compression

Mark projection results as already compacted/truncated/lossy. Later context compaction should not summarize away their metadata or re-truncate in a way that drops failure cause.

### 6. Add tests

Add tests that simulate promoted shell output flowing through context packing and compaction.

## Tests

Add tests for:

1. Projection budget varies by model tier/profile.
2. Failed command receives a larger or more preservation-oriented budget than successful long output.
3. Exact/raw request bypasses lossy projection within safe limits.
4. Compaction preserves command ID and raw handle.
5. Compaction preserves failed test names.
6. Compaction preserves diagnostic file/line spans.
7. Compaction preserves redaction state.
8. Already-projected output is not double-compressed into useless text.
9. Security/review mode uses richer evidence-preserving policy.
10. Aggressive policy still preserves minimum critical facts.

## Success Criteria

- Projection budgets can be supplied by context packing/model profile logic.
- Shell projection metadata is represented in the context ledger or equivalent state.
- Conversation compaction preserves command facts and recovery handles.
- Double compression of projected shell output is avoided.
- Tests cover model-tier budget differences and compaction preservation.

## Non-Goals

- Do not redesign the entire context system.
- Do not implement full semantic memory for command output.
- Do not make large raw logs default just because a model has a large context window.
- Do not remove expansion handles during compaction.

## Handoff Notes

This phase connects shell projection to the wider codegg architecture. The key quality bar is not just smaller context; it is durable, recoverable, task-relevant command evidence across long sessions and compaction cycles.
