# Runtime Consolidation, Deletion, and Footprint M003 — Closure Status

Status: corrective pass required

Source implementation plan:

- `plans/implementation/runtime-consolidation-deletion-footprint/003-agent-loop-ownership-decomposition.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Repository baseline reviewed: `bd9b3b610af0fa72ce3fe5a8b8f59222659f006d`

Implementation commits or pull requests:

- Pending commit for this evidence-backed partial pass.

## 1. Executive finding

M003 established concrete internal boundaries for tool batches, provider turns,
and ephemeral context-policy state, and all exercised paths compile and pass
focused tests. It does not satisfy strict M003 closure because the existing
permission/dispatch implementation and the large context-policy implementation
remain physically in `src/agent/loop.rs`; the new owners currently delegate to
that implementation. The plan's central deletion/ownership-footprint outcome
therefore requires a corrective pass.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Typed tool-batch boundary | `src/agent/tool_batch.rs`; primary and follow-up call sites | partial | Boundary is typed, implementation body remains in loop |
| Provider adapter boundary | `src/agent/provider_turn.rs`; both stream call sites | pass | Canonical event stream preserved |
| Context policy owner | `src/agent/context_runtime.rs` | partial | Ephemeral state moved; policy methods remain in loop |
| Recovery owner remains structured | Existing `progress_recovery` tests | pass | No second recovery state machine added |
| Behavior preservation | Focused loop/recovery tests | pass | See verification below |
| Material loop footprint reduction | `git diff --stat`; loop remains approximately same size | fail | Corrective extraction required |
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
| medium | Large tool permission/dispatch body and context policy still live in `loop.rs` behind delegating seams | Strict M003 footprint and ownership acceptance criteria are not met | Complete physical extraction in corrective M003 pass before M006 can start |

## 11. Roadmap disposition

Corrective implementation pass required. M004 and M005 remain independently
ready. M006 remains blocked because M003 is not strictly closed; no future
registered plan became unblocked from this partial pass.

## 12. Registry updates

- M003 is removed from dependency-ready work and recorded as corrective-pass
  required.
- M004 and M005 remain ready.
- M006 and M007 remain blocked on their existing predecessor sets.
