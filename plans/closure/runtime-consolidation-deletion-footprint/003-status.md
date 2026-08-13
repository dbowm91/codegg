# Runtime Consolidation, Deletion, and Footprint M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/runtime-consolidation-deletion-footprint/003-agent-loop-ownership-decomposition.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Repository baseline reviewed: `bd9b3b610af0fa72ce3fe5a8b8f59222659f006d`

Implementation commits or pull requests:

- Pending commit — corrective physical extraction of context policy and tool-batch ownership.

## 1. Executive finding

M003 is strictly closed. The corrective pass physically moved context packing
and policy methods into `context_runtime.rs` and permission, execution-context,
and batch execution methods into `tool_batch.rs`. `AgentLoop` remains the
orchestration owner, while concrete subsystem modules now own their bodies.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Typed tool-batch boundary | `src/agent/tool_batch.rs`; primary and follow-up call sites | pass | Batch execution body is physically owned by the module |
| Provider adapter boundary | `src/agent/provider_turn.rs`; both stream call sites | pass | Canonical event stream preserved |
| Context policy owner | `src/agent/context_runtime.rs` | pass | Context packing, observation, reduction, starvation, and cache-stat methods are physically owned by the module |
| Recovery owner remains structured | Existing `progress_recovery` tests | pass | No second recovery state machine added |
| Behavior preservation | Focused loop/recovery tests | pass | See verification below |
| Material loop footprint reduction | `src/agent/loop.rs` 6,641 → 4,845 LOC; extracted owners total 1,809 LOC | pass | Large multi-domain bodies are no longer in the turn driver |
| Architecture documentation | `architecture/agent.md` | pass | Ownership description updated; field list remains marked illustrative |

## 3. Production implementation evidence

`ToolBatchExecutor` now returns the existing ordered
`Vec<(String, ToolExecutionOutcome)>` boundary. `ProviderTurnAdapter` owns the
entry point for retrying and receiving normalized provider events. The
ephemeral context-policy state is in `context_runtime.rs`. No protocol,
storage, scheduler, permission authority, or schema semantics changed.

## 4. Verification executed

```bash
cargo fmt --all
cargo fmt --all -- --check
cargo check -p codegg --lib
cargo test -p codegg --lib agent::r#loop::tests -- --nocapture
cargo test -p codegg --lib agent::progress_recovery -- --nocapture
cargo test --test agent_loop_harness -- --test-threads=1
scripts/verify.sh quick
git diff --check
```

Focused AgentLoop tests passed (39) and recovery tests passed (14). Quick
verification passed, and locked workspace Clippy passed.

## 5. Invariant review

Permission receipts, broker authority, workspace CWD authority, structured
outcomes, result ordering, and recovery state ownership remain unchanged.
No new shared mutable global or `Arc<Mutex<...>>` was introduced.

## 6. Failure and recovery review

The adapter seams preserve existing retry, timeout, cancellation, MCP, tool,
question, and recovery behavior. No restart or durable-state behavior changed.

## 7. Migration and compatibility review

No schema, protocol, configuration, or migration change was made.

## 8. Security review

Permission and Tool Broker enforcement remain on the existing path. The pass
does not broaden tool authority or alter path validation.

## 9. Documentation and operations

`architecture/agent.md` now documents concrete ownership boundaries and the
provider/tool/context module seams.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
No critical, high, medium, or low finding remains in M003 scope.

## 11. Roadmap disposition

M003 is closed. M006 is now dependency-ready because M001–M005 are closed;
M007 remains blocked until the post-extraction M006 measurement and disposition
are accepted.

## 12. Registry updates

- M003 is marked closed after the corrective physical extraction.
- M006 is promoted to ready for its required post-extraction measurement pass.
- M007 remains blocked on M006.
